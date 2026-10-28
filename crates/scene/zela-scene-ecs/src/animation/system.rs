// system.rs — BindingApplySystem + 等价性测试
//
// 把时钟树 + 曲线 应用到 hecs 组件。SceneSync 阶段。
// 纯 Resource 读、组件（ctx.world）写。
// 等价性单测：新链路 vs 旧 AnimationApplySystem 公式，误差 < 1e-5。
//
// P0：目标由 `BindingTarget` 枚举改为 `Arc<dyn Target>` —— 本 System 只做
// "遍历 + 求值 + 派发"，**不认识任何具体目标**（新增目标零 System 改动）。

use ze_time::{AnimValue, ClockForest};
use zela_scene_ecs::TimeState;
use zela_schedule::context::ExecutorContext;
use zela_schedule::system::System;

use crate::animation::binding::BindingDriver;
use crate::animation::binding::BindingTable;
use crate::animation::binding::ResetBehavior;
use crate::contract::WorldScene;
use crate::expr_mono::{SceneMono, anim_value};

/// 遍历 BindingTable，按 `Binding.target` 把 `Curve::sample(forest.eval_local(clock, t))` 写入组件字段。
pub struct BindingApplySystem;

impl System for BindingApplySystem {
    type Reads = (TimeState, ClockForest, BindingTable);
    type Writes = ();

    fn run(&mut self, ctx: &mut ExecutorContext<'_>) {
        // 先从 ctx.get 取出所有数据（&mut ctx 的借用到此释放），
        // 再独立用 ctx.world 做 &mut query。
        let time_state = ctx.get::<TimeState>().copied().unwrap_or_default();
        let t_global = time_state.elapsed as f64;
        let frame = time_state.frame;
        let forest = match ctx.get::<ClockForest>().cloned() {
            Some(f) => f,
            None => return,
        };
        let table = match ctx.get::<BindingTable>().cloned() {
            Some(t) => t,
            None => return,
        };

        // 目标层**不认识 hecs**：把世界包成契约层适配器再派发，
        // `apply` 内部只走契约方法（`Scene::get_mut`）。
        //
        // S1（G1）：多段绑定必须按段起点升序应用，且**段前不写**。
        // - 段前不写：后段不会用 `from` 初值覆盖前段/初始状态；
        // - 升序 + 稳定排序：同一段内 `parallel` 保持声明顺序（同字段冲突时后者胜，
        //   行为确定，不依赖 HashMap 遍历顺序）；
        // - 段后照常写：`Hold` 求值已钳到末值，写它即冻结。
        let mut order: Vec<usize> = (0..table.bindings.len()).collect();
        order.sort_by(|&a, &b| {
            forest
                .start_offset(table.bindings[a].clock)
                .partial_cmp(&forest.start_offset(table.bindings[b].clock))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut scene = WorldScene::new(&mut *ctx.world);
        for index in order {
            let b = &table.bindings[index];
            if forest.is_before_start(b.clock, t_global) {
                continue;
            }
            let lt = forest.eval_local(b.clock, t_global);
            // **C 批**：predicate 与 root **分开求值**（Bool 不经过 `anim_value`）。
            // 可分析（区间集）时生效判定是纯函数，不再逐帧求值 predicate。
            let active = match (&b.active, &b.predicate) {
                (Some(set), _) => set.time_contains(lt),
                (None, None) => true,
                (None, Some(predicate)) => {
                    let mono = SceneMono::with_global(lt, t_global, frame, &scene);
                    match predicate.eval(&mono) {
                        Ok(value) => match value.as_bool() {
                            Some(value) => value,
                            None => {
                                eprintln!(
                                    "[expr] 绑定目标 {} 的 predicate 必须返回 Bool，实际 {value:?}（本帧按不生效处理）",
                                    b.target.name()
                                );
                                false
                            }
                        },
                        Err(error) => {
                            eprintln!(
                                "[expr] 绑定目标 {} 的 predicate 求值失败：{error}",
                                b.target.name()
                            );
                            false
                        }
                    }
                }
            };
            // 本帧要**在哪个局部时间求值**：
            // - 生效 ⇒ 当前局部时间；
            // - `reset = Reset` ⇒ 写回初值（不评价 root）；
            // - `reset = Hold` 且 predicate 可分析 ⇒ `last_active(t)`（AH-07：纯函数，
            //   seek 到任意一帧都能直接算，与绑定顺序无关）；
            // - 其余（Skip / 不可分析的 Hold）⇒ 不写。
            let eval_at = if active {
                Some(lt)
            } else {
                match b.reset {
                    ResetBehavior::Reset => {
                        if let Some(initial) = &b.initial {
                            if let Err(error) = b.target.apply(&mut scene, initial) {
                                eprintln!("[anim] {error}");
                            }
                        }
                        None
                    }
                    ResetBehavior::Skip => None,
                    ResetBehavior::Hold => b.active.as_ref().and_then(|set| set.time_last_active(lt)),
                }
            };
            let Some(eval_at) = eval_at else {
                continue;
            };
            let value: AnimValue = match &b.driver {
                BindingDriver::Curve(curve) => curve.sample(eval_at),
                // **B 批**：表达式里的 `time` 读**该绑定的时钟局部时间**（与 Curve 同源），
                // 不再用全局 elapsed —— G1（两条路径 t 不同源）由此消失。
                BindingDriver::Expr(program) => match program
                    .eval(&SceneMono::with_global(eval_at, t_global, frame, &scene))
                {
                    Ok(value) => match anim_value(&value) {
                        Some(value) => value,
                        None => {
                            // 类型在装配期已校验；这里只兜底，显式记录而不是写 0。
                            eprintln!(
                                "[expr] 绑定目标 {} 不支持值类型 {:?}（本帧跳过）",
                                b.target.name(),
                                value
                            );
                            continue;
                        }
                    },
                    Err(error) => {
                        eprintln!("[expr] 绑定目标 {} 求值失败：{error}", b.target.name());
                        continue;
                    }
                },
            };
            // 目标自己负责"类型匹配 + 写组件"；**类型不符显式报错**（不再静默跳过）。
            if let Err(error) = b.target.apply(&mut scene, &value) {
                eprintln!("[anim] {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BindingApplySystem;
    use crate::TimeState;
    use crate::animation::binding::{Binding, BindingDriver, BindingTable, SeekKind};
    use crate::animation::target::CircleRadius;
    use hecs::World;
    use ze_notion::Vec3;
    use ze_scene_core::Circle;
    use ze_time::{ClockForest as XcClockForest, ClockNode, Curve};
    use zela_schedule::resource::Resources;
    use zela_schedule::scheduler::{Phase, Scheduler};

    /// 场景向量（M2.5 后 `zela_scene_ecs::types::Vec2` 就是 `ze_notion::Vec2`（= glam）的转发，
    /// 图元对象用的也是同一类型 —— 原来的 `vec2` / `obj_vec2` 两个包装已被消灭）。
    fn vec2(x: f32, y: f32) -> ze_notion::Vec2 {
        ze_notion::Vec2::new(x, y)
    }

    /// 等价性测试：旧公式 vs 新链路，1000 个采样点最大误差 < 1e-5。
    #[test]
    fn equivalence_sin_radius() {
        // 旧：radius = base + amp * sin(t * omega)
        let base = 1.0f32;
        let amp = 0.4f32;
        let omega = 2.0f32;

        let mut forest = XcClockForest::new();
        let root = forest.insert(ClockNode::new(None)).unwrap();

        let curve = Curve::Sin { base, amp, omega };

        for i in 0..1000 {
            let t_global = (i as f64) * 0.01;
            let lt = forest.eval_local(root, t_global);
            let new_v = curve.sample(lt).scalar();
            let old_v = base + amp * (t_global as f32 * omega).sin();
            assert!(
                (new_v - old_v).abs() < 1e-5,
                "t={t_global}: new={new_v}, old={old_v}"
            );
        }
    }

    #[test]
    fn equivalence_phase_linear() {
        let omega = 1.5f32;
        let mut forest = XcClockForest::new();
        let root = forest.insert(ClockNode::new(None)).unwrap();
        let curve = Curve::Phase { omega };

        for t_global in [0.0f64, 1.0, 2.5, -3.0, 500.0] {
            let lt = forest.eval_local(root, t_global);
            let new_v = curve.sample(lt).scalar();
            let old_v = omega * t_global as f32;
            assert!((new_v - old_v).abs() < 1e-5);
        }
    }

    /// 真等价性测试：新 Curve::Orbit vs 旧 OrbitAnim 公式（参考实现内联保留）。
    /// t ∈ [0, 10] 采 1000 点逐点比对（不只是"轨迹是圆"，起点也必须一致）。
    #[test]
    fn equivalence_orbit_matches_legacy_orbit_anim() {
        // 参考（旧 OrbitAnim）实现：
        // eye_offset = Quat::from_axis_angle(axis, angle) * Vec3::new(0, 0, radius)
        fn legacy_orbit_offset(
            radius: f32,
            axis: Vec3,
            speed: f32,
            initial_angle: f32,
            t: f64,
        ) -> Vec3 {
            let angle = initial_angle + (t as f32) * speed;
            let axis = if axis.length_squared() > f32::EPSILON {
                axis.normalize()
            } else {
                Vec3::Y
            };
            ze_notion::Quat::from_axis_angle(axis, angle) * Vec3::new(0.0, 0.0, radius)
        }

        let mut forest = XcClockForest::new();
        let root = forest.insert(ClockNode::new(None)).unwrap();

        let curve = Curve::Orbit {
            radius: 5.0,
            axis: Vec3::Y,
            speed: 0.5,
            initial_angle: 0.0,
        };

        for i in 0..1000 {
            let t_global = (i as f64) * 0.01; // t ∈ [0, 10)
            let new_v = curve.sample(forest.eval_local(root, t_global)).vec3();
            let old_v = legacy_orbit_offset(5.0, Vec3::Y, 0.5, 0.0, t_global);

            assert!(
                (new_v - old_v).length() < 1e-5,
                "t={t_global}: new={new_v}, old={old_v}"
            );
        }
    }

    #[test]
    fn binding_apply_writes_circle_radius_via_scheduler() {
        let mut world = World::new();
        let entity = world.spawn((Circle::new(vec2(0.0, 0.0), 1.0),));

        let mut forest = XcClockForest::new();
        let root = forest.insert(ClockNode::new(None)).unwrap();

        let mut table = BindingTable::new();
        table.add(Binding {
            clock: root,
            driver: BindingDriver::Curve(Curve::Sin {
                base: 1.0,
                amp: 0.5,
                omega: 1.0,
            }),
            target: std::sync::Arc::new(CircleRadius {
                id: crate::contract::object_id_of(entity),
            }),
            predicate: None,
            active: None,
            reset: crate::animation::ResetBehavior::Hold,
            initial: None,
            seek_kind: SeekKind::O1,
        });

        let mut resources = Resources::new();
        resources.insert(TimeState::at(0.0));
        resources.insert(forest);
        resources.insert(table);

        let mut scheduler = Scheduler::new(world, resources);
        scheduler.add_system(Phase::SceneSync, BindingApplySystem);
        scheduler.run_frame(0.1);

        let radius = scheduler.world().get::<&Circle>(entity).unwrap().radius;
        // t=0, sin(0)=0 → radius = 1.0 + 0.5 * 0 = 1.0
        assert!((radius - 1.0).abs() < 1e-5);
    }

    /// **S1 / G1**：后段的 `from` 初值不得从第 0 帧覆盖前段。
    ///
    /// 两段 `Const`（55 → 170），中间 `wait`：
    /// - t=0.5：第一段 → 55；
    /// - t=1.5：第二段尚未开始 ⇒ **不写**，保持 55（修复前是 170）；
    /// - t=2.5：第二段段内 → 170。
    #[test]
    fn later_segment_is_not_written_before_start() {
        use crate::animation::target::Camera2DFraming;
        use std::sync::Arc;
        use ze_comb::{ChannelRef, CompiledExpr, Expr, OpRef};
        use ze_scene_core::Framing2D;

        let mut world = World::new();
        let entity = world.spawn((Framing2D::default(),));
        let id = crate::contract::object_id_of(entity);

        let mut forest = XcClockForest::new();
        let mut table = BindingTable::new();
        // 窗口 `[0, 1)`：`time` 是**该绑定的局部时间**（起点 2 的绑定在 t<2 时为负），
        // 必须显式写 `t >= 0`，否则"段前"会被判成生效。
        let window = CompiledExpr::compile(&Expr::call(
            OpRef::new("zela.math", "and"),
            [
                Expr::call(
                    OpRef::new("zela.math", "ge"),
                    [
                        Expr::channel(ChannelRef::builtin("time")),
                        Expr::pure(0.0),
                    ],
                ),
                Expr::call(
                    OpRef::new("zela.math", "lt"),
                    [
                        Expr::channel(ChannelRef::builtin("time")),
                        Expr::pure(1.0),
                    ],
                ),
            ],
        ))
        .expect("可编译");
        // 两条窗口绑定：起点 0 / 2，值 55 / 170（旧 `play 1.0 / wait 1.0 / play 1.0`）。
        for (shift, value) in [(0.0f64, 55.0f64), (2.0, 170.0)] {
            let mut node = ClockNode::new(None);
            node.local_offset = shift;
            let clock = forest.insert(node).expect("插入节点");
            table.bind_expr_gated(
                clock,
                CompiledExpr::compile(&Expr::pure(value)).expect("可编译"),
                Arc::new(Camera2DFraming { id }),
                Some(window.clone()),
                None,
                crate::animation::ResetBehavior::Hold,
            );
        }

        let mut resources = Resources::new();
        resources.insert(TimeState::at(0.0));
        resources.insert(forest);
        resources.insert(table);

        let mut sched = Scheduler::new(world, resources);
        sched.add_system(Phase::SceneSync, BindingApplySystem);
        sched.init_resources();

        let scale_at = |sched: &mut Scheduler, t: f32| -> f32 {
            sched
                .resources_mut()
                .get_mut::<TimeState>()
                .unwrap()
                .elapsed = t;
            sched.run_frame(0.0);
            sched.world().get::<&Framing2D>(entity).unwrap().scale
        };
        assert_eq!(scale_at(&mut sched, 0.5), 55.0, "第一段");
        assert_eq!(
            scale_at(&mut sched, 1.5),
            55.0,
            "第二段段前不得写入 170（G1）"
        );
        assert_eq!(scale_at(&mut sched, 2.5), 170.0, "第二段段内");
    }

    /// **S1 边界（§2.3）**：同起点写同一字段时，后声明者胜（稳定排序）。
    #[test]
    fn same_start_same_field_last_declaration_wins() {
        use crate::animation::target::Camera2DFraming;
        use std::sync::Arc;
        use ze_comb::{CompiledExpr, Expr};
        use ze_scene_core::Framing2D;

        let mut world = World::new();
        let entity = world.spawn((Framing2D::default(),));
        let id = crate::contract::object_id_of(entity);

        let mut forest = XcClockForest::new();
        let mut table = BindingTable::new();
        // 同一 clock、同一字段：两条恒定值绑定，后者必须覆盖前者。
        let clock = forest.insert(ClockNode::new(None)).expect("插入节点");
        for value in [10.0f64, 20.0f64] {
            table.bind_expr_gated(
                clock,
                CompiledExpr::compile(&Expr::pure(value)).expect("可编译"),
                Arc::new(Camera2DFraming { id }),
                None,
                None,
                crate::animation::ResetBehavior::Hold,
            );
        }

        let mut resources = Resources::new();
        resources.insert(TimeState::at(0.5));
        resources.insert(forest);
        resources.insert(table);

        let mut sched = Scheduler::new(world, resources);
        sched.add_system(Phase::SceneSync, BindingApplySystem);
        sched.init_resources();
        sched.run_frame(0.0);

        assert_eq!(sched.world().get::<&Framing2D>(entity).unwrap().scale, 20.0);
    }

    /// **T1 验收 5**：表达式驱动 `Transform.position` —— 窗口内插值、窗口外冻结。
    #[test]
    fn expression_drives_transform_position_with_clamp() {
        use crate::animation::target::{TransformChannel, TransformChannelValue};
        use std::sync::Arc;
        use ze_comb::{ChannelRef, CompiledExpr, Expr, OpRef};
        use ze_scene_core::Transform;

        let mut world = World::new();
        let entity = world.spawn((Transform::default(),));
        let id = crate::contract::object_id_of(entity);

        // x = 2 · clamp(t / 2, 0, 1)（旧 `Keyframes (0,0) → (2,0)` 的线性段 + 两端冻结）。
        let math = |op: &str, args: [Expr; 3]| Expr::call(OpRef::new("zela.math", op), args);
        let progress = math(
            "clamp",
            [
                Expr::call(
                    OpRef::new("zela.math", "scale"),
                    [
                        Expr::pure(0.5),
                        Expr::channel(ChannelRef::builtin("time")),
                    ],
                ),
                Expr::pure(0.0),
                Expr::pure(1.0),
            ],
        );
        let root = Expr::call(
            OpRef::new("zela.math", "vec2"),
            [
                Expr::call(OpRef::new("zela.math", "scale"), [Expr::pure(2.0), progress]),
                Expr::pure(0.0),
            ],
        );

        let mut forest = XcClockForest::new();
        let mut table = BindingTable::new();
        let clock = forest.insert(ClockNode::new(None)).expect("插入节点");
        table.bind_expr_gated(
            clock,
            CompiledExpr::compile(&root).expect("可编译"),
            Arc::new(TransformChannelValue {
                channel: TransformChannel::Position,
                id,
            }),
            None,
            None,
            crate::animation::ResetBehavior::Hold,
        );

        let mut resources = Resources::new();
        resources.insert(TimeState::at(0.0));
        resources.insert(forest);
        resources.insert(table);

        let mut sched = Scheduler::new(world, resources);
        sched.add_system(Phase::SceneSync, BindingApplySystem);
        sched.init_resources();

        let x_at = |sched: &mut Scheduler, t: f32| -> f32 {
            sched
                .resources_mut()
                .get_mut::<TimeState>()
                .unwrap()
                .elapsed = t;
            sched.run_frame(0.0);
            sched.world().get::<&Transform>(entity).unwrap().position.x
        };
        assert_eq!(x_at(&mut sched, 0.0), 0.0);
        assert!((x_at(&mut sched, 1.0) - 1.0).abs() < 1e-4, "段内线性插值");
        assert!((x_at(&mut sched, 2.0) - 2.0).abs() < 1e-4, "段末到终值");
        assert!(
            (x_at(&mut sched, 5.0) - 2.0).abs() < 1e-6,
            "段外冻结（Hold）"
        );
    }

    /// **改动 1 的行为等价证据**：同一场景里八个 target 各一条绑定，`BindingApplySystem`
    /// 跑一帧后各组件值 = 曲线在 `t = 0.5` 的精确取值（`Keyframes` 线性中点）。
    ///
    /// 这钉住的是"apply 改成契约适配器后，写入落点与字段逐位不变"。
    #[test]
    fn all_eight_targets_write_expected_values_in_one_frame() {
        use crate::animation::target::{
            Camera2DFraming, CameraEye, MorphT, OpacityValue, Phase as PhaseTarget, RevealRange,
            TransformChannel, TransformChannelValue,
        };
        use crate::scene::components::{MorphShape, PhaseAnim, Reveal};
        use std::sync::Arc;
        use ze_scene_core::{Camera3D, Framing2D, Projection, Transform};
        use ze_render_ir::Opacity;
        use ze_time::{AnimValue, Interp};

        fn kf(from: AnimValue, to: AnimValue) -> Curve {
            Curve::Keyframes {
                keys: vec![(0.0, from), (1.0, to)],
                interpolation: Interp::Linear,
            }
        }

        let mut world = World::new();
        let circle = world.spawn((Circle::new((0.0, 0.0), 1.0),));
        let phase = world.spawn((PhaseAnim::new(0.0, 1.0),));
        let camera3d = world.spawn((Camera3D::new(
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::Y,
            Projection::perspective(1.0, 1.0, 0.1, 100.0),
        ),));
        let morph = world.spawn((MorphShape::new(
            ze_notion::Path2D::new(),
            ze_notion::Path2D::new(),
        ),));
        let camera2d = world.spawn((Framing2D::default(),));
        let reveal = world.spawn((Reveal::new(ze_notion::Path2D::new()),));
        let opacity = world.spawn((Opacity::new(1.0),));
        let transform = world.spawn((Transform::default(),));

        let mut forest = XcClockForest::new();
        let root = forest.insert(ClockNode::new(None)).unwrap();
        let mut table = BindingTable::new();
        let mut bind = |curve: Curve, target: Arc<dyn crate::animation::Target>| {
            table.add(Binding {
                clock: root,
                driver: BindingDriver::Curve(curve),
                target,
                predicate: None,
                active: None,
                reset: crate::animation::ResetBehavior::Hold,
                initial: None,
                seek_kind: SeekKind::O1,
            });
        };
        bind(
            kf(AnimValue::Scalar(0.0), AnimValue::Scalar(1.0)),
            Arc::new(CircleRadius {
                id: crate::contract::object_id_of(circle),
            }),
        );
        bind(
            kf(AnimValue::Scalar(0.0), AnimValue::Scalar(2.0)),
            Arc::new(PhaseTarget {
                id: crate::contract::object_id_of(phase),
            }),
        );
        bind(
            kf(
                AnimValue::Vec3(Vec3::ZERO),
                AnimValue::Vec3(Vec3::new(1.0, 2.0, 3.0)),
            ),
            Arc::new(CameraEye {
                id: crate::contract::object_id_of(camera3d),
            }),
        );
        bind(
            kf(AnimValue::Scalar(0.0), AnimValue::Scalar(1.0)),
            Arc::new(MorphT {
                id: crate::contract::object_id_of(morph),
            }),
        );
        // 相机取景：scale（Scalar）与 center（Vec2）各一条绑定。
        bind(
            kf(AnimValue::Scalar(40.0), AnimValue::Scalar(80.0)),
            Arc::new(Camera2DFraming {
                id: crate::contract::object_id_of(camera2d),
            }),
        );
        bind(
            kf(
                AnimValue::Vec2(vec2(0.0, 0.0)),
                AnimValue::Vec2(vec2(2.0, 4.0)),
            ),
            Arc::new(Camera2DFraming {
                id: crate::contract::object_id_of(camera2d),
            }),
        );
        // 书写区间：end（Scalar）与 (start, end)（Vec2）各一条。
        bind(
            kf(AnimValue::Scalar(0.0), AnimValue::Scalar(1.0)),
            Arc::new(RevealRange {
                id: crate::contract::object_id_of(reveal),
            }),
        );
        bind(
            kf(
                AnimValue::Vec2(vec2(0.0, 0.0)),
                AnimValue::Vec2(vec2(0.5, 1.0)),
            ),
            Arc::new(RevealRange {
                id: crate::contract::object_id_of(reveal),
            }),
        );
        bind(
            kf(AnimValue::Scalar(1.0), AnimValue::Scalar(0.0)),
            Arc::new(OpacityValue {
                id: crate::contract::object_id_of(opacity),
            }),
        );
        bind(
            kf(
                AnimValue::Vec2(vec2(0.0, 0.0)),
                AnimValue::Vec2(vec2(2.0, 4.0)),
            ),
            Arc::new(TransformChannelValue {
                channel: TransformChannel::Position,
                id: crate::contract::object_id_of(transform),
            }),
        );

        let mut resources = Resources::new();
        resources.insert(TimeState::at(0.5));
        resources.insert(forest);
        resources.insert(table);

        let mut sched = Scheduler::new(world, resources);
        sched.add_system(Phase::SceneSync, BindingApplySystem);
        sched.init_resources();
        sched.run_frame(0.0);

        let w = sched.world();
        assert_eq!(w.get::<&Circle>(circle).unwrap().radius, 0.5);
        assert_eq!(w.get::<&PhaseAnim>(phase).unwrap().phase, 1.0);
        assert_eq!(
            w.get::<&Camera3D>(camera3d).unwrap().eye,
            Vec3::new(0.5, 1.0, 1.5)
        );
        assert_eq!(w.get::<&MorphShape>(morph).unwrap().t, 0.5);
        {
            let f = w.get::<&Framing2D>(camera2d).unwrap();
            assert_eq!(f.scale, 60.0);
            assert_eq!(f.center, vec2(1.0, 2.0));
        }
        {
            let r = w.get::<&Reveal>(reveal).unwrap();
            assert_eq!((r.start, r.end), (0.25, 0.5));
        }
        assert_eq!(w.get::<&Opacity>(opacity).unwrap().0, 0.5);
        {
            let t = w.get::<&Transform>(transform).unwrap();
            assert_eq!(t.position, Vec3::new(1.0, 2.0, 0.0));
            assert_eq!(t.scale, Vec3::ONE);
        }
    }

    /// **G 批集成**：写者（时钟起点靠前）写 `Circle.radius`，读者表达式绑定
    /// （时钟起点靠后）`sin(read(Circle.radius))` 写 `PhaseAnim.phase`
    /// ⇒ 一帧后 phase == 半径**本帧值**的 sin（读端看到写者的新值，不是旧值）。
    ///
    /// 注：集成测试绕过 `commit`（直接构造绑定表），`read` 的 object 用打包值；
    /// 正常路径由 `SceneScript::commit` 的 `rewrite_read_objects` 重写。
    #[test]
    fn expression_reads_component_written_earlier_in_frame() {
        use crate::animation::target::{FieldTarget, target_entry};
        use std::sync::Arc;
        use ze_comb::{CompiledExpr, Expr};
        use ze_scene_core::components::PhaseAnim;
        use ze_time::AnimValue;

        let mut world = World::new();
        let circle = world.spawn((Circle::new(vec2(0.0, 0.0), 1.0),));
        let phase_obj = world.spawn((PhaseAnim::new(0.0, 1.0),));
        let circle_id = crate::contract::object_id_of(circle);
        let phase_id = crate::contract::object_id_of(phase_obj);

        let mut forest = XcClockForest::new();
        // 写者起点 0；读者起点 = 写者起点 + 1.0（帧内定序：写者先于读者）。
        let writer = forest.insert(ClockNode::new(None)).unwrap();
        let mut reader_node = ClockNode::new(Some(writer));
        reader_node.local_offset = 1.0;
        let reader = forest.insert(reader_node).unwrap();

        let mut table = BindingTable::new();
        table.add(Binding {
            clock: writer,
            driver: BindingDriver::Curve(Curve::Const(AnimValue::Scalar(0.5))),
            target: Arc::new(FieldTarget {
                id: circle_id,
                spec: target_entry("Circle.radius").unwrap().spec,
            }),
            predicate: None,
            active: None,
            reset: crate::animation::ResetBehavior::Hold,
            initial: None,
            seek_kind: SeekKind::O1,
        });
        let read_expr = Expr::read(
            ze_comb::ChannelRef::new("zela.scene", "Circle.radius")
                .with_type(ze_comb::ChannelType::Number),
            circle_id.to_packed(),
        );
        let program = CompiledExpr::compile(&Expr::call(
            ze_comb::OpRef::new("zela.math", "sin"),
            [read_expr],
        ))
        .expect("Read 是叶子，必然可编译");
        table.add(Binding {
            clock: reader,
            driver: BindingDriver::Expr(program.into()),
            target: Arc::new(FieldTarget {
                id: phase_id,
                spec: target_entry("PhaseAnim.phase").unwrap().spec,
            }),
            predicate: None,
            active: None,
            reset: crate::animation::ResetBehavior::Hold,
            initial: None,
            seek_kind: SeekKind::O1,
        });

        let mut resources = Resources::new();
        resources.insert(TimeState::at(1.5));
        resources.insert(forest);
        resources.insert(table);

        let mut sched = Scheduler::new(world, resources);
        sched.add_system(Phase::SceneSync, BindingApplySystem);
        sched.init_resources();
        sched.run_frame(0.0);

        let w = sched.world();
        // 写者（start=0）先应用：t=1.5 ⇒ lt=1.5 ⇒ radius=0.5。
        assert!((w.get::<&Circle>(circle).unwrap().radius - 0.5).abs() < 1e-6);
        // 读者（start=1）后应用：lt=0.5 ⇒ sin(0.5)，读的是本帧已写入的 0.5。
        let expected = (0.5f64).sin() as f32;
        assert!(
            (w.get::<&PhaseAnim>(phase_obj).unwrap().phase - expected).abs() < 1e-5,
            "phase 应等于 sin(0.5)"
        );
    }
}
