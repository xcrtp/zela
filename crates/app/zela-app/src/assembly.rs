// assembly.rs — App 的 **Pod 静态插件装配**。
//
// 描述层 / 执行层分离（D3）：
//
// ```text
// plan()     纯数据：节点名 / provides / requires（不建 Pod，可单测）
//    │
// assemble() 执行层：建 domain Service + 语言适配器 → Broker 校验 / 排序 / 注入
// ```
//
// 扩展方式（D3.3）：
// - 新增语言 = 新增 `ze-pod-*` crate + 这里加一个节点；
// - 新增 domain capability = `zela-scene` 加一个函数（本文件与语言适配器都不改）。

use std::sync::Arc;

use ze_pod_core::{
    AssemblyPlan, Broker, BrokerError, Capability, FnSig, NodeSpec, Pod, PodError, PodNode,
    ScalarKind, ServiceRequirement, TypeSpec,
};
use ze_pod_func::{
    ChannelType, ExprPod, LIBRARY_NAMESPACE, LIBRARY_VERSION, MathApi, SCENE_NAMESPACE,
    SERVICE as EXPR_SERVICE, SugarApi,
};
use ze_pod_lua::LuaPod;
use ze_prerender::InMemoryEngine;
use zela_scene_ecs::pod::{self, ContentApi, PathStore, SceneApi};
use zela_scene_ecs::scene_script::SceneScript;

/// 本 App 的分镜脚本（**App 自己的资产**，不属于语言适配器）。
pub const STORYBOARD_LUA: &str = include_str!("../scripts/storyboard.lua");
/// Python 版分镜（与 `storyboard.lua` 逐项等价）。
#[cfg(feature = "python")]
pub const STORYBOARD_PY: &str = include_str!("../scripts/storyboard.py");

/// 描述层：本 App 的静态插件清单（**纯数据**，可脱离 Pod 单测）。
///
/// `with_python` / `with_js` 只描述"是否挂对应语言节点"；能力需求的形状与运行时一致。
pub fn plan(with_python: bool, with_js: bool) -> AssemblyPlan {
    let mut nodes = vec![
        NodeSpec::new("scene", &["scene"], Vec::new()),
        NodeSpec::new("content", &["content"], Vec::new()),
        // 函子库：独立 Service / 独立 namespace ⇒ 加算子不动 `zela.scene` 的指纹。
        NodeSpec::new(EXPR_SERVICE, &[EXPR_SERVICE], Vec::new()),
    ];

    let mut lua_requires = requirements("scene", SceneApi::signatures(), true);
    lua_requires.extend(requirements("content", ContentApi::signatures(), true));
    lua_requires.extend(requirements(EXPR_SERVICE, MathApi::signatures(), true));

    #[cfg(feature = "python")]
    {
        lua_requires.extend(requirements(
            "python",
            ze_pod_py::PythonApi::signatures(),
            false,
        ));
        if with_python {
            let mut python_requires = requirements("scene", SceneApi::signatures(), true);
            python_requires.extend(requirements("content", ContentApi::signatures(), true));
            python_requires.extend(requirements(EXPR_SERVICE, MathApi::signatures(), true));
            nodes.push(NodeSpec::new("python", &["python"], python_requires));
        }
    }
    #[cfg(not(feature = "python"))]
    let _ = with_python;

    // Lua 节点：默认入口（`zela.script::run` 的消费者）。
    // Broker 的拓扑序是"按插入序 DFS" ⇒ 无依赖边的节点保持声明序；
    // `run_broker` 用拓扑序与它逐项对拍。
    nodes.push(NodeSpec::new("lua", &["script"], lua_requires));

    // JS 节点（B3-Pc）：与 python 同形状 —— 只依赖 scene / content。
    #[cfg(feature = "js")]
    if with_js {
        let mut js_requires = requirements("scene", SceneApi::signatures(), true);
        js_requires.extend(requirements("content", ContentApi::signatures(), true));
        js_requires.extend(requirements(EXPR_SERVICE, MathApi::signatures(), true));
        nodes.push(NodeSpec::new("js", &["js"], js_requires));
    }
    #[cfg(not(feature = "js"))]
    let _ = with_js;

    AssemblyPlan::new(nodes)
}

fn requirements(service: &str, signatures: &[FnSig], required: bool) -> Vec<ServiceRequirement> {
    signatures
        .iter()
        .map(|signature| {
            ServiceRequirement::new(service, &signature.qualified().to_string(), required)
        })
        .collect()
}

/// 装配失败：Broker 校验（语言节点初始化失败不再算装配失败，见 [`assemble`]）。
#[derive(Debug)]
pub enum AssemblyError {
    Broker(BrokerError),
}

impl std::fmt::Display for AssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Broker(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for AssemblyError {}

impl From<BrokerError> for AssemblyError {
    fn from(error: BrokerError) -> Self {
        Self::Broker(error)
    }
}

/// 装配产物：Broker（注入完成）+ 语言 Pod 句柄（测试 / 排查用）。
pub struct PodAssembly {
    pub broker: Broker,
    /// Lua Pod 句柄：只用于测试 / 排查（`registered_global_names`）。
    // 保留：默认构建无读取点，但 python feature 的测试会用到；属"排查句柄"而非漏接线。
    #[allow(dead_code)]
    pub lua: Arc<LuaPod>,
    /// 函子库 Pod 句柄：脚本结束后取走表达式绑定，并入 `SceneScript`。
    pub(crate) expr: Arc<ExprPod>,
    /// 场景 Service 句柄：表达式绑定的 pod 句柄 → 脚本句柄（装配期转换）。
    pub(crate) scene: Arc<pod::SceneServicePod>,
    /// 内容 Service 句柄：`compile_script` 用它读"内容能力是否被实际使用"。
    pub(crate) content: Arc<pod::ContentServicePod>,
    /// JS Pod 句柄（`js` feature）：只用于测试 / 排查。
    #[cfg(feature = "js")]
    #[allow(dead_code)]
    pub js: Arc<ze_pod_js::JsPod>,
}

impl PodAssembly {
    /// `content` 能力是否被脚本实际调用过（ZSB Meta 的最小集口径）。
    pub fn content_used(&self) -> bool {
        self.content.used()
    }

    /// Python 节点是否真的装上了（解释器缺失 / 嵌入失败时会跳过，见 [`assemble`]）。
    #[cfg(all(test, feature = "python"))]
    pub fn has_python(&self) -> bool {
        self.broker.nodes().iter().any(|node| node.name == "python")
    }

    /// 宿主 → Lua：执行分镜脚本，取回 `SceneScript`（含表达式绑定合并）。
    pub fn run_script(&self, source: &str) -> Result<SceneScript, BrokerError> {
        self.clear_expr_bindings();
        let owned = source.to_string();
        let arg: &dyn std::any::Any = &owned;
        let script = self.broker.run_with("lua", ze_pod_lua::RUN_ENTRY, &[arg])?;
        self.merge_expr_bindings(script)
    }

    /// 宿主 → Python：执行分镜脚本，取回 `SceneScript`。
    #[cfg(feature = "python")]
    pub fn run_python_script(&self, source: &str) -> Result<SceneScript, BrokerError> {
        self.clear_expr_bindings();
        let owned = source.to_string();
        let arg: &dyn std::any::Any = &owned;
        let script = self
            .broker
            .run_with("python", ze_pod_py::RUN_ENTRY, &[arg])?;
        self.merge_expr_bindings(script)
    }

    /// 宿主 → JS：执行分镜脚本，取回 `SceneScript`。
    #[cfg(feature = "js")]
    pub fn run_js_script(&self, source: &str) -> Result<SceneScript, BrokerError> {
        self.clear_expr_bindings();
        let owned = source.to_string();
        let arg: &dyn std::any::Any = &owned;
        let script = self.broker.run_with("js", ze_pod_js::RUN_ENTRY, &[arg])?;
        self.merge_expr_bindings(script)
    }

    /// 清掉上一轮残留（脚本执行失败时也可能留下半成品）。
    fn clear_expr_bindings(&self) {
        let _ = self.expr.take_bindings();
        let _ = self.expr.take_bind_each();
        let _ = self.expr.take_shaders();
    }

    /// 把脚本登记的表达式动画并入成品脚本（S3 / 提案 #6 B）。
    ///
    /// 对象是脚本拿到的 **pod 句柄**：这里（装配期）转成本脚本内的句柄，
    /// 产物只存后者。装配期错误（未知目标 / 越界句柄 / 未注入通道）在这里**显式失败**。
    fn merge_expr_bindings(&self, mut script: SceneScript) -> Result<SceneScript, BrokerError> {
        let (bindings, errors) = self.expr.take_bindings();
        let bind_each = self.expr.take_bind_each();
        if let Some(message) = errors.into_iter().next() {
            return Err(BrokerError::Injection {
                node: EXPR_SERVICE.to_string(),
                source: PodError::CallFailed(message),
            });
        }
        let library_namespace = LIBRARY_NAMESPACE.to_string();
        let library_version = LIBRARY_VERSION.to_string();
        // 对象集在脚本结束后冻结 ⇒ `exists` 在这里就折成常量（B4 Query）。
        let component_names = self.scene.component_names();
        for pending in bindings {
            let expr = fold_exists(&pending.expr, &component_names)?;
            let predicate = pending
                .predicate
                .as_ref()
                .map(|predicate| fold_exists(predicate, &component_names))
                .transpose()?;
            let namespace = pending.target.namespace.as_str();
            if !namespace.is_empty() && namespace != SCENE_NAMESPACE {
                return Err(BrokerError::Injection {
                    node: EXPR_SERVICE.to_string(),
                    source: PodError::CallFailed(format!(
                        "表达式绑定目标的命名空间未知：`{namespace}`（只接受 `{SCENE_NAMESPACE}`）"
                    )),
                });
            }
            let object = self.scene.object_handle(&pending.object).ok_or_else(|| {
                BrokerError::Injection {
                    node: EXPR_SERVICE.to_string(),
                    source: PodError::CallFailed(format!(
                        "表达式绑定的对象句柄无效：{}",
                        pending.object
                    )),
                }
            })?;
            // 根表达式类型 vs 目标字段类型：装配期显式失败（硬约束 5）。
            if let Some(entry) = zela_scene_ecs::target_entry(&pending.target.name) {
                if let Some(expected) = expected_value_type(entry.spec.ty) {
                    if expected != pending.root_type {
                        return Err(BrokerError::Injection {
                            node: EXPR_SERVICE.to_string(),
                            source: PodError::CallFailed(format!(
                                "表达式绑定 `{}` 的目标 `{}` 期望 {}，根表达式是 {}",
                                pending.object,
                                pending.target.name,
                                expected.name(),
                                pending.root_type.name()
                            )),
                        });
                    }
                }
            }
            // 时钟是**数据**：Pod 侧的扁平链在这里转成产物类型（与通道同一套路）。
            let clock = zela_scene_ecs::ClockProgram {
                links: pending
                    .clock
                    .links
                    .iter()
                    .map(|link| zela_scene_ecs::ClockLink {
                        shift: link.shift,
                        rate: link.rate,
                    })
                    .collect(),
            };
            // predicate / reset 也是数据：Pod 侧的枚举在这里映射成产物侧的策略。
            let reset = match pending.reset {
                ze_pod_func::ResetSpec::Hold => zela_scene_ecs::animation::ResetBehavior::Hold,
                ze_pod_func::ResetSpec::Reset => zela_scene_ecs::animation::ResetBehavior::Reset,
                ze_pod_func::ResetSpec::Skip => zela_scene_ecs::animation::ResetBehavior::Skip,
            };
            // **第 4 批**：把这条绑定**实际用到的算子** revision 写进产物
            // （predicate 是独立表达式，同样记录；算子身份与 revision 同源于函子库）。
            let mut op_revisions = ze_pod_func::collect_op_revisions(&expr);
            if let Some(predicate) = &predicate {
                op_revisions.extend(ze_pod_func::collect_op_revisions(predicate));
            }
            let library = zela_scene_ecs::animation::LibraryRef {
                namespace: library_namespace.clone(),
                version: library_version.clone(),
                op_revisions,
            };
            script
                .bind_expr_gated(
                    object,
                    &pending.target.name,
                    expr,
                    library,
                    clock,
                    predicate,
                    reset,
                    pending.duration,
                )
                .map_err(|error| BrokerError::Injection {
                    node: EXPR_SERVICE.to_string(),
                    source: PodError::CallFailed(error.to_string()),
                })?;
        }
        self.expand_bind_each(&mut script, bind_each)?;
        // **阶段 2（着色 codegen）**：脚本登记的 `expr.shader` 挂成对象组件（装饰性）。
        for pending in self.expr.take_shaders() {
            let object = self.scene.object_handle(&pending.object).ok_or_else(|| {
                BrokerError::Injection {
                    node: EXPR_SERVICE.to_string(),
                    source: PodError::CallFailed(format!(
                        "着色 codegen 的对象句柄无效：{}",
                        pending.object
                    )),
                }
            })?;
            script
                .shader_fill(
                    object,
                    ze_render_ir::ShaderPaint {
                        source: pending.source,
                        params: pending
                            .params
                            .into_iter()
                            .map(|param| ze_render_ir::ShaderParam {
                                channel: param.channel,
                                kind: match param.kind {
                                    ze_comb::SlotType::F64 => ze_render_ir::ShaderParamKind::F32,
                                    ze_comb::SlotType::I64 => ze_render_ir::ShaderParamKind::I32,
                                    ze_comb::SlotType::Vec2 => ze_render_ir::ShaderParamKind::Vec2,
                                    ze_comb::SlotType::Vec3 => ze_render_ir::ShaderParamKind::Vec3,
                                    ze_comb::SlotType::Bool => ze_render_ir::ShaderParamKind::F32,
                                },
                                offset: param.offset,
                            })
                            .collect(),
                        size: pending.size,
                        fragment: pending.fragment,
                    },
                )
                .map_err(|error| BrokerError::Injection {
                    node: EXPR_SERVICE.to_string(),
                    source: PodError::CallFailed(error),
                })?;
        }
        Ok(script)
    }

    /// **B4 Query**：把批量绑定在**装配期**展开成 N 条具体绑定。
    ///
    /// 三条硬约束：
    /// 1. 结果集按 `ObjectId` 升序（脚本句柄下标即 create 序）；不依赖任何容器迭代序；
    /// 2. 过滤条件只能是组件类型组合（装配期可判定）；未知组件名 ⇒ 显式失败；
    /// 3. 展开数 **N 必须可见**（`N = 0` 打印但不报错，与 `Sum{n=0}` 同类）。
    fn expand_bind_each(
        &self,
        script: &mut SceneScript,
        pendings: Vec<ze_pod_func::PendingBindEach>,
    ) -> Result<(), BrokerError> {
        if pendings.is_empty() {
            return Ok(());
        }
        let component_names = self.scene.component_names();
        for pending in pendings {
            let namespace = pending.target.namespace.as_str();
            if !namespace.is_empty() && namespace != SCENE_NAMESPACE {
                return Err(BrokerError::Injection {
                    node: EXPR_SERVICE.to_string(),
                    source: PodError::CallFailed(format!(
                        "批量绑定目标的命名空间未知：`{namespace}`（只接受 `{SCENE_NAMESPACE}`）"
                    )),
                });
            }
            for name in pending.requires.iter().chain(pending.excludes.iter()) {
                if ze_scene_core::registry::known_component_type_id(name).is_none() {
                    return Err(BrokerError::Injection {
                        node: EXPR_SERVICE.to_string(),
                        source: PodError::CallFailed(format!(
                            "批量绑定的过滤组件名未知：`{name}`（必须是已登记组件的稳定名）"
                        )),
                    });
                }
            }
            if let Some(entry) = zela_scene_ecs::target_entry(&pending.target.name) {
                if let Some(expected) = expected_value_type(entry.spec.ty) {
                    if expected != pending.root_type {
                        return Err(BrokerError::Injection {
                            node: EXPR_SERVICE.to_string(),
                            source: PodError::CallFailed(format!(
                                "批量绑定目标 `{}` 期望 {}，根表达式是 {}",
                                pending.target.name,
                                expected.name(),
                                pending.root_type.name()
                            )),
                        });
                    }
                }
            }

            let reset = reset_behavior(pending.reset);
            let clock = clock_program(&pending.clock);
            let expr = fold_exists(&pending.expr, &component_names)?;
            let predicate = pending
                .predicate
                .as_ref()
                .map(|predicate| fold_exists(predicate, &component_names))
                .transpose()?;
            let mut expanded = 0usize;
            // 句柄下标 = create 顺序 = `ObjectId` 升序 ⇒ 这里**天然**是稳定排序。
            for (index, components) in component_names.iter().enumerate() {
                if index >= script.len() {
                    break;
                }
                let matched = pending
                    .requires
                    .iter()
                    .all(|name| components.contains(&name.as_str()))
                    && pending
                        .excludes
                        .iter()
                        .all(|name| !components.contains(&name.as_str()));
                if !matched {
                    continue;
                }
                let expr = substitute_query_self(&expr, index as u64);
                let mut op_revisions = ze_pod_func::collect_op_revisions(&expr);
                if let Some(predicate) = &predicate {
                    op_revisions.extend(ze_pod_func::collect_op_revisions(predicate));
                }
                let library = zela_scene_ecs::animation::LibraryRef {
                    namespace: LIBRARY_NAMESPACE.to_string(),
                    version: LIBRARY_VERSION.to_string(),
                    op_revisions,
                };
                script
                    .bind_expr_gated(
                        ze_scene_core::scene_script::Handle(index),
                        &pending.target.name,
                        expr,
                        library,
                        clock.clone(),
                        predicate.clone(),
                        reset,
                        0.0,
                    )
                    .map_err(|error| BrokerError::Injection {
                        node: EXPR_SERVICE.to_string(),
                        source: PodError::CallFailed(error.to_string()),
                    })?;
                expanded += 1;
            }
            println!(
                "[query] 批量绑定 `{}` 展开 {expanded} 条（requires=[{}] excludes=[{}]）",
                pending.target.name,
                pending.requires.join(","),
                pending.excludes.join(",")
            );
        }
        Ok(())
    }
}

/// 装配期**常量折叠** `exists`（B4 Query）。
///
/// 对象集在脚本结束后已冻结 ⇒ `exists` 的值是常量（折叠**先于**区间代数：
/// 区间代数只认依赖连续变量的表达式）。过滤组件名必须已登记，否则显式失败。
fn fold_exists(
    expr: &ze_comb::Expr,
    component_names: &[Vec<&'static str>],
) -> Result<ze_comb::Expr, BrokerError> {
    if let Some((requires, excludes)) = expr.node.exists() {
        for name in requires.iter().chain(excludes.iter()) {
            if ze_scene_core::registry::known_component_type_id(name).is_none() {
                return Err(BrokerError::Injection {
                    node: EXPR_SERVICE.to_string(),
                    source: PodError::CallFailed(format!(
                        "`exists` 的过滤组件名未知：`{name}`（必须是已登记组件的稳定名）"
                    )),
                });
            }
        }
        let found = component_names.iter().any(|components| {
            requires
                .iter()
                .all(|name| components.contains(&name.as_str()))
                && excludes
                    .iter()
                    .all(|name| !components.contains(&name.as_str()))
        });
        return Ok(ze_comb::Expr::pure(ze_comb::Literal::Bool(found)));
    }
    let mut failure = None;
    let folded = expr.map_children(|child| match fold_exists(child, component_names) {
        Ok(child) => child,
        Err(error) => {
            failure.get_or_insert(error);
            child.clone()
        }
    });
    match failure {
        Some(error) => Err(error),
        None => Ok(folded),
    }
}

/// 把表达式树里的 `it` 占位对象替换成具体脚本句柄下标。
fn substitute_query_self(expr: &ze_comb::Expr, object: u64) -> ze_comb::Expr {
    if let Some((channel, current)) = expr.node.read() {
        if current == ze_pod_func::QUERY_SELF_OBJECT {
            return ze_comb::Expr {
                id: expr.id.clone(),
                node: ze_comb::Node::Read {
                    channel: channel.clone(),
                    object,
                },
            };
        }
    }
    if let Some((method, current, args)) = expr.node.method() {
        if current == ze_pod_func::QUERY_SELF_OBJECT {
            return ze_comb::Expr {
                id: expr.id.clone(),
                node: ze_comb::Node::Method {
                    method: method.clone(),
                    object,
                    args: args
                        .iter()
                        .map(|arg| substitute_query_self(arg, object))
                        .collect(),
                },
            };
        }
    }
    expr.map_children(|child| substitute_query_self(child, object))
}

/// Pod 侧时钟（扁平链）→ 产物侧 `ClockProgram`。
fn clock_program(clock: &ze_pod_func::ClockSpec) -> zela_scene_ecs::ClockProgram {
    zela_scene_ecs::ClockProgram {
        links: clock
            .links
            .iter()
            .map(|link| zela_scene_ecs::ClockLink {
                shift: link.shift,
                rate: link.rate,
            })
            .collect(),
    }
}

/// Pod 侧 `ResetSpec` → 产物侧 `ResetBehavior`。
fn reset_behavior(reset: ze_pod_func::ResetSpec) -> zela_scene_ecs::animation::ResetBehavior {
    match reset {
        ze_pod_func::ResetSpec::Hold => zela_scene_ecs::animation::ResetBehavior::Hold,
        ze_pod_func::ResetSpec::Reset => zela_scene_ecs::animation::ResetBehavior::Reset,
        ze_pod_func::ResetSpec::Skip => zela_scene_ecs::animation::ResetBehavior::Skip,
    }
}

/// 目标字段类型（pod `TypeSpec`）→ 表达式层粗类型。
///
/// 未知形状返回 `None`（不阻断）：类型系统只覆盖数值 / 布尔 / 向量，
/// 其它字段由 `Target::write` 在运行期显式报 `TypeMismatch`。
fn expected_value_type(ty: &TypeSpec) -> Option<ChannelType> {
    match ty {
        TypeSpec::Scalar(ScalarKind::F32 | ScalarKind::F64) => Some(ChannelType::Number),
        TypeSpec::Scalar(ScalarKind::Bool) => Some(ChannelType::Bool),
        TypeSpec::Struct { name, .. } | TypeSpec::Enum { name, .. } | TypeSpec::Opaque(name) => {
            match name.strip_prefix("zela.").unwrap_or(name) {
                "Vec2" => Some(ChannelType::Vec2),
                "Vec3" => Some(ChannelType::Vec3),
                _ => None,
            }
        }
        _ => None,
    }
}

/// 静态装配：scene / content / lua（+ feature 开启时的 python / js）。
pub fn assemble(engine: Arc<InMemoryEngine>) -> Result<PodAssembly, AssemblyError> {
    // `nodes` 在开启 python / js feature 时需要追加节点。
    #[cfg_attr(not(any(feature = "python", feature = "js")), allow(unused_mut))]
    let (lua, scene, content, expr, mut nodes) = base_nodes(engine);

    #[cfg(feature = "python")]
    {
        // 内嵌 Python 的初始化失败是 CPython 的 **Fatal**（`Py_InitializeEx` 直接
        // `abort()`），Rust 侧既捕获不到也没机会提示。所以：
        //   1. 先起子进程探测"有一个能 import encodings 的解释器"，并把它指给
        //      `PYTHONHOME`（`python_env` 的文档里写了真因）；
        //   2. 探测或 `PyPod::new()` 任一步失败 ⇒ **只提示、跳过本节点**，
        //      其余 Pod（scene / content / lua）照常装配。
        match crate::python_env::ensure() {
            Ok(()) => match ze_pod_py::PyPod::new() {
                Ok(python) => {
                    let python = Arc::new(python);
                    python.register_capability::<SceneApi>("scene", true);
                    python.register_capability::<ContentApi>("content", true);
                    python.register_capability::<MathApi>(EXPR_SERVICE, true);
                    // 第 BB 批（阶段 4）：糖层——同一个 expr Pod，脚本里是 `sugar` 表。
                    python.register_capability_as::<SugarApi>(EXPR_SERVICE, "sugar", true);
                    // P1：Python 闭包要求 `Send + Sync` ⇒ 登记 SyncPod 徽章，Broker 走 inject_sync。
                    nodes.push(PodNode::new_sync("python", python, &["python"]));
                }
                Err(error) => {
                    eprintln!("[zela-app] Python Pod 未装配（其余 Pod 照常）：{error}")
                }
            },
            Err(why) => {
                eprintln!("[zela-app] Python Pod 未装配（其余 Pod 照常）：{why}")
            }
        }
    }

    #[cfg(feature = "js")]
    let js = {
        let js = Arc::new(ze_pod_js::JsPod::new());
        js.register_capability::<SceneApi>("scene", true);
        js.register_capability::<ContentApi>("content", true);
        js.register_capability::<MathApi>(EXPR_SERVICE, true);
        // 第 BB 批（阶段 4）：糖层（`sugar.ramp` …）。
        js.register_capability_as::<SugarApi>(EXPR_SERVICE, "sugar", true);
        // boa 的 `Context` 是 `!Send + !Sync`（`examples/send_probe.rs` 实测）
        // ⇒ 与 LuaPod 一样不登记 SyncPod 徽章。
        nodes.push(PodNode::new("js", Arc::clone(&js) as Arc<dyn Pod>, &["js"]));
        js
    };

    let broker = run_broker(&nodes)?;
    Ok(PodAssembly {
        broker,
        lua,
        expr,
        scene,
        content,
        #[cfg(feature = "js")]
        js,
    })
}

/// 四个内置节点 + Lua 的能力注册。
fn base_nodes(
    engine: Arc<InMemoryEngine>,
) -> (
    Arc<LuaPod>,
    Arc<pod::SceneServicePod>,
    Arc<pod::ContentServicePod>,
    Arc<ExprPod>,
    Vec<PodNode>,
) {
    let paths = PathStore::new();
    let scene = pod::scene_service(paths.clone());
    let content = pod::content_service(engine, paths);
    let expr = Arc::new(ExprPod::new());

    let lua = Arc::new(LuaPod::new());
    lua.register_capability::<SceneApi>("scene", true);
    lua.register_capability::<ContentApi>("content", true);
    lua.register_capability::<MathApi>(EXPR_SERVICE, true);
    // 第 BB 批（阶段 4）：糖层（`sugar.ramp` / `sugar.wave` …）。
    lua.register_capability_as::<SugarApi>(EXPR_SERVICE, "sugar", true);
    #[cfg(feature = "python")]
    lua.register_capability::<ze_pod_py::PythonApi>("python", false);

    let nodes = vec![
        // scene / content / expr 只含装配期状态（内部用锁），登记 SyncPod 徽章供 Python 闭包捕获。
        PodNode::new_sync("scene", Arc::clone(&scene), &["scene"]),
        PodNode::new_sync("content", Arc::clone(&content), &["content"]),
        PodNode::new_sync(EXPR_SERVICE, Arc::clone(&expr), &[EXPR_SERVICE]),
        // LuaPod 持有 mlua 解释器（非 Send + Sync）⇒ 不登记徽章。
        PodNode::new("lua", Arc::clone(&lua) as Arc<dyn Pod>, &["script"]),
    ];
    (lua, scene, content, expr, nodes)
}

fn run_broker(nodes: &[PodNode]) -> Result<Broker, AssemblyError> {
    // 描述层先校验（不建 Pod 也能跑）；再与 Broker 从真实 Pod 推导的图对齐。
    let declared = plan(
        nodes.iter().any(|node| node.name == "python"),
        nodes.iter().any(|node| node.name == "js"),
    );
    let declared_report = declared.validate()?;

    let broker = Broker::new(nodes.to_vec());
    let report = broker.assemble()?;
    debug_assert_eq!(
        declared_report.order, report.order,
        "描述层与执行层的拓扑序不一致"
    );
    debug_assert_eq!(
        declared_report.edges, report.edges,
        "描述层与执行层的依赖边不一致"
    );
    debug_assert_eq!(
        declared_report.optional_missing, report.optional_missing,
        "描述层与执行层的 optional 缺失不一致"
    );

    eprintln!("[zela-app] Pod 装配：\n{report}");
    Ok(broker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_pod_core::BrokerError;

    /// **B4 验收**：`exists` 在装配期折叠成常量（折叠先于区间代数）。
    #[test]
    fn exists_folds_to_a_constant_at_assembly() {
        let names: Vec<Vec<&'static str>> = vec![vec!["Circle"], vec!["Circle", "Reveal"]];
        let hit = ze_comb::Expr::call(
            ze_comb::OpRef::new("zela.math", "mul"),
            [ze_comb::Expr::exists(
                vec!["Circle".to_string()],
                vec!["Reveal".to_string()],
            )],
        );
        let folded = fold_exists(&hit, &names).expect("折叠成功");
        let (_, args) = folded.node.call().expect("外层仍是 Call");
        assert_eq!(args[0].node.literal(), Some(&ze_comb::Literal::Bool(true)));

        let miss = ze_comb::Expr::exists(vec!["Parent".to_string()], vec![]);
        let folded = fold_exists(&miss, &names).expect("折叠成功");
        assert_eq!(folded.node.literal(), Some(&ze_comb::Literal::Bool(false)));

        // 组件名写错是装配期错误，不是"匹配空集"。
        let bad = ze_comb::Expr::exists(vec!["NoSuchComponent".to_string()], vec![]);
        assert!(fold_exists(&bad, &names).is_err());
    }

    /// 描述层：拓扑 / 边 / optional 缺失全部可在**不建 Pod** 时断言。
    #[test]
    fn plan_is_pure_and_ordered() {
        let report = plan(false, false).validate().expect("静态清单合法");
        let position = |name: &str| {
            report
                .order
                .iter()
                .position(|item| item == name)
                .unwrap_or_else(|| panic!("缺节点 {name}"))
        };
        assert!(position("scene") < position("lua"));
        assert!(position("content") < position("lua"));
        assert!(
            report
                .edges
                .contains(&("scene".to_string(), "lua".to_string()))
        );
        assert!(
            report
                .edges
                .contains(&("content".to_string(), "lua".to_string()))
        );
        assert!(report.injected.is_empty(), "描述层不做注入");
    }

    /// 描述层：Python 节点（启用时）依赖 scene，且因 lua 消费 python 而排在 lua 之前。
    #[cfg(feature = "python")]
    #[test]
    fn plan_with_python_is_ordered() {
        let report = plan(true, false).validate().expect("静态清单合法");
        let position = |name: &str| {
            report
                .order
                .iter()
                .position(|item| item == name)
                .unwrap_or_else(|| panic!("缺节点 {name}"))
        };
        assert!(position("scene") < position("python"));
        assert!(position("python") < position("lua"));
        assert!(report.optional_missing.is_empty());
    }

    /// 描述层：JS 节点（启用时）依赖 scene / content，且与 lua 无先后约束
    /// （插入序在 lua 之后 ⇒ 拓扑序保持该序）。
    #[cfg(feature = "js")]
    #[test]
    fn plan_with_js_is_ordered() {
        let report = plan(false, true).validate().expect("静态清单合法");
        let position = |name: &str| {
            report
                .order
                .iter()
                .position(|item| item == name)
                .unwrap_or_else(|| panic!("缺节点 {name}"))
        };
        assert!(position("scene") < position("js"));
        assert!(position("content") < position("js"));
        assert!(position("lua") < position("js"));
        assert!(
            report
                .edges
                .contains(&("scene".to_string(), "js".to_string()))
        );
        assert!(
            report
                .edges
                .contains(&("content".to_string(), "js".to_string()))
        );
    }

    /// 描述层：环检测不需要真实 Pod。
    #[test]
    fn plan_detects_cycle_without_pods() {
        let plan = AssemblyPlan::new(vec![
            NodeSpec::new(
                "a",
                &["a"],
                vec![ServiceRequirement::required("b", "b::ping() -> ()")],
            ),
            NodeSpec::new(
                "b",
                &["b"],
                vec![ServiceRequirement::required("a", "a::ping() -> ()")],
            ),
        ]);
        assert!(matches!(plan.validate(), Err(BrokerError::Cycle { .. })));
    }

    /// 目标字段类型 → 表达式层粗类型（根 / 目标核对的桥）。
    #[test]
    fn target_types_map_to_expression_types() {
        static VEC2: TypeSpec = TypeSpec::Struct {
            name: "zela.Vec2",
            layout: None,
            fields: &[],
        };
        static VEC3: TypeSpec = TypeSpec::Struct {
            name: "zela.Vec3",
            layout: None,
            fields: &[],
        };
        assert_eq!(
            expected_value_type(&TypeSpec::Scalar(ScalarKind::F32)),
            Some(ChannelType::Number)
        );
        assert_eq!(
            expected_value_type(&TypeSpec::Scalar(ScalarKind::F64)),
            Some(ChannelType::Number)
        );
        assert_eq!(
            expected_value_type(&TypeSpec::Scalar(ScalarKind::Bool)),
            Some(ChannelType::Bool)
        );
        assert_eq!(expected_value_type(&VEC2), Some(ChannelType::Vec2));
        assert_eq!(expected_value_type(&VEC3), Some(ChannelType::Vec3));
    }
}
