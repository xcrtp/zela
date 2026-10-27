//! `zela.math` 的**标量算子语义**（纯函数，无状态）。
//!
//! # 为什么在这里
//!
//! 算子的**身份**（namespace + name）由本 crate 的 capability 声明；
//! 算子的**语义**必须与身份同源，否则表达式层与求值层会各说各话。
//!
//! # 边界
//!
//! 只做标量（`f64`）。向量 / 路径算子仍留在场景侧；错误里带算子限定名，
//! 便于把「脚本作者看到的写法」映射回「函子库里的那一条签名」。

use ze_func::{EvalError, OpRef, Value};

/// 本函子库认识的算子（装配期校验用；未知算子直接拒绝）。
///
/// # 为什么有 `and` / `or` / `not`
///
/// predicate（`bind` 的生效条件）是 Bool 表达式，必须有比较与布尔组合子；
/// 它们是**值算子**，与 `sin` 等同层。Bool 结果不进 `anim_value`（root 侧语义不变）。
pub const OPS: &[&str] = &[
    "sin", "cos", "add", "mul", "scale", "sum", "mix", "clamp", "vec2", "vec3", "x", "y", "z",
    "rotate3", "splat", "vec3_from_vec2", "vec2_from_vec3", "length", "normalize", "round_f32",
    "lt", "gt", "le", "ge", "eq", "ne", "and", "or", "not",
];

/// `op` 是否属于本库（namespace 必须一致，名字在 [`OPS`] 里）。
pub fn is_known(op: &OpRef) -> bool {
    op.namespace == crate::LIBRARY_NAMESPACE && OPS.contains(&op.name.as_str())
}

/// 标量算子求值。
///
/// 未知算子 / 实参类型不符 ⇒ [`EvalError`]，不静默返回 0。
pub fn eval(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let number = |index: usize| -> Result<f64, EvalError> {
        args.get(index)
            .and_then(Value::as_f64)
            .ok_or_else(|| EvalError::op(op, format!("第 {index} 个实参不是数值")))
    };
    let boolean = |index: usize| -> Result<bool, EvalError> {
        match args.get(index) {
            Some(Value::Bool(value)) => Ok(*value),
            _ => Err(EvalError::op(
                op,
                format!("第 {index} 个实参不是布尔（Bool 由比较算子产出）"),
            )),
        }
    };

    match op.name.as_str() {
        "sin" => Ok(Value::F64(number(0)?.sin())),
        "cos" => Ok(Value::F64(number(0)?.cos())),
        "add" => Ok(Value::F64(number(0)? + number(1)?)),
        "mul" => Ok(Value::F64(number(0)? * number(1)?)),
        // `scale(k, of)`：k 在产物里是字面量叶子，这里只是普通乘法。
        "scale" => Ok(Value::F64(number(0)? * number(1)?)),
        "sum" => {
            let mut total = 0.0;
            for index in 0..args.len() {
                total += number(index)?;
            }
            Ok(Value::F64(total))
        }
        // ─── 第 1 批（算子补齐）────────────────────────────────────────
        // `mix(a, b, t)`：线性插值（数值 / Vec2 / Vec3 同型）。
        "mix" => {
            let t = number(2)?;
            let a = args.first().ok_or_else(|| EvalError::op(op, "需要三个实参"))?;
            let b = args.get(1).ok_or_else(|| EvalError::op(op, "需要三个实参"))?;
            // **端点返回原值**：`a + (b − a) × 1` 在 f32 上可能偏离 `b` 一个 ulp。
            // 旧 `Curve::Keyframes` 在 `t ≤ t0 / t ≥ t1` 时直接返回端点 ⇒ 这里同口径
            // （`Ramp` 的钳制保证端点会精确命中 0 / 1）。
            if t == 0.0 {
                return Ok(a.clone());
            }
            if t == 1.0 {
                return Ok(b.clone());
            }
            match (a, b) {
                // 标量走 **f32 插值**：与旧 `Curve::Keyframes`（`AnimValue::Scalar`）的
                // `a + (b - a) * α` 逐位一致（第 5a 批「先证明等价」的硬要求）。
                (Value::F64(a), Value::F64(b)) => Ok(Value::F64(f64::from(lerp_f32(
                    *a as f32,
                    *b as f32,
                    t,
                )))),
                (Value::Vec2(a), Value::Vec2(b)) => Ok(Value::Vec2([
                    lerp_f32(a[0], b[0], t),
                    lerp_f32(a[1], b[1], t),
                ])),
                (Value::Vec3(a), Value::Vec3(b)) => Ok(Value::Vec3([
                    lerp_f32(a[0], b[0], t),
                    lerp_f32(a[1], b[1], t),
                    lerp_f32(a[2], b[2], t),
                ])),
                _ => Err(EvalError::op(
                    op,
                    "两个端点必须是同型数值 / 向量（不做隐式提升）",
                )),
            }
        }
        // `clamp(x, lo, hi)`：`lo > hi` 时按 `lo` 优先（与 `f32::clamp` 的 panic 口径不同 ——
        // 表达式层不 panic，取"先夹下界"的确定行为）。
        "clamp" => {
            let x = number(0)?;
            let lo = number(1)?;
            let hi = number(2)?;
            Ok(Value::F64(if x < lo {
                lo
            } else if x > hi {
                hi
            } else {
                x
            }))
        }
        "vec2" => Ok(Value::Vec2([
            number(0)? as f32,
            number(1)? as f32,
        ])),
        "vec3" => Ok(Value::Vec3([
            number(0)? as f32,
            number(1)? as f32,
            number(2)? as f32,
        ])),
        // `x(v)` / `y(v)` / `z(v)`：分量访问；`Vec2` 上取 z 是**显式错误**（不是静默 0）。
        "x" | "y" | "z" => {
            let index = match op.name.as_str() {
                "x" => 0,
                "y" => 1,
                _ => 2,
            };
            match args.first() {
                Some(Value::Vec2(v)) => {
                    if index == 2 {
                        return Err(EvalError::op(op, "Vec2 没有 z 分量（请先 vec3 升维）"));
                    }
                    Ok(Value::F64(f64::from(v[index])))
                }
                Some(Value::Vec3(v)) => Ok(Value::F64(f64::from(v[index]))),
                _ => Err(EvalError::op(op, "第 0 个实参不是向量")),
            }
        }
        // `rotate3(v, axis, angle)`：罗德里格斯（与旧 `Curve::Orbit` 同一公式）。
        "rotate3" => {
            let v = vec3_arg(op, args, 0)?;
            let axis = vec3_arg(op, args, 1)?;
            let angle = number(2)?;
            Ok(Value::Vec3(rotate_rodrigues(v, axis, angle)))
        }
        // ─── 第 3 批（类型系统扩展）：转换算子族 ───────────────────────
        // `splat(v)`：一个标量铺成三个分量（`F64 → Vec3`）。
        "splat" => {
            let v = number(0)? as f32;
            Ok(Value::Vec3([v, v, v]))
        }
        // `vec3_from_vec2(v, z)`：显式补 z（不猜 0）。
        "vec3_from_vec2" => match args.first() {
            Some(Value::Vec2(v)) => Ok(Value::Vec3([v[0], v[1], number(1)? as f32])),
            _ => Err(EvalError::op(op, "第 0 个实参不是 Vec2")),
        },
        // `vec2_from_vec3(v)`：**显式**丢弃 z（不是静默截断 —— 算子名写明了）。
        "vec2_from_vec3" => match args.first() {
            Some(Value::Vec3(v)) => Ok(Value::Vec2([v[0], v[1]])),
            _ => Err(EvalError::op(op, "第 0 个实参不是 Vec3")),
        },
        // `length(v)`：向量模长；`normalize(v)`：单位向量（零向量保持零，不产出 NaN）。
        "length" | "normalize" => {
            let normalized = op.name == "normalize";
            match args.first() {
                Some(Value::Vec2(v)) => {
                    let length = (v[0] * v[0] + v[1] * v[1]).sqrt();
                    if normalized {
                        Ok(Value::Vec2(unit2(*v, length)))
                    } else {
                        Ok(Value::F64(f64::from(length)))
                    }
                }
                Some(Value::Vec3(v)) => {
                    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                    if normalized {
                        Ok(Value::Vec3(unit3(*v, length)))
                    } else {
                        Ok(Value::F64(f64::from(length)))
                    }
                }
                _ => Err(EvalError::op(op, "第 0 个实参不是向量")),
            }
        }
        // `round_f32(x)`：把数值落成 f32 再升回 f64。
        //
        // **为什么需要**：动画值域是 f32（`AnimValue::Scalar`），而表达式层是 f64。
        // 旧曲线（`Sin` / `Phase` / `Orbit2D`）每一步都在 f32 上取整；要把它们
        // **逐位**迁移过来，必须在同样的位置显式落位（第 5a 批的等价证明）。
        "round_f32" => Ok(Value::F64(f64::from(number(0)? as f32))),
        "lt" => Ok(Value::Bool(number(0)? < number(1)?)),
        "gt" => Ok(Value::Bool(number(0)? > number(1)?)),
        "le" => Ok(Value::Bool(number(0)? <= number(1)?)),
        "ge" => Ok(Value::Bool(number(0)? >= number(1)?)),
        "eq" | "ne" => {
            let equal = match (args.first(), args.get(1)) {
                (Some(Value::Bool(a)), Some(Value::Bool(b))) => a == b,
                (Some(a), Some(b)) => match (a.as_f64(), b.as_f64()) {
                    (Some(a), Some(b)) => a == b,
                    _ => {
                        return Err(EvalError::op(
                            op,
                            "只支持数值或布尔比较（不做隐式真值转换）",
                        ))
                    }
                },
                _ => return Err(EvalError::op(op, "需要两个实参")),
            };
            Ok(Value::Bool(if op.name == "eq" { equal } else { !equal }))
        }
        "and" => Ok(Value::Bool(boolean(0)? && boolean(1)?)),
        "or" => Ok(Value::Bool(boolean(0)? || boolean(1)?)),
        "not" => Ok(Value::Bool(!boolean(0)?)),
        other => Err(EvalError::op(op, format!("未知算子 `{other}`"))),
    }
}

/// 线性插值（**f32 域**）。
///
/// `α` 先落 f32 再参与运算，与 `ze-time` 的 `Curve::Keyframes` 同一精度口径：
/// `a + (b - a) * α` 全部在 f32 上完成（第 5a 批的逐位等价要求）。
fn lerp_f32(a: f32, b: f32, t: f64) -> f32 {
    a + (b - a) * (t as f32)
}

/// 取第 `index` 个 `Vec3` 实参（错误点名算子与位置）。
fn vec3_arg(op: &OpRef, args: &[Value], index: usize) -> Result<[f64; 3], EvalError> {
    match args.get(index) {
        Some(Value::Vec3(v)) => Ok([f64::from(v[0]), f64::from(v[1]), f64::from(v[2])]),
        _ => Err(EvalError::op(op, format!("第 {index} 个实参不是 Vec3"))),
    }
}

/// `Vec2` 单位化：零长度保持零（与 `glam::normalize_or_zero` 同一约定，不产出 NaN）。
fn unit2(v: [f32; 2], length: f32) -> [f32; 2] {
    if length > 1e-12 {
        [v[0] / length, v[1] / length]
    } else {
        [0.0, 0.0]
    }
}

/// `Vec3` 单位化：零长度保持零。
fn unit3(v: [f32; 3], length: f32) -> [f32; 3] {
    if length > 1e-12 {
        [v[0] / length, v[1] / length, v[2] / length]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// 罗德里格斯旋转：`v cosθ + (axis × v) sinθ + axis (axis·v)(1 − cosθ)`。
///
/// `axis` 先归一化；零向量轴退回 `Y`（与旧 `Curve::Orbit` 的退化处理一致）。
fn rotate_rodrigues(v: [f64; 3], axis: [f64; 3], angle: f64) -> [f32; 3] {
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
        let x = Value::F64(0.5);
        assert_eq!(
            eval(&math("sin"), std::slice::from_ref(&x)).unwrap(),
            Value::F64(0.5_f64.sin())
        );
        assert_eq!(
            eval(&math("mul"), &[x.clone(), Value::F64(3.0)]).unwrap(),
            Value::F64(1.5)
        );
        assert_eq!(
            eval(&math("scale"), &[Value::F64(4.0), x.clone()]).unwrap(),
            Value::F64(2.0)
        );
        assert_eq!(
            eval(&math("sum"), &[Value::F64(1.0), x, Value::F64(2.0)]).unwrap(),
            Value::F64(3.5)
        );
        assert_eq!(eval(&math("sum"), &[]).unwrap(), Value::F64(0.0));
    }

    /// 第 1 批（算子补齐）：`mix` 覆盖数值 / Vec2 / Vec3，且不做隐式提升。
    #[test]
    fn mix_interpolates_numbers_and_vectors() {
        assert_eq!(
            eval(
                &math("mix"),
                &[Value::F64(0.0), Value::F64(10.0), Value::F64(0.25)]
            )
            .unwrap(),
            Value::F64(2.5)
        );
        assert_eq!(
            eval(
                &math("mix"),
                &[
                    Value::Vec2([0.0, -2.0]),
                    Value::Vec2([4.0, 2.0]),
                    Value::F64(0.5),
                ]
            )
            .unwrap(),
            Value::Vec2([2.0, 0.0])
        );
        assert_eq!(
            eval(
                &math("mix"),
                &[
                    Value::Vec3([0.0, 0.0, 0.0]),
                    Value::Vec3([2.0, 4.0, 6.0]),
                    Value::F64(0.5),
                ]
            )
            .unwrap(),
            Value::Vec3([1.0, 2.0, 3.0])
        );
        // 端点混型 ⇒ 显式错误（不静默当真值提升）。
        let err = eval(
            &math("mix"),
            &[
                Value::F64(0.0),
                Value::Vec2([1.0, 1.0]),
                Value::F64(0.5),
            ],
        )
        .unwrap_err();
        assert!(matches!(err, EvalError::Op { ref op, .. } if op == "zela.math::mix"));
    }

    #[test]
    fn clamp_bounds_both_ends() {
        let clamp = |x: f64| {
            eval(
                &math("clamp"),
                &[Value::F64(x), Value::F64(0.0), Value::F64(1.0)],
            )
            .unwrap()
        };
        assert_eq!(clamp(-0.5), Value::F64(0.0));
        assert_eq!(clamp(0.25), Value::F64(0.25));
        assert_eq!(clamp(2.0), Value::F64(1.0));
    }

    /// 第 1 批：`vec2` / `vec3` 显式构造，`x|y|z` 取分量（`Vec2` 上取 z 是显式错误）。
    #[test]
    fn vector_constructors_and_components() {
        let v2 = eval(&math("vec2"), &[Value::F64(1.0), Value::F64(2.0)]).unwrap();
        assert_eq!(v2, Value::Vec2([1.0, 2.0]));
        assert_eq!(eval(&math("x"), &[v2.clone()]).unwrap(), Value::F64(1.0));
        assert_eq!(eval(&math("y"), &[v2.clone()]).unwrap(), Value::F64(2.0));
        assert!(eval(&math("z"), &[v2]).is_err(), "Vec2 没有 z 分量");

        let v3 = eval(
            &math("vec3"),
            &[Value::F64(1.0), Value::F64(2.0), Value::F64(3.0)],
        )
        .unwrap();
        assert_eq!(v3, Value::Vec3([1.0, 2.0, 3.0]));
        assert_eq!(eval(&math("z"), &[v3]).unwrap(), Value::F64(3.0));
    }

    /// `rotate3` 与旧 `Curve::Orbit` 同一公式：绕 Y 转 90° ⇒ `(0,0,r) → (r,0,0)`。
    #[test]
    fn rotate3_matches_the_orbit_convention() {
        let rotated = eval(
            &math("rotate3"),
            &[
                Value::Vec3([0.0, 0.0, 5.0]),
                Value::Vec3([0.0, 1.0, 0.0]),
                Value::F64(std::f64::consts::FRAC_PI_2),
            ],
        )
        .unwrap();
        let Value::Vec3(v) = rotated else {
            panic!("rotate3 必须返回 Vec3");
        };
        assert!((v[0] - 5.0).abs() < 1e-4 && v[1].abs() < 1e-4 && v[2].abs() < 1e-4, "{v:?}");
    }

    /// 第 3 批（类型系统扩展）：转换算子族 —— 升降维是**显式**的，长度/单位化覆盖两种向量。
    #[test]
    fn conversion_operators_are_explicit() {
        assert_eq!(
            eval(&math("splat"), &[Value::F64(2.0)]).unwrap(),
            Value::Vec3([2.0, 2.0, 2.0])
        );
        assert_eq!(
            eval(
                &math("vec3_from_vec2"),
                &[Value::Vec2([1.0, 2.0]), Value::F64(3.0)]
            )
            .unwrap(),
            Value::Vec3([1.0, 2.0, 3.0])
        );
        assert_eq!(
            eval(&math("vec2_from_vec3"), &[Value::Vec3([4.0, 5.0, 6.0])]).unwrap(),
            Value::Vec2([4.0, 5.0]),
            "vec2_from_vec3 显式丢弃 z"
        );

        let length = eval(&math("length"), &[Value::Vec3([3.0, 4.0, 0.0])]).unwrap();
        assert_eq!(length, Value::F64(5.0));
        let unit = eval(&math("normalize"), &[Value::Vec2([0.0, 3.0])]).unwrap();
        assert_eq!(unit, Value::Vec2([0.0, 1.0]));
        // 零向量保持零（不产出 NaN）。
        assert_eq!(
            eval(&math("normalize"), &[Value::Vec3([0.0, 0.0, 0.0])]).unwrap(),
            Value::Vec3([0.0, 0.0, 0.0])
        );

        // 类型不符是显式错误（不做隐式升降维）。
        assert!(
            eval(
                &math("vec3_from_vec2"),
                &[Value::Vec3([1.0, 2.0, 3.0]), Value::F64(0.0)]
            )
            .is_err()
        );
        assert!(eval(&math("length"), &[Value::F64(1.0)]).is_err());
    }

    /// 第 5a 批：`round_f32` 把数值落成 f32 再升回（逐位复刻旧曲线的精度口径）。
    #[test]
    fn round_f32_lands_on_the_f32_grid() {
        let value = eval(&math("round_f32"), &[Value::F64(0.1 + 0.2)]).unwrap();
        assert_eq!(value, Value::F64(f64::from((0.1_f64 + 0.2_f64) as f32)));
    }

    #[test]
    fn type_errors_name_the_operator() {
        let err = eval(&math("sin"), &[Value::Text("x".to_string())]).unwrap_err();
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
        let err = eval(&math("tan"), &[Value::F64(1.0)]).unwrap_err();
        assert!(matches!(err, EvalError::Op { ref op, .. } if op == "zela.math::tan"));
    }

    /// 比较算子产出 `Bool`（predicate 的原料）；顺序与边界都按 IEEE 比较。
    #[test]
    fn comparisons_yield_bool() {
        let one = Value::F64(1.0);
        let two = Value::F64(2.0);
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

        let err = eval(&math("and"), &[Value::F64(1.0), Value::Bool(true)]).unwrap_err();
        assert!(matches!(err, EvalError::Op { ref op, .. } if op == "zela.math::and"));
    }

    /// `eq` / `ne` 支持数值与布尔两种比较，其他类型显式报错。
    #[test]
    fn equality_compares_numbers_and_bools_only() {
        assert_eq!(
            eval(&math("eq"), &[Value::Bool(true), Value::Bool(true)]).unwrap(),
            Value::Bool(true)
        );
        let err = eval(&math("eq"), &[Value::Text("a".into()), Value::Text("a".into())])
            .unwrap_err();
        assert!(matches!(err, EvalError::Op { ref op, .. } if op == "zela.math::eq"));
    }
}
