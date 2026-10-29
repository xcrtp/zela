//! `zela.math` 的**标量算子语义**（纯函数，无状态）。
//!
//! # 语义按**注册表查表**分派（阶段 4）
//!
//! 旧形态是 `match op.name { "sin" => …, … }`：加一个算子要在两个文件各改一处，
//! 漏改只有运行期才知道。现在目录（`op.rs` 的 `math_ops!`）是**唯一登记点**，
//! 本模块的每个函数只是被它引用的实现体；`eval` 不再有 `match`。
//!
//! # 边界
//!
//! 只做标量 / 向量（`f64` / `Vec2` / `Vec3` / `Vec4` 的算术子集）。路径等私有类型
//! 走 `Value::Bytes` + 命名类型。错误里带算子限定名，便于把「脚本作者看到的写法」
//! 映射回「函子库里的那一条签名」。

use ze_comb::{EvalError, OpRef, Value};

use std::sync::OnceLock;

use ze_reflect::{Blame, SymbolTable};

/// 算子名 → 符号（`OnceLock`：只建一次）。
fn index() -> &'static SymbolTable<&'static str, &'static str> {
    static INDEX: OnceLock<SymbolTable<&'static str, &'static str>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut table = SymbolTable::new();
        for &name in crate::op::OPS {
            table
                .intern(name, name, Blame::Assembly)
                .expect("OPS 的名字唯一");
        }
        table
    })
}

/// `op` 是否属于本库（namespace 必须一致，名字在 [`crate::op::OPS`] 里）。
///
/// 名字查找是 **O(1)**：符号表在首次查询时建立一次，之后是整数槽位。
pub fn is_known(op: &OpRef) -> bool {
    op.namespace == crate::LIBRARY_NAMESPACE && index().resolve(op.name.as_str()).is_some()
}

/// 算子求值：**查表分派**（没有 `match`；未知算子显式失败）。
///
/// 未知算子 / 实参类型不符 ⇒ [`EvalError`]，不静默返回 0。
pub fn eval(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let entry = crate::op::eval_entry(&op.name)
        .filter(|_| op.namespace == crate::LIBRARY_NAMESPACE);
    match entry {
        Some(eval) => eval(op, args),
        None => Err(EvalError::op(op, format!("未知算子 `{}`", op.name))),
    }
}

// ─── 实参提取（每个算子只做自己那一件事）─────────────────────────────────

fn number(op: &OpRef, args: &[Value], index: usize) -> Result<f64, EvalError> {
    args.get(index)
        .and_then(Value::as_f64)
        .ok_or_else(|| EvalError::op(op, format!("第 {index} 个实参不是数值")))
}

fn boolean(op: &OpRef, args: &[Value], index: usize) -> Result<bool, EvalError> {
    args.get(index)
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            EvalError::op(
                op,
                format!("第 {index} 个实参不是布尔（Bool 由比较算子产出）"),
            )
        })
}

fn vec2_arg(op: &OpRef, args: &[Value], index: usize) -> Result<[f32; 2], EvalError> {
    args.get(index)
        .and_then(Value::as_vec2)
        .ok_or_else(|| EvalError::op(op, format!("第 {index} 个实参不是 Vec2")))
}

fn vec3_arg(op: &OpRef, args: &[Value], index: usize) -> Result<[f32; 3], EvalError> {
    args.get(index)
        .and_then(Value::as_vec3)
        .ok_or_else(|| EvalError::op(op, format!("第 {index} 个实参不是 Vec3")))
}

// ─── 实现体（`op.rs` 的目录逐个引用它们）────────────────────────────────

/// `sin(x)`。
pub fn sin(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::f64(number(op, args, 0)?.sin()))
}

/// `cos(x)`。
pub fn cos(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::f64(number(op, args, 0)?.cos()))
}

/// `add(a, b)`：同型加法（数值 / `Vec2` / `Vec3`）。
///
/// AU-11：签名表早就声明了 `SAME_VEC` 重载，但求值只吃标量 ⇒ 装配期能过、
/// 运行期报错。现在两侧对齐：**同型才算**，混型是显式错误（不做隐式提升）。
pub fn add(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let a = args.first().ok_or_else(|| EvalError::op(op, "需要两个实参"))?;
    let b = args.get(1).ok_or_else(|| EvalError::op(op, "需要两个实参"))?;
    if let (Some(a), Some(b)) = (a.as_vec2(), b.as_vec2()) {
        return Ok(Value::vec2([a[0] + b[0], a[1] + b[1]]));
    }
    if let (Some(a), Some(b)) = (a.as_vec3(), b.as_vec3()) {
        return Ok(Value::vec3([a[0] + b[0], a[1] + b[1], a[2] + b[2]]));
    }
    if let (Some(a), Some(b)) = (a.as_f64(), b.as_f64()) {
        return Ok(Value::f64(a + b));
    }
    Err(EvalError::op(
        op,
        "两个实参必须同型（数值 / Vec2 / Vec3；不做隐式提升）",
    ))
}

/// `mul(a, b)`：数值 × 数值 / 向量 × 数值 / 数值 × 向量。
pub fn mul(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    scale(op, args)
}

/// `scale(k, of)`：`k` 在产物里是字面量叶子，这里只是普通乘法。
///
/// 向量侧按 **f32** 逐分量相乘（向量分量本来就是 f32，`AU-04` 的口径）。
pub fn scale(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let a = args.first().ok_or_else(|| EvalError::op(op, "需要两个实参"))?;
    let b = args.get(1).ok_or_else(|| EvalError::op(op, "需要两个实参"))?;
    if let (Some(vector), Some(k)) = (a.as_vec2(), b.as_f64()) {
        let k = k as f32;
        return Ok(Value::vec2([vector[0] * k, vector[1] * k]));
    }
    if let (Some(k), Some(vector)) = (a.as_f64(), b.as_vec2()) {
        let k = k as f32;
        return Ok(Value::vec2([vector[0] * k, vector[1] * k]));
    }
    if let (Some(vector), Some(k)) = (a.as_vec3(), b.as_f64()) {
        let k = k as f32;
        return Ok(Value::vec3([vector[0] * k, vector[1] * k, vector[2] * k]));
    }
    if let (Some(k), Some(vector)) = (a.as_f64(), b.as_vec3()) {
        let k = k as f32;
        return Ok(Value::vec3([vector[0] * k, vector[1] * k, vector[2] * k]));
    }
    if let (Some(a), Some(b)) = (a.as_f64(), b.as_f64()) {
        return Ok(Value::f64(a * b));
    }
    Err(EvalError::op(
        op,
        "需要「数值 × 数值」或「向量 × 数值」（同型；不做隐式提升）",
    ))
}

/// `sum(terms…)`：任意多项**同型**求和（空 ⇒ `0.0`）。
///
/// `Vec2` / `Vec3` 按 f32 逐分量累加；混型是显式错误（不静默跳过）。
pub fn sum(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Ok(Value::f64(0.0));
    }
    if args[0].as_vec2().is_some() {
        let mut total = [0.0_f32; 2];
        for index in 0..args.len() {
            let v = vec2_arg(op, args, index)?;
            total[0] += v[0];
            total[1] += v[1];
        }
        return Ok(Value::vec2(total));
    }
    if args[0].as_vec3().is_some() {
        let mut total = [0.0_f32; 3];
        for index in 0..args.len() {
            let v = vec3_arg(op, args, index)?;
            total[0] += v[0];
            total[1] += v[1];
            total[2] += v[2];
        }
        return Ok(Value::vec3(total));
    }
    let mut total = 0.0;
    for index in 0..args.len() {
        total += number(op, args, index)?;
    }
    Ok(Value::f64(total))
}

/// `mix(a, b, t)`：线性插值（数值 / Vec2 / Vec3 同型）。
pub fn mix(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let t = number(op, args, 2)?;
    let a = args.first().ok_or_else(|| EvalError::op(op, "需要三个实参"))?;
    let b = args.get(1).ok_or_else(|| EvalError::op(op, "需要三个实参"))?;
    // **端点返回原值**：`a + (b − a) × 1` 在 f32 上可能偏离 `b` 一个 ulp。
    if t == 0.0 {
        return Ok(a.clone());
    }
    if t == 1.0 {
        return Ok(b.clone());
    }
    // 标量走 **f32 插值**：与旧 `Curve::Keyframes`（`AnimValue::Scalar`）的
    // `a + (b - a) * α` 逐位一致（第 5a 批「先证明等价」的硬要求）。
    if let (Some(a), Some(b)) = (a.as_f64(), b.as_f64()) {
        return Ok(Value::f64(f64::from(lerp_f32(a as f32, b as f32, t))));
    }
    if let (Some(a), Some(b)) = (a.as_vec2(), b.as_vec2()) {
        return Ok(Value::vec2([
            lerp_f32(a[0], b[0], t),
            lerp_f32(a[1], b[1], t),
        ]));
    }
    if let (Some(a), Some(b)) = (a.as_vec3(), b.as_vec3()) {
        return Ok(Value::vec3([
            lerp_f32(a[0], b[0], t),
            lerp_f32(a[1], b[1], t),
            lerp_f32(a[2], b[2], t),
        ]));
    }
    Err(EvalError::op(
        op,
        "两个端点必须是同型数值 / 向量（不做隐式提升）",
    ))
}

/// `clamp(x, lo, hi)`：`lo > hi` 时按 `lo` 优先（不 panic）。
pub fn clamp(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let x = number(op, args, 0)?;
    let lo = number(op, args, 1)?;
    let hi = number(op, args, 2)?;
    Ok(Value::f64(if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }))
}

/// `vec2(x, y)`：两个标量合成 `Vec2`。
pub fn vec2(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::vec2([
        number(op, args, 0)? as f32,
        number(op, args, 1)? as f32,
    ]))
}

/// `vec3(x, y, z)`：三个标量合成 `Vec3`。
pub fn vec3(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::vec3([
        number(op, args, 0)? as f32,
        number(op, args, 1)? as f32,
        number(op, args, 2)? as f32,
    ]))
}

/// 分量访问的公共实现（`x` / `y` / `z` 只差 `index`）。
fn component(op: &OpRef, args: &[Value], index: usize) -> Result<Value, EvalError> {
    if let Some(v) = args.first().and_then(Value::as_vec2) {
        if index == 2 {
            return Err(EvalError::op(op, "Vec2 没有 z 分量（请先 vec3 升维）"));
        }
        return Ok(Value::f64(f64::from(v[index])));
    }
    if let Some(v) = args.first().and_then(Value::as_vec3) {
        return Ok(Value::f64(f64::from(v[index])));
    }
    Err(EvalError::op(op, "第 0 个实参不是向量"))
}

/// `x(v)`：向量 x 分量。
pub fn x(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    component(op, args, 0)
}

/// `y(v)`：向量 y 分量。
pub fn y(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    component(op, args, 1)
}

/// `z(v)`：向量 z 分量（`Vec2` 上显式报错）。
pub fn z(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    component(op, args, 2)
}

/// `rotate3(v, axis, angle)`：罗德里格斯（与旧 `Curve::Orbit` 同一公式）。
pub fn rotate3(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let v = vec3_arg(op, args, 0)?;
    let axis = vec3_arg(op, args, 1)?;
    let angle = number(op, args, 2)?;
    Ok(Value::vec3(rotate_rodrigues(
        [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])],
        [
            f64::from(axis[0]),
            f64::from(axis[1]),
            f64::from(axis[2]),
        ],
        angle,
    )))
}

/// `splat(v)`：一个标量铺成三个分量（`F64 → Vec3`）。
pub fn splat(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let v = number(op, args, 0)? as f32;
    Ok(Value::vec3([v, v, v]))
}

/// `vec3_from_vec2(v, z)`：显式补 z（不猜 0）。
pub fn vec3_from_vec2(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let v = vec2_arg(op, args, 0)?;
    Ok(Value::vec3([v[0], v[1], number(op, args, 1)? as f32]))
}

/// `vec2_from_vec3(v)`：**显式**丢弃 z（不是静默截断 —— 算子名写明了）。
pub fn vec2_from_vec3(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let v = vec3_arg(op, args, 0)?;
    Ok(Value::vec2([v[0], v[1]]))
}

/// `length(v)`：向量模长。
pub fn length(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    if let Some(v) = args.first().and_then(Value::as_vec2) {
        return Ok(Value::f64(f64::from(
            (v[0] * v[0] + v[1] * v[1]).sqrt(),
        )));
    }
    if let Some(v) = args.first().and_then(Value::as_vec3) {
        return Ok(Value::f64(f64::from(
            (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt(),
        )));
    }
    Err(EvalError::op(op, "第 0 个实参不是向量"))
}

/// `normalize(v)`：单位向量（零向量保持零，不产出 NaN）。
pub fn normalize(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    if let Some(v) = args.first().and_then(Value::as_vec2) {
        let length = (v[0] * v[0] + v[1] * v[1]).sqrt();
        return Ok(Value::vec2(unit2(v, length)));
    }
    if let Some(v) = args.first().and_then(Value::as_vec3) {
        let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        return Ok(Value::vec3(unit3(v, length)));
    }
    Err(EvalError::op(op, "第 0 个实参不是向量"))
}

/// `round_f32(x)`：把数值落成 f32 再升回 f64（旧曲线的逐位等价工具）。
pub fn round_f32(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::f64(f64::from(number(op, args, 0)? as f32)))
}

/// `distance(a, b)`：两点距离（`Vector, Vector → F64`；两实参必须同维）。
pub fn distance(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    if let (Some(a), Some(b)) = (
        args.first().and_then(Value::as_vec2),
        args.get(1).and_then(Value::as_vec2),
    ) {
        let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
        return Ok(Value::f64(f64::from((dx * dx + dy * dy).sqrt())));
    }
    if let (Some(a), Some(b)) = (
        args.first().and_then(Value::as_vec3),
        args.get(1).and_then(Value::as_vec3),
    ) {
        let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
        return Ok(Value::f64(f64::from(
            (dx * dx + dy * dy + dz * dz).sqrt(),
        )));
    }
    Err(EvalError::op(op, "需要两个同维向量（Vec2 或 Vec3）"))
}

/// `smoothstep(e0, e1, x)`：Hermite 平滑（与 WGSL 内建 `smoothstep` 同式）。
///
/// `e0 == e1` 时 `(x - e0) / 0` 是 ±inf / NaN，`clamp` 后得到与 shader 一致的结果
/// （不做特判：特判会让 CPU 与 GPU 出现两条不同公式）。
pub fn smoothstep(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let e0 = number(op, args, 0)? as f32;
    let e1 = number(op, args, 1)? as f32;
    let x = number(op, args, 2)? as f32;
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    Ok(Value::f64(f64::from(t * t * (3.0 - 2.0 * t))))
}

/// `fract(x)`：小数部分（`x - floor(x)`，与 WGSL `fract` 同式）。
pub fn fract(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let x = number(op, args, 0)? as f32;
    Ok(Value::f64(f64::from(x - x.floor())))
}

/// `lt(a, b)`。
pub fn lt(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::Bool(
        number(op, args, 0)? < number(op, args, 1)?,
    ))
}

/// `gt(a, b)`。
pub fn gt(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::Bool(
        number(op, args, 0)? > number(op, args, 1)?,
    ))
}

/// `le(a, b)`。
pub fn le(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::Bool(
        number(op, args, 0)? <= number(op, args, 1)?,
    ))
}

/// `ge(a, b)`。
pub fn ge(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::Bool(
        number(op, args, 0)? >= number(op, args, 1)?,
    ))
}

/// `eq` / `ne` 的公共实现（数值或布尔，不做隐式真值转换）。
fn equal(op: &OpRef, args: &[Value]) -> Result<bool, EvalError> {
    match (args.first(), args.get(1)) {
        (Some(a), Some(b)) => match (a.as_bool(), b.as_bool()) {
            (Some(a), Some(b)) => Ok(a == b),
            _ => match (a.as_f64(), b.as_f64()) {
                (Some(a), Some(b)) => Ok(a == b),
                _ => Err(EvalError::op(
                    op,
                    "只支持数值或布尔比较（不做隐式真值转换）",
                )),
            },
        },
        _ => Err(EvalError::op(op, "需要两个实参")),
    }
}

/// `eq(a, b)`。
pub fn eq(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::Bool(equal(op, args)?))
}

/// `ne(a, b)`。
pub fn ne(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::Bool(!equal(op, args)?))
}

/// `and(a, b)`。
pub fn and(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::Bool(
        boolean(op, args, 0)? && boolean(op, args, 1)?,
    ))
}

/// `or(a, b)`。
pub fn or(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::Bool(
        boolean(op, args, 0)? || boolean(op, args, 1)?,
    ))
}

/// `not(a)`。
pub fn not(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    Ok(Value::Bool(!boolean(op, args, 0)?))
}

// ─── 数值helper ─────────────────────────────────────────────────────────

/// 线性插值（**f32 域**）。
///
/// `α` 先落 f32 再参与运算，与 `ze-time` 的 `Curve::Keyframes` 同一精度口径。
pub(crate) fn lerp_f32(a: f32, b: f32, t: f64) -> f32 {
    a + (b - a) * (t as f32)
}

/// `Vec2` 单位化：零长度保持零（与 `glam::normalize_or_zero` 同一约定，不产出 NaN）。
pub(crate) fn unit2(v: [f32; 2], length: f32) -> [f32; 2] {
    if length > 1e-12 {
        [v[0] / length, v[1] / length]
    } else {
        [0.0, 0.0]
    }
}

/// `Vec3` 单位化：零长度保持零。
pub(crate) fn unit3(v: [f32; 3], length: f32) -> [f32; 3] {
    if length > 1e-12 {
        [v[0] / length, v[1] / length, v[2] / length]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// 罗德里格斯旋转：`v cosθ + (axis × v) sinθ + axis (axis·v)(1 − cosθ)`。
///
/// `axis` 先归一化；零向量轴退回 `Y`（与旧 `Curve::Orbit` 的退化处理一致）。
pub(crate) fn rotate_rodrigues(v: [f64; 3], axis: [f64; 3], angle: f64) -> [f32; 3] {
    let length = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let axis = if length > 1e-12 {
        [axis[0] / length, axis[1] / length, axis[2] / length]
    } else {
        [0.0, 1.0, 0.0]
    };
    let (sin, cos) = angle.sin_cos();
    let dot = axis[0] * v[0] + axis[1] * v[1] + axis[2] * v[2];
    let cross = [
        axis[1] * v[2] - axis[2] * v[1],
        axis[2] * v[0] - axis[0] * v[2],
        axis[0] * v[1] - axis[1] * v[0],
    ];
    [
        (v[0] * cos + cross[0] * sin + axis[0] * dot * (1.0 - cos)) as f32,
        (v[1] * cos + cross[1] * sin + axis[1] * dot * (1.0 - cos)) as f32,
        (v[2] * cos + cross[2] * sin + axis[2] * dot * (1.0 - cos)) as f32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn math(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    #[test]
    fn scalar_operators_match_std_math() {
        let x = Value::f64(0.5);
        assert_eq!(
            eval(&math("sin"), std::slice::from_ref(&x)).unwrap(),
            Value::f64(0.5_f64.sin())
        );
        assert_eq!(
            eval(&math("mul"), &[x.clone(), Value::f64(3.0)]).unwrap(),
            Value::f64(1.5)
        );
        assert_eq!(
            eval(&math("scale"), &[Value::f64(4.0), x.clone()]).unwrap(),
            Value::f64(2.0)
        );
        assert_eq!(
            eval(&math("sum"), &[Value::f64(1.0), x, Value::f64(2.0)]).unwrap(),
            Value::f64(3.5)
        );
        assert_eq!(eval(&math("sum"), &[]).unwrap(), Value::f64(0.0));
    }

    /// 第 1 批（算子补齐）：`mix` 覆盖数值 / Vec2 / Vec3，且不做隐式提升。
    #[test]
    fn mix_interpolates_numbers_and_vectors() {
        assert_eq!(
            eval(
                &math("mix"),
                &[Value::f64(0.0), Value::f64(10.0), Value::f64(0.25)]
            )
            .unwrap(),
            Value::f64(2.5)
        );
        assert_eq!(
            eval(
                &math("mix"),
                &[
                    Value::vec2([0.0, -2.0]),
                    Value::vec2([4.0, 2.0]),
                    Value::f64(0.5),
                ]
            )
            .unwrap(),
            Value::vec2([2.0, 0.0])
        );
        assert_eq!(
            eval(
                &math("mix"),
                &[
                    Value::vec3([0.0, 0.0, 0.0]),
                    Value::vec3([2.0, 4.0, 6.0]),
                    Value::f64(0.5),
                ]
            )
            .unwrap(),
            Value::vec3([1.0, 2.0, 3.0])
        );
        // 端点混型 ⇒ 显式错误（不静默当真值提升）。
        let err = eval(
            &math("mix"),
            &[
                Value::f64(0.0),
                Value::vec2([1.0, 1.0]),
                Value::f64(0.5),
            ],
        )
        .unwrap_err();
        assert!(matches!(err, EvalError::Op { ref op, .. } if op == "zela.math::mix"));
    }

    /// **AU-11**：签名里声明的向量重载必须真有实现（同型才算，混型显式失败）。
    #[test]
    fn vector_overloads_are_implemented() {
        let v2 = Value::vec2([1.0, 2.0]);
        let w2 = Value::vec2([3.0, 4.0]);
        assert_eq!(
            eval(&math("add"), &[v2.clone(), w2]).unwrap(),
            Value::vec2([4.0, 6.0])
        );

        let v3 = Value::vec3([1.0, 2.0, 3.0]);
        assert_eq!(
            eval(&math("add"), &[v3.clone(), Value::vec3([4.0, 5.0, 6.0])]).unwrap(),
            Value::vec3([5.0, 7.0, 9.0])
        );

        // `mul` / `scale`：向量 × 数值 与 数值 × 向量 两侧都成立（f32 域）。
        assert_eq!(
            eval(&math("scale"), &[v2.clone(), Value::f64(2.0)]).unwrap(),
            Value::vec2([2.0, 4.0])
        );
        assert_eq!(
            eval(&math("mul"), &[Value::f64(0.5), v2.clone()]).unwrap(),
            Value::vec2([0.5, 1.0])
        );
        assert_eq!(
            eval(&math("scale"), &[Value::f64(2.0), v3.clone()]).unwrap(),
            Value::vec3([2.0, 4.0, 6.0])
        );
        assert_eq!(
            eval(&math("mul"), &[v3.clone(), Value::f64(0.5)]).unwrap(),
            Value::vec3([0.5, 1.0, 1.5])
        );

        // `sum`：同型逐分量累加；空序列仍是标量零元（与 AU-11 之前的唯一形态一致）。
        assert_eq!(
            eval(&math("sum"), &[v2.clone(), v2.clone()]).unwrap(),
            Value::vec2([2.0, 4.0])
        );
        assert_eq!(
            eval(&math("sum"), &[v3.clone(), v3]).unwrap(),
            Value::vec3([2.0, 4.0, 6.0])
        );
        assert_eq!(eval(&math("sum"), &[]).unwrap(), Value::f64(0.0));

        // 混型 ⇒ 显式错误（不做隐式提升，也不静默跳过）。
        assert!(eval(&math("add"), &[v2.clone(), Value::f64(1.0)]).is_err());
        assert!(eval(&math("scale"), &[v2.clone(), v2]).is_err());
        assert!(eval(&math("sum"), &[Value::vec2([1.0, 2.0]), Value::f64(1.0)]).is_err());
    }

    #[test]
    fn clamp_bounds_both_ends() {
        let clamp = |x: f64| {
            eval(
                &math("clamp"),
                &[Value::f64(x), Value::f64(0.0), Value::f64(1.0)],
            )
            .unwrap()
        };
        assert_eq!(clamp(-0.5), Value::f64(0.0));
        assert_eq!(clamp(0.25), Value::f64(0.25));
        assert_eq!(clamp(2.0), Value::f64(1.0));
    }

    /// 第 1 批：`vec2` / `vec3` 显式构造，`x|y|z` 取分量（`Vec2` 上取 z 是显式错误）。
    #[test]
    fn vector_constructors_and_components() {
        let v2 = eval(&math("vec2"), &[Value::f64(1.0), Value::f64(2.0)]).unwrap();
        assert_eq!(v2, Value::vec2([1.0, 2.0]));
        assert_eq!(eval(&math("x"), &[v2.clone()]).unwrap(), Value::f64(1.0));
        assert_eq!(eval(&math("y"), &[v2.clone()]).unwrap(), Value::f64(2.0));
        assert!(eval(&math("z"), &[v2]).is_err(), "Vec2 没有 z 分量");

        let v3 = eval(
            &math("vec3"),
            &[Value::f64(1.0), Value::f64(2.0), Value::f64(3.0)],
        )
        .unwrap();
        assert_eq!(v3, Value::vec3([1.0, 2.0, 3.0]));
        assert_eq!(eval(&math("z"), &[v3]).unwrap(), Value::f64(3.0));
    }

    /// `rotate3` 与旧 `Curve::Orbit` 同一公式：绕 Y 转 90° ⇒ `(0,0,r) → (r,0,0)`。
    #[test]
    fn rotate3_matches_the_orbit_convention() {
        let rotated = eval(
            &math("rotate3"),
            &[
                Value::vec3([0.0, 0.0, 5.0]),
                Value::vec3([0.0, 1.0, 0.0]),
                Value::f64(std::f64::consts::FRAC_PI_2),
            ],
        )
        .unwrap();
        let Some(v) = rotated.as_vec3() else {
            panic!("rotate3 必须返回 Vec3");
        };
        assert!((v[0] - 5.0).abs() < 1e-4 && v[1].abs() < 1e-4 && v[2].abs() < 1e-4, "{v:?}");
    }

    /// 第 3 批（类型系统扩展）：转换算子族 —— 升降维是**显式**的，长度/单位化覆盖两种向量。
    #[test]
    fn conversion_operators_are_explicit() {
        assert_eq!(
            eval(&math("splat"), &[Value::f64(2.0)]).unwrap(),
            Value::vec3([2.0, 2.0, 2.0])
        );
        assert_eq!(
            eval(
                &math("vec3_from_vec2"),
                &[Value::vec2([1.0, 2.0]), Value::f64(3.0)]
            )
            .unwrap(),
            Value::vec3([1.0, 2.0, 3.0])
        );
        assert_eq!(
            eval(&math("vec2_from_vec3"), &[Value::vec3([4.0, 5.0, 6.0])]).unwrap(),
            Value::vec2([4.0, 5.0]),
            "vec2_from_vec3 显式丢弃 z"
        );

        let length = eval(&math("length"), &[Value::vec3([3.0, 4.0, 0.0])]).unwrap();
        assert_eq!(length, Value::f64(5.0));
        let unit = eval(&math("normalize"), &[Value::vec2([0.0, 3.0])]).unwrap();
        assert_eq!(unit, Value::vec2([0.0, 1.0]));
        // 零向量保持零（不产出 NaN）。
        assert_eq!(
            eval(&math("normalize"), &[Value::vec3([0.0, 0.0, 0.0])]).unwrap(),
            Value::vec3([0.0, 0.0, 0.0])
        );

        // 类型不符是显式错误（不做隐式升降维）。
        assert!(
            eval(
                &math("vec3_from_vec2"),
                &[Value::vec3([1.0, 2.0, 3.0]), Value::f64(0.0)]
            )
            .is_err()
        );
        assert!(eval(&math("length"), &[Value::f64(1.0)]).is_err());
    }

    /// 第 5a 批：`round_f32` 把数值落成 f32 再升回（逐位复刻旧曲线的精度口径）。
    #[test]
    fn round_f32_lands_on_the_f32_grid() {
        let value = eval(&math("round_f32"), &[Value::f64(0.1 + 0.2)]).unwrap();
        assert_eq!(value, Value::f64(f64::from((0.1_f64 + 0.2_f64) as f32)));
    }

    /// 阶段 3 的内建 mono：`distance` / `smoothstep` / `fract`。
    #[test]
    fn builtin_monos_work_in_both_dimensions() {
        // 3-4-5 直角三角形：Vec2 与 Vec3 走同一公式。
        assert_eq!(
            eval(
                &math("distance"),
                &[Value::vec2([0.0, 0.0]), Value::vec2([3.0, 4.0])]
            )
            .unwrap(),
            Value::f64(5.0)
        );
        assert_eq!(
            eval(
                &math("distance"),
                &[Value::vec3([0.0, 0.0, 0.0]), Value::vec3([3.0, 4.0, 0.0])]
            )
            .unwrap(),
            Value::f64(5.0)
        );
        assert!(
            eval(
                &math("distance"),
                &[Value::vec2([0.0, 0.0]), Value::vec3([3.0, 4.0, 0.0])]
            )
            .is_err(),
            "两实参必须同维"
        );

        // smoothstep：端点夹住、中点对称。
        assert_eq!(
            eval(
                &math("smoothstep"),
                &[Value::f64(0.0), Value::f64(1.0), Value::f64(-1.0)]
            )
            .unwrap(),
            Value::f64(0.0)
        );
        assert_eq!(
            eval(
                &math("smoothstep"),
                &[Value::f64(0.0), Value::f64(1.0), Value::f64(2.0)]
            )
            .unwrap(),
            Value::f64(1.0)
        );
        let mid = eval(
            &math("smoothstep"),
            &[Value::f64(0.0), Value::f64(1.0), Value::f64(0.5)],
        )
        .unwrap();
        assert_eq!(mid, Value::f64(0.5), "中点应是 0.5");

        // fract：负数靠 floor（-0.25 → 0.75），与 WGSL `fract` 同式。
        assert_eq!(
            eval(&math("fract"), &[Value::f64(2.25)]).unwrap(),
            Value::f64(0.25)
        );
        assert_eq!(
            eval(&math("fract"), &[Value::f64(-0.25)]).unwrap(),
            Value::f64(0.75)
        );
    }

    #[test]
    fn type_errors_name_the_operator() {
        let err = eval(&math("sin"), &[Value::text("x")]).unwrap_err();
        assert_eq!(
            err,
            EvalError::Op {
                op: "zela.math::sin".to_string(),
                message: "第 0 个实参不是数值".to_string(),
            }
        );
    }

    #[test]
    fn unknown_operator_is_explicit() {
        let err = eval(&math("tan"), &[Value::f64(1.0)]).unwrap_err();
        assert!(matches!(err, EvalError::Op { ref op, .. } if op == "zela.math::tan"));
    }

    /// 比较算子产出 `Bool`（predicate 的原料）；顺序与边界都按 IEEE 比较。
    #[test]
    fn comparisons_yield_bool() {
        let one = Value::f64(1.0);
        let two = Value::f64(2.0);
        assert_eq!(eval(&math("lt"), &[one.clone(), two.clone()]).unwrap(), Value::Bool(true));
        assert_eq!(eval(&math("gt"), &[one.clone(), two.clone()]).unwrap(), Value::Bool(false));
        assert_eq!(eval(&math("le"), &[two.clone(), two.clone()]).unwrap(), Value::Bool(true));
        assert_eq!(eval(&math("ge"), &[one.clone(), two.clone()]).unwrap(), Value::Bool(false));
        assert_eq!(eval(&math("eq"), &[two.clone(), two.clone()]).unwrap(), Value::Bool(true));
        assert_eq!(eval(&math("ne"), &[one, two]).unwrap(), Value::Bool(true));
    }

    /// 布尔组合子只接受 Bool：不做 `0 = false` 的隐式真值转换（U2）。
    #[test]
    fn boolean_combinators_require_bool() {
        assert_eq!(
            eval(&math("and"), &[Value::Bool(true), Value::Bool(false)]).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            eval(&math("or"), &[Value::Bool(true), Value::Bool(false)]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(eval(&math("not"), &[Value::Bool(true)]).unwrap(), Value::Bool(false));

        let err = eval(&math("and"), &[Value::f64(1.0), Value::Bool(true)]).unwrap_err();
        assert!(matches!(err, EvalError::Op { ref op, .. } if op == "zela.math::and"));
    }

    /// `eq` / `ne` 支持数值与布尔两种比较，其他类型显式报错。
    #[test]
    fn equality_compares_numbers_and_bools_only() {
        assert_eq!(
            eval(&math("eq"), &[Value::Bool(true), Value::Bool(true)]).unwrap(),
            Value::Bool(true)
        );
        let err = eval(&math("eq"), &[Value::text("a"), Value::text("a")]).unwrap_err();
        assert!(matches!(err, EvalError::Op { ref op, .. } if op == "zela.math::eq"));
    }
}
