//! **展示数据（UI side table 的载体）**。
//!
//! 计划 §4「第一刀」：`NodeMeta` 曾经把 `display_name` / `interaction` 焊进
//! core trait 的返回值里（`Node::meta()`），为了这两个 UI 字段连向量数学都绕开。
//! 现在它们不在这里，而在 [`NodePresentation`]：**核心 trait 只看 `NodeMeta`**。
//!
//! 数据留在 `ze-node-core` 是为了让 schema 宏与 UI 两端共用同一个类型；
//! **表**（按 `type_name` 查）由编辑器持有（`zela-node-editor::PresentationTable`）。

use serde::{Deserialize, Serialize};

/// Per-node-type editor interaction policy.
///
/// Sizes are in graph-local units (1 unit = 1 pixel at zoom 1.0); stored as
/// `[width, height]` pairs so this crate stays free of vector-math dependencies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeInteraction {
    /// The card can be dragged by grabbing its title band.
    pub draggable: bool,
    /// Clicking the card adds it to the selection.
    pub selectable: bool,
    /// The card exposes a resize grip and accepts resize gestures.
    pub resizable: bool,
    /// Minimum card size `[width, height]`.
    pub min_size: [f32; 2],
    /// Maximum card size `[width, height]`.
    pub max_size: [f32; 2],
}

impl Default for NodeInteraction {
    fn default() -> Self {
        Self {
            draggable: true,
            selectable: true,
            resizable: true,
            min_size: [120.0, 64.0],
            max_size: [1200.0, 900.0],
        }
    }
}

impl NodeInteraction {
    /// A fully locked node: no drag, no selection, no resize (fixed card).
    pub const fn locked() -> Self {
        Self {
            draggable: false,
            selectable: false,
            resizable: false,
            min_size: [120.0, 64.0],
            max_size: [1200.0, 900.0],
        }
    }

    pub fn draggable(mut self, draggable: bool) -> Self {
        self.draggable = draggable;
        self
    }

    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn min_size(mut self, width: f32, height: f32) -> Self {
        self.min_size = [width, height];
        self
    }

    pub fn max_size(mut self, width: f32, height: f32) -> Self {
        self.max_size = [width, height];
        self
    }

    /// Clamp a proposed card size into `[min_size, max_size]` (componentwise).
    pub fn clamp_size(&self, width: f32, height: f32) -> [f32; 2] {
        [
            width.clamp(self.min_size[0], self.max_size[0]),
            height.clamp(self.min_size[1], self.max_size[1]),
        ]
    }
}

/// 一个节点类型的**展示信息**（UI side table 的值）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodePresentation {
    pub display_name: String,
    pub interaction: NodeInteraction,
}

impl NodePresentation {
    /// 安全缺省：显示名 = 类型名，交互策略全开。
    pub fn for_type(type_name: &str) -> Self {
        Self {
            display_name: type_name.to_string(),
            interaction: NodeInteraction::default(),
        }
    }

    pub fn new(display_name: impl Into<String>, interaction: NodeInteraction) -> Self {
        Self {
            display_name: display_name.into(),
            interaction,
        }
    }

    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = display_name.into();
        self
    }

    pub fn with_interaction(mut self, interaction: NodeInteraction) -> Self {
        self.interaction = interaction;
        self
    }
}
