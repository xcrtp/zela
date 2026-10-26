// camera_drive.rs — 「**谁驱动哪台相机**」的开关（K3 + 补：2D 自由相机）
//
// # 问题（同一条实测结论，两台相机都中）
//
// `BindingApplySystem` 遍历绑定表**每帧无条件写入**目标字段（无 active 判断），而
// `ClockNode.mode = Hold` 把局部时间 clamp 进 `[lo, hi]` ⇒ **时间轴的任何时刻都有值**。
// 于是：
// - 3D 相机：`Curve::Orbit` → `CameraEye` 每帧写 `eye`；
// - 2D 相机：分镜的 `Camera2DFraming` → `center` / `scale` 每帧写取景。
// 用户交互（Orbit/Dolly/Look/Move 或 Pan/Zoom）在 `Input` 相位写入，`SceneSync` 相位的
// `BindingApplySystem` **同帧**就把它抹掉 ⇒ 表现是"拖了没反应"。
//
// # 处置：模式互斥 + **显式让位**
//
// 让位 = 把对应绑定**移出 `BindingTable`**（切回时放回）。这是与既有机制一致的唯一做法
// （绑定是每帧重算的纯函数，"交互优先"在求值链上没有落点）。
//
// | `UiConfig` | 2D 相机 | 3D 相机 |
// |---|---|---|
// | `free_2d_camera = false`（默认） | 分镜绑定**在位** + 模式 `Path` | — |
// | `free_2d_camera = true` | 分镜绑定**移出** + 模式 `PanZoom` | — |
// | `viewport_camera = TwoD`（默认） | — | 3D 相机的全部动画绑定**在位** + 模式 `Path` |
// | `viewport_camera = ThreeDOrbit/Fps` | — | 3D 相机的全部动画绑定**移出** + 模式 `Orbit` / `Fps` |
//
// 「3D 相机的全部动画绑定」= 自动轨道 + 脚本自己用 `Target.CameraEye` 建的绑定。
// 后者在装配期还看不到（`Camera3DHandle` 由宿主登记）⇒ 按目标 id **动态**收集。
//
// # 为什么这两件事在**同一个 System** 里
//
// `Scheduler::add_system` 在 debug 下做静态借用冲突检查：**同相位内不能有两个
// `BindingTable` 写者**（3D 让位最初就是踩这个坑崩的，见 ARCHITECTURE「K 线实测坑」）。
// 两个相机要让位的是同一张表 ⇒ 必须由**一个** System 原子地完成两台相机的让位判定。
//
// A4：本 System 是**场景 system**，从 `zela-app` 归位到此；app 只负责注册。

use ze_scene_core::{Camera2DHandle, Camera3DHandle, FpsView};
use zela_schedule::context::ExecutorContext;
use zela_schedule::system::System;

use crate::animation::{Binding, BindingTable};
use crate::contract::{ObjectId, entity_of};
use crate::domain::{CameraMode, UiConfig, ViewportCamera};

/// 按 `UiConfig` 决定两台相机"由分镜驱动还是由交互驱动"，并做绑定让位/复位。
pub struct CameraDriveSystem {
    /// 分镜里指向 2D 相机（`Camera2DFraming`）的绑定（`center` + `scale` 各一条）。
    cam2d: Vec<Binding>,
    /// 自动轨道绑定（`Curve::Orbit` → `CameraEye`）。
    orbit: Binding,
    /// 分镜的 2D 相机绑定当前是否在位。
    cam2d_driven: bool,
    /// 3D 相机的动画绑定当前是否在位（`want_orbit`；含自动轨道与脚本的 `CameraEye`）。
    orbit_active: bool,
    /// 让位时**从表里移出**的 3D 相机绑定（切回 `TwoD` 时原样放回）。
    ///
    /// 为什么要动态收集而不是只记 `orbit`：脚本作品（如 `capability_gaps`）自己
    /// 用 `Target.CameraEye` 驱动相机，而装配期 `Camera3DHandle` 还没登记
    /// （`install_camera3d` 在宿主侧才做）⇒ 这类绑定不在构造参数里。只让位
    /// "自动轨道"一条的话，切到轨道/第一人称后相机仍被脚本每帧写回
    /// ⇒ 用户看到的是"3D 相机控制完全没反应"。
    suspended3d: Vec<Binding>,
}

impl CameraDriveSystem {
    /// `cam2d`：分镜的 2D 相机绑定（装配时已在表内）；
    /// `orbit`：自动轨道绑定（装配时已在表内）。
    pub fn new(cam2d: Vec<Binding>, orbit: Binding) -> Self {
        Self {
            cam2d,
            orbit,
            cam2d_driven: true,
            orbit_active: true,
            suspended3d: Vec::new(),
        }
    }
}

impl System for CameraDriveSystem {
    type Reads = (UiConfig, Camera2DHandle, Camera3DHandle);
    type Writes = (BindingTable,);

    fn run(&mut self, ctx: &mut ExecutorContext<'_>) {
        let cfg = ctx.get::<UiConfig>().cloned().unwrap_or_default();
        let cam2d = ctx.get::<Camera2DHandle>().copied().unwrap_or_default();
        let cam3d = ctx.get::<Camera3DHandle>().copied().unwrap_or_default();
        // 想要的状态（`true` = 绑定在位 / 由动画驱动）。
        let want_cam2d_driven = !cfg.free_2d_camera;
        let want_orbit = cfg.viewport_camera == ViewportCamera::TwoD;
        // 相机模式（组件）：与"谁驱动"一致 —— 动画驱动 ⇒ `Path`（交互被忽略），
        // 自由 ⇒ `PanZoom`（2D）/ `Orbit` / `Fps`（3D）。
        let mode_2d = if want_cam2d_driven {
            CameraMode::PATH
        } else {
            CameraMode::PAN_ZOOM
        };
        let mode_3d = match cfg.viewport_camera {
            ViewportCamera::TwoD => CameraMode::PATH,
            ViewportCamera::ThreeDOrbit => CameraMode::ORBIT,
            ViewportCamera::ThreeDFps => CameraMode::FPS,
        };

        // 1) 相机模式写进**场景对象**（组件即状态；仅在变化时写，避免每帧 churn）。
        write_mode(ctx, cam2d.0, mode_2d);
        write_mode(ctx, cam3d.0, mode_3d);

        // 2) 绑定让位 / 复位（幂等：状态未变则不动表）。
        let cam2d_changed = want_cam2d_driven != self.cam2d_driven;
        let orbit_changed = want_orbit != self.orbit_active;
        if cam2d_changed || orbit_changed {
            let mut table = ctx.get::<BindingTable>().cloned().unwrap_or_default();
            if cam2d_changed {
                if want_cam2d_driven {
                    for b in &self.cam2d {
                        table.add(b.clone());
                    }
                } else {
                    let clocks: Vec<_> = self.cam2d.iter().map(|b| b.clock).collect();
                    table.bindings.retain(|b| !clocks.contains(&b.clock));
                }
                self.cam2d_driven = want_cam2d_driven;
            }
            if orbit_changed {
                if want_orbit {
                    // 原样放回（自动轨道 + 脚本自己写的 CameraEye 绑定）。
                    for binding in self.suspended3d.drain(..) {
                        table.add(binding);
                    }
                } else {
                    // 移出**所有**写这台 3D 相机的绑定 —— 不只是装配期的自动轨道。
                    let target = cam3d.0;
                    let mut taken = Vec::new();
                    table.bindings.retain(|binding| {
                        if binding.target.target_id() == target {
                            taken.push(binding.clone());
                            false
                        } else {
                            true
                        }
                    });
                    // 自动轨道的目标可能是悬垂 id（无 3D 相机，或脚本相机在宿主侧才
                    // 登记）⇒ 再按 clock 兜底移出一次，保持旧行为。
                    if !taken.iter().any(|binding| binding.clock == self.orbit.clock) {
                        table.bindings.retain(|binding| binding.clock != self.orbit.clock);
                        taken.push(self.orbit.clone());
                    }
                    self.suspended3d = taken;
                }
                self.orbit_active = want_orbit;
            }
            if let Some(t) = ctx.get_mut::<BindingTable>() {
                *t = table;
            }
        }
    }
}

/// 把 `mode` 写到 `id` 指向的相机对象上（对象不存在则静默跳过）。
///
/// 只在**变化时**写组件：本层的相机模式默认是 `PanZoom`（未挂组件时的取值），
/// 但这里显式落组件，让"谁驱动"这件事在场景数据里可查（而不是靠缺省）。
///
/// **模式切换同时作废 `FpsView`**：`FpsView` 是 FPS **会话内的累积朝向**，而另一个
/// 模式（Orbit / Path）会直接改 `eye` / `target`。留着旧值 ⇒ 下次进 FPS 时第一下
/// 操作会把相机**拉回上一次的朝向**（实测症状：切回第一人称后按 W 画面瞬间跳转）。
/// 作废后由 `CameraInputSystem` 用相机当前朝向重新初始化（"接管不跳变"）。
fn write_mode(ctx: &mut ExecutorContext<'_>, id: ObjectId, mode: CameraMode) {
    let Some(entity) = entity_of(id) else {
        return;
    };
    let changed = ctx
        .world
        .get::<&CameraMode>(entity)
        .map(|m| *m != mode)
        .unwrap_or(true);
    if changed {
        let _ = ctx.world.insert_one(entity, mode);
        let _ = ctx.world.remove_one::<FpsView>(entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use ze_scene_core::{Camera3D, Framing2D, Projection};
    use zela_schedule::resource::Resources;
    use zela_schedule::scheduler::{Phase, Scheduler};

    use crate::animation::{BindingDriver, Camera2DFraming, CameraEye, SeekKind};
    use crate::{ClockForest, ClockId, ClockNode, Curve};

    /// 测试脚手架：1 台 2D 相机 + 1 台 3D 相机 + 分镜 2D 绑定×2 + 自动轨道×1。
    struct Fixture {
        sched: Scheduler,
        cam2d: hecs::Entity,
        cam3d: hecs::Entity,
    }

    impl Fixture {
        fn new(cfg: UiConfig) -> Self {
            let mut world = hecs::World::new();
            let cam2d = world.spawn((Framing2D::default(), CameraMode::PATH));
            let cam3d = world.spawn((
                Camera3D::new(
                    ze_notion::Vec3::new(0.0, 0.0, 5.0),
                    ze_notion::Vec3::ZERO,
                    ze_notion::Vec3::Y,
                    Projection::perspective(1.0, 1.0, 0.1, 100.0),
                ),
                CameraMode::PATH,
            ));

            let mut forest = ClockForest::new();
            let c_center = forest.insert(ClockNode::new(None)).unwrap();
            let c_scale = forest.insert(ClockNode::new(None)).unwrap();
            let c_orbit = forest.insert(ClockNode::new(None)).unwrap();
            let cam2d_bindings = vec![
                Binding {
                    clock: c_center,
                    driver: BindingDriver::Curve(Curve::Keyframes {
                        keys: vec![
                            (0.0, crate::AnimValue::Vec2((0.0, 0.0).into())),
                            (1.0, crate::AnimValue::Vec2((1.0, 1.0).into())),
                        ],
                        interpolation: crate::Interp::Linear,
                    }),
                    target: Arc::new(Camera2DFraming {
                        id: crate::contract::object_id_of(cam2d),
                    }),
                    predicate: None,
                    active: None,
                    reset: crate::animation::ResetBehavior::Hold,
                    initial: None,
                    seek_kind: SeekKind::O1,
                },
                Binding {
                    clock: c_scale,
                    driver: BindingDriver::Curve(Curve::Keyframes {
                        keys: vec![
                            (0.0, crate::AnimValue::Scalar(40.0)),
                            (1.0, crate::AnimValue::Scalar(110.0)),
                        ],
                        interpolation: crate::Interp::Linear,
                    }),
                    target: Arc::new(Camera2DFraming {
                        id: crate::contract::object_id_of(cam2d),
                    }),
                    predicate: None,
                    active: None,
                    reset: crate::animation::ResetBehavior::Hold,
                    initial: None,
                    seek_kind: SeekKind::O1,
                },
            ];
            let orbit = Binding {
                clock: c_orbit,
                driver: BindingDriver::Curve(Curve::Orbit {
                    radius: 5.0,
                    axis: ze_notion::Vec3::Y,
                    speed: 0.5,
                    initial_angle: 0.0,
                }),
                target: Arc::new(CameraEye {
                    id: crate::contract::object_id_of(cam3d),
                }),
                predicate: None,
                active: None,
                reset: crate::animation::ResetBehavior::Hold,
                initial: None,
                seek_kind: SeekKind::O1,
            };

            let mut table = BindingTable::new();
            for b in &cam2d_bindings {
                table.add(b.clone());
            }
            table.add(orbit.clone());

            let mut res = Resources::new();
            res.insert(cfg);
            res.insert(Camera2DHandle(crate::contract::object_id_of(cam2d)));
            res.insert(Camera3DHandle(crate::contract::object_id_of(cam3d)));
            res.insert(table);

            let mut sched = Scheduler::new(world, res);
            sched.add_system(Phase::Input, CameraDriveSystem::new(cam2d_bindings, orbit));
            sched.init_resources();
            Self {
                sched,
                cam2d,
                cam3d,
            }
        }

        fn run(&mut self) {
            self.sched.run_frame(0.016);
        }

        fn binding_count(&self) -> usize {
            self.sched
                .resources()
                .get::<BindingTable>()
                .map(|t| t.bindings.len())
                .unwrap_or(0)
        }

        fn mode2d(&self) -> CameraMode {
            *self.sched.world().get::<&CameraMode>(self.cam2d).unwrap()
        }

        fn mode3d(&self) -> CameraMode {
            *self.sched.world().get::<&CameraMode>(self.cam3d).unwrap()
        }

        fn set_cfg(&mut self, f: impl FnOnce(&mut UiConfig)) {
            f(self.sched.resources_mut().get_mut::<UiConfig>().unwrap());
        }

        /// 表里是否还有这个 clock 的绑定。
        fn has_binding(&self, clock: ClockId) -> bool {
            self.sched
                .resources()
                .get::<BindingTable>()
                .is_some_and(|table| table.bindings.iter().any(|binding| binding.clock == clock))
        }
    }

    /// 默认（分镜驱动）：3 条绑定全在位；2D 相机 `Path`（不接受交互）。
    #[test]
    fn default_keeps_storyboard_driving_2d_camera() {
        let mut f = Fixture::new(UiConfig::default());
        f.run();
        assert_eq!(f.binding_count(), 3, "分镜 2 条 + 自动轨道 1 条");
        assert_eq!(f.mode2d(), CameraMode::Path);
        assert_eq!(f.mode3d(), CameraMode::Path);
    }

    /// **开关打开（自由 2D）**：分镜的 2D 相机绑定移出（否则每帧覆盖用户操作），
    /// 2D 相机改 `PanZoom`；3D 侧不受影响。
    #[test]
    fn free_2d_switch_releases_storyboard_camera_bindings() {
        let mut f = Fixture::new(UiConfig::default());
        f.set_cfg(|c| c.free_2d_camera = true);
        f.run();
        assert_eq!(f.binding_count(), 1, "只应剩自动轨道绑定");
        assert_eq!(
            f.mode2d(),
            CameraMode::PAN_ZOOM,
            "自由态下 2D 交互必须被接受"
        );
        assert_eq!(f.mode3d(), CameraMode::Path);
    }

    /// 切回分镜：2 条 2D 绑定**放回**且不重复（幂等）。
    #[test]
    fn switching_back_restores_storyboard_bindings_exactly_once() {
        let mut f = Fixture::new(UiConfig::default());
        f.set_cfg(|c| c.free_2d_camera = true);
        f.run();
        assert_eq!(f.binding_count(), 1);

        f.set_cfg(|c| c.free_2d_camera = false);
        f.run();
        assert_eq!(f.binding_count(), 3, "2 条分镜绑定应放回");
        assert_eq!(f.mode2d(), CameraMode::Path);

        f.run();
        f.run();
        assert_eq!(f.binding_count(), 3, "多帧不得重复添加");
    }

    /// 两个开关**互不干扰**：自由 2D + 3D 轨道交互 ⇒ 0 条绑定。
    #[test]
    fn free_2d_and_3d_orbit_are_independent() {
        let mut f = Fixture::new(UiConfig::default());
        f.set_cfg(|c| {
            c.free_2d_camera = true;
            c.viewport_camera = ViewportCamera::ThreeDOrbit;
        });
        f.run();
        assert_eq!(f.binding_count(), 0, "两个让位都生效");
        assert_eq!(f.mode2d(), CameraMode::PAN_ZOOM);
        assert_eq!(f.mode3d(), CameraMode::Orbit);
    }

    /// 3D 交互（Orbit/Fps）⇒ 自动轨道让位；2D 侧保持分镜驱动。
    #[test]
    fn three_d_interaction_suspends_auto_orbit_only() {
        let mut f = Fixture::new(UiConfig::default());
        f.set_cfg(|c| c.viewport_camera = ViewportCamera::ThreeDFps);
        f.run();
        assert_eq!(f.binding_count(), 2, "只让位自动轨道");
        assert_eq!(f.mode3d(), CameraMode::Fps);
        assert_eq!(f.mode2d(), CameraMode::Path, "2D 仍由分镜驱动");
    }

    /// FPS 目标 ⇒ 模式 `Fps`。
    #[test]
    fn fps_target_sets_fps_mode() {
        let mut f = Fixture::new(UiConfig::default());
        f.set_cfg(|c| c.viewport_camera = ViewportCamera::ThreeDFps);
        f.run();
        assert_eq!(f.mode3d(), CameraMode::Fps);
    }

    /// **脚本自己的 `CameraEye` 绑定也必须让位**：脚本作品（如 `capability_gaps`）的
    /// 相机由 `Target.CameraEye` 驱动，而 `Camera3DHandle` 是宿主侧才登记的 ⇒ 它不在
    /// 构造参数的 `orbit` 里。只让位自动轨道的话，交互会被脚本每帧抹掉
    /// （用户看到"3D 相机控制完全没反应"）。
    #[test]
    fn script_camera_binding_is_suspended_and_restored() {
        let mut f = Fixture::new(UiConfig::default());
        // 模拟脚本绑定：另一条 `CameraEye`，写同一台 3D 相机、用不同的时钟。
        let script = Binding {
            clock: ClockId(999),
            driver: BindingDriver::Curve(Curve::Orbit {
                radius: 3.0,
                axis: ze_notion::Vec3::Y,
                speed: 0.2,
                initial_angle: 0.0,
            }),
            target: Arc::new(CameraEye {
                id: crate::contract::object_id_of(f.cam3d),
            }),
            predicate: None,
            active: None,
            reset: crate::animation::ResetBehavior::Hold,
            initial: None,
            seek_kind: SeekKind::O1,
        };
        f.sched
            .resources_mut()
            .get_mut::<BindingTable>()
            .unwrap()
            .add(script.clone());
        assert_eq!(f.binding_count(), 4, "自动轨道 + 2D 两条 + 脚本一条");

        f.set_cfg(|c| c.viewport_camera = ViewportCamera::ThreeDFps);
        f.run();
        assert!(
            !f.has_binding(script.clock),
            "脚本的 CameraEye 绑定必须让位，否则交互被每帧写回"
        );

        f.set_cfg(|c| c.viewport_camera = ViewportCamera::TwoD);
        f.run();
        assert!(f.has_binding(script.clock), "切回 TwoD 后必须原样放回");
        assert_eq!(f.binding_count(), 4, "放回不得重复或丢失");
    }

    /// 句柄失效（无相机对象）⇒ 不 panic；绑定让位仍生效。
    #[test]
    fn missing_cameras_are_noop() {
        let mut res = Resources::new();
        res.insert(UiConfig {
            free_2d_camera: true,
            ..UiConfig::default()
        });
        res.insert(Camera2DHandle::default());
        res.insert(Camera3DHandle::default());
        let orbit = Binding {
            clock: ClockId(0),
            driver: BindingDriver::Curve(Curve::Phase { omega: 1.0 }),
            target: Arc::new(CameraEye {
                id: crate::contract::ObjectId::DANGLING,
            }),
            predicate: None,
            active: None,
            reset: crate::animation::ResetBehavior::Hold,
            initial: None,
            seek_kind: SeekKind::O1,
        };
        let mut table = BindingTable::new();
        table.add(orbit.clone());
        res.insert(table);

        let mut sched = Scheduler::new(hecs::World::new(), res);
        sched.add_system(Phase::Input, CameraDriveSystem::new(Vec::new(), orbit));
        sched.init_resources();
        sched.run_frame(0.016);
        assert_eq!(
            sched
                .resources()
                .get::<BindingTable>()
                .unwrap()
                .bindings
                .len(),
            1
        );
    }

    // ── FPS 朝向状态的生命周期（PH 之后修的实测 bug）────────────────────

    /// **切进 FPS 时作废旧朝向**：另一个模式改过 `eye`/`target` 后，旧的 `FpsView`
    /// 若留着，下一次操作会把相机拉回上一次的朝向（实测：切回第一人称按 W 画面跳转）。
    #[test]
    fn entering_fps_discards_the_previous_heading() {
        let mut f = Fixture::new(UiConfig::default());
        // 模拟上一次 FPS 会话留下的朝向。
        f.sched
            .world_mut()
            .insert_one(f.cam3d, FpsView::new(1.0, 0.4))
            .unwrap();
        f.set_cfg(|c| c.viewport_camera = ViewportCamera::ThreeDFps);
        f.run();
        assert_eq!(f.mode3d(), CameraMode::Fps);
        assert!(
            f.sched.world().get::<&FpsView>(f.cam3d).is_err(),
            "切进 FPS 必须作废旧朝向，交给 CameraInputSystem 从相机重新推导"
        );
    }

    /// **停留在 FPS 内不清朝向**：模式没变时不得每帧抹掉正在累积的 `FpsView`。
    #[test]
    fn staying_in_fps_keeps_the_heading() {
        let mut f = Fixture::new(UiConfig::default());
        f.set_cfg(|c| c.viewport_camera = ViewportCamera::ThreeDFps);
        f.run();
        f.sched
            .world_mut()
            .insert_one(f.cam3d, FpsView::new(1.0, 0.4))
            .unwrap();
        f.run();
        let heading = *f.sched.world().get::<&FpsView>(f.cam3d).unwrap();
        assert_eq!(heading, FpsView::new(1.0, 0.4), "同模式内不得清掉朝向");
    }
}
