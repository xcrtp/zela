//! **单态化求值**（typed fast path）：编译期定类型、按类型分槽、直接调函数指针。
//!
//! # 为什么另起一条路径
//!
//! [`crate::program::CompiledExpr`] 的值表是 `Vec<Value>`：每步一次装箱值、每个算子
//! 一次 `Vec<Value>` 参数（clone）、一次符号表查找，算子里还要用 `as_f64` / `as_vec2`
//! 做**运行期类型判断**。这些都是"类型在运行期才知道"的代价。
//!
//! 本模块把类型推到**编译期**（装配期）：
//!
//! 1. **单态化**：`(算子, 实参类型)` 由 [`OpResolver`] 解析成具体的 [`TypedOp`]
//!    （函数指针）—— 运行期不再查符号表、不再按名字分派；
//! 2. **按类型分槽**：[`Slots`] 是 SoA（`Vec<f64>` / `Vec<[f32;2]>` / …），
//!    每步的槽下标在编译期分配 ⇒ 求值时没有 `Value`、没有参数 `Vec` 分配；
//! 3. **类型转换下沉**：算子只拿**精确类型**的实参；"这个实参该是什么类型、
//!    要不要转换"由解析器按**注册的签名**在编译期决定，算子实现里不再有 `as_*`。
//!
//! # 与动态路径的关系
//!
//! typed 路径是**可选快路径**：任何一环解析不了（未知算子 / 不支持的节点 /
//! 通道类型未知）⇒ [`compile`] 返回 `None`，调用方回落到 `CompiledExpr`。
//! 动态路径（`CompiledExpr` / 参考求值器）仍是**语义基准**，typed 必须与它逐位一致
//! （`ze-func` 里有对拍用例）。

use std::collections::BTreeMap;

use ze_type::ScalarKind;

use crate::expr::{ChannelRef, ChannelType, Expr, Literal, Node, OpRef};
use crate::program::{EvalError, Mono, Value};

/// 槽类型：**编译期确定、运行时不再变**。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SlotType {
    F64,
    I64,
    Bool,
    Vec2,
    Vec3,
    /// **BT-1**：四维向量（`rgba`）。此前 `Vec4` 值只能走慢路径。
    Vec4,
}

impl SlotType {
    /// 运行期值 → 槽类型（`Bytes` / `Text` / `Iter` / `Stream` ⇒ `None`，走慢路径）。
    pub fn of(value: &Value) -> Option<Self> {
        match value {
            Value::Scalar(scalar) => match (scalar.ty(), scalar.dim()) {
                (ScalarKind::F64, 1) => Some(Self::F64),
                (ScalarKind::I64, 1) => Some(Self::I64),
                (ScalarKind::F32, 2) => Some(Self::Vec2),
                (ScalarKind::F32, 3) => Some(Self::Vec3),
                (ScalarKind::F32, 4) => Some(Self::Vec4),
                _ => None,
            },
            Value::Bool(_) => Some(Self::Bool),
            _ => None,
        }
    }

    /// 通道声明类型 → 槽类型（`Unknown` / `Bytes` ⇒ `None`，走慢路径）。
    pub fn from_channel(ty: ChannelType) -> Option<Self> {
        match ty {
            ChannelType::Number => Some(Self::F64),
            ChannelType::I64 => Some(Self::I64),
            ChannelType::Bool => Some(Self::Bool),
            ChannelType::Vec2 => Some(Self::Vec2),
            ChannelType::Vec3 => Some(Self::Vec3),
            ChannelType::Vec4 => Some(Self::Vec4),
            ChannelType::Unknown | ChannelType::Bytes => None,
        }
    }

    /// 诊断用的短名。
    pub fn name(self) -> &'static str {
        match self {
            Self::F64 => "scalar",
            Self::I64 => "i64",
            Self::Bool => "bool",
            Self::Vec2 => "vec2",
            Self::Vec3 => "vec3",
            Self::Vec4 => "vec4",
        }
    }
}

/// 槽引用：`(类型, 同类槽下标)`。编译期分配，运行期只读。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRef {
    pub ty: SlotType,
    pub index: u32,
}

/// 每种类型各需要多少个槽（编译期算出的布局）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlotLayout {
    pub f64: u32,
    pub i64: u32,
    pub bool: u32,
    pub vec2: u32,
    pub vec3: u32,
    pub vec4: u32,
}

/// 按类型分区的值池（SoA）。
///
/// 求值时**预分配一次**、按下标读写；没有 `Value` 装箱，也没有参数 `Vec`。
#[derive(Clone, Debug, Default)]
pub struct Slots {
    f64: Vec<f64>,
    i64: Vec<i64>,
    bools: Vec<bool>,
    vec2: Vec<[f32; 2]>,
    vec3: Vec<[f32; 3]>,
    vec4: Vec<[f32; 4]>,
}

impl Slots {
    fn new(layout: SlotLayout) -> Self {
        Self {
            f64: vec![0.0; layout.f64 as usize],
            i64: vec![0; layout.i64 as usize],
            bools: vec![false; layout.bool as usize],
            vec2: vec![[0.0; 2]; layout.vec2 as usize],
            vec3: vec![[0.0; 3]; layout.vec3 as usize],
            vec4: vec![[0.0; 4]; layout.vec4 as usize],
        }
    }

    fn to_value(&self, root: SlotRef) -> Value {
        match root.ty {
            SlotType::F64 => Value::f64(self.f64[root.index as usize]),
            SlotType::I64 => Value::i64(self.i64[root.index as usize]),
            SlotType::Bool => Value::Bool(self.bools[root.index as usize]),
            SlotType::Vec2 => Value::vec2(self.vec2[root.index as usize]),
            SlotType::Vec3 => Value::vec3(self.vec3[root.index as usize]),
            SlotType::Vec4 => Value::vec4(self.vec4[root.index as usize]),
        }
    }
}

/// 单态算子：**只做一种类型组合**，实参已由编译期保证。
///
/// 变体名就是类型签名（`F64xF64ToF64` = `(F64, F64) -> F64`），
/// 运行期只有一次枚举分派，然后就是裸函数指针调用。
#[derive(Clone, Copy, Debug)]
pub enum TypedOp {
    F64ToF64(fn(f64) -> f64),
    F64xF64ToF64(fn(f64, f64) -> f64),
    F64x3ToF64(fn(f64, f64, f64) -> f64),
    /// 变参（`sum`）：实参先收集成切片（每帧一次小分配，仍远小于 `Vec<Value>`）。
    F64sToF64(fn(&[f64]) -> f64),
    Vec2sToVec2(fn(&[[f32; 2]]) -> [f32; 2]),
    Vec3sToVec3(fn(&[[f32; 3]]) -> [f32; 3]),
    F64xF64ToBool(fn(f64, f64) -> bool),
    BoolToBool(fn(bool) -> bool),
    BoolxBoolToBool(fn(bool, bool) -> bool),
    Vec2ToVec2(fn([f32; 2]) -> [f32; 2]),
    Vec2xVec2ToVec2(fn([f32; 2], [f32; 2]) -> [f32; 2]),
    Vec3ToVec3(fn([f32; 3]) -> [f32; 3]),
    Vec3xVec3ToVec3(fn([f32; 3], [f32; 3]) -> [f32; 3]),
    Vec2xF64ToVec2(fn([f32; 2], f64) -> [f32; 2]),
    Vec3xF64ToVec3(fn([f32; 3], f64) -> [f32; 3]),
    F64xVec2ToVec2(fn(f64, [f32; 2]) -> [f32; 2]),
    F64xVec3ToVec3(fn(f64, [f32; 3]) -> [f32; 3]),
    Vec2ToF64(fn([f32; 2]) -> f64),
    Vec3ToF64(fn([f32; 3]) -> f64),
    /// `distance`：两个同维向量 → 标量。
    Vec2xVec2ToF64(fn([f32; 2], [f32; 2]) -> f64),
    Vec3xVec3ToF64(fn([f32; 3], [f32; 3]) -> f64),
    F64xF64ToVec2(fn(f64, f64) -> [f32; 2]),
    F64x3ToVec3(fn(f64, f64, f64) -> [f32; 3]),
    F64ToVec3(fn(f64) -> [f32; 3]),
    Vec2xF64ToVec3(fn([f32; 2], f64) -> [f32; 3]),
    Vec3ToVec2(fn([f32; 3]) -> [f32; 2]),
    Vec3xVec3xF64ToVec3(fn([f32; 3], [f32; 3], f64) -> [f32; 3]),
    /// `mix`：端点短路 + f32 域插值（与动态路径同一实现）。
    MixF64(fn(f64, f64, f64) -> f64),
    MixVec2(fn([f32; 2], [f32; 2], f64) -> [f32; 2]),
    MixVec3(fn([f32; 3], [f32; 3], f64) -> [f32; 3]),
    /// `faceforward`：三个同维向量。
    Vec2x3ToVec2(fn([f32; 2], [f32; 2], [f32; 2]) -> [f32; 2]),
    Vec3x3ToVec3(fn([f32; 3], [f32; 3], [f32; 3]) -> [f32; 3]),
    /// **BT-1**：`vec4(r, g, b, a)` 构造器。
    F64x4ToVec4(fn(f64, f64, f64, f64) -> [f32; 4]),
    /// **BT-1**：`w(v)` 取第四分量（与 `x` / `y` / `z` 同形）。
    Vec4ToF64(fn([f32; 4]) -> f64),
}

impl TypedOp {
    fn eval(&self, slots: &mut Slots, args: &[SlotRef], out: SlotRef) {
        // 实参类型由 `OpResolver` 在编译期保证；这里的索引/类型不做运行期校验。
        match self {
            Self::F64ToF64(f) => {
                slots.f64[out.index as usize] = f(slots.f64[args[0].index as usize]);
            }
            Self::F64xF64ToF64(f) => {
                slots.f64[out.index as usize] =
                    f(slots.f64[args[0].index as usize], slots.f64[args[1].index as usize]);
            }
            Self::F64x3ToF64(f) => {
                slots.f64[out.index as usize] = f(
                    slots.f64[args[0].index as usize],
                    slots.f64[args[1].index as usize],
                    slots.f64[args[2].index as usize],
                );
            }
            Self::F64sToF64(f) => {
                let values: Vec<f64> = args
                    .iter()
                    .map(|arg| slots.f64[arg.index as usize])
                    .collect();
                slots.f64[out.index as usize] = f(&values);
            }
            Self::Vec2sToVec2(f) => {
                let values: Vec<[f32; 2]> = args
                    .iter()
                    .map(|arg| slots.vec2[arg.index as usize])
                    .collect();
                slots.vec2[out.index as usize] = f(&values);
            }
            Self::Vec3sToVec3(f) => {
                let values: Vec<[f32; 3]> = args
                    .iter()
                    .map(|arg| slots.vec3[arg.index as usize])
                    .collect();
                slots.vec3[out.index as usize] = f(&values);
            }
            Self::F64xF64ToBool(f) => {
                slots.bools[out.index as usize] =
                    f(slots.f64[args[0].index as usize], slots.f64[args[1].index as usize]);
            }
            Self::BoolToBool(f) => {
                slots.bools[out.index as usize] = f(slots.bools[args[0].index as usize]);
            }
            Self::BoolxBoolToBool(f) => {
                slots.bools[out.index as usize] = f(
                    slots.bools[args[0].index as usize],
                    slots.bools[args[1].index as usize],
                );
            }
            Self::Vec2ToVec2(f) => {
                slots.vec2[out.index as usize] = f(slots.vec2[args[0].index as usize]);
            }
            Self::Vec2xVec2ToVec2(f) => {
                slots.vec2[out.index as usize] = f(
                    slots.vec2[args[0].index as usize],
                    slots.vec2[args[1].index as usize],
                );
            }
            Self::Vec3ToVec3(f) => {
                slots.vec3[out.index as usize] = f(slots.vec3[args[0].index as usize]);
            }
            Self::Vec3xVec3ToVec3(f) => {
                slots.vec3[out.index as usize] = f(
                    slots.vec3[args[0].index as usize],
                    slots.vec3[args[1].index as usize],
                );
            }
            Self::Vec2xF64ToVec2(f) => {
                slots.vec2[out.index as usize] = f(
                    slots.vec2[args[0].index as usize],
                    slots.f64[args[1].index as usize],
                );
            }
            Self::Vec3xF64ToVec3(f) => {
                slots.vec3[out.index as usize] = f(
                    slots.vec3[args[0].index as usize],
                    slots.f64[args[1].index as usize],
                );
            }
            Self::F64xVec2ToVec2(f) => {
                slots.vec2[out.index as usize] = f(
                    slots.f64[args[0].index as usize],
                    slots.vec2[args[1].index as usize],
                );
            }
            Self::F64xVec3ToVec3(f) => {
                slots.vec3[out.index as usize] = f(
                    slots.f64[args[0].index as usize],
                    slots.vec3[args[1].index as usize],
                );
            }
            Self::Vec2ToF64(f) => {
                slots.f64[out.index as usize] = f(slots.vec2[args[0].index as usize]);
            }
            Self::Vec3ToF64(f) => {
                slots.f64[out.index as usize] = f(slots.vec3[args[0].index as usize]);
            }
            Self::Vec2xVec2ToF64(f) => {
                slots.f64[out.index as usize] = f(
                    slots.vec2[args[0].index as usize],
                    slots.vec2[args[1].index as usize],
                );
            }
            Self::Vec3xVec3ToF64(f) => {
                slots.f64[out.index as usize] = f(
                    slots.vec3[args[0].index as usize],
                    slots.vec3[args[1].index as usize],
                );
            }
            Self::F64xF64ToVec2(f) => {
                slots.vec2[out.index as usize] =
                    f(slots.f64[args[0].index as usize], slots.f64[args[1].index as usize]);
            }
            Self::F64x3ToVec3(f) => {
                slots.vec3[out.index as usize] = f(
                    slots.f64[args[0].index as usize],
                    slots.f64[args[1].index as usize],
                    slots.f64[args[2].index as usize],
                );
            }
            Self::F64ToVec3(f) => {
                slots.vec3[out.index as usize] = f(slots.f64[args[0].index as usize]);
            }
            Self::Vec2xF64ToVec3(f) => {
                slots.vec3[out.index as usize] = f(
                    slots.vec2[args[0].index as usize],
                    slots.f64[args[1].index as usize],
                );
            }
            Self::Vec3ToVec2(f) => {
                slots.vec2[out.index as usize] = f(slots.vec3[args[0].index as usize]);
            }
            Self::Vec3xVec3xF64ToVec3(f) => {
                slots.vec3[out.index as usize] = f(
                    slots.vec3[args[0].index as usize],
                    slots.vec3[args[1].index as usize],
                    slots.f64[args[2].index as usize],
                );
            }
            Self::MixF64(f) => {
                slots.f64[out.index as usize] = f(
                    slots.f64[args[0].index as usize],
                    slots.f64[args[1].index as usize],
                    slots.f64[args[2].index as usize],
                );
            }
            Self::MixVec2(f) => {
                slots.vec2[out.index as usize] = f(
                    slots.vec2[args[0].index as usize],
                    slots.vec2[args[1].index as usize],
                    slots.f64[args[2].index as usize],
                );
            }
            Self::MixVec3(f) => {
                slots.vec3[out.index as usize] = f(
                    slots.vec3[args[0].index as usize],
                    slots.vec3[args[1].index as usize],
                    slots.f64[args[2].index as usize],
                );
            }
            Self::Vec2x3ToVec2(f) => {
                slots.vec2[out.index as usize] = f(
                    slots.vec2[args[0].index as usize],
                    slots.vec2[args[1].index as usize],
                    slots.vec2[args[2].index as usize],
                );
            }
            Self::Vec3x3ToVec3(f) => {
                slots.vec3[out.index as usize] = f(
                    slots.vec3[args[0].index as usize],
                    slots.vec3[args[1].index as usize],
                    slots.vec3[args[2].index as usize],
                );
            }
            Self::F64x4ToVec4(f) => {
                slots.vec4[out.index as usize] = f(
                    slots.f64[args[0].index as usize],
                    slots.f64[args[1].index as usize],
                    slots.f64[args[2].index as usize],
                    slots.f64[args[3].index as usize],
                );
            }
            Self::Vec4ToF64(f) => {
                slots.f64[out.index as usize] = f(slots.vec4[args[0].index as usize]);
            }
        }
    }
}

/// 一次单态解析的结果：实现 + **返回类型**。
#[derive(Clone, Copy, Debug)]
pub struct TypedCall {
    pub op: TypedOp,
    pub ret: SlotType,
}

/// **单态解析器**：类型推导 + 实例选择的唯一接缝。
///
/// 实现方是算子库（它持有注册的签名）：给定 `(算子, 实参槽类型)`，要么给出
/// 精确类型的实现，要么 `None`（整棵表达式回落动态路径）。
/// **类型转换在这一层完成**——算子里不再出现"这个实参大概是数值吧"的判断。
pub trait OpResolver {
    fn resolve(&self, op: &OpRef, args: &[SlotType]) -> Option<TypedCall>;
}

/// 编译后的一步（槽下标已分配）。
#[derive(Clone, Debug)]
pub enum TypedStep {
    ConstF64(SlotRef, f64),
    ConstI64(SlotRef, i64),
    ConstBool(SlotRef, bool),
    ConstVec2(SlotRef, [f32; 2]),
    ConstVec3(SlotRef, [f32; 3]),
    /// **BT-1**：四维字面量（`Literal::Vec4`）。
    ConstVec4(SlotRef, [f32; 4]),
    Channel(SlotRef, ChannelRef),
    Call {
        out: SlotRef,
        op: TypedOp,
        args: Vec<SlotRef>,
    },
}

/// 单态化后的程序：后序扫描、槽下标固定、算子指针已解析。
#[derive(Clone, Debug)]
pub struct TypedProgram {
    steps: Vec<TypedStep>,
    root: SlotRef,
    layout: SlotLayout,
}

impl TypedProgram {
    /// 尝试把表达式编译成单态程序；任何一环不支持 ⇒ `None`（调用方回落）。
    ///
    /// 不支持（本阶段）：`Read` / `Method` / `Select` / `NatRec` 等非逐点节点、
    /// 类型未知的通道、`Bytes` / `Text` / `Iter` / `Stream` 值。
    /// **BT-1** 起 `Vec4` 也走快路径。
    pub fn compile(expr: &Expr, resolver: &dyn OpResolver) -> Option<Self> {
        let defs = crate::expr::definitions(expr);
        let mut builder = Builder {
            defs,
            memo: BTreeMap::new(),
            layout: SlotLayout::default(),
            steps: Vec::new(),
            resolver,
        };
        let root = builder.node(expr)?;
        Some(Self {
            steps: builder.steps,
            root,
            layout: builder.layout,
        })
    }

    /// 求值一次（槽池按布局一次性预分配）。
    pub fn eval(&self, mono: &dyn Mono) -> Result<Value, EvalError> {
        let mut slots = Slots::new(self.layout);
        for step in &self.steps {
            match step {
                TypedStep::ConstF64(out, value) => slots.f64[out.index as usize] = *value,
                TypedStep::ConstI64(out, value) => slots.i64[out.index as usize] = *value,
                TypedStep::ConstBool(out, value) => slots.bools[out.index as usize] = *value,
                TypedStep::ConstVec2(out, value) => slots.vec2[out.index as usize] = *value,
                TypedStep::ConstVec3(out, value) => slots.vec3[out.index as usize] = *value,
                TypedStep::ConstVec4(out, value) => slots.vec4[out.index as usize] = *value,
                TypedStep::Channel(out, channel) => {
                    let value = mono.read(channel)?;
                    store(&mut slots, *out, channel, &value)?;
                }
                TypedStep::Call { out, op, args } => op.eval(&mut slots, args, *out),
            }
        }
        Ok(slots.to_value(self.root))
    }

    /// 步数（共享节点只计一次）。
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// 程序（只读；诊断用）。
    pub fn steps(&self) -> &[TypedStep] {
        &self.steps
    }

    /// 根槽的静态类型。
    pub fn root_type(&self) -> SlotType {
        self.root.ty
    }
}

/// 通道值 → 槽（类型不符 ⇒ 显式错误，不静默截断）。
fn store(
    slots: &mut Slots,
    out: SlotRef,
    channel: &ChannelRef,
    value: &Value,
) -> Result<(), EvalError> {
    let mismatched = |actual: &Value| EvalError::ChannelTypeMismatch {
        channel: channel.clone(),
        expected: out.ty.name(),
        actual: actual.kind(),
    };
    match out.ty {
        SlotType::F64 => match value.as_f64() {
            Some(value) => slots.f64[out.index as usize] = value,
            None => return Err(mismatched(value)),
        },
        SlotType::I64 => match value.as_i64() {
            Some(value) => slots.i64[out.index as usize] = value,
            None => return Err(mismatched(value)),
        },
        SlotType::Bool => match value.as_bool() {
            Some(value) => slots.bools[out.index as usize] = value,
            None => return Err(mismatched(value)),
        },
        SlotType::Vec2 => match value.as_vec2() {
            Some(value) => slots.vec2[out.index as usize] = value,
            None => return Err(mismatched(value)),
        },
        SlotType::Vec3 => match value.as_vec3() {
            Some(value) => slots.vec3[out.index as usize] = value,
            None => return Err(mismatched(value)),
        },
        SlotType::Vec4 => match value.as_vec4() {
            Some(value) => slots.vec4[out.index as usize] = value,
            None => return Err(mismatched(value)),
        },
    }
    Ok(())
}

struct Builder<'a> {
    defs: BTreeMap<&'a str, &'a Expr>,
    memo: BTreeMap<&'a str, SlotRef>,
    layout: SlotLayout,
    steps: Vec<TypedStep>,
    resolver: &'a dyn OpResolver,
}

impl<'a> Builder<'a> {
    fn alloc(&mut self, ty: SlotType) -> SlotRef {
        let counter = match ty {
            SlotType::F64 => &mut self.layout.f64,
            SlotType::I64 => &mut self.layout.i64,
            SlotType::Bool => &mut self.layout.bool,
            SlotType::Vec2 => &mut self.layout.vec2,
            SlotType::Vec3 => &mut self.layout.vec3,
            SlotType::Vec4 => &mut self.layout.vec4,
        };
        let index = *counter;
        *counter += 1;
        SlotRef { ty, index }
    }

    fn node(&mut self, expr: &'a Expr) -> Option<SlotRef> {
        if let Some(id) = expr.id.as_deref() {
            if let Some(slot) = self.memo.get(id) {
                return Some(*slot);
            }
        }
        let slot = match &expr.node {
            Node::Pure { value } => self.constant(value)?,
            Node::Channel { channel } => {
                let ty = SlotType::from_channel(channel.ty)?;
                let out = self.alloc(ty);
                self.steps.push(TypedStep::Channel(out, channel.clone()));
                out
            }
            Node::Call { op, args } => {
                let mut refs = Vec::with_capacity(args.len());
                let mut types = Vec::with_capacity(args.len());
                for arg in args {
                    let reference = self.node(arg)?;
                    types.push(reference.ty);
                    refs.push(reference);
                }
                let call = self.resolver.resolve(op, &types)?;
                let out = self.alloc(call.ret);
                self.steps.push(TypedStep::Call {
                    out,
                    op: call.op,
                    args: refs,
                });
                out
            }
            Node::Ref { to } => {
                let target = *self.defs.get(to.as_str())?;
                self.node(target)?
            }
            // 其余节点本阶段一律回落（`Select` 的惰性 / `Read` 的对象 / `NatRec` 的递归
            // 都有自己的槽语义，留给后续批次，不在快路径里半做）。
            Node::Read { .. }
            | Node::Method { .. }
            | Node::Select { .. }
            | Node::NatRec { .. }
            | Node::RecPrev
            | Node::RecIndex
            | Node::Exists { .. } => return None,
        };
        if let Some(id) = expr.id.as_deref() {
            self.memo.insert(id, slot);
        }
        Some(slot)
    }

    /// 字面量：与 `literal_value` 的**归一**口径一致（`F32` / `I32` 落成 `F64`）。
    fn constant(&mut self, literal: &Literal) -> Option<SlotRef> {
        let (out, step) = match literal {
            Literal::F32(value) => self.f64_const(f64::from(*value)),
            Literal::F64(value) => self.f64_const(*value),
            Literal::I32(value) => self.f64_const(f64::from(*value)),
            Literal::I64(value) => {
                let out = self.alloc(SlotType::I64);
                (out, TypedStep::ConstI64(out, *value))
            }
            Literal::Bool(value) => {
                let out = self.alloc(SlotType::Bool);
                (out, TypedStep::ConstBool(out, *value))
            }
            Literal::Vec2(value) => {
                let out = self.alloc(SlotType::Vec2);
                (out, TypedStep::ConstVec2(out, *value))
            }
            Literal::Vec3(value) => {
                let out = self.alloc(SlotType::Vec3);
                (out, TypedStep::ConstVec3(out, *value))
            }
            Literal::Vec4(value) => {
                let out = self.alloc(SlotType::Vec4);
                (out, TypedStep::ConstVec4(out, *value))
            }
            // 文本是 `Bytes + zela.Text`（阶段 2）：没有专用槽 ⇒ 回落。
            Literal::Text(_) => return None,
        };
        self.steps.push(step);
        Some(out)
    }

    fn f64_const(&mut self, value: f64) -> (SlotRef, TypedStep) {
        let out = self.alloc(SlotType::F64);
        (out, TypedStep::ConstF64(out, value))
    }
}
