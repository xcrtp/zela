//! **装配期程序的两种形态**：`lowered`（语义基准）+ 可选 `typed`（单态化快路径）。
//!
//! 这是**领域无关**的组合策略：它不知道任何算子词汇，只知道"能不能单态化"。
//! 具体解析器由调用方注入（[`OpResolver`]，如 `ze_func::monomorphizer()`）——
//! 内核因此不依赖任何算子库。
//!
//! # 为什么默认回落而不是报错
//!
//! 单态化失败（含 `Select` / `NatRec` / 未知通道类型）只说明"这条表达式不走快路径"，
//! **不是**装配错误：语义永远由 `lowered` 保证。两条路径的求值结果由
//! `ze-func/tests/typed_equivalence.rs` 逐位对拍。

use crate::expr::{Expr, ExprError};
use crate::program::{CompiledExpr, EvalError, Mono, Value};
use crate::typed::{OpResolver, TypedProgram};

/// 表达式程序：`lowered` 恒有、`typed` 可选。
#[derive(Debug, Clone)]
pub struct ExprProgram {
    lowered: CompiledExpr,
    typed: Option<TypedProgram>,
}

impl ExprProgram {
    /// 装配期编译：先编译 lowered（失败即装配期错误），再**尽力**单态化。
    ///
    /// typed 失败不是错误："这个表达式含 `Select` / `NatRec` / 未知通道" 只说明
    /// 它不走快路径，语义仍由 lowered 保证。
    pub fn compile(expr: &Expr, resolver: &dyn OpResolver) -> Result<Self, ExprError> {
        let lowered = CompiledExpr::compile(expr)?;
        let typed = TypedProgram::compile(expr, resolver);
        Ok(Self { lowered, typed })
    }

    /// 只包一个已编译的 lowered 程序（测试 / 手写装配：不启用快路径）。
    pub fn lowered_only(lowered: CompiledExpr) -> Self {
        Self {
            lowered,
            typed: None,
        }
    }

    /// 求值：typed 可用则走快路径，否则 lowered（结果逐位相同）。
    pub fn eval(&self, mono: &dyn Mono) -> Result<Value, EvalError> {
        match &self.typed {
            Some(program) => program.eval(mono),
            None => self.lowered.eval(mono),
        }
    }

    /// 是否含离散层节点（`NatRec` 等）—— 与 `CompiledExpr::requires_replay` 同源。
    pub fn requires_replay(&self) -> bool {
        self.lowered.requires_replay()
    }

    /// 语义基准程序（读集分析 / 诊断用）。
    pub fn lowered(&self) -> &CompiledExpr {
        &self.lowered
    }

    /// 是否真的走了单态快路径（诊断 / 用例用）。
    pub fn is_typed(&self) -> bool {
        self.typed.is_some()
    }
}

impl From<CompiledExpr> for ExprProgram {
    fn from(lowered: CompiledExpr) -> Self {
        Self::lowered_only(lowered)
    }
}

/// **不单态化**的解析器：总是 `None`（回落 lowered）。
///
/// 给"只想编译 lowered"的调用方一个显式选择，而不是让它们传一个假实现。
#[derive(Clone, Copy, Debug, Default)]
pub struct NoMonomorphizer;

impl OpResolver for NoMonomorphizer {
    fn resolve(&self, _op: &crate::expr::OpRef, _args: &[crate::typed::SlotType]) -> Option<crate::typed::TypedCall> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ChannelRef, OpRef};
    use crate::typed::{OpResolver, SlotType, TypedCall, TypedOp};

    fn math(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    /// 宿主只实现 `add` 的动态语义（typed 路径不该调用它）。
    struct Host;

    impl Mono for Host {
        fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
            Err(EvalError::MissingChannel {
                channel: channel.clone(),
            })
        }

        fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
            match op.name.as_str() {
                "add" => Ok(Value::f64(
                    args[0].as_f64().expect("f64") + args[1].as_f64().expect("f64"),
                )),
                other => Err(EvalError::op(op, format!("未知算子 `{other}`"))),
            }
        }
    }

    /// 只认 `add(f64, f64)` 的假解析器：证明单态化机制**不依赖任何算子库**。
    struct AddOnly;

    impl OpResolver for AddOnly {
        fn resolve(&self, op: &OpRef, args: &[SlotType]) -> Option<TypedCall> {
            match (op.name.as_str(), args) {
                ("add", [SlotType::F64, SlotType::F64]) => Some(TypedCall {
                    op: TypedOp::F64xF64ToF64(|a, b| a + b),
                    ret: SlotType::F64,
                }),
                _ => None,
            }
        }
    }

    fn add_expr() -> Expr {
        Expr::call(math("add"), [Expr::pure(1.5), Expr::pure(2.25)])
    }

    #[test]
    fn typed_path_is_domain_free() {
        let host = Host;
        let program = ExprProgram::compile(&add_expr(), &AddOnly).expect("可编译");
        assert!(program.is_typed(), "解析器认识 add ⇒ 应走单态路径");
        assert_eq!(program.eval(&host).expect("可求值"), Value::f64(3.75));

        // 同一棵树的 lowered 结果必须逐位相同。
        let lowered = program.lowered().eval(&host).expect("可求值");
        assert_eq!(lowered, Value::f64(3.75));
    }

    #[test]
    fn no_monomorphizer_keeps_the_lowered_path() {
        let host = Host;
        let program = ExprProgram::compile(&add_expr(), &NoMonomorphizer).expect("可编译");
        assert!(!program.is_typed(), "显式不单态化 ⇒ 保持 lowered");
        assert_eq!(program.eval(&host).expect("可求值"), Value::f64(3.75));
        assert!(!program.requires_replay());
    }

    #[test]
    fn unsupported_operator_falls_back_without_error() {
        let host = Host;
        let expr = Expr::call(math("sin"), [Expr::pure(0.5)]);
        let program = ExprProgram::compile(&expr, &AddOnly).expect("回落不是错误");
        assert!(!program.is_typed());
        // lowered 路径下宿主不认识 sin ⇒ 显式求值错误（语义基准仍然生效）。
        assert!(program.eval(&host).is_err());
    }
}
