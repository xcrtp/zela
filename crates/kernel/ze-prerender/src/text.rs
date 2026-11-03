//! 场景图元文字（CG-b）：直接由 ttf 轮廓生成 `ContentIR` + `TextRun`。
//!
//! # 与 typst 路径的关系
//!
//! **并行，不替换**：既有作品仍走 `typst`（像素基线零改动）；本模块给「UI 标签 /
//! 面板文字」这类**单行或短多行**文本一条轻路径 —— 只需要"把字摆出来、能点中"，
//! 不需要排版引擎。替换 typst 是后续独立一批（CH）。
//!
//! # 产出两份数据
//!
//! ```text
//! ContentIR.items   逐字形的轮廓（StyledPath；带字符的字节锚点）
//! ContentIR.runs    TextRun：字符下标 ↔ 矩形（不是"多一份几何"，是另一种索引）
//! ```
//!
//! 两者**平行**：轮廓决定"长什么样"，字符盒决定"点在哪、念什么"。前者给像素，
//! 后者给光标 / 读屏 / 命中反查 / Agent 断言。
//!
//! # 坐标系
//!
//! 与 typst 路径一致：`ContentIR` 用 **y 向上**，整体平移使包围盒最小角为 `(0, 0)`；
//! `baseline` 是基线在该坐标系里的 y。字符盒用同一坐标系。

use ttf_parser::{Face, GlyphId, OutlineBuilder};
use ze_content::ir::{ContentIR, ContentSource, Span, StyledPath};
use ze_content::text_metrics::{CharBox, TextRun};
use ze_notion::{Color32, Path2D, Rect2D, Transform2D, Vec2};

use crate::metrics::{MetricsDb, MetricsError};

/// 一次「文本 → 图元 + 字符盒」的产物。
#[derive(Debug, Clone)]
pub struct TextOutput {
    pub ir: ContentIR,
    pub run: TextRun,
    /// 主字体没有轮廓的字符数（advance 可能来自回退字体；轮廓没有 ⇒ 记账）。
    pub missing_outlines: usize,
}

/// 直排文字引擎：度量与轮廓走**同一份**字体数据（`MetricsDb` 的面）。
#[derive(Debug, Clone)]
pub struct DirectTextEngine {
    db: MetricsDb,
}

impl DirectTextEngine {
    pub fn new(db: MetricsDb) -> Self {
        Self { db }
    }

    /// `ZELA_TYPST_FONT_DIR` 优先，否则系统字体（与 typst 世界同一条发现规则）。
    pub fn from_env() -> Result<Self, MetricsError> {
        Ok(Self::new(MetricsDb::from_env()?))
    }

    pub fn db(&self) -> &MetricsDb {
        &self.db
    }

    /// 排版一段纯文本。
    ///
    /// - `wrap_width <= 0` / 非有限 ⇒ 不换行（只按 `\n` 断）；
    /// - `fill` 是轮廓填充色（描边留给调用方）；
    /// - 文本按原样进 `ContentSource`（**不做**数学源码归一化：UI 文本里的 `//`
    ///   不是注释）。
    pub fn layout(
        &self,
        text: &str,
        family: Option<&str>,
        font_size: f32,
        wrap_width: f32,
        fill: Color32,
    ) -> Result<TextOutput, MetricsError> {
        let metrics = self.db.measure(text, family, font_size)?;
        let (data, index) = self.db.face_data(family)?;
        let face = Face::parse(&data, index).map_err(|_| MetricsError::UnparsableFont {
            family: family.unwrap_or("<default>").to_string(),
            index,
        })?;
        let upem = f32::from(face.units_per_em().max(1));
        let scale = font_size / upem;
        let ascent = f32::from(face.ascender()) * scale;
        let descent = -f32::from(face.descender()) * scale;
        let line_height = metrics.line_height;

        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let lines = metrics.wrap(wrap_width);
        let mut run = TextRun::new(line_height);
        let mut items: Vec<StyledPath> = Vec::new();
        let mut missing_outlines = 0usize;

        for (line_index, line) in lines.iter().enumerate() {
            let mut pen = 0.0f32;
            // y 向上：第 n 行的基线在 `-n * line_height`。
            let baseline = -(line_index as f32) * line_height;
            for glyph_index in line.start..line.end {
                let Some(&(byte_start, ch)) = chars.get(glyph_index) else {
                    continue;
                };
                let advance = metrics.glyphs[glyph_index].advance;
                run.chars.push(CharBox {
                    index: glyph_index,
                    ch,
                    rect: Rect2D::new(
                        Vec2::new(pen, baseline - descent),
                        Vec2::new(pen + advance, baseline + ascent),
                    ),
                });

                if !ch.is_whitespace() {
                    match outline(&face, ch) {
                        Some(path) => {
                            let placed = path.transformed(
                                &Transform2D::scale(Vec2::splat(scale))
                                    .compose(Transform2D::translate(Vec2::new(pen, baseline))),
                            );
                            items.push(
                                StyledPath::filled(placed, fill).with_anchor(Span::new(
                                    byte_start,
                                    byte_start + ch.len_utf8(),
                                )),
                            );
                        }
                        None => missing_outlines += 1,
                    }
                }
                pen += advance;
            }
        }

        // 归一化：包围盒最小角移到 (0, 0)；基线随之平移。
        let min = bounds_min(&items, &run).unwrap_or(Vec2::ZERO);
        let shift = Vec2::new(-min.x, -min.y);
        if shift != Vec2::ZERO {
            let transform = Transform2D::translate(shift);
            for item in &mut items {
                item.path = item.path.transformed(&transform);
            }
            for char_box in &mut run.chars {
                char_box.rect = Rect2D::new(char_box.rect.min + shift, char_box.rect.max + shift);
            }
        }

        let mut ir = ContentIR::empty(ContentSource::from_normalized(text), font_size);
        ir.items = items;
        ir.recompute_bounds();
        if ir.items.is_empty() {
            // 全空白：没有轮廓可量 ⇒ 用字符盒的并集（显式，不退化成零面积）。
            ir.bounds = union_of_boxes(&run).unwrap_or_default();
        }
        ir.baseline = -min.y;
        ir.runs = vec![run.clone()];

        Ok(TextOutput {
            ir,
            run,
            missing_outlines,
        })
    }
}

fn outline(face: &Face<'_>, ch: char) -> Option<Path2D> {
    let glyph: GlyphId = face.glyph_index(ch)?;
    let mut sink = OutlineSink {
        path: Path2D::new(),
    };
    face.outline_glyph(glyph, &mut sink)?;
    (!sink.path.commands.is_empty()).then_some(sink.path)
}

fn bounds_min(items: &[StyledPath], run: &TextRun) -> Option<Vec2> {
    let from_items = items
        .iter()
        .filter_map(|item| item.path.compute_bounds())
        .reduce(|a, b| a.union(&b));
    match from_items {
        Some(bounds) => Some(bounds.min),
        None => run
            .chars
            .iter()
            .map(|char_box| char_box.rect.min)
            .reduce(|a, b| Vec2::new(a.x.min(b.x), a.y.min(b.y))),
    }
}

fn union_of_boxes(run: &TextRun) -> Option<Rect2D> {
    run.chars
        .iter()
        .map(|char_box| char_box.rect)
        .reduce(|a, b| a.union(&b))
}

/// ttf 轮廓 → [`Path2D`]（原始字体单位，y 向上）。
struct OutlineSink {
    path: Path2D,
}

impl OutlineBuilder for OutlineSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(Vec2::new(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(Vec2::new(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path.quad_to(Vec2::new(x1, y1), Vec2::new(x, y));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path
            .cubic_to(Vec2::new(x1, y1), Vec2::new(x2, y2), Vec2::new(x, y));
    }
    fn close(&mut self) {
        self.path.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_or_skip() -> Option<DirectTextEngine> {
        match DirectTextEngine::from_env() {
            Ok(engine) if !engine.db().families().is_empty() => Some(engine),
            Ok(_) => {
                eprintln!("[text] 跳过：本机没有可用字体");
                None
            }
            Err(error) => {
                eprintln!("[text] 跳过：{error}");
                None
            }
        }
    }

    #[test]
    fn layout_produces_outlines_and_char_boxes() {
        let Some(engine) = engine_or_skip() else {
            return;
        };
        let output = engine
            .layout("Hi g 你", None, 24.0, 0.0, Color32::BLACK)
            .expect("可排版");
        assert_eq!(output.run.chars.len(), 6, "H / i / 空格 / g / 空格 / 你");
        assert!(!output.ir.items.is_empty(), "有轮廓");
        assert_eq!(output.ir.runs.len(), 1);
        assert_eq!(output.ir.source.text, "Hi g 你", "源文本原样保留");
        // 归一化后最小角是 (0,0)；`g` 有下伸部 ⇒ 基线在包围盒内（> 0）。
        assert!(output.ir.bounds.min.x.abs() < 1e-3 && output.ir.bounds.min.y.abs() < 1e-3);
        assert!(output.ir.baseline > 0.0);
        assert!(output.run.line_height > 0.0);
    }

    /// **CG-b 主判据**：点 → 字符下标。
    #[test]
    fn point_maps_back_to_a_character_index() {
        let Some(engine) = engine_or_skip() else {
            return;
        };
        let output = engine
            .layout("abc", None, 20.0, 0.0, Color32::BLACK)
            .expect("可排版");
        for (index, char_box) in output.run.chars.iter().enumerate() {
            let center = char_box.rect.center();
            assert_eq!(
                output.run.char_at(center),
                Some(index),
                "字符 {index} 的中心必须命中它自己"
            );
        }
        // 所有盒上方（y 超过上界）不命中。
        let top = output.run.chars[0].rect.max.y + 1.0;
        assert_eq!(
            output.run.char_at(Vec2::new(output.run.chars[0].rect.min.x, top)),
            None
        );
    }

    #[test]
    fn two_layouts_are_bitwise_identical() {
        let Some(engine) = engine_or_skip() else {
            return;
        };
        let a = engine
            .layout("Panel 面板", None, 18.0, 0.0, Color32::WHITE)
            .expect("可排版");
        let b = engine
            .layout("Panel 面板", None, 18.0, 0.0, Color32::WHITE)
            .expect("可排版");
        assert_eq!(a.ir, b.ir);
        assert_eq!(a.run, b.run);
    }

    #[test]
    fn wrapping_puts_later_lines_below() {
        let Some(engine) = engine_or_skip() else {
            return;
        };
        let text = "alpha beta gamma delta";
        let single = engine
            .layout(text, None, 16.0, 0.0, Color32::BLACK)
            .expect("可排版");
        let wrapped = engine
            .layout(text, None, 16.0, single.ir.bounds.size().x * 0.5, Color32::BLACK)
            .expect("可排版");
        let single_height = single.ir.bounds.size().y;
        assert!(
            wrapped.ir.bounds.size().y > single_height,
            "换行后应更高：{} vs {}",
            wrapped.ir.bounds.size().y,
            single_height
        );
        // 后续行的字符盒在更低的 y（y 向上 ⇒ 更小的 y）。
        let first_line_y = wrapped.run.chars[0].rect.min.y;
        let last_y = wrapped.run.chars.last().unwrap().rect.min.y;
        assert!(last_y < first_line_y);
    }
}
