-- cap_mix.lua — PF Step 2 能力展示：2D / 3D 混排（屏幕平面 overlay）。
--
-- 目的：证明同一帧里的 2D 记录**不再被整帧丢弃**，而是以 OVERLAY 位叠到 3D 帧上：
-- 3D 网格走透视，2D 图元固定压在**屏幕平面**（z 不参与投影）。
--
-- 判据（矢量后端用例 `overlay_path_lands_on_the_screen_plane_in_a_3d_frame` 钉住机制）：
-- - 3D 立方体在中景正常透视；
-- - 左上红圆 / 右下绿箭头在**固定屏幕位置**，尺寸不随 3D 相机 / 深度变化；
-- - 2D 内容盖在 3D 之上（overlay 记录在排序里被放在 3D 之后）。
--
-- 静态（无动画）⇒ 只渲 1 帧。

-- 2D 相机：只决定 2D 图元落在屏幕哪一处（overlay 不吃 3D 相机）。
scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 0.0 }, scale = 55.0 },
})

-- 3D 相机 + 立方体（Lambert ⇒ 可见明暗，证明它走的是透视而不是屏幕平面）。
scene.spawn({
    Camera3D {
        eye = Vec3 { x = 3.2, y = 2.4, z = 4.2 },
        target = Vec3 { x = 0.0, y = 0.0, z = 0.0 },
        up = Vec3 { x = 0.0, y = 1.0, z = 0.0 },
        fov_y = 0.9,
    },
})
scene.spawn({
    Cube { half_extent = 1.4, solid = true },
    Material3D {
        color = Color { r = 235, g = 120, b = 120, a = 255 },
        variant = ShaderVariant.Lambert,
        metallic = 0.0,
        roughness = 0.5,
        double_sided = false,
        transparent = false,
        texture = Texture.None,
    },
})
scene.spawn({
    Light {
        kind = LightKind.Directional {
            direction = Vec3 { x = 0.3, y = -0.5, z = -0.8 },
        },
        color = Color { r = 255, g = 245, b = 230, a = 255 },
        intensity = 1.1,
    },
})

-- 2D 图元（屏幕平面 overlay）：世界坐标经 Camera2D 落到屏幕上。
scene.spawn({
    Circle { x = -4.6, y = 3.0, r = 0.8 },
    Fill { color = Color { r = 90, g = 220, b = 160, a = 255 } },
})
scene.spawn({
    Arrow {
        from = Vec2 { x = 3.2, y = -3.0 },
        to = Vec2 { x = 5.0, y = -1.2 },
        head_length = 0.6,
        head_width = 0.4,
    },
    Stroke { width = 5.0, color = Color { r = 120, g = 180, b = 255, a = 255 } },
})
