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

/// 文本的逐字符盒。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextRun {
    /// 与文本同序的字符格（含空格；换行符不入盒）。
    pub chars: Vec<CharBox>,
    /// 行高（换行后的行距也用它）。
    pub line_height: f64,
}

impl TextRun {
    pub fn new(line_height: f64) -> Self {
        Self {
            chars: Vec::new(),
            line_height,
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
}
