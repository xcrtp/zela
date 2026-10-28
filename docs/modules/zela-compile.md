# zela-compile

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`app`（`crates/app`）
- 路径：`crates/app/zela-compile`
- 生产依赖（上游，dev-deps 不计）：`zela-app`、`zela-scene-bundle`
- 被依赖（下游）：（无）
- 源文件：1 个 · 151 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/main.rs` | 151 | `zela-compile`：把脚本编译成 ZSB 装配产物。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

（未解析到模块级 `pub` 项）
<!-- AUTO:END -->

## 职责

编译入口二进制：把 Lua / Python 脚本编译成 ZSB 装配产物。

## 运行机制

先解析命令行（脚本路径、输出文件、可选的 JSON 明细），按扩展名推断语言，再调用组装层的脚本编译入口 —— 与 GUI 和 --script 是同一条装配路径；随后用产物格式 crate 编码，写出 Header + Payload + Meta 的 .zsb。--json 额外落一份 payload 明文便于调试 diff，明文不参与内容 hash。产物里的算子标识用名字而非注册序号，因此编译器与回放端注册顺序不同也不会画错。

## 边界与不做什么

- 不回放、不渲染、不校验像素。
- 不自己实现装配或脚本执行，全部复用组装层。
- 不是库：只有 main，没有对外 API。
