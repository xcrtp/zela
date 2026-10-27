//! 合一 / 子类型 / 布局计算 —— **纯代数**，无缓存、无注册表。
//!
//! # 名义类型
//!
//! `Struct` / `Enum` 的身份由 `name` 决定（计划 §3.1 的口径）：
//! 同名但字段或布局不同 ⇒ **冲突**，而不是"两个类型"。
//! 因为 `namespace + name` 是跨 pod 的契约，同名却不同布局正是最典型的静默。
//!
//! # 安全缺省
//!
//! `layout: None` 不是错误：没有布局就走序列化慢路径，语义与零拷贝完全一致。

use alloc::format;

use crate::conflict::{Blame, ReflectError};
use crate::layout::{Layout, LayoutError};
use crate::ty::{Field, ScalarKind, TypeSpec};

impl TypeSpec {
    /// 计算布局（`None` = 变长 / 未声明 ⇒ 慢路径）。
    pub fn layout(&self) -> Option<Layout> {
        match self {
            TypeSpec::Scalar(kind) => scalar_layout(*kind),
            TypeSpec::Struct { layout, .. } => *layout,
            TypeSpec::Array {
                elem,
                len: Some(len),
            } => {
                let elem = elem.layout()?;
                let count = *len;
                Some(Layout::new(
                    elem.size.checked_mul(count)?,
                    elem.align,
                    &[],
                ))
            }
            TypeSpec::Enum { .. }
            | TypeSpec::Array { len: None, .. }
            | TypeSpec::Opaque(_)
            | TypeSpec::Var(_)
            | TypeSpec::App { .. } => None,
        }
    }

    /// 两个描述是否指同**一个**类型（名义身份 + 结构一致 + 布局相容）。
    pub fn is_same(&self, other: &TypeSpec) -> bool {
        self.unify(other).is_ok()
    }

    /// 合一：成功返回统一后的类型；失败是显式诊断。
    pub fn unify(&self, other: &TypeSpec) -> Result<TypeSpec, ReflectError> {
        self.unify_with(other, Blame::Assembly)
    }

    /// 带污染源的合一（装配期用它把来源填进诊断）。
    pub fn unify_with(&self, other: &TypeSpec, origin: Blame) -> Result<TypeSpec, ReflectError> {
        // 类型变量是**待定**，合一到具体类型（rank-1，不做出现检查）。
        if matches!(self, TypeSpec::Var(_)) {
            return Ok(*other);
        }
        if matches!(other, TypeSpec::Var(_)) {
            return Ok(*self);
        }

        match (self, other) {
            (TypeSpec::Scalar(left), TypeSpec::Scalar(right)) if left == right => Ok(*self),
            (TypeSpec::Opaque(left), TypeSpec::Opaque(right)) if left == right => Ok(*self),
            (
                TypeSpec::Array {
                    elem: left_elem,
                    len: left_len,
                },
                TypeSpec::Array {
                    elem: right_elem,
                    len: right_len,
                },
            ) => {
                let elem = left_elem.unify_with(right_elem, origin.clone())?;
                let len = match (left_len, right_len) {
                    (Some(left), Some(right)) if left != right => {
                        return Err(ReflectError::TypeMismatch {
                            expected: format!("Array<{}, {left}>", left_elem.render()),
                            actual: format!("Array<{}, {right}>", right_elem.render()),
                            origin,
                        })
                    }
                    (Some(len), _) | (_, Some(len)) => Some(*len),
                    (None, None) => None,
                };
                Ok(TypeSpec::Array {
                    elem: BoxedSpec::leak(elem),
                    len,
                })
            }
            (
                TypeSpec::Struct {
                    name: left_name,
                    layout: left_layout,
                    fields: left_fields,
                },
                TypeSpec::Struct {
                    name: right_name,
                    layout: right_layout,
                    fields: right_fields,
                },
            ) => {
                if left_name != right_name {
                    return Err(ReflectError::TypeMismatch {
                        expected: self.render(),
                        actual: other.render(),
                        origin,
                    });
                }
                let layout = merge_layouts(left_name, *left_layout, *right_layout, origin.clone())?;
                unify_fields(left_name, left_fields, right_fields, origin)?;
                Ok(TypeSpec::Struct {
                    name: left_name,
                    layout,
                    fields: left_fields,
                })
            }
            (
                TypeSpec::Enum {
                    name: left_name,
                    variants: left_variants,
                },
                TypeSpec::Enum {
                    name: right_name,
                    variants: right_variants,
                },
            ) => {
                if left_name != right_name || left_variants.len() != right_variants.len() {
                    return Err(ReflectError::TypeMismatch {
                        expected: self.render(),
                        actual: other.render(),
                        origin,
                    });
                }
                for (left, right) in left_variants.iter().zip(right_variants.iter()) {
                    if left.name != right.name {
                        return Err(mismatch(self, other, origin));
                    }
                    unify_fields(left_name, left.fields, right.fields, origin.clone())?;
                }
                Ok(TypeSpec::Enum {
                    name: left_name,
                    variants: left_variants,
                })
            }
            (
                TypeSpec::App {
                    ty: left_ty,
                    args: left_args,
                },
                TypeSpec::App {
                    ty: right_ty,
                    args: right_args,
                },
            ) => {
                if left_ty != right_ty || left_args.len() != right_args.len() {
                    return Err(mismatch(self, other, origin));
                }
                for (left, right) in left_args.iter().zip(right_args.iter()) {
                    left.unify_with(right, origin.clone())?;
                }
                Ok(*self)
            }
            _ => Err(mismatch(self, other, origin)),
        }
    }

    /// 子类型：`self` 能否顶替 `other`。
    ///
    /// 当前口径（与合一同源，不引入第二套规则）：
    /// - 类型变量是顶（任何类型都是它的子类型）；
    /// - 同名名义类型且字段相容；
    /// - `layout: None` **不是**子类型关系的一部分（它只影响走哪条路径）。
    pub fn is_subtype_of(&self, other: &TypeSpec) -> bool {
        if matches!(other, TypeSpec::Var(_)) {
            return true;
        }
        self.unify(other).is_ok()
    }
}

fn mismatch(left: &TypeSpec, right: &TypeSpec, origin: Blame) -> ReflectError {
    ReflectError::TypeMismatch {
        expected: left.render(),
        actual: right.render(),
        origin,
    }
}

fn merge_layouts(
    name: &'static str,
    left: Option<Layout>,
    right: Option<Layout>,
    origin: Blame,
) -> Result<Option<Layout>, ReflectError> {
    match (left, right) {
        (Some(left), Some(right)) if left != right => Err(ReflectError::LayoutMismatch {
            ty: alloc::string::String::from(name),
            detail: format!(
                "同名类型声明了两份布局（size {} vs {}）—— 名义类型不允许第二个真源",
                left.size, right.size
            ),
            origin,
        }),
        (Some(layout), _) | (_, Some(layout)) => Ok(Some(layout)),
        (None, None) => Ok(None),
    }
}

fn unify_fields(
    name: &'static str,
    left: &'static [Field],
    right: &'static [Field],
    origin: Blame,
) -> Result<(), ReflectError> {
    if left.len() != right.len() {
        return Err(ReflectError::LayoutMismatch {
            ty: alloc::string::String::from(name),
            detail: format!("同名类型字段数不同（{} vs {}）", left.len(), right.len()),
            origin,
        });
    }
    for (left, right) in left.iter().zip(right.iter()) {
        if left.name != right.name {
            return Err(ReflectError::LayoutMismatch {
                ty: alloc::string::String::from(name),
                detail: format!("同名字段顺序不同（`{}` vs `{}`）", left.name, right.name),
                origin: origin.clone(),
            });
        }
        left.ty.unify_with(right.ty, origin.clone())?;
    }
    Ok(())
}

/// 标量的定长布局（`String` 变长 ⇒ `None`）。
fn scalar_layout(kind: ScalarKind) -> Option<Layout> {
    let layout = match kind {
        ScalarKind::F32 => Layout::new(4, 4, &[]),
        ScalarKind::F64 => Layout::new(8, 8, &[]),
        ScalarKind::I32 | ScalarKind::U32 => Layout::new(4, 4, &[]),
        ScalarKind::U64 => Layout::new(8, 8, &[]),
        ScalarKind::U8 | ScalarKind::Bool => Layout::new(1, 1, &[]),
        ScalarKind::String => return None,
    };
    Some(layout)
}

/// `TypeSpec` 的字段都是 `&'static`；合一时新建的数组元素需要一份 `'static` 存储。
///
/// 合一结果是**描述**，不是热路径数据：leak 一份极小的描述是可接受的
/// （与 `Layout::custom` 的第三方注册同口径）。若两边都没变（最常见），
/// 直接复用左值，不 leak。
struct BoxedSpec;

impl BoxedSpec {
    fn leak(spec: TypeSpec) -> &'static TypeSpec {
        // 复用已有静态描述，避免每次合一都分配。
        match spec {
            TypeSpec::Scalar(ScalarKind::F32) => &SCALAR_F32,
            TypeSpec::Scalar(ScalarKind::F64) => &SCALAR_F64,
            TypeSpec::Scalar(ScalarKind::I32) => &SCALAR_I32,
            TypeSpec::Scalar(ScalarKind::U32) => &SCALAR_U32,
            TypeSpec::Scalar(ScalarKind::U64) => &SCALAR_U64,
            TypeSpec::Scalar(ScalarKind::U8) => &SCALAR_U8,
            TypeSpec::Scalar(ScalarKind::Bool) => &SCALAR_BOOL,
            TypeSpec::Scalar(ScalarKind::String) => &SCALAR_STRING,
            other => alloc::boxed::Box::leak(alloc::boxed::Box::new(other)),
        }
    }
}

static SCALAR_F32: TypeSpec = TypeSpec::Scalar(ScalarKind::F32);
static SCALAR_F64: TypeSpec = TypeSpec::Scalar(ScalarKind::F64);
static SCALAR_I32: TypeSpec = TypeSpec::Scalar(ScalarKind::I32);
static SCALAR_U32: TypeSpec = TypeSpec::Scalar(ScalarKind::U32);
static SCALAR_U64: TypeSpec = TypeSpec::Scalar(ScalarKind::U64);
static SCALAR_U8: TypeSpec = TypeSpec::Scalar(ScalarKind::U8);
static SCALAR_BOOL: TypeSpec = TypeSpec::Scalar(ScalarKind::Bool);
static SCALAR_STRING: TypeSpec = TypeSpec::Scalar(ScalarKind::String);

/// 让 `LayoutError` 的转换在本模块也可见（诊断统一入口）。
pub fn layout_error(error: LayoutError) -> ReflectError {
    ReflectError::from(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{FieldLayout, FieldTy};

    const F32: TypeSpec = TypeSpec::Scalar(ScalarKind::F32);
    const F64: TypeSpec = TypeSpec::Scalar(ScalarKind::F64);

    const POINT_FIELDS: &[Field] = &[Field { name: "x", ty: &F32 }];
    const POINT_LAYOUT: Layout = Layout::new(4, 4, &[FieldLayout::new("x", 0, FieldTy::F32)]);

    const POINT: TypeSpec = TypeSpec::Struct {
        name: "acme.Point",
        layout: Some(POINT_LAYOUT),
        fields: POINT_FIELDS,
    };
    const POINT_NO_LAYOUT: TypeSpec = TypeSpec::Struct {
        name: "acme.Point",
        layout: None,
        fields: POINT_FIELDS,
    };
    const POINT_OTHER_FIELDS: TypeSpec = TypeSpec::Struct {
        name: "acme.Point",
        layout: None,
        fields: &[Field { name: "y", ty: &F32 }],
    };
    const OTHER_LAYOUT: Layout = Layout::new(8, 8, &[]);

    #[test]
    fn same_nominal_type_with_compatible_layouts_unifies() {
        assert!(POINT.unify(&POINT_NO_LAYOUT).is_ok(), "None 是安全缺省");
        assert!(POINT_NO_LAYOUT.unify(&POINT).is_ok());
    }

    #[test]
    fn same_name_with_different_layouts_is_a_conflict() {
        let conflicting = TypeSpec::Struct {
            name: "acme.Point",
            layout: Some(OTHER_LAYOUT),
            fields: POINT_FIELDS,
        };
        let error = POINT.unify(&conflicting).unwrap_err();
        assert_eq!(error.class(), "布局");
        assert_eq!(error.name(), Some("acme.Point"));
    }

    #[test]
    fn same_name_with_different_fields_is_a_conflict() {
        let error = POINT.unify(&POINT_OTHER_FIELDS).unwrap_err();
        assert_eq!(error.class(), "布局");
    }

    #[test]
    fn scalars_unify_only_with_themselves() {
        assert!(F32.unify(&F32).is_ok());
        assert_eq!(F32.unify(&F64).unwrap_err().class(), "类型");
    }

    #[test]
    fn type_variables_unify_with_concrete_types() {
        let var = TypeSpec::Var("T");
        assert_eq!(F32.unify(&var).unwrap(), F32);
        assert!(F32.is_subtype_of(&var));
        assert!(F32.is_subtype_of(&F32));
    }

    #[test]
    fn layout_is_computed_for_fixed_sized_types() {
        assert_eq!(F32.layout().map(|l| l.size), Some(4));
        assert_eq!(POINT.layout().map(|l| l.size), Some(4));
        assert_eq!(
            TypeSpec::Array {
                elem: &F32,
                len: Some(3)
            }
            .layout()
            .map(|l| l.size),
            Some(12)
        );
        assert!(TypeSpec::Scalar(ScalarKind::String).layout().is_none());
        assert!(
            TypeSpec::Array {
                elem: &F32,
                len: None
            }
            .layout()
            .is_none()
        );
    }
}
