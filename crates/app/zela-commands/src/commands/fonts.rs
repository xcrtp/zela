//! 确定字体（`fonts`）：下载 + SHA-256 校验 `tools/typst-fonts.txt` 的清单。
//!
//! # 为什么要有它
//!
//! 以前下载字体要 `pwsh crates/web/zela-web/build.ps1 -FontsOnly` —— 对使用者完全
//! 莫名其妙，还要求装 pwsh。这里用系统自带的 `curl` 完成同一件事。
//!
//! 字体目录固定为 `target/fonts/typst-assets-v<version>`（与 build.ps1 同一来源）。
//! 找到后**自动设置** `ZELA_TYPST_FONT_DIR`（当前进程），避免静默回退系统字体。

use std::path::{Path, PathBuf};

use zela_scene_bundle::sha256;

use crate::host::{CommandHost, Level};
use crate::response::{Fact, Response};
use crate::spec::CommandSpec;

pub const SPEC: CommandSpec = CommandSpec {
    name: "fonts",
    about: "下载并校验确定性字体（Typst 字体集）",
    args: &[],
};

/// 字体清单（`tools/typst-fonts.txt` 的解析结果）。
#[derive(Debug, Clone)]
pub struct FontSet {
    pub version: String,
    pub dir: PathBuf,
    /// (文件名, SHA-256)。
    pub files: Vec<(String, String)>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

/// 解析清单：`version X` + `name hash` 行。
pub fn manifest() -> Result<FontSet, String> {
    let path = repo_root().join("tools").join("typst-fonts.txt");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取字体清单 {} 失败：{e}", path.display()))?;
    let mut version = None;
    let mut files = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cells = line.split_whitespace();
        match cells.next() {
            Some("version") => version = cells.next().map(str::to_string),
            Some(name) => {
                let hash = cells
                    .next()
                    .ok_or_else(|| format!("字体清单行缺 SHA-256：{line}"))?;
                files.push((name.to_string(), hash.to_ascii_lowercase()));
            }
            None => {}
        }
    }
    let version = version.ok_or("字体清单缺 `version` 行")?;
    let dir = repo_root()
        .join("target")
        .join("fonts")
        .join(format!("typst-assets-v{version}"));
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    Ok(FontSet {
        version,
        dir,
        files,
    })
}

/// 某个文件是否已存在且哈希匹配。
pub fn verify_file(dir: &Path, name: &str, expected: &str) -> bool {
    let path = dir.join(name);
    match std::fs::read(&path) {
        Ok(bytes) => sha256::hex(&sha256::sha256(&bytes)) == expected,
        Err(_) => false,
    }
}

/// 检测字体目录（不下载）。
///
/// 返回 `(FontSet, 缺失文件列表)`；清单本身读不到 ⇒ `Err`。
pub fn detect() -> Result<(FontSet, Vec<String>), String> {
    let set = manifest()?;
    let missing: Vec<String> = set
        .files
        .iter()
        .filter(|(name, hash)| !verify_file(&set.dir, name, hash))
        .map(|(name, _)| name.clone())
        .collect();
    Ok((set, missing))
}

/// 若字体已就绪，**自动**设置 `ZELA_TYPST_FONT_DIR` 并返回目录。
///
/// 用户显式设置的环境变量优先（尊重显式配置，即便目录里不是确定字体）。
pub fn ensure_font_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ZELA_TYPST_FONT_DIR").map(PathBuf::from) {
        if dir.is_dir() {
            return Some(dir);
        }
    }
    let (set, missing) = detect().ok()?;
    if !missing.is_empty() {
        return None;
    }
    // 单线程装配期设置：这是"确定性字体"与"静默回退"之间的开关。
    unsafe {
        std::env::set_var("ZELA_TYPST_FONT_DIR", &set.dir);
    }
    Some(set.dir)
}

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
        let target = set.dir.join(name);
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
        let actual = sha256::hex(&sha256::sha256(
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
