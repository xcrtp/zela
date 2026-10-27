//! `zela-playback` — **回放运行时**（浏览器与 native `--bundle` 共用）。
//!
//! # 为什么单独成 crate
//!
//! `zela-app` 无条件依赖 Lua / Typst，**不能**进浏览器。但回放只需要
//! 「ZSB payload → Scene → 调度器 → 帧回调」，与脚本运行时无关。
//!
//! # 防双轨
//!
//! 装配尾只有一份：[`assemble_runtime`]。
//! - `zela-app`：脚本 / 内置场景 → 建 `Scene` → 调 [`assemble_runtime`]；
//! - 本 crate：`Bundle` → 还原 `SceneScript` → commit → 调 [`assemble_runtime`]。
//!
//! # 时钟
//!
//! 默认沿用 `zela-schedule` 的安全零时钟（wasm 不注入也不 panic）；
//! native 宿主（`zela-app`）在拿到 [`Runtime`] 后自行注入真实时钟，
//! 浏览器注入 `performance.now()` 实现。

use std::sync::Arc;

// B11 Step 4/5：3D → 矢量后端的桥（native 与浏览器共用）与按帧渲染。
pub mod input_replay;
pub mod vector3d;
pub mod vector_frame;

pub use input_replay::apply_input_frame;

/// B12 Step 1.1：headless / 浏览器入口的 tracing 订阅器。
///
/// 转发 `zela_schedule::telemetry::init_tracing_headless`（默认 `warn`、
/// `ZELA_LOG` 控等级、`ZELA_LOG_FILE` 控目标）；重复调用安全。
pub fn init_tracing() {
    zela_schedule::telemetry::init_tracing_headless();
}

use ze_notion::Vec3;
use zela_render::{
    ContentTextureQueue, DrawQueue, DrawQueue3D, LightQueue, MorphQueue, RevealQueue, ViewRect,
};
use zela_render_backends::{
    RenderContext, SceneCompositorCallback, ScreenParams, StandardBackends,
    render_frame_headless_async, render_frame_to_view,
};
#[cfg(feature = "blocking")]
use zela_render_backends::render_frame_headless;
use zela_scene_ecs::animation::{
    Binding, BindingDriver, BindingTable, CameraEye, ResetBehavior,
};
use zela_scene_ecs::contract::{GroupVisibility, ObjectId, SceneRead, WorldView};
use zela_scene_ecs::director_state::DirectorState;
use zela_scene_ecs::domain::{ContentTextureSpecs, InputEvents, UiConfig};
use zela_scene_ecs::scene::Scene;
use zela_scene_ecs::{
    ClockForest, ClockNode, ComposerQueue, Curve, DeferredCommands, DriverCommand, DriverPod,
    DynamicObjects, LifetimeEvents, LifetimeTracker, SeekKind, TimeState,
};
use zela_scene_bundle::{
    Available, Bundle, BundleError, Capability, Pod, RestoreError,
};
use zela_schedule::resource::Resources;
use zela_schedule::scheduler::Scheduler;
use ze_scene_core::{Camera2DHandle, Camera3DHandle};

/// 回放装配失败（全部显式，不静默降级）。
#[derive(Debug, thiserror::Error)]
pub enum PlaybackError {
    /// bundle 的可还原性检查失败（缺能力 / 版本不匹配）。
    #[error("{0}")]
    Restore(#[from] RestoreError),
    /// payload 解析 / 还原失败。
    #[error("ZSB payload 还原失败：{0}")]
    Bundle(#[from] BundleError),
    /// 场景装配失败（例如没有 2D 相机）。
    #[error("{0}")]
    Assembly(String),
}

pub type Result<T> = std::result::Result<T, PlaybackError>;

/// 一次装配的产物：GUI / headless / 浏览器共用。
pub struct Runtime {
    /// 命令驱动的场景推进器。
    pub driver: Arc<DriverPod>,
    /// 渲染上下文（由宿主创建后传入）。
    pub render_ctx: RenderContext,
    /// 默认录制时长（秒）：静止时刻提示；不可推导时由时钟树兜底。
    pub duration: f64,
}

/// [`assemble_runtime`] 的输入：**场景已经建好**，只差资源 / 系统图 / Driver。
///
/// 脚本路径与 bundle 路径都先把 `Scene` 准备好，再走同一条尾部装配。
pub struct SceneAssembly {
    pub world: hecs::World,
    pub forest: ClockForest,
    pub bindings: BindingTable,
    /// 2D 相机（**可缺席**：纯 3D 场景不需要占位相机 —— PF Step 2）。
    pub camera2d: Option<Camera2DHandle>,
    pub group_visibility: GroupVisibility,
    /// 静止时刻提示（`None` = 不可推导 / 未提供；录多长由 frames 决定）。
    pub record_hint: Option<f64>,
    /// 可选 3D 相机（GUI demo 用）。
    pub camera3d: Option<Camera3DHandle>,
    /// 可选内容纹理规格（GUI demo 用；B3 Step 4c-2 起为多实例容器）。
    pub content_textures: Option<ContentTextureSpecs>,
}

/// **回放端能解释什么**：payload 是已烘焙的纯数据 ⇒ 只需要 scene 能力与 scene Pod。
///
/// 装配期用到的 content Pod 不在这里（见 ZSB Meta 的 `required_pods` 语义）。
pub fn available_playback_caps() -> Available {
    Available::new(vec![scene_capability()], vec![scene_pod()])
}

fn scene_pod() -> Pod {
    Pod::new(
        zela_scene_ecs::pod::SERVICE_NAME,
        zela_scene_ecs::pod::SERVICE_VERSION,
    )
}

fn scene_capability() -> Capability {
    let meta = <zela_scene_ecs::pod::SceneApi<'static> as ze_pod_core::Capability<'static>>::meta();
    let name = <zela_scene_ecs::pod::SceneApi<'static> as ze_pod_core::Capability<'static>>::signatures()
        .first()
        .map(|signature| signature.namespace)
        .unwrap_or("");
    Capability::new(name, meta.version)
}

/// **ZSB → Runtime**：可还原性检查 → 渲染参数校验 → 还原 `SceneScript` → commit → 公共尾装配。
pub fn build_bundle_runtime(render_ctx: RenderContext, bundle: &Bundle) -> Result<Runtime> {
    bundle.check_restorable(&available_playback_caps())?;
    // B4 Step 2 / D1：参数非法 ⇒ 显式错误（不静默纠正）。
    let _params = bundle_render_params(bundle)?;

    let script = bundle.script()?;
    // **第 4 批**：产物记录的算子 revision vs 当前库 —— 不符即**点名算子**失败
    // （不是笼统的库版本不符）。老 payload 没有记录 ⇒ 空表，直接通过。
    for library in script.libraries() {
        ze_pod_func::check_op_revisions(&library.op_revisions)
            .map_err(PlaybackError::Assembly)?;
    }
    // 静止时刻是**派生量**（可能不可推导）；录多长仍由调用方的 frames 决定。
    let record_hint = script.still_time();

    let mut scene = Scene::new();
    let recipe = script
        .clone()
        .commit(&mut scene)
        .map_err(|e| PlaybackError::Assembly(format!("ZSB commit 失败：{e}")))?;
    let camera2d = find_camera2d(&scene)?;
    // B3 Step 5：Surface 内容纹理同样从场景推导（与 app 脚本路径同一条入口）。
    let content_textures = zela_scene_ecs::domain::content_texture_specs(&scene)
        .map_err(|error| PlaybackError::Assembly(format!("Surface 纹理装配失败：{error}")))?;
    let content_textures = (!content_textures.is_empty()).then_some(content_textures);
    let world = scene.into_world();

    Ok(assemble_runtime(
        render_ctx,
        SceneAssembly {
            world,
            forest: recipe.forest,
            bindings: recipe.bindings,
            camera2d,
            group_visibility: GroupVisibility::default(),
            record_hint,
            camera3d: None,
            content_textures,
        },
    ))
}

/// **回放端读渲染参数**（B4 Step 2 / D1）：取 Meta → 有效性校验 → 返回。
///
/// 宿主拿到它之后还应调用 [`check_render_params_supported`]（需要后端的
/// `Capabilities`），两者都不通过就**中止**，不得降级。
pub fn bundle_render_params(bundle: &Bundle) -> Result<zela_scene_bundle::RenderParams> {
    let params = bundle.meta().render_params;
    params
        .validate()
        .map_err(|e| PlaybackError::Assembly(format!("ZSB 渲染参数非法：{e}")))?;
    Ok(params)
}

/// **一致性校验**（B4 Step 2 / §0.3 ②）：后端不支持该参数 ⇒ 显式失败，不静默降级。
pub fn check_render_params_supported(
    params: &zela_scene_bundle::RenderParams,
    caps: &ze_render_ir::Capabilities,
) -> Result<()> {
    params
        .is_supported_by(caps)
        .map_err(|e| PlaybackError::Assembly(format!("回放端不支持该渲染参数：{e}")))
}

/// **唯一的装配尾**：资源插入序列 + System 注册 + Driver pod。
pub fn assemble_runtime(render_ctx: RenderContext, assembly: SceneAssembly) -> Runtime {
    let SceneAssembly {
        world,
        mut forest,
        mut bindings,
        camera2d,
        group_visibility,
        record_hint,
        camera3d,
        content_textures,
    } = assembly;

    // 2D 相机取景绑定（`center` + `scale`）交给 `CameraDriveSystem` 托管。
    // 无 2D 相机（纯 3D 场景）⇒ 没有该目标的绑定。
    let cam2d_bindings: Vec<Binding> = match camera2d {
        Some(camera) => bindings
            .bindings
            .iter()
            .filter(|binding| binding.target.target_id() == camera.0)
            .cloned()
            .collect(),
        None => Vec::new(),
    };

    // 3D 轨道相机：eye = radius * rotate_y(speed * t)，radius=5, speed=0.5。
    let orbit_clock = forest.insert(ClockNode::new(None)).unwrap();
    let orbit_binding = Binding {
        clock: orbit_clock,
        driver: BindingDriver::Curve(Curve::Orbit {
            radius: 5.0,
            axis: Vec3::Y,
            speed: 0.5,
            initial_angle: 0.0,
        }),
        // 无 3D 相机时指向悬垂 id（不会命中任何对象）。
        target: Arc::new(CameraEye {
            id: camera3d.map_or(ObjectId::DANGLING, |handle| handle.0),
        }),
        predicate: None,
        active: None,
        reset: ResetBehavior::Hold,
        initial: None,
        seek_kind: SeekKind::O1,
    };
    bindings.add(orbit_binding.clone());

    let duration = record_hint
        .filter(|seconds| *seconds > 0.0)
        .unwrap_or_else(|| forest_duration(&forest, &bindings));

    let mut resources = Resources::new();
    // 2D 相机可缺席：缺席 ⇒ `SceneRecordSystem` 等按"无相机"跳过 2D 录制
    // （纯 3D 帧本来就没有 2D 记录；图元却无相机的情况在装配期由调用方判定）。
    if let Some(camera2d) = camera2d {
        resources.insert(camera2d);
    }
    if let Some(camera3d) = camera3d {
        resources.insert(camera3d);
    }
    resources.insert(TimeState::default());
    resources.insert(UiConfig::default());
    resources.insert(zela_scene_ecs::WorldTransforms::default());
    resources.insert(zela_scene_ecs::TransformHierarchyError::default());
    resources.insert(ViewRect::default());
    resources.insert(InputEvents::default());
    resources.insert(DrawQueue::new());
    resources.insert(DrawQueue3D::new());
    // B14（X5-01）：`LightQueue` 此前**从未注入** ⇒ `SceneRecord3DSystem` 的
    // `with_world_and_resource_mut::<LightQueue>` 静默不生效，两个后端都只用默认灯、
    // 脚本里的 `Light` 组件被无声忽略。注入后灯才真正进入渲染（native 基线随之重录）。
    resources.insert(LightQueue::new());
    if let Some(specs) = content_textures {
        resources.insert(specs);
        resources.insert(ContentTextureQueue::default());
    }
    resources.insert(MorphQueue::default());
    resources.insert(RevealQueue::default());
    resources.insert(forest);
    resources.insert(bindings);
    resources.insert(DirectorState::new());
    resources.insert(group_visibility);
    resources.insert(DeferredCommands::new());
    resources.insert(DynamicObjects::new());
    resources.insert(LifetimeTracker::new());
    resources.insert(LifetimeEvents::new());
    resources.insert(ComposerQueue::new());

    let mut scheduler = Scheduler::new(world, resources);
    zela_scene_ecs::register::register_systems(&mut scheduler, cam2d_bindings, orbit_binding);
    zela_render::register_record_systems(&mut scheduler);

    Runtime {
        driver: Arc::new(DriverPod::new(scheduler)),
        render_ctx,
        duration,
    }
}

/// **视口相机控制**的初始配置（"相机控制与作品接线"的唯一判据）。
///
/// - `camera`：拖拽 / 滚轮操作哪台相机（见 `ViewportCamera`）；
/// - `free_2d`：2D 相机是否交给用户（分镜绑定让位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportControl {
    pub camera: zela_scene_ecs::domain::ViewportCamera,
    pub free_2d: bool,
}

/// 按**作品实际有什么相机**给出视口控制配置。
///
/// - 只有 3D 相机 ⇒ `ThreeDOrbit`：否则 3D 相机停在 `Path`（动画驱动）模式，
///   拖拽 / 滚轮被 `CameraInputSystem` 直接忽略 —— 用户看到的是"相机控制没反应"；
/// - 有 2D 相机 ⇒ `TwoD`；若它**没有动画绑定**（静态取景）⇒ `free_2d = true`，
///   用户可以直接平移 / 缩放；有动画则保持分镜驱动（面板里仍可手动接管）。
pub fn viewport_control(runtime: &Runtime) -> ViewportControl {
    runtime.driver.with_scheduler(|scheduler| {
        let resources = scheduler.resources();
        let cam2d = resources.get::<Camera2DHandle>().copied();
        let mut has_3d = false;
        zela_scene_ecs::contract::SceneRead::for_each::<ze_scene_core::Camera3D>(
            &zela_scene_ecs::contract::WorldView::new(scheduler.world()),
            |_, _| has_3d = true,
        );
        // 2D 相机的 `center` / `scale` 若有绑定 ⇒ 分镜在推它，默认不让位。
        let animated_2d = cam2d.is_some_and(|camera| {
            resources
                .get::<zela_scene_ecs::animation::BindingTable>()
                .is_some_and(|bindings| {
                    bindings
                        .bindings
                        .iter()
                        .any(|binding| binding.target.target_id() == camera.0)
                })
        });
        ViewportControl {
            camera: if has_3d && cam2d.is_none() {
                zela_scene_ecs::domain::ViewportCamera::ThreeDOrbit
            } else {
                zela_scene_ecs::domain::ViewportCamera::TwoD
            },
            free_2d: cam2d.is_some() && !animated_2d,
        }
    })
}

/// 把场景里的第一台 3D 相机登记为 `Camera3DHandle`（**宿主侧**，GUI 的 3D 相机控制需要）。
///
/// 为什么不在 [`assemble_runtime`] 里做：那里注册的**自动轨道**绑定在
/// `viewport_camera == TwoD`（headless 默认）时会每帧写 `eye`；目标一旦从悬垂 id
/// 变成真实相机，headless 基线里的 3D 相机就会开始公转。GUI 用 3D 轨道模式时该绑定
/// 本来就让位，因此只在宿主侧登记 —— headless 逐位不变。
pub fn install_camera3d(runtime: &Runtime) {
    let found = runtime.driver.with_scheduler(|scheduler| {
        if scheduler.resources().get::<Camera3DHandle>().is_some() {
            return None;
        }
        let mut found = None;
        zela_scene_ecs::contract::SceneRead::for_each::<ze_scene_core::Camera3D>(
            &zela_scene_ecs::contract::WorldView::new(scheduler.world()),
            |id, _| {
                if found.is_none() {
                    found = Some(id);
                }
            },
        );
        found
    });
    if let Some(id) = found {
        runtime
            .driver
            .with_scheduler_mut(|scheduler| scheduler.resources_mut().insert(Camera3DHandle(id)));
    }
}

/// 把 [`viewport_control`] 写进场景的 `UiConfig`（GUI 每次装配 / 切作品后调用）。
pub fn apply_viewport_control(runtime: &Runtime) {
    let control = viewport_control(runtime);
    runtime.driver.with_scheduler_mut(|scheduler| {
        if let Some(config) = scheduler.resources_mut().get_mut::<UiConfig>() {
            config.viewport_camera = control.camera;
            config.free_2d_camera = control.free_2d;
        }
    });
}

/// 2D 相机的语义判据：场景里唯一带 `Framing2D` 的对象。
///
/// **PF Step 2**：纯 3D 场景**允许没有 2D 相机**（返回 `Ok(None)`）—— 此前这个约束
/// 从渲染层泄漏到了脚本层，逼作者补占位 `Camera2D`。
///
/// H3（不静默丢）：没有相机但场景里**确实有 2D 图元** ⇒ 显式错误。
/// 判据复用它自己那条录制路径（`record_scene` 的 8 组组合），不另维护一张清单。
pub fn find_camera2d(scene: &Scene) -> Result<Option<Camera2DHandle>> {
    let mut cameras = Vec::new();
    scene.for_each::<ze_scene_core::Framing2D>(|id, _| cameras.push(id));
    if let Some(id) = cameras.first() {
        return Ok(Some(Camera2DHandle(*id)));
    }
    // 用默认取景录一遍：只要产出一条记录，就说明"有 2D 内容却没相机"。
    let probe = ze_scene_core::Camera2D {
        center: ze_notion::Vec2::ZERO,
        scale: 55.0,
        rect: ze_notion::Rect2D::new(
            ze_notion::Vec2::ZERO,
            ze_notion::Vec2::new(640.0, 480.0),
        ),
    };
    let mut queue = DrawQueue::new();
    zela_render::draw::record_scene(scene, &probe, &mut queue);
    if queue.commands().is_empty() {
        Ok(None)
    } else {
        Err(PlaybackError::Assembly(format!(
            "场景有 {} 条 2D 图元但没有 2D 相机（缺少 Framing2D）：\
             要么声明 Camera2D，要么移除这些图元（不静默丢弃）",
            queue.commands().len()
        )))
    }
}

/// 动画时长 = 最晚结束的绑定（`local_offset + span.1`）；无绑定 ⇒ 0。
fn forest_duration(forest: &ClockForest, bindings: &BindingTable) -> f64 {
    bindings
        .bindings
        .iter()
        .filter_map(|binding| {
            let node = forest.get(binding.clock)?;
            let (_, end) = node.span?;
            Some(node.local_offset + end)
        })
        .fold(0.0, f64::max)
}

/// 设置画布/视口尺寸（物理像素）。宿主在每帧推进前调用一次。
pub fn set_view_rect(runtime: &Runtime, width: f32, height: f32) {
    runtime.driver.with_scheduler_mut(|scheduler| {
        scheduler.resources_mut().insert(ViewRect {
            rect: ze_notion::Rect2D::new(
                ze_notion::Vec2::ZERO,
                ze_notion::Vec2::new(width, height),
            ),
        });
    });
}

/// **帧循环一步**：推进一帧并构造本帧回调；无图层时返回 `None`。
pub fn step_frame(runtime: &Runtime, dt: f32) -> Option<SceneCompositorCallback> {
    runtime
        .driver
        .submit(DriverCommand::Advance { dt })
        .wait();

    runtime.driver.with_scheduler(|scheduler| {
        let options = zela_render::FrameOptions::default();
        let view = zela_render::frame::build_scene_view(
            &WorldView::new(scheduler.world()),
            scheduler.resources(),
            options,
        );
        zela_render::frame::build_frame_inputs(&view, scheduler.resources(), options)
            .and_then(zela_render_backends::frame::build_scene_callback)
    })
}

/// 帧末清理（**不能漏**：与 native 导出循环逐字一致）。
pub fn end_frame(runtime: &Runtime) {
    runtime
        .driver
        .with_scheduler_mut(|scheduler| zela_render::frame::end_frame(scheduler.resources_mut()));
}

/// 推进一步并离屏渲染，返回**紧凑 RGBA8**（跨运行时对拍的规范字节）。
///
/// 同步版：仅 native `blocking` 构建可用（浏览器用 [`advance_and_render_async`]）。
#[cfg(feature = "blocking")]
pub fn advance_and_render(
    runtime: &Runtime,
    dt: f32,
    ctx: &RenderContext,
    backends: &mut StandardBackends,
    screen: ScreenParams,
    clear: wgpu::Color,
) -> Option<Vec<u8>> {
    let Some(callback) = step_frame(runtime, dt) else {
        end_frame(runtime);
        return None;
    };
    let pixels = render_frame_headless(ctx, backends, &callback, screen, clear);
    let rgba = to_rgba(ctx, &pixels);
    end_frame(runtime);
    Some(rgba)
}

/// 推进一步并离屏渲染（异步回读版，浏览器对拍用）。
pub async fn advance_and_render_async(
    runtime: &Runtime,
    dt: f32,
    ctx: &RenderContext,
    backends: &mut StandardBackends,
    screen: ScreenParams,
    clear: wgpu::Color,
) -> Option<Vec<u8>> {
    let Some(callback) = step_frame(runtime, dt) else {
        end_frame(runtime);
        return None;
    };
    let pixels = render_frame_headless_async(ctx, backends, &callback, screen, clear).await;
    let rgba = to_rgba(ctx, &pixels);
    end_frame(runtime);
    Some(rgba)
}

/// 直接把一帧渲染进给定 view（浏览器**显示**路径）。
pub fn render_step_to_view(
    runtime: &Runtime,
    dt: f32,
    ctx: &RenderContext,
    backends: &mut StandardBackends,
    screen: ScreenParams,
    clear: wgpu::Color,
    view: &wgpu::TextureView,
) -> bool {
    let Some(callback) = step_frame(runtime, dt) else {
        end_frame(runtime);
        return false;
    };
    render_frame_to_view(ctx, backends, &callback, screen, clear, view);
    end_frame(runtime);
    true
}

/// 矢量后端自述的能力：后处理四项全开，采样数取 4（与 2D 路径同口径）。
///
/// **单一真源**：headless 帧循环（`zela-app::frame_loop`）与 GUI 都从这里取 ——
/// 否则同一个后端在两处得到不同的默认参数。
pub fn vector_capabilities() -> ze_render_ir::Capabilities {
    ze_render_ir::Capabilities {
        max_sample_count: 4,
        color_format: ze_render_ir::ColorFormat::Rgba8UnormSrgb,
        depth_format: ze_render_ir::DepthFormat::Depth32Float,
        post: zela_render_backends::BackendKind::Vector.post_caps(),
    }
}

/// 矢量后端的默认渲染参数（尺寸已写入）。
pub fn vector_params(width: u32, height: u32) -> zela_scene_bundle::RenderParams {
    let mut params = zela_scene_bundle::RenderParams::default_for(&vector_capabilities());
    params.size = (width, height);
    params
}

/// 把目标格式的紧凑像素转换为 **RGBA8**（仅通道顺序，不做域转换）。
///
/// 与 `zela_app::export::to_rgba` 同一实现（后者改为委托本函数）。
pub fn to_rgba(ctx: &RenderContext, pixels: &[u8]) -> Vec<u8> {
    use wgpu::TextureFormat;

    let mut out = pixels.to_vec();
    match ctx.target_format() {
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => {
            for px in out.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        _ => {}
    }
    out
}

/// 把 **RGBA8** 转换成目标纹理格式的字节序（[`to_rgba`] 的逆）。
///
/// 矢量后端在 CPU 侧产出 RGBA8，而显示 / sink 的口径是目标格式 ⇒ 需要这一趟交换。
pub fn rgba_to_target(ctx: &RenderContext, rgba: &[u8]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    if matches!(
        ctx.target_format(),
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        for px in out.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
    }
    out
}
