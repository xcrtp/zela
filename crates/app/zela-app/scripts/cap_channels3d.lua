-- cap_channels3d.lua — F 批新字段通道（3D）：PBR 金属度 / 粗糙度 / 光照强度。
--
-- 目的：给 F 批注册的三个 3D 标量通道留作品级证据（2D 部分见 cap_channels2d.lua）。
-- 覆盖：`Material3D.metallic` / `Material3D.roughness` / `Light.intensity`。
--
-- 画面：两个立方体（左：电介质→金属，右：金属→哑光），一盏点光从暗到亮
-- 把两者的高光变化同时照亮 —— 三个通道在同一条时间轴上互相印证。
--
-- 判据：整段 3s 播放完，左立方体从哑光介质变成亮面金属（高光增强），
-- 右立方体相反；点光强度 0.4 → 2.2（整体变亮）。
-- 数值量化到 1e-4。

local T = 3.0

local function rgba(r, g, b, a)
    return Color { r = r, g = g, b = b, a = a }
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

-- 旧 `Step::Play` 区间：局部时间 `[0, T)`（段外由 Hold 钳到两端）。
local WINDOW = Predicate.Of {
    expr = And {
        a = Ge { a = clock.time(), b = expr.pure(0.0) },
        b = Lt { a = clock.time(), b = expr.pure(T) },
    },
}

local function bind(object, target, root)
    expr.bind {
        object = object,
        target = target,
        root = root,
        clock = clock.scale(1.0),
        predicate = WINDOW,
    }
end

-- 相机：略微俯视，两个立方体都在视野内。
scene.spawn({
    Camera3D {
        eye = Vec3 { x = 0.0, y = 1.6, z = 6.4 },
        target = Vec3 { x = 0.0, y = 0.0, z = 0.0 },
        up = Vec3 { x = 0.0, y = 1.0, z = 0.0 },
        fov_y = 1.0,
    },
})

-- 点光：强度由暗到亮（唯一被驱动的标量）。
local key = scene.spawn({
    Light {
        kind = LightKind.Point { range = 8.0 },
        color = rgba(255, 245, 230, 255),
        intensity = 0.4,
    },
    Transform { x = 1.6, y = 2.4, z = 2.0 },
})

-- 立方体：半边 0.55，实体；材质参数随后由通道驱动。
local function cube(x, metallic, roughness, color)
    return scene.spawn({
        Cube { half_extent = 0.55, solid = true },
        Material3D {
            color = color,
            variant = ShaderVariant.Pbr,
            metallic = metallic,
            roughness = roughness,
            double_sided = false,
            transparent = false,
            texture = Texture.None,
        },
        Transform3D {
            position = Vec3 { x = x, y = 0.0, z = 0.0 },
            rotation = Vec4 { x = 0.0, y = 0.0, z = 0.0, w = 1.0 },
            scale = Vec3 { x = 1.0, y = 1.0, z = 1.0 },
        },
    })
end

local to_metal = cube(-1.2, 0.0, 0.5, rgba(220, 226, 235, 255))
local to_matte = cube(1.2, 0.9, 0.3, rgba(215, 120, 80, 255))

-- 左：电介质 → 金属（metallic 0 → 1），同时抛光（roughness 0.5 → 0.15）。
bind(to_metal, Material3D.channel.metallic, ramp(0.0, 1.0, T))
bind(to_metal, Material3D.channel.roughness, ramp(0.5, 0.15, T))

-- 右：金属 → 电介质（metallic 0.9 → 0.1），同时变哑（roughness 0.3 → 0.8）。
bind(to_matte, Material3D.channel.metallic, ramp(0.9, 0.1, T))
bind(to_matte, Material3D.channel.roughness, ramp(0.3, 0.8, T))

-- 点光：0.4 → 2.2（暗场到亮场）。
bind(key, Light.channel.intensity, ramp(0.4, 2.2, T))
