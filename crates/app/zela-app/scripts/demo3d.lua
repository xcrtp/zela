-- demo3d.lua — 3D 场景脚本（B1-D：脚本层 3D 入口）。
--
-- 用途：验证 Camera3D / Cube(solid) / Transform3D / Material3D / Light 的
-- 脚本路径（`scene.camera3d` 等）与 ZSB bundle 往返逐字节一致
-- （`bundle_roundtrip_matches_script_path_3d`）。
-- 覆盖：相机、方向光 + 点光、实体立方体（PBR + 旋转缩放）、线框立方体，
--       以及 3D 摆放动画（Rotation / Position 并行 + Scale 缩放）。
--
-- 唯一来源：点光**不带 position** —— 位置由同一对象的 `Transform` 提供。

local T = 2.0

-- `Ramp` / `Ramp2` 的算子化形态（显式时钟；`t` 是时钟局部时间）。

-- 旧 `Step::Play` 区间：局部时间 `[0, dur)`（段外由 Hold 钳到两端）。

-- 实体立方体（PBR 材质）+ 摆放（旋转 + 缩放 + 平移）。
local cube = scene.spawn({
    Cube { half_extent = 1.2, solid = true },
    Material3D {
        color = Color { r = 220, g = 120, b = 60, a = 255 },
        variant = ShaderVariant.Pbr,
        metallic = 0.3,
        roughness = 0.6,
    },
    Transform3D {
        position = scene.vec3(0.0, 0.0, 0.0),
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = scene.vec3(1.0, 1.0, 1.0),
    },
})

-- 实体立方体动画：绕 Z 旋转 + XY 平面平移（同一段 [0, T)，互不覆盖）。
expr.bind {
    object = cube,
    target = Transform.channel.rotation,
    root = sugar.ramp(0.0, 6.28318530718, T),
    clock = clock.scale(1.0),
    duration = T,
}
-- `Transform.position` 的字段类型是 `Vec3`；旧的 `Ramp2`（Vec2）挡位在这里
-- **显式补 z = 0**（与 `coerce::position3` 保留原 z 的口径一致：该对象 z 恒 0）。
expr.bind {
    object = cube,
    target = Transform.channel.position,
    root = Vec3FromVec2 {
        v = sugar.ramp2(-1.5, -1.0, 1.5, 1.0, T),
        z = expr.pure(0.0),
    },
    clock = clock.scale(1.0),
    duration = T,
}

-- 线框立方体（Lambert），旋转 45° 并缩放，验证 Transform3D 的旋转 / 缩放字段。
local wire = scene.spawn({
    Cube { half_extent = 0.8, solid = false },
    Material3D {
        color = Color { r = 80, g = 200, b = 255, a = 255 },
    },
    Transform3D {
        position = scene.vec3(-2.8, 1.0, 0.0),
        rotation = Vec4 { x = 0.0, y = 0.38268343, z = 0.0, w = 0.92387953 },
        scale = scene.vec3(1.5, 0.8, 1.0),
    },
})

-- 线框立方体动画：统一缩放脉冲（旧 `scene.animate` 的第二段 ⇒ 时钟后移 T）。
expr.bind {
    object = wire,
    target = Transform.channel.scale,
    root = sugar.ramp(0.8, 1.6, T),
    clock = clock.shift(T),
    duration = T,
}

-- 方向光（方向是光传播方向，世界空间）。
local key_light = scene.spawn({
    Light {
        kind = LightKind.Directional {
            direction = scene.vec3(0.4, -0.6, -0.7),
        },
        color = Color { r = 255, g = 240, b = 220, a = 255 },
        intensity = 1.2,
    },
})

-- 点光：位置由同对象的 Transform 提供（不写在 Light 上）。
local rim = scene.spawn({
    Light {
        kind = LightKind.Point { range = 6.0 },
        color = Color { r = 120, g = 180, b = 255, a = 255 },
        intensity = 0.8,
    },
    Transform { x = 3.0, y = 2.0, z = -2.0 },
})

-- 3D 相机（aspect 由渲染视口覆盖，脚本不传）。
scene.spawn({
    Camera3D {
        eye = scene.vec3(4.0, 3.0, 5.0),
        fov_y = 1.1,
    },
})

-- PF Step 2：**不再需要占位 2D 相机**。纯 3D 场景可以没有 `Framing2D`
-- （渲染层不再把"2D 通道"当成整帧前置；有 2D 图元却没相机才报错）。
