-- cap_text_group.lua — 动画层阶段 3 能力展示：文字排版糖 `scene.text` + 组级控制
--
-- 判据（计划 §阶段 3）：
--   1. 用糖重写的 fourier / storyboard / surface_matrix / vector_paths **payload 逐位不变**
--      （`--step demo` 断言：四件作品的 manifest 哈希未改）；
--   2. **组级淡出**：只改 `glyph_root` 的 `Opacity.value` ⇒ 全部字形跟随；
--   3. 默认排版参数与手写结果一致（糖不引入新的默认偏移）。
--
-- 本作品给出的是"整组只动一个容器"的可见证据：
--   · 组 A：`Opacity.value` 从 1 → 0（整组淡出）；
--   · 组 B：`Transform.scale` 从 1 → 1.6（整组放大）。
-- 两个组都只有**一个**对象被驱动，字形全部靠 `Parent` 跟随。
--
-- 能力使用：`scene.text`（前置块糖）+ `Opacity.value` + `Transform.scale` +
-- `Parent`（缺省全量继承变换）+ `sugar.ramp`。

local T = 3.0

scene.spawn({
    Camera2D { scale = 44.0 },
})

-- ── 组 A：整组淡出 ────────────────────────────────────────────────────
-- `opacity = 1.0` 给容器一个 `Opacity` 组件，`inherit_opacity = true` 让字形
-- 继承它（`Parent { opacity = 1.0 }`）；之后只写容器这一个通道即可。
local fade_group = scene.text("fade", {
    size = 1.5,
    x = -3.6,
    y = 1.4,
    name = "fade-group",
    opacity = 1.0,
    inherit_opacity = true,
    style = { Fill { color = scene.rgba(255, 220, 130, 255) } },
})
expr.bind {
    object = fade_group,
    target = Opacity.channel.value,
    root = sugar.ramp(1.0, 0.0, T),
}

-- ── 组 B：整组放大 ────────────────────────────────────────────────────
-- 只写容器的 `Transform.scale`（标量通道）；字形没有自己的 Transform，
-- 缩放完全来自父链（`Parent` 的 scale 权重默认 1.0）。
local scale_group = scene.text("scale", {
    size = 1.5,
    x = -3.6,
    y = -1.4,
    name = "scale-group",
    opacity = 1.0,
    style = { Fill { color = scene.rgba(140, 220, 255, 255) } },
})
expr.bind {
    object = scale_group,
    target = Transform.channel.scale,
    root = sugar.ramp(1.0, 1.6, T),
}
