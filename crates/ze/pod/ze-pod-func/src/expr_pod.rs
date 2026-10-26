//! 表达式 Pod：**表达式数据留在 Pod 内，跨边界只传 [`Handle`]**。
//!
//! # 形态
//!
//! 与 `SceneServicePod` 的组件句柄同构：`HandleTable<Expr>` + 一组 `fn(&dyn Pod, ..)`
//! 叶子算子。`RegistryPod` 提供 typed / erased 两张调用表（快慢路径同源），
//! 本 Pod 只多一件事：**内部可变的状态**（表达式存储）。
//!
//! # 为什么不做 `Ref` 折叠
//!
//! S1 的格式已保留 `Ref`，但 S2 的导出仍按**树**展开：同一个句柄用两次会得到两棵
//! 相同子树。共享是 S3「一次读取、多处使用」的前提，届时在绑定层折叠成 `Ref`；
//! 现在做等于为假想需求预付。
//!
//! # 错误
//!
//! 算子签名是 `fn(..) -> Handle`（无 `Result`）：**脚本拿不到非法句柄** —— 句柄只能由
//! 本 Pod 的函数产出。因此这里的 `expect` 是"内部不变量"而非用户输入校验；
//! 用户输入（字段类型 / 缺字段）由绑定层在跨边界时显式报错。

use std::any::Any;
use std::sync::Mutex;

use ze_func::{ChannelRef, CompiledExpr, Expr, Literal, Node, OpRef};
use ze_pod_core::{FunctionId, Handle, HandleTable, Pod, PodError, PodId, QualifiedName, RegistryPod};

use crate::api::{
    AddSpec, AndSpec, BindSpec, ChannelSpec, ClockLinkSpec, ClockSpec, CosSpec, EqSpec, GeSpec,
    GtSpec, LeSpec, LtSpec, MulSpec, NeSpec, NotSpec, OrSpec, PredicateSpec, ResetSpec, ScaleSpec,
    SinSpec, SumSpec,
};

/// 函子库 namespace（进产物）。
pub const LIBRARY_NAMESPACE: &str = "zela.math";
/// 函子库版本（进产物；加算子递增它，`zela.scene` 不受影响）。
///
/// `6`：绑定带时钟（`Bind.clock`），产物里的 `Step::Expr` 可携带非全局时钟。
pub const LIBRARY_VERSION: &str = "6";

/// 组件字段通道的命名空间（`Circle.radius` 属于场景地址表）。
pub const SCENE_NAMESPACE: &str = "zela.scene";

const HANDLE: &str = "ze_pod_core.Handle";

/// 一条待并入 `SceneScript` 的表达式动画（S3）。
#[derive(Clone, Debug)]
pub struct PendingBinding {
    /// 场景对象**句柄**（pod 侧；装配期转成本脚本内的局部句柄）。
    pub object: Handle,
    /// 目标通道（`name` 是 `zela.scene` 的地址表键）。
    pub target: ChannelSpec,
    pub expr: Expr,
    /// 该绑定的时钟（空链 = 全局时钟）。
    pub clock: ClockSpec,
    /// 生效条件（`None` = 恒真）。
    pub predicate: Option<Expr>,
    /// 失效时行为。
    pub reset: ResetSpec,
    /// 语法糖：`> 0` ⇒ 先生成 predicate 后由场景层展开（`predicate` 必须为 None）。
    pub duration: f64,
}

/// 表达式 Pod。
pub struct ExprPod {
    id: PodId,
    registry: RegistryPod,
    store: Mutex<HandleTable<Expr>>,
    /// 脚本登记的表达式动画（宿主在脚本结束后取走）。
    bindings: Mutex<Vec<PendingBinding>>,
    /// 装配期错误（未知通道 / 未知算子 / 非法句柄）—— 宿主取走时显式失败。
    errors: Mutex<Vec<String>>,
}

impl ExprPod {
    /// 新建（装配期调用一次）。
    pub fn new() -> Self {
        let id = PodId::next();
        let mut registry = RegistryPod::new();

        registry.register_fn0_named("zela.math::time", HANDLE, time);
        registry.register_fn0_named("zela.math::frame", HANDLE, frame);
        registry.register_fn0_named("zela.math::event", HANDLE, event);
        registry.register_fn0_named(
            "zela.math::component_state",
            HANDLE,
            component_state,
        );
        registry.register_fn0_named("zela.math::env", HANDLE, env);

        registry.register_fn1_named::<f64, Handle>("zela.math::pure", "f64", HANDLE, pure);
        registry.register_fn1_named::<SinSpec, Handle>("zela.math::sin", "zela.math.Sin", HANDLE, sin_ctor);
        registry.register_fn1_named::<CosSpec, Handle>("zela.math::cos", "zela.math.Cos", HANDLE, cos_ctor);
        registry.register_fn1_named::<Handle, String>(
            "zela.math::expression",
            HANDLE,
            "String",
            expression,
        );
        registry.register_fn1_named::<AddSpec, Handle>("zela.math::add", "zela.math.Add", HANDLE, add_ctor);
        registry.register_fn1_named::<MulSpec, Handle>("zela.math::mul", "zela.math.Mul", HANDLE, mul_ctor);
        registry.register_fn1_named::<ScaleSpec, Handle>(
            "zela.math::scale",
            "zela.math.Scale",
            HANDLE,
            scale_ctor,
        );
        registry.register_fn1_named::<SumSpec, Handle>("zela.math::sum", "zela.math.Sum", HANDLE, sum_ctor);
        // 比较 / 布尔组合子（predicate 的原料；值语义，Bool 不进 `anim_value`）。
        registry.register_fn1_named::<LtSpec, Handle>("zela.math::lt", "zela.math.Lt", HANDLE, lt_ctor);
        registry.register_fn1_named::<GtSpec, Handle>("zela.math::gt", "zela.math.Gt", HANDLE, gt_ctor);
        registry.register_fn1_named::<LeSpec, Handle>("zela.math::le", "zela.math.Le", HANDLE, le_ctor);
        registry.register_fn1_named::<GeSpec, Handle>("zela.math::ge", "zela.math.Ge", HANDLE, ge_ctor);
        registry.register_fn1_named::<EqSpec, Handle>("zela.math::eq", "zela.math.Eq", HANDLE, eq_ctor);
        registry.register_fn1_named::<NeSpec, Handle>("zela.math::ne", "zela.math.Ne", HANDLE, ne_ctor);
        registry.register_fn1_named::<AndSpec, Handle>("zela.math::and", "zela.math.And", HANDLE, and_ctor);
        registry.register_fn1_named::<OrSpec, Handle>("zela.math::or", "zela.math.Or", HANDLE, or_ctor);
        registry.register_fn1_named::<NotSpec, Handle>("zela.math::not", "zela.math.Not", HANDLE, not_ctor);
        // 时钟（B 批）：脚本侧构造纯线性时间源；`bind` 收它，取代隐式 cursor。
        registry.register_fn1_named::<f64, ClockSpec>(
            "zela.math::clock_shift",
            "f64",
            "zela.math.Clock",
            clock_shift,
        );
        registry.register_fn1_named::<f64, ClockSpec>(
            "zela.math::clock_scale",
            "f64",
            "zela.math.Clock",
            clock_scale,
        );
        registry.register_fn2_named::<ClockSpec, ClockSpec, ClockSpec>(
            "zela.math::clock_nest",
            "zela.math.Clock",
            "zela.math.Clock",
            "zela.math.Clock",
            clock_nest,
        );
        registry.register_fn0_named("zela.math::clock_time", HANDLE, clock_time);
        registry.register_fn1_named::<BindSpec, ()>(
            "zela.math::bind",
            "zela.math.Bind",
            "()",
            bind,
        );

        Self {
            id,
            registry,
            store: Mutex::new(HandleTable::new(id)),
            bindings: Mutex::new(Vec::new()),
            errors: Mutex::new(Vec::new()),
        }
    }

    /// 取走脚本登记的表达式动画与装配期错误（宿主在 `run` 之后调用）。
    pub fn take_bindings(&self) -> (Vec<PendingBinding>, Vec<String>) {
        let bindings = std::mem::take(
            &mut *self
                .bindings
                .lock()
                .expect("表达式绑定表只在持有期间被 panic 毒化"),
        );
        let errors = std::mem::take(
            &mut *self
                .errors
                .lock()
                .expect("表达式错误表只在持有期间被 panic 毒化"),
        );
        (bindings, errors)
    }

    /// 取回句柄对应的表达式（宿主侧 / 测试用）。
    pub fn tree(&self, root: Handle) -> Result<Expr, PodError> {
        let table = self
            .store
            .lock()
            .map_err(|_| PodError::CallFailed("表达式存储锁中毒".to_string()))?;
        table.get(&root).cloned()
    }

    /// 导出表达式 JSON（产物侧证据）。
    ///
    /// 含 `schema_version` 与函子库 `namespace + version` —— 声明文件与产物都由它派生。
    pub fn export(&self, root: Handle) -> Result<String, PodError> {
        let expr = self.tree(root)?;
        Ok(serde_json::json!({
            "schema_version": ze_func::SCHEMA_VERSION,
            "library": {
                "namespace": LIBRARY_NAMESPACE,
                "version": LIBRARY_VERSION,
            },
            "expr": serde_json::to_value(&expr)
                .expect("表达式树只含字符串键，序列化不可失败"),
        })
        .to_string())
    }

    fn intern(&self, expr: Expr) -> Handle {
        self.store
            .lock()
            .expect("表达式存储锁只在持有期间被 panic 毒化")
            .insert(expr)
    }
}

impl Default for ExprPod {
    fn default() -> Self {
        Self::new()
    }
}

impl Pod for ExprPod {
    fn id(&self) -> PodId {
        self.id
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn functions(&self) -> &[QualifiedName] {
        self.registry.functions()
    }

    fn resolve(&self, name: &QualifiedName) -> Option<&(dyn Any + Send + Sync)> {
        self.registry.resolve(name)
    }

    /// 慢路径：按函数名分派，**与快路径调用同一组 helper**（结果必然一致）。
    fn call(&self, id: FunctionId, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError> {
        let name = self
            .functions()
            .get(id.get() as usize)
            .ok_or_else(|| PodError::UnknownFunction(format!("函数编号 {id}")))?;
        Ok(match name.name() {
            "pure" => Box::new(pure(self, *arg::<f64>(args, 0)?)),
            "time" => Box::new(time(self)),
            "frame" => Box::new(frame(self)),
            "event" => Box::new(event(self)),
            "component_state" => Box::new(component_state(self)),
            "env" => Box::new(env(self)),
            "sin" => Box::new(sin_ctor(self, arg::<SinSpec>(args, 0)?.clone())),
            "cos" => Box::new(cos_ctor(self, arg::<CosSpec>(args, 0)?.clone())),
            "add" => Box::new(add_ctor(self, arg::<AddSpec>(args, 0)?.clone())),
            "mul" => Box::new(mul_ctor(self, arg::<MulSpec>(args, 0)?.clone())),
            "scale" => Box::new(scale_ctor(self, arg::<ScaleSpec>(args, 0)?.clone())),
            "sum" => Box::new(sum_ctor(self, arg::<SumSpec>(args, 0)?.clone())),
            "lt" => Box::new(lt_ctor(self, arg::<LtSpec>(args, 0)?.clone())),
            "gt" => Box::new(gt_ctor(self, arg::<GtSpec>(args, 0)?.clone())),
            "le" => Box::new(le_ctor(self, arg::<LeSpec>(args, 0)?.clone())),
            "ge" => Box::new(ge_ctor(self, arg::<GeSpec>(args, 0)?.clone())),
            "eq" => Box::new(eq_ctor(self, arg::<EqSpec>(args, 0)?.clone())),
            "ne" => Box::new(ne_ctor(self, arg::<NeSpec>(args, 0)?.clone())),
            "and" => Box::new(and_ctor(self, arg::<AndSpec>(args, 0)?.clone())),
            "or" => Box::new(or_ctor(self, arg::<OrSpec>(args, 0)?.clone())),
            "not" => Box::new(not_ctor(self, arg::<NotSpec>(args, 0)?.clone())),
            "clock_shift" => Box::new(clock_shift(self, *arg::<f64>(args, 0)?)),
            "clock_scale" => Box::new(clock_scale(self, *arg::<f64>(args, 0)?)),
            "clock_nest" => Box::new(clock_nest(
                self,
                arg::<ClockSpec>(args, 0)?.clone(),
                arg::<ClockSpec>(args, 1)?.clone(),
            )),
            "clock_time" => Box::new(clock_time(self)),
            "expression" => Box::new(expression(self, *arg::<Handle>(args, 0)?)),
            "bind" => Box::new(bind(self, arg::<BindSpec>(args, 0)?.clone())),
            other => {
                return Err(PodError::UnknownFunction(format!(
                    "{}::{other}",
                    name.namespace()
                )))
            }
        })
    }
}

fn arg<'a, T: 'static>(args: &'a [&'a dyn Any], index: usize) -> Result<&'a T, PodError> {
    args.get(index)
        .and_then(|value| value.downcast_ref::<T>())
        .ok_or_else(|| PodError::CallFailed(format!("第 {index} 个实参类型不匹配")))
}

fn expr_pod(pod: &dyn Pod) -> &ExprPod {
    pod.as_any()
        .downcast_ref::<ExprPod>()
        .expect("ExprPod 的函数只能由 ExprPod 自己调用")
}

fn op(name: &str) -> OpRef {
    OpRef::new(LIBRARY_NAMESPACE, name)
}

fn child(pod: &ExprPod, handle: Handle) -> Expr {
    pod.tree(handle)
        .expect("表达式句柄必须由本 Pod 产出且未释放")
}

fn leaf_channel(pod: &dyn Pod, channel: ChannelRef) -> Handle {
    expr_pod(pod).intern(Expr::channel(channel))
}

fn call1(pod: &dyn Pod, name: &str, of: Handle) -> Handle {
    let this = expr_pod(pod);
    let arg = child(this, of);
    this.intern(Expr::call(op(name), [arg]))
}

fn call2(pod: &dyn Pod, name: &str, a: Handle, b: Handle) -> Handle {
    let this = expr_pod(pod);
    let left = child(this, a);
    let right = child(this, b);
    this.intern(Expr::call(op(name), [left, right]))
}

fn pure(pod: &dyn Pod, value: f64) -> Handle {
    expr_pod(pod).intern(Expr::pure(Literal::F64(value)))
}

fn time(pod: &dyn Pod) -> Handle {
    leaf_channel(pod, ChannelRef::builtin("time"))
}

fn frame(pod: &dyn Pod) -> Handle {
    leaf_channel(pod, ChannelRef::builtin("frame"))
}

fn event(pod: &dyn Pod) -> Handle {
    leaf_channel(pod, ChannelRef::builtin("event"))
}

fn component_state(pod: &dyn Pod) -> Handle {
    leaf_channel(pod, ChannelRef::builtin("component_state"))
}

fn env(pod: &dyn Pod) -> Handle {
    leaf_channel(pod, ChannelRef::builtin("env"))
}

fn sin(pod: &dyn Pod, of: Handle) -> Handle {
    call1(pod, "sin", of)
}

fn cos(pod: &dyn Pod, of: Handle) -> Handle {
    call1(pod, "cos", of)
}

fn add(pod: &dyn Pod, a: Handle, b: Handle) -> Handle {
    call2(pod, "add", a, b)
}

fn mul(pod: &dyn Pod, a: Handle, b: Handle) -> Handle {
    call2(pod, "mul", a, b)
}

fn scale(pod: &dyn Pod, k: f64, of: Handle) -> Handle {
    let this = expr_pod(pod);
    let of = child(this, of);
    // `k` 是装配期算术的结果 ⇒ 直接是字面量叶子（§5.4）。
    this.intern(Expr::call(op("scale"), [Expr::pure(Literal::F64(k)), of]))
}

fn sum(pod: &dyn Pod, terms: Vec<Handle>) -> Handle {
    let this = expr_pod(pod);
    if terms.is_empty() {
        return this.intern(Expr::pure(Literal::F64(0.0)));
    }
    let terms: Vec<Expr> = terms.into_iter().map(|handle| child(this, handle)).collect();
    this.intern(Expr::call(op("sum"), terms))
}

// ─── 表构造器入口（`Sin { of = ... }` 等）──────────────────────────────

fn sin_ctor(pod: &dyn Pod, spec: SinSpec) -> Handle {
    sin(pod, spec.of)
}

fn cos_ctor(pod: &dyn Pod, spec: CosSpec) -> Handle {
    cos(pod, spec.of)
}

fn add_ctor(pod: &dyn Pod, spec: AddSpec) -> Handle {
    add(pod, spec.a, spec.b)
}

fn mul_ctor(pod: &dyn Pod, spec: MulSpec) -> Handle {
    mul(pod, spec.a, spec.b)
}

fn scale_ctor(pod: &dyn Pod, spec: ScaleSpec) -> Handle {
    scale(pod, spec.k, spec.of)
}

fn sum_ctor(pod: &dyn Pod, spec: SumSpec) -> Handle {
    sum(pod, spec.terms)
}

fn lt_ctor(pod: &dyn Pod, spec: LtSpec) -> Handle {
    call2(pod, "lt", spec.a, spec.b)
}

fn gt_ctor(pod: &dyn Pod, spec: GtSpec) -> Handle {
    call2(pod, "gt", spec.a, spec.b)
}

fn le_ctor(pod: &dyn Pod, spec: LeSpec) -> Handle {
    call2(pod, "le", spec.a, spec.b)
}

fn ge_ctor(pod: &dyn Pod, spec: GeSpec) -> Handle {
    call2(pod, "ge", spec.a, spec.b)
}

fn eq_ctor(pod: &dyn Pod, spec: EqSpec) -> Handle {
    call2(pod, "eq", spec.a, spec.b)
}

fn ne_ctor(pod: &dyn Pod, spec: NeSpec) -> Handle {
    call2(pod, "ne", spec.a, spec.b)
}

fn and_ctor(pod: &dyn Pod, spec: AndSpec) -> Handle {
    call2(pod, "and", spec.a, spec.b)
}

fn or_ctor(pod: &dyn Pod, spec: OrSpec) -> Handle {
    call2(pod, "or", spec.a, spec.b)
}

fn not_ctor(pod: &dyn Pod, spec: NotSpec) -> Handle {
    call1(pod, "not", spec.of)
}

// ─── 时钟（B 批）───────────────────────────────────────────────────────

/// 全局时钟之上加一层 `shift`（`local = (parent_local − shift) × 1`）。
fn clock_shift(_pod: &dyn Pod, shift: f64) -> ClockSpec {
    ClockSpec {
        links: vec![ClockLinkSpec { shift, rate: 1.0 }],
    }
}

/// 全局时钟之上加一层 `rate`（`local = parent_local × rate`）。
fn clock_scale(_pod: &dyn Pod, rate: f64) -> ClockSpec {
    ClockSpec {
        links: vec![ClockLinkSpec { shift: 0.0, rate }],
    }
}

/// 嵌套：`child` 的链接接在 `parent` 之后（扁平链，无父指针）。
fn clock_nest(_pod: &dyn Pod, parent: ClockSpec, child: ClockSpec) -> ClockSpec {
    let mut links = Vec::with_capacity(parent.links.len() + child.links.len());
    links.extend(parent.links);
    links.extend(child.links);
    ClockSpec { links }
}

/// 时钟局部时间叶子 —— 与 `expr.time()` 是**同一个**通道。
fn clock_time(pod: &dyn Pod) -> Handle {
    time(pod)
}

fn expression(pod: &dyn Pod, root: Handle) -> String {
    expr_pod(pod)
        .export(root)
        .expect("表达式句柄必须由本 Pod 产出且未释放")
}

/// 装配期校验：结构合法 + 只读已注入的通道 + 算子都在本库。
fn validate_binding(expr: &Expr) -> Result<(), String> {
    CompiledExpr::compile(expr).map_err(|error| error.to_string())?;
    fn walk(expr: &Expr) -> Result<(), String> {
        match &expr.node {
            Node::Channel { channel } => {
                let injected = channel.is_builtin()
                    && matches!(channel.name.as_str(), "time" | "frame");
                if !injected {
                    return Err(format!("通道 `{channel}` 尚未由宿主注入"));
                }
            }
            Node::Call { op, .. } if !crate::ops::is_known(op) => {
                return Err(format!("未知算子 `{op}`"));
            }
            _ => {}
        }
        for child in expr.node.children() {
            walk(child)?;
        }
        Ok(())
    }
    walk(expr)
}

fn bind(pod: &dyn Pod, spec: BindSpec) {
    let this = expr_pod(pod);
    let expr = match this.tree(spec.root) {
        Ok(expr) => expr,
        Err(error) => {
            push_binding_error(this, format!("表达式句柄无效：{error}"));
            return;
        }
    };
    if let Err(message) = validate_binding(&expr) {
        push_binding_error(
            this,
            format!("表达式绑定 `{}` 非法：{message}", spec.object),
        );
        return;
    }
    // predicate 是**独立的** Bool 表达式：单独校验（Bool 不经过 `anim_value`）。
    let predicate = match spec.predicate {
        PredicateSpec::Always => None,
        PredicateSpec::Of { expr: root } => match this.tree(root) {
            Ok(tree) => {
                if let Err(message) = validate_binding(&tree) {
                    push_binding_error(
                        this,
                        format!("predicate 绑定 `{}` 非法：{message}", spec.object),
                    );
                    return;
                }
                Some(tree)
            }
            Err(error) => {
                push_binding_error(this, format!("predicate 句柄无效：{error}"));
                return;
            }
        },
    };
    if predicate.is_some() && spec.duration > 0.0 {
        push_binding_error(
            this,
            format!(
                "表达式绑定 `{}` 同时给了 predicate 与 duration（后者是 `t < d` 的糖）",
                spec.object
            ),
        );
        return;
    }
    this.bindings
        .lock()
        .expect("表达式绑定表未被毒化")
        .push(PendingBinding {
            object: spec.object,
            target: spec.target,
            expr,
            clock: spec.clock,
            predicate,
            reset: spec.reset,
            duration: spec.duration,
        });
}

fn push_binding_error(this: &ExprPod, message: String) {
    this.errors
        .lock()
        .expect("表达式错误表未被毒化")
        .push(message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_func::{Node, OpRef};

    fn pod() -> ExprPod {
        ExprPod::new()
    }

    /// 通过公开入口建树：`Σ sin(k t) / k` 的单项。
    fn harmonic(this: &ExprPod, k: f64) -> Handle {
        let t = time(this);
        let kt = {
            let k_lit = pure(this, k);
            mul(this, t, k_lit)
        };
        let s = sin(this, kt);
        scale(this, 4.0 / (std::f64::consts::PI * k), s)
    }

    #[test]
    fn builds_an_expression_tree() {
        let this = pod();
        let h = harmonic(&this, 1.0);
        let tree = this.tree(h).expect("句柄有效");
        let Node::Call { op, args } = &tree.node else {
            panic!("期望 call 节点");
        };
        assert_eq!(op, &OpRef::new(LIBRARY_NAMESPACE, "scale"));
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].node, Node::Pure { value: Literal::F64(4.0 / std::f64::consts::PI) });
        assert_eq!(
            args[1].node,
            Node::Call {
                op: OpRef::new(LIBRARY_NAMESPACE, "sin"),
                args: vec![Expr::call(
                    OpRef::new(LIBRARY_NAMESPACE, "mul"),
                    [Expr::channel(ChannelRef::builtin("time")), Expr::pure(Literal::F64(1.0))]
                )],
            }
        );
    }

    #[test]
    fn export_carries_library_identity() {
        let this = pod();
        let h = harmonic(&this, 2.0);
        let json: serde_json::Value =
            serde_json::from_str(&this.export(h).unwrap()).expect("合法 JSON");
        assert_eq!(
            json["schema_version"].as_u64(),
            Some(u64::from(ze_func::SCHEMA_VERSION))
        );
        assert_eq!(json["library"]["namespace"], LIBRARY_NAMESPACE);
        assert_eq!(json["library"]["version"], LIBRARY_VERSION);
        assert_eq!(json["expr"]["node"]["type"], "call");
    }

    #[test]
    fn channels_are_distinct_leaves() {
        let this = pod();
        let a = time(&this);
        let b = time(&this);
        let f = frame(&this);
        assert_ne!(a, b, "两次读取是两个叶子（不是共享）");
        assert_ne!(a, f);
        assert!(matches!(
            this.tree(f).unwrap().node,
            Node::Channel { channel } if channel == ChannelRef::builtin("frame")
        ));
    }

    #[test]
    fn empty_sum_is_a_literal_zero() {
        let this = pod();
        let h = sum(&this, Vec::new());
        assert_eq!(this.tree(h).unwrap().node, Node::Pure { value: Literal::F64(0.0) });
    }

    /// **本批 A**：比较算子建出 `call` 节点，结果类型是 Bool（不经过 `anim_value`）。
    #[test]
    fn comparison_operators_build_bool_calls() {
        let this = pod();
        let t = time(&this);
        let two = pure(&this, 2.0);
        let lt = lt_ctor(
            &this,
            LtSpec {
                a: t,
                b: two,
            },
        );
        let Node::Call { op, args } = &this.tree(lt).unwrap().node else {
            panic!("期望 call 节点");
        };
        assert_eq!(op, &OpRef::new(LIBRARY_NAMESPACE, "lt"));
        assert_eq!(args.len(), 2);

        // `And` / `Not` 也是值算子：结构上只是 call。
        let and = and_ctor(
            &this,
            AndSpec {
                a: lt,
                b: lt,
            },
        );
        let not = not_ctor(&this, NotSpec { of: and });
        assert!(matches!(
            this.tree(not).unwrap().node,
            Node::Call { ref op, .. } if op.name == "not"
        ));
    }

    /// **B 批**：脚本侧时钟是**数据**（扁平链），`clock.time()` 与 `expr.time()` 同源。
    #[test]
    fn clock_builders_produce_flat_links() {
        let this = pod();
        assert_eq!(
            clock_shift(&this, 2.0),
            ClockSpec {
                links: vec![ClockLinkSpec {
                    shift: 2.0,
                    rate: 1.0
                }]
            }
        );
        assert_eq!(
            clock_scale(&this, 0.5),
            ClockSpec {
                links: vec![ClockLinkSpec {
                    shift: 0.0,
                    rate: 0.5
                }]
            }
        );
        let nested = clock_nest(&this, clock_scale(&this, 0.5), clock_shift(&this, 1.0));
        assert_eq!(nested.links.len(), 2, "嵌套 = 两条链拼接");
        assert_eq!(nested.links[0].rate, 0.5);
        assert_eq!(nested.links[1].shift, 1.0);

        // `clock.time()` 与 `expr.time()` 是同一个通道叶子（时钟身份属于绑定）。
        let a = clock_time(&this);
        let b = time(&this);
        assert_eq!(this.tree(a).unwrap(), this.tree(b).unwrap());
        assert!(matches!(
            this.tree(a).unwrap().node,
            Node::Channel { channel } if channel == ChannelRef::builtin("time")
        ));
    }

    /// 快慢路径调用同一组 helper ⇒ 结果一致（结构可序列化对比）。
    #[test]
    fn fast_and_slow_paths_agree() {
        let this = pod();
        let fast = harmonic(&this, 3.0);

        let id = this
            .function_id(&QualifiedName::from_path(
                "zela.math::expression",
                vec![HANDLE.to_string()],
                "String",
            ))
            .expect("expression 已注册");
        let args: [&dyn Any; 1] = [&fast];
        let slow = this
            .call(id, &args)
            .expect("慢路径成功")
            .downcast::<String>()
            .expect("返回 String");
        assert_eq!(*slow, this.export(fast).unwrap());
    }
}
