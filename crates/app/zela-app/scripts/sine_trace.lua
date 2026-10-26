-- sine_trace.lua — 小球随 Reveal 画出正弦曲线（**读端管道**驱动）。
--
-- 数据流是一条管道，没有任何逐段动画循环：
--
--   Curve.Ramp ──写──> Reveal.end ──读──> Reveal.current_pos ──写──> 小球 Circle.center
--
--   · `Reveal.end` 由一条 Ramp 常速推进（时间管道）；
--   · 小球不自己算曲线：它通过 `expr.read` 读曲线对象的 **`Reveal.current_pos`**
--     （只读派生通道 = 归一化弧长 `end` 处的笔尖位置，渲染裁剪同一口径）；
--   · 因此"小球在笔尖上"是**结构上成立**的，不依赖采样密度或插值巧合。
--
-- 脚本里唯一的循环是**几何采样**（生成正弦折线本身），与动画无关。
--
-- 能力使用：Reveal（S2 书写）+ `Reveal.end` / `Reveal.current_pos` /
-- `Circle.center` 通道 + `expr.read`（G 批读端）+ `Polyline`（静态坐标轴）。
-- 数值量化到 1e-4，保证跨平台逐位一致。

local TAU = 2.0 * math.pi

local PERIODS = 2.0     -- 画几个正弦周期
local AMP = 1.2         -- 振幅（世界单位）
local X0, X1 = -4.2, 4.2
local BASE_Y = 0.0      -- 中轴高度
local SAMPLES = 240     -- 折线点数（只需足够平滑；同步与密度无关）
local TOTAL_T = 6.0     -- 总时长（秒）

local function q(v)
    return math.floor(v * 10000.0 + 0.5) / 10000.0
end

local function rgba(r, g, b, a)
    return Color { r = r, g = g, b = b, a = a }
end

-- ── 相机：世界 ±5.3 × ±4 落在 640×480 里（scale = 60）────────────────
scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 0.0 }, scale = 60.0 },
})

-- ── 坐标轴：水平中轴 + 左侧起点竖线（静态折线，初态可见）──────────────
local axis_color = rgba(96, 110, 130, 255)
scene.spawn({
    Polyline {
        points = {
            Vec2 { x = X0 - 0.2, y = BASE_Y },
            Vec2 { x = X1 + 0.2, y = BASE_Y },
        },
        closed = false,
    },
    Stroke { width = 1.5, color = axis_color },
})
scene.spawn({
    Polyline {
        points = {
            Vec2 { x = X0, y = BASE_Y - AMP - 0.3 },
            Vec2 { x = X0, y = BASE_Y + AMP + 0.3 },
        },
        closed = false,
    },
    Stroke { width = 1.5, color = axis_color },
})

-- ── 几何采样：正弦折线（唯一的循环，纯数据准备）──────────────────────
local curve_points = {}
for i = 0, SAMPLES do
    local u = i / SAMPLES
    local x = X0 + (X1 - X0) * u
    local y = BASE_Y + AMP * math.sin(PERIODS * TAU * u)
    curve_points[#curve_points + 1] = Vec2 { x = q(x), y = q(y) }
end
local start_x = q(X0)
local start_y = q(BASE_Y)

-- ── 曲线：`Reveal` 初态 start = end = 0（不可见），由 `Reveal.end` 写出来 ──
local curve = scene.spawn({
    Reveal { points = curve_points, closed = false },
    Stroke { width = 3.0, color = rgba(120, 220, 255, 255) },
})
scene.animate(curve, scene.clip("Reveal.end", Curve.Ramp {
    from = 0.0,
    to = 1.0,
    dur = TOTAL_T,
}), TOTAL_T)

-- ── 小球：读端管道 —— 位置完全由 `Reveal.current_pos` 决定 ────────────
local ball = scene.spawn({
    Circle { x = start_x, y = start_y, r = 0.16 },
    Fill { color = rgba(255, 210, 120, 255) },
    Stroke { width = 1.2, color = rgba(255, 245, 220, 255) },
})
expr.bind {
    object = ball,
    target = Circle.channel.center,
    root = expr.read {
        channel = Reveal.channel.current_pos,
        ty = "Vec2",
        object = curve,
    },
}
