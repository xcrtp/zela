-- cap_codegen.lua — 阶段 2 能力展示：着色 codegen（填充色由生成的 WGSL 在 GPU 上算）
--
-- 判据：左右两个圆**几何相同**，只有颜色来源不同 ——
--   左：普通 `Fill`（颜色写在记录里）；
--   右：`expr.shader`，根表达式 `vec3(0.20, 0.65, 0.95)` 由
--       `ze_comb::emit_wgsl` 生成 `fn eval(...) -> vec3<f32>`，光栅前由 compute pass
--       写进记录的填充槽（像素里没有 CPU 算出的颜色）。
-- 装饰性：不改几何 / 状态，也不进结构指纹；路由到 wgpu 时决策理由会写"已忽略：着色 codegen"。
-- 静态（无动画）⇒ 只渲 1 帧。

-- 2D 相机：世界 ±5.3 × ±4 落在 640×480 里（scale = 60）。
scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 0.0 }, scale = 60.0 },
})

-- 对照组：普通填充（记录自带颜色）。
scene.spawn({
    Circle { x = -2.6, y = 0.0, r = 1.6 },
    Fill { color = Color { r = 40, g = 120, b = 220, a = 255 } },
})

-- 实验组：同一几何，颜色由 GPU 求值 —— `clock.time()` 是**外部注入**（uniform，
-- 布局由生成器的参数表推导，不是按通道名特判）。
local shaded = scene.spawn({
    Circle { x = 2.6, y = 0.0, r = 1.6 },
    Fill { color = Color { r = 10, g = 20, b = 30, a = 255 } },
})
expr.shader {
    object = shaded,
    root = Vec3Of {
        x = expr.pure(0.20),
        y = Cos { of = clock.time() },
        z = expr.pure(0.95),
    },
}
