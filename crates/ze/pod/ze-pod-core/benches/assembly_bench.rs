//! 装配图基准：`AssemblyPlan::validate` 的拓扑排序与环检测（阶段 2 收敛后）。
//!
//! ```text
//! cargo bench -p ze-pod-core --bench assembly_bench -- --quick
//! ```
//!
//! 这两条路径现在都走 `ze-graph-core`（`topo_sort` / `find_cycle`），
//! 基准锁住"描述层校验的成本随节点数线性增长"这个性质。

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ze_pod_core::{AssemblyPlan, NodeSpec, ServiceRequirement};

/// 链式依赖：`n0 ← n1 ← … ← n(k-1)`（提供者在前，无环）。
fn chain(n: usize) -> AssemblyPlan {
    let mut nodes = Vec::with_capacity(n);
    for index in 0..n {
        let name = format!("n{index}");
        let service = format!("s{index}");
        let requires = if index == 0 {
            Vec::new()
        } else {
            let provider = format!("s{}", index - 1);
            vec![ServiceRequirement::required(
                provider.as_str(),
                "s::ping() -> ()",
            )]
        };
        nodes.push(NodeSpec::new(name, &[service.as_str()], requires));
    }
    AssemblyPlan::new(nodes)
}

/// 首尾相接：每个节点依赖前一个，`n0` 又依赖最后一个 ⇒ 整圈成环。
fn ring(n: usize) -> AssemblyPlan {
    let mut nodes = Vec::with_capacity(n);
    for index in 0..n {
        let name = format!("n{index}");
        let service = format!("s{index}");
        let provider = (index + n - 1) % n;
        let service_name = format!("s{provider}");
        nodes.push(NodeSpec::new(
            name,
            &[service.as_str()],
            vec![ServiceRequirement::required(
                service_name.as_str(),
                "s::ping() -> ()",
            )],
        ));
    }
    AssemblyPlan::new(nodes)
}

fn benchmarks(c: &mut Criterion) {
    for size in [16usize, 128, 512] {
        let dag = chain(size);
        let cyclic = ring(size);

        let mut group = c.benchmark_group("assembly_validate");
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(dag.validate().expect("链式装配无环").order.len()))
        });
        group.finish();

        let mut group = c.benchmark_group("assembly_cycle");
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(cyclic.validate().is_err()))
        });
        group.finish();
    }
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
