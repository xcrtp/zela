// event.rs — 场景语义事件（向上契约：UI → 场景）
//
// UI 适配器把原生输入翻译成 [`SceneEvent`]；事件里不得有任何 `egui::` 类型。
// 坐标一律是场景坐标（换算由适配器借助 `Camera2D::screen_to_world` 完成）；
// 控件按 [`WidgetId`] 标识——场景侧只认 ID，不知道控件长什么样。
//
// [`InputEvents`] 是普通 Vec，可 headless 构造 → 场景 System 可以脱离窗口单测。

use serde::{Deserialize, Serialize};
use ze_notion::Vec2;

/// 控件唯一标识（场景契约的一部分，UI 侧按 ID 发事件）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WidgetId(pub u32);

/// 集中定义的控件 ID（当前全部控件；新增控件在此登记）。
pub mod ids {
    use super::WidgetId;

    // ── 控制面板（UiConfig）──
    pub const SHOW_GRID: WidgetId = WidgetId(1);
    pub const SHOW_FIELD: WidgetId = WidgetId(2);

    // ── 时间控制（DirectorState）──
    pub const PLAY_PAUSE: WidgetId = WidgetId(3);
    pub const SPEED: WidgetId = WidgetId(4);
    pub const SEEK: WidgetId = WidgetId(5);
    /// 重新播放：把全局时间拨回 0 并恢复播放（分镜从第 1 条重放）。
    ///
    /// 为什么"拨回 0"就够了：绑定要么无记忆（`O1` 系列），要么有记忆但可从初始态
    /// 向前重算（L2：`State` / `EventReplay` / 弹簧）⇒ 回到 `t = 0` 即回到初始画面，
    /// 不需要重建场景。反向 seek 仍不支持（`ReverseNotSeekable`）。
    ///
    /// [`SeekKind::O1`]: https://docs.rs/zela-scene
    pub const REPLAY: WidgetId = WidgetId(6);

    // ── 参数面板（BindingTable）──
    pub const CIRCLE_BASE: WidgetId = WidgetId(10);
    pub const CIRCLE_AMP: WidgetId = WidgetId(11);
    pub const CIRCLE_OMEGA: WidgetId = WidgetId(12);
    pub const PHASE_OMEGA: WidgetId = WidgetId(13);
    pub const ORBIT_RADIUS: WidgetId = WidgetId(14);
    pub const ORBIT_SPEED: WidgetId = WidgetId(15);
    pub const ORBIT_ANGLE: WidgetId = WidgetId(16);
}

/// 场景语义事件。
///
/// 注：派生 `Clone` 但**不派生 `Copy`** —— CG-d 的 `Text` / `Command` 携带 `String`。
/// 格式版本必须**显式**（`type` 标签是编码的一部分），但旧格式不必留读取分支：
/// 新增事件只是追加，解码端按当前版本 fail-fast，重录后逐位可复现。
///
/// **输入流**（D 批）把它按 tagged JSON 写进独立文件：帧索引 + 事件，
/// 因此这里是跨进程格式的一部分（字段改名 = 新格式）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SceneEvent {
    // ─── 视口拾取（坐标已换算为场景坐标；非相机操作）───
    ViewportHover {
        layer: LayerId,
        scene_pos: Vec2,
    },
    ViewportClick {
        layer: LayerId,
        scene_pos: Vec2,
    },
    /// **CE-e**：按键抬起（与 `ViewportClick` 配对）。
    ///
    /// 拖拽需要"按下 / 抬起"两个端点：只有按下会让交互进入 capture（锁定目标），
    /// 抬起才解除。缺了它，拖拽期间指针移出就会"丢抓"或永久锁定。
    ViewportRelease {
        layer: LayerId,
        scene_pos: Vec2,
    },

    // ─── 相机命令（语义化，不含 center/scale/eye 等字段名）───
    // 命令如何改变相机状态由相机控制器按 CameraMode 决定（PanZoom/Orbit/Path）。
    Camera {
        layer: LayerId,
        command: crate::domain::camera::CameraCommand,
    },

    // ─── 控件事件（按 ID 标识，场景侧认不出 egui）───
    Toggle {
        id: WidgetId,
        value: bool,
    },
    Slider {
        id: WidgetId,
        value: f32,
    },
    Button {
        id: WidgetId,
    },

    // ─── CG-d：四类输入的**形状**（只定形状，暂不实现）───
    //
    // 为什么一次留够：这里是跨进程 tagged JSON，**字段改名 = 新格式**。
    // headless 一旦依赖事件流，逐个加就是一次次破坏兼容；四类一次留好，
    // 后续批次只填实现，不改格式。
    Pointer {
        input: PointerInput,
    },
    Key {
        input: KeyInput,
    },
    Text {
        input: TextInput,
    },
    Command {
        input: CommandInput,
    },

    // ─── CK-d：结构变化是一等输入 ───
    //
    // **不进产物**：它属于输入流（与 `zela.window/1` 同源），不是作品 payload 字段。
    // 回放端从「已有声明 + 事件流前缀」重算；`delta` 在**帧边界**生效，
    // 不在事件分发中途（否则冒泡正沿命中链上溯时树变了，父子关系同帧二义）。
    //
    // 跨帧身份：拖拽 / focus / capture 指向的控件被删 ⇒ 显式取消 capture 并记诊断，
    // 或冒泡到容器 —— 二选一由消费方写死，**不许静默**。
    StructureDelta {
        ops: Vec<StructureOp>,
    },
}

/// **CK-d**：一条结构变化（声明期稳定 id，不是运行时句柄）。
///
/// id 用 [`WidgetId`]（控件声明键）：稳定、内容寻址、与插入顺序无关。
/// 这正是「事件流换台机器重放不漂」的前提。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StructureOp {
    /// 挂载一个控件（`parent = None` 表示根）。
    Mount {
        id: WidgetId,
        parent: Option<WidgetId>,
    },
    /// 卸载一个控件。
    Unmount { id: WidgetId },
    /// 改变归属（树语义：入度仍 ≤ 1）。
    Reparent {
        id: WidgetId,
        parent: Option<WidgetId>,
    },
}

/// 指针按钮（CG-d 形状）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

/// **CG-d**：指针输入（形状）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PointerInput {
    Moved {
        scene_pos: Vec2,
    },
    Pressed {
        scene_pos: Vec2,
        button: PointerButton,
    },
    Released {
        scene_pos: Vec2,
        button: PointerButton,
    },
    /// 滚轮 / 触控板（`delta` 为场景单位）。
    Wheel {
        scene_pos: Vec2,
        delta: Vec2,
    },
}

/// **CG-d**：修饰键（形状）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

/// **CG-d**：键盘输入（形状）。`key` 用 W3C `KeyboardEvent.key` 口径（`"a"` / `"Enter"`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KeyInput {
    Down {
        key: String,
        modifiers: KeyModifiers,
    },
    Up {
        key: String,
        modifiers: KeyModifiers,
    },
}

/// **CI-d 口子⑤**：输入法锚点的**引用**（不存字符 offset）。
///
/// 锚点只引用「哪段文本 + 哪一端」；真正的屏幕 / 场景坐标由布局在消费端现算。
/// 这样文本重排后候选窗跟着走，不会因为录制时存了旧 offset 而漂移。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextAnchorRef {
    /// 被引用的文本对象的**语义名**（不是运行期 id）。
    pub target: String,
    pub edge: TextAnchorEdge,
}

/// 锚点端（当前只有两端；预留中间插入点由布局反查）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAnchorEdge {
    Start,
    End,
}

/// **CG-d / CI-d**：文本输入（形状）。输入法组合单独一支（组合期不能当提交）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TextInput {
    /// 已提交的文本（键入 / 粘贴）。
    Commit { text: String },
    /// 输入法组合中的临时文本（`composing = false` 表示组合结束）。
    Compose { text: String, composing: bool },
    /// **CI-d**：输入法候选窗锚点（引用文本，不存 offset）。
    Anchor { anchor: TextAnchorRef },
}

/// **CG-d**：语义命令（形状）。Agent 与快捷键共用一条通道。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandInput {
    /// 按名字触发（与控件语义名同一套命名）。
    Invoke { name: String },
    /// 直接设值（Agent 断言 / 脚本回放用；不经过拖拽）。
    SetValue { name: String, value: f64 },
}

use crate::domain::view::LayerId;

/// 每帧收集的事件（Resource，由 UI 适配器填充、System 消费）。
#[derive(Debug, Clone, Default)]
pub struct InputEvents {
    events: Vec<SceneEvent>,
}

impl InputEvents {
    pub fn from_events(events: Vec<SceneEvent>) -> Self {
        Self { events }
    }

    pub fn push(&mut self, event: SceneEvent) {
        self.events.push(event);
    }

    pub fn extend<I: IntoIterator<Item = SceneEvent>>(&mut self, events: I) {
        self.events.extend(events);
    }

    pub fn iter(&self) -> impl Iterator<Item = &SceneEvent> {
        self.events.iter()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 取走全部事件（清空队列）。
    pub fn drain(&mut self) -> std::vec::Drain<'_, SceneEvent> {
        self.events.drain(..)
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::view::LayerId;

    /// headless 构造：不需要任何窗口/UI 上下文。
    #[test]
    fn input_events_can_be_constructed_headless() {
        let mut events = InputEvents::default();
        assert!(events.is_empty());

        events.push(SceneEvent::Camera {
            layer: LayerId(0),
            command: crate::domain::CameraCommand::Zoom { factor: 1.1 },
        });
        events.push(SceneEvent::Button {
            id: ids::PLAY_PAUSE,
        });
        events.push(SceneEvent::Slider {
            id: ids::SPEED,
            value: 2.0,
        });

        assert_eq!(events.len(), 3);
        assert!(matches!(
            events.iter().next(),
            Some(SceneEvent::Camera {
                command: crate::domain::CameraCommand::Zoom { factor },
                ..
            }) if *factor == 1.1
        ));

        // drain 取走后清空
        let drained: Vec<_> = events.drain().collect();
        assert_eq!(drained.len(), 3);
        assert!(events.is_empty());
    }

    /// **CG-d**：四类输入形状的 tagged JSON 往返（旧变体不受影响）。
    #[test]
    fn four_input_shapes_round_trip_through_json() {
        let cases = [
            SceneEvent::Pointer {
                input: PointerInput::Pressed {
                    scene_pos: Vec2::new(10.0, 20.0),
                    button: PointerButton::Primary,
                },
            },
            SceneEvent::Key {
                input: KeyInput::Down {
                    key: "Enter".to_string(),
                    modifiers: KeyModifiers {
                        shift: true,
                        ..KeyModifiers::default()
                    },
                },
            },
            SceneEvent::Text {
                input: TextInput::Commit {
                    text: "你好".to_string(),
                },
            },
            SceneEvent::Text {
                input: TextInput::Anchor {
                    anchor: TextAnchorRef {
                        target: "editor".to_string(),
                        edge: TextAnchorEdge::End,
                    },
                },
            },
            SceneEvent::Command {
                input: CommandInput::SetValue {
                    name: "panel_a.base".to_string(),
                    value: 0.75,
                },
            },
        ];
        for event in cases {
            let json = serde_json::to_string(&event).expect("可序列化");
            let back: SceneEvent = serde_json::from_str(&json).expect("可反序列化");
            assert_eq!(event, back, "四类形状必须可往返：{json}");
        }
    }

    /// **CK-d**：结构变化是输入流的一部分（稳定 id + 显式 op），可逐位往返。
    #[test]
    fn structure_delta_is_an_explicit_input() {
        let event = SceneEvent::StructureDelta {
            ops: vec![
                StructureOp::Mount {
                    id: WidgetId(30),
                    parent: None,
                },
                StructureOp::Mount {
                    id: WidgetId(31),
                    parent: Some(WidgetId(30)),
                },
                StructureOp::Reparent {
                    id: WidgetId(31),
                    parent: None,
                },
                StructureOp::Unmount { id: WidgetId(30) },
            ],
        };
        let json = serde_json::to_string(&event).expect("可序列化");
        let back: SceneEvent = serde_json::from_str(&json).expect("可反序列化");
        assert_eq!(event, back, "结构 delta 必须逐位往返：{json}");
        assert!(json.contains("structure_delta"), "类型标签进 JSON：{json}");
    }

    /// 老 payload（只有旧变体）照常解码 —— 新增变体是追加，不是改格式。
    #[test]
    fn legacy_payload_still_decodes() {
        let json = r#"{"type":"button","id":3}"#;
        let event: SceneEvent = serde_json::from_str(json).expect("老 payload 可解码");
        assert!(matches!(event, SceneEvent::Button { id } if id == ids::PLAY_PAUSE));
    }
}
