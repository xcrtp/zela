-- expr_demo.lua — 表达式层作品（S3/S4 验收夹）
--
-- 与其它作品的区别：**脚本不手算采样**。半径与光晕透明度都是组合子表达式，
-- 由宿主单子 M 每帧注入 Time；脚本只提交结构。
--
-- 用到的表达式 API（zela.math@3）：
--   - 零元通道：expr.time()
--   - 表构造器：Add / Scale / Mul / Sin（与 scene.spawn{} 同构）
--   - 字面量：expr.pure(k)（自动提升尚未落地，见 GAPS AF-06）
--   - 绑定：expr.bind{ object, target, root, duration }

scene.spawn({
    Camera2D { center = Vec2 { x = 0.0, y = 0.0 }, scale = 150.0 },
})

-- 主体：半径 = 0.55 + 0.30 * sin(1.6 t)
local dot = scene.spawn({
    Circle { x = 0.0, y = 0.0, r = 0.55 },
    Fill { color = Color { r = 96, g = 196, b = 255, a = 255 } },
    Stroke { width = 2.5, color = Color { r = 18, g = 40, b = 70, a = 255 } },
    Name { value = "dot" },
})

local radius = Add {
    a = expr.pure(0.55),
    b = Scale {
        k = 0.30,
        of = Sin { of = Mul { a = expr.time(), b = expr.pure(1.6) } },
    },
}
expr.bind { object = "dot", target = "CircleRadius", root = radius, duration = 4.0 }

-- 光晕：透明度 = 0.30 + 0.20 * sin(0.8 t + π/2)（用 cos 相位差）
local halo = scene.spawn({
    Circle { x = 0.0, y = 0.0, r = 1.05 },
    Stroke { width = 1.5, color = Color { r = 255, g = 220, b = 120, a = 255 } },
    Opacity { value = 0.30 },
    Name { value = "halo" },
})

local fade = Add {
    a = expr.pure(0.30),
    b = Scale {
        k = 0.20,
        of = Cos { of = Mul { a = expr.time(), b = expr.pure(0.8) } },
    },
}
expr.bind { object = "halo", target = "OpacityValue", root = fade, duration = 4.0 }
