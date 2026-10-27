//! 最小拓扑接口：**只认节点 id + 边**。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

/// 抽象拓扑。`Id` 只需可比较（确定性排序）。
pub trait Topology {
    type Id: Copy + Ord + Debug;

    /// 全部节点（顺序不参与语义；算法内部自行排序）。
    fn nodes(&self) -> Vec<Self::Id>;
    /// `node` 的出边目标（可含重复与自环；算法自行去重）。
    fn successors(&self, node: Self::Id) -> Vec<Self::Id>;
    /// `node` 的入边来源。
    fn predecessors(&self, node: Self::Id) -> Vec<Self::Id>;
}

/// 简单邻接表：测试 / 基准 / 轻量宿主用。
///
/// 内部用 `BTreeMap` / `BTreeSet`：遍历顺序由 id 决定，不随插入顺序漂移。
#[derive(Clone, Debug)]
pub struct Adjacency<Id> {
    nodes: BTreeSet<Id>,
    successors: BTreeMap<Id, BTreeSet<Id>>,
    predecessors: BTreeMap<Id, BTreeSet<Id>>,
}

impl<Id> Default for Adjacency<Id> {
    fn default() -> Self {
        Self {
            nodes: BTreeSet::new(),
            successors: BTreeMap::new(),
            predecessors: BTreeMap::new(),
        }
    }
}

impl<Id> Adjacency<Id>
where
    Id: Copy + Ord + Debug,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: Id) {
        self.nodes.insert(node);
        self.successors.entry(node).or_default();
        self.predecessors.entry(node).or_default();
    }

    /// 加边（两端节点不存在时自动创建）。
    pub fn add_edge(&mut self, from: Id, to: Id) {
        self.add_node(from);
        self.add_node(to);
        self.successors.entry(from).or_default().insert(to);
        self.predecessors.entry(to).or_default().insert(from);
    }

    pub fn from_edges(
        nodes: impl IntoIterator<Item = Id>,
        edges: impl IntoIterator<Item = (Id, Id)>,
    ) -> Self {
        let mut graph = Self::new();
        for node in nodes {
            graph.add_node(node);
        }
        for (from, to) in edges {
            graph.add_edge(from, to);
        }
        graph
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.successors.values().map(BTreeSet::len).sum()
    }
}

impl<Id> Topology for Adjacency<Id>
where
    Id: Copy + Ord + Debug,
{
    type Id = Id;

    fn nodes(&self) -> Vec<Id> {
        self.nodes.iter().copied().collect()
    }

    fn successors(&self, node: Id) -> Vec<Id> {
        self.successors
            .get(&node)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    fn predecessors(&self, node: Id) -> Vec<Id> {
        self.predecessors
            .get(&node)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }
}
