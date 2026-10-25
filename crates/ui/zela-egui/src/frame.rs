// frame.rs — eframe 宿主的**每帧帧体**（A5：从 `zela-app` 归位到 UI 层）。
//
// app 只保留壳子职责（装配 / 命令提交 / 事件订阅）；"怎么画一帧"属于 UI 层。
// 本模块与 `EguiSceneHost` 的关系：host 负责场景 ↔ egui 的适配，本模块负责帧序
// （构建视图 → 收集输入 → 提交命令 → 呈现 → 可观测性 → 帧末清理）。

use std::sync::Arc;
use std::time::Instant;

use zela_scene_ecs::domain::SceneView;
use zela_scene_ecs::{DriverCommand, DriverPod};

use crate::health::HealthOverlay;
use crate::host::EguiSceneHost;

/// 安装宿主视觉（深色主题 + 提亮弱文字）。
pub fn install_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.weak_text_color = Some(egui::Color32::from_rgb(210, 210, 210));
    visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(200, 200, 200);
    ctx.set_visuals(visuals);
}

/// GUI 窗口参数（app 的 `main` 不再认识 egui 类型）。
pub fn window_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    }
}

/// **帧首**产物：本帧视图声明 + 视口 + 本帧 dt。
///
/// 拆成 `begin / present_wgpu / finish` 的唯一理由是**让呈现方式可选**：
/// wgpu 路径用 [`SceneFrame::present_wgpu`]，矢量路径由宿主自己把离屏像素画进
/// [`Self::viewport`]；两者共用同一条推进 / 输入 / 帧末清理，不形成第二条帧序。
pub struct FrameBegin {
    /// 本帧视图声明（图层序 = 合成序）。
    pub view: SceneView,
    /// 中央视口矩形（egui 点坐标；已写进场景的 `ViewRect` 资源）。
    pub viewport: egui::Rect,
    /// 本帧墙钟 dt（秒）。
    pub dt: f32,
}

/// 每帧帧体状态（墙钟、健康 overlay）。
pub struct SceneFrame {
    driver: Arc<DriverPod>,
    health: HealthOverlay,
    /// 墙钟时间基准：每帧用 `Instant::now() - last_frame` 算 dt，
    /// 不依赖渲染帧间隔（帧率波动、卡顿都不影响 Director 时间流速）。
    last_frame: Instant,
}

impl SceneFrame {
    pub fn new(driver: Arc<DriverPod>) -> Self {
        Self {
            driver,
            health: HealthOverlay::new(),
            last_frame: Instant::now(),
        }
    }

    /// 帧首：构建视图 → 写输入 / 视口 → **推进一帧**。
    pub fn begin(&mut self, ui: &mut egui::Ui) -> FrameBegin {
        // 墙钟 dt：用单调时钟算，不随渲染帧率波动。
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;

        // 1. 场景 → UI：构建本帧视图声明（2D/3D 图层 + 相机）。
        //    相机取上一帧末状态——视口事件换算对该滞后不敏感。
        let driver = Arc::clone(&self.driver);
        let view = driver.with_scheduler(|scheduler| {
            EguiSceneHost::build_scene_view(scheduler.world(), scheduler.resources())
        });

        // 2. UI → 场景：收集输入事件、绘制面板 chrome、写 ViewRect。
        //    必须在推进之前：System 消费 InputEvents，Axes/FunctionGraph
        //    刷新当帧读 ViewRect。
        let viewport = driver.with_scheduler_mut(|scheduler| {
            EguiSceneHost::begin_frame(ui, &view, scheduler.resources_mut());
            // 与 `present` 同一来源（布局完成后才写进 ViewRect）。
            scheduler
                .resources()
                .get::<zela_render::ViewRect>()
                .map(|v| v.rect)
                .unwrap_or_default()
        });

        // 3. **命令推进**：GUI 与 headless 共用同一条 `Advance(dt)` 命令。
        let _receipt = driver.submit(DriverCommand::Advance { dt });

        FrameBegin {
            view,
            viewport: egui::Rect::from_min_max(
                egui::pos2(viewport.min.x, viewport.min.y),
                egui::pos2(viewport.max.x, viewport.max.y),
            ),
            dt,
        }
    }

    /// wgpu 路径的呈现：2D 队列 + 3D 队列 → 合成回调 → painter。
    pub fn present_wgpu(&self, ui: &mut egui::Ui, begin: &FrameBegin) {
        let driver = Arc::clone(&self.driver);
        driver.with_scheduler_mut(|scheduler| {
            EguiSceneHost::present(ui, &begin.view, scheduler.resources_mut());
        });
    }

    /// 帧尾：可观测性（F1 / F10）→ 清理已消费的队列 → 请求下一帧。
    pub fn finish(&mut self, ui: &mut egui::Ui, begin: &FrameBegin, progress: (u32, f32)) {
        let driver = Arc::clone(&self.driver);
        // ── 可观测性：F1 切换 health overlay，F10 导出 World 快照 ──
        if ui.input(|i| i.key_pressed(egui::Key::F1)) {
            self.health.toggle();
        }
        if ui.input(|i| i.key_pressed(egui::Key::F10)) {
            let (tick, result) = driver.with_scheduler(|scheduler| {
                let tick = scheduler.tick();
                let result =
                    self.health
                        .dump_snapshot(scheduler.world(), scheduler.resources(), tick);
                (tick, result)
            });
            match result {
                Ok(path) => {
                    tracing::info!(tick, path = %path.display(), "world snapshot exported")
                }
                Err(e) => tracing::error!(%e, tick, "snapshot export failed"),
            }
        }
        driver.with_scheduler(|scheduler| {
            let draw_calls = EguiSceneHost::draw_call_count(scheduler.resources());
            self.health.update(
                begin.dt,
                scheduler.frame_report(),
                scheduler.world().len() as usize,
                draw_calls,
            );
        });
        self.health.set_progress(progress.0, progress.1);
        self.health.show(ui);

        // 帧末：清空已被消费的 2D / 内容队列（帧管理归 host）。
        driver.with_scheduler_mut(|scheduler| {
            EguiSceneHost::end_frame(scheduler.resources_mut());
        });

        // egui 默认事件驱动，无鼠标/键盘事件时不调度 ui()，导致
        // TimeState.elapsed 停止增长、动画卡住。动画引擎需持续重绘，每帧请求下一帧。
        ui.ctx().request_repaint();
    }

    /// **wgpu 整帧**（等价于 `begin + present_wgpu + finish`）。
    ///
    /// `progress` = 事件订阅方收到的 `(帧数, 全局时间)`。
    pub fn ui(&mut self, ui: &mut egui::Ui, progress: (u32, f32)) {
        let begin = self.begin(ui);
        self.present_wgpu(ui, &begin);
        self.finish(ui, &begin, progress);
    }
}
