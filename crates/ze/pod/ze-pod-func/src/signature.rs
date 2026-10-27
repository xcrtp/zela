//! **算子签名表**（L2 类型化的数据面）。
//!
//! 每条 [`OpSig`] 是 rank-1 多态 + kind 约束（`Num` / `Vector` / `Bool`）。
//! 为什么需要它：`mul` 的 `Vec2 × F64 → Vec2` 里**返回类型等于第一个参数的类型**，
//! L0 的单态重载列表必须穷举所有几何组合；有了类型变量 + kind，一行就够。
//!
//! 本文件只放**数据**：
//! - 文档生成器把 [`OpDef::render_lines`] 打进声明文件（`Sin: (F64) -> F64`）；
//! - 装配期（B 批）逐条尝试合一，多条成功 ⇒ **显式歧义错误**（硬约束 4）。
//!
//! 签名里 [`TypeSpec::Var`] 的名字只是 `vars` 的键，**不是**自由命名空间。

use ze_pod_core::{Kind, OpDef, OpSig, ScalarKind, TypeSpec};

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
    fields: &[],
};
const VEC3: &TypeSpec = &TypeSpec::Struct {
    name: "zela.Vec3",
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

/// `zela.math` 全部算子的签名表（顺序与 [`crate::OPS`] 一致）。
///
/// `spec` 是构造器 PodType 的完整名：存根生成器据此把签名挂到
/// `Sin` / `Mul` … 上（`Sin(...) -> Expr<number>`）。
pub static SIGNATURES: &[OpDef] = &[
    OpDef {
        name: "sin",
        spec: "zela.math.Sin",
        sigs: &[UNARY_NUM],
    },
    OpDef {
        name: "cos",
        spec: "zela.math.Cos",
        sigs: &[UNARY_NUM],
    },
    OpDef {
        name: "add",
        spec: "zela.math.Add",
        sigs: &[SAME_NUM, SAME_VEC],
    },
    OpDef {
        name: "mul",
        spec: "zela.math.Mul",
        sigs: &[SCALE_NUM, SCALE_VEC_LEFT, SCALE_VEC_RIGHT],
    },
    OpDef {
        name: "scale",
        spec: "zela.math.Scale",
        sigs: &[SCALE_NUM, SCALE_VEC_LEFT, SCALE_VEC_RIGHT],
    },
    OpDef {
        name: "sum",
        spec: "zela.math.Sum",
        sigs: &[SUM_NUM, SUM_VEC],
    },
    OpDef {
        name: "mix",
        spec: "zela.math.Mix",
        sigs: &[MIX_NUM, MIX_VEC],
    },
    OpDef {
        name: "clamp",
        spec: "zela.math.Clamp",
        sigs: &[CLAMP],
    },
    OpDef {
        name: "vec2",
        spec: "zela.math.Vec2Of",
        sigs: &[TO_VEC2],
    },
    OpDef {
        name: "vec3",
        spec: "zela.math.Vec3Of",
        sigs: &[TO_VEC3],
    },
    OpDef {
        name: "x",
        spec: "zela.math.Component",
        sigs: &[COMPONENT],
    },
    OpDef {
        name: "y",
        spec: "zela.math.Component",
        sigs: &[COMPONENT],
    },
    OpDef {
        name: "z",
        spec: "zela.math.Component",
        sigs: &[COMPONENT],
    },
    OpDef {
        name: "rotate3",
        spec: "zela.math.Rotate3",
        sigs: &[ROTATE3],
    },
    OpDef {
        name: "splat",
        spec: "zela.math.Splat",
        sigs: &[SPLAT],
    },
    OpDef {
        name: "vec3_from_vec2",
        spec: "zela.math.Vec3FromVec2",
        sigs: &[VEC3_FROM_VEC2],
    },
    OpDef {
        name: "vec2_from_vec3",
        spec: "zela.math.Vec2FromVec3",
        sigs: &[VEC2_FROM_VEC3],
    },
    OpDef {
        name: "length",
        spec: "zela.math.Length",
        sigs: &[LENGTH],
    },
    OpDef {
        name: "normalize",
        spec: "zela.math.Normalize",
        sigs: &[NORMALIZE],
    },
    OpDef {
        name: "lt",
        spec: "zela.math.Lt",
        sigs: &[CMP_NUM],
    },
    OpDef {
        name: "gt",
        spec: "zela.math.Gt",
        sigs: &[CMP_NUM],
    },
    OpDef {
        name: "le",
        spec: "zela.math.Le",
        sigs: &[CMP_NUM],
    },
    OpDef {
        name: "ge",
        spec: "zela.math.Ge",
        sigs: &[CMP_NUM],
    },
    OpDef {
        name: "eq",
        spec: "zela.math.Eq",
        sigs: &[EQ_NUM, EQ_BOOL],
    },
    OpDef {
        name: "ne",
        spec: "zela.math.Ne",
        sigs: &[EQ_NUM, EQ_BOOL],
    },
    OpDef {
        name: "and",
        spec: "zela.math.And",
        sigs: &[AND],
    },
    OpDef {
        name: "or",
        spec: "zela.math.Or",
        sigs: &[AND],
    },
    OpDef {
        name: "not",
        spec: "zela.math.Not",
        sigs: &[NOT],
    },
];

/// 查一个算子的签名（未知算子返回 `None`）。
pub fn signature_of(name: &str) -> Option<&'static OpDef> {
    SIGNATURES.iter().find(|def| def.name == name)
}

#[cfg(test)]
mod tests {
    /// 签名表与算子表**同名同序**：漏登记会在这里当场暴露。
    #[test]
    fn signatures_cover_every_op_once() {
        let names: Vec<&str> = super::SIGNATURES.iter().map(|def| def.name).collect();
        assert_eq!(names.as_slice(), crate::ops::OPS);
    }

    /// 文档行用构造器名（`Sin`），与脚本侧的 `Sin { … }` 一致。
    #[test]
    fn signature_renders_for_docs() {
        let sin = super::signature_of("sin").expect("sin 已登记");
        assert_eq!(sin.render_lines(), vec!["Sin: (F64) -> F64"]);

        let mul = super::signature_of("mul").expect("mul 已登记");
        assert_eq!(
            mul.render_lines(),
            vec![
                "Mul: (F64, F64) -> F64",
                "Mul: (Vector, F64) -> Vector",
                "Mul: (F64, Vector) -> Vector",
            ]
        );
    }
}

