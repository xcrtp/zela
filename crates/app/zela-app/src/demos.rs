//! 内置作品清单（GUI 的"作品"下拉；默认加载**全部** demo）。
//!
//! # 唯一来源
//!
//! 条目与 `scripts/demo/manifest.txt` **同序同脚本**：manifest 负责构建期断言
//! （`cargo verify --step demo` 逐条编译并校验 payload hash），这里只把同样的脚本
//! `include_str!` 编进二进制 —— 这样 GUI 换机器、换工作目录也能直接播。
//!
//! 新增作品时**两处都要改**（manifest 的 hash 由 `cargo verify --step demo` 给出）。
//!
//! # 语言
//!
//! `surface_matrix.js` 只在 `js` feature 打开时列出：关掉时 broker 里没有 `js` 节点，
//! 列出来只会得到一条"适配器未启用"的报错。

use crate::ScriptLanguage;

/// 一件可播放的内置作品。
pub struct Demo {
    /// 输出文件名（`www/<id>.zsb`），与 manifest 第一列一致。
    pub id: &'static str,
    /// 页面 / 下拉显示名。
    pub title: &'static str,
    /// 脚本文件名（诊断用；`scripts/` 下）。
    pub script: &'static str,
    /// 编进二进制的脚本正文。
    pub source: &'static str,
    pub language: ScriptLanguage,
}

macro_rules! lua {
    ($id:literal, $title:literal, $script:literal) => {
        Demo {
            id: $id,
            title: $title,
            script: $script,
            source: include_str!(concat!("../scripts/", $script)),
            language: ScriptLanguage::Lua,
        }
    };
}

/// 全部内置 demo（顺序 = `scripts/demo/manifest.txt`）。
pub static DEMOS: &[Demo] = &[
    lua!("fourier", "傅里叶级数展开", "fourier.lua"),
    lua!("storyboard", "分镜演示", "storyboard.lua"),
    lua!("demo3d", "3D 场景", "demo3d.lua"),
    lua!("fusion", "融合演示(双面+透明)", "fusion.lua"),
    lua!("pbr_materials", "PBR材质画廊", "pbr_materials.lua"),
    lua!("pbr_lights", "PBR光照实验室", "pbr_lights.lua"),
    lua!("pbr_scene", "PBR材质秀", "pbr_scene.lua"),
    lua!("pendulum", "钟摆动画", "pendulum.lua"),
    lua!("orbit", "公转行星", "orbit.lua"),
    lua!("spring", "弹跳小球", "spring.lua"),
    lua!("surface_matrix", "Surface能力矩阵", "surface_matrix.lua"),
    #[cfg(feature = "js")]
    Demo {
        id: "surface_matrix_js",
        title: "Surface能力矩阵(JS)",
        script: "surface_matrix.js",
        source: include_str!("../scripts/surface_matrix.js"),
        language: ScriptLanguage::JavaScript,
    },
    lua!("atlas", "2D纹理进3D", "atlas.lua"),
    lua!("vector_paths", "矢量后端作品", "vector_paths.lua"),
    lua!("capability_gaps", "能力缺口补齐", "capability_gaps.lua"),
    lua!("shaded_meshes", "3D网格着色栈", "shaded_meshes.lua"),
    lua!("cap_gradient", "渐变(2/3stop)", "cap_gradient.lua"),
    lua!("cap_dash", "虚线", "cap_dash.lua"),
    lua!("cap_second", "双paint", "cap_second.lua"),
    lua!("cap_background", "背景描边", "cap_background.lua"),
    lua!("cap_window", "曲线窗口", "cap_window.lua"),
    lua!("cap_mix", "2D/3D混排", "cap_mix.lua"),
    lua!("expr_demo", "表达式驱动", "expr_demo.lua"),
    lua!("cap_channels2d", "2D字段通道", "cap_channels2d.lua"),
    lua!("cap_channels3d", "3D字段通道", "cap_channels3d.lua"),
    lua!("expr_read", "通道读端与值分支", "expr_read.lua"),
    lua!("sine_trace", "小球画正弦", "sine_trace.lua"),
    lua!("cap_codegen", "着色codegen", "cap_codegen.lua"),
    lua!("cap_stroke_width", "线宽通道", "cap_stroke_width.lua"),
    lua!("cap_parent_inherit", "Parent继承", "cap_parent_inherit.lua"),
    lua!("cap_plot", "函数图像", "cap_plot.lua"),
    lua!("cap_path_follow", "路径跟随", "cap_path_follow.lua"),
    lua!("cap_text_group", "文字组", "cap_text_group.lua"),
    lua!("cap_sequence", "时序", "cap_sequence.lua"),
];

/// 按 id 找作品（找不到 ⇒ `None`，调用方决定回退）。
pub fn demo(id: &str) -> Option<&'static Demo> {
    DEMOS.iter().find(|demo| demo.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// manifest 的一行：`id  title（可含空白）  script  hash`。
    fn rows() -> Vec<(String, String, String)> {
        include_str!("../scripts/demo/manifest.txt")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                let cells: Vec<&str> = line.split_whitespace().collect();
                let (id, script) = (cells[0], cells[cells.len() - 2]);
                (
                    id.to_string(),
                    cells[1..cells.len() - 2].join(" "),
                    script.to_string(),
                )
            })
            .collect()
    }

    /// **唯一来源**：本表必须与 `scripts/demo/manifest.txt` 同序、同 id / 标题 / 脚本。
    ///
    /// 证伪：往 DEMOS 里加一件 manifest 没有的作品（或改了脚本名）⇒ 本用例失败。
    /// `surface_matrix.js` 只在 `js` feature 下列入 ⇒ 两侧都按当前 feature 过滤。
    #[test]
    fn demos_match_the_manifest() {
        let expected: Vec<(String, String, String)> = rows();
        assert!(!expected.is_empty(), "manifest 解析为空");

        #[cfg(not(feature = "js"))]
        let expected: Vec<(String, String, String)> = expected
            .into_iter()
            .filter(|(_, _, script)| !script.ends_with(".js"))
            .collect();

        let actual: Vec<(String, String, String)> = DEMOS
            .iter()
            .map(|demo| {
                (
                    demo.id.to_string(),
                    demo.title.to_string(),
                    demo.script.to_string(),
                )
            })
            .collect();
        assert_eq!(
            actual, expected,
            "DEMOS 与 manifest 不同源（顺序 / id / 标题 / 脚本）"
        );
    }
}
