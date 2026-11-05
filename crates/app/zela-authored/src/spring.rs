//! **CF-d 作品**：节点编辑器里的弹簧（Vec2 状态）→ 场景 `State`。
//!
//! # 它证明什么
//!
//! 节点图里的一个有状态节点，lowering 之后**就是**场景层的 `State`：
//!
//! ```text
//! SpringNode（Vec2 状态）
//!   --lowering-->  Expr::state(vec2(x0, v0), vec2(x + v, d·v − k·x))
//!   --node_state::verify_states-->  StatePlan（2×2 仿射图，已过谱半径门禁）
//! ```
//!
//! 半隐式欧拉、参数含义与 BX 批的二维弹簧同形（`crates/kernel/ze-comb` 的
//! `tuple_state_matches_replay_bit_for_bit`）：`x' = x + v`，`v' = d·v − k·x`，
//! 稳定性条件 `d + k < 1` —— 越过这条线，场景层直接拒绝（`AffineError::Unstable`），
//! 不产出 inf / NaN。
//!
//! 节点侧的状态按 **f64** 存放：状态是节点自己的运行时数据（`NodeValue` 是类型
//! 擦除的），而属性仍是 f32。这样节点逐帧折叠与场景闭式定义可以用同一个精度对拍。

use ze_comb::{Expr, OpRef};
use ze_node_core::{EvalError, ExecContext, Node};
use ze_node_lower::{Lowering, RuleError};

/// 节点类型名（规则表 / 编辑器注册表都用它作键）。
pub const SPRING_TYPE: &str = "zela.physics.spring.vec2";
/// 状态类型名（`node_state::state_shape` 把它映射到 `StateShape::Vec2`）。
pub const SPRING_STATE: &str = "Vec2";

fn math(name: &str) -> OpRef {
    OpRef::new("zela.math", name)
}

/// 二维弹簧：状态 `[x, v]`，参数是初值 + 阻尼 + 刚度。
#[derive(Debug, Clone, ze_node_schema::NodeSpec)]
#[node(
    type_name = "zela.physics.spring.vec2",
    display = "Spring",
    category = "Physics",
    outputs = ["Vec2"],
    state = "Vec2",
    dirty = "always"
)]
pub struct SpringNode {
    #[property(label = "init x", range = -10.0..=10.0, step = 0.01, default = 1.5)]
    pub init_x: f32,
    #[property(label = "init v", range = -10.0..=10.0, step = 0.01, default = 0.0)]
    pub init_v: f32,
    #[property(label = "stiffness", range = 0.0..=0.5, step = 0.001, default = 0.05)]
    pub stiffness: f32,
    #[property(label = "damping", range = 0.0..=1.0, step = 0.01, default = 0.9)]
    pub damping: f32,
}

impl SpringNode {
    /// 初始状态（`[x, v]`）。
    pub fn init(&self) -> [f64; 2] {
        [f64::from(self.init_x), f64::from(self.init_v)]
    }

    /// 一步半隐式欧拉。
    pub fn advance(&self, prev: [f64; 2]) -> [f64; 2] {
        let stiffness = f64::from(self.stiffness);
        let damping = f64::from(self.damping);
        [prev[0] + prev[1], damping * prev[1] - stiffness * prev[0]]
    }
}

// ---------------------------------------------------------------------------
// 节点侧：LazyPullEval 的 executor（状态跨 `evaluate` 保留）
// ---------------------------------------------------------------------------

/// 弹簧的执行器：每次 `evaluate` 推进一步，并把新状态写到输出 0。
#[derive(Debug, Default, Clone, Copy)]
pub struct SpringExecutor;

impl ze_node_core::NodeExecutor for SpringExecutor {
    fn execute(&self, node: &dyn Node, ctx: &mut dyn ExecContext) -> Result<(), EvalError> {
        let here = ctx.current_node();
        let spring = node
            .as_any()
            .downcast_ref::<SpringNode>()
            .ok_or_else(|| EvalError::new("节点不是 SpringNode", here, None))?;
        // 首次求值没有状态 ⇒ 用属性里的初值。
        let prev = ctx
            .get_state()
            .ok()
            .and_then(|state| state.downcast_ref::<[f64; 2]>().copied())
            .unwrap_or_else(|| spring.init());
        let next = spring.advance(prev);
        ctx.set_state(Box::new(next))?;
        ctx.set_output(ze_node_core::OutputSocketId::new(here, 0), Box::new(next))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 表达式侧：lowering 规则
// ---------------------------------------------------------------------------

fn prev_x() -> Expr {
    Expr::call(math("x"), [Expr::rec_prev()])
}

fn prev_v() -> Expr {
    Expr::call(math("y"), [Expr::rec_prev()])
}

/// `SpringNode` ⇒ `State(vec2(x0, v0), vec2(x + v, d·v − k·x))`。
pub fn rule_spring(node: &dyn Node, inputs: &[Expr]) -> Result<Vec<Expr>, RuleError> {
    if !inputs.is_empty() {
        return Err(RuleError::new("弹簧节点不接受输入（参数走属性）"));
    }
    let spring = node
        .as_any()
        .downcast_ref::<SpringNode>()
        .ok_or_else(|| RuleError::new("节点不是 SpringNode"))?;
    let init = Expr::call(
        math("vec2"),
        [Expr::pure(spring.init_x), Expr::pure(spring.init_v)],
    );
    let step = Expr::call(
        math("vec2"),
        [
            Expr::call(math("add"), [prev_x(), prev_v()]),
            Expr::call(
                math("add"),
                [
                    Expr::call(math("scale"), [Expr::pure(spring.damping), prev_v()]),
                    Expr::call(math("scale"), [Expr::pure(-spring.stiffness), prev_x()]),
                ],
            ),
        ],
    );
    Ok(vec![Expr::state(init, step)])
}

/// 节点 → 表达式的规则表（当前只有弹簧；加节点类型 = 多一行登记）。
pub fn lowering() -> Lowering {
    let mut lowering = Lowering::new();
    lowering.register(SPRING_TYPE, rule_spring);
    lowering
}

#[cfg(test)]
mod tests {
    use ze_graph_algo::KahnSorter;
    use ze_graph_eval::LazyPullEval;
    use ze_node_core::{EvalErrorPolicy, EvalStrategy, Graph, NodeId, OutputSocketId, SimpleGraph};
    use ze_node_lower::Lowered;
    use ze_scene_core::animation::affine::StateShape;

    use super::*;
    use crate::node_state::verify_states;

    fn graph_with(spring: SpringNode) -> (SimpleGraph, NodeId) {
        let mut graph = SimpleGraph::default();
        let node = graph.add_node(Box::new(spring));
        (graph, node)
    }

    fn lowered(graph: &SimpleGraph) -> Lowered {
        lowering().lower(graph).expect("弹簧图可 lowering")
    }

    /// 默认参数（k = 0.05, d = 0.9）：产物是 Vec2 状态，仿射图就是弹簧的矩阵。
    #[test]
    fn spring_lowers_to_a_vec2_scene_state() {
        let (graph, node) = graph_with(SpringNode::default());
        let lowered = lowered(&graph);
        let states = verify_states(&graph, &lowered).expect("状态可校验");
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].node, node);
        assert_eq!(states[0].shape, StateShape::Vec2);

        let plan = &states[0].plan;
        assert_eq!(plan.linear(0, 0), Some(1.0), "x' 含 1·x");
        assert_eq!(plan.linear(0, 1), Some(1.0), "x' 含 1·v");
        assert_eq!(plan.linear(1, 0), Some(-f64::from(0.05f32)));
        assert_eq!(plan.linear(1, 1), Some(f64::from(0.9f32)));
        // 谱半径 |λ| = sqrt(det) = sqrt(0.95) < 1 ⇒ 通过 BZ-a。
        let radius = plan.map().spectral_radius();
        assert!(radius < 1.0, "默认参数的谱半径应 < 1，实际 {radius}");
    }

    /// 越过 `d + k < 1`：场景层直接拒绝，不是静默产出 inf。
    #[test]
    fn unstable_spring_is_rejected_by_the_scene_gate() {
        let (graph, _) = graph_with(SpringNode {
            stiffness: 0.2,
            ..SpringNode::default()
        });
        let lowered = lowered(&graph);
        let error = verify_states(&graph, &lowered).unwrap_err();
        let crate::node_state::NodeStateError::Affine { error, .. } = error else {
            panic!("期望仿射识别失败，实际 {error:?}");
        };
        match error {
            ze_scene_core::animation::affine::AffineError::Unstable { radius } => {
                assert!(radius >= 1.0, "d + k = 1.1 ⇒ |λ| ≥ 1，实际 {radius}");
            }
            other => panic!("期望 Unstable，实际 {other:?}"),
        }
    }

    /// 逐帧折叠（节点侧）与场景闭式定义（`value_at`）语义一致。
    ///
    /// 口径与 BY-02 相同：闭式走矩阵快速幂，逐次折叠走 n 步乘法 —— IEEE 下两者
    /// **不是**逐位关系（BY 已有 `replay_within` 这个 oracle），因此这里用容差比较。
    #[test]
    fn folding_matches_the_scene_state_oracle() {
        let (graph, node) = graph_with(SpringNode::default());
        let lowered = lowered(&graph);
        let states = verify_states(&graph, &lowered).expect("状态可校验");
        let plan = &states[0].plan;

        let mut eval = LazyPullEval::new(Box::new(KahnSorter::new()), EvalErrorPolicy::FailFast);
        for frame in 1..=60 {
            eval.evaluate(&graph, &SpringExecutor, Some(OutputSocketId::new(node, 0)))
                .expect("节点图可求值");
            let folded = *eval
                .cached_output(OutputSocketId::new(node, 0))
                .expect("输出已缓存")
                .downcast_ref::<[f64; 2]>()
                .expect("状态是 [f64; 2]");
            let closed = plan.value_at(frame as i64);
            let closed = closed.as_vec2().expect("闭式值是 Vec2");
            for (index, (folded, closed)) in folded.iter().zip(closed).enumerate() {
                let folded = *folded as f32;
                let scale = 1.0 + folded.abs().max(closed.abs());
                assert!(
                    (folded - closed).abs() <= 1e-5 * scale,
                    "frame {frame} 分量 {index}：折叠 {folded} ≠ 闭式 {closed}"
                );
            }
        }
    }
}
