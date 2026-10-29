-- smoke.lua — 短时对拍脚本（smoke test，**不是**完整基线的等价替代）。
--
-- 用途：PR / `cargo verify --fast` 的快速反馈。15 帧 @ 30fps（0.5s），320×240。
-- 覆盖：Camera2D / Circle Fill + 半径动画 / Reveal 折线书写。
--
-- 边界（不得省略）：
--   短路径抓不到跨端浮点分歧。已知分歧首次出现在 fourier 第 103 帧；
--   本脚本只渲前 15 帧 ⇒ 它绿**不代表**跨端确定性没问题。
--   完整基线（fourier / storyboard）必须定期跑（main push / nightly）。
--
-- 本脚本不是网页展示用脚本，与未来的 scripts/demo/ 无关，不进 demo 清单。

local T = 0.5
local N = 32

scene.spawn({
    Camera2D { scale = 40.0 },
})

local dot = scene.spawn({
    Circle { x = -0.8, y = 0.0, r = 0.8 },
    Fill { color = Color { r = 80, g = 200, b = 255, a = 255 } },
})

local points = {}
for i = 0, N do
    local x = -2.0 + 4.0 * i / N
    points[#points + 1] = Vec2 { x = x, y = 0.7 * math.sin(x * 1.5) }
end
local curve = scene.spawn({
    Reveal { points = points, closed = false },
    Stroke { width = 2.0, color = Color { r = 255, g = 220, b = 120, a = 255 } },
})

-- 动画：`Ramp` 的算子化形态 —— `mix(from, to, clamp(t / dur, 0, 1))`。
-- `t` 是**显式时钟**的局部时间（第 5a 批：不再有 `scene.animate` 的隐式游标）。
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

expr.bind {
    object = dot,
    target = Circle.channel.radius,
    root = ramp(0.8, 1.6, T),
    clock = clock.scale(1.0),
    duration = T,
}
expr.bind {
    object = curve,
    target = Reveal.channel["end"],
    root = ramp(0.0, 1.0, T),
    clock = clock.scale(1.0),
    duration = T,
}
