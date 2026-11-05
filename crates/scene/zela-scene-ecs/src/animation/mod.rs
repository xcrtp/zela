// animation — ECS 侧（B3-1b 后）
//
// 数据/词汇（Binding / Timeline / Target 及其实现）已下沉 `ze-scene-core::animation`；
// 本层只留**驱动 System**（它们需要 `ExecutorContext` / hecs 世界）。
//
// 路径兼容：`zela_scene_ecs::animation::{binding, target, timeline}` 与
// `zela_scene_ecs::animation::{Binding, Target, …}` 全部转发自核心层。

pub mod camera_drive;
pub mod event_log;
pub mod morph;
pub mod path_follow;
pub mod system;

#[cfg(test)]
mod target_tests;

pub use camera_drive::CameraDriveSystem;
pub use event_log::StateEventLog;
pub use morph::MorphApplySystem;
pub use path_follow::PathFollowApplySystem;
pub use system::BindingApplySystem;

pub use ze_scene_core::animation::{binding, target, timeline};
// CA-b/c：参数注册表与地址回写（装配期 side table + 稳定 WidgetId 分配）。
pub use ze_scene_core::animation::params;
// CC-a：作用域（独立时钟 + 命名空间）。
pub use ze_scene_core::animation::{ScopeId, ScopeSpec, scope};
// CC-c：字段级写入仲裁（绑定 vs 绑定）。
pub use ze_scene_core::animation::{
    Binding, BindingDriver, BindingTable, Camera2DFraming, CameraEye, CircleRadius, FieldSpec,
    FieldTarget, LibraryRef, MorphT, OpacityValue, Phase, ResetBehavior, RevealRange, SeekKind,
    Target, TargetCtor, TargetEntry, TargetError, Timeline, TransformChannel,
    TransformChannelValue, target_ctor_by_name, target_entry,
};
pub use ze_scene_core::animation::{SuppressedWrite, WriteResolution, resolve};
