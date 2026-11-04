//! 注册表 + lowering 主体：拓扑序遍历，逐节点把「属性 + 已 lowering 的输入」
//! 交给该类型登记的规则，得到每个输出插槽一个 [`Expr`]。
//!
//! 顺序的两条来源必须分清：
//!
//! - **节点之间**：`KahnSorter`（tiebreak = `NodeId` 升序），不读 `all_nodes`；
//! - **输入插槽之间**：按插槽下标（`0..inputs().len()`），与节点声明顺序一致。

use std::collections::BTreeMap;

use ze_comb::Expr;
use ze_graph_algo::KahnSorter;
use ze_node_core::algo::{CycleError, TopoSort};
use ze_node_core::{Graph, InputSocketId, Node, NodeId};

use crate::error::{LowerError, RuleError};

/// 一条 lowering 规则：`(节点, 已 lowering 的输入)` ⇒ 每个输出插槽一个表达式。
///
/// 输入按插槽下标排列（`inputs()[i]` 对应 `inputs_expr[i]`）。规则拿到的是
/// `&dyn Node` —— 属性由规则自己取（同 crate 的节点可 downcast，通用节点可走
/// `NodeSpec`），本层不替它决定属性模型。
pub type Rule = fn(&dyn Node, &[Expr]) -> Result<Vec<Expr>, RuleError>;

/// 节点类型名 → 规则。键是 `meta().type_name`（数据，不是 Rust 类型名）。
#[derive(Clone, Default)]
pub struct Lowering {
    rules: BTreeMap<String, Rule>,
}

impl Lowering {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记 / 覆盖一个节点类型的规则。
    pub fn register(&mut self, type_name: impl Into<String>, rule: Rule) -> &mut Self {
        self.rules.insert(type_name.into(), rule);
        self
    }

    /// 该类型是否有规则。
    pub fn is_registered(&self, type_name: &str) -> bool {
        self.rules.contains_key(type_name)
    }

    /// 已登记的类型名（升序）。
    pub fn type_names(&self) -> impl Iterator<Item = &str> {
        self.rules.keys().map(String::as_str)
    }

    /// lowering 整张图。
    pub fn lower(&self, graph: &dyn Graph) -> Result<Lowered, LowerError> {
        let order = KahnSorter::new()
            .sort(graph)
            .map_err(|error| LowerError::Cycle {
                nodes: match error {
                    CycleError::Cycle { nodes } => nodes,
                    CycleError::Opaque => Vec::new(),
                },
            })?;

        let mut outputs: BTreeMap<NodeId, Vec<Expr>> = BTreeMap::new();
        for &node_id in &order {
            let node = graph
                .get_node(node_id)
                .ok_or(LowerError::MissingNode { node: node_id })?;
            let type_name = node.meta().type_name.clone();
            let rule = *self
                .rules
                .get(&type_name)
                .ok_or_else(|| LowerError::UnsupportedNode {
                    node: node_id,
                    type_name: type_name.clone(),
                })?;

            let mut inputs = Vec::with_capacity(node.inputs().len());
            for index in 0..node.inputs().len() {
                inputs.push(pull_input(graph, &outputs, node_id, index)?);
            }

            let produced = rule(node, &inputs).map_err(|error| LowerError::Rule {
                node: node_id,
                type_name: type_name.clone(),
                reason: error.to_string(),
            })?;
            if produced.len() != node.outputs().len() {
                return Err(LowerError::OutputArity {
                    node: node_id,
                    type_name,
                    expected: node.outputs().len(),
                    got: produced.len(),
                });
            }
            outputs.insert(node_id, produced);
        }

        Ok(Lowered { order, outputs })
    }
}

impl std::fmt::Debug for Lowering {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 不打印函数指针（地址不是产物信息），只打印已登记的类型名。
        f.debug_struct("Lowering")
            .field("type_names", &self.rules.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// 取某个输入插槽的上游表达式。
///
/// 上游集合的先后**没有定义**（`Graph::upstream` 返回 `Vec`）⇒ 先排序让诊断稳定；
/// 随后要求恰好一条：多连接的先后语义本层不替调用方决定。
fn pull_input(
    graph: &dyn Graph,
    outputs: &BTreeMap<NodeId, Vec<Expr>>,
    node: NodeId,
    index: usize,
) -> Result<Expr, LowerError> {
    let mut upstream = graph.upstream(InputSocketId::new(node, index as u32));
    upstream.sort();
    match upstream.as_slice() {
        [] => Err(LowerError::MissingInput { node, input: index }),
        [source] => {
            let exprs = outputs
                .get(&source.node())
                .ok_or(LowerError::MissingNode { node: source.node() })?;
            exprs
                .get(source.index() as usize)
                .cloned()
                .ok_or(LowerError::MissingOutput {
                    node: source.node(),
                    index: source.index() as usize,
                })
        }
        many => Err(LowerError::AmbiguousInput {
            node,
            input: index,
            count: many.len(),
        }),
    }
}

/// lowering 产物。
///
/// `order` 是确定性拓扑序（upstream 先、同层 `NodeId` 升序）；`outputs` 按
/// `NodeId` 索引（`BTreeMap` ⇒ 迭代顺序也确定）。
#[derive(Debug, Clone, PartialEq)]
pub struct Lowered {
    order: Vec<NodeId>,
    outputs: BTreeMap<NodeId, Vec<Expr>>,
}

impl Lowered {
    /// 确定性拓扑序。
    pub fn order(&self) -> &[NodeId] {
        &self.order
    }

    /// 某节点的全部输出表达式（长度 = 该节点输出插槽数）。
    pub fn node(&self, node: NodeId) -> Option<&[Expr]> {
        self.outputs.get(&node).map(Vec::as_slice)
    }

    /// 某节点某个输出插槽的表达式。
    pub fn output(&self, node: NodeId, index: usize) -> Option<&Expr> {
        self.outputs.get(&node)?.get(index)
    }

    /// 已 lowering 的节点数。
    pub fn len(&self) -> usize {
        self.outputs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.outputs.is_empty()
    }

    /// 按拓扑序迭代 `(节点, 输出表达式)`。
    pub fn iter_in_order(&self) -> impl Iterator<Item = (NodeId, &[Expr])> {
        self.order.iter().filter_map(|&node| {
            self.outputs
                .get(&node)
                .map(|exprs| (node, exprs.as_slice()))
        })
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use ze_comb::OpRef;
    use ze_node_core::{
        DirtyPolicy, InputSocket, NamedTypeTag, NodeMeta, OutputSocket, OutputSocketId,
        SimpleGraph, TypeTag,
    };

    use super::*;

    macro_rules! impl_as_any {
        () => {
            fn as_any(&self) -> &dyn Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        };
    }

    fn f32_tag() -> Box<dyn TypeTag> {
        Box::new(NamedTypeTag::new("f32"))
    }

    /// 0 入 1 出：输出常量 `value`。
    #[derive(Debug)]
    struct Const {
        meta: NodeMeta,
        outputs: Vec<OutputSocket>,
        value: f32,
    }
    impl Const {
        fn new(value: f32) -> Self {
            Self {
                meta: NodeMeta::new("test.const", 1),
                outputs: vec![OutputSocket::new(f32_tag()).with_index(0)],
                value,
            }
        }
    }
    impl Node for Const {
        impl_as_any!();
        fn meta(&self) -> &NodeMeta {
            &self.meta
        }
        fn inputs(&self) -> &[InputSocket] {
            &[]
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
    }

    /// 1 入 1 出：`input + b`。
    #[derive(Debug)]
    struct Add {
        meta: NodeMeta,
        inputs: Vec<InputSocket>,
        outputs: Vec<OutputSocket>,
        b: f32,
    }
    impl Add {
        fn new(b: f32) -> Self {
            Self {
                meta: NodeMeta::new("test.add", 1),
                inputs: vec![InputSocket::new(f32_tag()).with_index(0)],
                outputs: vec![OutputSocket::new(f32_tag()).with_index(0)],
                b,
            }
        }
    }
    impl Node for Add {
        impl_as_any!();
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
    }

    /// 2 入 1 出：`a * b`（用来验证输入顺序按插槽下标）。
    #[derive(Debug)]
    struct Mul {
        meta: NodeMeta,
        inputs: Vec<InputSocket>,
        outputs: Vec<OutputSocket>,
    }
    impl Mul {
        fn new() -> Self {
            Self {
                 meta: NodeMeta::new("test.mul", 1),
                inputs: (0..2)
                    .map(|i| InputSocket::new(f32_tag()).with_index(i as u32))
                    .collect(),
                outputs: vec![OutputSocket::new(f32_tag()).with_index(0)],
            }
        }
    }
    impl Node for Mul {
        impl_as_any!();
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
    }

    fn add_op() -> OpRef {
        OpRef::new("zela.math", "add")
    }
    fn mul_op() -> OpRef {
        OpRef::new("zela.math", "mul")
    }

    fn rule_const(node: &dyn Node, inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
        if !inputs.is_empty() {
            return Err(RuleError::new("常量节点不接受输入"));
        }
        let node = node
            .as_any()
            .downcast_ref::<Const>()
            .ok_or_else(|| RuleError::new("节点不是 Const"))?;
        Ok(vec![Expr::pure(node.value)])
    }

    fn rule_add(node: &dyn Node, inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
        let node = node
            .as_any()
            .downcast_ref::<Add>()
            .ok_or_else(|| RuleError::new("节点不是 Add"))?;
        let [value] = inputs else {
            return Err(RuleError::new("Add 需要 1 个输入"));
        };
        Ok(vec![Expr::call(
            add_op(),
            [value.clone(), Expr::pure(node.b)],
        )])
    }

    fn rule_mul(_node: &dyn Node, inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
        let [a, b] = inputs else {
            return Err(RuleError::new("Mul 需要 2 个输入"));
        };
        Ok(vec![Expr::call(mul_op(), [a.clone(), b.clone()])])
    }

    fn registry() -> Lowering {
        let mut lowering = Lowering::new();
        lowering
            .register("test.const", rule_const)
            .register("test.add", rule_add)
            .register("test.mul", rule_mul);
        lowering
    }

    /// `const(2) → add(+3) → mul(with const(5))`：汇点最后。
    fn build_graph() -> (SimpleGraph, NodeId, NodeId, NodeId, NodeId) {
        let mut graph = SimpleGraph::default();
        let c = graph.add_node(Box::new(Const::new(2.0)));
        let a = graph.add_node(Box::new(Add::new(3.0)));
        let five = graph.add_node(Box::new(Const::new(5.0)));
        let m = graph.add_node(Box::new(Mul::new()));
        graph
            .quick_connect(OutputSocketId::new(c, 0), InputSocketId::new(a, 0))
            .unwrap();
        graph
            .quick_connect(OutputSocketId::new(a, 0), InputSocketId::new(m, 0))
            .unwrap();
        graph
            .quick_connect(OutputSocketId::new(five, 0), InputSocketId::new(m, 1))
            .unwrap();
        (graph, c, a, five, m)
    }

    #[test]
    fn lowers_upstream_first_in_deterministic_order() {
        let (graph, c, a, five, m) = build_graph();
        let lowered = registry().lower(&graph).unwrap();
        assert_eq!(lowered.len(), 4);
        // 拓扑序：c 与 five 同层 ⇒ NodeId 升序；a 在 c 之后、m 在最后。
        assert_eq!(lowered.order(), &[c, a, five, m]);

        assert_eq!(
            lowered.output(c, 0),
            Some(&Expr::pure(2.0f32)),
            "常量按 value 属性 lowering"
        );
        let expected_add = Expr::call(add_op(), [Expr::pure(2.0f32), Expr::pure(3.0f32)]);
        assert_eq!(lowered.output(a, 0), Some(&expected_add));
        // 输入的先后按插槽下标：m 的 0 号是 add 的结果，1 号是 five。
        let expected_mul = Expr::call(mul_op(), [expected_add, Expr::pure(5.0f32)]);
        assert_eq!(lowered.output(m, 0), Some(&expected_mul));
    }

    /// 同一张图 lowering 两次 ⇒ 产物逐位相同（含表达式树与顺序）。
    #[test]
    fn lowering_twice_is_bitwise_identical() {
        let (graph, ..) = build_graph();
        let lowering = registry();
        let first = lowering.lower(&graph).unwrap();
        let second = lowering.lower(&graph).unwrap();
        assert_eq!(first, second);
        assert_eq!(format!("{first:?}"), format!("{second:?}"));
    }

    /// 只读视图：`all_nodes` 返回逆序，其余委托给内层图。
    ///
    /// 这是「lowering 与 `all_nodes` 顺序解耦」的证据：顺序若被偷用，本用例失败。
    #[derive(Debug)]
    struct ReversedGraph<'a>(&'a SimpleGraph);

    impl Graph for ReversedGraph<'_> {
        fn node_count(&self) -> usize {
            self.0.node_count()
        }
        fn get_node(&self, node: NodeId) -> Option<&dyn Node> {
            self.0.get_node(node)
        }
        fn get_node_mut(&mut self, _node: NodeId) -> Option<&mut dyn Node> {
            unreachable!("ReversedGraph 是只读视图")
        }
        fn upstream(&self, input: InputSocketId) -> Vec<OutputSocketId> {
            self.0.upstream(input)
        }
        fn downstream(&self, output: OutputSocketId) -> Vec<InputSocketId> {
            self.0.downstream(output)
        }
        fn all_nodes(&self) -> Box<dyn Iterator<Item = NodeId> + '_> {
            let mut nodes: Vec<NodeId> = self.0.all_nodes().collect();
            nodes.reverse();
            Box::new(nodes.into_iter())
        }
        fn add_node(&mut self, _node: Box<dyn Node>) -> NodeId {
            unreachable!("ReversedGraph 是只读视图")
        }
        fn remove_node(&mut self, _node: NodeId) -> Option<Box<dyn Node>> {
            unreachable!("ReversedGraph 是只读视图")
        }
        fn connect_with_data(
            &mut self,
            _source: OutputSocketId,
            _target: InputSocketId,
            _data: Box<dyn ze_node_core::EdgeData>,
        ) -> Result<(), ze_node_core::ConnectError> {
            unreachable!("ReversedGraph 是只读视图")
        }
        fn disconnect(
            &mut self,
            _source: OutputSocketId,
            _target: InputSocketId,
        ) -> Option<Box<dyn ze_node_core::EdgeData>> {
            unreachable!("ReversedGraph 是只读视图")
        }
        fn edge_data_mut(
            &mut self,
            _source: OutputSocketId,
            _target: InputSocketId,
        ) -> Option<&mut dyn ze_node_core::EdgeData> {
            unreachable!("ReversedGraph 是只读视图")
        }
        fn edge_data(
            &self,
            source: OutputSocketId,
            target: InputSocketId,
        ) -> Option<&dyn ze_node_core::EdgeData> {
            self.0.edge_data(source, target)
        }
        fn edge_version(&self, source: OutputSocketId, target: InputSocketId) -> Option<u64> {
            self.0.edge_version(source, target)
        }
        fn type_cast(&self) -> &dyn ze_node_core::TypeCast {
            self.0.type_cast()
        }
    }

    #[test]
    fn lowering_is_independent_of_all_nodes_order() {
        let (graph, ..) = build_graph();
        let lowering = registry();
        let forward = lowering.lower(&graph).unwrap();
        let reversed = lowering.lower(&ReversedGraph(&graph)).unwrap();
        assert_eq!(forward, reversed);
        assert_eq!(format!("{forward:?}"), format!("{reversed:?}"));
    }

    #[test]
    fn unregistered_node_type_is_an_explicit_error() {
        let mut graph = SimpleGraph::default();
        let node = graph.add_node(Box::new(Const::new(1.0)));
        let error = Lowering::new().lower(&graph).unwrap_err();
        assert_eq!(
            error,
            LowerError::UnsupportedNode {
                node,
                type_name: "test.const".into()
            }
        );
    }

    #[test]
    fn unconnected_input_is_an_explicit_error() {
        let mut graph = SimpleGraph::default();
        let add = graph.add_node(Box::new(Add::new(1.0)));
        let error = registry().lower(&graph).unwrap_err();
        assert_eq!(
            error,
            LowerError::MissingInput {
                node: add,
                input: 0
            }
        );
    }

    #[test]
    fn rule_output_arity_mismatch_is_an_explicit_error() {
        fn wrong(_node: &dyn Node, _inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
            Ok(vec![Expr::pure(1.0f32), Expr::pure(2.0f32)])
        }
        let mut graph = SimpleGraph::default();
        let node = graph.add_node(Box::new(Const::new(1.0)));
        let mut lowering = Lowering::new();
        lowering.register("test.const", wrong as Rule);
        let error = lowering.lower(&graph).unwrap_err();
        assert_eq!(
            error,
            LowerError::OutputArity {
                node,
                type_name: "test.const".into(),
                expected: 1,
                got: 2
            }
        );
    }

    #[test]
    fn rule_rejection_carries_node_context() {
        fn reject(_node: &dyn Node, _inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
            Err(RuleError::new("属性缺失"))
        }
        let mut graph = SimpleGraph::default();
        let node = graph.add_node(Box::new(Const::new(1.0)));
        let mut lowering = Lowering::new();
        lowering.register("test.const", reject as Rule);
        let error = lowering.lower(&graph).unwrap_err();
        assert_eq!(
            error,
            LowerError::Rule {
                node,
                type_name: "test.const".into(),
                reason: "属性缺失".into()
            }
        );
    }

    #[test]
    fn registry_lists_registered_types_in_ascending_order() {
        let lowering = registry();
        assert_eq!(
            lowering.type_names().collect::<Vec<_>>(),
            vec!["test.add", "test.const", "test.mul"]
        );
        assert!(lowering.is_registered("test.add"));
        assert!(!lowering.is_registered("test.missing"));
    }
}
