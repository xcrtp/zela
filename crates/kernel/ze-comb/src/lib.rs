//! ze-comb: **领域无关的确定性计算内核**（原 `ze-func` 改名）。
//!
//! # 这个名字在说什么
//!
//! 它不是"又一个表达式库"：本 crate 不知道时间、帧、事件、路径、场景或渲染。
//! 它只知道**组合子结构**（[`Expr`] / [`Node`]）、**值**（[`Value`]）、**宿主单子**
//! （[`Mono`]）与 **pass 约定**（[`PassInfo`]）。领域词汇由宿主注入，
//! 算子语义由基础函子库 `ze-func` 提供。
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
//! 3. **分析结果不写回 IR**：本 crate 的类型 / 区间 / 点态性都是 side table；
//!    [`Expr`] 只有 `id` 与 `node`。
//!
//! # 三条硬约束（续）
//!
//! 4. **无隐式可变状态**：本 crate 没有时钟、没有全局计数、没有环境读取。宿主词汇是
//!    [`Node::Channel`] 叶子 —— 运行期由宿主单子 `M` 注入。
//! 5. **求值有两条实现**：lowered（[`CompiledExpr`]）与参考（tree-walking），
//!    两者对同一棵树必须逐位一致（[`pass`] 里的对拍）。
//!
//! # 非目标（当前批次）
//!
//! - **不做算子语义**：`zela.math` 在 `ze-func`；本 crate 只定义 `Mono::call` 的入口。
//! - **不做 `fix`**：组合子不是图灵完备的；合法递归是结构性的（阶段 3 的 `NatRec`），
//!   [`Node::Ref`] 仍是**非递归的 `let`**（装配期禁环）。
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
pub mod graph_view;
pub mod pass;
pub mod program;
pub mod reference;

pub use codata::{CodataError, CodataSpec, Expanded, Unfold, expand, observe};
pub use expr::{ChannelRef, ChannelType, Expr, ExprError, Id, Literal, Node, OpRef};
pub use graph_view::ExprGraphView;
pub use pass::{
    AnalysisPass, Lower, LoweredEval, PassInfo, PassKind, PassRegistry, PassRegistryError,
    ReferenceEval, Semantics, TransformPass, Validate, kernel_registry,
};
pub use program::{CompiledExpr, EvalError, Mono, RecCache, Step, Value, ValueBytes, REC_MAX_STEPS, hash_values};
pub use reference::{ReferenceEvaluator, eval_reference};

/// 表达式 payload 的 schema 版本。
///
/// 与 capability 的 `version` 分开：后者描述**函数集合**，本常量描述**表达式树的序列化格式**。
/// 加算子不递增它；改变节点 / 字面量的**编码形态**才递增。
///
/// `1 → 2`（提案 #6 B 批）：通道从封闭枚举 `"channel": "time"` 改为**数据引用**
/// `"channel": { "namespace": "", "name": "time" }` —— 组件字段通道与基础通道同形。
/// 受影响的作品只有 `expr_demo`（唯一使用表达式层的作品）。
///
/// 阶段 1 / 3 的新增（`i64` 字面量、`bytes` 值、`nat_rec` / `rec_prev` / `rec_index`
/// 节点）都是**追加**：老 payload 不含这些标签，解码路径一个字节不动 ⇒ 不递增。
pub const SCHEMA_VERSION: u32 = 2;
