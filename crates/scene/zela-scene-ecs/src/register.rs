// register.rs — **场景侧 System 注册点**（A6：注册序列从 app 归位到场景层）。
//
// 为什么归位：若"注册哪些场景 system"留在 app，app 就会把场景细节吸回来
// （瘦完还会胖回去）。这里只注册**场景自己拥有的** System；渲染录制与 UI 接线
// 分别由 `zela-render` / `zela-egui` 各自的注册函数负责，避免反向依赖。
//
// Phase 分配（Director 是唯一时间源）：
// - `Input`：DirectorAdvance（W=DirectorState/TimeState）、
//   CameraDrive（R=UiConfig/Camera*Handle，W=BindingTable）、
//   CameraInput（R=InputEvents/Camera*Handle，写场景对象组件）；
//   相机驱动必须在 `CameraInput` **之前** ⇒ 同帧生效。
// - `SceneSync`：BindingApply（R=BindingTable）。
// - `Prepare`：Replay（重播清理，在录制之前）。
// - `FrameEnd`：DeferredApply（帧末统一应用延迟命令，避免录制遍历中删对象）。

use zela_schedule::scheduler::{Phase, Scheduler};

use crate::animation::{Binding, BindingApplySystem, CameraDriveSystem, PathFollowApplySystem};
use crate::camera_input::CameraInputSystem;
use crate::deferred::{DeferredApplySystem, ReplaySystem};
use crate::director_state::DirectorAdvanceSystem;
use crate::geometry::{AxesRefreshSystem, FunctionGraphRefreshSystem};
use crate::input::{ParamPanelSystem, TimeControlSystem};
use crate::lifetime::LifetimeFlushSystem;
use crate::transform_hierarchy::TransformHierarchySystem;

/// 注册场景侧全部 System。
///
/// `cam2d_bindings`：分镜里指向 2D 相机的绑定（`center` + `scale`）；
/// `orbit_binding`：3D 自动轨道绑定。两者由装配层构造，`CameraDriveSystem` 托管让位。
pub fn register_systems(
    scheduler: &mut Scheduler,
    cam2d_bindings: Vec<Binding>,
    orbit_binding: Binding,
) {
    scheduler.add_system(Phase::Input, DirectorAdvanceSystem);
    // K3 + 2D 自由开关：按「2D 相机自由」与「视口操作目标」决定两台相机由分镜还是交互
    // 驱动，并让对应绑定让位/复位。放在 `CameraInputSystem` 之前：同帧内先改模式，
    // 交互才开始按新模式解释。
    scheduler.add_system(
        Phase::Input,
        CameraDriveSystem::new(cam2d_bindings, orbit_binding),
    );
    // P1：相机输入由场景层消费（相机状态归场景对象，UI 只发命令）。
    scheduler.add_system(Phase::Input, CameraInputSystem);

    scheduler.add_system(Phase::SceneSync, BindingApplySystem);
    // 阶段 2（路径跟随）：`PathFollow.t` → `Transform`。必须在 BindingApply 之后
    // （后者本帧写 `t`），且早于 Prepare 的 `TransformHierarchySystem`（父子累乘）。
    scheduler.add_system(Phase::SceneSync, PathFollowApplySystem);

    // P6/B4：重播清理 —— 运行时创建的对象在重播时销毁（`Prepare`，在录制之前；
    // 该相位当前无其它 Resource 写者）。
    scheduler.add_system(Phase::Prepare, ReplaySystem);
    // **CK-f**：`ComposerSystem` 已删除 —— 合成顺序由 `composer::layer_mounts`
    // 每次从声明重算（纯函数），不再维护增量派生态。

    // P6/B3：延迟命令应用 —— 帧末统一应用（避免录制遍历中删对象）。
    scheduler.add_system(Phase::FrameEnd, DeferredApplySystem);
    // S1：生命周期事件必须在**删除生效之后**求差 ⇒ 注册在 DeferredApply 之后。
    scheduler.add_system(Phase::FrameEnd, LifetimeFlushSystem);

    // S0：UI 输入的**场景侧**消费者（从 zela-egui 归位；面板 chrome 仍在 UI 层）。
    scheduler.add_system(Phase::LogicUpdate, TimeControlSystem);
    // S0 + **CE-e**：参数回写（场景内面板与历史 `WidgetId` 分支共用一条 `params::apply` 路径）。
    scheduler.add_system(Phase::LogicUpdate, ParamPanelSystem);
    // **CG-f / CK-e**：自研面板的控件状态机（视口事件 → ControlSet → 资源回写）。
    //
    // **#17 方案 B（已写死）**：`HudSystem` 保留在 `FrameEnd`。方案 A（前移到
    // `Input` / `LogicUpdate` 与 `ParamPanelSystem` 合并单一写者）被调度器的静态
    // 冲突检查否决：它与 `TimeControlSystem`（`DirectorState`）和 `ParamPanelSystem`
    // （`BindingTable`）在同一相位争写同一批资源。承认跨帧，并把相位与延迟帧数
    // 写进 `TraceReport` 的 `Diagnostics`（`writeback_phase` / `writeback_delay_frames`），
    // **不假装同帧**。记账见 GAPS `CK-` 段。
    scheduler.add_system(Phase::FrameEnd, crate::hud::HudSystem);
    // S5：父子变换累乘（BindingApply 写入 local 之后、录制之前）。
    scheduler.add_system(Phase::Prepare, TransformHierarchySystem);
    // S0：场景几何刷新（从 zela-egui 归位；几何算法在对象层）。
    scheduler.add_system(Phase::Prepare, AxesRefreshSystem);
    scheduler.add_system(Phase::Prepare, FunctionGraphRefreshSystem);
}
