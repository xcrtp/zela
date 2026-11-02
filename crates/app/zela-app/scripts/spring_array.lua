-- spring_array.lua — 弹簧阵列（BZ-d：多实例复用同一组 (A, b)，各自不同初值）
--
-- 能力使用：
--   1. 8 个实例共用同一组阻尼弹簧系数（0.97 / 0.02），只有 init 不同：
--        x' = x + v
--        v' = 0.97·v − 0.02·x
--      装配期每个 State 都识别出同一份 (A, b)；多实例共享一次矩阵幂的
--      批量入口是 `AffineMap::apply_many`（见 affine.rs 的 BZ-d 用例）。
--   2. 每个实例仍是一条独立绑定（同一帧内按声明顺序应用），阵列的相位差
--      完全来自初值 —— 这就是"共享系数、只差初值"的多实例形态。
--   3. 谱半径 sqrt(0.99) < 1（BZ-a 门禁）⇒ 阵列在 4 秒内收敛，不发散。
--
-- 注：参数写成**字面量表**（不在脚本里做浮点运算）。装配期会把数值写进
-- payload；运行期算出来的 dtoa 结果与本文件里的字面量不是同一条舍入路径，
-- 会破坏 ZSB 的"反序列化再序列化逐字节相同"门禁。

local DURATION = 4.0
local COUNT = 8

scene.spawn({
    Camera2D { center = scene.vec2(0.0, 0.0), scale = 78.0 },
})

-- 每行的 y / 初位移 / 初速度 / 颜色。
local YS = { 1.10, 0.78, 0.47, 0.16, -0.16, -0.47, -0.78, -1.10 }
local XS = { 1.90, 1.80, 1.70, 1.60, 1.50, 1.40, 1.30, 1.20 }
local VS = { -0.10, -0.08, -0.06, -0.04, -0.02, 0.00, 0.02, 0.04 }
local COLORS = {
    { 140, 180, 230 },
    { 154, 174, 220 },
    { 168, 168, 210 },
    { 182, 162, 200 },
    { 196, 156, 190 },
    { 210, 150, 180 },
    { 224, 144, 170 },
    { 238, 138, 160 },
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
                a = Scale { k = 0.97, of = v },
                b = Scale { k = -0.02, of = x },
            },
        },
    }
end

for i = 1, COUNT do
    local y = YS[i]
    local color = COLORS[i]

    scene.spawn({
        Circle { x = 0.0, y = y, r = 0.035 },
        Fill { color = scene.rgba(120, 130, 160, 255) },
    })
    local ball = scene.spawn({
        Circle { x = XS[i], y = y, r = 0.085 },
        Fill { color = scene.rgba(color[1], color[2], color[3], 255) },
        Stroke { width = 1.2, color = scene.rgba(28, 34, 48, 255) },
    })
    local state = spring(XS[i], VS[i])
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
