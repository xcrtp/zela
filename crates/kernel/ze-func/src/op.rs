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

/// `zela.math` 全部算子（顺序与 [`crate::OPS`] 一致）。
///
/// `spec` 是构造器 PodType 的完整名：存根生成器据此把签名挂到
/// `Sin` / `Mul` … 上（`Sin(...) -> Expr<number>`）。
///
/// `since` / `changed` 是**函数级版本**（第 4 批）：revision 取算子库版本的序号
/// （`1` = 追踪开始前的既有算子，`9` = 第 1 批，`11` = 第 3 批，`12` = 第 5a 批）。
/// 改某个算子的**语义**（签名不变也算）时只 bump 它的 `changed` —— 用到它的
/// payload 会在加载时被**点名**拒绝，而不是整库作废。
pub static DEFS: &[OpDef] = &[
    def("sin", "zela.math.Sin", &[UNARY_NUM], 1, 1),
    def("cos", "zela.math.Cos", &[UNARY_NUM], 1, 1),
    def("add", "zela.math.Add", &[SAME_NUM, SAME_VEC], 1, 1),
    def(
        "mul",
        "zela.math.Mul",
        &[SCALE_NUM, SCALE_VEC_LEFT, SCALE_VEC_RIGHT],
        1,
        1,
    ),
    def(
        "scale",
        "zela.math.Scale",
        &[SCALE_NUM, SCALE_VEC_LEFT, SCALE_VEC_RIGHT],
        1,
        1,
    ),
    def("sum", "zela.math.Sum", &[SUM_NUM, SUM_VEC], 1, 1),
    def("mix", "zela.math.Mix", &[MIX_NUM, MIX_VEC], 9, 9),
    def("clamp", "zela.math.Clamp", &[CLAMP], 9, 9),
    def("vec2", "zela.math.Vec2Of", &[TO_VEC2], 9, 9),
    def("vec3", "zela.math.Vec3Of", &[TO_VEC3], 9, 9),
    def("x", "zela.math.Component", &[COMPONENT], 9, 9),
    def("y", "zela.math.Component", &[COMPONENT], 9, 9),
    def("z", "zela.math.Component", &[COMPONENT], 9, 9),
    def("rotate3", "zela.math.Rotate3", &[ROTATE3], 9, 9),
    def("splat", "zela.math.Splat", &[SPLAT], 11, 11),
    def(
        "vec3_from_vec2",
        "zela.math.Vec3FromVec2",
        &[VEC3_FROM_VEC2],
        11,
        11,
    ),
    def(
        "vec2_from_vec3",
        "zela.math.Vec2FromVec3",
        &[VEC2_FROM_VEC3],
        11,
        11,
    ),
    def("length", "zela.math.Length", &[LENGTH], 11, 11),
    def("normalize", "zela.math.Normalize", &[NORMALIZE], 11, 11),
    def("round_f32", "zela.math.RoundF32", &[ROUND_F32], 12, 12),
    def("lt", "zela.math.Lt", &[CMP_NUM], 1, 1),
    def("gt", "zela.math.Gt", &[CMP_NUM], 1, 1),
    def("le", "zela.math.Le", &[CMP_NUM], 1, 1),
    def("ge", "zela.math.Ge", &[CMP_NUM], 1, 1),
    def("eq", "zela.math.Eq", &[EQ_NUM, EQ_BOOL], 1, 1),
    def("ne", "zela.math.Ne", &[EQ_NUM, EQ_BOOL], 1, 1),
    def("and", "zela.math.And", &[AND], 1, 1),
    def("or", "zela.math.Or", &[AND], 1, 1),
    def("not", "zela.math.Not", &[NOT], 1, 1),
];

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
    /// 目录与算子表**同名同序**：漏登记 / 顺序漂移会在这里当场暴露。
    #[test]
    fn defs_cover_every_op_once() {
        let names: Vec<&str> = super::DEFS.iter().map(|def| def.name).collect();
        assert_eq!(names.as_slice(), crate::ops::OPS);
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
