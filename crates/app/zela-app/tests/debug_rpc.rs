//! **CH-c 判据**：同一串**带标记**的事件，离线直接调用与 RPC 通道 ⇒ `TraceReport`
//! 逐位相同（窗口里真人走的是同一条 `collect_events`，见 CH-a）。
//!
//! 同时验证两条 RPC 纪律：`inject` 只投递不推进；服务只绑本地。
//!
//! 线程模型：`DriverPod` 内部是 `RefCell`（确定性优先）⇒ **运行时不能跨线程**。
//! 所以测试把**客户端**放到线程里，服务在主线程上跑 `serve_once`。

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};

use zela_app::debug::TraceReport;
use zela_app::headless;
use zela_app::rpc::{self, Command, Request, Response};
use zela_app::SceneChoice;
use ze_scene_core::domain::{Injected, PointerState, WindowInput};

/// 构造"点滑块 80%"的窗口输入注入（帧号 0）。
fn injected_click(scene: &headless::HeadlessScene) -> Injected {
    let frame = scene.frame().expect("有 Frame");
    let name = frame
        .semantics
        .iter()
        .find(|node| node.role == "slider" && node.name.contains(":curve:"))
        .map(|node| node.name.clone())
        .expect("有参数滑块");
    let (wx, wy) = scene.slider_point(&name, 0.8).expect("世界坐标");
    let camera = scene
        .input_context()
        .expect("有输入上下文")
        .layer2d
        .expect("有 2D 图层")
        .1;
    let screen = camera.world_to_screen(ze_notion::Vec2::new(wx, wy));
    let input = WindowInput {
        pointer: PointerState {
            position: Some(screen),
            hovering: true,
            primary_pressed: true,
            ..PointerState::default()
        },
        viewport: camera.rect,
        window_size: ze_notion::Vec2::new(camera.rect.max.x, camera.rect.max.y),
        ..WindowInput::default()
    };
    Injected::window(1, 0, input).with_label("rpc click")
}

/// 客户端线程：连上后依次发请求、读完响应再关连接（让 `serve_once` 返回）。
fn client(
    addr: std::net::SocketAddr,
    requests: Vec<Request>,
) -> std::thread::JoinHandle<Vec<Response>> {
    std::thread::spawn(move || {
        let mut stream = TcpStream::connect(addr).expect("连接");
        let mut responses = Vec::new();
        for request in requests {
            let line = serde_json::to_string(&request).expect("请求可序列化");
            writeln!(stream, "{line}").expect("写请求");
            stream.flush().expect("flush");
            let mut reply = String::new();
            BufReader::new(stream.try_clone().expect("clone"))
                .read_line(&mut reply)
                .expect("读响应");
            responses.push(serde_json::from_str(&reply).expect("响应是 JSON"));
        }
        responses
    })
}

fn frame_of(response: &Response) -> u32 {
    response.result.as_ref().expect("有结果")["frame"]
        .as_u64()
        .expect("帧号") as u32
}

#[test]
fn rpc_and_offline_produce_the_same_trace_report() {
    // 离线基准：全新场景直接跑 trace。
    let offline_scene = headless::build(SceneChoice::Hud).expect("headless 装配");
    let injected = injected_click(&offline_scene);
    let offline = offline_scene.trace(&injected, 8).expect("离线回执");

    // RPC：另一个全新场景 + 真正的 TCP 通道，投递**同一串**事件。
    let rpc_scene = headless::build(SceneChoice::Hud).expect("headless 装配");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("本地端口");
    let addr = listener.local_addr().expect("地址");
    assert!(addr.ip().is_loopback(), "RPC 必须只绑本地");
    let responses = client(
        addr,
        vec![
            Request {
                id: 1,
                command: Command::GetFrame,
            },
            Request {
                id: 2,
                command: Command::Inject {
                    injected: injected.clone(),
                },
            },
            Request {
                id: 3,
                command: Command::GetFrame,
            },
            Request {
                id: 4,
                command: Command::Trace {
                    injected,
                    max_frames: Some(8),
                },
            },
        ],
    );
    rpc::serve_once(&listener, &rpc_scene).expect("服务一个连接");
    let responses = responses.join().expect("客户端线程");

    assert!(responses.iter().all(|response| response.ok), "{responses:?}");
    assert_eq!(
        frame_of(&responses[0]),
        frame_of(&responses[2]),
        "inject 不得推进帧"
    );
    let via_rpc: TraceReport =
        serde_json::from_value(responses[3].result.clone().expect("有结果")).expect("回执可反序列化");
    assert_eq!(
        serde_json::to_value(&via_rpc).expect("可序列化"),
        serde_json::to_value(&offline).expect("可序列化"),
        "离线与 RPC 必须逐位相同"
    );
}

/// **CH-d 口径**：截图只在 GUI + Vector 生效；headless 缺 GPU ⇒ 显式报错。
#[test]
fn screenshot_is_explicitly_unsupported_on_headless() {
    let scene = headless::build(SceneChoice::Hud).expect("headless 装配");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("本地端口");
    let addr = listener.local_addr().expect("地址");
    let responses = client(
        addr,
        vec![Request {
            id: 21,
            command: Command::Screenshot {
                path: "debug.png".to_string(),
            },
        }],
    );
    rpc::serve_once(&listener, &scene).expect("服务一个连接");
    let responses = responses.join().expect("客户端线程");
    assert!(!responses[0].ok, "headless 不能截图：{:?}", responses[0]);
    assert!(
        responses[0].error.as_deref().unwrap_or("").contains("不支持截图"),
        "错误必须说清边界：{:?}",
        responses[0].error
    );
}

#[test]
fn probe_over_rpc_matches_the_offline_trace() {
    let offline_scene = headless::build(SceneChoice::Hud).expect("headless 装配");
    let injected = injected_click(&offline_scene);
    let offline = offline_scene.trace(&injected, 8).expect("离线回执");

    let rpc_scene = headless::build(SceneChoice::Hud).expect("headless 装配");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("本地端口");
    let addr = listener.local_addr().expect("地址");
    let responses = client(
        addr,
        vec![Request {
            id: 11,
            command: Command::Probe {
                injected,
                max_frames: Some(8),
            },
        }],
    );
    rpc::serve_once(&listener, &rpc_scene).expect("服务一个连接");
    let responses = responses.join().expect("客户端线程");
    assert!(responses[0].ok, "{:?}", responses[0]);

    let probe: TraceReport =
        serde_json::from_value(responses[0].result.clone().expect("有结果")).expect("回执可反序列化");
    assert_eq!(probe, offline, "probe 与 trace 是同一个机制");
}
