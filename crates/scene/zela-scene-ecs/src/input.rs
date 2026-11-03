// input.rs — UI 输入事件的场景侧消费者（S0：从 `zela-egui` 归位）。
//
// 面板 chrome（按钮 / 滑块）仍在 `zela-egui` 的 host 里绘制；它们只把交互写成
// `SceneEvent`（按 `WidgetId` 标识）放进 `InputEvents`。本模块的两个 System
// 只消费这些纯数据事件并改场景状态 —— 可 headless 测试，不含任何 egui 类型。
//
// Phase 分配：LogicUpdate（与 `DirectorAdvanceSystem` 的 Input 相位分开）。

use zela_schedule::context::ExecutorContext;
use zela_schedule::system::System;

use crate::Curve;
use crate::animation::BindingTable;
use crate::animation::params::{self, widget_index};
use crate::director_state::DirectorState;
use crate::domain::{InputEvents, SceneEvent, WidgetId, ids};
use crate::interact::{InteractSet, InteractState};
use ze_property::{ParameterRegistry, PropertyValue};

// ─── TimeControlSystem（播放/暂停、速度、seek）────────────────────────────

/// 时间控制事件消费（LogicUpdate 阶段）。
pub struct TimeControlSystem;
impl System for TimeControlSystem {
    type Reads = (InputEvents,);
    type Writes = (DirectorState,);

    fn run(&mut self, ctx: &mut ExecutorContext<'_>) {
        let events: Vec<SceneEvent> = match ctx.get::<InputEvents>() {
            Some(e) => e.iter().cloned().collect(),
            None => return,
        };
        if events.is_empty() {
            return;
        }

        // 写者可读自身：clone 出本地副本应用事件，结束后写回。
        let mut dir = ctx.get::<DirectorState>().cloned().unwrap_or_default();
        for event in events {
            match event {
                SceneEvent::Button { id } if id == ids::PLAY_PAUSE => dir.director.toggle(),
                // 重新播放：拨回 0 并恢复播放。全部绑定是 O1（纯函数、无记忆）
                // ⇒ 场景状态由 t 唯一决定，回到 0 即回到初始画面（无需重建场景）。
                SceneEvent::Button { id } if id == ids::REPLAY => {
                    // 重播 = 回到起点：帧号与 t 一起归零（帧号是一等通道）。
                    dir.director.rewind();
                    dir.director.resume();
                }
                SceneEvent::Slider { id, value } if id == ids::SPEED => {
                    dir.director.set_speed(value as f64);
                }
                SceneEvent::Slider { id, value } if id == ids::SEEK => {
                    dir.director.seek(value as f64);
                }
                _ => {}
            }
        }
        if let Some(d) = ctx.get_mut::<DirectorState>() {
            *d = dir;
        }
    }
}

// ─── ParamPanelSystem（滑块 → BindingTable）──────────────────────────────

/// 参数面板事件消费（LogicUpdate 阶段）。
///
/// **CA-c**：回写键从"手写 WidgetId + 曲线类型"泛化为**参数地址**：
/// 注册表按地址升序分配 `WidgetId`（[`params::widget_id`]），本 System 把 id 换回
/// 地址后统一走 [`params::apply`]。既有 13 个手写 id 的分支保留（渐进式，
/// `SceneEvent` 契约不变）。
///
/// 写回只靠 `WidgetId`（面板里每个控件互不重复）；旧分支的语义与 CA 前一致。
pub struct ParamPanelSystem;
impl System for ParamPanelSystem {
    type Reads = (InputEvents, ParameterRegistry, InteractSet);
    type Writes = (BindingTable, ParameterRegistry, InteractState);

    fn run(&mut self, ctx: &mut ExecutorContext<'_>) {
        let events: Vec<SceneEvent> = match ctx.get::<InputEvents>() {
            Some(e) => e.iter().cloned().collect(),
            None => return,
        };
        if events.is_empty() {
            return;
        }

        // 克隆出 BindingTable / 注册表 / 路由状态修改，避免同时持有 &mut ctx 与资源借用。
        let mut table = ctx.get::<BindingTable>().cloned().unwrap_or_default();
        let mut registry = ctx.get::<ParameterRegistry>().cloned().unwrap_or_default();
        let mut state = ctx.get::<InteractState>().cloned().unwrap_or_default();

        // **CE-e**：场景内参数面板（命中 → 路由 → 回写）。与下面的控件 id 分支
        // 共用同一对 `(BindingTable, ParameterRegistry)` —— 同一 System 内不会互相抢写。
        let knob_moves = match ctx.get::<InteractSet>() {
            Some(set) if !set.is_empty() => crate::interact::apply_viewport_events(
                &events,
                set,
                &mut state,
                &mut table,
                &mut registry,
            ),
            _ => Vec::new(),
        };

        // 只关心 Slider 事件。
        let sliders: Vec<(WidgetId, f32)> = events
            .iter()
            .filter_map(|e| match e {
                SceneEvent::Slider { id, value } => Some((*id, *value)),
                _ => None,
            })
            .collect();

        // 旧分支：手写 id（阶段 1 保留，场景侧代码不必立刻改）。
        for binding in table.bindings.iter_mut() {
            for (id, value) in &sliders {
                // 表达式绑定没有可调曲线字段（滑块只服务于曲线绑定）。
                let Some(curve) = binding.driver.as_curve_mut() else {
                    continue;
                };
                match (*id, curve) {
                    (ids::CIRCLE_BASE, Curve::Sin { base, .. }) => *base = *value,
                    (ids::CIRCLE_AMP, Curve::Sin { amp, .. }) => *amp = *value,
                    (ids::CIRCLE_OMEGA, Curve::Sin { omega, .. }) => *omega = *value,
                    (ids::PHASE_OMEGA, Curve::Phase { omega }) => *omega = *value,
                    (ids::ORBIT_RADIUS, Curve::Orbit { radius, .. }) => *radius = *value,
                    (ids::ORBIT_SPEED, Curve::Orbit { speed, .. }) => *speed = *value,
                    (ids::ORBIT_ANGLE, Curve::Orbit { initial_angle, .. }) => {
                        *initial_angle = *value
                    }
                    _ => {}
                }
            }
        }

        // 新分支：注册表分配的动态 id → 参数地址 → 语义化回写。
        for (id, value) in &sliders {
            let Some(index) = widget_index(*id) else {
                continue;
            };
            let Some(entry) = registry.at(index) else {
                continue;
            };
            let address = entry.address.to_string();
            match params::apply(&mut table, &address, *value) {
                // 生效值同步回注册表（下一帧滑块才停在新位置）。
                Ok(()) => {
                    let _ = registry.set_value(&address, PropertyValue::Scalar(*value));
                }
                // R6：不稳定 / 绑定让位 / 种类不符 ⇒ 显式拒绝，注册表保持旧值。
                Err(error) => {
                    eprintln!("[zela] 参数 `{address}` 写入被拒绝：{error}");
                }
            }
        }

        // 写回
        if let Some(t) = ctx.get_mut::<BindingTable>() {
            *t = table;
        }
        if let Some(r) = ctx.get_mut::<ParameterRegistry>() {
            *r = registry;
        }
        if let Some(s) = ctx.get_mut::<InteractState>() {
            *s = state;
        }
        // 滑块圆心跟随取值（世界写回放在最后，避免与上面的资源借用交叉）。
        for (knob, x) in knob_moves {
            if let Some(entity) = crate::contract::entity_of(knob) {
                if let Ok(mut circle) = ctx.world.get::<&mut ze_scene_core::Circle>(entity) {
                    circle.0.center.x = x;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::ClockId;
    use crate::animation::{Binding, BindingDriver, CameraEye, CircleRadius, SeekKind};
    use crate::contract::ObjectId;
    use zela_schedule::resource::Resources;
    use zela_schedule::scheduler::{Phase, Scheduler};

    fn run_with_events(events: Vec<SceneEvent>) -> DirectorState {
        let mut resources = Resources::new();
        resources.insert(DirectorState::new());
        resources.insert(InputEvents::from_events(events));

        let mut sched = Scheduler::new(hecs::World::new(), resources);
        sched.add_system(Phase::LogicUpdate, TimeControlSystem);
        sched.init_resources();
        sched.run_frame(0.016);
        sched.resources().get::<DirectorState>().cloned().unwrap()
    }

    /// headless：Button(PLAY_PAUSE) 切换播放状态。
    #[test]
    fn play_pause_button_toggles_director() {
        let before = DirectorState::new();
        let after = run_with_events(vec![SceneEvent::Button {
            id: ids::PLAY_PAUSE,
        }]);
        assert_ne!(after.director.playing, before.director.playing);
    }

    /// headless：Button(REPLAY) 把时间拨回 0 并恢复播放（无论当前是否暂停）。
    #[test]
    fn replay_button_seeks_to_zero_and_resumes() {
        // 当前状态：已跑到 3.7s 且处于暂停 —— 重播应同时"归零"与"开始播放"。
        let mut resources = Resources::new();
        let mut director = DirectorState::new();
        director.director.seek(3.7);
        director.director.pause();
        resources.insert(director);
        resources.insert(InputEvents::from_events(vec![SceneEvent::Button {
            id: ids::REPLAY,
        }]));

        let mut sched = Scheduler::new(hecs::World::new(), resources);
        sched.add_system(Phase::LogicUpdate, TimeControlSystem);
        sched.init_resources();
        sched.run_frame(0.016);

        let d = sched.resources().get::<DirectorState>().unwrap();
        assert!(
            d.director.global_t.abs() < 1e-9,
            "应拨回 0，实际 {}",
            d.director.global_t
        );
        assert!(d.director.playing, "重播后应处于播放态");
    }

    /// headless：Slider(SPEED) / Slider(SEEK) 分别作用于 speed / global_t。
    #[test]
    fn slider_events_drive_speed_and_seek() {
        let after = run_with_events(vec![
            SceneEvent::Slider {
                id: ids::SPEED,
                value: 2.0,
            },
            SceneEvent::Slider {
                id: ids::SEEK,
                value: 5.5,
            },
        ]);
        assert!((after.director.speed - 2.0).abs() < 1e-5);
        assert!((after.director.global_t - 5.5).abs() < 1e-5);
    }

    /// 无关事件（其他控件 ID）不影响 DirectorState。
    #[test]
    fn unrelated_events_are_ignored() {
        let before = DirectorState::new();
        let after = run_with_events(vec![
            SceneEvent::Button { id: ids::SHOW_GRID },
            SceneEvent::Slider {
                id: ids::CIRCLE_BASE,
                value: 9.9,
            },
        ]);
        assert_eq!(after.director.playing, before.director.playing);
        assert_eq!(after.director.speed, before.director.speed);
        assert_eq!(after.director.global_t, before.director.global_t);
    }

    /// headless：Slider 事件按 WidgetId 精确写入对应绑定字段，顺序无关。
    #[test]
    fn slider_events_patch_binding_fields_by_widget_id() {
        let mut table = BindingTable::new();
        table.add(Binding {
            clock: ClockId(0),
            scope: Default::default(),
            priority: 0,
            driver: BindingDriver::Curve(Curve::Sin {
                base: 1.0,
                amp: 0.4,
                omega: 2.0,
            }),
            target: Arc::new(CircleRadius {
                id: ObjectId::DANGLING,
            }),
            predicate: None,
            active: None,
            reset: crate::animation::ResetBehavior::Hold,
            initial: None,
            state_plans: Vec::new(),
            seek_kind: SeekKind::O1,
        });
        table.add(Binding {
            clock: ClockId(1),
            scope: Default::default(),
            priority: 0,
            driver: BindingDriver::Curve(Curve::Orbit {
                radius: 5.0,
                axis: ze_notion::Vec3::Y,
                speed: 0.5,
                initial_angle: 0.0,
            }),
            target: Arc::new(CameraEye {
                id: ObjectId::DANGLING,
            }),
            predicate: None,
            active: None,
            reset: crate::animation::ResetBehavior::Hold,
            initial: None,
            state_plans: Vec::new(),
            seek_kind: SeekKind::O1,
        });

        let mut resources = Resources::new();
        resources.insert(table);
        resources.insert(InputEvents::from_events(vec![
            SceneEvent::Slider {
                id: ids::CIRCLE_AMP,
                value: 0.8,
            },
            SceneEvent::Slider {
                id: ids::ORBIT_RADIUS,
                value: 12.0,
            },
        ]));

        let mut sched = Scheduler::new(hecs::World::new(), resources);
        sched.add_system(Phase::LogicUpdate, ParamPanelSystem);
        sched.init_resources();
        sched.run_frame(0.016);

        let table = sched.resources().get::<BindingTable>().unwrap();
        let mut saw_sin = false;
        let mut saw_orbit = false;
        for binding in &table.bindings {
            // T0-② 后按**曲线类型**分辨绑定（target 类别清单已删除）。
            let Some(curve) = binding.driver.as_curve() else {
                continue;
            };
            match curve {
                Curve::Sin { amp, base, omega } => {
                    saw_sin = true;
                    assert!((amp - 0.8).abs() < 1e-5, "amp = {amp}");
                    assert!((base - 1.0).abs() < 1e-5, "base 不应被无关事件改动");
                    assert!((omega - 2.0).abs() < 1e-5);
                }
                Curve::Orbit { radius, .. } => {
                    saw_orbit = true;
                    assert!((radius - 12.0).abs() < 1e-5, "radius = {radius}");
                }
                _ => {}
            }
        }
        assert!(saw_sin && saw_orbit, "两条绑定都应被核对过");
    }

    /// **CA-c**：注册表分配的动态 `WidgetId` 按地址回写，并把生效值同步回注册表。
    #[test]
    fn registry_widget_id_routes_slider_to_its_address() {
        let mut table = BindingTable::new();
        table.add(Binding {
            clock: ClockId(0),
            scope: Default::default(),
            priority: 0,
            driver: BindingDriver::Curve(Curve::Sin {
                base: 0.0,
                amp: 1.0,
                omega: 2.0,
            }),
            target: Arc::new(CircleRadius {
                id: ObjectId {
                    index: 3,
                    generation: 1,
                },
            }),
            predicate: None,
            active: None,
            reset: crate::animation::ResetBehavior::Hold,
            initial: None,
            state_plans: Vec::new(),
            seek_kind: SeekKind::O1,
        });
        let registry = crate::animation::params::build_registry(&table);
        let (index, address) = registry
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.address.ends_with(":amp"))
            .map(|(index, entry)| (index, entry.address.to_string()))
            .expect("Sin 绑定应登记 amp");

        let mut resources = Resources::new();
        resources.insert(table);
        resources.insert(registry.clone());
        resources.insert(InputEvents::from_events(vec![SceneEvent::Slider {
            id: crate::animation::params::widget_id(index),
            value: 0.7,
        }]));

        let mut sched = Scheduler::new(hecs::World::new(), resources);
        sched.add_system(Phase::LogicUpdate, ParamPanelSystem);
        sched.init_resources();
        sched.run_frame(0.016);

        let table = sched.resources().get::<BindingTable>().unwrap();
        let curve = table.bindings[0].driver.as_curve().unwrap();
        assert!(matches!(curve, Curve::Sin { amp, .. } if (*amp - 0.7).abs() < 1e-6));
        let registry = sched.resources().get::<ParameterRegistry>().unwrap();
        assert!(matches!(registry.value(&address), Some(PropertyValue::Scalar(v)) if (v - 0.7).abs() < 1e-6));
    }
}
