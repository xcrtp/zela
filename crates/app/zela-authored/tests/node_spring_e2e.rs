//! **CF-f 端到端判据**：节点编辑器里放一个弹簧 → lowering → 动画 → 面板可拖 → 可回放。
//!
//! ```text
//! Editor::AddNode(SpringNode)
//!   → ze_node_lower::lower  → Expr::state（Vec2）
//!   → node_state::verify_states → StatePlan（谱半径门禁）
//!   → SceneEncoder::bind_expr("Transform.position")
//!   → Scheduler + BindingApplySystem 逐帧 → Transform.position
//! ```
//!
//! 三段各证一件事（同一个弹簧节点、同一张 lowering 规则表）：
//!
//! 1. **动画**：场景里的 `Transform.position` 逐帧等于闭式定义 `value_at(frame)`；
//! 2. **回放**：同一帧从零重跑一次，结果逐位相同（`State` 是帧号的纯函数）；
//! 3. **面板可拖**：CE-e 的参数面板作品照常工作（拖动写入曲线参数）。
//!
//! 为什么面板单独一段：CE-e 的面板绑定是**曲线参数**（`BindingDriver::Curve`），
//! 弹簧是**表达式状态**绑定 —— 两者各有一条时钟森林，本用例不把它们强行并进同一个
//! 调度器（那是装配层的事，不是本判据）。

use ze_comb::Expr;
use ze_node_core::NodeId;
use ze_notion::{Quat, Vec2, Vec3};
use ze_scene_core::animation::LibraryRef;
use ze_scene_core::object::{Circle, Transform};
use ze_scene_core::scene_script::{SceneEncoder, SceneScript};
use ze_time::ClockProgram;
use zela_authored::node_state::verify_states;
use zela_authored::panels;
use zela_authored::spring::{self, SPRING_TYPE, SpringNode};
use zela_node_editor::{BezierGeometry, Command, Editor, EditorEvent, FreeLayout, NodePos};
use zela_scene_ecs::animation::{BindingApplySystem, BindingDriver, params};
use zela_scene_ecs::domain::SceneEvent;
use zela_scene_ecs::interact::{
    InteractState, apply_viewport_events, build_panels_for_views, default_panel_metrics,
};
use zela_scene_ecs::{Curve, Scene, TimeState};
use zela_schedule::resource::Resources;
use zela_schedule::scheduler::{Phase, Scheduler};

fn library() -> LibraryRef {
    LibraryRef {
        namespace: "zela.math".to_string(),
        version: "4".to_string(),
        op_revisions: Default::default(),
    }
}

/// 在节点编辑器里放一个弹簧节点，返回（编辑器，节点 id）。
fn editor_with_spring(spring: SpringNode) -> (Editor, NodeId) {
    let mut editor = Editor::new(
        Box::new(ze_node_core::SimpleGraph::default()),
        Box::new(FreeLayout::new()),
        Box::new(BezierGeometry::default()),
    );
    editor
        .apply(Command::AddNode {
            node: Box::new(spring),
            at: NodePos::new(40.0, 40.0),
        })
        .expect("添加弹簧节点");
    let id = editor
        .drain_events()
        .find_map(|event| match event {
            EditorEvent::NodeAdded(id) => Some(id),
            _ => None,
        })
        .expect("节点已加入");
    (editor, id)
}

/// 编辑器里的弹簧 → lowering → 该节点的输出表达式。
fn spring_expression(editor: &Editor, node: NodeId) -> Expr {
    let lowered = spring::lowering()
        .lower(editor.graph())
        .expect("图可 lowering");
    let states = verify_states(editor.graph(), &lowered).expect("状态可校验");
    assert_eq!(states.len(), 1, "弹簧是唯一的 State");
    assert_eq!(states[0].node, node);
    lowered.output(node, 0).expect("弹簧有输出 0").clone()
}

/// 建一个只有弹簧驱动对象的最小脚本。
fn spring_script(expr: Expr) -> SceneScript {
    let mut enc = SceneEncoder::new();
    let object = enc.spawn(vec![
        Box::new(Transform {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }),
        Box::new(Circle::new(Vec2::ZERO, 0.4)),
    ]);
    enc.bind_expr(
        object,
        "Transform.position",
        expr,
        library(),
        ClockProgram::new(),
        0.0,
    )
    .expect("绑定表达式");
    enc.finish()
}

/// 推 `frames` 帧，返回最后一帧对象的 `Transform.position.xy`。
fn position_after(script: SceneScript, frames: u32) -> (f32, f32) {
    let mut scene = Scene::new();
    let recipe = script.commit(&mut scene).expect("commit 成功");
    let mut resources = Resources::new();
    resources.insert(TimeState::at(0.0));
    resources.insert(recipe.forest);
    resources.insert(recipe.bindings);

    let mut sched = Scheduler::new(scene.into_world(), resources);
    sched.add_system(Phase::SceneSync, BindingApplySystem);
    sched.init_resources();
    for frame in 1..=frames {
        let time = sched
            .resources_mut()
            .get_mut::<TimeState>()
            .expect("TimeState");
        time.elapsed = frame as f32 / 60.0;
        // `State` 的轮数来自帧号（不是秒）：两处一起推进。
        time.frame = u64::from(frame);
        sched.run_frame(1.0 / 60.0);
    }
    let transform = sched
        .world()
        .query::<&Transform>()
        .iter()
        .next()
        .map(|transform| (transform.position.x, transform.position.y))
        .expect("场景里有一个 Transform");
    transform
}

/// **端到端**：编辑器里的弹簧 → 动画（逐帧对拍闭式定义）→ 回放（同帧逐位相同）。
#[test]
fn spring_node_drives_a_scene_and_replays_identically() {
    let (editor, node) = editor_with_spring(SpringNode::default());
    let expr = spring_expression(&editor, node);

    // 闭式定义：第 n 帧 = f^n(init)。
    let lowered = spring::lowering().lower(editor.graph()).unwrap();
    let states = verify_states(editor.graph(), &lowered).unwrap();
    let plan = &states[0].plan;

    for frames in [1u32, 2, 5, 30] {
        let (x, y) = position_after(spring_script(expr.clone()), frames);
        let closed = plan.value_at(frames as i64);
        let closed = closed.as_vec2().expect("闭式值是 Vec2");
        let scale = 1.0 + x.abs().max(closed[0].abs());
        assert!(
            (x - closed[0]).abs() <= 1e-5 * scale && (y - closed[1]).abs() <= 1e-5 * scale,
            "第 {frames} 帧：场景 ({x}, {y}) ≠ 闭式 {closed:?}"
        );
    }

    // 回放：同一帧从零重跑，结果逐位相同。
    let (x1, y1) = position_after(spring_script(expr.clone()), 30);
    let (x2, y2) = position_after(spring_script(expr), 30);
    assert_eq!((x1, y1), (x2, y2), "同一帧的回放必须逐位相同");
}

/// 弹簧节点类型与规则表登记一致（编辑器里放的就是 lowering 认的）。
#[test]
fn spring_node_type_is_registered_in_the_lowering_table() {
    let (editor, node) = editor_with_spring(SpringNode::default());
    let type_name = editor
        .graph()
        .get_node(node)
        .expect("节点存在")
        .meta()
        .type_name
        .clone();
    assert_eq!(type_name, SPRING_TYPE);
    assert!(spring::lowering().is_registered(&type_name));
}

/// **面板可拖**：CE-e 的参数面板作品在装配后仍可被拖动（写入曲线参数）。
///
/// 这里直接调 `apply_viewport_events`（`ParamPanelSystem` 内部调用的同一个纯函数）：
/// 资源接线那条路径由 CE-e 的 `interact_system_writes_only_the_captured_panel` 覆盖，
/// 本判据只关心"装配出来的面板还能被拖"。
#[test]
fn panel_stays_draggable_after_assembly() {
    let mut scene = Scene::new();
    let built = panels::build(&mut scene);
    let mut registry = params::build_registry(&built.bindings);
    let set = build_panels_for_views(
        &mut scene,
        &registry,
        &built.views,
        built.bindings.scopes(),
        default_panel_metrics(),
        |_, _, _| Vec::new(),
    );
    let slider = set.sliders().first().expect("面板至少有一个滑块").clone();
    let track = slider.track;
    // 按到轨道 75% 处。
    let press = Vec2::new(
        track.min.x + (track.max.x - track.min.x) * 0.75,
        track.center().y,
    );
    assert!(
        set.hit(press).is_some(),
        "按点必须命中滑块（track = {track:?}，press = {press:?}）"
    );

    let mut state = InteractState::default();
    let mut table = built.bindings.clone();
    let knob_moves = apply_viewport_events(
        &[
            SceneEvent::ViewportClick {
                layer: ze_render_ir::LayerId(0),
                scene_pos: press,
            },
            SceneEvent::ViewportRelease {
                layer: ze_render_ir::LayerId(0),
                scene_pos: press,
            },
        ],
        &set,
        &mut state,
        &mut table,
        &mut registry,
    );
    assert_eq!(knob_moves.len(), 1, "按下必须写一次");

    // 被拖的那一块面板有一个字段写成 75%；另一块三个字段都不动。
    let sin_fields = |table: &zela_scene_ecs::animation::BindingTable| -> Vec<[f32; 3]> {
        table
            .bindings
            .iter()
            .filter_map(|binding| match &binding.driver {
                BindingDriver::Curve(Curve::Sin { base, amp, omega }) => {
                    Some([*base, *amp, *omega])
                }
                _ => None,
            })
            .collect()
    };
    let before = sin_fields(&built.bindings);
    let after = sin_fields(&table);
    assert_eq!(before.len(), 2, "两块面板各一条 Sin 曲线");
    let changed: Vec<usize> = (0..before.len())
        .filter(|&index| before[index] != after[index])
        .collect();
    assert_eq!(
        changed.len(),
        1,
        "只有被拖的那一块改动：{before:?} → {after:?}"
    );
    let index = changed[0];
    // 期望值由滑块自己的映射给出（轨道 75% 处的参数值）。
    let expected = slider.value_at(press.x);
    assert!(
        after[index]
            .iter()
            .any(|field| (*field - expected).abs() < 1e-5),
        "拖动必须写回曲线参数 {expected}：{before:?} → {after:?}"
    );
    let other = 1 - index;
    assert_eq!(before[other], after[other], "未被拖的另一块面板不得改动");
}
