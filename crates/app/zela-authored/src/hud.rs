//! **CG-f 作品**：三组自研控件（control / time / param）全部是场景图元。
//!
//! # 它证明什么
//!
//! - 控件的外观是 `Rect`（轨道 / 底板）+ `Circle`（滑块）+ `Path`（标签轮廓），
//!   由渲染器画，**不是 egui 控件**；
//! - 标签走 CG-a/b 的直排文字引擎（ttf 度量 + ttf 轮廓 + `TextRun`）——
//!   这是新文字路径的第一个生产消费者；
//! - 交互走 CG-d 的 `ControlSet`（hover / focus / capture），几何与语义由
//!   [`zela_scene_ecs::Hud`] 供给（Frame）。
//!
//! 数值回写仍走既有入口：`DirectorState`（播放 / 速度 / seek）、`UiConfig`
//! （网格 / 曲面 / 滤镜）、参数注册表地址（`params::apply`）。

use std::sync::Arc;

use ze_interact::{ControlSpec, LayoutId};
use ze_notion::{Transform2D, Vec2};
use ze_prerender::text::DirectTextEngine;
use zela_scene_ecs::hud::{HudMetrics, HudState};
use zela_scene_ecs::{Scene, build_hud};

/// 面板原点（世界坐标；与默认相机的取景范围匹配）。
///
/// 放在两块 CE-e 面板（`panels` 作品：`y ∈ [-4, -0.8]`）**上方**，面板之间不重叠。
pub const HUD_ORIGIN: Vec2 = Vec2::new(-9.0, -0.2);
/// 面板宽度（世界单位）。
pub const HUD_WIDTH: f32 = 5.0;

/// **CI-b**：单选组的静态选项（`ControlKind` 保持 `Copy`）。
const ON_OFF: &[&str] = &["显示", "隐藏"];
const CAMERA_MODES: &[&str] = &["2D", "3D 轨道", "3D 第一人称"];
const CAMERA_DRIVE: &[&str] = &["分镜", "自由"];
const LAYER_ORDER: &[&str] = &["正常", "翻转"];

/// 四组控件的声明（顺序 = 行序，id 从 0 递增 ⇒ 布局确定）。
///
/// `param_sliders`：`(语义名 / 参数地址, 最小值, 最大值, 当前值)`。空 = 本场景没有
/// 可调参数（面板只剩 control + time 两组）。
pub fn hud_specs(param_sliders: &[(String, f64, f64, f64)]) -> Vec<ControlSpec> {
    let mut specs = Vec::new();
    let mut next = 0u32;
    let mut id = || {
        let id = LayoutId(next);
        next += 1;
        id
    };

    // ── 控制 ──
    specs.push(ControlSpec::button(id(), "show_grid"));
    specs.push(ControlSpec::button(id(), "show_field"));
    specs.push(ControlSpec::button(id(), "filter_gain"));
    specs.push(ControlSpec::button(id(), "filter_distortion"));
    specs.push(ControlSpec::button(id(), "filter_blur"));
    // **CI-b / CG-08**：图层与相机开关从宿主侧搬进场景图元（单选组描述）。
    specs.push(ControlSpec::radio_group(id(), "show_2d", ON_OFF, 0));
    specs.push(ControlSpec::radio_group(id(), "show_3d", ON_OFF, 0));
    specs.push(ControlSpec::radio_group(
        id(),
        "viewport_camera",
        CAMERA_MODES,
        0,
    ));
    specs.push(ControlSpec::radio_group(
        id(),
        "free_2d_camera",
        CAMERA_DRIVE,
        0,
    ));
    specs.push(ControlSpec::radio_group(
        id(),
        "flip_layer_order",
        LAYER_ORDER,
        0,
    ));

    // ── 时间 ──
    specs.push(ControlSpec::button(id(), "play_pause"));
    specs.push(ControlSpec::button(id(), "replay"));
    specs.push(ControlSpec::slider(id(), "speed", -3.0, 3.0, 1.0));
    specs.push(ControlSpec::slider(id(), "seek", -10.0, 30.0, 0.0));

    // ── 参数（地址即语义名；HUD System 用 `contains(':')` 分流到 params::apply）──
    for (address, min, max, value) in param_sliders {
        specs.push(ControlSpec::slider(
            id(),
            address.clone(),
            *min,
            *max,
            *value,
        ));
    }
    specs
}

/// 面板的世界坐标口径（比 CE-e 的面板更紧凑：20 行也要落进取景框）。
///
/// # 位置是**录制期派生量**（CL-R 删掉了相机耦合补丁）
///
/// 旧版这里写死了一条不等式（`padding + 21.4·row_height + 19·gap ≤ 8.8`），
/// 把「默认取景能看见 20 行」变成布局的硬前提 —— 那是宿主覆盖层时代的补丁。
/// 现在面板是场景内的声明实体：位置由 `HudMetrics` 给出，取景看清与否是**录制期**
/// 的事，不进产物、也不反向约束布局。
///
/// 仍保留的只有一条观感要求：`gap` 必须容得下标签文字（约 `label_font_size * 1.3`），
/// 否则标签会压到上一行的轨道。
pub fn hud_metrics() -> HudMetrics {
    HudMetrics {
        origin: HUD_ORIGIN,
        width: HUD_WIDTH,
        row_height: 0.22,
        gap: 0.195,
        padding: 0.10,
        label_font_size: 0.14,
        knob_radius: 0.09,
    }
}

/// 在**已有场景**里建面板：标签用直排文字引擎烘焙，失败时退化为无标签（不阻塞场景）。
///
/// `engine = None` ⇒ 本机没有可用字体：面板仍有轨道 / 滑块 / 命中（只是没有标签）——
/// 与 CE-e 的 `bake_label` 同一降级口径。
pub fn build(
    scene: &mut Scene,
    engine: Option<&Arc<DirectTextEngine>>,
    param_sliders: &[(String, f64, f64, f64)],
) -> HudState {
    let metrics = hud_metrics();
    let mut bake = |text: &str, anchor: Vec2, font_size: f32| -> Vec<ze_notion::Path2D> {
        let Some(engine) = engine else {
            return Vec::new();
        };
        match engine.layout(text, None, font_size, 0.0, ze_notion::Color32::WHITE) {
            Ok(output) => output
                .ir
                .items
                .into_iter()
                .map(|item| item.path.transformed(&Transform2D::translate(anchor)))
                .collect(),
            Err(error) => {
                eprintln!("[zela-authored] HUD 标签 `{text}` 烘焙失败：{error}");
                Vec::new()
            }
        }
    };
    HudState::new(build_hud(
        scene,
        hud_specs(param_sliders),
        metrics,
        &mut bake,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ze_interact::{ControlOutcome, Point};
    use ze_render_ir::LayerId;
    use zela_scene_ecs::domain::SceneEvent;

    fn engine() -> Option<Arc<DirectTextEngine>> {
        match DirectTextEngine::from_env() {
            Ok(engine) if !engine.db().families().is_empty() => Some(Arc::new(engine)),
            _ => {
                eprintln!("[zela-authored::hud] 跳过：本机没有可用字体");
                None
            }
        }
    }

    #[test]
    fn specs_cover_four_groups_in_declaration_order() {
        let specs = hud_specs(&[("1:curve:0000:base".to_string(), 0.0, 1.0, 0.5)]);
        assert_eq!(specs.len(), 15, "control 5 + radio 5 + time 4 + param 1");
        let names: Vec<&str> = specs.iter().map(|spec| spec.name.as_str()).collect();
        assert_eq!(names[0], "show_grid");
        assert_eq!(names[5], "show_2d");
        assert_eq!(names[9], "flip_layer_order");
        assert_eq!(names[10], "play_pause");
        assert_eq!(names[14], "1:curve:0000:base");
    }

    /// **真实口径**：面板的 `row_height` / `gap` / 字号必须容得下标签（标签画在行距里）。
    #[test]
    fn real_metrics_keep_labels_out_of_the_tracks() {
        let mut scene = Scene::new();
        let metrics = hud_metrics();
        let specs = hud_specs(&[]);
        let ids: Vec<LayoutId> = specs.iter().map(|spec| spec.id).collect();
        let mut anchors = Vec::new();
        let hud = zela_scene_ecs::hud::build(&mut scene, specs, metrics, |_, anchor, _| {
            anchors.push(anchor);
            Vec::new()
        });
        assert_eq!(anchors.len(), ids.len(), "每行一个标签");
        let text_height = metrics.label_font_size * 1.3;
        for (index, anchor) in anchors.iter().enumerate() {
            let (bottom, top) = (anchor.y, anchor.y + text_height);
            for id in &ids {
                let rect = hud.world_rect(*id).expect("有控件矩形");
                assert!(
                    !(bottom < rect.max.y && top > rect.min.y),
                    "第 {index} 行标签 [{bottom:.3}, {top:.3}] 压到轨道 [{:.3}, {:.3}]",
                    rect.min.y,
                    rect.max.y
                );
            }
        }
    }

    /// **可见 + 可拖**：滑块按下后写值，且滑块圆心跟着走（世界坐标）。
    #[test]
    fn dragging_a_slider_writes_value_and_moves_the_knob() {
        let Some(engine) = engine() else {
            return;
        };
        let mut scene = Scene::new();
        let state = build(&mut scene, Some(&engine), &[]);
        let mut hud = state.hud().expect("有 HUD").clone();

        // 找到 speed 那一行（单位坐标），按到 75%。
        let id = hud.id_by_name("speed").expect("有 speed");
        let frame = hud.frame();
        let rect = frame
            .geometry
            .iter()
            .find(|placed| placed.id == id)
            .expect("有几何")
            .rect;
        let metrics = hud_metrics();
        let unit = metrics.row_height / 20.0;
        let world = |p: Point| {
            Vec2::new(
                metrics.origin.x + p.x as f32 * unit,
                metrics.origin.y + p.y as f32 * unit,
            )
        };
        let press = world(Point::new(
            rect.min.x + rect.width() * 0.75,
            rect.center().y,
        ));
        let outcomes = hud.handle_world(SceneEvent::ViewportClick {
            layer: LayerId(0),
            scene_pos: press,
        });
        assert!(
            matches!(outcomes.first(), Some(ControlOutcome::ValueChanged { .. })),
            "按下必须产生 ValueChanged：{outcomes:?}"
        );
        let value = hud.value(id).expect("有值");
        assert!(
            (value - 1.5).abs() < 0.05,
            "speed 在 75% 处应约 1.5，实际 {value}"
        );
        // 圆钮已跟随（世界 x 落在轨道 75% 附近）—— 不再有需要逐帧搬的圆实体，
        // 位置由命令列表里的 `Shape2D::Circle` 直接给出。单选组也有圆钮，
        // 所以按行高挑出 speed 那一行的圆。
        hud.sync_from_set();
        let world_rect = hud.world_rect(id).expect("有世界矩形");
        let commands = hud.commands();
        let circle = commands
            .iter()
            .find_map(|command| match &command.shape {
                ze_render_ir::Shape2D::Circle { center, .. }
                    if center.y > world_rect.min.y && center.y < world_rect.max.y =>
                {
                    Some(*center)
                }
                _ => None,
            })
            .expect("有 speed 的滑块圆钮");
        let knob_x = circle.x;
        assert!(knob_x.is_finite());
        assert!(
            knob_x > world_rect.min.x && knob_x < world_rect.max.x,
            "圆心必须落在轨道内：{knob_x} ∉ ({}, {})",
            world_rect.min.x,
            world_rect.max.x
        );
        // 75% 处：圆心应明显偏右（> 半宽）。
        assert!(
            knob_x > world_rect.center().x,
            "圆心应偏右：{knob_x} vs {}",
            world_rect.center().x
        );
    }
}
