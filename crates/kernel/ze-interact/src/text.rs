//! **CG-b/c**：文本的逐字符盒（字符下标 ↔ 矩形）。
//!
//! 一份数据服务四件事：光标定位、选区、读屏（语义层）、命中反查（Agent 断言也走它）。
//! 它比字形轮廓值钱 —— 轮廓只决定"长什么样"，它决定"点在哪、念什么"。
//!
//! 坐标口径与 [`crate::hit`] / [`crate::layout`] 一致：**f64**、半开矩形。
//! 字符盒属于**布局坐标**（不是像素），由宿主决定它画在屏幕的哪里。

use crate::hit::{Point, Rect};

/// 一个字符的可命中格。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharBox {
    /// 字符在文本中的**标量下标**（不是字节下标）。
    pub index: usize,
    pub ch: char,
    pub rect: Rect,
}

/// **CI-d 口子②**：一段范围上的**不透明**标注。
///
/// 范围用字符**标量下标**（与 [`CharBox::index`] 同口径、半开区间）。
/// `payload` 不定义任何样式语义 —— 富文本 span / 诊断波浪线 / 读屏标注
/// 都只是"在这段范围上挂一份数据"，解释权在消费者。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextRunMarker {
    pub start: usize,
    pub end: usize,
    /// 不透明载荷（调用方自行编码；内核不解释）。
    #[cfg_attr(feature = "serde", serde(default))]
    pub payload: Vec<u8>,
}

/// 文本的逐字符盒。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextRun {
    /// 与文本同序的字符格（含空格；换行符不入盒）。
    pub chars: Vec<CharBox>,
    /// 行高（换行后的行距也用它）。
    pub line_height: f64,
    /// **CI-d 口子②**：范围标注（默认空集）。
    ///
    /// 空时不进序列化 ⇒ 默认路径的 `ContentIR` 字节不变（零产物风险）。
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub markers: Vec<TextRunMarker>,
}

impl TextRun {
    pub fn new(line_height: f64) -> Self {
        Self {
            chars: Vec::new(),
            line_height,
            markers: Vec::new(),
        }
    }

    /// 追加一个字符盒（`min` 是左上角，`advance` × `line_height` 是尺寸）。
    pub fn push(&mut self, index: usize, ch: char, min: Point, advance: f64) {
        self.chars.push(CharBox {
            index,
            ch,
            rect: Rect::from_min_size(min, advance, self.line_height),
        });
    }

    /// 点 → 字符下标（**半开矩形** `[min, max)`，与 `Rect::contains` 同一口径）。
    ///
    /// 相邻字符共享的边界点只属于后一个；点在所有盒之外 ⇒ `None`。
    pub fn char_at(&self, point: Point) -> Option<usize> {
        self.chars
            .iter()
            .find(|char_box| char_box.rect.contains(point))
            .map(|char_box| char_box.index)
    }

    /// 字符下标 → 盒。
    pub fn char_box(&self, index: usize) -> Option<&CharBox> {
        self.chars.iter().find(|char_box| char_box.index == index)
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// **CI-d 口子②**：追加一条范围标注（不解释 payload）。
    pub fn mark(&mut self, start: usize, end: usize, payload: Vec<u8>) {
        self.markers.push(TextRunMarker {
            start,
            end,
            payload,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> TextRun {
        let mut run = TextRun::new(12.0);
        run.push(0, 'a', Point::new(0.0, 0.0), 10.0);
        run.push(1, 'b', Point::new(10.0, 0.0), 10.0);
        run
    }

    #[test]
    fn char_at_is_half_open() {
        let run = run();
        assert_eq!(run.char_at(Point::new(5.0, 6.0)), Some(0));
        assert_eq!(run.char_at(Point::new(10.0, 6.0)), Some(1), "边界属于后一个");
        assert_eq!(run.char_at(Point::new(20.0, 6.0)), None, "末尾之外不命中");
        assert_eq!(run.char_at(Point::new(5.0, 12.0)), None, "上界之外不命中");
        assert_eq!(run.char_at(Point::new(-1.0, 6.0)), None);
    }

    #[test]
    fn char_index_and_box_are_inverse() {
        let run = run();
        assert_eq!(run.char_box(1).map(|b| b.ch), Some('b'));
        assert_eq!(run.char_box(9), None);
        let center = run.char_box(1).unwrap().rect.center();
        assert_eq!(run.char_at(center), Some(1));
    }

    #[test]
    fn empty_run_misses_everything() {
        let run = TextRun::new(10.0);
        assert!(run.is_empty());
        assert_eq!(run.char_at(Point::new(0.0, 0.0)), None);
    }

    /// **CI-d 口子②**：默认空集不进序列化 ⇒ 默认路径字节不变。
    #[cfg(feature = "serde")]
    #[test]
    fn empty_markers_are_omitted_from_json_and_payload_round_trips() {
        let mut run = run();
        let json = serde_json::to_string(&run).expect("可序列化");
        assert!(
            !json.contains("markers"),
            "空标注不得出现在默认产物里：{json}"
        );
        run.mark(0, 2, b"diag".to_vec());
        let json = serde_json::to_string(&run).expect("可序列化");
        let back: TextRun = serde_json::from_str(&json).expect("可反序列化");
        assert_eq!(run, back);
        assert_eq!(back.markers[0].payload, b"diag");
    }
}
