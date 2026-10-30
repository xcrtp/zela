//! `zela-verify`：批量对拍的**命令行壳**（参数解析 + GPU 上下文 + 退出码）。
//!
//! 帧体在 [`zela_commands::commands::batch`]（宿主无关，GUI / 批量出片复用同一实现）。
//!
//! ```text
//! zela-verify --scripts DIR --tmp DIR --work name=WxH[:vector] [--work ...]
//!             [--fps 30] [--sink-bypass]
//! ```
//!
//! # 输出（机器可读，一行一个作品）
//!
//! ```text
//! [decision] {"work":"fourier","pass":"script","backend":"Wgpu",…}
//! [structure] {"work":"fourier","backend":"wgpu","pass":"script","digest":"…"}
//! [verify] {"work":"fourier","size":"640x480","frames":216,"script":"…","bundle":"…","payload":"…"}
//! ```
//!
//! 输出格式是 `xtask` 的解析契约：**不得改**（`--step hashes` 逐行读取）。

use std::path::PathBuf;

use zela_app::frame_loop::LoopBackend;
use zela_commands::commands::batch::{self, BatchOptions, Work};
use zela_render_backends::RenderContext;

#[derive(Debug)]
struct Args {
    scripts: PathBuf,
    tmp: PathBuf,
    fps: f64,
    works: Vec<Work>,
    /// B7 Step 0.3：把 V3-12（挂/不挂 HashSink 的像素一致性）并入本进程。
    sink_bypass: bool,
}

impl Args {
    /// 纯解析入口（默认值快照的被测点）。
    fn parse_from(raw: &[String]) -> Result<Self, String> {
        if raw.is_empty() || raw.iter().any(|a| a == "--help" || a == "-h") {
            return Err(
                "用法：zela-verify --scripts DIR --tmp DIR --work name=WxH[:vector] [--work ...]\n\
                 　　：zela-verify --scripts DIR --tmp DIR --sink-bypass（V3-12 检查）"
                    .to_string(),
            );
        }
        let mut scripts: Option<PathBuf> = None;
        let mut tmp: Option<PathBuf> = None;
        let mut fps = 30.0f64;
        let mut works = Vec::new();
        let mut sink_bypass = false;
        let mut i = 0;
        while i < raw.len() {
            match raw[i].as_str() {
                "--scripts" => {
                    i += 1;
                    scripts = Some(PathBuf::from(
                        raw.get(i).ok_or("--scripts 需要一个目录参数")?,
                    ));
                }
                "--tmp" => {
                    i += 1;
                    tmp = Some(PathBuf::from(raw.get(i).ok_or("--tmp 需要一个目录参数")?));
                }
                "--fps" => {
                    i += 1;
                    fps = raw
                        .get(i)
                        .ok_or("--fps 需要一个数字参数")?
                        .parse()
                        .map_err(|_| "--fps 必须是数字".to_string())?;
                }
                "--work" => {
                    i += 1;
                    works.push(parse_work(raw.get(i).ok_or("--work 需要 name=WxH[:vector]")?)?);
                }
                "--sink-bypass" => sink_bypass = true,
                other => return Err(format!("未知参数：{other}")),
            }
            i += 1;
        }
        Ok(Self {
            scripts: scripts.ok_or("缺少 --scripts")?,
            tmp: tmp.ok_or("缺少 --tmp")?,
            fps,
            works,
            sink_bypass,
        })
    }
}

/// `fourier=640x480` / `vector_paths=640x480:vector`。
fn parse_work(spec: &str) -> Result<Work, String> {
    let (name, rest) = spec
        .split_once('=')
        .ok_or_else(|| format!("--work 需要一个 name=WxH[:backend]：{spec}"))?;
    let (size, backend) = match rest.split_once(':') {
        Some((size, "vector")) => (size, LoopBackend::Vector),
        Some((size, "wgpu")) => (size, LoopBackend::Wgpu),
        Some((_, other)) => return Err(format!("--work 的后端只接受 wgpu|vector：{other}")),
        None => (rest, LoopBackend::Wgpu),
    };
    let (w, h) = size
        .split_once('x')
        .ok_or_else(|| format!("--work 的尺寸必须是 WxH：{size}"))?;
    Ok(Work {
        name: name.to_string(),
        width: w.parse().map_err(|_| format!("宽度不是数字：{w}"))?,
        height: h.parse().map_err(|_| format!("高度不是数字：{h}"))?,
        backend,
    })
}

fn main() {
    // B12 Step 1.1：门禁路径也接上订阅器（此前默认采集、默认丢弃）。
    zela_schedule::telemetry::init_tracing_headless();
    if let Err(message) = run() {
        eprintln!("zela-verify: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = Args::parse_from(&raw)?;
    std::fs::create_dir_all(&args.tmp)
        .map_err(|e| format!("创建目录 {} 失败：{e}", args.tmp.display()))?;

    // **一次** GPU 初始化：逐作品的进程启动开销在这里被摊平。
    //
    // B12 Step 4：只有显式开启 GPU 计时才请求 `TIMESTAMP_QUERY`（默认空集，B11 C2）。
    // 请求失败 ⇒ 退回不请求，让 `run_frames` 把状态标成 `unsupported` 并带理由。
    let want_gpu_timing = std::env::var("ZELA_GPU_TIMING").is_ok_and(|value| value != "0");
    let ctx = if want_gpu_timing {
        match RenderContext::headless_with_format_and_features(
            zela_render_backends::DEFAULT_TARGET_FORMAT,
            zela_render_backends::gpu_timing::TIMING_FEATURE,
        ) {
            Ok(ctx) => ctx,
            Err(error) => {
                eprintln!(
                    "[gpu-timing] 无法请求 TIMESTAMP_QUERY：{error}（退回不测，**不填 0**）"
                );
                RenderContext::headless()
                    .map_err(|e| format!("无法创建 headless GPU 上下文：{e}"))?
            }
        }
    } else {
        RenderContext::headless().map_err(|e| format!("无法创建 headless GPU 上下文：{e}"))?
    };

    // B12 Step 3：环境指纹**必填**（GPU / API / os / arch / debug 断言），报告里逐条带上。
    let env = zela_app::frame_loop::environment_json(ctx.adapter());

    if args.sink_bypass {
        return batch::run_sink_bypass(&ctx, &args.scripts, &args.tmp, args.fps, &env);
    }

    batch::run_batch(
        &ctx,
        &BatchOptions {
            scripts: args.scripts,
            tmp: args.tmp,
            fps: args.fps,
            works: args.works,
        },
    )?;
    Ok(())
}

/// **阶段 0：默认值快照**（仓库门面批次 §阶段 0）。
///
/// `zela-verify` 的默认值（fps、`--work` 的尺寸切分与后端缺省）直接决定 36 条像素
/// 基线与结构指纹的口径 ⇒ 迁移到 `zela-commands` 后必须逐条复现。
#[cfg(test)]
mod defaults_snapshot {
    use super::*;

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|value| value.to_string()).collect()
    }

    fn base(raw: &[&str]) -> Vec<String> {
        let mut out = args(&["--scripts", "scripts", "--tmp", "tmp"]);
        out.extend(args(raw));
        out
    }

    #[test]
    fn fps_defaults_to_30_and_sink_bypass_is_off() {
        let parsed = Args::parse_from(&base(&["--work", "fourier=640x480"])).expect("可解析");
        assert_eq!(parsed.fps, 30.0, "默认 30 fps");
        assert!(!parsed.sink_bypass, "--sink-bypass 默认关");
        assert_eq!(parsed.works.len(), 1);
        assert_eq!(parsed.works[0].name, "fourier");
        assert_eq!((parsed.works[0].width, parsed.works[0].height), (640, 480));
        assert_eq!(
            parsed.works[0].backend,
            LoopBackend::Wgpu,
            "--work 未写后端 ⇒ wgpu（批量对拍基线口径）"
        );
    }

    #[test]
    fn work_backend_suffix_is_parsed() {
        let parsed =
            Args::parse_from(&base(&["--work", "vector_paths=640x480:vector"])).expect("可解析");
        assert_eq!(parsed.works[0].backend, LoopBackend::Vector);
        assert!(parse_work("x=640x480:metal").is_err(), "未知后端必须失败");
        assert!(parse_work("x=640").is_err(), "缺高度必须失败");
        assert!(parse_work("x").is_err(), "缺尺寸必须失败");
    }

    #[test]
    fn required_directories_fail_explicitly() {
        assert!(Args::parse_from(&args(&["--tmp", "tmp", "--work", "a=1x1"])).is_err());
        assert!(Args::parse_from(&args(&["--scripts", "s", "--work", "a=1x1"])).is_err());
    }

    #[test]
    fn help_is_an_error_with_usage() {
        let error = Args::parse_from(&args(&["--help"])).expect_err("--help 走用法分支");
        assert!(error.contains("用法：zela-verify"));
    }
}
