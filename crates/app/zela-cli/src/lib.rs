//! `zela` 伞命令的宿主实现（clap 适配 + CLI 帧 sink）。
//!
//! 命令**帧体**在 `zela-commands`；本 crate 只做两件事：
//! 1. `CommandSpec → clap::Command`（参数声明不重复写第二份）；
//! 2. 把宿主能力落到终端（进度 / 诊断 / PNG / mp4 / 哈希）。

use std::cell::Cell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Process, Stdio};

use clap::ArgAction;
use zela_app::export::{ExportError, target_pix_fmt, write_png};
use zela_app::frame_loop::FrameView;
use zela_commands::commands::render::{RenderOptions, SPEC as RENDER_SPEC};
use zela_commands::commands::{
    api as api_command, check as check_command, compile as compile_command,
    doctor as doctor_command, fingerprint as fingerprint_command, fonts as fonts_command,
    frames as frames_command, inspect as inspect_command, list as list_command,
    render as render_command,
};
use zela_commands::{
    ArgSpec, ArgType, CommandArgs, CommandHost, CommandSpec, Fact, FrameSink, FrameSinkFactory,
    Level, Response,
};
use zela_render_backends::RenderContext;
use zela_scene_bundle::frame_hash::FrameSequenceHash;

/// `CommandSpec → clap::Command`（唯一映射，杜绝两处声明）。
pub fn spec_to_clap(spec: &CommandSpec) -> clap::Command {
    let mut command = clap::Command::new(spec.name).about(spec.about);
    let mut position = 0usize;
    for arg in spec.args {
        let mut clap_arg = clap::Arg::new(arg.name).help(arg.help);
        if arg.positional {
            position += 1;
            clap_arg = clap_arg.index(position).value_name(arg.ty.placeholder());
            if let Some(alias) = arg.alias {
                // clap 不允许同一 Arg 同时是位置参数与长参数 ⇒ 另加一个隐藏长参数，
                // 收集参数时合并（两者同时给 ⇒ 显式报错）。
                command = command.arg(
                    clap::Arg::new(alias_option_id(alias))
                        .long(alias)
                        .num_args(1)
                        .hide(true),
                );
            }
        } else {
            clap_arg = clap_arg.long(arg.name);
            if arg.ty == ArgType::Flag {
                clap_arg = clap_arg.action(ArgAction::SetTrue);
            } else {
                clap_arg = clap_arg.num_args(1).value_name(arg.ty.placeholder());
            }
            if arg.repeatable {
                clap_arg = clap_arg.action(ArgAction::Append);
            }
        }
        command = command.arg(clap_arg);
    }
    command
}

/// 位置参数的「同名长参数」用独立 id（clap 的 `Id` 不接受运行时 `String`）。
fn alias_option_id(alias: &'static str) -> &'static str {
    match alias {
        "script" => "script__alias",
        _ => alias,
    }
}

/// `clap::ArgMatches → CommandArgs`（类型与默认值仍由 spec 统一决定）。
pub fn collect_args(
    spec: &CommandSpec,
    matches: &clap::ArgMatches,
) -> Result<CommandArgs, String> {
    let mut pairs: Vec<(&'static str, Option<String>)> = Vec::new();
    for arg in spec.args {
        if arg.ty == ArgType::Flag {
            let present = matches.get_flag(arg.name);
            pairs.push((arg.name, Some(if present { "true" } else { "false" }.to_string())));
            continue;
        }
        if arg.positional {
            let positional = matches.get_one::<String>(arg.name).cloned();
            let via_alias = arg
                .alias
                .and_then(|alias| matches.get_one::<String>(alias_option_id(alias)).cloned());
            let value = match (positional, via_alias) {
                (Some(_), Some(_)) => {
                    return Err(format!(
                        "位置参数与 --{} 只能给一个",
                        arg.alias.unwrap_or(arg.name)
                    ));
                }
                (Some(value), None) | (None, Some(value)) => Some(value),
                (None, None) => None,
            };
            pairs.push((arg.name, value));
            continue;
        }
        if arg.repeatable {
            let values: Vec<String> = matches
                .get_many::<String>(arg.name)
                .map(|values| values.cloned().collect())
                .unwrap_or_default();
            if !values.is_empty() {
                pairs.push((arg.name, Some(values.join("\n"))));
            }
            continue;
        }
        if let Some(value) = matches.get_one::<String>(arg.name) {
            pairs.push((arg.name, Some(value.clone())));
        }
    }
    spec.from_pairs(&pairs)
}

/// 终端宿主：进度按 1% 节流；诊断带级别前缀。
#[derive(Default)]
pub struct CliHost {
    last_percent: Cell<u32>,
}

impl CliHost {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CommandHost for CliHost {
    fn progress(&self, done: u32, total: u32) {
        if total == 0 {
            return;
        }
        let percent = done.saturating_mul(100) / total;
        if percent != self.last_percent.get() {
            self.last_percent.set(percent);
            eprint!("\r[{done}/{total} {percent}%]");
            if done == total {
                eprintln!();
            }
        }
    }

    fn diagnostic(&self, level: Level, message: &str) {
        match level {
            Level::Error => eprintln!("[error] {message}"),
            Level::Warn => eprintln!("[warn] {message}"),
            Level::Debug => eprintln!("[debug] {message}"),
            Level::Info => eprintln!("{message}"),
        }
    }
}

/// CLI 的 sink 工厂：PNG / mp4 + 可选哈希。
pub struct CliSinkFactory;

impl FrameSinkFactory for CliSinkFactory {
    fn open(
        &mut self,
        ctx: &RenderContext,
        options: &RenderOptions,
    ) -> Result<Box<dyn FrameSink>, String> {
        let mut sinks: Vec<Box<dyn FrameSink>> = Vec::new();
        match &options.frames_dir {
            Some(dir) => sinks.push(Box::new(PngSink { dir: dir.clone() })),
            None => sinks.push(Box::new(FfmpegSink(Ffmpeg::spawn(
                ctx,
                &options.out,
                options.fps,
                options.size,
            )?))),
        }
        if options.hash || options.hash_out.is_some() {
            sinks.push(Box::new(HashSink {
                hash: FrameSequenceHash::new(options.size.0, options.size.1),
                size: options.size,
                hash_out: options.hash_out.clone(),
            }));
        }
        Ok(Box::new(CompositeSink { sinks }))
    }
}

/// 一组 sink（顺序即写入顺序；`wants_rgba` 取并集）。
struct CompositeSink {
    sinks: Vec<Box<dyn FrameSink>>,
}

impl FrameSink for CompositeSink {
    fn wants_rgba(&self) -> bool {
        self.sinks.iter().any(|sink| sink.wants_rgba())
    }

    fn on_frame(&mut self, index: u32, frame: &FrameView<'_>) -> Result<(), String> {
        for sink in &mut self.sinks {
            sink.on_frame(index, frame)?;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        for sink in &mut self.sinks {
            sink.finish()?;
        }
        Ok(())
    }
}

/// PNG 序列（`--frames DIR`）。
struct PngSink {
    dir: PathBuf,
}

impl FrameSink for PngSink {
    fn wants_rgba(&self) -> bool {
        true
    }

    fn on_frame(&mut self, index: u32, frame: &FrameView<'_>) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("创建目录 {} 失败：{e}", self.dir.display()))?;
        let path = self.dir.join(format!("frame_{index:05}.png"));
        write_png(&path, frame.size.0, frame.size.1, frame.rgba).map_err(|e| e.to_string())
    }

    fn finish(&mut self) -> Result<(), String> {
        eprintln!("完成：序列帧在 {}", self.dir.display());
        Ok(())
    }
}

/// mp4（`--out FILE`，经 ffmpeg 管道）。
struct FfmpegSink(Ffmpeg);

impl FrameSink for FfmpegSink {
    fn on_frame(&mut self, _index: u32, frame: &FrameView<'_>) -> Result<(), String> {
        self.0.write(frame.raw)
    }

    fn finish(&mut self) -> Result<(), String> {
        self.0.finish()
    }
}

/// 帧序列哈希（`--hash` / `--hash-out`）。
struct HashSink {
    hash: FrameSequenceHash,
    size: (u32, u32),
    hash_out: Option<PathBuf>,
}

impl FrameSink for HashSink {
    fn wants_rgba(&self) -> bool {
        true
    }

    fn on_frame(&mut self, _index: u32, frame: &FrameView<'_>) -> Result<(), String> {
        self.hash.push_frame(frame.rgba);
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        let sequence = self.hash.hex();
        eprintln!(
            "[frame-hash] sequence = {sequence}（{} 帧，{}×{}，RGBA8）",
            self.hash.frames(),
            self.size.0,
            self.size.1
        );
        if let Some(path) = &self.hash_out {
            if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("创建目录 {} 失败：{e}", parent.display()))?;
            }
            std::fs::write(path, format!("{sequence}\n"))
                .map_err(|e| format!("写 {} 失败：{e}", path.display()))?;
        }
        Ok(())
    }
}

struct Ffmpeg {
    child: Child,
    out: PathBuf,
}

impl Ffmpeg {
    fn spawn(
        ctx: &RenderContext,
        out: &Path,
        fps: f64,
        (width, height): (u32, u32),
    ) -> Result<Self, String> {
        // ffmpeg 不在 PATH ⇒ **明确报错**（不静默降级到序列帧）。
        if Process::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            return Err(
                "未找到 ffmpeg（不在 PATH）。请安装 ffmpeg，或改用 `--frames DIR` 导出序列帧。"
                    .to_string(),
            );
        }
        let pix_fmt = target_pix_fmt(ctx).map_err(|e: ExportError| e.to_string())?;
        if let Some(parent) = out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录 {} 失败：{e}", parent.display()))?;
        }
        let child = Process::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "rawvideo",
                "-pix_fmt",
                pix_fmt,
                "-s",
                &format!("{width}x{height}"),
                "-r",
                &format!("{fps}"),
                "-i",
                "-",
                "-an",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(out)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("启动 ffmpeg 失败：{e}"))?;
        eprintln!("ffmpeg 管道已启动（{pix_fmt}），输出 {}", out.display());
        Ok(Self {
            child,
            out: out.to_path_buf(),
        })
    }

    fn write(&mut self, pixels: &[u8]) -> Result<(), String> {
        let stdin = self.child.stdin.as_mut().ok_or("ffmpeg stdin 不可用")?;
        stdin
            .write_all(pixels)
            .map_err(|e| format!("写入 ffmpeg 失败：{e}"))
    }

    fn finish(&mut self) -> Result<(), String> {
        drop(self.child.stdin.take());
        let status = self
            .child
            .wait()
            .map_err(|e| format!("等待 ffmpeg 失败：{e}"))?;
        if !status.success() {
            return Err(format!(
                "ffmpeg 退出码 {status}（输出 {}）",
                self.out.display()
            ));
        }
        eprintln!("完成：{}", self.out.display());
        Ok(())
    }
}

/// `gui` 的声明（参数只决定"先播哪件"，与 `zela-gui::initial_from` 同口径）。
const GUI_SPEC: CommandSpec = CommandSpec {
    name: "gui",
    about: "打开 GUI 播放器",
    args: &[
        ArgSpec::option("work", ArgType::Text, "", "启动时选中某件内置作品（id）"),
        ArgSpec::flag("script", "启动 storyboard（Lua）"),
        ArgSpec::flag("python", "启动 Python 分镜"),
        ArgSpec::flag("demo", "启动能力 demo"),
        ArgSpec::flag("capability-demo", "同 --demo"),
    ],
};

/// `play` 的声明：直接打开 GUI 播放某件内置作品。
const PLAY_SPEC: CommandSpec = CommandSpec {
    name: "play",
    about: "在 GUI 里播放某件内置作品（id 见 `zela list`）",
    args: &[ArgSpec::positional("id", ArgType::Text, "内置作品 id")],
};

/// 顶层命令树（阶段 1：render / list；其余在后续阶段接入）。
pub fn root() -> clap::Command {
    clap::Command::new("zela")
        .about("zela：脚本 → 动画（渲染 / 编译 / 检查 / 回放）")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(spec_to_clap(&RENDER_SPEC))
        .subcommand(spec_to_clap(&frames_command::SPEC))
        .subcommand(spec_to_clap(&compile_command::SPEC))
        .subcommand(spec_to_clap(&check_command::SPEC))
        .subcommand(spec_to_clap(&inspect_command::SPEC))
        .subcommand(spec_to_clap(&api_command::SPEC))
        .subcommand(spec_to_clap(&fingerprint_command::SPEC))
        .subcommand(spec_to_clap(&doctor_command::SPEC))
        .subcommand(spec_to_clap(&fonts_command::SPEC))
        .subcommand(spec_to_clap(&list_command::SPEC))
        .subcommand(spec_to_clap(&PLAY_SPEC))
        .subcommand(spec_to_clap(&GUI_SPEC))
}

/// 执行一条命令（`raw` 含程序名，与 `std::env::args` 同形）。
pub fn dispatch(raw: Vec<String>) -> Result<Response, String> {
    let matches = root()
        .try_get_matches_from(raw)
        .map_err(|error| error.to_string())?;
    let host = CliHost::new();
    match matches.subcommand() {
        Some(("list", sub)) => {
            let _ = collect_args(&list_command::SPEC, sub)?;
            list_command::run(&host)
        }
        Some(("render", sub)) => {
            let args = collect_args(&RENDER_SPEC, sub)?;
            let options = RenderOptions::from_args(&args)?;
            render_command::run(&host, &mut CliSinkFactory, &options)
        }
        Some(("frames", sub)) => {
            let args = collect_args(&frames_command::SPEC, sub)?;
            let options = frames_command::options_from_args(&args)?;
            frames_command::run(&host, &mut CliSinkFactory, &options)
        }
        Some(("compile", sub)) => {
            let args = collect_args(&compile_command::SPEC, sub)?;
            let options = compile_command::CompileOptions::from_args(&args)?;
            compile_command::run(&host, &options)
        }
        Some(("check", sub)) => {
            let args = collect_args(&check_command::SPEC, sub)?;
            let script = check_command::script_from_args(&args)?;
            check_command::run(&host, &script)
        }
        Some(("inspect", sub)) => {
            let args = collect_args(&inspect_command::SPEC, sub)?;
            let file = inspect_command::file_from_args(&args)?;
            inspect_command::run(&host, &file)
        }
        Some(("doctor", sub)) => {
            let _ = collect_args(&doctor_command::SPEC, sub)?;
            doctor_command::run(&host)
        }
        Some(("fonts", sub)) => {
            let _ = collect_args(&fonts_command::SPEC, sub)?;
            fonts_command::run(&host)
        }
        Some(("api", sub)) => {
            let args = collect_args(&api_command::SPEC, sub)?;
            api_command::run(&host, &api_command::output_dir(&args))
        }
        Some(("fingerprint", sub)) => {
            let args = collect_args(&fingerprint_command::SPEC, sub)?;
            let invocation = fingerprint_command::invocation_from_args(&args)?;
            fingerprint_command::run(&host, &invocation)
        }
        Some(("play", sub)) => {
            let args = collect_args(&PLAY_SPEC, sub)?;
            let id = args
                .text("id")
                .ok_or_else(|| "缺少作品 id（见 `zela list`）".to_string())?
                .to_string();
            #[cfg(feature = "gui")]
            {
                zela_gui::launch_with(zela_gui::Initial::Demo(id.clone()))
                    .map_err(|e| e.to_string())?;
                Ok(Response::facts(
                    "play",
                    vec![("id".to_string(), Fact::Text(id))],
                ))
            }
            #[cfg(not(feature = "gui"))]
            {
                let _ = id;
                Err("本二进制未启用 `gui` feature（重新构建时加 `--features gui`）".to_string())
            }
        }
        Some(("gui", sub)) => {
            let _ = collect_args(&GUI_SPEC, sub)?;
            #[cfg(feature = "gui")]
            {
                zela_gui::launch().map_err(|e| e.to_string())?;
                Ok(Response::facts(
                    "gui",
                    vec![("launched".to_string(), Fact::Flag(true))],
                ))
            }
            #[cfg(not(feature = "gui"))]
            {
                Err("本二进制未启用 `gui` feature（重新构建时加 `--features gui`）".to_string())
            }
        }
        other => Err(format!("未知子命令：{other:?}")),
    }
}

/// 面向终端的人类可读输出（结构化返回值由 `Response` 提供，不在这里 parse）。
pub fn format_human(response: &Response) -> String {
    match response {
        Response::Render(summary) => {
            let target = summary
                .frames_dir
                .as_deref()
                .map(|dir| format!("序列帧 {}", dir.display()))
                .or_else(|| summary.out.as_deref().map(|out| out.display().to_string()))
                .unwrap_or_else(|| "(无输出)".to_string());
            let cancelled = if summary.cancelled { "（已取消）" } else { "" };
            if summary.skipped {
                return format!("序列帧已完整（{target}），跳过渲染");
            }
            format!(
                "渲染 {} 帧（{}×{} @ {} fps，{:?}）→ {target}{cancelled}",
                summary.frames, summary.size.0, summary.size.1, summary.fps, summary.backend
            )
        }
        Response::List(summary) => summary
            .demos
            .iter()
            .map(|row| format!("{:<22} {:<24} {} ({})", row.id, row.title, row.script, row.language))
            .collect::<Vec<_>>()
            .join("\n"),
        Response::Facts { facts, .. } => facts
            .iter()
            .map(|(name, fact)| match fact {
                Fact::Text(value) => format!("{name} = {value}"),
                Fact::Number(value) => format!("{name} = {value}"),
                Fact::Count(value) => format!("{name} = {value}"),
                Fact::Flag(value) => format!("{name} = {value}"),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// 供测试直接取 clap 匹配（避免重复构造命令树）。
pub fn matches_for(raw: &[&str]) -> Result<clap::ArgMatches, String> {
    let raw: Vec<String> = raw.iter().map(|value| value.to_string()).collect();
    root()
        .try_get_matches_from(raw)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zela_commands::commands::list::SPEC as LIST_SPEC;

    #[test]
    fn render_defaults_survive_the_clap_frontend() {
        let matches = matches_for(&["zela", "render", "demo.lua"]).expect("clap 可解析");
        let (_, sub) = matches.subcommand().expect("有子命令");
        let args = collect_args(&RENDER_SPEC, sub).expect("参数定型");
        let options = RenderOptions::from_args(&args).expect("渲染参数");
        assert_eq!(options.fps, 30.0);
        assert_eq!(options.size, (640, 480));
        assert_eq!(options.out, PathBuf::from("out.mp4"));
        assert_eq!(options.script, Some(PathBuf::from("demo.lua")));
    }

    #[test]
    fn list_runs_through_the_cli_host() {
        let matches = matches_for(&["zela", "list"]).expect("clap 可解析");
        let (_, sub) = matches.subcommand().expect("有子命令");
        let _ = collect_args(&LIST_SPEC, sub).expect("参数定型");
        let response = list_command::run(&CliHost::new()).expect("清单");
        assert_eq!(response.command(), "list");
    }

    #[test]
    fn unknown_argument_is_rejected() {
        assert!(matches_for(&["zela", "render", "demo.lua", "--bogus"]).is_err());
        assert!(matches_for(&["zela", "nope"]).is_err());
    }
}
