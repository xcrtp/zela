//! 表达式求值的**宿主单子 `M`**（S3）。
//!
//! # 它是唯一的不纯入口
//!
//! `ze-func` 的组合子树只描述结构；值必须由宿主注入。本模块把
//! `TimeState`（时间 + 帧号）包成 [`Mono`]，算子语义委托给函子库
//! `zela.math`（[`ze_pod_func::eval_op`]）—— **同一份算子身份与语义**。
//!
//! # 当前通道
//!
//! 通道是 `(namespace, name)` 数据（[`ze_func::ChannelRef`]），不再是封闭枚举：
//!
//! | 通道 | 状态 |
//! |---|---|
//! | `("", "time")` | ✅ 由 `TimeState.elapsed` 注入（S3 起由帧号派生） |
//! | `("", "frame")` | ✅ 由 `TimeState.frame` 注入 |
//! | `("zela.scene", "Circle.radius")` 等组件字段 | **显式缺失**（读方向已具备 `SceneRead`，尚未接进 M） |
//! | `("", "event")` / `env` | **显式缺失**（S3 后续：输入 / 运行环境） |
//!
//! 缺失通道返回 [`EvalError::MissingChannel`]，不静默给 0。
//!
//! # 边界（记账）
//!
//! `TimeState.elapsed` 是 `f32`（既有跨边界量化点）；这里 `as f64` 只是提升，
//! 不引入新的量化。若将来需要 f64 时间轴，应把 `TimeState` 一起升级，而不是在
//! 求值器里偷偷改精度。

use ze_func::{ChannelRef, EvalError, Mono, OpRef, Value};
use ze_time::AnimValue;

use crate::TimeState;

/// 宿主基础通道名（空 namespace）—— 与 `zela.math::time` / `frame` 是同一份词汇。
pub const TIME_CHANNEL: &str = "time";
pub const FRAME_CHANNEL: &str = "frame";

/// 场景侧的 `M`：时间 / 帧号来自资源，算子来自 `zela.math`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneMono {
    pub time: f64,
    pub frame: u64,
}

impl SceneMono {
    pub fn new(time: f64, frame: u64) -> Self {
        Self { time, frame }
    }

    /// 从 [`TimeState`] 取通道值（每帧构造一次，无状态）。
    pub fn from_time_state(state: &TimeState) -> Self {
        Self {
            time: state.elapsed as f64,
            frame: state.frame,
        }
    }
}

impl Mono for SceneMono {
    fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
        match (channel.namespace.as_str(), channel.name.as_str()) {
            ("", TIME_CHANNEL) => Ok(Value::F64(self.time)),
            ("", FRAME_CHANNEL) => Ok(Value::F64(self.frame as f64)),
            _ => Err(EvalError::MissingChannel {
                channel: channel.clone(),
            }),
        }
    }

    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
        ze_pod_func::eval_op(op, args)
    }
}

/// 求值结果 → 绑定目标可写的值。
///
/// `Bool` / `Text` 没有对应的组件字段通道 ⇒ 返回 `None`（调用方显式记录，不静默写 0）。
pub fn anim_value(value: &Value) -> Option<AnimValue> {
    match value {
        Value::F64(value) => Some(AnimValue::Scalar(*value as f32)),
        Value::Vec2(value) => Some(AnimValue::Vec2((*value).into())),
        Value::Vec3(value) => Some(AnimValue::Vec3((*value).into())),
        Value::Bool(_) | Value::Text(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_func::{CompiledExpr, Expr};

    #[test]
    fn channels_come_from_time_state() {
        let state = TimeState::at_frame(1.5, 45);
        let mono = SceneMono::from_time_state(&state);
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
        // 组件字段通道与基础通道同形，但当前 M 没有场景句柄 ⇒ 显式缺失。
        assert!(mono
            .read(&ChannelRef::new("zela.scene", "Circle.radius"))
            .is_err());
    }

    /// 宿主 M + 函子库语义：`sin(t)` 在 `t = π/2` 处为 1。
    #[test]
    fn evaluation_uses_the_host_monad_and_functor_library() {
        let expr = Expr::call(
            ze_func::OpRef::new("zela.math", "sin"),
            [Expr::channel(ChannelRef::builtin(TIME_CHANNEL))],
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        let mono = SceneMono::new(std::f64::consts::FRAC_PI_2, 0);
        assert_eq!(program.eval(&mono).unwrap(), Value::F64(1.0));
    }

    /// 帧号通道与时间通道互相独立：`sin(frame)` 用的是帧号。
    #[test]
    fn frame_channel_is_independent_of_time() {
        let expr = Expr::call(
            ze_func::OpRef::new("zela.math", "sin"),
            [Expr::channel(ChannelRef::builtin(FRAME_CHANNEL))],
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        let mono = SceneMono::new(0.0, 30);
        assert_eq!(program.eval(&mono).unwrap(), Value::F64(30.0_f64.sin()));
    }

    /// **本批 A**：比较算子经同一个宿主 M 求值得到 `Bool`；
    /// Bool 不进 `anim_value`（root 绑定语义不变，predicate 将走独立路径）。
    #[test]
    fn comparisons_evaluate_to_bool_but_are_not_animatable() {
        let expr = Expr::call(
            ze_func::OpRef::new("zela.math", "lt"),
            [
                Expr::channel(ChannelRef::builtin(TIME_CHANNEL)),
                Expr::pure(2.0),
            ],
        );
        let program = CompiledExpr::compile(&expr).unwrap();
        let value = program.eval(&SceneMono::new(1.0, 0)).unwrap();
        assert_eq!(value, Value::Bool(true));
        assert_eq!(anim_value(&value), None);

        let value = program.eval(&SceneMono::new(3.0, 0)).unwrap();
        assert_eq!(value, Value::Bool(false));
    }
}
