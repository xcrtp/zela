# ze-node-lower

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`graph`（`crates/ze/graph`）
- 路径：`crates/ze/graph/ze-node-lower`
- 生产依赖（上游，dev-deps 不计）：`ze-comb`、`ze-graph-algo`、`ze-node-core`
- 被依赖（下游）：（无）
- 源文件：3 个 · 700 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/error.rs` | 86 | lowering 的失败形态 —— 全部显式，不静默降级。 |
| `src/lib.rs` | 31 | `ze-node-lower` — 把节点图（[`ze_node_core::Graph`]）lowering 成表达式层 |
| `src/lowering.rs` | 583 | 注册表 + lowering 主体：拓扑序遍历，逐节点把「属性 + 已 lowering 的输入」 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/error.rs` | `pub enum LowerError` |
| `src/error.rs` | `pub struct RuleError` |
| `src/lowering.rs` | `pub struct Lowering` |
| `src/lowering.rs` | `pub struct Lowered` |
<!-- AUTO:END -->

## 职责

把节点图（`ze-node-core::Graph`）投影成表达式层（`ze-comb::Expr`），并保证投影顺序确定：节点按拓扑序、同层按 `NodeId` 升序。它持有的只有一张「节点类型名 → 规则」的注册表（`BTreeMap`），图与产物都由调用方传入 / 取走。

## 运行机制

`Lowering::lower` 先用 `KahnSorter` 取得确定性拓扑序（`ze-graph-core` 的就绪集合是 `BTreeSet`，tiebreak 即 `NodeId` 升序），再逐个节点：按输入插槽下标取上游已 lowering 的输出表达式（`Graph::upstream` 的返回先排序，再要求恰好一条连接），调该节点 `meta().type_name` 登记的规则，得到「每个输出插槽一个 `Expr`」，存入 `BTreeMap<NodeId, Vec<Expr>>`。`all_nodes` 从头到尾不读。

三条失败路径都显式：没有登记的节点类型、输入缺连接 / 多连接、规则产出的表达式数与输出插槽数不符，分别返回 `LowerError` 的具体变体；规则自己拒绝时由调用点补上节点 id 与类型名，包装成 `LowerError::Rule`。

## 边界与不做什么

- 不做求值：只产出表达式树，求值在 `ze-comb` / `ze-graph-eval`（两者互为对拍）。
- 不拥有节点语义：属性怎么读、算子叫什么由规则决定；换一套节点类型 = 换一张注册表。
- 不定义状态语义：节点状态与场景 `State` 的对应关系不在本层（它只看规则产出的表达式）。
- 不猜顺序：多连接输入直接报错，不替调用方选一个「看起来对」的上游。
