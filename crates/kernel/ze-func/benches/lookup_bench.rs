//! 符号表 vs 线性扫描：阶段 3 的性能主张（每条都有对照）。
//!
//! ```text
//! cargo bench -p ze-func --bench lookup_bench -- --quick
//! ```

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use ze_comb::OpRef;

/// `op_def`：`DEFS.iter().find`（旧）vs 符号表（新）。
fn op_def_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("op_def_lookup");

    group.bench_function("symbol_table_hit", |b| {
        b.iter(|| black_box(ze_func::op_def(black_box("not"))))
    });
    group.bench_function("linear_scan_hit", |b| {
        b.iter(|| {
            black_box(
                ze_func::OP_DEFS
                    .iter()
                    .find(|def| def.name == black_box("not")),
            )
        })
    });
    group.bench_function("symbol_table_miss", |b| {
        b.iter(|| black_box(ze_func::op_def(black_box("warp"))))
    });
    group.bench_function("linear_scan_miss", |b| {
        b.iter(|| {
            black_box(
                ze_func::OP_DEFS
                    .iter()
                    .find(|def| def.name == black_box("warp")),
            )
        })
    });

    group.finish();
}

/// `is_known`：`OPS.contains`（旧）vs 符号表（新）。
fn op_membership(c: &mut Criterion) {
    let known = OpRef::new(ze_func::LIBRARY_NAMESPACE, "not");
    let unknown = OpRef::new(ze_func::LIBRARY_NAMESPACE, "warp");

    let mut group = c.benchmark_group("is_known");
    group.bench_function("symbol_table_hit", |b| {
        b.iter(|| black_box(ze_func::is_known(black_box(&known))))
    });
    group.bench_function("ops_contains_hit", |b| {
        b.iter(|| {
            black_box(
                known.namespace == ze_func::LIBRARY_NAMESPACE
                    && ze_func::OPS.contains(&known.name.as_str()),
            )
        })
    });
    group.bench_function("symbol_table_miss", |b| {
        b.iter(|| black_box(ze_func::is_known(black_box(&unknown))))
    });
    group.bench_function("ops_contains_miss", |b| {
        b.iter(|| {
            black_box(
                unknown.namespace == ze_func::LIBRARY_NAMESPACE
                    && ze_func::OPS.contains(&unknown.name.as_str()),
            )
        })
    });

    group.finish();
}

criterion_group!(benches, op_def_lookup, op_membership);
criterion_main!(benches);
