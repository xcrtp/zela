-- cap_second.lua — PE 能力展示：双 paint（tween 的另一端）。
--
-- 目的：给 `ze_render_ir::SecondPaint` 的场景层入口留一条作品级证据。
-- 语义：`mix = 0` 全用第一套 paint、`1` 全用第二套；中间是逐通道插值
-- （后端 `dash[3]` 就是这条混合进度）。
--
-- 判据：0 / 0.5 / 1 三态**互不相同**。三件并排即三态对照。
--
-- 静态（无动画）⇒ 只渲 1 帧；"tween"只体现在同一帧里的三种混合进度。

-- 2D 相机：世界 ±5.3 × ±4 落在 640×480 里（scale = 60）。
scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 0.0 }, scale = 60.0 },
})

local warm = Color { r = 235, g = 90, b = 80, a = 255 }
local cool = Color { r = 80, g = 120, b = 240, a = 255 }
local blank = Color { r = 0, g = 0, b = 0, a = 0 }

-- 第一套 paint = 圆的填充色（warm）；第二套 paint 的 fill = cool。
-- stroke / background 留全透明（本作品只演示 fill 的混合）。
local function tween(x, mix)
    scene.spawn({
        Circle { x = x, y = 0.0, r = 1.7 },
        Fill { color = warm },
        SecondPaint { fill = cool, stroke = blank, background = blank, mix = mix },
    })
end

tween(-3.2, 0.0)
tween(0.0, 0.5)
tween(3.2, 1.0)
