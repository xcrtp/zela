-- cap_sdf.lua — BT-5 能力展示：2D 解析 SDF（隐式形状 + 覆盖抗锯齿）
--
-- 判据：形状不是几何路径，而是**逐像素求值**的距离场 ——
--   `d = length(p - 圆心) - 半径`（像素空间，`|∇d| = 1`），
--   `a = 1 - smoothstep(-0.5, 0.5, d)`：内部 1、外部 0、边缘 1 像素过渡（真抗锯齿）。
-- 三个圆用同一条几何（各自的包围盒），只有距离场的**布尔组合**不同：
--   左：`min(d1, d2)` 并；中：`max(d1, d2)` 交；右：`max(d_outer, -d_inner)` 差（圆环）。
-- 像素空间求值 ⇒ **放大时边缘宽度恒定**（不经过 `fwidth`，见 docs/research/BT_前置核实.md）。
-- 装饰性：不改几何 / 状态，也不进结构指纹；路由到 wgpu 时决策理由会写"已忽略：着色 codegen"。
-- 静态（无动画）⇒ 只渲 1 帧。

-- 2D 相机：世界 ±5.3 × ±4 落在 640×480 里（scale = 60）。
scene.spawn({
    Camera2D { scale = 60.0 },
})

-- 包围盒（像素）与盒内中心坐标；`extent` = min(宽, 高)，所有距离都按它归一。
-- `p = pos2 - center`：相对包围盒中心的像素偏移（`local` 是 Lua 保留字，这里用等价写法）。
local size = expr.size()
local extent = Min { a = expr.x { v = size }, b = expr.y { v = size } }
local p = Add { a = expr.pos2(), b = Scale { k = -1.0, of = expr.center() } }

-- 圆心在盒内偏移 (ox, oy)，半径 r —— 都以 `extent` 为单位。
local function circle(ox, oy, r)
    local dx = Add { a = expr.x { v = p }, b = Scale { k = -ox, of = extent } }
    local dy = Add { a = expr.y { v = p }, b = Scale { k = -oy, of = extent } }
    local len = Length { v = Vec2Of { x = dx, y = dy } }
    return Add { a = len, b = Scale { k = -r, of = extent } }
end

-- 覆盖率：内部（d < -0.5）1，外部（d > 0.5）0，边缘一像素过渡。
local function coverage(d)
    return Add {
        a = expr.pure(1.0),
        b = Scale {
            k = -1.0,
            of = Smoothstep {
                e0 = expr.pure(-0.5),
                e1 = expr.pure(0.5),
                x = d,
            },
        },
    }
end

local function sdf_object(x, d, color)
    local object = scene.spawn({
        Circle { x = x, y = 0.0, r = 1.4 },
        Fill { color = Color { r = 10, g = 20, b = 30, a = 255 } },
    })
    expr.shader {
        object = object,
        root = Vec4Of {
            x = expr.pure(color[1]),
            y = expr.pure(color[2]),
            z = expr.pure(color[3]),
            w = coverage(d),
        },
    }
end

-- 并：两个圆的 min。
sdf_object(-3.2, Min {
    a = circle(-0.13, 0.0, 0.26),
    b = circle(0.13, 0.0, 0.26),
}, { 0.25, 0.60, 0.95 })

-- 交：两个圆的 max（透镜）。
sdf_object(0.0, Max {
    a = circle(-0.11, 0.0, 0.30),
    b = circle(0.11, 0.0, 0.30),
}, { 0.95, 0.80, 0.20 })

-- 差：外圆减内圆（圆环）—— `max(outer, -inner)`。
sdf_object(3.2, Max {
    a = circle(0.0, 0.0, 0.40),
    b = Scale { k = -1.0, of = circle(0.0, 0.0, 0.26) },
}, { 0.95, 0.35, 0.55 })
