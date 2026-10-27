//! **符号化简**（阶段 4）：常量折叠 + **层间迁移**（离散层 → 连续层）。
//!
//! # 两类规则
//!
//! 1. **常量折叠**：所有实参都是字面量的 `Call` 直接求值成 `Pure`。
//!    用的是同一份 [`crate::ops::eval`]，因此逐位结果与运行期一致。
//! 2. **层间迁移**：`NatRec` 若是**闭合**的（不含通道 / 组件读 / `Ref`），
//!    在装配期展开成 `Pure` 常量 —— 它从"非 pointwise 的离散折叠"变成
//!    "pointwise 的连续常量"，`seek` 从 O(n) 退回 O(1)。
//!
//! 不闭合的 `NatRec` 原样保留（不猜、不丢递归）。
//!
//! # 为什么这是"语义保持"pass
//!
//! 折叠只调用同一个算子实现；展开只重放同一个 `NatRec` 语义。因此结果与
//! 参考求值器**逐位一致**（不是容差内），并且不需要进指纹的"语义改变"集合。

use ze_comb::{Expr, Literal, Node, PassInfo, PassKind, PassRegistry, Semantics, TransformPass, Value};

use crate::ops;

/// 化简时允许展开的 `NatRec` 轮数上限（装配期成本护栏；超过则保留原节点）。
pub const SIMPLIFY_REC_LIMIT: i64 = 4096;

/// 化简一棵表达式（不动点只需一次：规则都是自底向上、结果不再触发新规则）。
pub fn simplify(expr: &Expr) -> Expr {
    simplify_node(expr)
}

/// 自底向上：**子节点递归统一走 [`Expr::map_children`]**，本节点只做折叠判定。
fn simplify_node(expr: &Expr) -> Expr {
    fold(expr.map_children(simplify_node))
}

/// 折叠规则（对已化简的节点做一次判定，不递归子节点）。
///
/// 三条规则都只在**本节点**上判定：常量 `Call`、字面量 `Select`、闭合 `NatRec`。
fn fold(expr: Expr) -> Expr {
    let Expr { id, node } = expr;
    match node {
        Node::Call { op, args } => match fold_call(&op, &args) {
            Some(value) => Expr {
                id,
                node: Node::Pure { value },
            },
            None => Expr {
                id,
                node: Node::Call { op, args },
            },
        },
        Node::Select { cond, then, r#else } => {
            // 带 id 的 Select 不折叠：单个 Expr 只带一个 id，折叠会丢引用目标。
            if id.is_none() {
                match &cond.node {
                    Node::Pure {
                        value: Literal::Bool(true),
                    } => return *then,
                    Node::Pure {
                        value: Literal::Bool(false),
                    } => return *r#else,
                    _ => {}
                }
            }
            Expr {
                id,
                node: Node::Select { cond, then, r#else },
            }
        }
        Node::NatRec { count, init, step } => {
            let candidate = Expr {
                id,
                node: Node::NatRec { count, init, step },
            };
            // 层间迁移：闭合的 NatRec 在装配期展开成常量。
            match eval_closed(&candidate, None).and_then(|value| value_to_literal(&value)) {
                Some(value) => Expr {
                    id: candidate.id.clone(),
                    node: Node::Pure { value },
                },
                None => candidate,
            }
        }
        other => Expr { id, node: other },
    }
}

/// 全字面量实参 ⇒ 直接求值（同一份算子实现）。
fn fold_call(op: &ze_comb::OpRef, args: &[Expr]) -> Option<Literal> {
    if op.namespace != crate::LIBRARY_NAMESPACE {
        return None;
    }
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        let Node::Pure { value } = &arg.node else {
            return None;
        };
        values.push(literal_to_value(value)?);
    }
    let result = ops::eval(op, &values).ok()?;
    value_to_literal(&result)
}

/// 闭合表达式求值（装配期）：不含通道 / 组件读 / `Ref`。
fn eval_closed(expr: &Expr, rec: Option<(&Value, i64)>) -> Option<Value> {
    let value = match &expr.node {
        Node::Pure { value } => literal_to_value(value)?,
        Node::Channel { .. } | Node::Read { .. } => return None,
        Node::Call { op, args } => {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(eval_closed(arg, rec)?);
            }
            ops::eval(op, &values).ok()?
        }
        Node::Select { cond, then, r#else } => match eval_closed(cond, rec)? {
            Value::Bool(true) => eval_closed(then, rec)?,
            Value::Bool(false) => eval_closed(r#else, rec)?,
            _ => return None,
        },
        Node::NatRec { count, init, step } => {
            let count = match eval_closed(count, rec)? {
                Value::I64(value) if (0..=SIMPLIFY_REC_LIMIT).contains(&value) => value,
                _ => return None,
            };
            let mut state = eval_closed(init, rec)?;
            let kind = state.kind();
            for index in 1..=count {
                let next = eval_closed(step, Some((&state, index)))?;
                if next.kind() != kind {
                    return None;
                }
                state = next;
            }
            state
        }
        Node::RecPrev => rec.map(|(prev, _)| prev.clone())?,
        Node::RecIndex => Value::I64(rec?.1),
        // `Ref` 需要整棵表达式的定义表；化简阶段保守地视为非闭合。
        Node::Ref { .. } => return None,
    };
    Some(value)
}

fn literal_to_value(literal: &Literal) -> Option<Value> {
    Some(match literal {
        Literal::F32(value) => Value::F64(f64::from(*value)),
        Literal::F64(value) => Value::F64(*value),
        Literal::I32(value) => Value::F64(f64::from(*value)),
        Literal::I64(value) => Value::I64(*value),
        Literal::Bool(value) => Value::Bool(*value),
        Literal::Text(value) => Value::Text(value.clone()),
        Literal::Vec2(value) => Value::Vec2(*value),
        Literal::Vec3(value) => Value::Vec3(*value),
    })
}

fn value_to_literal(value: &Value) -> Option<Literal> {
    Some(match value {
        Value::F64(value) => Literal::F64(*value),
        Value::I64(value) => Literal::I64(*value),
        Value::Bool(value) => Literal::Bool(*value),
        Value::Text(value) => Literal::Text(value.clone()),
        Value::Vec2(value) => Literal::Vec2(*value),
        Value::Vec3(value) => Literal::Vec3(*value),
        Value::Bytes(_) => return None,
    })
}

/// `expr.simplify` 的 pass 形态。
pub struct Simplify;

impl TransformPass for Simplify {
    type Ir = Expr;
    type Out = Expr;

    const INFO: PassInfo = PassInfo {
        name: "expr.simplify",
        kind: PassKind::Transform,
        semantics: Semantics::Preserving,
        owner: "ze-func",
    };

    fn run(&self, ir: &Expr) -> Self::Out {
        simplify(ir)
    }
}

/// 把本 crate 的 pass 追加到注册表。
pub fn register_passes(registry: &mut PassRegistry) {
    registry
        .register(Simplify::INFO)
        .expect("simplify pass 名唯一");
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_comb::{OpRef, Value};

    fn math(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    fn pure_f64(value: f64) -> Expr {
        Expr::pure(value)
    }

    #[test]
    fn constant_calls_fold_to_pure() {
        let expr = Expr::call(
            math("add"),
            [pure_f64(1.0), Expr::call(math("mul"), [pure_f64(2.0), pure_f64(3.0)])],
        );
        let simplified = simplify(&expr);
        assert_eq!(simplified.node, Node::Pure { value: Literal::F64(7.0) });
    }

    #[test]
    fn select_with_a_literal_condition_folds_to_a_branch() {
        let expr = Expr::select(Expr::pure(true), pure_f64(1.0), pure_f64(2.0));
        assert_eq!(
            simplify(&expr).node,
            Node::Pure {
                value: Literal::F64(1.0)
            }
        );
    }

    /// **层间迁移**：闭合 `NatRec`（离散层）→ `Pure`（连续层，pointwise）。
    #[test]
    fn a_closed_nat_rec_migrates_to_the_continuous_layer() {
        let expr = Expr::nat_rec(
            Expr::pure(4_i64),
            pure_f64(1.0),
            Expr::call(math("add"), [Expr::rec_prev(), pure_f64(2.0)]),
        );
        let simplified = simplify(&expr);
        assert_eq!(
            simplified.node,
            Node::Pure {
                value: Literal::F64(9.0)
            },
            "闭合 NatRec 应展开成常量"
        );
    }

    /// 不闭合的 `NatRec` 原样保留（不猜、不丢递归）。
    #[test]
    fn an_open_nat_rec_is_kept() {
        let expr = Expr::nat_rec(
            Expr::pure(4_i64),
            Expr::channel(ze_comb::ChannelRef::builtin("time")),
            Expr::call(math("add"), [Expr::rec_prev(), pure_f64(2.0)]),
        );
        let simplified = simplify(&expr);
        assert!(matches!(simplified.node, Node::NatRec { .. }));
    }

    /// **验收**：化简前后与参考求值器逐位一致。
    #[test]
    fn simplification_preserves_the_reference_value() {
        struct Mono;
        impl ze_comb::Mono for Mono {
            fn read(&self, channel: &ze_comb::ChannelRef) -> Result<Value, ze_comb::EvalError> {
                Err(ze_comb::EvalError::MissingChannel {
                    channel: channel.clone(),
                })
            }
            fn call(
                &self,
                op: &OpRef,
                args: &[Value],
            ) -> Result<Value, ze_comb::EvalError> {
                ops::eval(op, args)
            }
        }

        let expr = Expr::call(
            math("add"),
            [
                Expr::nat_rec(
                    Expr::pure(3_i64),
                    pure_f64(1.0),
                    Expr::call(math("add"), [Expr::rec_prev(), pure_f64(2.0)]),
                ),
                Expr::call(math("mul"), [pure_f64(2.0), pure_f64(3.0)]),
            ],
        );
        let reference = ze_comb::eval_reference(&expr, &Mono).unwrap();
        let simplified = simplify(&expr);
        let after = ze_comb::eval_reference(&simplified, &Mono).unwrap();
        assert_eq!(reference, after);
        assert_eq!(reference, Value::F64(13.0));
    }

    #[test]
    fn pass_registers_with_stable_metadata() {
        let mut registry = ze_comb::kernel_registry();
        register_passes(&mut registry);
        assert!(registry.names().contains(&"expr.simplify"));
        let info = registry
            .entries()
            .iter()
            .find(|info| info.name == "expr.simplify")
            .expect("已注册");
        assert_eq!(info.kind, PassKind::Transform);
        assert_eq!(info.semantics, Semantics::Preserving, "折叠/展开不改语义");
    }
}
