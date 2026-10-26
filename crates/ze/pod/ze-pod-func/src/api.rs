//! `zela.math`：表达式层的**叶子算子**能力（函子库）。
//!
//! # 它做什么 / 不做什么
//!
//! - **只做表达**：每个算子返回一个新的表达式句柄；不做采样、不读时间。
//! - **不取数**：`time` / `frame` / `event` / `component_state` / `env` 是零元组合子，
//!   装配期没有值，运行期由宿主单子 `M` 注入（S3）。脚本拿不到 `t` 的数值 ——
//!   不是禁止，是 API 层**根本没有这个入口**。
//!
//! # 为什么返回值是句柄而不是值
//!
//! 表达式是「大类型」：数据留在 `ExprPod` 内部，跨边界只传 [`Handle`]
//! （与 `SceneServicePod` 的组件句柄同一形态）。这样：
//! - 组合子树天然共享（同一句柄用两次 = 一份结构）；
//! - 边界上不出现派生浮点 —— 装配期算术已经是字面量叶子（§5.4）。
//!
//! # 与 `zela.scene@10` 的关系
//!
//! 本能力在**独立 namespace**（`zela.math`）与独立 Service（`expr`），
//! 因此加一百个算子也不会动 `zela.scene` 的指纹（指纹按 namespace 隔离）。

use ze_pod_core::Handle;
use ze_pod_macro::{PodType, capability};

/// 一条待绑定的表达式动画（S3）。
///
/// 表构造器形态（与 `scene.spawn{...}` 同构）：
/// `expr.bind{ object = "curve", target = "CircleRadius", root = y, duration = 2.0 }`。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Bind")]
pub struct BindSpec {
    /// 目标对象名（装配期由 scene 侧解析为句柄；缺失 / 重名都是显式错误）。
    pub object: String,
    /// 动画目标（`zela.scene` 的目标名字表）。
    pub target: String,
    /// 表达式根句柄。
    pub root: Handle,
    /// 段时长（秒）。
    pub duration: f64,
}

/// 表达式叶子算子集合。
///
/// 字段名就是脚本侧函数名（`expr.sin` / `expr.mul` …）。
///
/// 参数顺序与 [`crate::expr_pod`] 的注册顺序**逐条一致**（协商按完整函数名匹配，
/// 漂移会得到显式失败而不是错位调用）。
#[capability(
    namespace = "zela.math",
    version = "2",
    description = "表达式层叶子算子（组合子库；只做表达，不取数）",
    bindings
)]
pub struct MathApi<'a> {
    /// 数值字面量（装配期算术的结果落在这里）。
    pub pure: fn(&'a dyn Pod, value: f64) -> Handle,

    /// 时间通道（零元组合子；运行期由宿主注入）。
    pub time: fn(&'a dyn Pod) -> Handle,

    /// 帧号通道（一等通道）。
    pub frame: fn(&'a dyn Pod) -> Handle,

    /// 输入事件通道（`ViewportClick` / `ViewportHover` 的第一个消费方）。
    pub event: fn(&'a dyn Pod) -> Handle,

    /// 组件状态通道（经 `SceneRead` 读取）。
    pub component_state: fn(&'a dyn Pod) -> Handle,

    /// 运行环境通道（`RuntimeEval`）。
    pub env: fn(&'a dyn Pod) -> Handle,

    /// `sin(of)`。
    pub sin: fn(&'a dyn Pod, of: Handle) -> Handle,

    /// `cos(of)`。
    pub cos: fn(&'a dyn Pod, of: Handle) -> Handle,

    /// `a + b`。
    pub add: fn(&'a dyn Pod, a: Handle, b: Handle) -> Handle,

    /// `a * b`。
    pub mul: fn(&'a dyn Pod, a: Handle, b: Handle) -> Handle,

    /// `k * of`（`k` 进产物时是字面量叶子）。
    pub scale: fn(&'a dyn Pod, k: f64, of: Handle) -> Handle,

    /// `Σ terms`（数组参数；空数组 = 零元 `Pure(0)`）。
    pub sum: fn(&'a dyn Pod, terms: Vec<Handle>) -> Handle,

    /// 把表达式导出为 JSON（**产物侧证据**：含库 namespace + version）。
    pub expression: fn(&'a dyn Pod, root: Handle) -> String,

    /// 登记一条**表达式动画**（S3）：对象名 + 目标名 + 根句柄 + 时长。
    ///
    /// 这里只记录；宿主在脚本结束后把它并入 `SceneScript`（对象名在装配期解析）。
    ///
    /// ⚠️ 语义：表达式读**全局** `Time` / `Frame`；`duration` 只决定段区间（`Hold`）。
    /// 需要段内局部时间时应新增通道，而不是改 `Time` 的含义。
    pub bind: fn(&'a dyn Pod, spec: BindSpec) -> (),
}
