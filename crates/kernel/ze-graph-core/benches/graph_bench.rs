//! 纯拓扑算法基准（规模 × 算法）。
//!
//! ```text
//! cargo bench -p ze-graph-core --bench graph_bench -- --quick
//! ```

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ze_graph_core::{
    Adjacency, dirty_downstream, find_cycle, reachable, strongly_connected, topo_sort,
};

/// 分层 DAG：`i → i+1`、`i → i+3`（无环，规模可控）。
fn layered(n: u32) -> Adjacency<u32> {
    let mut graph = Adjacency::new();
    for node in 0..n {
        graph.add_node(node);
        if node + 1 < n {
            graph.add_edge(node, node + 1);
        }
        if node + 3 < n {
            graph.add_edge(node, node + 3);
        }
    }
    graph
}

/// 有环图：`i → (i × 7 + 1) mod n`（几乎处处成环，用于 SCC / 环检测）。
fn cyclic(n: u32) -> Adjacency<u32> {
    let mut graph = Adjacency::new();
    for node in 0..n {
        graph.add_node(node);
        graph.add_edge(node, (node.wrapping_mul(7).wrapping_add(1)) % n);
    }
    graph
}

fn benchmarks(c: &mut Criterion) {
    for size in [128u32, 1024, 8192] {
        let dag = layered(size);
        let graph = cyclic(size);

        let mut group = c.benchmark_group("topo_sort");
        group.throughput(Throughput::Elements(u64::from(size)));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(topo_sort(&dag).expect("layered DAG")))
        });
        group.finish();

        let mut group = c.benchmark_group("scc");
        group.throughput(Throughput::Elements(u64::from(size)));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(strongly_connected(&graph)))
        });
        group.finish();

        let mut group = c.benchmark_group("find_cycle");
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(find_cycle(&graph)))
        });
        group.finish();

        let mut group = c.benchmark_group("reachable");
        group.throughput(Throughput::Elements(u64::from(size)));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(reachable(&dag, 0)))
        });
        group.finish();

        let mut group = c.benchmark_group("dirty_downstream");
        group.throughput(Throughput::Elements(u64::from(size)));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(dirty_downstream(&dag, &[0])))
        });
        group.finish();
    }
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
