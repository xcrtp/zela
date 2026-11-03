//! 字体度量（装配期）：用 `fontdb` 找字体、`ttf-parser` 取 advance。
//!
//! # 口径（CG 计划 §7①）
//!
//! 度量在**装配期**算，结果进 [`TextMetrics`]（纯值）；运行期只查表，不引字体库。
//!
//! # 为什么不复用 typst 的排版结果
//!
//! typst 的 `TextItem` 已经在算 `x_advance`（见 `typst/convert.rs` 的
//! `advance_cursor`），但那是**排版内部**的中间量：拿它当度量接口，等于把
//! 「字体度量」钉在 typst 的版本与行为上。这里独立算（同一份 ttf 数据、同一套
//! font units），两条路径各管各的出口（CG-a 的判据是「可回读 + 逐位一致」，
//! 与既有 typst 像素基线互不影响）。
//!
//! # 确定性
//!
//! - 字体面按 `(family, index)` 升序排列 ⇒ 回退查找顺序确定；
//! - 同一次进程内相同输入逐位相同（无随机源、无哈希容器）；
//! - 字体文件本身变了，度量随之变化 —— 这是「可解释的差异」，由 CD 的
//!   `role = Source` 负责登记来源。

use std::path::Path;
use std::sync::Arc;

use ze_content::text_metrics::{GlyphMetrics, TextMetrics};

/// 与 typst 世界同一个环境变量：设置后**只**读该目录（构建脚本下载并校验的字体）。
pub const FONT_DIR_ENV: &str = "ZELA_TYPST_FONT_DIR";

/// 度量失败：显式，不静默返回空表。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MetricsError {
    #[error("没有可用字体（检查 {FONT_DIR_ENV} 或系统字体）")]
    NoFonts,
    #[error("找不到字体族 `{0}`")]
    UnknownFamily(String),
    #[error("字体面无法解析：`{family}` #{index}")]
    UnparsableFont { family: String, index: u32 },
}

/// 一个字体面（已读入内存，按需解析）。
#[derive(Debug, Clone)]
struct Face {
    family: String,
    index: u32,
    data: Arc<Vec<u8>>,
}

/// 度量用的字体集合。
#[derive(Debug, Clone, Default)]
pub struct MetricsDb {
    faces: Vec<Face>,
}

impl MetricsDb {
    /// 与 typst 世界同一条发现规则：`ZELA_TYPST_FONT_DIR` 优先，否则系统字体。
    pub fn from_env() -> Result<Self, MetricsError> {
        match std::env::var_os(FONT_DIR_ENV) {
            Some(dir) => Self::from_dir(Path::new(&dir)),
            None => Self::from_system(),
        }
    }

    /// 只加载目录中的字体文件（排序后读入，顺序确定）。
    pub fn from_dir(dir: &Path) -> Result<Self, MetricsError> {
        let mut db = fontdb::Database::new();
        db.load_fonts_dir(dir);
        Self::from_database(db)
    }

    /// 系统字体（跨机器不保证一致；装配期诊断会记录实际用到的族）。
    pub fn from_system() -> Result<Self, MetricsError> {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Self::from_database(db)
    }

    fn from_database(db: fontdb::Database) -> Result<Self, MetricsError> {
        let mut faces = Vec::new();
        for info in db.faces() {
            let Some((data, index)) = db.with_face_data(info.id, |data, index| (data.to_vec(), index))
            else {
                continue;
            };
            let family = info
                .families
                .first()
                .map(|(name, _)| name.clone())
                .unwrap_or_default();
            faces.push(Face {
                family,
                index,
                data: Arc::new(data),
            });
        }
        // 排序：回退查找的先后必须确定（fontdb 的枚举顺序取决于文件系统）。
        faces.sort_by(|a, b| a.family.cmp(&b.family).then(a.index.cmp(&b.index)));
        if faces.is_empty() {
            return Err(MetricsError::NoFonts);
        }
        Ok(Self { faces })
    }

    /// 可用字体族（升序、去重）。
    pub fn families(&self) -> Vec<String> {
        let mut families: Vec<String> = self.faces.iter().map(|f| f.family.clone()).collect();
        families.sort();
        families.dedup();
        families
    }

    /// 度量一段文本。
    ///
    /// - `family = None` ⇒ 用排序后的第一个面（显式默认，不猜"最合适"的字体）；
    /// - 主字体缺字时按排序顺序回退到第一个含该字形的面，并记进
    ///   [`TextMetrics::fallback_families`]；
    /// - 全部缺字 ⇒ `advance = 0` 且 `missing += 1`（显式记账）。
    pub fn measure(
        &self,
        text: &str,
        family: Option<&str>,
        font_size: f32,
    ) -> Result<TextMetrics, MetricsError> {
        let primary = match family {
            Some(name) => self
                .faces
                .iter()
                .position(|face| face.family.eq_ignore_ascii_case(name))
                .ok_or_else(|| MetricsError::UnknownFamily(name.to_string()))?,
            None => 0,
        };
        let primary_face = parse_face(&self.faces[primary])?;
        let upem = f32::from(primary_face.units_per_em().max(1));
        let ascent = f32::from(primary_face.ascender()) / upem * font_size;
        let descent = -f32::from(primary_face.descender()) / upem * font_size;
        let gap = f32::from(primary_face.line_gap()) / upem * font_size;

        let mut metrics = TextMetrics::empty(self.faces[primary].family.clone(), font_size);
        metrics.ascent = ascent;
        metrics.line_height = ascent + descent + gap;

        let mut cursor = 0.0f32;
        for ch in text.chars() {
            let advance = if ch == '\n' {
                0.0
            } else if let Some(advance) = advance_of(&primary_face, ch, upem, font_size) {
                advance
            } else {
                // 回退：按排序顺序找第一个含该字形的面。
                let mut found = None;
                for (index, face) in self.faces.iter().enumerate() {
                    if index == primary {
                        continue;
                    }
                    let Ok(parsed) = parse_face(face) else {
                        continue;
                    };
                    let fallback_upem = f32::from(parsed.units_per_em().max(1));
                    if let Some(advance) = advance_of(&parsed, ch, fallback_upem, font_size) {
                        found = Some((advance, face.family.clone()));
                        break;
                    }
                }
                match found {
                    Some((advance, fallback_family)) => {
                        if !metrics.fallback_families.contains(&fallback_family) {
                            metrics.fallback_families.push(fallback_family);
                        }
                        advance
                    }
                    None => {
                        metrics.missing += 1;
                        0.0
                    }
                }
            };
            metrics.glyphs.push(GlyphMetrics {
                ch,
                x: cursor,
                advance,
            });
            cursor += advance;
        }
        metrics.fallback_families.sort();
        metrics.width = cursor;
        Ok(metrics)
    }
}

fn parse_face(face: &Face) -> Result<ttf_parser::Face<'_>, MetricsError> {
    ttf_parser::Face::parse(&face.data, face.index).map_err(|_| MetricsError::UnparsableFont {
        family: face.family.clone(),
        index: face.index,
    })
}

/// 单字符 advance（px）。字体缺该字形 / 无水平度量 ⇒ `None`。
fn advance_of(face: &ttf_parser::Face<'_>, ch: char, upem: f32, font_size: f32) -> Option<f32> {
    let glyph = face.glyph_index(ch)?;
    let advance = face.glyph_hor_advance(glyph)?;
    Some(f32::from(advance) / upem * font_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 系统字体不可用时不静默通过：打印一行显式跳过。
    fn db_or_skip() -> Option<MetricsDb> {
        match MetricsDb::from_env() {
            Ok(db) if !db.families().is_empty() => Some(db),
            Ok(_) => {
                eprintln!("[metrics] 跳过：本机没有可用字体");
                None
            }
            Err(error) => {
                eprintln!("[metrics] 跳过：{error}");
                None
            }
        }
    }

    #[test]
    fn measure_is_deterministic_and_serializable() {
        let Some(db) = db_or_skip() else {
            return;
        };
        let first = db.measure("Hello, 世界", None, 16.0).expect("可度量");
        let second = db.measure("Hello, 世界", None, 16.0).expect("可度量");
        assert_eq!(first, second, "同字体同串两次度量必须逐位相同");
        assert!(first.width > 0.0);
        assert_eq!(first.glyphs.len(), "Hello, 世界".chars().count());
        assert_eq!(
            first.glyphs.iter().map(|g| g.advance).sum::<f32>(),
            first.width,
            "总宽 = 逐字符推进之和"
        );
        let json = serde_json::to_string(&first).expect("可序列化");
        let back: TextMetrics = serde_json::from_str(&json).expect("可反序列化");
        assert_eq!(first, back, "度量表可回读");
    }

    #[test]
    fn advance_grows_with_font_size() {
        let Some(db) = db_or_skip() else {
            return;
        };
        let small = db.measure("abc", None, 10.0).expect("可度量");
        let large = db.measure("abc", None, 20.0).expect("可度量");
        assert!(
            (large.width - small.width * 2.0).abs() < 0.05,
            "字号翻倍 ⇒ 宽度翻倍（线性度量）：{} vs {}",
            small.width,
            large.width
        );
        assert!(large.line_height > small.line_height);
    }

    #[test]
    fn unknown_family_is_an_explicit_error() {
        let Some(db) = db_or_skip() else {
            return;
        };
        let error = db
            .measure("x", Some("Definitely Not A Real Font 12345"), 12.0)
            .unwrap_err();
        assert!(matches!(error, MetricsError::UnknownFamily(_)));
    }

    #[test]
    fn point_maps_back_to_a_character() {
        let Some(db) = db_or_skip() else {
            return;
        };
        let metrics = db.measure("abc", None, 20.0).expect("可度量");
        let middle = metrics.glyphs[1].x + metrics.glyphs[1].advance * 0.5;
        assert_eq!(metrics.char_index_at(middle), Some(1));
        assert_eq!(metrics.char_index_at(metrics.width + 1.0), None);
    }
}
