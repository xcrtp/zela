// bind.rs — 绑定组的**符号表**（BP-1）。
//
// 回答三个问题，全部是纯数据：
//   1. 一个组名属于哪个组号（`group_of`）；
//   2. 一个组携带哪些资源名（`GroupSymbol::resources`）；
//   3. 一个资源名在哪个组里（`group_of_resource`）。
//
// 为什么放在 IR 层：名字是**声明**的一部分（`PassDecl.inputs/outputs` 已经在用），
// 而后端才持有真正的 `BindGroup`。本层不认识 wgpu，只让"名字 → 组"这一层
// 在两端共用同一份数据 —— 后端拿组号去取自己的组，校验侧拿资源名对声明。
//
// 名字不在这里新造：调用方用 shader 的 `@group/@binding` 名与资源槽位名建表。

/// 一个绑定组的符号：组号 + 组名 + 它携带的资源名。
///
/// `resources` 是**资源级**名字（`instances` / `atlas` / `shadow_maps` …），
/// 不是 binding 编号 —— 编号由后端的 layout 决定，IR 层不复制它。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupSymbol {
    /// 组号（`@group(n)` 的 n）。
    pub group: u32,
    /// 组名（稳定字面量，错误信息与声明用）。
    pub name: &'static str,
    /// 这个组携带的资源名。
    pub resources: &'static [&'static str],
}

/// 一张绑定组符号表。
#[derive(Debug, Clone, Copy)]
pub struct BindSymbols {
    groups: &'static [GroupSymbol],
}

/// 符号表自身的结构问题（显式报错，不纠正）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BindSymbolError {
    #[error("绑定组组号重复：{group}")]
    DuplicateGroup { group: u32 },
    #[error("绑定组名重复：{name:?}")]
    DuplicateName { name: &'static str },
    #[error("资源名 {resource:?} 同时出现在组 {first:?} 与组 {again:?}（一个资源只能绑在一个组里）")]
    DuplicateResource {
        resource: &'static str,
        first: &'static str,
        again: &'static str,
    },
}

impl BindSymbols {
    pub const fn new(groups: &'static [GroupSymbol]) -> Self {
        Self { groups }
    }

    /// 全部组符号（声明序）。
    pub const fn groups(&self) -> &'static [GroupSymbol] {
        self.groups
    }

    /// 校验：组号唯一、组名唯一、资源名不跨组重复。
    ///
    /// 组号**允许不连续**：编号来自 shader 的 `@group(n)`，不是本表分配的
    /// （有的 pass 不绑组 2，但组 3 仍然存在）。
    pub fn validate(&self) -> Result<(), BindSymbolError> {
        for (at, symbol) in self.groups.iter().enumerate() {
            if self.groups[..at].iter().any(|s| s.name == symbol.name) {
                return Err(BindSymbolError::DuplicateName { name: symbol.name });
            }
            if self.groups[..at].iter().any(|s| s.group == symbol.group) {
                return Err(BindSymbolError::DuplicateGroup {
                    group: symbol.group,
                });
            }
        }
        for (at, symbol) in self.groups.iter().enumerate() {
            for resource in symbol.resources {
                if let Some(first) = self.groups[..at]
                    .iter()
                    .find(|s| s.resources.contains(resource))
                {
                    return Err(BindSymbolError::DuplicateResource {
                        resource,
                        first: first.name,
                        again: symbol.name,
                    });
                }
            }
        }
        Ok(())
    }

    /// 组名 → 组符号。
    pub fn symbol(&self, name: &str) -> Option<&'static GroupSymbol> {
        self.groups.iter().find(|s| s.name == name)
    }

    /// 组名 → 组号。
    pub fn group_of(&self, name: &str) -> Option<u32> {
        self.symbol(name).map(|s| s.group)
    }

    /// 资源名 → 组符号。
    pub fn symbol_of_resource(&self, resource: &str) -> Option<&'static GroupSymbol> {
        self.groups
            .iter()
            .find(|s| s.resources.contains(&resource))
    }

    /// 资源名 → 组号。
    pub fn group_of_resource(&self, resource: &str) -> Option<u32> {
        self.symbol_of_resource(resource).map(|s| s.group)
    }

    /// 表里是否登记过这个名字（组名或资源名）。
    pub fn carries(&self, name: &str) -> bool {
        self.symbol(name).is_some() || self.group_of_resource(name).is_some()
    }

    /// `names` 里第一个表里没有登记的名字（校验"绑定了一个表外的名字"）。
    pub fn unknown<'a>(&self, names: &[&'a str]) -> Option<&'a str> {
        names.iter().copied().find(|name| !self.carries(name))
    }

    /// 这张表携带的全部资源名（声明序、去重）。
    pub fn resources(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.groups
            .iter()
            .flat_map(|symbol| symbol.resources.iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 本 crate 自己的（小）表：数字组的顺序与名字固定，用例拿它做查找。
    const TABLE: BindSymbols = BindSymbols::new(&[
        GroupSymbol {
            group: 0,
            name: "scene",
            resources: &["instances", "ctrl", "subs", "objects", "rows", "tiles"],
        },
        GroupSymbol {
            group: 1,
            name: "images",
            resources: &["atlas", "base"],
        },
        GroupSymbol {
            group: 3,
            name: "lighting",
            resources: &["shadow_maps", "shadow_compare"],
        },
    ]);

    #[test]
    fn names_resolve_to_groups() {
        TABLE.validate().unwrap();
        assert_eq!(TABLE.group_of("scene"), Some(0));
        assert_eq!(TABLE.group_of("images"), Some(1));
        assert_eq!(TABLE.group_of("lighting"), Some(3));
        assert_eq!(TABLE.group_of("atlas"), None, "资源名不是组名");
        assert_eq!(TABLE.group_of_resource("shadow_maps"), Some(3));
        assert_eq!(TABLE.group_of_resource("atlas"), Some(1));
        assert_eq!(TABLE.group_of_resource("environment"), None);
        assert!(TABLE.carries("rows"));
        assert!(TABLE.carries("lighting"));
        assert!(!TABLE.carries("view"));
    }

    #[test]
    fn unknown_names_are_named() {
        assert_eq!(TABLE.unknown(&["atlas", "rows"]), None);
        assert_eq!(TABLE.unknown(&["atlas", "not_a_thing"]), Some("not_a_thing"));
        let all: Vec<&str> = TABLE.resources().collect();
        assert_eq!(all.len(), 10);
        assert_eq!(all[0], "instances");
    }

    /// 组号可以不连续（编号来自 shader，不是本表分配）：唯一即可。
    #[test]
    fn group_numbers_may_be_sparse() {
        const SPARSE: BindSymbols = BindSymbols::new(&[
            GroupSymbol {
                group: 0,
                name: "scene",
                resources: &["instances"],
            },
            GroupSymbol {
                group: 3,
                name: "lighting",
                resources: &["shadow_maps"],
            },
        ]);
        SPARSE.validate().unwrap();
        assert_eq!(SPARSE.group_of("lighting"), Some(3));
    }

    /// 证伪：组号重复 ⇒ 红。
    #[test]
    fn duplicate_group_numbers_are_rejected() {
        const TWICE: BindSymbols = BindSymbols::new(&[
            GroupSymbol {
                group: 0,
                name: "a",
                resources: &["x"],
            },
            GroupSymbol {
                group: 0,
                name: "b",
                resources: &["y"],
            },
        ]);
        assert_eq!(
            TWICE.validate(),
            Err(BindSymbolError::DuplicateGroup { group: 0 })
        );
    }

    /// 证伪 2：组名重复 ⇒ 红。
    #[test]
    fn duplicate_group_names_are_rejected() {
        const TWICE: BindSymbols = BindSymbols::new(&[
            GroupSymbol {
                group: 0,
                name: "same",
                resources: &["x"],
            },
            GroupSymbol {
                group: 1,
                name: "same",
                resources: &["y"],
            },
        ]);
        assert_eq!(
            TWICE.validate(),
            Err(BindSymbolError::DuplicateName { name: "same" })
        );
    }

    /// 证伪 3：同一资源名出现在两个组 ⇒ 红（否则"名 → 组"有歧义）。
    #[test]
    fn resources_may_not_span_groups() {
        const TWICE: BindSymbols = BindSymbols::new(&[
            GroupSymbol {
                group: 0,
                name: "a",
                resources: &["shared"],
            },
            GroupSymbol {
                group: 1,
                name: "b",
                resources: &["shared"],
            },
        ]);
        assert_eq!(
            TWICE.validate(),
            Err(BindSymbolError::DuplicateResource {
                resource: "shared",
                first: "a",
                again: "b"
            })
        );
    }
}
