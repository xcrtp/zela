//! **E5 / E6（阶段 4-B / 5）**：GPU 侧几何与 bounds 的最小可行实验。
//!
//! 这两个实验回答的是"compute pass 能不能成为几何/包围盒的求值位置"，**不改后端**：
//! - **E5**：compute pass 写控制点后，CPU 按同一组控制点展开的折线与"先 mix 再展开"
//!   在容差内一致；同一份输入连跑两次**逐字节相同**（本机逐次一致）。
//! - **E6**：`min/max` 归约与 CPU 参考**逐位相同**（不是容差）。min/max 有结合律与交换律、
//!   不涉及舍入 ⇒ 树形归约与顺序无关；唯一的小心点是 `-0.0`，两端都做 `x + 0.0` 归一。
//!
//! 边界（诚实）：本文件是**实验**，没有接进 `zela-render-vector` 的生产路径。
//! 接入需要"持久几何前缀 + 增量上传 + 新 binding / pass"（GAPS AW 里记为 C，触发条件
//! 写在那一行）。E5 的判据只覆盖"写了 ctrl 之后的结果正确性"，不覆盖渲染链路。

use ze_notion::{Path2D, PathCommand, Vec2};

fn device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("本机应有 GPU adapter（GPU 用例）");
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("gpu-geometry-experiments"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .expect("本机应有 GPU device")
}

fn storage_buffer(device: &wgpu::Device, label: &str, bytes: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes as u64,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    bytes: usize,
) -> Vec<u8> {
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: bytes as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("readback"),
    });
    encoder.copy_buffer_to_buffer(buffer, 0, &readback, 0, bytes as u64);
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("等待 GPU");
    let mapped = readback
        .slice(..)
        .get_mapped_range()
        .expect("映射读回缓冲");
    let bytes = mapped.to_vec();
    drop(mapped);
    readback.unmap();
    bytes
}

/// 控制点（每段 4 个，`vec4<f32>`：xyz + 占位 w）→ 字节。
fn ctrl_bytes(points: &[[f32; 3]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(points.len() * 16);
    for point in points {
        for lane in [point[0], point[1], point[2], 1.0] {
            bytes.extend_from_slice(&lane.to_le_bytes());
        }
    }
    bytes
}

/// 字节 → 控制点（丢掉 w）。
fn ctrl_points(bytes: &[u8]) -> Vec<[f32; 3]> {
    bytes
        .chunks_exact(16)
        .map(|chunk| {
            let lane = |at: usize| f32::from_le_bytes(chunk[at..at + 4].try_into().unwrap());
            [lane(0), lane(4), lane(8)]
        })
        .collect()
}

/// 一条三次贝塞尔折线（后端 `ctrl` 的同一口径：每 4 点一段）。
fn flatten(points: &[[f32; 3]]) -> Vec<Vec2> {
    let mut path = Path2D::new();
    let first = points[0];
    path.commands
        .push(PathCommand::MoveTo(Vec2::new(first[0], first[1])));
    for chunk in points.chunks_exact(4) {
        let at = |p: [f32; 3]| Vec2::new(p[0], p[1]);
        path.commands.push(PathCommand::CubicTo {
            ctrl1: at(chunk[1]),
            ctrl2: at(chunk[2]),
            end: at(chunk[3]),
        });
    }
    path.flatten(0.25)
}

const COMPUTE_SHADER: &str = r#"
struct Params { mix: f32 };
@group(0) @binding(0) var<storage, read> base: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> to_ctrl: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> out: array<vec4<f32>>;
@group(0) @binding(3) var<uniform> params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= arrayLength(&base)) { return; }
    out[i] = mix(base[i], to_ctrl[i], params.mix);
}
"#;

/// 跑一次 E5 的 compute pass，返回 `out` 的字节。
fn run_lerp_pass(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    base: &[[f32; 3]],
    target: &[[f32; 3]],
    mix: f32,
) -> Vec<u8> {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("e5-lerp"),
        source: wgpu::ShaderSource::Wgsl(COMPUTE_SHADER.into()),
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("e5"),
        entries: &[
            storage_entry(0, true),
            storage_entry(1, true),
            storage_entry(2, false),
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("e5"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("e5"),
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });

    let bytes = base.len() * 16;
    let base_buffer = storage_buffer(device, "base", bytes);
    let target_buffer = storage_buffer(device, "target", bytes);
    let out = storage_buffer(device, "out", bytes);
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("params"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&base_buffer, 0, &ctrl_bytes(base));
    queue.write_buffer(&target_buffer, 0, &ctrl_bytes(target));
    let mut params = [0u8; 16];
    params[0..4].copy_from_slice(&mix.to_le_bytes());
    queue.write_buffer(&uniforms, 0, &params);

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("e5"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: base_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: target_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: out.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: uniforms.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("e5"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("e5"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups((base.len() as u32).div_ceil(64), 1, 1);
    }
    queue.submit([encoder.finish()]);
    read_back(device, queue, &out, bytes)
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn sample_curves() -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let base = vec![
        [-1.0, -0.5, 0.0],
        [-0.5, 1.0, 0.0],
        [0.5, 1.0, 0.0],
        [1.0, -0.5, 0.0],
        [1.0, -0.5, 0.0],
        [1.5, -1.0, 0.0],
        [2.5, -1.0, 0.0],
        [3.0, 0.0, 0.0],
    ];
    let target = base
        .iter()
        .map(|p| [p[0] * 0.5, p[1] + 2.0, p[2]])
        .collect();
    (base, target)
}

/// **E5**：compute pass 写控制点 → CPU 展开；与"先 mix 再展开"容差内一致，
/// 且同一输入两次运行逐字节相同（本机逐次一致）。
#[test]
fn e5_compute_pass_written_controls_flatten_correctly() {
    let (device, queue) = device();
    let (base, target) = sample_curves();
    let mix = 0.37f32;

    let first = run_lerp_pass(&device, &queue, &base, &target, mix);
    let second = run_lerp_pass(&device, &queue, &base, &target, mix);
    assert_eq!(first, second, "同一份输入两次运行必须逐字节相同（本机逐次一致）");

    let gpu_points = ctrl_points(&first);
    let expected_points: Vec<[f32; 3]> = base
        .iter()
        .zip(&target)
        .map(|(a, b)| {
            [
                a[0] + (b[0] - a[0]) * mix,
                a[1] + (b[1] - a[1]) * mix,
                a[2] + (b[2] - a[2]) * mix,
            ]
        })
        .collect();

    let gpu = flatten(&gpu_points);
    let cpu = flatten(&expected_points);
    assert_eq!(gpu.len(), cpu.len(), "展开点数必须一致");
    let worst = gpu
        .iter()
        .zip(&cpu)
        .map(|(a, b)| (*a - *b).length())
        .fold(0.0f32, f32::max);
    assert!(
        worst < 1e-3,
        "compute 写出的控制点展开后与 CPU 参考最多差 {worst}（应 < 1e-3）"
    );
}

const BOUNDS_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read> ctrl: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> bounds: array<vec4<f32>>;

@compute @workgroup_size(1)
fn main() {
    var lo = vec3<f32>(1e30, 1e30, 1e30);
    var hi = vec3<f32>(-1e30, -1e30, -1e30);
    let n = arrayLength(&ctrl);
    for (var i = 0u; i < n; i = i + 1u) {
        let p = ctrl[i].xyz;
        lo = min(lo, p);
        hi = max(hi, p);
    }
    // `-0.0 + 0.0 = +0.0`：两端同一归一（GAPS 里记过的指纹坑）。
    bounds[0] = vec4<f32>(lo + vec3<f32>(0.0), 0.0);
    bounds[1] = vec4<f32>(hi + vec3<f32>(0.0), 0.0);
}
"#;

/// **E6**：GPU min/max 归约与 CPU 参考**逐位相同**（含 `-0.0` 归一）。
#[test]
fn e6_gpu_bounds_reduce_is_bit_exact() {
    let (device, queue) = device();
    let mut points = vec![
        [-2.5f32, 0.75, -1.0],
        [3.25, -0.5, 2.0],
        [0.0, 1.5, -3.5],
        [-0.0, -0.0, -0.0],
        [0.0, 0.0, 0.0],
    ];
    points.push([12.5, -7.25, 0.125]);

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("e6-bounds"),
        source: wgpu::ShaderSource::Wgsl(BOUNDS_SHADER.into()),
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("e6"),
        entries: &[storage_entry(0, true), storage_entry(1, false)],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("e6"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("e6"),
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let ctrl = storage_buffer(&device, "ctrl", points.len() * 16);
    let out = storage_buffer(&device, "bounds", 32);
    queue.write_buffer(&ctrl, 0, &ctrl_bytes(&points));
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("e6"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: ctrl.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: out.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("e6"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("e6"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    queue.submit([encoder.finish()]);

    let bytes = read_back(&device, &queue, &out, 32);
    let lane = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let gpu_lo = [lane(0), lane(4), lane(8)];
    let gpu_hi = [lane(16), lane(20), lane(24)];
    let normalize_zero = |v: [f32; 3]| [v[0] + 0.0, v[1] + 0.0, v[2] + 0.0];
    let cpu_lo = normalize_zero([
        points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
        points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
        points.iter().map(|p| p[2]).fold(f32::INFINITY, f32::min),
    ]);
    let cpu_hi = normalize_zero([
        points.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max),
        points.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max),
        points.iter().map(|p| p[2]).fold(f32::NEG_INFINITY, f32::max),
    ]);
    assert_eq!(
        gpu_lo.map(f32::to_bits),
        cpu_lo.map(f32::to_bits),
        "min 必须逐位相同：GPU {gpu_lo:?} vs CPU {cpu_lo:?}"
    );
    assert_eq!(
        gpu_hi.map(f32::to_bits),
        cpu_hi.map(f32::to_bits),
        "max 必须逐位相同：GPU {gpu_hi:?} vs CPU {cpu_hi:?}"
    );
}
