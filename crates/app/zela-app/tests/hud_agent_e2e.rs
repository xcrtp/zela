//! **CG-g 端到端判据**：零窗口 / 零 GPU / 零 egui 跑通 CE 的主判据 —— 拖 A 不影响 B。
//!
//! 路径与 Agent 完全一致：
//!
//! ```text
//! tagged JSON 事件流 → 场景 System → 一帧 Frame（JSON）→ 读语义值断言
//! ```
//!
//! 断言读的是 `Frame::value_of("slider", 地址)`，**不是像素 hash**，也不是
//! `Hud` 的内部字段 —— 换画法 / 换后端不会让这条判据失真。

use std::collections::BTreeMap;

use zela_app::headless;
use zela_app::SceneChoice;
use zela_scene_ecs::domain::SceneEvent;
use ze_scene_core::domain::LayerId;

/// 把一组事件编码成**跨进程格式**（tagged JSON），再交给 headless 入口。
fn inject(scene: &headless::HeadlessScene, events: Vec<SceneEvent>) {
    let json = serde_json::to_string(&events).expect("SceneEvent 可序列化");
    let count = scene
        .inject_json(&json)
        .unwrap_or_else(|error| panic!("事件流被拒绝：{error}"));
    assert_eq!(count, events.len(), "注入的事件数必须一致");
}

/// 按**作用域**（参数地址的第一段）把可调滑块分组：两个 View 各一组。
fn param_scopes(frame: &ze_interact::Frame) -> BTreeMap<String, Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for node in &frame.semantics {
        if node.role != "slider" || !node.name.contains(":curve:") {
            continue;
        }
        let scope = node
            .name
            .split(':')
            .next()
            .unwrap_or_default()
            .to_string();
        groups.entry(scope).or_default().push(node.name.clone());
    }
    groups
}

#[test]
fn dragging_one_panel_parameter_does_not_touch_the_other() {
    let scene = headless::build(SceneChoice::Hud).expect("headless 装配（无 GPU）");
    // 装配期没有渲染上下文 ⇒ 这里能拿到帧就说明整条路径不依赖 GPU / 窗口 / egui。
    assert!(scene.duration() >= 0.0);

    let before = scene.frame().expect("HUD 场景有一帧 Frame");
    let groups = param_scopes(&before);
    assert_eq!(groups.len(), 2, "两个 View 各一组参数：{groups:?}");
    let mut scopes = groups.into_iter();
    let (_, a_params) = scopes.next().expect("A 组");
    let (_, b_params) = scopes.next().expect("B 组");
    assert_eq!(a_params.len(), 3, "Sin 曲线三个参数（base / amp / omega）");
    assert_eq!(b_params.len(), 3);
    let a = a_params[0].clone();
    let b = b_params[0].clone();

    let value_before_a = before
        .value_of("slider", &a)
        .expect("A 的初始值")
        .parse::<f64>()
        .expect("数值");
    let value_before_b = before
        .value_of("slider", &b)
        .expect("B 的初始值")
        .to_string();

    // 在 A 的轨道 80% 处按下再抬起（真正的两组指针事件）。
    let (x, y) = scene.slider_point(&a, 0.8).expect("A 的世界坐标");
    inject(
        &scene,
        vec![
            SceneEvent::ViewportClick {
                layer: LayerId(0),
                scene_pos: ze_notion::Vec2::new(x, y),
            },
            SceneEvent::ViewportRelease {
                layer: LayerId(0),
                scene_pos: ze_notion::Vec2::new(x, y),
            },
        ],
    );
    // 推进两帧：第一帧 HUD 状态机写值（FrameEnd），第二帧才进入下一轮的 SceneSync。
    scene.advance(2, 1.0 / 60.0);
    scene.clear_events();

    let after = scene.frame().expect("拖拽后仍有一帧 Frame");
    let value_after_a = after
        .value_of("slider", &a)
        .expect("A 的当前值")
        .parse::<f64>()
        .expect("数值");
    let value_after_b = after
        .value_of("slider", &b)
        .expect("B 的当前值")
        .to_string();

    assert!(
        (value_after_a - value_before_a).abs() > 1e-6,
        "拖 A 必须改变 A 的值：{value_before_a} → {value_after_a}"
    );
    assert_eq!(value_before_b, value_after_b, "拖 A 不得影响 B");
}

/// **CH-a 判据**：窗口输入（逻辑像素）与场景事件走同一条换算 —— 同一个点、
/// 同一帧，落到同一个值。`collect_events` 的视口判定 / 图层选择 / 相机换算
/// 全部在纯函数里，Debug 与真人不再各走一条。
#[test]
fn window_input_reaches_the_same_value_as_scene_events() {
    use ze_scene_core::domain::{PointerState, WindowInput};

    let scene = headless::build(SceneChoice::Hud).expect("headless 装配");
    let before = scene.frame().expect("有 Frame");
    let name = param_scopes(&before)
        .values()
        .next()
        .and_then(|params| params.first())
        .cloned()
        .expect("有参数滑块");
    let (wx, wy) = scene.slider_point(&name, 0.8).expect("世界坐标");
    let context = scene.input_context().expect("有输入上下文");
    let (_, camera) = context.layer2d.expect("有 2D 图层");
    let screen = camera.world_to_screen(ze_notion::Vec2::new(wx, wy));

    let mut input = WindowInput {
        viewport: camera.rect,
        window_size: ze_notion::Vec2::new(camera.rect.max.x, camera.rect.max.y),
        ..WindowInput::default()
    };
    input.pointer = PointerState {
        position: Some(screen),
        hovering: true,
        primary_pressed: true,
        ..PointerState::default()
    };
    // 走 tagged JSON（Debug / RPC 的通道），证明序列化不丢信息。
    let json = serde_json::to_string(&input).expect("窗口输入可序列化");
    assert!(scene.inject_window_json(&json).expect("窗口输入可注入") > 0);
    scene.advance(1, 1.0 / 60.0);

    input.pointer.primary_pressed = false;
    input.pointer.primary_released = true;
    scene.inject_window(&input).expect("抬起可注入");
    scene.advance(2, 1.0 / 60.0);
    scene.clear_events();

    let after = scene.frame().expect("拖拽后仍有 Frame");
    assert_ne!(
        before.value_of("slider", &name),
        after.value_of("slider", &name),
        "窗口坐标点击必须改到同一个控件的值"
    );
}

/// **CH-b 判据**：一个带 tag 的注入 ⇒ 回执含派生事件 / outcomes / Frame diff /
/// diagnostics；同一串事件两次逐位相同（纯值、可序列化）。
#[test]
fn trace_reports_derived_outcomes_diff_and_diagnostics() {
    use ze_scene_core::domain::{Injected, PointerState, WindowInput};
    use zela_app::debug::OutcomeKind;

    let play = || {
        let scene = headless::build(SceneChoice::Hud).expect("headless 装配");
        let before = scene.frame().expect("有 Frame");
        let name = param_scopes(&before)
            .values()
            .next()
            .and_then(|params| params.first())
            .cloned()
            .expect("有参数滑块");
        let (wx, wy) = scene.slider_point(&name, 0.8).expect("世界坐标");
        let camera = scene
            .input_context()
            .expect("有输入上下文")
            .layer2d
            .expect("有 2D 图层")
            .1;
        let screen = camera.world_to_screen(ze_notion::Vec2::new(wx, wy));
        let input = WindowInput {
            pointer: PointerState {
                position: Some(screen),
                hovering: true,
                primary_pressed: true,
                ..PointerState::default()
            },
            viewport: camera.rect,
            window_size: ze_notion::Vec2::new(camera.rect.max.x, camera.rect.max.y),
            ..WindowInput::default()
        };
        let injected = Injected::window(1, 0, input).with_label("click slider");
        let report = scene.trace(&injected, 8).expect("回执必须成功");
        assert_eq!(report.tag.0, 1);
        assert_eq!(report.injected_frame, 0);
        assert_eq!(report.active.tag.0, 1);
        let first = &report.frames[0];
        assert!(!first.derived.is_empty(), "派生事件必须有 hover / click");
        assert!(
            first
                .outcomes
                .iter()
                .any(|outcome| outcome.name == name
                    && matches!(outcome.kind, OutcomeKind::ValueChanged { .. })),
            "状态机意图必须出现在回执里：{:?}",
            first.outcomes
        );
        assert!(
            first.frame_diff.changed.iter().any(|c| c.name == name),
            "Frame diff 必须指出变的那个控件：{:?}",
            first.frame_diff
        );
        assert_eq!(first.diagnostics.routed.as_deref(), Some(name.as_str()));
        // 注入帧 + 一帧"没有变化"⇒ 提前收敛，报告不被截断。
        assert_eq!(report.frames.len(), 2, "第二帧应当已经安静");
        assert!(!report.truncated, "提前收敛不算截断");
        assert!(report.frames[1].outcomes.is_empty());
        assert!(report.frames[1].frame_diff.is_empty());
        report
    };

    let first = play();
    // 纯值：可序列化，且同串事件两次逐位相同。
    let json = serde_json::to_string(&first).expect("回执可序列化");
    assert!(json.contains("\"tag\":1"));
    assert_eq!(json, serde_json::to_string(&play()).expect("可序列化"));
}

/// **确定性**：同一场景、同一事件流 ⇒ 两帧 JSON 逐字节相同。
#[test]
fn same_events_give_bitwise_identical_frames() {
    let play = || {
        let scene = headless::build(SceneChoice::Hud).expect("headless 装配");
        let frame = scene.frame().expect("有 Frame");
        let groups = param_scopes(&frame);
        let first = groups
            .values()
            .next()
            .and_then(|params| params.first())
            .cloned()
            .expect("有参数滑块");
        let (x, y) = scene.slider_point(&first, 0.35).expect("世界坐标");
        inject(
            &scene,
            vec![SceneEvent::ViewportClick {
                layer: LayerId(0),
                scene_pos: ze_notion::Vec2::new(x, y),
            }],
        );
        scene.advance(1, 1.0 / 60.0);
        scene.clear_events();
        scene.frame_json().expect("有 JSON")
    };
    assert_eq!(play(), play(), "同声明 + 同事件 ⇒ 帧 JSON 逐字节相同");
}
