//! **CF-d**：节点状态 → 场景 `State` 的映射与校验（D1 / D4 的兑现点）。
//!
//! # 它解决什么
//!
//! 节点用自己的 `state_type()` 声明「我有哪一种状态」（`()` 表示无状态，
//! `Vec2` 表示二维状态……）。场景层的 `State` 只认四档值形状
//! （[`StateShape`]：标量 / Vec2 / Vec3 / Vec4）。两边之间需要一张**显式**的表：
//!
//! ```text
//! node.state_type().type_name()  →  StateShape
//!          "()"    → 无状态（跳过）
//!        "f32"/"f64" → Scalar
//!        "Vec2..4"  → Vec2..4
//!          其它     → 显式错误（不静默降级）
//! ```
//!
//! 映射成功之后，节点的 lowering 产物直接交给场景层的仿射识别：
//! [`plan_from_expr`] 一次给出「仿射图 + init + predicate」，并**顺带**执行
//! BZ-a 的谱半径门禁（`|λ|max ≥ 1 ⇒ AffineError::Unstable`）。因此节点编辑器里
//! 拖出来的弹簧自动获得与手写作品相同的稳定性检查，不需要第二套判据。
//!
//! # 约定（本模块检查的那一条）
//!
//! **有状态节点的状态必须位于它输出的根**：规则产出 `Expr::state(init, step)`
//! 作为该输出的整棵树。这样「哪个 State 属于哪个节点」不需要额外标记；
//! 下游纯函数节点继承上游状态时也不会被误判（它们声明的是 `()`，直接跳过）。

use ze_comb::Node as ExprNode;
use ze_node_core::{Graph, NodeId};
use ze_node_lower::Lowered;
use ze_scene_core::animation::affine::{AffineError, StatePlan, StateShape, plan_from_expr};

/// `Node::state_type()` 的「无状态」标记（派生宏的默认值）。
pub const NO_STATE: &str = "()";

/// 状态类型名 → 场景值形状。表外的类型返回 `None`（调用方必须报错，不得猜）。
pub fn state_shape(type_name: &str) -> Option<StateShape> {
    match type_name {
        "f32" | "f64" | "Scalar" => Some(StateShape::Scalar),
        "Vec2" | "vec2" => Some(StateShape::Vec2),
        "Vec3" | "vec3" => Some(StateShape::Vec3),
        "Vec4" | "vec4" => Some(StateShape::Vec4),
        _ => None,
    }
}

/// 一个节点的状态校验结果（side table：**不进产物**）。
#[derive(Debug, Clone, PartialEq)]
pub struct NodeState {
    pub node: NodeId,
    /// 节点声明的形状（与 `plan.shape()` 一致）。
    pub shape: StateShape,
    /// 场景层的仿射计划（已过谱半径门禁）。
    pub plan: StatePlan,
}

/// 状态映射 / 校验失败。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum NodeStateError {
    /// 节点声明的状态类型不在映射表内。
    #[error(
        "节点 `{type_name}`（{node:?}）的状态类型 `{state_type}` 不在映射表内\
         （支持 {NO_STATE} / f32 / f64 / Scalar / Vec2 / Vec3 / Vec4）"
    )]
    Unmapped {
        node: NodeId,
        type_name: String,
        state_type: String,
    },

    /// 节点声明了状态，但它的 lowering 产物根不是 `State`。
    #[error(
        "节点 `{type_name}`（{node:?}）声明了 {shape:?} 状态，但输出根不是 `State`\
         （lowering 规则必须把状态放在输出根）"
    )]
    MissingState {
        node: NodeId,
        type_name: String,
        shape: StateShape,
    },

    /// 声明的形状与 `State.init` 推断出的形状不一致。
    #[error("节点 `{type_name}`（{node:?}）声明了 {declared:?} 状态，产物实际是 {actual:?}")]
    ShapeMismatch {
        node: NodeId,
        type_name: String,
        declared: StateShape,
        actual: StateShape,
    },

    /// 场景层识别失败（不是仿射 / 宽度不支持 / 谱半径 ≥ 1 ……）。
    #[error("节点 `{type_name}`（{node:?}）的 State 不可用：{error}")]
    Affine {
        node: NodeId,
        type_name: String,
        error: AffineError,
    },
}

/// 校验 `lowered` 里每个有状态节点的状态，并产出场景层计划。
///
/// 顺序 = `lowered.order()`（确定性拓扑序），因此调用方拿到的表也是确定的。
pub fn verify_states(
    graph: &dyn Graph,
    lowered: &Lowered,
) -> Result<Vec<NodeState>, NodeStateError> {
    let mut out = Vec::new();
    for &node_id in lowered.order() {
        let Some(node) = graph.get_node(node_id) else {
            continue;
        };
        let type_name = node.meta().type_name.clone();
        let declared_tag = node.state_type();
        let declared_name = declared_tag.type_name();
        if declared_name == NO_STATE {
            // 无状态节点：输出里可能**继承**上游的 State，不能据此报错。
            continue;
        }
        let Some(shape) = state_shape(declared_name) else {
            return Err(NodeStateError::Unmapped {
                node: node_id,
                type_name,
                state_type: declared_name.to_string(),
            });
        };
        let Some(exprs) = lowered.node(node_id) else {
            continue;
        };
        let mut plans = Vec::new();
        for expr in exprs {
            let ExprNode::State {
                init,
                step,
                predicate,
            } = &expr.node
            else {
                continue;
            };
            let plan = plan_from_expr(init, step, predicate.as_deref()).map_err(|error| {
                NodeStateError::Affine {
                    node: node_id,
                    type_name: type_name.clone(),
                    error,
                }
            })?;
            plans.push(plan);
        }
        if plans.is_empty() {
            return Err(NodeStateError::MissingState {
                node: node_id,
                type_name,
                shape,
            });
        }
        for plan in &plans {
            if plan.shape() != shape {
                return Err(NodeStateError::ShapeMismatch {
                    node: node_id,
                    type_name,
                    declared: shape,
                    actual: plan.shape(),
                });
            }
        }
        out.extend(plans.into_iter().map(|plan| NodeState {
            node: node_id,
            shape,
            plan,
        }));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use ze_comb::{Expr, OpRef};
    use ze_node_core::{
        DirtyPolicy, Graph, InputSocket, NamedTypeTag, Node, NodeMeta, OutputSocket, SimpleGraph,
        TypeTag,
    };
    use ze_node_lower::{Lowering, RuleError};

    use super::*;

    /// 测试节点：声明任意状态类型名；`stateful` 决定规则产出 `State` 还是纯表达式。
    #[derive(Debug)]
    struct Stub {
        meta: NodeMeta,
        outputs: Vec<OutputSocket>,
        state: &'static str,
        stateful: bool,
    }

    impl Stub {
        fn new(meta: &str, state: &'static str, stateful: bool) -> Self {
            Self {
                meta: NodeMeta::new(meta, 1),
                outputs: vec![OutputSocket::new(Box::new(NamedTypeTag::new("Vec2"))).with_index(0)],
                state,
                stateful,
            }
        }
    }

    impl Node for Stub {
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
            Box::new(NamedTypeTag::new(self.state))
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    /// 规则：`stateful` ⇒ 根是 `State`；否则是一棵纯表达式。
    fn rule(node: &dyn Node, _inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
        let stub = node
            .as_any()
            .downcast_ref::<Stub>()
            .ok_or_else(|| RuleError::new("节点不是 Stub"))?;
        if !stub.stateful {
            return Ok(vec![Expr::pure(1.0f32)]);
        }
        let x = || Expr::call(OpRef::new("zela.math", "x"), [Expr::rec_prev()]);
        let v = || Expr::call(OpRef::new("zela.math", "y"), [Expr::rec_prev()]);
        let init = Expr::call(
            OpRef::new("zela.math", "vec2"),
            [Expr::pure(1.0f32), Expr::pure(0.0f32)],
        );
        let step = Expr::call(
            OpRef::new("zela.math", "vec2"),
            [
                Expr::call(OpRef::new("zela.math", "add"), [x(), v()]),
                Expr::call(
                    OpRef::new("zela.math", "add"),
                    [
                        Expr::call(OpRef::new("zela.math", "scale"), [Expr::pure(0.9f32), v()]),
                        Expr::call(
                            OpRef::new("zela.math", "scale"),
                            [Expr::pure(-0.05f32), x()],
                        ),
                    ],
                ),
            ],
        );
        Ok(vec![Expr::state(init, step)])
    }

    fn registry() -> Lowering {
        let mut lowering = Lowering::new();
        lowering.register("test.vec2.state", rule);
        lowering.register("test.unmapped.state", rule);
        lowering.register("test.flat.state", rule);
        lowering
    }

    fn lowered_state(graph: &SimpleGraph) -> Vec<NodeState> {
        let lowered = registry().lower(graph).unwrap();
        verify_states(graph, &lowered).unwrap()
    }

    #[test]
    fn mapped_state_type_produces_a_scene_plan() {
        let mut graph = SimpleGraph::default();
        let node = graph.add_node(Box::new(Stub::new("test.vec2.state", "Vec2", true)));
        let states = lowered_state(&graph);
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].node, node);
        assert_eq!(states[0].shape, StateShape::Vec2);
        assert_eq!(states[0].plan.shape(), StateShape::Vec2);
        // 半隐式欧拉：A = [[1, 1], [-k, d]]
        assert_eq!(states[0].plan.linear(0, 0), Some(1.0));
        assert_eq!(states[0].plan.linear(0, 1), Some(1.0));
        assert_eq!(
            states[0].plan.linear(1, 0),
            Some(-f64::from(0.05f32)),
            "stiffness 以 f32 读入，再原样进入仿射图"
        );
        assert_eq!(states[0].plan.linear(1, 1), Some(f64::from(0.9f32)));
    }

    #[test]
    fn unmapped_state_type_is_an_explicit_error() {
        let mut graph = SimpleGraph::default();
        let node = graph.add_node(Box::new(Stub::new("test.unmapped.state", "Quat", true)));
        let lowered = registry().lower(&graph).unwrap();
        let error = verify_states(&graph, &lowered).unwrap_err();
        assert_eq!(
            error,
            NodeStateError::Unmapped {
                node,
                type_name: "test.unmapped.state".into(),
                state_type: "Quat".into()
            }
        );
    }

    #[test]
    fn declared_state_without_root_state_is_an_explicit_error() {
        let mut graph = SimpleGraph::default();
        let node = graph.add_node(Box::new(Stub::new("test.flat.state", "Vec2", false)));
        let lowered = registry().lower(&graph).unwrap();
        let error = verify_states(&graph, &lowered).unwrap_err();
        assert_eq!(
            error,
            NodeStateError::MissingState {
                node,
                type_name: "test.flat.state".into(),
                shape: StateShape::Vec2
            }
        );
    }

    #[test]
    fn shape_mismatch_is_an_explicit_error() {
        // 声明 Scalar，但产物是 Vec2 状态。
        let mut graph = SimpleGraph::default();
        let node = graph.add_node(Box::new(Stub::new("test.vec2.state", "f32", true)));
        let lowered = registry().lower(&graph).unwrap();
        let error = verify_states(&graph, &lowered).unwrap_err();
        assert_eq!(
            error,
            NodeStateError::ShapeMismatch {
                node,
                type_name: "test.vec2.state".into(),
                declared: StateShape::Scalar,
                actual: StateShape::Vec2
            }
        );
    }

    #[test]
    fn stateless_nodes_are_skipped_even_when_they_reuse_an_upstream_state() {
        // 纯函数节点继承上游状态不会被误判：它声明的是 `()`。
        let mut graph = SimpleGraph::default();
        graph.add_node(Box::new(Stub::new("test.vec2.state", "Vec2", true)));
        let flat = graph.add_node(Box::new(Stub::new("test.flat.state", "()", false)));
        let states = lowered_state(&graph);
        assert_eq!(states.len(), 1);
        assert_ne!(states[0].node, flat);
    }

    #[test]
    fn unstable_state_is_rejected_by_the_spectral_gate() {
        // 同一张规则表，但用一个把 step 放大到 |λ| ≥ 1 的节点。
        #[derive(Debug)]
        struct Divergent {
            meta: NodeMeta,
            outputs: Vec<OutputSocket>,
        }
        impl Node for Divergent {
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
                Box::new(NamedTypeTag::new("Vec2"))
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        }

        fn divergent_rule(_node: &dyn Node, _inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
            let x = || Expr::call(OpRef::new("zela.math", "x"), [Expr::rec_prev()]);
            let v = || Expr::call(OpRef::new("zela.math", "y"), [Expr::rec_prev()]);
            let init = Expr::call(
                OpRef::new("zela.math", "vec2"),
                [Expr::pure(1.0f32), Expr::pure(0.0f32)],
            );
            // det = d + k = 0.9 + 0.2 = 1.1 ⇒ |λ| > 1
            let step = Expr::call(
                OpRef::new("zela.math", "vec2"),
                [
                    Expr::call(OpRef::new("zela.math", "add"), [x(), v()]),
                    Expr::call(
                        OpRef::new("zela.math", "add"),
                        [
                            Expr::call(OpRef::new("zela.math", "scale"), [Expr::pure(0.9f32), v()]),
                            Expr::call(
                                OpRef::new("zela.math", "scale"),
                                [Expr::pure(-0.2f32), x()],
                            ),
                        ],
                    ),
                ],
            );
            Ok(vec![Expr::state(init, step)])
        }

        let mut graph = SimpleGraph::default();
        let node = graph.add_node(Box::new(Divergent {
            meta: NodeMeta::new("test.divergent", 1),
            outputs: vec![OutputSocket::new(Box::new(NamedTypeTag::new("Vec2"))).with_index(0)],
        }));
        let mut lowering = Lowering::new();
        lowering.register("test.divergent", divergent_rule);
        let lowered = lowering.lower(&graph).unwrap();
        match verify_states(&graph, &lowered).unwrap_err() {
            NodeStateError::Affine {
                node: got,
                type_name,
                error: AffineError::Unstable { radius },
            } => {
                assert_eq!(got, node);
                assert_eq!(type_name, "test.divergent");
                assert!(radius >= 1.0, "谱半径应 ≥ 1，实际 {radius}");
            }
            other => panic!("期望 Unstable，实际 {other:?}"),
        }
    }

    #[test]
    fn state_shape_table_is_explicit() {
        assert_eq!(state_shape("()"), None);
        assert_eq!(state_shape("f32"), Some(StateShape::Scalar));
        assert_eq!(state_shape("f64"), Some(StateShape::Scalar));
        assert_eq!(state_shape("Vec2"), Some(StateShape::Vec2));
        assert_eq!(state_shape("Vec3"), Some(StateShape::Vec3));
        assert_eq!(state_shape("Vec4"), Some(StateShape::Vec4));
        assert_eq!(state_shape("Quat"), None);
    }

    #[test]
    fn node_state_default_marker_is_the_derive_default() {
        // 派生宏把 state 缺省写成 "()"；映射表必须与它对齐。
        assert_eq!(NO_STATE, "()");
    }
}
