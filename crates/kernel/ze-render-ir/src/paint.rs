// paint.rs — 2D **绘制参数**（PE Step 1：由矢量后端 crate 下沉到 IR）。
//
// # 为什么下沉
//
// 这些类型原先只在 `zela-render-vector::api` 里，而场景层（`ze-scene-*`）**不依赖**
// `zela-render-vector` ⇒ 后端已实现的能力在场景层**没有入口**，不是"没人用"。
// 矢量 crate → IR 是单向依赖，故类型的唯一真源放在 IR，后端与场景层共用一份。
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
        bits
    }

    /// 把位掩码展开成人可读的名字（顺序固定 ⇒ 同一输入同一字符串）。
    pub fn feature_names(bits: u8) -> String {
        const TABLE: [(u8, &str); 5] = [
            (PathOptions::FEATURE_GRADIENT, "渐变"),
            (PathOptions::FEATURE_DASH, "虚线"),
            (PathOptions::FEATURE_SECOND, "双 paint"),
            (PathOptions::FEATURE_BACKGROUND, "背景描边"),
            (PathOptions::FEATURE_WINDOW, "曲线窗口"),
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

    /// 每个字段各自对应一位，且名字顺序固定（可 diff）。
    #[test]
    fn feature_bits_are_independent_and_named_in_a_fixed_order() {
        let gradient_only = PathOptions {
            gradient: Some(gradient()),
            ..Default::default()
        };
        assert_eq!(gradient_only.feature_bits(), PathOptions::FEATURE_GRADIENT);
        assert_eq!(PathOptions::feature_names(gradient_only.feature_bits()), "渐变");

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
