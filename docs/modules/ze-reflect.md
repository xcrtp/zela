# ze-reflect

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`kernel`（`crates/kernel`）
- 路径：`crates/kernel/ze-reflect`
- 生产依赖（上游，dev-deps 不计）：`ze-type`
- 被依赖（下游）：`ze-func`、`ze-pod-core`、`ze-pod-func`、`ze-scene-core`
- 源文件：5 个 · 869 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/cache.rs` | 98 | **单态化缓存**：按需生成、按 op 缓存、带上界。 |
| `src/codec.rs` | 259 | **Codec**：跨边界 `Bytes ↔ typed`，慢路径的正式名分。 |
| `src/handle.rs` | 233 | **统一句柄机制**：generation 感知的槽位表 + 解析协议。 |
| `src/lib.rs` | 31 | ze-reflect：**运行时反射层**。 |
| `src/sym.rs` | 248 | **符号表 = handle arena**（计划 §1）。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/cache.rs` | `pub struct MonoCache` |
| `src/codec.rs` | `pub struct TypeRegistry` |
| `src/codec.rs` | `pub trait Codec` |
| `src/codec.rs` | `pub struct RecordCodec` |
| `src/codec.rs` | `pub struct PathCodec` |
| `src/codec.rs` | `pub fn encode_path_bytes(points: &[[f32; 2]], closed: bool) -> Vec<u8>` |
| `src/codec.rs` | `pub fn decode_path_bytes(bytes: &[u8]) -> Result<(Vec<[f32; 2]>, bool), LayoutError>` |
| `src/handle.rs` | `pub trait HandleCore` |
| `src/handle.rs` | `pub enum HandleError` |
| `src/handle.rs` | `pub trait Resolvable` |
| `src/handle.rs` | `pub struct SlotTable<T>` |
| `src/handle.rs` | `pub fn duplicate_name(kind: NameKind, name: &str, origin: Blame) -> ReflectError` |
| `src/sym.rs` | `pub struct SymbolSpace` |
| `src/sym.rs` | `pub struct SymbolId` |
| `src/sym.rs` | `pub struct SymbolTable<K, E>` |
<!-- AUTO:END -->

## 职责

运行时反射层：把慢形式的类型、名字与运行时对象一次性解析成进程内快形式，并用 generation 保证这次转换不会失效；持有符号表、句柄槽位表、类型注册表与单态化缓存。

## 运行机制

装配期把类型描述注册进 src/codec.rs 的类型注册表（名字 → 布局，没有布局的变长类型安全退回序列化慢路径）；src/handle.rs 的槽位表按 owner + slot + generation 解析与校验，src/sym.rs 复用同一机制把限定名解析成带 generation 的符号 id，快路径只做符号 → 槽位 → 引用。src/cache.rs 按算子名缓存单态化实例，实例数越界返回显式错误而不是静默截断，同一份签名永远得到同一组实例。

## 边界与不做什么

- 不认识任何具体领域类型（Pod / 场景 / GPU）：各领域在自己的 crate 里实现句柄协议，内核层因此不会反向依赖宿主层。
- 不做类型检查与单态化算法本身，那是 ze-type 的纯函数；这里只持有运行时状态（注册表、缓存、槽位表）。
- 不提供全局单例，缓存与注册表由调用方持有并传入。
