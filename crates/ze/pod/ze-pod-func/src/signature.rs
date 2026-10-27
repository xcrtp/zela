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

use std::collections::BTreeMap;

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

/// `zela.math` 全部算子的签名表（顺序与 [`crate::OPS`] 一致）。
///
/// `spec` 是构造器 PodType 的完整名：存根生成器据此把签名挂到
/// `Sin` / `Mul` … 上（`Sin(...) -> Expr<number>`）。
///
/// `since` / `changed` 是**函数级版本**（第 4 批）：revision 取算子库版本的序号
/// （`1` = 追踪开始前的既有算子，`9` = 第 1 批，`11` = 第 3 批）。改某个算子的
/// **语义**（签名不变也算）时只 bump 它的 `changed` —— 用到它的 payload 会在加载时
/// 被**点名**拒绝，而不是整库作废。
pub static SIGNATURES: &[OpDef] = &[
    OpDef {
        name: "sin",
        spec: "zela.math.Sin",
        sigs: &[UNARY_NUM],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "cos",
        spec: "zela.math.Cos",
        sigs: &[UNARY_NUM],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "add",
        spec: "zela.math.Add",
        sigs: &[SAME_NUM, SAME_VEC],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "mul",
        spec: "zela.math.Mul",
        sigs: &[SCALE_NUM, SCALE_VEC_LEFT, SCALE_VEC_RIGHT],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "scale",
        spec: "zela.math.Scale",
        sigs: &[SCALE_NUM, SCALE_VEC_LEFT, SCALE_VEC_RIGHT],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "sum",
        spec: "zela.math.Sum",
        sigs: &[SUM_NUM, SUM_VEC],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "mix",
        spec: "zela.math.Mix",
        sigs: &[MIX_NUM, MIX_VEC],
        since: 9,
        changed: 9,
    },
    OpDef {
        name: "clamp",
        spec: "zela.math.Clamp",
        sigs: &[CLAMP],
        since: 9,
        changed: 9,
    },
    OpDef {
        name: "vec2",
        spec: "zela.math.Vec2Of",
        sigs: &[TO_VEC2],
        since: 9,
        changed: 9,
    },
    OpDef {
        name: "vec3",
        spec: "zela.math.Vec3Of",
        sigs: &[TO_VEC3],
        since: 9,
        changed: 9,
    },
    OpDef {
        name: "x",
        spec: "zela.math.Component",
        sigs: &[COMPONENT],
        since: 9,
        changed: 9,
    },
    OpDef {
        name: "y",
        spec: "zela.math.Component",
        sigs: &[COMPONENT],
        since: 9,
        changed: 9,
    },
    OpDef {
        name: "z",
        spec: "zela.math.Component",
        sigs: &[COMPONENT],
        since: 9,
        changed: 9,
    },
    OpDef {
        name: "rotate3",
        spec: "zela.math.Rotate3",
        sigs: &[ROTATE3],
        since: 9,
        changed: 9,
    },
    OpDef {
        name: "splat",
        spec: "zela.math.Splat",
        sigs: &[SPLAT],
        since: 11,
        changed: 11,
    },
    OpDef {
        name: "vec3_from_vec2",
        spec: "zela.math.Vec3FromVec2",
        sigs: &[VEC3_FROM_VEC2],
        since: 11,
        changed: 11,
    },
    OpDef {
        name: "vec2_from_vec3",
        spec: "zela.math.Vec2FromVec3",
        sigs: &[VEC2_FROM_VEC3],
        since: 11,
        changed: 11,
    },
    OpDef {
        name: "length",
        spec: "zela.math.Length",
        sigs: &[LENGTH],
        since: 11,
        changed: 11,
    },
    OpDef {
        name: "normalize",
        spec: "zela.math.Normalize",
        sigs: &[NORMALIZE],
        since: 11,
        changed: 11,
    },
    OpDef {
        name: "round_f32",
        spec: "zela.math.RoundF32",
        sigs: &[ROUND_F32],
        since: 12,
        changed: 12,
    },
    OpDef {
        name: "lt",
        spec: "zela.math.Lt",
        sigs: &[CMP_NUM],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "gt",
        spec: "zela.math.Gt",
        sigs: &[CMP_NUM],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "le",
        spec: "zela.math.Le",
        sigs: &[CMP_NUM],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "ge",
        spec: "zela.math.Ge",
        sigs: &[CMP_NUM],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "eq",
        spec: "zela.math.Eq",
        sigs: &[EQ_NUM, EQ_BOOL],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "ne",
        spec: "zela.math.Ne",
        sigs: &[EQ_NUM, EQ_BOOL],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "and",
        spec: "zela.math.And",
        sigs: &[AND],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "or",
        spec: "zela.math.Or",
        sigs: &[AND],
        since: 1,
        changed: 1,
    },
    OpDef {
        name: "not",
        spec: "zela.math.Not",
        sigs: &[NOT],
        since: 1,
        changed: 1,
    },
];

/// 查一个算子的签名（未知算子返回 `None`）。
pub fn signature_of(name: &str) -> Option<&'static OpDef> {
    SIGNATURES.iter().find(|def| def.name == name)
}

/// 某算子的 `(since, changed)`（产物记录 / 加载比对用）。
pub fn op_revision(name: &str) -> Option<(u32, u32)> {
    signature_of(name).map(|def| (def.since, def.changed))
}

/// 表达式树里用到的**值算子** `(since, changed)`（产物记录用）。
///
/// 只记本库（`zela.math`）的 `Call` 节点；`Read` / `Select` / 通道叶子没有算子
/// revision（它们不是 [`SIGNATURES`] 里的值算子）。`Ref` 的子树是另一份 `Expr`，
/// 照常遍历。
pub fn collect_op_revisions(expr: &ze_func::Expr) -> BTreeMap<String, (u32, u32)> {
    fn walk(expr: &ze_func::Expr, out: &mut BTreeMap<String, (u32, u32)>) {
        if let ze_func::Node::Call { op, .. } = &expr.node {
            if op.namespace == "zela.math" {
                if let Some(revision) = op_revision(&op.name) {
                    out.insert(op.name.clone(), revision);
                }
            }
        }
        for child in expr.node.children() {
            walk(child, out);
        }
    }
    let mut out = BTreeMap::new();
    walk(expr, &mut out);
    out
}

/// **加载期比对**（第 4 批）：产物记录的算子 revision vs 当前库。
///
/// 不符 ⇒ 点名**具体算子**（不是笼统的"库版本不符"）。空表（老 payload）直接通过：
/// 回放路径不做推导，缺记录是合法状态。
pub fn check_op_revisions(revisions: &BTreeMap<String, (u32, u32)>) -> Result<(), String> {
    let mut stale = Vec::new();
    for (name, &(since, changed)) in revisions {
        match op_revision(name) {
            None => stale.push(format!("`{name}`（当前库已无此算子）")),
            Some((current_since, current_changed))
                if current_since != since || current_changed != changed =>
            {
                stale.push(format!(
                    "`{name}`（产物 since/changed = {since}/{changed}，当前 = {current_since}/{current_changed}）"
                ));
            }
            Some(_) => {}
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "表达式产物与当前 `zela.math` 不兼容：{}",
            stale.join("；")
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

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

    /// 第 4 批：函数级 revision 与批次对应（既有 = 1、第 1 批 = 9、第 3 批 = 11）。
    #[test]
    fn op_revisions_follow_the_batches() {
        assert_eq!(super::op_revision("sin"), Some((1, 1)));
        assert_eq!(super::op_revision("mix"), Some((9, 9)));
        assert_eq!(super::op_revision("normalize"), Some((11, 11)));
        assert_eq!(super::op_revision("read"), None, "read 不是值算子");
    }

    /// 第 4 批验收：产物记录与当前库不符 ⇒ **点名算子**；空表（老 payload）通过。
    #[test]
    fn stale_op_revision_names_the_operator() {
        let mut stale = BTreeMap::new();
        stale.insert("sin".to_string(), (1, 2));
        let error = super::check_op_revisions(&stale).expect_err("语义变更必须拒绝");
        assert!(error.contains("`sin`"), "{error}");
        assert!(error.contains("1/2"), "{error}");

        let mut gone = BTreeMap::new();
        gone.insert("tan".to_string(), (1, 1));
        let error = super::check_op_revisions(&gone).expect_err("已删除的算子必须拒绝");
        assert!(error.contains("`tan`"), "{error}");

        assert!(super::check_op_revisions(&BTreeMap::new()).is_ok());
        let mut current = BTreeMap::new();
        current.insert("sin".to_string(), super::op_revision("sin").unwrap());
        assert!(super::check_op_revisions(&current).is_ok());
    }
}

