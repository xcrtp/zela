//! **WGSL codegen 后端**（阶段 4-A）：把 `zela.math` 的算子渲染成 WGSL 表达式。
//!
//! # 分层
//!
//! 结构递归、类型映射、确定性拼接都在 `ze_comb::codegen`；本模块只回答
//! "这个算子（这组实参类型）在 WGSL 里长什么样"。签名校验复用单态解析器
//! （[`crate::mono`]）：**生成规则与求值实例同源**，不另写一份类型判断。
//!
//! # 精度口径
//!
//! WGSL 只有 `f32`：`SlotType::F64` 在这里生成 `f32` 运算。这是**已知的 payload
//! 外差异**（GAPS AU-04），三后端对拍按容差判定，不承诺逐位一致。
//!
//! # 辅助函数
//!
//! 两个"零长度不产出 NaN"的约定与 `crate::ops` 一致（`unit2` / `unit3` /
//! `rotate_rodrigues`），落在 [`PRELUDE`] 里；算子模板只调用它们，不做内联展开。

use ze_comb::{OpRef, OpResolver, SlotType, WgslBackend};

/// 后端前导：与 `crate::ops` 同口径的辅助函数。
///
/// - `zela_normalize2/3`：零长度保持零（`unit2` / `unit3`），不是 NaN；
/// - `zela_rotate3`：Rodrigues 公式，轴长度 ≈ 0 时退回 `(0, 1, 0)`
///   （`rotate_rodrigues`）。
pub const PRELUDE: &str = "\
// ze-func: 与 CPU 语义同口径的辅助函数（零长度不产出 NaN）。
fn zela_normalize2(v: vec2<f32>) -> vec2<f32> {
    let l = length(v);
    return select(v / l, vec2<f32>(0.0, 0.0), l <= 1e-12);
}
fn zela_normalize3(v: vec3<f32>) -> vec3<f32> {
    let l = length(v);
    return select(v / l, vec3<f32>(0.0, 0.0, 0.0), l <= 1e-12);
}
fn zela_rotate3(v: vec3<f32>, axis: vec3<f32>, angle: f32) -> vec3<f32> {
    let l = length(axis);
    let a = select(axis / l, vec3<f32>(0.0, 1.0, 0.0), l <= 1e-12);
    let c = cos(angle);
    let s = sin(angle);
    return v * c + cross(a, v) * s + a * dot(a, v) * (1.0 - c);
}
";

/// `zela.math` 的 WGSL 后端（无状态）。
#[derive(Clone, Copy, Debug, Default)]
pub struct MathWgsl;

/// 全局唯一实例的便捷取用。
pub fn backend() -> &'static MathWgsl {
    &MathWgsl
}

impl WgslBackend for MathWgsl {
    fn op(&self, op: &OpRef, types: &[SlotType], args: &[String]) -> Option<(SlotType, String)> {
        if op.namespace != crate::LIBRARY_NAMESPACE {
            return None;
        }
        // 签名 / 实参类型与求值实例同源：解析不出 ⇒ 生成不出来（显式失败）。
        let call = crate::mono::monomorphizer().resolve(op, types)?;
        let text = match op.name.as_str() {
            "sin" => format!("sin({})", args[0]),
            "cos" => format!("cos({})", args[0]),
            "add" => format!("({} + {})", args[0], args[1]),
            "mul" | "scale" => format!("({} * {})", args[0], args[1]),
            "sum" => {
                if args.is_empty() {
                    "0.0".to_string()
                } else {
                    format!("({})", args.join(" + "))
                }
            }
            "mix" => format!("mix({}, {}, {})", args[0], args[1], args[2]),
            "clamp" => format!("clamp({}, {}, {})", args[0], args[1], args[2]),
            "vec2" => format!("vec2<f32>({}, {})", args[0], args[1]),
            "vec3" => format!("vec3<f32>({}, {}, {})", args[0], args[1], args[2]),
            "x" => format!("({}).x", args[0]),
            "y" => format!("({}).y", args[0]),
            "z" => format!("({}).z", args[0]),
            "rotate3" => format!("zela_rotate3({}, {}, {})", args[0], args[1], args[2]),
            "splat" => format!("vec3<f32>({})", args[0]),
            "vec3_from_vec2" => format!("vec3<f32>({}, {})", args[0], args[1]),
            "vec2_from_vec3" => format!("({}).xy", args[0]),
            "length" => format!("length({})", args[0]),
            "normalize" => match types[0] {
                SlotType::Vec2 => format!("zela_normalize2({})", args[0]),
                SlotType::Vec3 => format!("zela_normalize3({})", args[0]),
                _ => return None,
            },
            // `round_f32` 是"把 f64 收窄到 f32"，不是"四舍五入到整数"：shader 全域都是
            // f32 ⇒ 它就是恒等（写 `round()` 会把语义改成最近整数，是错的）。
            "round_f32" => format!("({})", args[0]),
            "lt" => format!("({} < {})", args[0], args[1]),
            "gt" => format!("({} > {})", args[0], args[1]),
            "le" => format!("({} <= {})", args[0], args[1]),
            "ge" => format!("({} >= {})", args[0], args[1]),
            "eq" => format!("({} == {})", args[0], args[1]),
            "ne" => format!("({} != {})", args[0], args[1]),
            "and" => format!("({} && {})", args[0], args[1]),
            "or" => format!("({} || {})", args[0], args[1]),
            "not" => format!("(!{})", args[0]),
            _ => return None,
        };
        Some((call.ret, text))
    }

    fn channel(&self, channel: &ze_comb::ChannelRef) -> Option<(SlotType, String)> {
        let ty = ze_comb::slot_of_channel(channel.ty)?;
        // 标识符只留 ASCII 字母数字，其余换 `_`；统一加 `c_` 前缀避开 WGSL 关键字。
        let mut name = String::from("c_");
        for byte in channel.qualified().bytes() {
            name.push(if byte.is_ascii_alphanumeric() {
                byte as char
            } else {
                '_'
            });
        }
        Some((ty, name))
    }

    fn prelude(&self) -> &str {
        PRELUDE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_comb::{ChannelRef, Expr, OpRef};

    fn math(name: &str) -> OpRef {
        OpRef::new(crate::LIBRARY_NAMESPACE, name)
    }

    /// 目录门禁：**每个算子都有一条生成规则**（新增算子漏写 ⇒ 这里红）。
    ///
    /// 签名从 `OP_DEFS` 展开（不是手写清单）：`Vector` 变量各生成 `Vec2` / `Vec3`
    /// 两个实例，变参补第二个实参。
    #[test]
    fn every_operator_has_a_wgsl_rule() {
        for def in crate::OP_DEFS {
            let op = OpRef::new(crate::LIBRARY_NAMESPACE, def.name);
            let mut covered = 0usize;
            for signature in def.sigs {
                for types in signature_instances(signature) {
                    if crate::mono::monomorphizer().resolve(&op, &types).is_none() {
                        continue; // 签名声明了但求值实例没有 ⇒ 由 AV-02 的用例负责
                    }
                    let args: Vec<String> = (0..types.len()).map(|i| format!("a{i}")).collect();
                    assert!(
                        backend().op(&op, &types, &args).is_some(),
                        "算子 `{}`（{:?}）缺 WGSL 生成规则",
                        def.name,
                        types
                    );
                    covered += 1;
                }
            }
            assert!(covered > 0, "算子 `{}` 连一个可生成签名都没有", def.name);
        }
    }

    /// 把 `OpSig` 展开成具体槽类型（`Vector` 变量各试 `Vec2` / `Vec3`）。
    fn signature_instances(sig: &ze_type::OpSig) -> Vec<Vec<SlotType>> {
        use ze_type::{Kind, ScalarKind, TypeSpec};
        let vector = sig.vars.iter().any(|(_, kind)| *kind == Kind::Vector);
        let candidates = if vector {
            vec![SlotType::Vec2, SlotType::Vec3]
        } else {
            vec![SlotType::F64]
        };
        candidates
            .into_iter()
            .filter_map(|vector_ty| {
                let mut types = Vec::new();
                for param in sig.params {
                    let ty = match param {
                        TypeSpec::Scalar(ScalarKind::F64) => SlotType::F64,
                        TypeSpec::Scalar(ScalarKind::Bool) => SlotType::Bool,
                        TypeSpec::Var(name) => {
                            let (_, kind) = sig.vars.iter().find(|(n, _)| n == name)?;
                            match kind {
                                Kind::Num => SlotType::F64,
                                Kind::Vector => vector_ty,
                                _ => return None,
                            }
                        }
                        TypeSpec::Struct { name, .. } if *name == "zela.Vec2" => SlotType::Vec2,
                        TypeSpec::Struct { name, .. } if *name == "zela.Vec3" => SlotType::Vec3,
                        _ => return None,
                    };
                    types.push(ty);
                }
                if sig.variadic {
                    let last = *types.last()?;
                    types.push(last);
                }
                Some(types)
            })
            .collect()
    }

    #[test]
    fn vector_overloads_emit_vector_math() {
        let op = math("mix");
        let (ret, text) = backend()
            .op(
                &op,
                &[SlotType::Vec3, SlotType::Vec3, SlotType::F64],
                &["a".into(), "b".into(), "t".into()],
            )
            .expect("Vec3 mix 可生成");
        assert_eq!(ret, SlotType::Vec3);
        assert_eq!(text, "mix(a, b, t)");
    }

    #[test]
    fn normalize_uses_the_zero_safe_helper() {
        let op = math("normalize");
        let (ret, text) = backend()
            .op(&op, &[SlotType::Vec2], &["v".into()])
            .expect("Vec2 normalize 可生成");
        assert_eq!(ret, SlotType::Vec2);
        assert_eq!(text, "zela_normalize2(v)");
    }

    #[test]
    fn unknown_namespace_has_no_rule() {
        let op = OpRef::new("acme.math", "sin");
        assert!(backend().op(&op, &[SlotType::F64], &["x".into()]).is_none());
    }

    #[test]
    fn channels_get_stable_identifiers_and_types() {
        let (ty, name) = backend()
            .channel(&ChannelRef::builtin("time"))
            .expect("time 可进 shader");
        assert_eq!(ty, SlotType::F64);
        assert_eq!(name, "c_time");

        let (ty, name) = backend()
            .channel(&ChannelRef::new("zela.scene", "Circle.radius").with_type(
                ze_comb::ChannelType::Number,
            ))
            .expect("组件通道有类型时可进 shader");
        assert_eq!(ty, SlotType::F64);
        assert_eq!(name, "c_zela_scene__Circle_radius");

        assert!(
            backend()
                .channel(&ChannelRef::new("zela.scene", "Circle.radius"))
                .is_none(),
            "类型未知 ⇒ 显式拒绝"
        );
    }

    /// 端到端：一条混合表达式能生成，且两次生成逐字节相同。
    #[test]
    fn end_to_end_emission_is_deterministic() {
        let time = Expr::channel(ChannelRef::builtin("time"));
        let sin = Expr::call(math("sin"), [time.clone()]);
        let scaled = Expr::call(math("mul"), [sin, Expr::pure(2.0)]);
        let expr = Expr::call(math("add"), [scaled, Expr::pure(0.5)]);
        let first = ze_comb::emit_wgsl(&expr, backend()).expect("可生成");
        let second = ze_comb::emit_wgsl(&expr, backend()).expect("可生成");
        assert_eq!(first.source, second.source);
        assert!(first.source.contains("fn eval(p: Params, index: u32) -> f32"));
        assert!(first.source.contains("p.c_time"));
        assert!(first.source.contains("sin("));
    }
}
