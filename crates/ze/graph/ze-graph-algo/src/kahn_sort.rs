//! Kahn's algorithm topological sort.

use std::collections::{BTreeMap, BTreeSet};

use ze_node_core::algo::{CycleError, TopoSort};
use ze_node_core::graph::Graph;
use ze_node_core::id::{InputSocketId, NodeId};

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
        // Build node-level dependency edges (deduplicated).
        let mut succ: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
        let mut in_degree: BTreeMap<NodeId, usize> = BTreeMap::new();
        let all: BTreeSet<NodeId> = graph.all_nodes().collect();

        for n in graph.all_nodes() {
            in_degree.entry(n).or_insert(0);
            succ.entry(n).or_default();
            let node = match graph.get_node(n) {
                Some(nd) => nd,
                None => continue,
            };
            for i in 0..node.inputs().len() {
                let in_id = InputSocketId::new(n, i as u32);
                for out in graph.upstream(in_id) {
                    let from = out.node();
                    if from == n {
                        continue; // ignore self-loops for degree; cycle caught later
                    }
                    if succ.entry(from).or_default().insert(n) {
                        *in_degree.entry(n).or_insert(0) += 1;
                    }
                }
            }
        }

        // 就绪集合用 `BTreeSet`：`pop_first` = 最小 id ⇒ 输出确定。
        let mut ready: BTreeSet<NodeId> = in_degree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(&node, _)| node)
            .collect();

        let mut order = Vec::with_capacity(all.len());
        while let Some(n) = ready.pop_first() {
            order.push(n);
            if let Some(succs) = succ.get(&n) {
                for &s in succs {
                    let d = in_degree.get_mut(&s).unwrap();
                    *d -= 1;
                    if *d == 0 {
                        ready.insert(s);
                    }
                }
            }
        }

        if order.len() == all.len() {
            Ok(order)
        } else {
            let placed: BTreeSet<NodeId> = order.iter().copied().collect();
            let remaining: Vec<NodeId> = all.difference(&placed).copied().collect();
            Err(CycleError::Cycle { nodes: remaining })
        }
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
            meta: NodeMeta::new("t", "t", 1),
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
