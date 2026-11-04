//! **CI-d 口子④**：文字能力的策略形状。
//!
//! 本模块**只定义入口**，不定义语义：
//! - [`MetricsProvider`] / [`LineBreaker`] 把**已经存在**的函数收在 trait 后面；
//! - [`Shaping`] / [`Bidi`] 只留签名，默认实现是 Identity 直通（拉丁 + CJK 不需要）；
//! - [`MathLayout`] 是公式的默认出口（具体实现交给 `ze-prerender` 的 Typst 能力）。
//!
//! 三条硬约束（§0.5）：
//! 1. 只定义形状，不定义语义；
//! 2. 默认实现与今天的计算逐位相同；
//! 3. 不引入新依赖、不增加运行期分支。

use ze_notion::Color32;

use crate::text_metrics::{TextLine, TextMetrics};
use crate::ContentIR;

/// 字体度量入口（包住 `ze-prerender::metrics::MetricsDb::measure`）。
pub trait MetricsProvider {
    type Error;

    fn measure(
        &self,
        text: &str,
        family: Option<&str>,
        font_size: f32,
    ) -> Result<TextMetrics, Self::Error>;
}

/// 断行入口（包住 `TextMetrics::wrap` 的贪心规则）。
pub trait LineBreaker {
    /// 半开区间 `[start, end)` 是 `metrics.glyphs` 的下标。
    fn line_breaks(&self, metrics: &TextMetrics, max_width: f32) -> Vec<TextLine>;
}

impl LineBreaker for TextMetrics {
    fn line_breaks(&self, _metrics: &TextMetrics, max_width: f32) -> Vec<TextLine> {
        self.wrap(max_width)
    }
}

/// 一个被 shaping 出来的字形（Identity = 一个字符一个字形）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// 字符标量下标（与 `TextRun` / `CharBox` 同口径）。
    pub char_index: usize,
    /// 字形 id（Identity 实现里 = 字符码位，仅作占位）。
    pub glyph_id: u32,
}

/// shaping 入口（形状；本批不实现复杂脚本）。
pub trait Shaping {
    fn shape(&self, text: &str) -> Vec<ShapedGlyph>;
}

/// **Identity 直通**：一个字符一个字形。
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentityShaping;

impl Shaping for IdentityShaping {
    fn shape(&self, text: &str) -> Vec<ShapedGlyph> {
        text.chars()
            .enumerate()
            .map(|(char_index, ch)| ShapedGlyph {
                char_index,
                glyph_id: ch as u32,
            })
            .collect()
    }
}

/// bidi 入口（返回**视觉顺序**的字符下标；默认 = 逻辑序）。
pub trait Bidi {
    fn visual_order(&self, text: &str) -> Vec<usize>;
}

/// **Identity 直通**：逻辑序即视觉序。
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentityBidi;

impl Bidi for IdentityBidi {
    fn visual_order(&self, text: &str) -> Vec<usize> {
        (0..text.chars().count()).collect()
    }
}

/// 公式排版入口（默认交给 Typst capability）。
pub trait MathLayout {
    type Error;

    fn layout_math(
        &self,
        source: &str,
        font_size: f32,
        fill: Color32,
    ) -> Result<ContentIR, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_metrics::{GlyphMetrics, TextMetrics};

    fn table() -> TextMetrics {
        let mut metrics = TextMetrics::empty("test", 10.0);
        metrics.line_height = 12.0;
        metrics.ascent = 8.0;
        let mut x = 0.0;
        for ch in "ab 中".chars() {
            let advance = match ch {
                'a' => 10.0,
                'b' => 20.0,
                ' ' => 5.0,
                _ => 30.0,
            };
            metrics.glyphs.push(GlyphMetrics { ch, x, advance });
            x += advance;
        }
        metrics.width = x;
        metrics
    }

    /// 口子④：trait 默认实现与直接调用**逐位相同**。
    #[test]
    fn line_breaker_default_matches_the_existing_wrap() {
        let metrics = table();
        let via_trait = LineBreaker::line_breaks(&metrics, &metrics, 40.0);
        assert_eq!(via_trait, metrics.wrap(40.0));
    }

    #[test]
    fn identity_shaping_and_bidi_are_straight_through() {
        let shaped = IdentityShaping.shape("ab中");
        assert_eq!(shaped.len(), 3);
        assert_eq!(shaped[2].char_index, 2);
        assert_eq!(IdentityBidi.visual_order("ab中"), vec![0, 1, 2]);
    }
}
