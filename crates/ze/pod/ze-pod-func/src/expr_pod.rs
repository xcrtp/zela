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

use ze_comb::{
    ChannelRef, ChannelType, ClockLinkShape, ClockShape, CompiledExpr, Expr, Literal, MethodRef,
    OpRef,
};
use ze_pod_core::{FunctionId, Handle, HandleTable, Pod, PodError, PodId, QualifiedName, RegistryPod};

use crate::api::{
    AddSpec, AndSpec, BindSpec, ChannelSpec, ChannelTypeSpec, ClampSpec, ClockLinkSpec, ClockSpec,
    ComponentSpec, CosSpec, EqSpec, GeSpec, GtSpec, LeSpec, LengthSpec, LtSpec, MixSpec, MulSpec,
    BindEachSpec, ExistsSpec, MethodCallSpec, NatRecSpec, NeSpec, NormalizeSpec, NotSpec, OrSpec,
    PredicateSpec, ReadSpec, ResetSpec, Rotate3Spec, RoundF32Spec, ScaleSpec, SelectSpec,
    ShaderSpec, SinSpec, SplatSpec, SumSpec, Vec2FromVec3Spec,
    Vec2OfSpec, Vec3FromVec2Spec, Vec3OfSpec,
};
use crate::infer::{infer_predicate, infer_root};

/// 函子库 namespace（进产物；与基础函子库同一份常量）。
pub const LIBRARY_NAMESPACE: &str = ze_func::LIBRARY_NAMESPACE;
/// 函子库版本（进产物；加算子递增它，`zela.scene` 不受影响）。
///
/// `7`：**G 批**新增 `read`（读某对象的组件字段；`zela.math::read`）。
/// `8`：**H 批**新增 `select`（值分支；`zela.math::select`）。
/// `9`：**算子补齐批**新增 `mix` / `clamp` / `vec2` / `vec3` / `x` / `y` / `z` / `rotate3`。
/// `10`：**第 2 批**通道常量带结构化类型（`zela.math.Channel` 增加 `ty`），`Read` 去掉 `ty` 字符串。
/// `11`：**第 3 批**转换算子族（`splat` / `vec3_from_vec2` / `vec2_from_vec3` /
/// `length` / `normalize`）。
/// `12`：**第 5a 批**新增 `round_f32`（旧曲线的 f32 精度落位）。
/// `13`：**5b B1** 删除 `time`（`clock.time()` 为唯一入口），新增时钟身份
/// （`clock_time_of`）与窗口函子（`clock_range` / `clock_range_of`）。
/// `14`：**5b B5** 新增离散层入口 `nat_rec` / `rec_prev` / `rec_index`。
/// `15`：**5b B2** 新增 `method`（对象方法；`Reveal.current_pos` 改走方法）。
/// `16`：**5b B4** 新增 `query_self` / `bind_each`（装配期展开的批量绑定）。
/// `17`：**5b B4** 新增 `exists`（集合判空；装配期常量折叠）。
/// `18`：**阶段 2** 新增 `shader`（表达式 → WGSL，装饰性着色 codegen）。
/// `19`：**阶段 2** 新增 `center` / `size`（对象级内建通道）。
/// `20`：**阶段 3** 新增 `pos2` / `local` / `uv`（逐片元内建通道）。
pub const LIBRARY_VERSION: &str = "20";

/// 组件字段通道的命名空间（`Circle.radius` 属于场景地址表）。
pub const SCENE_NAMESPACE: &str = "zela.scene";

const HANDLE: &str = "ze_pod_core.Handle";

/// **`it` 的占位对象**：批量绑定里的"当前对象"，由宿主在装配期替换成具体句柄。
pub const QUERY_SELF_OBJECT: u64 = u64::MAX;

/// 一条待**装配期展开**的批量绑定（B4 Query）。
#[derive(Clone, Debug)]
pub struct PendingBindEach {
    pub target: ChannelSpec,
    pub expr: Expr,
    pub clock: ClockSpec,
    pub predicate: Option<Expr>,
    pub reset: ResetSpec,
    pub requires: Vec<String>,
    pub excludes: Vec<String>,
    /// 装配期推导出的根类型（宿主核对目标通道用；不进 payload）。
    pub root_type: ChannelType,
}

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
    /// 装配期推导出的根表达式类型（不进 payload；宿主核对目标通道用）。
    pub root_type: ChannelType,
}

/// 一个待注入参数（pod 层只认槽类型；宿主转成渲染 IR 的种类）。
#[derive(Clone, Debug, PartialEq)]
pub struct PendingShaderParam {
    /// 通道的限定名（`time` …）；宿主按它找供给者。
    pub channel: String,
    pub kind: ze_comb::SlotType,
    /// 相对 `Params` 起点的字节偏移（std140）。
    pub offset: u32,
}

/// 一条待并入场景的**着色 codegen**（阶段 2）：对象 + 生成的 WGSL。
#[derive(Clone, Debug)]
pub struct PendingShader {
    /// 场景对象**句柄**（pod 侧；装配期转成本脚本内的局部句柄）。
    pub object: Handle,
    /// 生成的 WGSL 纯函数（`eval` 返回 `vec3<f32>`）。
    pub source: String,
    /// **外部注入的参数表**（由生成器的参数表按 uniform 布局推导）。
    pub params: Vec<PendingShaderParam>,
    /// `Params` 的 uniform 字节数（16 的倍数）。
    pub size: u32,
    /// **逐片元**：表达式读了 `zela.shader.fragment.*` ⇒ 颜色随像素变。
    pub fragment: bool,
}

/// 表达式 Pod。
pub struct ExprPod {
    id: PodId,
    registry: RegistryPod,
    store: Mutex<HandleTable<Expr>>,
    /// 脚本登记的表达式动画（宿主在脚本结束后取走）。
    bindings: Mutex<Vec<PendingBinding>>,
    /// 脚本登记的**着色 codegen**（宿主在脚本结束后挂成组件）。
    shaders: Mutex<Vec<PendingShader>>,
    /// 脚本登记的**批量绑定**（宿主在装配期展开成 N 条具体绑定）。
    bind_each: Mutex<Vec<PendingBindEach>>,
    /// 装配期错误（未知通道 / 未知算子 / 非法句柄）—— 宿主取走时显式失败。
    errors: Mutex<Vec<String>>,
}

impl ExprPod {
    /// 新建（装配期调用一次）。
    pub fn new() -> Self {
        let id = PodId::next();
        let mut registry = RegistryPod::new();

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
        // 第 1 批（算子补齐）：插值 / 钳制 / 向量构造 / 分量 / 旋转。
        registry.register_fn1_named::<MixSpec, Handle>("zela.math::mix", "zela.math.Mix", HANDLE, mix_ctor);
        registry.register_fn1_named::<ClampSpec, Handle>(
            "zela.math::clamp",
            "zela.math.Clamp",
            HANDLE,
            clamp_ctor,
        );
        registry.register_fn1_named::<Vec2OfSpec, Handle>(
            "zela.math::vec2",
            "zela.math.Vec2Of",
            HANDLE,
            vec2_ctor,
        );
        registry.register_fn1_named::<Vec3OfSpec, Handle>(
            "zela.math::vec3",
            "zela.math.Vec3Of",
            HANDLE,
            vec3_ctor,
        );
        registry.register_fn1_named::<ComponentSpec, Handle>(
            "zela.math::x",
            "zela.math.Component",
            HANDLE,
            component_x_ctor,
        );
        registry.register_fn1_named::<ComponentSpec, Handle>(
            "zela.math::y",
            "zela.math.Component",
            HANDLE,
            component_y_ctor,
        );
        registry.register_fn1_named::<ComponentSpec, Handle>(
            "zela.math::z",
            "zela.math.Component",
            HANDLE,
            component_z_ctor,
        );
        registry.register_fn1_named::<Rotate3Spec, Handle>(
            "zela.math::rotate3",
            "zela.math.Rotate3",
            HANDLE,
            rotate3_ctor,
        );
        // 第 3 批（类型系统扩展）：转换算子族（显式升降维 + 长度 / 单位化）。
        registry.register_fn1_named::<SplatSpec, Handle>(
            "zela.math::splat",
            "zela.math.Splat",
            HANDLE,
            splat_ctor,
        );
        registry.register_fn1_named::<Vec3FromVec2Spec, Handle>(
            "zela.math::vec3_from_vec2",
            "zela.math.Vec3FromVec2",
            HANDLE,
            vec3_from_vec2_ctor,
        );
        registry.register_fn1_named::<Vec2FromVec3Spec, Handle>(
            "zela.math::vec2_from_vec3",
            "zela.math.Vec2FromVec3",
            HANDLE,
            vec2_from_vec3_ctor,
        );
        registry.register_fn1_named::<LengthSpec, Handle>(
            "zela.math::length",
            "zela.math.Length",
            HANDLE,
            length_ctor,
        );
        registry.register_fn1_named::<NormalizeSpec, Handle>(
            "zela.math::normalize",
            "zela.math.Normalize",
            HANDLE,
            normalize_ctor,
        );
        registry.register_fn1_named::<RoundF32Spec, Handle>(
            "zela.math::round_f32",
            "zela.math.RoundF32",
            HANDLE,
            round_f32_ctor,
        );
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
        // 离散层（B5）：合法递归的唯一入口。
        registry.register_fn1_named::<NatRecSpec, Handle>(
            "zela.math::nat_rec",
            "zela.math.NatRec",
            HANDLE,
            nat_rec_ctor,
        );
        // 方法（B2）：对象 + 方法名 → 方法调用节点。
        registry.register_fn1_named::<MethodCallSpec, Handle>(
            "zela.math::method",
            "zela.math.Method",
            HANDLE,
            method_ctor,
        );
        registry.register_fn0_named("zela.math::rec_prev", HANDLE, rec_prev);
        registry.register_fn0_named("zela.math::rec_index", HANDLE, rec_index);
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
        // B1：时钟身份与窗口函子（`clock.time_of(c)` / `clock.range(lo, hi)`）。
        registry.register_fn1_named::<ClockSpec, Handle>(
            "zela.math::clock_time_of",
            "zela.math.Clock",
            HANDLE,
            clock_time_of,
        );
        registry.register_fn2_named::<f64, f64, Handle>(
            "zela.math::clock_range",
            "f64",
            "f64",
            HANDLE,
            clock_range,
        );
        registry.register_fn3_named::<ClockSpec, f64, f64, Handle>(
            "zela.math::clock_range_of",
            "zela.math.Clock",
            "f64",
            "f64",
            HANDLE,
            clock_range_of,
        );
        registry.register_fn1_named::<ReadSpec, Handle>(
            "zela.math::read",
            "zela.math.Read",
            HANDLE,
            read_ctor,
        );
        registry.register_fn1_named::<SelectSpec, Handle>(
            "zela.math::select",
            "zela.math.Select",
            HANDLE,
            select_ctor,
        );
        registry.register_fn1_named::<BindSpec, ()>(
            "zela.math::bind",
            "zela.math.Bind",
            "()",
            bind,
        );
        // **阶段 2**：把表达式编译成 WGSL，作为对象的"着色 codegen"（装饰性）。
        registry.register_fn1_named::<ShaderSpec, ()>(
            "zela.math::shader",
            "zela.math.Shader",
            "()",
            shader,
        );
        // **阶段 2**：着色内建通道（对象 / 片元属性；不进 uniform）。
        registry.register_fn0_named("zela.math::center", HANDLE, center_ctor);
        registry.register_fn0_named("zela.math::size", HANDLE, size_ctor);
        // **阶段 3**：逐片元内建（像素位置 / 框内坐标 / 归一化坐标）。
        registry.register_fn0_named("zela.math::pos2", HANDLE, pos2_ctor);
        registry.register_fn0_named("zela.math::local", HANDLE, local_ctor);
        registry.register_fn0_named("zela.math::uv", HANDLE, uv_ctor);
        // B4 Query：装配期展开的批量绑定 + `it` 占位读。
        registry.register_fn1_named::<ChannelSpec, Handle>(
            "zela.math::query_self",
            "zela.math.Channel",
            HANDLE,
            query_self_ctor,
        );
        registry.register_fn1_named::<BindEachSpec, ()>(
            "zela.math::bind_each",
            "zela.math.BindEach",
            "()",
            bind_each,
        );
        registry.register_fn1_named::<ExistsSpec, Handle>(
            "zela.math::exists",
            "zela.math.Exists",
            HANDLE,
            exists_ctor,
        );

        Self {
            id,
            registry,
            store: Mutex::new(HandleTable::new(id)),
            bindings: Mutex::new(Vec::new()),
            shaders: Mutex::new(Vec::new()),
            bind_each: Mutex::new(Vec::new()),
            errors: Mutex::new(Vec::new()),
        }
    }

    /// 取走脚本登记的着色 codegen（宿主在 `run` 之后挂成组件）。
    pub fn take_shaders(&self) -> Vec<PendingShader> {
        std::mem::take(&mut *self.shaders.lock().expect("着色 codegen 表未被毒化"))
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

    /// 取走本轮登记的**批量绑定**（装配期展开用）。
    pub fn take_bind_each(&self) -> Vec<PendingBindEach> {
        std::mem::take(
            &mut *self
                .bind_each
                .lock()
                .expect("批量绑定表只在持有期间被 panic 毒化"),
        )
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
            "schema_version": ze_comb::SCHEMA_VERSION,
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
            "mix" => Box::new(mix_ctor(self, arg::<MixSpec>(args, 0)?.clone())),
            "clamp" => Box::new(clamp_ctor(self, arg::<ClampSpec>(args, 0)?.clone())),
            "vec2" => Box::new(vec2_ctor(self, arg::<Vec2OfSpec>(args, 0)?.clone())),
            "vec3" => Box::new(vec3_ctor(self, arg::<Vec3OfSpec>(args, 0)?.clone())),
            "x" => Box::new(component_x_ctor(self, arg::<ComponentSpec>(args, 0)?.clone())),
            "y" => Box::new(component_y_ctor(self, arg::<ComponentSpec>(args, 0)?.clone())),
            "z" => Box::new(component_z_ctor(self, arg::<ComponentSpec>(args, 0)?.clone())),
            "rotate3" => Box::new(rotate3_ctor(self, arg::<Rotate3Spec>(args, 0)?.clone())),
            "splat" => Box::new(splat_ctor(self, arg::<SplatSpec>(args, 0)?.clone())),
            "vec3_from_vec2" => Box::new(vec3_from_vec2_ctor(
                self,
                arg::<Vec3FromVec2Spec>(args, 0)?.clone(),
            )),
            "vec2_from_vec3" => Box::new(vec2_from_vec3_ctor(
                self,
                arg::<Vec2FromVec3Spec>(args, 0)?.clone(),
            )),
            "length" => Box::new(length_ctor(self, arg::<LengthSpec>(args, 0)?.clone())),
            "normalize" => Box::new(normalize_ctor(self, arg::<NormalizeSpec>(args, 0)?.clone())),
            "round_f32" => Box::new(round_f32_ctor(self, arg::<RoundF32Spec>(args, 0)?.clone())),
            "lt" => Box::new(lt_ctor(self, arg::<LtSpec>(args, 0)?.clone())),
            "gt" => Box::new(gt_ctor(self, arg::<GtSpec>(args, 0)?.clone())),
            "le" => Box::new(le_ctor(self, arg::<LeSpec>(args, 0)?.clone())),
            "ge" => Box::new(ge_ctor(self, arg::<GeSpec>(args, 0)?.clone())),
            "eq" => Box::new(eq_ctor(self, arg::<EqSpec>(args, 0)?.clone())),
            "ne" => Box::new(ne_ctor(self, arg::<NeSpec>(args, 0)?.clone())),
            "and" => Box::new(and_ctor(self, arg::<AndSpec>(args, 0)?.clone())),
            "or" => Box::new(or_ctor(self, arg::<OrSpec>(args, 0)?.clone())),
            "not" => Box::new(not_ctor(self, arg::<NotSpec>(args, 0)?.clone())),
            "method" => Box::new(method_ctor(self, arg::<MethodCallSpec>(args, 0)?.clone())),
            "nat_rec" => Box::new(nat_rec_ctor(self, arg::<NatRecSpec>(args, 0)?.clone())),
            "rec_prev" => Box::new(rec_prev(self)),
            "rec_index" => Box::new(rec_index(self)),
            "clock_shift" => Box::new(clock_shift(self, *arg::<f64>(args, 0)?)),
            "clock_scale" => Box::new(clock_scale(self, *arg::<f64>(args, 0)?)),
            "clock_nest" => Box::new(clock_nest(
                self,
                arg::<ClockSpec>(args, 0)?.clone(),
                arg::<ClockSpec>(args, 1)?.clone(),
            )),
            "clock_time" => Box::new(clock_time(self)),
            "clock_time_of" => Box::new(clock_time_of(self, arg::<ClockSpec>(args, 0)?.clone())),
            "clock_range" => Box::new(clock_range(
                self,
                *arg::<f64>(args, 0)?,
                *arg::<f64>(args, 1)?,
            )),
            "clock_range_of" => Box::new(clock_range_of(
                self,
                arg::<ClockSpec>(args, 0)?.clone(),
                *arg::<f64>(args, 1)?,
                *arg::<f64>(args, 2)?,
            )),
            "read" => Box::new(read_ctor(self, arg::<ReadSpec>(args, 0)?.clone())),
            "select" => Box::new(select_ctor(self, arg::<SelectSpec>(args, 0)?.clone())),
            "expression" => Box::new(expression(self, *arg::<Handle>(args, 0)?)),
            "bind" => Box::new(bind(self, arg::<BindSpec>(args, 0)?.clone())),
            "query_self" => Box::new(query_self_ctor(
                self,
                arg::<ChannelSpec>(args, 0)?.clone(),
            )),
            "bind_each" => Box::new(bind_each(self, arg::<BindEachSpec>(args, 0)?.clone())),
            "exists" => Box::new(exists_ctor(self, arg::<ExistsSpec>(args, 0)?.clone())),
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

fn call3(pod: &dyn Pod, name: &str, a: Handle, b: Handle, c: Handle) -> Handle {
    let this = expr_pod(pod);
    let first = child(this, a);
    let second = child(this, b);
    let third = child(this, c);
    this.intern(Expr::call(op(name), [first, second, third]))
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

// ─── 第 1 批（算子补齐）：插值 / 钳制 / 向量 / 分量 / 旋转 ─────────────────

fn mix_ctor(pod: &dyn Pod, spec: MixSpec) -> Handle {
    call3(pod, "mix", spec.a, spec.b, spec.t)
}

fn clamp_ctor(pod: &dyn Pod, spec: ClampSpec) -> Handle {
    call3(pod, "clamp", spec.x, spec.lo, spec.hi)
}

fn vec2_ctor(pod: &dyn Pod, spec: Vec2OfSpec) -> Handle {
    call2(pod, "vec2", spec.x, spec.y)
}

fn vec3_ctor(pod: &dyn Pod, spec: Vec3OfSpec) -> Handle {
    call3(pod, "vec3", spec.x, spec.y, spec.z)
}

fn component_x_ctor(pod: &dyn Pod, spec: ComponentSpec) -> Handle {
    call1(pod, "x", spec.v)
}

fn component_y_ctor(pod: &dyn Pod, spec: ComponentSpec) -> Handle {
    call1(pod, "y", spec.v)
}

fn component_z_ctor(pod: &dyn Pod, spec: ComponentSpec) -> Handle {
    call1(pod, "z", spec.v)
}

fn rotate3_ctor(pod: &dyn Pod, spec: Rotate3Spec) -> Handle {
    call3(pod, "rotate3", spec.v, spec.axis, spec.angle)
}

// ─── 第 3 批（类型系统扩展）：转换算子族 ──────────────────────────────

fn splat_ctor(pod: &dyn Pod, spec: SplatSpec) -> Handle {
    call1(pod, "splat", spec.v)
}

fn vec3_from_vec2_ctor(pod: &dyn Pod, spec: Vec3FromVec2Spec) -> Handle {
    call2(pod, "vec3_from_vec2", spec.v, spec.z)
}

fn vec2_from_vec3_ctor(pod: &dyn Pod, spec: Vec2FromVec3Spec) -> Handle {
    call1(pod, "vec2_from_vec3", spec.v)
}

fn length_ctor(pod: &dyn Pod, spec: LengthSpec) -> Handle {
    call1(pod, "length", spec.v)
}

fn normalize_ctor(pod: &dyn Pod, spec: NormalizeSpec) -> Handle {
    call1(pod, "normalize", spec.v)
}

fn round_f32_ctor(pod: &dyn Pod, spec: RoundF32Spec) -> Handle {
    call1(pod, "round_f32", spec.of)
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

// ─── 方法（B2）─────────────────────────────────────────────────────────

/// `Method { name, object, args }` → `Node::Method`。
///
/// 方法名与参数个数在**装配期**由宿主核对（`ExprBindError::UnknownMethod`）；
/// Pod 侧只负责把它们编进树（类型由 `infer` 的方法签名目录给）。
fn method_ctor(pod: &dyn Pod, spec: MethodCallSpec) -> Handle {
    let this = expr_pod(pod);
    let args: Vec<Expr> = spec.args.iter().map(|handle| child(this, *handle)).collect();
    this.intern(Expr::method(
        MethodRef::new("zela.scene", spec.name),
        spec.object.slot() as u64,
        args,
    ))
}

// ─── 离散层（B5）：NatRec ───────────────────────────────────────────────

/// `NatRec { count, init, step }`。
///
/// `count` 必须是非负整数：非整数 / 负数 ⇒ 装配期显式报错（并返回 `init` 句柄
/// 占位；装配在错误表非空时整体失败，不会产出半个绑定）。
fn nat_rec_ctor(pod: &dyn Pod, spec: NatRecSpec) -> Handle {
    let this = expr_pod(pod);
    if !spec.count.is_finite() || spec.count < 0.0 || spec.count.fract() != 0.0 {
        push_binding_error(
            this,
            format!(
                "`NatRec.count` 必须是非负整数，实际 {}（非整数请显式写 Floor）",
                spec.count
            ),
        );
        return spec.init;
    }
    let count = Expr::pure(Literal::I64(spec.count as i64));
    let init = child(this, spec.init);
    let step = child(this, spec.step);
    this.intern(Expr::nat_rec(count, init, step))
}

fn rec_prev(pod: &dyn Pod) -> Handle {
    expr_pod(pod).intern(Expr::rec_prev())
}

fn rec_index(pod: &dyn Pod) -> Handle {
    expr_pod(pod).intern(Expr::rec_index())
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

/// **绑定时钟**的局部时间叶子（`clock.time()`；叶子不带时钟身份）。
fn clock_time(pod: &dyn Pod) -> Handle {
    leaf_channel(pod, ChannelRef::builtin("time"))
}

/// `ClockSpec` → 产物里的时钟形状（只搬两列浮点）。
fn clock_shape(spec: &ClockSpec) -> ClockShape {
    ClockShape {
        links: spec
            .links
            .iter()
            .map(|link| ClockLinkShape {
                shift: link.shift,
                rate: link.rate,
            })
            .collect(),
    }
}

/// `clock.time_of(c)`：叶子带 `c` 的身份（运行期按 `c` 反算同一帧的局部时间）。
fn clock_time_of(pod: &dyn Pod, clock: ClockSpec) -> Handle {
    leaf_channel(
        pod,
        ChannelRef::builtin("time").with_clock(clock_shape(&clock)),
    )
}

/// 窗口 `[lo, hi)`：`And(Ge(t, lo), Lt(t, hi))`（`clock` 为 `None` ⇒ 绑定时钟）。
fn window(pod: &dyn Pod, clock: Option<&ClockSpec>, lo: f64, hi: f64) -> Handle {
    let leaf = || match clock {
        Some(clock) => {
            Expr::channel(ChannelRef::builtin("time").with_clock(clock_shape(clock)))
        }
        None => Expr::channel(ChannelRef::builtin("time")),
    };
    let ge = Expr::call(op("ge"), [leaf(), Expr::pure(Literal::F64(lo))]);
    let lt = Expr::call(op("lt"), [leaf(), Expr::pure(Literal::F64(hi))]);
    expr_pod(pod).intern(Expr::call(op("and"), [ge, lt]))
}

/// `clock.range(lo, hi)`：绑定时钟的窗口（四个脚本 `in_segment` helper 的库内形态）。
fn clock_range(pod: &dyn Pod, lo: f64, hi: f64) -> Handle {
    window(pod, None, lo, hi)
}

/// `clock.range_of(c, lo, hi)`：指定时钟的窗口。
fn clock_range_of(pod: &dyn Pod, clock: ClockSpec, lo: f64, hi: f64) -> Handle {
    window(pod, Some(&clock), lo, hi)
}

/// **G 批通道读端**：`Read { channel, object }` → `Expr::read`（叶子）。
///
/// **第 2 批**：类型从常量自带的 [`ChannelTypeSpec`] 取（不再有 `ty` 字符串）——
/// 一份类型只写一遍。`Unknown` 原样带进 `ChannelRef.ty` ⇒ 装配期 `infer`
/// **显式报错**（不再静默退化成 `Number`）；回放路径不推导，`Unknown` 是合法状态。
fn read_ctor(pod: &dyn Pod, spec: ReadSpec) -> Handle {
    let this = expr_pod(pod);
    let channel = ChannelRef::new(spec.channel.namespace, spec.channel.name)
        .with_type(channel_type_of(spec.channel.ty));
    this.intern(Expr::read(channel, spec.object.slot() as u64))
}

/// **H 批**：`Select { cond, then, otherwise }` → `Expr::select`（值分支）。
///
/// 脚本侧第三个字段叫 `otherwise`（`else` 是三种宿主语言的关键字）；产物节点
/// 仍是 `cond / then / else` —— 转换只在这一行发生。
fn select_ctor(pod: &dyn Pod, spec: SelectSpec) -> Handle {
    let this = expr_pod(pod);
    this.intern(Expr::select(
        child(this, spec.cond),
        child(this, spec.then),
        child(this, spec.otherwise),
    ))
}

/// 结构化通道类型 → 表达式层类型（一一对应，**没有默认分支**）。
fn channel_type_of(ty: ChannelTypeSpec) -> ChannelType {
    match ty {
        ChannelTypeSpec::Unknown => ChannelType::Unknown,
        ChannelTypeSpec::Number => ChannelType::Number,
        ChannelTypeSpec::Bool => ChannelType::Bool,
        ChannelTypeSpec::Vec2 => ChannelType::Vec2,
        ChannelTypeSpec::Vec3 => ChannelType::Vec3,
    }
}

fn expression(pod: &dyn Pod, root: Handle) -> String {
    expr_pod(pod)
        .export(root)
        .expect("表达式句柄必须由本 Pod 产出且未释放")
}

/// 装配期校验：结构合法 + 只读已注入的通道 + 算子都在本库。
fn validate_binding(expr: &Expr) -> Result<(), String> {
    CompiledExpr::compile(expr).map_err(|error| error.to_string())?;

    /// 命中即用 `String` 提前结束遍历（`walk` 的错误通道）。
    struct Check;

    impl<'a> ze_comb::ExprVisitor<'a> for Check {
        type Error = String;

        fn enter(
            &mut self,
            expr: &'a Expr,
            _parent: Option<&'a ze_comb::Node>,
        ) -> Result<bool, String> {
            if let Some(channel) = expr.node.channel() {
                // `zela.shader` 是**后端**内建（对象 / 片元属性），与 `time` / `frame`
                // 一样不需要宿主登记供给者。
                let injected = (channel.is_builtin()
                    && matches!(channel.name.as_str(), "time" | "frame"))
                    || channel.namespace == "zela.shader";
                if !injected {
                    return Err(format!("通道 `{channel}` 尚未由宿主注入"));
                }
            }
            if let Some((op, _)) = expr.node.call() {
                if !crate::ops::is_known(op) {
                    return Err(format!("未知算子 `{op}`"));
                }
            }
            Ok(true)
        }
    }

    expr.walk(&mut Check)
}

/// **阶段 2**：`shader { object, root }` —— 把表达式编译成 WGSL（装饰性着色）。
///
/// 通道分三类：`time` / `frame`（每帧注入）、`zela.shader::*`（后端内建：对象 / 片元属性）、
/// 其余（宿主登记的外部注入）。同时校验 `eval` 返回 `vec3<f32>`（标量请用 `splat`）。
fn shader(pod: &dyn Pod, spec: ShaderSpec) {
    let this = expr_pod(pod);
    let expr = match this.tree(spec.root) {
        Ok(expr) => expr,
        Err(error) => {
            push_binding_error(this, format!("着色 codegen 的表达式句柄无效：{error}"));
            return;
        }
    };
    if let Err(message) = validate_binding(&expr) {
        push_binding_error(this, format!("着色 codegen 的表达式非法：{message}"));
        return;
    }
    let emitted = match ze_comb::emit_wgsl(&expr, ze_func::wgsl::backend()) {
        Ok(emitted) => emitted,
        Err(error) => {
            push_binding_error(this, format!("着色 codegen 不能生成 WGSL：{error}"));
            return;
        }
    };
    if emitted.ret != ze_comb::SlotType::Vec3 {
        push_binding_error(
            this,
            format!(
                "着色 codegen 的根表达式必须返回 Vec3（当前 {:?}；标量请用 `splat`）",
                emitted.ret
            ),
        );
        return;
    }
    // **外部注入**：参数表由生成器的参数表按 WGSL uniform 布局（std140）推导 ——
    // 与通道语义无关（标量 / 向量 / 矩阵同一套规则）。通道限定名从表达式里收集，
    // 按与后端**同一 mangle 规则**反查回 `WgslParam.name`。
    let mut channels: Vec<(String, ze_comb::SlotType)> = Vec::new();
    let mut collect = CollectChannels(&mut channels);
    let _ = expr.walk(&mut collect);
    let mangle = |qualified: &str| {
        let mut name = String::from("c_");
        for byte in qualified.bytes() {
            name.push(if byte.is_ascii_alphanumeric() { byte as char } else { '_' });
        }
        name
    };
    let mut params: Vec<PendingShaderParam> = Vec::with_capacity(emitted.params.len());
    let mut offset = 0u32;
    for param in &emitted.params {
        let Some((channel, _)) = channels
            .iter()
            .find(|(qualified, _)| mangle(qualified) == param.name)
        else {
            push_binding_error(
                this,
                format!("着色 codegen 无法把参数 `{}` 映射回通道", param.name),
            );
            return;
        };
        let (kind, align, size) = match param.ty {
            ze_comb::SlotType::F64 => (ze_comb::SlotType::F64, 4u32, 4u32),
            ze_comb::SlotType::I64 => (ze_comb::SlotType::I64, 4, 4),
            ze_comb::SlotType::Vec2 => (ze_comb::SlotType::Vec2, 8, 8),
            ze_comb::SlotType::Vec3 => (ze_comb::SlotType::Vec3, 16, 12),
            ze_comb::SlotType::Bool => {
                push_binding_error(this, "着色 codegen 不支持 Bool 参数（uniform 里没有 bool）".to_string());
                return;
            }
        };
        offset = offset.div_ceil(align) * align;
        params.push(PendingShaderParam { channel: channel.clone(), kind, offset });
        offset += size;
    }
    let size = offset.div_ceil(16) * 16;
    // **阶段 3**：读了逐片元内建 ⇒ 颜色随像素变，后端改走逐像素通道。
    let fragment = channels
        .iter()
        .any(|(qualified, _)| qualified.starts_with("zela.shader::fragment."));
    this.shaders
        .lock()
        .expect("着色 codegen 表未被毒化")
        .push(PendingShader {
            object: spec.object,
            source: emitted.source,
            params,
            size,
            fragment,
        });
}

/// 收集表达式里出现过的通道（限定名 + 槽类型）。
struct CollectChannels<'a>(&'a mut Vec<(String, ze_comb::SlotType)>);

impl<'a> ze_comb::ExprVisitor<'a> for CollectChannels<'_> {
    type Error = std::convert::Infallible;

    fn enter(
        &mut self,
        expr: &'a ze_comb::Expr,
        _parent: Option<&'a ze_comb::Node>,
    ) -> Result<bool, Self::Error> {
        if let Some(channel) = expr.node.channel() {
            if let Some(ty) = ze_comb::slot_of_channel(channel.ty) {
                self.0.push((channel.qualified(), ty));
            }
        }
        Ok(true)
    }
}

/// **阶段 2**：`center()` —— 对象在视图里的中心（像素，屏幕坐标）。
fn center_ctor(pod: &dyn Pod) -> Handle {
    shader_builtin(pod, "center")
}

/// **阶段 2**：`size()` —— 对象包围盒的尺寸（像素）。
fn size_ctor(pod: &dyn Pod) -> Handle {
    shader_builtin(pod, "size")
}

/// **阶段 3**：`pos2()` —— 片元在视图里的位置（像素，y 向下）。
fn pos2_ctor(pod: &dyn Pod) -> Handle {
    shader_builtin(pod, "fragment.pos2")
}

/// **阶段 3**：`local()` —— 片元在对象包围盒里的位置（像素）。
fn local_ctor(pod: &dyn Pod) -> Handle {
    shader_builtin(pod, "fragment.local")
}

/// **阶段 3**：`uv()` —— `local / size`（0..1）。
fn uv_ctor(pod: &dyn Pod) -> Handle {
    shader_builtin(pod, "fragment.uv")
}

fn shader_builtin(pod: &dyn Pod, name: &str) -> Handle {
    let channel = ChannelRef::new("zela.shader", name).with_type(ChannelType::Vec2);
    expr_pod(pod).intern(Expr::channel(channel))
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
    // 装配期类型推导（L2）：类型错误在这里显式失败，不留到运行期。
    let root_type = match infer_root(&expr) {
        Ok(ty) => ty,
        Err(error) => {
            push_binding_error(
                this,
                format!("表达式绑定 `{}` 类型检查失败：{error}", spec.object),
            );
            return;
        }
    };
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
                if let Err(error) = infer_predicate(&tree) {
                    push_binding_error(
                        this,
                        format!("predicate 绑定 `{}` 类型检查失败：{error}", spec.object),
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
    // **B1 硬约束**：predicate 只允许单时钟（`c.time()` 不得出现在 predicate 里）。
    // 区间代数假设 predicate 只依赖一个连续变量；多时钟 = 二维区域求交，
    // `still_time` 的语义要重新定义 ⇒ 装配期显式拒绝。
    if let Some(tree) = &predicate {
        if let Err(message) = reject_clock_scoped_time(tree) {
            push_binding_error(this, format!("predicate 绑定 `{}` 非法：{message}", spec.object));
            return;
        }
    }
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
            root_type,
        });
}

/// predicate 里禁止带时钟身份的时间叶子（多时钟只有 root 允许）。
fn reject_clock_scoped_time(expr: &Expr) -> Result<(), String> {
    struct Check;

    impl<'a> ze_comb::ExprVisitor<'a> for Check {
        type Error = String;

        fn enter(
            &mut self,
            expr: &'a Expr,
            _parent: Option<&'a ze_comb::Node>,
        ) -> Result<bool, String> {
            if let Some(channel) = expr.node.channel() {
                if channel.clock.is_some() {
                    return Err(format!(
                        "predicate 只能依赖**绑定时钟**（`clock.time()`），不允许 `clock.time_of(..)` \
                         —— 多时钟请放到 root（通道 `{channel}`）"
                    ));
                }
            }
            Ok(true)
        }
    }

    expr.walk(&mut Check)
}

/// `exists { requires, excludes }`：集合判空（装配期常量折叠成 `Bool`）。
///
/// 这里只造**标记节点**：对象集只有装配期（脚本执行完）才知道，宿主在
/// `merge_expr_bindings` 里把它折成 `Expr::pure(Bool)`。
fn exists_ctor(pod: &dyn Pod, spec: ExistsSpec) -> Handle {
    expr_pod(pod).intern(Expr::exists(spec.requires, spec.excludes))
}

/// `query_self(channel)`：批量绑定里的"当前对象"字段读（`object = QUERY_SELF_OBJECT`）。
fn query_self_ctor(pod: &dyn Pod, channel: ChannelSpec) -> Handle {
    let this = expr_pod(pod);
    let channel =
        ChannelRef::new(channel.namespace, channel.name).with_type(channel_type_of(channel.ty));
    this.intern(Expr::read(channel, QUERY_SELF_OBJECT))
}

/// `bind_each { … }`：登记一条**装配期展开**的批量绑定。
///
/// 这里只做与 `bind` 同口径的装配期校验（结构 / 通道 / 类型 / predicate 单时钟）；
/// 过滤条件的组件名由宿主在展开时解析（未知名字 ⇒ 显式失败）。
fn bind_each(pod: &dyn Pod, spec: BindEachSpec) {
    let this = expr_pod(pod);
    let expr = match this.tree(spec.root) {
        Ok(expr) => expr,
        Err(error) => {
            push_binding_error(this, format!("批量绑定根表达式句柄无效：{error}"));
            return;
        }
    };
    if let Err(message) = validate_binding(&expr) {
        push_binding_error(this, format!("批量绑定非法：{message}"));
        return;
    }
    let root_type = match infer_root(&expr) {
        Ok(ty) => ty,
        Err(error) => {
            push_binding_error(this, format!("批量绑定类型检查失败：{error}"));
            return;
        }
    };
    let predicate = match spec.predicate {
        PredicateSpec::Always => None,
        PredicateSpec::Of { expr: root } => match this.tree(root) {
            Ok(tree) => {
                if let Err(message) = validate_binding(&tree) {
                    push_binding_error(this, format!("批量绑定 predicate 非法：{message}"));
                    return;
                }
                if let Err(error) = infer_predicate(&tree) {
                    push_binding_error(this, format!("批量绑定 predicate 类型检查失败：{error}"));
                    return;
                }
                if let Err(message) = reject_clock_scoped_time(&tree) {
                    push_binding_error(this, format!("批量绑定 predicate 非法：{message}"));
                    return;
                }
                Some(tree)
            }
            Err(error) => {
                push_binding_error(this, format!("批量绑定 predicate 句柄无效：{error}"));
                return;
            }
        },
    };
    if spec
        .requires
        .iter()
        .chain(spec.excludes.iter())
        .any(|name| name.trim().is_empty())
    {
        push_binding_error(this, "批量绑定的过滤组件名不能为空".to_string());
        return;
    }
    this.bind_each
        .lock()
        .expect("批量绑定表未被毒化")
        .push(PendingBindEach {
            target: spec.target,
            expr,
            clock: spec.clock,
            predicate,
            reset: spec.reset,
            requires: spec.requires,
            excludes: spec.excludes,
            root_type,
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
    use ze_comb::{Node, OpRef};

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
            Some(u64::from(ze_comb::SCHEMA_VERSION))
        );
        assert_eq!(json["library"]["namespace"], LIBRARY_NAMESPACE);
        assert_eq!(json["library"]["version"], LIBRARY_VERSION);
        assert_eq!(json["expr"]["node"]["type"], "call");
    }

    /// **H 批**：`select` 构造器生成 `Node::Select`；脚本侧 `otherwise`
    /// 映射到产物字段 `else`（节点形状不随脚本命名变化）。
    #[test]
    fn select_builds_a_value_branch() {
        let this = pod();
        let cond = gt_ctor(
            &this,
            GtSpec {
                a: time(&this),
                b: pure(&this, 0.5),
            },
        );
        let h = select_ctor(
            &this,
            SelectSpec {
                cond,
                then: pure(&this, 1.0),
                otherwise: pure(&this, 0.0),
            },
        );
        let Node::Select { cond, then, r#else } = &this.tree(h).unwrap().node else {
            panic!("期望 Select 节点");
        };
        assert!(matches!(cond.node, Node::Call { ref op, .. } if op.name == "gt"));
        assert!(matches!(then.node, Node::Pure { value: Literal::F64(1.0) }));
        assert!(
            matches!(r#else.node, Node::Pure { value: Literal::F64(0.0) }),
            "otherwise → 产物字段 else"
        );
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

    /// **G 批**：`read` 构造器生成 `Node::Read` 叶子，channel / ty / object 字段正确。
    #[test]
    fn read_builds_a_read_leaf() {
        let this = pod();
        let object = Handle::new(PodId::next(), 3, 1);
        let h = read_ctor(
            &this,
            ReadSpec {
                channel: ChannelSpec {
                    namespace: SCENE_NAMESPACE.to_string(),
                    name: "Circle.radius".to_string(),
                    ty: ChannelTypeSpec::Number,
                },
                object,
            },
        );
        let Node::Read { channel, object: read_object } = &this.tree(h).unwrap().node else {
            panic!("期望 Read 节点");
        };
        assert_eq!(channel.namespace, SCENE_NAMESPACE);
        assert_eq!(channel.name, "Circle.radius", "name 是规范地址");
        assert_eq!(channel.ty, ChannelType::Number);
        assert_eq!(*read_object, 3, "object 取句柄 slot（脚本局部句柄下标）");

        // 快路径与慢路径同一组 helper：`Vec2` 类型也一致（第 2 批：类型来自常量）。
        let pod_any: &dyn Pod = &this;
        let read_spec = ReadSpec {
            channel: ChannelSpec {
                namespace: SCENE_NAMESPACE.to_string(),
                name: "Circle.center".to_string(),
                ty: ChannelTypeSpec::Vec2,
            },
            object,
        };
        let slow: Box<dyn Any> = pod_any
            .call(
                FunctionId::new(
                    this.functions()
                        .iter()
                        .position(|f| f.name() == "read")
                        .expect("read 已注册") as u64,
                ),
                &[&read_spec],
            )
            .expect("慢路径调用成功");
        let slow_handle = slow.downcast::<Handle>().unwrap();
        let Node::Read { channel, .. } = &this.tree(*slow_handle).unwrap().node else {
            panic!("期望 Read 节点");
        };
        assert_eq!(channel.ty, ChannelType::Vec2, "常量类型直接带过来");
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

    /// **B 批 / B1**：脚本侧时钟是**数据**（扁平链）；`clock.time()` 读绑定时钟，\n    /// `clock.time_of(c)` 的叶子带 `c` 的身份（一个表达式可混多条时间轴）。
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

        // `clock.time()` 读绑定时钟；`clock.time_of(c)` 的叶子带 `c` 的身份。
        let a = clock_time(&this);
        let b = time(&this);
        assert_eq!(this.tree(a).unwrap(), this.tree(b).unwrap());
        assert!(matches!(
            this.tree(a).unwrap().node,
            Node::Channel { channel } if channel == ChannelRef::builtin("time")
        ));
    }

    /// **B1 时钟身份**：`clock.time_of(c)` 的叶子带 `c` 的形状；`clock.time()` 不带。
    #[test]
    fn clock_time_of_carries_the_clock_identity() {
        let this = pod();
        let clock = clock_nest(&this, clock_scale(&this, 0.5), clock_shift(&this, 2.0));
        let scoped = clock_time_of(&this, clock);
        let Node::Channel { channel } = &this.tree(scoped).unwrap().node else {
            panic!("期望 Channel 叶子");
        };
        let shape = channel.clock.as_ref().expect("叶子必须带时钟身份");
        assert_eq!(shape.links.len(), 2);
        assert_eq!(shape.links[0].rate, 0.5);
        assert_eq!(shape.links[1].shift, 2.0);
        // 无身份的 `clock.time()` 仍是 `None`（读绑定时钟）。
        assert!(this
            .tree(clock_time(&this))
            .unwrap()
            .node
            .channel()
            .unwrap()
            .clock
            .is_none());
    }

    /// **B1 窗口函子**：`clock.range(lo, hi)` ≡ `And(Ge(t, lo), Lt(t, hi))`。
    ///
    /// 可分析性由场景层区间代数钉住（同一形状）；这里只钉**结构**，
    /// 保证它与四个脚本里被替换掉的 `in_segment` helper 逐节点一致。
    #[test]
    fn clock_range_builds_the_standard_window_shape() {
        let this = pod();
        let window = clock_range(&this, 0.0, 2.0);
        let Node::Call { op, args } = &this.tree(window).unwrap().node else {
            panic!("期望 And 调用");
        };
        assert_eq!(op.name, "and");
        assert_eq!(args.len(), 2);
        let Node::Call { op, args: ge } = &args[0].node else {
            panic!("期望 Ge 调用");
        };
        assert_eq!(op.name, "ge");
        assert_eq!(ge[1].node, Node::Pure { value: Literal::F64(0.0) });
        let Node::Call { op, args: lt } = &args[1].node else {
            panic!("期望 Lt 调用");
        };
        assert_eq!(op.name, "lt");
        assert_eq!(lt[1].node, Node::Pure { value: Literal::F64(2.0) });
    }

    /// **B1 硬约束**：predicate 里出现带时钟身份的时间 ⇒ 装配期显式拒绝。
    #[test]
    fn bind_rejects_clock_scoped_time_in_predicate() {
        let this = pod();
        let clock = clock_shift(&this, 1.0);
        let predicate = clock_range_of(&this, clock, 0.0, 1.0);
        let mut spec = bind_spec(pure(&this, 1.0));
        spec.predicate = PredicateSpec::Of { expr: predicate };
        bind(&this, spec);
        let (_, errors) = this.take_bindings();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("绑定时钟") || error.contains("clock.time_of")),
            "{errors:?}"
        );
    }

    /// **B5**：`nat_rec` 建出 `Node::NatRec`；`count` 必须是非负整数（否则显式报错）。
    #[test]
    fn nat_rec_builds_the_discrete_fold_and_rejects_fractional_counts() {
        let this = pod();
        let step = call2(&this, "add", rec_prev(&this), pure(&this, 2.0));
        let ok = nat_rec_ctor(
            &this,
            NatRecSpec {
                count: 4.0,
                init: pure(&this, 1.0),
                step,
            },
        );
        let Node::NatRec { count, .. } = &this.tree(ok).unwrap().node else {
            panic!("期望 NatRec 节点");
        };
        assert_eq!(count.node, Node::Pure { value: Literal::I64(4) });

        let bad = nat_rec_ctor(
            &this,
            NatRecSpec {
                count: 2.5,
                init: pure(&this, 1.0),
                step: rec_index(&this),
            },
        );
        let _ = bad;
        let (_, errors) = this.take_bindings();
        assert!(
            errors.iter().any(|error| error.contains("非负整数")),
            "{errors:?}"
        );
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

    fn bind_spec(root: Handle) -> BindSpec {
        BindSpec {
            object: Handle::new(PodId::next(), 0, 1),
            target: ChannelSpec {
                namespace: String::new(),
                name: "Circle.radius".to_string(),
                ty: ChannelTypeSpec::Number,
            },
            root,
            clock: ClockSpec { links: Vec::new() },
            predicate: PredicateSpec::Always,
            reset: ResetSpec::Hold,
            duration: 0.0,
        }
    }

    /// **B 批验收**：`Bool` 根在装配期被拒（不经过 `anim_value` 的静默）。
    #[test]
    fn bind_rejects_bool_root_at_assembly() {
        let this = pod();
        let lt = lt_ctor(
            &this,
            LtSpec {
                a: time(&this),
                b: pure(&this, 1.0),
            },
        );
        bind(&this, bind_spec(lt));
        let (bindings, errors) = this.take_bindings();
        assert!(bindings.is_empty(), "Bool 根不应登记为绑定");
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("Bool"), "{errors:?}");
    }

    /// predicate 必须是 `Bool`：装配期失败（AH-03 的运行期检查前移）。
    #[test]
    fn bind_rejects_non_bool_predicate_at_assembly() {
        let this = pod();
        let mut spec = bind_spec(pure(&this, 1.0));
        spec.predicate = PredicateSpec::Of {
            expr: pure(&this, 1.0),
        };
        bind(&this, spec);
        let (_, errors) = this.take_bindings();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("predicate") && error.contains("Bool")),
            "{errors:?}"
        );
    }

    /// 推导结果进 `PendingBinding`（宿主核对目标通道类型用；不进 payload）。
    #[test]
    fn bind_records_the_inferred_root_type() {
        let this = pod();
        let t = time(&this);
        let k = pure(&this, 2.0);
        let root = mul(&this, t, k);
        bind(&this, bind_spec(root));
        let (bindings, errors) = this.take_bindings();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].root_type, ChannelType::Number);
    }
}
