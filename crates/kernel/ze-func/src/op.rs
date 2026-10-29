//! **算子目录**：签名 + 语义元数据，**唯一一张表**。
//!
//! # 为什么只有一张
//!
//! 计划 §0.1：`OpDef` 曾有两份（`ze-type` 持 `spec/sigs`、本 crate 持
//! `pointwise/targets`），靠"名字相同"隐式对应 ⇒ 任何一边漏登记都不会报错。
//! 现在 `OpDef` 只有一处定义（`ze-type`），本模块持有**唯一的目录**：
//!
//! ```text
//! DEFS: &[OpDef] = 签名（sigs / spec）+ 版本（since / changed）
//!                + 语义（pointwise）+ 出口（targets）
//! ```
//!
//! # 查表是 O(1)
//!
//! 名字在首次查询时解析一次进符号表，之后是整数槽位（计划 §3.5 的同一机制）。

use std::sync::OnceLock;

use ze_reflect::{Blame, SymbolTable};
pub use ze_type::{OpDef, Target};
use ze_type::{Kind, OpSig, ScalarKind, TypeSpec};

/// 本函子库的 namespace（进产物）。
pub const LIBRARY_NAMESPACE: &str = "zela.math";

/// 当前全部算子：只由宿主实现。
const HOST_ONLY: &[Target] = &[Target::HostOnly];

const F64: &TypeSpec = &TypeSpec::Scalar(ScalarKind::F64);
const BOOL: &TypeSpec = &TypeSpec::Scalar(ScalarKind::Bool);
const VAR_A: &TypeSpec = &TypeSpec::Var("a");

const NUM_A: &[(&str, Kind)] = &[("a", Kind::Num)];
const VEC_A: &[(&str, Kind)] = &[("a", Kind::Vector)];
const NO_VARS: &[(&str, Kind)] = &[];

/// 一元数值：`sin` / `cos`。
const UNARY_NUM: OpSig = OpSig {
    params: &[F64],
    ret: F64,
    vars: NO_VARS,
    variadic: false,
};

/// 二元同类型：`add` 的两种（数值 / 向量）。
const SAME_NUM: OpSig = OpSig {
    params: &[VAR_A, VAR_A],
    ret: VAR_A,
    vars: NUM_A,
    variadic: false,
};
const SAME_VEC: OpSig = OpSig {
    params: &[VAR_A, VAR_A],
    ret: VAR_A,
    vars: VEC_A,
    variadic: false,
};

/// `mul` / `scale`：数值×数值、向量×数值、数值×向量（**返回值绑在向量那一侧**）。
const SCALE_NUM: OpSig = OpSig {
    params: &[F64, F64],
    ret: F64,
    vars: NO_VARS,
    variadic: false,
};
const SCALE_VEC_LEFT: OpSig = OpSig {
    params: &[VAR_A, F64],
    ret: VAR_A,
    vars: VEC_A,
    variadic: false,
};
const SCALE_VEC_RIGHT: OpSig = OpSig {
    params: &[F64, VAR_A],
    ret: VAR_A,
    vars: VEC_A,
    variadic: false,
};

/// `sum`：任意多项同类型求和（变参）。
const SUM_NUM: OpSig = OpSig {
    params: &[VAR_A],
    ret: VAR_A,
    vars: NUM_A,
    variadic: true,
};
const SUM_VEC: OpSig = OpSig {
    params: &[VAR_A],
    ret: VAR_A,
    vars: VEC_A,
    variadic: true,
};

// ─── 第 1 批（算子补齐）────────────────────────────────────────────────

const VEC2: &TypeSpec = &TypeSpec::Struct {
    name: "zela.Vec2",
    layout: None,
    fields: &[],
};
const VEC3: &TypeSpec = &TypeSpec::Struct {
    name: "zela.Vec3",
    layout: None,
    fields: &[],
};

/// `mix(a, b, t)`：`(T, T, F64) → T`，`T ∈ Num | Vector`。
const MIX_NUM: OpSig = OpSig {
    params: &[VAR_A, VAR_A, F64],
    ret: VAR_A,
    vars: NUM_A,
    variadic: false,
};
const MIX_VEC: OpSig = OpSig {
    params: &[VAR_A, VAR_A, F64],
    ret: VAR_A,
    vars: VEC_A,
    variadic: false,
};

/// `clamp(x, lo, hi)`：`(F64, F64, F64) → F64`。
const CLAMP: OpSig = OpSig {
    params: &[F64, F64, F64],
    ret: F64,
    vars: NO_VARS,
    variadic: false,
};

/// `vec2(x, y)` / `vec3(x, y, z)`：标量 → 向量（显式构造，不做隐式提升）。
const TO_VEC2: OpSig = OpSig {
    params: &[F64, F64],
    ret: VEC2,
    vars: NO_VARS,
    variadic: false,
};
const TO_VEC3: OpSig = OpSig {
    params: &[F64, F64, F64],
    ret: VEC3,
    vars: NO_VARS,
    variadic: false,
};

/// `x(v)` / `y(v)` / `z(v)`：向量分量（`Vector → F64`）。
const COMPONENT: OpSig = OpSig {
    params: &[VAR_A],
    ret: F64,
    vars: VEC_A,
    variadic: false,
};

/// `rotate3(v, axis, angle)`：罗德里格斯旋转（`Vec3, Vec3, F64 → Vec3`）。
const ROTATE3: OpSig = OpSig {
    params: &[VEC3, VEC3, F64],
    ret: VEC3,
    vars: NO_VARS,
    variadic: false,
};

// ─── 第 3 批（类型系统扩展）：转换算子族 ────────────────────────────────

/// `splat(v)`：`F64 → Vec3`（三个分量同值）。
const SPLAT: OpSig = OpSig {
    params: &[F64],
    ret: VEC3,
    vars: NO_VARS,
    variadic: false,
};

/// `vec3_from_vec2(v, z)`：`Vec2, F64 → Vec3`。
const VEC3_FROM_VEC2: OpSig = OpSig {
    params: &[VEC2, F64],
    ret: VEC3,
    vars: NO_VARS,
    variadic: false,
};

/// `vec2_from_vec3(v)`：`Vec3 → Vec2`（显式丢弃 z）。
const VEC2_FROM_VEC3: OpSig = OpSig {
    params: &[VEC3],
    ret: VEC2,
    vars: NO_VARS,
    variadic: false,
};

/// `length(v)`：`Vector → F64`。
const LENGTH: OpSig = OpSig {
    params: &[VAR_A],
    ret: F64,
    vars: VEC_A,
    variadic: false,
};

/// `normalize(v)`：`Vector → Vector`（零向量保持零）。
const NORMALIZE: OpSig = OpSig {
    params: &[VAR_A],
    ret: VAR_A,
    vars: VEC_A,
    variadic: false,
};

/// `round_f32(x)`：`F64 → F64`（落成 f32 再升回；逐位复刻旧曲线用）。
const ROUND_F32: OpSig = OpSig {
    params: &[F64],
    ret: F64,
    vars: NO_VARS,
    variadic: false,
};

/// `distance(a, b)`：两个同维向量 → `F64`。
const DISTANCE2: OpSig = OpSig {
    params: &[VEC2, VEC2],
    ret: F64,
    vars: NO_VARS,
    variadic: false,
};
const DISTANCE3: OpSig = OpSig {
    params: &[VEC3, VEC3],
    ret: F64,
    vars: NO_VARS,
    variadic: false,
};

/// `smoothstep(e0, e1, x)`：`(F64, F64, F64) → F64`。
const SMOOTHSTEP: OpSig = OpSig {
    params: &[F64, F64, F64],
    ret: F64,
    vars: NO_VARS,
    variadic: false,
};

/// `fract(x)`：`F64 → F64`（小数部分，靠 `floor` 定义）。
const FRACT: OpSig = OpSig {
    params: &[F64],
    ret: F64,
    vars: NO_VARS,
    variadic: false,
};

/// 比较：数值 → Bool。
const CMP_NUM: OpSig = OpSig {
    params: &[F64, F64],
    ret: BOOL,
    vars: NO_VARS,
    variadic: false,
};

/// 相等：数值或布尔。
const EQ_NUM: OpSig = OpSig {
    params: &[F64, F64],
    ret: BOOL,
    vars: NO_VARS,
    variadic: false,
};
const EQ_BOOL: OpSig = OpSig {
    params: &[BOOL, BOOL],
    ret: BOOL,
    vars: NO_VARS,
    variadic: false,
};

/// 布尔组合子：**只吃 Bool**（最严）。
const AND: OpSig = OpSig {
    params: &[BOOL, BOOL],
    ret: BOOL,
    vars: NO_VARS,
    variadic: false,
};
const NOT: OpSig = OpSig {
    params: &[BOOL],
    ret: BOOL,
    vars: NO_VARS,
    variadic: false,
};

/// 目录条目：签名 + 版本；语义默认逐点、仅宿主（改语义时显式覆盖）。
const fn def(
    name: &'static str,
    spec: &'static str,
    sigs: &'static [OpSig],
    since: u32,
    changed: u32,
) -> OpDef {
    OpDef {
        name,
        spec,
        sigs,
        since,
        changed,
        // 全部现有算子都是纯函数（同一实参 ⇒ 同一结果），属于连续层。
        pointwise: true,
        targets: HOST_ONLY,
    }
}

/// 语义实现签名（`ops.rs` 里的同名函数）。
pub type EvalFn =
    fn(&ze_comb::OpRef, &[ze_comb::Value]) -> Result<ze_comb::Value, ze_comb::EvalError>;

/// **唯一登记点**：加一个算子只改这里的一行（元数据 + 语义在同一行）。
///
/// 一次展开同时产出三样东西，且**名字只写一遍**：
///
/// - [`DEFS`]：`OpDef` 目录（签名 + `since` / `changed` + pointwise / targets）；
/// - [`OPS`]：名字清单（装配期符号表据此建立，O(1) 查表）；
/// - `EVALS`：名称 → 实现（`ops::eval` 查表分派，**没有 `match`**）。
///
/// `$name` 同时是 `ops` 模块里的实现函数名 —— 少写实现会直接**编译失败**，
/// 少写目录行则算子根本不存在（不存在"登记了但没实现"的中间态）。
macro_rules! math_ops {
    ($(
        $name:ident : $spec:literal, [$($sig:expr),*], $since:literal, $changed:literal;
    )*) => {
        /// `zela.math` 全部算子（顺序 = 声明顺序）。
        ///
        /// `spec` 是构造器 PodType 的完整名：存根生成器据此把签名挂到
        /// `Sin` / `Mul` … 上（`Sin(...) -> Expr<number>`）。
        ///
        /// `since` / `changed` 是**函数级版本**：改某个算子的**语义**（签名不变也算）
        /// 时只 bump 它的 `changed` —— 用到它的 payload 会在加载时被**点名**拒绝。
        pub static DEFS: &[OpDef] = &[
            $( def(stringify!($name), $spec, &[$($sig),*], $since, $changed) ),*
        ];

        /// 算子名清单（与 [`DEFS`] 同一次展开产生）。
        pub static OPS: &[&str] = &[ $( stringify!($name) ),* ];

        /// 语义注册表（名称 → 实现），与 [`DEFS`] 同一次展开产生。
        pub(crate) static EVALS: &[(&str, EvalFn)] =
            &[ $( (stringify!($name), crate::ops::$name) ),* ];
    };
}

math_ops! {
    sin: "zela.math.Sin", [UNARY_NUM], 1, 1;
    cos: "zela.math.Cos", [UNARY_NUM], 1, 1;
    add: "zela.math.Add", [SAME_NUM, SAME_VEC], 1, 18;
    mul: "zela.math.Mul", [SCALE_NUM, SCALE_VEC_LEFT, SCALE_VEC_RIGHT], 1, 18;
    scale: "zela.math.Scale", [SCALE_NUM, SCALE_VEC_LEFT, SCALE_VEC_RIGHT], 1, 18;
    sum: "zela.math.Sum", [SUM_NUM, SUM_VEC], 1, 18;
    mix: "zela.math.Mix", [MIX_NUM, MIX_VEC], 9, 9;
    clamp: "zela.math.Clamp", [CLAMP], 9, 9;
    vec2: "zela.math.Vec2Of", [TO_VEC2], 9, 9;
    vec3: "zela.math.Vec3Of", [TO_VEC3], 9, 9;
    x: "zela.math.Component", [COMPONENT], 9, 9;
    y: "zela.math.Component", [COMPONENT], 9, 9;
    z: "zela.math.Component", [COMPONENT], 9, 9;
    rotate3: "zela.math.Rotate3", [ROTATE3], 9, 9;
    splat: "zela.math.Splat", [SPLAT], 11, 11;
    vec3_from_vec2: "zela.math.Vec3FromVec2", [VEC3_FROM_VEC2], 11, 11;
    vec2_from_vec3: "zela.math.Vec2FromVec3", [VEC2_FROM_VEC3], 11, 11;
    length: "zela.math.Length", [LENGTH], 11, 11;
    normalize: "zela.math.Normalize", [NORMALIZE], 11, 11;
    round_f32: "zela.math.RoundF32", [ROUND_F32], 12, 12;
    distance: "zela.math.Distance", [DISTANCE2, DISTANCE3], 21, 21;
    smoothstep: "zela.math.Smoothstep", [SMOOTHSTEP], 21, 21;
    fract: "zela.math.Fract", [FRACT], 21, 21;
    lt: "zela.math.Lt", [CMP_NUM], 1, 1;
    gt: "zela.math.Gt", [CMP_NUM], 1, 1;
    le: "zela.math.Le", [CMP_NUM], 1, 1;
    ge: "zela.math.Ge", [CMP_NUM], 1, 1;
    eq: "zela.math.Eq", [EQ_NUM, EQ_BOOL], 1, 1;
    ne: "zela.math.Ne", [EQ_NUM, EQ_BOOL], 1, 1;
    and: "zela.math.And", [AND], 1, 1;
    or: "zela.math.Or", [AND], 1, 1;
    not: "zela.math.Not", [NOT], 1, 1;
}

/// 名字 → 语义（`ops::eval` 的查表入口；O(1)）。
pub(crate) fn eval_entry(name: &str) -> Option<EvalFn> {
    eval_index().resolve(name).map(|(_, eval)| *eval)
}

/// 名字 → 实现的符号表（`OnceLock`：只建一次）。
fn eval_index() -> &'static SymbolTable<&'static str, EvalFn> {
    static INDEX: OnceLock<SymbolTable<&'static str, EvalFn>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut table = SymbolTable::new();
        for &(name, eval) in EVALS {
            table
                .intern(name, eval, Blame::Assembly)
                .expect("EVALS 的名字唯一（与 DEFS 同一次展开）");
        }
        table
    })
}

/// 查一个算子的完整描述（**O(1)**：符号表在首次查询时建立一次）。
pub fn op_def(name: &str) -> Option<&'static OpDef> {
    index().resolve(name).map(|(_, def)| *def)
}

/// 查一个算子的签名（`op_def` 的别名：签名与元数据同一张表）。
pub fn signature_of(name: &str) -> Option<&'static OpDef> {
    op_def(name)
}

/// 名字 → 条目的符号表（`OnceLock`：只建一次）。
fn index() -> &'static SymbolTable<&'static str, &'static OpDef> {
    static INDEX: OnceLock<SymbolTable<&'static str, &'static OpDef>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut table = SymbolTable::new();
        for def in DEFS {
            table
                .intern(def.name, def, Blame::Assembly)
                .expect("DEFS 的名字唯一（顺序测试已钉住）");
        }
        table
    })
}

/// 某算子的 `(since, changed)`（产物记录 / 加载比对用）。
pub fn op_revision(name: &str) -> Option<(u32, u32)> {
    op_def(name).map(|def| (def.since, def.changed))
}

/// 算子是否逐点（未知算子返回 `None`，不静默当作 `true`）。
pub fn is_pointwise(name: &str) -> Option<bool> {
    op_def(name).map(|def| def.pointwise)
}

/// 算子出口能力（未知算子返回 `None`）。
pub fn targets(name: &str) -> Option<&'static [Target]> {
    op_def(name).map(|def| def.targets)
}

/// 完整限定名（`zela.math::sin`）。
pub fn qualified(def: &OpDef) -> String {
    format!("{LIBRARY_NAMESPACE}::{}", def.name)
}

#[cfg(test)]
mod tests {
    /// 目录与名字清单**同名同序**（两者由同一次 `math_ops!` 展开产生）。
    #[test]
    fn defs_cover_every_op_once() {
        let names: Vec<&str> = super::DEFS.iter().map(|def| def.name).collect();
        assert_eq!(names.as_slice(), super::OPS);
    }

    /// **阶段 4 门禁**：目录里的每个名字都必须能查到语义实现（反之亦然）。
    ///
    /// `math_ops!` 已经让"漏写实现"变成编译错误；本用例再钉住**运行期**的注册表
    /// 与目录逐条对应（防止有人把 `EVALS` 改成手写）。
    #[test]
    fn every_def_has_an_eval_implementation() {
        for def in super::DEFS {
            assert!(
                super::eval_entry(def.name).is_some(),
                "算子 `{}` 没有语义实现",
                def.name
            );
        }
        assert_eq!(super::EVALS.len(), super::DEFS.len());
        assert_eq!(
            super::EVALS.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            super::OPS
        );
        assert!(super::eval_entry("warp").is_none(), "未知算子不静默查表命中");
    }

    /// 每条目录条目都带签名与构造器名（不再是"半张表"）。
    #[test]
    fn every_def_carries_signatures_and_spec() {
        for def in super::DEFS {
            assert!(!def.sigs.is_empty(), "`{}` 缺签名", def.name);
            assert!(
                def.spec.starts_with("zela.math."),
                "`{}` 的 spec 不是本库限定名：{}",
                def.name,
                def.spec
            );
        }
    }

    /// `changed` 记录的是**语义最后一次变更**：`add` / `mul` / `scale` / `sum` 在
    /// AU-11 补齐了签名早就声明的向量重载 ⇒ `changed = 18`（`since` 不变）。
    #[test]
    fn vector_overloads_landed_at_revision_18() {
        assert_eq!(super::op_revision("add"), Some((1, 18)));
        assert_eq!(super::op_revision("mul"), Some((1, 18)));
        assert_eq!(super::op_revision("scale"), Some((1, 18)));
        assert_eq!(super::op_revision("sum"), Some((1, 18)));
        assert_eq!(super::op_revision("sin"), Some((1, 1)), "未改语义的算子不 bump");
    }

    /// 既有算子的 revision 与批次对应（1 = 追踪开始前、9 = 第 1 批、11 = 第 3 批、12 = 第 5a 批）。
    #[test]
    fn revisions_follow_the_batches() {
        assert_eq!(super::op_revision("sin"), Some((1, 1)));
        assert_eq!(super::op_revision("mix"), Some((9, 9)));
        assert_eq!(super::op_revision("normalize"), Some((11, 11)));
        assert_eq!(super::op_revision("round_f32"), Some((12, 12)));
        assert_eq!(super::op_revision("read"), None, "read 不是值算子");
    }

    /// 现有算子全部声明为连续层（逐点）；`targets` 占位是"仅宿主"。
    #[test]
    fn every_op_is_pointwise_and_host_only_for_now() {
        for def in super::DEFS {
            assert!(def.pointwise, "{} 应逐点", def.name);
            assert_eq!(def.targets, &[super::Target::HostOnly]);
            assert_eq!(super::qualified(def), format!("zela.math::{}", def.name));
        }
    }

    /// 未知算子不静默返回 `true` / 空能力。
    #[test]
    fn unknown_ops_are_explicit() {
        assert_eq!(super::is_pointwise("warp"), None);
        assert_eq!(super::targets("warp"), None);
    }
}
