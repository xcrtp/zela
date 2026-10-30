-- spring.lua — 弹跳小球（S1 多段序列 + 挤压-拉伸，全局游标编排）。
--
-- 画面：地面线 + 三只小球经 `wait` 错峰弹跳。
-- 能力使用：
--   Position 通道 · Ramp2 —— 下落 / 弹起（同一对象多段序列，S1 段语义）；
--   Scale 通道 · Ramp —— 落地挤压（1.0→0.7）、离地拉伸（0.7→1.15）、回落归一；
--   S6 Opacity —— 小球淡入；
--   Camera2DFraming —— 收尾推近。
-- 数值全部量化到 1e-4，保证跨平台逐位一致。

local FLOOR_Y = 0.3
local HOP_HI = 1.6
local DOWN = 0.5
local UP = 0.5
local SETTLE = 0.3

local function q(v)
    return math.floor(v * 10000.0 + 0.5) / 10000.0
end

-- `Ramp` / `Ramp2` 的算子化形态（显式时钟；`t` 是时钟局部时间）。

-- `Transform.position` 的字段类型是 `Vec3`；旧的 `Ramp2`（Vec2）在这里显式补 z。
local function move2(ax, ay, bx, by, dur)
    return Vec3FromVec2 {
        v = Mix {
            a = Vec2Of { x = expr.pure(ax), y = expr.pure(ay) },
            b = Vec2Of { x = expr.pure(bx), y = expr.pure(by) },
            t = Clamp {
                x = Scale { k = 1.0 / dur, of = clock.time() },
                lo = expr.pure(0.0),
                hi = expr.pure(1.0),
            },
        },
        z = expr.pure(0.0),
    }
end

-- 旧 `Step::Play` 区间：局部时间 `[0, dur)`（段外由 Hold 钳到两端）。

-- 地面线（始终可见）。
scene.spawn({
    Polyline {
        points = {
            scene.vec2(-2.4, q(FLOOR_Y)),
            scene.vec2(2.4, q(FLOOR_Y)),
        },
        closed = false,
    },
    Stroke { width = 2.0, color = scene.rgba(150, 150, 175, 255) },
})

local camera = scene.spawn({
    Camera2D { center = scene.vec2(0.0, 0.7), scale = 55.0 },
})

-- 一只小球的完整弹跳：下落（挤压）→ 弹起（拉伸）→ 回落归一。
-- 时序糖 `scene.sequence`（前置块）把这 6 个段收成一张表：
-- 每段产出的仍是 `expr.bind { clock = clock.shift(起点), duration = … }`，
-- 与手写版 AST 逐字节相同（本作品 payload 即对拍基准）。
-- 同一段里的并行绑定靠显式 `at` 共享起点（`at` 不推进游标）。
local cursor = 0.0

local function hop(x, color)
    local b = scene.spawn({
        Circle { x = 0.0, y = 0.0, r = 0.28 },
        Fill { color = color },
        Transform { x = x, y = q(HOP_HI) },
        Opacity { value = 0.0 },
    })

    local down = cursor
    local up = down + DOWN
    local settle = up + UP

    scene.sequence {
        -- 下落 + 落地挤压 + 淡入（同一段 [down, down+DOWN)）。
        {
            object = b,
            target = Transform.channel.position,
            root = move2(x, q(HOP_HI), x, q(FLOOR_Y), DOWN),
            duration = DOWN,
            at = down,
        },
        {
            object = b,
            target = Transform.channel.scale,
            root = sugar.ramp(1.0, 0.7, DOWN),
            duration = DOWN,
            at = down,
        },
        {
            object = b,
            target = Opacity.channel.value,
            root = sugar.ramp(0.0, 1.0, 0.3),
            duration = DOWN,
            at = down,
        },
        -- 弹起 + 离地拉伸（同一段 [up, up+UP)）。
        {
            object = b,
            target = Transform.channel.position,
            root = move2(x, q(FLOOR_Y), x, q(HOP_HI), UP),
            duration = UP,
            at = up,
        },
        {
            object = b,
            target = Transform.channel.scale,
            root = sugar.ramp(0.7, 1.15, UP),
            duration = UP,
            at = up,
        },
        -- 回落归一（只动 Scale，Position 不再写）。
        {
            object = b,
            target = Transform.channel.scale,
            root = sugar.ramp(1.15, 1.0, SETTLE),
            duration = SETTLE,
            at = settle,
        },
    }

    cursor = settle + SETTLE
end

-- 三只球错峰弹跳：旧 `scene.wait` 只推进游标（没有写入）。
hop(-1.2, scene.rgba(255, 120, 120, 255))
cursor = cursor + 0.4
hop(0.0, scene.rgba(120, 220, 130, 255))
cursor = cursor + 0.4
hop(1.2, scene.rgba(140, 170, 255, 255))

-- 收尾推近（最后一段）。
scene.sequence({
    {
        object = camera,
        target = Camera2D.channel.scale,
        root = sugar.ramp(55.0, 74.0, 1.3),
        duration = 1.3,
    },
}, { start = cursor })
