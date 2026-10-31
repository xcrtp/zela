// paint.rs — 2D **绘制参数**（PE Step 1：由矢量后端 crate 下沉到 IR）。
//
// # 为什么下沉
//
// 这些类型原先只在 `zela-render-vector::api` 里，而场景层（`ze-scene-*`）**不依赖**
// `zela-render-vector` ⇒ 后端已实现的能力在场景层**没有入口**，不是"没人用"。
// 矢量 crate → IR 是单向依赖，故类型的唯一来源放在 IR，后端与场景层共用一份。
//
// # 语义锚点（不得由上层"包装形状"改写）
//
// - 渐变 = 两色以上沿一条**端点定义的轴**均匀分布；stops 走 brush rows，
//   `paint.wgsl` 的 `stops(first, second, count, m, t)` 原生支持多 stop；
// - `dash` 的单位是**曲线参数 u**（1 = 一整条曲线），不是屏幕像素；
// - 第二 paint 是 tween 的另一端，`mix = 0` 全用第一套、`1` 全用第二套；
// - 渐变轴端点是**场景空间**（模型变换之后）坐标 —— 上游 `paint()` 直接投影它们，
//   不乘记录自己的模型矩阵。

use serde::{Deserialize, Serialize};

/// 渐变：[`stops`](Self::stops) 沿 `from → to` 均匀分布。
///
/// **坐标口径**：`from` / `to` 是**场景空间**（模型变换之后）的两端。2D 场景的
/// `Style2D` 里存的是**屏幕空间**端点，由 `zela-render-vector::scene` 在映射时换算。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    /// 至少两个色标（straight RGBA，0..1）。
    pub stops: Vec<[f32; 4]>,
    pub from: [f32; 3],
    pub to: [f32; 3],
}

/// 虚线：三项都是**曲线参数 u**（1 = 一整条曲线），不是屏幕像素。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Dash {
    /// 周期（u）；必须 > 0。
    pub period: f32,
    /// 每个周期里显示的比例（0..1；1 = 实线）。
    pub duty: f32,
    /// 相位（u）。
    pub phase: f32,
}

/// 第二套 paint（tween 的另一端）：`mix` = 0 全用第一套、1 全用第二套。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SecondPaint {
    pub fill: [f32; 4],
    pub stroke: [f32; 4],
    pub background: [f32; 4],
    pub mix: f32,
}

/// 背景描边（第三色）：画在填充之下，宽度是场景单位。
///
/// 作为**场景组件**存在时，"挂上 = 开启"（缺席 = 不画）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BackgroundStroke {
    pub color: [f32; 4],
    pub width: f32,
}

/// 部分路径窗口：`[lo, hi]`，单位是**曲线索引 + t**（不是像素）。
///
/// 作为**场景组件**存在时，"挂上 = 开启"（缺席 = 整条路径）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurveWindow {
    pub lo: f32,
    pub hi: f32,
}

/// 2D 绘制的可选参数：**全部默认 ⇒ 当前行为逐位不变**。
///
/// 每个字段都对应渲染后端 `Record` 里一个**已实现**的槽位（PD §4.1）：
/// `background`/`background_width` → `Record.background` + `params[3]`；
/// `second` → `*2` 三色 + `dash[3]`；`dash` → `dash[0..3]`；
/// `curve_window` → `params[0..2]`；`gradient` → `fill_rows`/`stroke_rows` + `gradient_a/b`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PathOptions {
    /// 背景描边（第三色）；`None` ⇒ 不画。
    pub background: Option<[f32; 4]>,
    /// 背景描边的宽度（场景单位；`background = None` 时忽略）。
    pub background_width: f32,
    /// 第二套 paint 与混合进度。
    pub second: Option<SecondPaint>,
    /// 虚线。
    pub dash: Option<Dash>,
    /// 只显示曲线的 `[lo, hi]` 参数窗口（曲线索引 + t；`None` = 整条）。
    pub curve_window: Option<[f32; 2]>,
    /// 填充（或描边）的渐变。
    pub gradient: Option<Gradient>,
    /// 屏幕空间叠加（PF Step 2）...
    #[serde(default)]
    pub overlay: bool,
    /// **阶段 2（着色 codegen）**：填充色由一段生成的 WGSL 在 GPU 上算
    /// （`None` = 普通填充）。装饰性：不影响几何 / 状态，也不进结构指纹。
    #[serde(default)]
    pub shader: Option<ShaderPaint>,
}

/// **阶段 2（着色 codegen）**：由表达式生成的一段 WGSL。
///
/// 生成的 `eval` 返回 `vec3<f32>`；本版**不含参数**（常量着色），逐帧参数留给后续。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderPaint {
    /// 生成的 WGSL 纯函数（含 `struct Params` 与 `fn eval`）。
    pub source: String,
    /// **外部注入的参数表**：由生成器的参数表按 **WGSL uniform 布局**推导，
    /// 与具体通道无关（标量 / 向量 / 矩阵同一套规则）。
    #[serde(default)]
    pub params: Vec<ShaderParam>,
    /// `Params` 的 uniform 字节数（16 的倍数；无参数时为 0）。
    #[serde(default)]
    pub size: u32,
    /// **逐片元**：表达式读了 `zela.shader.fragment.*`（像素位置 / 框内坐标），
    /// 颜色随像素变 ⇒ 后端不能把它折成每对象一个颜色（走逐像素通道）。
    #[serde(default)]
    pub fragment: bool,
}

/// 一个外部注入参数：通道名 + 槽类型 + 在 `Params` 里的字节偏移。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderParam {
    /// 通道的限定名（`time`、`zela.scene.Circle.radius` …），宿主按它找供给者。
    pub channel: String,
    pub kind: ShaderParamKind,
    /// 相对 `Params` 起点的字节偏移（由 std140 对齐规则算出）。
    pub offset: u32,
}

/// 参数槽类型（只影响 uniform 的对齐与写入宽度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShaderParamKind {
    F32,
    I32,
    Vec2,
    Vec3,
    /// **BT-1**：四维参数（`rgba`）。
    Vec4,
    Mat4,
}

impl ShaderParamKind {
    /// WGSL uniform 的对齐（std140）。
    pub fn align(self) -> u32 {
        match self {
            Self::F32 | Self::I32 => 4,
            Self::Vec2 => 8,
            Self::Vec3 | Self::Vec4 | Self::Mat4 => 16,
        }
    }

    /// WGSL uniform 的大小。
    pub fn size(self) -> u32 {
        match self {
            Self::F32 | Self::I32 => 4,
            Self::Vec2 => 8,
            Self::Vec3 => 12,
            Self::Vec4 => 16,
            Self::Mat4 => 64,
        }
    }
}

impl PathOptions {
    /// 渐变。
    pub const FEATURE_GRADIENT: u8 = 1 << 0;
    /// 虚线。
    pub const FEATURE_DASH: u8 = 1 << 1;
    /// 第二 paint（tween）。
    pub const FEATURE_SECOND: u8 = 1 << 2;
    /// 背景描边。
    pub const FEATURE_BACKGROUND: u8 = 1 << 3;
    /// 曲线窗口。
    pub const FEATURE_WINDOW: u8 = 1 << 4;
    /// **阶段 2**：着色 codegen（颜色由生成的 WGSL 在 GPU 上算）。
    pub const FEATURE_CODEGEN: u8 = 1 << 5;

    /// 本组参数用到了哪些**只有矢量后端支持**的特性（位掩码）。
    ///
    /// 用途：路由选到 wgpu 时**必须**在决策理由里写明"已忽略"，不得静默丢效果
    /// （PE Step 3，与 B12「空理由 = 静默决策 = 失败」同口径）。
    pub fn feature_bits(&self) -> u8 {
        let mut bits = 0;
        if self.gradient.is_some() {
            bits |= Self::FEATURE_GRADIENT;
        }
        if self.dash.is_some() {
            bits |= Self::FEATURE_DASH;
        }
        if self.second.is_some() {
            bits |= Self::FEATURE_SECOND;
        }
        if self.background.is_some() {
            bits |= Self::FEATURE_BACKGROUND;
        }
        if self.curve_window.is_some() {
            bits |= Self::FEATURE_WINDOW;
        }
        if self.shader.is_some() {
            bits |= Self::FEATURE_CODEGEN;
        }
        bits
    }

    /// 把位掩码展开成人可读的名字（顺序固定 ⇒ 同一输入同一字符串）。
    pub fn feature_names(bits: u8) -> String {
        const TABLE: [(u8, &str); 6] = [
            (PathOptions::FEATURE_GRADIENT, "渐变"),
            (PathOptions::FEATURE_DASH, "虚线"),
            (PathOptions::FEATURE_SECOND, "双 paint"),
            (PathOptions::FEATURE_BACKGROUND, "背景描边"),
            (PathOptions::FEATURE_WINDOW, "曲线窗口"),
            (PathOptions::FEATURE_CODEGEN, "着色 codegen"),
        ];
        let names: Vec<&str> = TABLE
            .iter()
            .filter(|(bit, _)| bits & bit != 0)
            .map(|(_, name)| *name)
            .collect();
        if names.is_empty() {
            "无".to_string()
        } else {
            names.join(" / ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient() -> Gradient {
        Gradient {
            stops: vec![[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]],
            from: [0.0; 3],
            to: [1.0, 0.0, 0.0],
        }
    }

    /// 默认参数 = "什么都没用" ⇒ 位掩码为 0（wgpu 不需要任何忽略说明）。
    #[test]
    fn default_paint_has_no_feature_bits() {
        assert_eq!(PathOptions::default().feature_bits(), 0);
        assert_eq!(PathOptions::feature_names(0), "无");
    }

    /// **BT-1**：四维参数的 std140 对齐 / 大小（与 `Vec3` 同对齐、比它多 4 字节）。
    #[test]
    fn vec4_param_is_std140_aligned() {
        assert_eq!(ShaderParamKind::Vec4.align(), 16);
        assert_eq!(ShaderParamKind::Vec4.size(), 16);
        assert_eq!(ShaderParamKind::Vec3.align(), 16);
        assert_eq!(ShaderParamKind::Vec3.size(), 12);
    }

    /// 每个字段各自对应一位，且名字顺序固定（可 diff）。
    #[test]
    fn feature_bits_are_independent_and_named_in_a_fixed_order() {
        let gradient_only = PathOptions {
            gradient: Some(gradient()),
            ..Default::default()
        };
        assert_eq!(gradient_only.feature_bits(), PathOptions::FEATURE_GRADIENT);
        assert_eq!(
            PathOptions::feature_names(gradient_only.feature_bits()),
            "渐变"
        );

        // **阶段 2**：着色 codegen 是 vector-only 能力 ⇒ 路由到 wgpu 时必须报"已忽略"。
        let shader_only = PathOptions {
            shader: Some(ShaderPaint {
                source: "fn eval() {}".to_string(),
                params: Vec::new(),
                size: 0,
                fragment: false,
            }),
            ..Default::default()
        };
        assert_eq!(shader_only.feature_bits(), PathOptions::FEATURE_CODEGEN);
        assert_eq!(
            PathOptions::feature_names(shader_only.feature_bits()),
            "着色 codegen"
        );

        let all = PathOptions {
            gradient: Some(gradient()),
            dash: Some(Dash {
                period: 0.1,
                duty: 0.5,
                phase: 0.0,
            }),
            second: Some(SecondPaint {
                fill: [1.0; 4],
                stroke: [1.0; 4],
                background: [1.0; 4],
                mix: 0.5,
            }),
            background: Some([0.0, 0.0, 0.0, 1.0]),
            background_width: 2.0,
            curve_window: Some([0.0, 1.0]),
            overlay: false,
            shader: None,
        };
        assert_eq!(all.feature_bits(), 0b1_1111);
        assert_eq!(
            PathOptions::feature_names(all.feature_bits()),
            "渐变 / 虚线 / 双 paint / 背景描边 / 曲线窗口"
        );
    }

    /// serde 往返（组件要进 ZSB 持久化）。
    #[test]
    fn paint_round_trips_through_json() {
        let paint = PathOptions {
            gradient: Some(gradient()),
            dash: Some(Dash {
                period: 0.25,
                duty: 0.5,
                phase: 0.1,
            }),
            ..Default::default()
        };
        let text = serde_json::to_string(&paint).expect("序列化");
        let back: PathOptions = serde_json::from_str(&text).expect("反序列化");
        assert_eq!(back, paint);
    }
}
