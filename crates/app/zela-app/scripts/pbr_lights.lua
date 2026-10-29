-- pbr_lights.lua — PBR 光照实验室（B1-A/B：光照数据化 + 点光范围衰减）。
--
-- 画面：中央高光金属立方体（金，roughness 0.14）配 4 个绕行小立方体
--       （不同金属度 / 粗糙度），上方一块哑面电介质对照；
--       1 盏方向光主光 + 4 盏彩色点光（红 / 绿 / 蓝 / 琥珀）各以不同
--       半径、速度、高度公转 —— 镜面高光实时在表面游走，点光的
--       range 窗口衰减（近距离爆亮、超范围熄灭）随距离直接可见。
-- 能力使用：
--   B1-D 3D 入口 —— Cube / Material3D / Light / Camera3D；
--   PBR 材质 —— 金属度 × 粗糙度 5 组组合；
--   LightKind.Point 的 range 衰减 × Transform 提供的位置（铁律 3）；
--   多动画 —— 主光旋转（Rotation · Ramp）、缩放宽高（Scale · Sin）、
--              4 小立方体公转 + 4 点光公转（Position · Orbit）、
--              哑面立方体钟摆（Rotation · Sin）。
-- 数值全部量化到 1e-4，保证跨平台逐位一致。

local PI = math.pi
local TAU = 2.0 * PI

local MAIN = 17.0    -- 主段时长

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

-- ── 相机（静态；aspect 由渲染视口覆盖）─────────────────────────────────
scene.spawn({
    Camera3D {
        eye = Vec3 { x = 0.0, y = 3.4, z = 6.8 },
        fov_y = 1.05,
    },
})
-- PF Step 2：纯 3D 场景不再需要占位 2D 相机（`Framing2D` 可缺席）。

-- ── 光照：方向光主光 + 4 盏彩色点光 ───────────────────────────────────
scene.spawn({
    Light {
        kind = LightKind.Directional {
            direction = Vec3 { x = 0.4, y = -0.6, z = -0.7 },
        },
        color = rgba(255, 255, 250, 255),
        intensity = 1.2,
    },
})

-- 点光公转参数：{ 半径, 速度, 初始角, z 高度, range, 颜色, 强度 }
local RIG = {
    { 3.4, 1.3, 0.0, 1.8, 3.2, rgba(255, 70, 70, 255), 1.0 },
    { 3.0, -1.5, 1.2, 0.6, 3.0, rgba(70, 230, 120, 255), 0.9 },
    { 2.6, 1.7, 2.6, -0.6, 3.6, rgba(90, 150, 255, 255), 0.95 },
    { 2.2, -1.9, 4.0, -1.8, 2.8, rgba(255, 190, 70, 255), 0.85 },
}
local lights = {}
for i = 1, 4 do
    local r, speed, a0, z, range, color, intensity =
        RIG[i][1], RIG[i][2], RIG[i][3], RIG[i][4], RIG[i][5], RIG[i][6], RIG[i][7]
    lights[i] = scene.spawn({
        Light {
            kind = LightKind.Point { range = range },
            color = color,
            intensity = intensity,
        },
        Transform {
            x = q(r * math.cos(a0)),
            y = q(r * math.sin(a0)),
            z = z,
        },
    })
end

-- ── 立方体 ────────────────────────────────────────────────────────────
-- 中央英雄立方体：金色亮面金属（金属度 1.0 / 粗糙度 0.14）。
local hero = scene.spawn({
    Cube { half_extent = 0.85, solid = true },
    Material3D {
        color = rgba(235, 185, 60, 255),
        variant = ShaderVariant.Pbr,
        metallic = 1.0,
        roughness = 0.14,
    },
    Transform3D {
        position = Vec3 { x = 0.0, y = 0.0, z = 0.0 },
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = Vec3 { x = 1.0, y = 1.0, z = 1.0 },
    },
})

-- 哑面电介质：粗糙度 0.95 —— 高光被抹平，只留漫反射（与金属 hero 对照）。
local matte = scene.spawn({
    Cube { half_extent = 0.5, solid = true },
    Material3D {
        color = rgba(210, 140, 170, 255),
        variant = ShaderVariant.Pbr,
        roughness = 0.95,
    },
    Transform3D {
        position = Vec3 { x = 0.0, y = 2.7, z = 0.5 },
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = Vec3 { x = 1.0, y = 1.0, z = 1.0 },
    },
})

-- 绕行立方体：{ 公转半径, 速度, 初始角, z, 颜色, metallic, roughness }
local MINIS = {
    { 2.35, 1.0, 0.0, -1.6, rgba(220, 70, 60, 255), 0.0, 0.3 },
    { 2.35, 1.3, 1.57, -0.5, rgba(200, 205, 215, 255), 1.0, 0.2 },
    { 2.35, -1.15, 3.14, 0.5, rgba(60, 200, 120, 255), 0.4, 0.6 },
    { 2.35, 0.8, 4.71, 1.6, rgba(90, 160, 255, 255), 0.0, 0.1 },
}
local minis = {}
for i = 1, 4 do
    local r, speed, a0, z, color, m, rough =
        MINIS[i][1], MINIS[i][2], MINIS[i][3], MINIS[i][4], MINIS[i][5], MINIS[i][6], MINIS[i][7]
    minis[i] = scene.spawn({
        Cube { half_extent = 0.4, solid = true },
        Material3D {
            color = color,
            variant = ShaderVariant.Pbr,
            metallic = m,
            roughness = rough,
        },
        Transform3D {
            position = Vec3 {
                x = q(r * math.cos(a0)),
                y = q(r * math.sin(a0)),
                z = z,
            },
            rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
            scale = Vec3 { x = 1.0, y = 1.0, z = 1.0 },
        },
    })
end

-- ── 主段（0–17s，单段并行）：转动 + 缩放 + 公转 + 点光巡游 ────────────
-- 英雄立方体：1.5 圈自转 + 缩放呼吸。
expr.bind {
    object = hero,
    target = Transform.channel.rotation,
    root = ramp(0.0, q(TAU * 1.5), MAIN),
    clock = clock.scale(1.0),
    predicate = Predicate.Of { expr = clock.range(0.0, MAIN) },
}
expr.bind {
    object = hero,
    target = Transform.channel.scale,
    root = sin_curve(1.0, 0.12, 1.1),
    clock = clock.scale(1.0),
    predicate = Predicate.Of { expr = clock.range(0.0, MAIN) },
}
-- 绕行立方体：沿各自高度公转。
for i = 1, 4 do
    expr.bind {
        object = minis[i],
        target = Transform.channel.position,
        root = orbit2d(MINIS[i][1], MINIS[i][2], MINIS[i][3]),
        clock = clock.scale(1.0),
        predicate = Predicate.Of { expr = clock.range(0.0, MAIN) },
    }
end
-- 4 盏点光：不同半径 / 速度 / 初始角公转，光色跟着扫过曲面。
for i = 1, 4 do
    expr.bind {
        object = lights[i],
        target = Transform.channel.position,
        root = orbit2d(RIG[i][1], RIG[i][2], RIG[i][3]),
        clock = clock.scale(1.0),
        predicate = Predicate.Of { expr = clock.range(0.0, MAIN) },
    }
end
-- 哑面立方体：±90° 钟摆，展示粗糙 0.95 下高光几乎消失。
expr.bind {
    object = matte,
    target = Transform.channel.rotation,
    root = sin_curve(0.0, q(PI * 0.5), 0.8),
    clock = clock.scale(1.0),
    predicate = Predicate.Of { expr = clock.range(0.0, MAIN) },
}
