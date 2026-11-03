//! `ze-node-lower` — 把节点图（[`ze_node_core::Graph`]）lowering 成表达式层
//! （[`ze_comb::Expr`]）。
//!
//! # 为什么需要它
//!
//! 节点栈自带一套求值路径（`ze-graph-eval::LazyPullEval`）。如果节点图永远停在
//! 自己的运行时里，它就是**第二套 IR + 第二套求值** —— 表达式层已经有的算子、
//! 状态折叠、稳定性门禁在这里都要重造一遍。lowering 把节点图投影到
//! [`ze_comb::Expr`]，让下游（场景动画 / 编译 / 代码生成）只认表达式层。
//!
//! # 两个承诺
//!
//! 1. **顺序确定**：节点按 [`ze_graph_algo::KahnSorter`] 的拓扑序 lowering，
//!    tiebreak 是 `NodeId` 升序；**不读 `Graph::all_nodes` 的顺序**（那个顺序未指定）。
//!    同一张图 lowering 两次逐位相同。
//! 2. **失败显式**：没有规则的节点类型、缺连接、多连接、输出数量不符都返回
//!    [`LowerError`]，不静默跳过、不猜一个默认值。
//!
//! # 规则是数据
//!
//! [`Lowering::register`] 以节点的 `meta().type_name` 为键 —— 与「算子名是数据、
//! 不是 Rust 类型名」一致（备忘录 C §9）。换一套节点类型 = 换一张注册表，
//! 本 crate 的遍历 / 装配逻辑不动。

#![forbid(unsafe_code)]

pub mod error;
pub mod lowering;

pub use error::{LowerError, RuleError};
pub use lowering::{Lowered, Lowering, Rule};
