-- cap_shader_3d.lua — BT-6 能力展示：3D 视图里的程序化材质（屏幕空间内建）
--
-- BT-0 已核实：逐片元入口没有 per-pixel 的 `world_pos` / `normal`
-- （见 docs/research/BT_前置核实.md §2），所以这里用**屏幕空间内建**
-- （`uv` / `pos2` / `center` / `size`）—— 它们在 3D 视图里同样可用：
-- 2D 记录按屏幕平面 overlay 叠加进 3D 帧（PF Step 2 的接线）。
-- `world_pos` / `normal` 作为缺口记账，后续要做得先给逐片元入口补 per-pixel 3D 属性。
--
-- 画面：3D 场景（相机 + 立方体 + 方向光）上叠一块屏幕空间平面，
-- 颜色随像素位置变化（`uv` 的横向渐变 + `fract` 条纹），证明"材质在 3D 帧里是活的"。
-- 装饰性：不改几何 / 状态，也不进结构指纹；静态（无动画）⇒ 只渲 1 帧。

-- 3D 相机（aspect 由渲染视口覆盖，脚本不传）。
scene.spawn({
    Camera3D {
        eye = scene.vec3(0.4, 1.2, 6.2),
        fov_y = 1.0,
    },
})

-- 2D 相机：屏幕空间圆按它定位（scale = 60 ⇒ 世界 ±5.3 × ±4 落在 640×480 里）；
-- 没有它 2D 图元不会被录制。
scene.spawn({
    Camera2D { scale = 60.0 },
})

-- 方向光：让立方体有明暗面（3D 帧的参照物）。
scene.spawn({
    Light {
        kind = LightKind.Directional {
            direction = scene.vec3(0.4, -0.6, -0.7),
        },
        color = Color { r = 255, g = 240, b = 220, a = 255 },
        intensity = 1.1,
    },
})

-- 立方体：3D 参照物（屏幕空间平面叠在它前面）。
scene.spawn({
    Cube { half_extent = 1.0, solid = true },
    Material3D {
        color = Color { r = 220, g = 226, b = 235, a = 255 },
        roughness = 0.55,
    },
    Transform3D {
        position = scene.vec3(1.6, 0.2, 0.0),
        rotation = Vec4 { x = 0.0, y = 0.38268343, z = 0.0, w = 0.92387953 },
        scale = scene.vec3(1.0, 1.0, 1.0),
    },
})

-- 屏幕空间平面：圆 + 逐片元材质（2D 记录 ⇒ 3D 帧里是 overlay）。
local plane = scene.spawn({
    Circle { x = -2.6, y = 0.0, r = 1.6 },
    Fill { color = Color { r = 10, g = 20, b = 30, a = 255 } },
})

-- 颜色 = (uv.x, uv.y, 0.5 + 0.5 * fract(6 * uv.x))：位置相关 + 程序化条纹。
expr.shader {
    object = plane,
    root = Vec4Of {
        x = expr.x { v = expr.uv() },
        y = expr.y { v = expr.uv() },
        z = Add {
            a = expr.pure(0.5),
            b = Scale {
                k = 0.5,
                of = Fract {
                    of = Scale { k = 6.0, of = expr.x { v = expr.uv() } },
                },
            },
        },
        w = expr.pure(1.0),
    },
}
