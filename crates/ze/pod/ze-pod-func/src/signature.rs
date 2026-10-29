//! **算子签名表的接线层**：数据面在 `ze-func`（[`OpDef`] 的唯一来源）。
//!
//! 计划 §0.1：签名与语义元数据曾经分居两张表，靠"名字相同"隐式对应。
//! 现在 `ze-func::OP_DEFS` 是**唯一**目录，本模块只保留 Pod 侧的两件事：
//! - 产物里的函数级 revision 记录 / 加载期点名比对；
//! - 装配期 pass 登记。
//!
//! 签名与元数据的查询直接转发到 `ze-func`（同一张表、同一个符号索引）。

use std::collections::BTreeMap;

pub use ze_func::{OP_DEFS as SIGNATURES, op_revision, signature_of};
pub use ze_pod_core::OpDef;

/// 表达式树里用到的**值算子** `(since, changed)`（产物记录用）。
///
/// 只记本库（`zela.math`）的 `Call` 节点；`Read` / `Select` / 通道叶子没有算子
/// revision（它们不是 [`SIGNATURES`] 里的值算子）。`Ref` 的子树是另一份 `Expr`，
/// 照常遍历。
pub fn collect_op_revisions(expr: &ze_comb::Expr) -> BTreeMap<String, (u32, u32)> {
    fn walk(expr: &ze_comb::Expr, out: &mut BTreeMap<String, (u32, u32)>) {
        if let ze_comb::Node::Call { op, .. } = &expr.node {
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

/// **阶段 0**：算子 revision 收集也是装配期分析 pass（只读 IR、写 side table）。
pub fn register_passes(registry: &mut ze_comb::PassRegistry) {
    registry
        .register(ze_comb::PassInfo {
            name: "expr.op_revisions",
            kind: ze_comb::PassKind::Analysis,
            semantics: ze_comb::Semantics::Preserving,
            owner: "ze-pod-func",
        })
        .expect("op revision pass 名唯一");
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

    /// **阶段 0 判据**：只有一个 `OpDef`、只有一张表 —— 两个名字指向同一份数据。
    #[test]
    fn one_table_backs_both_names() {
        assert!(
            std::ptr::eq(ze_func::OP_DEFS, super::SIGNATURES),
            "SIGNATURES 必须是 OP_DEFS 的别名，不是第二张表"
        );
        for def in super::SIGNATURES {
            assert_eq!(
                (def.since, def.changed),
                ze_func::op_revision(def.name).expect("同一张表"),
                "算子 `{}` 的 revision 漂移",
                def.name
            );
        }
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

    /// **阶段 4**：向量实例由 kind **lift** 生成，算子表里不出现 `cos_vec2` 之类条目。
    #[test]
    fn vector_instances_are_lifted_not_enumerated() {
        use ze_type::Kind;

        // 这些算子的向量版本必须由 kind lift，而不是逐条登记。
        for base in [
            "sin", "cos", "add", "mul", "scale", "sum", "mix", "clamp", "length", "normalize",
        ] {
            for suffix in ["_vec2", "_vec3"] {
                let lifted = format!("{base}{suffix}");
                assert!(
                    !crate::OPS.contains(&lifted.as_str()),
                    "算子表不得按维度逐条登记（`{lifted}`）"
                );
            }
        }
        assert!(super::signature_of("cos_vec2").is_none());

        let cos = super::signature_of("cos").expect("cos 已登记");
        let mut scalar_cache = ze_reflect::MonoCache::default();
        let scalar = scalar_cache
            .instances("cos", &cos.sigs[0])
            .expect("单态化在界内");
        assert_eq!(scalar.len(), 1, "标量签名已是具体类型（F64）");
        assert_eq!(scalar[0].ret.type_name(), None);

        let mul = super::signature_of("mul").expect("mul 已登记");
        let vector_sig = mul
            .sigs
            .iter()
            .find(|sig| sig.vars.iter().any(|(_, kind)| *kind == Kind::Vector))
            .expect("mul 有向量重载");
        let mut vector_cache = ze_reflect::MonoCache::default();
        let vector = vector_cache
            .instances("mul", vector_sig)
            .expect("单态化在界内");
        assert_eq!(vector.len(), 2, "Vector ⇒ Vec2 / Vec3");
        assert_eq!(vector[0].ret.type_name(), Some("zela.Vec2"));
    }

    /// **阶段 4 门禁**：目录里的每个算子都必须能从脚本侧调到（作者面已登记）。
    ///
    /// 加算子少了 `MathApi` 字段 / 注册 / `call` 分支时，这里会点名红 ——
    /// 而不是等到某个脚本第一次调用才发现"库里登记了但脚本够不着"。
    #[test]
    fn every_operator_is_reachable_from_scripts() {
        use std::collections::BTreeSet;
        use ze_pod_core::Capability as _;

        let reachable: BTreeSet<&str> = crate::MathApi::docs()
            .iter()
            .filter(|doc| doc.namespace == ze_func::LIBRARY_NAMESPACE)
            .map(|doc| doc.name)
            .collect();
        for def in super::SIGNATURES {
            assert!(
                reachable.contains(def.name),
                "算子 `{}` 没有作者面入口（`MathApi` 字段 / 注册 / call 分支漏了）",
                def.name
            );
        }
    }

    /// 既有算子的 revision 与批次对应（1 = 追踪开始前、9 = 第 1 批、11 = 第 3 批、12 = 第 5a 批）。
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
    }
}
