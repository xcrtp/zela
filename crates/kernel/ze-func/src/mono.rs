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
        use SlotType::{Bool, F64, Vec2, Vec3};

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
        x.sin()
    }

    pub fn cos(x: f64) -> f64 {
        x.cos()
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
