-- sine_trace.lua — 小球随 Reveal 画出正弦曲线（**读端管道 + 显式时钟**驱动）。
--
-- 数据流是一条管道，没有任何逐段动画循环，也不依赖隐式游标（`scene.animate`）：
--
--   clock.time ──Scale(1/T)──写──> Reveal.end ──读──> Reveal.current_pos ──写──> 小球 Circle.center
--
--   · 书写进度 `Reveal.end = t / T`：**显式时钟**（`clock = clock.scale(1.0)`）
--     的局部时间经表达式算出，没有"当前游标"这种隐式状态；
--   · 小球不自己算曲线：它通过 **方法** `Reveal.current_pos` 读曲线对象的笔尖
--     （派生量 = 方法，不是字段 ⇒ 不可能被写端选中；`reads` 由方法目录静态声明）；
--   · 因此"小球在笔尖上"是**结构上成立**的，不依赖采样密度或插值巧合。
--
-- 脚本里唯一的循环是**几何采样**（生成正弦折线本身），与动画无关。
--
-- 能力使用：Reveal（S2 书写）+ `Reveal.end` / `Circle.center` 通道 +
-- `expr.method`（B2 方法调用）+ `clock`（B 批显式时钟）+ `Polyline`（静态坐标轴）。
-- 数值量化到 1e-4，保证跨平台逐位一致。

local TAU = 2.0 * math.pi

local PERIODS = 2.0 -- 画几个正弦周期
local AMP = 1.2     -- 振幅（世界单位）
local X0, X1 = -4.2, 4.2
local BASE_Y = 0.0  -- 中轴高度
local SAMPLES = 240 -- 折线点数（只需足够平滑；同步与密度无关）
local TOTAL_T = 6.0 -- 总时长（秒）

local function q(v)
    return math.floor(v * 10000.0 + 0.5) / 10000.0
end

local function rgba(r, g, b, a)
    return Color { r = r, g = g, b = b, a = a }
end

-- ── 相机：世界 ±5.3 × ±4 落在 640×480 里（scale = 60）────────────────
scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 0.0 }, scale = 60.0 },
})

-- ── 坐标轴：水平中轴 + 左侧起点竖线（静态折线，初态可见）──────────────
local axis_color = rgba(96, 110, 130, 255)
scene.spawn({
    Polyline {
        points = {
            Vec2 { x = X0 - 0.2, y = BASE_Y },
            Vec2 { x = X1 + 0.2, y = BASE_Y },
        },
        closed = false,
    },
    Stroke { width = 1.5, color = axis_color },
})
scene.spawn({
    Polyline {
        points = {
            Vec2 { x = X0, y = BASE_Y - AMP - 0.3 },
            Vec2 { x = X0, y = BASE_Y + AMP + 0.3 },
        },
        closed = false,
    },
    Stroke { width = 1.5, color = axis_color },
})

-- ── 几何采样：正弦折线（唯一的循环，纯数据准备）──────────────────────
local curve_points = {}
for i = 0, SAMPLES do
    local u = i / SAMPLES
    local x = X0 + (X1 - X0) * u
    local y = BASE_Y + AMP * math.sin(PERIODS * TAU * u)
    curve_points[#curve_points + 1] = Vec2 { x = q(x), y = q(y) }
end
local start_x = q(X0)
local start_y = q(BASE_Y)

-- ── 曲线：`Reveal` 初态 start = end = 0（不可见），由 `Reveal.end` 写出来 ──
local curve = scene.spawn({
    Reveal { points = curve_points, closed = false },
    Stroke { width = 3.0, color = rgba(120, 220, 255, 255) },
})
-- 书写进度：`end = t / T`。`t` 是**显式时钟**的局部时间（不再是隐式游标）。
-- `t > T` 后 `end` 继续增长，但渲染裁剪与 `Reveal::current_pos` 都会把 `end` 钳到 1
-- ⇒ 画面停在"整条曲线 + 球在终点"，不会越界。
expr.bind {
    object = curve,
    target = Reveal.channel["end"],
    root = Scale { k = 1.0 / TOTAL_T, of = Mul(clock.time(), clock.time()) },
    clock = clock.scale(1.0),
}

-- ── 小球：读端管道 —— 位置完全由 `Reveal.current_pos` 决定 ────────────
-- **方法**（B2）：`Reveal.current_pos(curve)` 是方法常量（等价于
-- `expr.method { name = "Reveal.current_pos", object = curve }`）。
-- 它不是字段，因此不可能被写端选中；`reads` 由方法目录静态声明。
local ball = scene.spawn({
    Circle { x = start_x, y = start_y, r = 0.16 },
    Fill { color = rgba(255, 210, 120, 255) },
    Stroke { width = 1.2, color = rgba(255, 245, 220, 255) },
})
expr.bind {
    object = ball,
    target = Circle.channel.center,
    root = Reveal.current_pos(curve),
}
