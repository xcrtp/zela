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

/// 通道**值类型**（结构化，不是字符串）。
///
/// 第 2 批：类型随通道常量一起过边界 —— 脚本不再手写 `ty = "Vec2"`，
/// 也就没有"写错类型名静默退化成 Number"的路径（未知类型是显式状态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PodType)]
#[pod_type(name = "zela.math.ChannelType")]
pub enum ChannelTypeSpec {
    /// 未知（老 payload / 未标注的第三方常量）：装配期推导会**显式报错**，
    /// 回放路径不做推导，所以它是合法状态。
    Unknown,
    Number,
    Bool,
    Vec2,
    Vec3,
}

/// 一条**通道引用**（提案 #6）：`namespace` + `name` + `ty` 是数据。
///
/// - 基础通道：`{ namespace = "", name = "time", ty = Number }`（宿主 M 直接注入）；
/// - 组件字段：`{ namespace = "zela.scene", name = "Circle.radius", ty = Number }`
///   （G 批起 `Read` 用它读回组件字段）。
///
/// 它与 `ze-comb` 的 `ChannelRef` **同形**：Pod 边界上只是数据，跨层不需要转换规则。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Channel")]
pub struct ChannelSpec {
    /// 命名词汇表的 namespace（空串 = 宿主基础通道）。
    pub namespace: String,
    /// 通道名（基础通道 `time` / `frame`；组件字段是地址 `Circle.radius`）。
    pub name: String,
    /// 值类型（来自 `FieldSpec.ty`；基础通道是 `Number`）。
    pub ty: ChannelTypeSpec,
}

/// `Read { channel = ..., object = ... }`：读**某对象**上的组件字段
/// （**G 批通道读端**；运行期由宿主 `Mono::read_object` 经 `SceneRead` 读回）。
///
/// `channel` 用组件的 `channel` 常量（`Circle.channel.radius`）—— **类型在常量里**，
/// 所以这里不再有 `ty` 字符串参数（第 2 批）；`object` 是目标对象句柄。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Read")]
pub struct ReadSpec {
    /// 通道选择键（常量自带 `namespace` / `name` / `ty`）。
    pub channel: ChannelSpec,
    /// 目标对象**句柄**（`scene.spawn` 的返回值）。
    pub object: Handle,
}

/// `Select { cond = ..., then = ..., otherwise = ... }`：**值分支**（H 批求值已实现）。
///
/// 脚本侧第三个字段叫 `otherwise` 而不是 `else` —— Python / Lua / TS 里 `else` 是
/// 关键字（与 `Reset.none → Skip` 同一处理）。产物里的节点形状仍是 `cond / then / else`，
/// 两套命名在 `select_ctor` 一处转换。
///
/// 语义：`cond` 必须是 Bool（装配期 L2 推导 + 运行期 `EvalError::SelectCondition`
/// 双重把关）；两个分支**惰性求值** —— 只算被选中那一支。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Select")]
pub struct SelectSpec {
    /// 条件（Bool 表达式，通常是 `Gt { a = expr.read{…}, b = … }`）。
    pub cond: Handle,
    /// `cond == true` 时求值的分支。
    pub then: Handle,
    /// `cond == false` 时求值的分支（产物字段名是 `else`）。
    pub otherwise: Handle,
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

/// **阶段 2**：`expr.shader { object = <句柄>, root = <表达式> }`。
///
/// 把表达式编译成 WGSL（`eval` 返回 `vec3<f32>`），作为对象的"着色 codegen"：
/// 填充色由 GPU 求值决定（装饰性，不改几何 / 状态）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Shader")]
pub struct ShaderSpec {
    /// 目标对象**句柄**（`scene.spawn` 的返回值）。
    pub object: Handle,
    /// 表达式根句柄（必须返回 `Vec3`；本版不含通道）。
    pub root: Handle,
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

// ─── 第 1 批（算子补齐）：插值 / 向量 / 分量 / 旋转 ──────────────────────
//
// 这批算子让脚本**不再需要 `Curve` 变体**表达常见动画：
// `Ramp` ⇒ `mix(from, to, clamp(t / dur, 0, 1))`，`Orbit` ⇒ `rotate3`。

/// `mix(a, b, t)`：线性插值（数值 / Vec2 / Vec3 同型）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Mix")]
pub struct MixSpec {
    pub a: Handle,
    pub b: Handle,
    pub t: Handle,
}

/// `clamp(x, lo, hi)`：区间钳制（与 predicate / reset 同族）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Clamp")]
pub struct ClampSpec {
    pub x: Handle,
    pub lo: Handle,
    pub hi: Handle,
}

/// `vec2(x, y)`：两个标量合成一个 `Vec2`。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Vec2Of")]
pub struct Vec2OfSpec {
    pub x: Handle,
    pub y: Handle,
}

/// `vec3(x, y, z)`：三个标量合成一个 `Vec3`。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Vec3Of")]
pub struct Vec3OfSpec {
    pub x: Handle,
    pub y: Handle,
    pub z: Handle,
}

/// `x(v)` / `y(v)` / `z(v)`：向量分量访问（`z` 对 `Vec2` 显式报错）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Component")]
pub struct ComponentSpec {
    pub v: Handle,
}

/// `rotate3(v, axis, angle)`：罗德里格斯旋转（`Curve::Orbit` 的算子化形态）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Rotate3")]
pub struct Rotate3Spec {
    pub v: Handle,
    pub axis: Handle,
    pub angle: Handle,
}

// ─── 第 3 批（类型系统扩展）：转换算子族 ────────────────────────────────
//
// 这五个是**纯结构转换**（不引入新语义），补齐 `Curve` 变体里需要的
// 标量 ↔ 向量升降维与向量归一化。

/// `splat(v)`：一个标量铺成三个分量（`F64 → Vec3`）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Splat")]
pub struct SplatSpec {
    pub v: Handle,
}

/// `vec3_from_vec2(v, z)`：二维升三维（显式补 z）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Vec3FromVec2")]
pub struct Vec3FromVec2Spec {
    pub v: Handle,
    pub z: Handle,
}

/// `vec2_from_vec3(v)`：三维降二维（**显式**丢弃 z）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Vec2FromVec3")]
pub struct Vec2FromVec3Spec {
    pub v: Handle,
}

/// `length(v)`：向量模长（`Vector → F64`）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Length")]
pub struct LengthSpec {
    pub v: Handle,
}

/// `normalize(v)`：单位化（零向量保持零，见 `ops::eval`）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Normalize")]
pub struct NormalizeSpec {
    pub v: Handle,
}

/// `round_f32(x)`：把数值落成 f32 再升回 f64（逐位复刻旧曲线的精度口径）。
///
/// **已更名**为 [`NarrowF32Spec`]（BB 批）：`round` 算子进来后，旧名读起来像
/// "四舍五入到整数"。旧名保留，老 payload 仍能加载。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.RoundF32")]
pub struct RoundF32Spec {
    pub of: Handle,
}

/// `narrow_f32(x)`：**收窄**到 f32 再升回 f64（`RoundF32` 的新名字，语义相同）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.NarrowF32")]
pub struct NarrowF32Spec {
    pub of: Handle,
}

/// 第 BB 批（算子扩充）：一元 / 二元 / 三段算子的参数结构体。
///
/// 每行只写一次：PodType 全名、结构体名、字段（= 实参顺序）。
/// 字段说明见各算子函数；这里只声明形状，语义在 `ze-func::ops`。
macro_rules! numeric_specs {
    ($(
        $(#[$doc:meta])*
        $spec:ident, $pod:literal, [$($field:ident),+] ;
    )*) => {
        $(
            $(#[$doc])*
            #[derive(Clone, Debug, PartialEq, PodType)]
            #[pod_type(name = $pod)]
            pub struct $spec {
                $( pub $field: Handle, )+
            }
        )*
    };
}

numeric_specs! {
    /// `sqrt(x)`：平方根。
    SqrtSpec, "zela.math.Sqrt", [of];
    /// `inversesqrt(x)`：`1 / sqrt(x)`。
    InverseSqrtSpec, "zela.math.InverseSqrt", [of];
    /// `abs(x)`：绝对值。
    AbsSpec, "zela.math.Abs", [of];
    /// `sign(x)`：正 1 / 负 −1 / 其余 0。
    SignSpec, "zela.math.Sign", [of];
    /// `min(a, b)`。
    MinSpec, "zela.math.Min", [a, b];
    /// `max(a, b)`。
    MaxSpec, "zela.math.Max", [a, b];
    /// `floor(x)`：向下取整。
    FloorSpec, "zela.math.Floor", [of];
    /// `ceil(x)`：向上取整。
    CeilSpec, "zela.math.Ceil", [of];
    /// `trunc(x)`：向零取整。
    TruncSpec, "zela.math.Trunc", [of];
    /// `round(x)`：就近取整，半数远离零。
    RoundSpec, "zela.math.Round", [of];
    /// `modulo(a, b)`：截断余数（作者侧叫 `modulo`：`mod` 在 Rust / Lua 里是关键字）。
    ModuloSpec, "zela.math.Modulo", [a, b];
    /// `exp(x)`。
    ExpSpec, "zela.math.Exp", [of];
    /// `exp2(x)`。
    Exp2Spec, "zela.math.Exp2", [of];
    /// `log(x)`：自然对数。
    LogSpec, "zela.math.Log", [of];
    /// `log2(x)`。
    Log2Spec, "zela.math.Log2", [of];
    /// `pow(a, b)`：`a^b`。
    PowSpec, "zela.math.Pow", [a, b];
    /// `tan(x)`。
    TanSpec, "zela.math.Tan", [of];
    /// `asin(x)`。
    AsinSpec, "zela.math.Asin", [of];
    /// `acos(x)`。
    AcosSpec, "zela.math.Acos", [of];
    /// `atan(x)`。
    AtanSpec, "zela.math.Atan", [of];
    /// `atan2(y, x)`：实参顺序与 WGSL `atan2(y, x)` 一致。
    Atan2Spec, "zela.math.Atan2", [y, x];
    /// `degrees(x)`：弧度 → 角度。
    DegreesSpec, "zela.math.Degrees", [of];
    /// `radians(x)`：角度 → 弧度。
    RadiansSpec, "zela.math.Radians", [of];
    /// `step(edge, x)`：`x < edge` 取 0，否则 1。
    StepSpec, "zela.math.Step", [edge, x];
    /// `dot(a, b)`：点积（两个同维向量）。
    DotSpec, "zela.math.Dot", [a, b];
    /// `cross(a, b)`：叉积（两个 `Vec3`）。
    CrossSpec, "zela.math.Cross", [a, b];
    /// `reflect(e1, e2)`：入射向量关于法线反射。
    ReflectSpec, "zela.math.Reflect", [e1, e2];
    /// `faceforward(e1, e2, e3)`：三个同维向量。
    FaceforwardSpec, "zela.math.Faceforward", [e1, e2, e3];
}

// ─── 内建 mono（阶段 3）：shader 里常用、但此前只能在表达式里拼出来的几个 ──

/// `distance(a, b)`：两点距离（`Vec2` / `Vec3` → 数）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Distance")]
pub struct DistanceSpec {
    pub a: Handle,
    pub b: Handle,
}

/// `smoothstep(e0, e1, x)`：Hermite 平滑。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Smoothstep")]
pub struct SmoothstepSpec {
    pub e0: Handle,
    pub e1: Handle,
    pub x: Handle,
}

/// `fract(x)`：小数部分。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Fract")]
pub struct FractSpec {
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

/// `BindEach { … }`：**装配期展开**的批量绑定（B4 Query）。
///
/// 过滤条件只能是"组件类型组合"（装配期可判定）；结果按 `ObjectId` 升序冻结成
/// N 份具体绑定 —— 产物里既没有 `it` 也没有 query。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.BindEach")]
pub struct BindEachSpec {
    /// 目标通道（`Circle.channel.radius`）。
    pub target: ChannelSpec,
    /// 根表达式（可用 `expr.query_self(channel)` 引用"当前对象"）。
    pub root: Handle,
    /// 必须**同时**具备的组件名（稳定 Rust 类型名）。
    ///
    /// 字段名是 `requires` / `excludes` 而不是 `with` / `without`：
    /// `with` 是 Python 关键字（同 `Select` 的 `otherwise` 之于 `else`）。
    /// 必填字段排在可选字段之前 —— 生成的 `.pyi` 里"默认值后接必填"是语法错误。
    pub requires: Vec<String>,
    /// 必须**不**具备的组件名。
    pub excludes: Vec<String>,
    /// 绑定时钟。
    pub clock: ClockSpec,
    pub predicate: PredicateSpec,
    pub reset: ResetSpec,
}

/// `exists { requires, excludes }`：集合**判空**（B4 Query）。
///
/// 过滤条件只有组件类型组合 ⇒ 装配期即可判定 ⇒ **装配期常量折叠**成 `Bool`
/// 字面量（折叠优先于区间代数：常量不依赖连续变量）。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Exists")]
pub struct ExistsSpec {
    /// 必须**同时**具备的组件名（稳定 Rust 类型名）。
    pub requires: Vec<String>,
    /// 必须**不**具备的组件名。
    pub excludes: Vec<String>,
}

/// `Method { name, object, args }`：调用**对象方法**（B2）。
///
/// 方法与字段的区别是类型级的：方法不在写表里，永远选不中作 target。
/// `name` 是稳定方法名（`Reveal.current_pos`）；`object` 是隐式首参。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.Method")]
pub struct MethodCallSpec {
    /// 方法名（`Reveal.current_pos`）。
    pub name: String,
    /// 接收者对象句柄。
    pub object: Handle,
    /// 显式实参（当前已注册的方法都无参）。
    pub args: Vec<Handle>,
}

/// `NatRec { count, init, step }`：离散折（阶段 3 的**合法递归**）。
///
/// `count` **必须是非负整数**（脚本侧是 `f64`：值系统还没有 i64 标量；
/// 非整数在装配期显式报错，强制作者自己写 `Floor` —— 刀刃取整只做一次）。
/// `step` 里只能用 `rec_prev()` 与 `rec_index()` 引用前一项 / 下标。
#[derive(Clone, Debug, PartialEq, PodType)]
#[pod_type(name = "zela.math.NatRec")]
pub struct NatRecSpec {
    /// 折几次（非负整数）。
    pub count: f64,
    /// 初值表达式。
    pub init: Handle,
    /// 递推步表达式（可引用 `rec_prev()` / `rec_index()`）。
    pub step: Handle,
}

/// 表达式叶子算子集合。
///
/// 字段名就是脚本侧函数名（`expr.sin` / `expr.mul` …）。
///
/// 参数顺序与 [`crate::expr_pod`] 的注册顺序**逐条一致**（协商按完整函数名匹配，
/// 漂移会得到显式失败而不是错位调用）。
#[capability(
    namespace = "zela.math",
    version = "22",
    description = "表达式层叶子算子（组合子库；只做表达，不取数）",
    bindings
)]
pub struct MathApi<'a> {
    /// 数值字面量（装配期算术的结果落在这里）。
    pub pure: fn(&'a dyn Pod, value: f64) -> Handle,

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

    /// `Select { cond, then, otherwise }`：**值分支**（H 批）。
    ///
    /// `cond` 必须 Bool；两个分支惰性求值（只算被选中那一支）。
    pub select: fn(&'a dyn Pod, spec: SelectSpec) -> Handle,

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

    /// `mix(a, b, t)`：线性插值（`Num | Vector` 同型；`Ramp` / `Ramp2` 的算子化形态）。
    pub mix: fn(&'a dyn Pod, spec: MixSpec) -> Handle,

    /// `clamp(x, lo, hi)`：区间钳制（与 predicate / reset 同族）。
    pub clamp: fn(&'a dyn Pod, spec: ClampSpec) -> Handle,

    /// `vec2(x, y)`：两个标量合成 `Vec2`。
    pub vec2: fn(&'a dyn Pod, spec: Vec2OfSpec) -> Handle,

    /// `vec3(x, y, z)`：三个标量合成 `Vec3`。
    pub vec3: fn(&'a dyn Pod, spec: Vec3OfSpec) -> Handle,

    /// `x(v)`：向量 x 分量。
    pub x: fn(&'a dyn Pod, spec: ComponentSpec) -> Handle,

    /// `y(v)`：向量 y 分量。
    pub y: fn(&'a dyn Pod, spec: ComponentSpec) -> Handle,

    /// `z(v)`：向量 z 分量（`Vec2` 上显式报错）。
    pub z: fn(&'a dyn Pod, spec: ComponentSpec) -> Handle,

    /// `rotate3(v, axis, angle)`：罗德里格斯旋转（`Curve::Orbit` 的算子化形态）。
    pub rotate3: fn(&'a dyn Pod, spec: Rotate3Spec) -> Handle,

    /// `splat(v)`：标量铺成 `Vec3`（三个分量同值）。
    pub splat: fn(&'a dyn Pod, spec: SplatSpec) -> Handle,

    /// `vec3_from_vec2(v, z)`：`Vec2` 升维到 `Vec3`（显式补 z）。
    pub vec3_from_vec2: fn(&'a dyn Pod, spec: Vec3FromVec2Spec) -> Handle,

    /// `vec2_from_vec3(v)`：`Vec3` 降维到 `Vec2`（**显式**丢弃 z）。
    pub vec2_from_vec3: fn(&'a dyn Pod, spec: Vec2FromVec3Spec) -> Handle,

    /// `length(v)`：向量模长（`Vector → F64`）。
    pub length: fn(&'a dyn Pod, spec: LengthSpec) -> Handle,

    /// `normalize(v)`：单位向量（零向量保持零）。
    pub normalize: fn(&'a dyn Pod, spec: NormalizeSpec) -> Handle,

    /// `round_f32(x)`：落成 f32 再升回 f64（第 5a 批：旧曲线的逐位等价工具）。
    ///
    /// **已更名**：新脚本请用 `narrow_f32`（语义相同，旧名保留给老 payload）。
    pub round_f32: fn(&'a dyn Pod, spec: RoundF32Spec) -> Handle,

    // ─── 第 BB 批（算子扩充）：对齐 WGSL 标准库 ─────────────────────────────
    /// `narrow_f32(x)`：收窄到 f32 再升回 f64（`round_f32` 的新名字）。
    pub narrow_f32: fn(&'a dyn Pod, spec: NarrowF32Spec) -> Handle,
    /// `sqrt(x)`。
    pub sqrt: fn(&'a dyn Pod, spec: SqrtSpec) -> Handle,
    /// `inversesqrt(x)`：`1 / sqrt(x)`。
    pub inversesqrt: fn(&'a dyn Pod, spec: InverseSqrtSpec) -> Handle,
    /// `abs(x)`。
    pub abs: fn(&'a dyn Pod, spec: AbsSpec) -> Handle,
    /// `sign(x)`：正 1 / 负 −1 / 其余 0（与 WGSL 同口径）。
    pub sign: fn(&'a dyn Pod, spec: SignSpec) -> Handle,
    /// `min(a, b)`。
    pub min: fn(&'a dyn Pod, spec: MinSpec) -> Handle,
    /// `max(a, b)`。
    pub max: fn(&'a dyn Pod, spec: MaxSpec) -> Handle,
    /// `floor(x)`。
    pub floor: fn(&'a dyn Pod, spec: FloorSpec) -> Handle,
    /// `ceil(x)`。
    pub ceil: fn(&'a dyn Pod, spec: CeilSpec) -> Handle,
    /// `trunc(x)`：向零取整。
    pub trunc: fn(&'a dyn Pod, spec: TruncSpec) -> Handle,
    /// `round(x)`：就近取整，半数远离零。
    pub round: fn(&'a dyn Pod, spec: RoundSpec) -> Handle,
    /// `modulo(a, b)`：截断余数（WGSL 生成 `%`）。
    pub modulo: fn(&'a dyn Pod, spec: ModuloSpec) -> Handle,
    /// `exp(x)`。
    pub exp: fn(&'a dyn Pod, spec: ExpSpec) -> Handle,
    /// `exp2(x)`。
    pub exp2: fn(&'a dyn Pod, spec: Exp2Spec) -> Handle,
    /// `log(x)`：自然对数。
    pub log: fn(&'a dyn Pod, spec: LogSpec) -> Handle,
    /// `log2(x)`。
    pub log2: fn(&'a dyn Pod, spec: Log2Spec) -> Handle,
    /// `pow(a, b)`。
    pub pow: fn(&'a dyn Pod, spec: PowSpec) -> Handle,
    /// `tan(x)`。
    pub tan: fn(&'a dyn Pod, spec: TanSpec) -> Handle,
    /// `asin(x)`。
    pub asin: fn(&'a dyn Pod, spec: AsinSpec) -> Handle,
    /// `acos(x)`。
    pub acos: fn(&'a dyn Pod, spec: AcosSpec) -> Handle,
    /// `atan(x)`。
    pub atan: fn(&'a dyn Pod, spec: AtanSpec) -> Handle,
    /// `atan2(y, x)`。
    pub atan2: fn(&'a dyn Pod, spec: Atan2Spec) -> Handle,
    /// `degrees(x)`：弧度 → 角度。
    pub degrees: fn(&'a dyn Pod, spec: DegreesSpec) -> Handle,
    /// `radians(x)`：角度 → 弧度。
    pub radians: fn(&'a dyn Pod, spec: RadiansSpec) -> Handle,
    /// `step(edge, x)`：`x < edge` 取 0，否则 1。
    pub step: fn(&'a dyn Pod, spec: StepSpec) -> Handle,
    /// `dot(a, b)`：点积（两个同维向量）。
    pub dot: fn(&'a dyn Pod, spec: DotSpec) -> Handle,
    /// `cross(a, b)`：叉积（两个 `Vec3`）。
    pub cross: fn(&'a dyn Pod, spec: CrossSpec) -> Handle,
    /// `reflect(e1, e2)`：入射向量关于法线反射。
    pub reflect: fn(&'a dyn Pod, spec: ReflectSpec) -> Handle,
    /// `faceforward(e1, e2, e3)`：`dot(e2, e3) < 0` 时保持 `e1`。
    pub faceforward: fn(&'a dyn Pod, spec: FaceforwardSpec) -> Handle,

    /// `distance(a, b)`：两点距离（`Vec2` 或 `Vec3` → 数；两实参同维）。
    pub distance: fn(&'a dyn Pod, spec: DistanceSpec) -> Handle,

    /// `smoothstep(e0, e1, x)`：Hermite 平滑（0..1；与 WGSL 内建同式）。
    pub smoothstep: fn(&'a dyn Pod, spec: SmoothstepSpec) -> Handle,

    /// `fract(x)`：小数部分（`x - floor(x)`；与 WGSL 内建同式）。
    pub fract: fn(&'a dyn Pod, spec: FractSpec) -> Handle,

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

    /// `Method { name, object, args }`：调用对象方法（`Reveal.current_pos`）。
    pub method: fn(&'a dyn Pod, spec: MethodCallSpec) -> Handle,

    /// `query_self(channel)`：批量绑定里的"当前对象"字段读（装配期替换为具体句柄）。
    pub query_self: fn(&'a dyn Pod, channel: ChannelSpec) -> Handle,

    /// `BindEach { … }`：装配期展开的批量绑定（按组件类型组合过滤）。
    pub bind_each: fn(&'a dyn Pod, spec: BindEachSpec) -> (),

    /// `exists { requires, excludes }`：集合判空（装配期常量折叠成 `Bool`）。
    pub exists: fn(&'a dyn Pod, spec: ExistsSpec) -> Handle,

    /// `NatRec { count, init, step }`：离散折（合法递归；`count` 必须是非负整数）。
    pub nat_rec: fn(&'a dyn Pod, spec: NatRecSpec) -> Handle,

    /// `NatRec.step` 内的前一项（只能出现在 `step` 内，装配期校验）。
    pub rec_prev: fn(&'a dyn Pod) -> Handle,

    /// `NatRec.step` 内的当前下标（I64）。
    pub rec_index: fn(&'a dyn Pod) -> Handle,

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

    /// **绑定时钟**的局部时间叶子（`clock.time()`；叶子不带时钟身份）。
    pub clock_time: fn(&'a dyn Pod) -> Handle,

    /// **指定时钟**的局部时间叶子（`clock.time_of(c)`）：叶子带上 `c` 的身份，
    /// 运行期按 `c` 的链在**同一全局时间**上反算 ⇒ 一个表达式里可以混多条时间轴。
    pub clock_time_of: fn(&'a dyn Pod, clock: ClockSpec) -> Handle,

    /// 窗口 `[lo, hi)`（绑定时钟）：`And(Ge(t, lo), Lt(t, hi))`。
    ///
    /// 这是四个脚本里 `in_segment` helper 的库内形态（事实标准库，不再各写一份）。
    pub clock_range: fn(&'a dyn Pod, lo: f64, hi: f64) -> Handle,

    /// 窗口 `[lo, hi)`（**指定时钟**）：叶子带时钟身份。
    pub clock_range_of: fn(&'a dyn Pod, clock: ClockSpec, lo: f64, hi: f64) -> Handle,

    /// 登记一条**表达式动画**（S3）：对象句柄 + 目标通道 + 根句柄 + 时长。
    ///
    /// 这里只记录；宿主在脚本结束后把它并入 `SceneScript`（句柄在装配期转换，
    /// 产物只存脚本内的局部句柄）。
    ///
    /// ⚠️ 语义：表达式读**全局** `Time` / `Frame`；`duration` 只决定段区间（`Hold`）。
    /// 需要段内局部时间时应新增通道，而不是改 `Time` 的含义。
    pub bind: fn(&'a dyn Pod, spec: BindSpec) -> (),

    /// **阶段 2**：把表达式编译成 WGSL，作为对象的"着色 codegen"（装饰性）。
    ///
    /// 这里只记录；宿主在脚本结束后把它挂成对象组件（`ShaderPaint`）。
    pub shader: fn(&'a dyn Pod, spec: ShaderSpec) -> (),

    /// **阶段 2**：着色内建通道（对象在视图里的中心 / 尺寸，像素）。
    pub center: fn(&'a dyn Pod) -> Handle,
    /// **阶段 2**：`size()` —— 对象包围盒的尺寸（像素）。
    pub size: fn(&'a dyn Pod) -> Handle,

    /// **阶段 3**：`pos2()` —— 片元在视图里的位置（像素，y 向下，与 `center` 同框架）。
    pub pos2: fn(&'a dyn Pod) -> Handle,
    /// **阶段 3**：`local()` —— 片元在对象包围盒里的位置（像素，从左上角起）。
    pub local: fn(&'a dyn Pod) -> Handle,
    /// **阶段 3**：`uv()` —— `local / size`（0..1）。
    pub uv: fn(&'a dyn Pod) -> Handle,
}

// ─── G 批（D14）：能力指纹与版本的配对基线 ─────────────────────────────────
//
// 规则（硬，失败不是警告）：`signatures()` 变了而 `version` 没递增 ⇒ 测试红。
// 改 API 的正确流程：**递增 `version`** → 更新对应版本常量 → 更新指纹常量。
// 指纹算法是固定 FNV-1a（跨 Rust 版本稳定），见 `ze_pod_core::capability_fingerprint`。

/// `zela.math` 的当前自述版本（必须与 `#[capability(version = ...)]` 逐字一致）。
///
/// `@10`（第 2 批）：`Channel` 增加结构化 `ty`，`Read` 去掉 `ty` 字符串。
/// `@11`（第 3 批）：新增转换算子族 `splat` / `vec3_from_vec2` / `vec2_from_vec3` /
/// `length` / `normalize`。
/// `@12`（第 5a 批）：新增 `round_f32`（旧曲线的 f32 精度落位，供逐位迁移）。
/// `@13`（5b B1）：删除 `time`（改由 `clock.time()` 作唯一入口），新增
/// `clock_time_of` / `clock_range` / `clock_range_of`（时钟身份 + 窗口函子）。
/// `@14`（5b B5）：新增 `nat_rec` / `rec_prev` / `rec_index`（离散层脚本入口）。
/// `@15`（5b B2）：新增 `method`（对象方法调用；`Reveal.current_pos` 从字段改为方法）。
/// `@16`（5b B4）：新增 `query_self` / `bind_each`（装配期展开的批量绑定）。
/// `@17`（5b B4）：新增 `exists`（集合判空；装配期常量折叠）。
/// `@18`（阶段 2）：新增 `shader`（表达式 → WGSL；装饰性着色 codegen）。
/// `@19`（阶段 2）：新增 `center` / `size`（对象级内建通道）。
/// `@20`（阶段 3）：新增 `pos2` / `local` / `uv`（逐片元内建通道）。
/// `@21`（阶段 3）：新增 `distance` / `smoothstep` / `fract`（内建 mono）。
/// `@22`（第 BB 批）：新增 28 个 WGSL 标准库算子（代数 / 指数 / 三角 / 向量），
/// 并新增 `narrow_f32`（`round_f32` 的新名字，旧名保留）。
///
/// **加算子的代价（实测）**：新增算子必然改 `signatures()` ⇒ 必须递增 version ⇒
/// `LIBRARY_VERSION` 进表达式产物 ⇒ **payload 逐位变化**（像素不变）。第 BB 批
/// 因此把 28 个算子**一次**加完、只 bump 一次（`@21 → @22`），而不是分五批 bump 五次。
pub const MATH_API_VERSION: &str = "22";
/// `zela.math` 的 `hash(signatures())` 基线。
pub const MATH_API_FINGERPRINT: u64 = 5_482_242_184_379_522_694;

/// **糖层能力**（第 BB 批，阶段 4）：装配期展开成 `zela.math` 的 core 算子组合。
///
/// # 为什么是独立 namespace（不是 `zela.math` 的字段）
///
/// 加一个 `zela.math` 字段 ⇒ `signatures()` 变 ⇒ `LIBRARY_VERSION` 必须递增 ⇒
/// **所有表达式 payload 变化**。糖的定位是"改它不 bump、不重录"（计划 §5.1），
/// 所以它挂在 `zela.sugar`：独立 namespace、独立版本，不碰 `zela.math` 的签名。
///
/// # 形式：函数（不是表构造器）
///
/// 算子是表构造器 `Mix { a = … }`，糖是函数 `sugar.ramp(0, 1, 0.5)` —— 形式差异
/// 是**有意的**：一眼能看出哪个是糖，以及糖该去哪里找等价 core 写法
/// （每个糖的文档都带「核心形态」，见 [`crate::sugar`] 的门禁用例）。
///
/// # 签名吃 `f64`
///
/// 糖的数值实参是 `f64`（不是 `Handle`）⇒ 作者不再写 `expr.pure(0.0)`；
/// 展开时由糖自己补 `pure` 叶子。
#[capability(
    namespace = "zela.sugar",
    version = "1",
    description = "表达式糖：装配期展开成 zela.math 的 core 算子组合（不进 payload）",
    bindings
)]
pub struct SugarApi<'a> {
    /// `ramp(from, to, dur)`：`Mix{pure(from), pure(to), Clamp{Scale{1/dur, t}, 0, 1}}`。
    pub ramp: fn(&'a dyn Pod, from: f64, to: f64, dur: f64) -> Handle,

    /// `ramp2(ax, ay, bx, by, dur)`：`ramp` 的 `Vec2` 版本（`Vec2Of` 两端）。
    pub ramp2: fn(&'a dyn Pod, ax: f64, ay: f64, bx: f64, by: f64, dur: f64) -> Handle,

    /// `orbit2d(radius, speed, phase)`：绕原点的圆轨道（`Vec3`，z = 0）。
    pub orbit2d: fn(&'a dyn Pod, radius: f64, speed: f64, phase: f64) -> Handle,

    /// `wave(base, amp, omega)`：`base + amp · sin(omega · t)`（f32 落位）。
    pub wave: fn(&'a dyn Pod, base: f64, amp: f64, omega: f64) -> Handle,

    /// `f32(x)`：把表达式落到 f32 精度（= `RoundF32 { of = x }`）。
    pub f32: fn(&'a dyn Pod, of: Handle) -> Handle,
}

/// `zela.sugar` 的当前自述版本（必须与 `#[capability(version = ...)]` 逐字一致）。
pub const SUGAR_API_VERSION: &str = "1";
/// `zela.sugar` 的 `hash(signatures())` 基线。
pub const SUGAR_API_FINGERPRINT: u64 = 7_485_630_297_182_827_369;

/// **方法签名目录**（作者面）：`(方法名, 显式参数个数, 返回类型)`。
///
/// 这是 `zela.math` 侧的唯一一份：`infer` 用它给 `Node::Method` 定型。
/// 求值注册表在 `ze-scene-core::animation::METHODS`；两侧由
/// `zela-scene-ecs` 的一致性用例逐条对拍（缺失任何一侧都红）。
pub const METHOD_SIGNATURES: &[(&str, usize, ChannelTypeSpec)] =
    &[("Reveal.current_pos", 0, ChannelTypeSpec::Vec2)];

/// 按名字查方法签名（未知 ⇒ `None`）。
pub fn method_signature(name: &str) -> Option<(usize, ChannelTypeSpec)> {
    METHOD_SIGNATURES
        .iter()
        .find(|(method, _, _)| *method == name)
        .map(|(_, arity, ret)| (*arity, *ret))
}

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

    /// 糖层的版本与指纹同样配对（独立 namespace ⇒ 独立基线）。
    #[test]
    fn sugar_api_fingerprint_is_pinned_to_version() {
        assert_eq!(
            SugarApi::<'static>::meta().version,
            Some(SUGAR_API_VERSION),
            "版本常量与 capability 声明不一致（以代码为准，先修常量）"
        );
        assert_eq!(
            capability_fingerprint(SugarApi::<'static>::signatures()),
            SUGAR_API_FINGERPRINT,
            "糖的签名变了 ⇒ 递增 `zela.sugar` 的 version 并更新指纹"
        );
    }
}
