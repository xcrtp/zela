//! 场景层的 proc-macro：`#[derive(SceneComponent)]` 与 `#[derive(Channels)]`。
//!
//! # `#[derive(SceneComponent)]`
//!
//! 组件类型（`ze-scene-api::types`）在 `#[derive(PodType)]` 之外再派生本宏：
//!
//! ```ignore
//! #[derive(Clone, Copy, Debug, PartialEq, PodType, SceneComponent)]
//! #[pod_type(name = "zela.Circle")]
//! pub struct Circle { pub x: f32, pub y: f32, pub r: f32 }
//! ```
//!
//! 生成两个**同源**常量（消除 `#[pod_type]` 与 `register_fn1_named` 的手写字符串漂移）：
//!
//! - `POD_TYPE_NAME`：读 `#[pod_type(name = "...")]`（缺省 = ident），如 `"zela.Circle"`；
//! - `CTOR_NAME`：ident 的 snake_case 推导（**仅「小写字母直接后跟大写字母」插 `_`**），
//!   如 `Circle → "circle"`、`Camera2D → "camera2d"`、`RevealPath → "reveal_path"`。
//!
//! 生成物引用 `::ze_scene_api::SceneComponent`（本宏当前只在 `ze-scene-api` 内使用）。
//!
//! # `#[derive(Channels)]`
//!
//! 场景组件结构体在既有 derive 之外再派生本宏，把可动画字段**显式注册**为通道
//! （F 批：字段全量注册）。用法：
//!
//! ```ignore
//! #[derive(Describable, Channels)]
//! #[channel(name = "CircleRadius", radius: f32)]  // name = 持久化名覆盖（缺省 = 规范地址）
//! #[channel(center: Vec2)]                        // 缺省 name = "Circle.center"
//! pub struct Circle(pub Circle2D);
//! ```
//!
//! 每个 `#[channel(...)]` 声明一个字段通道：`name = "..."`（可选，若有必须最前）
//! + `字段名: 类型`。类型只支持 `f32` / `Vec2` / `Vec3` / `bool`（其余编译期报错）。
//!
//! 生成物（**全部 fn 项**，保证 const 可构造；**全部全限定路径**，因为宏产出的
//! 代码被用到 `ze-scene-core` 内部各模块）：
//!
//! - `pub const {STRUCT}_{FIELD}: FieldSpec`（`{STRUCT}` = ident 的 SCREAMING_SNAKE）；
//! - `pub fn {struct}_{field}_write` / `{struct}_{field}_read`（write/read 与
//!   `ze-scene-core::animation::target` 的手写规格同语义）；
//! - `pub fn make_{struct}_{field}`：构造 `Arc<dyn Target>`；
//! - `pub const {STRUCT}_CHANNELS: [TargetEntry; N]`：每通道一项（target.rs 的
//!   `TARGETS` 用 `..{STRUCT}_CHANNELS` 展开合并）。
#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, spanned::Spanned, Attribute, DeriveInput, Ident, LitStr, Token, Type};

/// 从组件类型声明生成 `SceneComponent` 常量（`POD_TYPE_NAME` / `CTOR_NAME`）。
#[proc_macro_derive(SceneComponent, attributes(pod_type))]
pub fn derive_scene_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.ident.span(),
            "SceneComponent 不支持泛型参数（场景无消费方）",
        ));
    }
    let name = &input.ident;
    let pod_type_name = pod_type_name(&input.attrs, name)?;
    let ctor_name = snake_case(&name.to_string());

    Ok(quote! {
        impl ::ze_scene_api::SceneComponent for #name {
            const POD_TYPE_NAME: &'static str = #pod_type_name;
            const CTOR_NAME: &'static str = #ctor_name;
        }
    })
}

/// 读 `#[pod_type(name = "...")]`（与 `ze-pod-macro` 的 `pod_type_name` 同规则；缺省 = ident）。
fn pod_type_name(attrs: &[Attribute], ident: &Ident) -> syn::Result<LitStr> {
    for attr in attrs {
        if !attr.path().is_ident("pod_type") {
            continue;
        }
        let mut found = None;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                let lit: LitStr = meta.value()?.parse()?;
                found = Some(lit);
                Ok(())
            } else {
                Err(meta.error("PodType 的 `#[pod_type(...)]` 只支持 name = \"...\""))
            }
        })?;
        if let Some(lit) = found {
            return Ok(lit);
        }
    }
    Ok(LitStr::new(&ident.to_string(), ident.span()))
}

/// ident → snake_case。规则：**仅「小写字母直接后跟大写字母」插 `_`**，随后全部转小写。
///
/// `Camera2D → camera2d`（`2` 不是小写字母，不插）；`RevealPath → reveal_path`（`l→P` 插）。
fn snake_case(ident: &str) -> String {
    let mut out = String::with_capacity(ident.len() + 4);
    let mut prev_is_lower = false;
    for ch in ident.chars() {
        if prev_is_lower && ch.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(ch.to_ascii_lowercase());
        prev_is_lower = ch.is_ascii_lowercase();
    }
    out
}

// ─── `#[derive(Channels)]`：字段通道显式注册（F 批）───────────────────────

/// `#[channel(...)]` 的一个字段通道：`name = "..."`（可选，若有必须最前）+ `字段: 类型`。
///
/// 为什么手工 `Parse`（不用 `parse_nested_meta`）：`radius: f32` 不是合法 Meta 语法。
struct ChannelField {
    /// Rust 字段名（如 `radius`）。
    field: Ident,
    /// 字段类型（只支持 `f32` / `Vec2` / `Vec3` / `bool`）。
    ty: Type,
    /// 持久化名覆盖（`name = "..."`；缺省 = 规范地址 `Component.field`）。
    name: Option<LitStr>,
}

impl syn::parse::Parse for ChannelField {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let name = if input.peek(Ident) && input.peek2(Token![=]) {
            let kw: Ident = input.parse()?;
            if kw != "name" {
                return Err(syn::Error::new(
                    kw.span(),
                    "channel 属性只支持 `name = \"...\"` 或 `字段名: 类型`",
                ));
            }
            input.parse::<Token![=]>()?;
            let name: LitStr = input.parse()?;
            input.parse::<Token![,]>()?;
            Some(name)
        } else {
            None
        };
        let field: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty: Type = input.parse()?;
        Ok(Self { field, ty, name })
    }
}

/// 字段类型 → 通道的 TypeSpec 种类（**封闭**：只支持这四种）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MappedKind {
    F32,
    Bool,
    Vec2,
    Vec3,
}

/// 按类型路径**最后一段**匹配（`Vec2` / `ze_notion::Vec2` 都算）。
fn map_type(ty: &Type) -> syn::Result<MappedKind> {
    let last = match ty {
        Type::Path(p) => p
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    };
    match last.as_str() {
        "f32" => Ok(MappedKind::F32),
        "bool" => Ok(MappedKind::Bool),
        "Vec2" => Ok(MappedKind::Vec2),
        "Vec3" => Ok(MappedKind::Vec3),
        other => Err(syn::Error::new(
            ty.span(),
            format!("字段通道只支持 f32/Vec2/Vec3/bool，收到 `{other}`"),
        )),
    }
}

/// 从组件类型声明生成 `FieldSpec` / 通道表 / make 构造器（见文件头 `#[derive(Channels)]`）。
#[proc_macro_derive(Channels, attributes(channel))]
pub fn derive_channels(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_channels(&input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand_channels(input: &DeriveInput) -> syn::Result<TokenStream2> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.ident.span(),
            "Channels 不支持泛型参数（场景无消费方）",
        ));
    }
    let struct_ident = &input.ident;
    let struct_snake = snake_case(&struct_ident.to_string());
    let struct_scream = struct_snake.to_ascii_uppercase();
    let component = struct_ident.to_string();
    let span = struct_ident.span();

    let mut channels = Vec::new();
    for attr in &input.attrs {
        if attr.path().is_ident("channel") {
            channels.push(attr.parse_args::<ChannelField>()?);
        }
    }

    let mut items = Vec::new();
    let mut entries = Vec::new();
    for cf in &channels {
        let field = &cf.field;
        let field_str = field.to_string();
        let field_scream = field_str.to_ascii_uppercase();
        let const_name = Ident::new(&format!("{struct_scream}_{field_scream}"), span);
        let write_fn = Ident::new(&format!("{struct_snake}_{field_str}_write"), span);
        let read_fn = Ident::new(&format!("{struct_snake}_{field_str}_read"), span);
        let make_fn = Ident::new(&format!("make_{struct_snake}_{field_str}"), span);
        let address = format!("{component}.{field_str}");
        // 持久化名：`name = "..."` 覆盖，缺省 = 规范地址。
        let name = cf.name.as_ref().map(|l| l.value()).unwrap_or_else(|| address.clone());

        let (ty, write_body, read_body) = match map_type(&cf.ty)? {
            MappedKind::F32 => (
                quote! { ::ze_pod_core::TypeSpec::Scalar(::ze_pod_core::ScalarKind::F32) },
                quote! {
                    let value = value.try_scalar().ok_or_else(|| {
                        ::ze_scene_core::animation::target::TargetError::mismatch(#name, "scalar", value)
                    })?;
                    ::ze_scene_core::scene_write::with_component::<#struct_ident, _>(scene, id, |c| {
                        c.#field = value
                    });
                    Ok(())
                },
                quote! {
                    ::ze_scene_core::scene_write::read_component::<#struct_ident, _>(scene, id, |c| {
                        ::ze_time::AnimValue::Scalar(c.#field)
                    })
                },
            ),
            MappedKind::Vec2 => (
                quote! {
                    ::ze_pod_core::TypeSpec::Struct {
                        name: "zela.Vec2",
                        layout: ::core::option::Option::Some(::ze_pod_core::Layout::new(
                            8,
                            4,
                            ::ze_pod_core::VEC2_FIELDS,
                        )),
                        fields: &[],
                    }
                },
                quote! {
                    let value = value.try_vec2().ok_or_else(|| {
                        ::ze_scene_core::animation::target::TargetError::mismatch(#name, "vec2", value)
                    })?;
                    ::ze_scene_core::scene_write::with_component::<#struct_ident, _>(scene, id, |c| {
                        c.#field = value
                    });
                    Ok(())
                },
                quote! {
                    ::ze_scene_core::scene_write::read_component::<#struct_ident, _>(scene, id, |c| {
                        ::ze_time::AnimValue::Vec2(c.#field)
                    })
                },
            ),
            MappedKind::Vec3 => (
                quote! {
                    ::ze_pod_core::TypeSpec::Struct {
                        name: "zela.Vec3",
                        layout: ::core::option::Option::Some(::ze_pod_core::Layout::new(
                            12,
                            4,
                            ::ze_pod_core::VEC3_FIELDS,
                        )),
                        fields: &[],
                    }
                },
                quote! {
                    let value = value.try_vec3().ok_or_else(|| {
                        ::ze_scene_core::animation::target::TargetError::mismatch(#name, "vec3", value)
                    })?;
                    ::ze_scene_core::scene_write::with_component::<#struct_ident, _>(scene, id, |c| {
                        c.#field = value
                    });
                    Ok(())
                },
                quote! {
                    ::ze_scene_core::scene_write::read_component::<#struct_ident, _>(scene, id, |c| {
                        ::ze_time::AnimValue::Vec3(c.#field)
                    })
                },
            ),
            MappedKind::Bool => (
                quote! { ::ze_pod_core::TypeSpec::Scalar(::ze_pod_core::ScalarKind::Bool) },
                quote! {
                    let value = value.try_scalar().ok_or_else(|| {
                        ::ze_scene_core::animation::target::TargetError::mismatch(#name, "scalar", value)
                    })?;
                    ::ze_scene_core::scene_write::with_component::<#struct_ident, _>(scene, id, |c| {
                        c.#field = value != 0.0
                    });
                    Ok(())
                },
                quote! {
                    ::ze_scene_core::scene_write::read_component::<#struct_ident, _>(scene, id, |c| {
                        ::ze_time::AnimValue::Scalar(if c.#field { 1.0 } else { 0.0 })
                    })
                },
            ),
        };

        items.push(quote! {
            pub const #const_name: ::ze_scene_core::animation::target::FieldSpec =
                ::ze_scene_core::animation::target::FieldSpec {
                    name: #name,
                    address: #address,
                    component: #component,
                    field: #field_str,
                    ty: &#ty,
                    is_composite: false,
                    write: #write_fn,
                    read: #read_fn,
                };

            pub fn #write_fn(
                scene: &mut dyn ::ze_scene_core::scene_write::SceneWrite,
                id: ::ze_scene_contract::ObjectId,
                value: &::ze_time::AnimValue,
            ) -> Result<(), ::ze_scene_core::animation::target::TargetError> {
                #write_body
            }

            pub fn #read_fn(
                scene: &dyn ::ze_scene_core::scene_write::SceneRead,
                id: ::ze_scene_contract::ObjectId,
            ) -> Option<::ze_time::AnimValue> {
                #read_body
            }

            pub fn #make_fn(
                id: ::ze_scene_contract::ObjectId,
            ) -> std::sync::Arc<dyn ::ze_scene_core::animation::target::Target> {
                std::sync::Arc::new(::ze_scene_core::animation::target::FieldTarget {
                    id,
                    spec: &#const_name,
                })
            }
        });
        entries.push(quote! {
            ::ze_scene_core::animation::target::TargetEntry {
                spec: &#const_name,
                make: #make_fn,
            }
        });
    }

    let n = entries.len();
    let channels_const = Ident::new(&format!("{struct_scream}_CHANNELS"), span);
    Ok(quote! {
        #(#items)*
        pub const #channels_const: [::ze_scene_core::animation::target::TargetEntry; #n] = [
            #(#entries),*
        ];
    })
}

#[cfg(test)]
mod tests {
    use super::{ChannelField, MappedKind, map_type, snake_case};

    #[test]
    fn plain_ident_is_lowercased() {
        assert_eq!(snake_case("Circle"), "circle");
        assert_eq!(snake_case("Stroke"), "stroke");
        assert_eq!(snake_case("Fill"), "fill");
    }

    #[test]
    fn digit_before_uppercase_does_not_insert_underscore() {
        assert_eq!(snake_case("Camera2D"), "camera2d");
        assert_eq!(snake_case("Transform3D"), "transform3d");
        assert_eq!(snake_case("Axes2D"), "axes2d");
    }

    #[test]
    fn lower_to_upper_inserts_underscore() {
        assert_eq!(snake_case("RevealPath"), "reveal_path");
        assert_eq!(snake_case("PhaseAnim"), "phase_anim");
        assert_eq!(snake_case("SecondPaint"), "second_paint");
        assert_eq!(snake_case("BackgroundStroke"), "background_stroke");
        assert_eq!(snake_case("CurveWindow"), "curve_window");
        assert_eq!(snake_case("PathShape"), "path_shape");
    }

    /// `#[channel(...)]` 解析：`name = "..."` 覆盖（必须最前）+ 字段: 类型。
    #[test]
    fn channel_attr_parses_name_override() {
        let f: ChannelField = syn::parse_str(r#"name = "CircleRadius", radius: f32"#).unwrap();
        assert_eq!(f.field.to_string(), "radius");
        assert_eq!(f.name.as_ref().unwrap().value(), "CircleRadius");
    }

    /// `#[channel(...)]` 解析：无 `name` 时缺省为 `None`（展开时 = 规范地址）。
    #[test]
    fn channel_attr_parses_without_name() {
        let f: ChannelField = syn::parse_str("center: Vec2").unwrap();
        assert_eq!(f.field.to_string(), "center");
        assert!(f.name.is_none(), "无 name 覆盖 ⇒ name 应为 None");
        assert!(matches!(map_type(&f.ty).unwrap(), MappedKind::Vec2));
    }

    /// 类型映射：f32/Vec2/Vec3/bool 通过；非法类型编译期报错。
    #[test]
    fn type_map_known_and_unknown() {
        for (src, kind) in [
            ("radius: f32", MappedKind::F32),
            ("flag: bool", MappedKind::Bool),
            ("center: Vec2", MappedKind::Vec2),
            ("eye: Vec3", MappedKind::Vec3),
        ] {
            let f: ChannelField = syn::parse_str(src).unwrap();
            assert_eq!(map_type(&f.ty).unwrap(), kind, "{src}");
        }
        let f: ChannelField = syn::parse_str("color: Color32").unwrap();
        let err = map_type(&f.ty).unwrap_err();
        assert!(
            err.to_string().contains("字段通道只支持 f32/Vec2/Vec3/bool"),
            "非法类型应报编译期错误：{err}"
        );
    }
}
