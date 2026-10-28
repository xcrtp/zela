//! **E4（阶段 4-A）**：着色 codegen 的三后端对拍。
//!
//! 三个"后端"指：参考求值（tree-walking，f64）、lowered（后序程序，f64）、
//! **生成的 WGSL**（GPU，f32）。前两者由 `ze-func` 的 `typed_equivalence` 钉住；
//! 本用例补上第三条：把 `ze_comb::emit_wgsl` + `ze_func::wgsl` 生成的纯函数
//! 包进一个最小的 compute pass，按 GPU 实算的结果与 CPU 参考**按容差**比较
//! （WGSL 只有 f32，逐位一致不在承诺内，见 GAPS AU-04）。
//!
//! 为什么放在渲染后端 crate：内核（`ze-comb` / `ze-func`）不依赖 wgpu；
//! 这里是"能真的跑 shader"的最低层，且 wgpu 已是它的依赖。

use ze_comb::{ChannelRef, Expr, OpRef, Value, emit_wgsl};
use ze_comb::program::{EvalError, Mono};

fn math(name: &str) -> OpRef {
    OpRef::new("zela.math", name)
}

fn f(value: f64) -> Expr {
    Expr::pure(value)
}

fn call(name: &str, args: impl IntoIterator<Item = Expr>) -> Expr {
    Expr::call(math(name), args)
}

fn x_of(expr: Expr) -> Expr {
    call("x", [expr])
}

/// 被测表达式（返回标量；向量表达式取 `.x`）。
fn expressions() -> Vec<(&'static str, Expr)> {
    let time = || Expr::channel(ChannelRef::builtin("time"));
    vec![
        (
            "sin(t) * 2 + 0.5",
            call("add", [call("mul", [call("sin", [time()]), f(2.0)]), f(0.5)]),
        ),
        (
            "clamp(t * 3 - 1, 0, 1)",
            call(
                "clamp",
                [
                    call("add", [call("mul", [time(), f(3.0)]), f(-1.0)]),
                    f(0.0),
                    f(1.0),
                ],
            ),
        ),
        (
            "x(mix(vec2(sin t, cos t), vec2(1, 2), 0.25))",
            x_of(call(
                "mix",
                [
                    call("vec2", [call("sin", [time()]), call("cos", [time()])]),
                    call("vec2", [f(1.0), f(2.0)]),
                    f(0.25),
                ],
            )),
        ),
        (
            "length(vec3(sin t, cos t, round_f32(t)))",
            call(
                "length",
                [call(
                    "vec3",
                    [
                        call("sin", [time()]),
                        call("cos", [time()]),
                        call("round_f32", [time()]),
                    ],
                )],
            ),
        ),
        (
            "normalize(vec3(1, 2, 2)).x",
            x_of(call(
                "normalize",
                [call("vec3", [f(1.0), f(2.0), f(2.0)])],
            )),
        ),
        (
            "rotate3((1,0,0), (0,1,0), t).z",
            x_of(call(
                "rotate3",
                [
                    call("vec3", [f(1.0), f(0.0), f(0.0)]),
                    call("vec3", [f(0.0), f(1.0), f(0.0)]),
                    time(),
                ],
            )),
        ),
        (
            "sum(sin t, cos t, 0.25)",
            call("sum", [call("sin", [time()]), call("cos", [time()]), f(0.25)]),
        ),
    ]
}

struct Channels(f64);

impl Mono for Channels {
    fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
        if channel.qualified() == "time" {
            Ok(Value::f64(self.0))
        } else {
            Err(EvalError::MissingChannel {
                channel: channel.clone(),
            })
        }
    }

    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
        ze_func::eval(op, args)
    }
}

/// 在 CPU 上按**参考求值器**算一遍（语义基准）。
fn reference(expr: &Expr, time: f64) -> f64 {
    let value = ze_comb::eval_reference(expr, &Channels(time)).expect("参考求值");
    value.as_f64().expect("标量结果")
}

/// 把生成的 WGSL 包成最小 compute pass，返回 GPU 实算值。
fn run_on_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    function: &ze_comb::WgslFunction,
    time: f32,
) -> f32 {
    let assignments: String = function
        .params
        .iter()
        .enumerate()
        .map(|(index, param)| {
            assert_eq!(
                param.name, "c_time",
                "本用例只喂 `time`；新增通道时在这里补夹具"
            );
            format!("    p.{} = inputs[{index}u];\n", param.name)
        })
        .collect();
    let source = format!(
        "{}\n@group(0) @binding(0) var<storage, read> inputs: array<f32>;\n\
         @group(0) @binding(1) var<storage, read_write> output: array<f32>;\n\
         @compute @workgroup_size(1)\nfn main() {{\n    var p: Params;\n{assignments}    \
         output[0] = eval(p, 0u);\n}}\n",
        function.source
    );
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("wgsl-codegen"),
        source: wgpu::ShaderSource::Wgsl(source.clone().into()),
    });
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("生成的 WGSL 未通过校验：{error}\n{source}");
    }
    // 显式 layout（不用 auto）：没有通道读端的表达式不会用到 `inputs`，
    // auto layout 会把它裁掉，bind group 就与 descriptor 对不上。
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("wgsl-codegen"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("wgsl-codegen"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("wgsl-codegen"),
        layout: Some(&layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });

    let inputs = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("inputs"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("output"),
        size: 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: 4,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&inputs, 0, &time.to_le_bytes());

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("wgsl-codegen"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: inputs.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("wgsl-codegen"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("wgsl-codegen"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 4);
    queue.submit([encoder.finish()]);

    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("等待 GPU");
    let mapped = readback
        .slice(..)
        .get_mapped_range()
        .expect("映射读回缓冲");
    let value = f32::from_le_bytes(mapped[0..4].try_into().expect("4 字节"));
    drop(mapped);
    readback.unmap();
    value
}

#[test]
fn generated_wgsl_matches_the_reference_within_tolerance() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("本机应有 GPU adapter（GPU 用例）");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("wgsl-codegen"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .expect("本机应有 GPU device");

    for time in [0.0f64, 0.7, 2.3] {
        for (label, expr) in expressions() {
            let generated =
                emit_wgsl(&expr, ze_func::wgsl_backend()).unwrap_or_else(|error| {
                    panic!("`{label}` 生成失败：{error}")
                });
            assert_eq!(
                generated.ret,
                ze_comb::SlotType::F64,
                "`{label}` 的返回类型应为标量"
            );
            let expected = reference(&expr, time);
            let actual = run_on_gpu(&device, &queue, &generated, time as f32);
            let tolerance = 2e-5 * expected.abs().max(1.0);
            assert!(
                (actual as f64 - expected).abs() <= tolerance,
                "`{label}` @ t={time}：GPU {actual} vs 参考 {expected}（容差 {tolerance}）"
            );
        }
    }
}
