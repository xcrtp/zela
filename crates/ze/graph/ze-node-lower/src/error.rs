//! lowering 的失败形态 —— 全部显式，不静默降级。
//!
//! 「映射不上就报错」与本仓其它地方同一条纪律（D4）：节点图里的一个 Vec2 状态
//! 若被悄悄降级成标量，像素会悄悄漂移；同理，没有规则的节点若被跳过，产物会
//! 少一块而调用方毫无察觉。

use ze_node_core::NodeId;

/// lowering 失败。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LowerError {
    /// 图里有环（lowering 只接受 DAG）。
    ///
    /// 环在 `connect` 时就被拒绝（[`ze_node_core::ConnectError::CycleDetected`]），
    /// 走到这里说明图是被别的途径改成有环的；仍然是显式错误。
    #[error("节点图有环（涉及节点 {nodes:?}；lowering 只接受 DAG）")]
    Cycle { nodes: Vec<NodeId> },

    /// 拓扑序里的节点在图中取不到（图在 lowering 期间被并发修改）。
    #[error("节点 {node:?} 在图中不存在")]
    MissingNode { node: NodeId },

    /// 上游节点已经 lowering 过，但取不到它的某个输出（内部不一致）。
    #[error("节点 {node:?} 的输出 {index} 不在 lowering 结果里（内部不一致）")]
    MissingOutput { node: NodeId, index: usize },

    /// 该节点类型没有登记 lowering 规则。
    #[error("节点 `{type_name}`（{node:?}）没有 lowering 规则")]
    UnsupportedNode { node: NodeId, type_name: String },

    /// 输入插槽没有连接。
    #[error("节点 {node:?} 的输入 {input} 没有连接（lowering 要求输入全连接）")]
    MissingInput { node: NodeId, input: usize },

    /// 输入插槽有多条上游连接：语义上「哪一个」没有定义。
    #[error(
        "节点 {node:?} 的输入 {input} 有 {count} 条上游连接（lowering 要求单连接：\
         多输入的先后没有定义）"
    )]
    AmbiguousInput {
        node: NodeId,
        input: usize,
        count: usize,
    },

    /// 规则产出的表达式数量与节点的输出插槽数量不一致。
    #[error(
        "节点 `{type_name}`（{node:?}）的输出数量不匹配：插槽 {expected} 个，规则产出 {got} 个"
    )]
    OutputArity {
        node: NodeId,
        type_name: String,
        expected: usize,
        got: usize,
    },

    /// 规则自己拒绝了这次 lowering（属性缺失 / 类型不认识等）。
    #[error("节点 `{type_name}`（{node:?}）的 lowering 失败：{reason}")]
    Rule {
        node: NodeId,
        type_name: String,
        reason: String,
    },
}

/// 规则内部的失败原因。
///
/// 规则只知道「这个节点 lowering 不了」，不知道自己在拓扑序里的位置 —— 节点 id 与
/// 类型名由 [`LowerError::Rule`] 在调用点补上，所以这里只带一句原因。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct RuleError {
    message: String,
}

impl RuleError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}
