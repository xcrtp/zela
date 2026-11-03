-- cap_shader_alpha.lua — BT-4 能力展示：着色器给的**透明度**（Vec4 出口的 w）真的生效
--
-- 判据：三个圆**几何相同**，只有着色器返回的颜色 / 透明度不同 ——
--   左上：`vec3` 程序（无透明度，alpha 记 1.0）；
--   右上：同样的颜色，`vec4` 程序的 w = 0.35（半透明 ⇒ 屏幕像素明显更暗）；
--   下方：w 随像素变化（`expr.uv()` 的 x），透明度从左到右渐变。
-- 装饰性：不改几何 / 状态，也不进结构指纹；路由到 wgpu 时决策理由会写"已忽略：着色 codegen"。
-- 静态（无动画）⇒ 只渲 1 帧。

-- 2D 相机：世界 ±5.3 × ±4 落在 640×480 里（scale = 60）。
scene.spawn({
    Camera2D { scale = 60.0 },
})

-- 对照组：不透明（vec3 程序 ⇒ 后端把 alpha 记作 1.0）。
local opaque = scene.spawn({
    Circle { x = -2.6, y = 1.2, r = 1.5 },
    Fill { color = Color { r = 10, g = 20, b = 30, a = 255 } },
})
expr.shader {
    object = opaque,
    root = Vec3Of {
        x = expr.pure(0.20),
        y = expr.pure(0.65),
        z = expr.pure(0.95),
    },
}

-- 实验组：同一颜色，着色器给 0.35 的透明度（记录里仍是 a = 255）。
local translucent = scene.spawn({
    Circle { x = 2.6, y = 1.2, r = 1.5 },
    Fill { color = Color { r = 10, g = 20, b = 30, a = 255 } },
})
expr.shader {
    object = translucent,
    root = Vec4Of {
        x = expr.pure(0.20),
        y = expr.pure(0.65),
        z = expr.pure(0.95),
        w = expr.pure(0.35),
    },
}

-- 逐片元组：透明度随像素变（`expr.uv()` 的 x：包围盒内 0..1）。
local fading = scene.spawn({
    Circle { x = 0.0, y = -1.8, r = 1.5 },
    Fill { color = Color { r = 10, g = 20, b = 30, a = 255 } },
})
expr.shader {
    object = fading,
    root = Vec4Of {
        x = expr.pure(0.90),
        y = expr.pure(0.45),
        z = expr.pure(0.20),
        w = expr.x { v = expr.uv() },
    },
}
