-- pendulum.lua — 钟摆（2D 动画综合演示：多轨道并行 + 阻尼收摆）。
--
-- 画面：顶部挂点，摆杆 + 摆锤 + 摆锤内环，底部轨迹弧线。
-- 能力使用：
--   S5 Parent：摆杆 / 摆锤 / 内环跟随摆动组整体旋转；
--   S3 Rotation 通道 · Curve.Sin —— 摆绕挂点往复摆动；
--   S2 Reveal · RevealRange —— 轨迹弧线逐段书写；
--   CircleRadius —— 内环半径呼吸；
--   S6 Opacity —— 三对象淡入。
-- 数值全部量化到 1e-4，保证跨平台逐位一致。

local PI = math.pi
local TAU = 2.0 * PI
local PIVOT_Y = 2.4
local ROD_LEN = 2.0
local AMP1 = 0.55   -- 段 1 摆幅（弧度，≈31.5°）
local AMP2 = 0.22   -- 段 2 摆幅（模拟阻尼）
local T1 = 6.0
local T2 = 4.0

local function q(v)
    return math.floor(v * 10000.0 + 0.5) / 10000.0
end

-- `Ramp` 的算子化形态（显式时钟；`t` 是时钟局部时间）。

-- 动画值域是 f32，而表达式层是 f64：`round_f32` 在**旧曲线的同一位置**落位，
-- 使迁移后的每一帧与旧 `Curve` 逐位相同（第 5a 批的等价要求）。

-- `Sin { base, amp, omega }` ⇒ `base + amp · sin(omega · t)`（f32 口径）。

-- 旧 `Step::Play` 区间：局部时间 `[0, dur)`（段外由 Hold 钳到两端）。

-- 2D 相机（渲染器要求 Framing2D；世界中心对准钟摆）。
scene.spawn({
    Camera2D { center = scene.vec2(0.0, 1.4), scale = 55.0 },
})

-- 轨迹弧线（以挂点为圆心、杆长为半径，±AMP1 摆幅的圆弧）。
local N = 48
local trace_pts = {}
for i = 0, N do
    local a = q(-AMP1 + (2.0 * AMP1) * i / N)
    trace_pts[#trace_pts + 1] = Vec2 {
        x = q(ROD_LEN * math.sin(a)),
        y = q(PIVOT_Y - ROD_LEN * math.cos(a)),
    }
end

-- 轨迹：初态不可见（Reveal end=0），由 RevealRange 书写。
local trace = scene.spawn({
    Reveal { points = trace_pts, closed = false },
    Stroke { width = 1.5, color = scene.rgba(140, 160, 255, 255) },
    Opacity { value = 0.0 },
})

-- 摆动组：原点即挂点；摆杆 / 摆锤 / 内环都是它的子对象。
local swing = scene.spawn({
    Transform { x = 0.0, y = PIVOT_Y },
})
scene.spawn({
    Polyline {
        points = {
            scene.vec2(0.0, 0.0),
            scene.vec2(0.0, q(-ROD_LEN)),
        },
        closed = false,
    },
    Stroke { width = 3.0, color = scene.rgba(225, 225, 240, 255) },
    Parent { parent = swing },
})
local bob = scene.spawn({
    Circle { x = 0.0, y = q(-ROD_LEN), r = 0.24 },
    Fill { color = scene.rgba(255, 140, 80, 255) },
    Transform { x = 0.0, y = 0.0 },
    Parent { parent = swing },
})
local ring = scene.spawn({
    Circle { x = 0.0, y = q(-ROD_LEN), r = 0.34 },
    Stroke { width = 2.0, color = scene.rgba(255, 210, 120, 255) },
    Transform { x = 0.0, y = 0.0 },
    Parent { parent = swing },
})

-- 段 1（0–6s）：摆起 + 轨迹书写 + 内环呼吸 + 三对象淡入（同一区间的并行绑定）。
expr.bind {
    object = swing,
    target = Transform.channel.rotation,
    root = sugar.wave(0.0, AMP1, q(TAU / 2.0)),
    clock = clock.scale(1.0),
    predicate = Predicate.Of { expr = clock.range(0.0, T1) },
}
expr.bind {
    object = trace,
    target = Reveal.channel["end"],
    root = sugar.ramp(0.0, 1.0, T1),
    clock = clock.scale(1.0),
    predicate = Predicate.Of { expr = clock.range(0.0, T1) },
}
expr.bind {
    object = ring,
    target = Circle.channel.radius,
    root = sugar.wave(0.34, 0.06, 3.0),
    clock = clock.scale(1.0),
    predicate = Predicate.Of { expr = clock.range(0.0, T1) },
}
for _, object in ipairs({ trace, bob, ring }) do
    expr.bind {
        object = object,
        target = Opacity.channel.value,
        root = sugar.ramp(0.0, 1.0, 0.6),
        clock = clock.scale(1.0),
        predicate = Predicate.Of { expr = clock.range(0.0, T1) },
    }
end

-- 段 2（6–10s）：摆幅收小（阻尼）+ 内环呼吸变慢（时钟整体后移 T1）。
expr.bind {
    object = swing,
    target = Transform.channel.rotation,
    root = sugar.wave(0.0, AMP2, q(TAU / 2.5)),
    clock = clock.shift(T1),
    predicate = Predicate.Of { expr = clock.range(0.0, T2) },
}
expr.bind {
    object = ring,
    target = Circle.channel.radius,
    root = sugar.wave(0.34, 0.05, 4.0),
    clock = clock.shift(T1),
    predicate = Predicate.Of { expr = clock.range(0.0, T2) },
}
