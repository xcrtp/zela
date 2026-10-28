# ze-pod

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`pod`（`crates/ze/pod`）
- 路径：`crates/ze/pod/ze-pod`
- 生产依赖（上游，dev-deps 不计）：`ze-pod-binding`、`ze-pod-core`、`ze-pod-macro`
- 被依赖（下游）：`zela`
- 源文件：1 个 · 18 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 18 | zela Pod 统一入口。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

（未解析到模块级 `pub` 项）
<!-- AUTO:END -->

## 职责

Pod 层统一门面：把核心抽象、绑定驱动与能力派生宏聚合成一个引用点。

## 运行机制

只有三条重导出与一个编译期断言（Pod 可用）的文档用例；调用方引入一次就同时拿到抽象、绑定驱动与宏。本 crate 不新增类型、不持有状态，也没有初始化步骤。

## 边界与不做什么

- 刻意不含 Lua 与 Python 适配器：二者分别需要 C 工具链与系统 Python，放进来会破坏纯 Rust 与 wasm 可编译性。
- 不做装配、不提供任何服务实现。
- 不做 feature 组合与版本门控，门面里没有自己的逻辑。
