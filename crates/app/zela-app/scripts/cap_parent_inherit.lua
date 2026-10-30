-- cap_parent_inherit.lua — 第 BC 批：Parent 多维度连续继承（变换三维 + 样式三维）。
--
-- 目的：给 `Parent` 的六个继承权重通道留作品级证据 —— 它们此前只能作为
-- 构造参数写下就固定，动画层没有入口。
--
-- 覆盖（每个通道都用 `<组件>.channel.<字段>` 常量选中）：
--   变换   —— position / rotation / scale（父链变换按维度混合）
--   样式   —— opacity / fill / stroke（父链不透明度与颜色）
--
-- 判据：payload 由 `demo/manifest.txt` 断言；像素基线待录制设备批次。

local T = 2.0

local WINDOW = Predicate.Of {
    expr = And {
        a = Ge { a = clock.time(), b = expr.pure(0.0) },
        b = Lt { a = clock.time(), b = expr.pure(T) },
    },
}

local function bind(object, target, root)
    expr.bind {
        object = object,
        target = target,
        root = root,
        clock = clock.scale(1.0),
        predicate = WINDOW,
    }
end

-- 相机：父子两级都在画面内。
scene.spawn({
    Camera2D { scale = 52.0 },
})

-- 父：位于 (1.6, 0)，红色填充 + 金黄描边 + 半透明。
local root = scene.spawn({
    Circle { x = 1.6, y = 0.0, r = 0.9 },
    Fill { color = scene.rgba(230, 70, 70, 255) },
    Stroke { width = 3.0, color = scene.rgba(255, 210, 90, 255) },
    Opacity { value = 0.5 },
})

-- 子 A：不设 Fill / Opacity，靠继承拿到父的颜色与不透明度（三个样式权重 0 → 1）。
local child_a = scene.spawn({
    Circle { x = 0.0, y = 0.0, r = 0.55 },
    Stroke { width = 2.0, color = scene.rgba(90, 160, 255, 255) },
    Parent { parent = root },
})
bind(child_a, Parent.channel.opacity, sugar.ramp(0.0, 1.0, T))
bind(child_a, Parent.channel.fill, sugar.ramp(0.0, 1.0, T))
bind(child_a, Parent.channel.stroke, sugar.ramp(0.0, 1.0, T))

-- 子 B：变换三维权重 1 → 0（位置 / 旋转 / 缩放逐维松开父链）。
local child_b = scene.spawn({
    Circle { x = -1.6, y = 0.0, r = 0.4 },
    Fill { color = scene.rgba(90, 210, 160, 255) },
    Parent { parent = root },
})
bind(child_b, Parent.channel.position, sugar.ramp(1.0, 0.0, T))
bind(child_b, Parent.channel.rotation, sugar.ramp(1.0, 0.5, T))
bind(child_b, Parent.channel.scale, sugar.ramp(1.0, 0.5, T))
