//! **CK-g**：`Widget2D` —— 一等控件实体，绘制走 `Vec<DrawCommand>`。
//!
//! # 为什么控件是图元而不是一批图元
//!
//! 旧 HUD 每行拆成「轨道 Rect + N 个标签 Path（一个字形一个实体）+ 滑块 Circle」，
//! 20 行就是几百个实体。`Widget2D` 把**声明**收成一个实体，绘制是纯函数
//! [`Widget2D::draw`] → `Vec<DrawCommand>`；每条命令自带 `Style2D`，
//! 多色 / hover / 主题切换只改命令列表，实体数不变。
//!
//! # 不进产物
//!
//! `Widget2D` 是声明期组件（可进 payload）；`Vec<DrawCommand>` 是编译产物
//! （派生态、不进 payload、回放端重算）。`Shape2D` 一个变体都不改。
//!
//! # 主题只存 id
//!
//! [`Theme2D::id`] 是唯一进产物 / 诊断的部分；颜色由宿主解析。
//! `Diagnostics.environment.theme` 记录本帧用的是哪套主题。

use ze_notion::{Color32, Path2D, Rect2D, Vec2};
use ze_render_ir::{DrawCommand, Shape2D, Style2D, ZOrder};

/// 控件的一等实体（声明）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Widget2D {
    /// 控件声明期 id（与 `Frame::geometry` / 事件流的稳定键同口径）。
    pub control: u32,
    /// 角色（`slider` / `button` / `label` / `radio`）。
    pub role: String,
    /// 可读标签（读屏 / Agent 定位 / 绘制共用）。
    pub label: String,
    /// 主题 id（只存 id；颜色由 [`Theme2D`] 解析）。
    pub theme: String,
}

impl Widget2D {
    pub fn new(
        control: u32,
        role: impl Into<String>,
        label: impl Into<String>,
        theme: impl Into<String>,
    ) -> Self {
        Self {
            control,
            role: role.into(),
            label: label.into(),
            theme: theme.into(),
        }
    }

    /// 绘制声明 → 命令列表（纯函数；不持句柄、不做布局）。
    ///
    /// 顺序固定：轨道 → 标签字形 → 滑块。`z` 由调用方给（与场景层级同一套口径）。
    pub fn draw(&self, ctx: &WidgetDrawContext, theme: &Theme2D) -> Vec<DrawCommand> {
        let track_color = if ctx.hover { theme.hover } else { theme.track };
        let mut commands = vec![DrawCommand::new(
            ZOrder(ctx.z),
            Shape2D::Rect(ctx.track),
            Style2D {
                fill: Some(track_color),
                ..Style2D::default()
            },
        )];
        for glyph in &ctx.glyphs {
            commands.push(DrawCommand::new(
                ZOrder(ctx.z + 10),
                Shape2D::Path(glyph.clone()),
                Style2D {
                    fill: Some(theme.label),
                    ..Style2D::default()
                },
            ));
        }
        if let Some(center) = ctx.knob {
            commands.push(DrawCommand::new(
                ZOrder(ctx.z + 20),
                Shape2D::Circle {
                    center,
                    radius: ctx.knob_radius,
                },
                Style2D {
                    fill: Some(theme.knob),
                    ..Style2D::default()
                },
            ));
        }
        commands
    }
}

/// 一次绘制需要的几何（由布局 / 状态机给出，不是声明）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WidgetDrawContext {
    /// 轨道矩形（世界 / 布局坐标由调用方决定）。
    pub track: Rect2D,
    /// 滑块圆心（按钮 / 标签为 `None`）。
    pub knob: Option<Vec2>,
    pub knob_radius: f32,
    /// 指针是否悬停在本控件上（只改颜色，不改实体数）。
    pub hover: bool,
    /// 基础绘制层级（标签 +10、滑块 +20）。
    pub z: i32,
    /// 已烘焙的标签字形轮廓（一个字形一条）。
    pub glyphs: Vec<Path2D>,
}

/// 主题（颜色表；`id` 是唯一进产物 / 诊断的部分）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme2D {
    pub id: String,
    pub track: Color32,
    pub hover: Color32,
    pub label: Color32,
    pub knob: Color32,
}

impl Theme2D {
    /// 默认主题（与 HUD 既有配色逐位相同）。
    pub fn default_theme() -> Self {
        Self {
            id: "zela.default".to_string(),
            track: Color32::new(52, 54, 64, 255),
            hover: Color32::new(68, 71, 84, 255),
            label: Color32::new(210, 214, 226, 255),
            knob: Color32::new(228, 232, 244, 255),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> WidgetDrawContext {
        WidgetDrawContext {
            track: Rect2D::from_min_size(Vec2::new(0.0, 0.0), Vec2::new(100.0, 10.0)),
            knob: Some(Vec2::new(25.0, 5.0)),
            knob_radius: 4.0,
            hover: false,
            z: 100,
            glyphs: vec![Path2D::from_polyline(
                &[Vec2::ZERO, Vec2::new(1.0, 0.0)],
                false,
            )],
        }
    }

    /// 一条控件声明 ⇒ 固定条命令；每条命令自带样式（多色不靠共享 Style2D）。
    #[test]
    fn one_widget_draws_all_its_commands_with_per_command_style() {
        let widget = Widget2D::new(7, "slider", "speed", "zela.default");
        let theme = Theme2D::default_theme();
        let commands = widget.draw(&ctx(), &theme);
        assert_eq!(commands.len(), 3, "轨道 + 1 个字形 + 滑块");
        assert_eq!(commands[0].shape.kind_name(), "rect");
        assert_eq!(commands[1].shape.kind_name(), "path");
        assert_eq!(commands[2].shape.kind_name(), "circle");
        assert_eq!(commands[0].style.fill, Some(theme.track));
        assert_eq!(commands[1].style.fill, Some(theme.label));
        assert_eq!(commands[2].style.fill, Some(theme.knob));
        // 层级：轨道 → 标签 → 滑块。
        assert!(commands[0].z < commands[1].z && commands[1].z < commands[2].z);
    }

    /// hover 只改颜色、不改命令条数（实体数不变）。
    #[test]
    fn hover_changes_only_colors() {
        let widget = Widget2D::new(7, "button", "play", "zela.default");
        let theme = Theme2D::default_theme();
        let normal = widget.draw(&ctx(), &theme);
        let hovered = widget.draw(
            &WidgetDrawContext {
                hover: true,
                ..ctx()
            },
            &theme,
        );
        assert_eq!(normal.len(), hovered.len());
        assert_eq!(normal[0].style.fill, Some(theme.track));
        assert_eq!(hovered[0].style.fill, Some(theme.hover));
    }
}
