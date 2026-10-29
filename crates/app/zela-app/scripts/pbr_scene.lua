-- pbr_scene.lua — PBR 材质秀（多幕：点光逐盏巡游 + 材质光谱）。
--
-- 画面：5 个立方体一字排开，材质从左到右跨「电介质→金属→哑光」光谱；
--       方向光主光常驻，4 盏彩色点光（红 / 绿 / 蓝 / 琥珀）分幕依次
--       从画面外横扫入场 —— 每盏光扫过时，亮面金属的高光、电介质
--       的漫反射依次点亮，最终四光齐照进入终幕。
-- 能力使用：
--   B1-D 3D 入口 —— Cube / Material3D / Light / Camera3D；
--   PBR 材质 —— 5 组 metallic × roughness 组合；
--   多幕编排 —— 连续 Ramp 自转跨幕无缝续转（无跳变）、
--               Ramp2 点光横扫入场（Position 通道）、
--               终幕 Orbit 公转 + Sin 缩放呼吸。
-- 数值全部量化到 1e-4，保证跨平台逐位一致。

local PI = math.pi
local TAU = 2.0 * PI

local ACT = 4.0      -- 巡游各幕时长
local FINALE = 6.0   -- 终幕时长

local function q(v)
    return math.floor(v * 10000.0 + 0.5) / 10000.0
end

-- 动画值域是 f32，而表达式层是 f64：`round_f32` 在旧曲线的同一位置落位
-- （第 5a 批：迁移后的每一帧必须与旧 `Curve` 逐位相同）。

-- `Sin { base, amp, omega }` ⇒ `base + amp · sin(omega · t)`（f32 口径）。

-- `Orbit2D` ⇒ `(r cos θ, r sin θ, 0)`，`θ = speed · t + initial_angle`（f32 口径）。

-- 旧 `Step::Play` 区间：局部时间 `[0, dur)`（段外由 Hold 钳到两端）。

-- ── 相机（静态；aspect 由渲染视口覆盖）─────────────────────────────────
scene.spawn({
    Camera3D {
        eye = scene.vec3(0.0, 2.6, 7.2),
        fov_y = 1.0,
    },
})

-- ── 光照：方向光主光 + 4 盏巡游点光 ───────────────────────────────────
scene.spawn({
    Light {
        kind = LightKind.Directional {
            direction = scene.vec3(0.3, -0.5, -0.81),
        },
        color = scene.rgba(255, 250, 240, 255),
        intensity = 1.1,
    },
})

-- 巡游点光：{ 出生 x, 出生 y, 目标 x, 目标 y, z, range, 颜色, 强度 }
-- 出生点都在 range 之外（× 距离熄灭），扫描入场时灯光效果逐渐叠加。
local LIGHTS = {
    { -6.5, -2.2, 1.2, -2.2, 1.4, 3.4, scene.rgba(255, 90, 80, 255), 1.0 },
    { -6.5, 0.4, -1.2, 0.4, 0.4, 3.2, scene.rgba(80, 225, 130, 255), 0.9 },
    { 6.5, 1.6, 1.2, 1.6, -0.6, 3.6, scene.rgba(100, 150, 255, 255), 0.95 },
    { 6.5, -0.9, -1.2, -0.9, -1.6, 3.0, scene.rgba(255, 200, 90, 255), 0.85 },
}
local lights = {}
for i = 1, 4 do
    lights[i] = scene.spawn({
        Light {
            kind = LightKind.Point { range = LIGHTS[i][6] },
            color = LIGHTS[i][7],
            intensity = LIGHTS[i][8],
        },
        Transform { x = LIGHTS[i][1], y = LIGHTS[i][2], z = LIGHTS[i][5] },
    })
end

-- ── 立方体：材质光谱一字排开 ─────────────────────────────────────────
-- { x, 颜色, metallic, roughness }
local SPECTRUM = {
    { -2.4, scene.rgba(220, 80, 60, 255), 0.0, 0.15 },    -- 电介质 · 亮面
    { -1.2, scene.rgba(195, 120, 60, 255), 0.7, 0.35 },   -- 青铜金属
    { 0.0, scene.rgba(215, 218, 225, 255), 1.0, 0.2 },    -- 银金属 · 亮面
    { 1.2, scene.rgba(60, 190, 130, 255), 0.5, 0.55 },    -- 半金属
    { 2.4, scene.rgba(170, 90, 230, 255), 0.0, 0.75 },    -- 电介质 · 哑面
}
local cubes = {}
for i = 1, 5 do
    cubes[i] = scene.spawn({
        Cube { half_extent = 0.55, solid = true },
        Material3D {
            color = SPECTRUM[i][2],
            variant = ShaderVariant.Pbr,
            metallic = SPECTRUM[i][3],
            roughness = SPECTRUM[i][4],
        },
        Transform3D {
            position = scene.vec3(SPECTRUM[i][1], 0.0, 0.0),
            rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
            scale = scene.vec3(1.0, 1.0, 1.0),
        },
    })
end

-- ── 自转规划：跨幕累计转角（单位：圈），每幕 Ramp 续接上一幕终点，
--    保证自转连续无跳变。各立方体速率不同（factors 错峰）。
local CUM = { 0.7, 1.6, 2.5, 3.1, 4.0 }   -- 每幕累计圈数（×factor）
local FACTORS = { 1.0, 1.2, 0.9, 1.1, 0.8 }
local turns = {}
for i = 1, 5 do
    turns[i] = {}
    for a = 1, 5 do
        turns[i][a] = q(TAU * CUM[a] * FACTORS[i])
    end
end

-- ── 幕 1（0–4s）：红灯从左缘横扫入场 ─────────────────────────────────
local function act(a, start)
    for i = 1, 5 do
        local from = 0.0
        if a > 1 then
            from = turns[i][a - 1]
        end
        expr.bind {
            object = cubes[i],
            target = Transform.channel.rotation,
            root = sugar.ramp(from, turns[i][a], ACT),
            clock = clock.shift(start),
            predicate = Predicate.Of { expr = clock.range(0.0, ACT) },
        }
    end
end

-- 一盏点光的横扫入场（旧 `Ramp2`：xy 插值，**保留**光的 z 高度）。
local function light_sweep(a, start)
    expr.bind {
        object = lights[a],
        target = Transform.channel.position,
        root = Vec3FromVec2 {
            v = Mix {
                a = Vec2Of {
                    x = expr.pure(q(LIGHTS[a][1])),
                    y = expr.pure(q(LIGHTS[a][2])),
                },
                b = Vec2Of {
                    x = expr.pure(q(LIGHTS[a][3])),
                    y = expr.pure(q(LIGHTS[a][4])),
                },
                t = Clamp {
                    x = Scale { k = 1.0 / ACT, of = clock.time() },
                    lo = expr.pure(0.0),
                    hi = expr.pure(1.0),
                },
            },
            z = expr.pure(LIGHTS[a][5]),
        },
        clock = clock.shift(start),
        predicate = Predicate.Of { expr = clock.range(0.0, ACT) },
    }
end

act(1, 0.0)
light_sweep(1, 0.0)

-- ── 幕 2（4–8s）：绿灯自左入场 ───────────────────────────────────────
act(2, ACT)
light_sweep(2, ACT)

-- ── 幕 3（8–12s）：蓝灯自右缘入场 ────────────────────────────────────
act(3, 2 * ACT)
light_sweep(3, 2 * ACT)

-- ── 幕 4（12–16s）：琥珀灯自右入场 ───────────────────────────────────
act(4, 3 * ACT)
light_sweep(4, 3 * ACT)

-- ── 终幕（16–22s）：四光齐照，琥珀光转为公转，银立方体呼吸 ────────────
-- 琥珀光的巡游终点 (-1.2, -0.9) 恰好落在半径 1.5 的圆上
-- （初始角 = atan2(-0.9, -1.2) ≈ π + atan(0.75) ≈ 3.7851），
-- 转轨时位置连续不跳变。
local FINALE_START = 4 * ACT
for i = 1, 5 do
    expr.bind {
        object = cubes[i],
        target = Transform.channel.rotation,
        root = sugar.ramp(turns[i][4], turns[i][5], FINALE),
        clock = clock.shift(FINALE_START),
        predicate = Predicate.Of { expr = clock.range(0.0, FINALE) },
    }
end
expr.bind {
    object = lights[4],
    target = Transform.channel.position,
    root = sugar.orbit2d(1.5, 1.2, 3.7851),
    clock = clock.shift(FINALE_START),
    predicate = Predicate.Of { expr = clock.range(0.0, FINALE) },
}
-- 银立方体（正中央）缩放呼吸。
expr.bind {
    object = cubes[3],
    target = Transform.channel.scale,
    root = sugar.wave(1.0, 0.16, 1.4),
    clock = clock.shift(FINALE_START),
    predicate = Predicate.Of { expr = clock.range(0.0, FINALE) },
}
