//! **CL-R**：布局策略落地 —— 两遍扫描是默认策略，约束求解是**同一条链上的第二个策略**。
//!
//! # 为什么现在引求解器
//!
//! 之前的说法是「参数面板是垂直列表，用不上约束，等思维导图那批再引」。这句话把求解器
//! 误当成了「编辑器里连约束」的功能。真实的消费者一直就在：`ui_layout` 的
//! 「声明树 → 编译 → 放置」链需要一个 `StrategyId` 的实现者。两遍扫描是默认，
//! `StrategyId("kasuari")` 是同一接口下的第二个实现 —— 不是新系统。
//!
//! # 三类落物
//!
//! ```text
//! 声明   Constraint { expr, op, rhs, strength } + ConstraintDecl（BTreeSet 稳定序）
//! 策略   BTreeMap<StrategyId, SolverFactory>：two-pass（默认）/ kasuari（feature 后）
//! 缓存   LayoutEngine：revision 不变 ⇒ 不重解（60 帧只求解 1 次）
//! ```
//!
//! # 确定性三条（写死，别在别处再写一遍）
//!
//! 1. **稳定序**：约束进 `BTreeSet`，比较键是「按 `LayoutVar` 排序后的项 + 算子 + rhs 位模式
//!    + 强度」⇒ 不同插入顺序逐位相同；
//! 2. **唯一解才算解**：`kasuari` 后端先做 Required 等式的秩检查，秩 < 变量数 ⇒
//!    `SolveErrorKind::Ambiguous`，**不拿求解器的初值 0 当答案**；
//! 3. **显式失败**：不支持的声明 / 不可满足 / 欠定 / 越界 / 超迭代上限都返回 `SolveError`
//!    （带冲突项路径），**不静默降级回两遍扫描**。
//!
//! # 不进产物
//!
//! 求解器实例、变量表、求解结果缓存都是派生态。进产物的只有**声明**
//! （`WidgetDecl.strategy` / `WidgetDecl.constraints`）。
//!
//! # 零依赖
//!
//! 本模块（约束形状 + 策略表 + 两遍扫描）只用 `std`；`kasuari` 只在
//! `layout-kasuari` feature 后编译。`ze-interact` 默认构建仍是零依赖。

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::layout::{Constraints, LayoutId, PlacedRect};

/// 布局策略 id（默认 = `two-pass`）。
///
/// 与 `WidgetSpecId` 同构：独立命名空间，不与 `ContentOriginKind` / `OpRef` 共用。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StrategyId(pub String);

impl StrategyId {
    /// 默认策略：回溯测量 + 前推分配（垂直列表的本命）。
    pub const TWO_PASS: &'static str = "two-pass";
    /// 约束求解策略（`layout-kasuari` feature）。
    pub const KASUARI: &'static str = "kasuari";

    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl Default for StrategyId {
    fn default() -> Self {
        Self(Self::TWO_PASS.to_string())
    }
}

impl fmt::Display for StrategyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// **布局 token**：会改变 measure 结果（因此必须进 revision）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayoutTokens {
    pub row_height: f64,
    pub gap: f64,
    pub padding: f64,
    pub font_size: f64,
    pub knob_radius: f64,
}

impl LayoutTokens {
    /// 布局 token 的声明哈希（FNV-1a，`f64` 走 `to_bits()`）。
    pub fn revision(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for value in [
            self.row_height,
            self.gap,
            self.padding,
            self.font_size,
            self.knob_radius,
        ] {
            for byte in value.to_bits().to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        hash
    }
}

// ─── 约束形状（声明期纯值，可序列化）──────────────────────────────────────────

/// 布局变量：**内容寻址**（`LayoutId` + 四边），绝不 `next_id()`。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LayoutVar {
    X(LayoutId),
    Y(LayoutId),
    Width(LayoutId),
    Height(LayoutId),
}

impl LayoutVar {
    /// 该变量属于哪个控件。
    pub fn widget(self) -> LayoutId {
        match self {
            Self::X(id) | Self::Y(id) | Self::Width(id) | Self::Height(id) => id,
        }
    }
}

impl fmt::Display for LayoutVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (axis, id) = match self {
            Self::X(id) => ("x", id.0),
            Self::Y(id) => ("y", id.0),
            Self::Width(id) => ("w", id.0),
            Self::Height(id) => ("h", id.0),
        };
        write!(f, "{axis}({id})")
    }
}

/// 线性项：`coef * var`。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy)]
pub struct LinearTerm {
    pub var: LayoutVar,
    pub coef: f64,
}

impl LinearTerm {
    pub const fn new(var: LayoutVar, coef: f64) -> Self {
        Self { var, coef }
    }
}

impl PartialEq for LinearTerm {
    fn eq(&self, other: &Self) -> bool {
        self.var == other.var && self.coef.to_bits() == other.coef.to_bits()
    }
}

impl Eq for LinearTerm {}

impl PartialOrd for LinearTerm {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LinearTerm {
    fn cmp(&self, other: &Self) -> Ordering {
        self.var
            .cmp(&other.var)
            .then_with(|| self.coef.to_bits().cmp(&other.coef.to_bits()))
    }
}

/// 关系算子。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConstraintOp {
    Le,
    Eq,
    Ge,
}

impl ConstraintOp {
    fn as_str(self) -> &'static str {
        match self {
            Self::Le => "<=",
            Self::Eq => "==",
            Self::Ge => ">=",
        }
    }

    /// Required 约束的可行性验证（只有求解后端用得到）。
    #[cfg(feature = "layout-kasuari")]
    fn holds(self, lhs: f64, rhs: f64) -> bool {
        const EPS: f64 = 1e-9;
        match self {
            Self::Le => lhs <= rhs + EPS,
            Self::Eq => (lhs - rhs).abs() <= EPS,
            Self::Ge => lhs >= rhs - EPS,
        }
    }
}

/// 约束强度（`Required` 不可违反；其余由求解器按强弱让步）。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConstraintStrength {
    Required,
    Strong,
    Medium,
    Weak,
}

/// 一条线性约束：`expr op rhs`。
///
/// `expr` 是 `Σ coef·var`，常量放在 `rhs` —— 计划 §3.1 只写了 `{ expr, op, strength }`，
/// 但「`A.width == B.width + 2*padding`」的 `2*padding` 必须有地方放，所以补 `rhs`
/// （冲突点已记入交付说明）。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Constraint {
    pub expr: Vec<LinearTerm>,
    pub op: ConstraintOp,
    pub rhs: f64,
    pub strength: ConstraintStrength,
}

impl Constraint {
    /// 默认强度 `Required`（布局关系通常不允许让步）。
    pub fn new(expr: Vec<LinearTerm>, op: ConstraintOp, rhs: f64) -> Self {
        Self {
            expr,
            op,
            rhs,
            strength: ConstraintStrength::Required,
        }
    }

    /// 单变量约束：`var op value`。
    pub fn var(var: LayoutVar, op: ConstraintOp, value: f64) -> Self {
        Self::new(vec![LinearTerm::new(var, 1.0)], op, value)
    }

    pub fn with_strength(mut self, strength: ConstraintStrength) -> Self {
        self.strength = strength;
        self
    }

    /// 规范化项序（按 `LayoutVar` 排序）：语义上 `expr` 是求和，顺序不该参与比较。
    pub fn canonical_expr(&self) -> Vec<LinearTerm> {
        let mut terms = self.expr.clone();
        terms.sort();
        terms
    }

    /// 该约束涉及的控件（跨控件 = 两遍扫描解不开的那类）。
    pub fn widgets(&self) -> BTreeSet<LayoutId> {
        self.expr.iter().map(|term| term.var.widget()).collect()
    }
}

/// 规范化后的 rhs 位模式（`-0.0` 与 `0.0` 视为同一条）。
fn rhs_bits(value: f64) -> u64 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

impl PartialEq for Constraint {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Constraint {}

impl PartialOrd for Constraint {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Constraint {
    fn cmp(&self, other: &Self) -> Ordering {
        self.canonical_expr()
            .cmp(&other.canonical_expr())
            .then_with(|| self.op.cmp(&other.op))
            .then_with(|| rhs_bits(self.rhs).cmp(&rhs_bits(other.rhs)))
            .then_with(|| self.strength.cmp(&other.strength))
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for term in self.canonical_expr() {
            if !first {
                write!(f, " + ")?;
            }
            write!(f, "{}*{}", term.coef, term.var)?;
            first = false;
        }
        if first {
            write!(f, "0")?;
        }
        write!(f, " {} {}", self.op.as_str(), self.rhs)
    }
}

/// **约束集**：`BTreeSet` 稳定序 ⇒ 插入顺序不参与。
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConstraintDecl {
    items: BTreeSet<Constraint>,
}

impl ConstraintDecl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, constraint: Constraint) {
        self.items.insert(constraint);
    }

    pub fn extend(&mut self, other: &Self) {
        self.items.extend(other.items.iter().cloned());
    }

    pub fn iter(&self) -> impl Iterator<Item = &Constraint> + '_ {
        self.items.iter()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 声明涉及的控件（按 id 升序）。
    pub fn widgets(&self) -> BTreeSet<LayoutId> {
        self.items
            .iter()
            .flat_map(|constraint| constraint.widgets())
            .collect()
    }

    /// 是否存在「一条约束牵涉两个以上控件」——两遍扫描明确解不了的那类。
    pub fn has_cross_widget_constraint(&self) -> bool {
        self.items
            .iter()
            .any(|constraint| constraint.widgets().len() > 1)
    }

    /// 声明哈希（FNV-1a；顺序由 `BTreeSet` 决定）。
    pub fn revision(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |byte: u8| {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        };
        for constraint in &self.items {
            for term in constraint.canonical_expr() {
                for byte in (term.var.widget().0).to_le_bytes() {
                    eat(byte);
                }
                eat(match term.var {
                    LayoutVar::X(_) => 0,
                    LayoutVar::Y(_) => 1,
                    LayoutVar::Width(_) => 2,
                    LayoutVar::Height(_) => 3,
                });
                for byte in term.coef.to_bits().to_le_bytes() {
                    eat(byte);
                }
            }
            eat(match constraint.op {
                ConstraintOp::Le => 0,
                ConstraintOp::Eq => 1,
                ConstraintOp::Ge => 2,
            });
            for byte in rhs_bits(constraint.rhs).to_le_bytes() {
                eat(byte);
            }
            eat(match constraint.strength {
                ConstraintStrength::Required => 0,
                ConstraintStrength::Strong => 1,
                ConstraintStrength::Medium => 2,
                ConstraintStrength::Weak => 3,
            });
        }
        hash
    }
}

// ─── 失败与策略 ───────────────────────────────────────────────────────────────

/// 求解失败的**显式**原因（不静默降级）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveErrorKind {
    /// 未注册的策略 id。
    UnknownStrategy,
    /// 该策略明确解不了这份声明（例如两遍扫描遇到跨控件约束）。
    Unsupported,
    /// Required 约束互相冲突。
    Unsatisfiable,
    /// 变量存在但控件缺几何（`X/Y/Width/Height` 没被声明决定）。
    Unplaced(LayoutId),
    /// Required 等式秩不足：解不唯一。
    Ambiguous,
    /// 求解结果超出可用空间。
    Overflow(LayoutId),
    /// 迭代上限（可观测；当前实现是验证轮次上限）。
    IterationLimit,
}

/// 求解失败：策略 id + 原因 + 冲突项路径（稳定序，便于回执比对）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolveError {
    pub strategy: StrategyId,
    pub kind: SolveErrorKind,
    pub conflicts: Vec<String>,
}

impl SolveError {
    pub fn new(
        strategy: &StrategyId,
        kind: SolveErrorKind,
        conflicts: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut conflicts: Vec<String> = conflicts.into_iter().collect();
        conflicts.sort();
        conflicts.dedup();
        Self {
            strategy: strategy.clone(),
            kind,
            conflicts,
        }
    }
}

impl fmt::Display for SolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "布局策略 `{}` ", self.strategy)?;
        match &self.kind {
            SolveErrorKind::UnknownStrategy => write!(f, "未注册")?,
            SolveErrorKind::Unsupported => write!(f, "不支持该约束声明")?,
            SolveErrorKind::Unsatisfiable => write!(f, "Required 约束冲突")?,
            SolveErrorKind::Unplaced(id) => write!(f, "控件 {} 缺几何声明", id.0)?,
            SolveErrorKind::Ambiguous => write!(f, "解不唯一（Required 等式秩不足）")?,
            SolveErrorKind::Overflow(id) => write!(f, "控件 {} 超出可用空间", id.0)?,
            SolveErrorKind::IterationLimit => write!(f, "超过迭代上限")?,
        }
        if !self.conflicts.is_empty() {
            write!(f, "；冲突项：{}", self.conflicts.join(" | "))?;
        }
        Ok(())
    }
}

impl std::error::Error for SolveError {}

/// 布局策略的实现（object-safe；**不持有 World、不写回 ECS**）。
pub trait LayoutSolver: Send + Sync + 'static {
    fn strategy(&self) -> StrategyId;

    /// 按声明给出落点。`available` 只作上界校验（超出 ⇒ 显式错误，不悄悄裁剪）。
    fn solve(
        &self,
        declaration: &ConstraintDecl,
        available: Constraints,
    ) -> Result<Vec<PlacedRect>, SolveError>;
}

/// 策略工厂（与 `WidgetSpecId → WidgetFactory` 同构）。
pub type SolverFactory = fn(&ConstraintDecl) -> Box<dyn LayoutSolver>;

/// **默认策略**：两遍扫描。
///
/// 它不读约束声明：声明非空 ⇒ 显式 `Unsupported`（静默忽略约束才是真正的违约）。
/// 空声明 ⇒ `Ok([])`，真正的落点由 `ui_layout` 的回溯/前推两遍扫描给出。
pub struct TwoPassSolver;

impl LayoutSolver for TwoPassSolver {
    fn strategy(&self) -> StrategyId {
        StrategyId::default()
    }

    fn solve(
        &self,
        declaration: &ConstraintDecl,
        _available: Constraints,
    ) -> Result<Vec<PlacedRect>, SolveError> {
        if declaration.is_empty() {
            return Ok(Vec::new());
        }
        Err(SolveError::new(
            &self.strategy(),
            SolveErrorKind::Unsupported,
            declaration.iter().map(ToString::to_string),
        ))
    }
}

/// 策略表：**唯一**的 `StrategyId → 实现` 注册处。
pub fn strategies() -> BTreeMap<StrategyId, SolverFactory> {
    let mut table: BTreeMap<StrategyId, SolverFactory> = BTreeMap::new();
    table.insert(StrategyId::default(), |_| Box::new(TwoPassSolver));
    #[cfg(feature = "layout-kasuari")]
    {
        table.insert(StrategyId::new(StrategyId::KASUARI), |declaration| {
            Box::new(kasuari_backend::KasuariSolver::for_declaration(declaration))
        });
    }
    table
}

/// 取策略实现；未注册 ⇒ 显式错误（不回落默认策略）。
pub fn solver_for(
    strategy: &StrategyId,
    declaration: &ConstraintDecl,
) -> Result<Box<dyn LayoutSolver>, SolveError> {
    match strategies().get(strategy) {
        Some(factory) => Ok(factory(declaration)),
        None => Err(SolveError::new(
            strategy,
            SolveErrorKind::UnknownStrategy,
            declaration.iter().map(ToString::to_string),
        )),
    }
}

// ─── 唯一 revision 来源 + 求解缓存 ────────────────────────────────────────────

/// **两条布局链共用的唯一 revision 来源**：`hash(声明 + StrategyId + 约束集 + LayoutTokens)`。
///
/// `declaration` 是调用方（`ui_layout` 用 `relation::revision`；旧链用 `ConstraintSet`
/// 的声明哈希）算出的声明摘要 —— 本函数只负责把「策略 / 约束 / token」并进去。
pub fn plan_revision(
    declaration: u64,
    strategy: &StrategyId,
    constraints: &ConstraintDecl,
    layout: &LayoutTokens,
) -> u64 {
    let mut hash = declaration;
    let mut eat = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for byte in strategy.0.bytes() {
        eat(byte);
    }
    hash ^= constraints.revision();
    hash ^= layout.revision();
    hash
}

/// 求解缓存：**revision 不变 ⇒ 不重解**（判据：60 帧只求解 1 次）。
///
/// 缓存的是派生结果（`PlacedRect`），不进产物；失败不缓存（每次都显式报错）。
#[derive(Debug, Clone, Default)]
pub struct LayoutEngine {
    revision: Option<u64>,
    cached: Vec<PlacedRect>,
    solves: u32,
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// 按 revision 求解。同一 revision 再次调用直接返回缓存。
    pub fn solve(
        &mut self,
        revision: u64,
        strategy: &StrategyId,
        declaration: &ConstraintDecl,
        available: Constraints,
    ) -> Result<&[PlacedRect], SolveError> {
        if self.revision != Some(revision) {
            let solver = solver_for(strategy, declaration)?;
            let mut placed = solver.solve(declaration, available)?;
            placed.sort_by_key(|entry| entry.id);
            self.cached = placed;
            self.revision = Some(revision);
            self.solves += 1;
        }
        Ok(&self.cached)
    }

    /// 累计求解次数（诊断 / 判据）。
    pub fn solves(&self) -> u32 {
        self.solves
    }

    pub fn revision(&self) -> Option<u64> {
        self.revision
    }

    pub fn invalidate(&mut self) {
        self.revision = None;
    }
}

// ─── kasuari 后端（feature `layout-kasuari`）───────────────────────────────────

#[cfg(feature = "layout-kasuari")]
mod kasuari_backend {
    //! Cassowary（增量单纯形）。**只用于唯一确定解的系统**：
    //! 先做 Required 等式的秩检查，秩不足直接 `Ambiguous` —— 求解器的初值 0
    //! 不是「解」，把它当答案就是静默降级。

    use std::collections::BTreeMap;

    use kasuari::{
        Constraint as KConstraint, Expression, RelationalOperator, Solver, Strength, Term, Variable,
    };

    use super::{
        ConstraintDecl, ConstraintOp, ConstraintStrength, LayoutSolver, LayoutVar, PlacedRect,
        SolveError, SolveErrorKind, StrategyId,
    };
    use crate::hit::{Point, Rect};
    use crate::layout::Constraints;

    /// 验证轮次上限（可观测；超限 ⇒ `IterationLimit`，不无限循环）。
    pub const MAX_VERIFY_PASSES: u32 = 8;

    /// 线性规划的数值容差（`to_bits()` 逐位对拍不适用于中间量，这里只做可行性验证）。
    const TOLERANCE: f64 = 1e-9;

    pub struct KasuariSolver {
        strategy: StrategyId,
    }

    impl KasuariSolver {
        pub fn for_declaration(_declaration: &ConstraintDecl) -> Self {
            Self {
                strategy: StrategyId::new(StrategyId::KASUARI),
            }
        }
    }

    fn strength(value: ConstraintStrength) -> Strength {
        match value {
            ConstraintStrength::Required => Strength::REQUIRED,
            ConstraintStrength::Strong => Strength::STRONG,
            ConstraintStrength::Medium => Strength::MEDIUM,
            ConstraintStrength::Weak => Strength::WEAK,
        }
    }

    fn relation(op: ConstraintOp) -> RelationalOperator {
        match op {
            ConstraintOp::Le => RelationalOperator::LessOrEqual,
            ConstraintOp::Eq => RelationalOperator::Equal,
            ConstraintOp::Ge => RelationalOperator::GreaterOrEqual,
        }
    }

    /// Required 等式组的秩（对变量数求满秩 = 解唯一）。
    ///
    /// 用高斯消元，行序 / 列序都来自 `BTreeSet` / `BTreeMap` ⇒ 结果稳定。
    fn required_equations_are_full_rank(
        declaration: &ConstraintDecl,
        columns: &BTreeMap<LayoutVar, usize>,
    ) -> bool {
        let mut rows: Vec<Vec<f64>> = declaration
            .iter()
            .filter(|constraint| {
                constraint.op == ConstraintOp::Eq
                    && constraint.strength == ConstraintStrength::Required
            })
            .map(|constraint| {
                let mut row = vec![0.0f64; columns.len()];
                for term in constraint.canonical_expr() {
                    if let Some(&column) = columns.get(&term.var) {
                        row[column] += term.coef;
                    }
                }
                row
            })
            .collect();
        if columns.is_empty() {
            return true;
        }
        let mut rank = 0usize;
        for column in 0..columns.len() {
            let Some(pivot) = (rank..rows.len()).find(|&row| rows[row][column].abs() > TOLERANCE)
            else {
                continue;
            };
            rows.swap(rank, pivot);
            let divisor = rows[rank][column];
            for slot in rows[rank].iter_mut() {
                *slot /= divisor;
            }
            let pivot_row = rows[rank].clone();
            for (index, row) in rows.iter_mut().enumerate() {
                if index != rank && row[column].abs() > TOLERANCE {
                    let factor = row[column];
                    for (slot, pivot_value) in row.iter_mut().zip(pivot_row.iter()) {
                        *slot -= factor * pivot_value;
                    }
                }
            }
            rank += 1;
            if rank == columns.len() {
                break;
            }
        }
        rank == columns.len()
    }

    impl LayoutSolver for KasuariSolver {
        fn strategy(&self) -> StrategyId {
            self.strategy.clone()
        }

        fn solve(
            &self,
            declaration: &ConstraintDecl,
            available: Constraints,
        ) -> Result<Vec<PlacedRect>, SolveError> {
            if declaration.is_empty() {
                return Ok(Vec::new());
            }
            let widgets = declaration.widgets();
            // 变量按（控件 id, 轴）稳定序创建：插入顺序不参与。
            let mut columns: BTreeMap<LayoutVar, usize> = BTreeMap::new();
            let mut variables: BTreeMap<LayoutVar, Variable> = BTreeMap::new();
            for widget in &widgets {
                for var in [
                    LayoutVar::X(*widget),
                    LayoutVar::Y(*widget),
                    LayoutVar::Width(*widget),
                    LayoutVar::Height(*widget),
                ] {
                    columns.insert(var, columns.len());
                    variables.insert(var, Variable::new());
                }
            }

            let mut solver = Solver::new();
            for constraint in declaration.iter() {
                let terms = constraint.canonical_expr();
                let Some(first) = terms.first().copied() else {
                    // 纯常量约束（`0 == c`）不是布局关系，显式拒绝。
                    return Err(SolveError::new(
                        &self.strategy,
                        SolveErrorKind::Unsupported,
                        [constraint.to_string()],
                    ));
                };
                let mut expression =
                    Expression::from_term(Term::new(variables[&first.var], first.coef));
                for term in terms.iter().skip(1) {
                    expression = expression + Term::new(variables[&term.var], term.coef);
                }
                expression = expression - constraint.rhs;
                solver
                    .add_constraint(KConstraint::new(
                        expression,
                        relation(constraint.op),
                        strength(constraint.strength),
                    ))
                    .map_err(|_| {
                        SolveError::new(
                            &self.strategy,
                            SolveErrorKind::Unsatisfiable,
                            [constraint.to_string()],
                        )
                    })?;
            }

            if !required_equations_are_full_rank(declaration, &columns) {
                return Err(SolveError::new(
                    &self.strategy,
                    SolveErrorKind::Ambiguous,
                    declaration.iter().map(ToString::to_string),
                ));
            }

            // 验证轮（上限可观测）：Required 裁决必须真的成立。
            let mut passes = 0u32;
            loop {
                passes += 1;
                if passes > MAX_VERIFY_PASSES {
                    return Err(SolveError::new(
                        &self.strategy,
                        SolveErrorKind::IterationLimit,
                        declaration.iter().map(ToString::to_string),
                    ));
                }
                let mut violations: Vec<String> = Vec::new();
                for constraint in declaration.iter() {
                    let lhs: f64 = constraint
                        .canonical_expr()
                        .iter()
                        .map(|term| term.coef * solver.get_value(variables[&term.var]))
                        .sum();
                    if constraint.strength == ConstraintStrength::Required
                        && !constraint.op.holds(lhs, constraint.rhs)
                    {
                        violations.push(constraint.to_string());
                    }
                }
                if violations.is_empty() {
                    break;
                }
                return Err(SolveError::new(
                    &self.strategy,
                    SolveErrorKind::Unsatisfiable,
                    violations,
                ));
            }

            let mut placed = Vec::with_capacity(widgets.len());
            for widget in &widgets {
                let (x, y, width, height) = (
                    solver.get_value(variables[&LayoutVar::X(*widget)]),
                    solver.get_value(variables[&LayoutVar::Y(*widget)]),
                    solver.get_value(variables[&LayoutVar::Width(*widget)]),
                    solver.get_value(variables[&LayoutVar::Height(*widget)]),
                );
                if ![x, y, width, height].iter().all(|value| value.is_finite()) {
                    return Err(SolveError::new(
                        &self.strategy,
                        SolveErrorKind::Unplaced(*widget),
                        declaration.iter().map(ToString::to_string),
                    ));
                }
                if width > available.max.width + TOLERANCE
                    || height > available.max.height + TOLERANCE
                    || x < -TOLERANCE
                    || y < -TOLERANCE
                    || x + width > available.max.width + TOLERANCE
                    || y + height > available.max.height + TOLERANCE
                {
                    return Err(SolveError::new(
                        &self.strategy,
                        SolveErrorKind::Overflow(*widget),
                        declaration.iter().map(ToString::to_string),
                    ));
                }
                placed.push(PlacedRect {
                    id: *widget,
                    rect: Rect::from_min_size(
                        Point::new(x.max(0.0), y.max(0.0)),
                        width.max(0.0),
                        height.max(0.0),
                    ),
                });
            }
            placed.sort_by_key(|entry| entry.id);
            Ok(placed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit::Point;

    fn term(var: LayoutVar, coef: f64) -> LinearTerm {
        LinearTerm::new(var, coef)
    }

    /// 跨容器用例：A 在左、B 在右，`A.width == B.width + 2*padding`，B 紧跟 A。
    ///
    /// 两条约束牵涉两个控件 ⇒ 不在同一条父子链上，两遍扫描逐控件测量给不出解。
    fn cross_container() -> ConstraintDecl {
        let (a, b) = (LayoutId(0), LayoutId(1));
        let padding = 8.0;
        let mut decl = ConstraintDecl::new();
        decl.insert(Constraint::var(LayoutVar::X(a), ConstraintOp::Eq, 0.0));
        decl.insert(Constraint::var(LayoutVar::Y(a), ConstraintOp::Eq, 0.0));
        decl.insert(Constraint::var(
            LayoutVar::Height(a),
            ConstraintOp::Eq,
            40.0,
        ));
        decl.insert(Constraint::var(
            LayoutVar::Width(b),
            ConstraintOp::Eq,
            100.0,
        ));
        decl.insert(Constraint::var(LayoutVar::Y(b), ConstraintOp::Eq, 0.0));
        decl.insert(Constraint::var(
            LayoutVar::Height(b),
            ConstraintOp::Eq,
            40.0,
        ));
        decl.insert(Constraint::new(
            vec![
                term(LayoutVar::Width(a), 1.0),
                term(LayoutVar::Width(b), -1.0),
            ],
            ConstraintOp::Eq,
            padding * 2.0,
        ));
        decl.insert(Constraint::new(
            vec![term(LayoutVar::X(b), 1.0), term(LayoutVar::Width(a), -1.0)],
            ConstraintOp::Eq,
            0.0,
        ));
        decl
    }

    /// **判据 6**：不同插入顺序 ⇒ 同一个声明、同一个 revision。
    #[test]
    fn constraint_decl_is_insertion_order_independent() {
        let forward = cross_container();
        let mut reversed = ConstraintDecl::new();
        let mut all: Vec<Constraint> = cross_container().iter().cloned().collect();
        all.reverse();
        for constraint in all {
            reversed.insert(constraint);
        }
        assert_eq!(forward, reversed);
        assert_eq!(forward.revision(), reversed.revision());
        assert_eq!(
            forward.iter().collect::<Vec<_>>(),
            reversed.iter().collect::<Vec<_>>()
        );
    }

    /// **判据 5（前半）**：两遍扫描对跨容器声明显式给不出解。
    #[test]
    fn two_pass_refuses_cross_container_constraints() {
        let decl = cross_container();
        let solver = solver_for(&StrategyId::default(), &decl).expect("默认策略已注册");
        let error = solver
            .solve(&decl, Constraints::new(400.0, 400.0))
            .expect_err("两遍扫描不得假装能解");
        assert_eq!(error.kind, SolveErrorKind::Unsupported);
        assert!(error.conflicts.len() >= 2, "冲突项路径必须可读");

        // 空声明 ⇒ 不报错（默认策略的常态）。
        assert!(
            TwoPassSolver
                .solve(&ConstraintDecl::new(), Constraints::new(400.0, 400.0))
                .expect("空声明")
                .is_empty()
        );
    }

    /// 未注册的策略必须显式报错，不回落默认策略。
    #[test]
    fn unknown_strategy_is_an_explicit_error() {
        let error = solver_for(&StrategyId::new("nope"), &ConstraintDecl::new())
            .err()
            .expect("未注册的策略必须报错");
        assert_eq!(error.kind, SolveErrorKind::UnknownStrategy);
    }

    /// **判据 5（后半）**：kasuari 给出唯一确定解。
    #[cfg(feature = "layout-kasuari")]
    #[test]
    fn kasuari_solves_the_cross_container_case() {
        let decl = cross_container();
        let solver = solver_for(&StrategyId::new(StrategyId::KASUARI), &decl).expect("策略已注册");
        let placed = solver
            .solve(&decl, Constraints::new(400.0, 400.0))
            .expect("有唯一解");
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0].id, LayoutId(0));
        assert_eq!(placed[0].rect.min, Point::new(0.0, 0.0));
        assert_eq!(placed[0].rect.width(), 116.0, "100 + 2*8");
        assert_eq!(placed[0].rect.height(), 40.0);
        assert_eq!(placed[1].id, LayoutId(1));
        assert_eq!(placed[1].rect.min, Point::new(116.0, 0.0));
        assert_eq!(placed[1].rect.width(), 100.0);
    }

    /// **判据 5**：换 f64 最小步长能观测到结果变化（证明不是常量兜底）。
    #[cfg(feature = "layout-kasuari")]
    #[test]
    fn one_ulp_change_is_observable() {
        let solver = solver_for(
            &StrategyId::new(StrategyId::KASUARI),
            &ConstraintDecl::new(),
        )
        .expect("策略已注册");
        let base = cross_container();
        let placed_base = solver.solve(&base, Constraints::new(400.0, 400.0)).unwrap();

        let mut bumped = ConstraintDecl::new();
        for constraint in base.iter() {
            let mut next = constraint.clone();
            if next.rhs == 100.0 {
                next.rhs = f64::from_bits(100.0f64.to_bits() + 1);
            }
            bumped.insert(next);
        }
        let placed_bumped = solver
            .solve(&bumped, Constraints::new(400.0, 400.0))
            .unwrap();
        assert_ne!(
            placed_base[1].rect.width().to_bits(),
            placed_bumped[1].rect.width().to_bits(),
            "1 ULP 的声明差异必须出现在结果里"
        );
    }

    /// 欠定声明必须显式报错（求解器初值 0 不是解）。
    #[cfg(feature = "layout-kasuari")]
    #[test]
    fn underdetermined_declaration_is_ambiguous() {
        let solver = solver_for(
            &StrategyId::new(StrategyId::KASUARI),
            &ConstraintDecl::new(),
        )
        .expect("策略已注册");
        let mut decl = ConstraintDecl::new();
        decl.insert(Constraint::var(
            LayoutVar::X(LayoutId(0)),
            ConstraintOp::Eq,
            0.0,
        ));
        let error = solver
            .solve(&decl, Constraints::new(100.0, 100.0))
            .expect_err("缺 Y/宽/高，不得拿 0 兜底");
        assert_eq!(error.kind, SolveErrorKind::Ambiguous);
    }

    /// 冲突的 Required 约束必须显式报错并给出路径。
    #[cfg(feature = "layout-kasuari")]
    #[test]
    fn conflicting_required_constraints_are_explicit() {
        let solver = solver_for(
            &StrategyId::new(StrategyId::KASUARI),
            &ConstraintDecl::new(),
        )
        .expect("策略已注册");
        let id = LayoutId(0);
        let mut decl = cross_container();
        decl.insert(Constraint::var(LayoutVar::Width(id), ConstraintOp::Eq, 7.0));
        let error = solver
            .solve(&decl, Constraints::new(400.0, 400.0))
            .expect_err("Width 同时等于 116 与 7");
        assert!(
            matches!(
                error.kind,
                SolveErrorKind::Unsatisfiable
                    | SolveErrorKind::Ambiguous
                    | SolveErrorKind::Overflow(_)
            ),
            "实际：{:?}",
            error.kind
        );
        assert!(!error.conflicts.is_empty(), "冲突路径必须可读");
    }

    /// **判据 6**：`60 帧只求解 1 次`；换策略 / 约束 / token 才重解。
    #[test]
    fn layout_engine_solves_once_per_revision() {
        let decl = ConstraintDecl::new();
        let strategy = StrategyId::default();
        let layout = LayoutTokens::default();
        let revision = plan_revision(0x1234, &strategy, &decl, &layout);

        let mut engine = LayoutEngine::new();
        for _ in 0..60 {
            engine
                .solve(revision, &strategy, &decl, Constraints::new(400.0, 400.0))
                .expect("空声明对两遍扫描是 Ok");
        }
        assert_eq!(engine.solves(), 1, "60 帧只有 1 次求解");

        let cross = cross_container();
        let converted = plan_revision(0x1234, &strategy, &cross, &layout);
        assert_ne!(revision, converted, "换约束集必须改变 revision");
        let other = plan_revision(
            0x1234,
            &StrategyId::new(StrategyId::KASUARI),
            &decl,
            &layout,
        );
        assert_ne!(revision, other, "换策略必须改变 revision");
        let mut repainted = layout;
        repainted.gap = 5.0;
        assert_ne!(
            revision,
            plan_revision(0x1234, &strategy, &decl, &repainted),
            "换 LayoutTokens 必须改变 revision"
        );
        // 换 paint token 不重排由 `widget::paint_tokens_change_commands_but_not_layout_revision` 守着。
    }
}
