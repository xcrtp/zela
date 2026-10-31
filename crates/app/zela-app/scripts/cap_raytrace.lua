-- cap_raytrace.lua — BV-5 能力展示：3D 视图的 brute-force 光追（矢量后端）
--
-- 开关：`RayTrace {}` 与 `Camera3D` 挂在**同一个对象**上 —— 这是"这个视图跑光追"的
-- 唯一入口；缺席 = 只光栅（既有作品逐位不变）。wgpu 没有光追 pass，路由会**显式拒绝**
-- 这件作品（不静默丢效果），所以它只进 `[vector]` 基线。
--
-- 规模刻意压到最小：光追是每像素 O(三角形) 的 brute-force，对拍按 640×480 跑。
-- BV-0 的实测（本机 37 ns / 千次求交）下，**两个实体立方体 = 24 个三角形** ⇒ 每帧
-- 640×480×24 ≈ 7.4M 次求交；CI（lavapipe，按 ×100 外推）也在 30 ms/帧量级，
-- 而本作品**静态**（无 timeline ⇒ 只渲 1 帧），总成本可忽略。
-- 记录的偏离：计划里写的"96×72 + ≤256 三角形"是按"改对拍分辨率"设计的；
-- 实际 `zela-verify` 的作品尺寸固定在 640×480（`HASHES_WORKS`），所以改用
-- "三角形数 ≤ 24"来控成本（GAPS BV-05）。
--
-- 画面：暖色金属立方体（低粗糙度，光追的高光与光栅明显不同）+ 冷色哑光立方体，
-- 一盏方向光。两个立方体的材质差异让"光追真的跑了"在像素上有清晰判据。

-- 3D 相机（aspect 由渲染视口覆盖，脚本不传）。
scene.spawn({
    Camera3D {
        eye = scene.vec3(0.0, 0.8, 6.0),
        fov_y = 1.0,
    },
    -- BV-5：该视图跑光追。
    RayTrace { enabled = true },
})

-- 方向光（与 demo3d 同参：`direction` 是"指向光源"）。
scene.spawn({
    Light {
        kind = LightKind.Directional {
            direction = scene.vec3(0.4, -0.6, -0.7),
        },
        color = Color { r = 255, g = 240, b = 220, a = 255 },
        intensity = 1.2,
    },
})

-- 左：暖色金属（镜面强、粗糙度低 —— 光追与光栅的高光差异最明显）。
scene.spawn({
    Cube { half_extent = 1.1, solid = true },
    Material3D {
        color = Color { r = 220, g = 120, b = 60, a = 255 },
        variant = ShaderVariant.Pbr,
        metallic = 0.9,
        roughness = 0.15,
    },
    Transform3D {
        position = scene.vec3(-1.7, 0.0, 0.0),
        rotation = Vec4 { x = 0.0, y = 0.38268343, z = 0.0, w = 0.92387953 },
        scale = scene.vec3(1.0, 1.0, 1.0),
    },
})

-- 右：冷色哑光（粗糙度高 ⇒ 漫反射主导，用来对照"不是所有像素都被高光改写"）。
scene.spawn({
    Cube { half_extent = 1.1, solid = true },
    Material3D {
        color = Color { r = 90, g = 150, b = 210, a = 255 },
        variant = ShaderVariant.Pbr,
        metallic = 0.0,
        roughness = 0.85,
    },
    Transform3D {
        position = scene.vec3(1.7, 0.0, 0.0),
        rotation = Vec4 { x = 0.0, y = -0.38268343, z = 0.0, w = 0.92387953 },
        scale = scene.vec3(1.0, 1.0, 1.0),
    },
})
