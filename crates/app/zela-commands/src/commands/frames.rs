//! `frames`：PNG 序列帧导出（`render --frames` 的一等入口）。
//!
//! 与 `render` **同一个帧体**（[`super::render::run`]）；差别只在默认产物是序列帧目录。
//! 这让 `--frames` 从"ffmpeg 不可用时的降级路径"变成正式入口，也与断点续渲合流。

use std::path::PathBuf;

use zela_app::frame_loop::LoopBackend;

use crate::commands::render::{self, RenderOptions};
use crate::host::{CommandHost, FrameSinkFactory};
use crate::response::Response;
use crate::spec::{ArgSpec, ArgType, CommandArgs, CommandSpec};

const BACKENDS: &[&str] = &["auto", "auto-frame", "wgpu", "vector"];

pub const SPEC: CommandSpec = CommandSpec {
    name: "frames",
    about: "把脚本 / ZSB 渲染成 PNG 序列帧",
    args: &[
        ArgSpec::positional("script", ArgType::Path, "脚本路径（.lua / .py / .js）"),
        ArgSpec::option("bundle", ArgType::Path, "", "读取 ZSB 产物（与脚本二选一）"),
        ArgSpec::option("out", ArgType::Path, "export/frames", "序列帧输出目录"),
        ArgSpec::option("fps", ArgType::Number, "30", "帧率"),
        ArgSpec::option("size", ArgType::Size, "640x480", "画幅 WxH"),
        ArgSpec::option("backend", ArgType::Choice(BACKENDS), "auto", "出帧后端"),
        ArgSpec::flag("hash", "打印逐帧序列哈希"),
        ArgSpec::option("hash-out", ArgType::Path, "", "把序列哈希写到文件"),
        ArgSpec::optional("from", ArgType::Number, "起始帧（含；默认 0）"),
        ArgSpec::optional("to", ArgType::Number, "结束帧（不含；默认到末尾）"),
    ],
};

/// 参数 → 渲染请求（强制走序列帧）。
pub fn options_from_args(args: &CommandArgs) -> Result<RenderOptions, String> {
    let script = args.path("script").filter(|path| !path.as_os_str().is_empty());
    let bundle = args.path("bundle").filter(|path| !path.as_os_str().is_empty());
    if script.is_some() && bundle.is_some() {
        return Err("脚本路径与 --bundle 只能给一个".to_string());
    }
    if script.is_none() && bundle.is_none() {
        return Err("缺少脚本路径（或 --bundle <file.zsb>）".to_string());
    }
    let hash_out = args
        .path("hash-out")
        .filter(|path| !path.as_os_str().is_empty());
    Ok(RenderOptions {
        script,
        bundle,
        input_stream: None,
        frames_dir: Some(
            args.path("out")
                .unwrap_or_else(|| PathBuf::from("export/frames")),
        ),
        hash: args.flag("hash") || hash_out.is_some(),
        hash_out,
        fps: args.number("fps").unwrap_or(30.0),
        out: PathBuf::from("export/frames"),
        size: args.size("size").unwrap_or((640, 480)),
        backend: args
            .text("backend")
            .and_then(LoopBackend::parse)
            .unwrap_or(LoopBackend::Auto),
        from: args.number("from").map(|value| value.max(0.0) as u32),
        to: args.number("to").map(|value| value.max(0.0) as u32),
    })
}

/// 执行导出（复用 `render` 的唯一帧体）。
pub fn run(
    host: &dyn CommandHost,
    factory: &mut dyn FrameSinkFactory,
    options: &RenderOptions,
) -> Result<Response, String> {
    render::run(host, factory, options)
}
