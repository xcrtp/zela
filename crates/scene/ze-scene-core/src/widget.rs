//! **CL-R**：控件的**声明**进产物，**命令列表**是派生态资源。
//!
//! # 与 CL 的差别（这是本批的「切了没有」）
//!
//! CK-g 在这里放过一个 `Widget2D` **struct**（声明 + `draw` 方法 + `Theme2D` 颜色表）。
//! 它和 `ze-interact::widget::Widget2D`（trait）同名，靠门禁只扫 `pub struct/enum`
//! 才蒙混过关；而且它自己画自己，跟表达层的 `WidgetRuntime` 是两套。
//!
//! 现在：
//!
//! ```text
//! 声明   ze_interact::widget::WidgetDecl  ← 组件登记表里的那一份（serde 进产物）
//! 运行   WorldHud 的 WidgetRuntime        （dyn，不进产物）
//! 命令   WidgetCommands                   （本帧 Vec<DrawCommand>，不进产物）
//! ```
//!
//! 渲染器消费 [`WidgetCommands`]：**20 行 HUD = 20 个实体**（每行一个声明实体），
//! 不再是「轨道 Rect + N 个字形 Path + 滑块 Circle」几百个实体。
//! `Shape2D` 一个变体都没改。

use ze_render_ir::DrawCommand;

/// 控件声明（组件登记表的类型就是这个；只做转出，定义在 `ze-interact`）。
pub use ze_interact::widget::WidgetDecl;

/// 本帧自研面板的绘制命令（**派生态**资源：每帧重算，不进产物）。
#[derive(Debug, Clone, Default)]
pub struct WidgetCommands {
    /// 本帧使用的主题 id（进 `Diagnostics.environment`）。
    pub theme: String,
    /// **世界坐标**下的命令（渲染层用相机仿射落到屏幕，唯一换算点仍在渲染层）。
    pub commands: Vec<DrawCommand>,
}

impl WidgetCommands {
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}
