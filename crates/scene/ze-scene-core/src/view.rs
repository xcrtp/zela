//! View（CE-d）：给 UI 面一个**空间**与**映射**的声明。
//!
//! # 为什么是场景对象
//!
//! 参数面板必须是场景对象（线 / 圆 / typst 烘焙的文字轮廓），才能 seek、进像素基线、
//! 参与动画。若 UI 画完贴成纹理，它就"看起来做完了"却不是场景的一部分。
//!
//! # 为什么不动 `Surface`
//!
//! `Surface` 有 5 个生产消费者（app / demos / demo3d / playback / vector_frame），
//! 是活的生产路径。改造风险大 ⇒ **组合，不提升**：View 只声明自己的空间，
//! 需要内容纹理时由调用方另外挂 Surface。
//!
//! # 作用域不在这里定义
//!
//! View 的命名空间**复用 CC 的作用域**（`SceneScript.object_scopes` 的路径），
//! 本组件只描述空间（`bounds`）与坐标语义（[`ViewMapping`]）。两件事分开，
//! 才不会出现"作用域有两个来源"。

use serde::{Deserialize, Serialize};
use ze_notion::Rect2D;

/// `View.bounds` 的坐标语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ViewMapping {
    /// 屏幕空间（像素 / 点）。
    #[default]
    Screen,
    /// 世界空间（与 3D 同一坐标系）。
    World,
}

/// UI 面的空间声明（可嵌套：沿 `Parent` 链）。
///
/// 它只描述"这一面占多大、坐标怎么解释"；不含位置（位置由 `Transform` 唯一提供）。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct View {
    /// 这一面的矩形（逻辑坐标）。
    pub bounds: Rect2D,
    /// 坐标语义。
    pub mapping: ViewMapping,
}

impl View {
    pub fn new(bounds: Rect2D, mapping: ViewMapping) -> Self {
        Self { bounds, mapping }
    }

    /// 屏幕空间的面（最常见：参数面板 / HUD）。
    pub fn screen(bounds: Rect2D) -> Self {
        Self::new(bounds, ViewMapping::Screen)
    }

    /// 世界空间的面（与场景内容同坐标系）。
    pub fn world(bounds: Rect2D) -> Self {
        Self::new(bounds, ViewMapping::World)
    }

    /// 本地坐标是否落在这面内（半开区间 `[min, max)`，与命中口径同一条）。
    pub fn contains(&self, point: ze_notion::Vec2) -> bool {
        self.bounds.contains(point)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_notion::Vec2;

    fn bounds() -> Rect2D {
        Rect2D::from_min_size(Vec2::new(10.0, 20.0), Vec2::new(100.0, 60.0))
    }

    #[test]
    fn view_keeps_bounds_and_mapping() {
        let view = View::screen(bounds());
        assert_eq!(view.mapping, ViewMapping::Screen);
        assert_eq!(view.bounds.max, Vec2::new(110.0, 80.0));

        let world = View::world(bounds());
        assert_eq!(world.mapping, ViewMapping::World);
        assert_ne!(world.mapping, view.mapping);
    }

    #[test]
    fn contains_uses_the_same_half_open_bounds_as_hit_testing() {
        let view = View::screen(bounds());
        assert!(view.contains(Vec2::new(10.0, 20.0)), "min 端点属于本面");
        assert!(
            !view.contains(Vec2::new(110.0, 80.0)),
            "max 端点属于下一个面"
        );
        assert!(view.contains(Vec2::new(109.999, 79.999)));
        assert!(!view.contains(Vec2::new(9.0, 50.0)));
    }

    /// **CE-d**：View 的命名空间复用 CC 的作用域声明（不新建第二套）。
    #[test]
    fn view_scope_comes_from_the_cc_scope_declaration() {
        use crate::scene_script::SceneEncoder;
        use ze_time::ClockProgram;

        let mut enc = SceneEncoder::new();
        let token = enc.begin_scope("panel_a", ClockProgram::new());
        let inner = enc.spawn(vec![Box::new(View::screen(bounds()))]);
        enc.end_scope(token);
        let outer = enc.spawn(vec![Box::new(View::screen(bounds()))]);
        let script = enc.finish();

        assert_eq!(
            script.object_scope(inner).map(|scope| scope.path.as_str()),
            Some("panel_a")
        );
        assert!(script.object_scope(outer).is_none(), "根作用域不写 scope");
    }

    /// View 必须能进产物（组件登记表 + serde）。
    #[test]
    fn view_survives_tagged_json_roundtrip() {
        let components: ze_scene_contract::Components = vec![Box::new(View::world(bounds()))];
        let tagged = crate::persist::serialize_components(&components).expect("序列化 View");
        let json = serde_json::to_string(&tagged).expect("tagged JSON");
        assert!(json.contains("View"), "类型名进 JSON：{json}");
        let restored = crate::persist::deserialize_components(tagged).expect("反序列化 View");
        let view = restored
            .iter()
            .find_map(|component| (**component).as_any().downcast_ref::<View>())
            .expect("View 必须被还原");
        assert_eq!(view.mapping, ViewMapping::World);
        assert_eq!(view.bounds, bounds());
    }
}
