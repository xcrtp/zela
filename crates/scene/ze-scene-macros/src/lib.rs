//! 场景层的 proc-macro：`#[derive(SceneComponent)]`。
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
#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, Attribute, DeriveInput, Ident, LitStr};

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

#[cfg(test)]
mod tests {
    use super::snake_case;

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
}
