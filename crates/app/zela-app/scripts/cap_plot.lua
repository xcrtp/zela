-- cap_plot.lua — 动画层阶段 1 能力展示：函数图像在装配期采样成折线
--
-- 判据（计划 §阶段 1）：
--   1. 左侧抛物线是**手写**折线（脚本逐点算好），右侧两条曲线是 `expr.plot`
--      一次采样得到的 —— 顶点由同一套 `Path2D::from_polyline` 生成
--      （左侧 121 点，两侧定义域与采样点数一致 ⇒ 逐位相同）；
--   2. `samples` 是**装配期固定**：脚本结束后一次算完，运行期不重采样 ⇒ 只渲 1 帧；
--   3. 表达式是闭的：自变量只有 `plot.x`（逐元素通道），脚本里没有 lambda。
--
-- 能力使用：`expr.plot`（Plot 采样）+ `expr.plot_x`（逐元素自变量）+ `Polyline`（对照组）。

local X0, X1 = -4.4, -0.8 -- 抛物线定义域
local BASE = -1.2          -- 抛物线常数项
local K = 0.6              -- 抛物线二次项系数
local SAMPLES = 121        -- 两个对照组共用同一采样点数（端点含在内）

scene.spawn({
    Camera2D { scale = 60.0 },
})

-- ── 对照组：手写折线（唯一的逐点循环，纯数据准备）──────────────────────
local points = {}
for i = 0, SAMPLES - 1 do
    local x = X0 + (X1 - X0) * (i / (SAMPLES - 1))
    -- 结合顺序与表达式一致（先 x·x，再乘系数，最后加常数）⇒ 两侧逐位相同
    points[#points + 1] = scene.vec2(x, K * (x * x) + BASE)
end
scene.spawn({
    Polyline { points = points, closed = false },
    Stroke { width = 3.0, color = scene.rgba(120, 255, 160, 255) },
})

-- ── 实验组 1：同一条抛物线，改由 `expr.plot` 采样（脚本里没有循环）────────
local parabola = scene.spawn({
    Stroke { width = 3.0, color = scene.rgba(120, 255, 160, 255) },
})
expr.plot {
    object = parabola,
    lo = X0,
    hi = X1,
    samples = SAMPLES,
    -- y = 0.6·x² − 1.2，自变量是 `plot.x`（第 i 个采样点由宿主给值）
    y = expr.add {
        a = expr.scale { k = K, of = expr.mul { a = expr.plot_x(), b = expr.plot_x() } },
        b = expr.pure(BASE),
    },
}

-- ── 实验组 2：正弦叠加（右半屏）——同一条采样机制，函数换成 sin ──────────
local wave = scene.spawn({
    Stroke { width = 3.0, color = scene.rgba(255, 200, 120, 255) },
})
expr.plot {
    object = wave,
    lo = 0.2,
    hi = 4.4,
    samples = 241,
    y = expr.add {
        a = expr.scale {
            k = 1.4,
            of = expr.sin { of = expr.scale { k = 2.2, of = expr.plot_x() } },
        },
        b = expr.pure(0.3),
    },
}
