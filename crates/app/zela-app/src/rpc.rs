//! **CH-c**：debug RPC —— 把 [`crate::headless`] 的命令/响应形状套上 socket 传输。
//!
//! # 原则
//!
//! - **不另写一套协议**：离线文件与 RPC 是同一份数据的两种通道；命令共 6 个：
//!   `get_frame` / `get_diagnostics` / `inject` / `probe` / `trace` / `screenshot`。
//! - **只投递，不驱动**：`inject` 带显式帧号进队列；**绝不因为 RPC 请求多跑一帧**。
//!   `trace` / `probe` 是显式窗口（`max_frames`），它按契约推进并在回执里写明是否截断。
//! - **只读 + 注入**：不开「改场景对象 / 设属性 / 热重载」——那会绕过装配期与
//!   CC 的字段级写仲裁，凭空多一个写入者。
//! - **默认关 + 只绑 127.0.0.1 + 不进产物**：`serve` 只接受调用方传入的 listener；
//!   地址由调用方显式给（默认建议 `127.0.0.1:0`）。
//!
//! # `probe` 与 `trace` 是同一个机制
//!
//! `probe = 注入一个带 tag 的事件 + 读它的 TraceReport`。任意 debug 点击（空白处、
//! 共享边、被遮挡控件、任意修饰键组合）都走这条链返回 —— 归因链与事件标记是
//! 同一个机制的两面。

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use serde::{Deserialize, Serialize};

use ze_scene_core::domain::Injected;

use crate::debug::{DEFAULT_TRACE_FRAMES, TraceError};
use crate::headless::HeadlessScene;

/// 一条 debug 命令（NDJSON：一行一个请求）。
///
/// `command` **扁平**在请求对象里：`{"id":1,"command":"get_frame"}` /
/// `{"id":2,"command":"trace","injected":{...},"max_frames":8}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub command: Command,
}

/// 命令集（最小集：只读 + 注入）。
///
/// **内部标记**（`tag = "command"`）+ `Request` 的 `flatten` ⇒ 协议是扁平的：
/// `{"id":1,"command":"get_frame"}` / `{"id":2,"command":"probe","injected":{...}}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    /// 当前帧号 + 结构化帧（`None` = 本场景没有自研面板）。
    GetFrame,
    /// 派生态诊断（hover / focus / capture 的投递目标）。
    GetDiagnostics,
    /// 投递一个标记事件（**不推进**；帧号必须等于当前帧）。
    Inject { injected: Injected },
    /// 注入 + 读回执（与 `trace` 同一机制；`probe` 是它的语义别名）。
    Probe {
        injected: Injected,
        #[serde(default)]
        max_frames: Option<u32>,
    },
    /// 注入 + 逐帧差分回执。
    Trace {
        injected: Injected,
        #[serde(default)]
        max_frames: Option<u32>,
    },
    /// **CH-d**：debug-only 截图（仅 Vector 后端；PNG 不进逐位基线）。
    Screenshot { path: String },
}

/// 一条响应。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: u64, error: impl Into<String>) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(error.into()),
        }
    }
}

/// RPC 目标（GUI 与 headless 共用；`execute` 只做纯查询 / 注入）。
pub trait DebugTarget {
    fn execute(&self, request: &Request) -> Response;

    /// **CH-d**：debug-only 截图（写入 `path`，返回画幅）。
    ///
    /// 默认**不支持**（headless 零 GPU、Wgpu 后端另开出口）；GUI + Vector
    /// 在 zela-gui 的实现里覆盖它。截图不进任何逐位基线。
    fn screenshot(&self, _path: &str) -> Result<(u32, u32), String> {
        Err("当前目标不支持截图（仅 GUI + Vector 后端；Wgpu 后端另开出口）".to_string())
    }
}

/// 把一行 JSON 变成一行 JSON（解析失败也给结构化错误，不静默断开）。
pub fn handle(target: &dyn DebugTarget, line: &str) -> String {
    let request: Request = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return serde_json::to_string(&Response::error(
                0,
                format!("请求不是合法 JSON：{error}"),
            ))
            .unwrap_or_else(|_| "{\"id\":0,\"ok\":false}".to_string());
        }
    };
    let response = target.execute(&request);
    match serde_json::to_string(&response) {
        Ok(line) => line,
        // 兜底也必须走序列化器：手工拼 `format!` 不转义引号 / 换行 ⇒ 回执会变成非法 JSON。
        Err(error) => serde_json::to_string(&serde_json::json!({
            "id": request.id,
            "ok": false,
            "error": format!("编码失败：{error}"),
        }))
        .unwrap_or_else(|_| "{\"id\":0,\"ok\":false}".to_string()),
    }
}

/// **服务一个连接**（阻塞；连接关闭即返回）。测试与单线程宿主用它。
pub fn serve_once(listener: &TcpListener, target: &dyn DebugTarget) -> std::io::Result<()> {
    let (stream, _) = listener.accept()?;
    serve_stream(&stream, target)
}

/// 在给定连接上逐行服务（NDJSON）。
pub fn serve_stream(stream: &TcpStream, target: &dyn DebugTarget) -> std::io::Result<()> {
    let mut writer = stream.try_clone()?;
    let reader = BufReader::new(stream.try_clone()?);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = handle(target, &line);
        writeln!(writer, "{reply}")?;
        writer.flush()?;
    }
    Ok(())
}

// ─── headless 目标（与运行时同线程；`DriverPod` 内部是 `RefCell` ⇒ 非 Send）───

impl DebugTarget for HeadlessScene {
    fn execute(&self, request: &Request) -> Response {
        match &request.command {
            Command::GetFrame => Response::ok(
                request.id,
                serde_json::json!({
                    "frame": self.frame_number(),
                    "frame_json": self.frame_json(),
                }),
            ),
            Command::GetDiagnostics => Response::ok(
                request.id,
                serde_json::to_value(self.diagnostics_snapshot())
                    .unwrap_or(serde_json::Value::Null),
            ),
            Command::Inject { injected } => match self.inject_injected(injected) {
                Ok(count) => Response::ok(request.id, serde_json::json!({ "injected": count })),
                Err(error) => Response::error(request.id, error),
            },
            Command::Probe {
                injected,
                max_frames,
            }
            | Command::Trace {
                injected,
                max_frames,
            } => match self.trace(injected, max_frames.unwrap_or(DEFAULT_TRACE_FRAMES)) {
                Ok(report) => Response::ok(
                    request.id,
                    serde_json::to_value(report).unwrap_or(serde_json::Value::Null),
                ),
                Err(error) => Response::error(request.id, error.to_string()),
            },
            Command::Screenshot { path } => match self.screenshot(path) {
                Ok((width, height)) => Response::ok(
                    request.id,
                    serde_json::json!({ "path": path, "width": width, "height": height }),
                ),
                Err(error) => Response::error(request.id, error),
            },
        }
    }
}

// ─── GUI 桥：socket 线程 ⇄ GUI 主线程（命令带 Reply 通道，主线程每帧排空）───

/// 一条投递到 GUI 主线程的请求（`reply` 由主线程回填）。
pub struct Job {
    pub request: Request,
    pub reply: Sender<Response>,
}

/// GUI 主线程这一端：每帧 `poll` 一次（非阻塞）。
pub struct GuiBridge {
    jobs: Receiver<Job>,
}

impl GuiBridge {
    /// 处理所有待办请求（在 `ui()` 顶部调用；不阻塞：没有请求就立即返回）。
    pub fn poll(&self, target: &dyn DebugTarget) {
        while let Ok(job) = self.jobs.try_recv() {
            let _ = job.reply.send(target.execute(&job.request));
        }
    }
}

/// socket 线程这一端：把请求转给 GUI 主线程并等回执。
#[derive(Clone)]
pub struct BridgeTarget {
    jobs: Sender<Job>,
}

impl DebugTarget for BridgeTarget {
    fn execute(&self, request: &Request) -> Response {
        let (reply, wait) = std::sync::mpsc::channel();
        if self
            .jobs
            .send(Job {
                request: request.clone(),
                reply,
            })
            .is_err()
        {
            return Response::error(request.id, "GUI 主线程已退出");
        }
        match wait.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(response) => response,
            Err(_) => Response::error(request.id, "GUI 主线程未在 5s 内回执（窗口卡住？）"),
        }
    }
}

/// 启动桥：返回（GUI 端，socket 端）。地址由调用方显式绑定（默认建议 127.0.0.1）。
pub fn bridge(listener: TcpListener) -> std::io::Result<(GuiBridge, BridgeTarget)> {
    let (tx, rx) = std::sync::mpsc::channel();
    let target = BridgeTarget { jobs: tx };
    let worker = target.clone();
    std::thread::Builder::new()
        .name("zela-debug-rpc".to_string())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let target = worker.clone();
                let _ = std::thread::Builder::new()
                    .name("zela-debug-conn".to_string())
                    .spawn(move || {
                        let _ = serve_stream(&stream, &target);
                    });
            }
        })?;
    Ok((GuiBridge { jobs: rx }, target))
}

/// **在同一条线程**上服务直到 listener 关闭（headless 宿主用）。
///
/// 为什么不是后台线程：`DriverPod` 内部是 `RefCell` / `Cell`（确定性优先），
/// **不是 `Send`** —— 运行时只能待在拥有它的那条线程。GUI 用 [`bridge`] 把请求
/// 转给主线程；headless 宿主直接调用本函数。
pub fn serve_forever(listener: TcpListener, target: &dyn DebugTarget) -> std::io::Result<()> {
    for stream in listener.incoming().flatten() {
        let _ = serve_stream(&stream, target);
    }
    Ok(())
}

/// 从环境读监听端口（`ZELA_DEBUG_RPC`：缺省 / `0` = **关**；正整数 = 绑定
/// `127.0.0.1:<port>`）。默认关 + 只绑本地，是教条不是可选项。
pub fn listen_from_env() -> std::io::Result<Option<TcpListener>> {
    let Some(raw) = std::env::var_os("ZELA_DEBUG_RPC") else {
        return Ok(None);
    };
    let port: u16 = match raw.to_string_lossy().trim().parse() {
        Ok(port) if port != 0 => port,
        _ => return Ok(None),
    };
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!("[zela] debug RPC 监听 {}", listener.local_addr()?);
    Ok(Some(listener))
}

/// 把 `TraceError` 变成 RPC 错误文本（保持类型在 app 层）。
pub fn trace_error_text(error: &TraceError) -> String {
    error.to_string()
}

/// **驱动器目标**：把 debug 原语接到任意 `DriverPod`（GUI 的 `Runtime` 直接用）。
///
/// 与 `impl DebugTarget for HeadlessScene` 同一份实现（都走 `crate::debug`），
/// 所以「离线 / RPC / 真人窗口」三条入口的回执同源。
#[derive(Clone)]
pub struct DriverTarget {
    driver: Arc<zela_scene_ecs::DriverPod>,
}

impl DriverTarget {
    pub fn new(driver: Arc<zela_scene_ecs::DriverPod>) -> Self {
        Self { driver }
    }
}

impl DebugTarget for DriverTarget {
    fn execute(&self, request: &Request) -> Response {
        match &request.command {
            Command::GetFrame => Response::ok(
                request.id,
                serde_json::json!({
                    "frame": self.driver.frame(),
                    "frame_json": crate::debug::frame_json_of(&self.driver),
                }),
            ),
            Command::GetDiagnostics => Response::ok(
                request.id,
                serde_json::to_value(crate::debug::diagnostics_of(&self.driver))
                    .unwrap_or(serde_json::Value::Null),
            ),
            Command::Inject { injected } => {
                match crate::debug::inject_injected(&self.driver, injected) {
                    Ok(count) => Response::ok(request.id, serde_json::json!({ "injected": count })),
                    Err(error) => Response::error(request.id, error),
                }
            }
            Command::Probe {
                injected,
                max_frames,
            }
            | Command::Trace {
                injected,
                max_frames,
            } => match crate::debug::trace_on(
                &self.driver,
                injected,
                max_frames.unwrap_or(DEFAULT_TRACE_FRAMES),
            ) {
                Ok(report) => Response::ok(
                    request.id,
                    serde_json::to_value(report).unwrap_or(serde_json::Value::Null),
                ),
                Err(error) => Response::error(request.id, error.to_string()),
            },
            Command::Screenshot { path } => match self.screenshot(path) {
                Ok((width, height)) => Response::ok(
                    request.id,
                    serde_json::json!({ "path": path, "width": width, "height": height }),
                ),
                Err(error) => Response::error(request.id, error),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl DebugTarget for Echo {
        fn execute(&self, request: &Request) -> Response {
            Response::ok(request.id, serde_json::json!({ "echo": request.id }))
        }
    }

    #[test]
    fn invalid_json_still_gets_a_structured_error() {
        let reply = handle(&Echo, "{not json");
        let response: Response = serde_json::from_str(&reply).expect("响应是 JSON");
        assert!(!response.ok);
        assert_eq!(response.id, 0);
        assert!(response.error.expect("有错误说明").contains("合法 JSON"));
    }

    #[test]
    fn request_round_trips_and_maps_to_the_target() {
        let request = Request {
            id: 7,
            command: Command::GetFrame,
        };
        let line = serde_json::to_string(&request).expect("可序列化");
        let response: Response = serde_json::from_str(&handle(&Echo, &line)).expect("响应是 JSON");
        assert!(response.ok);
        assert_eq!(response.result, Some(serde_json::json!({ "echo": 7 })));
    }

    #[test]
    fn probe_and_trace_share_one_command_shape() {
        let request: Request =
            serde_json::from_str(r#"{"id":1,"command":"get_frame"}"#).expect("可反序列化");
        assert_eq!(request.command, Command::GetFrame);
        let probe: Request = serde_json::from_str(
            r#"{"id":2,"command":"probe","injected":{"tag":1,"frame":0,"source":"scene","payload":{"kind":"external","name":"x"}}}"#,
        )
        .expect("probe 可反序列化");
        assert!(matches!(probe.command, Command::Probe { .. }));
    }

    /// **CJ-05（B4）**：错误消息含引号 / 换行时，回执仍是合法 JSON。
    #[test]
    fn error_messages_with_quotes_and_newlines_stay_valid_json() {
        struct Failing;
        impl DebugTarget for Failing {
            fn execute(&self, request: &Request) -> Response {
                Response::error(request.id, "坏的 \"引号\"\n第二行")
            }
        }
        let reply = handle(&Failing, r#"{"id":9,"command":"get_frame"}"#);
        let response: Response = serde_json::from_str(&reply).expect("回执必须是合法 JSON");
        assert!(!response.ok);
        assert_eq!(response.id, 9);
        assert!(response.error.expect("有错误说明").contains("第二行"));
    }
}
