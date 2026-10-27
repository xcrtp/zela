-- expr_read.lua — G 批通道读端 + H 批值分支（脚本级验收夹）。
--
-- 目的：证明**一个量可以跟随另一个量**（而不只是"是时间的函数"）。
-- 读取写法（第 2 批）：通道常量**可调用** —— `<组件>.channel.<字段>(<对象>)`，
-- 类型由常量自带（不再手写 `ty = "Vec2"`）：
--   - `object` **必须显式传入**（没有"默认读绑定自己的对象"这种隐式）；
--   - 等价于 `expr.read { channel = <常量>, object = <句柄> }`。
--
-- 覆盖：
--   读 Vec2  —— 箭头终点跟随圆点中心（跨对象）；
--   读 Number + `Select` —— 圆点半径 > 0.6 时内圈取 0.22，否则 0.34（真条件，不是时间分支）；
--   读 Vec3 —— 渐变两个端点跟随两个方块的 `Transform.position`。
--
-- 帧内定序：所有**写**通道都在第 0 段（曲线），读绑定用全局时钟（start = 0）
-- ⇒ 写者不晚于读者，装配期定序检查通过（倒置会在装配期报 `ReadOrderConflict`）。

scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 1.2 }, scale = 80.0 },
})

local function rgba(r, g, b, a)
    return Color { r = r, g = g, b = b, a = a }
end

-- ── 写者 A：圆点（半径 + 中心都由曲线驱动 ⇒ 读端有非平凡值）────────────
local dot = scene.spawn({
    Circle { x = -3.6, y = 2.0, r = 0.55 },
    Fill { color = rgba(96, 196, 255, 255) },
})
scene.animate(dot, scene.clip("Circle.radius", Curve.Sin {
    base = 0.55,
    amp = 0.25,
    omega = 2.4,
}), 4.0)
scene.parallel(dot, scene.clip("Circle.center", Curve.Ramp2 {
    from = Vec2 { x = -3.6, y = 2.0 },
    to = Vec2 { x = -1.2, y = 1.2 },
    dur = 4.0,
}))

-- ── 写者 B / C：两个方块的 `Transform.position`（Vec3 源）──────────────
local a = scene.spawn({ Transform { x = -2.4, y = -0.8, z = 0.0 } })
local b = scene.spawn({ Transform { x = 2.4, y = -0.8, z = 0.0 } })
scene.parallel(a, scene.clip_transform(TransformChannel.Position, Curve.Ramp2 {
    from = Vec2 { x = -2.4, y = -0.8 },
    to = Vec2 { x = -3.0, y = 0.2 },
    dur = 4.0,
}))
scene.parallel(b, scene.clip_transform(TransformChannel.Position, Curve.Ramp2 {
    from = Vec2 { x = 2.4, y = -0.8 },
    to = Vec2 { x = 3.0, y = 0.2 },
    dur = 4.0,
}))

-- ── 读者 1（Vec2）：箭头终点 = 圆点中心 ───────────────────────────────
local arrow = scene.spawn({
    Arrow {
        from = Vec2 { x = -3.6, y = -2.4 },
        to = Vec2 { x = -3.0, y = -2.4 },
        head_length = 0.22,
        head_width = 0.18,
    },
    Fill { color = rgba(255, 180, 80, 255) },
})
expr.bind {
    object = arrow,
    target = Arrow.channel.to,
    -- 通道常量可调用（第 2 批语法糖）≡ `expr.read { channel = …, object = dot }`。
    root = Circle.channel.center(dot),
}

-- ── 读者 2（Number + Select）：半径 > 0.6 ⇒ 0.22，否则 0.34 ────────────
local halo = scene.spawn({
    Circle { x = 3.0, y = 2.0, r = 0.34 },
    Stroke { width = 2.0, color = rgba(255, 220, 120, 255) },
})
expr.bind {
    object = halo,
    target = Circle.channel.radius,
    root = Select {
        cond = Gt {
            a = Circle.channel.radius(dot),
            b = expr.pure(0.6),
        },
        -- `then` 是 Lua 关键字 ⇒ 必须用 ["then"] 的表键写法。
        ["then"] = expr.pure(0.22),
        otherwise = expr.pure(0.34),
    },
}

-- ── 读者 3 / 4（Vec3）：渐变端点 = 两个方块的位置 ────────────────────
local grad = scene.spawn({
    Circle { x = 0.0, y = -1.6, r = 1.1 },
    Fill { color = rgba(255, 255, 255, 255) },
    Gradient {
        stops = { rgba(230, 80, 90, 255), rgba(80, 140, 255, 255) },
        from = Vec3 { x = -2.4, y = -0.8, z = 0.0 },
        to = Vec3 { x = 2.4, y = -0.8, z = 0.0 },
    },
})
expr.bind {
    object = grad,
    target = Gradient.channel.from,
    root = Transform.channel.position(a),
}
expr.bind {
    object = grad,
    target = Gradient.channel.to,
    root = Transform.channel.position(b),
}
