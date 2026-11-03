//! Node placement strategies.
//!
//! A [`LayoutStrategy`] answers "where is node N?" in graph-local
//! coordinates. The editor layer only *stores and consumes* positions: the UI
//! layer measures node sizes and injects them via [`NodeSizeMap`], while
//! automatic strategies (stage 10) may rearrange nodes topologically.

use std::collections::BTreeMap;

use ze_node_core::{Graph, NodeId};

use crate::geometry::NodePos;

/// Measured node card sizes, keyed by node. Populated by the UI layer after
/// laying out each card's body; the editor layer treats it as read-only input.
///
/// `BTreeMap`（不是 `HashMap`）：节点栈的遍历 / 布局结果必须是**确定序**
/// （按 `NodeId` 升序），否则同一张图两次布局可能给出不同的落点。
pub type NodeSizeMap = BTreeMap<NodeId, NodePos>;

/// Where nodes live in graph space.
pub trait LayoutStrategy: Send + Sync + 'static {
    /// Current position of a node's top-left corner, if known.
    fn position_of(&self, node: NodeId) -> Option<NodePos>;
    /// Record/override a node position (user drag or spawn).
    fn set_position(&mut self, node: NodeId, pos: NodePos);
    /// Forget a node (called when the node is removed).
    fn remove(&mut self, node: NodeId);
    /// Drop all positions.
    fn clear(&mut self);
    /// Automatic layout: rearrange nodes by topology. Free-form layouts
    /// implement this as a no-op.
    fn recompute(&mut self, _graph: &dyn Graph, _sizes: &NodeSizeMap) {}
    /// Mark a node's layout dirty (for future incremental recompute).
    fn invalidate(&mut self, _node: NodeId) {}
    /// `false` for free-form layouts: automatic layout must never overwrite
    /// user-chosen positions.
    fn auto_layout_supported(&self) -> bool {
        true
    }
}

/// Free-form layout: positions are entirely user-driven; `recompute` is a
/// no-op. New nodes land on a diagonal staircase so they never spawn exactly
/// on top of each other.
#[derive(Debug, Clone)]
pub struct FreeLayout {
    /// 按 `NodeId` 升序（确定序）；数量决定下一个落点。
    positions: BTreeMap<NodeId, NodePos>,
    spawn_origin: NodePos,
    spawn_step: NodePos,
}

impl Default for FreeLayout {
    fn default() -> Self {
        Self {
            positions: BTreeMap::new(),
            spawn_origin: NodePos::new(64.0, 64.0),
            spawn_step: NodePos::new(40.0, 40.0),
        }
    }
}

impl FreeLayout {
    pub fn new() -> Self {
        Self::default()
    }

    /// Customize the diagonal spawn staircase.
    pub fn with_spawn(origin: NodePos, step: NodePos) -> Self {
        Self {
            spawn_origin: origin,
            spawn_step: step,
            ..Self::default()
        }
    }

    /// Default landing spot for the next spawned node:
    /// `spawn_origin + n * spawn_step`, where `n` is the number of placed
    /// nodes. Consecutive calls (without nodes being added in between) return
    /// the same spot; the editor calls this once per spawn.
    pub fn next_spawn_position(&self) -> NodePos {
        self.spawn_origin + self.spawn_step * self.positions.len() as f32
    }
}

impl LayoutStrategy for FreeLayout {
    fn position_of(&self, node: NodeId) -> Option<NodePos> {
        self.positions.get(&node).copied()
    }

    fn set_position(&mut self, node: NodeId, pos: NodePos) {
        self.positions.insert(node, pos);
    }

    fn remove(&mut self, node: NodeId) {
        self.positions.remove(&node);
    }

    fn clear(&mut self) {
        self.positions.clear();
    }

    fn auto_layout_supported(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_layout_set_get_remove_clear() {
        let mut l = FreeLayout::new();
        let a = NodeId(1);
        let b = NodeId(2);
        assert_eq!(l.position_of(a), None);
        l.set_position(a, NodePos::new(10.0, 20.0));
        l.set_position(b, NodePos::new(30.0, 40.0));
        assert_eq!(l.position_of(a), Some(NodePos::new(10.0, 20.0)));
        assert_eq!(l.position_of(b), Some(NodePos::new(30.0, 40.0)));
        l.remove(a);
        assert_eq!(l.position_of(a), None);
        assert_eq!(l.position_of(b), Some(NodePos::new(30.0, 40.0)));
        l.clear();
        assert_eq!(l.position_of(b), None);
    }

    #[test]
    fn free_layout_spawn_positions_staircase_and_never_overlap() {
        let mut l = FreeLayout::new();
        let p0 = l.next_spawn_position();
        l.set_position(NodeId(1), p0);
        let p1 = l.next_spawn_position();
        l.set_position(NodeId(2), p1);
        let p2 = l.next_spawn_position();
        assert_ne!(p0, p1);
        assert_ne!(p1, p2);
        // Strictly increasing along the diagonal.
        assert!(p1.x > p0.x && p1.y > p0.y);
        assert!(p2.x > p1.x && p2.y > p1.y);
    }

    #[test]
    fn free_layout_does_not_support_auto_layout() {
        assert!(!FreeLayout::new().auto_layout_supported());
    }

    /// `NodeSizeMap` 的遍历顺序按 `NodeId` 升序（与插入顺序无关）。
    #[test]
    fn node_size_map_iterates_in_node_id_order() {
        let mut sizes = NodeSizeMap::new();
        sizes.insert(NodeId(7), NodePos::new(1.0, 1.0));
        sizes.insert(NodeId(2), NodePos::new(2.0, 2.0));
        sizes.insert(NodeId(5), NodePos::new(3.0, 3.0));
        let order: Vec<u64> = sizes.keys().map(|id| id.0).collect();
        assert_eq!(order, vec![2, 5, 7]);
    }
}
