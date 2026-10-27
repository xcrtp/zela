//! `Graph` → `ze-graph-core::Topology` 适配器。
//!
//! 算法层从这里只看到 **节点 id + 边**（计划 §3.1 / 硬约束 2）。socket 计数走
//! `Graph::input_count` / `output_count`（阶段 2 新增；注册表实现可覆盖）。

use std::collections::BTreeSet;

use ze_graph_core::Topology;
use ze_node_core::graph::Graph;
use ze_node_core::id::{InputSocketId, NodeId, OutputSocketId};

/// 把任意 [`Graph`] 投影成纯拓扑。
pub struct GraphTopology<'a> {
    graph: &'a dyn Graph,
}

impl<'a> GraphTopology<'a> {
    pub fn new(graph: &'a dyn Graph) -> Self {
        Self { graph }
    }
}

impl Topology for GraphTopology<'_> {
    type Id = NodeId;

    fn nodes(&self) -> Vec<NodeId> {
        let mut nodes: Vec<NodeId> = self.graph.all_nodes().collect();
        nodes.sort_unstable();
        nodes.dedup();
        nodes
    }

    fn successors(&self, node: NodeId) -> Vec<NodeId> {
        let mut out: BTreeSet<NodeId> = BTreeSet::new();
        for index in 0..self.graph.output_count(node).unwrap_or(0) {
            for input in self.graph.downstream(OutputSocketId::new(node, index as u32)) {
                out.insert(input.node());
            }
        }
        out.into_iter().collect()
    }

    fn predecessors(&self, node: NodeId) -> Vec<NodeId> {
        let mut out: BTreeSet<NodeId> = BTreeSet::new();
        for index in 0..self.graph.input_count(node).unwrap_or(0) {
            for output in self.graph.upstream(InputSocketId::new(node, index as u32)) {
                out.insert(output.node());
            }
        }
        out.into_iter().collect()
    }
}
