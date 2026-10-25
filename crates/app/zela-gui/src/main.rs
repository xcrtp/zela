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

    eframe::run_native(
        "zela",
        ui::window_options(),
        Box::new(move |cc| Ok(Box::new(App::with_initial(cc, initial)?))),
    )
}
