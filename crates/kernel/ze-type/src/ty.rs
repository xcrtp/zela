//! 类型描述（IDL schema）：语言无关、零依赖、编译期常量。
//!
//! # 为什么需要它
//!
//! 旧形态里 `PodTypeName = &'static str`，`"FloatList"` / `"ComponentList"` 只是
//! **没有定义的名字** —— 生成器不知道元素类型，文档只能写 `table`，绑定只能手写。
//!
//! [`TypeSpec`] 把"类型"从字符串升级成**结构**：标量 / 结构体 / 枚举 / 数组 / 不透明句柄。
//! 它是**描述**，不是值载体；跨语言的值见 `ze_pod_core::PodValue`。
//!
//! # Layout 为什么在这里
//!
//! `Layout` 原本是 `ze-func` 的平行子系统（计划 §0.1 的第 ⑤ 项）。它是
//! `Struct` 的字段，不是另一套类型描述：`layout: Option<Layout>` 中的 `None`
//! 是**安全缺省**（没有布局就走序列化慢路径，语义完全一致）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::layout::Layout;

/// 标量种类（最小集：场景实际用到的）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarKind {
    F32,
    F64,
    I32,
    /// 64 位有符号整数（离散层索引 / 帧号；**不经过 f64**）。
    ///
    /// 值侧 `Value::Scalar` 的 I64 必须能投影成类型侧的一种标量，
    /// 否则「值侧对齐类型侧」会留下一个没有类型的值种类。
    I64,
    U32,
    /// 64 位无符号整数（`u64` / `PodHandle` 的寻址号）。
    U64,
    /// 颜色分量（`Color32` 的 r/g/b/a）。
    U8,
    Bool,
    String,
}

/// 类型描述。
///
/// 所有字段都是 `&'static`：schema 由 derive 生成成编译期常量，零运行时开销。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeSpec {
    Scalar(ScalarKind),
    Struct {
        name: &'static str,
        /// 定长布局；`None` = 走序列化慢路径（**安全缺省**，不是隐患）。
        layout: Option<Layout>,
        fields: &'static [Field],
    },
    Enum {
        name: &'static str,
        variants: &'static [Variant],
    },
    Array {
        elem: &'static TypeSpec,
        /// 定长数组的长度；`None` = 变长。
        len: Option<usize>,
    },
    /// 不透明句柄（组件 / 对象 / 曲线 / 动作）：值只以 `Handle` 过边界。
    Opaque(&'static str),
    /// 类型变量（S1）：只出现在**函子库作者侧**的签名与诊断里。
    ///
    /// 导出到脚本侧时退化为 `any` / `unknown` / `Any` —— 本批不做合一。
    Var(&'static str),
    /// 类型构造应用（S1）：如 `zela.Expr<f32>`。
    ///
    /// `ty` 是构造器的**限定名**，`args` 是实参。它让 `Expr<f32>` 这类类型能精确进
    /// 声明文件，而不需要引入类型变量与合一 —— 两者是独立的事。
    App {
        ty: &'static str,
        args: &'static [TypeSpec],
    },
}

impl TypeSpec {
    /// 命名类型的名字（标量 / 数组 / 类型变量返回 `None`）。
    ///
    /// `App` 返回**构造器名**（`"zela.Expr"`）；含实参的完整串由导出器按需拼出。
    pub const fn type_name(&self) -> Option<&'static str> {
        match self {
            TypeSpec::Struct { name, .. } | TypeSpec::Enum { name, .. } => Some(*name),
            TypeSpec::Opaque(name) => Some(*name),
            TypeSpec::App { ty, .. } => Some(*ty),
            TypeSpec::Scalar(_) | TypeSpec::Array { .. } | TypeSpec::Var(_) => None,
        }
    }

    /// 是否是不透明句柄。
    pub const fn is_opaque(&self) -> bool {
        matches!(self, TypeSpec::Opaque(_))
    }

    /// 结构体声明的布局（`None` = 慢路径）。
    pub const fn declared_layout(&self) -> Option<Layout> {
        match self {
            TypeSpec::Struct { layout, .. } => *layout,
            _ => None,
        }
    }

    /// 文档里的短名（`zela.Vec2` → `Vec2`）。
    pub fn render(&self) -> String {
        render_type(self, &[])
    }
}

/// 结构体字段（名字 + 类型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub ty: &'static TypeSpec,
}

/// 枚举变体（名字 + 字段；无字段即 unit 变体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variant {
    pub name: &'static str,
    pub fields: &'static [Field],
}

/// 类型变量的 **kind**（L2 的封闭小集合）。
///
/// 数值 / 向量 / 布尔 / 字节块。第三方扩展走单态重载列表兜底（U4：不开放注册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 数值（F32 / F64）。
    Num,
    /// 向量（`zela.Vec2` / `zela.Vec3`）。
    Vector,
    /// 布尔。
    Bool,
    /// 字节块 + 命名类型（`zela.Path` / 第三方私有类型）。
    ///
    /// 内核不知道路径是什么：`zela.Path` 只是类型注册表里的一个名字。
    Bytes,
}

impl Kind {
    pub const fn name(self) -> &'static str {
        match self {
            Kind::Num => "Num",
            Kind::Vector => "Vector",
            Kind::Bool => "Bool",
            Kind::Bytes => "Bytes",
        }
    }

    /// 这个 kind 允许的具体类型（**按需单态化的候选**）。
    ///
    /// 返回 `&'static` 是为了让实例表能进编译期常量；空表表示"由注册表决定"
    /// （如 `Bytes`：内核不知道有哪些私有类型）。
    pub const fn concrete(self) -> &'static [TypeSpec] {
        match self {
            Kind::Num => NUM_CONCRETE,
            Kind::Vector => VECTOR_CONCRETE,
            Kind::Bool => BOOL_CONCRETE,
            Kind::Bytes => &[],
        }
    }
}

/// `Num` 的具体类型（`F32` / `F64`）。
static NUM_CONCRETE: &[TypeSpec] = &[
    TypeSpec::Scalar(ScalarKind::F32),
    TypeSpec::Scalar(ScalarKind::F64),
];

/// `Vector` 的具体类型（`Vec2` / `Vec3`）—— 向量实例由 lift 生成，不进算子表。
static VECTOR_CONCRETE: &[TypeSpec] = &[*VEC2_SPEC, *VEC3_SPEC];

static BOOL_CONCRETE: &[TypeSpec] = &[TypeSpec::Scalar(ScalarKind::Bool)];

/// **算子签名**（L2：rank-1 + kind + 重载列表中的一条）。
///
/// `params` / `ret` 里的 [`TypeSpec::Var`] 按名字引用 `vars` 中的 kind；
/// 合一逐条尝试，多条成功 ⇒ **显式歧义错误**（硬约束 4）。
#[derive(Clone, Copy, Debug)]
pub struct OpSig {
    pub params: &'static [&'static TypeSpec],
    pub ret: &'static TypeSpec,
    /// 变量名 → kind。
    pub vars: &'static [(&'static str, Kind)],
    /// 变参：`params` 只写元素类型（`sum` 的任意多项求和）。
    pub variadic: bool,
}

impl OpSig {
    /// 人类可读签名：`(Num, Vector) -> Vector`（文档与诊断共用）。
    pub fn render(&self) -> String {
        let mut params: Vec<String> = self
            .params
            .iter()
            .map(|ty| render_type(ty, self.vars))
            .collect();
        if self.variadic {
            params.push("...".to_string());
        }
        format!("({}) -> {}", params.join(", "), render_type(self.ret, self.vars))
    }

    /// 返回类型的候选（`Var` 按 kind 展开；语言侧再把 kind 映射成具体类型）。
    ///
    /// 存根生成器用它把 `mul` 的返回类型写成
    /// `Expr<number> | Expr<Vec2> | Expr<Vec3>`，而不是一个笼统的 `Handle`。
    pub fn ret_types(&self) -> Vec<RetType> {
        match self.ret {
            TypeSpec::Var(name) => self
                .vars
                .iter()
                .find(|(candidate, _)| *candidate == *name)
                .map(|(_, kind)| vec![RetType::Kind(*kind)])
                .unwrap_or_default(),
            concrete => vec![RetType::Concrete(concrete)],
        }
    }
}

/// 一条签名的返回类型候选（见 [`OpSig::ret_types`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetType {
    /// 具体类型。
    Concrete(&'static TypeSpec),
    /// 类型变量 → 由 kind 决定（`Vector` 在存根里展开为 `Vec2 | Vec3`）。
    Kind(Kind),
}

/// 算子出口能力（代码生成 / 仅宿主 / 需运行时）。
///
/// **单一定义**：它随 `OpDef` 一起住在 `ze-type`，`ze-func` 不再有第二份。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// 可代码生成（未来：WGSL / C）。
    Codegen,
    /// 只能由宿主实现。
    HostOnly,
    /// 需要运行时支持（如查表 / 环境）。
    Runtime,
}

impl Target {
    pub const fn name(self) -> &'static str {
        match self {
            Target::Codegen => "codegen",
            Target::HostOnly => "host_only",
            Target::Runtime => "runtime",
        }
    }
}

/// 一个算子的**完整**描述：签名 + 语义元数据（计划 §0.1 的合并）。
///
/// 合并前有两张 `OpDef`：`ze-type` 持 `spec/sigs`，`ze-func` 持
/// `pointwise/targets`，两边靠"名字相同"隐式对应 —— 任何一边漏登记另一边都不会报错。
/// 现在只有一个结构、一张表。
#[derive(Clone, Copy, Debug)]
pub struct OpDef {
    pub name: &'static str,
    /// 构造器 PodType 的**完整名**（`zela.math.Sin`）。
    ///
    /// 存根生成器用它把签名挂到对应的 spec 类型上（`Sin(...) -> Expr<number>`）；
    /// JSON 也带上它，脚本侧不必按名字猜。
    pub spec: &'static str,
    pub sigs: &'static [OpSig],
    /// 引入此签名的算子库 revision（第 4 批：函数级版本）。
    pub since: u32,
    /// 语义最后一次变更的 revision（**签名不变但实现变了也要 bump**）。
    pub changed: u32,
    /// 连续层声明：逐点求值（`f: ℝⁿ → V`）。
    pub pointwise: bool,
    /// 出口能力（占位）。
    pub targets: &'static [Target],
}

impl OpDef {
    /// 文档行：`Mul: (Vector, Num) -> Vector`。
    ///
    /// 用**构造器名**（首字母大写）而不是函数名：脚本侧写的是 `Mul { … }`。
    pub fn render_lines(&self) -> Vec<String> {
        let mut chars = self.name.chars();
        let display = match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => String::new(),
        };
        self.sigs
            .iter()
            .map(|sig| format!("{display}: {}", sig.render()))
            .collect()
    }
}

/// 类型在文档里的短名（`zela.Vec2` → `Vec2`，变量 → kind）。
pub fn render_type(spec: &TypeSpec, vars: &[(&'static str, Kind)]) -> String {
    match spec {
        TypeSpec::Scalar(kind) => match kind {
            ScalarKind::F32 => "F32".to_string(),
            ScalarKind::F64 => "F64".to_string(),
            ScalarKind::I64 => "I64".to_string(),
            ScalarKind::Bool => "Bool".to_string(),
            ScalarKind::String => "String".to_string(),
            other => format!("{other:?}"),
        },
        TypeSpec::Struct { name, .. } | TypeSpec::Enum { name, .. } | TypeSpec::Opaque(name) => {
            short(name)
        }
        TypeSpec::Array { elem, len } => match len {
            Some(len) => format!("Array<{}, {len}>", render_type(elem, vars)),
            None => format!("Array<{}>", render_type(elem, vars)),
        },
        TypeSpec::Var(name) => vars
            .iter()
            .find(|(candidate, _)| *candidate == *name)
            .map(|(_, kind)| kind.name().to_string())
            .unwrap_or_else(|| (*name).to_string()),
        TypeSpec::App { ty, args } => {
            let args: Vec<String> = args.iter().map(|arg| render_type(arg, vars)).collect();
            if args.is_empty() {
                short(ty)
            } else {
                format!("{}<{}>", short(ty), args.join(", "))
            }
        }
    }
}

/// 去掉 `zela.` 前缀的短名。
fn short(name: &'static str) -> String {
    name.strip_prefix("zela.").unwrap_or(name).to_string()
}

/// `()` 的 schema：**空结构体**（不额外引入 `Unit` 变体，保持五变体最小集）。
pub const UNIT_SPEC: &TypeSpec = &TypeSpec::Struct {
    name: "()",
    layout: Some(Layout::new(1, 1, &[])),
    fields: &[],
};

/// `zela.Vec2` 的 schema（布局内联：零拷贝协商的直接输入）。
pub const VEC2_SPEC: &TypeSpec = &TypeSpec::Struct {
    name: "zela.Vec2",
    layout: Some(Layout::new(8, 4, crate::layout::VEC2_FIELDS)),
    fields: &[],
};

/// `zela.Vec3` 的 schema。
pub const VEC3_SPEC: &TypeSpec = &TypeSpec::Struct {
    name: "zela.Vec3",
    layout: Some(Layout::new(12, 4, crate::layout::VEC3_FIELDS)),
    fields: &[],
};

/// `ze_pod_core::Handle` 的 schema。
pub const HANDLE_SPEC: &TypeSpec = &TypeSpec::Opaque("ze_pod_core.Handle");

/// 一个形参：**名字** + 类型。
///
/// 形参名进 schema 是硬要求：否则 LuaLS 只能生成 `p1..pN`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamSpec {
    pub name: &'static str,
    pub ty: &'static TypeSpec,
}

/// 一条函数的 I/O 描述（与 `FnSig` 按序对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FnIo {
    pub params: &'static [ParamSpec],
    pub ret: &'static TypeSpec,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{FieldLayout, FieldTy};

    const EXPR_F32: TypeSpec = TypeSpec::App {
        ty: "zela.Expr",
        args: &[TypeSpec::Scalar(ScalarKind::F32)],
    };

    const POINT_FIELDS: &[Field] = &[
        Field {
            name: "x",
            ty: &TypeSpec::Scalar(ScalarKind::F32),
        },
        Field {
            name: "y",
            ty: &TypeSpec::Scalar(ScalarKind::F32),
        },
    ];
    const POINT_LAYOUT: Layout = Layout::new(
        8,
        4,
        &[
            FieldLayout::new("x", 0, FieldTy::F32),
            FieldLayout::new("y", 4, FieldTy::F32),
        ],
    );

    #[test]
    fn app_reports_its_constructor_name() {
        assert_eq!(EXPR_F32.type_name(), Some("zela.Expr"));
    }

    #[test]
    fn type_variables_have_no_named_type() {
        assert_eq!(TypeSpec::Var("T").type_name(), None);
    }

    #[test]
    fn struct_keeps_its_declared_layout() {
        let point = TypeSpec::Struct {
            name: "acme.Point",
            layout: Some(POINT_LAYOUT),
            fields: POINT_FIELDS,
        };
        assert_eq!(point.declared_layout(), Some(POINT_LAYOUT));
        assert_eq!(TypeSpec::Opaque("x").declared_layout(), None);
    }
}
