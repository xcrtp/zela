//! 能力计划：把 `C::calls()` 摊平成"函数 + 可达类型"，供任意语言适配器消费。
//!
//! 这里**没有任何语言类型**：适配器只拿到 `ErasedFn` 与
//! `TypeSpec`，自行决定怎么建闭包 / 模块。

use std::collections::BTreeMap;

use ze_pod_core::{ErasedFn, TypeSpec};

/// 一个 capability 的绑定计划。
#[derive(Clone)]
pub struct BindingPlan {
    /// 函数清单（与 `signatures()` 同名同序）。
    pub calls: &'static [ErasedFn],
    /// 可达命名类型（递归收集；不含 `()`）。
    pub types: BTreeMap<&'static str, &'static TypeSpec>,
}

impl BindingPlan {
    /// 从 `C::calls()` 构造。
    pub fn new(calls: &'static [ErasedFn]) -> Self {
        let mut types = BTreeMap::new();
        for item in calls {
            for param in item.io.params {
                collect_types(param.ty, &mut types);
            }
            collect_types(item.io.ret, &mut types);
        }
        Self { calls, types }
    }

    /// 「单参数是该结构体 + 返回不透明句柄」的 capability 视为组件构造器。
    pub fn component_ctor(&self, type_name: &'static str) -> Option<ErasedFn> {
        self.calls.iter().copied().find(|item| {
            item.io.params.len() == 1
                && matches!(
                    item.io.params[0].ty,
                    TypeSpec::Struct { name, .. } if *name == type_name
                )
                && item.io.ret.is_opaque()
        })
    }
}

/// 递归收集命名类型（struct / enum）。
pub fn collect_types(
    spec: &'static TypeSpec,
    out: &mut BTreeMap<&'static str, &'static TypeSpec>,
) {
    match spec {
        TypeSpec::Struct { name, fields } => {
            if *name != "()" {
                out.insert(*name, spec);
            }
            for field in *fields {
                collect_types(field.ty, out);
            }
        }
        TypeSpec::Enum { name, variants } => {
            out.insert(*name, spec);
            for variant in *variants {
                for field in variant.fields {
                    collect_types(field.ty, out);
                }
            }
        }
        TypeSpec::Array { elem } => collect_types(elem, out),
        // 泛型实参也是可达类型：`Expr<Vec2>` 里的 `Vec2` 必须进类型表（§7.3）。
        TypeSpec::App { args, .. } => {
            for arg in *args {
                collect_types(arg, out);
            }
        }
        TypeSpec::Scalar(_) | TypeSpec::Opaque(_) | TypeSpec::Var(_) => {}
    }
}

/// `"zela.Color"` → `"Color"`（语言侧全局名用短名）。
pub fn short_name(full: &'static str) -> &'static str {
    full.rsplit(|c| c == '.' || c == ':').next().unwrap_or(full)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_pod_core::TypeSpec;

    const VEC2_SPEC: TypeSpec = TypeSpec::Struct {
        name: "zela.Vec2",
        fields: &[],
    };
    const EXPR_VEC2: TypeSpec = TypeSpec::App {
        ty: "zela.Expr",
        args: &[VEC2_SPEC],
    };

    #[test]
    fn app_type_arguments_are_reachable() {
        // 不做这一步，`Expr<Vec2>` 里的 `Vec2` 就不会进类型表（§7.3 / S1）。
        let mut out = BTreeMap::new();
        collect_types(&EXPR_VEC2, &mut out);
        assert!(out.contains_key("zela.Vec2"));
    }

    #[test]
    fn type_variables_contribute_nothing() {
        let mut out = BTreeMap::new();
        collect_types(&TypeSpec::Var("T"), &mut out);
        assert!(out.is_empty());
    }
}
