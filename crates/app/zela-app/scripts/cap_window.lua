-- cap_window.lua — PE 能力展示：曲线窗口（部分路径）。
--
-- 目的：给 `ze_render_ir::CurveWindow` 的场景层入口留一条作品级证据。
-- ⚠️ 单位是**曲线索引 + t**（后端 `params[0..2]`），不是像素：
-- `[0, 1]` = 只显示第 0 条曲线（对圆 = 四分之一弧）。
--
-- 判据：窗口收窄 ⇒ 可见段数减少（整圆 → 四分之一）。
-- 左：整圆；右：同一几何 + 窗口 [0, 1]。
--
-- 静态（无动画）⇒ 只渲 1 帧。

-- 2D 相机：世界 ±5.3 × ±4 落在 640×480 里（scale = 60）。
scene.spawn({
    Camera2D { scale = 60.0 },
})

local fill = Color { r = 220, g = 120, b = 220, a = 255 }

-- 对照组：整条路径。
scene.spawn({
    Circle { x = -2.8, y = 0.0, r = 1.6 },
    Fill { color = fill },
})

-- 实验组：只显示第 0 条曲线（圆 → 四分之一）。
scene.spawn({
    Circle { x = 2.8, y = 0.0, r = 1.6 },
    Fill { color = fill },
    CurveWindow { lo = 0.0, hi = 1.0 },
})
