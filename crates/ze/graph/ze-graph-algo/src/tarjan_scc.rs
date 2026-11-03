//! Cycle detection via SCC (delegates to `ze-graph-core`).
//!
//! **确定性**由纯拓扑层保证：同一张图两次输出逐位相同（硬约束 1）。

use ze_node_core::algo::CycleDetector;
use ze_node_core::graph::Graph;
use ze_node_core::id::NodeId;

use crate::GraphTopology;

/// Cycle detector based on SCC (the implementation lives in `ze-graph-core`).
#[derive(Debug, Default)]
pub struct TarjanSccDetector;

impl TarjanSccDetector {
    pub fn new() -> Self {
        Self
    }
}

impl CycleDetector for TarjanSccDetector {
    fn detect(&self, graph: &dyn Graph) -> Option<Vec<NodeId>> {
        ze_graph_core::find_cycle(&GraphTopology::new(graph))
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
    fn detects_two_node_cycle() {
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
        let cycle = TarjanSccDetector::new().detect(&g).expect("有环");
        let mut sorted = cycle.clone();
        sorted.sort_unstable();
        assert_eq!(cycle, sorted, "分量内必须升序");
    }

    #[test]
    fn no_cycle_in_dag() {
        let mut g = SimpleGraph::default();
        let a = g.add_node(Box::new(mk(0, 1)));
        let b = g.add_node(Box::new(mk(1, 0)));
        g.quick_connect(
            ze_node_core::OutputSocketId::new(a, 0),
            ze_node_core::InputSocketId::new(b, 0),
        )
        .unwrap();
        assert!(TarjanSccDetector::new().detect(&g).is_none());
    }

    /// 同一张图跑两次，SCC 输出逐位相同（硬约束 1）。
    #[test]
    fn detection_is_deterministic() {
        let mut g = SimpleGraph::default();
        let a = g.add_node(Box::new(mk(1, 1)));
        let b = g.add_node(Box::new(mk(1, 1)));
        let c = g.add_node(Box::new(mk(1, 1)));
        let d = g.add_node(Box::new(mk(1, 1)));
        for (from, to) in [(a, b), (b, a), (c, d), (d, c)] {
            g.quick_connect(
                ze_node_core::OutputSocketId::new(from, 0),
                ze_node_core::InputSocketId::new(to, 0),
            )
            .unwrap();
        }
        let detector = TarjanSccDetector::new();
        let first = detector.detect(&g).expect("有环");
        let second = detector.detect(&g).expect("有环");
        assert_eq!(first, second);
        assert_eq!(first[0], first.iter().copied().min().unwrap(), "取最小分量");
    }
}
