//! **CH-a**：窗口输入 —— `collect_events` 的纯函数形态。
//!
//! # 它切开了什么
//!
//! CG 的 headless 注入点是**场景事件**：Agent 自己把「点哪儿」换算成世界坐标，
//! `slider_point()` 甚至替它算好。被跳过的是从窗口到场景的那一整段：
//!
//! ```text
//! 窗口指针（逻辑像素）→ 视口判定 → 2D 图层选择 → 相机换算 → SceneEvent
//! ```
//!
//! 本模块把这段搬出 egui：
//!
//! ```text
//! 真人：eframe::InputState → WindowInput ─┐
//!                                        ├→ collect_events（本函数）→ Vec<SceneEvent>
//! Debug / RPC：tagged JSON → WindowInput ─┘
//! ```
//!
//! 于是「同一份计算，不是另一条测试专用路径」（CG-g 的原则）往上游推了一层。
//!
//! # 三条口径
//!
//! 1. **零 egui 类型**：输入是纯数据，换算只用 `Camera2D`（对象层）；
//! 2. **坐标是逻辑像素**：egui 给的就是逻辑点，`scale_factor` 只作为**环境参数**
//!    记录（进快照 / 输入流），不参与换算 —— 否则 2× DPI 上同一个坐标会落到不同控件；
//! 3. **环境显式**：窗口尺寸 / 缩放比 / 视口矩形都由调用方填，不隐式读系统。

use std::collections::BTreeSet;

use ze_notion::{Rect2D, Vec2};

use crate::domain::camera::CameraCommand;
use crate::domain::config::ViewportCamera;
use crate::domain::event::SceneEvent;
use crate::domain::view::{LayerId, LayerKind, SceneView};
use crate::object::camera::Camera2D;

/// 本批消费的按键（FPS 相机用）。
///
/// 只列**有消费者**的键：多列一个键就要多一份「为什么它在这里」的理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    W,
    A,
    S,
    D,
    Q,
    E,
}

/// 指针状态（逻辑像素；`position` 是**窗口坐标**，与视口矩形同一坐标系）。
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct PointerState {
    /// 指针当前位置（`None` = 从未有过位置）。
    pub position: Option<Vec2>,
    /// 指针是否**真的在窗口上**（egui 的 `hover_pos` 语义：离开窗口 ⇒ false）。
    ///
    /// 与 `position` 分开：`position` 保留最后一次位置（弹出 / 拾取要用），
    /// 而"是否在窗口内"决定相机命令是否生效 —— 混用会让指针离开后仍缩放画面。
    pub hovering: bool,
    /// 本帧位移。
    pub delta: Vec2,
    /// 本帧平滑滚动量（> 0 = 向上滚）。
    pub scroll_y: f32,
    pub primary_down: bool,
    pub secondary_down: bool,
    pub middle_down: bool,
    pub primary_pressed: bool,
    pub primary_released: bool,
}

/// 一帧的窗口输入（**纯数据**，可序列化 ⇒ 可进输入流 / RPC）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WindowInput {
    pub pointer: PointerState,
    /// 当前按下的键（本批只消费 [`Key`] 里的六个）。
    pub keys: BTreeSet<Key>,
    /// 文本输入中：键盘让位（不抢 WASD/QE）。
    pub typing: bool,
    /// 本帧时长（秒）：FPS 位移**按秒**，与帧率无关。
    pub stable_dt: f32,
    /// 窗口尺寸（逻辑像素）。
    pub window_size: Vec2,
    /// HiDPI 缩放比（环境参数；**不参与**换算，只进快照）。
    pub scale_factor: f32,
    /// 中央视口矩形（逻辑像素）。
    pub viewport: Rect2D,
}

impl Default for WindowInput {
    fn default() -> Self {
        Self {
            pointer: PointerState::default(),
            keys: BTreeSet::new(),
            typing: false,
            stable_dt: 0.0,
            window_size: Vec2::ZERO,
            scale_factor: 1.0,
            viewport: Rect2D::default(),
        }
    }
}

/// 换算所需的**场景侧上下文**（每帧从 [`SceneView`] 取，不进输入流）。
#[derive(Debug, Clone, Copy, Default)]
pub struct SceneInputContext {
    /// 2D 图层（图层 id + 本帧相机）。没有 2D 图层 ⇒ 拾取事件不上报。
    pub layer2d: Option<(LayerId, Camera2D)>,
    /// 3D 图层（3D 语义的相机命令用它）。
    pub layer3d: Option<LayerId>,
    /// 视口操作目标（K3）。
    pub target: ViewportCamera,
}

impl SceneInputContext {
    /// 从本帧视图声明取上下文（GUI 与 headless 共用）。
    pub fn from_view(view: &SceneView, target: ViewportCamera) -> Self {
        let layer2d = view.layers.iter().find_map(|layer| match &layer.kind {
            LayerKind::Viewport2D { camera } => Some((layer.id, *camera)),
            _ => None,
        });
        let layer3d = view
            .layers
            .iter()
            .find(|layer| matches!(layer.kind, LayerKind::Viewport3D { .. }))
            .map(|layer| layer.id);
        Self {
            layer2d,
            layer3d,
            target,
        }
    }
}

/// 每帧位移/旋转换算系数（交互手感，纯数值选择）。
const ORBIT_RAD_PER_PX: f32 = 0.01;
/// 滚轮 → Dolly 步长（世界单位/滚轮量）。
const DOLLY_PER_SCROLL: f32 = 0.02;
/// FPS 按键 → 每秒位移（世界单位/秒）。
const MOVE_PER_SECOND: f32 = 4.8;

/// **窗口输入 → 场景事件**（纯函数；同一份输入永远得到同一组事件）。
///
/// 语义与 `zela-egui::host` 原实现逐条一致：
/// - 相机命令按 [`ViewportCamera`] 分流；2D 的 Pan 先经相机换世界位移再取负；
/// - 拾取事件只在**视口内**且**有 2D 图层**时上报；
/// - FPS 的 WASD/QE 不要求指针在视口内，但文本输入时让位。
pub fn collect_events(input: &WindowInput, ctx: &SceneInputContext) -> Vec<SceneEvent> {
    // 纯 3D 作品没有 2D 图层：只要求"有可操作的图层"（优先 2D，其次 3D）。
    let layer = match (ctx.layer2d, ctx.layer3d) {
        (Some((id, _)), _) => id,
        (None, Some(id)) => id,
        (None, None) => return Vec::new(),
    };

    // 与 egui 的指针浮点坐标对齐：边界算"在视口内"（闭区间，保持既有行为）。
    let in_viewport = |p: Vec2| {
        p.x >= input.viewport.min.x
            && p.x <= input.viewport.max.x
            && p.y >= input.viewport.min.y
            && p.y <= input.viewport.max.y
    };

    let pointer = input.pointer;
    let inside = pointer.hovering && pointer.position.is_some_and(in_viewport);
    let mut events = Vec::new();

    match ctx.target {
        ViewportCamera::TwoD => {
            if inside && pointer.scroll_y != 0.0 {
                events.push(SceneEvent::Camera {
                    layer,
                    command: CameraCommand::Zoom {
                        factor: 1.0 + pointer.scroll_y * 0.001,
                    },
                });
            }
            if let Some((_, camera)) = ctx.layer2d {
                if inside && (pointer.secondary_down || pointer.middle_down)
                    && pointer.delta != Vec2::ZERO
                {
                    events.push(SceneEvent::Camera {
                        layer,
                        command: CameraCommand::Pan {
                            delta: -camera.screen_delta_to_world(pointer.delta),
                        },
                    });
                }
            }
        }
        ViewportCamera::ThreeDOrbit => {
            if inside && pointer.scroll_y != 0.0 {
                events.push(SceneEvent::Camera {
                    layer,
                    command: CameraCommand::Dolly {
                        delta: pointer.scroll_y * DOLLY_PER_SCROLL,
                    },
                });
            }
            let dragging =
                pointer.primary_down || pointer.secondary_down || pointer.middle_down;
            if inside && dragging && pointer.delta != Vec2::ZERO {
                events.push(SceneEvent::Camera {
                    layer,
                    command: CameraCommand::Orbit {
                        delta_yaw: -pointer.delta.x * ORBIT_RAD_PER_PX,
                        delta_pitch: -pointer.delta.y * ORBIT_RAD_PER_PX,
                    },
                });
            }
        }
        ViewportCamera::ThreeDFps => {
            let dragging =
                pointer.primary_down || pointer.secondary_down || pointer.middle_down;
            if inside && dragging && pointer.delta != Vec2::ZERO {
                events.push(SceneEvent::Camera {
                    layer,
                    command: CameraCommand::Look {
                        delta_yaw: pointer.delta.x * ORBIT_RAD_PER_PX,
                        delta_pitch: -pointer.delta.y * ORBIT_RAD_PER_PX,
                    },
                });
            }
            let axis = |neg: Key, pos: Key| {
                (input.keys.contains(&pos) as i32 - input.keys.contains(&neg) as i32) as f32
            };
            let delta = ze_notion::Vec3::new(
                axis(Key::A, Key::D),
                axis(Key::Q, Key::E),
                axis(Key::S, Key::W),
            ) * (MOVE_PER_SECOND * input.stable_dt);
            if !input.typing && delta != ze_notion::Vec3::ZERO {
                events.push(SceneEvent::Camera {
                    layer,
                    command: CameraCommand::Move { delta },
                });
            }
        }
    }

    // 拾取类事件：只在有 2D 相机时上报（`screen_to_world` 是 2D 语义）。
    if let (Some((_, camera)), Some(pos)) = (ctx.layer2d, pointer.position) {
        if in_viewport(pos) {
            let world = camera.screen_to_world(pos);
            events.push(SceneEvent::ViewportHover {
                layer,
                scene_pos: world,
            });
            if pointer.primary_pressed {
                events.push(SceneEvent::ViewportClick {
                    layer,
                    scene_pos: world,
                });
            }
            if pointer.primary_released {
                events.push(SceneEvent::ViewportRelease {
                    layer,
                    scene_pos: world,
                });
            }
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Camera2D {
        Camera2D {
            center: Vec2::new(10.0, 20.0),
            scale: 2.0,
            rect: Rect2D::from_min_size(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0)),
        }
    }

    fn viewport() -> Rect2D {
        Rect2D::from_min_size(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0))
    }

    fn input(pointer: PointerState) -> WindowInput {
        WindowInput {
            pointer,
            viewport: viewport(),
            window_size: Vec2::new(100.0, 100.0),
            ..WindowInput::default()
        }
    }

    fn ctx(target: ViewportCamera, layer2d: bool) -> SceneInputContext {
        SceneInputContext {
            layer2d: layer2d.then(|| (LayerId(7), camera())),
            layer3d: Some(LayerId(8)),
            target,
        }
    }

    /// 视口内的点按相机换算成场景坐标（半开与否不影响拾取的闭区间判定）。
    #[test]
    fn click_inside_viewport_maps_through_the_camera() {
        let pointer = PointerState {
            position: Some(Vec2::new(50.0, 50.0)),
            hovering: true,
            primary_pressed: true,
            ..PointerState::default()
        };
        let events = collect_events(&input(pointer), &ctx(ViewportCamera::TwoD, true));
        // 相机 (center 10,20 / scale 2)：屏幕中心 → 世界中心。
        assert!(events.contains(&SceneEvent::ViewportHover {
            layer: LayerId(7),
            scene_pos: Vec2::new(10.0, 20.0),
        }));
        assert!(events.contains(&SceneEvent::ViewportClick {
            layer: LayerId(7),
            scene_pos: Vec2::new(10.0, 20.0),
        }));
    }

    /// 视口外：拾取与相机命令都不产生（避免在面板上滚轮缩放画面）。
    #[test]
    fn outside_viewport_produces_nothing() {
        let pointer = PointerState {
            position: Some(Vec2::new(150.0, 50.0)),
            hovering: true,
            scroll_y: 3.0,
            primary_pressed: true,
            ..PointerState::default()
        };
        assert!(collect_events(&input(pointer), &ctx(ViewportCamera::TwoD, true)).is_empty());
    }

    /// 2D 平移：屏幕位移先经相机换世界再取负（拖拽方向 → 取景中心反向移动）。
    #[test]
    fn secondary_drag_pans_with_the_inverted_camera_delta() {
        let pointer = PointerState {
            position: Some(Vec2::new(50.0, 50.0)),
            hovering: true,
            delta: Vec2::new(6.0, -4.0),
            secondary_down: true,
            ..PointerState::default()
        };
        let events = collect_events(&input(pointer), &ctx(ViewportCamera::TwoD, true));
        assert!(events.contains(&SceneEvent::Camera {
            layer: LayerId(7),
            command: CameraCommand::Pan {
                // scale 2：世界位移 = (3, 2)，取负 ⇒ (-3, -2)。
                delta: Vec2::new(-3.0, -2.0),
            },
        }));
    }

    /// FPS：WASD 按**秒**位移，不要求指针在视口内；文本输入时让位。
    #[test]
    fn fps_keys_move_per_second_and_yield_to_typing() {
        let mut keys = BTreeSet::new();
        keys.insert(Key::W);
        let input = WindowInput {
            keys: keys.clone(),
            stable_dt: 0.5,
            pointer: PointerState {
                position: Some(Vec2::new(150.0, 150.0)),
                ..PointerState::default()
            },
            viewport: viewport(),
            ..WindowInput::default()
        };
        let events = collect_events(&input, &ctx(ViewportCamera::ThreeDFps, false));
        assert!(events.contains(&SceneEvent::Camera {
            layer: LayerId(8),
            command: CameraCommand::Move {
                delta: ze_notion::Vec3::new(0.0, 0.0, MOVE_PER_SECOND * 0.5),
            },
        }));

        let typing = WindowInput { typing: true, ..input };
        assert!(collect_events(&typing, &ctx(ViewportCamera::ThreeDFps, false)).is_empty());
    }

    /// 从视图声明取上下文：第一个 2D 图层 + 第一个 3D 图层。
    #[test]
    fn context_picks_the_first_layers_from_the_view() {
        let view = SceneView::default();
        let ctx = SceneInputContext::from_view(&view, ViewportCamera::TwoD);
        assert!(ctx.layer2d.is_none());
        assert!(ctx.layer3d.is_none());
        assert_eq!(ctx.target, ViewportCamera::TwoD);
    }
}

