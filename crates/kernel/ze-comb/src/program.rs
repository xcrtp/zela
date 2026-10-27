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
#[derive(Clone, Debug, PartialEq)]
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
#[derive(Clone, Debug, PartialEq, Eq, Default)]
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
}

/// 编译后的表达式（装配期一次，运行期线性扫描）。
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledExpr {
    steps: Vec<Step>,
    root: usize,
}

impl CompiledExpr {
    /// 编译并校验（悬空 `Ref` / 环 / 重名 id 都在这里显式失败）。
    pub fn compile(expr: &Expr) -> Result<Self, ExprError> {
        expr.validate()?;
        let defs = crate::expr::definitions(expr);
        let mut steps = Vec::new();
        let mut memo = BTreeMap::new();
        let root = compile_node(expr, &defs, &mut memo, &mut steps);
        Ok(Self { steps, root })
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
        let mut values: Vec<Value> = Vec::with_capacity(self.steps.len());
        for step in &self.steps {
            let value = match step {
                Step::Pure(literal) => literal_value(literal),
                Step::Channel(channel) => mono.read(channel)?,
                Step::Read { channel, object } => mono.read_object(channel, *object)?,
                Step::Select { cond, then, r#else } => match &values[*cond] {
                    Value::Bool(true) => then.eval(mono)?,
                    Value::Bool(false) => r#else.eval(mono)?,
                    other => {
                        return Err(EvalError::SelectCondition {
                            actual: value_kind(other),
                        })
                    }
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
    CompiledExpr { steps, root }
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
