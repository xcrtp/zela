//! `RegistryPod` 的名字解析：符号表（新）vs 线性扫描（旧）。
//!
//! ```text
//! cargo bench -p ze-pod-core --bench function_lookup -- --quick
//! ```

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use ze_pod_core::{Pod, QualifiedName, RegistryPod};

fn build(size: usize) -> (RegistryPod, Vec<QualifiedName>) {
    let mut pod = RegistryPod::new();
    let mut names = Vec::with_capacity(size);
    for index in 0..size {
        let path = format!("bench::f{index}");
        pod.register_fn0::<u64>(&path, |_| 0u64);
        names.push(QualifiedName::from_path(&path, Vec::new(), "u64"));
    }
    (pod, names)
}

fn resolve_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("registry_resolve");
    for size in [16usize, 256, 4096] {
        let (pod, names) = build(size);
        let target = &names[size - 1];

        group.bench_with_input(BenchmarkId::new("symbol_table_last", size), &size, |b, _| {
            b.iter(|| black_box(pod.resolve(black_box(target)).is_some()))
        });
        group.bench_with_input(
            BenchmarkId::new("linear_scan_last", size),
            &size,
            |b, _| {
                b.iter(|| {
                    black_box(
                        pod.functions()
                            .iter()
                            .position(|name| name == black_box(target)),
                    )
                })
            },
        );
    }
    group.finish();
}

fn function_id_lookup(c: &mut Criterion) {
    let (pod, names) = build(256);
    let target = &names[255];
    let mut group = c.benchmark_group("function_id");
    group.bench_function("symbol_table_last", |b| {
        b.iter(|| black_box(pod.function_id(black_box(target)).is_some()))
    });
    group.bench_function("linear_scan_last", |b| {
        b.iter(|| {
            black_box(
                pod.functions()
                    .iter()
                    .position(|name| name == black_box(target)),
            )
        })
    });
    group.finish();
}

criterion_group!(benches, resolve_lookup, function_id_lookup);
criterion_main!(benches);
