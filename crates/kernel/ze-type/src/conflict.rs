//! **冲突诊断**：五类冲突收敛为一个类型。
//!
//! # 为什么只有一个
//!
//! 现在（迁移前）"名字冲突"这件事有四种诊断形态：
//! `ExprError::DuplicateId` / `PassRegistryError::Duplicate` /
//! `LayoutError::DuplicateType` / `ze-node` 侧的 `DuplicateEdge`。
//! 作者要学四套，而这四套说的是同一件事。本模块给它**一个**名字：
//! [`ReflectError`]。
//!
//! # 污染源
//!
//! 每条诊断都带 [`Blame`]（污染源）：是哪个 Pod / 哪份产物带来的。
//! 与 `pointwise` 的 `blocked(污染源)` 同形态 —— 出了问题要能顺藤摸瓜，
//! 而不是只说"冲突了"。

use alloc::string::String;
use core::fmt;

use crate::layout::LayoutError;

/// 冲突发生在**哪一类名字**上。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    /// 表达式节点 id。
    NodeId,
    /// 分析 pass 名。
    Pass,
    /// 类型名（`namespace + name` 是跨 pod 契约）。
    Type,
    /// 符号 / 函数名。
    Symbol,
    /// 字段名。
    Field,
    /// 图边（`(from, to)`）。
    Edge,
}

impl NameKind {
    /// 诊断里的中文名。
    pub const fn label(self) -> &'static str {
        match self {
            NameKind::NodeId => "节点 id",
            NameKind::Pass => "pass 名",
            NameKind::Type => "类型名",
            NameKind::Symbol => "符号名",
            NameKind::Field => "字段名",
            NameKind::Edge => "图边",
        }
    }
}

/// **污染源**：这条诊断是谁带来的。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Blame {
    /// 装配期（本进程内的注册 / 校验）。
    Assembly,
    /// 某个 Pod（名字即 Pod 的 service 名）。
    Pod(String),
    /// 某份产物 / payload。
    Payload(String),
    /// 已知有冲突，但拿不到更具体的来源（不静默省略，显式标注）。
    Unknown,
}

impl Blame {
    pub fn pod(name: impl Into<String>) -> Self {
        Self::Pod(name.into())
    }

    pub fn payload(name: impl Into<String>) -> Self {
        Self::Payload(name.into())
    }
}

impl fmt::Display for Blame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Assembly => write!(f, "装配期"),
            Self::Pod(name) => write!(f, "pod `{name}`"),
            Self::Payload(name) => write!(f, "产物 `{name}`"),
            Self::Unknown => write!(f, "未知来源"),
        }
    }
}

/// 解析协议 / 类型系统的统一诊断。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReflectError {
    /// 重名（同一作用域里同名对象出现两次）。
    DuplicateName {
        kind: NameKind,
        name: String,
        origin: Blame,
    },
    /// 合一失败（两个类型描述不相容）。
    TypeMismatch {
        expected: String,
        actual: String,
        origin: Blame,
    },
    /// 布局协商失败（零拷贝条件不成立且无法重编码）。
    LayoutMismatch {
        ty: String,
        detail: String,
        origin: Blame,
    },
    /// 版本不匹配（`since` / `changed` 点名拒绝）。
    VersionMismatch {
        op: String,
        required_since: u32,
        available_since: u32,
        origin: Blame,
    },
    /// 能力不满足（`provides` / `requires`）。
    CapabilityMissing {
        capability: String,
        required_by: String,
        origin: Blame,
    },
    /// 布局自检失败（结构问题，不是"冲突"，但同样是显式错误）。
    Layout(LayoutError),
}

impl ReflectError {
    /// 重名诊断的统一构造入口（四个旧枚举都从这里走）。
    pub fn duplicate(kind: NameKind, name: impl Into<String>, origin: Blame) -> Self {
        Self::DuplicateName {
            kind,
            name: name.into(),
            origin,
        }
    }

    /// 污染源。
    pub fn origin(&self) -> Blame {
        match self {
            Self::DuplicateName { origin, .. }
            | Self::TypeMismatch { origin, .. }
            | Self::LayoutMismatch { origin, .. }
            | Self::VersionMismatch { origin, .. }
            | Self::CapabilityMissing { origin, .. } => origin.clone(),
            Self::Layout(_) => Blame::Assembly,
        }
    }

    /// 诊断能定位到的具体名字（没有名字的冲突返回 `None`）。
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::DuplicateName { name, .. } => Some(name),
            Self::LayoutMismatch { ty, .. } => Some(ty),
            Self::VersionMismatch { op, .. } => Some(op),
            Self::TypeMismatch { .. }
            | Self::CapabilityMissing { .. }
            | Self::Layout(_) => None,
        }
    }

    /// 冲突类别（文档 / 汇总用）。
    pub const fn class(&self) -> &'static str {
        match self {
            Self::DuplicateName { .. } => "重名",
            Self::TypeMismatch { .. } => "类型",
            Self::LayoutMismatch { .. } | Self::Layout(_) => "布局",
            Self::VersionMismatch { .. } => "版本",
            Self::CapabilityMissing { .. } => "能力",
        }
    }
}

impl fmt::Display for ReflectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateName { kind, name, origin } => write!(
                f,
                "{}冲突：`{name}`（污染源：{origin}）",
                kind.label()
            ),
            Self::TypeMismatch {
                expected,
                actual,
                origin,
            } => write!(
                f,
                "类型不合一：期望 `{expected}`，实际 `{actual}`（污染源：{origin}）"
            ),
            Self::LayoutMismatch { ty, detail, origin } => write!(
                f,
                "类型 `{ty}` 布局协商失败：{detail}（污染源：{origin}）"
            ),
            Self::VersionMismatch {
                op,
                required_since,
                available_since,
                origin,
            } => write!(
                f,
                "算子 `{op}` 版本不匹配：需要 since ≥ {required_since}，实际 {available_since}（污染源：{origin}）"
            ),
            Self::CapabilityMissing {
                capability,
                required_by,
                origin,
            } => write!(
                f,
                "缺少能力 `{capability}`（由 `{required_by}` 要求；污染源：{origin}）"
            ),
            Self::Layout(error) => write!(f, "{error}"),
        }
    }
}

impl core::error::Error for ReflectError {}

impl From<LayoutError> for ReflectError {
    fn from(error: LayoutError) -> Self {
        Self::Layout(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    #[test]
    fn every_conflict_carries_a_blame_and_a_name() {
        let duplicate = ReflectError::duplicate(NameKind::NodeId, "dup", Blame::Assembly);
        assert_eq!(duplicate.name(), Some("dup"));
        assert_eq!(duplicate.origin(), Blame::Assembly);
        assert!(duplicate.to_string().contains("dup"));

        let missing = ReflectError::CapabilityMissing {
            capability: "zela.math.Sin".to_string(),
            required_by: "expr".to_string(),
            origin: Blame::pod("lua"),
        };
        assert_eq!(missing.name(), None);
        assert_eq!(missing.origin(), Blame::pod("lua"));
        assert!(format!("{missing}").contains("zela.math.Sin"));
    }

    #[test]
    fn layout_errors_convert_into_the_unified_diagnostic() {
        let error = ReflectError::from(LayoutError::ZeroSize);
        assert_eq!(error.class(), "布局");
        assert_eq!(error.origin(), Blame::Assembly);
    }
}
