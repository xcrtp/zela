//! **UI side table**：`type_name → NodePresentation`。
//!
//! 计划 §4「第一刀」：展示数据不写回 IR / 不挂在 core trait 上；编辑器按
//! `type_name` 查自己的表。查不到 ⇒ 安全缺省（显示名 = 类型名、交互全开）。

use std::collections::BTreeMap;

use ze_node_core::{Node, NodeInteraction, NodePresentation};

/// 编辑器持有的展示信息表。
#[derive(Debug, Default, Clone)]
pub struct PresentationTable {
    entries: BTreeMap<String, NodePresentation>,
}

impl PresentationTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记 / 覆盖一个节点类型的展示信息。
    pub fn insert(&mut self, type_name: impl Into<String>, presentation: NodePresentation) {
        self.entries.insert(type_name.into(), presentation);
    }

    pub fn get(&self, type_name: &str) -> Option<&NodePresentation> {
        self.entries.get(type_name)
    }

    /// 按节点查展示信息（缺省回退到类型名 + 默认交互）。
    pub fn of(&self, type_name: &str) -> NodePresentation {
        self.entries
            .get(type_name)
            .cloned()
            .unwrap_or_else(|| NodePresentation::for_type(type_name))
    }

    /// 便捷入口：直接按 `&dyn Node` 查。
    pub fn of_node(&self, node: &dyn Node) -> NodePresentation {
        self.of(&node.meta().type_name)
    }

    pub fn interaction(&self, type_name: &str) -> NodeInteraction {
        self.of(type_name).interaction
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 清空（测试 / 切换文档）。
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
