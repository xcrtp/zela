//! **CK-f**：合成器在 ECS 侧只剩转发。
//!
//! 旧的 `ComposerQueue` / `ComposerSystem`（用生命周期事件增量维护派生态）
//! 已按元规则 B 删除：渲染层与 app 层对它零 `get`，而增量结果必须与全量重算
//! 逐位对拍 ⇒ 增量白写，还引入插入顺序依赖。现在合成顺序由
//! [`layer_mounts`] 每次从声明重算（纯函数、固定声明顺序）。
//!
//! `Composer` 仍然是场景状态组件（进 payload），只能在命令路径上修改。

pub use ze_scene_core::composer::{Composer, ComposerRef, LayerMount, layer_mounts};
