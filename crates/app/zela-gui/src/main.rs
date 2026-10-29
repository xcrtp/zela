use zela_app::SceneChoice;
use zela_egui as ui;
use zela_gui::{App, Initial};

/// 启动时的初始作品：
/// - 默认：内置作品列表的第一件（列表里含**全部** demo，见 `zela_app::demos`）；
/// - `--work <id>` ⇒ 直接选中某件内置 demo（`id` 见 manifest 第一列）；
/// - `--script` ⇒ `storyboard`（与默认路径同一条 Lua 消费链）；
/// - `--python` / `--python-script` ⇒ 内置 Python 分镜；
/// - `--demo` / `--capability-demo` / `ZELA_DEMO=1` ⇒ 内置能力 demo。
///
/// 启动参数只决定"先播哪件"：运行态可在 GUI 里随时切换（会重新装配 Runtime）。
fn initial() -> Initial {
    let args: Vec<String> = std::env::args().collect();
    if let Some(index) = args.iter().position(|arg| arg == "--work") {
        if let Some(id) = args.get(index + 1) {
            return Initial::Demo(id.clone());
        }
    }
    if args.iter().any(|a| a == "--script") {
        return Initial::Demo("storyboard".to_string());
    }
    if args
        .iter()
        .any(|a| a == "--python" || a == "--python-script")
    {
        return Initial::Choice(SceneChoice::PythonStoryboard);
    }
    let by_arg = args
        .iter()
        .any(|a| a == "--demo" || a == "--capability-demo");
    let by_env = std::env::var("ZELA_DEMO")
        .map(|v| v == "1")
        .unwrap_or(false);
    if by_arg || by_env {
        Initial::Choice(SceneChoice::CapabilityDemo)
    } else {
        Initial::First
    }
}

fn main() -> eframe::Result<()> {
    // 结构化日志：过滤器由 ZELA_LOG 控制（默认 zela=debug,warn）。
    zela_schedule::telemetry::init_tracing();

    let initial = initial();

    let mut options = ui::window_options();
    request_vector_device(&mut options);

    eframe::run_native(
        "zela",
        options,
        Box::new(move |cc| Ok(Box::new(App::with_initial(cc, initial)?))),
    )
}

/// **阶段 1（零回读）**：让 eframe 的 device 带上矢量后端需要的 features / limits。
///
/// 矢量后端现在跑在**宿主这一套** device 上（`App::launch` 的 `init_gpu`），而它建管线时
/// 需要 `FLOAT32_BLENDABLE`（R32Float 覆盖率累加）与 `DEPTH32FLOAT_STENCIL8`（reversed-Z）；
/// 缺了就是建管线时直接校验失败。口径与 headless 侧同一份：已注册且可用后端之并。
fn request_vector_device(options: &mut eframe::NativeOptions) {
    use zela_render_backends::{BackendKind, BackendRegistry};

    let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup else {
        // 宿主自带 device（`WgpuSetup::Existing`）⇒ 能力由调用方负责，这里不猜。
        return;
    };
    setup.device_descriptor = std::sync::Arc::new(|adapter| {
        let mut registry = BackendRegistry::with_standard();
        let _ = registry.register(BackendKind::Vector);
        // 并集失败（没有任何可用后端）在 GUI 里不可能发生：wgpu 后端总是注册着。
        let features = registry.required_device_features(adapter).unwrap_or_default();
        eframe::wgpu::DeviceDescriptor {
            label: Some("zela"),
            required_features: features,
            required_limits: registry.required_device_limits(adapter),
            ..Default::default()
        }
    });
}
