//! 表达式树：节点、字面量、通道、算子引用，以及装配期校验。
//!
//! # 为什么节点自带可选 `id`、并保留 `Ref` 类型
//!
//! 本批行为先按**纯树**求值（fourier 规模够用），但格式从第一版就留口子：
//! 节点图是 DAG，纯树映射不过去；事后加字段是**新格式**，而这项目刚在
//! `zela.scene@7→@8→@9` 上付过学费。留字段是零成本。
//!
//! # 为什么算子用「namespace + name」而不是 Rust 类型名
//!
//! `registry.rs` 的 `stringify!` 对泛型产出 `Map<Sin,Pure<f32>>` —— 改实现就变。
//! 这里把算子身份写成**数据**：`{ "namespace": "zela.math", "name": "sin" }`。
//! 加一个新算子只是多一条数据，老 payload 的解码路径一个字节不动。

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// 表达式局部作用域里的节点名（`Def { id = "h1", ... }` 的对应物）。
pub type Id = String;

/// 表达式节点。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Node {
    /// 字面量叶子。装配期算术的结果直接落在这里（`4/(πk)` 是 Lua 的数字，不是节点）。
    Pure { value: Literal },
    /// **零元组合子**：不是值，是「读取某通道」的表达式叶子。
    ///
    /// 脚本拿不到通道的数值：装配期它没值，运行期由宿主单子 `M` 注入（S3）。
    Channel { channel: ChannelRef },
    /// 调用一个已注册的函子算子。
    Call { op: OpRef, args: Vec<Expr> },
    /// 引用本表达式内**已定义**的节点（非递归的 `let`）。
    ///
    /// 自引用 = 递归 = 要 `fix` ⇒ 装配期禁环（[`Expr::validate`]）。
    Ref { to: Id },
    /// **值分支**（提案 #6 D：只留格式，尚未实现求值）。
    ///
    /// 形状从第一版就定下来（`cond` / `then` / `else` 三个命名子表达式），
    /// 理由与 `Ref` 相同：事后加字段 = 新格式，而这批已经付过一次
    /// `payload 1 → 2` 的代价。`CompiledExpr::compile` 对它是**显式未实现错误**，
    /// 不会静默算错。
    Select {
        cond: Box<Expr>,
        then: Box<Expr>,
        r#else: Box<Expr>,
    },
}

impl Node {
    /// 直接子表达式（`Pure` / `Channel` / `Ref` 是叶子）。
    pub fn children(&self) -> Vec<&Expr> {
        match self {
            Node::Call { args, .. } => args.iter().collect(),
            Node::Select {
                cond, then, r#else,
            } => vec![cond, then, r#else],
            Node::Pure { .. } | Node::Channel { .. } | Node::Ref { .. } => Vec::new(),
        }
    }
}

/// 字面量（跨边界前的最后一层：过了 Pod 边界就只剩这些值）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Literal {
    F32(f32),
    F64(f64),
    I32(i32),
    Bool(bool),
    Text(String),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
}

impl From<f32> for Literal {
    fn from(value: f32) -> Self {
        Literal::F32(value)
    }
}

impl From<f64> for Literal {
    fn from(value: f64) -> Self {
        Literal::F64(value)
    }
}

impl From<i32> for Literal {
    fn from(value: i32) -> Self {
        Literal::I32(value)
    }
}

impl From<bool> for Literal {
    fn from(value: bool) -> Self {
        Literal::Bool(value)
    }
}

impl From<String> for Literal {
    fn from(value: String) -> Self {
        Literal::Text(value)
    }
}

impl From<&str> for Literal {
    fn from(value: &str) -> Self {
        Literal::Text(value.to_string())
    }
}

impl From<[f32; 2]> for Literal {
    fn from(value: [f32; 2]) -> Self {
        Literal::Vec2(value)
    }
}

impl From<[f32; 3]> for Literal {
    fn from(value: [f32; 3]) -> Self {
        Literal::Vec3(value)
    }
}

/// **通道引用**：`namespace` + `name` 是**数据**（与 [`OpRef`] 同形）。
///
/// # 为什么不是封闭枚举
///
/// 旧形态是 `enum Channel { Time, Frame, Event, … }` —— 场景词汇被钉在通用层，
/// 每加一个可读字段都要改 `ze-func`。提案 #6 后组件字段本身就是地址
/// （`Circle.radius`），与 `time` / `frame` 是同一机制的两种写法：
///
/// - **空 namespace** = 宿主基础通道（`time` / `frame`）；
/// - 组件字段通道 = `("zela.scene", "Circle.radius")`。
///
/// 领域无关层只定义**形状**，词汇由宿主单子 `M` 的实现决定。
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelRef {
    /// 命名词汇表的 namespace（空串 = 宿主基础通道）。
    pub namespace: String,
    /// 通道名（基础通道是 `time` / `frame`；组件字段是地址 `Circle.radius`）。
    pub name: String,
}

impl ChannelRef {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
        }
    }

    /// 宿主基础通道（`time` / `frame` …）—— 空 namespace。
    pub fn builtin(name: impl Into<String>) -> Self {
        Self::new("", name)
    }

    /// 是否宿主基础通道（由 `Mono::read` 直接提供）。
    pub fn is_builtin(&self) -> bool {
        self.namespace.is_empty()
    }

    /// `namespace::name`（基础通道只写 name）。
    pub fn qualified(&self) -> String {
        if self.is_builtin() {
            self.name.clone()
        } else {
            format!("{}::{}", self.namespace, self.name)
        }
    }
}

impl fmt::Display for ChannelRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.qualified())
    }
}

/// 算子引用：`namespace` + `name` 是**数据**，不是 Rust 类型名。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpRef {
    /// 函子库 namespace（如 `zela.math`；空串表示无 namespace）。
    pub namespace: String,
    /// 算子名（不含 namespace）。
    pub name: String,
}

impl OpRef {
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
        }
    }

    /// 完整算子名（`"zela.math::sin"`；无 namespace 时只有名字）。
    pub fn qualified(&self) -> String {
        self.to_string()
    }

    /// `"zela.math::sin"` ⇒ `OpRef { namespace: "zela.math", name: "sin" }`。
    pub fn parse(qualified: &str) -> Option<Self> {
        match qualified.rsplit_once("::") {
            Some((namespace, name)) if !name.is_empty() => Some(Self::new(namespace, name)),
            Some(_) => None,
            None if !qualified.is_empty() => Some(Self::new("", qualified)),
            None => None,
        }
    }
}

impl fmt::Display for OpRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.namespace.is_empty() {
            f.write_str(&self.name)
        } else {
            write!(f, "{}::{}", self.namespace, self.name)
        }
    }
}

/// 表达式节点 + 可选局部 id。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Expr {
    /// 表达式局部作用域里的名字；带 id 的节点可被 [`Node::Ref`] 复用（共享 = 每帧一次读取）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    pub node: Node,
}

impl Expr {
    pub fn new(node: Node) -> Self {
        Self { id: None, node }
    }

    /// 给节点挂局部 id（重复由 [`Expr::validate`] 显式报错）。
    pub fn with_id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn pure(value: impl Into<Literal>) -> Self {
        Self::new(Node::Pure {
            value: value.into(),
        })
    }

    pub fn channel(channel: ChannelRef) -> Self {
        Self::new(Node::Channel { channel })
    }

    pub fn call(op: OpRef, args: impl IntoIterator<Item = Expr>) -> Self {
        Self::new(Node::Call {
            op,
            args: args.into_iter().collect(),
        })
    }

    pub fn reference(to: impl Into<Id>) -> Self {
        Self::new(Node::Ref { to: to.into() })
    }

    /// 值分支（格式已定，求值未实现；见 [`Node::Select`]）。
    pub fn select(cond: Expr, then: Expr, otherwise: Expr) -> Self {
        Self::new(Node::Select {
            cond: Box::new(cond),
            then: Box::new(then),
            r#else: Box::new(otherwise),
        })
    }

    /// 装配期校验：重名 id / 悬空 `Ref` / 环。
    ///
    /// 环检测沿用 `ze-time` 的 visited-set 形态（`clock.rs` 是全仓唯一递归环检测），
    /// 不新建一套算法。
    pub fn validate(&self) -> Result<(), ExprError> {
        let mut defs: BTreeMap<&str, &Expr> = BTreeMap::new();
        collect_defs(self, &mut defs)?;
        check_dangling(self, &defs)?;
        check_cycles(&defs)
    }
}

/// 表达式层的装配期错误 —— 全部显式，不静默跳过。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExprError {
    /// 同一作用域里两个节点用了同一个 id。
    #[error("重复的节点 id：`{0}`")]
    DuplicateId(Id),
    /// `Ref` 指向未定义的 id。
    #[error("悬空的 Ref：`{0}` 没有定义")]
    DanglingRef(Id),
    /// `Ref` 成环（`a → b → a`）—— 自引用是递归，本层不支持。
    #[error("Ref 成环：{0}")]
    Cycle(String),
    /// 节点类型只有序列化格式、尚未实现求值（`Select`，提案 #6 D）。
    #[error("节点 `{0}` 尚未实现求值（格式已保留）")]
    UnsupportedNode(&'static str),
}

/// 收集全部带 `id` 的节点（**调用方必须先 `validate`**；重名时后写覆盖）。
pub(crate) fn definitions(expr: &Expr) -> BTreeMap<&str, &Expr> {
    fn walk<'a>(expr: &'a Expr, out: &mut BTreeMap<&'a str, &'a Expr>) {
        if let Some(id) = expr.id.as_deref() {
            out.insert(id, expr);
        }
        for child in expr.node.children() {
            walk(child, out);
        }
    }
    let mut out = BTreeMap::new();
    walk(expr, &mut out);
    out
}

fn collect_defs<'a>(
    expr: &'a Expr,
    defs: &mut BTreeMap<&'a str, &'a Expr>,
) -> Result<(), ExprError> {
    if let Some(id) = expr.id.as_deref() {
        if defs.insert(id, expr).is_some() {
            return Err(ExprError::DuplicateId(id.to_string()));
        }
    }
    for child in expr.node.children() {
        collect_defs(child, defs)?;
    }
    Ok(())
}

fn check_dangling(expr: &Expr, defs: &BTreeMap<&str, &Expr>) -> Result<(), ExprError> {
    if let Node::Ref { to } = &expr.node {
        if !defs.contains_key(to.as_str()) {
            return Err(ExprError::DanglingRef(to.clone()));
        }
    }
    for child in expr.node.children() {
        check_dangling(child, defs)?;
    }
    Ok(())
}

const WHITE: u8 = 0;
const GRAY: u8 = 1;
const BLACK: u8 = 2;

fn check_cycles(defs: &BTreeMap<&str, &Expr>) -> Result<(), ExprError> {
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    for id in defs.keys() {
        if state.get(id).copied().unwrap_or(WHITE) == WHITE {
            let mut stack = Vec::new();
            visit(id, defs, &mut state, &mut stack)?;
        }
    }
    Ok(())
}

fn visit<'a>(
    id: &'a str,
    defs: &BTreeMap<&'a str, &'a Expr>,
    state: &mut BTreeMap<&'a str, u8>,
    stack: &mut Vec<&'a str>,
) -> Result<(), ExprError> {
    match state.get(id).copied().unwrap_or(WHITE) {
        BLACK => return Ok(()),
        GRAY => {
            let start = stack.iter().position(|item| *item == id).unwrap_or(0);
            let mut cycle: Vec<String> = stack[start..].iter().map(|item| (*item).to_string()).collect();
            cycle.push(id.to_string());
            return Err(ExprError::Cycle(cycle.join(" → ")));
        }
        _ => {}
    }
    state.insert(id, GRAY);
    stack.push(id);
    let Some(expr) = defs.get(id) else {
        stack.pop();
        state.insert(id, BLACK);
        return Ok(());
    };
    for target in refs_in(expr) {
        if defs.contains_key(target) {
            visit(target, defs, state, stack)?;
        }
    }
    stack.pop();
    state.insert(id, BLACK);
    Ok(())
}

/// 收集子树里的全部 `Ref` 目标（只向下走子节点，不跨 `Ref`）。
fn refs_in<'a>(expr: &'a Expr) -> Vec<&'a str> {
    let mut out = Vec::new();
    fn walk<'a>(expr: &'a Expr, out: &mut Vec<&'a str>) {
        if let Node::Ref { to } = &expr.node {
            out.push(to.as_str());
        }
        for child in expr.node.children() {
            walk(child, out);
        }
    }
    walk(expr, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn math(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    /// 一个 schema v2 payload：只用 `call` / `pure` / `channel`，没有 Ref。
    fn v2_payload() -> Value {
        json!({
            "id": "h1",
            "node": {
                "type": "call",
                "data": {
                    "op": { "namespace": "zela.math", "name": "sin" },
                    "args": [
                        {
                            "node": {
                                "type": "pure",
                                "data": { "value": { "type": "f32", "data": 1.0 } }
                            }
                        },
                        {
                            "node": {
                                "type": "channel",
                                "data": { "channel": { "namespace": "", "name": "time" } }
                            }
                        }
                    ]
                }
            }
        })
    }

    #[test]
    fn v2_payload_round_trips() {
        let raw = v2_payload();
        let expr: Expr = serde_json::from_value(raw.clone()).expect("v2 payload 必须可解码");
        assert_eq!(expr.id.as_deref(), Some("h1"));
        let Node::Call { op, args } = &expr.node else {
            panic!("期望 call 节点");
        };
        assert_eq!(op, &math("sin"));
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].node, Node::Pure { value: Literal::F32(1.0) });
        assert_eq!(
            args[1].node,
            Node::Channel {
                channel: ChannelRef::builtin("time")
            }
        );

        let again = serde_json::to_value(&expr).expect("必须可序列化");
        assert_eq!(again, raw, "round-trip 必须逐字段一致");
    }

    #[test]
    fn unknown_operators_stay_decodable() {
        // 算子身份是数据：新增 / 未知算子不影响老 payload 的解码路径。
        let raw = json!({
            "node": {
                "type": "call",
                "data": {
                    "op": { "namespace": "zela.future", "name": "warp" },
                    "args": []
                }
            }
        });
        let expr: Expr = serde_json::from_value(raw).expect("未知算子仍是合法表达式（装配期再查）");
        let Node::Call { op, .. } = &expr.node else {
            panic!("期望 call 节点");
        };
        assert_eq!(op.qualified(), "zela.future::warp");
    }

    #[test]
    fn ids_and_refs_round_trip() {
        let shared = Expr::call(math("sin"), [Expr::channel(ChannelRef::builtin("time"))]).with_id("h1");
        let root = Expr::call(
            math("sum"),
            [
                shared.clone(),
                Expr::call(scale_op(), [Expr::reference("h1"), Expr::pure(2.0)]),
            ],
        );
        root.validate().expect("共享引用必须通过校验");

        let encoded = serde_json::to_value(&root).expect("必须可序列化");
        let decoded: Expr = serde_json::from_value(encoded).expect("必须可解码");
        assert_eq!(decoded, root);
        assert_eq!(shared.id.as_deref(), Some("h1"));

        // Ref 是**保留的节点类型**，不被展开成两棵子树。
        let text = serde_json::to_string(&root).unwrap();
        assert!(text.contains("\"type\":\"ref\""), "Ref 必须保留");
    }

    fn scale_op() -> OpRef {
        math("scale")
    }

    #[test]
    fn channels_are_zero_ary_leaves() {
        let raw = serde_json::to_value(Expr::channel(ChannelRef::builtin("frame"))).unwrap();
        assert_eq!(
            raw,
            json!({
                "node": {
                    "type": "channel",
                    "data": { "channel": { "namespace": "", "name": "frame" } }
                }
            })
        );
    }

    /// 组件字段通道与基础通道是**同一形状**：只有 namespace / name 不同。
    #[test]
    fn component_channels_are_data_not_vocabulary() {
        let component = ChannelRef::new("zela.scene", "Circle.radius");
        assert_eq!(component.qualified(), "zela.scene::Circle.radius");
        assert!(!component.is_builtin());
        assert!(ChannelRef::builtin("time").is_builtin());
    }

    /// **提案 #6 D**：`Select` 的序列化形状钉死（本轮只留格式、不实现求值）。
    #[test]
    fn select_format_is_pinned() {
        let expr = Expr::select(
            Expr::channel(ChannelRef::builtin("event")),
            Expr::pure(1.0),
            Expr::pure(0.0),
        );
        let raw = serde_json::to_value(&expr).expect("必须可序列化");
        assert_eq!(
            raw,
            json!({
                "node": {
                    "type": "select",
                    "data": {
                        "cond": {
                            "node": {
                                "type": "channel",
                                "data": { "channel": { "namespace": "", "name": "event" } }
                            }
                        },
                        "then": {
                            "node": {
                                "type": "pure",
                                "data": { "value": { "type": "f64", "data": 1.0 } }
                            }
                        },
                        "else": {
                            "node": {
                                "type": "pure",
                                "data": { "value": { "type": "f64", "data": 0.0 } }
                            }
                        }
                    }
                }
            })
        );
        let decoded: Expr = serde_json::from_value(raw).expect("格式必须可解码");
        assert_eq!(decoded, expr, "round-trip 必须逐字段一致");
    }

    #[test]
    fn validate_accepts_a_well_formed_dag() {
        let def = Expr::pure(1.0).with_id("a");
        let root = Expr::call(math("sum"), [Expr::reference("a"), def]);
        root.validate().unwrap();
    }

    #[test]
    fn validate_rejects_dangling_ref() {
        let err = Expr::reference("missing").validate().unwrap_err();
        assert_eq!(err, ExprError::DanglingRef("missing".to_string()));
    }

    #[test]
    fn validate_rejects_duplicate_ids() {
        let root = Expr::call(
            math("sum"),
            [Expr::pure(1.0).with_id("dup"), Expr::pure(2.0).with_id("dup")],
        );
        assert_eq!(
            root.validate().unwrap_err(),
            ExprError::DuplicateId("dup".to_string())
        );
    }

    #[test]
    fn validate_rejects_cycles() {
        let a = Expr::call(math("sum"), [Expr::reference("b")]).with_id("a");
        let b = Expr::call(math("sum"), [Expr::reference("a")]).with_id("b");
        let root = Expr::call(math("sum"), [a, b]);
        let err = root.validate().unwrap_err();
        assert!(
            matches!(err, ExprError::Cycle(ref path) if path.contains("a") && path.contains("b")),
            "期望环错误，实得 {err:?}"
        );
    }

    #[test]
    fn op_ref_parses_qualified_names() {
        assert_eq!(
            OpRef::parse("zela.math::sin"),
            Some(OpRef::new("zela.math", "sin"))
        );
        assert_eq!(OpRef::parse("sin"), Some(OpRef::new("", "sin")));
        assert_eq!(OpRef::parse(""), None);
        assert_eq!(OpRef::parse("zela.math::"), None);
        assert_eq!(OpRef::new("zela.math", "sin").to_string(), "zela.math::sin");
    }
}
