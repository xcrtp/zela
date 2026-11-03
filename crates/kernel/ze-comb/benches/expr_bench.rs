//! 表达式遍历 / 图视图 / 分析基准（阶段 1–4 的新路径）。
//!
//! ```text
//! cargo bench -p ze-comb --bench expr_bench -- --quick
//! ```
//!
//! 三组对照：
//! - **词法树**：`walk`（前序 + 后序）与 `map_children`（重建）；
//! - **DAG**：`ExprGraphView::build`（投影 + `Ref` 解析）；
//! - **分析**：`shared_definitions` / `affected_consumers` / `validate`（含环检测）。

use std::convert::Infallible;
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ze_comb::{
    Expr, ExprGraphView, ExprVisitor, Node, OpRef, affected_consumers, shared_definitions,
};

fn math(name: &str) -> OpRef {
    OpRef::new("zela.math", name)
}

/// 深链：`add(add(… add(pure(0), pure(1)) …), pure(n))`。
fn deep(n: usize) -> Expr {
    let mut expr = Expr::pure(0.0);
    for index in 0..n {
        expr = Expr::call(math("add"), [expr, Expr::pure(index as f64)]);
    }
    expr
}

/// 共享 `Def`：根聚合 `base`（深链 n/2）+ 两次 `Ref base`。
fn shared(n: usize) -> Expr {
    let base = deep(n / 2).with_id("base");
    Expr::call(
        math("add"),
        [base, Expr::reference("base"), Expr::reference("base")],
    )
}

/// 只数节点（遍历骨架本身的开销，不含业务判定）。
struct Count(usize);

impl<'a> ExprVisitor<'a> for Count {
    type Error = Infallible;

    fn enter(&mut self, _expr: &'a Expr, _parent: Option<&'a Node>) -> Result<bool, Infallible> {
        self.0 += 1;
        Ok(true)
    }
}

fn benchmarks(c: &mut Criterion) {
    for size in [64usize, 256, 1024] {
        let tree = deep(size);
        let dag_expr = shared(size);

        let mut group = c.benchmark_group("expr_walk");
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let mut count = Count(0);
                let _ = black_box(&tree).walk(&mut count);
                black_box(count.0)
            })
        });
        group.finish();

        let mut group = c.benchmark_group("expr_map_children");
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(&tree).map_children(|child| child.clone()))
        });
        group.finish();

        let mut group = c.benchmark_group("expr_graph_view");
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(ExprGraphView::build(black_box(&dag_expr))).len())
        });
        group.finish();

        let mut group = c.benchmark_group("expr_analysis");
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("shared", size), &size, |b, _| {
            b.iter(|| black_box(shared_definitions(black_box(&dag_expr))))
        });
        group.bench_with_input(BenchmarkId::new("affected", size), &size, |b, _| {
            b.iter(|| black_box(affected_consumers(black_box(&dag_expr), &["base"])))
        });
        group.bench_with_input(BenchmarkId::new("validate", size), &size, |b, _| {
            b.iter(|| black_box(dag_expr.validate().is_ok()))
        });
        group.finish();
    }
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
