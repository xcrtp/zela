-- cap_path_follow.lua — 动画层阶段 2 能力展示：路径跟随（PathFollow + Reveal.point_at / tangent_at）
--
-- 判据（计划 §阶段 2）：
--   1. 抽取公共实现后 `Reveal.current_pos` 逐位不变（由 `ze-notion` 的
--      `point_at_walks_the_normalized_arc_length` 与 36 项像素基线守护）；
--   2. `t` 从 0 走到 1，物体位置与**手写逐帧 Transform** 逐位相同
--      （`follow_matches_handwritten_transforms_bitwise`）；
--   3. `orient = false` 只写位置，旋转一位都不动（`orient_false_leaves_rotation_alone`）。
--
-- 本作品给出的是**可看见的那一面**：
--   · 黄球：`PathFollow { orient = true }` —— 位置与朝向都由 `PathFollow.t` 驱动；
--   · 绿球：`PathFollow { orient = false }` —— 只写位置（圆看不出朝向，判据在单测里）；
--   · 红球 + 白针：不挂 `PathFollow`，直接用**方法** `Reveal.point_at` / `Reveal.tangent_at`
--     取同一条路径上的位置与切线（与 PathFollow 共用 `Path2D::point_at` / `tangent_at`）。
--
-- 能力使用：`PathFollow`（组件）+ `PathFollow.t`（通道）+ `Polyline` / `Reveal` +
-- `Reveal.point_at` / `Reveal.tangent_at`（方法）+ `Circle.center` / `Transform.*` 通道。

local T = 6.0        -- 走完整条路径的时长（秒）
local SAMPLES = 64   -- 轨道折线点数（唯一的循环 = 几何采样）
local X0, X1 = -4.6, 4.6
local AMP = 1.6      -- 正弦轨道振幅（世界单位）

scene.spawn({
    Camera2D { scale = 60.0 },
})

-- ── 轨道：正弦折线 ──────────────────────────────────────────────────
local track = {}
for i = 0, SAMPLES do
    local u = i / SAMPLES
    track[#track + 1] = scene.vec2(
        X0 + (X1 - X0) * u,
        AMP * math.sin(2.0 * math.pi * u)
    )
end

-- 静态可见的轨道（对照：跟随者应当正好走在这条线上）
scene.spawn({
    Polyline { points = track, closed = false },
    Stroke { width = 2.0, color = scene.rgba(96, 110, 130, 255) },
})

-- 被跟随的 `Reveal` 对象：既是 `point_at` / `tangent_at` 的路径来源，
-- 也用它自己的 `Reveal.end` 画一遍（进度与跟随者同一个 t）。
local guide = scene.spawn({
    Reveal { points = track, closed = false },
    Stroke { width = 1.0, color = scene.rgba(90, 160, 220, 255) },
})

-- 归一化进度：`t = clock / T`（显式时钟；越过 1 后由弧长实现钳位到终点）。
local t_norm = Scale { k = 1.0 / T, of = clock.time() }

expr.bind {
    object = guide,
    target = Reveal.channel["end"],
    root = t_norm,
}

-- ── (1) orient = true：位置 + 朝向 ──────────────────────────────────
-- 跟随者**必须**带 `Transform`：`PathFollow` 只提供 (path, t)，系统写的是同实体的摆放。
-- 圆的几何中心留在原点（`x = y = 0`），位置全由 `Transform` 承担 —— 否则会叠加两次。
local rover = scene.spawn({
    Circle { x = 0.0, y = 0.0, r = 0.18 },
    Fill { color = scene.rgba(255, 210, 120, 255) },
    Transform { x = X0, y = 0.0 },
    PathFollow { points = track, closed = false, t = 0.0, orient = true },
})
expr.bind {
    object = rover,
    target = PathFollow.channel.t,
    root = t_norm,
}

-- ── (2) orient = false：只写位置 ────────────────────────────────────
local follower = scene.spawn({
    Circle { x = 0.0, y = 0.0, r = 0.11 },
    Fill { color = scene.rgba(120, 255, 160, 255) },
    Transform { x = X0, y = 0.0 },
    PathFollow { points = track, closed = false, t = 0.0, orient = false },
})
expr.bind {
    object = follower,
    target = PathFollow.channel.t,
    root = t_norm,
}

-- ── (3) 原语：`Reveal.point_at` / `Reveal.tangent_at`（不挂 PathFollow）──
local probe = scene.spawn({
    Circle { x = X0, y = 0.0, r = 0.10 },
    Fill { color = scene.rgba(255, 120, 120, 255) },
})
expr.bind {
    object = probe,
    target = Circle.channel.center,
    root = Reveal.point_at(guide, t_norm),
}

-- 白针：位置同样由 `point_at` 给，朝向由 `tangent_at` 经 atan2 折算成弧度。
local needle = scene.spawn({
    Polyline {
        points = { scene.vec2(-0.35, 0.0), scene.vec2(0.35, 0.0) },
        closed = false,
    },
    Stroke { width = 2.0, color = scene.rgba(255, 255, 255, 200) },
    Transform { x = X0, y = 0.0 },
})
local tangent = Reveal.tangent_at(guide, t_norm)
expr.bind {
    object = needle,
    target = Transform.channel.position,
    -- `Transform.position` 是 Vec3 通道 ⇒ 显式补 z（不靠隐式提升）。
    root = expr.vec3_from_vec2 { v = Reveal.point_at(guide, t_norm), z = expr.pure(0.0) },
}
expr.bind {
    object = needle,
    target = Transform.channel.rotation,
    root = expr.atan2 { y = expr.y { v = tangent }, x = expr.x { v = tangent } },
}
