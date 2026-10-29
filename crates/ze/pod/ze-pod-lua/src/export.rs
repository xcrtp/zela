// export.rs — **语言侧文档生成**：JSON 由通用层产出，LuaLS 注解在这里生成。
//
// 本文件不引用任何 domain 类型：能力清单由调用方（App / domain crate）传入。

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ze_pod_core::{Field, Kind, RetType, ScalarKind, TypeSpec};
use ze_pod_binding::{
    CapabilityDocs, has_ops, op_by_name, op_by_spec, short_name,
};

/// Lua 侧类型名（LuaLS 注解用）。
pub fn lua_type_name(spec: &'static TypeSpec) -> &'static str {
    match spec {
        TypeSpec::Scalar(kind) => match kind {
            ScalarKind::F32 | ScalarKind::F64 => "number",
            ScalarKind::I32 | ScalarKind::I64 | ScalarKind::U32 | ScalarKind::U64 | ScalarKind::U8 => "integer",
            ScalarKind::Bool => "boolean",
            ScalarKind::String => "string",
        },
        // 通道常量的值形状：`Channel<T>` 的 T 由调用方（组件字段）给出；
        // `Bind.target` 不限定值类型 ⇒ `Channel<any>`。
        TypeSpec::Struct { name, .. } if *name == "zela.math.Channel" => "Channel<any>",
        TypeSpec::Struct { name, .. } | TypeSpec::Enum { name, .. } => short_name(name),
        TypeSpec::Array { .. } => "table",
        TypeSpec::Opaque(_) => "Handle",
        // Lua 没有泛型：`Expr<f32>` 只暴露构造器名；类型变量退化为 `any`。
        TypeSpec::App { ty, .. } => short_name(ty),
        TypeSpec::Var(_) => "any",
    }
}

fn push_description(out: &mut String, text: &str) {
    for line in text.lines() {
        out.push_str("---");
        out.push_str(line);
        out.push('\n');
    }
}

/// 从 `CapabilityDocs` 生成 LuaLS 注解（**无 domain 知识**）。
pub fn lua_ls(capabilities: &[CapabilityDocs]) -> String {
    let types = ze_pod_binding::reachable_types(capabilities);
    let typed_expr = has_ops(capabilities);
    // 组件短名 → 通道常量（`Circle.channel.radius`），收在构造器的 `channel` 字段下。
    // 常量因此能作为字段声明出来（否则 `Circle.channel.radius` 会被诊断成 undefined-field）。
    let mut channels: BTreeMap<&'static str, Vec<&'static ze_pod_core::ChannelDecl>> =
        BTreeMap::new();
    for capability in capabilities {
        for channel in capability.channels {
            channels.entry(channel.component).or_default().push(channel);
        }
    }
    // 组件短名 → 方法（`Reveal.current_pos(obj)`）：直接挂成字段函数。
    let mut methods: BTreeMap<&'static str, Vec<&'static ze_pod_core::MethodDecl>> =
        BTreeMap::new();
    for capability in capabilities {
        for method in capability.methods {
            methods.entry(method.component).or_default().push(method);
        }
    }
    // `zela.math.Channel` 结构体会被类型循环渲染成 `Channel<T>`；
    // 只有它缺席时才需要一个独立的泛型类兜底（避免两个同名 `@class`）。
    let has_channel_type = types.contains_key("zela.math.Channel");

    let mut out =
        String::from("---@meta\n\n---@class Handle zela 场景句柄（组件 / 对象 / 曲线 / 动作）\n\n");
    if typed_expr {
        // **类型能力（B/C 批）**：`Expr<T>` 是**纯静态**的表达式句柄 —— 运行期仍是
        // `Handle`，但编辑器可据此拒绝 `And { a = Sin { … } }` 这类组合。
        out.push_str(
            "---@class Expr<T>: Handle 表达式句柄（T 是装配期推导的静态类型；运行期仍是 Handle）\n\n",
        );
    }
    if !channels.is_empty() && !has_channel_type {
        // 通道携带值类型（A 批）：`Circle.channel.radius` 是 `Channel<number>`。
        // 只作绑定目标，脚本读不到值 —— 泛型参数只用于编辑器提示与未来类型检查。
        out.push_str("---@class Channel<T> 组件字段通道（只能作绑定目标）\n\n");
    }

    // ── 类型（一次）────────────────────────────────────────────────────
    for (&full_name, &spec) in &types {
        let lua_name = short_name(full_name);
        match spec {
            TypeSpec::Struct { name, fields, .. } => {
                let own_channels = channels.get(lua_name).map(Vec::as_slice).unwrap_or(&[]);
                let own_methods = methods.get(lua_name).map(Vec::as_slice).unwrap_or(&[]);
                // `zela.math.Channel` 是 `Channel<T>` 的值形状（namespace + name）：
                // 与组件通道常量共用同一个泛型类，避免两个同名 `@class`。
                let is_channel = full_name == "zela.math.Channel";
                if is_channel {
                    out.push_str("---@class Channel<T> 组件字段通道（只能作绑定目标）\n");
                } else {
                    out.push_str(&format!("---@class {lua_name}\n"));
                }
                for (index, field) in fields.iter().enumerate() {
                    // 提案 #6 C：默认值进声明文件（`Color.a = 255`），但不进产物。
                    // **有默认值 ⇒ 字段可选**：LuaLS 的 `@field` 不带 `?` 就是必填，
                    // 少了 `?` 会把 `expr.bind{ … }`（省略 duration 等）误报成"缺字段"。
                    let default = ze_pod_binding::field_default_literal(name, field.name);
                    let optional = if default.is_some() { "?" } else { "" };
                    let default = default
                        .map(|value| format!(" 默认 {value}"))
                        .unwrap_or_default();
                    // 阶段 5：算子 spec 的字段也按签名展开（与 `---@overload` 同一份口径）。
                    let ty = lua_op_param(capabilities, full_name, index)
                        .unwrap_or_else(|| lua_field_type(full_name, field, typed_expr));
                    out.push_str(&format!(
                        "---@field {}{} {}{}\n",
                        field.name, optional, ty, default
                    ));
                }
                // 通道常量收在 `X.channel.*` 下（与组件字段不撞名）。
                // 方法（`Reveal.current_pos(obj)`）直接挂成字段函数。
                if !own_channels.is_empty() || !own_methods.is_empty() {
                    for method in own_methods {
                        out.push_str(&format!(
                            "---@field {} {}\n",
                            method.name,
                            lua_method_type(method)
                        ));
                    }
                    if !own_channels.is_empty() {
                        out.push_str(&format!("---@field channel {lua_name}Channel\n"));
                    }
                    // ⚠️ 类的全局必须紧跟自己的 `---@class` 块：LuaLS 把**最后一个**
                    // 紧邻的 doc 块绑给赋值语句；若先写 `XChannel` 再写 `X = {}`，
                    // `X` 会被当成 `XChannel`，于是 `X.channel` 诊断成 unknown。
                    out.push_str(&format!("{lua_name} = {{}}\n\n"));
                    if !own_channels.is_empty() {
                        out.push_str(&format!("---@class {lua_name}Channel\n"));
                        for channel in own_channels {
                            out.push_str(&format!(
                                "---@field {} Channel<{}>\n",
                                channel.name,
                                lua_type_name(channel.ty)
                            ));
                        }
                        out.push('\n');
                    }
                    continue;
                }
                out.push('\n');
                // 类型能力：算子构造器返回**带静态类型的表达式句柄**（B/C 批的签名表）。
                let returns = if let Some(op) = op_by_spec(capabilities, full_name) {
                    lua_expr_returns(op)
                } else if is_channel {
                    "Channel<any>".to_string()
                } else if let Some(ctor) = ze_pod_binding::component_ctor_of(capabilities, full_name) {
                    // 非算子构造器（`Exists { … }` 这类）也按同一张表给返回值：
                    // 否则 `Predicate.Of { expr = Exists { … } }` 在编辑器里是类型错。
                    lua_call_return(capabilities, &ctor).unwrap_or_else(|| "Handle".to_string())
                } else {
                    lua_name.to_string()
                };
                let spec_type = if is_channel { "Channel<any>" } else { lua_name };
                // 无通道常量的类型保持函数形态（与旧声明一致）。
                let spec_op = op_by_spec(capabilities, full_name).is_some();
                let positional: Vec<String> = fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| {
                        // 阶段 5：算子 spec 的参数按签名表展开（向量 ⇒ Vec2 | Vec3）。
                        let ty = if spec_op {
                            lua_op_param(capabilities, full_name, index).unwrap_or_else(|| {
                                lua_field_type(full_name, field, typed_expr)
                            })
                        } else {
                            lua_field_type(full_name, field, typed_expr)
                        };
                        let optional =
                            ze_pod_binding::field_default_literal(name, field.name).is_some();
                        if optional {
                            format!("{}?: {}", field.name, ty)
                        } else {
                            format!("{}: {}", field.name, ty)
                        }
                    })
                    .collect();
                out.push_str(&format!(
                    "---@overload fun({}): {returns}\n",
                    positional.join(", ")
                ));
                out.push_str(&format!(
                    "---@param spec {spec_type}\n---@return {returns}\nfunction {lua_name}(spec) end\n\n"
                ));
            }
            TypeSpec::Enum { variants, .. } => {
                out.push_str(&format!("---@class {lua_name}\n{lua_name} = {{}}\n\n"));
                for variant in *variants {
                    if variant.fields.is_empty() {
                        out.push_str(&format!(
                            "---@type {lua_name}\n{lua_name}.{} = {{}}\n\n",
                            variant.name
                        ));
                        continue;
                    }
                    out.push_str(&format!("---@class {lua_name}.{}\n", variant.name));
                    let parent = format!("{full_name}.{}", variant.name);
                    for field in variant.fields {
                        out.push_str(&format!(
                            "---@field {} {}\n",
                            field.name,
                            lua_field_type(&parent, field, typed_expr)
                        ));
                    }
                    out.push_str(&format!(
                        "---@param spec {lua_name}.{}\n---@return {lua_name}\nfunction {lua_name}.{}(spec) end\n\n",
                        variant.name, variant.name
                    ));
                }
            }
            _ => {}
        }
    }

    // ── 能力表 ────────────────────────────────────────────────────────
    for capability in capabilities {
        out.push_str(&format!(
            "---@class {}Api\n{} = {{}}\n\n",
            capitalize(capability.name),
            capability.name
        ));
        for (index, item) in capability.calls.iter().enumerate() {
            if let Some(doc) = capability.docs.get(index) {
                push_description(&mut out, doc.description);
            }
            for param in item.io.params {
                out.push_str(&format!(
                    "---@param {} {}\n",
                    param.name,
                    lua_type_name(param.ty)
                ));
            }
            if let Some(returns) = lua_call_return(capabilities, item) {
                out.push_str(&format!("---@return {returns}\n"));
            }
            let names: Vec<&str> = item.io.params.iter().map(|param| param.name).collect();
            out.push_str(&format!(
                "function {}.{}({}) end\n\n",
                capability.name,
                item.signature.name,
                names.join(", ")
            ));
        }
        // ── 算子类型签名（L2 前哨，A 批）──────────────────────────────
        // 纯注释：签名是**文档/诊断**用的数据，脚本侧没有对应的全局。
        if !capability.ops.is_empty() {
            out.push_str(&format!(
                "-- ── {} 算子类型签名（L2）────────────────────────\n",
                capability.name
            ));
            for op in capability.ops {
                for line in op.render_lines() {
                    out.push_str(&format!("-- {line}\n"));
                }
            }
            out.push('\n');
        }
        // `clock_*` 的别名表（与绑定层一致）：`clock.time()` / `clock.shift(…)` …
        let clock_calls: Vec<_> = capability
            .calls
            .iter()
            .filter(|item| item.signature.name.starts_with("clock_"))
            .collect();
        if !clock_calls.is_empty() {
            out.push_str("---@class ClockApi\nclock = {}\n\n");
            for item in clock_calls {
                let short = item
                    .signature
                    .name
                    .strip_prefix("clock_")
                    .unwrap_or(item.signature.name);
                for param in item.io.params {
                    out.push_str(&format!(
                        "---@param {} {}\n",
                        param.name,
                        lua_type_name(param.ty)
                    ));
                }
                if let Some(returns) = lua_call_return(capabilities, item) {
                    out.push_str(&format!("---@return {returns}\n"));
                }
                let names: Vec<&str> = item.io.params.iter().map(|param| param.name).collect();
                out.push_str(&format!("function clock.{short}({}) end\n\n", names.join(", ")));
            }
        }
    }
    out
}

fn capitalize(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Api".to_string(),
    }
}

/// 字段类型：`zela.math.*` 里的 `Handle` 字段是**表达式输入**，标成 `Expr<any>`；
/// `Predicate.Of.expr` 是 Bool 表达式。其余字段沿用类型名。
fn lua_field_type(parent: &str, field: &Field, typed_expr: bool) -> String {
    // `Bind.object` 是**场景对象**句柄，不是表达式输入。
    if typed_expr
        && parent.starts_with("zela.math.")
        && !(parent == "zela.math.Bind" && field.name == "object")
    {
        if parent == "zela.math.Predicate.Of" && field.name == "expr" {
            return "Expr<boolean>".to_string();
        }
        if matches!(*field.ty, TypeSpec::Opaque(_)) {
            return "Expr<any>".to_string();
        }
    }
    lua_type_name(field.ty).to_string()
}

/// **参数侧泛型（阶段 5，方案 a）**：算子 spec 的第 `index` 个字段按签名表展开。
///
/// LuaLS 没有类型变量 ⇒ `add(Vec2,Vec2)→Vec2` 与 `add(Vec3,Vec3)→Vec3` 在签名表里
/// 是同一条，编辑器却需要具体类型。这里把 `Kind::Vector` 展开成 `Vec2` / `Vec3`
/// 两个**候选**，参数不再退化成 `Expr<any>`（= 隐式接受任意表达式）。
///
/// 返回 `None` ⇒ 不是算子 spec（`Bind` / `Select` 等），调用方沿用 `lua_field_type`。
fn lua_op_param(capabilities: &[CapabilityDocs], spec: &str, index: usize) -> Option<String> {
    let op = op_by_spec(capabilities, spec)?;
    let mut names: Vec<&'static str> = Vec::new();
    for signature in op.sigs {
        let candidate: &[&'static str] = match signature.params.get(index).copied()? {
            TypeSpec::Scalar(ScalarKind::F64) | TypeSpec::Scalar(ScalarKind::F32) => &["number"],
            TypeSpec::Scalar(ScalarKind::Bool) => &["boolean"],
            TypeSpec::Struct { name, .. } if *name == "zela.Vec2" => &["Vec2"],
            TypeSpec::Struct { name, .. } if *name == "zela.Vec3" => &["Vec3"],
            TypeSpec::Var(var) => {
                let (_, kind) = signature.vars.iter().find(|(name, _)| name == var)?;
                match kind {
                    Kind::Num => &["number"],
                    // 方案 a：只展开向量变量（覆盖绝大多数场景）。
                    Kind::Vector => &["Vec2", "Vec3"],
                    _ => return None,
                }
            }
            _ => return None,
        };
        for name in candidate {
            if !names.contains(name) {
                names.push(name);
            }
        }
    }
    if names.is_empty() {
        return None;
    }
    Some(
        names
            .into_iter()
            .map(|name| format!("Expr<{name}>"))
            .collect::<Vec<_>>()
            .join("|"),
    )
}

/// 一条能力函数的返回类型：算子按签名表给 `Expr<…>`，其余按 `TypeSpec`。
fn lua_call_return(
    capabilities: &[CapabilityDocs],
    item: &ze_pod_core::ErasedFn,
) -> Option<String> {
    if matches!(item.io.ret, TypeSpec::Struct { name: "()", .. }) {
        return None;
    }
    if let Some(op) = op_by_name(capabilities, item.signature.name) {
        return Some(lua_expr_returns(op));
    }
    Some(match item.signature.name {
        "pure" | "frame" | "clock_time" | "clock_time_of" => "Expr<number>".to_string(),
        "exists" => "Expr<boolean>".to_string(),
        "clock_range" | "clock_range_of" => "Expr<boolean>".to_string(),
        "rec_index" => "Expr<number>".to_string(),
        "query_self" | "method" | "nat_rec" | "rec_prev" | "event" | "component_state" | "env" => "Expr<any>".to_string(),
        _ => lua_type_name(item.io.ret).to_string(),
    })
}

/// 方法常量的类型：`fun(object: Handle): Expr<Vec2>`（返回值是表达式句柄）。
///
/// 与通道常量并列：`Reveal.channel.end`（值）vs `Reveal.current_pos(obj)`（函数）。
fn lua_method_type(method: &ze_pod_core::MethodDecl) -> String {
    let mut params = vec!["object: Handle".to_string()];
    for _ in 0..method.arity {
        params.push("arg: Expr<any>".to_string());
    }
    format!(
        "fun({}): Expr<{}>",
        params.join(", "),
        lua_type_name(method.ret)
    )
}

/// 算子的返回类型候选：`Expr<number>` / `Expr<Vec2> | Expr<Vec3>` …
fn lua_expr_returns(op: &ze_pod_core::OpDef) -> String {
    let mut types: Vec<&'static str> = Vec::new();
    for sig in op.sigs {
        for ret in sig.ret_types() {
            match ret {
                RetType::Concrete(spec) => {
                    let ty = lua_type_name(spec);
                    if !types.contains(&ty) {
                        types.push(ty);
                    }
                }
                RetType::Kind(Kind::Num) => {
                    if !types.contains(&"number") {
                        types.push("number");
                    }
                }
                RetType::Kind(Kind::Bool) => {
                    if !types.contains(&"boolean") {
                        types.push("boolean");
                    }
                }
                RetType::Kind(Kind::Vector) => {
                    for ty in ["Vec2", "Vec3"] {
                        if !types.contains(&ty) {
                            types.push(ty);
                        }
                    }
                }
                RetType::Kind(Kind::Bytes) => {
                    if !types.contains(&"Bytes") {
                        types.push("Bytes");
                    }
                }
            }
        }
    }
    if types.is_empty() {
        return "Handle".to_string();
    }
    types
        .into_iter()
        .map(|ty| format!("Expr<{ty}>"))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// 把 JSON + LuaLS 写到 `dir`（返回实际写入的两个路径）。
pub fn export_docs(dir: &Path, capabilities: &[CapabilityDocs]) -> io::Result<[PathBuf; 2]> {
    fs::create_dir_all(dir)?;
    let json_path = dir.join("scene_api.json");
    let lua_path = dir.join("scene_api.lua");
    fs::write(&json_path, ze_pod_binding::capabilities_json(capabilities))?;
    fs::write(&lua_path, lua_ls(capabilities))?;
    Ok([json_path, lua_path])
}
