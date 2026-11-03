# ze-node

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`graph`（`crates/ze/graph`）
- 路径：`crates/ze/graph/ze-node`
- 生产依赖（上游，dev-deps 不计）：`ze-graph-algo`、`ze-graph-eval`、`ze-node-core`、`ze-node-schema`
- 被依赖（下游）：`zela`
- 源文件：1 个 · 27 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 27 | zela 节点图统一入口。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

（未解析到模块级 `pub` 项）
<!-- AUTO:END -->

## 职责

节点图门面：把节点核心 trait 层、属性模型、图算法、求值策略四个平台无关 crate 聚合成一个入口。

## 运行机制

单文件只做四条整体重导出；因为核心层与属性层都有 error 模块，这里显式把 error 固定指向核心层，避免歧义。聚合结果不含 UI，可编到 wasm32，也能在 headless 环境直接使用。

## 边界与不做什么

- 不含节点编辑器、节点 egui 视图与属性控件三个 UI crate。
- 不做实现：这里只有重导出，没有新增逻辑。
- 不承担场景与渲染语义。
