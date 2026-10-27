//! **算子元数据**：`OpDef { since, changed, pointwise, targets }`。
//!
//! # 为什么元数据与语义同源
//!
//! 算子身份（namespace + name）是数据，元数据描述这条数据的**契约**：
//! - `since` / `changed`：函数级版本，产物加载时点名拒绝（不是整库作废）；
//! - `pointwise`：连续层的事实来源（见计划 §3.1），读了非 pointwise ⇒ 整棵表达式变非 pointwise；
//! - `targets`：出口能力占位（可代码生成 / 仅宿主 / 需运行时），与 `provides/requires` 同构。
//!
//! 语义实现见 [`crate::ops`]，本模块只描述元数据；两者由
//! [`crate::op::DEFS`] 的顺序测试钉在一起（漏登记当场暴露）。
//!
//! # 查表是 O(1)
//!
//! `op_def` 以前是 `DEFS.iter().find(..)` —— 每次都扫。现在装配期把名字解析
//! 一次进符号表，之后是整数槽位（计划 §3.5 的同一机制）。

use std::sync::OnceLock;

use ze_reflect::{Blame, SymbolTable};

/// 算子出口能力（阶段 4 的占位；当前全部只由宿主求值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// 可代码生成（未来：WGSL / C）。
    Codegen,
    /// 只能由宿主实现（当前唯一已兑现场景）。
    HostOnly,
    /// 需要运行时支持（如查表 / 环境）。
    Runtime,
}

/// 一个算子的元数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpDef {
    pub name: &'static str,
    /// 引入此算子的库 revision。
    pub since: u32,
    /// 语义最后一次变更的 revision（**签名不变但实现变了也要 bump**）。
    pub changed: u32,
    /// 连续层声明：逐点求值（`f: ℝⁿ → V`）。
    pub pointwise: bool,
    /// 出口能力（占位）。
    pub targets: &'static [Target],
}

impl OpDef {
    /// 完整限定名（`zela.math::sin`）。
    pub fn qualified(&self) -> String {
        format!("{LIBRARY_NAMESPACE}::{}", self.name)
    }
}

/// 本函子库的 namespace（进产物）。
pub const LIBRARY_NAMESPACE: &str = "zela.math";

/// 当前全部算子：只由宿主实现。
const HOST_ONLY: &[Target] = &[Target::HostOnly];

const fn def(name: &'static str, since: u32, changed: u32) -> OpDef {
    OpDef {
        name,
        since,
        changed,
        // 全部现有算子都是纯函数（同一实参 ⇒ 同一结果），属于连续层。
        pointwise: true,
        targets: HOST_ONLY,
    }
}

/// 全部算子元数据（顺序与 [`crate::ops::OPS`] 一致）。
pub static DEFS: &[OpDef] = &[
    def("sin", 1, 1),
    def("cos", 1, 1),
    def("add", 1, 1),
    def("mul", 1, 1),
    def("scale", 1, 1),
    def("sum", 1, 1),
    def("mix", 9, 9),
    def("clamp", 9, 9),
    def("vec2", 9, 9),
    def("vec3", 9, 9),
    def("x", 9, 9),
    def("y", 9, 9),
    def("z", 9, 9),
    def("rotate3", 9, 9),
    def("splat", 11, 11),
    def("vec3_from_vec2", 11, 11),
    def("vec2_from_vec3", 11, 11),
    def("length", 11, 11),
    def("normalize", 11, 11),
    def("round_f32", 12, 12),
    def("lt", 1, 1),
    def("gt", 1, 1),
    def("le", 1, 1),
    def("ge", 1, 1),
    def("eq", 1, 1),
    def("ne", 1, 1),
    def("and", 1, 1),
    def("or", 1, 1),
    def("not", 1, 1),
];

/// 查一个算子的元数据（**O(1)**：符号表在首次查询时建立一次）。
pub fn op_def(name: &str) -> Option<&'static OpDef> {
    index().resolve(name).map(|(_, def)| *def)
}

/// 名字 → 元数据的符号表（`OnceLock`：只建一次）。
fn index() -> &'static SymbolTable<&'static str, &'static OpDef> {
    static INDEX: OnceLock<SymbolTable<&'static str, &'static OpDef>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut table = SymbolTable::new();
        for def in DEFS {
            table
                .intern(def.name, def, Blame::Assembly)
                .expect("DEFS 的名字唯一（顺序测试已钉住）");
        }
        table
    })
}

/// 某算子的 `(since, changed)`（产物记录 / 加载比对用）。
pub fn op_revision(name: &str) -> Option<(u32, u32)> {
    op_def(name).map(|def| (def.since, def.changed))
}

/// 算子是否逐点（未知算子返回 `None`，不静默当作 `true`）。
pub fn is_pointwise(name: &str) -> Option<bool> {
    op_def(name).map(|def| def.pointwise)
}

/// 算子出口能力（未知算子返回 `None`）。
pub fn targets(name: &str) -> Option<&'static [Target]> {
    op_def(name).map(|def| def.targets)
}

#[cfg(test)]
mod tests {
    /// 元数据表与算子表**同名同序**：漏登记 / 顺序漂移会在这里当场暴露。
    #[test]
    fn defs_cover_every_op_once() {
        let names: Vec<&str> = super::DEFS.iter().map(|def| def.name).collect();
        assert_eq!(names.as_slice(), crate::ops::OPS);
    }

    /// 既有算子的 revision 与批次对应（1 = 追踪开始前、9 = 第 1 批、11 = 第 3 批、12 = 第 5a 批）。
    #[test]
    fn revisions_follow_the_batches() {
        assert_eq!(super::op_revision("sin"), Some((1, 1)));
        assert_eq!(super::op_revision("mix"), Some((9, 9)));
        assert_eq!(super::op_revision("normalize"), Some((11, 11)));
        assert_eq!(super::op_revision("round_f32"), Some((12, 12)));
        assert_eq!(super::op_revision("read"), None, "read 不是值算子");
    }

    /// 现有算子全部声明为连续层（逐点）；`targets` 占位是"仅宿主"。
    #[test]
    fn every_op_is_pointwise_and_host_only_for_now() {
        for def in super::DEFS {
            assert!(def.pointwise, "{} 应逐点", def.name);
            assert_eq!(def.targets, &[super::Target::HostOnly]);
            assert_eq!(def.qualified(), format!("zela.math::{}", def.name));
        }
    }

    /// 未知算子不静默返回 `true` / 空能力。
    #[test]
    fn unknown_ops_are_explicit() {
        assert_eq!(super::is_pointwise("warp"), None);
        assert_eq!(super::targets("warp"), None);
    }
}
