//! 字体度量表（装配期烘焙的**纯值**）。
//!
//! # 它是谁
//!
//! 一次「文本 + 字体 + 字号」测量的结果：逐字符的**起始 x** 与**推进宽度**，
//! 加上行高 / 基线。字形轮廓另有去处（`ContentIR` 的 `StyledPath`）；本模块只管
//! 「字放在哪里、占多宽」。
//!
//! # 为什么在 kernel 的 `ze-content`
//!
//! 它是**数据**：可序列化、可比较、可哈希，运行期只查表（口径见 CG 计划 §7①：
//! 运行期不引字体库）。产生它的字体库在 `ze-prerender`（比本 crate 重）；
//! 本模块零依赖 `ze-notion` 之外的任何东西。
//!
//! # 收窄
//!
//! 拉丁 + CJK，**不做 shaping**：没有连字、没有上下文变形、没有 GSUB/GPOS。
//! 换行只按「空格 / 连字符 / CJK 字边界」这三类机会贪心断行。

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphMetrics {
    /// 字符（一个 UTF-8 标量）。
    pub ch: char,
    /// 起始 x（相对文本原点）。
    pub x: f32,
    /// 推进宽度（下一字符的起点 = `x + advance`）。
    pub advance: f32,
}

/// 一段文本的度量表。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct TextMetrics {
    /// 主字体族（诊断用）。
    pub family: String,
    /// 测量时用的字号。
    pub font_size: f32,
    /// 基线到行顶的距离。
    pub ascent: f32,
    /// 行高（`ascent + descent + line_gap`）。
    pub line_height: f32,
    /// 总推进宽度（末字符 `x + advance`）。
    pub width: f32,
    /// 逐字符度量，按输入顺序。
    pub glyphs: Vec<GlyphMetrics>,
    /// 主字体缺字时实际用到的回退字体族（升序去重）。
    pub fallback_families: Vec<String>,
    /// 全部字体都缺的字形个数（显式记账，不静默）。
    pub missing: u32,
}

/// 一条断行结果（半开区间：`[start, end)` 是 `glyphs` 的下标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextLine {
    pub start: usize,
    pub end: usize,
}

impl TextMetrics {
    /// 空文本的度量（行高仍来自字体）。
    pub fn empty(family: impl Into<String>, font_size: f32) -> Self {
        Self {
            family: family.into(),
            font_size,
            ascent: 0.0,
            line_height: 0.0,
            width: 0.0,
            glyphs: Vec::new(),
            fallback_families: Vec::new(),
            missing: 0,
        }
    }

    /// 点 → 字符下标（半开区间 `[x, x + advance)`）。
    ///
    /// 这套口径与 `ze-interact::Rect::contains` 一致：相邻字符共享的边界点只属于
    /// 后一个。超出末尾返回 `None`（不是"最后一个字符"）。
    pub fn char_index_at(&self, x: f32) -> Option<usize> {
        self.glyphs
            .iter()
            .position(|glyph| x >= glyph.x && x < glyph.x + glyph.advance)
    }

    /// 贪心断行：只在显式断点（空格 / 连字符之后、CJK 字之间、`\n`）断开。
    ///
    /// `max_width <= 0` 或不有限 ⇒ 不换行（整段一行）。单个字符宽于上限时独占一行
    /// （硬断），保证一定前进。
    pub fn wrap(&self, max_width: f32) -> Vec<TextLine> {
        let hard_break = !(max_width.is_finite() && max_width > 0.0);
        let mut lines = Vec::new();
        let mut start = 0usize;
        let mut width = 0.0f32;

        for (index, glyph) in self.glyphs.iter().enumerate() {
            if glyph.ch == '\n' {
                lines.push(TextLine { start, end: index });
                start = index + 1;
                width = 0.0;
                continue;
            }
            if hard_break {
                continue;
            }
            if width + glyph.advance > max_width && index > start {
                // 从上一个字符往前找最近的断点；找不到就硬断在当前字符前。
                let mut break_at = index;
                for candidate in (start + 1..index).rev() {
                    if is_break_after(self.glyphs[candidate - 1].ch) {
                        break_at = candidate;
                        break;
                    }
                }
                lines.push(TextLine {
                    start,
                    end: break_at,
                });
                start = break_at;
                width = self.glyphs[start..=index]
                    .iter()
                    .map(|glyph| glyph.advance)
                    .sum();
                continue;
            }
            width += glyph.advance;
        }

        if start < self.glyphs.len() {
            lines.push(TextLine {
                start,
                end: self.glyphs.len(),
            });
        }
        lines
    }
}

/// 该字符之后可以断行吗？
///
/// 只认三类机会（不做完整 UAX #14）：空格、连字符、CJK 字。CJK 里「字之后可断」
/// 等价于「两字之间可断」，与浏览器的 `line-break: normal` 行为一致到够用。
fn is_break_after(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '-' | '\u{00AD}')
        || is_cjk(ch)
        || matches!(
            ch,
            '，' | '。'
                | '、'
                | '；'
                | '：'
                | '！'
                | '？'
                | '）'
                | '」'
                | '』'
                | '】'
                | '》'
                | '〉'
                | '·'
                | '…'
                | '—'
        )
}

/// CJK 统一表意文字 / 假名 / 全角形式。
pub fn is_cjk(ch: char) -> bool {
    matches!(ch as u32,
        0x2E80..=0x2FDF      // CJK 部首
        | 0x3040..=0x30FF    // 平假名 / 片假名
        | 0x3400..=0x4DBF    // 扩展 A
        | 0x4E00..=0x9FFF    // 基本区
        | 0xAC00..=0xD7AF    // 谚文音节
        | 0xF900..=0xFAFF    // 兼容表意
        | 0xFF00..=0xFFEF    // 全角形式
        | 0x20000..=0x3FFFF  // 扩展 B 及以后
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手写一张固定度量表：`a` 10、`b` 20、空格 5、`中` 30。
    fn table() -> TextMetrics {
        let mut metrics = TextMetrics::empty("test", 10.0);
        metrics.ascent = 8.0;
        metrics.line_height = 12.0;
        let mut x = 0.0;
        for ch in "ab 中".chars() {
            let advance = match ch {
                'a' => 10.0,
                'b' => 20.0,
                ' ' => 5.0,
                '中' => 30.0,
                _ => 0.0,
            };
            metrics.glyphs.push(GlyphMetrics { ch, x, advance });
            x += advance;
        }
        metrics.width = x;
        metrics
    }

    #[test]
    fn char_index_is_half_open_and_bounded() {
        let metrics = table();
        assert_eq!(metrics.char_index_at(0.0), Some(0));
        assert_eq!(metrics.char_index_at(9.999), Some(0));
        assert_eq!(metrics.char_index_at(10.0), Some(1), "边界点属于后一个字符");
        assert_eq!(metrics.char_index_at(29.999), Some(1));
        assert_eq!(metrics.char_index_at(30.0), Some(2), "空格");
        assert_eq!(metrics.char_index_at(35.0), Some(3), "中");
        assert_eq!(metrics.char_index_at(-1.0), None);
        assert_eq!(metrics.char_index_at(65.0), None, "超出末尾不是最后一个");
    }

    #[test]
    fn width_is_the_sum_of_advances() {
        let metrics = table();
        let sum: f32 = metrics.glyphs.iter().map(|g| g.advance).sum();
        assert_eq!(metrics.width, sum);
        assert_eq!(metrics.width, 65.0);
    }

    #[test]
    fn wrap_breaks_at_spaces_and_cjk_between_chars() {
        let metrics = table();
        // 宽度 40：`ab ` = 35 放得下，`中`(30) 到这里会超 ⇒ 断在空格之后。
        let lines = metrics.wrap(40.0);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            TextLine { start: 0, end: 3 },
            "`ab `（空格留在前一行）"
        );
        assert_eq!(lines[1], TextLine { start: 3, end: 4 }, "`中`");
    }

    #[test]
    fn wrap_hard_breaks_a_single_wide_glyph() {
        let metrics = table();
        // 宽度 15：`a`(10) 之后 `b`(20) 放不下，且 `b` 自身宽于上限 ⇒ 独占一行。
        let lines = metrics.wrap(15.0);
        let spans: Vec<(usize, usize)> = lines.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(spans, vec![(0, 1), (1, 2), (2, 3), (3, 4)]);
    }

    #[test]
    fn non_positive_width_keeps_one_line() {
        let metrics = table();
        assert_eq!(
            metrics.wrap(0.0),
            vec![TextLine { start: 0, end: 4 }],
            "上限无效 ⇒ 不换行"
        );
        assert_eq!(metrics.wrap(f32::NAN), vec![TextLine { start: 0, end: 4 }]);
    }

    #[test]
    fn newline_forces_a_break_and_is_not_part_of_any_line() {
        let mut metrics = TextMetrics::empty("test", 10.0);
        for (index, ch) in "a\nb".chars().enumerate() {
            metrics.glyphs.push(GlyphMetrics {
                ch,
                x: index as f32 * 10.0,
                advance: if ch == '\n' { 0.0 } else { 10.0 },
            });
        }
        metrics.width = 20.0;
        let lines = metrics.wrap(1000.0);
        assert_eq!(
            lines,
            vec![TextLine { start: 0, end: 1 }, TextLine { start: 2, end: 3 }]
        );
    }

    /// **CG-b**：字符盒与字符下标互为反查（半开矩形，边界唯一）。
    #[test]
    fn text_run_maps_points_back_to_characters() {
        let mut run = ze_interact::TextRun::new(12.0);
        run.push(0, 'a', ze_interact::Point::new(0.0, 0.0), 10.0);
        run.push(1, 'b', ze_interact::Point::new(10.0, 0.0), 10.0);
        assert_eq!(run.char_at(ze_interact::Point::new(5.0, 6.0)), Some(0));
        assert_eq!(
            run.char_at(ze_interact::Point::new(10.0, 6.0)),
            Some(1),
            "边界属于后一个"
        );
        assert_eq!(run.char_at(ze_interact::Point::new(20.0, 6.0)), None);
        assert_eq!(run.char_at(ze_interact::Point::new(5.0, 12.0)), None);
        assert_eq!(run.char_box(1).map(|b| b.ch), Some('b'));
        assert_eq!(run.char_box(9), None);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn metrics_round_trip_through_json() {
        let metrics = table();
        let json = serde_json::to_string(&metrics).expect("可序列化");
        let back: TextMetrics = serde_json::from_str(&json).expect("可反序列化");
        assert_eq!(metrics, back);
    }

    /// 「度量可回读」：JSON 里没有句柄 / 运行期 id，只有数值与字符。
    #[cfg(feature = "serde")]
    #[test]
    fn metrics_json_has_no_ids() {
        let json = serde_json::to_string(&table()).unwrap();
        assert!(json.contains("\"family\":\"test\""));
        assert!(json.contains("\"ch\":\"中\""));
        assert!(!json.contains("\"id\":"), "度量表不得带运行期 id：{json}");
    }
}
