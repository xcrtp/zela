//! **单态化路径基准**：同一条表达式，`lowered`（动态 `Value`）vs `typed`（按类型分槽 +
//! 函数指针）。
//!
//! 口径：两条路径都从同一棵 `Expr` 编译；typed 只在可单态化时参与（本基准的表达式
//! 全部是数值/向量，必然可单态化）。

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use ze_comb::{ChannelRef, CompiledExpr, EvalError, Expr, Mono, OpRef, TypedProgram, Value};

/// 最小宿主：只读 `time`，算子语义走 `zela.math`。
struct Host;

impl Mono for Host {
    fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
        match channel.name.as_str() {
            "time" => Ok(Value::f64(0.5)),
            _ => Err(EvalError::MissingChannel {
                channel: channel.clone(),
            }),
        }
    }

    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
        ze_func::eval(op, args)
    }
}

fn math(name: &str) -> OpRef {
    OpRef::new("zela.math", name)
}

fn time() -> Expr {
    Expr::channel(ChannelRef::builtin("time"))
}

/// `Σ sin(t)` 深链（与 `ze-comb::eval_bench` 同形状），规模 = 项数。
fn chain(n: usize) -> Expr {
    let mut sum = Expr::call(math("sin"), [time()]);
    for _ in 1..n {
        sum = Expr::call(math("add"), [sum, Expr::call(math("sin"), [time()])]);
    }
    sum
}

fn benches(c: &mut Criterion) {
    let host = Host;
    let mut group = c.benchmark_group("eval_typed");
    for n in [16usize, 64, 256] {
        let expr = chain(n);
        let lowered = CompiledExpr::compile(&expr).expect("可编译");
        let typed = TypedProgram::compile(&expr, ze_func::monomorphizer())
            .expect("纯数值/向量表达式必须可单态化");
        black_box(lowered.eval(&host).expect("可求值"));
        black_box(typed.eval(&host).expect("可求值"));

        group.throughput(Throughput::Elements(n as u64));
        group.bench_with_input(BenchmarkId::new("lowered", n), &n, |b, _| {
            b.iter(|| black_box(lowered.eval(&host).expect("可求值")))
        });
        group.bench_with_input(BenchmarkId::new("typed", n), &n, |b, _| {
            b.iter(|| black_box(typed.eval(&host).expect("可求值")))
        });
    }
    group.finish();
}

criterion_group!(typed, benches);
criterion_main!(typed);
