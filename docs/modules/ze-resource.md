# ze-resource

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`kernel`（`crates/kernel`）
- 路径：`crates/kernel/ze-resource`
- 生产依赖（上游，dev-deps 不计）：（无）
- 被依赖（下游）：`zela-render`、`zela-render-backends`
- 源文件：3 个 · 704 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 14 | — |
| `src/render_backend.rs` | 181 | — |
| `src/resource_cache.rs` | 509 | — |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/render_backend.rs` | `pub struct GpuResourceId` |
| `src/render_backend.rs` | `pub enum AccessError` |
| `src/render_backend.rs` | `pub struct GpuHandle` |
| `src/resource_cache.rs` | `pub enum ResourceKey` |
| `src/resource_cache.rs` | `pub struct DeferredDestroyer` |
| `src/resource_cache.rs` | `pub struct CacheEntry<T>` |
| `src/resource_cache.rs` | `pub struct ResourceCache<T>` |
<!-- AUTO:END -->

## 职责

GPU 资源标识与缓存契约：零外部依赖，持有世代感知的资源标识、类型擦除句柄与资源缓存。

## 运行机制

src/render_backend.rs 用 index + generation 标识一个资源，并用类型擦除句柄承载任意后端对象，跨世代访问返回显式错误而不是读到别人的数据；src/resource_cache.rs 用内容哈希或版本号作键索引条目，缓存维护世代感知槽位与键到 id 的映射，所有释放都进延迟销毁队列（默认延迟 3 帧）而不是直接 drop，避免销毁当帧仍在使用的资源。缓存的后端参数是泛型，内核层不知道后端 trait 的存在。

## 边界与不做什么

- 不引用 wgpu / hecs / egui，也不定义渲染后端 trait（后者在渲染层）。
- 不做真实的 GPU 分配、上传与销毁，只出标识、缓存决策与销毁时机。
- 不承担旧键类型的兼容转换，那层适配留在渲染层。
