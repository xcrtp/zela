//! S3 验收（ECS 侧）：表达式绑定从声明到**真实组件写入**的整条链路。
//!
//! 覆盖：
//! 1. `SceneEncoder::bind_expr` → `commit` → `BindingApplySystem` 写到 `Circle.radius`；
//! 2. 表达式由宿主单子 `M` 求值（Time 通道来自 `TimeState`）；
//! 3. 未注入通道 / 越界句柄 ⇒ **装配期显式错误**。
//!
//! 提案 #6 B：对象用**句柄**（`spawn` 的返回值），不再依赖 `Name{}` 组件。

use ze_func::{ChannelRef, Expr, OpRef};
use ze_scene_core::animation::LibraryRef;
use ze_scene_core::object::Circle;
use ze_scene_core::scene_script::{SceneEncoder, SceneScript};
use zela_schedule::resource::Resources;
use zela_schedule::scheduler::{Phase, Scheduler};

use crate::TimeState;
use crate::animation::BindingApplySystem;
use crate::scene::Scene;

fn library() -> LibraryRef {
    LibraryRef {
        namespace: "zela.math".to_string(),
        version: "4".to_string(),
    }
}

fn circle() -> ze_scene_contract::Components {
    vec![Box::new(Circle::new((0.0, 0.0), 0.25))]
}

fn sin_time() -> Expr {
    Expr::call(
        OpRef::new("zela.math", "sin"),
        [Expr::channel(ChannelRef::builtin("time"))],
    )
}

fn radius_at(script: ze_scene_core::scene_script::SceneScript, time: f32) -> f32 {
    let mut scene = Scene::new();
    let recipe = script.commit(&mut scene).expect("commit 成功");
    let world = scene.into_world();

    let mut resources = Resources::new();
    resources.insert(TimeState::at_frame(time, 0));
    resources.insert(recipe.forest);
    resources.insert(recipe.bindings);

    let mut sched = Scheduler::new(world, resources);
    sched.add_system(Phase::SceneSync, BindingApplySystem);
    sched.init_resources();
    sched.run_frame(0.0);

    let world = sched.world();
    let mut query = world.query::<&Circle>();
    query
        .iter()
        .next()
        .map(|circle| circle.radius)
        .expect("场景里有一个圆")
}

#[test]
fn expression_binding_writes_the_component_through_the_host_monad() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    enc.bind_expr(dot, "CircleRadius", sin_time(), library(), 10.0)
        .expect("装配期校验通过");
    let script = enc.finish();

    // t = π/2 ⇒ sin = 1；t = π ⇒ sin = 0。
    assert!((radius_at(script.clone(), std::f32::consts::FRAC_PI_2) - 1.0).abs() < 1e-5);
    assert!(radius_at(script, std::f32::consts::PI).abs() < 1e-5);
}

/// `--bundle` 路径的前提：表达式绑定必须随 ZSB 的 JSON 往返保留。
#[test]
fn expression_binding_survives_json_round_trip() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    enc.bind_expr(dot, "CircleRadius", sin_time(), library(), 2.0)
        .expect("装配期校验通过");
    let json = enc.finish().to_json().expect("序列化成功");

    // 库身份也在 JSON 里（产物自带 namespace + version）。
    assert!(json.contains("zela.math"), "payload 必须带函子库身份");
    assert!(json.contains("\"version\":\"4\""), "payload 必须带库版本");

    let restored = SceneScript::from_json(&json).expect("反序列化成功");
    // 往返之后仍能求值并写组件（这就是 `--bundle` 路径的数据链）。
    let radius = radius_at(restored, std::f32::consts::FRAC_PI_2);
    assert!((radius - 1.0).abs() < 1e-5, "sin(π/2) = 1，实测 {radius}");
}

/// `append`（合并脚本）必须把表达式绑定的句柄一起偏移。
#[test]
fn append_shifts_expression_target_handles() {
    let mut a = SceneEncoder::new();
    let first = a.spawn(circle());
    a.bind_expr(first, "CircleRadius", sin_time(), library(), 1.0)
        .expect("a 绑定成功");
    let mut b = SceneEncoder::new();
    let second = b.spawn(circle());
    b.bind_expr(second, "CircleRadius", sin_time(), library(), 1.0)
        .expect("b 绑定成功");

    let mut script = a.finish();
    script.append(b.finish());
    let mut scene = Scene::new();
    let recipe = script.commit(&mut scene).expect("commit 成功");
    assert_eq!(recipe.bindings.bindings.len(), 2);
    let targets: Vec<_> = recipe
        .bindings
        .bindings
        .iter()
        .map(|binding| binding.target.target_id())
        .collect();
    assert_eq!(targets[0], recipe.handles[0], "第一条指向 a");
    assert_eq!(targets[1], recipe.handles[1], "偏移后第二条指向 b");
}

#[test]
fn unimplemented_channels_fail_at_assembly() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    let expr = Expr::channel(ChannelRef::builtin("event"));
    let error = enc
        .bind_expr(dot, "CircleRadius", expr, library(), 1.0)
        .expect_err("Event 通道尚未注入 ⇒ 必须失败");
    assert_eq!(
        error,
        ze_scene_core::scene_script::ExprBindError::UnsupportedChannel(ChannelRef::builtin("event"))
    );
}

/// 组件字段通道与基础通道同形，但读方向还没接进宿主 M ⇒ 同样装配期显式失败。
#[test]
fn component_channels_are_not_injected_yet() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    let channel = ChannelRef::new("zela.scene", "Circle.radius");
    let error = enc
        .bind_expr(
            dot,
            "OpacityValue",
            Expr::channel(channel.clone()),
            library(),
            1.0,
        )
        .expect_err("组件字段通道尚未注入 ⇒ 必须失败");
    assert_eq!(
        error,
        ze_scene_core::scene_script::ExprBindError::UnsupportedChannel(channel)
    );
}

#[test]
fn unknown_object_fails_at_assembly() {
    let mut enc = SceneEncoder::new();
    enc.spawn(circle());
    let error = enc
        .bind_expr(
            ze_scene_core::scene_script::Handle(7),
            "CircleRadius",
            sin_time(),
            library(),
            1.0,
        )
        .expect_err("句柄越界 ⇒ 必须失败");
    assert_eq!(
        error,
        ze_scene_core::scene_script::ExprBindError::UnknownObject(7)
    );
}
