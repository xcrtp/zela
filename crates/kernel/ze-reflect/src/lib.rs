//! ze-reflect：**运行时反射层**。
//!
//! 类型描述（慢形式）在 `ze-type`；本 crate 负责把它**一次性解析**成进程内形式，
//! 并用 generation 保证这个转换不会失效：
//!
//! | 对象 | 慢形式 | 快形式 | 模块 |
//! |---|---|---|---|
//! | 类型 | `TypeSpec` | `Layout` / 槽位 | [`codec`] |
//! | 名字 | 限定名字符串 | [`sym::SymbolId`] | [`sym`] |
//! | 运行时对象 | `Handle { owner, slot, gen }` | 槽位里的引用 | [`handle`] |
//!
//! 三者是**同一个机制**作用在三种对象上（计划 §1 / §7）：符号表本身就是 handle
//! arena，句柄本身就是运行时对象的符号。
//!
//! # 边界
//!
//! 本 crate 不认识任何具体领域类型（Pod / 场景 / GPU）：各领域在自己的 crate 里
//! 实现 [`handle::HandleCore`] 即可。这样内核层不会反向依赖宿主层。

pub mod cache;
pub mod codec;
pub mod handle;
pub mod sym;

pub use cache::MonoCache;
pub use codec::{Codec, PathCodec, RecordCodec, TypeRegistry, PATH_TYPE};
pub use handle::{duplicate_name, HandleCore, HandleError, Resolvable, SlotTable};
pub use sym::{SymbolId, SymbolSpace, SymbolTable};

// 诊断只有一个名字：类型层定义，运行时层重导出（一个概念一处定义）。
pub use ze_type::conflict::{Blame, NameKind, ReflectError};
