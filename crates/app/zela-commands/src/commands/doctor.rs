//! `doctor`：环境自检（本批最值钱的发现之一）。
//!
//! # 防的那个坑
//!
//! `ze-prerender/src/typst/world.rs` 在 `ZELA_TYPST_FONT_DIR` 缺失时**静默**退回
//! 系统字体：不报错，但带文本 / 公式的作品在这台机器上与别人不同（payload / 帧哈希漂移）。
//! CI 里由 xtask 硬注入，那层保护**只在 CI**。`doctor` 把它变成用户可见的警告。

use zela_render_backends::registry::BackendKind;
use zela_render_backends::{BackendRegistry, DEFAULT_TARGET_FORMAT, RenderContext};

use crate::host::{CommandHost, Level};
use crate::response::{Fact, Response};
use crate::spec::CommandSpec;

pub const SPEC: CommandSpec = CommandSpec {
    name: "doctor",
    about: "自检：字体 / ffmpeg / 后端 / Python",
    args: &[],
};

pub fn run(host: &dyn CommandHost) -> Result<Response, String> {
    // ── 1. 确定字体（静默回退是本项目最不该发生的事）──
    let fonts_ok = check_fonts(host);

    // ── 2. ffmpeg（mp4 输出依赖它）──
    let ffmpeg = std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if ffmpeg {
        host.diagnostic(Level::Info, "ffmpeg：可用（mp4 输出）");
    } else {
        host.diagnostic(
            Level::Warn,
            "ffmpeg：未找到（不影响 PNG 序列帧；需要 mp4 时请安装 ffmpeg）",
        );
    }

    // ── 3. 后端可用性（GPU）──
    let (gpu_ok, gpu_detail) = check_backends(host);

    // ── 4. Python（可选）──
    let python = cfg!(feature = "python");
    if python {
        host.diagnostic(Level::Info, "Python：本二进制已启用（`.py` 脚本可用）");
    } else {
        host.diagnostic(
            Level::Info,
            "Python：未启用（`.py` 脚本不可用；需要时用 `--features python` 构建）",
        );
    }

    Ok(Response::facts(
        "doctor",
        vec![
            ("fonts".to_string(), Fact::Flag(fonts_ok)),
            ("ffmpeg".to_string(), Fact::Flag(ffmpeg)),
            ("gpu".to_string(), Fact::Flag(gpu_ok)),
            ("gpu_detail".to_string(), Fact::Text(gpu_detail)),
            ("python".to_string(), Fact::Flag(python)),
        ],
    ))
}

fn check_fonts(host: &dyn CommandHost) -> bool {
    if let Some(dir) = std::env::var_os("ZELA_TYPST_FONT_DIR").map(std::path::PathBuf::from) {
        if dir.is_dir() {
            host.diagnostic(
                Level::Info,
                &format!("字体：ZELA_TYPST_FONT_DIR = {}", dir.display()),
            );
            return true;
        }
        host.diagnostic(
            Level::Warn,
            &format!(
                "字体：ZELA_TYPST_FONT_DIR 指向 {}，但目录不存在 ⇒ 会静默回退系统字体",
                dir.display()
            ),
        );
    }
    match crate::commands::fonts::detect() {
        Ok((set, missing)) if missing.is_empty() => {
            host.diagnostic(
                Level::Info,
                &format!("字体：确定字体已就绪（{}）", set.dir.display()),
            );
            true
        }
        Ok((set, missing)) => {
            host.diagnostic(
                Level::Warn,
                &format!(
                    "字体：缺少 {}（{}）⇒ 会**静默回退系统字体**，带文本/公式的作品与别人不同。\n  修法：运行 `zela fonts`",
                    missing.join(", "),
                    set.dir.display()
                ),
            );
            false
        }
        Err(error) => {
            host.diagnostic(Level::Warn, &format!("字体：无法检测（{error}）"));
            false
        }
    }
}

fn check_backends(host: &dyn CommandHost) -> (bool, String) {
    let mut registry = BackendRegistry::with_standard();
    let _ = registry.register(BackendKind::Vector);
    let ctx = match RenderContext::headless_with_registry(DEFAULT_TARGET_FORMAT, &registry) {
        Ok(ctx) => ctx,
        Err(error) => {
            host.diagnostic(Level::Warn, &format!("GPU：无法创建 headless 上下文（{error}）"));
            return (false, format!("{error}"));
        }
    };
    let Some(adapter) = ctx.adapter() else {
        host.diagnostic(Level::Warn, "GPU：上下文没有 adapter");
        return (false, "no adapter".to_string());
    };
    let mut usable = Vec::new();
    for entry in registry.availability(adapter) {
        match entry.reason {
            Some(reason) => host.diagnostic(
                Level::Warn,
                &format!("后端 {:?} 不可用：{reason}", entry.kind),
            ),
            None => {
                usable.push(format!("{:?}", entry.kind));
                host.diagnostic(Level::Info, &format!("后端 {:?} 可用", entry.kind));
            }
        }
    }
    let ok = !usable.is_empty();
    (ok, usable.join(","))
}
