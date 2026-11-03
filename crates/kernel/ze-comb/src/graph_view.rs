//! **Expr DAG 图视图**：把表达式树投影成 [`ze_graph_core::Topology`]。
//!
//! # 为什么是图而不是树
//!
//! `Node::Def`（带 `id` 的节点）+ `Node::Ref` 是**共享**：同一份子表达式每帧只求值一次。
//! 图视图把 `Def` 算**一个节点**、`Ref` 算**一条边**（计划 W-C）——这样 SCC / 可达 /
//! 脏传播才有意义。
//!
//! # id 投影是遍历的副产品
//!
//! 遍历时按发现顺序分配整数下标，`Def` 的字符串 id 只用于解析 `Ref`。
//! 投影结果（`definitions` / `duplicate_ids` / `unresolved_refs`）同时是装配期诊断。
//!
//! # 无 HashMap
//!
//! 只用 `BTreeMap` / `BTreeSet`：遍历路径的顺序不随哈希种子漂移（硬约束 2）。

use std::collections::BTreeMap;

use ze_graph_core::Topology;

use crate::expr::{Expr, Id, Node};

/// 表达式 DAG 的只读图视图。
pub struct ExprGraphView<'a> {
    nodes: Vec<&'a Expr>,
    successors: Vec<Vec<usize>>,
    predecessors: Vec<Vec<usize>>,
    definitions: BTreeMap<&'a str, usize>,
    duplicate_ids: Vec<&'a str>,
    unresolved_refs: Vec<Id>,
}

impl<'a> ExprGraphView<'a> {
    /// 从表达式根构建视图（`Ref` 解析失败 / 重复 id 都记录为诊断，不 panic）。
    pub fn build(expr: &'a Expr) -> Self {
        let mut view = Self {
            nodes: Vec::new(),
            successors: Vec::new(),
            predecessors: Vec::new(),
            definitions: BTreeMap::new(),
            duplicate_ids: Vec::new(),
            unresolved_refs: Vec::new(),
        };
        let mut index_of: BTreeMap<*const Expr, usize> = BTreeMap::new();
        intern(expr, &mut view, &mut index_of);
        // 根本身是 `Ref` 时也要报悬空（它没有父节点替它解析）。
        if let Node::Ref { to } = &expr.node
            && !view.definitions.contains_key(to.as_str())
        {
            view.unresolved_refs.push(to.clone());
        }

        let mut successors: Vec<Vec<usize>> = vec![Vec::new(); view.nodes.len()];
        for (index, node) in view.nodes.iter().enumerate() {
            for child in node.node.children() {
                let target = match &child.node {
                    Node::Ref { to } => view.definitions.get(to.as_str()).copied().or_else(|| {
                        view.unresolved_refs.push(to.clone());
                        None
                    }),
                    _ => index_of.get(&(child as *const Expr)).copied(),
                };
                if let Some(target) = target {
                    successors[index].push(target);
                }
            }
            successors[index].sort_unstable();
            successors[index].dedup();
        }

        let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); view.nodes.len()];
        for (from, targets) in successors.iter().enumerate() {
            for &to in targets {
                predecessors[to].push(from);
            }
        }
        for list in &mut predecessors {
            list.sort_unstable();
            list.dedup();
        }

        view.successors = successors;
        view.predecessors = predecessors;
        view
    }

    /// 根节点下标（恒为 0）。
    pub fn root(&self) -> usize {
        0
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 下标 → 原始表达式节点。
    pub fn expr(&self, index: usize) -> Option<&'a Expr> {
        self.nodes.get(index).copied()
    }

    /// `id → 下标`（只含带 `id` 的 `Def`；重复 id 保留第一次）。
    pub fn definitions(&self) -> &BTreeMap<&'a str, usize> {
        &self.definitions
    }

    /// 重复 id（装配期诊断）。
    pub fn duplicate_ids(&self) -> &[&'a str] {
        &self.duplicate_ids
    }

    /// 悬空 `Ref` 的目标（装配期诊断）。
    pub fn unresolved_refs(&self) -> &[Id] {
        &self.unresolved_refs
    }
}

impl Topology for ExprGraphView<'_> {
    type Id = usize;

    fn nodes(&self) -> Vec<usize> {
        (0..self.nodes.len()).collect()
    }

    fn successors(&self, node: usize) -> Vec<usize> {
        self.successors.get(node).cloned().unwrap_or_default()
    }

    fn predecessors(&self, node: usize) -> Vec<usize> {
        self.predecessors.get(node).cloned().unwrap_or_default()
    }
}

/// 前序把出现顺序编成整数下标（`Ref` 不占节点：它是边）。
fn intern<'a>(
    expr: &'a Expr,
    view: &mut ExprGraphView<'a>,
    index_of: &mut BTreeMap<*const Expr, usize>,
) -> usize {
    let pointer = expr as *const Expr;
    if let Some(&index) = index_of.get(&pointer) {
        return index;
    }
    let index = view.nodes.len();
    view.nodes.push(expr);
    index_of.insert(pointer, index);
    if let Some(id) = expr.id.as_deref()
        && view.definitions.insert(id, index).is_some()
    {
        view.duplicate_ids.push(id);
    }
    for child in expr.node.children() {
        if matches!(child.node, Node::Ref { .. }) {
            continue;
        }
        intern(child, view, index_of);
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_graph_core::{postorder, preorder};

    fn math(name: &str) -> crate::expr::OpRef {
        crate::expr::OpRef::new("zela.math", name)
    }

    /// 共享 `Def`：两个 `Ref` 指向同一节点，视图里只算一个节点、一条边。
    fn shared() -> Expr {
        let leaf = Expr::pure(1.0).with_id("a");
        Expr::call(math("add"), [leaf, Expr::reference("a"), Expr::reference("a")])
    }

    #[test]
    fn a_shared_def_is_one_node_and_refs_are_edges() {
        let expr = shared();
        let view = ExprGraphView::build(&expr);
        assert_eq!(view.len(), 2, "root + 共享 Def");
        assert_eq!(view.successors(0), vec![1], "两个 Ref 去重成一条边");
        assert_eq!(view.definitions().get("a"), Some(&1));
        assert_eq!(preorder(&view, &[view.root()]), vec![0, 1]);
    }

    #[test]
    fn postorder_puts_the_shared_def_first() {
        let expr = shared();
        let view = ExprGraphView::build(&expr);
        assert_eq!(postorder(&view, &[view.root()]), vec![1, 0]);
    }

    #[test]
    fn traversal_is_deterministic() {
        let expr = shared();
        let view = ExprGraphView::build(&expr);
        assert_eq!(preorder(&view, &[0]), preorder(&view, &[0]));
    }

    #[test]
    fn duplicate_ids_and_dangling_refs_are_reported() {
        let duplicate = Expr::call(
            math("add"),
            [
                Expr::pure(1.0).with_id("dup"),
                Expr::pure(2.0).with_id("dup"),
            ],
        );
        let view = ExprGraphView::build(&duplicate);
        assert_eq!(view.duplicate_ids(), &["dup"]);

        let dangling = Expr::reference("missing");
        let view = ExprGraphView::build(&dangling);
        assert_eq!(view.unresolved_refs().to_vec(), vec!["missing".to_string()]);
    }
}
