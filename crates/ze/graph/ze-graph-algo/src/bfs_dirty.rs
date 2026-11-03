//! Breadth-first dirty propagation.

use std::collections::BTreeSet;

use ze_node_core::algo::DirtyTracker;
use ze_node_core::graph::Graph;
use ze_node_core::id::{NodeId, OutputSocketId};

use crate::GraphTopology;

/// Dirty tracker that propagates marks along downstream edges via BFS.
#[derive(Debug, Default)]
pub struct BfsDirtyTracker {
    /// `BTreeSet`：输出顺序由 `NodeId` 决定，不随哈希种子漂移。
    dirty: BTreeSet<NodeId>,
}

impl BfsDirtyTracker {
    pub fn new() -> Self {
        Self::default()
    }
}

impl DirtyTracker for BfsDirtyTracker {
    fn mark_dirty(&mut self, node: NodeId) {
        self.dirty.insert(node);
    }

    fn mark_socket_dirty(&mut self, graph: &dyn Graph, socket: OutputSocketId) {
        // 直接下游（socket 级）→ 节点级脏传播（`ze-graph-core`，确定性升序）。
        let mut seeds: Vec<NodeId> = graph
            .downstream(socket)
            .into_iter()
            .map(|input| input.node())
            .collect();
        seeds.sort_unstable();
        seeds.dedup();
        for &node in &seeds {
            self.dirty.insert(node);
        }
        let topology = GraphTopology::new(graph);
        for node in ze_graph_core::dirty_downstream(&topology, &seeds) {
            self.dirty.insert(node);
        }
    }

    fn is_dirty(&self, node: NodeId) -> bool {
        self.dirty.contains(&node)
    }

    fn clear_dirty(&mut self, node: NodeId) {
        self.dirty.remove(&node);
    }

    fn collect_dirty(&self) -> Vec<NodeId> {
        // `BTreeSet` 的迭代顺序即升序 ⇒ 与哈希种子无关。
        self.dirty.iter().copied().collect()
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
    fn propagates_to_all_downstream() {
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

        let mut t = BfsDirtyTracker::new();
        t.mark_socket_dirty(&g, ze_node_core::OutputSocketId::new(a, 0));
        assert!(t.is_dirty(b));
        assert!(t.is_dirty(c));
        assert!(!t.is_dirty(a)); // source not marked, only downstream
    }

    /// 同一个 tracker 两次收集必须逐位相同，且升序（硬约束 1）。
    #[test]
    fn collect_dirty_is_sorted_and_repeatable() {
        let mut t = BfsDirtyTracker::new();
        for id in [3u64, 1, 2] {
            t.mark_dirty(NodeId(id));
        }
        let first = t.collect_dirty();
        assert_eq!(first, vec![NodeId(1), NodeId(2), NodeId(3)]);
        assert_eq!(first, t.collect_dirty());
    }
}
