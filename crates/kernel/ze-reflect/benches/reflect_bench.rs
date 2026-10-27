//! 解析协议基准：符号表 O(1)、句柄槽位、单态化缓存。
//!
//! ```text
//! cargo bench -p ze-reflect --bench reflect_bench -- --quick
//! ```

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use ze_reflect::sym::SymbolTable;
use ze_reflect::{Blame, MonoCache, SlotTable};
use ze_type::r#gen::monomorphize;
use ze_type::{Kind, OpSig, TypeSpec};

/// 符号表解析：固定目标，改变表规模 —— 命中耗时应基本不变。
fn symbol_resolve(c: &mut Criterion) {
    let mut group = c.benchmark_group("symbol_resolve");
    for size in [16usize, 256, 4096] {
        let mut table: SymbolTable<String, usize> = SymbolTable::new();
        for index in 0..size {
            table
                .intern(format!("zela.math::op{index}"), index, Blame::Assembly)
                .expect("names are unique");
        }
        let last = format!("zela.math::op{}", size - 1);

        group.bench_with_input(BenchmarkId::new("hit_last", size), &size, |b, _| {
            b.iter(|| black_box(table.resolve(black_box(last.as_str())).map(|(_, v)| *v)))
        });
        group.bench_with_input(BenchmarkId::new("miss", size), &size, |b, _| {
            b.iter(|| black_box(table.resolve(black_box("zela.math::warp")).is_some()))
        });
    }
    group.finish();
}

/// generation 感知槽位读取（句柄机制的单次解析成本）。
fn slot_get(c: &mut Criterion) {
    const N: u64 = 1024;
    let mut table = SlotTable::new();
    for index in 0..N {
        table.insert(index);
    }

    let mut group = c.benchmark_group("slot_table");
    group.bench_function("get_last", |b| {
        b.iter(|| black_box(*table.get(black_box(N - 1), 0).unwrap()))
    });
    group.bench_function("get_miss", |b| {
        b.iter(|| black_box(table.get(black_box(N), 0).is_err()))
    });
    group.finish();
}

const NUM: &TypeSpec = &TypeSpec::Var("T");
const COS: OpSig = OpSig {
    params: &[NUM],
    ret: NUM,
    vars: &[("T", Kind::Num)],
    variadic: false,
};

/// 单态化：缓存命中 vs 每次重算（阶段 4 的按需单态化）。
fn monomorphization(c: &mut Criterion) {
    let mut group = c.benchmark_group("monomorphize");
    group.bench_function("recompute", |b| {
        b.iter(|| black_box(monomorphize(black_box(&COS)).expect("within bound")))
    });

    let mut cache = MonoCache::default();
    cache
        .instances("cos", &COS)
        .expect("first call fills the cache");
    group.bench_function("cache_hit", |b| {
        b.iter(|| {
            black_box(
                cache
                    .instances(black_box("cos"), black_box(&COS))
                    .map(|instances| instances.len()),
            )
        })
    });
    group.finish();
}

criterion_group!(benches, symbol_resolve, slot_get, monomorphization);
criterion_main!(benches);
