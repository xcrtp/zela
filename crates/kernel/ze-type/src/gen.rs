//! 泛型实例化：**按需单态化 + 上界**。
//!
//! # 为什么需要上界
//!
//! `cos × {F32, F64} × {标量, D2, D3, D4}` 是 14 个实例。全量展开会把算子表
//! 变成笛卡尔积；而这个上界问题已经第四次出现（Σ / codata / 求解器 / 单态化）。
//! 纪律同前三次：**上界进产物** —— 超过 [`MAX_INSTANCES`] 是显式错误，
//! 不是静默截断。
//!
//! # 为什么是纯函数
//!
//! 这里只生成"该有哪些实例"的**计划**；真正的缓存（`op + args` → 实例）
//! 是运行时状态，属于 `ze-reflect::cache`。
//!
//! # lift 而非手写
//!
//! 算子表里只有 `cos`，没有 `cos_vec2`。向量实例由 kind 约束（`Vector`）
//! **lift** 出来 —— 加一个向量维度不需要加一行算子表。

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use crate::conflict::{Blame, ReflectError};
use crate::ty::{Kind, OpSig, TypeSpec};

/// 单次单态化允许的实例上界。
pub const MAX_INSTANCES: usize = 64;

/// 一个具体实例：实参类型 + 返回类型。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonoInstance {
    pub args: Vec<TypeSpec>,
    pub ret: TypeSpec,
}

/// 按 kind 约束为一条签名生成具体实例（按变量做笛卡尔积，返回同一条赋值）。
///
/// 同一类型变量在 `params` / `ret` 中复用同一份赋值 —— 这是 rank-1 的核心，
/// 也是 `mix(a: T, b: T, t: Num) -> T` 不会产出 `(F32, F64) -> F32` 的原因。
pub fn monomorphize(sig: &OpSig) -> Result<Vec<MonoInstance>, ReflectError> {
    let groups: Vec<&'static [TypeSpec]> = sig
        .vars
        .iter()
        .map(|(_, kind)| kind.concrete())
        .collect();

    // 没有变量 ⇒ 本身就是具体类型，一个实例。
    if groups.is_empty() {
        return Ok(vec![MonoInstance {
            args: sig.params.iter().map(|ty| **ty).collect(),
            ret: *sig.ret,
        }]);
    }

    // `Bytes` 这类"由注册表决定"的 kind 没有静态候选 ⇒ 不生成实例（不是错误）。
    if groups.iter().any(|candidates| candidates.is_empty()) {
        return Ok(Vec::new());
    }

    let assignments = cartesian(&groups).ok_or_else(|| ReflectError::TypeMismatch {
        expected: format!("单态化实例数 ≤ {MAX_INSTANCES}"),
        actual: format!("{} 个（乘积越界）", product_len(&groups)),
        origin: Blame::Assembly,
    })?;

    let mut instances = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        let args = sig
            .params
            .iter()
            .map(|ty| substitute(ty, &sig.vars, &assignment))
            .collect();
        let ret = substitute(sig.ret, &sig.vars, &assignment);
        instances.push(MonoInstance { args, ret });
    }
    Ok(instances)
}

/// 按 kind 展开的候选（`Bytes` 返回空表：内核不知道私有类型）。
pub fn lift(kind: Kind) -> &'static [TypeSpec] {
    kind.concrete()
}

/// 笛卡尔积；任一维为空 ⇒ 空结果；乘积超过 [`MAX_INSTANCES`] ⇒ `None`（上界显式）。
pub fn cartesian(groups: &[&'static [TypeSpec]]) -> Option<Vec<Vec<TypeSpec>>> {
    let total = product_len(groups);
    if groups.iter().any(|group| group.is_empty()) {
        return Some(Vec::new());
    }
    if total > MAX_INSTANCES {
        return None;
    }
    let mut out: Vec<Vec<TypeSpec>> = vec![Vec::new()];
    for group in groups {
        let mut next = Vec::with_capacity(out.len() * group.len());
        for prefix in &out {
            for candidate in group.iter() {
                let mut row = prefix.clone();
                row.push(*candidate);
                next.push(row);
            }
        }
        out = next;
    }
    Some(out)
}

/// 乘积长度（用于诊断；饱和到 `MAX_INSTANCES + 1`，避免溢出刷屏）。
fn product_len(groups: &[&'static [TypeSpec]]) -> usize {
    let mut total = 1usize;
    for group in groups {
        total = total.saturating_mul(group.len());
        if total > MAX_INSTANCES {
            return MAX_INSTANCES + 1;
        }
    }
    total
}

/// 把类型里的变量替换成当前赋值。
fn substitute(
    spec: &TypeSpec,
    vars: &'static [(&'static str, Kind)],
    assignment: &[TypeSpec],
) -> TypeSpec {
    match spec {
        TypeSpec::Var(name) => vars
            .iter()
            .position(|(candidate, _)| candidate == name)
            .and_then(|index| assignment.get(index).copied())
            .unwrap_or(*spec),
        TypeSpec::Array { elem, len } => TypeSpec::Array {
            elem: static_of(substitute(elem, vars, assignment)),
            len: *len,
        },
        TypeSpec::App { ty, args } => TypeSpec::App {
            ty,
            args: alloc::boxed::Box::leak(
                args.iter()
                    .map(|arg| substitute(arg, vars, assignment))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
        },
        other => *other,
    }
}

/// 复用已有静态描述，避免为标量实例分配。
fn static_of(spec: TypeSpec) -> &'static TypeSpec {
    use crate::ty::ScalarKind;
    static SCALARS: [TypeSpec; 9] = [
        TypeSpec::Scalar(ScalarKind::F32),
        TypeSpec::Scalar(ScalarKind::F64),
        TypeSpec::Scalar(ScalarKind::I32),
        TypeSpec::Scalar(ScalarKind::U32),
        TypeSpec::Scalar(ScalarKind::U64),
        TypeSpec::Scalar(ScalarKind::U8),
        TypeSpec::Scalar(ScalarKind::Bool),
        TypeSpec::Scalar(ScalarKind::String),
        TypeSpec::Scalar(ScalarKind::I64),
    ];
    match spec {
        TypeSpec::Scalar(kind) => {
            let index = match kind {
                ScalarKind::F32 => 0,
                ScalarKind::F64 => 1,
                ScalarKind::I32 => 2,
                ScalarKind::U32 => 3,
                ScalarKind::U64 => 4,
                ScalarKind::U8 => 5,
                ScalarKind::Bool => 6,
                ScalarKind::String => 7,
                ScalarKind::I64 => 8,
            };
            &SCALARS[index]
        }
        other => alloc::boxed::Box::leak(alloc::boxed::Box::new(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty::ScalarKind;

    const F32: TypeSpec = TypeSpec::Scalar(ScalarKind::F32);
    const F64: TypeSpec = TypeSpec::Scalar(ScalarKind::F64);
    const NUM_VARS: &[(&str, Kind)] = &[("T", Kind::Num)];
    const VEC_VARS: &[(&str, Kind)] = &[("V", Kind::Vector)];
    const NUM: &TypeSpec = &TypeSpec::Var("T");
    const VEC: &TypeSpec = &TypeSpec::Var("V");

    const COS: OpSig = OpSig {
        params: &[NUM],
        ret: NUM,
        vars: NUM_VARS,
        variadic: false,
    };
    const MIX_VEC: OpSig = OpSig {
        params: &[VEC, VEC, &TypeSpec::Scalar(ScalarKind::F32)],
        ret: VEC,
        vars: VEC_VARS,
        variadic: false,
    };

    #[test]
    fn num_signature_lifts_to_scalar_instances() {
        let instances = monomorphize(&COS).unwrap();
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].args, vec![F32]);
        assert_eq!(instances[0].ret, F32);
        assert_eq!(instances[1].args, vec![F64]);
        assert_eq!(instances[1].ret, F64);
    }

    /// 向量实例由 lift 生成 —— 算子表里没有 `cos_vec2` 这种条目。
    #[test]
    fn vector_signature_lifts_to_vector_instances() {
        let instances = monomorphize(&MIX_VEC).unwrap();
        assert_eq!(instances.len(), 2);
        let names: Vec<Option<&str>> = instances
            .iter()
            .map(|instance| instance.ret.type_name())
            .collect();
        assert_eq!(names, vec![Some("zela.Vec2"), Some("zela.Vec3")]);
    }

    /// 同一个变量在参数与返回里复用同一份赋值（不会产出 `(F32, F64) -> F32`）。
    #[test]
    fn each_instance_uses_one_assignment_per_variable() {
        for instance in monomorphize(&MIX_VEC).unwrap() {
            assert_eq!(instance.args[0], instance.args[1]);
            assert_eq!(instance.args[0], instance.ret);
        }
    }

    #[test]
    fn upper_bound_is_explicit() {
        let groups = [
            &[F32, F64][..],
            &[F32, F64][..],
            &[F32, F64][..],
            &[F32, F64][..],
            &[F32, F64][..],
            &[F32, F64][..],
            &[F32, F64][..],
        ];
        assert_eq!(product_len(&groups), MAX_INSTANCES + 1);
        assert!(cartesian(&groups).is_none(), "越过上界必须显式失败");
    }

    #[test]
    fn bytes_kind_has_no_static_instances() {
        assert!(lift(Kind::Bytes).is_empty());
    }
}
