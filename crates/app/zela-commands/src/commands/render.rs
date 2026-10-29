//! `render`：脚本 / ZSB → 帧（唯一帧体）。
//!
//! 逻辑搬自 `zela-render` bin，差别只在三处适配层：
//! - 进度 / 诊断走 [`CommandHost`]（CLI 打印、GUI 进度条、测试收集）；
//! - 帧去往 [`FrameSinkFactory`] 建立的 sink（PNG / mp4 / 纹理）；
//! - 返回 [`Response`] 而不是打印字符串。
//!
//! 帧循环本身仍是 `zela_app::frame_loop::run_frames` —— **不另写一份**。

use std::path::PathBuf;
use std::time::Duration;

use zela_app::frame_loop::{FrameLoopOptions, LoopBackend, frames_for, run_frames};
use zela_app::{ScriptLanguage, build_bundle_runtime, build_script_runtime, language_from_path};
use zela_render_backends::{
    BackendKind, BackendRegistry, DEFAULT_TARGET_FORMAT, HeadlessTarget, RenderContext,
    StandardBackends,
};
use zela_scene_bundle::Bundle;

use crate::host::{CommandHost, FrameSinkFactory, Level};
use crate::response::{RenderSummary, Response};
use crate::spec::{ArgSpec, ArgType, CommandArgs, CommandSpec};

/// 取消标记（命令层内部；不暴露给宿主）。
const CANCELLED: &str = "命令已取消";

const BACKENDS: &[&str] = &["auto", "auto-frame", "wgpu", "vector"];

/// 声明式描述：CLI 由它生成 clap，GUI 由它生成表单。
pub const SPEC: CommandSpec = CommandSpec {
    name: "render",
    about: "把脚本 / ZSB 渲染成 mp4 或 PNG 序列帧",
    args: &[
        ArgSpec::positional("script", ArgType::Path, "脚本路径（.lua / .py / .js）")
            .with_alias("script"),
        ArgSpec::option("bundle", ArgType::Path, "", "读取 ZSB 产物（与脚本二选一）"),
        ArgSpec::option(
            "input-stream",
            ArgType::Path,
            "",
            "输入流（独立产物；只与 --bundle 搭配）",
        ),
        ArgSpec::option(
            "frames",
            ArgType::Path,
            "",
            "导出 PNG 序列帧到目录（不给则走 mp4）",
        ),
        ArgSpec::option("fps", ArgType::Number, "30", "帧率"),
        ArgSpec::option("out", ArgType::Path, "out.mp4", "mp4 输出路径"),
        ArgSpec::option("size", ArgType::Size, "640x480", "画幅 WxH"),
        ArgSpec::option("backend", ArgType::Choice(BACKENDS), "auto", "出帧后端"),
        ArgSpec::flag("hash", "打印逐帧序列哈希"),
        ArgSpec::option("hash-out", ArgType::Path, "", "把序列哈希写到文件"),
        ArgSpec::optional("from", ArgType::Number, "起始帧（含；默认 0）"),
        ArgSpec::optional("to", ArgType::Number, "结束帧（不含；默认到末尾）"),
    ],
};

/// 一次渲染请求（参数已按 spec 定型）。
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    pub script: Option<PathBuf>,
    pub bundle: Option<PathBuf>,
    pub input_stream: Option<PathBuf>,
    pub frames_dir: Option<PathBuf>,
    pub hash: bool,
    pub hash_out: Option<PathBuf>,
    pub fps: f64,
    pub out: PathBuf,
    pub size: (u32, u32),
    pub backend: LoopBackend,
    /// 帧范围（`--from` / `--to`；`None` = 全片）。
    pub from: Option<u32>,
    pub to: Option<u32>,
}

impl RenderOptions {
    /// 参数集合 → 渲染请求（交叉校验与原 `zela-render` 完全一致）。
    pub fn from_args(args: &CommandArgs) -> Result<Self, String> {
        let script = args.path("script").filter(|path| !path.as_os_str().is_empty());
        let bundle = args.path("bundle").filter(|path| !path.as_os_str().is_empty());
        let input_stream = args
            .path("input-stream")
            .filter(|path| !path.as_os_str().is_empty());
        let frames_dir = args
            .path("frames")
            .filter(|path| !path.as_os_str().is_empty());
        let hash_out = args
            .path("hash-out")
            .filter(|path| !path.as_os_str().is_empty());

        if script.is_some() && bundle.is_some() {
            return Err("脚本路径与 --bundle 只能给一个".to_string());
        }
        if script.is_none() && bundle.is_none() {
            return Err("缺少脚本路径（或 --bundle <file.zsb>）".to_string());
        }
        if input_stream.is_some() && bundle.is_none() {
            return Err(
                "输入流只能与 --bundle 搭配（配对校验需要 ZSB 的 payload hash）".to_string(),
            );
        }

        let from = args.number("from").map(|value| value.max(0.0) as u32);
        let to = args.number("to").map(|value| value.max(0.0) as u32);
        if let (Some(from), Some(to)) = (from, to) {
            if from >= to {
                return Err(format!("--from {from} 必须小于 --to {to}"));
            }
        }

        Ok(Self {
            script,
            bundle,
            input_stream,
            frames_dir,
            // `--hash-out` 隐含开启哈希（它要拿这个值写文件）。
            hash: args.flag("hash") || hash_out.is_some(),
            hash_out,
            fps: args.number("fps").unwrap_or(30.0),
            out: args
                .path("out")
                .unwrap_or_else(|| PathBuf::from("out.mp4")),
            size: args.size("size").unwrap_or((640, 480)),
            backend: args
                .text("backend")
                .and_then(LoopBackend::parse)
                .unwrap_or(LoopBackend::Auto),
            from,
            to,
        })
    }
}

/// 执行渲染。
///
/// 取消语义（阶段 4 的前置）：宿主请求取消 ⇒ 立即停止推进，**已渲帧保留**，
/// `summary.cancelled = true`（不返回 `Err`，因为产物仍然有效）。
pub fn run(
    host: &dyn CommandHost,
    factory: &mut dyn FrameSinkFactory,
    options: &RenderOptions,
) -> Result<Response, String> {
    let (width, height) = options.size;

    // 装配期可能用到 Typst（content.text）⇒ 先保证确定字体，避免静默回退。
    let _ = crate::commands::fonts::ensure_font_dir();

    // ── 后端注册表 → device features 并集 →（矢量可用时）注入同一块 device ──
    let mut registry = BackendRegistry::with_standard();
    if options.backend != LoopBackend::Wgpu {
        registry
            .register(BackendKind::Vector)
            .map_err(|e| e.to_string())?;
    }
    let ctx = if options.backend == LoopBackend::Wgpu {
        RenderContext::headless().map_err(|e| format!("无法创建 headless GPU 上下文：{e}"))?
    } else {
        RenderContext::headless_with_registry(DEFAULT_TARGET_FORMAT, &registry)
            .map_err(|e| format!("无法创建 headless GPU 上下文：{e}"))?
    };
    if options.backend != LoopBackend::Wgpu {
        let adapter = ctx
            .adapter()
            .ok_or_else(|| "上下文没有 adapter：无法判定后端可用性".to_string())?;
        for entry in registry.availability(adapter) {
            match entry.reason {
                Some(reason) => host.diagnostic(
                    Level::Warn,
                    &format!("[backend] {:?} 不可用：{reason}", entry.kind),
                ),
                None => host.diagnostic(Level::Info, &format!("[backend] {:?} 可用", entry.kind)),
            }
        }
        if registry
            .select_on(BackendKind::Vector, adapter)
            .is_ok()
        {
            zela_render_vector::init_gpu(ctx.device.clone(), ctx.queue.clone(), adapter)?;
        }
    }

    // ── 运行时（脚本 / ZSB 同一条装配尾）──
    let mut input_stream: Option<zela_scene_bundle::InputStream> = None;
    let (mut runtime, payload_hash, source_tag) = if let Some(script_path) = &options.script {
        let source = std::fs::read_to_string(script_path)
            .map_err(|e| format!("读取脚本 {} 失败：{e}", script_path.display()))?;
        let language = language_from_path(script_path).map_err(|e| e.to_string())?;
        if language == ScriptLanguage::Python && !cfg!(feature = "python") {
            return Err(
                "脚本是 .py，但本二进制编译时未启用 `python` feature（请重新构建或改用 .lua）"
                    .to_string(),
            );
        }
        // 参数指纹取**内容**哈希（换路径不失效；改内容必须拒绝续渲）。
        let tag = zela_scene_bundle::sha256::hex(&zela_scene_bundle::sha256::sha256(
            source.as_bytes(),
        ));
        let runtime = build_script_runtime(ctx.clone(), source, language)
            .map_err(|e| e.to_string())?;
        (runtime, None, tag)
    } else if let Some(bundle_path) = &options.bundle {
        let bytes = std::fs::read(bundle_path)
            .map_err(|e| format!("读取 ZSB {} 失败：{e}", bundle_path.display()))?;
        let bundle = Bundle::from_bytes(&bytes)
            .map_err(|e| format!("ZSB 解析失败（{}）：{e}", bundle_path.display()))?;
        host.diagnostic(
            Level::Info,
            &format!(
                "[zsb] payload hash = {}（{} 个对象，静止时刻 {}）",
                bundle.payload_hash_hex(),
                bundle.meta().object_count,
                bundle
                    .script()
                    .ok()
                    .and_then(|script| script.still_time())
                    .map_or_else(
                        || "不可推导".to_string(),
                        |value| format!("≈ {value:.3}s")
                    )
            ),
        );
        let mut tag = bundle.payload_hash_hex();
        if let Some(path) = &options.input_stream {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("读取输入流 {} 失败：{e}", path.display()))?;
            let stream = zela_scene_bundle::InputStream::from_json(&text)
                .map_err(|e| format!("输入流解析失败（{}）：{e}", path.display()))?;
            stream
                .pairing
                .validate(&bundle)
                .map_err(|e| format!("输入流与 ZSB 不匹配（{}）：{e}", path.display()))?;
            tag = format!(
                "{}|{}",
                tag,
                zela_scene_bundle::sha256::hex(&zela_scene_bundle::sha256::sha256(text.as_bytes()))
            );
            input_stream = Some(stream);
        }
        let runtime = build_bundle_runtime(ctx.clone(), &bundle).map_err(|e| e.to_string())?;
        (runtime, Some(bundle.payload_hash_hex()), tag)
    } else {
        unreachable!("RenderOptions::from_args 已保证二者恰有一个")
    };

    let frames = frames_for(&runtime, options.fps);
    host.diagnostic(
        Level::Info,
        &format!(
            "渲染 {frames} 帧（{}×{} @ {} fps，时长 {:.3}s）…",
            width, height, options.fps, runtime.duration
        ),
    );

    // ── 断点续渲（仅 PNG 序列；mp4 流已封口，不能续）──
    // 帧范围（`--from/--to`）与续渲互斥：范围产物不构成完整序列，不写 manifest。
    let ranged = options.from.is_some() || options.to.is_some();
    let fingerprint = render_fingerprint(&source_tag, options);
    let manifest_path = options
        .frames_dir
        .as_ref()
        .filter(|_| !ranged)
        .map(|dir| dir.join(MANIFEST_NAME));
    let mut manifest: Option<RenderManifest> = None;
    let mut resume_from = 0u32;
    let mut expected_boundary: Option<String> = None;
    if let (Some(dir), Some(path)) = (&options.frames_dir, &manifest_path) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("创建目录 {} 失败：{e}", dir.display()))?;
        if path.is_file() {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
            match serde_json::from_str::<RenderManifest>(&text) {
                Ok(loaded) if loaded.version == MANIFEST_VERSION => {
                    if loaded.fingerprint != fingerprint {
                        return Err(format!(
                            "渲染参数已变化，拒绝续渲（参数指纹 {} ≠ manifest {}）\n  修法：换 `--frames` 目录，或删掉 {}",
                            fingerprint, loaded.fingerprint, path.display()
                        ));
                    }
                    let done = loaded.hashes.len() as u32;
                    if done > frames {
                        return Err(format!(
                            "manifest 记录 {} 帧 > 本次 {} 帧：产物与参数不匹配",
                            done, frames
                        ));
                    }
                    if done == frames {
                        host.diagnostic(Level::Info, "序列帧已完整（manifest 覆盖全部帧），跳过渲染");
                        return Ok(Response::Render(RenderSummary {
                            frames,
                            size: options.size,
                            fps: options.fps,
                            backend: format!("{:?}", options.backend),
                            payload_hash,
                            out: None,
                            frames_dir: options.frames_dir.clone(),
                            cancelled: false,
                            skipped: true,
                        }));
                    }
                    if done > 0 {
                        let last = dir.join(format!("frame_{:05}.png", done - 1));
                        if !last.is_file() {
                            return Err(format!(
                                "manifest 记录已渲 {} 帧，但边界帧 {} 不存在（产物被删过？）\n  修法：删掉 {} 后从头渲",
                                done,
                                last.display(),
                                path.display()
                            ));
                        }
                        expected_boundary = loaded.hashes.last().cloned();
                        resume_from = done;
                        host.diagnostic(
                            Level::Info,
                            &format!("断点续渲：从第 {resume_from} 帧继续（回渲 1 帧校验 Seek 等价）"),
                        );
                    }
                    manifest = Some(loaded);
                }
                Ok(_) => host.diagnostic(Level::Warn, "manifest 版本不认识，忽略并从头渲"),
                Err(error) => host.diagnostic(
                    Level::Warn,
                    &format!("manifest 损坏（{error}），忽略并从头渲"),
                ),
            }
        }
        manifest.get_or_insert_with(|| RenderManifest::new(fingerprint.clone(), frames));
    }
    // 回渲边界帧（`resume_from - 1`）以校验 `Seek` 与顺序播放逐位一致；
    // 帧范围则直接 `Seek` 到 `--from`。
    let start_frame = if ranged {
        options.from.unwrap_or(0)
    } else {
        resume_from.saturating_sub(1)
    };

    // D 批：输入流按帧索引重放（起点帧先装填，回调里装填下一帧）。
    let input_driver = std::sync::Arc::clone(&runtime.driver);
    if let Some(stream) = &input_stream {
        zela_playback::apply_input_frame(&input_driver, stream.events_at(start_frame));
    }

    let mut sink = factory.open(&ctx, options)?;
    let want_rgba = sink.wants_rgba();

    let mut backends = StandardBackends::new(&ctx);
    let mut target = HeadlessTarget::new();
    let mut cancelled = false;
    let mut resume_mismatch: Option<String> = None;

    let run = run_frames(
        &ctx,
        &mut runtime,
        &mut backends,
        &mut target,
        FrameLoopOptions {
            width,
            height,
            fps: options.fps,
            backend: options.backend,
            start_frame,
            end_frame: options.to,
            vector_params: None,
        },
        want_rgba,
        &mut |index, frame| {
            if host.cancelled() {
                cancelled = true;
                return Err(CANCELLED.to_string());
            }
            if let Some(stream) = &input_stream {
                zela_playback::apply_input_frame(&input_driver, stream.events_at(index + 1));
            }
            // 序列帧产物才记哈希（mp4 不续，不付这份代价）。
            let frame_hash = options.frames_dir.as_ref().map(|_| {
                zela_scene_bundle::sha256::hex(&zela_scene_bundle::sha256::sha256(frame.rgba))
            });
            if let (Some(expected), Some(actual)) = (
                expected_boundary.as_deref(),
                frame_hash.as_deref(),
            ) {
                if index + 1 == resume_from && expected != actual {
                    resume_mismatch = Some(format!(
                        "断点续渲不一致：第 {index} 帧哈希 {actual} ≠ 首次渲染 {expected}\n  ⇒ `Seek` 不等价于顺序播放，这是真 bug（不得绕过）"
                    ));
                    return Err("断点续渲校验失败".to_string());
                }
            }
            sink.on_frame(index, &frame)?;
            if let (Some(manifest), Some(path), Some(frame_hash)) =
                (manifest.as_mut(), manifest_path.as_ref(), frame_hash)
            {
                let slot = index as usize;
                if slot < manifest.hashes.len() {
                    manifest.hashes[slot] = frame_hash;
                } else {
                    manifest.hashes.push(frame_hash);
                }
                manifest.total = manifest.total.max(index + 1);
                let text = serde_json::to_string(manifest)
                    .map_err(|e| format!("manifest 序列化失败：{e}"))?;
                std::fs::write(path, text)
                    .map_err(|e| format!("写 {} 失败：{e}", path.display()))?;
            }
            host.progress(index + 1, frames);
            Ok(Duration::ZERO)
        },
    );

    // 无论成功 / 取消 / 出错，都让 sink 收尾（已渲帧必须落盘）。
    let finish = sink.finish();
    if cancelled {
        finish?;
        host.diagnostic(
            Level::Warn,
            &format!("已取消：保留已渲帧（{frames} 帧中的前若干帧）"),
        );
        return Ok(Response::Render(RenderSummary {
            frames,
            size: options.size,
            fps: options.fps,
            backend: format!("{:?}", options.backend),
            payload_hash,
            out: (!has_frames_dir(options)).then(|| options.out.clone()),
            frames_dir: options.frames_dir.clone(),
            cancelled: true,
            skipped: false,
        }));
    }
    let run = match run {
        Ok(run) => {
            finish?;
            run
        }
        Err(error) => {
            // sink 收尾失败优先报出（它意味着产物不完整），否则报渲染错误。
            finish?;
            if let Some(reason) = resume_mismatch {
                return Err(reason);
            }
            return Err(error);
        }
    };

    // 计时 / 决策 / GPU 状态：诊断交给宿主（不是 stdout 的唯一消费者）。
    for decision in &run.decisions {
        host.diagnostic(
            Level::Debug,
            &format!("[decision] {}", decision.json("render", "render")),
        );
    }

    Ok(Response::Render(RenderSummary {
        frames,
        size: options.size,
        fps: options.fps,
        backend: format!("{:?}", options.backend),
        payload_hash,
        out: (!has_frames_dir(options)).then(|| options.out.clone()),
        frames_dir: options.frames_dir.clone(),
        cancelled: false,
        skipped: false,
    }))
}

fn has_frames_dir(options: &RenderOptions) -> bool {
    options.frames_dir.is_some()
}

/// 续渲 manifest 文件名（与 PNG 同目录）。
const MANIFEST_NAME: &str = ".zela-render.json";
const MANIFEST_VERSION: u32 = 1;

/// 续渲 manifest：已渲帧区间 + 每帧哈希 + **参数指纹**。
///
/// 参数指纹不匹配 ⇒ **拒绝续渲**（否则半段旧参数半段新参数，产物是错的）。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct RenderManifest {
    version: u32,
    fingerprint: String,
    /// 本次作业的总帧数（诊断用；进度以 `hashes.len()` 为准）。
    total: u32,
    /// 已落盘帧的 SHA-256（下标 = 帧号）。
    hashes: Vec<String>,
}

impl RenderManifest {
    fn new(fingerprint: String, total: u32) -> Self {
        Self {
            version: MANIFEST_VERSION,
            fingerprint,
            total,
            hashes: Vec::new(),
        }
    }
}

/// 参数指纹：源内容 + 画幅 + 帧率 + 后端。
fn render_fingerprint(source_tag: &str, options: &RenderOptions) -> String {
    let raw = format!(
        "v1|{source_tag}|{}x{}|{}|{:?}",
        options.size.0, options.size.1, options.fps, options.backend
    );
    zela_scene_bundle::sha256::hex(&zela_scene_bundle::sha256::sha256(raw.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::CommandArgs;

    fn parse(values: &[&str]) -> Result<CommandArgs, String> {
        let raw: Vec<String> = values.iter().map(|value| value.to_string()).collect();
        SPEC.parse(&raw)
    }

    /// **阶段 0 回归网**：`render` 的默认值必须与旧 `zela-render` 的快照逐条相同。
    #[test]
    fn defaults_match_the_legacy_parser_snapshot() {
        let options = RenderOptions::from_args(&parse(&["demo.lua"]).expect("可解析"))
            .expect("可构造");
        assert_eq!(options.script, Some(PathBuf::from("demo.lua")));
        assert_eq!(options.bundle, None);
        assert_eq!(options.frames_dir, None);
        assert_eq!(options.hash_out, None);
        assert!(!options.hash);
        assert_eq!(options.fps, 30.0);
        assert_eq!(options.out, PathBuf::from("out.mp4"));
        assert_eq!(options.size, (640, 480));
        assert_eq!(options.backend, LoopBackend::Auto);
    }

    #[test]
    fn cross_argument_rules_match_the_legacy_parser() {
        assert!(RenderOptions::from_args(&parse(&["--bundle", "a.zsb"]).expect("可解析")).is_ok());
        // 能解析但两个来源都没给 ⇒ 命令层报错（不是 spec 的用法分支）。
        assert!(
            RenderOptions::from_args(&parse(&["--fps", "30"]).expect("可解析")).is_err(),
            "缺脚本 / --bundle 必须失败"
        );
        assert!(
            RenderOptions::from_args(&parse(&["a.lua", "--bundle", "a.zsb"]).expect("可解析"))
                .is_err(),
            "脚本与 --bundle 只能给一个"
        );
        assert!(
            RenderOptions::from_args(&parse(&["a.lua", "--input-stream", "in.json"]).expect("可解析"))
                .is_err(),
            "输入流只能与 --bundle 搭配"
        );
    }

    #[test]
    fn hash_out_implies_hash() {
        let options = RenderOptions::from_args(
            &parse(&["a.lua", "--hash-out", "h.txt"]).expect("可解析"),
        )
        .expect("可构造");
        assert!(options.hash, "--hash-out 隐含开启哈希");
        assert_eq!(options.hash_out, Some(PathBuf::from("h.txt")));
    }
}
