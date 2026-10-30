// scene_script.rs — 场景脚本（Encoder / Script）：照抄 wgpu 的**两级结构**（P6/B1+B2）。
//
// # 为什么是 wgpu 形态（§1）
//
// ```text
// wgpu                                    本模块
// ────────────────────────────────────────────────────────────
// device.create_command_encoder()    →    SceneEncoder::new()
//   单线程录制、句柄是局部的               SceneEncoder（录制命令，句柄是**局部**的）
// encoder.finish()                   →    SceneEncoder::finish()
//   → CommandBuffer（不可变）              → SceneScript（不可变）
// queue.submit([cmd])                →    SceneScript::commit(&mut scene)
//   提交到已有资源                         提交到**已有 Scene**（不新建）
// ```
//
// 这个模型天然解决三件事：
//
// 1. **句柄** —— Encoder 内是局部下标 [`Handle`]，`commit` 时才映射为 [`ObjectId`]
//    ⇒ 声明期**不需要 Scene、也不需要 ObjectId**；
// 2. **合并** —— `SceneScript` 是命令序列 ⇒ `append` 就是"拼接 + 句柄偏移"（§3.3）；
// 3. **重播** —— Script 可再生（不依赖"状态由 t 唯一决定"，见 B4）。
//
// # 线程安全语义（§1.1）
//
// | 类型 | Send + Sync | 为什么 |
// |---|---|---|
// | `SceneEncoder` | 是（实现上无借用，故顺带 Send） | 录制中；本设计命令**持有**数据（不借用栈） |
// | `SceneScript`  | **是** | 成品、不可变；可跨线程传递、可合并 —— 有编译期断言钉住 |
//
// # 命令的载体：复用既有类型（§1.2）
//
// - 创建载荷直接用契约层既有的 `Components`（`Vec<Box<dyn Any + Send + Sync>>`）；
// - 动画目标用 [`TargetCtor`]（`commit` 时按真实 id 生成 trait object）；
// - 时序复用既有 [`Timeline`]（`play` / `wait` / `parallel` 的待解析目标版本）。
//
// # 句柄引用（"组件里存另一个对象的句柄"）
//
// 契约层的 `Visibility::for_camera` / `Parent` 里的 `ObjectId` 在**声明期**还不存在。
// 这里用 `generation == 0`（`ObjectId` 的**保留值**，真实对象恒 ≥ 1）编码"这是句柄引用"：
// 装配时写 [`handle_ref(h)`](handle_ref)，`commit` / `append` 在[**唯一改写点**](resolve_component_refs)
// 里统一解析 / 偏移。新增"携带句柄引用的组件"只需在那个函数里加一条 —— 不散落。

use ze_comb::{ChannelRef, CompiledExpr, Expr, OpRef};
use ze_scene_contract::{
    Components, ObjectId, Parent, Scene as SceneContract, Visibility, VisibilityTag,
};
use ze_time::{ClockForest, ClockProgram};

use crate::animation::binding::{BindingTable, LibraryRef, ResetBehavior};
use crate::animation::target::target_ctor_by_name;
use crate::animation::timeline::Timeline;
use crate::surface::SurfaceTextureRef;

/// 表达式绑定的装配期错误（**显式**，不静默丢弃）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExprBindError {
    /// 目标对象句柄越界（句柄只由 `spawn` 产出 ⇒ 正常不会发生，属内部不变量）。
    #[error("表达式绑定的目标对象句柄不存在：{0}")]
    UnknownObject(usize),
    /// 目标字段名不在构造器表里。
    #[error("未知的动画目标：`{0}`")]
    UnknownTarget(String),
    /// 表达式结构非法（悬空 Ref / 环 / 重名 id）。
    #[error("表达式非法：{0}")]
    BadExpression(String),
    /// 方法未注册 / 参数个数不符（**装配期**显式失败，不留给运行期）。
    #[error("未知或参数不匹配的方法：`{0}`")]
    UnknownMethod(String),
    /// 该通道尚未由宿主注入（Event / 组件字段 / Env 属后续批次）。
    #[error("通道 `{0}` 尚未由宿主注入")]
    UnsupportedChannel(ChannelRef),
    /// `predicate` 与 `duration` 同时给出（后者是 `clock.range(0, d)` 的糖）。
    #[error(
        "predicate 与 duration 不能同时指定（duration ≡ predicate `clock.range(0, d)` + reset hold）"
    )]
    ConflictingGate,
    /// `reset = Hold` 的 predicate 无法分析成局部时间区间集。
    ///
    /// Hold 的硬约束是**纯函数** `f(last_active(t))`（AH-07）；不可分析就只能退回
    /// "上一帧写过什么"的历史依赖 —— 这正是本批要消灭的东西，故在装配期拒绝。
    #[error(
        "reset = Hold 需要可分析的 predicate（`time` 与字面量比较 + And/Or/Not）；\
         当前形状含非时间依赖或非字面量边界 —— 请改成 reset = skip，或把窗口写成可分析形状"
    )]
    UnanalyzableHold,
    /// **G 批帧内定序**：装配期能静态看到的"读先于写"冲突 —— 表达式 `reader` 读
    /// `address`（某对象），而 `writer` 写同一对象的同一字段，且写者的时钟起点
    /// **晚于**读者 ⇒ 同一帧内读者会读到旧值 / 未定义值。显式报错，不静默。
    #[error(
        "帧内定序冲突：`{reader}` 读取 `{address}`，但 `{writer}` 在同一帧更晚处写入它 —— \
         读先于写 ⇒ 该帧读到的是旧值。请把写者放到读者之前（更早的时钟起点）"
    )]
    ReadOrderConflict {
        reader: String,
        writer: String,
        address: String,
    },
}

/// 宿主当前真正注入的通道（提案 #6 B：通道是数据，白名单也只有两条）。
///
/// **G 批**：组件字段通道（`zela.scene`）改查注册表 —— 地址在 [`TARGETS`] 里
/// 已登记（如 `Circle.radius`）⇒ 读端可访问，放行；未登记的 `zela.scene` 地址
/// 与其它 namespace 一律拒绝（与旧行为一致：显式 `UnsupportedChannel`）。
fn is_injected(channel: &ChannelRef) -> bool {
    if channel.is_builtin() {
        return matches!(channel.name.as_str(), "time" | "frame");
    }
    // 派生量是**方法**（B2），不再作为可读通道；这里只认可写字段地址。
    channel.namespace == "zela.scene"
        && crate::animation::target::target_entry(&channel.name).is_some()
}

fn validate_expression(expr: &Expr) -> Result<(), ExprBindError> {
    CompiledExpr::compile(expr).map_err(|error| ExprBindError::BadExpression(error.to_string()))?;

    // **B2**：方法先在注册表里解析（未知 / 参数个数不符 ⇒ 显式失败）。
    struct Methods;

    impl<'a> ze_comb::ExprVisitor<'a> for Methods {
        type Error = String;

        fn enter(
            &mut self,
            expr: &'a Expr,
            _parent: Option<&'a ze_comb::Node>,
        ) -> Result<bool, String> {
            if let Some((method, _object, args)) = expr.node.method() {
                return match crate::animation::target::method_entry(&method.name) {
                    Some(spec) if spec.params.len() == args.len() => Ok(true),
                    Some(spec) => Err(format!(
                        "`{method}` 参数个数应为 {}，实际 {}",
                        spec.params.len(),
                        args.len()
                    )),
                    None => Err(format!("`{method}` 未注册")),
                };
            }
            Ok(true)
        }
    }

    if let Some(message) = expr.walk(&mut Methods).err() {
        return Err(ExprBindError::UnknownMethod(message));
    }

    /// 未注通道检查：命中即用 `ChannelRef` 提前结束遍历（`walk` 的错误通道）。
    struct Unsupported;

    impl<'a> ze_comb::ExprVisitor<'a> for Unsupported {
        type Error = ChannelRef;

        fn enter(
            &mut self,
            expr: &'a Expr,
            _parent: Option<&'a ze_comb::Node>,
        ) -> Result<bool, ChannelRef> {
            // `Read` 是叶子（children() 为空）：它的 channel 也要过注入检查，
            // 否则未注册地址会在运行期才炸 —— 装配期就该显式拒绝。
            let channel = expr
                .node
                .channel()
                .or_else(|| expr.node.read().map(|(channel, _)| channel));
            match channel {
                Some(channel) if !is_injected(channel) => Err(channel.clone()),
                _ => Ok(true),
            }
        }
    }

    match expr.walk(&mut Unsupported).err() {
        Some(channel) => Err(ExprBindError::UnsupportedChannel(channel)),
        None => Ok(()),
    }
}

/// **G 批**：`Node::Read.object`（脚本局部句柄下标）必须落在已创建对象的范围内。
///
/// 越界说明脚本引用了不存在的句柄 —— 与绑定目标的 [`ExprBindError::UnknownObject`]
/// 同一口径，装配期显式报错。
fn check_read_objects(expr: &Expr, object_count: usize) -> Result<(), ExprBindError> {
    struct Bad {
        object_count: usize,
    }

    impl<'a> ze_comb::ExprVisitor<'a> for Bad {
        type Error = usize;

        fn enter(
            &mut self,
            expr: &'a Expr,
            _parent: Option<&'a ze_comb::Node>,
        ) -> Result<bool, usize> {
            if let Some((_, object, _)) = expr.node.method() {
                if object >= self.object_count as u64 {
                    return Err(object as usize);
                }
                return Ok(true);
            }
            match expr.node.read() {
                Some((_, object)) if object >= self.object_count as u64 => Err(object as usize),
                _ => Ok(true),
            }
        }
    }

    match expr.walk(&mut Bad { object_count }).err() {
        Some(index) => Err(ExprBindError::UnknownObject(index)),
        None => Ok(()),
    }
}

/// 录制一条**表达式绑定**（编码器与成品脚本共用）。
///
/// 对象用**脚本句柄**（提案 #6：`spawn` 已经返回句柄，按名字找对象的
/// `find_named` / `Name{}` 依赖已删除）。
///
/// `duration > 0` 是**糖**：展开成 `predicate = clock.range(0, duration)` + `reset = hold`
/// （产物里只留 predicate / reset；`duration` 不进新 payload）。
fn record_expression(
    timeline: &mut Timeline,
    object_count: usize,
    object: Handle,
    target: &str,
    expr: Expr,
    library: LibraryRef,
    clock: ClockProgram,
    predicate: Option<Expr>,
    reset: ResetBehavior,
    duration: f64,
) -> Result<(), ExprBindError> {
    validate_expression(&expr)?;
    if let Some(predicate) = &predicate {
        validate_expression(predicate)?;
    }
    check_read_objects(&expr, object_count)?;
    if let Some(predicate) = &predicate {
        check_read_objects(predicate, object_count)?;
    }
    let (predicate, reset) = match (predicate, duration) {
        (Some(_), duration) if duration > 0.0 => {
            return Err(ExprBindError::ConflictingGate);
        }
        (Some(predicate), _) => (Some(predicate), reset),
        (None, duration) if duration > 0.0 => {
            (Some(duration_predicate(duration)), ResetBehavior::Hold)
        }
        (None, _) => (None, reset),
    };
    // **AH-07 硬约束**：Hold 是纯函数 `f(last_active(t))`。不可分析的时间门在装配期
    // 拒绝，而不是留到运行期靠"上一帧写过什么"（那会重新引入顺序耦合）。
    if reset == ResetBehavior::Hold {
        if let Some(predicate) = &predicate {
            // 阶段 2：`Hold` 的 `last_active` 只能按**时间**反算；predicate 若依赖
            // 非时间连续量（`frame` / 组件字段读），显式拒绝（不静默当作恒真）。
            match crate::animation::predicate::analyze(predicate) {
                None => return Err(ExprBindError::UnanalyzableHold),
                Some(active) if active.has_non_time_variable() => {
                    return Err(ExprBindError::UnanalyzableHold);
                }
                Some(_) => {}
            }
        }
    }
    let (make, _) = target_ctor_by_name(target)
        .ok_or_else(|| ExprBindError::UnknownTarget(target.to_string()))?;
    if object.0 >= object_count {
        return Err(ExprBindError::UnknownObject(object.0));
    }
    timeline.bind_expr_pending(expr, library, clock, predicate, reset, make, object.0);
    Ok(())
}

/// `duration` 的糖：`predicate = clock.range(0, duration)`（`time` 是绑定时钟的局部时间）。
///
/// 展开形状与脚本手写 `Predicate.Of { expr = clock.range(0.0, d) }` **逐字节相同**
/// （`And [Ge [t, 0], Lt [t, d]]`）—— 因此把样板 predicate 换成 `duration = d`
/// 不改变产物（全批验收 #9）。加 `Ge(t, 0)` 对全局时钟是恒真，但让"段"成为
/// **闭合区间**：`clock.shift` 之后局部时间为负的前段不会被误算进窗口。
///
/// **装配层的用法**（`zela-app`）：在收集算子 revision **之前**先调用它把
/// `duration` 展开成 predicate ⇒ 产物里的 `and` / `ge` / `lt` revision 与
/// 手写 predicate 完全一致（否则两边的 `op_revisions` 会差三条）。
pub fn duration_predicate(duration: f64) -> Expr {
    let leaf = || Expr::channel(ChannelRef::builtin("time"));
    let ge = Expr::call(
        OpRef::new("zela.math", "ge"),
        [leaf(), Expr::pure(0.0)],
    );
    let lt = Expr::call(
        OpRef::new("zela.math", "lt"),
        [leaf(), Expr::pure(duration)],
    );
    Expr::call(OpRef::new("zela.math", "and"), [ge, lt])
}

/// **Encoder 内的局部句柄**：声明期索引，`commit` 时才映射到 [`ObjectId`]。
///
/// 与 wgpu 的录制期句柄同构：它只在**一段录制**内有意义，不可跨 Encoder 混用
/// （合并时由 [`SceneScript::append`] 的整体偏移负责对齐）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct Handle(pub usize);

/// 声明期"引用另一个 Encoder 句柄"的占位 [`ObjectId`]。
///
/// `generation == 0` 是 `ObjectId` 的**保留值**（`id.rs`：真实对象 `generation` 恒 ≥ 1），
/// 故该编码与任何真实句柄都不冲突；`commit` / `append` 在
/// [`resolve_component_refs`] / [`shift_component_refs`] 处解析 / 偏移。
pub const fn handle_ref(h: Handle) -> ObjectId {
    ObjectId::new(h.0 as u32, 0)
}

/// 该 `ObjectId` 是否为声明期占位引用（见 [`handle_ref`]）。
///
/// `pub`（非 `pub(crate)`）：ECS 侧的 `commit` 用例要断言"解析后不再有占位"。
pub fn is_handle_ref(id: ObjectId) -> bool {
    id.generation == 0
}

/// **句柄改写唯一落点（解析）**：把组件集合里的声明期占位引用换成真实 `ObjectId`。
///
/// 新增"携带句柄引用的组件"时**只改这里**（一次遍历，不散落）。
fn resolve_component_refs(components: &mut Components, resolve: impl Fn(ObjectId) -> ObjectId) {
    for c in components.iter_mut() {
        // 显式到 `dyn Component`（`Box<dyn Component>` 也满足 blanket impl，
        // 直接 `c.as_any_mut()` 会拿到 Box 的 Any）。
        if let Some(v) = (**c).as_any_mut().downcast_mut::<Visibility>() {
            for tag in &mut v.tags {
                if let VisibilityTag::Camera(id) = tag {
                    if is_handle_ref(*id) {
                        *id = resolve(*id);
                    }
                }
            }
        } else if let Some(p) = (**c).as_any_mut().downcast_mut::<Parent>() {
            if is_handle_ref(p.0) {
                p.0 = resolve(p.0);
            }
        } else if let Some(r) = (**c).as_any_mut().downcast_mut::<SurfaceTextureRef>() {
            if is_handle_ref(r.0) {
                r.0 = resolve(r.0);
            }
        }
    }
}

/// **句柄改写唯一落点（偏移）**：合并（[`SceneScript::append`]）时把占位引用整体 `+ base`。
fn shift_component_refs(components: &mut Components, base: usize) {
    for c in components.iter_mut() {
        if let Some(v) = (**c).as_any_mut().downcast_mut::<Visibility>() {
            for tag in &mut v.tags {
                if let VisibilityTag::Camera(id) = tag {
                    if is_handle_ref(*id) {
                        id.index += base as u32;
                    }
                }
            }
        } else if let Some(p) = (**c).as_any_mut().downcast_mut::<Parent>() {
            if is_handle_ref(p.0) {
                p.0.index += base as u32;
            }
        } else if let Some(r) = (**c).as_any_mut().downcast_mut::<SurfaceTextureRef>() {
            if is_handle_ref(r.0) {
                r.0.index += base as u32;
            }
        }
    }
}

/// 命令录制器（wgpu 的 `CommandEncoder`）。
///
/// 声明期的一切（创建 / 销毁 / 时序）都只记在**局部下标**上；`finish` 产出不可变的
/// [`SceneScript`]。构造与录制**不需要 `Scene`，也不需要 `ObjectId`**（§7.2.1）。
#[derive(Default)]
pub struct SceneEncoder {
    creates: Vec<Components>,
    despawns: Vec<Handle>,
    timeline: Timeline,
}

impl SceneEncoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 录一条创建命令，返回**局部句柄**。
    ///
    /// 不接住返回值也可以（与 `Scene::spawn` 一样）—— 后续要引用它就接住。
    pub fn spawn(&mut self, components: Components) -> Handle {
        let h = Handle(self.creates.len());
        self.creates.push(components);
        h
    }

    /// 录一条销毁命令（`commit` 时按映射到的 `ObjectId` 执行）。
    pub fn despawn(&mut self, handle: Handle) {
        self.despawns.push(handle);
    }

    /// 声明一条**表达式驱动**的动画（S3）：对象用本脚本内的句柄，目标按名字查表。
    ///
    /// 表达式在这里就完成结构校验（悬空 Ref / 环 / 尚未注入的通道），
    /// 错误是显式的 —— 不留给运行期静默跳过。
    ///
    /// `duration > 0` 是糖（`predicate = t < duration` + `reset = hold`）。
    /// **阶段 2（着色 codegen）**：给对象挂"填充色由生成的 WGSL 在 GPU 上算"的组件。
    ///
    /// 装饰性：不影响几何 / 状态；组件挂在创建载荷里 ⇒ 提交后与普通组件同样可见。
    pub fn shader_fill(&mut self, object: Handle, paint: ze_render_ir::ShaderPaint) -> Result<(), String> {
        let components = self
            .creates
            .get_mut(object.0 as usize)
            .ok_or_else(|| format!("着色 codegen 的对象句柄无效：{}", object.0))?;
        components.push(Box::new(paint));
        Ok(())
    }

    pub fn bind_expr(
        &mut self,
        object: Handle,
        target: &str,
        expr: Expr,
        library: LibraryRef,
        clock: ClockProgram,
        duration: f64,
    ) -> Result<(), ExprBindError> {
        self.bind_expr_gated(
            object,
            target,
            expr,
            library,
            clock,
            None,
            ResetBehavior::Hold,
            duration,
        )
    }

    /// 带 **predicate / reset** 的表达式绑定（C 批入口）。
    pub fn bind_expr_gated(
        &mut self,
        object: Handle,
        target: &str,
        expr: Expr,
        library: LibraryRef,
        clock: ClockProgram,
        predicate: Option<Expr>,
        reset: ResetBehavior,
        duration: f64,
    ) -> Result<(), ExprBindError> {
        record_expression(
            &mut self.timeline,
            self.creates.len(),
            object,
            target,
            expr,
            library,
            clock,
            predicate,
            reset,
            duration,
        )
    }

    /// 收尾：产出一份不可变的 [`SceneScript`]。
    pub fn finish(self) -> SceneScript {
        SceneScript {
            creates: self.creates,
            despawns: self.despawns,
            timeline: self.timeline,
        }
    }
}

/// 合并（[`SceneScript::append`]）后 `other` 的句柄落位区间。
///
/// `other` 原来的 `Handle(i)` 现在等于 `Handle(range.start + i)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleRange {
    pub start: usize,
    pub len: usize,
}

impl HandleRange {
    /// `other` 的第 `i` 个句柄在本脚本里的新句柄。
    pub fn get(&self, i: usize) -> Option<Handle> {
        (i < self.len).then(|| Handle(self.start + i))
    }
}

/// 脚本的编译产物：时钟树 + 绑定表 + 句柄映射（提交到调度器所需的三样）。
///
/// 与 Scene 的**对象**分开：`commit` 只把对象写进 Scene，时钟树 / 绑定表是 Resource，
/// 由调用方取出后插入（"提交到已有 Scene，不新建"）。
#[derive(Debug)]
pub struct SceneRecipe {
    pub forest: ClockForest,
    pub bindings: BindingTable,
    /// `handles[i]` = 本脚本第 `i` 个 [`Handle`] 对应的 `ObjectId`。
    pub handles: Vec<ObjectId>,
}

impl SceneRecipe {
    /// 句柄 → `ObjectId`（越界 ⇒ `None`）。
    pub fn id(&self, h: Handle) -> Option<ObjectId> {
        self.handles.get(h.0).copied()
    }
}

/// `commit` 的失败语义（§7.1.7）：`Create` 失败 ⇒ **中止整个 commit**，与
/// `spawn_dyn` 既有的"不造半成品"口径一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptError {
    /// 第 `handle` 条创建命令被 Scene 拒绝（组件未登记 / 句柄非法）。
    CreateFailed { handle: Handle },
    /// **G 批帧内定序**：装配期发现"读先于写"（见 [`ExprBindError::ReadOrderConflict`]）。
    OrderConflict(ExprBindError),
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptError::CreateFailed { handle } => {
                write!(f, "场景脚本 commit 失败：第 {} 条创建命令被拒绝", handle.0)
            }
            ScriptError::OrderConflict(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ScriptError {}

/// 不可变的命令序列（wgpu 的 `CommandBuffer`）。
///
/// - **不可变且可 Clone** ⇒ 可跨线程传递、可合并、可**克隆后重新 commit**；
/// - `commit(&mut scene)` ⇒ **提交到已有 Scene，不新建**；可多次提交到同一场景。
///
/// # `commit` 为什么仍消耗 `self`
///
/// `Scene::spawn_dyn` 按值接收 `Components`。复用同一份脚本靠 **Clone**
/// （`Components` 现在可 Clone），而不是把 `commit` 改成 `&self`——
/// 后者会迫使 `spawn_dyn` 也改成借用，波及既有全部调用点。
#[derive(Clone)]
pub struct SceneScript {
    creates: Vec<Components>,
    despawns: Vec<Handle>,
    timeline: Timeline,
}

impl SceneScript {
    /// **阶段 2（着色 codegen）**：给对象挂"填充色由生成的 WGSL 在 GPU 上算"的组件。
    ///
    /// 与 [`SceneEncoder::shader_fill`] 同一口径：装饰性，不影响几何 / 状态。
    pub fn shader_fill(&mut self, object: Handle, paint: ze_render_ir::ShaderPaint) -> Result<(), String> {
        let components = self
            .creates
            .get_mut(object.0 as usize)
            .ok_or_else(|| format!("着色 codegen 的对象句柄无效：{}", object.0))?;
        components.push(Box::new(paint));
        Ok(())
    }

    /// **动画层阶段 1（plot）**：给对象挂一条**装配期采样**出来的折线（完整可见）。
    ///
    /// 与脚本手写 `Polyline` 走同一条路径（`Path2D::from_polyline` → `Reveal::visible`），
    /// 因此渲染侧不需要新图元；`closed` 恒为 `false`（函数图像是开放折线）。
    pub fn attach_polyline(&mut self, object: Handle, points: &[(f32, f32)]) -> Result<(), String> {
        let vertices: Vec<ze_notion::Vec2> = points
            .iter()
            .map(|&(x, y)| ze_notion::Vec2::new(x, y))
            .collect();
        let path = ze_notion::Path2D::from_polyline(&vertices, false);
        let components = self
            .creates
            .get_mut(object.0 as usize)
            .ok_or_else(|| format!("函数图像的对象句柄无效：{}", object.0))?;
        components.push(Box::new(crate::components::Reveal::visible(path)));
        Ok(())
    }
}

impl serde::Serialize for SceneScript {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(serde::Serialize)]
        struct Repr<'a> {
            creates: Vec<Vec<crate::persist::TaggedComponent>>,
            despawns: &'a [Handle],
            timeline: &'a Timeline,
        }

        let creates = self
            .creates
            .iter()
            .map(|components| {
                crate::persist::serialize_components(components).map_err(serde::ser::Error::custom)
            })
            .collect::<Result<Vec<_>, S::Error>>()?;

        Repr {
            creates,
            despawns: &self.despawns,
            timeline: &self.timeline,
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for SceneScript {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Repr {
            creates: Vec<Vec<crate::persist::TaggedComponent>>,
            despawns: Vec<Handle>,
            timeline: Timeline,
        }

        let repr = <Repr as serde::Deserialize>::deserialize(deserializer)?;
        let mut creates = Vec::with_capacity(repr.creates.len());
        for tagged in repr.creates {
            creates.push(
                crate::persist::deserialize_components(tagged).map_err(serde::de::Error::custom)?,
            );
        }
        Ok(Self {
            creates,
            despawns: repr.despawns,
            timeline: repr.timeline,
        })
    }
}

impl SceneScript {
    /// 序列化为 JSON 文本（持久化入口）。
    pub fn to_json(&self) -> Result<String, crate::persist::PersistError> {
        serde_json::to_string(self).map_err(|e| crate::persist::PersistError::Json(e.to_string()))
    }

    /// 从 JSON 文本反序列化。
    ///
    /// 函数类组件（`FunctionGraph`）得到的是**只有名字的壳**，调用前必须按名字
    /// 重新绑定（`ze-notion` 的 `FunctionX2Y::name`）。
    pub fn from_json(text: &str) -> Result<Self, crate::persist::PersistError> {
        serde_json::from_str(text).map_err(|e| crate::persist::PersistError::Json(e.to_string()))
    }

    /// 向**成品脚本**追加一条表达式绑定（S3：宿主编排完成后合并）。
    ///
    /// 对象用本脚本内的句柄（宿主把 pod 句柄在**装配期**转成它，产物不存 pod 句柄）；
    /// `clock` 是这条绑定自己的时间源（空链 = 全局）；目标未知 / 句柄越界都是显式错误。
    pub fn bind_expr(
        &mut self,
        object: Handle,
        target: &str,
        expr: Expr,
        library: LibraryRef,
        clock: ClockProgram,
        duration: f64,
    ) -> Result<(), ExprBindError> {
        self.bind_expr_gated(
            object,
            target,
            expr,
            library,
            clock,
            None,
            ResetBehavior::Hold,
            duration,
        )
    }

    /// 带 **predicate / reset** 的成品脚本入口（C 批）。
    #[allow(clippy::too_many_arguments)]
    pub fn bind_expr_gated(
        &mut self,
        object: Handle,
        target: &str,
        expr: Expr,
        library: LibraryRef,
        clock: ClockProgram,
        predicate: Option<Expr>,
        reset: ResetBehavior,
        duration: f64,
    ) -> Result<(), ExprBindError> {
        record_expression(
            &mut self.timeline,
            self.creates.len(),
            object,
            target,
            expr,
            library,
            clock,
            predicate,
            reset,
            duration,
        )
    }
}

impl SceneScript {
    /// 本脚本声明的对象数（句柄数）。
    pub fn len(&self) -> usize {
        self.creates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.creates.is_empty()
    }

    /// **静止时刻**（秒；`None` = 不可推导，见 [`Timeline::still_time`]）。
    ///
    /// 它**不是**"作品时长"：录多长由 `ExportSpec { fps, frames }` 决定。
    pub fn still_time(&self) -> Option<f64> {
        self.timeline.still_time()
    }

    /// **阶段 2**：区分"不可分析"与"含非时间依赖"的完整报告。
    pub fn still_time_report(&self) -> crate::animation::StillTimeReport {
        self.timeline.still_time_report()
    }

    /// 产物里出现的**函子库身份**（输入流配对校验用；见 [`Timeline::libraries`]）。
    pub fn libraries(&self) -> Vec<LibraryRef> {
        self.timeline.libraries()
    }

    /// **合并**：把 `other` 整体接到本脚本之后（拼接命令序列，§3.3）。
    ///
    /// 三条规则：
    /// 1. **句柄偏移**：`other` 的 `Handle(n)` → `Handle(base + n)`（`base = self.len()`），
    ///    `other` 内部**所有引用**（创建载荷里的占位引用 + 销毁 + 待解析动画目标）同步改写；
    /// 2. **时间游标**：`other` 的步骤直接续在本脚本的步骤之后（顺序即游标）；
    /// 3. **返回 [`HandleRange`]**：告诉调用方 `other` 原句柄的新落位。
    ///
    /// 不做并行合并（两段时间轴对齐）—— 需要额外语义，等真实需求（§11.1）。
    pub fn append(&mut self, mut other: SceneScript) -> HandleRange {
        let base = self.creates.len();
        let other_len = other.creates.len();

        // 1. 创建载荷：先偏移 other 的占位引用，再整体搬过来（句柄 = 数组下标 ⇒ 自然对齐）。
        for components in &mut other.creates {
            shift_component_refs(components, base);
        }
        self.creates.append(&mut other.creates);

        // 2. 销毁命令：句柄偏移。
        for h in &mut other.despawns {
            h.0 += base;
        }
        self.despawns.append(&mut other.despawns);

        // 3. 时序：后一段的时钟整体后移到本脚本静止时刻 + 待解析目标句柄偏移。
        //    5b 之后没有顺序游标，"接在后面"必须显式落到时钟上。
        let time_base = self.timeline.still_time().unwrap_or(0.0);
        self.timeline
            .append_shifted(&other.timeline, base, time_base);

        HandleRange {
            start: base,
            len: other_len,
        }
    }

    /// **提交到已有 Scene，不新建**：按声明顺序 `spawn_dyn` ⇒ 解析引用 ⇒ 编译时序。
    ///
    /// - `Create` 失败 ⇒ 中止并返回 [`ScriptError`]（前面已创建的对象**保留**，
    ///   与 `Scene` 自身的提交粒度一致 —— Scene 无事务）；
    /// - 成功 ⇒ 产出 [`SceneRecipe`]（时钟树 + 绑定表 + 句柄映射），由调用方插入调度器。
    pub fn commit<S: SceneContract + crate::scene_write::SceneRead>(
        mut self,
        scene: &mut S,
    ) -> Result<SceneRecipe, ScriptError> {
        let mut handles: Vec<ObjectId> = Vec::with_capacity(self.creates.len());

        for (i, mut components) in self.creates.into_iter().enumerate() {
            // 声明期占位引用在此解析（引用**先前**创建的对象；未解析的落 `DANGLING`）。
            resolve_component_refs(&mut components, |refid| {
                handles
                    .get(refid.index as usize)
                    .copied()
                    .unwrap_or(ObjectId::DANGLING)
            });
            match scene.spawn_dyn(components) {
                Some(id) => {
                    // **T0-① 不变量防御**：`handle_ref` 用 `generation == 0` 编码"声明期占位
                    // 引用"（见上），其前提是**真实对象的 `generation` 恒 ≥ 1**。该前提若被
                    // 破坏，`is_handle_ref` 会把真实对象误判成占位引用 ⇒ 解析时改写 ⇒ 指向
                    // 错误对象（不报错、不崩溃，只画面错乱）。这里把不变量钉在**解析路径**上。
                    debug_assert!(
                        id.generation >= 1,
                        "真实对象的 generation 必须 ≥ 1（0 被 handle_ref 保留作占位引用）：{id:?}"
                    );
                    handles.push(id)
                }
                None => return Err(ScriptError::CreateFailed { handle: Handle(i) }),
            }
        }

        for h in &self.despawns {
            if let Some(id) = handles.get(h.0) {
                scene.despawn(*id);
            }
        }

        let mut forest = ClockForest::new();
        let mut bindings = BindingTable::new();
        // **G 批**：把 Expr 树里 `Node::Read.object`（脚本局部句柄下标）重写为
        // `ObjectId` 打包值，再编译 ⇒ 运行期 `Mono::read_object` 解包读组件。
        // 产物序列化发生在 commit **之前**（persist 走未编译的 `Timeline`），
        // payload 里 `Read.object` 仍是脚本句柄下标。
        self.timeline.rewrite_read_objects(&|object| {
            handles
                .get(object as usize)
                .copied()
                .unwrap_or(ObjectId::DANGLING)
                .to_packed()
        });
        self.timeline.compile_with(
            &|idx| handles.get(idx).copied().unwrap_or(ObjectId::DANGLING),
            &mut forest,
            &mut bindings,
        );

        // **G 批帧内定序**：完整表 + 时钟森林都在手 ⇒ 装配期检查"读先于写"，
        // 冲突显式报错（不留给运行期读到旧值 / 未定义值）。
        check_read_order(&bindings, &forest).map_err(ScriptError::OrderConflict)?;

        // `reset = Reset` 的**初值**：此刻场景刚建好、绑定还没写过任何字段
        // ⇒ 直接读就是"绑定生效前的值"（之后每帧不再需要历史）。
        for binding in &mut bindings.bindings {
            if binding.reset == ResetBehavior::Reset && binding.initial.is_none() {
                binding.initial = binding.target.read(scene);
            }
        }

        Ok(SceneRecipe {
            forest,
            bindings,
            handles,
        })
    }
}

impl From<SceneEncoder> for SceneScript {
    fn from(enc: SceneEncoder) -> Self {
        enc.finish()
    }
}

/// **G 批帧内定序**（装配期，显式报错）：对每条表达式绑定收集其读集
/// `(规范地址, ObjectId)`（从 `Step::Read`）；对每个读，找同表内所有写
/// **同一地址 + 同一对象**的绑定（曲线绑定也是写者）；若某写者的时钟起点
/// **晚于**读者 ⇒ 同一帧内读者会读到旧值 ⇒ [`ExprBindError::ReadOrderConflict`]。
/// 正确顺序（写者 start ≤ 读者 start）静默通过。
fn check_read_order(bindings: &BindingTable, forest: &ClockForest) -> Result<(), ExprBindError> {
    use crate::animation::binding::BindingDriver;
    use ze_comb::{ChannelRef, CompiledExpr, Step};

    /// 收集程序里的读集（含 `Select` 分支内的读 —— 分支子程序是独立值表，
    /// 必须递归，否则漏检）。
    fn collect_reads(program: &CompiledExpr, out: &mut Vec<(ChannelRef, u64)>) {
        for step in program.steps() {
            match step {
                Step::Read { channel, object } => out.push((channel.clone(), *object)),
                // **B2**：方法的读集来自 `MethodSpec.reads`（方法体是黑盒，
                // 定序器看不见它读了什么 ⇒ 必须静态声明）。
                Step::Method { method, object, .. } => {
                    if let Some(spec) = crate::animation::target::method_entry(&method.name) {
                        for address in spec.reads {
                            out.push((ChannelRef::new("zela.scene", *address), *object));
                        }
                    }
                }
                Step::Select { then, r#else, .. } => {
                    collect_reads(then, out);
                    collect_reads(r#else, out);
                }
                // 阶段 3：`NatRec` 的 init / step 子程序各自带值表，必须递归。
                Step::NatRec { init, step, .. } => {
                    collect_reads(init, out);
                    collect_reads(step, out);
                }
                _ => {}
            }
        }
    }

    for binding in &bindings.bindings {
        let BindingDriver::Expr(program) = &binding.driver else {
            continue; // 曲线绑定没有读集
        };
        let reader_start = forest.start_offset(binding.clock);
        let mut reads = Vec::new();
        collect_reads(program.lowered(), &mut reads);
        for (channel, object) in reads {
            for writer in &bindings.bindings {
                if writer.target.address() == channel.name.as_str()
                    && writer.target.target_id() == ObjectId::from_packed(object)
                    && forest.start_offset(writer.clock) > reader_start
                {
                    return Err(ExprBindError::ReadOrderConflict {
                        reader: binding.target.name().to_string(),
                        writer: writer.target.name().to_string(),
                        address: channel.name.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// **阶段 0**：帧内定序检查也是装配期分析 pass。
pub fn register_passes(registry: &mut ze_comb::PassRegistry) {
    registry
        .register(ze_comb::PassInfo {
            name: "scene.read_order",
            kind: ze_comb::PassKind::Analysis,
            semantics: ze_comb::Semantics::Preserving,
            owner: "ze-scene-core",
        })
        .expect("read order pass 名唯一");
}
