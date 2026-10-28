// graph.rs — 函数图像场景对象（M4-1）。
//
// 归位依据（M4 计划 §4.1）：函数采样循环原在 UI 层的 `SceneRecordSystem` 里
// （`(FunctionX2Y, Sampler1D, StrokeSolid, PhaseAnim)` → 逐点 `f(x + phase)` 折线）。
// 采样是**纯几何**，与 egui 无关 ⇒ 归场景对象层。
//
// 形态（数据纯、行为不纯）：
// - **数据部分**纯：`func`（理念层 `FunctionX2Y`）+ `sampler`（理念层 `Sampler1D` 区间 / 步长）；
// - **语义方法**纯：`sample_points` / `sample_path` / `extended_path` / `window_for`；
// - **装配行为**不纯（[`FunctionGraph::build`] / [`FunctionGraph::refresh`]）：依赖契约层
//   [`Scene`]，把**采样一次的扩展折线**写成对象几何，之后相位只移动 `CurveWindow`。
//
// 与迁移前的逐位一致性（可见段）：
// - 采样点 `(x, f(x + phase))`，`x` 自 `sampler.start` 起、步长 `sampler.step`、
//   止于 `x <= sampler.end`；
// - 折线**不闭合**；位置为**世界空间**（相机换算仍只发生在渲染录制的唯一换算点）；
// - 层级 z = 0（与内容同层）。
//
// **阶段 2（降格）**：几何只在装配时采样一次（区间两侧各扩一个 `span`），
// 逐帧刷新只写 `CurveWindow`（曲线索引 + t）；相位超出 `PHASE_MARGIN` 才回落重采样。
// 这条取舍由 E2 实测支撑（`e2_sampling_error_at_four_times_zoom`）。
//
// 为什么 `refresh` 不取相机：采样在世界空间完成，相机换算发生在渲染录制
// （`Path::to_screen`），故对象层不需要相机 —— 唯一换算点纪律的必然结果。
// 计划给的签名是 `refresh(scene, handle, camera)`，本实现按事实去掉 `camera`
// 并补上**相位**（函数图像逐帧变化的自变量，来自实体的 `PhaseAnim`）。

use ze_notion::{FunctionX2Y, Path2D, Sampler1D, Vec2};
use ze_render_ir::{CurveWindow, StrokeSolid};
use ze_scene_contract::{ObjectId, Scene};

use crate::primitives::Path;
use crate::zorder::ZOrder;

/// 函数图像内容层级（与其它内容同层）。
pub const CONTENT_Z: i32 = 0;

/// **阶段 2（降格）**：相位滑动的过采样边距 —— 每侧各一个区间宽。
///
/// 采样一次到 `[start − span, end + span]`，之后相位只在**窗口**里滑动
/// （`CurveWindow` 的单位是曲线索引 + t），不再重写 `Path`。
/// 相位超出这个边距才回落到重采样（显式，且只在极端相位下发生）。
pub const PHASE_MARGIN: f32 = 1.0;

/// 一元函数图像对象：`y = f(x + phase)` 在 `sampler` 区间上的折线。
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct FunctionGraph {
    /// 被绘制的函数（理念层函数元语）。
    pub func: FunctionX2Y<f32, f32>,
    /// 采样规格（区间 + 步长）。
    pub sampler: Sampler1D<f32>,
}

impl FunctionGraph {
    pub fn new(func: FunctionX2Y<f32, f32>, sampler: Sampler1D<f32>) -> Self {
        Self { func, sampler }
    }

    // ─── 语义方法（纯）────────────────────────────────────────────────

    /// 按相位采样出折线顶点（世界空间）。
    ///
    /// 相位偏移直接加在**自变量**上（`f(x + phase)`），这是迁移前
    /// `SceneRecordSystem` 的口径 —— 视觉上是曲线随时间水平滑动。
    pub fn sample_points(&self, phase: f32) -> Vec<Vec2> {
        self.points_between(self.sampler.start, self.sampler.end, phase)
    }

    /// 在任意区间上采样（区间外的扩展采样也走这一条，口径只有一份）。
    fn points_between(&self, from: f32, to: f32, phase: f32) -> Vec<Vec2> {
        let (step, start, end) = (self.sampler.step, self.sampler.start, self.sampler.end);
        let mut pts = Vec::new();
        if !(step > 0.0)
            || !step.is_finite()
            || !start.is_finite()
            || !end.is_finite()
            || end < start
            || !from.is_finite()
            || !to.is_finite()
            || to < from
        {
            return pts;
        }
        // 步数上限：区间有限 ⇒ `(to - from) / step` 有限。上限兜底防浮点漂移死循环。
        let max_n = ((to - from) / step).ceil() as usize + 2;
        let mut x = from;
        for _ in 0..max_n {
            if x > to {
                break;
            }
            pts.push(Vec2::new(x, (self.func.f)(x + phase)));
            x += step;
        }
        pts
    }

    /// 采样为**不闭合**折线。
    pub fn sample_path(&self, phase: f32) -> Path2D {
        Path2D::from_polyline(&self.sample_points(phase), false)
    }

    /// **阶段 2**：只采样一次的扩展折线（相位 0，区间两侧各扩一个 `span`）。
    ///
    /// 之后 `refresh` 只移动 [`CurveWindow`]，几何字节逐帧不变。
    pub fn extended_path(&self) -> Path2D {
        let span = self.sampler.end - self.sampler.start;
        if !span.is_finite() || span <= 0.0 {
            return self.sample_path(0.0);
        }
        let margin = span * PHASE_MARGIN;
        Path2D::from_polyline(
            &self.points_between(
                self.sampler.start - margin,
                self.sampler.end + margin,
                0.0,
            ),
            false,
        )
    }

    /// **阶段 2**：相位对应的窗口（曲线索引 + t）。
    ///
    /// `None` ⇒ 相位超出过采样边距（或采样规格退化），调用方必须回落重采样。
    pub fn window_for(&self, phase: f32) -> Option<[f32; 2]> {
        let (start, end, step) = (self.sampler.start, self.sampler.end, self.sampler.step);
        if !(step > 0.0) || !step.is_finite() || !start.is_finite() || !end.is_finite() {
            return None;
        }
        let span = end - start;
        if !span.is_finite() || span <= 0.0 || !phase.is_finite() {
            return None;
        }
        let margin = span * PHASE_MARGIN;
        if phase.abs() > margin {
            return None;
        }
        let lo = (margin + phase) / step;
        let hi = (margin + span + phase) / step;
        (hi > lo).then_some([lo, hi])
    }

    /// 折线的曲线数（`from_polyline` 每段一条 `LineTo`，与后端的曲线数同口径）。
    fn curves_of(path: &Path2D) -> f32 {
        path.commands
            .iter()
            .filter(|command| matches!(command, ze_notion::PathCommand::LineTo(_)))
            .count() as f32
    }

    // ─── 装配（不纯：依赖契约层 Scene）────────────────────────────────

    /// 创建函数图像对象（**采样一次的扩展折线** + 初相位窗口），返回句柄。
    ///
    /// 同一规格可装配多次（消耗 `&self`）。
    pub fn build<S: Scene>(&self, scene: &mut S, stroke: StrokeSolid) -> Option<ObjectId> {
        let path = self.extended_path();
        let window = self
            .window_for(0.0)
            .unwrap_or([0.0, Self::curves_of(&path).max(1.0)]);
        scene.spawn_dyn(vec![
            Box::new(self.clone()),
            Box::new(Path::new(path)),
            Box::new(CurveWindow {
                lo: window[0],
                hi: window[1],
            }),
            Box::new(stroke),
            Box::new(ZOrder::new(CONTENT_Z)),
        ])
    }

    /// 按相位刷新：**只移动窗口**（`CurveWindow`），不重写几何。
    ///
    /// 相位超出过采样边距（[`PHASE_MARGIN`]）时才回落为重采样 + 整窗；
    /// 返回 `false` = 句柄失效。
    pub fn refresh<S: Scene>(&self, scene: &mut S, handle: ObjectId, phase: f32) -> bool {
        match self.window_for(phase) {
            Some([lo, hi]) => scene.set(handle, CurveWindow { lo, hi }),
            None => {
                let path = self.sample_path(phase);
                let hi = Self::curves_of(&path).max(1.0);
                let written = scene.set(handle, Path::new(path));
                written & scene.set(handle, CurveWindow { lo: 0.0, hi })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(f: fn(f32) -> f32, start: f32, end: f32, step: f32) -> FunctionGraph {
        FunctionGraph::new(FunctionX2Y::new(f), Sampler1D::new(start, end, step))
    }

    /// 采样口径：`(x, f(x + phase))`，端点含 `end`（迁移前逐位一致）。
    #[test]
    fn sample_points_cover_closed_interval() {
        let g = graph(|x| 2.0 * x, -1.0, 1.0, 0.5);
        let pts = g.sample_points(0.0);
        assert_eq!(pts.len(), 5);
        assert_eq!(pts[0], Vec2::new(-1.0, -2.0));
        assert_eq!(pts[4], Vec2::new(1.0, 2.0));
    }

    /// 相位加在自变量上（水平滑动），不是加在函数值上。
    #[test]
    fn phase_shifts_along_x_axis() {
        let g = graph(|x| x, 0.0, 2.0, 1.0);
        let pts = g.sample_points(1.0);
        assert_eq!(pts[0], Vec2::new(0.0, 1.0));
        assert_eq!(pts[2], Vec2::new(2.0, 3.0));
    }

    /// 退化采样规格（非正步长 / 非有限区间）返回空，不死循环。
    #[test]
    fn degenerate_sampler_yields_nothing() {
        assert!(graph(|x| x, 0.0, 1.0, 0.0).sample_points(0.0).is_empty());
        assert!(
            graph(|x| x, 0.0, f32::INFINITY, 0.1)
                .sample_points(0.0)
                .is_empty()
        );
    }

    /// 折线不闭合。
    #[test]
    fn sample_path_is_open() {
        let g = graph(|x| x, 0.0, 1.0, 0.5);
        let path = g.sample_path(0.0);
        assert!(!matches!(
            path.commands.last(),
            Some(ze_notion::PathCommand::Close)
        ));
    }

    /// **阶段 2 / E2**：相位映射到窗口 —— 单位是**曲线索引 + t**，且随相位线性平移。
    ///
    /// 区间 `[0, 1]`、步长 `0.25` ⇒ `span/step = 4`：
    /// - 相位 0 ⇒ 窗口 `[4, 8]`（扩展折线从 `-1` 起采样，可见段正是 `[0,1]`）；
    /// - 相位 +0.5 ⇒ 窗口整体右移 `2` 条曲线；
    /// - 超出边距（`|phase| > span`）⇒ `None`（调用方必须重采样）。
    #[test]
    fn phase_maps_to_a_sliding_window() {
        let g = graph(|x| x, 0.0, 1.0, 0.25);
        let at_zero = g.window_for(0.0).expect("相位 0 在边距内");
        assert_eq!(at_zero, [4.0, 8.0]);
        let shifted = g.window_for(0.5).expect("相位 0.5 在边距内");
        assert_eq!(shifted, [6.0, 10.0]);
        assert!(g.window_for(1.01).is_none(), "超出过采样边距必须显式回落");
        assert!(g.window_for(f32::NAN).is_none(), "非有限相位必须回落");
    }

    /// **E2 实测**：折线采样的最大弦高误差（世界单位）与"4× 放大后是否可见"。
    ///
    /// 判据来自计划：放大到 4× 时边缘**无可见台阶**。台阶 = 折线弦与真实曲线的最大
    /// 偏差（弦高）折算到像素；`< 0.5 px` 视为不可见（半像素）。
    ///
    /// 采样密度由 `step` 固定、**不随缩放变化**，所以这条实验的结论是
    /// "在给定 step 下，放大到哪个倍率会开始露台阶"——不是"GPU 求值一定更好"。
    #[test]
    fn e2_sampling_error_at_four_times_zoom() {
        // 代表用例：sin 在 [0, 6.28] 上按 120 段采样（demo 常用密度量级）。
        let span = std::f32::consts::TAU;
        let step = span / 120.0;
        let g = graph(|x| x.sin(), 0.0, span, step);
        let points = g.sample_points(0.0);
        assert!(points.len() >= 120);
        let mut max_sagitta = 0.0f32;
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let mid_x = 0.5 * (a.x + b.x);
            let chord_y = 0.5 * (a.y + b.y);
            max_sagitta = max_sagitta.max((mid_x.sin() - chord_y).abs());
        }
        // 4×（4 px / 世界单位）下的偏差：
        let at_four = max_sagitta * 4.0;
        assert!(
            at_four < 0.5,
            "4× 放大时弦高 {at_four} px 已可见（> 0.5 px），E2 结论应改判 B"
        );
        // 露台阶的临界倍率：0.5 px 对应的 px / 世界单位。
        let threshold = 0.5 / max_sagitta.max(f32::EPSILON);
        assert!(
            threshold > 4.0,
            "临界倍率 {threshold} px/单位（≈ {:.0}×）应远高于 4×",
            threshold / 4.0
        );
    }
}
