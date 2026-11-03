//! **单态解析器**：把 `(算子, 实参槽类型)` 解析成具体类型的函数指针。
//!
//! # 职责边界（本模块存在的理由）
//!
//! 算子的**类型判断与转换**不发生在算子实现里，而发生在**装配期**的这里：
//! 给定实参的静态槽类型，按注册的签名选出**唯一**一种实例（`F64xF64ToF64` /
//! `Vec2xF64ToVec2` / …）。运行期只剩"读槽 → 调函数指针 → 写槽"。
//!
//! 签名声明了但这里没登记的**类型组合** ⇒ 返回 `None`（整棵表达式回落动态路径），
//! 不会静默挑一个"差不多"的实例。
//!
//! # 与动态实现的关系
//!
//! [`crate::ops`] 是**语义基准**（吃 `Value`、做错误诊断）；本模块的实例函数是
//! 同一公式的**精确类型版本**。两者由 `typed_matches_dynamic_for_every_operator`
//! 逐算子对拍（逐位相等），公式不允许漂移。

use ze_comb::{OpRef, SlotType, TypedCall, TypedOp};

/// `zela.math` 的单态解析器（无状态，可直接用 `&MathMonomorphizer`）。
#[derive(Clone, Copy, Debug, Default)]
pub struct MathMonomorphizer;

/// 全局唯一实例的便捷取用。
pub fn monomorphizer() -> &'static MathMonomorphizer {
    &MathMonomorphizer
}

impl ze_comb::OpResolver for MathMonomorphizer {
    fn resolve(&self, op: &OpRef, args: &[SlotType]) -> Option<TypedCall> {
        use SlotType::{Bool, F64, Vec2, Vec3, Vec4};

        if op.namespace != crate::LIBRARY_NAMESPACE {
            return None;
        }
        // 变参 `sum`：全部实参同型（空 ⇒ 标量零元，与动态路径一致）。
        if op.name == "sum" {
            use SlotType::{Vec2, Vec3};
            let kind = args.first().copied();
            let (op, ret) = match kind {
                None | Some(F64) if args.iter().all(|ty| *ty == F64) => {
                    (TypedOp::F64sToF64(instances::sum_f64), F64)
                }
                Some(Vec2) if args.iter().all(|ty| *ty == Vec2) => {
                    (TypedOp::Vec2sToVec2(instances::sum_vec2), Vec2)
                }
                Some(Vec3) if args.iter().all(|ty| *ty == Vec3) => {
                    (TypedOp::Vec3sToVec3(instances::sum_vec3), Vec3)
                }
                _ => return None,
            };
            return Some(TypedCall { op, ret });
        }

        let (op, ret) = match (op.name.as_str(), args) {
            ("sin", [F64]) => (TypedOp::F64ToF64(instances::sin), F64),
            ("cos", [F64]) => (TypedOp::F64ToF64(instances::cos), F64),
            ("add", [F64, F64]) => (TypedOp::F64xF64ToF64(instances::add_f64), F64),
            ("add", [Vec2, Vec2]) => (TypedOp::Vec2xVec2ToVec2(instances::add_vec2), Vec2),
            ("add", [Vec3, Vec3]) => (TypedOp::Vec3xVec3ToVec3(instances::add_vec3), Vec3),
            ("mul" | "scale", [F64, F64]) => (TypedOp::F64xF64ToF64(instances::scale_f64), F64),
            ("mul" | "scale", [Vec2, F64]) => {
                (TypedOp::Vec2xF64ToVec2(instances::scale_vec2), Vec2)
            }
            ("mul" | "scale", [F64, Vec2]) => (
                TypedOp::F64xVec2ToVec2(instances::scale_vec2_left),
                Vec2,
            ),
            ("mul" | "scale", [Vec3, F64]) => {
                (TypedOp::Vec3xF64ToVec3(instances::scale_vec3), Vec3)
            }
            ("mul" | "scale", [F64, Vec3]) => (
                TypedOp::F64xVec3ToVec3(instances::scale_vec3_left),
                Vec3,
            ),
            ("mix", [F64, F64, F64]) => (TypedOp::MixF64(instances::mix_f64), F64),
            ("mix", [Vec2, Vec2, F64]) => (TypedOp::MixVec2(instances::mix_vec2), Vec2),
            ("mix", [Vec3, Vec3, F64]) => (TypedOp::MixVec3(instances::mix_vec3), Vec3),
            ("clamp", [F64, F64, F64]) => (TypedOp::F64x3ToF64(instances::clamp), F64),
            ("vec2", [F64, F64]) => (TypedOp::F64xF64ToVec2(instances::vec2_of), Vec2),
            ("vec3", [F64, F64, F64]) => (TypedOp::F64x3ToVec3(instances::vec3_of), Vec3),
            // **BT-1**：`vec4` 构造器与 `w` 分量（其余算子暂不对 Vec4 开口）。
            ("vec4", [F64, F64, F64, F64]) => {
                (TypedOp::F64x4ToVec4(instances::vec4_of), Vec4)
            }
            ("w", [Vec4]) => (TypedOp::Vec4ToF64(instances::w_of_vec4), F64),
            ("x", [Vec2]) => (TypedOp::Vec2ToF64(instances::x_of_vec2), F64),
            ("x", [Vec3]) => (TypedOp::Vec3ToF64(instances::x_of_vec3), F64),
            ("y", [Vec2]) => (TypedOp::Vec2ToF64(instances::y_of_vec2), F64),
            ("y", [Vec3]) => (TypedOp::Vec3ToF64(instances::y_of_vec3), F64),
            // `Vec2` 上取 z 是**类型错误**：这里给不出实例 ⇒ 回落动态路径报错。
            ("z", [Vec3]) => (TypedOp::Vec3ToF64(instances::z_of_vec3), F64),
            ("rotate3", [Vec3, Vec3, F64]) => {
                (TypedOp::Vec3xVec3xF64ToVec3(instances::rotate3), Vec3)
            }
            ("splat", [F64]) => (TypedOp::F64ToVec3(instances::splat), Vec3),
            ("vec3_from_vec2", [Vec2, F64]) => {
                (TypedOp::Vec2xF64ToVec3(instances::vec3_from_vec2), Vec3)
            }
            ("vec2_from_vec3", [Vec3]) => (TypedOp::Vec3ToVec2(instances::vec2_from_vec3), Vec2),
            ("length", [Vec2]) => (TypedOp::Vec2ToF64(instances::length_vec2), F64),
            ("length", [Vec3]) => (TypedOp::Vec3ToF64(instances::length_vec3), F64),
            ("normalize", [Vec2]) => (TypedOp::Vec2ToVec2(instances::normalize_vec2), Vec2),
            ("normalize", [Vec3]) => (TypedOp::Vec3ToVec3(instances::normalize_vec3), Vec3),
            ("round_f32", [F64]) => (TypedOp::F64ToF64(instances::round_f32), F64),
            ("narrow_f32", [F64]) => (TypedOp::F64ToF64(instances::round_f32), F64),
            // ─── 第 BB 批：WGSL 标准库算子 ──────────────────────────────────
            ("sqrt", [F64]) => (TypedOp::F64ToF64(instances::sqrt), F64),
            ("inversesqrt", [F64]) => (TypedOp::F64ToF64(instances::inversesqrt), F64),
            ("abs", [F64]) => (TypedOp::F64ToF64(instances::abs), F64),
            ("sign", [F64]) => (TypedOp::F64ToF64(instances::sign), F64),
            ("min", [F64, F64]) => (TypedOp::F64xF64ToF64(instances::min_f64), F64),
            ("max", [F64, F64]) => (TypedOp::F64xF64ToF64(instances::max_f64), F64),
            ("floor", [F64]) => (TypedOp::F64ToF64(instances::floor), F64),
            ("ceil", [F64]) => (TypedOp::F64ToF64(instances::ceil), F64),
            ("trunc", [F64]) => (TypedOp::F64ToF64(instances::trunc), F64),
            ("round", [F64]) => (TypedOp::F64ToF64(instances::round), F64),
            ("modulo", [F64, F64]) => (TypedOp::F64xF64ToF64(instances::modulo), F64),
            ("exp", [F64]) => (TypedOp::F64ToF64(instances::exp), F64),
            ("exp2", [F64]) => (TypedOp::F64ToF64(instances::exp2), F64),
            ("log", [F64]) => (TypedOp::F64ToF64(instances::ln), F64),
            ("log2", [F64]) => (TypedOp::F64ToF64(instances::log2), F64),
            ("pow", [F64, F64]) => (TypedOp::F64xF64ToF64(instances::pow), F64),
            ("tan", [F64]) => (TypedOp::F64ToF64(instances::tan), F64),
            ("asin", [F64]) => (TypedOp::F64ToF64(instances::asin), F64),
            ("acos", [F64]) => (TypedOp::F64ToF64(instances::acos), F64),
            ("atan", [F64]) => (TypedOp::F64ToF64(instances::atan), F64),
            ("atan2", [F64, F64]) => (TypedOp::F64xF64ToF64(instances::atan2), F64),
            ("degrees", [F64]) => (TypedOp::F64ToF64(instances::degrees), F64),
            ("radians", [F64]) => (TypedOp::F64ToF64(instances::radians), F64),
            ("step", [F64, F64]) => (TypedOp::F64xF64ToF64(instances::step), F64),
            ("dot", [Vec2, Vec2]) => (TypedOp::Vec2xVec2ToF64(instances::dot_vec2), F64),
            ("dot", [Vec3, Vec3]) => (TypedOp::Vec3xVec3ToF64(instances::dot_vec3), F64),
            ("cross", [Vec3, Vec3]) => (TypedOp::Vec3xVec3ToVec3(instances::cross_vec3), Vec3),
            ("reflect", [Vec2, Vec2]) => {
                (TypedOp::Vec2xVec2ToVec2(instances::reflect_vec2), Vec2)
            }
            ("reflect", [Vec3, Vec3]) => {
                (TypedOp::Vec3xVec3ToVec3(instances::reflect_vec3), Vec3)
            }
            ("faceforward", [Vec2, Vec2, Vec2]) => (
                TypedOp::Vec2x3ToVec2(instances::faceforward_vec2),
                Vec2,
            ),
            ("faceforward", [Vec3, Vec3, Vec3]) => (
                TypedOp::Vec3x3ToVec3(instances::faceforward_vec3),
                Vec3,
            ),
            ("distance", [Vec2, Vec2]) => {
                (TypedOp::Vec2xVec2ToF64(instances::distance_vec2), F64)
            }
            ("distance", [Vec3, Vec3]) => {
                (TypedOp::Vec3xVec3ToF64(instances::distance_vec3), F64)
            }
            ("smoothstep", [F64, F64, F64]) => {
                (TypedOp::F64x3ToF64(instances::smoothstep), F64)
            }
            ("fract", [F64]) => (TypedOp::F64ToF64(instances::fract), F64),
            ("lt", [F64, F64]) => (TypedOp::F64xF64ToBool(instances::lt), Bool),
            ("gt", [F64, F64]) => (TypedOp::F64xF64ToBool(instances::gt), Bool),
            ("le", [F64, F64]) => (TypedOp::F64xF64ToBool(instances::le), Bool),
            ("ge", [F64, F64]) => (TypedOp::F64xF64ToBool(instances::ge), Bool),
            ("eq", [F64, F64]) => (TypedOp::F64xF64ToBool(instances::eq_f64), Bool),
            ("ne", [F64, F64]) => (TypedOp::F64xF64ToBool(instances::ne_f64), Bool),
            ("eq", [Bool, Bool]) => (TypedOp::BoolxBoolToBool(instances::eq_bool), Bool),
            ("ne", [Bool, Bool]) => (TypedOp::BoolxBoolToBool(instances::ne_bool), Bool),
            ("and", [Bool, Bool]) => (TypedOp::BoolxBoolToBool(instances::and), Bool),
            ("or", [Bool, Bool]) => (TypedOp::BoolxBoolToBool(instances::or), Bool),
            ("not", [Bool]) => (TypedOp::BoolToBool(instances::not), Bool),
            _ => return None,
        };
        Some(TypedCall { op, ret })
    }
}

/// 精确类型的算子实现（与 `crate::ops` 的公式逐位一致）。
///
/// 这里**没有** `as_f64` / `as_vec2`：能进到这个函数的调用，类型在装配期就已确定。
mod instances {
    use crate::ops::{lerp_f32, rotate_rodrigues, unit2, unit3};

    pub fn sin(x: f64) -> f64 {
        crate::det::sin(x)
    }

    pub fn cos(x: f64) -> f64 {
        crate::det::cos(x)
    }

    pub fn add_f64(a: f64, b: f64) -> f64 {
        a + b
    }

    pub fn add_vec2(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
        [a[0] + b[0], a[1] + b[1]]
    }

    pub fn add_vec3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
    }

    pub fn scale_f64(a: f64, b: f64) -> f64 {
        a * b
    }

    pub fn scale_vec2(v: [f32; 2], k: f64) -> [f32; 2] {
        let k = k as f32;
        [v[0] * k, v[1] * k]
    }

    pub fn scale_vec3(v: [f32; 3], k: f64) -> [f32; 3] {
        let k = k as f32;
        [v[0] * k, v[1] * k, v[2] * k]
    }

    pub fn scale_vec2_left(k: f64, v: [f32; 2]) -> [f32; 2] {
        scale_vec2(v, k)
    }

    pub fn scale_vec3_left(k: f64, v: [f32; 3]) -> [f32; 3] {
        scale_vec3(v, k)
    }

    /// 与动态 `sum` 同一累加顺序（`0.0` 起左折）。
    pub fn sum_f64(values: &[f64]) -> f64 {
        values.iter().fold(0.0, |total, value| total + value)
    }

    pub fn sum_vec2(values: &[[f32; 2]]) -> [f32; 2] {
        values.iter().fold([0.0, 0.0], |total, value| {
            [total[0] + value[0], total[1] + value[1]]
        })
    }

    pub fn sum_vec3(values: &[[f32; 3]]) -> [f32; 3] {
        values.iter().fold([0.0, 0.0, 0.0], |total, value| {
            [
                total[0] + value[0],
                total[1] + value[1],
                total[2] + value[2],
            ]
        })
    }

    pub fn mix_f64(a: f64, b: f64, t: f64) -> f64 {
        if t == 0.0 {
            a
        } else if t == 1.0 {
            b
        } else {
            f64::from(lerp_f32(a as f32, b as f32, t))
        }
    }

    pub fn mix_vec2(a: [f32; 2], b: [f32; 2], t: f64) -> [f32; 2] {
        if t == 0.0 {
            a
        } else if t == 1.0 {
            b
        } else {
            [lerp_f32(a[0], b[0], t), lerp_f32(a[1], b[1], t)]
        }
    }

    pub fn mix_vec3(a: [f32; 3], b: [f32; 3], t: f64) -> [f32; 3] {
        if t == 0.0 {
            a
        } else if t == 1.0 {
            b
        } else {
            [
                lerp_f32(a[0], b[0], t),
                lerp_f32(a[1], b[1], t),
                lerp_f32(a[2], b[2], t),
            ]
        }
    }

    pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
        if x < lo {
            lo
        } else if x > hi {
            hi
        } else {
            x
        }
    }

    pub fn vec2_of(x: f64, y: f64) -> [f32; 2] {
        [x as f32, y as f32]
    }

    pub fn vec3_of(x: f64, y: f64, z: f64) -> [f32; 3] {
        [x as f32, y as f32, z as f32]
    }

    /// **BT-1**：`vec4(r, g, b, a)`。
    pub fn vec4_of(x: f64, y: f64, z: f64, w: f64) -> [f32; 4] {
        [x as f32, y as f32, z as f32, w as f32]
    }

    /// **BT-1**：`w(v)`。
    pub fn w_of_vec4(v: [f32; 4]) -> f64 {
        f64::from(v[3])
    }

    pub fn x_of_vec2(v: [f32; 2]) -> f64 {
        f64::from(v[0])
    }

    pub fn y_of_vec2(v: [f32; 2]) -> f64 {
        f64::from(v[1])
    }

    pub fn x_of_vec3(v: [f32; 3]) -> f64 {
        f64::from(v[0])
    }

    pub fn y_of_vec3(v: [f32; 3]) -> f64 {
        f64::from(v[1])
    }

    pub fn z_of_vec3(v: [f32; 3]) -> f64 {
        f64::from(v[2])
    }

    pub fn rotate3(v: [f32; 3], axis: [f32; 3], angle: f64) -> [f32; 3] {
        rotate_rodrigues(
            [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])],
            [
                f64::from(axis[0]),
                f64::from(axis[1]),
                f64::from(axis[2]),
            ],
            angle,
        )
    }

    pub fn splat(x: f64) -> [f32; 3] {
        let x = x as f32;
        [x, x, x]
    }

    pub fn vec3_from_vec2(v: [f32; 2], z: f64) -> [f32; 3] {
        [v[0], v[1], z as f32]
    }

    pub fn vec2_from_vec3(v: [f32; 3]) -> [f32; 2] {
        [v[0], v[1]]
    }

    pub fn length_vec2(v: [f32; 2]) -> f64 {
        f64::from((v[0] * v[0] + v[1] * v[1]).sqrt())
    }

    pub fn length_vec3(v: [f32; 3]) -> f64 {
        f64::from((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt())
    }

    pub fn normalize_vec2(v: [f32; 2]) -> [f32; 2] {
        unit2(v, (v[0] * v[0] + v[1] * v[1]).sqrt())
    }

    pub fn normalize_vec3(v: [f32; 3]) -> [f32; 3] {
        unit3(v, (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt())
    }

    pub fn round_f32(x: f64) -> f64 {
        f64::from(x as f32)
    }

    // ─── 第 BB 批：WGSL 标准库算子（与 `crate::ops` 逐位一致）──────────────

    pub fn sqrt(x: f64) -> f64 {
        x.sqrt()
    }

    pub fn inversesqrt(x: f64) -> f64 {
        1.0 / x.sqrt()
    }

    pub fn abs(x: f64) -> f64 {
        x.abs()
    }

    /// 与动态 `sign` 同口径：`±0` / NaN ⇒ 0（不是 `f64::signum`）。
    pub fn sign(x: f64) -> f64 {
        if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        }
    }

    pub fn min_f64(a: f64, b: f64) -> f64 {
        a.min(b)
    }

    pub fn max_f64(a: f64, b: f64) -> f64 {
        a.max(b)
    }

    pub fn floor(x: f64) -> f64 {
        x.floor()
    }

    pub fn ceil(x: f64) -> f64 {
        x.ceil()
    }

    pub fn trunc(x: f64) -> f64 {
        x.trunc()
    }

    pub fn round(x: f64) -> f64 {
        x.round()
    }

    pub fn modulo(a: f64, b: f64) -> f64 {
        a % b
    }

    pub fn exp(x: f64) -> f64 {
        crate::det::exp(x)
    }

    pub fn exp2(x: f64) -> f64 {
        crate::det::exp2(x)
    }

    pub fn ln(x: f64) -> f64 {
        crate::det::ln(x)
    }

    pub fn log2(x: f64) -> f64 {
        crate::det::log2(x)
    }

    pub fn pow(a: f64, b: f64) -> f64 {
        crate::det::pow(a, b)
    }

    pub fn tan(x: f64) -> f64 {
        crate::det::tan(x)
    }

    pub fn asin(x: f64) -> f64 {
        crate::det::asin(x)
    }

    pub fn acos(x: f64) -> f64 {
        crate::det::acos(x)
    }

    pub fn atan(x: f64) -> f64 {
        crate::det::atan(x)
    }

    pub fn atan2(y: f64, x: f64) -> f64 {
        crate::det::atan2(y, x)
    }

    pub fn degrees(x: f64) -> f64 {
        x.to_degrees()
    }

    pub fn radians(x: f64) -> f64 {
        x.to_radians()
    }

    pub fn step(edge: f64, x: f64) -> f64 {
        if x < edge {
            0.0
        } else {
            1.0
        }
    }

    pub fn dot_vec2(a: [f32; 2], b: [f32; 2]) -> f64 {
        f64::from(a[0] * b[0] + a[1] * b[1])
    }

    pub fn dot_vec3(a: [f32; 3], b: [f32; 3]) -> f64 {
        f64::from(a[0] * b[0] + a[1] * b[1] + a[2] * b[2])
    }

    pub fn cross_vec3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    pub fn reflect_vec2(e1: [f32; 2], e2: [f32; 2]) -> [f32; 2] {
        let k = 2.0 * (e2[0] * e1[0] + e2[1] * e1[1]);
        [e1[0] - k * e2[0], e1[1] - k * e2[1]]
    }

    pub fn reflect_vec3(e1: [f32; 3], e2: [f32; 3]) -> [f32; 3] {
        let k = 2.0 * (e2[0] * e1[0] + e2[1] * e1[1] + e2[2] * e1[2]);
        [
            e1[0] - k * e2[0],
            e1[1] - k * e2[1],
            e1[2] - k * e2[2],
        ]
    }

    pub fn faceforward_vec2(e1: [f32; 2], e2: [f32; 2], e3: [f32; 2]) -> [f32; 2] {
        let d = e2[0] * e3[0] + e2[1] * e3[1];
        if d < 0.0 {
            e1
        } else {
            [-e1[0], -e1[1]]
        }
    }

    pub fn faceforward_vec3(e1: [f32; 3], e2: [f32; 3], e3: [f32; 3]) -> [f32; 3] {
        let d = e2[0] * e3[0] + e2[1] * e3[1] + e2[2] * e3[2];
        if d < 0.0 {
            e1
        } else {
            [-e1[0], -e1[1], -e1[2]]
        }
    }

    /// 与动态 `distance` 同一公式（f32 域相减、平方、开方）。
    pub fn distance_vec2(a: [f32; 2], b: [f32; 2]) -> f64 {
        let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
        f64::from((dx * dx + dy * dy).sqrt())
    }

    pub fn distance_vec3(a: [f32; 3], b: [f32; 3]) -> f64 {
        let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
        f64::from((dx * dx + dy * dy + dz * dz).sqrt())
    }

    /// 与动态 `smoothstep` 同式（含 `e0 == e1` 的 IEEE 结果）。
    pub fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
        let (e0, e1, x) = (e0 as f32, e1 as f32, x as f32);
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        f64::from(t * t * (3.0 - 2.0 * t))
    }

    pub fn fract(x: f64) -> f64 {
        let x = x as f32;
        f64::from(x - x.floor())
    }

    pub fn lt(a: f64, b: f64) -> bool {
        a < b
    }

    pub fn gt(a: f64, b: f64) -> bool {
        a > b
    }

    pub fn le(a: f64, b: f64) -> bool {
        a <= b
    }

    pub fn ge(a: f64, b: f64) -> bool {
        a >= b
    }

    pub fn eq_f64(a: f64, b: f64) -> bool {
        a == b
    }

    pub fn ne_f64(a: f64, b: f64) -> bool {
        a != b
    }

    pub fn eq_bool(a: bool, b: bool) -> bool {
        a == b
    }

    pub fn ne_bool(a: bool, b: bool) -> bool {
        a != b
    }

    pub fn and(a: bool, b: bool) -> bool {
        a && b
    }

    pub fn or(a: bool, b: bool) -> bool {
        a || b
    }

    pub fn not(a: bool) -> bool {
        !a
    }
}
