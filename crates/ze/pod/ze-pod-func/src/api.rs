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

/// 一条**通道引用**（提案 #6）：`namespace` + `name` 是数据。
///
/// - 基础通道：`{ namespace = "", name = "time" }`（宿主 M 直接注入）；
/// - 组件字段：`{ namespace = "zela.scene", name = "Circle.radius" }`
///   （B 批只提供选择键；G 批起 `Read` 用它读回组件字段）。
///
/// 它与 `ze-func` 的 `ChannelRef` **同形**：Pod 边界上只是数据，跨层不需要转换规则。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Channel")]
pub struct ChannelSpec {
    /// 命名词汇表的 namespace（空串 = 宿主基础通道）。
    pub namespace: String,
    /// 通道名（基础通道 `time` / `frame`；组件字段是地址 `Circle.radius`）。
    pub name: String,
}

/// `Read { channel = ..., ty = ..., object = ... }`：读**某对象**上的组件字段
/// （**G 批通道读端**；运行期由宿主 `Mono::read_object` 经 `SceneRead` 读回）。
///
/// `channel` 用组件的 `channel` 常量（`Circle.channel.radius`，`name` 是规范地址
/// `Circle.radius`）；`ty` 是 F 批 `FieldSpec.ty` 的映射（`Number` / `Vec2` / `Vec3`，
/// 脚本侧语言绑定在生成通道常量时填充 —— 本批的 Rust 侧构造直接填写）；
/// `object` 是目标对象句柄（`scene.spawn` 的返回值）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Read")]
pub struct ReadSpec {
    /// 通道选择键（`namespace = "zela.scene"`，`name` 是规范地址如 `Circle.radius`）。
    pub channel: ChannelSpec,
    /// 字段值类型名（F 批 `FieldSpec.ty` 的映射：`Number` / `Vec2` / `Vec3`）。
    pub ty: String,
    /// 目标对象**句柄**（`scene.spawn` 的返回值）。
    pub object: Handle,
}

/// 一层**时钟链接**：`local = (parent_local − shift) × rate`。
///
/// 这是脚本侧 `Clock` 的扁平化形态：`clock.nest(parent, child)` 只是把链拼起来，
/// 产物里没有"父指针"需要解析。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.ClockLink")]
pub struct ClockLinkSpec {
    /// 零点偏移（父级本地时间里的 shift）。
    pub shift: f64,
    /// 线性速率缩放。
    pub rate: f64,
}

/// 一条**时钟**（脚本侧）：从根到本时钟的线性链（空链 = 全局时钟）。
///
/// 每条绑定自带时钟：表达式里的 `time` 读的是**该绑定的时钟局部时间**，
/// 与 `Curve` 路径的 `eval_local` 同源（G1 由此消失，而不是被修）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Clock")]
pub struct ClockSpec {
    /// 从根到本时钟逐层链接（空 = 全局）。
    pub links: Vec<ClockLinkSpec>,
}

/// predicate：绑定的**生效条件**（Bool 表达式）。
///
/// 缺省 = `Always`；显式写 `Predicate.Of { expr = … }` 才算门槛。
/// Bool 走独立求值路径（不经过 `anim_value`）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Predicate")]
pub enum PredicateSpec {
    /// 恒真（缺省）。
    Always,
    /// 由一条 Bool 表达式决定。
    Of {
        /// Bool 表达式根句柄（`Lt` / `And` / `Not` …）。
        expr: Handle,
    },
}

/// reset：predicate 失效时的行为。
///
/// ⚠️ 计划里的 `none` 在这里叫 `Skip`（`None` 在 Python 里是字面量，不能作枚举成员）：
/// 语义相同 —— **不写**，字段保持原样，可被其它绑定接管。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Reset")]
pub enum ResetSpec {
    /// 保持**最后一次生效**时写入的值（有历史依赖 ⇒ 引擎推导为 Replay）。
    Hold,
    /// 写回绑定生效前的**初值**（`commit` 时从场景快照，无历史依赖）。
    Reset,
    /// 不写（字段保持原样，可被其它绑定接管）。
    Skip,
}

/// 一条待绑定的表达式动画（S3）。
///
/// 表构造器形态（与 `scene.spawn{...}` 同构）：
/// `expr.bind{ object = dot, target = Circle.channel.radius, root = y, duration = 2.0 }`。
///
/// `target` 用**组件的 `channel` 常量**（`Circle.channel.radius`，由 `Pod::channels()`
/// 挂在构造器上），不接受裸地址字符串 —— 写错常量名在绑定层立刻暴露。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Bind")]
pub struct BindSpec {
    /// 目标对象**句柄**（`scene.spawn` 的返回值；提案 #6 起不再按名字找对象）。
    pub object: Handle,
    /// 动画目标（通道选择键；`name` 是 `zela.scene` 的地址表键）。
    pub target: ChannelSpec,
    /// 表达式根句柄。
    pub root: Handle,
    /// 时间源（`clock.shift` / `clock.scale` / `clock.nest`；缺省 = 全局时钟）。
    pub clock: ClockSpec,
    /// 生效条件（缺省 `Predicate.Always`）。
    pub predicate: PredicateSpec,
    /// 失效时行为（缺省 `Reset.Hold`）。
    pub reset: ResetSpec,
    /// **语法糖**：`> 0` 时等价于 `predicate = Predicate.Of { expr = Lt { t, duration } }`
    /// + `reset = Reset.Hold`；与显式 predicate 同时出现是装配期错误。
    pub duration: f64,
}

/// `Sin { of = ... }`。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Sin")]
pub struct SinSpec {
    pub of: Handle,
}

/// `Cos { of = ... }`。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Cos")]
pub struct CosSpec {
    pub of: Handle,
}

/// `Add { a = ..., b = ... }`。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Add")]
pub struct AddSpec {
    pub a: Handle,
    pub b: Handle,
}

/// `Mul { a = ..., b = ... }`。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Mul")]
pub struct MulSpec {
    pub a: Handle,
    pub b: Handle,
}

/// `Scale { k = ..., of = ... }`（`k` 进产物时是字面量叶子）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Scale")]
pub struct ScaleSpec {
    pub k: f64,
    pub of: Handle,
}

/// `Sum { terms = { ... } }`。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Sum")]
pub struct SumSpec {
    pub terms: Vec<Handle>,
}

/// `Lt { a = ..., b = ... }`：`a < b`（数值 → Bool）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Lt")]
pub struct LtSpec {
    pub a: Handle,
    pub b: Handle,
}

/// `Gt { a = ..., b = ... }`：`a > b`（数值 → Bool）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Gt")]
pub struct GtSpec {
    pub a: Handle,
    pub b: Handle,
}

/// `Le { a = ..., b = ... }`：`a ≤ b`（数值 → Bool）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Le")]
pub struct LeSpec {
    pub a: Handle,
    pub b: Handle,
}

/// `Ge { a = ..., b = ... }`：`a ≥ b`（数值 → Bool）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Ge")]
pub struct GeSpec {
    pub a: Handle,
    pub b: Handle,
}

/// `Eq { a = ..., b = ... }`：数值或布尔相等。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Eq")]
pub struct EqSpec {
    pub a: Handle,
    pub b: Handle,
}

/// `Ne { a = ..., b = ... }`：数值或布尔不等（U2：不做隐式真值转换，请显式比较）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Ne")]
pub struct NeSpec {
    pub a: Handle,
    pub b: Handle,
}

/// `And { a = ..., b = ... }`：布尔合取。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.And")]
pub struct AndSpec {
    pub a: Handle,
    pub b: Handle,
}

/// `Or { a = ..., b = ... }`：布尔析取。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Or")]
pub struct OrSpec {
    pub a: Handle,
    pub b: Handle,
}

/// `Not { of = ... }`：布尔取反。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Not")]
pub struct NotSpec {
    pub of: Handle,
}

/// 表达式叶子算子集合。
///
/// 字段名就是脚本侧函数名（`expr.sin` / `expr.mul` …）。
///
/// 参数顺序与 [`crate::expr_pod`] 的注册顺序**逐条一致**（协商按完整函数名匹配，
/// 漂移会得到显式失败而不是错位调用）。
#[capability(
    namespace = "zela.math",
    version = "7",
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

    /// `Read { channel = ..., object = ... }`：读**某对象**上的组件字段
    /// （**G 批通道读端**；运行期由宿主 `Mono::read_object` 经 `SceneRead` 读回）。
    pub read: fn(&'a dyn Pod, spec: ReadSpec) -> Handle,

    /// `Sin { of = ... }`（表构造器；与 `spawn{}` 同构）。
    pub sin: fn(&'a dyn Pod, spec: SinSpec) -> Handle,

    /// `Cos { of = ... }`。
    pub cos: fn(&'a dyn Pod, spec: CosSpec) -> Handle,

    /// `Add { a = ..., b = ... }`。
    pub add: fn(&'a dyn Pod, spec: AddSpec) -> Handle,

    /// `Mul { a = ..., b = ... }`。
    pub mul: fn(&'a dyn Pod, spec: MulSpec) -> Handle,

    /// `Scale { k = ..., of = ... }`（`k` 进产物时是字面量叶子）。
    pub scale: fn(&'a dyn Pod, spec: ScaleSpec) -> Handle,

    /// `Sum { terms = { ... } }`（空数组 = 零元 `Pure(0)`）。
    pub sum: fn(&'a dyn Pod, spec: SumSpec) -> Handle,

    /// `Lt { a = ..., b = ... }`：`a < b`（数值 → `Bool`）。
    pub lt: fn(&'a dyn Pod, spec: LtSpec) -> Handle,

    /// `Gt { a = ..., b = ... }`：`a > b`（数值 → `Bool`）。
    pub gt: fn(&'a dyn Pod, spec: GtSpec) -> Handle,

    /// `Le { a = ..., b = ... }`：`a ≤ b`（数值 → `Bool`）。
    pub le: fn(&'a dyn Pod, spec: LeSpec) -> Handle,

    /// `Ge { a = ..., b = ... }`：`a ≥ b`（数值 → `Bool`）。
    pub ge: fn(&'a dyn Pod, spec: GeSpec) -> Handle,

    /// `Eq { a = ..., b = ... }`：数值或布尔相等（不做隐式真值转换）。
    pub eq: fn(&'a dyn Pod, spec: EqSpec) -> Handle,

    /// `Ne { a = ..., b = ... }`：数值或布尔不等（U2：请显式比较，不要依赖 0 = false）。
    pub ne: fn(&'a dyn Pod, spec: NeSpec) -> Handle,

    /// `And { a = ..., b = ... }`：布尔合取。
    ///
    /// ⚠️ 函数名就是 `and`（Lua / Python 里它是关键字，**那些语言请用表构造器**
    /// `And { a = ..., b = ... }`；JS 侧 `expr.and(...)` 可用）。
    pub and: fn(&'a dyn Pod, spec: AndSpec) -> Handle,

    /// `Or { a = ..., b = ... }`：布尔析取（同上：Lua / Python 用 `Or {}`）。
    pub or: fn(&'a dyn Pod, spec: OrSpec) -> Handle,

    /// `Not { of = ... }`：布尔取反（同上：Lua / Python 用 `Not {}`）。
    pub not: fn(&'a dyn Pod, spec: NotSpec) -> Handle,

    /// 把表达式导出为 JSON（**产物侧证据**：含库 namespace + version）。
    pub expression: fn(&'a dyn Pod, root: Handle) -> String,

    /// 时钟：在全局时钟之上加一层 `shift`（零点偏移）。
    ///
    /// 脚本侧也挂在 `clock.shift(s)`（见 binding 的 `clock_` 前缀别名表）。
    pub clock_shift: fn(&'a dyn Pod, shift: f64) -> ClockSpec,

    /// 时钟：在全局时钟之上加一层 `rate`（线性速率缩放）。
    pub clock_scale: fn(&'a dyn Pod, rate: f64) -> ClockSpec,

    /// 时钟：把 `child` 接在 `parent` 之后（嵌套）。
    ///
    /// `clock.nest(clock.scale(0.5), clock.shift(2.0))` = "半速时钟，内部再偏移 2s"。
    pub clock_nest: fn(&'a dyn Pod, parent: ClockSpec, child: ClockSpec) -> ClockSpec,

    /// 时钟局部时间叶子（与 `expr.time()` 是**同一个**通道）。
    ///
    /// 绑定求值时宿主用该绑定自己的时钟注入 `time` ⇒ 不需要把时钟身份写进表达式。
    pub clock_time: fn(&'a dyn Pod) -> Handle,

    /// 登记一条**表达式动画**（S3）：对象句柄 + 目标通道 + 根句柄 + 时长。
    ///
    /// 这里只记录；宿主在脚本结束后把它并入 `SceneScript`（句柄在装配期转换，
    /// 产物只存脚本内的局部句柄）。
    ///
    /// ⚠️ 语义：表达式读**全局** `Time` / `Frame`；`duration` 只决定段区间（`Hold`）。
    /// 需要段内局部时间时应新增通道，而不是改 `Time` 的含义。
    pub bind: fn(&'a dyn Pod, spec: BindSpec) -> (),
}

// ─── G 批（D14）：能力指纹与版本的配对基线 ─────────────────────────────────
//
// 规则（硬，失败不是警告）：`signatures()` 变了而 `version` 没递增 ⇒ 测试红。
// 改 API 的正确流程：**递增 `version`** → 更新对应版本常量 → 更新指纹常量。
// 指纹算法是固定 FNV-1a（跨 Rust 版本稳定），见 `ze_pod_core::capability_fingerprint`。

/// `zela.math` 的当前自述版本（必须与 `#[capability(version = ...)]` 逐字一致）。
pub const MATH_API_VERSION: &str = "7";
/// `zela.math` 的 `hash(signatures())` 基线（G 批 `expr.read` 后更新）。
pub const MATH_API_FINGERPRINT: u64 = 6_414_786_024_451_512_435;

#[cfg(test)]
mod fingerprint_tests {
    use super::*;
    use ze_pod_core::{Capability, capability_fingerprint};

    /// D14：签名变化而未递增 version ⇒ 本用例失败（先修常量，再补基线）。
    #[test]
    fn math_api_fingerprint_is_pinned_to_version() {
        assert_eq!(
            MathApi::<'static>::meta().version,
            Some(MATH_API_VERSION),
            "版本常量与 capability 声明不一致（以代码为准，先修常量）"
        );
        assert_eq!(
            capability_fingerprint(MathApi::<'static>::signatures()),
            MATH_API_FINGERPRINT,
            "signatures 变了 ⇒ 必须递增 version **并**更新指纹常量（D14：变而未递增即 fail）"
        );
    }
}
