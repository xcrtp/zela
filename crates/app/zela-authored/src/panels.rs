//! **CE-e 作品**：两个 View + 两个参数面板（场景对象）。
//!
//! # 它证明什么
//!
//! 参数面板**不是** egui 覆盖层：轨道是 `Rect`、滑块是 `Circle`、标签是 typst 烘焙的
//! `Path`，全部是场景对象（可 seek、可参数化、进像素基线）。
//!
//! 两个面板各属一个 CC 作用域（`panel_a` / `panel_b`）：路由键带上作用域，
//! 因此"两个面板各自的同一个控件"是两个不同目标 —— 拖 A 不会影响 B。
//!
//! 面板的**摆放**由 `ze_scene_ecs::interact::build_panels_for_views` 完成（唯一实现）；
//! 本作品只声明 View（空间）与两个被驱动的圆。

use std::sync::Arc;

use zela_scene_ecs::animation::{
    Binding, BindingDriver, BindingTable, CircleRadius, ResetBehavior,
};
use zela_scene_ecs::{ClockForest, ClockNode, Scene, SeekKind};
use ze_notion::{Rect2D, Vec2};
use ze_scene_core::scene_script::SceneEncoder;
use ze_scene_core::{Camera2DHandle, Circle, Framing2D, View, ViewMapping};
use ze_time::{ClockProgram, Curve};

use zela_scene_ecs::contract::ObjectId;

/// 相机缩放（像素 / 世界单位）：两块面板并排都在视口内。
const CAMERA_SCALE: f32 = 34.0;
/// 面板 A 的左上角。
const PANEL_A_ORIGIN: Vec2 = Vec2::new(-9.0, -4.0);
/// 面板 B 的左上角。
const PANEL_B_ORIGIN: Vec2 = Vec2::new(2.0, -4.0);
/// 面板尺寸（世界单位）。
const PANEL_SIZE: Vec2 = Vec2::new(6.0, 3.2);
/// 被驱动的圆（各自在一个面板作用域内）。
const CIRCLE_A: Vec2 = Vec2::new(-11.0, 2.5);
const CIRCLE_B: Vec2 = Vec2::new(0.0, 2.5);

/// [`build`] 的产物：装配尾需要的三样 + View 的空间声明。
pub struct Panels {
    pub forest: ClockForest,
    pub bindings: BindingTable,
    pub camera: Camera2DHandle,
    /// View 对象 → CC 作用域路径（面板摆放的输入）。
    pub views: Vec<(ObjectId, String)>,
}

/// 在**已有场景**里建两块面板 + 两个被驱动的圆。
pub fn build(scene: &mut Scene) -> Panels {
    let mut enc = SceneEncoder::new();
    // 相机必须先建：View 的摆放依赖它的取景范围（读端由宿主决定，这里只声明）。
    let camera_h = enc.spawn(vec![Box::new(Framing2D::new(Vec2::ZERO, CAMERA_SCALE))]);

    // 两个圆各自在**自己的作用域**里（绑定继承目标对象的作用域）。
    let token_a = enc.begin_scope("panel_a", ClockProgram::new());
    let circle_a = enc.spawn(vec![Box::new(Circle::new(CIRCLE_A, 0.35))]);
    enc.end_scope(token_a);
    let token_b = enc.begin_scope("panel_b", ClockProgram::new());
    let circle_b = enc.spawn(vec![Box::new(Circle::new(CIRCLE_B, 0.35))]);
    enc.end_scope(token_b);

    // 两个 View：屏幕坐标、并排摆放；`Parent` 不挂（两块面板是同级）。
    let view_a = enc.spawn(vec![Box::new(View::screen(Rect2D::from_min_size(
        PANEL_A_ORIGIN,
        PANEL_SIZE,
    )))]);
    let view_b = enc.spawn(vec![Box::new(View::new(
        Rect2D::from_min_size(PANEL_B_ORIGIN, PANEL_SIZE),
        ViewMapping::Screen,
    ))]);

    let recipe = enc
        .finish()
        .commit(scene)
        .expect("参数面板作品 commit 成功（组件均已登记）");

    let camera = Camera2DHandle(recipe.id(camera_h).expect("相机句柄已解析"));
    let circle_a = recipe.id(circle_a).expect("circle_a 已解析");
    let circle_b = recipe.id(circle_b).expect("circle_b 已解析");
    let view_a = recipe.id(view_a).expect("view_a 已解析");
    let view_b = recipe.id(view_b).expect("view_b 已解析");

    let mut forest = recipe.forest;
    let clock = forest.insert(ClockNode::new(None)).expect("时钟根");
    let mut bindings = recipe.bindings;
    for (path, object) in [("panel_a", circle_a), ("panel_b", circle_b)] {
        let scope = bindings.declare_scope(path);
        bindings.add(Binding {
            clock,
            scope,
            priority: 0,
            driver: BindingDriver::Curve(Curve::Sin {
                base: 0.3,
                amp: 0.2,
                omega: 1.0,
            }),
            target: Arc::new(CircleRadius { id: object }),
            predicate: None,
            active: None,
            reset: ResetBehavior::Hold,
            initial: None,
            state_plans: Vec::new(),
            seek_kind: SeekKind::O1,
        });
    }

    let views = vec![
        (view_a, "panel_a".to_string()),
        (view_b, "panel_b".to_string()),
    ];

    Panels {
        forest,
        bindings,
        camera,
        views,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zela_scene_ecs::animation::params;
    use zela_scene_ecs::interact::build_panels_for_views;

    /// **CE-e**：作品产出两块 View；装配期为每个 View 生成一组滑块，
    /// 且两个面板吸附的参数地址互不相同（拖 A 不可能改到 B）。
    #[test]
    fn panels_are_scoped_and_do_not_share_addresses() {
        let mut scene = Scene::new();
        let p = build(&mut scene);
        assert_eq!(p.views.len(), 2, "两个 View");
        assert_eq!(p.views[0].1, "panel_a");
        assert_eq!(p.views[1].1, "panel_b");

        let registry = params::build_registry(&p.bindings);
        let set = build_panels_for_views(
            &mut scene,
            &registry,
            &p.views,
            p.bindings.scopes(),
            zela_scene_ecs::interact::default_panel_metrics(),
            |_, _| Vec::new(),
        );
        // 每条 `Sin` 曲线三个参数（base / amp / omega）⇒ 两块面板共 6 个滑块。
        assert_eq!(set.sliders().len(), 6, "3 + 3");

        let a = set
            .sliders()
            .iter()
            .find(|slider| slider.scope == "panel_a")
            .expect("panel_a 的滑块");
        let b = set
            .sliders()
            .iter()
            .find(|slider| slider.scope == "panel_b")
            .expect("panel_b 的滑块");
        assert_ne!(a.address, b.address, "两个面板的参数地址必须不同");
        assert_ne!(a.target, b.target, "命中目标也必须不同");

        // 命中：A 的轨道只命中 A。
        let hit = set
            .hit(Vec2::new(a.track.center().x, a.track.center().y))
            .expect("命中 A");
        assert_eq!(hit.scope, "panel_a");
    }
}
