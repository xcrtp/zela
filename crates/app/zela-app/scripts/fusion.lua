-- fusion.lua — B2 新能力演示：双面 + 透明桶（D19 意图层 / B2-2 + B2-3）。
--
-- 用途：用小而聚焦的场景展示 **作者意图** 的两个新字段：
--   - `double_sided = true`：从背面也可见（纸片/开放几何）；
--   - `transparent = true`：进入透明桶，由统一 ZOrder 决定混合顺序。
-- 场景刻意让两个半透明立方体**相交**：不透明参考体在下方，透明体叠加后
-- 能直接看出混合顺序与双面可见性。
--
-- D19：脚本只出现 `double_sided` / `transparent`，不出现 cull_mode / blend /
-- sample_count（GPU 机制名不得进入脚本 API）。

local T = 3.0

-- `Ramp` 的算子化形态：`mix(from, to, clamp(t / dur, 0, 1))`（显式时钟）。
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

-- 半透明红色立方体（双面）。
local red = scene.spawn({
    Cube { half_extent = 1.0, solid = true },
    Material3D {
        color = Color { r = 255, g = 90, b = 90, a = 140 },
        variant = ShaderVariant.Flat,
        metallic = 0.0,
        roughness = 0.5,
        double_sided = true,
        transparent = true,
        texture = Texture.None,
    },
    Transform3D {
        position = Vec3 { x = -0.6, y = 0.0, z = 0.0 },
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = Vec3 { x = 1.0, y = 1.0, z = 1.0 },
    },
})

-- 半透明蓝色立方体（双面），与红色相交。
local blue = scene.spawn({
    Cube { half_extent = 1.0, solid = true },
    Material3D {
        color = Color { r = 90, g = 140, b = 255, a = 140 },
        variant = ShaderVariant.Flat,
        metallic = 0.0,
        roughness = 0.5,
        double_sided = true,
        transparent = true,
        texture = Texture.None,
    },
    Transform3D {
        position = Vec3 { x = 0.6, y = 0.0, z = 0.0 },
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = Vec3 { x = 1.0, y = 1.0, z = 1.0 },
    },
})

-- 不透明参考体（下方）。
scene.spawn({
    Cube { half_extent = 0.8, solid = true },
    Material3D {
        color = Color { r = 230, g = 230, b = 230, a = 255 },
        variant = ShaderVariant.Lambert,
        metallic = 0.0,
        roughness = 0.5,
        double_sided = false,
        transparent = false,
        texture = Texture.None,
    },
    Transform3D {
        position = Vec3 { x = 0.0, y = -1.8, z = 0.0 },
        rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
        scale = Vec3 { x = 1.0, y = 1.0, z = 1.0 },
    },
})

-- 两个透明体反向旋转（让相交区域的混合顺序变化可见）。
-- 旧 `scene.animate` 是**顺序**追加的段：红体在 [0, T)，蓝体在 [T, 2T)；
-- 这里用显式时钟还原同一时间轴（蓝体的时钟整体后移 T，predicate 限定本段）。
expr.bind {
    object = red,
    target = Transform.channel.rotation,
    root = ramp(0.0, 6.28318530718, T),
    clock = clock.scale(1.0),
    duration = T,
}
expr.bind {
    object = blue,
    target = Transform.channel.rotation,
    root = ramp(6.28318530718, 0.0, T),
    clock = clock.shift(T),
    predicate = Predicate.Of {
        expr = And {
            a = Ge { a = clock.time(), b = expr.pure(0.0) },
            b = Lt { a = clock.time(), b = expr.pure(T) },
        },
    },
}

-- 方向光（方向 = 光传播方向，世界空间）。
scene.spawn({
    Light {
        kind = LightKind.Directional {
            direction = Vec3 { x = 0.3, y = -0.5, z = -0.8 },
        },
        color = Color { r = 255, g = 245, b = 230, a = 255 },
        intensity = 1.1,
    },
})

-- 3D 相机。
scene.spawn({
    Camera3D {
        eye = Vec3 { x = 4.5, y = 3.0, z = 6.0 },
        target = Vec3 { x = 0.0, y = -0.2, z = 0.0 },
        up = Vec3 { x = 0.0, y = 1.0, z = 0.0 },
        fov_y = 1.05,
    },
})

-- PF Step 2：纯 3D 场景不再需要占位 2D 相机（`Framing2D` 可缺席）。

