# ze-graph-algo

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`graph`（`crates/ze/graph`）
- 路径：`crates/ze/graph/ze-graph-algo`
- 生产依赖（上游，dev-deps 不计）：`ze-graph-core`、`ze-node-core`
- 被依赖（下游）：`ze-node`、`zela-node-editor`
- 源文件：5 个 · 515 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/bfs_dirty.rs` | 154 | Breadth-first dirty propagation. |
| `src/kahn_sort.rs` | 150 | Kahn's algorithm topological sort. |
| `src/lib.rs` | 19 | `zela-graph-algo` — concrete graph algorithms for `zela-node`. |
| `src/tarjan_scc.rs` | 140 | Cycle detection via SCC (delegates to `ze-graph-core`). |
| `src/topology.rs` | 52 | `Graph` → `ze-graph-core::Topology` 适配器。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/bfs_dirty.rs` | `pub struct BfsDirtyTracker` |
| `src/kahn_sort.rs` | `pub struct KahnSorter` |
| `src/tarjan_scc.rs` | `pub struct TarjanSccDetector` |
| `src/topology.rs` | `pub struct GraphTopology<'a>` |
<!-- AUTO:END -->

## 职责

为 ze-node-core 的算法 trait 提供具体实现：拓扑排序、环检测、脏标记传播，以及把图投影成纯拓扑的适配器。

## 运行机制

GraphTopology 先把任意图投影成只含「节点 id + 边」的拓扑视图（后继 / 前驱按端口索引走输入输出计数），再交给 ze-graph-core 的拓扑算法；KahnSorter 在其上做拓扑排序，TarjanSccDetector 的环检测委托 ze-graph-core 的强连通分量实现，BfsDirtyTracker 做广度优先的下游脏传播。调用方只以 trait 对象注入这些实现，换一份算法不需要改求值侧。

## 边界与不做什么

- 不做求值与缓存（在 ze-graph-eval），也不持有图状态：每次查询都从传入的图现取。
- 不含节点语义：算法层只认节点 id 与边，看不到节点类型与属性。
- 不提供 UI、序列化或存储实现。
