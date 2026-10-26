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
use crate::animation::ResetBehavior;
use crate::scene::Scene;

fn library() -> LibraryRef {
    LibraryRef {
        namespace: "zela.math".to_string(),
        version: "4".to_string(),
    }
}

/// 全局时钟（空链）：这些用例只关心表达式本身，不需要额外的时钟层。
fn global_clock() -> crate::ClockProgram {
    crate::ClockProgram::new()
}

/// 同一调度器上按 `times` 顺序推帧，返回最后一帧结束后圆的半径。
///
/// predicate / reset 是**有状态**的行为（hold 依赖"最后一次生效值"），
/// 单帧断言覆盖不到 ⇒ 用多帧推进。
fn radius_over_frames(script: SceneScript, times: &[f32]) -> f32 {
    let mut scene = Scene::new();
    let recipe = script.commit(&mut scene).expect("commit 成功");
    let world = scene.into_world();

    let mut resources = Resources::new();
    resources.insert(TimeState::at(0.0));
    resources.insert(recipe.forest);
    resources.insert(recipe.bindings);

    let mut sched = Scheduler::new(world, resources);
    sched.add_system(Phase::SceneSync, BindingApplySystem);
    sched.init_resources();
    for time in times {
        sched
            .resources_mut()
            .get_mut::<TimeState>()
            .unwrap()
            .elapsed = *time;
        sched.run_frame(0.0);
    }
    sched
        .world()
        .query::<&Circle>()
        .iter()
        .next()
        .map(|circle| circle.radius)
        .expect("场景里有一个圆")
}

/// `predicate = Lt { time, 1.0 }`（Bool 表达式；`time` 是绑定时钟的局部时间）。
fn time_lt(limit: f64) -> Expr {
    Expr::call(
        OpRef::new("zela.math", "lt"),
        [
            Expr::channel(ChannelRef::builtin("time")),
            Expr::pure(limit),
        ],
    )
}

/// `predicate = time > start && time < end`（可分析成有限区间）。
fn time_window(start: f64, end: f64) -> Expr {
    Expr::call(
        OpRef::new("zela.math", "and"),
        [
            Expr::call(
                OpRef::new("zela.math", "gt"),
                [
                    Expr::channel(ChannelRef::builtin("time")),
                    Expr::pure(start),
                ],
            ),
            Expr::call(
                OpRef::new("zela.math", "lt"),
                [
                    Expr::channel(ChannelRef::builtin("time")),
                    Expr::pure(end),
                ],
            ),
        ],
    )
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
    enc.bind_expr(dot, "CircleRadius", sin_time(), library(), global_clock(), 10.0)
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
    enc.bind_expr(dot, "CircleRadius", sin_time(), library(), global_clock(), 2.0)
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
    a.bind_expr(first, "CircleRadius", sin_time(), library(), global_clock(), 1.0)
        .expect("a 绑定成功");
    let mut b = SceneEncoder::new();
    let second = b.spawn(circle());
    b.bind_expr(second, "CircleRadius", sin_time(), library(), global_clock(), 1.0)
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

/// **B 批**：表达式里的 `time` 读**该绑定的时钟局部时间**（与 Curve 路径同源）。
///
/// 时钟链 `[shift 1]` ⇒ `local = t − 1`。
#[test]
fn expression_binding_reads_its_own_clock_local_time() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    let clock = crate::ClockProgram {
        links: vec![crate::ClockLink {
            shift: 1.0,
            rate: 1.0,
        }],
    };
    enc.bind_expr(dot, "CircleRadius", sin_time(), library(), clock, 2.0)
        .expect("装配期校验通过");

    // 全局 t = 1 + π/2 ⇒ 局部 π/2 ⇒ sin = 1（用的是局部时间，不是全局）。
    let shifted = 1.0 + std::f32::consts::FRAC_PI_2;
    assert!((radius_at(enc.finish(), shifted) - 1.0).abs() < 1e-5);
}

/// **C 批**：`duration` 的糖 = `predicate: t < d` + `reset: hold` ⇒ 失效后**保持**最后值。
///
/// AH-07：Hold 现在是纯函数 `f(last_active(t))` —— 保持的是**窗口右端**的函数值
/// （`sin(2)`），不是"最后一个渲染帧恰好写下的值"。
#[test]
fn duration_sugar_holds_after_the_window() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    let clock = crate::ClockProgram {
        links: vec![crate::ClockLink {
            shift: 1.0,
            rate: 1.0,
        }],
    };
    // 局部时间 0..2 生效；t=10 时局部 9 ⇒ predicate 失效，反算到最后生效时刻 2。
    enc.bind_expr(dot, "CircleRadius", sin_time(), library(), clock, 2.0)
        .expect("装配期校验通过");
    let held = radius_over_frames(enc.finish(), &[1.0 + std::f32::consts::FRAC_PI_2, 10.0]);
    assert!(
        (held - 2.0f32.sin()).abs() < 1e-5,
        "hold = f(窗口右端) = sin(2)，实测 {held}"
    );
}

/// **C 批验收**：`And { Lt, Gt }` 可分析；Hold 在**单帧 seek** 下也正确
/// （不需要先跑前面的帧 ⇒ 零状态 / O1）。
#[test]
fn hold_is_pure_when_the_predicate_is_an_interval_window() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    let predicate = time_window(1.0, 4.0);
    enc.bind_expr_gated(
        dot,
        "CircleRadius",
        sin_time(),
        library(),
        global_clock(),
        Some(predicate),
        ResetBehavior::Hold,
        0.0,
    )
    .expect("装配期校验通过");
    let script = enc.finish();

    // 窗口 (1, 4)：窗口内 = sin(t)；窗口后用 sin(4)；首次生效前不写（保持初值 0.25）。
    assert!((radius_at(script.clone(), 2.0) - 2.0f32.sin()).abs() < 1e-5);
    assert!(
        (radius_at(script.clone(), 9.0) - 4.0f32.sin()).abs() < 1e-5,
        "单帧 seek 到窗口之后也必须得到 f(4)"
    );
    assert!((radius_at(script, 0.5) - 0.25).abs() < 1e-5);
}

/// `reset = Reset`：失效时写回 **commit 时的初值**（0.25），而不是保持最后值。
#[test]
fn reset_restores_the_initial_value() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    enc.bind_expr_gated(
        dot,
        "CircleRadius",
        sin_time(),
        library(),
        global_clock(),
        Some(time_lt(1.0)),
        ResetBehavior::Reset,
        0.0,
    )
    .expect("装配期校验通过");
    let radius = radius_over_frames(enc.finish(), &[0.5, 2.0]);
    assert!(
        (radius - 0.25).abs() < 1e-5,
        "失效后应写回初值 0.25，实测 {radius}"
    );
}

/// `reset = Skip`：失效时不写 ⇒ 字段保持最后值（与 hold 可观察结果相同，但**不需要历史**）。
#[test]
fn skip_leaves_the_field_untouched() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    enc.bind_expr_gated(
        dot,
        "CircleRadius",
        sin_time(),
        library(),
        global_clock(),
        Some(time_lt(1.0)),
        ResetBehavior::Skip,
        0.0,
    )
    .expect("装配期校验通过");
    let radius = radius_over_frames(enc.finish(), &[0.5, 2.0]);
    assert!((radius - 0.5f32.sin()).abs() < 1e-5);
}

/// 糖层展开后的 payload 与手写 predicate + hold **逐字节相同**。
#[test]
fn duration_sugar_equals_predicate_and_hold() {
    let mut sugar = SceneEncoder::new();
    let dot = sugar.spawn(circle());
    sugar
        .bind_expr(dot, "CircleRadius", sin_time(), library(), global_clock(), 2.0)
        .expect("糖写法装配成功");

    let mut explicit = SceneEncoder::new();
    let dot = explicit.spawn(circle());
    explicit
        .bind_expr_gated(
            dot,
            "CircleRadius",
            sin_time(),
            library(),
            global_clock(),
            Some(time_lt(2.0)),
            ResetBehavior::Hold,
            0.0,
        )
        .expect("显式写法装配成功");

    assert_eq!(
        sugar.finish().to_json().expect("serialize sugar"),
        explicit.finish().to_json().expect("serialize explicit"),
        "duration ≡ predicate t<d + reset hold"
    );
}

/// **D 批**：静止时刻由 predicate 的**纯 Time 依赖**推出（含时钟的仿射换算）。
#[test]
fn still_time_is_derived_from_time_only_predicates() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    // local = t × 0.5 − 1；窗口 local < 2 ⇒ 全局 t = (2 + 1) / 0.5 = 6。
    let clock = crate::ClockProgram {
        links: vec![
            crate::ClockLink {
                shift: 0.0,
                rate: 0.5,
            },
            crate::ClockLink {
                shift: 1.0,
                rate: 1.0,
            },
        ],
    };
    enc.bind_expr(dot, "CircleRadius", sin_time(), library(), clock, 2.0)
        .expect("装配期校验通过");
    assert_eq!(enc.finish().still_time(), Some(6.0));
}

/// 不可分析时**不给错的数**：恒生效 / 非 `Lt{time}` 形状 ⇒ `None`。
#[test]
fn still_time_is_none_when_not_analyzable() {
    // 恒生效（没有 predicate）：永远在动。
    let mut always = SceneEncoder::new();
    let dot = always.spawn(circle());
    always
        .bind_expr_gated(
            dot,
            "CircleRadius",
            sin_time(),
            library(),
            global_clock(),
            None,
            ResetBehavior::Skip,
            0.0,
        )
        .expect("装配成功");
    assert_eq!(always.finish().still_time(), None);

    // predicate 是 `Gt`：可分析，但区间延伸到 +∞ ⇒ 永远生效，没有静止时刻。
    let mut other = SceneEncoder::new();
    let dot = other.spawn(circle());
    let predicate = Expr::call(
        OpRef::new("zela.math", "gt"),
        [
            Expr::channel(ChannelRef::builtin("time")),
            Expr::pure(1.0),
        ],
    );
    other
        .bind_expr_gated(
            dot,
            "CircleRadius",
            sin_time(),
            library(),
            global_clock(),
            Some(predicate),
            ResetBehavior::Hold,
            0.0,
        )
        .expect("装配成功");
    assert_eq!(other.finish().still_time(), None);
}

/// **AH-06**：`And { Lt, Gt }` 的区间集也能给出静止时刻（不再"一用 And 就不可分析"）。
#[test]
fn still_time_understands_interval_windows() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    enc.bind_expr_gated(
        dot,
        "CircleRadius",
        sin_time(),
        library(),
        global_clock(),
        Some(time_window(1.0, 4.0)),
        ResetBehavior::Hold,
        0.0,
    )
    .expect("装配成功");
    assert_eq!(enc.finish().still_time(), Some(4.0));
}

/// **AH-05**：可分析计入、不可分析**跳过** —— 一条不可分析不再拖垮整部作品。
#[test]
fn still_time_skips_unanalyzable_bindings() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    enc.bind_expr(dot, "CircleRadius", sin_time(), library(), global_clock(), 2.0)
        .expect("可分析绑定");

    // `Eq { time, time }`：边界不是字面量 ⇒ 不可分析（被跳过，而不是让整体 None）。
    let other = enc.spawn(circle());
    let opaque = Expr::call(
        OpRef::new("zela.math", "eq"),
        [
            Expr::channel(ChannelRef::builtin("time")),
            Expr::channel(ChannelRef::builtin("time")),
        ],
    );
    enc.bind_expr_gated(
        other,
        "CircleRadius",
        sin_time(),
        library(),
        global_clock(),
        Some(opaque),
        ResetBehavior::Hold,
        0.0,
    )
    .expect("不可分析也可以装配");
    assert_eq!(enc.finish().still_time(), Some(2.0));
}

/// **AH-05**：全部不可分析才 `None`。
#[test]
fn still_time_is_none_when_every_predicate_is_unanalyzable() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    let opaque = Expr::call(
        OpRef::new("zela.math", "eq"),
        [
            Expr::channel(ChannelRef::builtin("time")),
            Expr::channel(ChannelRef::builtin("time")),
        ],
    );
    enc.bind_expr_gated(
        dot,
        "CircleRadius",
        sin_time(),
        library(),
        global_clock(),
        Some(opaque),
        ResetBehavior::Hold,
        0.0,
    )
    .expect("装配成功");
    assert_eq!(enc.finish().still_time(), None);
}

/// 嵌套时钟：`clock.nest(scale(0.5), shift(1))` ⇒ `local = t × 0.5 − 1`。
#[test]
fn nested_clocks_compose_linearly() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    let clock = crate::ClockProgram {
        links: vec![
            crate::ClockLink {
                shift: 0.0,
                rate: 0.5,
            },
            crate::ClockLink {
                shift: 1.0,
                rate: 1.0,
            },
        ],
    };
    enc.bind_expr(dot, "CircleRadius", sin_time(), library(), clock, 100.0)
        .expect("装配期校验通过");
    let script = enc.finish();

    // local = t × 0.5 − 1；取 t = 2 + π ⇒ local = π/2 ⇒ sin = 1。
    let t = 2.0 + std::f32::consts::PI;
    assert!((radius_at(script, t) - 1.0).abs() < 1e-5);
}

#[test]
fn unimplemented_channels_fail_at_assembly() {
    let mut enc = SceneEncoder::new();
    let dot = enc.spawn(circle());
    let expr = Expr::channel(ChannelRef::builtin("event"));
    let error = enc
        .bind_expr(dot, "CircleRadius", expr, library(), global_clock(), 1.0)
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
            global_clock(),
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
            global_clock(),
            1.0,
        )
        .expect_err("句柄越界 ⇒ 必须失败");
    assert_eq!(
        error,
        ze_scene_core::scene_script::ExprBindError::UnknownObject(7)
    );
}
