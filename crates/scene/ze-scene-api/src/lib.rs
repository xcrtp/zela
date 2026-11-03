//! 脚本 Pod 的 **domain 侧**：能力定义 + `scene` / `content` 两个 Service。
//!
//! 语言适配器（`ze-pod-lua` / `ze-pod-py`）只依赖这里的能力类型；
//! 本模块不认识任何语言运行时（无 mlua / pyo3）。
//!
//! ```text
//! zela-scene::pod
//! ├── api      SceneApi（16）/ ContentApi（2）
//! ├── types    结构化参数 / 返回值（PodType schema）
//! ├── scene_service    SceneServicePod
//! └── content_service  ContentServicePod + PathStore
//! ```
//!
//! B1 的绑定驱动（`ze-pod-binding`）只消费 `C::calls()`，与本模块无编译期耦合。

// `#[derive(SceneComponent)]` 生成的 impl 用 `::ze_scene_api::SceneComponent` 全路径引用；
// 自身 crate 不在 extern prelude，需要 `extern crate self` 别名才能解析（宏未知使用方）。
extern crate self as ze_scene_api;

pub mod api;
mod content_service;
mod scene_service;
pub mod types;

pub use api::{ContentApi, SceneApi};
pub use content_service::{
    CONTENT_SERVICE_NAME, CONTENT_SERVICE_VERSION, ContentServicePod, PathStore,
};
pub use scene_service::{
    FINISH_PATH, RESET_PATH, SERVICE_NAME, SERVICE_VERSION, SceneServicePod, channels, methods,
};
pub use types::*;

// ─── SceneComponent：组件注册的**两个名字同源**（提案 #6 / E 批）──────────────

/// 组件类型在 `#[derive(PodType)]` + `#[derive(SceneComponent)]` 之外还需具备的
/// 注册元数据：Pod 类型名（`"zela.Circle"`）与构造器函数名（`"circle"`）。
///
/// 两个常量都由 `#[derive(SceneComponent)]` 从声明生成 ——
/// 不再存在 `#[pod_type(name)]` 与 `register_fn1_named` 两处手写字符串漂移。
pub trait SceneComponent: ze_pod_core::PodType {
    /// Pod 类型名（`"zela.Circle"`）—— 与 `#[pod_type]` 同源。
    const POD_TYPE_NAME: &'static str;
    /// 构造器函数名（`"circle"`，不含 `zela.scene::` 前缀）—— 由 ident 的 snake_case 推导。
    const CTOR_NAME: &'static str;
}

/// `RegistryPod` 的组件注册便捷入口：一条注册从 5 行（3 个手写字符串 + 函数）降到 1 行。
pub trait RegisterSceneComponent {
    /// 注册一个单结构体参数的组件构造器；路径 / 参数类型名 / 返回类型名都由
    /// `<T as SceneComponent>` 提供（返回类型统一为 `ze_pod_core.Handle`）。
    /// `Clone` 是 `register_fn1_named` 的既有约束（组件类型全部 derive 了它）。
    fn register_component<T>(&mut self, ctor: fn(&dyn ze_pod_core::Pod, T) -> ze_pod_core::Handle)
    where
        T: SceneComponent + Clone;
}

impl RegisterSceneComponent for ze_pod_core::RegistryPod {
    fn register_component<T>(&mut self, ctor: fn(&dyn ze_pod_core::Pod, T) -> ze_pod_core::Handle)
    where
        T: SceneComponent + Clone,
    {
        self.register_fn1_named::<T, ze_pod_core::Handle>(
            &format!("zela.scene::{}", T::CTOR_NAME),
            T::POD_TYPE_NAME,
            "ze_pod_core.Handle",
            ctor,
        );
    }
}

/// 脚本**前置块**（Lua）：宿主在用户脚本之前执行。
///
/// 内容见 [`lua_prelude.lua`](lua_prelude.lua)：目前只有 `scene.text`（把
/// 「`content.text` 编译 + 逐字形 spawn」收成一行）。放在前置块而不是能力函数，
/// 是因为它是**组合调用**（跨 `scene` / `content`），做成能力会改 `zela.scene` 签名
/// ⇒ 所有作品 payload 改名，而它的定位是"改写法不改产物"。
///
/// 语言绑定侧保持通用：`LuaPod::with_prelude` 只接收一段脚本，不认识里面的领域名。
pub const LUA_PRELUDE: &str = include_str!("lua_prelude.lua");

use std::sync::Arc;

use ze_prerender::InMemoryEngine;

/// 构造场景 Service（返回**具体类型**：调用方可用它登记 `SyncPod` 徽章）。
pub fn scene_service(paths: PathStore) -> Arc<SceneServicePod> {
    Arc::new(SceneServicePod::with_paths(paths))
}

/// 构造内容 Service（返回具体类型，理由同上）。
pub fn content_service(
    engine: Arc<InMemoryEngine>,
    paths: PathStore,
) -> Arc<ContentServicePod> {
    Arc::new(ContentServicePod::new(engine, paths))
}
