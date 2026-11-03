// world_inherit.rs — 每帧派生的**样式继承结果**（Resource；不进持久化）。
//
// 与 [`crate::WorldTransforms`] 同批诞生：父链不只传变换，也按 `ParentInherit`
// 的权重传不透明度 / 填充色 / 描边色。生产在
// `zela-scene-ecs::transform_hierarchy::TransformHierarchySystem`（同一次递归），
// 消费在 `zela-render` 的录制路径。
//
// # 缺席 = 现状
//
// 只有"对象存在且有父链"时才有条目；条目里存的都是**已经算好的最终值**：
// - `opacity`：父链累积后的世界不透明度（`local × mix(1, parent, w)`）；
// - `fill` / `stroke`：仅当与本地颜色不同才登记（值是覆盖后的颜色）。
// 消费方查不到 ⇒ 回退本地组件 / 默认值 ⇒ 默认场景逐位不变。

use std::collections::HashMap;

use ze_notion::Color32;
use ze_scene_contract::ObjectId;

/// 每帧派生的**样式继承结果**（Resource；不进持久化）。
#[derive(Debug, Clone, Default)]
pub struct WorldInherit {
    opacity: HashMap<ObjectId, f32>,
    fill: HashMap<ObjectId, Color32>,
    stroke: HashMap<ObjectId, Color32>,
}

impl WorldInherit {
    /// 空表（无任何继承条目）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 由三份映射构造（生成侧每帧重建时用）。
    pub fn from_maps(
        opacity: impl IntoIterator<Item = (ObjectId, f32)>,
        fill: impl IntoIterator<Item = (ObjectId, Color32)>,
        stroke: impl IntoIterator<Item = (ObjectId, Color32)>,
    ) -> Self {
        Self {
            opacity: opacity.into_iter().collect(),
            fill: fill.into_iter().collect(),
            stroke: stroke.into_iter().collect(),
        }
    }

    /// 写入一个对象的世界不透明度（测试与增量构建用）。
    pub fn insert_opacity(&mut self, id: ObjectId, value: f32) {
        self.opacity.insert(id, value);
    }

    /// 写入一个对象的覆盖填充色。
    pub fn insert_fill(&mut self, id: ObjectId, color: Color32) {
        self.fill.insert(id, color);
    }

    /// 写入一个对象的覆盖描边色。
    pub fn insert_stroke(&mut self, id: ObjectId, color: Color32) {
        self.stroke.insert(id, color);
    }

    /// 父链累积后的世界不透明度（缺席 ⇒ `None`，消费方回退本地 `Opacity`）。
    pub fn opacity_of(&self, id: ObjectId) -> Option<f32> {
        self.opacity.get(&id).copied()
    }

    /// 继承后的填充色（缺席 ⇒ `None`，消费方用本地 `FillSolid`）。
    pub fn fill_of(&self, id: ObjectId) -> Option<Color32> {
        self.fill.get(&id).copied()
    }

    /// 继承后的描边色（缺席 ⇒ `None`，消费方用本地 `StrokeSolid`）。
    pub fn stroke_of(&self, id: ObjectId) -> Option<Color32> {
        self.stroke.get(&id).copied()
    }

    pub fn len(&self) -> usize {
        self.opacity.len()
    }

    pub fn is_empty(&self) -> bool {
        self.opacity.is_empty() && self.fill.is_empty() && self.stroke.is_empty()
    }
}
