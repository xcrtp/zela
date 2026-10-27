//! 求值：**装配期建图 → 运行期定序**，效果只能从 [`Mono`] 进。
//!
//! # 为什么不复用 `LazyPullEval`
//!
//! `LazyPullEval` 把状态放在**求值器自己**里（`&mut self` + Mutex 缓存）⇒ 不可枚举、
//! 不可结晶。这里的状态只在 `Mono`（宿主注入的 M）里 ⇒ 可枚举、可结晶。
//! 运行时缓存只能作为**优化**存在，且必须能证明与「朴素重放 M」逐位等价。
//!
//! # 形状
//!
//! [`CompiledExpr`] 是一段**后序程序**：每个 `Step` 的实参索引都小于自身 ⇒ 求值就是
//! 一次线性扫描，无需运行时拓扑排序、无需脏传播。
//!
//! - 共享节点（带 `id` 的 `Def` + `Ref`）**只编译一次** ⇒ 帧内只求一次、广播给引用者；
//! - 程序本身**无内部可变性** ⇒ 每一帧都是重新扫描，不存在跨帧缓存；
//! - 清空缓存重算逐位相同 —— 因为根本没有缓存可清。
//!
//! # 不纯值
//!
//! 字面量 / 通道 / 算子之外没有别的叶子。通道由 [`Mono::read`] 提供，算子由
//! [`Mono::call`] 提供 —— **宿主是唯一的不纯入口**，且它的实现可枚举。

use std::collections::BTreeMap;

use crate::expr::{ChannelRef, Expr, ExprError, Literal, Node, OpRef};

/// 运行时值（跨通道 / 算子的最小集）。
///
/// `Path` 已**迁移出核心**（阶段 1）：路径是 `Bytes` + 命名类型 `zela.Path`，
/// 内核只认识"带类型名的字节块"，不认识"路径"这个领域概念。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Value {
    F64(f64),
    /// 64 位整数（递归索引 / 帧号；**不经过 f64**）。
    I64(i64),
    Bool(bool),
    Text(String),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    /// **字节块 + 命名类型**（`zela.Path` / 第三方私有类型）。
    ///
    /// 命名类型与布局描述在 `ze-func::layout` 的注册表里；本 crate 只搬运字节。
    Bytes(ValueBytes),
}

/// 带类型名的字节块。
#[derive(
    Clone, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
pub struct ValueBytes {
    /// 命名类型（`zela.Path` / `acme.Tensor` …）。
    pub ty: String,
    pub bytes: Vec<u8>,
}

impl ValueBytes {
    pub fn new(ty: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            ty: ty.into(),
            bytes: bytes.into(),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl Value {
    /// 取数值（`F32` / `F64` / `I32` 字面量都归一到这里）。
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F64(value) => Some(*value),
            _ => None,
        }
    }

    /// 取整数（**只有** `I64`；不把 f64 静默取整）。
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::I64(value) => Some(*value),
            _ => None,
        }
    }

    /// 值种类名（错误消息 / 诊断用）。
    pub fn kind(&self) -> &'static str {
        value_kind(self)
    }

    /// **规范字节编码**：跨进程 / 跨语言对拍用的稳定表示。
    ///
    /// 浮点按 IEEE-754 位模式（小端）编码 —— 逐位可比，不做 `to_string` 的精度转换。
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Value::F64(value) => {
                out.push(0);
                out.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            Value::I64(value) => {
                out.push(6);
                out.extend_from_slice(&value.to_le_bytes());
            }
            Value::Bool(value) => {
                out.push(1);
                out.push(u8::from(*value));
            }
            Value::Text(text) => {
                out.push(2);
                out.extend_from_slice(&(text.len() as u32).to_le_bytes());
                out.extend_from_slice(text.as_bytes());
            }
            Value::Vec2(value) => {
                out.push(3);
                out.extend_from_slice(&value[0].to_bits().to_le_bytes());
                out.extend_from_slice(&value[1].to_bits().to_le_bytes());
            }
            Value::Vec3(value) => {
                out.push(4);
                for component in value {
                    out.extend_from_slice(&component.to_bits().to_le_bytes());
                }
            }
            Value::Bytes(bytes) => {
                out.push(5);
                out.extend_from_slice(&(bytes.ty.len() as u32).to_le_bytes());
                out.extend_from_slice(bytes.ty.as_bytes());
                out.extend_from_slice(&(bytes.bytes.len() as u32).to_le_bytes());
                out.extend_from_slice(&bytes.bytes);
            }
        }
        out
    }
}

/// 一串值的**规范哈希**（FNV-1a 64；fixture 的"期望值序列哈希"）。
///
/// 选择 FNV-1a 不是为了抗碰撞，而是为了**零依赖、跨语言十行可复现**：
/// JS / Lua 的 fixture runner 用同一份常量即可得到同一个数。
pub fn hash_values(values: &[Value]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for value in values {
        for byte in value.canonical_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// 单子 `M`：宿主注入的**唯一效果入口**。
///
/// 实现方必须可枚举（时间 / 帧 / 事件 / 组件 / 环境），且 `call` 只做纯函数。
pub trait Mono {
    /// 读取一条通道。
    fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError>;

    /// 调用一个叶子算子。
    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError>;

    /// 读**某对象**上的组件字段（G 批通道读端）。
    ///
    /// `object` 是 `ObjectId` 的打包值（`index:u32<<32 | generation:u32`；
    /// ze-comb 是 kernel 层，不依赖 `ze-scene-contract`）。默认实现**显式失败**
    /// （`MissingChannel`）：宿主没实现读端就不该静默返回 0 ——
    /// 既有 `Mono` 实现（如测试里的 `ScalarMono`）保持默认，不破。
    fn read_object(&self, channel: &ChannelRef, _object: u64) -> Result<Value, EvalError> {
        Err(EvalError::MissingChannel {
            channel: channel.clone(),
        })
    }
}

/// 求值错误：显式、带算子来源（§7.6 的运行时对应物）。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    /// 通道没有值 —— 说明宿主没有注入它（不是静默 0）。
    #[error("通道 `{channel}` 未由宿主注入")]
    MissingChannel { channel: ChannelRef },
    /// 对象上不存在通道对应的组件（地址已注册但该对象没挂这个组件）。
    #[error("对象 {object} 上没有通道 `{channel}` 对应的组件")]
    MissingComponent { object: u64, channel: ChannelRef },
    /// 算子求值失败（类型 / 定义域 / 未知算子）。
    #[error("算子 `{op}` 求值失败：{message}")]
    Op { op: String, message: String },
    /// `Select` 的条件不是 `Bool`（显式失败，不静默当真 / 假）。
    #[error("select 的条件必须是 Bool，实际 {actual}")]
    SelectCondition { actual: &'static str },
    /// 通道**声明类型**与运行期返回值标签不符（装配期推导 vs 实际值对账）。
    #[error("通道 `{channel}` 声明为 {expected}，运行期返回 {actual}")]
    ChannelTypeMismatch {
        channel: ChannelRef,
        expected: &'static str,
        actual: &'static str,
    },
    /// 组件的值类型没有表达式层表示（如 `Color`）。
    ///
    /// 以前是 `anim_to_value → None → 被当成 MissingComponent`（**错误的静默**）；
    /// 现在显式点名"这个值类型不支持读"。
    #[error("通道 `{channel}` 的值类型 {kind} 没有表达式层表示")]
    UnsupportedValue {
        channel: ChannelRef,
        kind: &'static str,
    },
    /// `NatRec.count` 不是非负 `I64`（硬约束 6：禁止刀刃取整）。
    #[error("NatRec.count 必须是 I64，实际 {actual}")]
    RecCount { actual: &'static str },
    /// `NatRec` 的轮数超过**动态护栏**（库级默认；只有影响语义的才进产物）。
    #[error("NatRec 轮数 {count} 超过上限 {max_steps}（不收敛/过大 ⇒ 显式错误）")]
    RecBudget { count: i64, max_steps: i64 },
    /// 递归叶子出现在没有 `NatRec` 上下文的地方（装配期应已拒绝）。
    #[error("递归叶子 `{leaf}` 不在 NatRec.step 求值上下文里")]
    RecContext { leaf: &'static str },
    /// 递归状态类型在折叠过程中变了（静默换类型会让结果不可解释）。
    #[error("NatRec 状态类型不一致：期望 {expected}，第 {index} 步得到 {actual}")]
    RecStateType {
        expected: &'static str,
        actual: &'static str,
        index: i64,
    },
}

impl EvalError {
    /// 构造一条带算子限定名的错误。
    pub fn op(op: &OpRef, message: impl Into<String>) -> Self {
        Self::Op {
            op: op.qualified(),
            message: message.into(),
        }
    }
}

/// `NatRec` 的**显式缓存**（阶段 3）：状态在参数里，可序列化、可清空。
///
/// `count` 是已经折叠到的索引（`0` = 只有 `init`）；`value` 是该索引处的状态。
/// 清空缓存重算 = `None` ⇒ 从 `init` 开始逐位重放 —— "逐位相同"因此是**结构事实**，
/// 而不是纪律。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecCache {
    /// 已折叠到的索引（非负）。
    pub count: i64,
    pub value: Value,
}

/// `NatRec` 轮数的**库级默认 + 动态护栏**（不进产物：它不改变语义，只拒绝失控）。
pub const REC_MAX_STEPS: i64 = 1_000_000;

/// 一次求值的递归上下文。
#[derive(Clone, Debug, Default)]
struct EvalContext {
    /// 当前 `NatRec.step` 的 `(上一步状态, 索引)`。
    rec: Option<RecFrame>,
    /// 根级 `NatRec` 的显式缓存（只在外层程序使用）。
    cache: Option<RecCache>,
    /// 是否允许读写缓存（子程序内关闭，避免污染）。
    cache_enabled: bool,
}

#[derive(Clone, Debug)]
struct RecFrame {
    prev: Value,
    index: i64,
}

/// 后序程序的一步。
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Pure(Literal),
    Channel(ChannelRef),
    /// 读**某对象**的组件字段（G 批通道读端）。
    ///
    /// `object` 是 `ObjectId` 打包值：`commit` 时场景层把 `Node::Read.object`
    /// （脚本局部句柄下标）重写为 `index:u32<<32 | generation:u32` 再编译，
    /// 运行期经 [`Mono::read_object`] 解包读组件。
    Read { channel: ChannelRef, object: u64 },
    Call { op: OpRef, args: Vec<usize> },
    /// **值分支**（H 批）：`cond` 是外层程序里的索引，两个分支各自是**独立子程序**。
    ///
    /// 子程序形态让"惰性"成为结构性事实：只有被选中的那一支会被 `eval`，
    /// 另一支的通道 / 算子一次都不求值（不是靠短路标记）。
    Select {
        cond: usize,
        then: Box<CompiledExpr>,
        r#else: Box<CompiledExpr>,
    },
    /// **结构递归**（阶段 3）：`count` 是外层程序里的索引（必须是 `I64`），
    /// `init` / `step` 是独立子程序。`step` 内的 `RecPrev` / `RecIndex` 由
    /// [`EvalContext`] 提供。
    NatRec {
        count: usize,
        init: Box<CompiledExpr>,
        step: Box<CompiledExpr>,
    },
    /// `NatRec.step` 内：上一步状态。
    RecPrev,
    /// `NatRec.step` 内：当前索引（`I64`）。
    RecIndex,
}

/// 编译后的表达式（装配期一次，运行期线性扫描）。
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledExpr {
    steps: Vec<Step>,
    root: usize,
    /// 是否含**离散层**节点（`NatRec` / `RecPrev` / `RecIndex`）。
    ///
    /// 这是 `infer::PointwiseReport` 在 kernel 侧的投影：含离散层 ⇒ 求值需要历史
    /// 或整段重放，不能按"单帧纯函数"寻址（`SeekKind::Replay`）。
    requires_replay: bool,
}

impl CompiledExpr {
    /// 编译并校验（悬空 `Ref` / 环 / 重名 id 都在这里显式失败）。
    pub fn compile(expr: &Expr) -> Result<Self, ExprError> {
        expr.validate()?;
        let defs = crate::expr::definitions(expr);
        let mut steps = Vec::new();
        let mut memo = BTreeMap::new();
        let root = compile_node(expr, &defs, &mut memo, &mut steps);
        Ok(Self {
            steps,
            root,
            requires_replay: expr.requires_replay(),
        })
    }

    /// 是否含离散层节点（`NatRec` 及其递归叶子）。
    pub fn requires_replay(&self) -> bool {
        self.requires_replay
    }

    /// 程序步数（共享节点只计一次）。
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// 程序（只读；用于诊断 / 导出）。
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// 求值一次。
    ///
    /// 每次调用都从头扫描 ⇒ **帧内 memo 由程序结构决定，不跨帧**。
    pub fn eval(&self, mono: &dyn Mono) -> Result<Value, EvalError> {
        let mut context = EvalContext::default();
        self.eval_with(mono, &mut context)
    }

    /// **阶段 3**：带显式 `NatRec` 缓存的求值。
    ///
    /// - `cache = None` ⇒ 从 `init` 开始逐位重放；
    /// - `cache = Some(RecCache { count, value })` 且 `count ≤ 目标轮数` ⇒ 从该状态续算；
    /// - 缓存**可序列化**；清空后重算逐位相同（结构事实）。
    pub fn eval_cached(
        &self,
        mono: &dyn Mono,
        cache: &mut Option<RecCache>,
    ) -> Result<Value, EvalError> {
        let mut context = EvalContext {
            rec: None,
            cache: cache.take(),
            cache_enabled: true,
        };
        let value = self.eval_with(mono, &mut context);
        *cache = context.cache;
        value
    }

    fn eval_with(&self, mono: &dyn Mono, context: &mut EvalContext) -> Result<Value, EvalError> {
        let mut values: Vec<Value> = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            let value = match step {
                Step::Pure(literal) => literal_value(literal),
                Step::Channel(channel) => mono.read(channel)?,
                Step::Read { channel, object } => mono.read_object(channel, *object)?,
                Step::Select { cond, then, r#else } => match &values[*cond] {
                    Value::Bool(true) => then.eval_with(mono, context)?,
                    Value::Bool(false) => r#else.eval_with(mono, context)?,
                    other => {
                        return Err(EvalError::SelectCondition {
                            actual: value_kind(other),
                        })
                    }
                },
                Step::NatRec { count, init, step } => {
                    let count = match &values[*count] {
                        Value::I64(value) if *value >= 0 => *value,
                        other => {
                            return Err(EvalError::RecCount {
                                actual: value_kind(other),
                            })
                        }
                    };
                    if count > REC_MAX_STEPS {
                        return Err(EvalError::RecBudget {
                            count,
                            max_steps: REC_MAX_STEPS,
                        });
                    }
                    // 根级显式缓存：**只有一个** NatRec 拥有它（第一个被求值的）。
                    // 子程序内关闭，避免污染；拥有后立刻关闭，避免第二个 NatRec
                    // 误用别人的状态。
                    let owns_cache = context.cache_enabled;
                    let cached = if owns_cache {
                        context.cache_enabled = false;
                        context.cache.take()
                    } else {
                        None
                    };
                    let (mut state, mut index) = match cached {
                        Some(cache) if cache.count <= count => (cache.value, cache.count),
                        _ => (init.eval_with(mono, &mut EvalContext::default())?, 0),
                    };
                    let state_kind = value_kind(&state);
                    while index < count {
                        index += 1;
                        let mut step_context = EvalContext {
                            rec: Some(RecFrame {
                                prev: state.clone(),
                                index,
                            }),
                            cache: None,
                            cache_enabled: false,
                        };
                        let next = step.eval_with(mono, &mut step_context)?;
                        if value_kind(&next) != state_kind {
                            return Err(EvalError::RecStateType {
                                expected: state_kind,
                                actual: value_kind(&next),
                                index,
                            });
                        }
                        state = next;
                    }
                    if owns_cache {
                        context.cache = Some(RecCache {
                            count,
                            value: state.clone(),
                        });
                    }
                    state
                }
                Step::RecPrev => match &context.rec {
                    Some(frame) => frame.prev.clone(),
                    None => return Err(EvalError::RecContext { leaf: "rec_prev" }),
                },
                Step::RecIndex => match &context.rec {
                    Some(frame) => Value::I64(frame.index),
                    None => return Err(EvalError::RecContext { leaf: "rec_index" }),
                },
                Step::Call { op, args } => {
                    let args: Vec<Value> = args.iter().map(|&index| values[index].clone()).collect();
                    mono.call(op, &args)?
                }
            };
            values.push(value);
        }
        Ok(values[self.root].clone())
    }
}

/// 值种类名（错误消息用；与 Pod 层词汇一致）。
pub(crate) fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::F64(_) => "scalar",
        Value::I64(_) => "i64",
        Value::Bool(_) => "bool",
        Value::Text(_) => "text",
        Value::Vec2(_) => "vec2",
        Value::Vec3(_) => "vec3",
        Value::Bytes(_) => "bytes",
    }
}

pub(crate) fn literal_value(literal: &Literal) -> Value {
    match literal {
        Literal::F32(value) => Value::F64(f64::from(*value)),
        Literal::F64(value) => Value::F64(*value),
        Literal::I32(value) => Value::F64(f64::from(*value)),
        Literal::I64(value) => Value::I64(*value),
        Literal::Bool(value) => Value::Bool(*value),
        Literal::Text(value) => Value::Text(value.clone()),
        Literal::Vec2(value) => Value::Vec2(*value),
        Literal::Vec3(value) => Value::Vec3(*value),
    }
}

fn compile_node<'a>(
    expr: &'a Expr,
    defs: &BTreeMap<&'a str, &'a Expr>,
    memo: &mut BTreeMap<&'a str, usize>,
    steps: &mut Vec<Step>,
) -> usize {
    if let Some(id) = expr.id.as_deref() {
        if let Some(index) = memo.get(id) {
            return *index;
        }
    }

    let index = match &expr.node {
        Node::Pure { value } => {
            steps.push(Step::Pure(value.clone()));
            steps.len() - 1
        }
        Node::Channel { channel } => {
            steps.push(Step::Channel(channel.clone()));
            steps.len() - 1
        }
        Node::Read { channel, object } => {
            // 把 `object` 原样放入 Step（声明期 = 脚本局部句柄下标；commit 时
            // 场景层已把 Expr 树里的 `Node::Read.object` 重写为 ObjectId 打包值，
            // 见 `Timeline::rewrite_read_objects` —— 两种语义对 `u64` 都成立）。
            steps.push(Step::Read {
                channel: channel.clone(),
                object: *object,
            });
            steps.len() - 1
        }
        Node::Call { op, args } => {
            let args: Vec<usize> = args
                .iter()
                .map(|arg| compile_node(arg, defs, memo, steps))
                .collect();
            steps.push(Step::Call {
                op: op.clone(),
                args,
            });
            steps.len() - 1
        }
        Node::Ref { to } => {
            // `validate` 已拒绝悬空 Ref；这里保留 expect 作为内部不变量。
            let target = defs
                .get(to.as_str())
                .expect("compile 之前必须通过 Expr::validate");
            compile_node(target, defs, memo, steps)
        }
        // **H 批**：`cond` 编进外层程序（它的值就在合并点前一步），
        // 两个分支各自编译成**独立子程序**（各自的值表 / 根索引），
        // 运行期只 `eval` 被选中那一支 ⇒ 惰性是结构性的。
        Node::Select { cond, then, r#else } => {
            let cond = compile_node(cond, defs, memo, steps);
            let then = compile_branch(then, defs);
            let r#else = compile_branch(r#else, defs);
            steps.push(Step::Select {
                cond,
                then: Box::new(then),
                r#else: Box::new(r#else),
            });
            steps.len() - 1
        }
        // **阶段 3**：`count` 编进外层；`init` / `step` 各自独立子程序
        // （`step` 内的 `RecPrev` / `RecIndex` 是普通叶子，编译成专用 `Step`）。
        Node::NatRec { count, init, step } => {
            let count = compile_node(count, defs, memo, steps);
            let init = compile_branch(init, defs);
            let step = compile_branch(step, defs);
            steps.push(Step::NatRec {
                count,
                init: Box::new(init),
                step: Box::new(step),
            });
            steps.len() - 1
        }
        Node::RecPrev => {
            steps.push(Step::RecPrev);
            steps.len() - 1
        }
        Node::RecIndex => {
            steps.push(Step::RecIndex);
            steps.len() - 1
        }
    };

    if let Some(id) = expr.id.as_deref() {
        memo.insert(id, index);
    }
    index
}

/// 把一个分支编译成独立子程序（H 批）。
///
/// 子程序有自己的值表与根索引；`defs` 仍是整棵表达式的定义表 ⇒ 分支里对
/// 外层 `id` 的 `Ref` 照常解析（共享节点在分支内重新编译一次是纯函数，无副作用）。
fn compile_branch<'a>(expr: &'a Expr, defs: &BTreeMap<&'a str, &'a Expr>) -> CompiledExpr {
    let mut steps = Vec::new();
    let mut memo = BTreeMap::new();
    let root = compile_node(expr, defs, &mut memo, &mut steps);
    CompiledExpr {
        steps,
        root,
        requires_replay: expr.requires_replay(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Id;
    use std::cell::Cell;

    fn math(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    struct ScalarMono {
        time: f64,
        frame: u64,
        calls: Cell<usize>,
    }

    impl ScalarMono {
        fn new(time: f64, frame: u64) -> Self {
            Self {
                time,
                frame,
                calls: Cell::new(0),
            }
        }
    }

    impl Mono for ScalarMono {
        fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
            match (channel.namespace.as_str(), channel.name.as_str()) {
                ("", "time") => Ok(Value::F64(self.time)),
                ("", "frame") => Ok(Value::F64(self.frame as f64)),
                _ => Err(EvalError::MissingChannel {
                    channel: channel.clone(),
                }),
            }
        }

        fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
            self.calls.set(self.calls.get() + 1);
            let num = |index: usize| -> Result<f64, EvalError> {
                args.get(index)
                    .and_then(Value::as_f64)
                    .ok_or_else(|| EvalError::op(op, format!("第 {index} 个实参不是数值")))
            };
            match op.name.as_str() {
                "sin" => Ok(Value::F64(num(0)?.sin())),
                "cos" => Ok(Value::F64(num(0)?.cos())),
                "mul" => Ok(Value::F64(num(0)? * num(1)?)),
                "add" => Ok(Value::F64(num(0)? + num(1)?)),
                "scale" => Ok(Value::F64(num(0)? * num(1)?)),
                "sum" => {
                    let mut total = 0.0;
                    for index in 0..args.len() {
                        total += num(index)?;
                    }
                    Ok(Value::F64(total))
                }
                other => Err(EvalError::op(op, format!("未知算子 `{other}`"))),
            }
        }
    }

    /// 共享节点：`h1 = sin(t × 1)`，根 = `h1 + 2 × h1`。
    fn shared_tree() -> Expr {
        let h1 = Expr::call(math("sin"), [Expr::call(math("mul"), [
            Expr::channel(ChannelRef::builtin("time")),
            Expr::pure(1.0),
        ])])
        .with_id("h1");
        Expr::call(
            math("add"),
            [
                h1.clone(),
                Expr::call(math("scale"), [Expr::pure(2.0), Expr::reference("h1")]),
            ],
        )
    }

    #[test]
    fn shared_node_is_compiled_and_evaluated_once() {
        let program = CompiledExpr::compile(&shared_tree()).expect("编译成功");
        // 通道 + 纯量 + mul + sin + 纯量 + scale + add = 7 步（h1 只编译一次）。
        assert_eq!(program.step_count(), 7);

        let t = std::f64::consts::FRAC_PI_2;
        let mono = ScalarMono::new(t, 0);
        let value = program.eval(&mono).expect("求值成功");
        let sin = t.sin();
        assert!((value.as_f64().unwrap() - (sin + 2.0 * sin)).abs() < 1e-12);
        // 4 次算子调用（mul / sin / scale / add），共享节点没有重复调用。
        assert_eq!(mono.calls.get(), 4);
    }

    #[test]
    fn evaluation_is_frame_local_and_recomputable() {
        let program = CompiledExpr::compile(&shared_tree()).unwrap();
        let mono = ScalarMono::new(0.0, 0);
        let first = program.eval(&mono).unwrap();
        let second = program.eval(&mono).unwrap();
        assert_eq!(first, second, "同一 M ⇒ 同一结果（无隐藏状态）");
        assert_eq!(mono.calls.get(), 8, "每次求值都重算，不跨帧缓存");
    }

    #[test]
    fn frame_channel_comes_from_the_monad() {
        let expr = Expr::call(math("sin"), [Expr::channel(ChannelRef::builtin("frame"))]);
        let program = CompiledExpr::compile(&expr).unwrap();
        let mono = ScalarMono::new(0.0, 30);
        let value = program.eval(&mono).unwrap().as_f64().unwrap();
        assert!((value - 30.0_f64.sin()).abs() < 1e-12);
    }

    #[test]
    fn missing_channel_is_explicit() {
        let expr = Expr::channel(ChannelRef::builtin("event"));
        let program = CompiledExpr::compile(&expr).unwrap();
        let err = program.eval(&ScalarMono::new(0.0, 0)).unwrap_err();
        assert_eq!(
            err,
            EvalError::MissingChannel {
                channel: ChannelRef::builtin("event")
            }
        );
    }

    #[test]
    fn bad_ref_is_rejected_at_compile() {
        let expr = Expr::reference("missing");
        assert_eq!(
            CompiledExpr::compile(&expr).unwrap_err(),
            ExprError::DanglingRef(Id::from("missing"))
        );
    }

    #[test]
    fn unknown_operator_reports_its_source() {
        let expr = Expr::call(OpRef::new("zela.future", "warp"), []);
        let program = CompiledExpr::compile(&expr).unwrap();
        let err = program.eval(&ScalarMono::new(0.0, 0)).unwrap_err();
        assert_eq!(
            err,
            EvalError::Op {
                op: "zela.future::warp".to_string(),
                message: "未知算子 `warp`".to_string(),
            }
        );
    }

    /// **H 批**：`Select` 按 `cond` 取值分支。
    #[test]
    fn select_picks_branch_by_condition() {
        let expr = Expr::select(Expr::pure(true), Expr::pure(1.0), Expr::pure(0.0));
        let program = CompiledExpr::compile(&expr).expect("Select 已实现");
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap(),
            Value::F64(1.0)
        );

        let expr = Expr::select(Expr::pure(false), Expr::pure(1.0), Expr::pure(0.0));
        let program = CompiledExpr::compile(&expr).unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap(),
            Value::F64(0.0)
        );
    }

    /// **H 批惰性**：未选中的分支**一次都不求值**（分支里读缺失通道也不报错）。
    #[test]
    fn select_is_lazy_on_the_unselected_branch() {
        let then = Expr::channel(ChannelRef::builtin("event"));
        let r#else = Expr::pure(0.0);
        let expr = Expr::select(Expr::pure(false), then, r#else);
        let program = CompiledExpr::compile(&expr).unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap(),
            Value::F64(0.0),
            "else 分支生效 ⇒ then 里的缺失通道不会被读到"
        );

        // 反向：选中 then（有值）时 else 的缺失通道同样不读。
        let expr = Expr::select(
            Expr::pure(true),
            Expr::pure(1.0),
            Expr::channel(ChannelRef::builtin("event")),
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap(),
            Value::F64(1.0)
        );
    }

    /// **H 批**：`cond` 不是 Bool ⇒ 显式报错（不静默当真 / 假）。
    #[test]
    fn select_condition_must_be_bool() {
        let expr = Expr::select(Expr::pure(1.0), Expr::pure(1.0), Expr::pure(0.0));
        let program = CompiledExpr::compile(&expr).unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap_err(),
            EvalError::SelectCondition { actual: "scalar" }
        );
    }

    /// **G 批**：`Read` 编译成 `Step::Read`；宿主未实现 `read_object` 时
    /// 默认行为是**显式失败**（`MissingChannel`），不是静默 0。
    #[test]
    fn read_without_host_impl_fails_explicitly() {
        let expr = Expr::read(
            ChannelRef::new("zela.scene", "Circle.radius").with_type(crate::expr::ChannelType::Number),
            7,
        );
        let program = CompiledExpr::compile(&expr).expect("Read 是叶子，编译必然成功");
        assert_eq!(
            program.steps(),
            &[Step::Read {
                channel: ChannelRef::new("zela.scene", "Circle.radius"),
                object: 7,
            }]
        );

        let err = program.eval(&ScalarMono::new(0.0, 0)).unwrap_err();
        assert_eq!(
            err,
            EvalError::MissingChannel {
                channel: ChannelRef::new("zela.scene", "Circle.radius"),
            },
            "默认 read_object ⇒ 显式 MissingChannel"
        );
    }

    /// **阶段 1**：`I64` 不经过 f64（递归索引不能撞刀刃取整）。
    #[test]
    fn i64_stays_integer_and_is_not_silently_cast() {
        let value = Value::I64(7);
        assert_eq!(value.as_i64(), Some(7));
        assert_eq!(value.as_f64(), None, "I64 不得静默转 f64");
        assert_eq!(value.kind(), "i64");
        assert_ne!(
            hash_values(&[Value::I64(7)]),
            hash_values(&[Value::F64(7.0)]),
            "整数与浮点必须是不同的值序列"
        );
    }

    /// **阶段 1**：`Value::Path` 已消失；路径是 `Bytes + zela.Path`。
    #[test]
    fn bytes_carry_a_named_type() {
        let bytes = Value::Bytes(ValueBytes::new("zela.Path", vec![1, 2, 3, 4]));
        assert_eq!(bytes.kind(), "bytes");
        let canonical = bytes.canonical_bytes();
        assert_eq!(canonical[0], 5, "Bytes 的规范标签");
        assert!(canonical.windows("zela.Path".len()).any(|w| w == b"zela.Path"));
    }

    // ─── 阶段 3：NatRec 离散层 ─────────────────────────────────────────

    /// `value(k) = value(k−1) + 2`，`init = 1`，`count = 4` ⇒ `9`。
    fn nat_rec_add_two(count: i64) -> Expr {
        Expr::nat_rec(
            Expr::pure(count),
            Expr::pure(1.0),
            Expr::call(math("add"), [Expr::rec_prev(), Expr::pure(2.0)]),
        )
    }

    #[test]
    fn nat_rec_matches_hand_unrolled_expansion() {
        let expr = nat_rec_add_two(4);
        expr.validate().expect("NatRec 是合法递归结构");
        let program = CompiledExpr::compile(&expr).unwrap();
        let mono = ScalarMono::new(0.0, 0);
        let lowered = program.eval(&mono).unwrap();
        assert_eq!(lowered, Value::F64(9.0));
        assert_eq!(
            crate::reference::eval_reference(&expr, &mono).unwrap(),
            lowered,
            "参考求值器与 lowered 必须逐位一致"
        );

        // 手写展开：((((1+2)+2)+2)+2)。
        let mut manual = Expr::pure(1.0);
        for _ in 0..4 {
            manual = Expr::call(math("add"), [manual, Expr::pure(2.0)]);
        }
        assert_eq!(
            CompiledExpr::compile(&manual).unwrap().eval(&mono).unwrap(),
            lowered
        );
    }

    /// `RecIndex` 是 `I64`：`value(k) = k` ⇒ `value(count) = count`。
    #[test]
    fn rec_index_is_an_i64_state() {
        let expr = Expr::nat_rec(
            Expr::pure(3_i64),
            Expr::pure(0_i64),
            Expr::rec_index(),
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap(),
            Value::I64(3)
        );
    }

    /// 硬约束 6：`count` 不是 `I64` / 为负 ⇒ 显式错误（不取整、不静默 0 轮）。
    #[test]
    fn nat_rec_rejects_non_i64_counts() {
        let program = CompiledExpr::compile(&Expr::nat_rec(
            Expr::pure(4.0),
            Expr::pure(1.0),
            Expr::rec_prev(),
        ))
        .unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap_err(),
            EvalError::RecCount { actual: "scalar" }
        );

        let program = CompiledExpr::compile(&Expr::nat_rec(
            Expr::pure(-1_i64),
            Expr::pure(1.0),
            Expr::rec_prev(),
        ))
        .unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap_err(),
            EvalError::RecCount { actual: "i64" }
        );
    }

    /// 动态护栏：轮数超过库级上限 ⇒ 显式错误（不"跑满上限用最后一次"）。
    #[test]
    fn nat_rec_budget_is_explicit() {
        let program = CompiledExpr::compile(&Expr::nat_rec(
            Expr::pure(REC_MAX_STEPS + 1),
            Expr::pure(1.0),
            Expr::rec_prev(),
        ))
        .unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap_err(),
            EvalError::RecBudget {
                count: REC_MAX_STEPS + 1,
                max_steps: REC_MAX_STEPS,
            }
        );
    }

    /// 状态类型在折叠过程中变化 ⇒ 显式错误（不静默换类型）。
    #[test]
    fn nat_rec_state_type_must_stay_stable() {
        let program = CompiledExpr::compile(&Expr::nat_rec(
            Expr::pure(1_i64),
            Expr::pure(1.0),
            Expr::rec_index(),
        ))
        .unwrap();
        assert_eq!(
            program.eval(&ScalarMono::new(0.0, 0)).unwrap_err(),
            EvalError::RecStateType {
                expected: "scalar",
                actual: "i64",
                index: 1,
            }
        );
    }

    /// **合法递归 vs 非法环**：`NatRec` 通过；`Ref` 成环仍然被拒绝。
    #[test]
    fn recursion_is_not_a_ref_cycle() {
        nat_rec_add_two(2).validate().expect("NatRec 合法");

        let a = Expr::call(math("add"), [Expr::reference("b")]).with_id("a");
        let b = Expr::call(math("add"), [Expr::reference("a")]).with_id("b");
        let root = Expr::call(math("add"), [a, b]);
        assert!(matches!(
            root.validate().unwrap_err(),
            ExprError::Cycle(_)
        ));
    }

    /// 递归叶子越界 ⇒ 结构守卫显式拒绝。
    #[test]
    fn recursive_leaves_outside_a_step_are_rejected() {
        let expr = Expr::call(math("add"), [Expr::rec_prev(), Expr::pure(1.0)]);
        assert_eq!(
            expr.validate().unwrap_err(),
            ExprError::OrphanRec("rec_prev")
        );

        let expr = Expr::nat_rec(
            Expr::pure(1_i64),
            Expr::rec_index(),
            Expr::rec_prev(),
        );
        assert_eq!(
            expr.validate().unwrap_err(),
            ExprError::OrphanRec("rec_index")
        );
    }

    /// **Cache 作显式参数**：可序列化、可清空；清空重算逐位相同。
    #[test]
    fn nat_rec_cache_is_serializable_and_replay_is_bit_identical() {
        let program = CompiledExpr::compile(&nat_rec_add_two(5)).unwrap();
        let mono = ScalarMono::new(0.0, 0);

        let mut cache = None;
        let first = program.eval_cached(&mono, &mut cache).unwrap();
        assert_eq!(first, Value::F64(11.0));
        let cache = cache.expect("根级 NatRec 必须产出缓存");
        assert_eq!(cache.count, 5);

        // 序列化 → 反序列化 → 复用。
        let json = serde_json::to_string(&cache).expect("缓存可序列化");
        let mut restored: Option<RecCache> = serde_json::from_str(&json).unwrap();
        assert_eq!(
            program.eval_cached(&mono, &mut restored).unwrap(),
            first,
            "复用缓存必须逐位相同"
        );

        // 清空缓存 ⇒ 从 init 重放，仍逐位相同。
        let mut cleared = None;
        assert_eq!(program.eval_cached(&mono, &mut cleared).unwrap(), first);

        // 部分缓存（count = 2）⇒ 从该状态续算到 5，与全量重放一致。
        let partial_program = CompiledExpr::compile(&nat_rec_add_two(2)).unwrap();
        let mut partial = None;
        partial_program.eval_cached(&mono, &mut partial).unwrap();
        assert_eq!(partial.as_ref().unwrap().count, 2);
        let resumed = program.eval_cached(&mono, &mut partial).unwrap();
        assert_eq!(resumed, first, "续算必须与全量重放逐位相同");
    }

    /// 多个 `NatRec` 时缓存只属于**第一个被求值**的那个（不串状态）。
    #[test]
    fn nat_rec_cache_is_owned_by_exactly_one_fold() {
        let expr = Expr::call(
            math("add"),
            [nat_rec_add_two(2), nat_rec_add_two(3)],
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        let mono = ScalarMono::new(0.0, 0);
        let mut cache = None;
        let value = program.eval_cached(&mono, &mut cache).unwrap();
        assert_eq!(value, Value::F64(12.0), "5 + 7");
        let cache = cache.expect("第一个 NatRec 产出缓存");
        assert_eq!(cache.count, 2, "缓存属于第一个 fold");
        assert_eq!(
            program.eval_cached(&mono, &mut Some(cache)).unwrap(),
            value
        );
    }

    /// **G 批**：宿主覆写 `read_object` 后，`Read` 返回注入值。
    #[test]
    fn read_returns_injected_value_when_host_overrides() {
        struct InjectedRead {
            radius: f64,
        }
        impl Mono for InjectedRead {
            fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
                Err(EvalError::MissingChannel {
                    channel: channel.clone(),
                })
            }
            fn call(&self, op: &OpRef, _args: &[Value]) -> Result<Value, EvalError> {
                // 本测试只求值 Read 叶子，算子不会被调用；显式失败兜底。
                Err(EvalError::op(op, "测试单子不实现 call".to_string()))
            }
            fn read_object(&self, channel: &ChannelRef, object: u64) -> Result<Value, EvalError> {
                if channel.name == "Circle.radius" {
                    Ok(Value::F64(self.radius))
                } else {
                    Err(EvalError::MissingComponent {
                        object,
                        channel: channel.clone(),
                    })
                }
            }
        }

        let expr = Expr::read(ChannelRef::new("zela.scene", "Circle.radius"), 7);
        let program = CompiledExpr::compile(&expr).unwrap();
        let value = program.eval(&InjectedRead { radius: 2.5 }).unwrap();
        assert!((value.as_f64().unwrap() - 2.5).abs() < 1e-12);

        // 地址匹配但对象上无组件 ⇒ `MissingComponent` 变体存在且显式。
        let missing = Expr::read(ChannelRef::new("zela.scene", "Dash.period"), 7);
        let program = CompiledExpr::compile(&missing).unwrap();
        let err = program.eval(&InjectedRead { radius: 0.0 }).unwrap_err();
        assert!(matches!(
            err,
            EvalError::MissingComponent { object, .. } if object == 7
        ));
    }
}
