//! **Pass 抽象**：把已有的能力收进一个约定（不是为假想需求预付）。
//!
//! # 两类 pass
//!
//! - **analysis**：只读 IR、写 **side table**（类型 / 区间 / revision）；可插拔、可动态。
//! - **transform**：写 IR；若**改变语义**则必须固定、进产物、集合进指纹（硬约束 2）。
//!
//! 本模块只提供**元数据 + 注册表**，不引入调度器：现有 5 个 pass 的调用点保持原位，
//! 只是各自登记一条描述（`ze-pod-func` / `ze-scene-core` 各自补自己的条目）。
//!
//! # 为什么参考求值器也登记
//!
//! 它和 lowered 求值器是同一语义的两个实现。把两者都登记成 pass，才能在
//! `cargo verify` 里**自动**跑对拍，而不是靠人记得"改化简要重录基线"。

use ze_type::{Blame, NameKind, ReflectError};

use crate::expr::{Expr, ExprError};
use crate::program::{CompiledExpr, EvalError, Mono, Value};
use crate::reference::ReferenceEvaluator;

/// pass 的类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassKind {
    /// 只读 IR、写 side table。
    Analysis,
    /// 写 IR。
    Transform,
}

/// pass 是否保持语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Semantics {
    /// 语义保持（可插拔、可动态）。
    Preserving,
    /// 语义改变（固定、进产物、集合进指纹）。
    Changing,
}

/// 一条 pass 的注册信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassInfo {
    /// 稳定名字（`expr.lower`）。
    pub name: &'static str,
    pub kind: PassKind,
    pub semantics: Semantics,
    /// 实现所属 crate（诊断用）。
    pub owner: &'static str,
}

/// 注册表：只保存描述，不保存 trait object（调度留给宿主）。
#[derive(Clone, Debug, Default)]
pub struct PassRegistry {
    entries: Vec<PassInfo>,
}

impl PassRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一条 pass；重名是**显式错误**（静默覆盖会让指纹与调度各说各话）。
    pub fn register(&mut self, info: PassInfo) -> Result<(), PassRegistryError> {
        if self.entries.iter().any(|entry| entry.name == info.name) {
            return Err(PassRegistryError::Duplicate(ReflectError::duplicate(
                NameKind::Pass,
                info.name,
                Blame::Assembly,
            )));
        }
        self.entries.push(info);
        Ok(())
    }

    pub fn entries(&self) -> &[PassInfo] {
        &self.entries
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.entries.iter().map(|entry| entry.name).collect()
    }

    /// 语义改变的 pass 集合（进指纹用）。
    pub fn semantic_changing(&self) -> Vec<&'static str> {
        self.entries
            .iter()
            .filter(|entry| entry.semantics == Semantics::Changing)
            .map(|entry| entry.name)
            .collect()
    }
}

/// 注册表错误。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PassRegistryError {
    #[error(transparent)]
    Duplicate(ReflectError),
}

/// 分析 pass：只读 IR。
pub trait AnalysisPass {
    type Ir;
    type Out;
    const INFO: PassInfo;

    fn run(&self, ir: &Self::Ir) -> Self::Out;
}

/// 变换 pass：读 IR、产出新 IR。
pub trait TransformPass {
    type Ir;
    type Out;
    const INFO: PassInfo;

    fn run(&self, ir: &Self::Ir) -> Self::Out;
}

// ─── 内核已有的 pass（原 `ze-func` 的 validate / compile / eval）─────────────

/// 结构校验（重名 id / 悬空 Ref / 环）。
pub struct Validate;

impl AnalysisPass for Validate {
    type Ir = Expr;
    type Out = Result<(), ExprError>;

    const INFO: PassInfo = PassInfo {
        name: "expr.validate",
        kind: PassKind::Analysis,
        semantics: Semantics::Preserving,
        owner: "ze-comb",
    };

    fn run(&self, ir: &Expr) -> Self::Out {
        ir.validate()
    }
}

/// lowering：`Expr` → 后序程序（[`CompiledExpr`]）。
pub struct Lower;

impl TransformPass for Lower {
    type Ir = Expr;
    type Out = Result<CompiledExpr, ExprError>;

    const INFO: PassInfo = PassInfo {
        name: "expr.lower",
        kind: PassKind::Transform,
        semantics: Semantics::Preserving,
        owner: "ze-comb",
    };

    fn run(&self, ir: &Expr) -> Self::Out {
        CompiledExpr::compile(ir)
    }
}

/// lowered 求值器（性能路径）。
pub struct LoweredEval<'a> {
    pub mono: &'a dyn Mono,
}

impl LoweredEval<'_> {
    pub const INFO: PassInfo = PassInfo {
        name: "expr.eval.lowered",
        kind: PassKind::Analysis,
        semantics: Semantics::Preserving,
        owner: "ze-comb",
    };

    pub fn run(&self, program: &CompiledExpr) -> Result<Value, EvalError> {
        program.eval(self.mono)
    }
}

/// 参考求值器（语义基准；tree-walking）。
pub struct ReferenceEval<'a> {
    pub mono: &'a dyn Mono,
}

impl ReferenceEval<'_> {
    pub const INFO: PassInfo = PassInfo {
        name: "expr.eval.reference",
        kind: PassKind::Analysis,
        semantics: Semantics::Preserving,
        owner: "ze-comb",
    };

    pub fn run(&self, expr: &Expr) -> Result<Value, EvalError> {
        ReferenceEvaluator::new(self.mono).run(expr)
    }
}

/// 内核侧注册表：`validate` / `lower` / 两个求值器。
///
/// 其余 pass（infer / analyze / revisions / read_order）由各自 crate 追加，
/// 避免让 `ze-comb` 反向知道 pod / 场景的词汇。
pub fn kernel_registry() -> PassRegistry {
    let mut registry = PassRegistry::new();
    registry
        .register(Validate::INFO)
        .expect("内核 pass 名唯一");
    registry.register(Lower::INFO).expect("内核 pass 名唯一");
    registry
        .register(LoweredEval::INFO)
        .expect("内核 pass 名唯一");
    registry
        .register(ReferenceEval::INFO)
        .expect("内核 pass 名唯一");
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_registry_lists_the_existing_passes() {
        let registry = kernel_registry();
        assert_eq!(
            registry.names(),
            vec![
                "expr.validate",
                "expr.lower",
                "expr.eval.lowered",
                "expr.eval.reference"
            ]
        );
        // 现有内核 pass 全部语义保持；没有偷偷改变语义的变换。
        assert!(registry.semantic_changing().is_empty());
    }

    #[test]
    fn duplicate_registration_is_explicit() {
        let mut registry = PassRegistry::new();
        registry.register(Validate::INFO).unwrap();
        let PassRegistryError::Duplicate(diagnostic) =
            registry.register(Validate::INFO).unwrap_err();
        assert_eq!(diagnostic.name(), Some("expr.validate"));
        assert_eq!(diagnostic.class(), "重名");
    }

    /// 参考求值器与 lowered 求值器逐位一致（抽象成立的证据）。
    #[test]
    fn reference_and_lowered_agree() {
        use crate::expr::{ChannelRef, OpRef};
        use std::cell::Cell;

        struct Mono0 {
            time: f64,
            calls: Cell<usize>,
        }
        impl Mono for Mono0 {
            fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
                if channel.name == "time" {
                    Ok(Value::F64(self.time))
                } else {
                    Err(EvalError::MissingChannel {
                        channel: channel.clone(),
                    })
                }
            }
            fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
                self.calls.set(self.calls.get() + 1);
                let x = args[0].as_f64().unwrap();
                match op.name.as_str() {
                    "sin" => Ok(Value::F64(x.sin())),
                    "mul" => Ok(Value::F64(x * args[1].as_f64().unwrap())),
                    other => Err(EvalError::op(op, format!("未知算子 `{other}`"))),
                }
            }
        }

        let math = |name: &str| OpRef::new("zela.math", name);
        let expr = Expr::call(
            math("mul"),
            [
                Expr::call(math("sin"), [Expr::channel(ChannelRef::builtin("time"))]),
                Expr::pure(2.0),
            ],
        );
        for time in [0.0, 0.25, 1.0, std::f64::consts::PI] {
            let mono = Mono0 {
                time,
                calls: Cell::new(0),
            };
            let lowered = Lower.run(&expr).unwrap();
            let a = LoweredEval { mono: &mono }.run(&lowered).unwrap();
            let b = ReferenceEval { mono: &mono }.run(&expr).unwrap();
            assert_eq!(a, b, "t = {time} 时两个求值器必须逐位一致");
        }
    }
}
