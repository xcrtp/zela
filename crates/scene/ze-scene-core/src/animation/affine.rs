//! **仿射识别**（BX Step 1）：判定 `State.step` 是否关于 `prev` 仿射，并提取 `(A, b)`。
//!
//! # 为什么在这里
//!
//! `ze-comb` 不解释数值（C1），`ze-notion` / 数学库只在这一层可见（C2）。
//! 识别器的形态是**装配期 analysis pass**：只读 `Expr`，产出 side table，不进产物。
//!
//! # 覆盖范围（首批，从严到宽）
//!
//! | 形态 | 宽度 |
//! |---|---|
//! | `prev + c` / `prev * c` / 两者线性组合 | 1 |
//! | `(x, v)` 互相线性引用（半隐式欧拉） | 2 |
//! | `sin(prev)` / `abs(prev)` / `prev²` | ❌ 显式失败 |
//!
//! # 与"逐位相同"的关系（**BY 翻转定义**）
//!
//! **`apply` 是定义**：`State(n) := 闭式 / 矩阵快速幂的值`。
//! 逐次重放是**近似实现**，只作为回归 oracle（容差见 [`STATE_ORACLE_TOLERANCE`]）。
//!
//! 两者算的是同一个实数递推、舍入路径不同；快速幂的运算更少 ⇒ 更接近真值。
//! 要求两个不同的浮点近似逐位相同是"巧合检测"，不是正确性判据（BY 计划 §2）。
//!
//! 本定义**只适用于 `State` 的闭式求值**；项目其余部分继续严守逐位相同。
//!
//! # 纯数值算法在 `ze-alg`（CB-b）
//!
//! 矩阵幂 / 特征多项式 / Durand–Kerner / 谱半径已抽到 [`ze_alg`]（kernel 级、
//! 零 glam）。本文件只保留**读 `Expr` 的识别逻辑**与场景侧的 `AffineMap` /
//! `StatePlan` / `StateShape`。
//!
//! # 精度口径（CB-c）
//!
//! `State` 的内部运算**一律 f64**（`ze-alg` 的当前唯一实现）。
//! 标量状态直接以 f64 输出；**向量**状态（Vec2/3/4）向 `f32` 的降级只发生在
//! **一次**：构造 [`EvalValue`] 的边界（[`StateShape::value`]）。不得在中间步骤
//! 反复降级 —— 否则舍入会随步数累积，且同一状态在「seek 到第 n 帧」与
//! 「逐帧播放」下会得到不同的 f32 值。
//! 用例 `state_demotes_to_f32_only_at_the_eval_boundary` 钉住这条口径。

use std::collections::BTreeSet;

use ze_comb::{Expr, Literal, Node, Value as EvalValue};

use crate::animation::predicate::{IntervalSet, TIME_VARIABLE, analyze as analyze_predicate};

/// **BZ-c**：事件通道的依赖键（脚本写作 `expr.channel { name = "event" }`）。
///
/// 语义：本帧 `InputEvents` 非空 ⇒ `true`（纯布尔存在性；具体事件用其它通道）。
pub const EVENT_CHANNEL: &str = "event";

/// 一条 `State` 依赖的通道集合（`namespace::name`；`BTreeSet` ⇒ 升序）。
///
/// **派生量**：装配期算出的 side table，不进 payload（纪律④）。
pub type StateDependencies = BTreeSet<String>;

/// 识别器支持的最大状态宽度（与 [`ze_alg::MAX_DIM`] 同一处定义）。
pub const AFFINE_MAX_WIDTH: usize = ze_alg::MAX_DIM;

/// **容差 oracle**（BY-a2）：闭式定义 vs 逐次重放的回归判据。
///
/// 单位是**分量绝对误差**。场景侧状态的分量量级为 1–10，f32 重放的舍入
/// （每步 ~1e-7，120 步 ~1e-5）远小于该阈值；而真正的语义错误（非线性、
/// 状态错位、predicate 边界错）是数量级差异，不会被放过。
///
/// ⚠️ 它不是"反正有容差"的口子：只用于 `State` 闭式求值的 oracle，
/// 其它所有判据仍要求逐位相同（BY 计划 §2.6）。
pub const STATE_ORACLE_TOLERANCE: f64 = 1e-3;

/// `out[i] = Σ_j a[i][j] · prev[j] + b[i]`。
#[derive(Clone, Debug, PartialEq)]
pub struct AffineMap {
    a: Vec<Vec<f64>>,
    b: Vec<f64>,
}

impl AffineMap {
    /// 状态宽度 `k`。
    pub fn width(&self) -> usize {
        self.b.len()
    }

    /// 线性部分（行优先，`k × k`）。
    pub fn a(&self) -> &[Vec<f64>] {
        &self.a
    }

    /// 平移部分（长度 `k`）。
    pub fn b(&self) -> &[f64] {
        &self.b
    }

    /// 读一个线性系数 `A[row][col]`（越界返回 `None`）。
    ///
    /// **CA-b**：参数面板把 `State` 的系数当可调参数枚举 / 回写。
    pub fn linear(&self, row: usize, col: usize) -> Option<f64> {
        self.a.get(row).and_then(|line| line.get(col)).copied()
    }

    /// 复制一份并把 `A[row][col]` 换成 `value`（越界返回 `None`）。
    ///
    /// 这是**纯函数式**改系数：调用方拿到新图后自行校验稳定性
    /// （[`StatePlan::set_linear`] 是带门禁的封装）。
    pub fn with_linear(&self, row: usize, col: usize, value: f64) -> Option<AffineMap> {
        let mut next = self.clone();
        *next.a.get_mut(row)?.get_mut(col)? = value;
        Some(next)
    }

    /// **增广矩阵幂**：`f^n(init)`，`O(k³ log n)`。
    ///
    /// **这是定义**（BY §2.2）：`State(n) := apply(init, n)`。
    /// 纯函数 ⇒ seek 与路径无关；只用 `+ − ×` ⇒ IEEE754 逐位可复现。
    ///
    /// 数值实现在 [`ze_alg::affine_apply`]：矩阵乘法的求和顺序由下标升序固定 ⇒
    /// **不得**改成并行 / 树形归约（那会改变舍入）。
    pub fn apply(&self, init: &[f64], n: u64) -> Vec<f64> {
        ze_alg::affine_apply(&self.a, &self.b, init, n)
    }

    /// **BZ-d 多实例**：N 个实例共享同一组 `(A, b)`、只差 `init`。
    ///
    /// **一次**矩阵快速幂 + N 次"矩阵 × 向量"（不是 N 次快速幂）。
    /// 每个结果与 [`apply`](Self::apply) **逐位相同**（同一个 `M^n`、同一求和顺序）。
    pub fn apply_many(&self, inits: &[Vec<f64>], n: u64) -> Vec<Vec<f64>> {
        ze_alg::affine_apply_many(&self.a, &self.b, inits, n)
    }

    /// 逐次重放（**近似实现 / 测试 oracle**，不是定义）。
    pub fn replay(&self, init: &[f64], n: u64) -> Vec<f64> {
        ze_alg::affine_replay(&self.a, &self.b, init, n)
    }

    /// 在 `0..=horizon` 上比对闭式定义与重放（**容差内**，不是逐位）。
    ///
    /// BY §2.2：这是回归 oracle，不是"是否启用快速幂"的开关。
    pub fn replay_within(&self, init: &[f64], horizon: u64, tolerance: f64) -> bool {
        ze_alg::affine_replay_within(&self.a, &self.b, init, horizon, tolerance)
    }

    /// **谱半径** `|λ|max`（BZ-a：装配期稳定性门禁用）。
    ///
    /// 数值实现位于 [`ze_alg::spectral_radius_slice`]（CB-b）：1 / 2 阶闭式，
    /// 3 / 4 阶走特征多项式 + Durand–Kerner（固定初值 / 固定迭代 ⇒ 确定性）。
    /// 只在装配期调用，**不在**每帧路径上。
    ///
    /// 非有限值（NaN / inf 来自非法常量）原样传播 ⇒ 调用方的 `< 1` 判定会拒绝。
    pub fn spectral_radius(&self) -> f64 {
        ze_alg::spectral_radius_slice(&self.a)
    }
}

/// 识别失败：**显式**错误（不是"静默当作非线性跳过"）。
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum AffineError {
    /// 出现了识别器不接受的节点 / 算子。
    #[error("`State.step` 含 `{0}`：不是关于 prev 的仿射表达式")]
    NotAffine(String),
    /// `prev` 与自身相乘（二次项）。
    #[error("`State.step` 含 `prev × prev`（二次项 ⇒ 非线性）")]
    Quadratic,
    /// 状态宽度不受支持。
    #[error("`State` 的状态宽度 {0} 不受支持（首批 1 / 2 维）")]
    UnsupportedWidth(usize),
    /// 宽度不一致（init 与 step 的 `prev` 形状不同）。
    #[error("`State.step` 的宽度 {actual} 与 init 宽度 {expected} 不一致")]
    WidthMismatch { expected: usize, actual: usize },
    /// init 不是可识别的数值 / 向量。
    #[error("`State.init` 不是数值或定长向量：{0}")]
    UnsupportedInit(String),
    /// predicate 不可分析（不是时间 / 索引区间）。
    #[error("`State.predicate` 不可分析：{0}")]
    UnsupportedPredicate(String),
    /// predicate 引用了状态自身（环依赖）。
    #[error("`State.predicate` 不得引用状态自身（`rec_prev` / 嵌套 State / NatRec）")]
    PredicateRefsState,
    /// **BZ-a**：仿射图不稳定（`|λ|max ≥ 1` 或非有限）⇒ 闭式求值会随帧数发散。
    #[error(
        "`State.step` 的仿射图不稳定：谱半径 |λ|max = {radius:.6}（要求 |λ|max < 1；\
         1 阶取 |a| < 1，2 阶半隐式欧拉取 damp + stiff < 1；边缘稳定 |λ| = 1 同样拒绝）"
    )]
    Unstable { radius: f64 },
}

/// 装配期门禁失败：`State.step` 不是仿射（显式拒绝，不降级 `Replay`）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("绑定 `{target}` 的 `State.step` 不是关于 prev 的仿射表达式：{reason}（本批 AI-07 不放开）")]
pub struct StateUnsupported {
    pub target: String,
    pub reason: String,
}

/// 一条 `State` 的装配期分析结果（side table：**不进产物**）。
#[derive(Clone, Debug, PartialEq)]
pub struct StateAnalysis {
    /// 绑定目标名（诊断用）。
    pub target: String,
    /// 闭式计划（仿射图 + init + predicate 区间）。
    pub plan: StatePlan,
}

/// 按**编译器的 DFS 序**收集一条表达式里的全部 `State` 计划。
///
/// 顺序与 [`ze_comb::Step::State`] 的 `id` 分配一致：`init` → `step` → `predicate`
/// → 自身（父节点的其余子表达式按 `Node::children()` 顺序）。
pub fn plans_for_expr(expr: &Expr) -> Result<Vec<StatePlan>, AffineError> {
    let mut out = Vec::new();
    collect_plans(expr, &mut out)?;
    Ok(out)
}

fn collect_plans(expr: &Expr, out: &mut Vec<StatePlan>) -> Result<(), AffineError> {
    if let Node::State {
        init,
        step,
        predicate,
    } = &expr.node
    {
        collect_plans(init, out)?;
        collect_plans(step, out)?;
        if let Some(predicate) = predicate {
            collect_plans(predicate, out)?;
        }
        out.push(plan_from_expr(init, step, predicate.as_deref())?);
        return Ok(());
    }
    for child in expr.node.children() {
        collect_plans(child, out)?;
    }
    Ok(())
}

/// 状态的**值形状**（决定 `Value` 的标签，运行时与重放路径一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateShape {
    Scalar,
    Vec2,
    Vec3,
    Vec4,
}

impl StateShape {
    pub fn width(self) -> usize {
        match self {
            StateShape::Scalar => 1,
            StateShape::Vec2 => 2,
            StateShape::Vec3 => 3,
            StateShape::Vec4 => 4,
        }
    }

    fn value(self, components: &[f64]) -> EvalValue {
        match self {
            StateShape::Scalar => EvalValue::f64(components[0]),
            StateShape::Vec2 => EvalValue::vec2([components[0] as f32, components[1] as f32]),
            StateShape::Vec3 => EvalValue::vec3([
                components[0] as f32,
                components[1] as f32,
                components[2] as f32,
            ]),
            StateShape::Vec4 => EvalValue::vec4([
                components[0] as f32,
                components[1] as f32,
                components[2] as f32,
                components[3] as f32,
            ]),
        }
    }
}

/// **闭式计划**：装配期把 `State` 固定成 `State(n) := apply(init, steps(n))`。
///
/// - `steps(n)` 是 predicate 生效的步数（无 predicate ⇒ `n`）；
/// - 计划是**派生量**，存在 `Binding` 的 side table 里，不进 payload；
/// - 没有计划时运行期退回重放（带 predicate 时**显式失败**，不静默忽略门槛）。
#[derive(Clone, Debug, PartialEq)]
pub struct StatePlan {
    map: AffineMap,
    init: Vec<f64>,
    shape: StateShape,
    /// 生效**状态索引**的集合（`time` 变量在此解释为索引；`None` = 每步都生效）。
    active: Option<IntervalSet>,
    /// **BZ-c**：依赖的通道集合（升序；派生量，不进产物）。
    dependencies: StateDependencies,
    /// **BZ-c**：predicate 依赖 `event` 通道 ⇒ 按事件帧稀疏重放（L2）。
    event_gated: bool,
    /// 事件 predicate 的编译程序（运行期逐帧求值，判定该事件帧是否推进）。
    event_predicate: Option<ze_comb::CompiledExpr>,
}

impl StatePlan {
    /// 状态宽度。
    pub fn width(&self) -> usize {
        self.map.width()
    }

    /// 仿射图（诊断 / 测试）。
    pub fn map(&self) -> &AffineMap {
        &self.map
    }

    /// 值形状。
    pub fn shape(&self) -> StateShape {
        self.shape
    }

    /// 到第 `frame` 帧为止**实际推进的步数**（predicate 生效的索引数）。
    ///
    /// ⚠️ 只适用于时间 / 索引 predicate；事件 predicate 的步数由
    /// [`value_at_event`](Self::value_at_event) 从事件日志给出。
    pub fn steps_at(&self, frame: i64) -> u64 {
        debug_assert!(!self.event_gated, "事件 predicate 的步数必须由事件日志给出");
        let frame = frame.max(0) as u64;
        match &self.active {
            Some(set) => set.count_integers(1, frame as i64).max(0) as u64,
            None => frame,
        }
    }

    /// **定义**：第 `frame` 帧的状态值。
    pub fn value_at(&self, frame: i64) -> EvalValue {
        debug_assert!(!self.event_gated, "事件 predicate 必须走 value_at_event");
        let steps = self.steps_at(frame);
        self.shape.value(&self.map.apply(&self.init, steps))
    }

    /// **BZ-c 定义**：事件 predicate 的状态值。
    ///
    /// `matched` 是**升序**的帧号列表：`P(n)` 为真的索引（`n ≥ 1`，索引 0 是 init）。
    /// `State(frame) = f^{# { n ∈ matched : n ≤ frame } }(init)` ——
    /// 只随事件帧推进，帧间是 identity（与 `Hold` 同构）。
    pub fn value_at_event(&self, frame: i64, matched: &[i64]) -> EvalValue {
        debug_assert!(self.event_gated, "非事件 predicate 不得使用事件日志");
        let frame = frame.max(0);
        let steps = matched.partition_point(|&index| index <= frame) as u64;
        self.shape.value(&self.map.apply(&self.init, steps))
    }

    /// predicate 生效的索引集（`None` = 每步生效 / 事件 predicate）。
    pub fn active(&self) -> Option<&IntervalSet> {
        self.active.as_ref()
    }

    /// **BZ-c**：依赖的通道集合（升序）。
    pub fn dependencies(&self) -> &StateDependencies {
        &self.dependencies
    }

    /// **BZ-c**：是否由事件 predicate 驱动（L2：按事件帧稀疏重放）。
    pub fn event_gated(&self) -> bool {
        self.event_gated
    }

    /// 事件 predicate 的编译程序（`None` = 非事件 predicate）。
    pub fn event_predicate(&self) -> Option<&ze_comb::CompiledExpr> {
        self.event_predicate.as_ref()
    }

    /// 读一个线性系数 `A[row][col]`（参数注册表用）。
    pub fn linear(&self, row: usize, col: usize) -> Option<f64> {
        self.map.linear(row, col)
    }

    /// 写一个线性系数，并**重新过 BZ-a 稳定性门禁**。
    ///
    /// 参数面板让用户改 `stiff` / `damp` 之后，这里必须复用同一判据：
    /// 谱半径 `|λ|max ≥ 1`（或非有限）⇒ [`AffineError::Unstable`] 显式拒绝，
    /// 原图保持不动（不允许先写进去、运行期再拿到 inf/NaN）。
    pub fn set_linear(
        &mut self,
        row: usize,
        col: usize,
        value: f64,
    ) -> Result<(), AffineError> {
        let next = self
            .map
            .with_linear(row, col, value)
            .ok_or_else(|| AffineError::NotAffine(format!("线性系数 A[{row}][{col}] 越界")))?;
        let radius = next.spectral_radius();
        if !radius.is_finite() || radius >= 1.0 {
            return Err(AffineError::Unstable { radius });
        }
        self.map = next;
        Ok(())
    }
}

/// 从表达式树建立闭式计划（装配期；只读）。
///
/// `predicate` 的 `time` 变量解释为**状态索引**；不可分析 / 引用状态自身 ⇒ 显式失败。
pub fn plan_from_expr(
    init: &Expr,
    step: &Expr,
    predicate: Option<&Expr>,
) -> Result<StatePlan, AffineError> {
    let (init_values, shape) = init_values(init)?;
    let map = recognize(step, shape.width())?;
    // **BZ-a 稳定性门禁**：闭式求值 `f^n` 只在 `|λ|max < 1` 时有界。
    // 发散 / 边缘稳定都在装配期显式失败（不静默产出 inf/NaN）。
    let radius = map.spectral_radius();
    if !radius.is_finite() || radius >= 1.0 {
        return Err(AffineError::Unstable { radius });
    }
    // **BZ-c**：依赖边 + 事件 predicate 分区（同一个遍历，一次做掉）。
    let dependencies = dependencies_of(init, step, predicate);
    let event_gated = predicate.is_some() && dependencies.contains(EVENT_CHANNEL);
    let (active, event_predicate) = match predicate {
        Some(predicate) => {
            // 硬边界 1：predicate 不得引用状态自身的值（含事件 predicate）。
            if references_state(predicate) {
                return Err(AffineError::PredicateRefsState);
            }
            if event_gated {
                let unexpected: Vec<&str> = dependencies
                    .iter()
                    .map(String::as_str)
                    .filter(|key| *key != EVENT_CHANNEL && *key != TIME_VARIABLE)
                    .collect();
                if !unexpected.is_empty() {
                    return Err(AffineError::UnsupportedPredicate(format!(
                        "事件 predicate 只能依赖 `{EVENT_CHANNEL}` / `{TIME_VARIABLE}`，\
                         实际还依赖 {unexpected:?}"
                    )));
                }
                let compiled = ze_comb::CompiledExpr::compile(predicate).map_err(|error| {
                    AffineError::UnsupportedPredicate(format!("事件 predicate 编译失败：{error}"))
                })?;
                // 事件 predicate 的索引集未知 ⇒ `active = None`；步数由事件日志给出。
                (None, Some(compiled))
            } else {
                (active_intervals(predicate)?, None)
            }
        }
        None => (None, None),
    };
    Ok(StatePlan {
        map,
        init: init_values,
        shape,
        active,
        dependencies,
        event_gated,
        event_predicate,
    })
}

/// 一条 `State` 的依赖通道集（`init` / `step` / `predicate` 全部计入）。
fn dependencies_of(
    init: &Expr,
    step: &Expr,
    predicate: Option<&Expr>,
) -> StateDependencies {
    let mut out = BTreeSet::new();
    for expression in [Some(init), Some(step), predicate].into_iter().flatten() {
        collect_dependencies(expression, &mut out);
    }
    out
}

/// 递归收集通道读（`Channel` / 组件字段 `Read`）。
fn collect_dependencies(expr: &Expr, out: &mut StateDependencies) {
    match &expr.node {
        Node::Channel { channel } => {
            out.insert(channel_key(channel));
        }
        Node::Read { channel, .. } => {
            out.insert(channel_key(channel));
        }
        _ => {}
    }
    for child in expr.node.children() {
        collect_dependencies(child, out);
    }
}

/// `namespace::name`（空 namespace 只写 `name`）—— 与脚本写法一致的升序键。
fn channel_key(channel: &ze_comb::ChannelRef) -> String {
    if channel.namespace.is_empty() {
        channel.name.clone()
    } else {
        format!("{}::{}", channel.namespace, channel.name)
    }
}

/// `State.init` → 分量值 + 形状（只接受字面量与定长向量构造）。
fn init_values(init: &Expr) -> Result<(Vec<f64>, StateShape), AffineError> {
    match &init.node {
        Node::Pure { value } => match value {
            Literal::F32(v) => Ok((vec![f64::from(*v)], StateShape::Scalar)),
            Literal::F64(v) => Ok((vec![*v], StateShape::Scalar)),
            Literal::I32(v) => Ok((vec![f64::from(*v)], StateShape::Scalar)),
            Literal::I64(v) => Ok((vec![*v as f64], StateShape::Scalar)),
            Literal::Vec2(v) => Ok((
                v.iter().map(|x| f64::from(*x)).collect(),
                StateShape::Vec2,
            )),
            Literal::Vec3(v) => Ok((
                v.iter().map(|x| f64::from(*x)).collect(),
                StateShape::Vec3,
            )),
            Literal::Vec4(v) => Ok((
                v.iter().map(|x| f64::from(*x)).collect(),
                StateShape::Vec4,
            )),
            Literal::Bool(_) | Literal::Text(_) => Err(AffineError::UnsupportedInit(
                "Bool / Text 不是状态值".to_string(),
            )),
        },
        Node::Call { op, args } => {
            let shape = match op.name.as_str() {
                "vec2" => StateShape::Vec2,
                "vec3" => StateShape::Vec3,
                "vec4" => StateShape::Vec4,
                other => {
                    return Err(AffineError::UnsupportedInit(format!(
                        "构造器 `{other}` 不是定长数值"
                    )))
                }
            };
            if args.len() != shape.width() {
                return Err(AffineError::UnsupportedInit(format!(
                    "`{}` 需要 {} 个分量，实际 {}",
                    op.name,
                    shape.width(),
                    args.len()
                )));
            }
            let values = args
                .iter()
                .map(const_scalar)
                .collect::<Result<Vec<_>, _>>()?;
            Ok((values, shape))
        }
        other => Err(AffineError::UnsupportedInit(format!("节点 `{other:?}`"))),
    }
}

/// predicate → **状态索引**区间集（`None` = 恒真 / 无约束）。
fn active_intervals(predicate: &Expr) -> Result<Option<IntervalSet>, AffineError> {
    if references_state(predicate) {
        return Err(AffineError::PredicateRefsState);
    }
    let Some(set) = analyze_predicate(predicate) else {
        return Err(AffineError::UnsupportedPredicate(
            "predicate 不是可分析的时间 / 索引区间（要求 time 与字面量比较 + And/Or/Not）"
                .to_string(),
        ));
    };
    if set.has_non_time_variable() {
        return Err(AffineError::UnsupportedPredicate(format!(
            "predicate 依赖非时间变量：{:?}",
            set.non_time_variables()
        )));
    }
    if set.is_empty() {
        return Ok(Some(IntervalSet::empty()));
    }
    let mut intervals = set.vars();
    match intervals.next() {
        Some((name, set)) if name == TIME_VARIABLE => Ok(Some(set.clone())),
        // `is_all`（无变量约束）⇒ 每步都生效。
        None => Ok(None),
        Some((name, _)) => Err(AffineError::UnsupportedPredicate(format!(
            "predicate 的变量名必须是 `{TIME_VARIABLE}`，实际 `{name}`"
        ))),
    }
}

/// predicate 是否引用了**状态自身**（`RecPrev` / 嵌套 `State` / 离散层）。
fn references_state(expr: &Expr) -> bool {
    match &expr.node {
        Node::RecPrev | Node::RecIndex | Node::State { .. } | Node::NatRec { .. } => true,
        _ => expr.node.children().into_iter().any(references_state),
    }
}

/// `State.init` 的宽度（`1` = 标量，`2..=4` = 向量）。
pub fn init_width(init: &Expr) -> Result<usize, AffineError> {
    match &init.node {
        Node::Pure { value } => literal_width(value).ok_or_else(|| {
            AffineError::UnsupportedInit(format!("字面量 `{value:?}` 不是数值 / 向量"))
        }),
        Node::Call { op, args } if op.name == "vec2" && args.len() == 2 => Ok(2),
        Node::Call { op, args } if op.name == "vec3" && args.len() == 3 => Ok(3),
        Node::Call { op, args } if op.name == "vec4" && args.len() == 4 => Ok(4),
        Node::Call { op, .. } => Err(AffineError::UnsupportedInit(format!(
            "构造器 `{}` 不是定长数值",
            op.name
        ))),
        other => Err(AffineError::UnsupportedInit(format!("节点 `{other:?}`"))),
    }
}

fn literal_width(value: &Literal) -> Option<usize> {
    match value {
        Literal::F32(_) | Literal::F64(_) | Literal::I32(_) | Literal::I64(_) => Some(1),
        Literal::Vec2(_) => Some(2),
        Literal::Vec3(_) => Some(3),
        Literal::Vec4(_) => Some(4),
        Literal::Bool(_) | Literal::Text(_) => None,
    }
}

/// 一条线性形式：`Σ coeffs[j] · prev[j] + offset`。
#[derive(Clone, Debug)]
struct Row {
    coeffs: Vec<f64>,
    offset: f64,
}

impl Row {
    fn constant(offset: f64, width: usize) -> Self {
        Self {
            coeffs: vec![0.0; width],
            offset,
        }
    }

    fn variable(index: usize, width: usize) -> Self {
        let mut coeffs = vec![0.0; width];
        coeffs[index] = 1.0;
        Self { coeffs, offset: 0.0 }
    }

    fn add(&self, other: &Self) -> Self {
        Self {
            coeffs: self
                .coeffs
                .iter()
                .zip(&other.coeffs)
                .map(|(a, b)| a + b)
                .collect(),
            offset: self.offset + other.offset,
        }
    }

    fn scaled(&self, k: f64) -> Self {
        Self {
            coeffs: self.coeffs.iter().map(|c| c * k).collect(),
            offset: self.offset * k,
        }
    }
}

/// 识别结果：`rows.len()` 个分量（1 = 标量，2..=4 = 向量）。
#[derive(Clone, Debug)]
struct Value {
    rows: Vec<Row>,
}

/// 识别 `step`：返回 `(A, b)`。
pub fn recognize(step: &Expr, width: usize) -> Result<AffineMap, AffineError> {
    if !(1..=AFFINE_MAX_WIDTH).contains(&width) {
        return Err(AffineError::UnsupportedWidth(width));
    }
    let value = affine_value(step, width)?;
    if value.rows.len() != width {
        return Err(AffineError::WidthMismatch {
            expected: width,
            actual: value.rows.len(),
        });
    }
    Ok(AffineMap {
        a: value.rows.iter().map(|row| row.coeffs.clone()).collect(),
        b: value.rows.iter().map(|row| row.offset).collect(),
    })
}

fn affine_value(expr: &Expr, width: usize) -> Result<Value, AffineError> {
    match &expr.node {
        Node::Pure { value } => value_from_literal(value, width),
        // 递归叶子：`prev`（向量宽度 = 状态宽度）。
        Node::RecPrev => Ok(Value {
            rows: (0..width).map(|i| Row::variable(i, width)).collect(),
        }),
        Node::Call { op, args } => affine_call(&op.name, args, width),
        // 其余一律显式拒绝（不猜常量值）：通道 / 组件读 / 方法 / 分支 / 嵌套递归。
        Node::Channel { .. } => Err(AffineError::NotAffine("通道读".into())),
        Node::Read { .. } => Err(AffineError::NotAffine("组件字段读".into())),
        Node::Method { .. } => Err(AffineError::NotAffine("方法调用".into())),
        Node::Select { .. } => Err(AffineError::NotAffine("值分支".into())),
        Node::Ref { .. } => Err(AffineError::NotAffine("Ref".into())),
        Node::Exists { .. } => Err(AffineError::NotAffine("exists".into())),
        Node::NatRec { .. } => Err(AffineError::NotAffine("NatRec".into())),
        Node::State { .. } => Err(AffineError::NotAffine("嵌套 State".into())),
        Node::RecIndex => Err(AffineError::NotAffine("rec_index".into())),
    }
}

fn value_from_literal(value: &Literal, width: usize) -> Result<Value, AffineError> {
    let scalar = match value {
        Literal::F32(v) => Some(f64::from(*v)),
        Literal::F64(v) => Some(*v),
        Literal::I32(v) => Some(f64::from(*v)),
        Literal::I64(v) => Some(*v as f64),
        _ => None,
    };
    if let Some(value) = scalar {
        return Ok(Value {
            rows: vec![Row::constant(value, width)],
        });
    }
    let components: Option<Vec<f64>> = match value {
        Literal::Vec2(v) => Some(v.iter().map(|x| f64::from(*x)).collect()),
        Literal::Vec3(v) => Some(v.iter().map(|x| f64::from(*x)).collect()),
        Literal::Vec4(v) => Some(v.iter().map(|x| f64::from(*x)).collect()),
        _ => None,
    };
    match components {
        Some(components) => Ok(Value {
            rows: components
                .into_iter()
                .map(|v| Row::constant(v, width))
                .collect(),
        }),
        None => Err(AffineError::NotAffine("非数值字面量".into())),
    }
}

fn affine_call(name: &str, args: &[Expr], width: usize) -> Result<Value, AffineError> {
    match name {
        "add" => {
            let [a, b] = args else {
                return Err(AffineError::NotAffine(format!("add/{n}", n = args.len())));
            };
            let a = affine_value(a, width)?;
            let b = affine_value(b, width)?;
            add_values(a, b)
        }
        "sum" => {
            let mut acc: Option<Value> = None;
            for arg in args {
                let value = affine_value(arg, width)?;
                acc = Some(match acc {
                    Some(acc) => add_values(acc, value)?,
                    None => value,
                });
            }
            acc.ok_or_else(|| AffineError::NotAffine("空 sum".into()))
        }
        "scale" => {
            // `scale { k, of }`：k 必须是常量标量。
            let [k, of] = args else {
                return Err(AffineError::NotAffine(format!("scale/{n}", n = args.len())));
            };
            let k = const_scalar(k)?;
            Ok(scale_value(affine_value(of, width)?, k))
        }
        "mul" => {
            let [a, b] = args else {
                return Err(AffineError::NotAffine(format!("mul/{n}", n = args.len())));
            };
            match (const_scalar(a), const_scalar(b)) {
                (Ok(k), _) => Ok(scale_value(affine_value(b, width)?, k)),
                (_, Ok(k)) => Ok(scale_value(affine_value(a, width)?, k)),
                _ => Err(AffineError::Quadratic),
            }
        }
        "x" | "y" | "z" | "w" => {
            let index = match name {
                "x" => 0,
                "y" => 1,
                "z" => 2,
                _ => 3,
            };
            let [of] = args else {
                return Err(AffineError::NotAffine(format!("{name}/{}", args.len())));
            };
            let value = affine_value(of, width)?;
            value
                .rows
                .get(index)
                .cloned()
                .map(|row| Value { rows: vec![row] })
                .ok_or_else(|| AffineError::NotAffine(format!("`{name}` 越界")))
        }
        "vec2" | "vec3" | "vec4" => {
            let mut rows = Vec::with_capacity(args.len());
            for arg in args {
                let value = affine_value(arg, width)?;
                if value.rows.len() != 1 {
                    return Err(AffineError::WidthMismatch {
                        expected: 1,
                        actual: value.rows.len(),
                    });
                }
                rows.push(value.rows[0].clone());
            }
            Ok(Value { rows })
        }
        "vec3_from_vec2" => {
            let [of] = args else {
                return Err(AffineError::NotAffine("vec3_from_vec2".into()));
            };
            let mut value = affine_value(of, width)?;
            if value.rows.len() != 2 {
                return Err(AffineError::WidthMismatch {
                    expected: 2,
                    actual: value.rows.len(),
                });
            }
            value.rows.push(Row::constant(0.0, width));
            Ok(value)
        }
        "vec2_from_vec3" => {
            let [of] = args else {
                return Err(AffineError::NotAffine("vec2_from_vec3".into()));
            };
            let mut value = affine_value(of, width)?;
            value.rows.truncate(2);
            if value.rows.len() != 2 {
                return Err(AffineError::WidthMismatch {
                    expected: 2,
                    actual: value.rows.len(),
                });
            }
            Ok(value)
        }
        other => Err(AffineError::NotAffine(format!("算子 `{other}`"))),
    }
}

fn add_values(a: Value, b: Value) -> Result<Value, AffineError> {
    if a.rows.len() != b.rows.len() {
        return Err(AffineError::WidthMismatch {
            expected: a.rows.len(),
            actual: b.rows.len(),
        });
    }
    Ok(Value {
        rows: a
            .rows
            .iter()
            .zip(&b.rows)
            .map(|(x, y)| x.add(y))
            .collect(),
    })
}

fn scale_value(value: Value, k: f64) -> Value {
    Value {
        rows: value.rows.iter().map(|row| row.scaled(k)).collect(),
    }
}

/// 常量标量（只接受字面量与字面量的四则组合；非闭合即显式失败）。
fn const_scalar(expr: &Expr) -> Result<f64, AffineError> {
    match &expr.node {
        Node::Pure { value } => match value {
            Literal::F32(v) => Ok(f64::from(*v)),
            Literal::F64(v) => Ok(*v),
            Literal::I32(v) => Ok(f64::from(*v)),
            Literal::I64(v) => Ok(*v as f64),
            _ => Err(AffineError::NotAffine("非数值常量".into())),
        },
        Node::Call { op, args } => match op.name.as_str() {
            "add" => {
                let [a, b] = args.as_slice() else {
                    return Err(AffineError::NotAffine("add".into()));
                };
                Ok(const_scalar(a)? + const_scalar(b)?)
            }
            "sum" => args.iter().try_fold(0.0, |acc, arg| Ok(acc + const_scalar(arg)?)),
            "mul" => {
                let [a, b] = args.as_slice() else {
                    return Err(AffineError::NotAffine("mul".into()));
                };
                Ok(const_scalar(a)? * const_scalar(b)?)
            }
            "scale" => {
                let [k, of] = args.as_slice() else {
                    return Err(AffineError::NotAffine("scale".into()));
                };
                Ok(const_scalar(k)? * const_scalar(of)?)
            }
            other => Err(AffineError::NotAffine(format!("算子 `{other}`"))),
        },
        _ => Err(AffineError::NotAffine("非常量".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prev() -> Expr {
        Expr::rec_prev()
    }

    fn pure(v: f64) -> Expr {
        Expr::pure(v)
    }

    fn variable(index: usize) -> Expr {
        let name = ["x", "y", "z", "w"][index];
        Expr::call(ze_comb::OpRef::new("zela.math", name), [prev()])
    }

    fn add(a: Expr, b: Expr) -> Expr {
        Expr::call(ze_comb::OpRef::new("zela.math", "add"), [a, b])
    }

    fn mul(a: Expr, b: Expr) -> Expr {
        Expr::call(ze_comb::OpRef::new("zela.math", "mul"), [a, b])
    }

    fn scale(k: f64, of: Expr) -> Expr {
        Expr::call(
            ze_comb::OpRef::new("zela.math", "scale"),
            [pure(k), of],
        )
    }

    fn vec2(x: Expr, y: Expr) -> Expr {
        Expr::call(ze_comb::OpRef::new("zela.math", "vec2"), [x, y])
    }

    fn scalar(step: Expr) -> Result<AffineMap, AffineError> {
        recognize(&step, 1)
    }

    fn vector(step: Expr) -> Result<AffineMap, AffineError> {
        recognize(&step, 2)
    }

    /// 计划 §5.1 的 **6 个正例**。
    #[test]
    fn six_affine_forms_are_recognized() {
        // 1. prev + c
        let map = scalar(add(prev(), pure(2.0))).unwrap();
        assert_eq!((map.a(), map.b()), (&[vec![1.0]][..], &[2.0][..]));

        // 2. prev * c
        let map = scalar(mul(prev(), pure(0.5))).unwrap();
        assert_eq!((map.a(), map.b()), (&[vec![0.5]][..], &[0.0][..]));

        // 3. prev + c 与 prev * c 的线性组合
        let map = scalar(add(scale(0.5, prev()), pure(-1.0))).unwrap();
        assert_eq!((map.a(), map.b()), (&[vec![0.5]][..], &[-1.0][..]));

        // 4. 常量在前（mul 的两种书写顺序）
        let map = scalar(mul(pure(3.0), prev())).unwrap();
        assert_eq!((map.a(), map.b()), (&[vec![3.0]][..], &[0.0][..]));

        // 5. 一维仿射在二维状态上的投影：x' = 0.5·x + 1（y 行由 vec2 补齐）
        let map = vector(vec2(add(scale(0.5, variable(0)), pure(1.0)), variable(1))).unwrap();
        assert_eq!(map.a(), &[vec![0.5, 0.0], vec![0.0, 1.0]]);
        assert_eq!(map.b(), &[1.0, 0.0]);

        // 6. 二维互相线性引用（半隐式欧拉）：x' = x + v，v' = v − x
        let x = variable(0);
        let v = variable(1);
        let next_x = add(x.clone(), v.clone());
        let next_v = add(v, scale(-1.0, x));
        let map = vector(vec2(next_x, next_v)).unwrap();
        assert_eq!(map.a(), &[vec![1.0, 1.0], vec![-1.0, 1.0]]);
        assert_eq!(map.b(), &[0.0, 0.0]);
    }

    /// 计划 §5.1 的 **3 个反例**：显式失败，不是静默跳过。
    #[test]
    fn three_nonlinear_forms_fail_explicitly() {
        let sin = Expr::call(ze_comb::OpRef::new("zela.math", "sin"), [prev()]);
        assert!(matches!(scalar(sin), Err(AffineError::NotAffine(_))));

        let abs = Expr::call(ze_comb::OpRef::new("zela.math", "abs"), [prev()]);
        assert!(matches!(scalar(abs), Err(AffineError::NotAffine(_))));

        let square = mul(prev(), prev());
        assert!(matches!(scalar(square), Err(AffineError::Quadratic)));
    }

    /// 宽度不一致是显式错误（不按较小维度静默截断）。
    #[test]
    fn width_mismatch_is_explicit() {
        let step = vec2(variable(0), variable(1));
        assert_eq!(
            recognize(&step, 3),
            Err(AffineError::WidthMismatch {
                expected: 3,
                actual: 2
            })
        );
    }

    /// **BY-a2**：精确算术下 oracle 当然成立（整数振荡器，周期 6）。
    #[test]
    fn exact_arithmetic_agrees_with_replay() {
        let map = AffineMap {
            a: vec![vec![1.0, 1.0], vec![-1.0, 0.0]],
            b: vec![0.0, 0.0],
        };
        let init = [1.0, 0.0];
        assert!(map.replay_within(&init, 512, 0.0), "整数矩阵在 f64 下逐位相同");
    }

    /// **BY-a2 翻转定义**：非精确系数下两者**不**逐位相同，但仍在容差内 ⇒
    /// 定义仍是快速幂，重放退化为 oracle（不再是启用开关）。
    #[test]
    fn inexact_coefficients_are_within_the_oracle_tolerance() {
        let map = AffineMap {
            a: vec![vec![1.0, 0.25], vec![-0.01, 0.0]],
            b: vec![0.0, 0.0],
        };
        let init = [1.0, 0.0];
        assert!(
            !map.replay_within(&init, 512, 0.0),
            "先确认两者不是逐位相同（否则本用例没有区分力）"
        );
        assert!(
            map.replay_within(&init, 512, STATE_ORACLE_TOLERANCE),
            "容差 oracle 必须通过"
        );
    }

    /// `StatePlan`：无 predicate ⇒ `steps(n) = n`；`value_at` 走闭式定义。
    #[test]
    fn plan_value_is_the_closed_form() {
        let init = Expr::call(
            ze_comb::OpRef::new("zela.math", "vec2"),
            [pure(1.5), pure(0.0)],
        );
        let x = variable(0);
        let v = variable(1);
        let step = Expr::call(
            ze_comb::OpRef::new("zela.math", "vec2"),
            [
                add(x.clone(), v.clone()),
                add(scale(0.97, v), scale(-0.02, x)),
            ],
        );
        let plan = plan_from_expr(&init, &step, None).unwrap();
        assert_eq!(plan.shape(), StateShape::Vec2);
        assert_eq!(plan.steps_at(120), 120);
        let components = plan.map().apply(&[1.5, 0.0], 120);
        assert_eq!(
            plan.value_at(120),
            EvalValue::vec2([components[0] as f32, components[1] as f32])
        );
    }

    /// **BY-a5/a6**：predicate 的 `time` 解释为状态索引；自引用显式失败。
    #[test]
    fn state_predicate_uses_index_intervals() {
        let init = pure(0.0);
        // 稳定的一阶递推（|λ| = 0.5）：BZ-a 的谱半径门禁要求 |λ| < 1。
        let step = add(scale(0.5, prev()), pure(1.0));
        let predicate = Expr::call(
            ze_comb::OpRef::new("zela.math", "lt"),
            [
                Expr::channel(ze_comb::ChannelRef::builtin("time")),
                pure(3.0),
            ],
        );
        // `t < 3` ⇒ 索引 1 / 2 推进（索引 0 是 init，不参与转移）；第 n 帧最多 2 步。
        let plan = plan_from_expr(&init, &step, Some(&predicate)).unwrap();
        assert_eq!(plan.steps_at(0), 0);
        assert_eq!(plan.steps_at(2), 2);
        assert_eq!(plan.steps_at(3), 2);
        assert_eq!(plan.steps_at(100), 2);

        let self_ref = plan_from_expr(&init, &step, Some(&prev()));
        assert_eq!(self_ref, Err(AffineError::PredicateRefsState));
    }

    /// **BZ-a**：`|λ|max ≥ 1` ⇒ 装配期显式失败（不静默产出 inf/NaN）。
    ///
    /// 二维半隐式欧拉 `x' = x + v`、`v' = damp·v − stiff·x` 的谱半径是
    /// `sqrt(damp + stiff)`；`damp = 0.98`、`stiff = 0.5` ⇒ `sqrt(1.48) > 1`。
    #[test]
    fn unstable_affine_state_is_rejected() {
        let init = Expr::call(
            ze_comb::OpRef::new("zela.math", "vec2"),
            [pure(1.5), pure(0.0)],
        );
        let x = variable(0);
        let v = variable(1);
        let step = vec2(
            add(x.clone(), v.clone()),
            add(scale(0.98, v), scale(-0.5, x)),
        );
        match plan_from_expr(&init, &step, None) {
            Err(AffineError::Unstable { radius }) => {
                let expected = 1.48f64.sqrt();
                assert!((radius - expected).abs() < 1e-9, "谱半径 {radius}");
                let message = AffineError::Unstable { radius }.to_string();
                assert!(message.contains("谱半径"), "{message}");
                assert!(message.contains("damp + stiff"), "{message}");
            }
            other => panic!("期望 Unstable，实际：{other:?}"),
        }
    }

    /// **BZ-a R5**：边缘稳定（`|λ| = 1`）同样显式失败 —— 只统计到有限帧也会线性增长，
    /// 不是"近似稳定"，不能静默放过。
    #[test]
    fn marginal_affine_state_is_rejected() {
        let step = add(prev(), pure(1.0));
        match plan_from_expr(&pure(0.0), &step, None) {
            Err(AffineError::Unstable { radius }) => assert_eq!(radius, 1.0),
            other => panic!("期望 Unstable，实际：{other:?}"),
        }
    }

    /// **BZ-a**：现有弹簧作品（`0.97 / 0.02` ⇒ 谱半径 `sqrt(0.99)`）不受门禁影响。
    #[test]
    fn stable_spring_is_accepted() {
        let init = Expr::call(
            ze_comb::OpRef::new("zela.math", "vec2"),
            [pure(1.5), pure(0.0)],
        );
        let x = variable(0);
        let v = variable(1);
        let step = vec2(
            add(x.clone(), v.clone()),
            add(scale(0.97, v), scale(-0.02, x)),
        );
        let plan = plan_from_expr(&init, &step, None).unwrap();
        let radius = plan.map().spectral_radius();
        assert!((radius - 0.99f64.sqrt()).abs() < 1e-9, "谱半径 {radius}");
        assert!(plan
            .map()
            .apply(&[1.5, 0.0], 5000)
            .iter()
            .all(|component| component.is_finite()));
    }

    /// **BZ-d**：多实例共享一次矩阵幂；结果与逐个 `apply` **逐位相同**。
    #[test]
    fn apply_many_shares_one_power_table() {
        let map = AffineMap {
            a: vec![vec![1.0, 1.0], vec![-0.02, 0.97]],
            b: vec![0.0, 0.0],
        };
        let inits: Vec<Vec<f64>> = (0..8)
            .map(|i| vec![1.9 - 0.1 * i as f64, -0.1 + 0.02 * i as f64])
            .collect();
        let batched = map.apply_many(&inits, 500);
        for (init, value) in inits.iter().zip(&batched) {
            assert_eq!(*value, map.apply(init, 500), "批量与单点必须逐位相同");
        }
        // 同一初值重复 128 次 ⇒ 结果与单实例完全一致（幂表复用，不随 N 变化）。
        let wide = map.apply_many(&vec![inits[0].clone(); 128], 500);
        assert!(wide.iter().all(|value| value == &batched[0]));
    }

    /// **BZ-c**：事件 predicate ⇒ 依赖边含 `event`，值只随匹配的事件帧推进。
    #[test]
    fn event_predicate_uses_sparse_matched_frames() {
        let init = pure(0.0);
        let step = add(scale(0.5, prev()), pure(0.5));
        let predicate = Expr::channel(ze_comb::ChannelRef::builtin("event"));
        let plan = plan_from_expr(&init, &step, Some(&predicate)).unwrap();
        assert!(plan.event_gated());
        assert!(plan.dependencies().contains("event"), "{:?}", plan.dependencies());
        assert!(plan.event_predicate().is_some());
        // 帧 1 / 3 / 5 命中 ⇒ 第 0..6 帧的推进步数 = 0/1/1/2/2/3/3。
        let matched = [1i64, 3, 5];
        let expected = [0u64, 1, 1, 2, 2, 3, 3];
        for (frame, steps) in expected.into_iter().enumerate() {
            let value = plan.value_at_event(frame as i64, &matched);
            let components = plan.map().apply(&[0.0], steps);
            assert_eq!(value, EvalValue::f64(components[0]), "frame {frame}");
        }
    }

    /// **BZ-c**：事件 predicate 仍受"不得引用自身"门禁约束。
    #[test]
    fn event_predicate_still_rejects_self_reference() {
        let init = pure(0.0);
        let step = add(scale(0.5, prev()), pure(0.5));
        let predicate = add(
            Expr::channel(ze_comb::ChannelRef::builtin("event")),
            prev(),
        );
        assert_eq!(
            plan_from_expr(&init, &step, Some(&predicate)),
            Err(AffineError::PredicateRefsState)
        );
    }

    /// **BZ-c**：时间 predicate 的依赖边只含 `time`（保持 OLogN / OPerInterval）。
    #[test]
    fn time_predicate_dependencies_are_time_only() {
        let init = pure(0.0);
        let step = add(scale(0.5, prev()), pure(1.0));
        let predicate = Expr::call(
            ze_comb::OpRef::new("zela.math", "lt"),
            [
                Expr::channel(ze_comb::ChannelRef::builtin("time")),
                pure(3.0),
            ],
        );
        let plan = plan_from_expr(&init, &step, Some(&predicate)).unwrap();
        assert!(!plan.event_gated());
        assert_eq!(
            plan.dependencies().iter().cloned().collect::<Vec<_>>(),
            vec!["time".to_string()]
        );
    }

    /// 3 / 4 阶走特征多项式 + Durand–Kerner：对角矩阵的谱半径 = 最大对角元。
    #[test]
    fn spectral_radius_covers_higher_widths() {
        let step = Expr::call(
            ze_comb::OpRef::new("zela.math", "vec3"),
            [
                scale(0.5, variable(0)),
                scale(0.25, variable(1)),
                scale(0.75, variable(2)),
            ],
        );
        let map = recognize(&step, 3).unwrap();
        let radius = map.spectral_radius();
        assert!((radius - 0.75).abs() < 1e-9, "谱半径 {radius}");
    }

    /// **CB-c**：`State` 内部一律 f64；向量状态向 f32 只在构造 `EvalValue` 时降级**一次**。
    #[test]
    fn state_demotes_to_f32_only_at_the_eval_boundary() {
        let init = Expr::call(
            ze_comb::OpRef::new("zela.math", "vec2"),
            [pure(1.0), pure(2.0)],
        );
        let step = vec2(scale(0.9999, variable(0)), scale(0.9998, variable(1)));
        let plan = plan_from_expr(&init, &step, None).unwrap();
        let steps = 100u64;

        // 定义路径：f64 快速幂 → 边界处一次 `as f32`。
        let exact = plan.map().apply(&[1.0, 2.0], steps);
        let once = [exact[0] as f32, exact[1] as f32];

        // 反例：每一步都降级到 f32（错误实现）——本用例必须能区分两者。
        let a = plan.map().a()[0][0];
        let mut stepped = 1.0f32;
        for _ in 0..steps {
            stepped = (f64::from(stepped) * a) as f32;
        }
        assert_ne!(
            once[0], stepped,
            "用例必须有区分力：中间反复降级与边界一次降级结果不同"
        );

        assert_eq!(plan.value_at(steps as i64), EvalValue::vec2(once));
    }
}
