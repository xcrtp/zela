//! **WGSL codegen（2 阶）**：把 [`Expr`] 编译成一个**确定性的纯函数**。
//!
//! # 边界（这个模块不知道什么）
//!
//! - 不知道后端、不知道 buffer 布局、不知道 bind group：只产出
//!   `fn eval(p: Params, index: u32) -> T { … }` 这段**纯函数**文本，适配器负责
//!   "`for i { out[i] = eval(p, i) }`" 与资源绑定。
//! - 不知道算子语义：[`WgslBackend`] 由算子库实现（`ze-func::wgsl`），本模块只做
//!   结构递归、类型映射与文本拼接。
//!
//! # 硬约束
//!
//! 1. **确定性**：同一棵 `Expr`（`validate` 通过、id 稳定）⇒ 同一份 WGSL **逐字节**。
//!    所有集合都用 `BTreeMap`，变量编号按后序递增，浮点字面量用最短往返表示。
//! 2. **可生成性由地址决定**：`ChannelClass::HostState` 的通道**不可进 shader**
//!    （显式失败，不静默换成 0）；`Frame` / `Element` 由后端决定映射。
//! 3. **有界**：`ze-comb` 是结构递归（`NatRec` 除外，本模块**显式拒绝**），
//!    ⇒ 不存在无界循环，终止性在 IR 层保证。
//! 4. **CPU 镜像不是 fallback**：同一条表达式的语义基准仍是 `CompiledExpr` /
//!    `eval_reference`；本模块只多一条"能生成"的出口。
//!
//! # 精度口径
//!
//! WGSL 只有 `f32`。`SlotType::F64` 在 shader 里映射为 `f32` —— 这是**已知的**
//! payload 外差异（GAPS AU-04：标量 f64 / 向量 f32 是两种显式类型；上 GPU 时统一
//! 落到 f32），跨设备逐位一致不承诺，容差由对拍用例给。

use std::collections::BTreeMap;

use crate::expr::{ChannelClass, ChannelType, Expr, ExprError, Id, Literal, Node, OpRef};
use crate::typed::SlotType;

/// 生成函数的参数（一个通道读端）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WgslParam {
    /// WGSL 字段名（后端给的标识符；必须满足 WGSL 的 identifier 规则）。
    pub name: String,
    /// 槽类型（`F64` 在这里表示 `f32`）。
    pub ty: SlotType,
}

/// 生成的一份 WGSL：参数表（按名字排序）+ 函数体 + 返回类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WgslFunction {
    pub params: Vec<WgslParam>,
    pub source: String,
    pub ret: SlotType,
}

/// 算子库实现的 codegen 后端。
pub trait WgslBackend {
    /// 一个算子的 WGSL 表达式。
    ///
    /// `types` 是实参的槽类型（与 `args` 一一对应），`args` 是可嵌入的子表达式。
    /// 返回 `None` ⇒ 该类型组合不能生成（例如未登记的签名），调用方**显式失败**。
    fn op(&self, op: &OpRef, types: &[SlotType], args: &[String]) -> Option<(SlotType, String)>;

    /// 通道读端的 WGSL 标识符。
    ///
    /// 生成函数的签名恒为 `fn eval(p: Params, index: u32) -> T`：
    /// - `Frame` 通道通常映射到 `p.<字段>`（后端负责把字段登记进 [`WgslParam`]）；
    /// - `Element` 通道可用 `index` 派生（如 `f32(index)`）。
    ///
    /// 返回 `None` ⇒ 该通道不能进 shader（显式失败）。
    fn channel(&self, channel: &crate::expr::ChannelRef) -> Option<(SlotType, String)>;

    /// 后端自带的**内建通道**（对象 / 片元属性：中心、尺寸、像素位置…）。
    ///
    /// 与 [`Self::channel`] 的区别：内建**不进 `Params`**（不是 uniform），而是映射到
    /// 着色器里的一个只读标识符（由后端在入口处赋值）。返回 `None` ⇒ 按普通通道处理。
    fn builtin(&self, _channel: &crate::expr::ChannelRef) -> Option<(SlotType, String)> {
        None
    }

    /// 后端自带的前导（辅助函数等）。默认空。
    fn prelude(&self) -> &str {
        ""
    }
}

/// codegen 失败（全部显式，不静默代入 0）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodegenError {
    #[error("表达式本身非法：{0}")]
    Invalid(#[from] ExprError),
    #[error("通道 `{channel}` 不能进 shader（宿主状态 / 类型未知 / 后端不支持）")]
    UnsupportedChannel { channel: String },
    #[error("节点 `{node}` 不能进 shader（离散层 / 宿主副作用不在 2 阶范围内）")]
    UnsupportedNode { node: &'static str },
    #[error("算子 `{op}`（实参 {args:?}）没有 WGSL 生成规则")]
    UnsupportedOp { op: String, args: Vec<SlotType> },
    #[error("字面量 `{value}` 不能进 WGSL（文本 / 非有限浮点 / 越界整数）")]
    UnsupportedLiteral { value: String },
    #[error("`Ref` 指向尚未生成的节点（只支持后序 / 后向引用）")]
    ForwardReference,
    #[error("`Select` 两支类型不一致：{then:?} vs {otherwise:?}")]
    SelectTypeMismatch {
        then: SlotType,
        otherwise: SlotType,
    },
}

impl CodegenError {
    fn channel(channel: &crate::expr::ChannelRef) -> Self {
        Self::UnsupportedChannel {
            channel: channel.qualified(),
        }
    }
}

/// 把表达式编译成 WGSL 纯函数（确定性）。
pub fn emit_wgsl(expr: &Expr, backend: &dyn WgslBackend) -> Result<WgslFunction, CodegenError> {
    expr.validate()?;
    let mut emitter = Emitter {
        backend,
        params: BTreeMap::new(),
        refs: BTreeMap::new(),
        lines: Vec::new(),
        next_var: 0,
    };
    let (root, ret) = emitter.value(expr)?;
    let mut source = String::new();
    source.push_str(backend.prelude());
    if !source.is_empty() && !source.ends_with('\n') {
        source.push('\n');
    }
    source.push_str("struct Params {\n");
    if emitter.params.is_empty() {
        // WGSL 不允许空 struct：没有通道读端时留一个恒不读的占位字段。
        source.push_str("    _unused: u32,\n");
    }
    for param in emitter.params.values() {
        source.push_str(&format!("    {}: {},\n", param.name, wgsl_type(param.ty)));
    }
    source.push_str("}\n\n");
    source.push_str(&format!(
        "fn eval(p: Params, index: u32) -> {} {{\n",
        wgsl_type(ret)
    ));
    for line in &emitter.lines {
        source.push_str("    ");
        source.push_str(line);
        source.push('\n');
    }
    source.push_str(&format!("    return {root};\n}}\n"));
    Ok(WgslFunction {
        params: emitter.params.into_values().collect(),
        source,
        ret,
    })
}

/// 槽类型 → WGSL 类型（`F64` 在 shader 里是 `f32`）。
pub fn wgsl_type(ty: SlotType) -> &'static str {
    match ty {
        SlotType::F64 => "f32",
        SlotType::I64 => "i32",
        SlotType::Bool => "bool",
        SlotType::Vec2 => "vec2<f32>",
        SlotType::Vec3 => "vec3<f32>",
        SlotType::Vec4 => "vec4<f32>",
    }
}

struct Emitter<'a> {
    backend: &'a dyn WgslBackend,
    params: BTreeMap<String, WgslParam>,
    refs: BTreeMap<Id, (String, SlotType)>,
    lines: Vec<String>,
    next_var: usize,
}

impl Emitter<'_> {
    fn value(&mut self, expr: &Expr) -> Result<(String, SlotType), CodegenError> {
        if let Some(id) = &expr.id
            && let Some((var, ty)) = self.refs.get(id)
        {
            return Ok((var.clone(), *ty));
        }
        let (text, ty) = match &expr.node {
            Node::Pure { value } => literal(value)?,
            Node::Channel { channel } => {
                if channel.class() == ChannelClass::HostState {
                    return Err(CodegenError::channel(channel));
                }
                // **阶段 2**：内建通道（对象 / 片元属性）→ 着色器标识符，不进 `Params`。
                if let Some((ty, name)) = self.backend.builtin(channel) {
                    (name, ty)
                } else {
                    let (ty, name) = self
                        .backend
                        .channel(channel)
                        .ok_or_else(|| CodegenError::channel(channel))?;
                    self.params.insert(
                        name.clone(),
                        WgslParam {
                            name: name.clone(),
                            ty,
                        },
                    );
                    (format!("p.{name}"), ty)
                }
            }
            Node::Call { op, args } => {
                let mut rendered = Vec::with_capacity(args.len());
                let mut types = Vec::with_capacity(args.len());
                for arg in args {
                    let (text, ty) = self.value(arg)?;
                    rendered.push(text);
                    types.push(ty);
                }
                let (ret, text) =
                    self.backend
                        .op(op, &types, &rendered)
                        .ok_or_else(|| CodegenError::UnsupportedOp {
                            op: op.qualified(),
                            args: types,
                        })?;
                (text, ret)
            }
            Node::Select {
                cond,
                then,
                r#else,
            } => {
                let (cond, cond_ty) = self.value(cond)?;
                if cond_ty != SlotType::Bool {
                    return Err(CodegenError::UnsupportedOp {
                        op: "select.cond".to_string(),
                        args: vec![cond_ty],
                    });
                }
                let (then_text, then_ty) = self.value(then)?;
                let (else_text, else_ty) = self.value(r#else)?;
                if then_ty != else_ty {
                    return Err(CodegenError::SelectTypeMismatch {
                        then: then_ty,
                        otherwise: else_ty,
                    });
                }
                (
                    format!("select({then_text}, {else_text}, {cond})"),
                    then_ty,
                )
            }
            Node::Ref { to } => {
                return self
                    .refs
                    .get(to)
                    .cloned()
                    .ok_or(CodegenError::ForwardReference);
            }
            Node::Read { .. } => return Err(unsupported("read")),
            Node::Method { .. } => return Err(unsupported("method")),
            Node::NatRec { .. } => return Err(unsupported("nat_rec")),
            Node::RecPrev => return Err(unsupported("rec_prev")),
            Node::RecIndex => return Err(unsupported("rec_index")),
            Node::Exists { .. } => return Err(unsupported("exists")),
        };
        let var = format!("v{}", self.next_var);
        self.next_var += 1;
        self.lines
            .push(format!("let {var}: {} = {text};", wgsl_type(ty)));
        if let Some(id) = &expr.id {
            self.refs.insert(id.clone(), (var.clone(), ty));
        }
        Ok((var, ty))
    }
}

fn unsupported(node: &'static str) -> CodegenError {
    CodegenError::UnsupportedNode { node }
}

/// 字面量 → （文本，槽类型）。浮点统一落 `f32`（payload 外，见模块头）。
fn literal(value: &Literal) -> Result<(String, SlotType), CodegenError> {
    let text = match value {
        Literal::F32(v) => finite_f32(f64::from(*v))?,
        Literal::F64(v) => finite_f32(*v)?,
        Literal::I32(v) => format!("{v}i"),
        Literal::I64(v) => {
            let narrowed = i32::try_from(*v).map_err(|_| CodegenError::UnsupportedLiteral {
                value: v.to_string(),
            })?;
            format!("{narrowed}i")
        }
        Literal::Bool(v) => v.to_string(),
        Literal::Vec2([x, y]) => format!(
            "vec2<f32>({}, {})",
            finite_f32(f64::from(*x))?,
            finite_f32(f64::from(*y))?
        ),
        Literal::Vec3([x, y, z]) => format!(
            "vec3<f32>({}, {}, {})",
            finite_f32(f64::from(*x))?,
            finite_f32(f64::from(*y))?,
            finite_f32(f64::from(*z))?
        ),
        Literal::Vec4([x, y, z, w]) => format!(
            "vec4<f32>({}, {}, {}, {})",
            finite_f32(f64::from(*x))?,
            finite_f32(f64::from(*y))?,
            finite_f32(f64::from(*z))?,
            finite_f32(f64::from(*w))?
        ),
        Literal::Text(text) => {
            return Err(CodegenError::UnsupportedLiteral {
                value: format!("text:{text}"),
            });
        }
    };
    Ok((text, slot_of(value)))
}

/// 最短往返表示的 f32 字面量（`Debug` 保证 round-trip，且与平台无关）。
fn finite_f32(value: f64) -> Result<String, CodegenError> {
    let narrowed = value as f32;
    if !narrowed.is_finite() {
        return Err(CodegenError::UnsupportedLiteral {
            value: value.to_string(),
        });
    }
    Ok(format!("{narrowed:?}"))
}

fn slot_of(value: &Literal) -> SlotType {
    match value {
        Literal::F32(_) | Literal::F64(_) => SlotType::F64,
        Literal::I32(_) | Literal::I64(_) => SlotType::I64,
        Literal::Bool(_) => SlotType::Bool,
        Literal::Vec2(_) => SlotType::Vec2,
        Literal::Vec3(_) => SlotType::Vec3,
        Literal::Vec4(_) => SlotType::Vec4,
        Literal::Text(_) => SlotType::F64, // 不可达：`literal` 已显式拒绝
    }
}

/// `ChannelType` → 槽类型；不能进 WGSL 的组合（`Bytes` / `Unknown`）⇒ `None`。
pub fn slot_of_channel(ty: ChannelType) -> Option<SlotType> {
    match ty {
        ChannelType::Number => Some(SlotType::F64),
        ChannelType::I64 => Some(SlotType::I64),
        ChannelType::Bool => Some(SlotType::Bool),
        ChannelType::Vec2 => Some(SlotType::Vec2),
        ChannelType::Vec3 => Some(SlotType::Vec3),
        ChannelType::Vec4 => Some(SlotType::Vec4),
        ChannelType::Bytes | ChannelType::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::{ChannelRef, OpRef};

    /// 最小后端：只认 `sin` 与 `add`，通道 `time` → `p.time`。
    struct TestBackend;

    impl WgslBackend for TestBackend {
        fn op(
            &self,
            op: &OpRef,
            types: &[SlotType],
            args: &[String],
        ) -> Option<(SlotType, String)> {
            match (op.name.as_str(), types) {
                ("sin", [SlotType::F64]) => Some((SlotType::F64, format!("sin({})", args[0]))),
                ("add", [SlotType::F64, SlotType::F64]) => {
                    Some((SlotType::F64, format!("({} + {})", args[0], args[1])))
                }
                _ => None,
            }
        }

        fn channel(&self, channel: &crate::expr::ChannelRef) -> Option<(SlotType, String)> {
            (channel.qualified() == "time").then(|| (SlotType::F64, "time".to_string()))
        }
    }

    fn math(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    fn sample() -> Expr {
        let time = Expr::channel(ChannelRef::builtin("time"));
        let sin = Expr::call(math("sin"), [time.clone()]);
        let two = Expr::pure(2.0);
        Expr::call(math("add"), [sin, two])
    }

    #[test]
    fn emission_is_byte_identical_across_runs() {
        let first = emit_wgsl(&sample(), &TestBackend).expect("可生成");
        let second = emit_wgsl(&sample(), &TestBackend).expect("可生成");
        assert_eq!(first, second, "同 Expr ⇒ 同 WGSL 逐字节");
        assert_eq!(first.ret, SlotType::F64);
        assert_eq!(
            first.params,
            vec![WgslParam {
                name: "time".to_string(),
                ty: SlotType::F64
            }]
        );
        // 结构钉死（改动会让基线漂移，必须显式 diff）。
        assert_eq!(
            first.source,
            "struct Params {\n    time: f32,\n}\n\n\
             fn eval(p: Params, index: u32) -> f32 {\n    \
             let v0: f32 = p.time;\n    \
             let v1: f32 = sin(v0);\n    \
             let v2: f32 = 2.0;\n    \
             let v3: f32 = (v1 + v2);\n    \
             return v3;\n}\n"
        );
    }

    #[test]
    fn forward_references_are_rejected() {
        // `Ref` 在定义之前出现：validate 允许（DAG 只看有无定义），emitter 只做后序 ⇒ 显式失败。
        let defined = Expr::call(math("sin"), [Expr::pure(1.0)]).with_id("t");
        let expr = Expr::call(
            math("add"),
            [Expr::reference("t"), defined],
        );
        let error = emit_wgsl(&expr, &TestBackend).unwrap_err();
        assert!(matches!(error, CodegenError::ForwardReference), "{error}");
    }

    #[test]
    fn host_state_channels_are_rejected() {
        let channel = Expr::channel(ChannelRef::new("zela.scene", "Circle.radius"));
        let error = emit_wgsl(&channel, &TestBackend).unwrap_err();
        assert!(
            matches!(error, CodegenError::UnsupportedChannel { .. }),
            "{error}"
        );
    }

    #[test]
    fn discrete_nodes_are_rejected() {
        let expr = Expr::nat_rec(Expr::pure(3.0), Expr::pure(0.0), Expr::pure(1.0));
        let error = emit_wgsl(&expr, &TestBackend).unwrap_err();
        assert!(
            matches!(error, CodegenError::UnsupportedNode { node: "nat_rec" }),
            "{error}"
        );
    }

    #[test]
    fn unknown_operators_fail_explicitly() {
        let expr = Expr::call(math("cos"), [Expr::pure(0.5)]);
        let error = emit_wgsl(&expr, &TestBackend).unwrap_err();
        assert!(matches!(error, CodegenError::UnsupportedOp { .. }), "{error}");
    }

    #[test]
    fn select_requires_bool_and_matching_branches() {
        let ok = Expr::select(
            Expr::pure(true),
            Expr::call(math("sin"), [Expr::pure(1.0)]),
            Expr::pure(0.5),
        );
        let emitted = emit_wgsl(&ok, &TestBackend).expect("同型两支可生成");
        assert!(emitted.source.contains("select("), "{}", emitted.source);

        let mismatched = Expr::select(
            Expr::pure(true),
            Expr::pure(1.0),
            Expr::pure(true),
        );
        let error = emit_wgsl(&mismatched, &TestBackend).unwrap_err();
        assert!(
            matches!(error, CodegenError::SelectTypeMismatch { .. }),
            "{error}"
        );
    }

    #[test]
    fn non_finite_literals_are_rejected() {
        let expr = Expr::pure(f64::INFINITY);
        let error = emit_wgsl(&expr, &TestBackend).unwrap_err();
        assert!(
            matches!(error, CodegenError::UnsupportedLiteral { .. }),
            "{error}"
        );
    }

    #[test]
    fn channel_types_map_to_slots() {
        assert_eq!(slot_of_channel(ChannelType::Number), Some(SlotType::F64));
        assert_eq!(slot_of_channel(ChannelType::Vec3), Some(SlotType::Vec3));
        // **BT-1**：`Vec4` 现在能进 WGSL（此前是 `None`）。
        assert_eq!(slot_of_channel(ChannelType::Vec4), Some(SlotType::Vec4));
        assert_eq!(slot_of_channel(ChannelType::Bytes), None);
        assert_eq!(slot_of_channel(ChannelType::Unknown), None);
    }
}
