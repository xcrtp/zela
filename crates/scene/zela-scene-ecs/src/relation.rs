//! **CK-c**：场景 `Parent` 树 → 关系契约的只读适配。
//!
//! 这个模块只做三件事，**不碰任何存储**：
//!
//! 1. 把「父子声明」（`BTreeMap<ObjectId, ObjectId>`）投影成 [`RelationSource`]；
//! 2. 调 `ze-graph-algo` 的 [`compile_tree`] 得到 Euler 区间；
//! 3. 给出 `revision`（声明哈希），供调用方判断是否需要重新编译。
//!
//! 编译结果是派生态（不进 payload、不持句柄）：`ObjectId` 只是声明期键，
//! `TreeIndex` 内部是纯 `u32` 区间。
//!
//! # 为什么不是第二套层级系统
//!
//! `Parent` 链、`Surface` 祖先、`View` 空间、`ZOrder` 绘制层级此前各写了一遍
//! 「向上找 / 向下推」。这里把它们共用的那部分（稳定序 + 环 + 区间）交给同一份
//! 契约；各自的语义（Surface / View / 深度）仍在原来的域里，不被吞并。

use std::collections::BTreeMap;

use ze_graph_algo::relation::{
    self, RelationPhase, RelationError, RelationSource, TreeIndex,
};
use ze_scene_contract::ObjectId;

/// 场景 `Parent` 声明的最小只读视图。
pub struct ParentTree<'a> {
    nodes: Vec<ObjectId>,
    parents: &'a BTreeMap<ObjectId, ObjectId>,
}

impl<'a> ParentTree<'a> {
    /// `extra_nodes` 补上「没有 `Parent` 但有 `Transform`」的对象（它们也是树节点）。
    ///
    /// 父 id 即使不是活对象也会进入节点集合：`Parent(DANGLING)` 应在上游被过滤，
    /// 其余悬空父会被 [`RelationError::UnknownParent`] 显式拦下，而不是静默当根。
    pub fn new(
        parents: &'a BTreeMap<ObjectId, ObjectId>,
        extra_nodes: impl IntoIterator<Item = ObjectId>,
    ) -> Self {
        let mut nodes: Vec<ObjectId> = extra_nodes
            .into_iter()
            .chain(parents.keys().copied())
            .chain(parents.values().copied())
            .collect();
        nodes.sort_unstable();
        nodes.dedup();
        Self { nodes, parents }
    }

    pub fn nodes(&self) -> &[ObjectId] {
        &self.nodes
    }
}

impl RelationSource for ParentTree<'_> {
    type Id = ObjectId;

    fn nodes(&self) -> Vec<ObjectId> {
        self.nodes.clone()
    }

    fn parent(&self, id: ObjectId) -> Option<ObjectId> {
        self.parents.get(&id).copied()
    }

    fn phase(&self, _id: ObjectId) -> RelationPhase {
        // 父子关系在装配期声明，变换/样式在 Prepare 相位前推。
        RelationPhase::Assembly
    }
}

/// 编译场景 `Parent` 树（Euler 区间）。
pub fn compile_parent_tree(
    parents: &BTreeMap<ObjectId, ObjectId>,
    extra_nodes: impl IntoIterator<Item = ObjectId>,
) -> Result<TreeIndex<ObjectId>, RelationError<ObjectId>> {
    relation::compile_tree(&ParentTree::new(parents, extra_nodes))
}

/// 父子声明的 revision（声明哈希；插入顺序不参与）。
pub fn parent_revision(
    parents: &BTreeMap<ObjectId, ObjectId>,
    extra_nodes: impl IntoIterator<Item = ObjectId>,
) -> u64 {
    relation::revision(&ParentTree::new(parents, extra_nodes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(edges: &[(u32, Option<u32>)]) -> BTreeMap<ObjectId, ObjectId> {
        edges
            .iter()
            .filter_map(|(child, parent)| {
                parent.map(|parent| {
                    (
                        ObjectId::new(*child, 1),
                        ObjectId::new(parent, 1),
                    )
                })
            })
            .collect()
    }

    #[test]
    fn parent_tree_compiles_to_euler_intervals() {
        // 0 根；1 / 2 是 0 的子；3 是 1 的子。
        let parents = map(&[(1, Some(0)), (2, Some(0)), (3, Some(1))]);
        let tree = compile_parent_tree(&parents, [ObjectId::new(0, 1)]).expect("合法树");
        let root = ObjectId::new(0, 1);
        let child = ObjectId::new(3, 1);
        assert!(tree.is_ancestor(root, child));
        assert_eq!(
            tree.subtree(root).unwrap().collect::<Vec<_>>(),
            vec![
                ObjectId::new(0, 1),
                ObjectId::new(1, 1),
                ObjectId::new(3, 1),
                ObjectId::new(2, 1),
            ]
        );
    }

    #[test]
    fn a_parent_without_a_component_is_tolerated_as_a_root() {
        // 与既有 `world_of` 行为一致：父对象没有 `Transform` ⇒ 当恒等根，
        // 不静默丢子对象，也不把它变成错误（悬空是声明期合法占位）。
        let mut parents = BTreeMap::new();
        let child = ObjectId::new(1, 1);
        let ghost = ObjectId::new(99, 1);
        parents.insert(child, ghost);
        let tree = compile_parent_tree(&parents, []).expect("合法树");
        assert_eq!(tree.parent(child), Some(ghost));
        assert_eq!(tree.parent(ghost), None);
        assert!(tree.is_ancestor(ghost, child));
    }

    #[test]
    fn revision_ignores_map_insertion_order() {
        let a = map(&[(1, Some(0)), (2, Some(1))]);
        let b: BTreeMap<ObjectId, ObjectId> = map(&[(2, Some(1)), (1, Some(0))]);
        assert_eq!(
            parent_revision(&a, [ObjectId::new(0, 1)]),
            parent_revision(&b, [ObjectId::new(0, 1)])
        );
    }
}
