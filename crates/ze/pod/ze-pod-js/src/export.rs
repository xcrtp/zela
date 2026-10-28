// export.rs — **语言侧文档生成**：JSON 由通用层产出，TypeScript `.d.ts` 在这里生成。
//
// 本文件不引用任何 domain 类型：能力清单由调用方（App / domain crate）传入。
//
// # 为什么是 `.d.ts`（而不是别的）
//
// JS 侧没有 LuaLS 那种注解方言；TypeScript 声明文件是**工具链原生**的补全来源，
// 编辑器（VSCode / WebStorm）直接识别，不需要插件。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ze_pod_binding::{
    BindingPlan, CapabilityDocs, has_ops, op_by_name, op_by_spec, short_name,
};
use ze_pod_core::{Field, Kind, OpDef, RetType, ScalarKind, TypeSpec};

/// JS 侧类型名（TypeScript 声明用）。
///
/// 注意：JS **只有 number** —— `i32` / `u32` / `u64` / `u8` 都落成 `number`，
/// 整数语义由绑定层显式校验（越界报错，不静默截断）。
pub fn ts_type_name(spec: &'static TypeSpec) -> String {
    match spec {
        TypeSpec::Scalar(kind) => match kind {
            ScalarKind::F32 | ScalarKind::F64 => "number".to_string(),
            ScalarKind::I32 | ScalarKind::U32 | ScalarKind::U64 | ScalarKind::U8 => {
                "number".to_string()
            }
            ScalarKind::Bool => "boolean".to_string(),
            ScalarKind::String => "string".to_string(),
        },
        // 通道常量的值形状：T 由组件字段给出；`Bind.target` 不限定值类型。
        TypeSpec::Struct { name, .. } if *name == "zela.math.Channel" => {
            "Channel<any>".to_string()
        }
        TypeSpec::Struct { name, .. } | TypeSpec::Enum { name, .. } => {
            short_name(name).to_string()
        }
        TypeSpec::Array { elem, .. } => format!("{}[]", ts_type_name(elem)),
        TypeSpec::Opaque(_) => "Handle".to_string(),
        TypeSpec::App { ty, args } => {
            let name = short_name(ty);
            if args.is_empty() {
                name.to_string()
            } else {
                let args: Vec<String> = args.iter().map(ts_type_name).collect();
                format!("{name}<{}>", args.join(", "))
            }
        }
        TypeSpec::Var(_) => "unknown".to_string(),
    }
}

/// 从 `CapabilityDocs` 生成 TypeScript 声明（**无 domain 知识**）。
pub fn d_ts(capabilities: &[CapabilityDocs]) -> String {
    let types = ze_pod_binding::reachable_types(capabilities);
    let typed_expr = has_ops(capabilities);

    let mut out = String::from(
        "// 自动生成：zela 脚本能力声明（TypeScript）\n\
         // JS 只有 number ⇒ i32/u32/u64/u8 一律映射为 number，整数语义在运行时校验。\n\n\
         /** zela 场景句柄（组件 / 对象 / 曲线 / 动作） */\n\
         export type Handle = { readonly __zela_handle: unique symbol };\n\n",
    );
    if typed_expr {
        // 纯静态品牌：`Expr<T>` 运行期仍是 `Handle`，但编辑器可据此检查组合。
        out.push_str(
            "/** 表达式句柄（T 是装配期推导的静态类型；运行期仍是 Handle） */\n\
             export type Expr<T> = Handle & { readonly __zela_expr?: T };\n\n",
        );
    }

    // ── 类型（一次）────────────────────────────────────────────────────
    for (&full_name, &spec) in &types {
        let name = short_name(full_name);
        match spec {
            TypeSpec::Struct { name: type_name, fields, .. } => {
                let is_channel = full_name == "zela.math.Channel";
                if is_channel {
                    out.push_str("export interface Channel<T = unknown> {\n");
                } else {
                    out.push_str(&format!("export interface {name} {{\n"));
                }
                for field in *fields {
                    // 提案 #6 C：默认值进声明文件（`Color.a = 255`），可选字段用 `?`。
                    let optional =
                        ze_pod_binding::field_default_literal(type_name, field.name).is_some();
                    out.push_str(&format!(
                        "  {}{}: {};\n",
                        field.name,
                        if optional { "?" } else { "" },
                        ts_field_type(full_name, field, typed_expr)
                    ));
                }
                if is_channel {
                    // 纯静态品牌：让 `Channel<number>` 与 `Channel<Vec2>` 不互相赋值。
                    out.push_str("  readonly __zela_channel?: T;\n");
                }
                out.push_str("}\n");
                let component = capabilities.iter().any(|capability| {
                    BindingPlan::new(capability.calls)
                        .component_ctor(full_name)
                        .is_some()
                });
                let returns = if let Some(op) = op_by_spec(capabilities, full_name) {
                    ts_expr_returns(op)
                } else if is_channel {
                    "Channel<any>".to_string()
                } else if component {
                    "Handle".to_string()
                } else {
                    name.to_string()
                };
                let positional: Vec<String> = fields
                    .iter()
                    .map(|field| {
                        let optional =
                            ze_pod_binding::field_default_literal(type_name, field.name).is_some();
                        format!(
                            "{}{}: {}",
                            field.name,
                            if optional { "?" } else { "" },
                            ts_field_type(full_name, field, typed_expr)
                        )
                    })
                    .collect();
                out.push_str(&format!(
                    "export declare function {name}(spec: {name}): {returns};\n"
                ));
                out.push_str(&format!(
                    "export declare function {name}({}): {returns};\n",
                    positional.join(", ")
                ));
                // 通道常量收在 `X.channel.*` 下（TS：function + namespace 合并）；
                // 多一层是因为组件字段可能与地址同名（`Opacity.value`）。
                let channels: Vec<&ze_pod_core::ChannelDecl> = capabilities
                    .iter()
                    .flat_map(|capability| capability.channels)
                    .filter(|channel| channel.component == name)
                    .collect();
                if !channels.is_empty() {
                    out.push_str(&format!("export interface {name}Channel {{\n"));
                    for channel in &channels {
                        out.push_str(&format!(
                            "  {}: Channel<{}>;\n",
                            channel.name,
                            ts_type_name(channel.ty)
                        ));
                    }
                    out.push_str("}\n");
                    out.push_str(&format!(
                        "export declare namespace {name} {{\n  const channel: {name}Channel;\n}}\n"
                    ));
                }
                out.push('\n');
            }
            TypeSpec::Enum { variants, .. } => {
                out.push_str(&format!("export type {name} = {name}.Value;\n"));
                out.push_str(&format!("export declare namespace {name} {{\n"));
                out.push_str("  interface Value { readonly __zela_enum: unique symbol }\n");
                for variant in *variants {
                    if variant.fields.is_empty() {
                        out.push_str(&format!("  const {0}: Value;\n", variant.name));
                        continue;
                    }
                    let fields: Vec<String> = variant
                        .fields
                        .iter()
                        .map(|field| format!("{}: {}", field.name, ts_type_name(field.ty)))
                        .collect();
                    out.push_str(&format!(
                        "  function {}(spec: {{ {} }}): Value;\n",
                        variant.name,
                        fields.join("; ")
                    ));
                }
                out.push_str("}\n\n");
            }
            _ => {}
        }
    }

    // ── 能力表 ────────────────────────────────────────────────────────
    for capability in capabilities {
        out.push_str(&format!("export declare const {}: {{\n", capability.name));
        for item in capability.calls {
            let params: Vec<String> = item
                .io
                .params
                .iter()
                .map(|param| format!("{}: {}", param.name, ts_type_name(param.ty)))
                .collect();
            let returns = ts_call_return(capabilities, item);
            out.push_str(&format!(
                "  {}({}): {returns};\n",
                item.signature.name,
                params.join(", ")
            ));
        }
        out.push_str("};\n\n");

        // ── 算子类型签名（L2 前哨，A 批）──────────────────────────────
        // 纯注释：签名是文档/诊断数据，脚本侧没有对应导出。
        if !capability.ops.is_empty() {
            out.push_str(&format!(
                "// ── {} 算子类型签名（L2）────────────────────────\n",
                capability.name
            ));
            for op in capability.ops {
                for line in op.render_lines() {
                    out.push_str(&format!("// {line}\n"));
                }
            }
            out.push('\n');
        }

        // `clock_*` 的别名表（与绑定层一致）：`clock.time()` / `clock.shift(…)` …
        let clock_calls: Vec<&ze_pod_core::ErasedFn> = capability
            .calls
            .iter()
            .filter(|item| item.signature.name.starts_with("clock_"))
            .collect();
        if !clock_calls.is_empty() {
            out.push_str("export declare const clock: {\n");
            for item in clock_calls {
                let short = item
                    .signature
                    .name
                    .strip_prefix("clock_")
                    .unwrap_or(item.signature.name);
                let params: Vec<String> = item
                    .io
                    .params
                    .iter()
                    .map(|param| format!("{}: {}", param.name, ts_type_name(param.ty)))
                    .collect();
                let returns = ts_call_return(capabilities, item);
                out.push_str(&format!("  {short}({}): {returns};\n", params.join(", ")));
            }
            out.push_str("};\n\n");
        }
    }
    out
}

/// 字段类型：`zela.math.*` 里的 `Handle` 字段是**表达式输入**，标成 `Expr<any>`；
/// `Predicate.Of.expr` 是 Bool 表达式。
fn ts_field_type(parent: &str, field: &Field, typed_expr: bool) -> String {
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
    ts_type_name(field.ty)
}

/// 一条能力函数的返回类型：算子按签名表给 `Expr<…>`。
fn ts_call_return(capabilities: &[CapabilityDocs], item: &ze_pod_core::ErasedFn) -> String {
    if matches!(item.io.ret, TypeSpec::Struct { name: "()", .. }) {
        return "void".to_string();
    }
    if let Some(op) = op_by_name(capabilities, item.signature.name) {
        return ts_expr_returns(op);
    }
    match item.signature.name {
        "pure" | "frame" | "clock_time" | "clock_time_of" => "Expr<number>".to_string(),
        "clock_range" | "clock_range_of" => "Expr<boolean>".to_string(),
        "event" | "component_state" | "env" => "Expr<any>".to_string(),
        _ => ts_type_name(item.io.ret),
    }
}

/// 算子的返回类型候选：`Expr<number>` / `Expr<Vec2> | Expr<Vec3>` …
fn ts_expr_returns(op: &OpDef) -> String {
    let mut types: Vec<String> = Vec::new();
    let mut add = |ty: String| {
        if !types.contains(&ty) {
            types.push(ty);
        }
    };
    for sig in op.sigs {
        for ret in sig.ret_types() {
            match ret {
                RetType::Concrete(spec) => add(ts_type_name(spec)),
                RetType::Kind(Kind::Num) => add("number".to_string()),
                RetType::Kind(Kind::Bool) => add("boolean".to_string()),
                RetType::Kind(Kind::Vector) => {
                    add("Vec2".to_string());
                    add("Vec3".to_string());
                }
                RetType::Kind(Kind::Bytes) => add("Bytes".to_string()),
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

/// 把 JSON + `.d.ts` 写到 `dir`（返回实际写入的两个路径）。
pub fn export_docs(dir: &Path, capabilities: &[CapabilityDocs]) -> io::Result<[PathBuf; 2]> {
    fs::create_dir_all(dir)?;
    let json_path = dir.join("scene_api.json");
    let ts_path = dir.join("scene_api.d.ts");
    fs::write(&json_path, ze_pod_binding::capabilities_json(capabilities))?;
    fs::write(&ts_path, d_ts(capabilities))?;
    Ok([json_path, ts_path])
}
