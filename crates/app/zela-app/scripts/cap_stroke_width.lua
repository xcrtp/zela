-- cap_stroke_width.lua — 第 BB 批（阶段 7）：线宽通道可动画。
--
-- 目的：给 `Stroke.width` 留作品级证据 —— 它此前只能作为构造参数写下就固定，
-- 动画层没有入口（能力面普查里"Stroke 线宽不能驱动"的那一条）。
--
-- 同时用到第 BB 批新增的算子与糖：
--   · 算子 `abs` / `pow`（表达式层，装配期展开成 core Call 节点）；
--   · 糖 `sugar.ramp` / `sugar.wave`（装配期展开，不进 payload 概念）。
--
-- 判据：payload 由 `demo/manifest.txt` 断言；像素基线待录制设备批次。

local T = 3.0

-- ① 左环：线宽线性收放（1 → 8，段内 [0, T)）。
local ring = scene.spawn({
    Circle { x = -1.6, y = 0.0, r = 1.1 },
    Stroke { width = 1.0, color = scene.rgba(240, 200, 90, 255) },
})
expr.bind {
    object = ring,
    target = Stroke.channel.width,
    root = sugar.ramp(1.0, 8.0, T),
    duration = T,
}

-- ② 右环：线宽"呼吸" —— `2 + 6·|sin(1.5 t)|^0.5`（幂次把正弦压成更宽的肩部）。
local ring2 = scene.spawn({
    Circle { x = 1.6, y = 0.0, r = 1.1 },
    Stroke { width = 2.0, color = scene.rgba(120, 200, 255, 255) },
})
expr.bind {
    object = ring2,
    target = Stroke.channel.width,
    root = Add {
        a = expr.pure(2.0),
        b = Scale {
            k = 6.0,
            of = Pow {
                a = Abs { of = sugar.wave(0.0, 1.0, 1.5) },
                b = expr.pure(0.5),
            },
        },
    },
}

-- 相机：两个环都在画面内。
scene.spawn({
    Camera2D { scale = 42.0 },
})
