-- cap_sequence.lua — 动画层阶段 4 能力展示：时序糖 `scene.sequence`
--
-- 判据（计划 §阶段 4）：
--   1. **纯展开**：每步产出的仍是 `expr.bind { clock = clock.shift(起点), duration = … }`，
--      与手写版 AST 逐字节相同（spring.lua 用糖重写后 payload 未变 = 证据）；
--   2. 不恢复旧 `scene.animate`、不引入隐式全局游标（游标是 `scene.sequence` 的局部量，
--      接续靠返回值 / `start`）。
--
-- 本作品把三种段形态都用上：
--   · 顺序推进（缺省：起点 = 当前游标，用完后游标 += duration）；
--   · 并行（显式 `at`：与上一段共享起点，且**不**推进游标）；
--   · 纯等待（`wait`：只推进游标，不产生绑定）。
--
-- 能力使用：`Transform.scale` / `Opacity.value` 通道 + `sugar.ramp` + `scene.sequence`。

local T1 = 0.8 -- 第一段（放大 / 淡出，两球并行）
local GAP = 0.3 -- 段间空档（只有游标前进）
local T2 = 0.5 -- 第二段（回落 / 再放大）

scene.spawn({
    Camera2D { scale = 46.0 },
})

-- 左球：只做缩放（Transform.scale 标量通道）。
local left = scene.spawn({
    Circle { x = -1.4, y = 0.0, r = 0.55 },
    Fill { color = scene.rgba(255, 200, 120, 255) },
    Transform { x = -1.4, y = 0.0 },
})

-- 右球：先淡出，再放大（Opacity.value + Transform.scale）。
local right = scene.spawn({
    Circle { x = 1.4, y = 0.0, r = 0.55 },
    Fill { color = scene.rgba(140, 200, 255, 255) },
    Transform { x = 1.4, y = 0.0 },
    Opacity { value = 1.0 },
})

scene.sequence {
    -- ① 左球放大（起点 = 0，游标推进到 T1）。
    {
        object = left,
        target = Transform.channel.scale,
        root = sugar.ramp(1.0, 1.6, T1),
        duration = T1,
    },
    -- ② 右球淡出：`at = 0` ⇒ 与 ① 并行（不推进游标）。
    {
        object = right,
        target = Opacity.channel.value,
        root = sugar.ramp(1.0, 0.2, T1),
        duration = T1,
        at = 0.0,
    },
    -- ③ 空档：只推进游标（T1 → T1 + GAP）。
    { wait = GAP },
    -- ④ 左球回落（起点 = T1 + GAP）。
    {
        object = left,
        target = Transform.channel.scale,
        root = sugar.ramp(1.6, 1.0, T2),
        duration = T2,
    },
    -- ⑤ 右球再放大（紧接 ④）。
    {
        object = right,
        target = Transform.channel.scale,
        root = sugar.ramp(1.0, 1.4, T2),
        duration = T2,
    },
}
