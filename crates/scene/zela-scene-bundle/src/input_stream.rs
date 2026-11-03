//! **输入流**（D 批）：与 ZSB 分离的独立产物。
//!
//! # 为什么是独立文件
//!
//! 渲染是 `Render(ZSB, InputStream) → Frames`：
//! - **ZSB 不记录输入** —— 它的确定性只由「纯函数 + 无隐式状态」保证；
//! - 输入流有自己的格式 / 版本 / 校验，换一份输入 + 同一份 ZSB ⇒ 另一份输出。
//!
//! 这就是 ROM + replay 模型（TAS / demo 文件）：程序与输入分离，组合可复现。
//!
//! # 索引是帧号，不是时间
//!
//! 帧号是一等通道；时间索引会引入浮点精度问题（同一帧必须落在同一时刻）。
//! 流是**稀疏**的：大部分帧没有事件，`frames` 只写有事件的帧。
//!
//! # 配对校验（这是本模型唯一的真风险）
//!
//! 输入流是为**某一份 ZSB** 录的。ZSB 改了（换算子 / 调系数 / 库版本变了）⇒
//! 同一份输入会产生不同结果。不做校验的症状是「画面变了但没报错」。
//! 因此流里显式记录 `zsb_payload_hash` + 函子库身份，加载时**不匹配即显式失败**。

use serde::{Deserialize, Serialize};

use zela_scene_ecs::animation::LibraryRef;
use zela_scene_ecs::domain::{InputEvents, SceneEvent};
use zela_scene_ecs::scene_script::SceneScript;

use crate::Bundle;

/// 输入流格式标识（进文件；改名 = 新格式）。
pub const INPUT_FORMAT: &str = "zela.input/1";

/// 输入流读取 / 配对失败（全部显式，不静默 desync）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    /// 格式标识不受支持。
    UnsupportedFormat { found: String },
    /// JSON 非法。
    Json { message: String },
    /// ZSB payload 还原失败（读不出函子库身份）。
    Zsb { message: String },
    /// 输入流是为**另一份** ZSB 录的。
    PayloadHashMismatch { expected: String, actual: String },
    /// payload hash 相同但函子库身份不同（理论不可达；防御显式篡改）。
    LibrariesMismatch {
        expected: Vec<LibraryId>,
        actual: Vec<LibraryId>,
    },
    /// 序列化不具确定性（NaN / Infinity）。
    NonCanonical { message: String },
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedFormat { found } => write!(
                f,
                "不支持的输入流格式 `{found}`（本实现支持 `{INPUT_FORMAT}`）"
            ),
            Self::Json { message } => write!(f, "输入流 JSON 非法：{message}"),
            Self::Zsb { message } => write!(f, "读取 ZSB 失败（配对信息）：{message}"),
            Self::PayloadHashMismatch { expected, actual } => write!(
                f,
                "输入流与 ZSB 不匹配：输入流记录 {expected}，当前 ZSB 是 {actual}\n  \
                 （ZSB 变了 ⇒ 同一份输入会产生不同结果；请重新录制，或换回匹配的 ZSB）"
            ),
            Self::LibrariesMismatch { expected, actual } => write!(
                f,
                "输入流的函子库身份与 ZSB 不一致：记录 {expected:?}，当前 {actual:?}"
            ),
            Self::NonCanonical { message } => {
                write!(f, "输入流不具确定性：{message}")
            }
        }
    }
}

impl std::error::Error for InputError {}

/// 函子库身份（namespace + version 是**数据**，不是 Rust 类型名）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryId {
    pub namespace: String,
    pub version: String,
}

impl From<&LibraryRef> for LibraryId {
    fn from(library: &LibraryRef) -> Self {
        Self {
            namespace: library.namespace.clone(),
            version: library.version.clone(),
        }
    }
}

/// 输入流为**谁**而录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pairing {
    /// 目标 ZSB 的 payload hash（hex）。
    pub zsb_payload_hash: String,
    /// ZSB 里出现的函子库身份（去重、排序）。hash 已覆盖它，这里显式写出来是为了
    /// 版本漂移时给出**可读**的错误。
    #[serde(default)]
    pub libraries: Vec<LibraryId>,
}

impl Pairing {
    /// 从一份 ZSB 读取配对信息。
    pub fn for_bundle(bundle: &Bundle) -> Result<Self, InputError> {
        let script = bundle
            .script()
            .map_err(|error| InputError::Zsb {
                message: error.to_string(),
            })?;
        Ok(Self {
            zsb_payload_hash: bundle.payload_hash_hex(),
            libraries: libraries_of(&script),
        })
    }

    /// 校验输入流确实是为这份 ZSB 录的；任何不匹配都**显式失败**。
    pub fn validate(&self, bundle: &Bundle) -> Result<(), InputError> {
        let actual_hash = bundle.payload_hash_hex();
        if self.zsb_payload_hash != actual_hash {
            return Err(InputError::PayloadHashMismatch {
                expected: self.zsb_payload_hash.clone(),
                actual: actual_hash,
            });
        }
        let actual = Self::for_bundle(bundle)?;
        if self.libraries != actual.libraries {
            return Err(InputError::LibrariesMismatch {
                expected: self.libraries.clone(),
                actual: actual.libraries,
            });
        }
        Ok(())
    }
}

fn libraries_of(script: &SceneScript) -> Vec<LibraryId> {
    script.libraries().iter().map(LibraryId::from).collect()
}

/// 某一帧的事件（`frame` 是**帧号**；空帧不写入）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameEvents {
    pub frame: u32,
    #[serde(default)]
    pub events: Vec<SceneEvent>,
}

/// 一份**输入流**：配对信息 + 按帧索引的稀疏事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputStream {
    /// 格式标识（读取时校验）。
    pub format: String,
    pub pairing: Pairing,
    #[serde(default)]
    pub frames: Vec<FrameEvents>,
}

impl InputStream {
    /// 空流（零输入 = `Render(ZSB, ∅)`）。
    pub fn new(pairing: Pairing) -> Self {
        Self {
            format: INPUT_FORMAT.to_string(),
            pairing,
            frames: Vec::new(),
        }
    }

    /// 按帧号追加事件；同一帧多次追加会**合并**（保持追加顺序）。
    pub fn record(&mut self, frame: u32, events: impl IntoIterator<Item = SceneEvent>) {
        let events: Vec<SceneEvent> = events.into_iter().collect();
        if events.is_empty() {
            return;
        }
        match self.frames.binary_search_by_key(&frame, |entry| entry.frame) {
            Ok(index) => self.frames[index].events.extend(events),
            Err(index) => self.frames.insert(
                index,
                FrameEvents {
                    frame,
                    events,
                },
            ),
        }
    }

    /// 第 `frame` 帧的事件（稀疏；没有事件返回空切片）。
    pub fn events_at(&self, frame: u32) -> &[SceneEvent] {
        match self.frames.binary_search_by_key(&frame, |entry| entry.frame) {
            Ok(index) => &self.frames[index].events,
            Err(_) => &[],
        }
    }

    /// 是否为零输入（`Render(ZSB, ∅)`）。
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// 规范 JSON（帧号升序、同帧合并）。返回前做一次 **round-trip 等价**检查：
    /// 反序列化再序列化必须逐字节相同（NaN / Infinity 会在这里暴露）。
    pub fn to_json(&self) -> Result<String, InputError> {
        let mut canonical = self.clone();
        canonical.canonicalize();
        let text = serde_json::to_string_pretty(&canonical).map_err(|error| InputError::Json {
            message: error.to_string(),
        })?;
        let back = Self::from_json(&text)?;
        let again = serde_json::to_string_pretty(&back).map_err(|error| InputError::Json {
            message: error.to_string(),
        })?;
        if again != text {
            return Err(InputError::NonCanonical {
                message: "反序列化后再序列化的字节不同（含 NaN / Infinity 或非确定顺序）"
                    .to_string(),
            });
        }
        Ok(text)
    }

    /// 从 JSON 读取并规范化（排序 + 合并同帧）。
    pub fn from_json(text: &str) -> Result<Self, InputError> {
        let mut stream: Self = serde_json::from_str(text).map_err(|error| InputError::Json {
            message: error.to_string(),
        })?;
        if stream.format != INPUT_FORMAT {
            return Err(InputError::UnsupportedFormat {
                found: stream.format,
            });
        }
        stream.canonicalize();
        Ok(stream)
    }

    fn canonicalize(&mut self) {
        let mut merged: Vec<FrameEvents> = Vec::with_capacity(self.frames.len());
        let mut frames = std::mem::take(&mut self.frames);
        frames.sort_by_key(|entry| entry.frame);
        for mut entry in frames {
            match merged.last_mut() {
                Some(last) if last.frame == entry.frame => {
                    last.events.append(&mut entry.events);
                }
                _ => {
                    if !entry.events.is_empty() {
                        merged.push(entry);
                    }
                }
            }
        }
        self.frames = merged;
    }
}

/// **录制开关**：默认关（零输入）；显式 [`Recorder::recording`] 才录。
#[derive(Debug, Clone, Default)]
pub struct Recorder {
    stream: Option<InputStream>,
}

impl Recorder {
    /// 关闭录制（默认）。
    pub fn disabled() -> Self {
        Self { stream: None }
    }

    /// 显式开启录制：`pairing` 指向当前 ZSB。
    pub fn recording(pairing: Pairing) -> Self {
        Self {
            stream: Some(InputStream::new(pairing)),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.stream.is_some()
    }

    /// 每帧调用：把该帧的 `InputEvents` 写进流（空帧不写）。
    ///
    /// 关闭时是**纯 no-op** —— 调用方不需要在热路径外加分支。
    pub fn capture(&mut self, frame: u32, events: &InputEvents) {
        let Some(stream) = &mut self.stream else {
            return;
        };
        stream.record(frame, events.iter().copied());
    }

    /// 收尾：关闭录制时返回 `None`（**没有流**，而不是空流）。
    pub fn finish(self) -> Option<InputStream> {
        self.stream
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MetaInput;
    use ze_comb::{ChannelRef, Expr, OpRef};
    use ze_scene_core::Circle;
    use zela_scene_ecs::animation::LibraryRef;
    use zela_scene_ecs::domain::ids;
    use zela_scene_ecs::scene_script::SceneEncoder;
    use zela_scene_ecs::ClockProgram;

    fn library() -> LibraryRef {
        LibraryRef {
            namespace: "zela.math".to_string(),
            version: "6".to_string(),
            op_revisions: Default::default(),
        }
    }

    fn script_with_expression() -> SceneScript {
        let mut encoder = SceneEncoder::new();
        let dot = encoder.spawn(vec![Box::new(Circle::new((0.0, 0.0), 1.0))]);
        let expr = Expr::call(
            OpRef::new("zela.math", "sin"),
            [Expr::channel(ChannelRef::builtin("time"))],
        );
        encoder
            .bind_expr(dot, "CircleRadius", expr, library(), ClockProgram::new(), 2.0)
            .expect("表达式绑定合法");
        encoder.finish()
    }

    fn script_without_expression() -> SceneScript {
        let mut encoder = SceneEncoder::new();
        encoder.spawn(vec![Box::new(Circle::new((0.0, 0.0), 2.0))]);
        encoder.finish()
    }

    fn bundle(script: SceneScript) -> Bundle {
        Bundle::encode(&script, MetaInput::default()).expect("最小 ZSB")
    }

    fn pairing() -> Pairing {
        Pairing::for_bundle(&bundle(script_with_expression())).expect("配对信息")
    }

    #[test]
    fn pairing_records_hash_and_library_identity() {
        let bundle = bundle(script_with_expression());
        let pairing = Pairing::for_bundle(&bundle).expect("配对信息");
        assert_eq!(pairing.zsb_payload_hash, bundle.payload_hash_hex());
        assert_eq!(
            pairing.libraries,
            vec![LibraryId {
                namespace: "zela.math".to_string(),
                version: "6".to_string(),
            }]
        );
        pairing.validate(&bundle).expect("同一份 ZSB 通过配对");
    }

    #[test]
    fn pairing_rejects_a_different_zsb() {
        // 验收：ZSB 变更后加载旧输入流 ⇒ **显式 fail**（不静默 desync）。
        let old = pairing();
        let changed = bundle(script_without_expression());
        match old.validate(&changed) {
            Err(InputError::PayloadHashMismatch { expected, actual }) => {
                assert_ne!(expected, actual);
            }
            other => panic!("必须报 payload hash 不匹配，实际 {other:?}"),
        }
    }

    #[test]
    fn stream_is_frame_indexed_and_byte_stable() {
        let mut stream = InputStream::new(pairing());
        stream.record(3, [SceneEvent::Button { id: ids::PLAY_PAUSE }]);
        stream.record(1, [SceneEvent::Slider {
            id: ids::SPEED,
            value: 0.5,
        }]);
        stream.record(3, [SceneEvent::Button { id: ids::REPLAY }]);

        let text = stream.to_json().expect("规范 JSON");
        assert_eq!(stream.to_json().expect("幂等"), text, "两次输出逐字节相同");
        let back = InputStream::from_json(&text).expect("可读回");
        assert_eq!(back.to_json().expect("规范"), text, "读回后仍逐字节相同");
        assert_eq!(
            back.frames.iter().map(|entry| entry.frame).collect::<Vec<_>>(),
            vec![1, 3],
            "帧号升序"
        );
        assert_eq!(back.events_at(3).len(), 2, "同帧事件合并");
        assert!(back.events_at(2).is_empty(), "稀疏：没事件的帧为空");
    }

    #[test]
    fn recorder_is_off_by_default() {
        let events = InputEvents::from_events(vec![SceneEvent::Button {
            id: ids::PLAY_PAUSE,
        }]);
        let mut off = Recorder::default();
        assert!(!off.is_enabled(), "默认关");
        off.capture(0, &events);
        assert!(off.finish().is_none(), "关闭时不产生流");

        let mut on = Recorder::recording(pairing());
        assert!(on.is_enabled());
        on.capture(0, &events);
        let stream = on.finish().expect("显式开启后才有流");
        assert_eq!(stream.events_at(0).len(), 1);
    }

    #[test]
    fn replay_twice_is_identical() {
        let mut stream = InputStream::new(pairing());
        stream.record(0, [SceneEvent::Button { id: ids::PLAY_PAUSE }]);
        stream.record(2, [SceneEvent::Slider {
            id: ids::SPEED,
            value: 1.5,
        }]);
        let text = stream.to_json().expect("规范 JSON");
        let run = || {
            let decoded = InputStream::from_json(&text).expect("读回");
            let mut out = String::new();
            for frame in 0..6 {
                for event in decoded.events_at(frame) {
                    out.push_str(&serde_json::to_string(event).expect("事件可序列化"));
                }
            }
            out
        };
        assert_eq!(run(), run(), "同一份输入流重放两次逐位相同");
    }

    #[test]
    fn format_is_explicit() {
        let stream = InputStream::new(pairing());
        let text = stream.to_json().expect("规范 JSON").replace(
            INPUT_FORMAT,
            "zela.input/999",
        );
        match InputStream::from_json(&text) {
            Err(InputError::UnsupportedFormat { found }) => {
                assert_eq!(found, "zela.input/999");
            }
            other => panic!("必须报格式不支持，实际 {other:?}"),
        }
    }
}
