//! **阶段 3 对拍**：`Iter` 的 fold / sum / map 与 `NatRec` 求值**逐位一致**。
//!
//! 纪律：对拍不重录基线 —— 这里不写"期望常量"，两边都从同一份语义算出来，
//! 只断言 `==`（逐位）。`Stream` 的上界与 `codata::expand` 同一份 `depth`。

use ze_comb::{ChannelRef, CompiledExpr, EvalError, Expr, Mono, OpRef, Value, ValueIter};

fn math(name: &str) -> OpRef {
    OpRef::new("zela.math", name)
}

/// 无通道宿主：算子语义走 `zela.math`（`Iter` 的测试不读任何通道）。
struct MathOnly;

impl Mono for MathOnly {
    fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
        Err(EvalError::MissingChannel {
            channel: channel.clone(),
        })
    }

    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
        ze_func::eval(op, args)
    }
}

/// `NatRec`：`init = 1`，`step = prev + 2`，`count = 4` ⇒ `9`。
fn nat_rec_add_two(count: i64) -> Expr {
    Expr::nat_rec(
        Expr::pure(count),
        Expr::pure(1.0),
        Expr::call(math("add"), [Expr::rec_prev(), Expr::pure(2.0)]),
    )
}

#[test]
fn iter_fold_matches_nat_rec_bit_for_bit() {
    let expr = nat_rec_add_two(4);
    let program = CompiledExpr::compile(&expr).expect("NatRec 可编译");
    let nat = program.eval(&MathOnly).expect("NatRec 可求值");
    assert_eq!(nat, Value::f64(9.0), "先钉住语义本身");

    // 同一序列走 `Iter`：结合顺序与 `NatRec.step(prev, i)` 完全一致。
    let iter = ValueIter::new(vec![Value::f64(2.0); 4]).expect("同型序列");
    let folded = iter
        .fold(Value::f64(1.0), |acc, item, _| {
            ze_func::eval(&math("add"), &[acc, item.clone()])
        })
        .expect("可折叠");
    assert_eq!(folded, nat, "Iter 的 fold 必须与 NatRec 逐位相同");

    // 清空重算仍逐位相同（`Iter` 没有隐藏状态）。
    assert_eq!(
        iter.fold(Value::f64(1.0), |acc, item, _| {
            ze_func::eval(&math("add"), &[acc, item.clone()])
        })
        .unwrap(),
        nat
    );
}

#[test]
fn iter_sum_uses_the_same_rule_as_the_sum_operator() {
    let iter = ValueIter::new(vec![
        Value::f64(0.5),
        Value::f64(0.25),
        Value::f64(0.125),
    ])
    .unwrap();
    assert_eq!(
        iter.sum().unwrap(),
        ze_func::eval(&math("sum"), iter.items()).unwrap(),
        "Iter::sum 与 zela.math::sum 同一口径（逐位）"
    );
}

#[test]
fn iter_map_keeps_length_and_stays_foldable() {
    let iter = ValueIter::new(vec![Value::f64(2.0); 4]).unwrap();
    let doubled = iter
        .map(|item, _| ze_func::eval(&math("scale"), &[Value::f64(2.0), item.clone()]))
        .expect("可映射");
    assert_eq!(doubled.len(), iter.len(), "map 不改变长度（仍是归纳侧）");
    assert_eq!(doubled.sum().unwrap(), Value::f64(16.0));
}
