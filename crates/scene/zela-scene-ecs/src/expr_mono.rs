//! 表达式求值的**宿主单子 `M`**（S3）。
//!
//! # 它是唯一的不纯入口
//!
//! `ze-comb` 的组合子树只描述结构；值必须由宿主注入。本模块把
//! `TimeState`（时间 + 帧号）+ [`SceneRead`]（组件字段读端）包成 [`Mono`]，
//! 算子语义委托给函子库 `zela.math`（[`ze_pod_func::eval_op`]）——
//! **同一份算子身份与语义**。
//!
//! # 当前通道
//!
//! 通道是 `(namespace, name)` 数据（[`ze_comb::ChannelRef`]），不再是封闭枚举：
//!
//! | 通道 | 状态 |
//! |---|---|
//! | `("", "time")` | ✅ 由 `TimeState.elapsed` 注入（S3 起由帧号派生） |
//! | `("", "frame")` | ✅ 由 `TimeState.frame` 注入 |
//! | `("zela.scene", "Circle.radius")` 等组件字段 | ✅ **G 批**：经 [`Mono::read_object`] 读回 |
//! | `("", "event")` / `env` | **显式缺失**（S3 后续：输入 / 运行环境） |
//!
//! 缺失通道返回 [`EvalError::MissingChannel`]，不静默给 0。
//!
//! # 边界（记账）
//!
//! `TimeState.elapsed` 是 `f32`（既有跨边界量化点）；这里 `as f64` 只是提升，
//! 不引入新的量化。若将来需要 f64 时间轴，应把 `TimeState` 一起升级，而不是在
//! 求值器里偷偷改精度。

use ze_comb::{ChannelRef, EvalError, Mono, OpRef, Value};
use ze_scene_contract::ObjectId;
use ze_scene_core::animation::target::{read_only_entry, target_entry};
use ze_scene_core::scene_write::SceneRead;
use ze_time::AnimValue;

use crate::TimeState;

/// 宿主基础通道名（空 namespace）—— 与 `zela.math::time` / `frame` 是同一份词汇。
pub const TIME_CHANNEL: &str = "time";
pub const FRAME_CHANNEL: &str = "frame";

/// 场景侧的 `M`：时间 / 帧号来自资源，组件字段读端来自 [`SceneRead`]，算子来自 `zela.math`。
///
/// 持 `&dyn SceneRead`（每帧构造一次，按值使用 ⇒ 不需要 `Clone` / `Copy`）。
pub struct SceneMono<'a> {
    /// 该绑定的**局部时间**（`clock.time()` 的取值）。
    pub time: f64,
    /// 该帧的**全局时间**（`c.time()` 按 `c` 的时钟链反算用）。
    pub global: f64,
    pub frame: u64,
    scene: &'a dyn SceneRead,
}

impl<'a> SceneMono<'a> {
    pub fn new(time: f64, frame: u64, scene: &'a dyn SceneRead) -> Self {
        Self {
            time,
            global: time,
            frame,
            scene,
        }
    }

    /// 同时给出局部 / 全局时间（绑定驱动路径：两者通常不同）。
    pub fn with_global(local: f64, global: f64, frame: u64, scene: &'a dyn SceneRead) -> Self {
        Self {
            time: local,
            global,
            frame,
            scene,
        }
    }

    /// 从 [`TimeState`] 取通道值（每帧构造一次，无状态）。
    pub fn from_time_state(state: &TimeState, scene: &'a dyn SceneRead) -> Self {
        Self {
            time: state.elapsed as f64,
            global: state.elapsed as f64,
            frame: state.frame,
            scene,
        }
    }
}

impl Mono for SceneMono<'_> {
    fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
        match (channel.namespace.as_str(), channel.name.as_str()) {
            ("", TIME_CHANNEL) => Ok(Value::F64(match &channel.clock {
                // `c.time()`：按叶子携带的时钟链在**同一个全局时间**上反算。
                Some(shape) => shape.eval(self.global),
                // `clock.time()`：绑定时钟的局部时间（`None` = 现状）。
                None => self.time,
            })),
            ("", FRAME_CHANNEL) => Ok(Value::F64(self.frame as f64)),
            _ => Err(EvalError::MissingChannel {
                channel: channel.clone(),
            }),
        }
    }

    /// **G 批通道读端**：`zela.scene::<address>` 的组件字段经 [`SceneRead`] 读回。
    ///
    /// `object` 是 `ObjectId` 打包值（`index:u32<<32 | generation:u32`）；按
    /// `channel.name`（规范地址，如 `Circle.radius`）查 [`TARGETS`] 注册表。
    fn read_object(&self, channel: &ChannelRef, object: u64) -> Result<Value, EvalError> {
        // 可写目标走 `TARGETS`；只读派生量（`Reveal.current_pos`）走第二张表 ——
        // 两张表在写端是分开的（只读项不可能被 clip / bind 选中）。
        let Some(spec) = target_entry(&channel.name)
            .map(|entry| entry.spec)
            .or_else(|| read_only_entry(&channel.name))
        else {
            // 地址未注册 ⇒ 与 `Mono::read` 的缺失通道同一口径（显式失败）。
            return Err(EvalError::MissingChannel {
                channel: channel.clone(),
            });
        };
        let id = ObjectId::from_packed(object);
        let value = (spec.read)(self.scene, id).ok_or_else(|| EvalError::MissingComponent {
            object,
            channel: channel.clone(),
        })?;
        let converted = anim_to_value(channel, &value)?;
        // **第 2 批对账**：装配期推导的类型（`channel.ty`）vs 运行期实际返回的标签。
        // `Unknown`（老 payload）不核对 —— 回放路径不做类型推导。
        check_declared_type(channel, &converted)?;
        Ok(converted)
    }

    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
        ze_pod_func::eval_op(op, args)
    }
}

/// 通道声明类型 vs 运行期值的标签（`Unknown` 跳过：老 payload / 未标注常量）。
fn check_declared_type(channel: &ChannelRef, value: &Value) -> Result<(), EvalError> {
    use ze_comb::ChannelType;
    let expected = match channel.ty {
        ChannelType::Unknown => return Ok(()),
        ChannelType::Number => "number",
        ChannelType::I64 => "i64",
        ChannelType::Bool => "bool",
        ChannelType::Vec2 => "vec2",
        ChannelType::Vec3 => "vec3",
        ChannelType::Bytes => "bytes",
    };
    let actual = match value {
        Value::F64(_) => "number",
        Value::I64(_) => "i64",
        Value::Bool(_) => "bool",
        Value::Vec2(_) => "vec2",
        Value::Vec3(_) => "vec3",
        Value::Text(_) => "text",
        Value::Bytes(_) => "bytes",
    };
    if expected == actual {
        Ok(())
    } else {
        Err(EvalError::ChannelTypeMismatch {
            channel: channel.clone(),
            expected,
            actual,
        })
    }
}

/// `AnimValue` → 表达式值（Scalar→F64、Vec2→[f32;2]、Vec3→[f32;3]）。
///
/// **阶段 1（Path 迁移）**：`AnimValue::Path` 不再报"不支持读"，而是走
/// `ze-func::layout` 的**慢路径**编码成 `Value::Bytes { ty = "zela.Path" }`。
/// 路径是变长结构、没有 Layout ⇒ 零拷贝不适用（硬约束 4 的安全缺省）。
fn anim_to_value(channel: &ChannelRef, value: &AnimValue) -> Result<Value, EvalError> {
    match value {
        AnimValue::Scalar(value) => Ok(Value::F64(*value as f64)),
        AnimValue::Vec2(value) => Ok(Value::Vec2([value.x, value.y])),
        AnimValue::Vec3(value) => Ok(Value::Vec3([value.x, value.y, value.z])),
        AnimValue::Color(_) => Err(EvalError::UnsupportedValue {
            channel: channel.clone(),
            kind: "color",
        }),
        AnimValue::Path(path) => Ok(Value::Bytes(encode_path_value(path))),
    }
}

/// `Path2D` → `zela.Path` 字节块（曲线按固定容差展平；慢路径，不进零拷贝）。
fn encode_path_value(path: &ze_notion::Path2D) -> ze_comb::ValueBytes {
    let closed = matches!(
        path.commands.last(),
        Some(ze_notion::PathCommand::Close)
    );
    let points: Vec<[f32; 2]> = path
        .flatten(ze_notion::MORPH_TOLERANCE)
        .into_iter()
        .map(|point| [point.x, point.y])
        .collect();
    ze_func::layout::encode_path(&points, closed)
}

/// `zela.Path` 字节块 → `Path2D`（折线重建；与 [`encode_path_value`] 成对）。
fn decode_path_value(bytes: &ze_comb::ValueBytes) -> Option<ze_notion::Path2D> {
    if bytes.ty != ze_func::layout::PATH_TYPE {
        return None;
    }
    let (points, closed) = ze_func::layout::decode_path(&bytes.bytes).ok()?;
    let points: Vec<ze_notion::Vec2> = points
        .into_iter()
        .map(|point| ze_notion::Vec2::new(point[0], point[1]))
        .collect();
    Some(ze_notion::Path2D::from_polyline(&points, closed))
}

/// 求值结果 → 绑定目标可写的值。
///
/// `Bool` / `Text` / `I64` 没有对应的组件字段通道 ⇒ 返回 `None`
/// （调用方显式记录，不静默写 0）。`Value::Bytes` 只在命名类型是
/// `zela.Path` 时解码回路径；其它命名类型同样显式跳过。
pub fn anim_value(value: &Value) -> Option<AnimValue> {
    match value {
        Value::F64(value) => Some(AnimValue::Scalar(*value as f32)),
        Value::Vec2(value) => Some(AnimValue::Vec2((*value).into())),
        Value::Vec3(value) => Some(AnimValue::Vec3((*value).into())),
        Value::Bytes(bytes) => decode_path_value(bytes).map(AnimValue::Path),
        Value::I64(_) | Value::Bool(_) | Value::Text(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::WorldScene;
    use hecs::World;
    use ze_comb::{CompiledExpr, Expr};
    use ze_scene_contract::Scene as _;
    use ze_scene_core::Circle;

    /// 没有任何组件的场景 —— 读端测试的空宿主（`with_component_ref` 恒 `false`）。
    struct NoScene;
    impl SceneRead for NoScene {
        fn with_component_ref(
            &self,
            _id: ObjectId,
            _type_id: std::any::TypeId,
            _f: &mut dyn FnMut(&dyn std::any::Any),
        ) -> bool {
            false
        }
    }

    #[test]
    fn channels_come_from_time_state() {
        let state = TimeState::at_frame(1.5, 45);
        let mono = SceneMono::from_time_state(&state, &NoScene);
        assert_eq!(
            mono.read(&ChannelRef::builtin(TIME_CHANNEL)).unwrap(),
            Value::F64(1.5)
        );
        assert_eq!(
            mono.read(&ChannelRef::builtin(FRAME_CHANNEL)).unwrap(),
            Value::F64(45.0)
        );
        assert_eq!(
            mono.read(&ChannelRef::builtin("event")).unwrap_err(),
            EvalError::MissingChannel {
                channel: ChannelRef::builtin("event")
            }
        );
        // 组件字段通道与基础通道同形：`read`（无对象）对它仍是显式缺失。
        assert!(mono
            .read(&ChannelRef::new("zela.scene", "Circle.radius"))
            .is_err());
    }

    /// **B1 时钟身份**：`c.time()` 按 `c` 的链在**全局时间**上反算；
    /// `clock.time()`（无身份）仍读绑定局部时间。两者可出现在同一表达式里。
    #[test]
    fn clock_scoped_time_uses_the_global_reference() {
        use ze_comb::{ClockLinkShape, ClockShape};
        let shape = ClockShape {
            links: vec![ClockLinkShape {
                shift: 2.0,
                rate: 0.5,
            }],
        };
        let scoped = ChannelRef::builtin(TIME_CHANNEL).with_clock(shape);
        // 绑定局部 = 0.5，全局 = 5.0 ⇒ `c.time()` = (5 − 2) × 0.5 = 1.5。
        let mono = SceneMono::with_global(0.5, 5.0, 0, &NoScene);
        assert_eq!(mono.read(&scoped).unwrap(), Value::F64(1.5));
        assert_eq!(
            mono.read(&ChannelRef::builtin(TIME_CHANNEL)).unwrap(),
            Value::F64(0.5),
            "无身份的 `clock.time()` 读绑定局部时间"
        );
    }

    /// 宿主 M + 函子库语义：`sin(t)` 在 `t = π/2` 处为 1。
    #[test]
    fn evaluation_uses_the_host_monad_and_functor_library() {
        let expr = Expr::call(
            ze_comb::OpRef::new("zela.math", "sin"),
            [Expr::channel(ChannelRef::builtin(TIME_CHANNEL))],
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        let mono = SceneMono::new(std::f64::consts::FRAC_PI_2, 0, &NoScene);
        assert_eq!(program.eval(&mono).unwrap(), Value::F64(1.0));
    }

    /// 帧号通道与时间通道互相独立：`sin(frame)` 用的是帧号。
    #[test]
    fn frame_channel_is_independent_of_time() {
        let expr = Expr::call(
            ze_comb::OpRef::new("zela.math", "sin"),
            [Expr::channel(ChannelRef::builtin(FRAME_CHANNEL))],
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        let mono = SceneMono::new(0.0, 30, &NoScene);
        assert_eq!(program.eval(&mono).unwrap(), Value::F64(30.0_f64.sin()));
    }

    /// **本批 A**：比较算子经同一个宿主 M 求值得到 `Bool`；
    /// Bool 不进 `anim_value`（root 绑定语义不变，predicate 将走独立路径）。
    #[test]
    fn comparisons_evaluate_to_bool_but_are_not_animatable() {
        let expr = Expr::call(
            ze_comb::OpRef::new("zela.math", "lt"),
            [
                Expr::channel(ChannelRef::builtin(TIME_CHANNEL)),
                Expr::pure(2.0),
            ],
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        let value = program.eval(&SceneMono::new(1.0, 0, &NoScene)).unwrap();
        assert_eq!(value, Value::Bool(true));
        assert_eq!(anim_value(&value), None);

        let value = program.eval(&SceneMono::new(3.0, 0, &NoScene)).unwrap();
        assert_eq!(value, Value::Bool(false));
    }

    /// **G 批通道读端**：`zela.scene::Circle.radius` 经 `SceneRead` 读回组件当前值。
    #[test]
    fn read_object_reads_component_field_through_scene() {
        let mut world = World::new();
        let id = {
            let mut ws = WorldScene::new(&mut world);
            ws.spawn_dyn(vec![Box::new(Circle::new((0.0, 0.0), 1.0))])
                .expect("Circle 已登记")
        };
        {
            let mut ws = WorldScene::new(&mut world);
            ws.set(id, Circle::new((0.0, 0.0), 2.5));
        }
        let ws = WorldScene::new(&mut world);
        let mono = SceneMono::new(0.0, 0, &ws);
        let channel = ChannelRef::new("zela.scene", "Circle.radius");
        let value = mono.read_object(&channel, id.to_packed()).unwrap();
        assert_eq!(value, Value::F64(2.5));
    }

    /// **第 2 批对账**：通道声明类型与运行期值的标签不符 ⇒ 显式错误。
    #[test]
    fn declared_channel_type_must_match_the_runtime_value() {
        let mut world = World::new();
        let id = {
            let mut ws = WorldScene::new(&mut world);
            ws.spawn_dyn(vec![Box::new(Circle::new((0.0, 0.0), 1.0))])
                .expect("Circle 已登记")
        };
        let ws = WorldScene::new(&mut world);
        let mono = SceneMono::new(0.0, 0, &ws);
        // `Circle.radius` 实际是标量，却声明成 Vec2。
        let channel = ChannelRef::new("zela.scene", "Circle.radius")
            .with_type(ze_comb::ChannelType::Vec2);
        let err = mono
            .read_object(&channel, id.to_packed())
            .expect_err("类型不符必须显式失败");
        assert!(
            matches!(
                err,
                EvalError::ChannelTypeMismatch {
                    expected: "vec2",
                    actual: "number",
                    ..
                }
            ),
            "错误应点名类型，实际 {err}"
        );
    }

    /// **阶段 1**：`Color` 仍然没有表达式层表示（显式点名）；`Path` 已迁移为
    /// `Bytes + zela.Path`，读端编码、写端解码，**往返一致**。
    #[test]
    fn color_is_unsupported_but_path_round_trips_through_bytes() {
        let channel = ChannelRef::new("zela.scene", "Shape.fill");
        let err = anim_to_value(&channel, &AnimValue::Color([0.1, 0.2, 0.3, 1.0]))
            .expect_err("Color 不支持读");
        assert_eq!(
            err,
            EvalError::UnsupportedValue {
                channel: channel.clone(),
                kind: "color",
            }
        );

        let points = [
            ze_notion::Vec2::new(0.0, 0.0),
            ze_notion::Vec2::new(1.0, 2.0),
        ];
        let path = ze_notion::Path2D::from_polyline(&points, true);
        let value = anim_to_value(&channel, &AnimValue::Path(path)).expect("Path 迁移为 Bytes");
        let ze_comb::Value::Bytes(bytes) = &value else {
            panic!("Path 必须编码成 Value::Bytes，实际 {value:?}");
        };
        assert_eq!(bytes.ty, ze_func::layout::PATH_TYPE);

        let AnimValue::Path(decoded) = anim_value(&value).expect("zela.Path 可写回") else {
            panic!("zela.Path 必须解码回 AnimValue::Path");
        };
        // 闭合折线展平后会重复首点；比较前 N 个点 + 闭合标记。
        let flattened = decoded.flatten(ze_notion::MORPH_TOLERANCE);
        assert_eq!(&flattened[..points.len()], &points[..]);
        assert!(matches!(
            decoded.commands.last(),
            Some(ze_notion::PathCommand::Close)
        ));

        // 其它私有类型字节块不会被误当成路径（显式跳过，不写 0）。
        let foreign = ze_comb::Value::Bytes(ze_comb::ValueBytes::new("acme.Tensor", vec![1, 2, 3]));
        assert_eq!(anim_value(&foreign), None);
    }

    /// **管道验收**：`Reveal.current_pos` 是只读派生量 —— 读端经 `SceneRead`
    /// 按 `end` 取笔尖点；整条链路不需要任何"位置字段"。
    #[test]
    fn read_reveal_current_pos_through_the_pipeline() {
        let mut world = World::new();
        let id = {
            let mut ws = WorldScene::new(&mut world);
            let path = ze_notion::Path2D::from_polyline(
                &[
                    ze_notion::Vec2::new(-1.0, 0.0),
                    ze_notion::Vec2::new(1.0, 0.0),
                ],
                false,
            );
            let mut reveal = ze_scene_core::components::Reveal::new(path);
            reveal.end = 0.5;
            ws.spawn_dyn(vec![Box::new(reveal)]).expect("Reveal 已登记")
        };
        let ws = WorldScene::new(&mut world);
        let mono = SceneMono::new(0.0, 0, &ws);
        let value = mono
            .read_object(
                &ChannelRef::new("zela.scene", "Reveal.current_pos"),
                id.to_packed(),
            )
            .expect("只读派生通道可读");
        let Value::Vec2([x, y]) = value else {
            panic!("Reveal.current_pos 必须是 Vec2，实际 {value:?}");
        };
        assert!(x.abs() < 1e-4 && y.abs() < 1e-4, "end = 0.5 ⇒ 线段中点");
    }

    /// **G 批**：地址未注册 ⇒ `MissingChannel`；对象上无该组件 ⇒ `MissingComponent`
    /// （两个"显式失败"都钉住，不静默给 0）。
    #[test]
    fn read_object_fails_explicitly_for_unregistered_and_missing_component() {
        let mut world = World::new();
        let id = {
            let mut ws = WorldScene::new(&mut world);
            ws.spawn_dyn(vec![Box::new(Circle::new((0.0, 0.0), 1.0))])
                .expect("Circle 已登记")
        };
        let ws = WorldScene::new(&mut world);
        let mono = SceneMono::new(0.0, 0, &ws);

        let err = mono
            .read_object(&ChannelRef::new("zela.scene", "Foo.bar"), id.to_packed())
            .unwrap_err();
        assert_eq!(
            err,
            EvalError::MissingChannel {
                channel: ChannelRef::new("zela.scene", "Foo.bar")
            }
        );

        let ghost = ObjectId::new(999, 1);
        let err = mono
            .read_object(&ChannelRef::new("zela.scene", "Circle.radius"), ghost.to_packed())
            .unwrap_err();
        assert!(
            matches!(err, EvalError::MissingComponent { .. }),
            "对象上无组件 ⇒ 显式 MissingComponent"
        );
    }
}
