// zela-gui — GUI 可执行壳。
//
// 依赖方向（S0 后）：`zela-app` 与 `zela-egui` 是**同级**下层；本 crate 在最上，
// 负责把它们撮合成 eframe::App。这样 `zela-app` 可以彻底不依赖 UI：
// headless 导出、脚本装配、录像都能独立编译。
//
// # 本轮（把矢量后端接进 GUI）
//
// 1. **作品可切换**：顶部下拉列出 `zela_app::demos` 的全部内置作品（默认播第一件），
//    另可"打开脚本…"载入任意 `.lua/.js/.py`；切换 = **重新装配 Runtime**
//    （场景在装配期一次性建好，运行态重建要走完整装配，见 `SceneChoice` 的说明）。
// 2. **后端可切换**：Auto（按帧路由，被拒回退 Wgpu）/ Vector / Wgpu。路由用
//    `zela_render_backends::routing`（与 headless 同一实现），决策按作品固定。
// 3. **矢量帧的呈现**：帧体（`SceneFrame::begin / finish`）仍然只有一份，
//    只有"呈现"这一步分叉 —— 矢量走离屏 CPU 读回 + egui 纹理，wgpu 走原合成回调。
//    读回一帧 ≈ 2MB（800×600），只为 GUI 演示的实时性服务；导出 / 对拍仍走 headless 循环。

use std::path::PathBuf;
use std::sync::Arc;

use eframe::App as EframeApp;
use eframe::egui as gui;

use ze_pod_core::Pod;
use zela_app::demos::Demo;
use zela_app::{
    Runtime, SceneChoice, ScriptLanguage, build_scene_runtime, build_script_runtime,
    language_from_path,
};
use zela_egui::{EguiSceneHost, FrameBegin, FrameProgress, SceneFrame, install_visuals};
use zela_playback::vector_frame::VectorFrame;
use zela_render_backends::registry::{AdapterFacts, BackendKind};
use zela_render_backends::routing::{self, LoopBackend};
use zela_render_backends::{RenderContext, RoutingDecision, install_standard_backends};
use zela_scene_ecs::DriverPod;
use zela_scene_ecs::domain::{UiConfig, ViewportCamera};
use zela_scene_ecs::Result;
use zela_scene_ecs::errors::Error;

/// 启动时的初始选中项。
#[derive(Debug, Clone)]
pub enum Initial {
    /// 内置作品列表的第一件（默认）。
    First,
    /// 按 id 选中某件内置作品（`--script` ⇒ `storyboard`；`--work <id>` 直接指定）。
    Demo(String),
    /// 启动参数指定的内置场景（`--demo` / `--python`）。
    Choice(SceneChoice),
}

/// 下拉里的一项：内置作品 / 内置场景 / 用户打开的文件。
struct Entry {
    title: String,
    source: EntrySource,
}

#[derive(Clone)]
enum EntrySource {
    Demo(&'static Demo),
    Choice(SceneChoice),
    File {
        source: String,
        language: ScriptLanguage,
    },
}

/// GUI 应用壳：持有 Runtime 与每帧帧体。
pub struct App {
    /// 当前作品的完整运行时（切作品时整体替换）。
    runtime: Runtime,
    /// 事件订阅方：只观测 `FrameFinished`（进度条 / health overlay）。
    progress: Arc<FrameProgress>,
    render_ctx: RenderContext,
    /// 每帧帧体（墙钟 / health / 呈现）—— UI 层实现，壳只委托。
    frame: SceneFrame,
    /// 可选作品（全部内置 demo + 启动参数场景 + 打开过的文件）。
    entries: Vec<Entry>,
    selected: usize,
    /// 本作品的后端偏好（Auto 按帧路由）。
    backend: LoopBackend,
    /// 作品级后端决策（第一帧决定后固定；换作品 / 换偏好时清空）。
    pinned: Option<RoutingDecision>,
    /// 矢量帧的上传纹理（尺寸变化时重建）。
    vector_texture: Option<gui::TextureHandle>,
    vector_size: (u32, u32),
    /// 状态行：后端决策 / 渲染失败原因。
    status: String,
    /// 载入失败（红字显示；不覆盖正在播放的作品）。
    error: Option<String>,
    /// "打开脚本…"的迷你文件浏览器。
    picker: Option<ScriptPicker>,
    /// 上一帧的矢量快照：**纯 2D 且逐项相同**时直接复用纹理（静止作品不再重渲）。
    last_vector: Option<LastVector>,
    /// `ZELA_GUI_TIMING=1` 时的帧耗时统计（默认关：不付任何代价）。
    timing: Option<GuiTiming>,
}

/// 复用上一帧所需的全部条件（快照 + 画幅 + 背景）。
struct LastVector {
    frame: VectorFrame,
    background: [f32; 4],
    size: (u32, u32),
}

/// 帧耗时统计（诊断用；每 60 帧打一行）。
struct GuiTiming {
    frames: u32,
    frame_ms: f64,
    vector_ms: f64,
    /// 矢量路径的分段：快照 / 渲染 / 上传。
    snapshot_ms: f64,
    render_ms: f64,
    upload_ms: f64,
}

impl App {
    /// 默认：作品列表含全部 demo，播第一件。
    pub fn new(ctx: &eframe::CreationContext) -> Result<Self> {
        Self::launch(ctx, Initial::First)
    }

    /// 启动参数指定的初始项。
    pub fn with_initial(ctx: &eframe::CreationContext, initial: Initial) -> Result<Self> {
        Self::launch(ctx, initial)
    }

    /// 兼容旧入口：按 [`SceneChoice`] 启动（等价于 [`Initial::Choice`]）。
    pub fn new_with_scene(ctx: &eframe::CreationContext, choice: SceneChoice) -> Result<Self> {
        Self::launch(ctx, Initial::Choice(choice))
    }

    fn launch(ctx: &eframe::CreationContext, initial: Initial) -> Result<Self> {
        // 宿主字体（平台相关）：候选路径 + `ZELA_CJK_FONT`；找不到只提示、不中止。
        if EguiSceneHost::install_cjk_font(&ctx.egui_ctx).is_none() {
            eprintln!(
                "警告：未找到可用的中文字体（可用环境变量 ZELA_CJK_FONT 指定字体路径），中文可能显示为方框。"
            );
        }
        install_visuals(&ctx.egui_ctx);

        // 渲染环境（后端 + 合成器 + RenderContext）由 backends 侧的"标准安装"负责。
        let render_ctx = {
            let rs = ctx
                .wgpu_render_state
                .clone()
                .ok_or(Error::InitializationError(
                    "Wgpu render state not available".to_string(),
                ))?;
            install_standard_backends(&rs)
        };

        let entries = entries();
        let selected = initial_index(&entries, initial);
        let (title, source) = (entries[selected].title.clone(), entries[selected].source.clone());
        let runtime = load(render_ctx.clone(), &source)
            .map_err(|why| Error::InitializationError(format!("载入「{title}」失败：{why}")))?;
        install_camera_control(&runtime);

        let progress = Arc::new(FrameProgress::new());
        attach_progress(&runtime, Arc::clone(&progress))?;
        let frame = SceneFrame::new(Arc::clone(&runtime.driver));
        Ok(Self {
            runtime,
            progress,
            render_ctx,
            frame,
            entries,
            selected,
            backend: LoopBackend::Auto,
            pinned: None,
            vector_texture: None,
            vector_size: (0, 0),
            status: String::new(),
            error: None,
            picker: None,
            last_vector: None,
            timing: std::env::var_os("ZELA_GUI_TIMING").map(|_| GuiTiming {
                frames: 0,
                frame_ms: 0.0,
                vector_ms: 0.0,
                snapshot_ms: 0.0,
                render_ms: 0.0,
                upload_ms: 0.0,
            }),
        })
    }

    /// 本帧渲染上下文。
    pub fn render_context(&self) -> RenderContext {
        self.render_ctx.clone()
    }

    /// 场景推进器句柄（测试 / headless 与 GUI 共用同一条命令路径）。
    pub fn driver(&self) -> Arc<DriverPod> {
        Arc::clone(&self.runtime.driver)
    }

    // ─── 作品切换 ─────────────────────────────────────────────────────────

    /// 重新装配当前选中项；失败时**保留**正在播放的运行时并显示原因。
    fn reload(&mut self) {
        let entry = self.entries[self.selected].source.clone();
        match load(self.render_ctx.clone(), &entry) {
            Ok(runtime) => {
                install_camera_control(&runtime);
                let progress = Arc::new(FrameProgress::new());
                if let Err(why) = attach_progress(&runtime, Arc::clone(&progress)) {
                    self.error = Some(format!("Driver 事件订阅失败：{why}"));
                } else {
                    self.error = None;
                }
                self.frame = SceneFrame::new(Arc::clone(&runtime.driver));
                self.progress = progress;
                self.runtime = runtime;
                // 作品级决策 / 纹理都随运行时一起失效。
                self.pinned = None;
                self.vector_texture = None;
                self.vector_size = (0, 0);
                self.last_vector = None;
                self.status.clear();
            }
            Err(why) => self.error = Some(why),
        }
    }

    /// 打开磁盘上的脚本：读入 → 追加进列表 → 立即载入。
    fn open_file(&mut self, path: PathBuf) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let read = std::fs::read_to_string(&path)
            .map_err(|why| format!("读取 {} 失败：{why}", path.display()));
        let language = language_from_path(&path).map_err(|why| why.to_string());
        match (read, language) {
            (Ok(source), Ok(language)) => {
                self.entries.push(Entry {
                    title: format!("文件 · {name}"),
                    source: EntrySource::File { source, language },
                });
                self.selected = self.entries.len() - 1;
                self.reload();
            }
            (Err(why), _) | (_, Err(why)) => self.error = Some(why),
        }
    }

    // ─── 每帧 ─────────────────────────────────────────────────────────────

    /// 顶上工具条：作品 / 后端 / 重新加载 / 打开脚本 / 状态。返回是否请求重载。
    fn toolbar(&mut self, ui: &mut gui::Ui) -> bool {
        let mut reload = false;
        ui.horizontal_wrapped(|ui| {
            ui.label("作品");
            let titles: Vec<String> = self.entries.iter().map(|e| e.title.clone()).collect();
            let mut selected = self.selected;
            gui::ComboBox::from_id_salt("zela-work")
                .width(260.0)
                .selected_text(titles[selected].clone())
                .show_ui(ui, |ui| {
                    for (index, title) in titles.iter().enumerate() {
                        ui.selectable_value(&mut selected, index, title);
                    }
                });
            if selected != self.selected {
                self.selected = selected;
                reload = true;
            }

            ui.separator();
            ui.label("后端");
            let mut backend = self.backend;
            gui::ComboBox::from_id_salt("zela-backend")
                .selected_text(backend_label(backend))
                .show_ui(ui, |ui| {
                    for (candidate, label) in [
                        (LoopBackend::Auto, "Auto（按帧路由，被拒回退）"),
                        (LoopBackend::Vector, "Vector（偏好，被拒回退）"),
                        (LoopBackend::Wgpu, "Wgpu"),
                    ] {
                        ui.selectable_value(&mut backend, candidate, label);
                    }
                });
            if backend != self.backend {
                self.backend = backend;
                self.pinned = None;
                self.status.clear();
            }

            if ui.button("重新加载").clicked() {
                reload = true;
            }
            if ui.button("打开脚本…").clicked() {
                self.picker = Some(ScriptPicker::new());
            }

            // 视口操作目标（与左侧控制面板里的单选是**同一份** `UiConfig`）：
            // 放一份在工具条上，面板收起 / 找不到时也能切到 3D 第一人称。
            ui.separator();
            ui.label("相机");
            let mut camera = self.runtime.driver.with_scheduler(|scheduler| {
                scheduler
                    .resources()
                    .get::<UiConfig>()
                    .map(|config| config.viewport_camera)
                    .unwrap_or_default()
            });
            let before = camera;
            gui::ComboBox::from_id_salt("zela-camera")
                .selected_text(camera_label(camera))
                .show_ui(ui, |ui| {
                    for (mode, label) in [
                        (ViewportCamera::TwoD, "2D（平移/缩放）"),
                        (ViewportCamera::ThreeDOrbit, "3D 轨道"),
                        (ViewportCamera::ThreeDFps, "3D 第一人称"),
                    ] {
                        ui.selectable_value(&mut camera, mode, label);
                    }
                });
            if camera != before {
                self.runtime.driver.with_scheduler_mut(|scheduler| {
                    if let Some(config) = scheduler.resources_mut().get_mut::<UiConfig>() {
                        config.viewport_camera = camera;
                    }
                });
                self.status = format!("视口操作：{}", camera_label(camera));
            }

            ui.separator();
            if self.status.is_empty() {
                ui.weak("后端：等待首帧决策…");
            } else {
                ui.add(gui::Label::new(&self.status).truncate());
            }
        });
        reload
    }

    /// 矢量呈现：离屏渲染 → 上传纹理 → 画进视口。
    ///
    /// 返回 `false` ⇒ 本帧应走 wgpu 呈现（显式 Wgpu / 路由选了 Wgpu / 渲染失败）。
    fn paint_vector(&mut self, ui: &mut gui::Ui, begin: &FrameBegin) -> bool {
        if self.backend == LoopBackend::Wgpu {
            return false;
        }
        // 已判定本作品走 wgpu（作品级决策固定）⇒ 不再取快照：
        // `VectorFrame::take` 会深拷贝本帧的全部 2D 命令，白付每帧的代价。
        if self
            .pinned
            .as_ref()
            .is_some_and(|decision| decision.backend != BackendKind::Vector)
        {
            return false;
        }
        let started = std::time::Instant::now();
        // 快照必须在本帧推进之后取（与 headless 循环同一时点）。
        let snapshot = match VectorFrame::take(&self.runtime) {
            Ok(snapshot) => snapshot,
            Err(why) => {
                self.status = format!("矢量快照失败：{why}");
                return false;
            }
        };
        let snapshot_ms = started.elapsed().as_secs_f64() * 1000.0;

        let decision = match &self.pinned {
            Some(pinned) => pinned.clone(),
            None => match routing::route(
                self.backend,
                snapshot.content(),
                self.render_ctx.adapter().map(AdapterFacts::of),
            ) {
                Ok(decision) => decision,
                Err(why) => {
                    self.status = format!("后端路由失败：{why}");
                    return false;
                }
            },
        };
        if self.pinned.is_none() {
            // 与 headless 的 `[backend] …` 同一口径：设备上排查时终端也能看到。
            eprintln!("[gui] 后端：{}", decision.report());
            self.status = format!("后端：{}", decision.report());
            self.pinned = Some(decision.clone());
        }
        if decision.backend != BackendKind::Vector {
            return false;
        }

        // **分辨率 = 视口物理像素**：每帧按视口大小重算（不是固定分辨率）。
        // 2D 记录是布局点 ⇒ 用 `render_at` 的 `scale` 换算（否则 HiDPI 下内容缩在左上角）。
        let scale = ui.ctx().pixels_per_point();
        let width = ((begin.viewport.width() * scale).round() as u32).max(1);
        let height = ((begin.viewport.height() * scale).round() as u32).max(1);
        let background = begin.view.background;
        let background = [
            background.r() as f32 / 255.0,
            background.g() as f32 / 255.0,
            background.b() as f32 / 255.0,
            background.a() as f32 / 255.0,
        ];
        let params = zela_playback::vector_params(width, height);
        // **静止画面直接复用上一帧**：纯 2D 帧的快照逐项相同（画幅 / 背景也没变）⇒
        // 不再渲染、不再上传。静止作品（duration = 0，如 surface_matrix）因此近乎零成本；
        // 动画作品（快照变了）与 3D 帧（快照不含网格）都照常重渲。
        let reusable = self.last_vector.as_ref().is_some_and(|last| {
            last.size == (width, height)
                && last.background == background
                && last.frame.same_2d_frame(&snapshot)
        });
        if reusable {
            if let Some(texture) = &self.vector_texture {
                ui.painter().image(
                    texture.id(),
                    begin.viewport,
                    gui::Rect::from_min_max(gui::pos2(0.0, 0.0), gui::pos2(1.0, 1.0)),
                    gui::Color32::WHITE,
                );
            }
            if let Some(timing) = &mut self.timing {
                timing.snapshot_ms += snapshot_ms;
                timing.vector_ms += started.elapsed().as_secs_f64() * 1000.0;
            }
            return true;
        }
        if self.vector_size == (0, 0) {
            eprintln!(
                "[gui] 矢量画布 {width}×{height}px（视口 {:.0}×{:.0}pt，ppp {scale}）",
                begin.viewport.width(),
                begin.viewport.height()
            );
        }
        let render_start = std::time::Instant::now();
        let rgba = match snapshot.render_at(&self.runtime, width, height, scale, &params, background)
        {
            Ok(rgba) => rgba,
            Err(why) => {
                self.status = format!("矢量渲染失败：{why}（本帧回退 Wgpu）");
                return false;
            }
        };
        let render_ms = render_start.elapsed().as_secs_f64() * 1000.0;

        let upload_start = std::time::Instant::now();
        self.last_vector = Some(LastVector {
            frame: snapshot,
            background,
            size: (width, height),
        });
        let image = gui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
        match &mut self.vector_texture {
            Some(texture) if self.vector_size == (width, height) => {
                texture.set(image, gui::TextureOptions::LINEAR);
            }
            _ => {
                self.vector_texture = Some(ui.ctx().load_texture(
                    "zela-vector-frame",
                    image,
                    gui::TextureOptions::LINEAR,
                ));
                self.vector_size = (width, height);
            }
        }
        if let Some(texture) = &self.vector_texture {
            ui.painter().image(
                texture.id(),
                begin.viewport,
                gui::Rect::from_min_max(gui::pos2(0.0, 0.0), gui::pos2(1.0, 1.0)),
                gui::Color32::WHITE,
            );
        }
        if let Some(timing) = &mut self.timing {
            timing.snapshot_ms += snapshot_ms;
            timing.render_ms += render_ms;
            timing.upload_ms += upload_start.elapsed().as_secs_f64() * 1000.0;
            timing.vector_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
        true
    }
}

impl EframeApp for App {
    fn ui(&mut self, ui: &mut gui::Ui, _frame: &mut eframe::Frame) {
        let started = std::time::Instant::now();
        // 文件浏览器（独立窗口；选中即载入）。
        let picked = self.picker.as_mut().map(|picker| picker.show(ui.ctx()));
        if let Some((chosen, keep_open)) = picked {
            if let Some(path) = chosen {
                self.open_file(path);
            }
            if !keep_open {
                self.picker = None;
            }
        }

        if self.toolbar(ui) {
            self.reload();
        }
        if let Some(error) = &self.error {
            ui.colored_label(gui::Color32::LIGHT_RED, error);
        }

        let progress = (self.progress.frames(), self.progress.time());
        // 帧序只有一份：begin（推进）→ 呈现（wgpu / 矢量二选一）→ finish（清理）。
        let begin = self.frame.begin(ui);
        if !self.paint_vector(ui, &begin) {
            self.frame.present_wgpu(ui, &begin);
        }
        self.frame.finish(ui, &begin, progress);

        // 诊断（`ZELA_GUI_TIMING=1`）：60 帧均值 = UI + 场景 + 呈现。
        if let Some(timing) = &mut self.timing {
            timing.frames += 1;
            timing.frame_ms += started.elapsed().as_secs_f64() * 1000.0;
            if timing.frames >= 60 {
                let n = timing.frames as f64;
                eprintln!(
                    "[gui] 60 帧均值：帧 {:.1} ms（矢量 {:.1} ms = 快照 {:.1} + 渲染 {:.1} + 上传 {:.1}；视口 {:.0}×{:.0}pt）",
                    timing.frame_ms / n,
                    timing.vector_ms / n,
                    timing.snapshot_ms / n,
                    timing.render_ms / n,
                    timing.upload_ms / n,
                    begin.viewport.width(),
                    begin.viewport.height()
                );
                timing.frames = 0;
                timing.frame_ms = 0.0;
                timing.vector_ms = 0.0;
                timing.snapshot_ms = 0.0;
                timing.render_ms = 0.0;
                timing.upload_ms = 0.0;
            }
        }
    }
}

fn backend_label(backend: LoopBackend) -> &'static str {
    match backend {
        LoopBackend::Auto => "Auto（按帧路由，被拒回退）",
        LoopBackend::AutoFrame => "AutoFrame（诊断）",
        LoopBackend::Vector => "Vector（偏好，被拒回退）",
        LoopBackend::Wgpu => "Wgpu",
    }
}

/// 视口操作目标的显示名（工具条与左侧控制面板共用同一份 `UiConfig`）。
fn camera_label(camera: ViewportCamera) -> &'static str {
    match camera {
        ViewportCamera::TwoD => "2D（平移/缩放）",
        ViewportCamera::ThreeDOrbit => "3D 轨道",
        ViewportCamera::ThreeDFps => "3D 第一人称",
    }
}

/// 作品列表：全部内置 demo，再补启动参数用的两个内置场景。
fn entries() -> Vec<Entry> {
    let mut entries: Vec<Entry> = zela_app::demos::DEMOS
        .iter()
        .map(|demo| Entry {
            title: format!("{} · {}", demo.id, demo.title),
            source: EntrySource::Demo(demo),
        })
        .collect();
    entries.push(Entry {
        title: "内置 · 能力 demo（N 线）".to_string(),
        source: EntrySource::Choice(SceneChoice::CapabilityDemo),
    });
    #[cfg(feature = "python")]
    entries.push(Entry {
        title: "内置 · Python 分镜".to_string(),
        source: EntrySource::Choice(SceneChoice::PythonStoryboard),
    });
    entries
}

fn initial_index(entries: &[Entry], initial: Initial) -> usize {
    match initial {
        Initial::First => 0,
        Initial::Demo(id) => entries
            .iter()
            .position(|entry| matches!(&entry.source, EntrySource::Demo(demo) if demo.id == id.as_str()))
            .unwrap_or(0),
        Initial::Choice(choice) => entries
            .iter()
            .position(|entry| matches!(&entry.source, EntrySource::Choice(c) if *c == choice))
            .unwrap_or(0),
    }
}

fn load(render_ctx: RenderContext, source: &EntrySource) -> std::result::Result<Runtime, String> {
    match source {
        EntrySource::Demo(demo) => {
            build_script_runtime(render_ctx, demo.source, demo.language).map_err(|why| why.to_string())
        }
        EntrySource::Choice(choice) => {
            build_scene_runtime(render_ctx, *choice).map_err(|why| why.to_string())
        }
        EntrySource::File { source, language } => {
            build_script_runtime(render_ctx, source.clone(), *language).map_err(|why| why.to_string())
        }
    }
}

/// 相机控制按**作品**接线：3D-only 作品 ⇒ 3D 轨道；2D 作品 ⇒ 2D（静态取景则交给用户）。
///
/// 判据只有一份（[`zela_playback::viewport_control`]），这里只负责写回 + 打一行日志。
fn install_camera_control(runtime: &Runtime) {
    // 脚本作品不会在装配期登记 `Camera3DHandle`（headless 不需要）⇒ 宿主补上，
    // 否则 `CameraInputSystem` 找不到可操作的 3D 相机（拖拽/滚轮全无反应）。
    zela_playback::install_camera3d(runtime);
    let control = zela_playback::viewport_control(runtime);
    zela_playback::apply_viewport_control(runtime);
    eprintln!(
        "[gui] 相机控制：{}（free_2d = {}）",
        camera_label(control.camera),
        control.free_2d
    );
}

fn attach_progress(runtime: &Runtime, progress: Arc<FrameProgress>) -> Result<()> {
    let sink: Arc<dyn Pod> = Arc::clone(&progress) as Arc<dyn Pod>;
    runtime
        .driver
        .attach_sink(sink)
        .map_err(|why| Error::InitializationError(format!("Driver 事件订阅失败：{why}")))
}

/// "打开脚本…"的迷你文件浏览器（无第三方对话框依赖）。
struct ScriptPicker {
    dir: PathBuf,
    error: Option<String>,
}

impl ScriptPicker {
    fn new() -> Self {
        // 开发时优先落到仓库脚本目录；安装后则从当前目录起步。
        let dev = PathBuf::from("crates/app/zela-app/scripts");
        let dir = if dev.is_dir() {
            dev
        } else {
            std::env::current_dir().unwrap_or_default()
        };
        Self { dir, error: None }
    }

    /// 返回 `(选中的脚本, 是否保持打开)`。
    fn show(&mut self, ctx: &gui::Context) -> (Option<PathBuf>, bool) {
        let mut chosen = None;
        let mut open = true;
        gui::Window::new("打开脚本")
            .open(&mut open)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("上级目录").clicked() {
                        if let Some(parent) = self.dir.parent() {
                            self.dir = parent.to_path_buf();
                        }
                    }
                    ui.label(self.dir.display().to_string());
                });
                ui.separator();

                let mut dirs = Vec::new();
                let mut files = Vec::new();
                match std::fs::read_dir(&self.dir) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                dirs.push(path);
                            } else if matches!(
                                path.extension().and_then(|ext| ext.to_str()),
                                Some("lua" | "js" | "py")
                            ) {
                                files.push(path);
                            }
                        }
                        self.error = None;
                    }
                    Err(why) => self.error = Some(format!("读取目录失败：{why}")),
                }
                dirs.sort();
                files.sort();

                gui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for dir in &dirs {
                            let name = file_name(dir);
                            if ui.selectable_label(false, format!("[目录] {name}")).clicked() {
                                self.dir = dir.clone();
                            }
                        }
                        for file in &files {
                            if ui.selectable_label(false, file_name(file)).clicked() {
                                chosen = Some(file.clone());
                            }
                        }
                    });

                if let Some(error) = &self.error {
                    ui.colored_label(gui::Color32::LIGHT_RED, error);
                }
            });
        (chosen, open)
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **默认加载所有 demo**：列表含清单里的每一件，默认播第一件（fourier）。
    #[test]
    fn default_entry_is_the_first_demo_and_all_demos_are_listed() {
        let entries = entries();
        let listed = entries
            .iter()
            .filter(|entry| matches!(&entry.source, EntrySource::Demo(_)))
            .count();
        assert_eq!(
            listed,
            zela_app::demos::DEMOS.len(),
            "下拉必须列出全部内置 demo"
        );
        assert_eq!(initial_index(&entries, Initial::First), 0);
        assert!(
            matches!(&entries[0].source, EntrySource::Demo(demo) if demo.id == "fourier"),
            "默认项应是 manifest 的第一件"
        );
    }

    /// 启动参数按 id / 场景定位到对应条目（找不到才回退到 0）。
    #[test]
    fn launch_flags_resolve_to_their_entries() {
        let entries = entries();
        let storyboard = initial_index(&entries, Initial::Demo("storyboard".to_string()));
        assert!(
            matches!(&entries[storyboard].source, EntrySource::Demo(demo) if demo.id == "storyboard")
        );
        #[cfg(feature = "python")]
        {
            let python = initial_index(&entries, Initial::Choice(SceneChoice::PythonStoryboard));
            assert!(matches!(
                &entries[python].source,
                EntrySource::Choice(SceneChoice::PythonStoryboard)
            ));
        }
    }
}
