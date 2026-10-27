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

use ze_comb::{Expr, Literal, Node};

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

/// **连续变量**的规范名：宿主基础通道 `time`（空 namespace）。
///
/// 时间只是 n=1 的一个实例；其它连续变量（`frame` / 组件字段读）用
/// [`ChannelRef::qualified`] 作为键。**不硬编码"时间"的语义**，只保留这个
/// 默认变量的名字，供 `still_time` 反算用。
pub const TIME_VARIABLE: &str = "time";

/// **多维区间集合**：若干连续变量上的约束**合取**（product region）。
///
/// - `truth = false` ⇒ 空集（predicate 从不生效）；
/// - `truth = true` 且 `vars` 为空 ⇒ 全集（所有变量都不受约束）；
/// - 每个 `vars[k]` 是变量 `k` 上的一维区间集合。
///
/// `And` = 逐变量求交（不同变量合并成 product）；`Or` = 只有"同一单变量"
/// 才能表示成并集，跨变量的析取**显式不可表示**（返回 `None`，不猜）。
#[derive(Clone, Debug, PartialEq)]
pub struct MultiIntervalSet {
    truth: bool,
    vars: std::collections::BTreeMap<String, IntervalSet>,
}

impl Default for MultiIntervalSet {
    fn default() -> Self {
        Self::all()
    }
}

impl MultiIntervalSet {
    /// 全集（所有连续变量都不受约束）。
    pub fn all() -> Self {
        Self {
            truth: true,
            vars: std::collections::BTreeMap::new(),
        }
    }

    /// 空集（predicate 从不生效）。
    pub fn never() -> Self {
        Self {
            truth: false,
            vars: std::collections::BTreeMap::new(),
        }
    }

    /// 单个变量上的约束；集合为空 ⇒ 整体空集。
    pub fn on(variable: impl Into<String>, set: IntervalSet) -> Self {
        if set.is_empty() {
            return Self::never();
        }
        let mut vars = std::collections::BTreeMap::new();
        vars.insert(variable.into(), set);
        Self { truth: true, vars }
    }

    /// 是否为"恒真"（不约束任何变量）。
    pub fn is_all(&self) -> bool {
        self.truth && self.vars.is_empty()
    }

    /// 是否为空集。
    pub fn is_empty(&self) -> bool {
        !self.truth || self.vars.values().any(IntervalSet::is_empty)
    }

    /// 各变量上的约束（诊断 / 测试用）。
    pub fn vars(&self) -> impl Iterator<Item = (&str, &IntervalSet)> {
        self.vars.iter().map(|(name, set)| (name.as_str(), set))
    }

    /// 是否含**非时间**连续变量（`still_time` 必须显式说"不可推导"，不得用 0 冒充）。
    pub fn has_non_time_variable(&self) -> bool {
        self.vars.keys().any(|name| name != TIME_VARIABLE)
    }

    /// 非时间变量名（错误诊断 / 报告用）。
    pub fn non_time_variables(&self) -> Vec<&str> {
        self.vars
            .keys()
            .filter(|name| name.as_str() != TIME_VARIABLE)
            .map(String::as_str)
            .collect()
    }

    /// 变量 `variable` 是否在 `value` 处满足约束（未出现的变量视为不受约束）。
    pub fn contains_var(&self, variable: &str, value: f64) -> bool {
        if !self.truth {
            return false;
        }
        match self.vars.get(variable) {
            Some(set) => set.contains(value),
            None => true,
        }
    }

    /// 时间投影上的包含判定（保留旧 API 的语义：只问 `time`）。
    pub fn time_contains(&self, t: f64) -> bool {
        self.contains_var(TIME_VARIABLE, t)
    }

    /// 时间投影上的 `sup { x ≤ t : predicate(x) }`。
    ///
    /// 时间不受约束 ⇒ 恒生效 ⇒ `Some(t)`；空集 ⇒ `None`。
    pub fn time_last_active(&self, t: f64) -> Option<f64> {
        if !self.truth {
            return None;
        }
        match self.vars.get(TIME_VARIABLE) {
            Some(set) => set.last_active(t),
            None => Some(t),
        }
    }

    /// 时间投影上最后一个区间的右端；`None` = 延伸到 `+∞`（永远生效）。
    pub fn time_finite_max_end(&self) -> Option<f64> {
        if !self.truth {
            return Some(f64::NEG_INFINITY);
        }
        match self.vars.get(TIME_VARIABLE) {
            Some(set) => set.finite_max_end(),
            None => None,
        }
    }

    /// 交集（不同变量合并成 product region；同一变量求交）。
    pub fn intersect(&self, other: &Self) -> Self {
        if !self.truth || !other.truth {
            return Self::never();
        }
        let mut vars = self.vars.clone();
        for (name, set) in &other.vars {
            match vars.get(name) {
                Some(existing) => {
                    let merged = existing.intersect(set);
                    if merged.is_empty() {
                        return Self::never();
                    }
                    vars.insert(name.clone(), merged);
                }
                None => {
                    vars.insert(name.clone(), set.clone());
                }
            }
        }
        Self { truth: true, vars }
    }

    /// 并集：只有"同一单变量"可表示；跨变量析取返回 `None`（显式不可分析）。
    pub fn union(&self, other: &Self) -> Option<Self> {
        if self.is_all() || other.is_all() {
            return Some(Self::all());
        }
        if !self.truth {
            return Some(other.clone());
        }
        if !other.truth {
            return Some(self.clone());
        }
        if self.vars.len() == 1 && other.vars.len() == 1 {
            let (left_name, left_set) = self.vars.iter().next().expect("len == 1");
            let (right_name, right_set) = other.vars.iter().next().expect("len == 1");
            if left_name == right_name {
                return Some(Self::on(
                    left_name.clone(),
                    left_set.union(right_set),
                ));
            }
        }
        None
    }

    /// 补集：只有"不约束任何变量"或"单变量"可表示；多变量 product 的补集
    /// 是跨变量析取 ⇒ 显式不可分析。
    pub fn complement(&self) -> Option<Self> {
        if !self.truth {
            return Some(Self::all());
        }
        match self.vars.len() {
            0 => Some(Self::never()),
            1 => {
                let (name, set) = self.vars.iter().next().expect("len == 1");
                Some(Self::on(name.clone(), set.complement()))
            }
            _ => None,
        }
    }
}

/// **predicate → 区间集合**（局部时间）。
///
/// 只接受「连续变量与数值字面量比较」以及它们的 `And` / `Or` / `Not`；
/// 其它形状（事件 / 非字面量边界）返回 `None` —— 显式不可分析，不猜。
///
/// **阶段 2**：`time` 不再是唯一变量 —— 任意 `Channel` / `Read` 都是连续变量，
/// 键分别是 `channel.qualified()` / `channel.qualified()#object`。
pub fn analyze(expr: &Expr) -> Option<MultiIntervalSet> {
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
            let (variable, value, _) = variable_and_number(args)?;
            Some(MultiIntervalSet::on(variable, IntervalSet::point(value)))
        }
        "ne" => {
            let (variable, value, _) = variable_and_number(args)?;
            Some(MultiIntervalSet::on(
                variable,
                IntervalSet::point(value).complement(),
            ))
        }
        "and" if args.len() == 2 => Some(analyze(&args[0])?.intersect(&analyze(&args[1])?)),
        "or" if args.len() == 2 => analyze(&args[0])?.union(&analyze(&args[1])?),
        "not" if args.len() == 1 => analyze(&args[0])?.complement(),
        _ => None,
    }
}

/// `lt` / `le` / `gt` / `ge`：连续变量与字面量比较（两侧顺序都接受）。
fn compare(args: &[Expr], inclusive: bool, greater: bool) -> Option<MultiIntervalSet> {
    let (variable, value, variable_is_left) = variable_and_number(args)?;
    let set = match (greater, variable_is_left) {
        // `x > d` / `x ≥ d`
        (true, true) => IntervalSet::above(value, inclusive),
        // `d > x` ≡ `x < d`
        (true, false) => IntervalSet::below(value, inclusive),
        // `x < d` / `x ≤ d`
        (false, true) => IntervalSet::below(value, inclusive),
        // `d < x` ≡ `x > d`
        (false, false) => IntervalSet::above(value, inclusive),
    };
    Some(MultiIntervalSet::on(variable, set))
}

/// `[a, b]` 里哪一侧是连续变量、另一侧的字面量是多少。
fn variable_and_number(args: &[Expr]) -> Option<(String, f64, bool)> {
    if args.len() != 2 {
        return None;
    }
    if let Some(variable) = continuous_variable(&args[0]) {
        return number(&args[1]).map(|value| (variable, value, true));
    }
    if let Some(variable) = continuous_variable(&args[1]) {
        return number(&args[0]).map(|value| (variable, value, false));
    }
    None
}

/// **阶段 0**：区间代数登记为装配期分析 pass（只读 IR、写 side table）。
pub fn register_passes(registry: &mut ze_comb::PassRegistry) {
    registry
        .register(ze_comb::PassInfo {
            name: "scene.predicate.analyze",
            kind: ze_comb::PassKind::Analysis,
            semantics: ze_comb::Semantics::Preserving,
            owner: "ze-scene-core",
        })
        .expect("predicate pass 名唯一");
}

/// 连续变量的身份：`Channel` / `Read` 都是"随帧采样的连续量"。
fn continuous_variable(expr: &Expr) -> Option<String> {
    match &expr.node {
        Node::Channel { channel } => Some(channel.qualified()),
        Node::Read { channel, object } => Some(format!("{}#{object}", channel.qualified())),
        _ => None,
    }
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
    use ze_comb::{ChannelRef, OpRef};

    fn op(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    fn time() -> Expr {
        Expr::channel(ChannelRef::builtin("time"))
    }

    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::call(op(name), args)
    }

    fn window(start: f64, end: f64) -> MultiIntervalSet {
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
        assert!(!set.time_contains(0.5));
        assert!(set.time_contains(2.0));
        assert!(!set.time_contains(4.0), "右端开");
        assert_eq!(set.time_finite_max_end(), Some(4.0));
    }

    #[test]
    fn last_active_takes_the_right_endpoint() {
        let set = window(1.0, 4.0);
        assert_eq!(set.time_last_active(0.5), None, "还没生效过");
        assert_eq!(set.time_last_active(2.0), Some(2.0), "区间内就是 t");
        assert_eq!(set.time_last_active(9.0), Some(4.0), "空隙取前一段右端（U3）");
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
        assert!(!outside.time_contains(2.0));
        assert!(outside.time_contains(0.0));
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
        assert!(outside.time_contains(1.0) && outside.time_contains(4.0));
        assert!(!before_or_after.time_contains(1.0) && !before_or_after.time_contains(4.0));
        assert!(before_or_after.time_contains(0.0) && before_or_after.time_contains(5.0));
    }

    #[test]
    fn equality_is_a_point_and_inequality_its_complement() {
        let at_two = analyze(&call(
            "eq",
            vec![time(), Expr::pure(2.0)],
        ))
        .expect("Eq 可分析");
        assert!(at_two.time_contains(2.0));
        assert!(!at_two.time_contains(2.0001));

        let not_two = analyze(&call(
            "ne",
            vec![time(), Expr::pure(2.0)],
        ))
        .expect("Ne 可分析");
        assert!(!not_two.time_contains(2.0));
        assert!(not_two.time_contains(1.0));
    }

    #[test]
    fn flipped_operands_are_understood() {
        // `2 < time` ≡ `time > 2`。
        let set = analyze(&call("lt", vec![Expr::pure(2.0), time()])).expect("可分析");
        assert!(set.time_contains(3.0));
        assert!(!set.time_contains(1.0));
    }

    #[test]
    fn unknown_shapes_are_not_guessed() {
        // 右侧不是字面量 ⇒ 不可分析。
        assert!(analyze(&call("lt", vec![time(), time()])).is_none());
        // 含恒真（无 predicate）不是表达式，另行处理。
        assert!(analyze(&Expr::pure(true)).is_none());
    }

    fn variable(name: &str) -> Expr {
        Expr::channel(ChannelRef::builtin(name))
    }

    /// **阶段 2 验收**：`And{Lt{x,4}, Gt{x,1}}` 可求交（同一个连续变量）。
    #[test]
    fn and_on_the_same_variable_intersects() {
        let set = analyze(&call(
            "and",
            vec![
                call("gt", vec![variable("x"), Expr::pure(1.0)]),
                call("lt", vec![variable("x"), Expr::pure(4.0)]),
            ],
        ))
        .expect("同一变量的 And 可分析");
        assert!(set.contains_var("x", 2.0));
        assert!(!set.contains_var("x", 0.5));
        assert!(!set.contains_var("x", 4.0), "右端开");
        // 时间没有出现在约束里 ⇒ 时间投影恒真（不是 0，也不是猜）。
        assert!(set.time_contains(0.0) && set.time_contains(1e9));
        assert!(set.has_non_time_variable());
    }

    /// 不同变量的 `And` 是 **product region**（两边的约束同时成立）。
    #[test]
    fn and_on_different_variables_is_a_product() {
        let set = analyze(&call(
            "and",
            vec![
                call("lt", vec![variable("x"), Expr::pure(4.0)]),
                call("gt", vec![variable("y"), Expr::pure(1.0)]),
            ],
        ))
        .expect("不同变量的 And 可分析");
        assert!(set.contains_var("x", 3.0) && set.contains_var("y", 2.0));
        assert!(!set.contains_var("x", 5.0));
        assert!(!set.contains_var("y", 0.0));
        assert_eq!(set.non_time_variables(), vec!["x", "y"]);
    }

    /// 跨变量析取 / 多变量补集**显式不可分析**（不静默猜成某种形状）。
    #[test]
    fn cross_variable_or_and_not_are_not_guessed() {
        let or = call(
            "or",
            vec![
                call("lt", vec![variable("x"), Expr::pure(1.0)]),
                call("gt", vec![variable("y"), Expr::pure(2.0)]),
            ],
        );
        assert!(analyze(&or).is_none(), "跨变量 Or 不可表示");

        let product = call(
            "and",
            vec![
                call("lt", vec![variable("x"), Expr::pure(4.0)]),
                call("gt", vec![variable("y"), Expr::pure(1.0)]),
            ],
        );
        assert!(
            analyze(&call("not", vec![product])).is_none(),
            "product 的补集是跨变量析取 ⇒ 不可表示"
        );
    }
}
