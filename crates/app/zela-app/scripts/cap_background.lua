-- cap_background.lua — PE 能力展示：背景描边（第三色）。
--
-- 目的：给 `ze_render_ir::BackgroundStroke` 的场景层入口留一条作品级证据。
-- 语义：画在**填充之下**的第三色描边，宽度是场景单位（后端 `params[3]`）。
--
-- 判据：开启 ⇒ **填充之外**出现像素，且填充本身不变（颜色 / 面积都不变）。
-- 左：只有填充；右：同色填充 + 暖色背景描边（一圈光晕）。
--
-- 静态（无动画）⇒ 只渲 1 帧。

-- 2D 相机：世界 ±5.3 × ±4 落在 640×480 里（scale = 60）。
scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 0.0 }, scale = 60.0 },
})

local fill = Color { r = 70, g = 190, b = 210, a = 255 }
local halo = Color { r = 255, g = 205, b = 90, a = 255 }

-- 对照组：无背景描边。
scene.spawn({
    Circle { x = -2.8, y = 0.0, r = 1.5 },
    Fill { color = fill },
})

-- 实验组：同一几何 / 同一填充，只多挂一个背景描边。
scene.spawn({
    Circle { x = 2.8, y = 0.0, r = 1.5 },
    Fill { color = fill },
    BackgroundStroke { color = halo, width = 0.6 },
})
