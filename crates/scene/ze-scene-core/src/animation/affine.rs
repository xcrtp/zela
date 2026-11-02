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
//! # 与"逐位相同"的关系
//!
//! `AffineMap::apply` 是**优化路径**（增广矩阵快速幂）。IEEE754 下它与逐次重放
//! **不是同一个计算**：只有系数与中间值都能被精确表示时两者才逐位相同。
//! 因此运行时默认仍走重放（[`ze_comb`] 的折叠循环）；快速幂只在
//! [`AffineMap::matches_replay`] 验证通过的形态上可用（见 GAPS BX-02）。

use ze_comb::{Expr, Literal, Node};

/// 识别器支持的最大状态宽度（与 `SCALAR_MAX_DIM` 同口径）。
pub const AFFINE_MAX_WIDTH: usize = 4;

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
    /// 这是优化路径；与逐次重放是否逐位相同由 [`Self::matches_replay`] 判定。
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

    /// 逐次重放（**定义**）：`init` 迭代 `n` 次 `A·x + b`。
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

    /// 在 `0..=horizon` 上逐帧比对快速幂与重放（**逐位**相等）。
    ///
    /// 判据来自计划 §7.2：优化值是"定义"的前提是它与参考实现逐位相同。
    pub fn matches_replay(&self, init: &[f64], horizon: u64) -> bool {
        (0..=horizon).all(|n| self.apply(init, n) == self.replay(init, n))
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

/// 识别失败：**显式**错误（不是"静默当作非线性跳过"）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
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
    /// 识别出的仿射图 `(A, b)`。
    pub map: AffineMap,
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

    /// 快速幂与重放在**精确算术**下逐位相同（整数振荡器，周期 6）。
    #[test]
    fn fast_power_matches_replay_when_arithmetic_is_exact() {
        let map = AffineMap {
            a: vec![vec![1.0, 1.0], vec![-1.0, 0.0]],
            b: vec![0.0, 0.0],
        };
        let init = [1.0, 0.0];
        assert!(map.matches_replay(&init, 512), "整数矩阵在 f64 下应逐位相同");
    }

    /// 非精确系数下快速幂与重放**不**逐位相同 —— 这正是计划 §7.1 的风险。
    #[test]
    fn fast_power_diverges_on_inexact_coefficients() {
        let map = AffineMap {
            a: vec![vec![1.0, 0.25], vec![-0.01, 0.0]],
            b: vec![0.0, 0.0],
        };
        let init = [1.0, 0.0];
        assert!(
            !map.matches_replay(&init, 512),
            "非二进制可表示系数必须被识别为不适用（运行时走重放）"
        );
    }
}
