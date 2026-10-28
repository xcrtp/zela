# ze-scene-macros

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`scene`（`crates/scene`）
- 路径：`crates/scene/ze-scene-macros`
- 生产依赖（上游，dev-deps 不计）：（无）
- 被依赖（下游）：`ze-scene-api`、`ze-scene-core`
- 源文件：1 个 · 444 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 444 | 场景层的 proc-macro：`#[derive(SceneComponent)]` 与 `#[derive(Channels)]`。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/lib.rs` | `pub fn derive_scene_component(input: TokenStream) -> TokenStream` |
| `src/lib.rs` | `pub fn derive_channels(input: TokenStream) -> TokenStream` |
<!-- AUTO:END -->

## 职责

场景层过程宏：SceneComponent 与 Channels 两个派生，把组件类型与可动画字段的注册信息在编译期生成出来。

## 运行机制

SceneComponent 从类型属性读出持久化类型名，并按 ident 推导构造器名，两者同源生成以消除手写字符串漂移；Channels 按每个通道声明为字段生成字段规格常量、读写函数、目标构造器与通道数组，字段类型只接受标量、二维 / 三维向量与布尔，其余编译期报错。生成物全部是全限定路径的函数项，以便被常量表直接展开合并。

## 边界与不做什么

- 只做语法级展开，不校验字段是否真的适合动画，也不注册到任何全局表（合并由场景核心的目标表完成）。
- 不提供运行时反射或序列化。
- SceneComponent 的生成物引用场景 API crate，因此不能用于其它 crate。
