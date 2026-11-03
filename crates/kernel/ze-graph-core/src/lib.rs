//! ze-graph-core：**纯拓扑图算法**。
//!
//! # 它是什么 / 不是什么
//!
//! 只认 **节点 id + 边**：不认识 `Node`、socket、`type_name`、编辑器交互
//! （计划 §3.1 / 硬约束 2）。领域层（`ze-node-core`）在自己的 `Graph` 上实现
//! [`Topology`]，算法即可复用。
//!
//! # 确定性是产物承诺
//!
//! **所有输出一律有序**（硬约束 1）：
//! - 拓扑序：就绪集合用 `BTreeSet` 取最小 id，结果与插入顺序无关；
//! - SCC / 环 / 可达 / 脏传播：结果按 id 升序（SCC 先按分量最小 id 排序）。
//!
//! 这条不是优化：同一张图跑两次必须逐位相同，否则依赖方会继承不确定性。

#![forbid(unsafe_code)]

pub mod dirty;
pub mod reach;
pub mod scc;
pub mod sort;
pub mod topology;
pub mod visitor;

pub use dirty::dirty_downstream;
pub use reach::reachable;
pub use scc::{find_cycle, strongly_connected};
pub use sort::{topo_sort, CycleNodes};
pub use topology::{Adjacency, Topology};
pub use visitor::{postorder, preorder, traverse, Visitor};
