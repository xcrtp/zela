# ze-scene-hecs

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`scene`（`crates/scene`）
- 路径：`crates/scene/ze-scene-hecs`
- 生产依赖（上游，dev-deps 不计）：`ze-scene-contract`
- 被依赖（下游）：`zela-render`、`zela-scene-ecs`
- 源文件：1 个 · 127 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 127 | — |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/lib.rs` | `pub fn object_id_of(e: Entity) -> ObjectId` |
| `src/lib.rs` | `pub fn entity_of(id: ObjectId) -> Option<Entity>` |
| `src/lib.rs` | `pub struct WorldView<'a>` |
<!-- AUTO:END -->

## 职责

契约层的 hecs 只读适配器：承载全仓唯一的 hecs 接触面——实体与对象 id 的互转，以及只读场景视图。

## 运行机制

身份互转是位模式直通：实体的低 32 位作 index、高 32 位作 generation 组成对象 id，反向构造在位模式非法时返回 None。只读视图借用不可变的世界实现契约层的只读场景 trait：包含性与计数直查，批量遍历用带实体项的查询迭代，按 id 取单组件时借用守卫只在回调内有效，组件缺失返回 None 而不 panic。

## 边界与不做什么

- 只读：写适配器留在 ECS 实现层，因为它需要组件登记表。
- 不依赖渲染层，也不把 hecs 类型泄露给契约层。
- 不做组件登记、系统调度或生命周期管理。
