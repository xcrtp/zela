-- spring_state.lua — 阻尼弹簧（BX：单帧 State + 二维元组 + 仿射识别）
--
-- 能力使用：
--   1. `expr.state { init, step }`：语义是 `f^frame(init)`（单帧纯函数，seek 不需要历史）。
--      轮数 = 宿主帧号（`TimeState.frame`），**不是**局部时间；
--      带 `clock.shift` 的绑定仍按全局帧推进（本批显式口径）。
--   2. `step` 关于 `expr.rec_prev()` **仿射**（半隐式欧拉）：
--        x' = x + v
--        v' = 0.97·v − 0.02·x
--      装配期由 `scene.state_affine` 识别出 (A, b)；非线性写法会被显式拒绝。
--      （稳定域是 `d + k < 1`；0.97 + 0.02 = 0.99 ⇒ 真阻尼，不会发散。）
--   3. `expr.nat_rec`：参考线高度 = 4 × 0.15（装配期采样成折线，不需要重放）。
--
-- 三条水平轨道各一只球，初速不同 ⇒ 相位不同；阻尼 0.97/帧 ⇒ 约 4 秒收敛。

local STIFF = 0.02
local DAMP = 0.97
local DURATION = 4.0

scene.spawn({
    Camera2D { center = scene.vec2(0.0, 0.7), scale = 92.0 },
})

-- 参考线：y = 0.60，由 `expr.nat_rec` 折叠 4 次（4 × 0.15）得到。
-- 这是 `nat_rec` 的**第一个脚本侧消费者**：plot 在装配期采样，产物里是折线，
-- 不进绑定 ⇒ 不触发 AI-07 的 Replay 拒绝。
local guide = scene.spawn({
    Stroke { width = 1.0, color = scene.rgba(96, 108, 140, 255) },
})
expr.plot {
    object = guide,
    lo = -2.8,
    hi = 2.8,
    samples = 2,
    y = expr.nat_rec {
        count = 4.0,
        init = expr.pure(0.0),
        step = Add { a = expr.rec_prev(), b = expr.pure(0.15) },
    },
}

-- 二维状态 (x, v)：半隐式欧拉；每一步都是 add / scale / 分量读取（仿射）。
local function spring(init_x, init_v)
    local prev = expr.rec_prev()
    local x = expr.x { v = prev }
    local v = expr.y { v = prev }
    return expr.state {
        init = expr.vec2 { x = expr.pure(init_x), y = expr.pure(init_v) },
        step = expr.vec2 {
            x = Add { a = x, b = v },
            y = Add {
                a = Scale { k = DAMP, of = v },
                b = Scale { k = -STIFF, of = x },
            },
        },
    }
end

-- 一条轨道：静平衡点 + 球（球心 = (状态 x, 轨道 y)）。
local function track(y, init_x, init_v, color)
    scene.spawn({
        Circle { x = 0.0, y = y, r = 0.045 },
        Fill { color = scene.rgba(150, 160, 190, 255) },
    })
    local ball = scene.spawn({
        Circle { x = init_x, y = y, r = 0.20 },
        Fill { color = color },
        Stroke { width = 1.5, color = scene.rgba(28, 34, 48, 255) },
    })
    local state = spring(init_x, init_v)
    expr.bind {
        object = ball,
        target = Circle.channel.center,
        root = expr.vec2 {
            x = expr.x { v = state },
            y = expr.pure(y),
        },
        duration = DURATION,
    }
end

track(1.45, 1.90, 0.00, scene.rgba(255, 128, 128, 255))
track(0.60, 1.50, 0.06, scene.rgba(120, 220, 140, 255))
track(-0.25, 1.10, -0.06, scene.rgba(140, 172, 255, 255))
