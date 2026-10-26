//! ze-func: 组合子表达式内核（S1）。
//!
//! # 这个 crate 在解决什么
//!
//! 旧形态里脚本只能提交**值**，不能提交**关系**。于是 `Σ sin(kt)/k` 这类表达式只能在脚本侧
//! 手算 241 次采样（`fourier.lua` 的 `q()`，lua / py 各维护一份），手算就要量化，量化就产生
//! 跨语言分歧。本 crate 让脚本提交一棵**结构化表达式树**，采样交给引擎。
//!
//! # 三条硬约束
//!
//! 1. **组合子即结构**：[`Expr`] / [`Node`] 是可序列化的数据，不是闭包、不是 trait object。
//!    加一个算子 = 多一条数据，不改 schema。
//! 2. **标签显式稳定**：[`Node`] / [`Literal`] 的 serde 标签是**手写**的（`pure` / `call` /
//!    `f32` …），**不用** `stringify!` —— 后者对泛型会产出
//!    `Map<Sin,Pure<f32>>`，改实现就变，与「加新算子老 payload 解码不变」直接冲突。
//! 3. **无隐式可变状态**：本 crate 没有时钟、没有全局计数、没有环境读取。时间 / 帧 / 事件 /
//!    组件 / 环境是 [`Node::Channel`] 叶子 —— 运行期由宿主单子 `M` 注入（S3）。
//!    脚本拿不到 `t` 的数值，是因为 API 层**根本没有这个入口**。
//!
//! # 非目标（本批明确不做）
//!
//! - **不做求值**：装配期建图、运行期定序（§6）在 S3 落地；本 crate 只保证结构可表达、
//!   可序列化、可校验。
//! - **不做 `fix`**：组合子不是图灵完备的；累积状态是 primitive recursion，无限细分面是
//!   codata（S4）。[`Node::Ref`] 是**非递归的 `let`**（装配期禁环）。
//! - **不并进节点图**：本 crate 是「可序列化的节点子集」，与 `ze-node-core` 双向转换（§9.3）
//!   是后续工作，不在 S1。
//!
//! # payload 形状
//!
//! ```json
//! {
//!   "id": "h1",
//!   "node": {
//!     "type": "call",
//!     "data": {
//!       "op": { "namespace": "zela.math", "name": "sin" },
//!       "args": [
//!         {
//!           "node": {
//!             "type": "channel",
//!             "data": { "channel": { "namespace": "", "name": "time" } }
//!           }
//!         }
//!       ]
//!     }
//!   }
//! }
//! ```

pub mod codata;
pub mod expr;
pub mod program;

pub use codata::{CodataError, CodataSpec, Expanded, Unfold, expand, observe};
pub use expr::{ChannelRef, ChannelType, Expr, ExprError, Id, Literal, Node, OpRef};
pub use program::{CompiledExpr, EvalError, Mono, Step, Value};

/// 表达式 payload 的 schema 版本。
///
/// 与 capability 的 `version` 分开：后者描述**函数集合**，本常量描述**表达式树的序列化格式**。
/// 加算子不递增它；改变节点 / 字面量的**编码形态**才递增。
///
/// `1 → 2`（提案 #6 B 批）：通道从封闭枚举 `"channel": "time"` 改为**数据引用**
/// `"channel": { "namespace": "", "name": "time" }` —— 组件字段通道与基础通道同形。
/// 受影响的作品只有 `expr_demo`（唯一使用表达式层的作品）。
pub const SCHEMA_VERSION: u32 = 2;
