//! 求值：**装配期建图 → 运行期定序**，效果只能从 [`Mono`] 进。
//!
//! # 为什么不复用 `LazyPullEval`
//!
//! `LazyPullEval` 把状态放在**求值器自己**里（`&mut self` + Mutex 缓存）⇒ 不可枚举、
//! 不可结晶。这里的状态只在 `Mono`（宿主注入的 M）里 ⇒ 可枚举、可结晶。
//! 运行时缓存只能作为**优化**存在，且必须能证明与「朴素重放 M」逐位等价。
//!
//! # 形状
//!
//! [`CompiledExpr`] 是一段**后序程序**：每个 `Step` 的实参索引都小于自身 ⇒ 求值就是
//! 一次线性扫描，无需运行时拓扑排序、无需脏传播。
//!
//! - 共享节点（带 `id` 的 `Def` + `Ref`）**只编译一次** ⇒ 帧内只求一次、广播给引用者；
//! - 程序本身**无内部可变性** ⇒ 每一帧都是重新扫描，不存在跨帧缓存；
//! - 清空缓存重算逐位相同 —— 因为根本没有缓存可清。
//!
//! # 不纯值
//!
//! 字面量 / 通道 / 算子之外没有别的叶子。通道由 [`Mono::read`] 提供，算子由
//! [`Mono::call`] 提供 —— **宿主是唯一的不纯入口**，且它的实现可枚举。

use std::collections::BTreeMap;

use crate::expr::{ChannelRef, Expr, ExprError, Literal, Node, OpRef};

/// 运行时值（跨通道 / 算子的最小集）。
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    F64(f64),
    Bool(bool),
    Text(String),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
}

impl Value {
    /// 取数值（`F32` / `F64` / `I32` 字面量都归一到这里）。
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F64(value) => Some(*value),
            _ => None,
        }
    }
}

/// 单子 `M`：宿主注入的**唯一效果入口**。
///
/// 实现方必须可枚举（时间 / 帧 / 事件 / 组件 / 环境），且 `call` 只做纯函数。
pub trait Mono {
    /// 读取一条通道。
    fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError>;

    /// 调用一个叶子算子。
    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError>;
}

/// 求值错误：显式、带算子来源（§7.6 的运行时对应物）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    /// 通道没有值 —— 说明宿主没有注入它（不是静默 0）。
    #[error("通道 `{channel}` 未由宿主注入")]
    MissingChannel { channel: ChannelRef },
    /// 算子求值失败（类型 / 定义域 / 未知算子）。
    #[error("算子 `{op}` 求值失败：{message}")]
    Op { op: String, message: String },
}

impl EvalError {
    /// 构造一条带算子限定名的错误。
    pub fn op(op: &OpRef, message: impl Into<String>) -> Self {
        Self::Op {
            op: op.qualified(),
            message: message.into(),
        }
    }
}

/// 后序程序的一步。
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Pure(Literal),
    Channel(ChannelRef),
    Call { op: OpRef, args: Vec<usize> },
}

/// 编译后的表达式（装配期一次，运行期线性扫描）。
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledExpr {
    steps: Vec<Step>,
    root: usize,
}

impl CompiledExpr {
    /// 编译并校验（悬空 `Ref` / 环 / 重名 id 都在这里显式失败）。
    pub fn compile(expr: &Expr) -> Result<Self, ExprError> {
        expr.validate()?;
        if let Some(name) = first_unimplemented(expr) {
            return Err(ExprError::UnsupportedNode(name));
        }
        let defs = crate::expr::definitions(expr);
        let mut steps = Vec::new();
        let mut memo = BTreeMap::new();
        let root = compile_node(expr, &defs, &mut memo, &mut steps);
        Ok(Self { steps, root })
    }

    /// 程序步数（共享节点只计一次）。
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// 程序（只读；用于诊断 / 导出）。
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// 求值一次。
    ///
    /// 每次调用都从头扫描 ⇒ **帧内 memo 由程序结构决定，不跨帧**。
    pub fn eval(&self, mono: &dyn Mono) -> Result<Value, EvalError> {
        let mut values: Vec<Value> = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            let value = match step {
                Step::Pure(literal) => literal_value(literal),
                Step::Channel(channel) => mono.read(channel)?,
                Step::Call { op, args } => {
                    let args: Vec<Value> = args.iter().map(|&index| values[index].clone()).collect();
                    mono.call(op, &args)?
                }
            };
            values.push(value);
        }
        Ok(values[self.root].clone())
    }
}

/// 只有格式、尚未实现求值的节点（提案 #6 D）。
fn first_unimplemented(expr: &Expr) -> Option<&'static str> {
    if let Node::Select { .. } = &expr.node {
        return Some("select");
    }
    expr.node.children().into_iter().find_map(first_unimplemented)
}

fn literal_value(literal: &Literal) -> Value {
    match literal {
        Literal::F32(value) => Value::F64(f64::from(*value)),
        Literal::F64(value) => Value::F64(*value),
        Literal::I32(value) => Value::F64(f64::from(*value)),
        Literal::Bool(value) => Value::Bool(*value),
        Literal::Text(value) => Value::Text(value.clone()),
        Literal::Vec2(value) => Value::Vec2(*value),
        Literal::Vec3(value) => Value::Vec3(*value),
    }
}

fn compile_node<'a>(
    expr: &'a Expr,
    defs: &BTreeMap<&'a str, &'a Expr>,
    memo: &mut BTreeMap<&'a str, usize>,
    steps: &mut Vec<Step>,
) -> usize {
    if let Some(id) = expr.id.as_deref() {
        if let Some(index) = memo.get(id) {
            return *index;
        }
    }

    let index = match &expr.node {
        Node::Pure { value } => {
            steps.push(Step::Pure(value.clone()));
            steps.len() - 1
        }
        Node::Channel { channel } => {
            steps.push(Step::Channel(channel.clone()));
            steps.len() - 1
        }
        Node::Call { op, args } => {
            let args: Vec<usize> = args
                .iter()
                .map(|arg| compile_node(arg, defs, memo, steps))
                .collect();
            steps.push(Step::Call {
                op: op.clone(),
                args,
            });
            steps.len() - 1
        }
        Node::Ref { to } => {
            // `validate` 已拒绝悬空 Ref；这里保留 expect 作为内部不变量。
            let target = defs
                .get(to.as_str())
                .expect("compile 之前必须通过 Expr::validate");
            compile_node(target, defs, memo, steps)
        }
        // `compile` 入口已用 `first_unimplemented` 显式拒绝（不静默算错）。
        Node::Select { .. } => unreachable!("Select 在 compile 入口被拒绝"),
    };

    if let Some(id) = expr.id.as_deref() {
        memo.insert(id, index);
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Id;
    use std::cell::Cell;

    fn math(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    struct ScalarMono {
        time: f64,
        frame: u64,
        calls: Cell<usize>,
    }

    impl ScalarMono {
        fn new(time: f64, frame: u64) -> Self {
            Self {
                time,
                frame,
                calls: Cell::new(0),
            }
        }
    }

    impl Mono for ScalarMono {
        fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
            match (channel.namespace.as_str(), channel.name.as_str()) {
                ("", "time") => Ok(Value::F64(self.time)),
                ("", "frame") => Ok(Value::F64(self.frame as f64)),
                _ => Err(EvalError::MissingChannel {
                    channel: channel.clone(),
                }),
            }
        }

        fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
            self.calls.set(self.calls.get() + 1);
            let num = |index: usize| -> Result<f64, EvalError> {
                args.get(index)
                    .and_then(Value::as_f64)
                    .ok_or_else(|| EvalError::op(op, format!("第 {index} 个实参不是数值")))
            };
            match op.name.as_str() {
                "sin" => Ok(Value::F64(num(0)?.sin())),
                "cos" => Ok(Value::F64(num(0)?.cos())),
                "mul" => Ok(Value::F64(num(0)? * num(1)?)),
                "add" => Ok(Value::F64(num(0)? + num(1)?)),
                "scale" => Ok(Value::F64(num(0)? * num(1)?)),
                "sum" => {
                    let mut total = 0.0;
                    for index in 0..args.len() {
                        total += num(index)?;
                    }
                    Ok(Value::F64(total))
                }
                other => Err(EvalError::op(op, format!("未知算子 `{other}`"))),
            }
        }
    }

    /// 共享节点：`h1 = sin(t × 1)`，根 = `h1 + 2 × h1`。
    fn shared_tree() -> Expr {
        let h1 = Expr::call(math("sin"), [Expr::call(math("mul"), [
            Expr::channel(ChannelRef::builtin("time")),
            Expr::pure(1.0),
        ])])
        .with_id("h1");
        Expr::call(
            math("add"),
            [
                h1.clone(),
                Expr::call(math("scale"), [Expr::pure(2.0), Expr::reference("h1")]),
            ],
        )
    }

    #[test]
    fn shared_node_is_compiled_and_evaluated_once() {
        let program = CompiledExpr::compile(&shared_tree()).expect("编译成功");
        // 通道 + 纯量 + mul + sin + 纯量 + scale + add = 7 步（h1 只编译一次）。
        assert_eq!(program.step_count(), 7);

        let t = std::f64::consts::FRAC_PI_2;
        let mono = ScalarMono::new(t, 0);
        let value = program.eval(&mono).expect("求值成功");
        let sin = t.sin();
        assert!((value.as_f64().unwrap() - (sin + 2.0 * sin)).abs() < 1e-12);
        // 4 次算子调用（mul / sin / scale / add），共享节点没有重复调用。
        assert_eq!(mono.calls.get(), 4);
    }

    #[test]
    fn evaluation_is_frame_local_and_recomputable() {
        let program = CompiledExpr::compile(&shared_tree()).unwrap();
        let mono = ScalarMono::new(0.0, 0);
        let first = program.eval(&mono).unwrap();
        let second = program.eval(&mono).unwrap();
        assert_eq!(first, second, "同一 M ⇒ 同一结果（无隐藏状态）");
        assert_eq!(mono.calls.get(), 8, "每次求值都重算，不跨帧缓存");
    }

    #[test]
    fn frame_channel_comes_from_the_monad() {
        let expr = Expr::call(math("sin"), [Expr::channel(ChannelRef::builtin("frame"))]);
        let program = CompiledExpr::compile(&expr).unwrap();
        let mono = ScalarMono::new(0.0, 30);
        let value = program.eval(&mono).unwrap().as_f64().unwrap();
        assert!((value - 30.0_f64.sin()).abs() < 1e-12);
    }

    #[test]
    fn missing_channel_is_explicit() {
        let expr = Expr::channel(ChannelRef::builtin("event"));
        let program = CompiledExpr::compile(&expr).unwrap();
        let err = program.eval(&ScalarMono::new(0.0, 0)).unwrap_err();
        assert_eq!(
            err,
            EvalError::MissingChannel {
                channel: ChannelRef::builtin("event")
            }
        );
    }

    #[test]
    fn bad_ref_is_rejected_at_compile() {
        let expr = Expr::reference("missing");
        assert_eq!(
            CompiledExpr::compile(&expr).unwrap_err(),
            ExprError::DanglingRef(Id::from("missing"))
        );
    }

    #[test]
    fn unknown_operator_reports_its_source() {
        let expr = Expr::call(OpRef::new("zela.future", "warp"), []);
        let program = CompiledExpr::compile(&expr).unwrap();
        let err = program.eval(&ScalarMono::new(0.0, 0)).unwrap_err();
        assert_eq!(
            err,
            EvalError::Op {
                op: "zela.future::warp".to_string(),
                message: "未知算子 `warp`".to_string(),
            }
        );
    }

    /// **提案 #6 D**：`Select` 只有格式 ⇒ 编译期**显式未实现**（不静默算错）。
    #[test]
    fn select_is_explicitly_unimplemented() {
        let expr = Expr::select(Expr::pure(true), Expr::pure(1.0), Expr::pure(0.0));
        assert_eq!(
            CompiledExpr::compile(&expr).unwrap_err(),
            ExprError::UnsupportedNode("select")
        );
    }
}
