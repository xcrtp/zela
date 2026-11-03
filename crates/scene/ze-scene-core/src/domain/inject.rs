//! **CH-a**：`Injected` envelope —— 给每一次注入打标记。
//!
//! # 它是什么，不是什么
//!
//! 不是追踪框架，是「这一帧这批输入是谁发的」**选择子**：
//!
//! ```text
//! 任意外部事件（窗口 / 场景 / 更多类别）
//!    ↓  Injected { tag, frame, source, payload }   ← 标记在外层，事件本体不动
//! WindowInput → collect_events → SceneEvent → System → Frame
//! ```
//!
//! # 三条反面约束（比 API 列表重要）
//!
//! | # | 不要 | 为什么 |
//! |---|---|---|
//! | ① | 做成全链路埋点 / 动态追踪 | 每个 System 传播 tag 会改遍全仓、必漏，还引入隐式状态，破坏确定性 |
//! | ② | 让 tag 进 [`SceneEvent`] | 它是跨进程 tagged JSON，加字段会改掉所有既有输入流的字节 |
//! | ③ | 靠 System 手工上报响应 | 会漏、会变成第二套；差分是**全覆盖**的（CH-b） |
//!
//! # 开放分类
//!
//! [`InjectedPayload::External`] **只留形状 + 显式报错**，不实现 —— 与 CG-d
//! 「四类形状一次留够、实现按消费者来」同一条纪律。本批真正的消费者只有
//! `Window`（CH-a）与 `Scene`（CG 路径）。

use crate::domain::event::SceneEvent;
use crate::domain::input_window::WindowInput;

/// 注入标记：声明序号（与 `Handle` / `LayoutId` / `ScopeId` 同构，单调、稳定）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
    serde::Serialize, serde::Deserialize,
)]
pub struct TagId(pub u32);

/// 注入来源（开放分类的最小三档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectSource {
    /// 窗口层：真人输入 / Debug 的窗口坐标事件（与真人走同一个 `collect_events`）。
    Window,
    /// 场景层：已换算的场景事件（CG 的 headless 路径）。
    Scene,
    /// 外部事件（只留形状；本批显式拒绝）。
    External,
}

/// **CH-f**：本帧窗口输入的来源标记（作为资源写入）。
///
/// 真人交互为 `None`；debug 注入时由 `zela-app::debug` 写入，GUI 的录制器据此把
/// tag 记进**窗口层流** —— 否则回放时分不清"这一帧是真点的还是注入的"。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameTag(pub Option<TagId>);

/// 注入载荷。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InjectedPayload {
    /// 窗口输入：交给 `collect_events` 换算。
    Window(WindowInput),
    /// 场景事件：CG 路径（已换算好）。
    Scene(SceneEvent),
    /// 外部事件：本批只留形状。
    External { name: String },
}

/// 一次带标记的注入。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Injected {
    pub tag: TagId,
    /// 可选的人类可读标签（只用于诊断，不参与语义）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 显式帧号：**绝不因请求多跑一帧**（调度器按帧号消费）。
    pub frame: u32,
    pub source: InjectSource,
    pub payload: InjectedPayload,
}

/// 注入被拒绝的原因（全部显式，不静默丢弃）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InjectError {
    /// `External` 类别只有形状，没有实现。
    #[error("外部事件类别尚未实现（只有形状）：`{name}`")]
    ExternalUnsupported { name: String },
    /// `source` 与 `payload` 不匹配（例如 source=Window 却给了场景事件）。
    ///
    /// 字段名避开 `source`：`thiserror` 会把它当成错误链的来源。
    #[error("注入来源与载荷不匹配：source={injected:?}，payload={payload}")]
    SourceMismatch {
        injected: InjectSource,
        payload: &'static str,
    },
}

impl Injected {
    /// 窗口输入注入（Debug / RPC / 窗口录制共用）。
    pub fn window(tag: u32, frame: u32, input: WindowInput) -> Self {
        Self {
            tag: TagId(tag),
            label: None,
            frame,
            source: InjectSource::Window,
            payload: InjectedPayload::Window(input),
        }
    }

    /// 已换算的场景事件注入（CG 的 headless 路径）。
    pub fn scene(tag: u32, frame: u32, event: SceneEvent) -> Self {
        Self {
            tag: TagId(tag),
            label: None,
            frame,
            source: InjectSource::Scene,
            payload: InjectedPayload::Scene(event),
        }
    }

    /// 外部事件（只有形状）。
    pub fn external(tag: u32, frame: u32, name: impl Into<String>) -> Self {
        Self {
            tag: TagId(tag),
            label: None,
            frame,
            source: InjectSource::External,
            payload: InjectedPayload::External { name: name.into() },
        }
    }

    /// 诊断可读标签。
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// 消费前的**显式校验**：类别未实现 / 来源与载荷不匹配 ⇒ 明确错误。
    pub fn validate(&self) -> Result<(), InjectError> {
        let payload = match &self.payload {
            InjectedPayload::Window(_) => "window",
            InjectedPayload::Scene(_) => "scene",
            InjectedPayload::External { .. } => "external",
        };
        let expected = match self.payload {
            InjectedPayload::Window(_) => InjectSource::Window,
            InjectedPayload::Scene(_) => InjectSource::Scene,
            InjectedPayload::External { .. } => InjectSource::External,
        };
        if self.source != expected {
            return Err(InjectError::SourceMismatch {
                injected: self.source,
                payload,
            });
        }
        if let InjectedPayload::External { name } = &self.payload {
            return Err(InjectError::ExternalUnsupported {
                name: name.clone(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_envelope_round_trips_through_json() {
        let injected = Injected::window(3, 7, WindowInput::default()).with_label("drag slider");
        let json = serde_json::to_string(&injected).expect("可序列化");
        let back: Injected = serde_json::from_str(&json).expect("可反序列化");
        assert_eq!(injected, back);
        assert_eq!(back.tag, TagId(3));
        assert_eq!(back.frame, 7);
        assert_eq!(back.source, InjectSource::Window);
        assert!(back.validate().is_ok());
    }

    /// **口径②**：envelope 在外层，`SceneEvent` 本体一个字节不动。
    #[test]
    fn scene_payload_is_wrapped_without_mutating_the_event() {
        let event = SceneEvent::ViewportClick {
            layer: crate::domain::view::LayerId(0),
            scene_pos: ze_notion::Vec2::new(1.0, 2.0),
        };
        let bare = serde_json::to_value(&event).expect("事件可序列化");
        let injected = Injected::scene(1, 0, event.clone());
        let wrapped = serde_json::to_value(&injected).expect("envelope 可序列化");
        assert_eq!(wrapped["payload"]["kind"], serde_json::json!("scene"));
        // 事件对象原样嵌套（字段一个不多、一个不少）。
        let inner = wrapped["payload"].clone();
        let mut inner = inner;
        inner.as_object_mut().expect("对象").remove("kind");
        assert_eq!(inner, bare);
        assert_eq!(injected.validate(), Ok(()));
    }

    #[test]
    fn external_is_shape_only_and_rejected_explicitly() {
        let injected = Injected::external(9, 0, "keyboard-shortcut");
        assert_eq!(
            injected.validate(),
            Err(InjectError::ExternalUnsupported {
                name: "keyboard-shortcut".to_string()
            })
        );
    }

    #[test]
    fn mismatched_source_and_payload_is_an_explicit_error() {
        let mut injected = Injected::window(1, 0, WindowInput::default());
        injected.source = InjectSource::Scene;
        assert!(matches!(
            injected.validate(),
            Err(InjectError::SourceMismatch { .. })
        ));
    }
}
