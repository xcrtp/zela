-- orbit.lua — 公转行星（2D 动画综合演示：Orbit / Parent 嵌套 / 分幕）。
--
-- 画面：中央太阳（缩放呼吸）+ 两颗行星公转（内快外慢），
--       外行星带一颗卫星（子级再公转）；流星横向划过并淡出。
-- 能力使用：
--   S4 Position 通道 · Curve.Orbit —— 行星 / 卫星公转；
--   S5 Parent —— 卫星跟随行星；
--   S3 Scale 通道 · Curve.Sin —— 太阳半径呼吸；
--   S2 Reveal · RevealRange —— 两条轨道环先后书写（分幕预演）；
--   S6 Opacity —— 轨道环 / 天体淡入，流星淡出；
--   Position · Ramp2 —— 流星直线划过。
-- 数值全部量化到 1e-4，保证跨平台逐位一致。

local PI = math.pi
local TAU = 2.0 * PI
local MAIN = 8.0        -- 主段时长（公转 + 呼吸）
local R1 = 1.0          -- 内行星轨道半径
local R2 = 1.8          -- 外行星轨道半径
local COMET_DUR = 1.5

local function q(v)
    return math.floor(v * 10000.0 + 0.5) / 10000.0
end

local function rgba(r, g, b, a)
    return Color { r = r, g = g, b = b, a = a }
end

-- 动画值域是 f32，而表达式层是 f64：`round_f32` 在旧曲线的同一位置落位
-- （第 5a 批：迁移后的每一帧必须与旧 `Curve` 逐位相同）。
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

-- `Sin { base, amp, omega }` ⇒ `base + amp · sin(omega · t)`（f32 口径）。
local function sin_curve(base, amp, omega)
    local angle = Mul(f32(expr.pure(omega)), clock.time())
    local s = f32(Sin { of = angle })
    local prod = f32(Mul(f32(expr.pure(amp)), s))
    return f32(Add { a = f32(expr.pure(base)), b = prod })
end

-- `Orbit2D` ⇒ `(r cos θ, r sin θ, 0)`，`θ = speed · t + initial_angle`（f32 口径）。
local function orbit2d(radius, speed, initial_angle)
    local angle = Add {
        a = Mul(f32(expr.pure(speed)), clock.time()),
        b = f32(expr.pure(initial_angle)),
    }
    return Vec3FromVec2 {
        v = Vec2Of {
            x = f32(Mul(f32(expr.pure(radius)), f32(Cos { of = angle }))),
            y = f32(Mul(f32(expr.pure(radius)), f32(Sin { of = angle }))),
        },
        z = expr.pure(0.0),
    }
end

-- 旧 `Step::Play` 区间：局部时间 `[0, dur)`（段外由 Hold 钳到两端）。

-- 串行段起点（旧 `scene.animate` 的全局游标）。
local PRE_RUN = 1.2      -- 两条轨道环各写 1.2s
local MAIN_START = 2.4   -- 预演段之后
local COMET_START = MAIN_START + MAIN

-- 2D 相机（渲染器要求 Framing2D；世界中心对准轨道原点）。
scene.spawn({
    Camera2D { scale = 55.0 },
})

local function ring_points(radius)
    local pts = {}
    local N = 96
    for i = 0, N do
        local a = TAU * i / N
        pts[#pts + 1] = Vec2 {
            x = q(radius * math.cos(a)),
            y = q(radius * math.sin(a)),
        }
    end
    return pts
end

-- ── 天体 ────────────────────────────────────────────────────────────
local sun = scene.spawn({
    Circle { x = 0.0, y = 0.0, r = 0.5 },
    Fill { color = rgba(255, 225, 130, 255) },
})

local planet1 = scene.spawn({
    Circle { x = 0.0, y = 0.0, r = 0.16 },
    Fill { color = rgba(255, 150, 90, 255) },
    Transform { x = q(R1), y = 0.0 },
    Opacity { value = 0.0 },
})
local planet2 = scene.spawn({
    Circle { x = 0.0, y = 0.0, r = 0.24 },
    Fill { color = rgba(120, 190, 255, 255) },
    Transform { x = q(R2), y = 0.0 },
    Opacity { value = 0.0 },
})
local moon = scene.spawn({
    Circle { x = 0.0, y = 0.0, r = 0.07 },
    Fill { color = rgba(235, 235, 245, 255) },
    Transform { x = 0.32, y = 0.0 },
    Opacity { value = 0.0 },
    Parent { parent = planet2 },
})

-- 轨道环：初态不可见，预演段先后书写，主段淡入。
local ring1 = scene.spawn({
    Reveal { points = ring_points(R1), closed = true },
    Stroke { width = 1.0, color = rgba(90, 90, 120, 255) },
    Opacity { value = 0.0 },
})
local ring2 = scene.spawn({
    Reveal { points = ring_points(R2), closed = true },
    Stroke { width = 1.0, color = rgba(90, 90, 120, 255) },
    Opacity { value = 0.0 },
})

-- 流星：初态不可见，主段结束后划过并淡出。
local comet = scene.spawn({
    Circle { x = 0.0, y = 0.0, r = 0.07 },
    Fill { color = rgba(255, 255, 255, 255) },
    Opacity { value = 0.0 },
})

-- ── 预演段（0–2.4s）：两条轨道环先后书写 ────────────────────────────
expr.bind {
    object = ring1,
    target = Reveal.channel["end"],
    root = ramp(0.0, 1.0, PRE_RUN),
    clock = clock.shift(0.0),
    predicate = Predicate.Of { expr = clock.range(0.0, PRE_RUN) },
}
expr.bind {
    object = ring2,
    target = Reveal.channel["end"],
    root = ramp(0.0, 1.0, PRE_RUN),
    clock = clock.shift(PRE_RUN),
    predicate = Predicate.Of { expr = clock.range(0.0, PRE_RUN) },
}

-- ── 主段（2.4–10.4s）：太阳呼吸 + 行星 / 卫星公转 + 淡入（并行轨道）──
expr.bind {
    object = sun,
    target = Transform.channel.scale,
    root = sin_curve(1.0, 0.12, 1.5),
    clock = clock.shift(MAIN_START),
    predicate = Predicate.Of { expr = clock.range(0.0, MAIN) },
}
expr.bind {
    object = planet1,
    target = Transform.channel.position,
    root = orbit2d(q(R1), q(TAU / 3.0), 0.0),
    clock = clock.shift(MAIN_START),
    predicate = Predicate.Of { expr = clock.range(0.0, MAIN) },
}
expr.bind {
    object = planet2,
    target = Transform.channel.position,
    root = orbit2d(q(R2), q(TAU / 6.0), 0.0),
    clock = clock.shift(MAIN_START),
    predicate = Predicate.Of { expr = clock.range(0.0, MAIN) },
}
expr.bind {
    object = moon,
    target = Transform.channel.position,
    root = orbit2d(0.32, q(TAU / 0.9), 0.0),
    clock = clock.shift(MAIN_START),
    predicate = Predicate.Of { expr = clock.range(0.0, MAIN) },
}
for _, object in ipairs({ ring1, ring2, planet1, planet2, moon }) do
    expr.bind {
        object = object,
        target = Opacity.channel.value,
        root = ramp(0.0, 1.0, 1.0),
        clock = clock.shift(MAIN_START),
        predicate = Predicate.Of { expr = clock.range(0.0, MAIN) },
    }
end

-- ── 段 3（10.4–11.9s）：流星划过并淡出 ──────────────────────────────
expr.bind {
    object = comet,
    target = Transform.channel.position,
    root = Vec3FromVec2 {
        v = Mix {
            a = Vec2Of { x = expr.pure(2.6), y = expr.pure(1.9) },
            b = Vec2Of { x = expr.pure(-2.6), y = expr.pure(1.9) },
            t = Clamp {
                x = Scale { k = 1.0 / COMET_DUR, of = clock.time() },
                lo = expr.pure(0.0),
                hi = expr.pure(1.0),
            },
        },
        z = expr.pure(0.0),
    },
    clock = clock.shift(COMET_START),
    predicate = Predicate.Of { expr = clock.range(0.0, COMET_DUR) },
}
expr.bind {
    object = comet,
    target = Opacity.channel.value,
    root = ramp(1.0, 0.0, COMET_DUR),
    clock = clock.shift(COMET_START),
    predicate = Predicate.Of { expr = clock.range(0.0, COMET_DUR) },
}
