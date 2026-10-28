# ze-macro

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`kernel`（`crates/kernel`）
- 路径：`crates/kernel/ze-macro`
- 生产依赖（上游，dev-deps 不计）：（无）
- 被依赖（下游）：`ze-scene-core`、`zela-app`、`zela-egui`、`zela-scene-ecs`
- 源文件：1 个 · 408 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 408 | — |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/lib.rs` | `pub fn derive_scene_object(input: TokenStream) -> TokenStream` |
| `src/lib.rs` | `pub fn derive_describable(input: TokenStream) -> TokenStream` |
| `src/lib.rs` | `pub fn derive_transform2d_into(input: TokenStream) -> TokenStream` |
<!-- AUTO:END -->

## 职责

场景对象层的派生宏集合：SceneObject、Describable 与 Transform2DInto 三个入口，把对象声明里的重复样板在编译期展开。

## 运行机制

Describable 只接受单字段 tuple struct，按属性里列出的槽位生成 Deref / DerefMut 与可描述实现，描述符元组由属性逐项决定，缺的槽位在类型层面就写不出来。Transform2DInto 逐字段生成逻辑空间与屏幕空间两个消耗式转换，字段可用 skip 或 iter 改变转换方式，unit struct 直接透传。SceneObject 目前只解析输入并返回空 TokenStream，是尚未展开的占位派生。

## 边界与不做什么

- 不做领域校验：槽位只支持描边与填充，材质槽位尚未提供，未知槽位直接编译错误。
- SceneObject 当前不产出任何代码，不要依赖它自动实现 trait。
- 只做语法到代码的映射，不做类型检查，也不注册任何全局表。
