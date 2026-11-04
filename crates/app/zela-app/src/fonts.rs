//! **CH-e**：确定性字体的环境解析（`tools/typst-fonts.txt` 清单 + 本地校验）。
//!
//! # 为什么从 `zela-commands` 下沉
//!
//! 字体不是 CLI 专属：GUI 装配 HUD 标签时同样要知道"用哪一套 face"。下沉到装配层
//! 后 GUI / headless / CLI 三条入口共用**同一份清单解析**，不再各自读环境变量。
//!
//! # 角色（CD 的 `CapabilityRole` 口径）
//!
//! face 清单是 **`Baked`**：回放端不执行它（标签几何已在 payload 里），**不进
//! `required_capabilities`**。否则会把 CD 刚拆掉的一元硬校验装回去。
//! 版本差异是可解释的差异：同一 payload + 同一窗口事件，`Frame` 语义逐位相同，
//! 像素只要求视觉一致（不要求 PNG 逐位）。
//!
//! # 边界
//!
//! 本模块**只解析与校验**，不下载 —— 下载要网络与 CLI 诊断（`zela-commands` 的
//! `fonts` 命令复用这里的 [`manifest`] / [`detect`]）。
//!
//! 字体目录固定为 `target/fonts/typst-assets-v<version>`；就绪时**自动设置**
//! `ZELA_TYPST_FONT_DIR`（当前进程），避免静默回退系统字体。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use zela_scene_bundle::sha256;

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

/// **CH-e**：装配环境的字体面（进 debug 快照的诊断层，**不进 payload**）。
///
/// `faces` 是清单列出的字体文件名；`missing` 非空 ⇒ 本机缺 face，
/// 标签会退化为"有轨道无文字"（CG-09）。两者都如实报告，不做隐式回退。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontEnvironment {
    /// 清单版本（`tools/typst-fonts.txt` 的 `version` 行）。
    pub version: String,
    /// 实际生效的目录（显式 `ZELA_TYPST_FONT_DIR` 优先）。
    pub dir: String,
    /// face 清单（字体文件名，稳定序 = 清单行序）。
    pub faces: Vec<String>,
    /// 缺失的 face（空 = 完整）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

/// 当前进程的字体环境（读清单 + **存在性**检查，不做哈希、不下载）。
///
/// `None` = 清单不可读（例如 wasm 构建里没有仓库文件）⇒ 快照里直接省略这一块，
/// 不用猜测值填充。只看存在性是因为本函数会进每帧诊断快照：哈希全量字体
/// （数 MB）会拖垮 `trace`，而"缺 face"这一判断只需要文件在不在。
pub fn environment() -> Option<FontEnvironment> {
    let set = manifest().ok()?;
    let dir = std::env::var_os("ZELA_TYPST_FONT_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| set.dir.clone());
    let missing: Vec<String> = set
        .files
        .iter()
        .filter(|(name, _)| !dir.join(name).is_file())
        .map(|(name, _)| name.clone())
        .collect();
    Some(FontEnvironment {
        version: set.version,
        dir: dir.display().to_string(),
        faces: set.files.iter().map(|(name, _)| name.clone()).collect(),
        missing,
    })
}
