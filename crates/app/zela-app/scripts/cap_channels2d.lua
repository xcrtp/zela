-- cap_channels2d.lua — F 批新字段通道（2D）：一条绑定一个通道。
--
-- 目的：给 `#[derive(Channels)]` 批量注册的 2D 字段通道留**作品级证据**。
-- 每个通道都有独立对象与独立绑定；动画期间字段值随曲线变化（不是静态占位）。
--
-- 覆盖（每个通道都用 `<组件>.channel.<字段>` 常量选中）：
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

-- 动画值域是 f32，而表达式层是 f64：`round_f32` 在旧曲线的同一位置落位。
local function f32(v)
    return RoundF32 { of = v }
end

local function ramp(from, to, dur)
    return Mix {
        a = expr.pure(from),
        b = expr.pure(to),
        t = Clamp {
            x = Scale { k = 1.0 / dur, of = clock.time() },
            lo = expr.pure(0.0),
            hi = expr.pure(1.0),
        },
    }
end

local function ramp2(ax, ay, bx, by, dur)
    return Mix {
        a = Vec2Of { x = expr.pure(ax), y = expr.pure(ay) },
        b = Vec2Of { x = expr.pure(bx), y = expr.pure(by) },
        t = Clamp {
            x = Scale { k = 1.0 / dur, of = clock.time() },
            lo = expr.pure(0.0),
            hi = expr.pure(1.0),
        },
    }
end

-- `Sin { base, amp, omega }` ⇒ `base + amp · sin(omega · t)`（f32 口径）。
local function sin_curve(base, amp, omega)
    local angle = Mul(f32(expr.pure(omega)), clock.time())
    local s = f32(Sin { of = angle })
    local prod = f32(Mul(f32(expr.pure(amp)), s))
    return f32(Add { a = f32(expr.pure(base)), b = prod })
end

local WINDOW = Predicate.Of {
    expr = And {
        a = Ge { a = clock.time(), b = expr.pure(0.0) },
        b = Lt { a = clock.time(), b = expr.pure(T) },
    },
}

-- 一条绑定一个通道（旧 `scene.animate` / `parallel` 的单段 [0, T)）。
local function bind(object, target, root)
    expr.bind {
        object = object,
        target = target,
        root = root,
        clock = clock.scale(1.0),
        predicate = WINDOW,
    }
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
bind(ball, Circle.channel.center, ramp2(-3.4, 2.2, -2.0, 2.2, T))
bind(ball, Circle.channel.radius, sin_curve(0.45, 0.18, 3.0))

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
bind(arrow, Arrow.channel.from, ramp2(0.4, -0.6, -0.4, -1.0, T))
bind(arrow, Arrow.channel.to, ramp2(2.6, -0.6, 3.2, 0.2, T))
bind(arrow, Arrow.channel.head_length, ramp(0.24, 0.42, T))
bind(arrow, Arrow.channel.head_width, ramp(0.18, 0.30, T))

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
bind(reveal_start, Reveal.channel.start, ramp(0.0, 0.5, T))

local reveal_end = scene.spawn({
    Reveal { points = line(-3.2), closed = false },
    Stroke { width = 2.5, color = rgba(180, 255, 210, 255) },
})
bind(reveal_end, Reveal.channel["end"], ramp(1.0, 0.6, T))

local reveal_range = scene.spawn({
    Reveal { points = line(-3.6), closed = false },
    Stroke { width = 2.5, color = rgba(255, 210, 150, 255) },
})
bind(reveal_range, Reveal.channel.range, ramp2(0.0, 1.0, 0.3, 0.8, T))

-- ── 虚线：周期 / 占空 / 相位 ──────────────────────────────────────────
local dashed = scene.spawn({
    Circle { x = 3.0, y = 2.4, r = 0.9 },
    Stroke { width = 4.0, color = rgba(160, 255, 200, 255) },
    Dash { period = 0.05, duty = 0.6, phase = 0.0 },
})
bind(dashed, Dash.channel.period, ramp(0.05, 0.08, T))
bind(dashed, Dash.channel.duty, ramp(0.6, 0.3, T))
bind(dashed, Dash.channel.phase, ramp(0.0, 0.5, T))

-- ── 曲线窗口：lo / hi ────────────────────────────────────────────────
local window = scene.spawn({
    Circle { x = 0.6, y = 1.6, r = 0.7 },
    Fill { color = rgba(210, 150, 255, 255) },
    CurveWindow { lo = 0.0, hi = 4.0 },
})
bind(window, CurveWindow.channel.lo, ramp(0.0, 0.5, T))
bind(window, CurveWindow.channel.hi, ramp(4.0, 2.0, T))

-- ── 2D 相机：缩放 + 中心 ─────────────────────────────────────────────
bind(cam, Camera2D.channel.scale, ramp(58.0, 54.0, T))
bind(cam, Camera2D.channel.center, ramp2(0.0, 0.0, 0.0, 0.15, T))
