//! 确定字体（`fonts`）：下载 + SHA-256 校验 `tools/typst-fonts.txt` 的清单。
//!
//! # 为什么要有它
//!
//! 以前下载字体要 `pwsh crates/web/zela-web/build.ps1 -FontsOnly` —— 对使用者完全
//! 莫名其妙，还要求装 pwsh。这里用系统自带的 `curl` 完成同一件事。
//!
//! # CH-e：清单解析已下沉
//!
//! 字体不是 CLI 专属（GUI 装配 HUD 标签也要用同一套 face）⇒ [`manifest`] /
//! [`verify_file`] / [`detect`] / [`ensure_font_dir`] 的唯一实现在
//! [`zela_app::fonts`]，本模块只保留**下载**这一步（它需要命令宿主做诊断输出）。

use std::path::PathBuf;

use crate::host::{CommandHost, Level};
use crate::response::{Fact, Response};
use crate::spec::CommandSpec;

pub use zela_app::fonts::{FontSet, detect, ensure_font_dir, manifest, verify_file};

pub const SPEC: CommandSpec = CommandSpec {
    name: "fonts",
    about: "下载并校验确定性字体（Typst 字体集）",
    args: &[],
};

/// 执行下载 + 校验。
pub fn run(host: &dyn CommandHost) -> Result<Response, String> {
    let (set, missing) = detect()?;
    if missing.is_empty() {
        host.diagnostic(
            Level::Info,
            &format!("字体已就绪：{}（v{}）", set.dir.display(), set.version),
        );
        unsafe {
            std::env::set_var("ZELA_TYPST_FONT_DIR", &set.dir);
        }
        return Ok(Response::facts(
            "fonts",
            vec![
                ("dir".to_string(), Fact::Text(set.dir.display().to_string())),
                ("downloaded".to_string(), Fact::Count(0)),
            ],
        ));
    }

    std::fs::create_dir_all(&set.dir)
        .map_err(|e| format!("创建目录 {} 失败：{e}", set.dir.display()))?;
    let base = format!(
        "https://raw.githubusercontent.com/typst/typst-assets/v{}/files/fonts",
        set.version
    );
    let mut downloaded = 0u64;
    for (name, hash) in &set.files {
        if !missing.contains(name) {
            continue;
        }
        let target: PathBuf = set.dir.join(name);
        host.diagnostic(Level::Info, &format!("下载 {name}"));
        let status = std::process::Command::new("curl")
            .args(["-fL", "--retry", "3", "-o"])
            .arg(&target)
            .arg(format!("{base}/{name}"))
            .status()
            .map_err(|e| format!("启动 curl 失败（安装 curl 或用 build.ps1 -FontsOnly）：{e}"))?;
        if !status.success() {
            return Err(format!("下载 {name} 失败（curl 退出码 {status}）"));
        }
        let actual = zela_scene_bundle::sha256::hex(&zela_scene_bundle::sha256::sha256(
            &std::fs::read(&target).map_err(|e| format!("读取 {} 失败：{e}", target.display()))?,
        ));
        if &actual != hash {
            let _ = std::fs::remove_file(&target);
            return Err(format!(
                "字体校验失败（{name}）：实测 {actual}，期望 {hash}"
            ));
        }
        downloaded += 1;
    }
    unsafe {
        std::env::set_var("ZELA_TYPST_FONT_DIR", &set.dir);
    }
    host.diagnostic(
        Level::Info,
        &format!(
            "字体就绪：{}（v{}）。已为当前进程设置 ZELA_TYPST_FONT_DIR；\
             其它进程请自行 export",
            set.dir.display(),
            set.version
        ),
    );
    Ok(Response::facts(
        "fonts",
        vec![
            ("dir".to_string(), Fact::Text(set.dir.display().to_string())),
            ("downloaded".to_string(), Fact::Count(downloaded)),
        ],
    ))
}
