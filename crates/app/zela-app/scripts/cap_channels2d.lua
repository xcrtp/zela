-- cap_channels2d.lua — F 批新字段通道（2D）：一条绑定一个通道。
--
-- 目的：给 `#[derive(Channels)]` 批量注册的 2D 字段通道留**作品级证据**。
-- 每个通道都有独立对象与独立绑定；动画期间字段值随曲线变化（不是静态占位）。
--
-- 覆盖（括号里是 `scene.clip` 的 target）：
--   2D 相机   —— 中心 / 缩放（`Framing2D.center` / `Framing2D.scale`）
--   圆        —— 中心 / 半径（`Circle.center` / `Circle.radius`）
--   箭头      —— 起点 / 终点 / 头长 / 头宽（`Arrow.from|to|head_length|head_width`）
--   书写      —— 起点 / 终点 / 区间（`Reveal.start` / `Reveal.end` / `Reveal.range`）
--   虚线      —— 周期 / 占空 / 相位（`Dash.period|duty|phase`）
--   曲线窗口  —— lo / hi（`CurveWindow.lo|hi`）
--
-- ⚠️ 2D 相机的可写通道是**存储组件** `Framing2D`，不是派生的 `Camera2D`
-- （`Camera2D` 由 `Framing2D::to_camera_2d` 现算，不落组件；见 GAPS AJ-05）。
--
-- 判据：整段 2s 播放完，各字段到达 Ramp 终点（帧哈希 / 像素基线间接观测）。
-- 数值量化到 1e-4，保证跨平台逐位一致。

local T = 2.0

local function q(v)
    return math.floor(v * 10000.0 + 0.5) / 10000.0
end

local function rgba(r, g, b, a)
    return Color { r = r, g = g, b = b, a = a }
end

-- 2D 相机：世界 ±5.5 × ±4.1 落在 640×480 里（scale = 58）。
local cam = scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 0.0 }, scale = 58.0 },
})

-- ── 圆：中心平移 + 半径呼吸 ────────────────────────────────────────────
local ball = scene.spawn({
    Circle { x = -3.4, y = 2.2, r = 0.45 },
    Fill { color = rgba(96, 196, 255, 255) },
})
scene.animate(ball, scene.clip("Circle.center", Curve.Ramp2 {
    from = Vec2 { x = -3.4, y = 2.2 },
    to = Vec2 { x = -2.0, y = 2.2 },
    dur = T,
}), T)
scene.parallel(ball, scene.clip("Circle.radius", Curve.Sin {
    base = 0.45,
    amp = 0.18,
    omega = 3.0,
}))

-- ── 箭头：四个字段同时动 ──────────────────────────────────────────────
local arrow = scene.spawn({
    Arrow {
        from = Vec2 { x = 0.4, y = -0.6 },
        to = Vec2 { x = 2.6, y = -0.6 },
        head_length = 0.24,
        head_width = 0.18,
    },
    Fill { color = rgba(255, 180, 80, 255) },
})
scene.parallel(arrow, scene.clip("Arrow.from", Curve.Ramp2 {
    from = Vec2 { x = 0.4, y = -0.6 },
    to = Vec2 { x = -0.4, y = -1.0 },
    dur = T,
}))
scene.parallel(arrow, scene.clip("Arrow.to", Curve.Ramp2 {
    from = Vec2 { x = 2.6, y = -0.6 },
    to = Vec2 { x = 3.2, y = 0.2 },
    dur = T,
}))
scene.parallel(arrow, scene.clip("Arrow.head_length", Curve.Ramp {
    from = 0.24,
    to = 0.42,
    dur = T,
}))
scene.parallel(arrow, scene.clip("Arrow.head_width", Curve.Ramp {
    from = 0.18,
    to = 0.30,
    dur = T,
}))

-- ── 书写：start / end / range 各一条（避免同一字段互相覆盖）────────────
local function line(y)
    local pts = {}
    for i = 0, 20 do
        pts[#pts + 1] = Vec2 { x = q(-4.0 + 4.0 * i / 20), y = y }
    end
    return pts
end

local ink = rgba(220, 235, 255, 255)

local reveal_start = scene.spawn({
    Reveal { points = line(3.2), closed = false },
    Stroke { width = 2.5, color = ink },
})
scene.parallel(reveal_start, scene.clip("Reveal.start", Curve.Ramp {
    from = 0.0,
    to = 0.5,
    dur = T,
}))

local reveal_end = scene.spawn({
    Reveal { points = line(-3.2), closed = false },
    Stroke { width = 2.5, color = rgba(180, 255, 210, 255) },
})
scene.parallel(reveal_end, scene.clip("Reveal.end", Curve.Ramp {
    from = 1.0,
    to = 0.6,
    dur = T,
}))

local reveal_range = scene.spawn({
    Reveal { points = line(-3.6), closed = false },
    Stroke { width = 2.5, color = rgba(255, 210, 150, 255) },
})
scene.parallel(reveal_range, scene.clip("Reveal.range", Curve.Ramp2 {
    from = Vec2 { x = 0.0, y = 1.0 },
    to = Vec2 { x = 0.3, y = 0.8 },
    dur = T,
}))

-- ── 虚线：周期 / 占空 / 相位 ──────────────────────────────────────────
local dashed = scene.spawn({
    Circle { x = 3.0, y = 2.4, r = 0.9 },
    Stroke { width = 4.0, color = rgba(160, 255, 200, 255) },
    Dash { period = 0.05, duty = 0.6, phase = 0.0 },
})
scene.parallel(dashed, scene.clip("Dash.period", Curve.Ramp {
    from = 0.05,
    to = 0.08,
    dur = T,
}))
scene.parallel(dashed, scene.clip("Dash.duty", Curve.Ramp {
    from = 0.6,
    to = 0.3,
    dur = T,
}))
scene.parallel(dashed, scene.clip("Dash.phase", Curve.Ramp {
    from = 0.0,
    to = 0.5,
    dur = T,
}))

-- ── 曲线窗口：lo / hi ────────────────────────────────────────────────
local window = scene.spawn({
    Circle { x = 0.6, y = 1.6, r = 0.7 },
    Fill { color = rgba(210, 150, 255, 255) },
    CurveWindow { lo = 0.0, hi = 4.0 },
})
scene.parallel(window, scene.clip("CurveWindow.lo", Curve.Ramp {
    from = 0.0,
    to = 0.5,
    dur = T,
}))
scene.parallel(window, scene.clip("CurveWindow.hi", Curve.Ramp {
    from = 4.0,
    to = 2.0,
    dur = T,
}))

-- ── 2D 相机：缩放 + 中心（存储组件是 Framing2D）──────────────────────
scene.parallel(cam, scene.clip("Framing2D.scale", Curve.Ramp {
    from = 58.0,
    to = 54.0,
    dur = T,
}))
scene.parallel(cam, scene.clip("Framing2D.center", Curve.Ramp2 {
    from = Vec2 { x = 0.0, y = 0.0 },
    to = Vec2 { x = 0.0, y = 0.15 },
    dur = T,
}))
