//! **阶段 2 · E-G2（后端切片）**：着色 codegen 真的在 GPU 上求值。
//!
//! 判据：
//! 1. `ze_comb::emit_wgsl` 生成的 WGSL 被 compute pass 执行，写进记录的填充槽
//!    ⇒ 像素的 RGB 等于 CPU 参考（容差 4/255，f32 与 shader 的实现差）；
//! 2. 同一帧连跑两次**逐位相同**（§1.4 的本机逐次一致）；
//! 3. 没有程序时像素与"普通填充"逐位相同（本机制不改既有行为）。
//!
//! 独立测试二进制：`init_gpu` 只能注入一次。

use ze_notion::{Color32, Path2D, Transform2D, Vec2};
use ze_render_ir::{Shape2D, Style2D};
use zela_render_vector::{FilledPath, PathRecord, SceneItem, ShaderFill, scene_records};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
const TIME: f32 = 0.75;

/// 生成 `vec3<f32>(sin(time), 0.25, 0.5)` 的 WGSL 与它的 uniform 字节。
fn program() -> (ShaderFill, [f32; 3]) {
    use ze_comb::{ChannelRef, Expr, OpRef};
    let math = |name: &str| OpRef::new("zela.math", name);
    let time = Expr::channel(ChannelRef::builtin("time"));
    let expr = Expr::call(
        math("vec3"),
        [
            Expr::call(math("sin"), [time]),
            Expr::pure(0.25),
            Expr::pure(0.5),
        ],
    );
    let emitted = ze_comb::emit_wgsl(&expr, ze_func::wgsl::backend()).expect("可生成");
    assert_eq!(emitted.ret, ze_comb::SlotType::Vec3);
    assert_eq!(emitted.params.len(), 1, "只有 time 一个参数");
    // `struct Params { c_time: f32 }`：uniform 里 16 字节对齐，值在前 4 字节。
    let mut params = vec![0u8; 16];
    params[..4].copy_from_slice(&TIME.to_le_bytes());
    (
        ShaderFill {
            source: emitted.source,
            params,
        },
        [(TIME).sin(), 0.25, 0.5],
    )
}

/// 盖满整幅画布的填充（屏幕坐标经 `scene_records` 映射，与作品同一条路径）。
fn canvas_fill(shader: Option<ShaderFill>) -> FilledPath {
    let path = Path2D::from_polyline(
        &[
            Vec2::new(0.0, 0.0),
            Vec2::new(WIDTH as f32, 0.0),
            Vec2::new(WIDTH as f32, HEIGHT as f32),
            Vec2::new(0.0, HEIGHT as f32),
        ],
        true,
    );
    let shape = Shape2D::Path(path);
    let style = Style2D {
        fill: Some(Color32::from_rgba(10, 20, 30, 255)),
        ..Default::default()
    };
    let mut records = scene_records(
        &[SceneItem {
            shape: &shape,
            transform: Transform2D::identity(),
            style,
        }],
        (WIDTH, HEIGHT),
    )
    .expect("2D 记录映射");
    let mut fill = match records.remove(0) {
        PathRecord::Fill(fill) => fill,
        other => panic!("应是填充记录，得到 {other:?}"),
    };
    fill.shader = shader;
    fill
}

fn render(fill: FilledPath) -> Vec<u8> {
    zela_render_vector::render_2d(WIDTH, HEIGHT, 1, [0.0, 0.0, 0.0, 1.0], &[fill.into()])
        .expect("渲染一帧")
}

fn center(frame: &[u8]) -> [u8; 4] {
    let at = ((HEIGHT as usize / 2) * WIDTH as usize + WIDTH as usize / 2) * 4;
    [frame[at], frame[at + 1], frame[at + 2], frame[at + 3]]
}

#[test]
fn generated_wgsl_paints_the_fill_on_the_gpu() {
    use zela_render_backends::{BackendKind, BackendRegistry};

    let Some((_gpu, ctx)) = zela_render_backends::require_gpu() else {
        return;
    };
    let adapter = ctx.adapter().expect("headless 上下文应带 adapter").clone();
    let mut registry = BackendRegistry::with_standard();
    registry.register(BackendKind::Vector).expect("注册矢量后端");
    let features = registry.required_device_features(&adapter).expect("能力并集");
    let limits = registry.required_device_limits(&adapter);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("codegen-fill"),
        required_features: features,
        required_limits: limits,
        ..Default::default()
    }))
    .expect("请求 device");
    zela_render_vector::init_gpu(device, queue, &adapter).expect("注入 GPU");

    // ① 普通填充：像素 = 记录自带的颜色（着色 codegen 不改既有行为）。
    let plain = render(canvas_fill(None));
    assert_eq!(
        center(&plain),
        [10, 20, 30, 255],
        "没有程序时填充色应逐位等于记录值"
    );

    // ② 挂着色程序：RGB 由 GPU 的 `eval` 算出，alpha 保持记录原值。
    let (shader, expected) = program();
    let first = render(canvas_fill(Some(shader.clone())));
    let second = render(canvas_fill(Some(shader)));
    assert_eq!(first, second, "同一帧连跑两次应逐位相同");

    let [r, g, b, a] = center(&first);
    assert_eq!(a, 255, "alpha 应保持记录原值");
    let expect = expected.map(|value| (value * 255.0).round() as i32);
    for (got, want) in [(r as i32, expect[0]), (g as i32, expect[1]), (b as i32, expect[2])] {
        assert!(
            (got - want).abs() <= 4,
            "GPU 求值与 CPU 参考差超出容差：得到 {got}，期望 {want}（整帧中心 {r},{g},{b}）"
        );
    }
}
