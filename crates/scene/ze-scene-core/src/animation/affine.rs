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

use ze_comb::{Expr, Literal, Node, Value as EvalValue};

use crate::animation::predicate::{IntervalSet, TIME_VARIABLE, analyze as analyze_predicate};

/// 识别器支持的最大状态宽度（与 `SCALAR_MAX_DIM` 同口径）。
pub const AFFINE_MAX_WIDTH: usize = 4;

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

    /// **增广矩阵幂**：`f^n(init)`，`O(k³ log n)`。
    ///
    /// **这是定义**（BY §2.2）：`State(n) := apply(init, n)`。
    /// 纯函数 ⇒ seek 与路径无关；只用 `+ − × ÷` ⇒ IEEE754 逐位可复现。
    ///
    /// 矩阵乘法的求和顺序由 [`mat_mul`] 的下标升序 `.sum()` 固定 ⇒ **不得**
    /// 改成并行 / 树形归约（那会改变舍入）。
    pub fn apply(&self, init: &[f64], n: u64) -> Vec<f64> {
        assert_eq!(init.len(), self.width(), "init 宽度必须与仿射图一致");
        // 增广 (k+1)×(k+1)：[[A, b], [0, 1]]
        let k = self.width();
        let mut base = vec![vec![0.0; k + 1]; k + 1];
        for i in 0..k {
            base[i][..k].copy_from_slice(&self.a[i]);
            base[i][k] = self.b[i];
        }
        base[k][k] = 1.0;
        let mut acc = identity(k + 1);
        let mut exp = n;
        let mut base = base;
        while exp > 0 {
            if exp & 1 == 1 {
                acc = mat_mul(&acc, &base);
            }
            exp >>= 1;
            if exp > 0 {
                base = mat_mul(&base, &base);
            }
        }
        (0..k)
            .map(|i| {
                (0..k).map(|j| acc[i][j] * init[j]).sum::<f64>() + acc[i][k]
            })
            .collect()
    }

    /// 逐次重放（**近似实现 / 测试 oracle**，不是定义）。
    pub fn replay(&self, init: &[f64], n: u64) -> Vec<f64> {
        let mut state = init.to_vec();
        for _ in 0..n {
            state = (0..self.width())
                .map(|i| {
                    (0..self.width())
                        .map(|j| self.a[i][j] * state[j])
                        .sum::<f64>()
                        + self.b[i]
                })
                .collect();
        }
        state
    }

    /// 在 `0..=horizon` 上比对闭式定义与重放（**容差内**，不是逐位）。
    ///
    /// BY §2.2：这是回归 oracle，不是"是否启用快速幂"的开关。
    pub fn replay_within(&self, init: &[f64], horizon: u64, tolerance: f64) -> bool {
        (0..=horizon).all(|n| {
            self.apply(init, n)
                .iter()
                .zip(self.replay(init, n))
                .all(|(a, b)| {
                    // NaN 永不相等（不能靠 NaN 绕过判据）。
                    !a.is_nan() && !b.is_nan() && (a - b).abs() <= tolerance
                })
        })
    }

    /// **谱半径** `|λ|max`（BZ-a：装配期稳定性门禁用）。
    ///
    /// 1 / 2 阶用解析式；3 / 4 阶由特征多项式（Faddeev–LeVerrier）+
    /// Durand–Kerner 求根（固定初值 / 固定迭代次数 ⇒ 确定性）。
    /// 只在装配期调用，**不在**每帧路径上。
    ///
    /// 非有限值（NaN / inf 来自非法常量）原样传播 ⇒ 调用方的 `< 1` 判定会拒绝。
    pub fn spectral_radius(&self) -> f64 {
        let k = self.width();
        match k {
            1 => self.a[0][0].abs(),
            2 => {
                let (a, b) = (self.a[0][0], self.a[0][1]);
                let (c, d) = (self.a[1][0], self.a[1][1]);
                let trace = a + d;
                let det = a * d - b * c;
                let disc = trace * trace - 4.0 * det;
                if disc >= 0.0 {
                    let root = disc.sqrt();
                    ((trace + root) / 2.0).abs().max(((trace - root) / 2.0).abs())
                } else {
                    // 共轭复根：模长 = sqrt(det)（此时 det > 0）。
                    det.abs().sqrt()
                }
            }
            _ => {
                let coefficients = characteristic_coefficients(&self.a);
                polynomial_root_moduli(&coefficients)
                    .into_iter()
                    .fold(0.0, f64::max)
            }
        }
    }
}

fn identity(n: usize) -> Vec<Vec<f64>> {
    let mut out = vec![vec![0.0; n]; n];
    for (i, row) in out.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    out
}

fn mat_mul(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    let mut out = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            out[i][j] = (0..n).map(|t| a[i][t] * b[t][j]).sum();
        }
    }
    out
}

/// 特征多项式 `λ^n + c[1]·λ^(n-1) + … + c[n]` 的系数（`c[0] = 1`）。
///
/// Faddeev–LeVerrier：`c_s = −tr(A·M_s) / s`，`M_s = A·M_(s−1) + c_(s−1)·I`，
/// `M_1 = I`。`k ≤ 4` ⇒ 常数开销。
fn characteristic_coefficients(a: &[Vec<f64>]) -> Vec<f64> {
    let n = a.len();
    let mut c = vec![0.0; n + 1];
    c[0] = 1.0;
    let mut m = identity(n);
    for s in 1..=n {
        if s > 1 {
            let mut next = mat_mul(a, &m);
            for (i, row) in next.iter_mut().enumerate() {
                row[i] += c[s - 1];
            }
            m = next;
        }
        let am = mat_mul(a, &m);
        let trace: f64 = (0..n).map(|i| am[i][i]).sum();
        c[s] = -trace / s as f64;
    }
    c
}

/// 极简复数（只用于求根；不引依赖）。
#[derive(Clone, Copy)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    const fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    fn sub(self, other: Self) -> Self {
        Self::new(self.re - other.re, self.im - other.im)
    }

    fn mul(self, other: Self) -> Self {
        Self::new(
            self.re * other.re - self.im * other.im,
            self.re * other.im + self.im * other.re,
        )
    }

    fn div(self, other: Self) -> Self {
        let denom = other.re * other.re + other.im * other.im;
        Self::new(
            (self.re * other.re + self.im * other.im) / denom,
            (self.im * other.re - self.re * other.im) / denom,
        )
    }

    fn modulus(self) -> f64 {
        self.re.hypot(self.im)
    }
}

/// 首一多项式的值（Horner）。
fn polynomial_at(coefficients: &[f64], z: Complex) -> Complex {
    let mut acc = Complex::new(0.0, 0.0);
    for &coefficient in coefficients {
        acc = acc.mul(z);
        acc.re += coefficient;
    }
    acc
}

/// Durand–Kerner 求根，返回各根的模长（首一多项式）。
///
/// 初值是固定的几何序列 ⇒ 同一输入永远给出同一输出（不依赖随机 / 迭代顺序）。
fn polynomial_root_moduli(coefficients: &[f64]) -> Vec<f64> {
    let n = coefficients.len() - 1;
    let seed = Complex::new(0.4, 0.9);
    let mut roots = Vec::with_capacity(n);
    let mut current = Complex::new(1.0, 0.0);
    for _ in 0..n {
        roots.push(current);
        current = current.mul(seed);
    }
    for _ in 0..500 {
        let mut max_delta: f64 = 0.0;
        for i in 0..n {
            let mut denom = Complex::new(1.0, 0.0);
            for (j, root) in roots.iter().enumerate() {
                if j != i {
                    denom = denom.mul(roots[i].sub(*root));
                }
            }
            if denom.modulus() == 0.0 {
                continue;
            }
            let delta = polynomial_at(coefficients, roots[i]).div(denom);
            roots[i] = roots[i].sub(delta);
            max_delta = max_delta.max(delta.modulus());
        }
        if max_delta <= 1e-14 {
            break;
        }
    }
    roots.iter().map(|root| root.modulus()).collect()
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
    pub fn steps_at(&self, frame: i64) -> u64 {
        let frame = frame.max(0) as u64;
        match &self.active {
            Some(set) => set.count_integers(1, frame as i64).max(0) as u64,
            None => frame,
        }
    }

    /// **定义**：第 `frame` 帧的状态值。
    pub fn value_at(&self, frame: i64) -> EvalValue {
        let steps = self.steps_at(frame);
        self.shape.value(&self.map.apply(&self.init, steps))
    }

    /// predicate 生效的索引集（`None` = 每步生效）。
    pub fn active(&self) -> Option<&IntervalSet> {
        self.active.as_ref()
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
    let active = match predicate {
        Some(predicate) => active_intervals(predicate)?,
        None => None,
    };
    Ok(StatePlan {
        map,
        init: init_values,
        shape,
        active,
    })
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
}
