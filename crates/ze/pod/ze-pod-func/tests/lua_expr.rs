//! S2 验收：**函子库作为外挂 Pod 接入脚本**，且不碰 `zela.scene`。
//!
//! 覆盖三件事：
//! 1. Lua 通过 `expr.*` 只提交结构，产物是表达式 JSON（含库 namespace + version）；
//! 2. 表达式通道是零元叶子（脚本拿不到 `t` 的数值）；
//! 3. 提供者缺席 ⇒ **装配期显式失败**（不是运行期空实现）。

#![allow(dead_code)] // capability 的 inherent 方法不全部调用。

use std::any::Any;
use std::cell::RefCell;
use std::sync::Arc;

use ze_pod_core::{
    Broker, Capability, FunctionId, Pod, PodError, PodId, PodNode, QualifiedName, RegistryPod,
};
use ze_pod_func::{ExprPod, MathApi, LIBRARY_NAMESPACE, LIBRARY_VERSION, SERVICE};
use ze_pod_lua::{LuaPod, RUN_ENTRY};
use ze_pod_macro::capability;

// ─── 测试内的最小 scene 服务（只收一段文本）─────────────────────────────

#[capability(namespace = "test.sink", bindings)]
struct SinkApi<'a> {
    /// 记录一段文本（测试用）。
    emit: fn(&'a dyn Pod, text: String) -> (),
}

const RESET_PATH: &str = "zela.scene::__reset";
const FINISH_PATH: &str = "zela.scene::__finish";

struct Sink {
    id: PodId,
    registry: RegistryPod,
    out: RefCell<Option<String>>,
}

impl Sink {
    fn new() -> Self {
        let mut registry = RegistryPod::new();
        registry.register_fn1_named::<String, ()>("test.sink::emit", "String", "()", emit);
        registry.register_erased(RESET_PATH, &[], "()", |pod, _| {
            *sink(pod).out.borrow_mut() = None;
            Ok(Box::new(()))
        });
        registry.register_erased(FINISH_PATH, &[], "Result<Box<dyn Any>, String>", |pod, _| {
            let out = sink(pod).out.borrow().clone();
            Ok(Box::new(match out {
                Some(text) => Ok(Box::new(text) as Box<dyn Any>),
                None => Err("脚本没有 emit 任何文本".to_string()),
            }))
        });
        Self {
            id: PodId::next(),
            registry,
            out: RefCell::new(None),
        }
    }
}

fn sink(pod: &dyn Pod) -> &Sink {
    pod.as_any().downcast_ref::<Sink>().expect("Sink")
}

fn emit(pod: &dyn Pod, text: String) {
    *sink(pod).out.borrow_mut() = Some(text);
}

impl Pod for Sink {
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
    fn call(&self, id: FunctionId, args: &[&dyn Any]) -> Result<Box<dyn Any>, PodError> {
        let name = self
            .functions()
            .get(id.get() as usize)
            .ok_or_else(|| PodError::UnknownFunction(format!("函数编号 {id}")))?;
        match name.name() {
            "__reset" => {
                *self.out.borrow_mut() = None;
                Ok(Box::new(()))
            }
            "__finish" => {
                let out = self.out.borrow().clone();
                Ok(Box::new(match out {
                    Some(text) => Ok(Box::new(text) as Box<dyn Any>),
                    None => Err("脚本没有 emit 任何文本".to_string()),
                }))
            }
            _ => self.registry.call(id, args),
        }
    }
}

// ─── 装配 ──────────────────────────────────────────────────────────────

fn assembly() -> (Arc<ExprPod>, Arc<Sink>, Broker) {
    let expr = Arc::new(ExprPod::new());
    let sink = Arc::new(Sink::new());
    let lua = Arc::new(LuaPod::new());
    lua.register_capability::<MathApi>(SERVICE, true);
    lua.register_capability::<SinkApi>("scene", true);

    let broker = Broker::new(vec![
        PodNode::new(SERVICE, Arc::clone(&expr) as Arc<dyn Pod>, &[SERVICE]),
        PodNode::new("scene", Arc::clone(&sink) as Arc<dyn Pod>, &["scene"]),
        PodNode::new("lua", Arc::clone(&lua) as Arc<dyn Pod>, &["script"]),
    ]);
    broker.assemble().expect("函子 pod 装配成功");
    (expr, sink, broker)
}

fn run(source: &str) -> String {
    let (_, _, broker) = assembly();
    let owned = source.to_string();
    let arg: &dyn Any = &owned;
    broker
        .run_with::<String>("lua", RUN_ENTRY, &[arg])
        .expect("Lua 脚本执行成功")
}

// ─── 用例 ──────────────────────────────────────────────────────────────

/// 脚本只提交结构：`Σ sin(k t) / k` 的单项，产物是表达式 JSON。
#[test]
fn lua_builds_an_expression_and_exports_it() {
    let out = run(
        r#"
        local k = 3.0
        local t = clock.time()
        -- 表构造器（与 `scene.spawn{}` 同构）；字面量仍需显式 `expr.pure`
        -- （自动提升属 AF-06 的剩余项）。
        local y = Scale {
            k = 4.0 / (3.141592653589793 * k),
            of = Sin { of = Mul { a = t, b = expr.pure(k) } },
        }
        scene.emit(expr.expression(y))
        "#,
    );
    let json: serde_json::Value = serde_json::from_str(&out).expect("产物必须是合法 JSON");
    assert_eq!(json["library"]["namespace"], LIBRARY_NAMESPACE);
    assert_eq!(json["library"]["version"], LIBRARY_VERSION);

    let root = &json["expr"];
    assert_eq!(root["node"]["type"], "call");
    assert_eq!(root["node"]["data"]["op"]["name"], "scale");
    // `4 / (πk)` 是装配期 Lua 算术 ⇒ 字面量叶子，不是子树。
    assert_eq!(root["node"]["data"]["args"][0]["node"]["type"], "pure");
    assert_eq!(
        root["node"]["data"]["args"][1]["node"]["data"]["op"]["name"],
        "sin"
    );
    // 通道是零元叶子：脚本拿不到 `t` 的数值（v2 形状是 namespace + name）。
    let channel = &root["node"]["data"]["args"][1]["node"]["data"]["args"][0]["node"]["data"]["args"]
        [0]["node"]["data"]["channel"];
    assert_eq!(channel["namespace"], "", "基础通道的空 namespace");
    assert_eq!(channel["name"], "time");
}

/// `sum` 接收数组；空数组退化为字面量零（不产生无参 call）。
#[test]
fn lua_sum_accepts_an_array() {
    let out = run(
        r#"
        local terms = {}
        for i = 1, 3 do
            local k = i * 2.0
            terms[i] = Scale {
                k = 4.0 / (3.141592653589793 * k),
                of = Sin { of = Mul { a = clock.time(), b = expr.pure(k) } },
            }
        end
        scene.emit(expr.expression(Sum { terms = terms }))
        "#,
    );
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    let args = json["expr"]["node"]["data"]["args"]
        .as_array()
        .expect("sum 的参数数组");
    assert_eq!(args.len(), 3);
}

/// **B5**：Lua 侧能构造 `NatRec`（离散层的脚本入口），且 `rec_prev` 落在 step 内。
#[test]
fn lua_can_build_a_nat_rec() {
    let out = run(
        r#"
        local fold = NatRec {
            count = 4.0,
            init = expr.pure(1.0),
            step = Add { a = expr.rec_prev(), b = expr.pure(2.0) },
        }
        scene.emit(expr.expression(fold))
        "#,
    );
    let json: serde_json::Value = serde_json::from_str(&out).expect("产物必须是合法 JSON");
    let node = &json["expr"]["node"];
    assert_eq!(node["type"], "nat_rec");
    assert_eq!(node["data"]["count"]["node"]["data"]["value"]["type"], "i64");
    assert_eq!(
        node["data"]["step"]["node"]["data"]["args"][0]["node"]["type"],
        "rec_prev"
    );
}

/// 能力身份：namespace 是 `zela.math`，与 `zela.scene` 隔离。
#[test]
fn capability_lives_in_its_own_namespace() {
    assert_eq!(MathApi::meta().version, Some(LIBRARY_VERSION));
    for sig in MathApi::signatures() {
        assert_eq!(sig.namespace, LIBRARY_NAMESPACE, "{}", sig.name);
        assert_ne!(sig.namespace, "zela.scene");
    }
    for doc in MathApi::docs() {
        assert!(!doc.description.is_empty(), "`{}` 缺 ///", doc.name);
    }
}

/// 提供者缺席 ⇒ 装配期显式失败（不是运行期空实现）。
#[test]
fn missing_functor_pod_fails_at_assembly() {
    let lua = Arc::new(LuaPod::new());
    lua.register_capability::<MathApi>(SERVICE, true);
    let sink = Arc::new(Sink::new());
    lua.register_capability::<SinkApi>("scene", true);

    let broker = Broker::new(vec![
        PodNode::new("scene", sink as Arc<dyn Pod>, &["scene"]),
        PodNode::new("lua", lua as Arc<dyn Pod>, &["script"]),
    ]);
    let err = broker.assemble().expect_err("缺 expr 提供者 ⇒ 必须失败");
    assert!(
        err.to_string().contains(SERVICE),
        "错误必须点名缺失的 Service，实际：{err}"
    );
}

/// 表构造器由**已注册的函子签名**自动生成（§7.5 第 3 条）。
#[test]
fn table_constructors_are_generated_for_expression_ops() {
    let capabilities = [ze_pod_binding::CapabilityDocs {
        name: SERVICE,
        calls: MathApi::calls(),
        docs: MathApi::docs(),
        meta: MathApi::meta(),
        channels: &[],
        methods: &[],
        ops: ze_pod_func::SIGNATURES,
    }];
    let lua = ze_pod_lua::lua_ls(&capabilities);
    assert!(lua.contains("---@class Sin"), "Sin 类型声明缺失");
    assert!(lua.contains("function Sin(spec) end"), "Sin 构造器缺失");
    assert!(lua.contains("function Mul(spec) end"), "Mul 构造器缺失");
    assert!(lua.contains("function Scale(spec) end"), "Scale 构造器缺失");

    // **A 批**：声明文件输出算子的 L2 类型签名（文档/诊断用）。
    assert!(lua.contains("Sin: (F64) -> F64"), "缺 Sin 的类型签名：\n{lua}");
    assert!(
        lua.contains("Mul: (Vector, F64) -> Vector"),
        "几何位置多态必须体现在签名里：\n{lua}"
    );
    assert!(lua.contains("And: (Bool, Bool) -> Bool"));

    let json = ze_pod_binding::capabilities_json(&capabilities);
    assert!(json.contains("zela.math.Sin"), "JSON 应含 Sin 的类型名");
    assert!(
        json.contains("\"display\":\"(F64) -> F64\""),
        "JSON 的 ops 应带可读签名"
    );
    let value: serde_json::Value = serde_json::from_str(&json).expect("合法 JSON");
    assert_eq!(
        value["schema_version"].as_u64(),
        Some(u64::from(ze_pod_binding::DOCS_SCHEMA_VERSION)),
        "文档 JSON 顶层必须带 schema_version"
    );
}

/// **类型存根优化**：L2 签名 → `Expr<T>`（Lua / TS / Python 三侧同一份定义）。
#[test]
fn typed_stubs_carry_l2_signatures() {
    let capabilities = [ze_pod_binding::CapabilityDocs {
        name: SERVICE,
        calls: MathApi::calls(),
        docs: MathApi::docs(),
        meta: MathApi::meta(),
        channels: &[],
        methods: &[],
        ops: ze_pod_func::SIGNATURES,
    }];

    let lua = ze_pod_lua::lua_ls(&capabilities);
    assert!(lua.contains("---@class Expr<T>: Handle"), "缺 Expr<T>：\n{lua}");
    assert!(lua.contains("---@field of Expr<any>"), "Sin.of 应是表达式：\n{lua}");
    assert!(
        lua.contains("---@overload fun(a: Expr<any>, b: Expr<any>): Expr<number> | Expr<Vec2> | Expr<Vec3>"),
        "Mul 的返回应是具体类型并集：\n{lua}"
    );
    assert!(lua.contains("---@return Expr<number>"));
    assert!(lua.contains("---@return Expr<boolean>"));
    assert!(lua.contains("---@field root Expr<any>"));
    assert!(lua.contains("---@field target Channel<any>"));
    assert!(lua.contains("---@field expr Expr<boolean>"));
    // 通道的泛型类只能出现一次（`zela.math.Channel` 与组件常量共用一个）。
    // 注：用 `Channel<` 而不是 `Channel` —— 第 2 批新增的枚举类 `ChannelType`
    // 前缀相同，宽松匹配会把它算进来。
    assert_eq!(
        lua.matches("---@class Channel<").count(),
        1,
        "Channel 类重复：\n{lua}"
    );

    let ts = ze_pod_js::d_ts(&capabilities);
    assert!(ts.contains("export type Expr<T> = Handle"));
    assert!(ts.contains("export declare function Sin(spec: Sin): Expr<number>;"));
    assert!(ts.contains(
        "export declare function Mul(a: Expr<any>, b: Expr<any>): Expr<number> | Expr<Vec2> | Expr<Vec3>;"
    ));
    assert!(ts.contains("root: Expr<any>;") && ts.contains("target: Channel<any>;"));
    assert!(ts.contains("export interface Channel<T = unknown>"));

    let pyi = ze_pod_py::pyi_files(&capabilities);
    let root = pyi.get("zela/__init__.pyi").expect("根包");
    assert!(root.contains("class Expr(Handle, Generic[T]):"));
    let math = pyi.get("zela/math/__init__.pyi").expect("math 类型包");
    assert!(math.contains("def Sin(of: Expr[Any]) -> Expr[float]: ..."));
    assert!(math.contains(
        "def Mul(a: Expr[Any], b: Expr[Any]) -> Expr[float] | Expr[Vec2] | Expr[Vec3]: ..."
    ));
    assert!(math.contains("class Channel(Generic[T]):"), "pyi:\n{math}");
    assert!(math.contains("target: Channel[Any]") && math.contains("root: Expr[Any]"));
    assert!(math.contains("def Of(expr: Expr[bool]) -> Predicate: ..."));
    let math_mod = pyi.get("zela/math.pyi").expect("math 能力模块");
    assert!(math_mod.contains("def sin(spec: Any) -> Expr[float]: ..."));
    assert!(math_mod.contains("from zela import Handle, Expr"));

    // JSON 定义也带 spec：脚本侧不必按名字猜构造器。
    let json = ze_pod_binding::capabilities_json(&capabilities);
    assert!(json.contains("\"spec\":\"zela.math.Sin\""));
    assert!(json.contains("\"spec\":\"zela.math.Mul\""));
}

/// 注册名 == capability 签名（绑定层与函数表不漂移）。
#[test]
fn registered_names_match_signatures() {
    let expr = ExprPod::new();
    let functions = expr.functions();
    let registered: Vec<&str> = functions.iter().map(|name| name.name()).collect();
    for sig in MathApi::signatures() {
        assert!(
            registered.contains(&sig.name),
            "Pod 未注册 `{}`（注册了 {registered:?}）",
            sig.name
        );
    }
}
