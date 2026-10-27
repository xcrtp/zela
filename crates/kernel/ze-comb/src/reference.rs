//! **参考求值器**：直接在 `Expr` 树上走一遍（tree-walking）。
//!
//! # 为什么它是 pass
//!
//! lowered 求值器（[`crate::CompiledExpr::eval`]）是性能路径；参考求值器是
//! **语义基准**。两者对同一棵树、同一份 `M` 必须逐位一致 —— 这条对拍把
//! "化简改了结果"从最贵的一类静默变成自动可测（计划 §九）。
//!
//! 参考求值器不做任何优化：没有步表、没有共享节点折叠以外的结构假设，
//! 只按 [`Node`] 的定义递归。它与 lowered 的**唯一**共同点是不纯入口只有 [`Mono`]。
//!
//! # 共享节点
//!
//! 带 `id` 的节点在一次求值内只算一次（与 lowered 的 `Def + Ref` 语义一致），
//! 结果放在**本次调用的** side table 里 —— 不跨调用，因此"清空重算逐位相同"
//! 仍是结构事实。

use std::collections::BTreeMap;

use crate::expr::{definitions, Expr, Node};
use crate::program::{literal_value, EvalError, Mono, Value};

/// 参考求值：`expr` 在宿主 `mono` 下的值。
///
/// 先做一次 [`Expr::validate`]，因此悬空 `Ref` / 环 / 重名 id 都在这里显式失败，
/// 不会在递归里栈溢出。
pub fn eval_reference(expr: &Expr, mono: &dyn Mono) -> Result<Value, EvalError> {
    // 校验错误不是运行期值错误；用 `ExprError` 包一层会污染 `EvalError`，
    // 因此参考求值器要求调用方**已经**校验（见 `ReferenceEvaluator::prepare`）。
    let defs = definitions(expr);
    let mut memo: BTreeMap<&str, Value> = BTreeMap::new();
    eval_node(expr, &defs, &mut memo, mono)
}

fn eval_node<'a>(
    expr: &'a Expr,
    defs: &BTreeMap<&'a str, &'a Expr>,
    memo: &mut BTreeMap<&'a str, Value>,
    mono: &dyn Mono,
) -> Result<Value, EvalError> {
    if let Some(id) = expr.id.as_deref() {
        if let Some(value) = memo.get(id) {
            return Ok(value.clone());
        }
    }

    let value = match &expr.node {
        Node::Pure { value } => literal_value(value),
        Node::Channel { channel } => mono.read(channel)?,
        Node::Read { channel, object } => mono.read_object(channel, *object)?,
        Node::Call { op, args } => {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(eval_node(arg, defs, memo, mono)?);
            }
            mono.call(op, &values)?
        }
        Node::Ref { to } => {
            // `validate` 已拒绝悬空 Ref；这里保留 expect 作为内部不变量。
            let target = defs
                .get(to.as_str())
                .expect("参考求值前必须通过 Expr::validate");
            eval_node(target, defs, memo, mono)?
        }
        Node::Select { cond, then, r#else } => match eval_node(cond, defs, memo, mono)? {
            Value::Bool(true) => eval_node(then, defs, memo, mono)?,
            Value::Bool(false) => eval_node(r#else, defs, memo, mono)?,
            other => {
                return Err(EvalError::SelectCondition {
                    actual: crate::program::value_kind(&other),
                })
            }
        },
    };

    if let Some(id) = expr.id.as_deref() {
        memo.insert(id, value.clone());
    }
    Ok(value)
}

/// 参考求值器的 pass 形态（注册项 / 对拍入口）。
pub struct ReferenceEvaluator<'a> {
    mono: &'a dyn Mono,
}

impl<'a> ReferenceEvaluator<'a> {
    pub fn new(mono: &'a dyn Mono) -> Self {
        Self { mono }
    }

    /// 求值（调用方必须已校验过表达式；`CompiledExpr::compile` 会做这一步）。
    pub fn run(&self, expr: &Expr) -> Result<Value, EvalError> {
        eval_reference(expr, self.mono)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ChannelRef, OpRef};
    use std::cell::Cell;

    struct ScalarMono {
        time: f64,
        calls: Cell<usize>,
    }

    impl Mono for ScalarMono {
        fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
            match channel.name.as_str() {
                "time" => Ok(Value::F64(self.time)),
                _ => Err(EvalError::MissingChannel {
                    channel: channel.clone(),
                }),
            }
        }

        fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
            self.calls.set(self.calls.get() + 1);
            let number = |index: usize| -> Result<f64, EvalError> {
                args.get(index)
                    .and_then(Value::as_f64)
                    .ok_or_else(|| EvalError::op(op, format!("第 {index} 个实参不是数值")))
            };
            match op.name.as_str() {
                "sin" => Ok(Value::F64(number(0)?.sin())),
                "mul" => Ok(Value::F64(number(0)? * number(1)?)),
                "add" => Ok(Value::F64(number(0)? + number(1)?)),
                other => Err(EvalError::op(op, format!("未知算子 `{other}`"))),
            }
        }
    }

    fn math(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    #[test]
    fn shared_nodes_are_evaluated_once() {
        let h1 = Expr::call(
            math("sin"),
            [
                Expr::channel(ChannelRef::builtin("time")),
                Expr::pure(1.0),
            ],
        )
        .with_id("h1");
        let root = Expr::call(
            math("add"),
            [h1.clone(), Expr::reference("h1")],
        );
        let mono = ScalarMono {
            time: 1.0,
            calls: Cell::new(0),
        };
        let value = eval_reference(&root, &mono).unwrap();
        assert_eq!(value, Value::F64(2.0 * 1.0_f64.sin()));
        // sin 只调一次（共享节点），add 一次。
        assert_eq!(mono.calls.get(), 2);
    }

    #[test]
    fn select_is_lazy_and_requires_bool() {
        let expr = Expr::select(
            Expr::pure(false),
            Expr::channel(ChannelRef::builtin("missing")),
            Expr::pure(2.0),
        );
        let mono = ScalarMono {
            time: 0.0,
            calls: Cell::new(0),
        };
        assert_eq!(eval_reference(&expr, &mono).unwrap(), Value::F64(2.0));

        let bad = Expr::select(Expr::pure(1.0), Expr::pure(1.0), Expr::pure(0.0));
        assert_eq!(
            eval_reference(&bad, &mono).unwrap_err(),
            EvalError::SelectCondition { actual: "scalar" }
        );
    }
}
