//! Kahn's algorithm topological sort.

use ze_node_core::algo::{CycleError, TopoSort};
use ze_node_core::graph::Graph;
use ze_node_core::id::NodeId;

use crate::GraphTopology;

/// Topological sort via Kahn's algorithm (BFS over in-degrees).
#[derive(Debug, Default)]
pub struct KahnSorter;

impl KahnSorter {
    pub fn new() -> Self {
        Self
    }
}

impl TopoSort for KahnSorter {
    fn sort(&self, graph: &dyn Graph) -> Result<Vec<NodeId>, CycleError> {
        // 算法在 `ze-graph-core`（纯拓扑、确定性）；本层只做适配与错误翻译。
        let topology = GraphTopology::new(graph);
        ze_graph_core::topo_sort(&topology)
            .map_err(|cycle| CycleError::Cycle { nodes: cycle.nodes })
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use super::*;
    use ze_node_core::node::{DirtyPolicy, Node, NodeMeta};
    use ze_node_core::simple_graph::SimpleGraph;
    use ze_node_core::socket::{InputSocket, OutputSocket};
    use ze_node_core::type_tag::NamedTypeTag;

    macro_rules! impl_as_any {
        () => {
            fn as_any(&self) -> &dyn Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        };
    }

    #[derive(Debug)]
    struct N {
        meta: NodeMeta,
        inputs: Vec<InputSocket>,
        outputs: Vec<OutputSocket>,
    }
    impl Node for N {
        impl_as_any!();
        fn meta(&self) -> &NodeMeta {
            &self.meta
        }
        fn inputs(&self) -> &[InputSocket] {
            &self.inputs
        }
        fn outputs(&self) -> &[OutputSocket] {
            &self.outputs
        }
        fn dirty_policy(&self) -> DirtyPolicy {
            DirtyPolicy::Always
        }
        fn state_type(&self) -> Box<dyn ze_node_core::TypeTag> {
            Box::new(NamedTypeTag::new("()"))
        }
    }
    fn mk(n_in: usize, n_out: usize) -> N {
        let tag: Box<dyn ze_node_core::TypeTag> = Box::new(NamedTypeTag::new("f32"));
        N {
            meta: NodeMeta::new("t", 1),
            inputs: (0..n_in)
                .map(|i| InputSocket::new(tag.clone()).with_index(i as u32))
                .collect(),
            outputs: (0..n_out)
                .map(|i| OutputSocket::new(tag.clone()).with_index(i as u32))
                .collect(),
        }
    }

    #[test]
    fn dag_sorts_upstream_first() {
        let mut g = SimpleGraph::default();
        let a = g.add_node(Box::new(mk(0, 1)));
        let b = g.add_node(Box::new(mk(1, 1)));
        let c = g.add_node(Box::new(mk(1, 0)));
        g.quick_connect(
            ze_node_core::OutputSocketId::new(a, 0),
            ze_node_core::InputSocketId::new(b, 0),
        )
        .unwrap();
        g.quick_connect(
            ze_node_core::OutputSocketId::new(b, 0),
            ze_node_core::InputSocketId::new(c, 0),
        )
        .unwrap();
        let order = KahnSorter::new().sort(&g).unwrap();
        let pos = |id: NodeId| order.iter().position(|&x| x == id).unwrap();
        assert!(pos(a) < pos(b));
        assert!(pos(b) < pos(c));
    }

    #[test]
    fn cycle_returns_error() {
        let mut g = SimpleGraph::default();
        let a = g.add_node(Box::new(mk(1, 1)));
        let b = g.add_node(Box::new(mk(1, 1)));
        g.quick_connect(
            ze_node_core::OutputSocketId::new(a, 0),
            ze_node_core::InputSocketId::new(b, 0),
        )
        .unwrap();
        g.quick_connect(
            ze_node_core::OutputSocketId::new(b, 0),
            ze_node_core::InputSocketId::new(a, 0),
        )
        .unwrap();
        let err = KahnSorter::new().sort(&g);
        assert!(matches!(err, Err(CycleError::Cycle { .. })));
    }

    /// 同一张图跑两次，拓扑序逐位相同（硬约束 1）。
    #[test]
    fn sort_is_deterministic() {
        let mut g = SimpleGraph::default();
        let a = g.add_node(Box::new(mk(0, 1)));
        let b = g.add_node(Box::new(mk(0, 1)));
        let c = g.add_node(Box::new(mk(2, 0)));
        g.quick_connect(
            ze_node_core::OutputSocketId::new(a, 0),
            ze_node_core::InputSocketId::new(c, 0),
        )
        .unwrap();
        g.quick_connect(
            ze_node_core::OutputSocketId::new(b, 0),
            ze_node_core::InputSocketId::new(c, 1),
        )
        .unwrap();
        let sorter = KahnSorter::new();
        let first = sorter.sort(&g).unwrap();
        assert_eq!(first, sorter.sort(&g).unwrap());
        assert_eq!(first.last().copied(), Some(c), "汇点最后");
        assert_eq!(first[0], first[..2].iter().copied().min().unwrap());
    }
}
