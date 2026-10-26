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
    AddSpec, BindSpec, ChannelSpec, CosSpec, MulSpec, ScaleSpec, SinSpec, SumSpec,
};

/// 函子库 namespace（进产物）。
pub const LIBRARY_NAMESPACE: &str = "zela.math";
/// 函子库版本（进产物；加算子递增它，`zela.scene` 不受影响）。
pub const LIBRARY_VERSION: &str = "4";

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
        registry.register_fn1_named::<String, ChannelSpec>(
            "zela.math::channel",
            "String",
            "zela.math.Channel",
            channel_ctor,
        );
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
            "channel" => Box::new(channel_ctor(self, arg::<String>(args, 0)?.clone())),
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

/// `expr.channel("Circle.radius")`：组件字段通道（场景地址表的键）。
fn channel_ctor(_pod: &dyn Pod, address: String) -> ChannelSpec {
    ChannelSpec {
        namespace: SCENE_NAMESPACE.to_string(),
        name: address,
    }
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
            this.errors
                .lock()
                .expect("表达式错误表未被毒化")
                .push(format!("表达式句柄无效：{error}"));
            return;
        }
    };
    if let Err(message) = validate_binding(&expr) {
        this.errors
            .lock()
            .expect("表达式错误表未被毒化")
            .push(format!("表达式绑定 `{}` 非法：{message}", spec.object));
        return;
    }
    this.bindings
        .lock()
        .expect("表达式绑定表未被毒化")
        .push(PendingBinding {
            object: spec.object,
            target: spec.target,
            expr,
            duration: spec.duration,
        });
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
