// host.rs — EguiSceneHost：egui ↔ 场景的唯一适配层（B 阶段，计划 §6.3/§6.4）
//
// 职责：
// 1. `build_scene_view` — 场景 → UI：把场景状态声明为纯数据 [`SceneView`]；
// 2. `collect_events`   — UI → 场景：把 egui 原生输入翻译为 [`SceneEvent`]，
//    视口坐标已换算为场景坐标（经由 `Camera2D::screen_to_world`）；
// 3. **CG-f**：控件不再由 host 绘制 —— 它们是场景图元（`Rect` / `Circle` / `Path`），
//    交互走 `HudSystem`（消费同一批 [`SceneEvent`]）。host 只保留一层纯数据同步：
//    `UiConfig` → `PostFilterChain`（渲染层的派生 Resource）。
// 4. `update_view_rect` — 面板布局完成后写入中央视口矩形（布局信息本就属 UI，
//    由 host 承担原 ViewRectUpdateSystem 的职责）；
// 5. `present` — 把 `DrawQueue`(2D) 与 `DrawQueue3D` 的录制内容提交到
//    egui painter（承担原 RenderSubmitSystem / RenderSubmit3DSystem 的职责）。
// 6. 宿主环境 — `install_cjk_font`：按平台候选路径（+ `ZELA_CJK_FONT`）安装 CJK 字体；
// 7. 帧末    — `draw_call_count`（可观测性取数）/ `end_frame`（清理已消费的队列）。
//
// 这是唯一同时接触 egui 与场景两者的模块；将来迁移 UI 只需另写一个 host，
// 场景逻辑一行不动。本文件之外的 zela-egui 模块不得出现 egui 类型（B 阶段红线）。
//
// 帧内调用顺序（zela-app::App::ui）：
// `build_scene_view` → `begin_frame` → `Scheduler::run_frame` → `present`。

use std::sync::Arc;

use ze_notion::Rect2D;
use zela_render::ViewRect;
use zela_render_backends::SceneCompositorCallback;
use zela_scene_ecs::contract::WorldView;
use zela_scene_ecs::domain::{
    InputEvents, SceneEvent, SceneView,
};
#[cfg(test)]
use zela_scene_ecs::domain::LayerKind;
use zela_schedule::resource::Resources;

use crate::resources::{UiConfig, ViewportCamera};

pub struct EguiSceneHost;

impl EguiSceneHost {
    // ─── 帧编排 ────────────────────────────────────────────────────────────

    /// 帧首调用（在 `Scheduler::run_frame` 之前）：
    /// 1. **CG-f**：控件已全部是场景图元（`Hud` / `panels`），host 不再画面板 chrome；
    ///    这里只把 `UiConfig` 派生成渲染层的 `PostFilterChain`（纯数据同步）；
    /// 2. 更新 [`ViewRect`]（只覆盖**中央视口**）；
    /// 3. 收集视口原始输入 → 场景事件（用刚算出的视口矩形做"是否在视口内"判据）；
    /// 4. 整帧替换写入 [`InputEvents`]（自动清空上一帧的事件）。
    ///
    /// ⚠️ **顺序不能反**：先 `update_view_rect` 会把侧栏区域也算进视口 ⇒
    /// 画面盖住控制面板（实测「面板消失」），面板里的滚动还会带动相机。
    ///
    /// 注意：ViewRect 必须在 run_frame 之前写入（而非 present 时）——
    /// Prepare/RenderCommit 阶段的 GridBuild/SceneRecord 当帧就要用它，
    /// 否则窗口 resize 时内容会慢一帧。
    pub fn begin_frame(ui: &mut egui::Ui, view: &SceneView, res: &mut Resources) {
        // 视口操作目标（K3）：拖拽/滚轮产生哪一类相机命令。
        let target = res
            .get::<UiConfig>()
            .map(|c| c.viewport_camera)
            .unwrap_or_default();
        // CG-f：控件由场景侧 `HudSystem` 写 `UiConfig`（帧末）⇒ 这里只把它翻译成滤镜链。
        let ui_config = res.get::<UiConfig>().cloned().unwrap_or_default();
        res.insert(crate::resources::post_filters(&ui_config));
        let viewport = Self::update_view_rect(ui, res);
        let events = Self::collect_events(ui, view, target, viewport);
        res.insert(InputEvents::from_events(events));
    }

    // ─── 宿主环境（平台相关）──────────────────────────────────────────────

    /// 安装 CJK 字体：按 `ZELA_CJK_FONT` 环境变量 + 各平台常见路径查找，
    /// 找到即注入 `Proportional` / `Monospace` 字体族，返回实际加载的路径。
    ///
    /// 为什么在 UI 层：字体是**宿主平台**的资产（字体文件路径与任何渲染后端无关），
    /// 且注入对象是 `egui::Context` —— 本 crate 的 host 是唯一允许出现 egui 类型的地方。
    ///
    /// 跨平台：候选列表覆盖 Windows / macOS / 常见 Linux 发行版；还可由环境变量指定。
    /// 找不到时返回 `None` 且**不 panic、不打日志**（提示文案由调用方决定）：中文会退化为方框。
    pub fn install_cjk_font(ctx: &egui::Context) -> Option<std::path::PathBuf> {
        let path = cjk_font_candidates().into_iter().find(|p| p.is_file())?;
        let data = std::fs::read(&path).ok()?;

        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("cjk".to_owned(), Arc::new(egui::FontData::from_owned(data)));
        // 比例字体：插到**最前**（默认正文字体）；等宽字体：追加（保留原有 ASCII 等宽字形）。
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "cjk".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("cjk".to_owned());
        ctx.set_fonts(fonts);
        Some(path)
    }

    // ─── 帧末管理 / 可观测性 ───────────────────────────────────────────────

    /// 本帧的**绘制调用数**（2D 命令 + 3D mesh/line 条数）。
    ///
    /// 逻辑已在 `zela-render::frame::draw_call_count`（S0 归位）；本方法只是
    /// egui 宿主的转发入口。
    pub fn draw_call_count(res: &Resources) -> usize {
        zela_render::frame::draw_call_count(res)
    }

    /// 帧末清理：2D 队列内容已被 `present` 消费，清空以便下帧重新录制。
    ///
    /// 逻辑已在 `zela-render::frame::end_frame`（S0 归位）；本方法只是转发。
    pub fn end_frame(res: &mut Resources) {
        zela_render::frame::end_frame(res);
    }

    // ─── 场景 → UI：视图声明 ───────────────────────────────────────────────

    /// 从场景状态构建本帧视图声明（纯数据，无 egui 类型）。
    ///
    /// 逻辑已在 `zela-render::frame::build_scene_view`（S0 归位）；本方法只负责
    /// 把 UI 配置翻译成 [`FrameOptions`](zela_render::FrameOptions)。
    pub fn build_scene_view(world: &hecs::World, res: &Resources) -> SceneView {
        let default = UiConfig::default();
        let config = res.get::<UiConfig>().unwrap_or(&default);
        zela_render::frame::build_scene_view(
            &WorldView::new(world),
            res,
            zela_render::FrameOptions::from_ui_config(config),
        )
    }

    // ─── UI → 场景：原始输入翻译 ───────────────────────────────────────────

    /// 把 egui 原生输入翻译为相机语义命令与视口拾取事件。
    ///
    /// **CH-a**：换算本身已搬进 `ze_scene_core::domain::collect_events`（纯函数、
    /// 零 egui 类型）；这里只把 `egui::InputState` 读成 [`WindowInput`]，再交给
    /// 同一个函数 —— 真人与 Debug / RPC 从此走同一条 `WindowInput → SceneEvent`。
    ///
    /// 视口交互路由到 SceneView 中第一个 2D 图层（B 阶段单视口）；
    /// 操作目标由 `target` 决定（K3，见 [`ViewportCamera`]）。
    pub fn collect_events(
        ui: &egui::Ui,
        view: &SceneView,
        target: ViewportCamera,
        viewport: Rect2D,
    ) -> Vec<SceneEvent> {
        let context = ze_scene_core::domain::SceneInputContext::from_view(view, target);
        let input = Self::window_input(ui, viewport);
        ze_scene_core::domain::collect_events(&input, &context)
    }

    /// egui 输入 → [`WindowInput`]（纯读取，不做任何场景换算）。
    ///
    /// 坐标语义：egui 的指针位置已是**逻辑点**，与 `viewport` 同一坐标系 ⇒
    /// `scale_factor` 只记录、不参与换算（否则 2× DPI 落点会漂）。
    pub fn window_input(ui: &egui::Ui, viewport: Rect2D) -> ze_scene_core::domain::WindowInput {
        let ctx = ui.ctx();
        let typing = ctx.egui_wants_keyboard_input();
        let scale_factor = ctx.pixels_per_point();
        ui.input(|i| {
            let mut keys = std::collections::BTreeSet::new();
            for (egui_key, key) in [
                (egui::Key::W, ze_scene_core::domain::Key::W),
                (egui::Key::A, ze_scene_core::domain::Key::A),
                (egui::Key::S, ze_scene_core::domain::Key::S),
                (egui::Key::D, ze_scene_core::domain::Key::D),
                (egui::Key::Q, ze_scene_core::domain::Key::Q),
                (egui::Key::E, ze_scene_core::domain::Key::E),
            ] {
                if i.key_down(egui_key) {
                    keys.insert(key);
                }
            }
            let screen_size = i.raw.screen_rect.map(|rect| rect.size()).unwrap_or_default();
            ze_scene_core::domain::WindowInput {
                pointer: ze_scene_core::domain::PointerState {
                    position: i
                        .pointer
                        .latest_pos()
                        .map(|p| ze_notion::Vec2::new(p.x, p.y)),
                    hovering: i.pointer.hover_pos().is_some(),
                    delta: ze_notion::Vec2::new(i.pointer.delta().x, i.pointer.delta().y),
                    scroll_y: i.smooth_scroll_delta.y,
                    primary_down: i.pointer.primary_down(),
                    secondary_down: i.pointer.secondary_down(),
                    middle_down: i.pointer.middle_down(),
                    primary_pressed: i.pointer.primary_pressed(),
                    primary_released: i.pointer.primary_released(),
                },
                keys,
                typing,
                stable_dt: i.stable_dt,
                window_size: ze_notion::Vec2::new(screen_size.x, screen_size.y),
                scale_factor,
                viewport,
            }
        })
    }

    // ─── 布局信息 ──────────────────────────────────────────────────────────

    /// 面板布局完成后写入中央视口矩形（原 ViewRectUpdateSystem 的职责）。
    ///
    /// 返回刚写入的矩形：本帧的输入换算（相机命令是否落在视口内）直接用它。
    pub fn update_view_rect(ui: &egui::Ui, res: &mut Resources) -> Rect2D {
        // egui::Rect → 理念层 `Rect2D` 的就地转换（后端转换由使用方负责）。
        let egui_rect = ui.available_rect_before_wrap();
        let rect = Rect2D::new(
            (egui_rect.min.x, egui_rect.min.y).into(),
            (egui_rect.max.x, egui_rect.max.y).into(),
        );
        if let Some(vr) = res.get_mut::<ViewRect>() {
            vr.rect = rect;
        }
        rect
    }

    // ─── 内容提交 ──────────────────────────────────────────────────────────

    /// 把本帧录制的内容提交到 egui painter（原 RenderSubmitSystem /
    /// RenderSubmit3DSystem / GridBuild 背景矩形的职责）。在 `Scheduler::run_frame`
    /// 之后调用。
    ///
    /// **顺序语义（S3-2）**：`SceneView::layers` 的**索引序 = 合成序**（先 = 下）。
    /// `UiConfig::flip_layer_order` 仅用于验收时翻转一次顺序（不改场景声明）。
    ///
    /// **单一回调（S3-2 修复）**：`egui_wgpu::CallbackResources` 是全局 TypeMap，
    /// 若每个图层各注册一个回调，它们会互相覆盖同一份合成资源
    /// （见 [`zela_render_backends::scene_callback`] 头部说明）。因此这里
    /// 只注册**一个** [`SceneCompositorCallback`]，由它一次性渲染并合成**全部**
    /// 图层（S4.1 后 2D / 3D / 内容纹理**全部**走 GPU，不再有 CPU 回放分支）。
    ///
    /// - 背景：先铺 `SceneView::background`（原 GridBuild 的 `-1000` 全屏矩形职责）；
    /// - 全部图层：进唯一回调（离屏 + 合成，按有效序）。
    pub fn present(ui: &mut egui::Ui, view: &SceneView, res: &mut Resources) {
        let view_rect = res
            .get::<ViewRect>()
            .map(|v| v.rect)
            .unwrap_or_else(|| ViewRect::default().rect);
        let egui_rect = egui::Rect::from_min_max(
            egui::pos2(view_rect.min.x, view_rect.min.y),
            egui::pos2(view_rect.max.x, view_rect.max.y),
        );

        let painter = ui.painter_at(egui_rect);

        // ── 背景铺底（图层全部在它之上） ──
        let bg = view.background;
        painter.rect_filled(
            egui_rect,
            0.0,
            egui::Color32::from_rgba_unmultiplied(bg.r, bg.g, bg.b, bg.a),
        );

        let Some(callback) = Self::build_render_callback(view, res) else {
            return;
        };
        painter.add(egui::epaint::Shape::Callback(
            egui_wgpu::Callback::new_paint_callback(egui_rect, callback),
        ));
    }

    /// 从本帧视图与资源构造**唯一**合成回调（不含任何 egui 调用）。
    ///
    /// 逻辑分两层（S0 归位）：
    /// - `zela_render::frame::build_frame_inputs`：视图 + 资源 → 后端中立输入；
    /// - `zela_render_backends::frame::build_scene_callback`：输入 → 具体回调。
    ///
    /// GUI（[`Self::present`]）与 headless 导出共用这条构造，帧体因此不可能分叉。
    /// 返回 `None` 表示本帧没有图层（只画背景）。
    pub fn build_render_callback(
        view: &SceneView,
        res: &Resources,
    ) -> Option<SceneCompositorCallback> {
        let default = UiConfig::default();
        let config = res.get::<UiConfig>().unwrap_or(&default);
        let options = zela_render::FrameOptions::from_ui_config(config);
        let input = zela_render::frame::build_frame_inputs(view, res, options)?;
        zela_render_backends::frame::build_scene_callback(input)
    }
}

/// CJK 字体候选路径：环境变量 `ZELA_CJK_FONT` **优先**，其次各平台常见内置路径。
///
/// 只做"列候选"，是否存在由调用方过滤 —— 这样非 Windows 平台也能跑（无 panic / 无方框提示之外的行为）。
/// 注意：下面的 `C:\Windows\Fonts\...` **只在 Windows 上存在**；在 Linux / macOS 上
/// 这些候选会被"文件不存在"过滤掉，不是错误路径。
fn cjk_font_candidates() -> Vec<std::path::PathBuf> {
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(custom) = std::env::var("ZELA_CJK_FONT") {
        if !custom.trim().is_empty() {
            paths.push(std::path::PathBuf::from(custom));
        }
    }
    paths.extend(
        [
            // Windows：微软雅黑
            r"C:\Windows\Fonts\msyh.ttc",
            // macOS：苹方
            "/System/Library/Fonts/PingFang.ttc",
            // Linux：Noto CJK（Debian/Ubuntu 与 Fedora/Arch 的常见落点）
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            // Linux：文泉驿正黑（轻量发行版常见）
            "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        ]
        .map(std::path::PathBuf::from),
    );
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_scene_core::{Camera2DHandle, Camera3D, Framing2D, Projection};
    use zela_scene_ecs::contract::WorldScene;
    use zela_scene_ecs::domain::{ContentHandle, LayerBlend};
    use zela_schedule::resource::Resources;

    /// 建含 3D 相机的世界（3D 图层来源）。
    fn spawn_3d_camera(world: &mut hecs::World) {
        world.spawn((Camera3D::new(
            ze_notion::Vec3::new(0.0, 0.0, 5.0),
            ze_notion::Vec3::ZERO,
            ze_notion::Vec3::Y,
            Projection::perspective(60.0f32.to_radians(), 16.0 / 9.0, 0.1, 100.0),
        ),));
    }

    /// 建含 2D 相机对象的场景 + 其句柄 Resource（P1：相机是场景对象）。
    fn spawn_2d_camera(world: &mut hecs::World) -> Camera2DHandle {
        let mut scene = WorldScene::new(world);
        ze_scene_core::spawn_camera2d(&mut scene, Framing2D::default()).expect("Framing2D 已登记")
    }

    /// headless：无任何 egui 上下文即可构建 SceneView。
    #[test]
    fn build_scene_view_declares_2d_and_3d_layers() {
        let mut world = hecs::World::new();
        spawn_3d_camera(&mut world);
        let handle = spawn_2d_camera(&mut world);

        let mut res = Resources::new();
        res.insert(handle);
        res.insert(ViewRect::default());

        let view = EguiSceneHost::build_scene_view(&world, &res);
        assert_eq!(view.layers.len(), 2);
        assert!(matches!(view.layers[0].kind, LayerKind::Viewport2D { .. }));
        assert!(matches!(view.layers[1].kind, LayerKind::Viewport3D { .. }));
        assert_eq!(view.layers[0].content, ContentHandle::OPAQUE);
        // S3-2：两个图层都渲染到透明底离屏纹理，必须声明为 Alpha
        // （用默认的 Opaque/REPLACE 会把透明像素写进主 pass，抹掉背景）。
        for l in &view.layers {
            assert_eq!(l.blend, LayerBlend::Alpha);
            assert_eq!(l.opacity, 1.0);
            assert_eq!(l.clamped_opacity(), 1.0);
        }
        assert_eq!(view.background, zela_scene_ecs::domain::DEFAULT_BACKGROUND);
    }

    /// S3-2 修复：`view.layers` 内 `LayerId` 必须**两两唯一**
    /// （此前是常量式 —— 所有层同一个值会让按层寻址的方案退化成冲突）。
    #[test]
    fn build_scene_view_assigns_unique_layer_ids() {
        let mut world = hecs::World::new();
        spawn_3d_camera(&mut world);
        let handle = spawn_2d_camera(&mut world);

        let mut res = Resources::new();
        res.insert(handle);
        res.insert(ViewRect::default());

        let view = EguiSceneHost::build_scene_view(&world, &res);
        assert_eq!(view.layers.len(), 2);
        for (i, a) in view.layers.iter().enumerate() {
            for b in view.layers.iter().skip(i + 1) {
                assert_ne!(a.id, b.id, "图层 LayerId 必须两两不相等");
            }
        }
    }

    /// 无 3D 相机实体时只声明 2D 图层；无 2D 相机句柄（或句柄失效）时无 2D 图层。
    #[test]
    fn build_scene_view_skips_missing_layers() {
        let mut world = hecs::World::new();
        let handle = spawn_2d_camera(&mut world);

        let mut res = Resources::new();
        res.insert(handle);
        let view = EguiSceneHost::build_scene_view(&world, &res);
        assert_eq!(view.layers.len(), 1);
        assert!(matches!(view.layers[0].kind, LayerKind::Viewport2D { .. }));

        // 空资源 ⇒ 无句柄 ⇒ 无图层。
        let res_empty = Resources::new();
        let view = EguiSceneHost::build_scene_view(&world, &res_empty);
        assert!(view.layers.is_empty());

        // 句柄存在但指向的场景对象已消失（空世界）⇒ 同样无 2D 图层。
        let empty_world = hecs::World::new();
        let mut res_stale = Resources::new();
        res_stale.insert(Camera2DHandle::default());
        assert!(
            EguiSceneHost::build_scene_view(&empty_world, &res_stale)
                .layers
                .is_empty()
        );
    }

    /// 2D 图层相机 = 场景里的取景参数（`Framing2D`）绑定 `ViewRect` —— P1-2 的
    /// "相机与渲染目标 rect 解绑"在场景 → UI 方向的落点。
    #[test]
    fn scene_view_2d_camera_binds_framing_to_view_rect() {
        let mut world = hecs::World::new();
        let mut scene = WorldScene::new(&mut world);
        let handle =
            ze_scene_core::spawn_camera2d(&mut scene, Framing2D::new((2.0, -3.0).into(), 55.0))
                .expect("Framing2D 已登记");

        let mut res = Resources::new();
        res.insert(handle);
        res.insert(ViewRect::default());

        let rect = res.get::<ViewRect>().unwrap().rect;
        let view = EguiSceneHost::build_scene_view(&world, &res);
        let LayerKind::Viewport2D { camera } = view.layers[0].kind else {
            panic!("expected 2D layer");
        };
        assert!((camera.center.x - 2.0).abs() < 1e-5);
        assert!((camera.center.y + 3.0).abs() < 1e-5);
        assert!((camera.scale - 55.0).abs() < 1e-5);
        // rect 来自渲染目标（ViewRect），不是相机自带。
        assert_eq!(camera.rect, rect);
    }

    /// 图层开关（= 关掉对应相机）：关掉的图层**不再出现在声明里**。
    #[test]
    fn build_scene_view_respects_layer_toggles() {
        let mut world = hecs::World::new();
        spawn_3d_camera(&mut world);
        let handle = spawn_2d_camera(&mut world);

        let res_with = |show_2d: bool, show_3d: bool| {
            let mut res = Resources::new();
            res.insert(handle);
            res.insert(ViewRect::default());
            res.insert(UiConfig {
                show_2d,
                show_3d,
                ..UiConfig::default()
            });
            res
        };

        // 默认两图都在（`UiConfig::default()` 两个开关为 true）。
        assert_eq!(
            EguiSceneHost::build_scene_view(&world, &res_with(true, true))
                .layers
                .len(),
            2
        );

        // 关 2D ⇒ 只剩 3D。
        let v = EguiSceneHost::build_scene_view(&world, &res_with(false, true));
        assert_eq!(v.layers.len(), 1);
        assert!(matches!(v.layers[0].kind, LayerKind::Viewport3D { .. }));

        // 关 3D ⇒ 只剩 2D。
        let v = EguiSceneHost::build_scene_view(&world, &res_with(true, false));
        assert_eq!(v.layers.len(), 1);
        assert!(matches!(v.layers[0].kind, LayerKind::Viewport2D { .. }));

        // 两个都关 ⇒ 无图层（`present` 只铺背景）。
        assert!(
            EguiSceneHost::build_scene_view(&world, &res_with(false, false))
                .layers
                .is_empty()
        );
    }
}
