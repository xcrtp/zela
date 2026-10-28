# ze-node-schema-macros

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`graph`（`crates/ze/graph`）
- 路径：`crates/ze/graph/ze-node-schema-macros`
- 生产依赖（上游，dev-deps 不计）：（无）
- 被依赖（下游）：`ze-node-schema`
- 源文件：1 个 · 700 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 700 | Proc-macro support for `zela-node-ui`. |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/lib.rs` | `pub fn derive_node_spec(input: TokenStream) -> TokenStream` |
<!-- AUTO:END -->

## 职责

过程宏 crate：实现节点属性派生宏，把普通 struct 展开成完整的节点实现与属性接口实现。

## 运行机制

读取 struct 上的节点属性（类型名、显示名、分类、输出、状态）与字段上的属性（标签、范围、步长等），一次生成节点实现、属性接口实现，以及默认节点构造器与分类常量。生成的属性写回逻辑用「属性名 + 变体」的穷举匹配直接改字段，全程不向下转型。

## 边界与不做什么

- 不依赖任何 crate（包括 ze-node-core）：宏只产出 token，路径留给使用方解析。
- 不生成 egui 控件，也不定义属性数据模型（在 ze-node-schema）。
- 只覆盖常用字段类型子集；枚举 / 范围 / 函子 / 动态 / 列表等属性变体暂不在派生范围内（后续可加，不破坏已生成代码）。
