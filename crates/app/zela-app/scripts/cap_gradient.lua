-- cap_gradient.lua — PE 能力展示：渐变（含多 stop）。
--
-- 目的：给 `ze_render_ir::Gradient` 的场景层入口留一条作品级证据。
-- 后端语义（不得由本作品改写）：色标沿一条由两个端点定义的轴**均匀分布**，
-- 多 stop 是 `paint.wgsl` 的 `stops(first, second, count, m, t)` 原生能力。
--
-- 判据（GPU 用例 `gradient_stops_vary_along_the_axis` 已钉住）：
-- 2 stop 与 3 stop 的**中点颜色不同**（3 stop 的中点是绿色）。
-- 本作品把两者并排摆，肉眼即是对照组。
--
-- 端点坐标是**世界坐标**（与图元同一空间）；录制层用与几何同一份相机仿射
-- 换算到屏幕，再由矢量映射换算到后端世界坐标（`zela-render-vector::scene`）。
--
-- 静态（无动画）⇒ 只渲 1 帧。

-- 2D 相机：世界 ±5.3 × ±4 落在 640×480 里（scale = 60）。
scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 0.0 }, scale = 60.0 },
})

local red = Color { r = 230, g = 60, b = 70, a = 255 }
local green = Color { r = 60, g = 220, b = 120, a = 255 }
local blue = Color { r = 70, g = 110, b = 240, a = 255 }
local white = Color { r = 255, g = 255, b = 255, a = 255 }

-- 左：2 stop（红 → 蓝）。中点是两者之间（偏紫）。
scene.spawn({
    Circle { x = -2.6, y = 0.0, r = 1.7 },
    Fill { color = white },
    Gradient {
        stops = { red, blue },
        from = Vec3 { x = -4.3, y = 0.0, z = 0.0 },
        to = Vec3 { x = -0.9, y = 0.0, z = 0.0 },
    },
})

-- 右：3 stop（红 → 绿 → 蓝）。中点是**绿色**（与左侧中点可分辨）。
scene.spawn({
    Circle { x = 2.6, y = 0.0, r = 1.7 },
    Fill { color = white },
    Gradient {
        stops = { red, green, blue },
        from = Vec3 { x = 0.9, y = 0.0, z = 0.0 },
        to = Vec3 { x = 4.3, y = 0.0, z = 0.0 },
    },
})
