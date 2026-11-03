-- pbr_materials.lua — PBR 材质画廊（B1-C：金属度 × 粗糙度扫描 + 变体对照）。
--
-- 画面：3×3 网格 —— 列 = 金属度（0.0 电介质 / 0.5 半金属 / 1.0 金属），
--       行 = 粗糙度（0.12 亮面 / 0.45 中度 / 0.82 哑面）；右侧另摆
--       Flat（无光照）与 Lambert（朗伯）两个变体对照同一光源环境，
--       衬托「Pbr 才有金属高光 / 粗糙度变化的镜面反射」。
-- 能力使用：
--   B1-D 3D 入口 —— Cube / Material3D / Transform3D / Light / Camera3D；
--   PBR 材质 —— metallic / roughness（仅 Pbr 变体消费）；
--   多动画 —— 逐立方体 Keyframes 差速自转、点光 Orbit 公转 /
--             点光 Ramp2 横扫、中心立方体 Sin 缩放呼吸。
-- 数值全部量化到 1e-4，保证跨平台逐位一致。

local PI = math.pi
local TAU = 2.0 * PI

local MAIN = 16.0    -- 主段时长
local SP = 1.7       -- 网格间距
local HE = 0.5       -- 网格立方体半边长

local function q(v)
    return math.floor(v * 10000.0 + 0.5) / 10000.0
end

-- 动画值域是 f32，而表达式层是 f64：`narrow_f32` 在旧曲线的同一位置落位
-- （第 5a 批：迁移后的每一帧必须与旧 `Curve` 逐位相同）。

-- `Keyframes`（三点线性）⇒ 两个 `mix` + 一个 `Select`：
-- `t < t1` 走第一段（否则第二段），段外由 clamp 钳到端点。
local function keyed3(t1, v1, t2, v2)
    local first = Mix {
        a = expr.pure(0.0),
        b = expr.pure(v1),
        t = Clamp {
            x = Scale { k = 1.0 / t1, of = clock.time() },
            lo = expr.pure(0.0),
            hi = expr.pure(1.0),
        },
    }
    local second = Mix {
        a = expr.pure(v1),
        b = expr.pure(v2),
        t = Clamp {
            x = Scale {
                k = 1.0 / (t2 - t1),
                of = Add { a = clock.time(), b = expr.pure(-t1) },
            },
            lo = expr.pure(0.0),
            hi = expr.pure(1.0),
        },
    }
    return Select {
        cond = Lt { a = clock.time(), b = expr.pure(t1) },
        ["then"] = first,
        otherwise = second,
    }
end

-- `Sin { base, amp, omega }` ⇒ `base + amp · sin(omega · t)`（f32 口径）。

-- `Orbit2D` ⇒ `(r cos θ, r sin θ, 0)`，`θ = speed · t + initial_angle`（f32 口径）。

-- 旧 `Step::Play` 区间：局部时间 `[0, dur)`（段外由 Hold 钳到两端）。

-- ── 相机（静态；aspect 由渲染视口覆盖）─────────────────────────────────
scene.spawn({
    Camera3D {
        eye = scene.vec3(0.0, 3.0, 6.6),
        fov_y = 1.05,
    },
})

-- ── 光照：方向光主光 + 冷暖两点光副光（位置由 Transform 提供）──
scene.spawn({
    Light {
        kind = LightKind.Directional {
            direction = scene.vec3(0.35, -0.5, -0.79),
        },
        color = scene.rgba(255, 242, 224, 255),
        intensity = 1.5,
    },
})
-- 冷光：绕 X-Y 平面公转（z 由 Transform 固定为 +1.6）。
local light_a = scene.spawn({
    Light {
        kind = LightKind.Point { range = 6.0 },
        color = scene.rgba(135, 190, 255, 255),
        intensity = 0.9,
    },
    Transform { x = 3.7, y = 0.0, z = 1.6 },
})
-- 暖光：反方向公转（z = -1.6）。
local light_b = scene.spawn({
    Light {
        kind = LightKind.Point { range = 5.5 },
        color = scene.rgba(255, 168, 88, 255),
        intensity = 0.85,
    },
    Transform { x = q(3.1 * math.cos(2.4)), y = q(3.1 * math.sin(2.4)), z = -1.6 },
})

-- ── 材质网格：金属度（列）× 粗糙度（行）───────────────────────────────
local METALLIC = { 0.0, 0.5, 1.0 }
local COLORS = {
    scene.rgba(215, 95, 65, 255),     -- 电介质（F0 = 0.04，高光弱、颜色主导）
    scene.rgba(70, 185, 150, 255),    -- 半金属（F0 居中）
    scene.rgba(235, 190, 70, 255),    -- 金属（F0 = 反照率，高光带色）
}
local ROUGH = { 0.12, 0.45, 0.82 }

local grid = {}
for row = 0, 2 do
    for col = 0, 2 do
        local x = q((col - 1) * SP)
        local y = q((row - 1) * SP)
        grid[row * 3 + col + 1] = scene.spawn({
            Cube { half_extent = HE, solid = true },
            Material3D {
                color = COLORS[col + 1],
                variant = ShaderVariant.Pbr,
                metallic = METALLIC[col + 1],
                roughness = ROUGH[row + 1],
            },
            Transform3D {
                position = scene.vec3(x, y, 0.0),
                rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
                scale = scene.vec3(1.0, 1.0, 1.0),
            },
        })
    end
end

-- ── 变体对照：Flat（无光照）与 Lambert（朗伯）──────────────────────────
local flat = scene.spawn({
    Cube { half_extent = 0.45, solid = true },
    Material3D {
        color = scene.rgba(45, 205, 220, 255),
        variant = ShaderVariant.Flat,
    },
    Transform3D {
        position = scene.vec3(3.5, -1.35, 0.0),
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = scene.vec3(1.0, 1.0, 1.0),
    },
})
local lambert = scene.spawn({
    Cube { half_extent = 0.45, solid = true },
    Material3D {
        color = scene.rgba(185, 100, 230, 255),
    },
    Transform3D {
        position = scene.vec3(3.5, 1.35, 0.0),
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = scene.vec3(1.0, 1.0, 1.0),
    },
})

-- ── 主段（0–16s，单段并行）：差速自转 + 点光公转 / 横扫 + 中心呼吸 ────
-- 网格立方体：Keyframes 三段自转（0→0.4 圈 → 满圈），各立方体速率不同。
local RATES = { 0.8, 1.0, 0.7, 1.1, 0.9, 1.2, 0.6, 1.05, 0.85 }
for i = 1, 9 do
    expr.bind {
        object = grid[i],
        target = Transform.channel.rotation,
        root = keyed3(7.0, q(TAU * RATES[i] * 0.5), 16.0, q(TAU * RATES[i])),
        clock = clock.scale(1.0),
        duration = MAIN,
    }
end
-- 变体对照也缓慢自转：转面时 Lambert 无高光、Flat 恒定纯色，与 Pbr 对照。
for _, object in ipairs({ flat, lambert }) do
    expr.bind {
        object = object,
        target = Transform.channel.rotation,
        root = sugar.ramp(0.0, q(TAU * 0.8), MAIN),
        clock = clock.scale(1.0),
        duration = MAIN,
    }
end
-- 冷光公转（Orbit）、暖光横扫（Ramp2）：高光在网格上移动。
expr.bind {
    object = light_a,
    target = Transform.channel.position,
    root = sugar.orbit2d(3.7, 1.1, 0.0),
    clock = clock.scale(1.0),
    duration = MAIN,
}
expr.bind {
    object = light_b,
    target = Transform.channel.position,
    -- 旧 `Ramp2` 写 `Vec2`：`coerce::position3` 保留原 z（这里 = -1.6）。
    root = Vec3FromVec2 {
        v = Mix {
            a = Vec2Of { x = expr.pure(q(-6.5)), y = expr.pure(q(-1.1)) },
            b = Vec2Of { x = expr.pure(q(6.5)), y = expr.pure(q(-1.1)) },
            t = Clamp {
                x = Scale { k = 1.0 / MAIN, of = clock.time() },
                lo = expr.pure(0.0),
                hi = expr.pure(1.0),
            },
        },
        z = expr.pure(-1.6),
    },
    clock = clock.scale(1.0),
    duration = MAIN,
}
-- 中心立方体（半金属青绿）：缩放呼吸，突显粗糙度对镜面高光宽度的影响。
expr.bind {
    object = grid[5],
    target = Transform.channel.scale,
    root = sugar.wave(1.0, 0.18, 1.3),
    clock = clock.scale(1.0),
    duration = MAIN,
}
