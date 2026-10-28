// export.rs — **Python 侧文档生成**：`.pyi` 按 capability 的命名空间路径落盘。
//
// 与 `ze-pod-lua` 的 `lua_ls` 对称：JSON 真源在 `ze-pod-binding`，
// 本文件只负责把同一份 schema 翻译成 Python 类型存根（不依赖 pyo3）。
//
// ```text
// zela.scene   → zela/scene.pyi
// zela.content → zela/content.pyi
// zela.python  → zela/python.pyi
//                zela/__init__.pyi（Handle + 共享类型 + 子模块再导出）
// ```
//
// 这样 `import zela.scene` / `from zela import scene` 与普通 Python 模块一致。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ze_pod_core::{Field, Kind, OpDef, RetType, ScalarKind, TypeSpec};
use ze_pod_binding::{
    BindingPlan, CapabilityDocs, has_ops, op_by_name, op_by_spec, short_name,
};

/// Python 关键字（不能直接作形参名）；仅对我们 domain 里出现的做最小映射。
fn py_ident(name: &str) -> String {
    match name {
        "from" => "from_".to_string(),
        other => other.to_string(),
    }
}

fn parent_of(full: &str) -> &str {
    full.rsplit_once('.').map(|(parent, _)| parent).unwrap_or("")
}

fn root_of(path: &str) -> &str {
    path.split('.').next().unwrap_or(path)
}

/// `a.b.c` → `["a", "a.b", "a.b.c"]`。
fn ancestors(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = path;
    loop {
        out.push(current.to_string());
        match current.rsplit_once('.') {
            Some((parent, _)) => current = parent,
            None => break,
        }
    }
    out.reverse();
    out
}

/// capability 模块的父包链（不含模块自身）。
fn package_ancestors(module: &str) -> Vec<String> {
    let mut packages = ancestors(module);
    packages.pop();
    packages
}

/// capability 的模块路径：优先取函数签名里的 namespace（如 `zela.scene`）。
fn capability_path(capability: &CapabilityDocs) -> String {
    capability
        .calls
        .first()
        .map(|item| item.signature.namespace)
        .filter(|namespace| !namespace.is_empty())
        .unwrap_or(capability.name)
        .to_string()
}

/// 收集一个类型（含嵌套字段）引用到的命名类型。
fn collect_named(spec: &'static TypeSpec, into: &mut BTreeSet<&'static str>) {
    match spec {
        TypeSpec::Struct { name, fields, .. } => {
            into.insert(name);
            for field in *fields {
                collect_named(field.ty, into);
            }
        }
        TypeSpec::Enum { name, variants } => {
            into.insert(name);
            for variant in *variants {
                for field in variant.fields {
                    collect_named(field.ty, into);
                }
            }
        }
        TypeSpec::Array { elem, .. } => collect_named(elem, into),
        TypeSpec::App { args, .. } => {
            for arg in *args {
                collect_named(arg, into);
            }
        }
        TypeSpec::Scalar(_) | TypeSpec::Opaque(_) | TypeSpec::Var(_) => {}
    }
}

/// 只收集签名**直接引用**的命名类型（不钻进字段；字段类型由声明处导入）。
fn collect_top(spec: &'static TypeSpec, into: &mut BTreeSet<&'static str>) {
    match spec {
        TypeSpec::Struct { name, .. } | TypeSpec::Enum { name, .. } => {
            into.insert(name);
        }
        TypeSpec::Array { elem, .. } => collect_top(elem, into),
        TypeSpec::App { args, .. } => {
            for arg in *args {
                collect_top(arg, into);
            }
        }
        TypeSpec::Scalar(_) | TypeSpec::Opaque(_) | TypeSpec::Var(_) => {}
    }
}

/// 组件构造器在运行时是**函数**（返回 `Handle`），不是类；
/// 它们没有可引用的 Python 类型名，作为形参类型时退化为 `Any`。
fn py_type(spec: &'static TypeSpec, components: &BTreeSet<&'static str>) -> String {
    match spec {
        TypeSpec::Scalar(kind) => match kind {
            ScalarKind::F32 | ScalarKind::F64 => "float".to_string(),
            ScalarKind::I32 | ScalarKind::U32 | ScalarKind::U64 | ScalarKind::U8 => "int".to_string(),
            ScalarKind::Bool => "bool".to_string(),
            ScalarKind::String => "str".to_string(),
        },
        TypeSpec::Struct { name, .. } if *name == "()" => "None".to_string(),
        // 通道常量的值形状：T 由组件字段给出；`Bind.target` 不限定值类型。
        TypeSpec::Struct { name, .. } if *name == "zela.math.Channel" => {
            "Channel[Any]".to_string()
        }
        TypeSpec::Struct { name, .. } if components.contains(name) => "Any".to_string(),
        TypeSpec::Struct { name, .. } | TypeSpec::Enum { name, .. } => {
            short_name(name).to_string()
        }
        TypeSpec::Array { elem, .. } => format!("List[{}]", py_type(elem, components)),
        TypeSpec::Opaque(_) => "Handle".to_string(),
        TypeSpec::App { ty, args } => {
            let name = short_name(ty);
            if args.is_empty() {
                name.to_string()
            } else {
                let args: Vec<String> = args.iter().map(|arg| py_type(arg, components)).collect();
                format!("{name}[{}]", args.join(", "))
            }
        }
        TypeSpec::Var(_) => "Any".to_string(),
    }
}

/// 字段类型：`zela.math.*` 里的 `Handle` 字段是**表达式输入**，标成 `Expr[Any]`；
/// `Predicate.Of.expr` 是 Bool 表达式。
fn py_field_type(
    parent: &str,
    field: &Field,
    components: &BTreeSet<&'static str>,
    typed_expr: bool,
) -> String {
    // `Bind.object` 是**场景对象**句柄，不是表达式输入。
    if typed_expr
        && parent.starts_with("zela.math.")
        && !(parent == "zela.math.Bind" && field.name == "object")
    {
        if parent == "zela.math.Predicate.Of" && field.name == "expr" {
            return "Expr[bool]".to_string();
        }
        if matches!(*field.ty, TypeSpec::Opaque(_)) {
            return "Expr[Any]".to_string();
        }
    }
    py_type(field.ty, components)
}

fn params(
    fields: &'static [ze_pod_core::Field],
    components: &BTreeSet<&'static str>,
    type_name: &'static str,
    parent: &str,
    typed_expr: bool,
) -> String {
    fields
        .iter()
        .map(|field| {
            // 提案 #6 C：默认值进声明文件（`Color.a = 255`）。
            let default = ze_pod_binding::field_default_literal(type_name, field.name)
                .map(|value| format!(" = {value}"))
                .unwrap_or_default();
            format!(
                "{}: {}{default}",
                py_ident(field.name),
                py_field_type(parent, field, components, typed_expr)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn push_description(out: &mut String, text: &str) {
    for line in text.lines() {
        out.push_str("# ");
        out.push_str(line);
        out.push('\n');
    }
}

fn push_imports(out: &mut String, imports: &BTreeMap<String, BTreeSet<String>>) {
    for (home, names) in imports {
        out.push_str(&format!(
            "from {home} import {}\n",
            names.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
}

/// 组件短名 → 通道常量（`Circle.channel.radius`）。
fn channels_by_component(
    capabilities: &[CapabilityDocs],
) -> BTreeMap<&'static str, Vec<&'static ze_pod_core::ChannelDecl>> {
    let mut out: BTreeMap<&'static str, Vec<&'static ze_pod_core::ChannelDecl>> = BTreeMap::new();
    for capability in capabilities {
        for channel in capability.channels {
            out.entry(channel.component).or_default().push(channel);
        }
    }
    out
}

fn render_type(
    out: &mut String,
    full_name: &'static str,
    spec: &'static TypeSpec,
    components: &BTreeSet<&'static str>,
    channels: &[&'static ze_pod_core::ChannelDecl],
    capabilities: &[CapabilityDocs],
    typed_expr: bool,
) {
    let name = short_name(full_name);
    match spec {
        TypeSpec::Struct { name: type_name, fields, .. } => {
            if components.contains(full_name) {
                // 算子构造器返回**带静态类型的表达式句柄**；其它组件返回 Handle。
                let returns = if let Some(op) = op_by_spec(capabilities, full_name) {
                    py_expr_returns(op, components)
                } else {
                    "Handle".to_string()
                };
                if channels.is_empty() {
                    out.push_str(&format!(
                        "def {name}({}) -> {returns}: ...\n",
                        params(fields, components, type_name, full_name, typed_expr)
                    ));
                } else {
                    // 带通道常量的组件在运行期是"可调用对象"⇒ 用 Protocol 建模，
                    // 这样 `Circle(channel=...)` 与 `Circle(...)` 都是合法表达式。
                    // 常量收在 `channel` 命名空间下（与组件字段不撞名）。
                    out.push_str(&format!("class _{name}Channel:\n"));
                    for channel in channels {
                        out.push_str(&format!(
                            "    {}: Channel[{}]\n",
                            channel.name,
                            py_type(channel.ty, components)
                        ));
                    }
                    out.push_str(&format!("class _{name}Ctor(Protocol):\n"));
                    out.push_str(&format!("    channel: _{name}Channel\n"));
                    out.push_str(&format!(
                        "    def __call__(self, {}) -> {returns}: ...\n",
                        params(fields, components, type_name, full_name, typed_expr)
                    ));
                    out.push_str(&format!("{name}: _{name}Ctor\n"));
                }
            } else {
                // 通道值形状是泛型 `Channel<T>`：T 由组件字段给出。
                if full_name == "zela.math.Channel" {
                    out.push_str("class Channel(Generic[T]):\n");
                } else {
                    out.push_str(&format!("class {name}:\n"));
                }
                if fields.is_empty() {
                    out.push_str("    ...\n");
                    return;
                }
                for field in *fields {
                    out.push_str(&format!(
                        "    {}: {}\n",
                        py_ident(field.name),
                        py_field_type(full_name, field, components, typed_expr)
                    ));
                }
                out.push_str(&format!(
                    "    def __init__(self, {}) -> None: ...\n",
                    params(fields, components, type_name, full_name, typed_expr)
                ));
            }
        }
        TypeSpec::Enum { name: type_name, variants } => {
            out.push_str(&format!("class {name}:\n"));
            if variants.is_empty() {
                out.push_str("    ...\n");
                return;
            }
            for variant in *variants {
                if variant.fields.is_empty() {
                    out.push_str(&format!("    {}: {}\n", variant.name, name));
                } else {
                    let parent = format!("{type_name}.{}", variant.name);
                    out.push_str(&format!(
                        "    @staticmethod\n    def {}({}) -> {}: ...\n",
                        variant.name,
                        params(
                            variant.fields,
                            components,
                            type_name,
                            &parent,
                            typed_expr
                        ),
                        name
                    ));
                }
            }
        }
        _ => {}
    }
}

/// 一条能力函数的返回类型：算子按签名表给 `Expr[…]`。
fn py_call_return(
    capabilities: &[CapabilityDocs],
    item: &ze_pod_core::ErasedFn,
    components: &BTreeSet<&'static str>,
) -> String {
    if matches!(item.io.ret, TypeSpec::Struct { name: "()", .. }) {
        return "None".to_string();
    }
    if let Some(op) = op_by_name(capabilities, item.signature.name) {
        return py_expr_returns(op, components);
    }
    match item.signature.name {
        "pure" | "frame" | "clock_time" | "clock_time_of" => "Expr[float]".to_string(),
        "clock_range" | "clock_range_of" => "Expr[bool]".to_string(),
        "rec_index" => "Expr[int]".to_string(),
        "method" | "nat_rec" | "rec_prev" | "event" | "component_state" | "env" => "Expr[Any]".to_string(),
        _ => py_type(item.io.ret, components),
    }
}

/// 算子的返回类型候选：`Expr[float] | Expr[Vec2] | Expr[Vec3]` …
fn py_expr_returns(op: &OpDef, components: &BTreeSet<&'static str>) -> String {
    let mut types: Vec<String> = Vec::new();
    let mut add = |ty: String| {
        if !types.contains(&ty) {
            types.push(ty);
        }
    };
    for sig in op.sigs {
        for ret in sig.ret_types() {
            match ret {
                RetType::Concrete(spec) => add(py_type(spec, components)),
                RetType::Kind(Kind::Num) => add("float".to_string()),
                RetType::Kind(Kind::Bool) => add("bool".to_string()),
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
        .map(|ty| format!("Expr[{ty}]"))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// 生成按 capability 路径组织的 `.pyi` 文件树。
///
/// key 是相对路径（`/` 分隔），例如 `zela/__init__.pyi` / `zela/scene.pyi`。
pub fn pyi_files(capabilities: &[CapabilityDocs]) -> BTreeMap<String, String> {
    let types = ze_pod_binding::reachable_types(capabilities);
    let typed_expr = has_ops(capabilities);

    // 组件类型（运行时是函数，不产生类）。
    let mut components: BTreeSet<&'static str> = BTreeSet::new();
    for capability in capabilities {
        let plan = BindingPlan::new(capability.calls);
        for (&full_name, &spec) in &plan.types {
            if matches!(spec, TypeSpec::Struct { .. }) && plan.component_ctor(full_name).is_some()
            {
                components.insert(full_name);
            }
        }
    }

    // capability 模块路径 → 文档。
    let mut modules: BTreeMap<String, &CapabilityDocs> = BTreeMap::new();
    for capability in capabilities {
        modules.insert(capability_path(capability), capability);
    }

    let default_root = modules
        .keys()
        .next()
        .map(|module| root_of(module).to_string())
        .unwrap_or_else(|| "zela".to_string());

    // 类型 → 所属包（类型名去掉最后一段）。
    let mut type_home: BTreeMap<&'static str, String> = BTreeMap::new();
    for &full_name in types.keys() {
        let parent = parent_of(full_name);
        type_home.insert(
            full_name,
            if parent.is_empty() {
                default_root.clone()
            } else {
                parent.to_string()
            },
        );
    }

    // 所有需要生成 `__init__.pyi` 的包。
    let mut packages: BTreeSet<String> = BTreeSet::new();
    for module in modules.keys() {
        for package in package_ancestors(module) {
            packages.insert(package);
        }
    }
    for home in type_home.values() {
        for package in ancestors(home) {
            packages.insert(package);
        }
    }

    // 包 → 直接子模块 / 子包（用于 `from . import x as x`）。
    let mut children: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for module in modules.keys() {
        if let Some((parent, name)) = module.rsplit_once('.') {
            children
                .entry(parent.to_string())
                .or_default()
                .insert(name.to_string());
        }
    }
    for package in &packages {
        if let Some((parent, name)) = package.rsplit_once('.') {
            children
                .entry(parent.to_string())
                .or_default()
                .insert(name.to_string());
        }
    }

    let mut files = BTreeMap::new();
    let component_channels = channels_by_component(capabilities);

    // ── 包 __init__.pyi：Handle + 共享类型 + 子模块再导出 ───────────────
    for package in &packages {
        let root = root_of(package);
        // 本包是否需要泛型（`Expr<T>` 由根包定义；`Channel<T>` 在 zela.math 定义）。
        let defines_channel = types.iter().any(|(&full, _)| {
            full == "zela.math.Channel" && type_home.get(full) == Some(package)
        });
        let needs_generic = defines_channel || (typed_expr && package == root);
        let package_has_math = types.iter().any(|(&full, _)| {
            type_home.get(full) == Some(package) && full.starts_with("zela.math.")
        });

        let mut out = String::from(
            "# 由 zela-app 的 export_scene_api 生成；与 scene_api.json 同源。\n",
        );
        if needs_generic {
            out.push_str("from typing import Any, Generic, List, Protocol, TypeVar\n\n");
            out.push_str("T = TypeVar(\"T\")\n\n");
        } else {
            out.push_str("from typing import Any, List, Protocol\n\n");
        }
        if package == root {
            out.push_str(
                "class Handle:\n    \"\"\"不透明句柄（组件 / 对象 / 曲线 / 动作）。\"\"\"\n\n",
            );
            if typed_expr {
                out.push_str(
                    "class Expr(Handle, Generic[T]):\n    \"\"\"表达式句柄（T 是装配期推导的静态类型；运行期仍是 Handle）。\"\"\"\n\n",
                );
            }
        } else {
            let mut imported = vec!["Handle"];
            if typed_expr && package_has_math {
                imported.push("Expr");
            }
            out.push_str(&format!(
                "from {root} import {}\n\n",
                imported.join(", ")
            ));
        }

        let package_types: Vec<(&'static str, &'static TypeSpec)> = types
            .iter()
            .filter(|(full_name, _)| type_home.get(*full_name) == Some(package))
            .map(|(&full_name, &spec)| (full_name, spec))
            .collect();

        let mut references = BTreeSet::new();
        for (_, spec) in &package_types {
            collect_named(spec, &mut references);
        }
        // 通道常量的值类型（`Channel`）也要进导入表。
        for (full_name, _) in &package_types {
            if let Some(channels) = component_channels.get(short_name(full_name)) {
                for channel in channels {
                    references.insert(channel.value_type);
                }
            }
        }
        let mut imports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for name in references {
            if name == "()" || !types.contains_key(name) {
                continue;
            }
            let home = &type_home[name];
            if home != package {
                imports
                    .entry(home.clone())
                    .or_default()
                    .insert(short_name(name).to_string());
            }
        }
        push_imports(&mut out, &imports);

        if let Some(kids) = children.get(package) {
            for child in kids {
                out.push_str(&format!("from . import {child} as {child}\n"));
            }
        }
        out.push('\n');

        // `clock_*` 的别名表挂在根模块上（与 Lua / JS 一致；`clock.time()` 与
        // `clock.time()` 读绑定时钟）。
        if package == root
            && capabilities
                .iter()
                .any(|capability| {
                    capability
                        .calls
                        .iter()
                        .any(|item| item.signature.name.starts_with("clock_"))
                })
        {
            out.push_str("clock: Any\n\n");
        }

        for (full_name, spec) in package_types {
            let own = component_channels
                .get(short_name(full_name))
                .cloned()
                .unwrap_or_default();
            render_type(
                &mut out,
                full_name,
                spec,
                &components,
                &own,
                capabilities,
                typed_expr,
            );
            out.push('\n');
        }
        files.insert(format!("{}/__init__.pyi", package.replace('.', "/")), out);
    }

    // ── capability 模块：函数 + 类型再导出 ─────────────────────────────
    for (path, capability) in &modules {
        let mut out = String::from(
            "# 由 zela-app 的 export_scene_api 生成；与 scene_api.json 同源。\n\
             from typing import Any, List\n",
        );
        let root = root_of(path);
        let needs_handle = capability.calls.iter().any(|item| {
            item.io
                .params
                .iter()
                .any(|param| matches!(param.ty, TypeSpec::Opaque(_)))
                || matches!(item.io.ret, TypeSpec::Opaque(_))
        });
        let needs_expr = typed_expr && !capability.ops.is_empty();
        let mut imported = Vec::new();
        if needs_handle {
            imported.push("Handle");
        }
        if needs_expr {
            imported.push("Expr");
        }
        if !imported.is_empty() {
            out.push_str(&format!("from {root} import {}\n", imported.join(", ")));
        }

        let mut references = BTreeSet::new();
        for item in capability.calls {
            for param in item.io.params {
                collect_top(param.ty, &mut references);
            }
            collect_top(item.io.ret, &mut references);
        }
        let mut imports: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for name in references {
            // 组件类型在签名里映射为 `Any`，不需要导入（且运行时是函数）。
            if name == "()" || !types.contains_key(name) || components.contains(name) {
                continue;
            }
            imports
                .entry(type_home[name].clone())
                .or_default()
                .insert(short_name(name).to_string());
        }
        push_imports(&mut out, &imports);
        out.push('\n');

        // ── 算子类型签名（L2 前哨，A 批）──────────────────────────────
        if !capability.ops.is_empty() {
            out.push_str("# ── 算子类型签名（L2）────────────────────────\n");
            for op in capability.ops {
                for line in op.render_lines() {
                    out.push_str(&format!("# {line}\n"));
                }
            }
            out.push('\n');
        }

        for (index, item) in capability.calls.iter().enumerate() {
            if let Some(doc) = capability.docs.get(index) {
                push_description(&mut out, doc.description);
            }
            let params: Vec<String> = item
                .io
                .params
                .iter()
                .map(|param| {
                    format!(
                        "{}: {}",
                        py_ident(param.name),
                        py_type(param.ty, &components)
                    )
                })
                .collect();
            out.push_str(&format!(
                "def {}({}) -> {}: ...\n\n",
                item.signature.name,
                params.join(", "),
                py_call_return(capabilities, item, &components)
            ));
        }
        files.insert(format!("{}.pyi", path.replace('.', "/")), out);
    }

    files
}

/// 把 `.pyi` 文件树写到 `dir`（返回实际写入的路径）。
pub fn export_docs(dir: &Path, capabilities: &[CapabilityDocs]) -> io::Result<Vec<PathBuf>> {
    let files = pyi_files(capabilities);
    let mut written = Vec::with_capacity(files.len());
    for (relative, content) in files {
        let path = dir.join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PythonApi;
    use ze_pod_core::{Capability, Handle};
    use ze_pod_macro::{PodType, capability};

    #[derive(Clone, Debug, PartialEq, PodType)]
    #[pod_type(name = "zela.Dot")]
    struct Dot {
        x: f32,
        y: f32,
    }

    #[allow(dead_code)] // capability 的 inherent 方法不全部调用。
    #[capability(namespace = "zela.scene", bindings)]
    struct MiniScene<'a> {
        /// 创建一个点组件。
        dot: fn(&'a dyn ze_pod_core::Pod, shape: Dot) -> Handle,
    }

    fn files() -> BTreeMap<String, String> {
        pyi_files(&[
            CapabilityDocs {
                name: "scene",
                calls: MiniScene::calls(),
                docs: MiniScene::docs(),
                meta: MiniScene::meta(),
                channels: &[],
                ops: &[],
            },
            CapabilityDocs {
                name: "python",
                calls: PythonApi::calls(),
                docs: PythonApi::docs(),
                meta: PythonApi::meta(),
                channels: &[],
                ops: &[],
            },
        ])
    }

    #[test]
    fn pyi_paths_follow_capability_namespaces() {
        let files = files();
        assert!(files.contains_key("zela/__init__.pyi"));
        assert!(files.contains_key("zela/scene.pyi"));
        assert!(files.contains_key("zela/python.pyi"));
    }

    #[test]
    fn pyi_root_declares_handle_and_types() {
        let files = files();
        let root = &files["zela/__init__.pyi"];
        assert!(root.contains("class Handle:"));
        assert!(root.contains("def Dot("), "组件构造器应是函数");
        assert!(root.contains("from . import scene as scene"));
        assert!(root.contains("from . import python as python"));
    }

    #[test]
    fn pyi_capability_module_reexports_types() {
        let files = files();
        let scene = &files["zela/scene.pyi"];
        assert!(scene.contains("from zela import Handle"));
        assert!(scene.contains("def dot("));
        assert!(scene.contains("-> Handle: ..."));
        // 组件类型映射为 Any，不产生 `from zela import Dot`。
        assert!(!scene.contains("import Dot"));
    }
}
