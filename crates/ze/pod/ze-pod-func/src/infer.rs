//! **装配期类型推导**（L2）：rank-1 合一 + kind 约束 + 重载列表。
//!
//! # 为什么不进产物
//!
//! 推导结果只服务装配期：显式拒绝类型错误的表达式、核对根表达式与目标通道。
//! 运行期不需要类型（`Value` 自带标签），因此结果不进 payload —— 与
//! [`ze_func::ChannelRef::ty`] 的 `#[serde(skip)]` 同一口径。
//!
//! # 算法
//!
//! - 每条签名**独立实例化**（不做 let-polymorphism）：类型变量是 rank-1 的，
//!   只出现在一条签名内部；
//! - 合一 = 槽位替换（union-find 的简化形态）+ occurs check；
//! - 一个算子有多条签名时**逐条尝试**：0 条成功 ⇒ 类型错误；>1 条成功 ⇒
//!   **歧义错误**（硬约束 4：不静默选第一条）；
//! - 错误必带来源：哪条签名、哪个参数位置。
//!
//! # 为什么叶子类型是封闭的
//!
//! 推导结果只有 [`ChannelType`] 的四个具体值（数值 / 布尔 / Vec2 / Vec3）：
//! 叶子（字面量 / 通道）都是具体的，调用结果的类型变量又被实参约束，因此
//! 不会出现逃逸的自由变量。这使「根类型」可以直接交给绑定层使用。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use ze_func::{ChannelType, Expr, Literal, Node};
use ze_pod_core::{Kind, OpDef, OpSig, ScalarKind, TypeSpec};

use crate::signature::signature_of;

/// 类型推导错误（全部显式；调用方负责补上"哪条绑定"等上下文）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeError {
    message: String,
}

impl TypeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// 错误文本（便于调用方拼接上下文）。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TypeError {}

/// 推导整棵树，返回根节点的具体类型。
pub fn infer(expr: &Expr) -> Result<ChannelType, TypeError> {
    let mut context = Infer {
        defs: definitions(expr),
        memo: HashMap::new(),
        visiting: HashSet::new(),
    };
    context.type_of(expr)
}

/// 绑定根：必须是可写动画值（数值 / 向量）。
///
/// `Bool` 是 predicate 的专利 —— 让它悄悄写进动画通道是本层要消灭的静默。
pub fn infer_root(expr: &Expr) -> Result<ChannelType, TypeError> {
    match infer(expr)? {
        ChannelType::Bool => Err(TypeError::new(
            "绑定根表达式是 Bool；动画通道只接受数值或向量（Bool 只用于 predicate）",
        )),
        ty => Ok(ty),
    }
}

/// predicate：必须是 `Bool` 表达式。
pub fn infer_predicate(expr: &Expr) -> Result<(), TypeError> {
    let ty = infer(expr)?;
    if ty == ChannelType::Bool {
        Ok(())
    } else {
        Err(TypeError::new(format!(
            "predicate 必须是 Bool 表达式，实际推导为 {}",
            ty.name()
        )))
    }
}

// ─── 合一 ─────────────────────────────────────────────────────────────

/// 推导中的类型：具体类型或签名内的类型变量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ty {
    Concrete(ChannelType),
    Var(u32),
}

/// 一条签名的实例化环境：变量名 / kind 约束 / 已绑定值。
struct Subst {
    names: Vec<&'static str>,
    kinds: Vec<Kind>,
    binds: Vec<Option<Ty>>,
}

impl Subst {
    fn new(sig: &OpSig) -> Self {
        Self {
            names: sig.vars.iter().map(|(name, _)| *name).collect(),
            kinds: sig.vars.iter().map(|(_, kind)| *kind).collect(),
            binds: vec![None; sig.vars.len()],
        }
    }

    fn var(&self, name: &str) -> Option<u32> {
        self.names
            .iter()
            .position(|candidate| *candidate == name)
            .map(|index| index as u32)
    }

    /// 顺着绑定走到底（rank-1 ⇒ 链很短；上限只是防御）。
    fn resolve(&self, ty: Ty) -> Ty {
        let mut ty = ty;
        for _ in 0..64 {
            match ty {
                Ty::Var(index) => match self.binds[index as usize] {
                    Some(next) => ty = next,
                    None => break,
                },
                Ty::Concrete(_) => break,
            }
        }
        ty
    }

    /// occurs check：变量不能通过绑定出现在自己的解里。
    fn occurs(&self, var: u32, ty: Ty) -> bool {
        match self.resolve(ty) {
            Ty::Concrete(_) => false,
            Ty::Var(other) => other == var,
        }
    }

    fn bind(&mut self, var: u32, value: Ty, context: &str) -> Result<(), String> {
        let concrete = match self.resolve(value) {
            Ty::Concrete(ty) => ty,
            Ty::Var(_) => {
                return Err(format!(
                    "{context}：类型变量 `{}` 没有被具体类型约束",
                    self.names[var as usize]
                ))
            }
        };
        let required = self.kinds[var as usize];
        if kind_of(concrete) != required {
            return Err(format!(
                "{context}：类型变量 `{}` 要求 {}，实际 {}",
                self.names[var as usize],
                required.name(),
                concrete.name()
            ));
        }
        if self.occurs(var, value) {
            return Err(format!(
                "{context}：类型变量 `{}` 出现自引用（occurs check）",
                self.names[var as usize]
            ));
        }
        self.binds[var as usize] = Some(value);
        Ok(())
    }

    fn unify(&mut self, expected: Ty, actual: Ty, context: &str) -> Result<(), String> {
        match (self.resolve(expected), self.resolve(actual)) {
            (Ty::Concrete(a), Ty::Concrete(b)) => {
                if a == b {
                    Ok(())
                } else {
                    Err(format!("{context}：期待 {}，实际 {}", a.name(), b.name()))
                }
            }
            (Ty::Var(a), Ty::Var(b)) if a == b => Ok(()),
            (Ty::Var(a), Ty::Var(b)) => Err(format!(
                "{context}：类型变量 `{}` 与 `{}` 被要求相等（签名未声明该约束）",
                self.names[a as usize], self.names[b as usize]
            )),
            (Ty::Var(var), other) | (other, Ty::Var(var)) => self.bind(var, other, context),
        }
    }

    fn bind_spec(
        &mut self,
        spec: &TypeSpec,
        actual: ChannelType,
        context: &str,
    ) -> Result<(), String> {
        let expected = self.spec_ty(spec)?;
        self.unify(expected, Ty::Concrete(actual), context)
    }

    fn resolve_spec(&self, spec: &TypeSpec) -> Result<ChannelType, String> {
        match self.resolve(self.spec_ty(spec)?) {
            Ty::Concrete(ty) => Ok(ty),
            Ty::Var(index) => Err(format!(
                "类型变量 `{}` 没有被实参约束（返回类型不确定）",
                self.names[index as usize]
            )),
        }
    }

    fn spec_ty(&self, spec: &TypeSpec) -> Result<Ty, String> {
        match spec {
            TypeSpec::Var(name) => self
                .var(name)
                .map(Ty::Var)
                .ok_or_else(|| format!("签名里的类型变量 `{name}` 未在 vars 中声明")),
            other => spec_concrete(other).map(Ty::Concrete),
        }
    }
}

/// 签名里的具体类型 → 表达式层类型。
fn spec_concrete(spec: &TypeSpec) -> Result<ChannelType, String> {
    match spec {
        TypeSpec::Scalar(kind) => match kind {
            ScalarKind::F32
            | ScalarKind::F64
            | ScalarKind::I32
            | ScalarKind::U32
            | ScalarKind::U64
            | ScalarKind::U8 => Ok(ChannelType::Number),
            ScalarKind::Bool => Ok(ChannelType::Bool),
            ScalarKind::String => Err("签名里的 string 不是动画值类型".to_string()),
        },
        TypeSpec::Struct { name, .. } | TypeSpec::Enum { name, .. } | TypeSpec::Opaque(name) => {
            match short(name) {
                "Vec2" => Ok(ChannelType::Vec2),
                "Vec3" => Ok(ChannelType::Vec3),
                other => Err(format!("签名里的类型 `{other}` 尚未接入表达式类型系统")),
            }
        }
        TypeSpec::Array { .. } | TypeSpec::App { .. } => {
            Err("签名里的数组 / 类型构造应用不支持（rank-1 限定）".to_string())
        }
        TypeSpec::Var(_) => unreachable!("类型变量由 `spec_ty` 处理"),
    }
}

fn short(name: &str) -> &str {
    name.strip_prefix("zela.").unwrap_or(name)
}

/// 具体类型 → kind（`Unknown` 只可能来自未接线的通道，推导前已拒绝）。
fn kind_of(ty: ChannelType) -> Kind {
    match ty {
        ChannelType::Number | ChannelType::Unknown => Kind::Num,
        ChannelType::Vec2 | ChannelType::Vec3 => Kind::Vector,
        ChannelType::Bool => Kind::Bool,
        ChannelType::Path => Kind::Path,
    }
}

/// 逐条尝试一个算子的重载。
fn unify_overloads(
    def: &OpDef,
    args: &[ChannelType],
) -> Result<ChannelType, TypeError> {
    let mut successes: Vec<(ChannelType, String)> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    for sig in def.sigs {
        match try_sig(sig, args) {
            Ok(ty) => successes.push((ty, sig.render())),
            Err(reason) => failures.push(format!("  - {} —— {}", sig.render(), reason)),
        }
    }
    let display = display_name(def.name);
    let actual = args
        .iter()
        .map(|ty| ty.name())
        .collect::<Vec<_>>()
        .join(", ");
    match successes.len() {
        0 => Err(TypeError::new(format!(
            "算子 `{display}` 的类型不匹配：实参 ({actual})\n{}",
            failures.join("\n")
        ))),
        1 => Ok(successes[0].0),
        _ => {
            let matched = successes
                .iter()
                .map(|(_, render)| format!("`{render}`"))
                .collect::<Vec<_>>()
                .join("；");
            Err(TypeError::new(format!(
                "算子 `{display}` 的类型有歧义：{} 条签名同时匹配（{matched}）—— 请显式收窄实参类型",
                successes.len()
            )))
        }
    }
}

/// 用一条签名合一实参类型。
fn try_sig(sig: &OpSig, args: &[ChannelType]) -> Result<ChannelType, String> {
    let arity = sig.params.len();
    if arity == 0 {
        if args.is_empty() {
            return Subst::new(sig).resolve_spec(sig.ret);
        }
        return Err(format!("签名没有形参，实际给了 {} 个实参", args.len()));
    }
    if sig.variadic {
        if args.len() < arity {
            return Err(format!("至少需要 {arity} 个实参，实际 {}", args.len()));
        }
    } else if args.len() != arity {
        return Err(format!("需要 {arity} 个实参，实际 {}", args.len()));
    }
    let mut subst = Subst::new(sig);
    for (index, actual) in args.iter().enumerate() {
        // 变参只声明元素类型：多出来的实参全部对最后一条参数合一。
        let position = index.min(arity - 1);
        let context = format!("第 {} 个参数", index + 1);
        subst.bind_spec(sig.params[position], *actual, &context)?;
    }
    subst.resolve_spec(sig.ret)
}

/// `and` → `And`（与声明文件里的构造器名一致）。
fn display_name(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// ─── 树的推导 ─────────────────────────────────────────────────────────

struct Infer<'a> {
    defs: BTreeMap<&'a str, &'a Expr>,
    memo: HashMap<&'a str, ChannelType>,
    visiting: HashSet<&'a str>,
}

impl<'a> Infer<'a> {
    fn type_of(&mut self, expr: &'a Expr) -> Result<ChannelType, TypeError> {
        match &expr.node {
            Node::Pure { value } => literal_type(value),
            Node::Channel { channel } => match channel.ty {
                ChannelType::Unknown => Err(TypeError::new(format!(
                    "通道 `{channel}` 缺少类型信息（装配期无法推导；组件字段通道应带 `ty`）"
                ))),
                ty => Ok(ty),
            },
            Node::Read { channel, .. } => match channel.ty {
                ChannelType::Unknown => Err(TypeError::new(format!(
                    "读通道 `{channel}` 缺少类型信息（装配期无法推导；组件字段通道应带 `ty`）"
                ))),
                ty => Ok(ty),
            },
            Node::Call { op, args } => {
                let arg_types = args
                    .iter()
                    .map(|arg| self.type_of(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let def = signature_of(&op.name).ok_or_else(|| {
                    TypeError::new(format!("算子 `{op}` 没有类型签名（L2 表未登记）"))
                })?;
                unify_overloads(def, &arg_types)
            }
            Node::Ref { to } => self.ref_type(to.as_str()),
            Node::Select { cond, then, r#else } => {
                let cond_ty = self.type_of(cond)?;
                if cond_ty != ChannelType::Bool {
                    return Err(TypeError::new(format!(
                        "`Select.cond` 必须是 Bool，实际 {}",
                        cond_ty.name()
                    )));
                }
                let then_ty = self.type_of(then)?;
                let else_ty = self.type_of(r#else)?;
                if then_ty != else_ty {
                    return Err(TypeError::new(format!(
                        "`Select` 两个分支类型不同：{} / {}",
                        then_ty.name(),
                        else_ty.name()
                    )));
                }
                Ok(then_ty)
            }
        }
    }

    fn ref_type(&mut self, to: &'a str) -> Result<ChannelType, TypeError> {
        if let Some(ty) = self.memo.get(to) {
            return Ok(*ty);
        }
        if !self.visiting.insert(to) {
            return Err(TypeError::new(format!("类型推导遇到环：`{to}` 引用自身")));
        }
        let target = *self
            .defs
            .get(to)
            .ok_or_else(|| TypeError::new(format!("悬空的 Ref：`{to}` 没有定义")))?;
        let ty = self.type_of(target)?;
        self.visiting.remove(to);
        self.memo.insert(to, ty);
        Ok(ty)
    }
}

fn literal_type(value: &Literal) -> Result<ChannelType, TypeError> {
    match value {
        Literal::F32(_) | Literal::F64(_) | Literal::I32(_) => Ok(ChannelType::Number),
        Literal::Bool(_) => Ok(ChannelType::Bool),
        Literal::Vec2(_) => Ok(ChannelType::Vec2),
        Literal::Vec3(_) => Ok(ChannelType::Vec3),
        Literal::Text(_) => Err(TypeError::new(
            "文本字面量不能作为动画值（没有通道接受 string）",
        )),
    }
}

fn definitions(expr: &Expr) -> BTreeMap<&str, &Expr> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ze_func::ChannelRef;
    use ze_func::OpRef;

    fn op(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    fn num(value: f64) -> Expr {
        Expr::pure(value)
    }

    fn vec2() -> Expr {
        Expr::pure([1.0_f32, 2.0])
    }

    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::call(op(name), args)
    }

    #[test]
    fn unary_numeric_operators_are_number() {
        assert_eq!(
            infer(&call("sin", vec![num(1.0)])).expect("sin 可推导"),
            ChannelType::Number
        );
    }

    #[test]
    fn vec2_times_number_is_vec2() {
        // 验收：`Vec2 * F64 -> Vec2`（返回类型绑在向量那一侧）。
        assert_eq!(
            infer(&call("mul", vec![vec2(), num(2.0)])).expect("Vec2 * F64"),
            ChannelType::Vec2
        );
        assert_eq!(
            infer(&call("mul", vec![num(2.0), vec2()])).expect("F64 * Vec2"),
            ChannelType::Vec2
        );
    }

    #[test]
    fn and_rejects_numbers_with_a_source_location() {
        // 验收：`And{f64, f64}` 在装配期被拒，错误带签名与参数位置。
        let error = infer(&call("and", vec![num(1.0), num(2.0)])).expect_err("And 只吃 Bool");
        let message = error.to_string();
        assert!(message.contains("And"), "{message}");
        assert!(message.contains("Bool"), "{message}");
        assert!(message.contains("第 1 个参数"), "{message}");
    }

    #[test]
    fn unknown_channel_type_is_rejected() {
        let expr = Expr::channel(ChannelRef::new("zela.scene", "Circle.radius"));
        assert!(infer(&expr).is_err(), "未知通道类型必须显式失败");
    }

    /// **G 批**：`Read` 与 `Channel` 同形 —— 带 `ty` 可推导，`Unknown` 显式失败。
    #[test]
    fn read_infers_from_channel_type() {
        let typed = ChannelRef::new("zela.scene", "Circle.radius").with_type(ChannelType::Number);
        assert_eq!(
            infer(&Expr::read(typed.clone(), 3)).expect("带 ty 的 Read 可推导"),
            ChannelType::Number
        );
        assert!(infer(&Expr::read(typed, 3)).is_ok());
        assert!(
            infer(&Expr::read(ChannelRef::new("zela.scene", "Foo.bar"), 3)).is_err(),
            "Unknown 类型的 Read 必须显式失败"
        );
    }

    #[test]
    fn predicate_must_be_bool() {
        assert!(infer_predicate(&num(1.0)).is_err());
        assert!(infer_predicate(&call("lt", vec![num(1.0), num(2.0)])).is_ok());
    }

    #[test]
    fn root_cannot_be_bool() {
        let compare = call("lt", vec![num(0.0), num(1.0)]);
        assert!(infer_root(&compare).is_err());
    }

    #[test]
    fn ref_inherits_the_definition_type() {
        // `v = Vec2 * F64`；`v + v` ⇒ Vec2（Ref 继承定义的类型）。
        let definition = call("mul", vec![vec2(), num(2.0)]).with_id("v");
        let root = call("add", vec![definition, Expr::reference("v")]);
        assert_eq!(infer(&root).expect("Ref 可推导"), ChannelType::Vec2);
    }

    #[test]
    fn ambiguous_overloads_are_explicit() {
        static F64: &TypeSpec = &TypeSpec::Scalar(ScalarKind::F64);
        const SIG: OpSig = OpSig {
            params: &[F64],
            ret: F64,
            vars: &[],
            variadic: false,
        };
        static DUPLICATED: &[OpSig] = &[SIG, SIG];
        let def = OpDef {
            name: "dup",
            spec: "test.Dup",
            sigs: DUPLICATED,
            since: 1,
            changed: 1,
        };
        let error = unify_overloads(&def, &[ChannelType::Number]).expect_err("必须报歧义");
        assert!(error.to_string().contains("歧义"), "{error}");
    }
}
