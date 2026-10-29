-- cap_dash.lua — PE 能力展示：虚线。
--
-- 目的：给 `ze_render_ir::Dash` 的场景层入口留一条作品级证据。
-- ⚠️ 单位是**曲线参数 u**（1 = 一整条曲线），不是屏幕像素 —— 这是后端语义，
-- 本作品不换算，也不假装它是像素。
--
-- 判据：duty（每个周期里显示的比例）变化 ⇒ 覆盖像素数**单调**减少。
-- 三个圆用同一几何、同一周期，只改 duty。
--
-- 静态（无动画）⇒ 只渲 1 帧。

-- 2D 相机：世界 ±5.3 × ±4 落在 640×480 里（scale = 60）。
scene.spawn({
    Camera2D { scale = 60.0 },
})

local ink = Color { r = 220, g = 235, b = 255, a = 255 }

-- 三个描边圆：duty 1.0（实线对照）→ 0.6 → 0.25。
-- 圆是 4 条三次曲线 ⇒ 周期 0.05 在每条曲线上约 5 段，足够看出虚线。
local function dashed(x, duty)
    scene.spawn({
        Circle { x = x, y = 0.0, r = 1.8 },
        Stroke { width = 5.0, color = ink },
        Dash { period = 0.05, duty = duty, phase = 0.0 },
    })
end

dashed(-3.2, 1.0)
dashed(0.0, 0.6)
dashed(3.2, 0.25)
