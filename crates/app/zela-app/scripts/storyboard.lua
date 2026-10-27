-- storyboard.lua — 现有分镜（zela-app/src/storyboard.rs）的**完整脚本化版本**。
--
-- 覆盖全部 6 个镜头 + T2 箭头 + T3 标注文字：
--   1. 常驻 Axes2D（容器；几何由 AxesRefreshSystem 每帧生成）
--   2. 2D 相机对象
--   3. 曲线书写 0–2s
--   4. wait 0.5s
--   5. 镜头推进 2.5–3.5s（scale + center 并行）
--   6. 公式书写 3.5–4.5s（逐轮廓依次；顺序由内容服务决定）
--
-- 数值与 storyboard.rs 逐项对应：
--   CAMERA_START = center (0, 0), scale 40
--   CAMERA_PEAK  = (π/2, 1), scale 110
--   曲线采样     = [-π, π] 上 128 段（129 个点）
--   公式         = "y = sin x"，字号 0.8，锚点 (1, 2)
--   标注文字     = "math animation"，字号 0.55，Transform (-1.5, 3, 0)
--   箭头         = (0.2, 2.1) → (π/2 − 0.15, 1.15)，头长 / 头宽 0.25

local N = 128
local FORMULA_WRITE_SECONDS = 1.0

-- `Ramp` / `Ramp2` 的算子化形态（显式时钟；`t` 是时钟局部时间）。
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

local function ramp2(ax, ay, bx, by, dur)
    return Mix {
        a = Vec2Of { x = expr.pure(ax), y = expr.pure(ay) },
        b = Vec2Of { x = expr.pure(bx), y = expr.pure(by) },
        t = Clamp {
            x = Scale { k = 1.0 / dur, of = clock.time() },
            lo = expr.pure(0.0),
            hi = expr.pure(1.0),
        },
    }
end

-- 旧的 `Step::Play` 区间：局部时间 `[0, dur)`（段外由 Hold 钳到两端）。
local function in_segment(dur)
    return Predicate.Of {
        expr = And {
            a = Ge { a = clock.time(), b = expr.pure(0.0) },
            b = Lt { a = clock.time(), b = expr.pure(dur) },
        },
    }
end

-- ── 分镜 1：常驻网格与坐标轴（Axes2D::unbounded() 的默认样式）──────────
scene.spawn({
    Axes2D {
        min_px = 10.0,
        grid = LineStyle { width = 1.0, color = Color { r = 50, g = 50, b = 50, a = 255 } },
        axis = LineStyle { width = 2.0, color = Color { r = 255, g = 255, b = 255, a = 255 } },
    },
})

-- ── 分镜 2：2D 相机 ────────────────────────────────────────────────────
local camera = scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 0.0 }, scale = 40.0 },
})

-- ── 分镜 3：曲线 y = sin x 的书写动画 ─────────────────────────────────
local points = {}
for i = 0, N do
    local x = -math.pi + i * (2 * math.pi / N)
    points[#points + 1] = Vec2 { x = x, y = math.sin(x) }
end
local curve = scene.spawn({
    Reveal { points = points, closed = false },
    Stroke { width = 2.0, color = Color { r = 0, g = 200, b = 200, a = 255 } },
})

-- ── 分镜 6：公式书写（每个描边子轮廓一个对象；顺序 = 内容服务的阅读顺序）──
local formula = content.formula(Formula {
    source = "y = sin x",
    font_size = 0.8,
    x = 1.0,
    y = 2.0,
})
local formula_handles = {}
for i = 1, #formula do
    formula_handles[i] = scene.spawn({ RevealPath { path = formula[i] } })
end

-- ── T3：标注文字（静态，经 Transform 摆放）────────────────────────────
local text = content.text(Text {
    source = "math animation",
    font_size = 0.55,
    x = 0.0,
    y = 0.0,
})
for i = 1, #text do
    scene.spawn({
        PathShape { path = text[i] },
        Transform { x = -1.5, y = 3.0, z = 0.0 },
    })
end

-- ── T2：箭头（实心，指向曲线峰值）────────────────────────────────────
scene.spawn({
    Arrow {
        from = Vec2 { x = 0.2, y = 2.1 },
        to = Vec2 { x = math.pi / 2 - 0.15, y = 1.0 + 0.15 },
        head_length = 0.25,
        head_width = 0.25,
    },
    Fill { color = Color { r = 255, g = 220, b = 120, a = 255 } },
})

-- ── 时间编排（显式时钟；旧 `scene.animate` / `parallel` / `wait` 的段序）────
-- 0–2s：曲线书写（phase(0.5) × 2s = 1）
expr.bind {
    object = curve,
    target = Reveal.channel["end"],
    root = Scale { k = 0.5, of = clock.time() },
    clock = clock.scale(1.0),
    predicate = in_segment(2.0),
}
-- 2–2.5s：停（没有绑定生效 ⇒ 上一步的值保持）
-- 2.5–3.5s：镜头推进（scale 与 center 同区间 —— 旧 `parallel` 的两条 action）
expr.bind {
    object = camera,
    -- `Framing2D` 是**存储组件**（`Camera2D` 是派生视图，没有自己的通道表），
    -- 因此这里显式构造通道键（与 `Camera2D { … }` 的构造器不是一回事）。
    target = Channel {
        namespace = "zela.scene",
        name = "Framing2D.scale",
        ty = ChannelType.Number,
    },
    root = ramp(40.0, 110.0, 1.0),
    clock = clock.shift(2.5),
    predicate = in_segment(1.0),
}
expr.bind {
    object = camera,
    target = Channel {
        namespace = "zela.scene",
        name = "Framing2D.center",
        ty = ChannelType.Vec2,
    },
    root = ramp2(0.0, 0.0, math.pi / 2, 1.0, 1.0),
    clock = clock.shift(2.5),
    predicate = in_segment(1.0),
}
-- 3.5–4.5s：公式逐轮廓依次书写（1s 平摊；轮廓数可能为 0）
if #formula_handles > 0 then
    local each = FORMULA_WRITE_SECONDS / #formula_handles
    for i = 1, #formula_handles do
        local start = 3.5 + (i - 1) * each
        expr.bind {
            object = formula_handles[i],
            target = Reveal.channel["end"],
            root = ramp(0.0, 1.0, each),
            clock = clock.shift(start),
            predicate = in_segment(each),
        }
    end
end
