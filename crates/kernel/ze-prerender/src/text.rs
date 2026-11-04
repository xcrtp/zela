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
use ze_content::ir::{ContentIR, ContentSource, GlyphKey, Span, StyledPath};
use ze_interact::{CharBox, Point as IxPoint, Rect as IxRect, TextRun};
use ze_notion::{Color32, Path2D, Rect2D, Transform2D, Vec2};

use crate::metrics::{MetricsDb, MetricsError};

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

/// 一次「文本 → 图元 + 字符盒」的产物。
#[derive(Debug, Clone)]
pub struct TextOutput {
    pub ir: ContentIR,
    pub run: TextRun,
    /// 主字体没有轮廓的字符数（advance 可能来自回退字体；轮廓没有 ⇒ 记账）。
    pub missing_outlines: usize,
}

/// **CI-d**：装配期矢量字形缓存。
///
/// 键 = `(face 版本, glyph_id, 字号位模式)`。缓存的是**已缩放、未平移**的轮廓；
/// 每个出现点只做一次 translate ⇒ 同字形在大文本里只解析一次。
#[derive(Debug, Default)]
struct GlyphCache {
    outlines: BTreeMap<GlyphKey, Path2D>,
    face_versions: BTreeMap<(String, u32), u64>,
}

/// 直排文字引擎：度量与轮廓走**同一份**字体数据（`MetricsDb` 的面）。
#[derive(Debug, Clone)]
pub struct DirectTextEngine {
    db: MetricsDb,
    /// 字形缓存跨 clone 共享（同一引擎实例的所有消费者受益）。
    cache: Arc<Mutex<GlyphCache>>,
    /// 仅测试：真实解析轮廓的次数（缓存命中不计）。
    #[cfg(test)]
    outline_builds: Arc<std::sync::atomic::AtomicUsize>,
}

impl DirectTextEngine {
    pub fn new(db: MetricsDb) -> Self {
        Self {
            db,
            cache: Arc::new(Mutex::new(GlyphCache::default())),
            #[cfg(test)]
            outline_builds: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// `ZELA_TYPST_FONT_DIR` 优先，否则系统字体（与 typst 世界同一条发现规则）。
    pub fn from_env() -> Result<Self, MetricsError> {
        Ok(Self::new(MetricsDb::from_env()?))
    }

    pub fn db(&self) -> &MetricsDb {
        &self.db
    }

    /// **CI-d**：真实解析轮廓的次数（测试断言"同字形只解析一次"）。
    #[cfg(test)]
    pub fn outline_builds(&self) -> usize {
        self.outline_builds
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// face 版本：字体数据 + face index 的指纹（同一次装配内稳定）。
    fn face_version(
        cache: &mut GlyphCache,
        family: &str,
        index: u32,
        data: &[u8],
    ) -> u64 {
        let key = (family.to_string(), index);
        if let Some(version) = cache.face_versions.get(&key) {
            return *version;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data.hash(&mut hasher);
        index.hash(&mut hasher);
        let version = hasher.finish();
        cache.face_versions.insert(key, version);
        version
    }

    /// 取（或解析并缓存）**已缩放**的字形轮廓（未平移）。
    fn scaled_outline(
        &self,
        face: &Face<'_>,
        face_version: u64,
        ch: char,
        scale: f32,
        font_size: f32,
    ) -> Option<Path2D> {
        let glyph = face.glyph_index(ch)?;
        let key = GlyphKey {
            face_version,
            glyph_id: glyph.0,
            size_bits: font_size.to_bits(),
        };
        if let Some(path) = self
            .cache
            .lock()
            .expect("字形缓存锁中毒")
            .outlines
            .get(&key)
        {
            return Some(path.clone());
        }
        let scaled = outline(face, ch)?
            .transformed(&Transform2D::scale(Vec2::splat(scale)));
        #[cfg(test)]
        self.outline_builds
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut cache = self.cache.lock().expect("字形缓存锁中毒");
        cache.outlines.insert(key, scaled.clone());
        Some(scaled)
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
        let face_version = {
            let mut cache = self.cache.lock().expect("字形缓存锁中毒");
            Self::face_version(
                &mut cache,
                family.unwrap_or_default(),
                index,
                data.as_slice(),
            )
        };

        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let lines = metrics.wrap(wrap_width);
        let mut run = TextRun::new(f64::from(line_height));
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
                    rect: IxRect::new(
                        IxPoint::new(f64::from(pen), f64::from(baseline - descent)),
                        IxPoint::new(f64::from(pen + advance), f64::from(baseline + ascent)),
                    ),
                });

                if !ch.is_whitespace() {
                    match self.scaled_outline(&face, face_version, ch, scale, font_size) {
                        Some(path) => {
                            let placed = path
                                .transformed(&Transform2D::translate(Vec2::new(pen, baseline)));
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
            let dx = f64::from(shift.x);
            let dy = f64::from(shift.y);
            for char_box in &mut run.chars {
                char_box.rect = IxRect::new(
                    IxPoint::new(char_box.rect.min.x + dx, char_box.rect.min.y + dy),
                    IxPoint::new(char_box.rect.max.x + dx, char_box.rect.max.y + dy),
                );
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
            .map(|char_box| Vec2::new(char_box.rect.min.x as f32, char_box.rect.min.y as f32))
            .reduce(|a, b| Vec2::new(a.x.min(b.x), a.y.min(b.y))),
    }
}

fn union_of_boxes(run: &TextRun) -> Option<Rect2D> {
    run.chars
        .iter()
        .map(|char_box| {
            Rect2D::new(
                Vec2::new(char_box.rect.min.x as f32, char_box.rect.min.y as f32),
                Vec2::new(char_box.rect.max.x as f32, char_box.rect.max.y as f32),
            )
        })
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
            output.run
                .char_at(IxPoint::new(output.run.chars[0].rect.min.x, top)),
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

    /// **CI-d 判据**：同字形只解析一次；缓存前后 `ContentIR` 逐位相同。
    #[test]
    fn repeated_glyphs_parse_once_and_cache_does_not_change_the_ir() {
        let Some(engine) = engine_or_skip() else {
            return;
        };
        let cold = engine
            .layout("aaaa", None, 24.0, 0.0, Color32::BLACK)
            .expect("可排版");
        let builds = engine.outline_builds();
        assert_eq!(builds, 1, "同字形同字号只解析一次，实际 {builds}");

        let warm = engine
            .layout("aaaa", None, 24.0, 0.0, Color32::BLACK)
            .expect("可排版");
        assert_eq!(engine.outline_builds(), builds, "第二次全部命中缓存");
        assert_eq!(cold.ir, warm.ir, "缓存前后 ContentIR 必须逐位相同");

        // 字号变化 ⇒ 新键（size 是缓存键的一部分）。
        engine
            .layout("aaaa", None, 25.0, 0.0, Color32::BLACK)
            .expect("可排版");
        assert_eq!(engine.outline_builds(), builds + 1);
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
