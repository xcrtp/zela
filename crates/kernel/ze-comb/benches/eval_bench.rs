//! **求值基准**：lowered（编译后线性扫描）vs reference（树walking）。
//!
//! ```text
//! cargo bench -p ze-comb --bench eval_bench -- --quick
//! ```
//!
//! 五组：
//! - `eval/compile`：装配期一次性的 lowering 成本；
//! - `eval/lowered`：每帧扫描 `Step` 表；
//! - `eval/reference`：同一棵树的参考求值（对拍口径）；
//! - `eval/shared`：共享 `Def`（`Ref`）两条路径的差异；
//! - `eval/nat_rec`：`NatRec` 冷启动（全量重放）vs 带缓存续算。

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ze_comb::{
    ChannelRef, CompiledExpr, EvalError, Expr, Mono, OpRef, RecCache, Value, eval_reference,
};

/// 最小宿主：只能读 `time`，只认 `sin` / `add`（与 kernel 测试同一口径）。
struct Host {
    time: f64,
}

impl Mono for Host {
    fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
        if channel.name == "time" {
            Ok(Value::f64(self.time))
        } else {
            Err(EvalError::MissingChannel {
                channel: channel.clone(),
            })
        }
    }

    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
        let number = |index: usize| args[index].as_f64().expect("实参必须是标量");
        match op.name.as_str() {
            "sin" => Ok(Value::f64(number(0).sin())),
            "add" => Ok(Value::f64(number(0) + number(1))),
            other => Err(EvalError::op(op, format!("未知算子 `{other}`"))),
        }
    }
}

fn math(name: &str) -> OpRef {
    OpRef::new("zela.math", name)
}

fn time() -> Expr {
    Expr::channel(ChannelRef::builtin("time"))
}

/// `Σ sin(t)`（深链 `add`）：模拟 fourier 的形状，规模 = 项数。
fn fourier(n: usize) -> Expr {
    let mut sum = Expr::call(math("sin"), [time()]);
    for _ in 1..n {
        sum = Expr::call(math("add"), [sum, Expr::call(math("sin"), [time()])]);
    }
    sum
}

/// 共享 `Def`：`base` 深链 n/2，根用两次 `Ref base`。
fn shared(n: usize) -> Expr {
    let mut base = Expr::call(math("sin"), [time()]);
    for _ in 1..n / 2 {
        base = Expr::call(math("add"), [base, Expr::call(math("sin"), [time()])]);
    }
    let base = base.with_id("base");
    Expr::call(math("add"), [base, Expr::reference("base")])
}

/// `NatRec`：`value(0) = 1`，`value(k) = value(k−1) + 1`。
fn nat_rec(count: i64) -> Expr {
    Expr::nat_rec(
        Expr::pure(count),
        Expr::pure(1.0),
        Expr::call(math("add"), [Expr::rec_prev(), Expr::pure(1.0)]),
    )
}

fn benchmarks(c: &mut Criterion) {
    let host = Host { time: 0.5 };

    for size in [64usize, 256, 1024] {
        let expr = fourier(size);
        let compiled = CompiledExpr::compile(&expr).expect("fourier 合法");

        let mut group = c.benchmark_group("eval_compile");
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                black_box(CompiledExpr::compile(black_box(&expr)).expect("合法")).step_count()
            })
        });
        group.finish();

        let mut group = c.benchmark_group("eval_lowered");
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(&compiled).eval(&host).expect("可求值"))
        });
        group.finish();

        let mut group = c.benchmark_group("eval_reference");
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(eval_reference(black_box(&expr), &host).expect("可求值")))
        });
        group.finish();
    }

    for size in [64usize, 256, 1024] {
        let expr = shared(size);
        let compiled = CompiledExpr::compile(&expr).expect("共享 Def 合法");

        let mut group = c.benchmark_group("eval_shared");
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::new("lowered", size), &size, |b, _| {
            b.iter(|| black_box(&compiled).eval(&host).expect("可求值"))
        });
        group.bench_with_input(BenchmarkId::new("reference", size), &size, |b, _| {
            b.iter(|| black_box(eval_reference(black_box(&expr), &host).expect("可求值")))
        });
        group.finish();
    }

    // `NatRec`：冷启动（0 → 256）vs 带缓存（250 → 256，只看续算的边际成本）。
    const STEPS: i64 = 256;
    const RESUME_AT: i64 = 250;
    let expr = nat_rec(STEPS);
    let compiled = CompiledExpr::compile(&expr).expect("NatRec 合法");

    let mut group = c.benchmark_group("eval_nat_rec");
    group.throughput(Throughput::Elements(STEPS as u64));
    group.bench_function("cold", |b| {
        b.iter(|| {
            let mut cache = None;
            black_box(compiled.eval_cached(&host, &mut cache).expect("可求值"))
        })
    });
    group.bench_function("resume", |b| {
        b.iter(|| {
            let mut cache = Some(RecCache {
                count: RESUME_AT,
                value: Value::f64((RESUME_AT + 1) as f64),
            });
            black_box(compiled.eval_cached(&host, &mut cache).expect("可求值"))
        })
    });
    group.finish();
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
