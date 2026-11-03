//! **阶段 1 · E-G1**：GUI 矢量上屏的零回读出口。
//!
//! 判据（与执行计划 §阶段 1 一致）：
//! 1. `render_2d_to_texture`（渲进调用方纹理）与 `render_2d`（CPU 读回）**逐位相同**
//!    —— 上屏路径换了，像素口径不能换；
//! 2. 同一帧连渲两次**逐位相同**（§1.4 的"本机逐次一致"）；
//! 3. 帧像素不经 CPU：只断言出口本身不读回（纹理由调用方持有，模型见 `check_target`）。
//!
//! 独立测试二进制：`init_gpu` 只能注入一次，这里独占一个进程。

use ze_notion::{Color32, Path2D, Transform2D, Vec2};
use ze_render_ir::{Shape2D, Style2D};
use zela_render_vector::{PathRecord, SceneItem, scene_records};

// 宽度取 128：`copy_texture_to_buffer` 的每行字节必须是 256 的倍数（128 × 4 = 512）。
const WIDTH: u32 = 128;
const HEIGHT: u32 = 96;

fn records() -> Vec<PathRecord> {
    let path = Path2D::from_polyline(
        &[
            Vec2::new(10.0, 12.0),
            Vec2::new(80.0, 24.0),
            Vec2::new(40.0, 84.0),
        ],
        true,
    );
    let shape = Shape2D::Path(path);
    let style = Style2D {
        fill: Some(Color32::from_rgba(40, 120, 220, 255)),
        stroke: Some(Color32::WHITE),
        stroke_width: 2.0,
        ..Default::default()
    };
    scene_records(
        &[SceneItem {
            shape: &shape,
            transform: Transform2D::identity(),
            style,
        }],
        (WIDTH, HEIGHT),
    )
    .expect("2D 记录映射")
}

/// `Rgba8Unorm` + 渲染 / 存储 / 采样三种用法（`check_target` 的硬约束）。
fn target(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vector-zero-copy-target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::TEXTURE_BINDING
            // 仅测试：读回对拍要 `copy_texture_to_buffer`。
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// 把整张纹理拷回 CPU（**测试专用**：生产路径不做这一步）。
fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let bytes_per_row = WIDTH * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vector-readback"),
        size: u64::from(bytes_per_row * HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("等待 GPU");
    let mapped = slice.get_mapped_range().expect("映射读回缓冲");
    let pixels = mapped.to_vec();
    drop(mapped);
    buffer.unmap();
    pixels
}

#[test]
fn texture_target_matches_the_cpu_readback_and_repeats_bitwise() {
    use zela_render_backends::{BackendKind, BackendRegistry};

    let Some((_gpu, ctx)) = zela_render_backends::require_gpu() else {
        return;
    };
    // 与 `zela-parity` 同一条口径：device features = 已注册且可用后端之并
    // （矢量要 FLOAT32_BLENDABLE / DEPTH32FLOAT_STENCIL8，否则 R32Float 不可混合）。
    let adapter = ctx.adapter().expect("headless 上下文应带 adapter").clone();
    let mut registry = BackendRegistry::with_standard();
    registry.register(BackendKind::Vector).expect("注册矢量后端");
    let features = registry.required_device_features(&adapter).expect("能力并集");
    let limits = registry.required_device_limits(&adapter);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("vector-gui-zero-copy"),
        required_features: features,
        required_limits: limits,
        ..Default::default()
    }))
    .expect("请求 device");
    // 注入宿主这一套：此后矢量后端不再自建 device（与 GUI 的 `launch` 同一做法）。
    zela_render_vector::init_gpu(device.clone(), queue.clone(), &adapter).expect("注入 GPU");

    let records = records();
    let clear = [0.0, 0.0, 0.0, 1.0];
    let reference =
        zela_render_vector::render_2d(WIDTH, HEIGHT, 1, clear, &records).expect("CPU 读回路径");

    let texture = target(&device);
    let first = {
        zela_render_vector::render_2d_to_texture(&texture, WIDTH, HEIGHT, 1, clear, &records)
            .expect("零回读出口");
        read_back(&device, &queue, &texture)
    };
    let second = {
        zela_render_vector::render_2d_to_texture(&texture, WIDTH, HEIGHT, 1, clear, &records)
            .expect("零回读出口（第二次）");
        read_back(&device, &queue, &texture)
    };

    assert_eq!(first.len(), reference.len(), "读回缓冲尺寸应与参考一致");
    assert!(
        first == reference,
        "纹理目标与 CPU 读回不一致：{} / {} 字节不同",
        first
            .iter()
            .zip(&reference)
            .filter(|(a, b)| a != b)
            .count(),
        reference.len()
    );
    assert!(first == second, "同一帧连渲两次应逐位相同");

    // 视口 resize / 重播会**新建纹理**：同尺寸也必须换目标。
    // 回归：Player 按 `(width, height, samples)` 缓存，旧实现只比尺寸 / 代数 ⇒ 继续写
    // 调用方已释放的旧纹理，新纹理全空（GUI 上表现为"改窗口大小后画面全黑"）。
    let replacement = target(&device);
    zela_render_vector::render_2d_to_texture(&replacement, WIDTH, HEIGHT, 1, clear, &records)
        .expect("零回读出口（同尺寸新纹理）");
    let after = read_back(&device, &queue, &replacement);
    assert!(
        after == reference,
        "同尺寸换纹理后仍写旧目标：新纹理与参考不一致（{} / {} 字节不同）",
        after
            .iter()
            .zip(&reference)
            .filter(|(a, b)| a != b)
            .count(),
        reference.len()
    );
}
