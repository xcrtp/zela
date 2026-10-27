// binding.rs — **Lua 适配层**：通用驱动（`ze-pod-binding`）+ 只属于 Lua 的 codec。
//
// ```text
// C::calls() ──► BindingPlan（语言无关）
//                      │
//                      ├─ 每个函数 → Lua 闭包 → invoke(&LuaCodec, ...)
//                      └─ 每个类型 → Lua 构造器（组件 / 值 / 枚举）
// ```
//
// 语言无关的部分（计划、类型收集、调用编排、JSON 文档）在 `ze-pod-binding`；
// 本文件只保留 `lua_to_pod` / `pod_to_lua` 与 Lua 闭包创建。

use std::sync::Arc;

use mlua::{AnyUserData, Lua, MultiValue, Table, UserData, Value};
use ze_pod_core::{Capability, ErasedFn, Field, Pod, PodValue, ScalarKind, ScalarValue, TypeSpec};
use ze_pod_binding::{BindingPlan, ValueCodec, channel_type_variant, short_name};

/// Lua 侧的不透明句柄令牌。
#[derive(Clone, Copy, Debug)]
pub struct LuaHandle(pub ze_pod_core::Handle);

impl UserData for LuaHandle {}

/// Lua 侧的结构体 / 枚举值令牌（`Color { ... }` / `Curve.Phase { ... }` 的产物）。
#[derive(Clone, Debug)]
pub struct LuaPodValue(pub PodValue);

impl UserData for LuaPodValue {}

/// 注册一个 capability 的全局表（`scene` / `content` / `python`）与类型构造器。
pub(crate) fn bind<C: Capability<'static>>(
    lua: &Lua,
    global_name: &str,
    service: Arc<dyn Pod>,
) -> mlua::Result<()> {
    let plan = BindingPlan::new(C::calls());
    let table = lua.create_table()?;
    for item in plan.calls {
        register_function(lua, &table, *item, Arc::clone(&service))?;
    }
    lua.globals().set(global_name, table.clone())?;
    install_clock_alias(lua, &table, plan.calls)?;
    register_types(lua, &plan, service)?;
    Ok(())
}

/// `clock_*` 前缀的函数同时挂到 **`clock` 表**（`clock.shift` / `clock.scale` /
/// `clock.nest` / `clock.time`）。
///
/// 剧本里 `clock.time()` 与 `expr.time()` 是**同一个**通道叶子：时钟身份属于绑定，
/// 不属于表达式（见 `BindSpec.clock`）。前缀约定让同一份 capability 暴露两个表，
/// 不必新增 Service。
fn install_clock_alias(
    lua: &Lua,
    table: &Table,
    calls: &'static [ErasedFn],
) -> mlua::Result<()> {
    let clock = lua.create_table()?;
    let mut any = false;
    for item in calls {
        if let Some(short) = item.signature.name.strip_prefix("clock_") {
            clock.set(short, table.get::<Value>(item.signature.name)?)?;
            any = true;
        }
    }
    if any {
        lua.globals().set("clock", clock)?;
    }
    Ok(())
}

/// Lua 值 ↔ `PodValue`（**本文件唯一的语言特定逻辑**）。
struct LuaCodec<'a> {
    lua: &'a Lua,
}

impl ValueCodec for LuaCodec<'_> {
    type Value = Value;
    type Error = mlua::Error;

    fn to_pod(&self, value: &Value, spec: &'static TypeSpec) -> mlua::Result<PodValue> {
        lua_to_pod(value, spec)
    }

    fn from_pod(&self, value: &PodValue) -> mlua::Result<Value> {
        pod_to_lua(self.lua, value)
    }

    fn error(&self, message: String) -> mlua::Error {
        mlua::Error::RuntimeError(message)
    }
}

fn register_function(
    lua: &Lua,
    table: &Table,
    item: ErasedFn,
    service: Arc<dyn Pod>,
) -> mlua::Result<()> {
    let name = item.signature.name;

    let func = lua.create_function(move |lua, args: MultiValue| {
        let values: Vec<Value> = args.into_iter().collect();
        let codec = LuaCodec { lua };
        ze_pod_binding::invoke(&codec, &*service, item, &values)
    })?;
    table.set(name, func)
}

/// 自动注册所有可达类型的构造器（组件 / 值 / 枚举）。
///
/// # 提案 #6 C：构造器是**可调用表**
///
/// - `Circle { ... }`（具名）与 `Circle(0.0, 0.0, 0.55)`（位置）都走同一个 `__call`；
/// - 组件 / 值类型的表上挂**通道常量**（`Circle.channel.radius`，来自 `Pod::channels()`）；
/// - 缺字段时读默认值白名单（`Color.a = 255`），默认值不进产物。
fn register_types(lua: &Lua, plan: &BindingPlan, service: Arc<dyn Pod>) -> mlua::Result<()> {
    for (&full_name, &spec) in &plan.types {
        let lua_name = short_name(full_name);
        match spec {
            TypeSpec::Struct { name, fields, .. } => {
                let fields = *fields;
                let type_name = *name;
                let call = plan.component_ctor(type_name).map(|ctor| ctor.call);
                let svc = Arc::clone(&service);
                let mt = lua.create_table()?;
                mt.set(
                    "__call",
                    lua.create_function(move |lua, (_this, args): (Table, MultiValue)| {
                        let value = args_to_pod(args, fields, type_name, None)?;
                        match call {
                            Some(call) => {
                                let out =
                                    call(&*svc, &[value]).map_err(mlua::Error::external)?;
                                pod_to_lua(lua, &out)
                            }
                            None => Ok(Value::UserData(
                                lua.create_userdata(LuaPodValue(value))?,
                            )),
                        }
                    })?,
                )?;
                let ctor = lua.create_table()?;
                ctor.set_metatable(Some(mt))?;
                lua.globals().set(lua_name, ctor)?;
            }
            TypeSpec::Enum { name, variants } => {
                let table = lua.create_table()?;
                for variant in *variants {
                    let type_name = *name;
                    let variant_name = variant.name;
                    let fields = variant.fields;
                    if fields.is_empty() {
                        table.set(
                            variant_name,
                            LuaPodValue(PodValue::Enum(type_name, variant_name, Vec::new())),
                        )?;
                    } else {
                        let func = lua.create_function(move |_, spec: Table| {
                            table_to_pod(&spec, fields, type_name, Some(variant_name))
                                .map(LuaPodValue)
                        })?;
                        table.set(variant_name, func)?;
                    }
                }
                lua.globals().set(lua_name, table)?;
            }
            _ => {}
        }
    }
    attach_channels(lua, service.channels())?;
    Ok(())
}

/// 把 `Pod::channels()` 的常量挂到组件的 **`channel` 字段**上
/// （`Circle.channel.radius`）。
///
/// 常量就是普通表 `{ namespace = ..., name = ... }` —— 与 `expr.bind` 的
/// `ChannelSpec` 同形，三种语言不需要专用令牌。
///
/// 为什么多一层 `channel`：组件字段可能与地址同名（`Opacity.value`），
/// 平铺会撞名；嵌套后 `Opacity.channel.value` 与构造参数 `value` 互不干扰。
fn attach_channels(
    lua: &Lua,
    channels: &'static [ze_pod_core::ChannelSpec],
) -> mlua::Result<()> {
    for channel in channels {
        let Value::Table(ctor) = lua.globals().get::<Value>(channel.component)? else {
            continue;
        };
        // 组件首次出现时建 `channel` 表；后续常量往里塞。
        let channel_table = match ctor.get::<Value>("channel")? {
            Value::Table(table) => table,
            _ => {
                let table = lua.create_table()?;
                ctor.set("channel", table.clone())?;
                table
            }
        };
        let value = lua.create_table()?;
        value.set("namespace", channel.namespace)?;
        value.set("name", channel.address)?;
        // 第 2 批：类型随常量过边界（结构化枚举，不再是脚本手写的字符串）。
        value.set(
            "ty",
            LuaPodValue(PodValue::Enum(
                "zela.math.ChannelType",
                channel_type_variant(channel.ty),
                Vec::new(),
            )),
        )?;
        // 第 2 批语法糖：常量**可调用** —— `Reveal.channel.current_pos(curve)`
        // 等价于 `expr.read { channel = Reveal.channel.current_pos, object = curve }`。
        // `expr` 在另一个 capability 里注册，故在**调用时**才解析（不依赖装配顺序）。
        let mt = lua.create_table()?;
        mt.set(
            "__call",
            lua.create_function(|lua, (channel, object): (Table, Value)| {
                let expr: Table = lua.globals().get("expr")?;
                let Value::Function(read) = expr.get::<Value>("read")? else {
                    return Err(mlua::Error::RuntimeError(
                        "`expr.read` 尚未注册（宿主未装配表达式能力）".into(),
                    ));
                };
                // Pod 侧 `read` 收一个 `ReadSpec` 表（具名），故这里组表再调用。
                let spec = lua.create_table()?;
                spec.set("channel", channel)?;
                spec.set("object", object)?;
                read.call::<Value>(spec)
            })?,
        )?;
        value.set_metatable(Some(mt))?;
        channel_table.set(channel.name, value)?;
    }
    Ok(())
}

/// 构造器实参 → `PodValue`（具名 / 位置双形态 + 默认值）。
///
/// **形态判别**：唯一实参是 table ⇒ 具名；否则按字段顺序取位置参数。
/// Lua 的空表因此不会被误判成"零个位置参数"（`Vec2{}` 是具名空结构）。
fn args_to_pod(
    args: MultiValue,
    fields: &'static [Field],
    type_name: &'static str,
    variant: Option<&'static str>,
) -> mlua::Result<PodValue> {
    let values: Vec<Value> = args.into_iter().collect();
    if values.len() == 1 {
        if let Value::Table(table) = &values[0] {
            return table_to_pod(table, fields, type_name, variant);
        }
    }
    if values.len() > fields.len() {
        return Err(mlua::Error::RuntimeError(format!(
            "`{type_name}` 最多接受 {} 个位置参数，实际 {}",
            fields.len(),
            values.len()
        )));
    }
    let mut out = Vec::with_capacity(fields.len());
    for (index, field) in fields.iter().enumerate() {
        let value = match values.get(index) {
            Some(value) if !value.is_nil() => lua_to_pod(value, field.ty)?,
            _ => ze_pod_binding::field_default(type_name, field.name).ok_or_else(|| {
                mlua::Error::RuntimeError(format!(
                    "`{type_name}` 缺少字段 `{}`（位置参数也不足）",
                    field.name
                ))
            })?,
        };
        out.push((field.name, value));
    }
    Ok(match variant {
        Some(variant) => PodValue::Enum(type_name, variant, out),
        None => PodValue::Struct(type_name, out),
    })
}

fn table_to_pod(
    table: &Table,
    fields: &'static [Field],
    type_name: &'static str,
    variant: Option<&'static str>,
) -> mlua::Result<PodValue> {
    let mut values = Vec::with_capacity(fields.len());
    for field in fields {
        let value: Value = table.get(field.name)?;
        if value.is_nil() {
            // 默认值白名单（`Color.a = 255`）；不在白名单里 ⇒ 显式报错。
            let default = ze_pod_binding::field_default(type_name, field.name).ok_or_else(|| {
                mlua::Error::RuntimeError(format!(
                    "`{type_name}` 缺少字段 `{}`",
                    field.name
                ))
            })?;
            values.push((field.name, default));
            continue;
        }
        values.push((field.name, lua_to_pod(&value, field.ty)?));
    }
    Ok(match variant {
        Some(variant) => PodValue::Enum(type_name, variant, values),
        None => PodValue::Struct(type_name, values),
    })
}

fn lua_to_pod(value: &Value, spec: &'static TypeSpec) -> mlua::Result<PodValue> {
    match spec {
        TypeSpec::Scalar(kind) => scalar_from_lua(value, *kind),
        TypeSpec::Opaque(_) => match value {
            Value::UserData(ud) => Ok(PodValue::Opaque(handle_of(ud)?)),
            other => Err(type_error("句柄（userdata）", other)),
        },
        TypeSpec::Array { elem, .. } => match value {
            Value::Table(table) => {
                let mut values = Vec::new();
                for item in table.sequence_values::<Value>() {
                    values.push(lua_to_pod(&item?, elem)?);
                }
                Ok(PodValue::Array(values))
            }
            other => Err(type_error("table（数组）", other)),
        },
        TypeSpec::Struct { name, fields, .. } => {
            if let Some(value) = wrapped_value(value) {
                return match &value {
                    PodValue::Struct(actual, _) if actual == name => Ok(value),
                    other => Err(mlua::Error::RuntimeError(format!(
                        "期望结构体 `{name}`，实际是 {}",
                        ze_pod_core::describe(other)
                    ))),
                };
            }
            match value {
                Value::Table(table) => table_to_pod(table, fields, name, None),
                other => Err(type_error("table（结构体）", other)),
            }
        }
        TypeSpec::Enum { name, .. } => {
            if let Some(value) = wrapped_value(value) {
                return match &value {
                    PodValue::Enum(actual, _, _) if actual == name => Ok(value),
                    other => Err(mlua::Error::RuntimeError(format!(
                        "期望枚举 `{name}`，实际是 {}",
                        ze_pod_core::describe(other)
                    ))),
                };
            }
            Err(mlua::Error::RuntimeError(format!(
                "枚举 `{name}` 请用构造器创建（如 `Curve.Phase {{ ... }}`）"
            )))
        }
        // 泛型 / 类型变量没有 PodValue 形态：它们是**装配期类型**。
        TypeSpec::Var(name) => Err(mlua::Error::RuntimeError(format!(
            "类型变量 `{name}` 没有运行时表示"
        ))),
        TypeSpec::App { ty, .. } => Err(mlua::Error::RuntimeError(format!(
            "泛型类型 `{ty}` 没有通用运行时转换"
        ))),
    }
}

/// 若 Lua 值是 `LuaPodValue` 包装，取出内部值。
fn wrapped_value(value: &Value) -> Option<PodValue> {
    let Value::UserData(ud) = value else {
        return None;
    };
    ud.borrow::<LuaPodValue>()
        .ok()
        .map(|wrapped| wrapped.0.clone())
}

fn handle_of(value: &AnyUserData) -> mlua::Result<ze_pod_core::Handle> {
    value
        .borrow::<LuaHandle>()
        .map(|handle| handle.0)
        .map_err(|_| mlua::Error::RuntimeError("期望 zela 场景句柄（userdata）".to_string()))
}

fn scalar_from_lua(value: &Value, kind: ScalarKind) -> mlua::Result<PodValue> {
    let scalar = match kind {
        ScalarKind::F32 => ScalarValue::F32(number(value)? as f32),
        ScalarKind::F64 => ScalarValue::F64(number(value)?),
        ScalarKind::I32 => ScalarValue::I32(integer(value)? as i32),
        ScalarKind::U32 => ScalarValue::U32(integer(value)? as u32),
        ScalarKind::U64 => {
            let n = integer(value)?;
            let n = u64::try_from(n).map_err(|_| {
                mlua::Error::RuntimeError(format!("u64 必须是非负整数，实际 {n}"))
            })?;
            ScalarValue::U64(n)
        }
        ScalarKind::U8 => {
            let n = integer(value)?;
            if !(0..=255).contains(&n) {
                return Err(mlua::Error::RuntimeError(format!(
                    "颜色分量必须在 0..=255，实际 {n}"
                )));
            }
            ScalarValue::U8(n as u8)
        }
        ScalarKind::Bool => match value {
            Value::Boolean(b) => ScalarValue::Bool(*b),
            other => return Err(type_error("boolean", other)),
        },
        ScalarKind::String => match value {
            Value::String(s) => ScalarValue::String(s.to_string_lossy().to_string()),
            other => return Err(type_error("string", other)),
        },
    };
    Ok(PodValue::Scalar(scalar))
}

fn number(value: &Value) -> mlua::Result<f64> {
    match value {
        Value::Number(n) => Ok(*n),
        Value::Integer(i) => Ok(*i as f64),
        other => Err(type_error("number", other)),
    }
}

fn integer(value: &Value) -> mlua::Result<i64> {
    match value {
        Value::Integer(i) => Ok(*i),
        // 精确零语义：判断的是"这个 f64 的数学值是否为整数"，不是浮点计算结果
        // 的近似比较；`fract() == 0.0` 对 Lua 传入的整数值是精确成立的。
        Value::Number(n) if n.fract() == 0.0 => Ok(*n as i64),
        other => Err(type_error("integer", other)),
    }
}

fn pod_to_lua(lua: &Lua, value: &PodValue) -> mlua::Result<Value> {
    match value {
        PodValue::Scalar(scalar) => Ok(match scalar {
            ScalarValue::F32(v) => Value::Number(*v as f64),
            ScalarValue::F64(v) => Value::Number(*v),
            ScalarValue::I32(v) => Value::Integer(*v as i64),
            ScalarValue::U32(v) => Value::Integer(*v as i64),
            ScalarValue::U64(v) => Value::Integer(i64::try_from(*v).map_err(|_| {
                mlua::Error::RuntimeError(format!("u64 超出 Lua 整数范围：{v}"))
            })?),
            ScalarValue::U8(v) => Value::Integer(*v as i64),
            ScalarValue::Bool(v) => Value::Boolean(*v),
            ScalarValue::String(v) => Value::String(lua.create_string(v)?),
        }),
        PodValue::Opaque(handle) => Ok(Value::UserData(lua.create_userdata(LuaHandle(*handle))?)),
        PodValue::Struct("()", _) => Ok(Value::Nil),
        PodValue::Struct(..) | PodValue::Enum(..) => Ok(Value::UserData(
            lua.create_userdata(LuaPodValue(value.clone()))?,
        )),
        PodValue::Array(items) => {
            let table = lua.create_table_with_capacity(items.len(), 0)?;
            for (index, item) in items.iter().enumerate() {
                table.set(index + 1, pod_to_lua(lua, item)?)?;
            }
            Ok(Value::Table(table))
        }
        // B3-0.5a：bulk 字节块不走脚本绑定（契约在签名层；传输走 typed 快路径）。
        PodValue::Bytes(bytes) => Err(mlua::Error::RuntimeError(format!(
            "字节块（{} 字节）不在脚本绑定层传输；请走 typed 快路径（D4.4）",
            bytes.len()
        ))),
    }
}

fn type_error(expected: &str, actual: &Value) -> mlua::Error {
    mlua::Error::RuntimeError(format!(
        "参数类型不匹配：期望 {expected}，实际 {}",
        actual.type_name()
    ))
}
