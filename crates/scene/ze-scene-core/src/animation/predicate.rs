//! predicate → **区间集合**（AH-06）+ `last_active` 反算（AH-07）。
//!
//! # 为什么是区间，不是单个窗口
//!
//! 旧口径只认 `Lt { time, d }`；作者写 `And { Lt { t, 4 }, Gt { t, 1 } }` 立刻
//! 「不可分析」。把比较算子映射到半无限区间后，`And` = 交、`Or` = 并、`Not` = 补，
//! 常见的时间门都落在封闭的小代数里。
//!
//! # 为什么能消灭 Hold 的历史依赖
//!
//! 若 predicate 只依赖（绑定时钟的）局部时间，那么「最后一次生效的时刻」是
//! `last_active(t) = sup { x ≤ t : predicate(x) }` —— 只由区间集合决定，**零状态**。
//! 于是 `reset = Hold` 变成纯函数 `f(last_active(t))`：seek 到任意一帧都能直接算，
//! 不再依赖"上一帧写过什么"，也与绑定应用顺序解耦。
//!
//! 不可分析（event / 组件状态 / 非字面量边界）⇒ [`analyze`] 返回 `None`，
//! 运行期回退到原来的逐帧 predicate 求值（并保守地保留 `SeekKind::Replay`）。

use ze_func::{Expr, Literal, Node};

/// 区间端点：值 + 是否闭合。
///
/// 无穷端点永远开（`±∞` 不在实数域里）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bound {
    pub value: f64,
    pub closed: bool,
}

impl Bound {
    fn new(value: f64, closed: bool) -> Self {
        Self {
            value,
            closed: closed && value.is_finite(),
        }
    }
}

/// 一个区间：`[start, end]` / `(start, end)` 等（端点可开可闭）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub start: Bound,
    pub end: Bound,
}

impl Interval {
    fn new(start: Bound, end: Bound) -> Option<Self> {
        let valid = start.value < end.value
            || (start.value == end.value && start.closed && end.closed);
        valid.then_some(Self { start, end })
    }

    /// 是否包含 `t`。
    pub fn contains(&self, t: f64) -> bool {
        let after_start = t > self.start.value || (t == self.start.value && self.start.closed);
        let before_end = t < self.end.value || (t == self.end.value && self.end.closed);
        after_start && before_end
    }
}

/// 规范化的区间集合：**有序、互不相交、已合并**。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IntervalSet {
    intervals: Vec<Interval>,
}

impl IntervalSet {
    /// 空集（predicate 从不生效）。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 全集 `(-∞, ∞)`。
    pub fn all() -> Self {
        Self {
            intervals: vec![Interval {
                start: Bound::new(f64::NEG_INFINITY, false),
                end: Bound::new(f64::INFINITY, false),
            }],
        }
    }

    /// 单点 `[value, value]`。
    pub fn point(value: f64) -> Self {
        Self::new(vec![Interval {
            start: Bound::new(value, true),
            end: Bound::new(value, true),
        }])
    }

    /// `(-∞, value)` / `(-∞, value]`。
    pub fn below(value: f64, closed: bool) -> Self {
        Self::new(vec![Interval {
            start: Bound::new(f64::NEG_INFINITY, false),
            end: Bound::new(value, closed),
        }])
    }

    /// `(value, ∞)` / `[value, ∞)`。
    pub fn above(value: f64, closed: bool) -> Self {
        Self::new(vec![Interval {
            start: Bound::new(value, closed),
            end: Bound::new(f64::INFINITY, false),
        }])
    }

    /// 从任意区间构造并**规范化**（排序 + 合并）。
    pub fn new(mut intervals: Vec<Interval>) -> Self {
        intervals.retain(|interval| {
            interval.start.value < interval.end.value
                || (interval.start.value == interval.end.value
                    && interval.start.closed
                    && interval.end.closed)
        });
        intervals.sort_by(|a, b| {
            a.start
                .value
                .partial_cmp(&b.start.value)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.start.closed.cmp(&a.start.closed))
        });
        let mut merged: Vec<Interval> = Vec::with_capacity(intervals.len());
        for interval in intervals {
            match merged.last_mut() {
                Some(last)
                    if interval.start.value < last.end.value
                        || (interval.start.value == last.end.value
                            && (last.end.closed || interval.start.closed)) =>
                {
                    if interval.end.value > last.end.value {
                        last.end = interval.end;
                    } else if interval.end.value == last.end.value {
                        last.end.closed |= interval.end.closed;
                    }
                }
                _ => merged.push(interval),
            }
        }
        Self { intervals: merged }
    }

    /// 是否为空集。
    pub fn is_empty(&self) -> bool {
        self.intervals.is_empty()
    }

    /// 是否包含 `t`。
    pub fn contains(&self, t: f64) -> bool {
        self.intervals.iter().any(|interval| interval.contains(t))
    }

    /// 交集。
    pub fn intersect(&self, other: &Self) -> Self {
        let mut out = Vec::new();
        let (mut a, mut b) = (0usize, 0usize);
        while a < self.intervals.len() && b < other.intervals.len() {
            let left = self.intervals[a];
            let right = other.intervals[b];
            let start = if left.start.value > right.start.value {
                left.start
            } else if right.start.value > left.start.value {
                right.start
            } else {
                Bound::new(left.start.value, left.start.closed && right.start.closed)
            };
            let end = if left.end.value < right.end.value {
                left.end
            } else if right.end.value < left.end.value {
                right.end
            } else {
                Bound::new(left.end.value, left.end.closed && right.end.closed)
            };
            if let Some(interval) = Interval::new(start, end) {
                out.push(interval);
            }
            // 推进右端更小的那一个。
            if left.end.value < right.end.value {
                a += 1;
            } else if right.end.value < left.end.value {
                b += 1;
            } else {
                a += 1;
                b += 1;
            }
        }
        Self::new(out)
    }

    /// 并集。
    pub fn union(&self, other: &Self) -> Self {
        let mut intervals = self.intervals.clone();
        intervals.extend(other.intervals.iter().copied());
        Self::new(intervals)
    }

    /// 补集（相对 `(-∞, ∞)`）。
    pub fn complement(&self) -> Self {
        let mut out = Vec::new();
        let mut cursor = Bound::new(f64::NEG_INFINITY, false);
        for interval in &self.intervals {
            if let Some(gap) = Interval::new(
                cursor,
                Bound::new(interval.start.value, !interval.start.closed),
            ) {
                out.push(gap);
            }
            cursor = Bound::new(interval.end.value, !interval.end.closed);
        }
        if let Some(gap) = Interval::new(cursor, Bound::new(f64::INFINITY, false)) {
            out.push(gap);
        }
        Self::new(out)
    }

    /// `sup { x ≤ t : x ∈ set }`。
    ///
    /// - `t` 在区间内 ⇒ `t`；
    /// - `t` 落在两段之间的空隙 ⇒ 前一段的**右端点**（U3：作者写 `t < 2`
    ///   的意思是"到 2 为止"，取右端点而不是左极限）；
    /// - `t` 在第一段之前 ⇒ `None`（还没生效过）。
    pub fn last_active(&self, t: f64) -> Option<f64> {
        let mut best = None;
        for interval in &self.intervals {
            if t < interval.start.value {
                break;
            }
            if t == interval.start.value {
                if interval.start.closed {
                    best = Some(t);
                }
                break;
            }
            if t <= interval.end.value {
                best = Some(t);
                break;
            }
            best = Some(interval.end.value);
        }
        best
    }

    /// 最后一个区间的右端；`None` = 延伸到 `+∞`（predicate 永远生效）。
    ///
    /// 空集返回 `Some(-∞)`（从不生效 ⇒ 对"静止时刻"没有贡献）。
    pub fn finite_max_end(&self) -> Option<f64> {
        match self.intervals.last() {
            None => Some(f64::NEG_INFINITY),
            Some(interval) if interval.end.value.is_finite() => Some(interval.end.value),
            Some(_) => None,
        }
    }
}

/// **predicate → 区间集合**（局部时间）。
///
/// 只接受「`time` 与数值字面量比较」以及它们的 `And` / `Or` / `Not`；
/// 其它形状（事件 / 组件状态 / 非字面量边界）返回 `None` —— 显式不可分析，
/// 不猜。
pub fn analyze(expr: &Expr) -> Option<IntervalSet> {
    let Node::Call { op, args } = &expr.node else {
        return None;
    };
    if op.namespace != "zela.math" {
        return None;
    }
    match op.name.as_str() {
        "lt" => compare(args, false, false),
        "le" => compare(args, true, false),
        "gt" => compare(args, false, true),
        "ge" => compare(args, true, true),
        "eq" => {
            let (_, value) = time_and_number(args)?;
            Some(IntervalSet::point(value))
        }
        "ne" => {
            let (_, value) = time_and_number(args)?;
            Some(IntervalSet::point(value).complement())
        }
        "and" if args.len() == 2 => Some(analyze(&args[0])?.intersect(&analyze(&args[1])?)),
        "or" if args.len() == 2 => Some(analyze(&args[0])?.union(&analyze(&args[1])?)),
        "not" if args.len() == 1 => Some(analyze(&args[0])?.complement()),
        _ => None,
    }
}

/// `lt` / `le` / `gt` / `ge`：`time` 与字面量比较（两侧顺序都接受）。
fn compare(args: &[Expr], inclusive: bool, greater: bool) -> Option<IntervalSet> {
    let (time_is_left, value) = time_and_number(args)?;
    Some(match (greater, time_is_left) {
        // `time > d` / `time ≥ d`
        (true, true) => IntervalSet::above(value, inclusive),
        // `d > time` ≡ `time < d`
        (true, false) => IntervalSet::below(value, inclusive),
        // `time < d` / `time ≤ d`
        (false, true) => IntervalSet::below(value, inclusive),
        // `d < time` ≡ `time > d`
        (false, false) => IntervalSet::above(value, inclusive),
    })
}

/// `[a, b]` 里哪一侧是时间通道、另一侧的字面量是多少。
fn time_and_number(args: &[Expr]) -> Option<(bool, f64)> {
    if args.len() != 2 {
        return None;
    }
    if is_time(&args[0]) {
        return number(&args[1]).map(|value| (true, value));
    }
    if is_time(&args[1]) {
        return number(&args[0]).map(|value| (false, value));
    }
    None
}

fn is_time(expr: &Expr) -> bool {
    matches!(
        &expr.node,
        Node::Channel { channel } if channel.is_builtin() && channel.name == "time"
    )
}

fn number(expr: &Expr) -> Option<f64> {
    match &expr.node {
        Node::Pure {
            value: Literal::F64(value),
        } => Some(*value),
        Node::Pure {
            value: Literal::F32(value),
        } => Some(f64::from(*value)),
        Node::Pure {
            value: Literal::I32(value),
        } => Some(f64::from(*value)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_func::{ChannelRef, OpRef};

    fn op(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    fn time() -> Expr {
        Expr::channel(ChannelRef::builtin("time"))
    }

    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::call(op(name), args)
    }

    fn window(start: f64, end: f64) -> IntervalSet {
        analyze(&call(
            "and",
            vec![
                call("gt", vec![time(), Expr::pure(start)]),
                call("lt", vec![time(), Expr::pure(end)]),
            ],
        ))
        .expect("And{Lt,Gt} 可分析")
    }

    #[test]
    fn and_of_lt_and_gt_is_a_finite_window() {
        let set = window(1.0, 4.0);
        assert!(!set.contains(0.5));
        assert!(set.contains(2.0));
        assert!(!set.contains(4.0), "右端开");
        assert_eq!(set.finite_max_end(), Some(4.0));
    }

    #[test]
    fn last_active_takes_the_right_endpoint() {
        let set = window(1.0, 4.0);
        assert_eq!(set.last_active(0.5), None, "还没生效过");
        assert_eq!(set.last_active(2.0), Some(2.0), "区间内就是 t");
        assert_eq!(set.last_active(9.0), Some(4.0), "空隙取前一段右端（U3）");
    }

    #[test]
    fn not_complements_and_or_unions() {
        let inside = window(1.0, 4.0);
        let outside = analyze(&call("not", vec![call(
            "and",
            vec![
                call("gt", vec![time(), Expr::pure(1.0)]),
                call("lt", vec![time(), Expr::pure(4.0)]),
            ],
        )]))
        .expect("Not 可分析");
        assert!(!outside.contains(2.0));
        assert!(outside.contains(0.0));
        assert!(outside.intersect(&inside).is_empty());

        let before_or_after = analyze(&call(
            "or",
            vec![
                call("lt", vec![time(), Expr::pure(1.0)]),
                call("gt", vec![time(), Expr::pure(4.0)]),
            ],
        ))
        .expect("Or 可分析");
        // `Not (1, 4)` 含端点（区间本身是开的）；`Lt(1) ∪ Gt(4)` 不含端点。
        assert!(outside.contains(1.0) && outside.contains(4.0));
        assert!(!before_or_after.contains(1.0) && !before_or_after.contains(4.0));
        assert!(before_or_after.contains(0.0) && before_or_after.contains(5.0));
    }

    #[test]
    fn equality_is_a_point_and_inequality_its_complement() {
        let at_two = analyze(&call(
            "eq",
            vec![time(), Expr::pure(2.0)],
        ))
        .expect("Eq 可分析");
        assert!(at_two.contains(2.0));
        assert!(!at_two.contains(2.0001));

        let not_two = analyze(&call(
            "ne",
            vec![time(), Expr::pure(2.0)],
        ))
        .expect("Ne 可分析");
        assert!(!not_two.contains(2.0));
        assert!(not_two.contains(1.0));
    }

    #[test]
    fn flipped_operands_are_understood() {
        // `2 < time` ≡ `time > 2`。
        let set = analyze(&call("lt", vec![Expr::pure(2.0), time()])).expect("可分析");
        assert!(set.contains(3.0));
        assert!(!set.contains(1.0));
    }

    #[test]
    fn unknown_shapes_are_not_guessed() {
        // 右侧不是字面量 ⇒ 不可分析。
        assert!(analyze(&call("lt", vec![time(), time()])).is_none());
        // 含恒真（无 predicate）不是表达式，另行处理。
        assert!(analyze(&Expr::pure(true)).is_none());
    }
}
