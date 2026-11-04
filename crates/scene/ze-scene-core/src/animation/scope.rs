//! 作用域（CC-a）：给绑定表与时钟一个**声明式**的归属维度。
//!
//! # 为什么是"声明序号"而不是运行期 id
//!
//! `ScopeId` 的内部形态与 [`Handle`](crate::scene_script::Handle) 同构：
//! **声明顺序**（第一次出现的次序）就是序号。`append` 的句柄偏移逻辑因此可以
//! 直接复用，且产物里只写**路径**（`panel_a`），不写运行期分配的 id。
//!
//! # 双轨
//!
//! - 内部（`ScopeId(u32)`）：绑定表 / 参数地址用序号，确定性天然成立；
//! - 对外（[`BindingTable::scope_path`]）：路径字符串，供参数面板 / 诊断显示。
//!
//! # 作用域时钟
//!
//! 作用域自带一条时钟链（`ScopeSpec::clock`）；其内部的绑定把各自的
//! `ClockProgram` **接在作用域时钟之下**。两个作用域因此各有独立的时间轴 ——
//! 这是"同一段脚本实例化 N 次"的本质（见 CC 计划 §2.2）。

use serde::{Deserialize, Serialize};

use ze_time::ClockProgram;

/// 作用域的内部序号（0 = 根作用域）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub struct ScopeId(pub u32);

impl ScopeId {
    /// 根作用域（无 scope 包装的既有脚本）。
    pub const ROOT: Self = Self(0);

    /// 是否根作用域。
    pub fn is_root(self) -> bool {
        self.0 == 0
    }
}

/// 一条作用域声明：**对外路径 + 自带时钟**。
///
/// 路径进产物（声明式）；时钟链也进产物（`ClockProgram` 本来就是声明的链）。
/// 序号不进产物 —— 反序列化后按 `Vec` 下标重新得到。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScopeSpec {
    /// 对外路径（`panel_a` / `spring_3`）。
    pub path: String,
    /// 作用域自带的时钟链（空 = 全局时间轴）。
    #[serde(default, skip_serializing_if = "ClockProgram::is_global")]
    pub clock: ClockProgram,
}

impl ScopeSpec {
    /// 新建一条作用域声明。
    pub fn new(path: impl Into<String>, clock: ClockProgram) -> Self {
        Self {
            path: path.into(),
            clock,
        }
    }
}
