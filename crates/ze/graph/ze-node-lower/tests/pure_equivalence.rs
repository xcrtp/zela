//! **CF-c 对拍**：纯函数节点图 lowering 成 `Expr` 后，求值结果与
//! `LazyPullEval`（节点自己的运行时）一致。
//!
//! 两条路径走同一张图、同样的浮点运算与同样的先后：
//!
//! ```text
//! 节点图 --LazyPullEval--> 节点值
//! 节点图 --lowering-----> Expr --CompiledExpr--> 表达式值
//! ```
//!
//! 因此断言是**逐位相同**（不是近似）。状态节点（需要跨帧折叠）不在本用例范围，见 CF-d。

use std::any::Any;

use ze_comb::{
    ChannelRef, CompiledExpr, EvalError as CombEvalError, Expr, Mono, OpRef, Value,
};
use ze_graph_algo::KahnSorter;
use ze_graph_eval::LazyPullEval;
use ze_node_core::{
    DirtyPolicy, EvalError, EvalErrorPolicy, EvalStrategy, ExecContext, Graph, InputSocket,
    InputSocketId, NamedTypeTag, Node, NodeExecutor, NodeId, NodeMeta, NodeValue, OutputSocket,
    OutputSocketId, SimpleGraph, TypeTag,
};
use ze_node_lower::{LowerError, Lowering, RuleError};

// ---------------------------------------------------------------------------
// 测试节点：一个结构体 + 一个 kind 枚举（不为每个类型手写五遍 trait）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// 0 入 1 出：f64 常量。
    ConstF64,
    /// 0 入 1 出：Vec2 常量。
    ConstVec2,
    /// 2 入 1 出：f64 加法。
    AddF64,
    /// 2 入 1 出：Vec2 加法。
    AddVec2,
    /// 2 入 1 出：f64 乘法。
    MulF64,
}

impl Kind {
    fn type_name(self) -> &'static str {
        match self {
            Kind::ConstF64 => "test.const.f64",
            Kind::ConstVec2 => "test.const.vec2",
            Kind::AddF64 => "test.add.f64",
            Kind::AddVec2 => "test.add.vec2",
            Kind::MulF64 => "test.mul.f64",
        }
    }

    fn input_count(self) -> usize {
        match self {
            Kind::ConstF64 | Kind::ConstVec2 => 0,
            Kind::AddF64 | Kind::AddVec2 | Kind::MulF64 => 2,
        }
    }

    fn payload(self) -> Payload {
        match self {
            Kind::ConstF64 | Kind::AddF64 | Kind::MulF64 => Payload::F64(0.0),
            Kind::ConstVec2 | Kind::AddVec2 => Payload::Vec2([0.0, 0.0]),
        }
    }
}

/// 测试节点的负载：标量或二维向量。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Payload {
    F64(f64),
    Vec2([f32; 2]),
}

#[derive(Debug)]
struct PureNode {
    meta: NodeMeta,
    inputs: Vec<InputSocket>,
    outputs: Vec<OutputSocket>,
    kind: Kind,
    payload: Payload,
}

impl PureNode {
    fn new(kind: Kind, payload: Payload) -> Self {
        let tag: Box<dyn TypeTag> = Box::new(NamedTypeTag::new("f64"));
        Self {
            meta: NodeMeta::new(kind.type_name(), 1),
            inputs: (0..kind.input_count())
                .map(|i| InputSocket::new(tag.clone()).with_index(i as u32))
                .collect(),
            outputs: vec![OutputSocket::new(tag).with_index(0)],
            kind,
            payload,
        }
    }

    fn scalar(value: f64) -> Self {
        Self::new(Kind::ConstF64, Payload::F64(value))
    }

    fn vec2(x: f32, y: f32) -> Self {
        Self::new(Kind::ConstVec2, Payload::Vec2([x, y]))
    }

    fn binary(kind: Kind) -> Self {
        Self::new(kind, kind.payload())
    }
}

impl Node for PureNode {
    fn meta(&self) -> &NodeMeta {
        &self.meta
    }
    fn inputs(&self) -> &[InputSocket] {
        &self.inputs
    }
    fn outputs(&self) -> &[OutputSocket] {
        &self.outputs
    }
    fn dirty_policy(&self) -> DirtyPolicy {
        DirtyPolicy::Always
    }
    fn state_type(&self) -> Box<dyn TypeTag> {
        Box::new(NamedTypeTag::new("()"))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ---------------------------------------------------------------------------
// 节点侧：LazyPullEval 的 executor（语义与 lowering 规则同源）
// ---------------------------------------------------------------------------

fn input_f64(ctx: &mut dyn ExecContext, node: NodeId, index: u32) -> Result<f64, EvalError> {
    let value = ctx.peek_input(InputSocketId::new(node, index))?;
    value
        .downcast_ref::<f64>()
        .copied()
        .ok_or_else(|| EvalError::new("输入不是 f64", node, None))
}

fn input_vec2(ctx: &mut dyn ExecContext, node: NodeId, index: u32) -> Result<[f32; 2], EvalError> {
    let value = ctx.peek_input(InputSocketId::new(node, index))?;
    value
        .downcast_ref::<[f32; 2]>()
        .copied()
        .ok_or_else(|| EvalError::new("输入不是 [f32; 2]", node, None))
}

struct PureExecutor;

impl NodeExecutor for PureExecutor {
    fn execute(&self, node: &dyn Node, ctx: &mut dyn ExecContext) -> Result<(), EvalError> {
        let here = ctx.current_node();
        let pure = node
            .as_any()
            .downcast_ref::<PureNode>()
            .ok_or_else(|| EvalError::new("节点不是 PureNode", here, None))?;
        let out = OutputSocketId::new(here, 0);
        match (pure.kind, pure.payload) {
            (Kind::ConstF64, Payload::F64(value)) => ctx.set_output(out, Box::new(value))?,
            (Kind::ConstVec2, Payload::Vec2(value)) => ctx.set_output(out, Box::new(value))?,
            (Kind::AddF64, _) => {
                let sum = input_f64(ctx, here, 0)? + input_f64(ctx, here, 1)?;
                ctx.set_output(out, Box::new(sum))?;
            }
            (Kind::MulF64, _) => {
                let product = input_f64(ctx, here, 0)? * input_f64(ctx, here, 1)?;
                ctx.set_output(out, Box::new(product))?;
            }
            (Kind::AddVec2, _) => {
                // 与 `zela.math::add` 的 Vec2 重载同序：逐分量相加。
                let a = input_vec2(ctx, here, 0)?;
                let b = input_vec2(ctx, here, 1)?;
                ctx.set_output(out, Box::new([a[0] + b[0], a[1] + b[1]]))?;
            }
            (kind, payload) => {
                return Err(EvalError::new(
                    format!("`{}` 的负载与类型不匹配：{payload:?}", kind.type_name()),
                    here,
                    None,
                ));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 表达式侧：lowering 规则 + 宿主单子
// ---------------------------------------------------------------------------

fn payload_of(node: &dyn Node) -> Result<Payload, RuleError> {
    node.as_any()
        .downcast_ref::<PureNode>()
        .map(|pure| pure.payload)
        .ok_or_else(|| RuleError::new("节点不是 PureNode"))
}

fn rule_const_f64(node: &dyn Node, _inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
    match payload_of(node)? {
        Payload::F64(value) => Ok(vec![Expr::pure(value)]),
        other => Err(RuleError::new(format!("不是 f64 常量：{other:?}"))),
    }
}

fn rule_const_vec2(node: &dyn Node, _inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
    match payload_of(node)? {
        Payload::Vec2(value) => Ok(vec![Expr::pure(value)]),
        other => Err(RuleError::new(format!("不是 Vec2 常量：{other:?}"))),
    }
}

fn binary(op: &str, inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
    let [a, b] = inputs else {
        return Err(RuleError::new("二元节点需要 2 个输入"));
    };
    Ok(vec![Expr::call(
        OpRef::new("zela.math", op),
        [a.clone(), b.clone()],
    )])
}

fn rule_add(_node: &dyn Node, inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
    binary("add", inputs)
}

fn rule_mul(_node: &dyn Node, inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
    binary("mul", inputs)
}

fn registry() -> Lowering {
    let mut lowering = Lowering::new();
    lowering
        .register("test.const.f64", rule_const_f64)
        .register("test.const.vec2", rule_const_vec2)
        .register("test.add.f64", rule_add)
        .register("test.add.vec2", rule_add)
        .register("test.mul.f64", rule_mul);
    lowering
}

/// 宿主单子：没有通道（纯函数图不读时间），算子语义交给 `zela.math`。
struct MathMono;

impl Mono for MathMono {
    fn read(&self, channel: &ChannelRef) -> Result<Value, CombEvalError> {
        Err(CombEvalError::MissingChannel {
            channel: channel.clone(),
        })
    }

    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, CombEvalError> {
        ze_func::eval(op, args).map_err(|error| CombEvalError::op(op, error.to_string()))
    }
}

/// 把某个节点的输出 lowering 成表达式并求值。
fn eval_lowered(lowering: &Lowering, graph: &dyn Graph, node: NodeId) -> Value {
    let lowered = lowering.lower(graph).expect("可 lowering");
    let expr = lowered.output(node, 0).expect("该节点有输出 0");
    expr.validate()
        .expect("lowering 产物必须结构合法（无悬空 Ref / 环）");
    CompiledExpr::compile(expr)
        .expect("可编译")
        .eval(&MathMono)
        .expect("可求值")
}

/// 节点侧求值：`LazyPullEval` 一次拉到目标输出。
fn eval_nodes(graph: &SimpleGraph, target: NodeId) -> Box<dyn NodeValue> {
    let mut eval = LazyPullEval::new(Box::new(KahnSorter::new()), EvalErrorPolicy::FailFast);
    eval.evaluate(graph, &PureExecutor, Some(OutputSocketId::new(target, 0)))
        .expect("节点图可求值");
    eval.cached_output(OutputSocketId::new(target, 0))
        .expect("目标输出已缓存")
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

/// 标量纯函数图：`(1.5 + 2.25) * 3.0`。
#[test]
fn scalar_pure_graph_matches_lazy_pull_eval() {
    let mut graph = SimpleGraph::default();
    let a = graph.add_node(Box::new(PureNode::scalar(1.5)));
    let b = graph.add_node(Box::new(PureNode::scalar(2.25)));
    let add = graph.add_node(Box::new(PureNode::binary(Kind::AddF64)));
    let three = graph.add_node(Box::new(PureNode::scalar(3.0)));
    let mul = graph.add_node(Box::new(PureNode::binary(Kind::MulF64)));
    graph
        .quick_connect(OutputSocketId::new(a, 0), InputSocketId::new(add, 0))
        .unwrap();
    graph
        .quick_connect(OutputSocketId::new(b, 0), InputSocketId::new(add, 1))
        .unwrap();
    graph
        .quick_connect(OutputSocketId::new(add, 0), InputSocketId::new(mul, 0))
        .unwrap();
    graph
        .quick_connect(OutputSocketId::new(three, 0), InputSocketId::new(mul, 1))
        .unwrap();

    let node_value = eval_nodes(&graph, mul);
    let scalar = *node_value.downcast_ref::<f64>().expect("节点侧输出 f64");
    assert_eq!(scalar, 11.25, "1.5 + 2.25 = 3.75，×3.0 = 11.25");

    assert_eq!(eval_lowered(&registry(), &graph, mul), Value::f64(scalar));
}

/// Vec2 纯函数图：逐分量相加。
#[test]
fn vec2_pure_graph_matches_lazy_pull_eval() {
    let mut graph = SimpleGraph::default();
    let a = graph.add_node(Box::new(PureNode::vec2(1.0, -2.0)));
    let b = graph.add_node(Box::new(PureNode::vec2(0.25, 8.0)));
    let add = graph.add_node(Box::new(PureNode::binary(Kind::AddVec2)));
    graph
        .quick_connect(OutputSocketId::new(a, 0), InputSocketId::new(add, 0))
        .unwrap();
    graph
        .quick_connect(OutputSocketId::new(b, 0), InputSocketId::new(add, 1))
        .unwrap();

    let node_value = eval_nodes(&graph, add);
    let vector = *node_value
        .downcast_ref::<[f32; 2]>()
        .expect("节点侧输出 [f32; 2]");
    assert_eq!(vector, [1.25, 6.0]);

    assert_eq!(eval_lowered(&registry(), &graph, add), Value::vec2(vector));
}

/// 中间结果被两个下游共享时，两条路径仍然一致。
#[test]
fn shared_intermediate_matches_lazy_pull_eval() {
    let mut graph = SimpleGraph::default();
    let a = graph.add_node(Box::new(PureNode::scalar(2.0)));
    let b = graph.add_node(Box::new(PureNode::scalar(5.0)));
    let add = graph.add_node(Box::new(PureNode::binary(Kind::AddF64)));
    let mul = graph.add_node(Box::new(PureNode::binary(Kind::MulF64)));
    graph
        .quick_connect(OutputSocketId::new(a, 0), InputSocketId::new(add, 0))
        .unwrap();
    graph
        .quick_connect(OutputSocketId::new(b, 0), InputSocketId::new(add, 1))
        .unwrap();
    // add 的输出扇出到 mul 的两个输入。
    graph
        .quick_connect(OutputSocketId::new(add, 0), InputSocketId::new(mul, 0))
        .unwrap();
    graph
        .quick_connect(OutputSocketId::new(add, 0), InputSocketId::new(mul, 1))
        .unwrap();

    let node_value = eval_nodes(&graph, mul);
    let scalar = *node_value.downcast_ref::<f64>().expect("f64");
    assert_eq!(scalar, 49.0, "(2 + 5) × (2 + 5) = 49");

    assert_eq!(eval_lowered(&registry(), &graph, mul), Value::f64(scalar));
}

/// 没有规则的节点类型是显式错误（不是"跳过这个节点"）。
#[test]
fn lowering_rejects_unregistered_pure_node() {
    let mut graph = SimpleGraph::default();
    let node = graph.add_node(Box::new(PureNode::scalar(1.0)));
    let error = Lowering::new().lower(&graph).unwrap_err();
    assert_eq!(
        error,
        LowerError::UnsupportedNode {
            node,
            type_name: "test.const.f64".into()
        }
    );
}

/// 规则拒绝时，错误里带着节点 id 与类型名（诊断不需要调用方再补）。
#[test]
fn rule_rejection_names_the_node() {
    let mut graph = SimpleGraph::default();
    let node = graph.add_node(Box::new(PureNode::scalar(1.0)));
    let mut lowering = Lowering::new();
    lowering.register(
        "test.const.f64",
        |_node: &dyn Node, _inputs: &[Expr]| Err(RuleError::new("本用例故意拒绝")),
    );
    let error = lowering.lower(&graph).unwrap_err();
    assert_eq!(
        error,
        LowerError::Rule {
            node,
            type_name: "test.const.f64".into(),
            reason: "本用例故意拒绝".into()
        }
    );
}
