-- capability_gaps.lua — B7 Step 4：零使用能力补齐（能力矩阵作品 A2）。
--
-- 覆盖 6 项此前零使用的能力：
--   ① Group            —— 纯组标签（不做父子变换累乘）
--   ② scene.despawn    —— 生命周期闭环（commit 期销毁）
--   ③ Curve.Const      —— 常量曲线（"保持不变"的显式表达）
--   ④ "Phase"     —— 相位目标（写 PhaseAnim.phase）
--   ⑤ "MorphT"    —— 形变目标（写 MorphShape.t）
--   ⑥ "CameraEye" —— 相机运镜（写 Camera3D.eye）
--
-- 其中 ①④⑤ 的脚本入口是 B7 Step 4 新增的（`zela.scene@8 → @9`：
-- `scene.group` / `scene.phase_anim` / `scene.morph`）—— 它们此前**没有入口**，
-- 不是"没人用"。
--
-- 诚实边界（像素上看不出来的三项，已在 GAPS 记账，不是偷懒）：
--   · `"Phase"` 写的 `phase` 只被**函数图像对象**消费，而脚本层还没有创建
--     函数图像对象的入口 ⇒ phase 每帧被写入，但画面不变（W1-06）。
--   · `Group` 是纯标签，不参与绘制 ⇒ 只证明"可声明、可持久化、可回放"（W1-05）。
--   · `scene.despawn` 在 **commit 期**执行 ⇒ 语义是"创建后取消"，不是
--     "动画中途移除"（脚本 API 没有带时间的销毁入口，W1-07）。

local T = 3.0 -- 唯一动画段时长（秒）

-- 旧 `Step::Play` 区间 `[0, T)`（显式时钟；段外由 Hold 钳到两端）。

-- 动画值域是 f32，而表达式层是 f64：`round_f32` 在旧曲线的同一位置落位。

-- `Phase { omega }`：`omega × t`（`t` 是时钟局部时间；与旧曲线的 f32 口径一致）。
local function phase(omega)
    return sugar.f32(Mul(sugar.f32(expr.pure(omega)), sugar.f32(clock.time())))
end

-- `Orbit2D` ⇒ `(r cos θ, r sin θ, 0)`，`θ = speed · t + initial_angle`（f32 口径）。

-- 2D 相机（morph / 圆都在世界坐标里；scale 30 ⇒ 视口约 21.3 × 16 世界单位）。
scene.spawn({
    Camera2D { scale = 30.0 },
})

-- ① 组标签：把它挂到本作品的全部可见对象上（纯标签，无变换）。
local layer = Group { id = 1 }

-- ⑤ Morph：方块（左）→ 菱形。`"MorphT"` 由相位曲线线性推进到 0.9。
local morph = scene.spawn({
    Morph {
        from = {
            scene.vec2(-7.5, -2.0),
            scene.vec2(-3.5, -2.0),
            scene.vec2(-3.5, 2.0),
            scene.vec2(-7.5, 2.0),
        },
        to = {
            scene.vec2(-5.5, -3.0),
            scene.vec2(-2.5, 0.0),
            scene.vec2(-5.5, 3.0),
            scene.vec2(-8.5, 0.0),
        },
        closed = true,
        t = 0.0,
        samples = 0,
    },
    Stroke { width = 6.0, color = scene.rgba(120, 255, 160, 255) },
    layer,
})
-- ⑤ Morph：形变进度由 `MorphShape.t` 线性推进到 0.9（显式时钟，段 [0, T)）。
expr.bind {
    object = morph,
    -- `Morph` 的存储组件是 `MorphShape`，常量挂在脚本组件 `Morph` 上。
    target = Morph.channel.t,
    root = phase(0.3),
    clock = clock.scale(1.0),
    predicate = Predicate.Of { expr = clock.range(0.0, T) },
}

-- ④ 相位目标：挂在右侧圆上；`phase` 每帧被写（无消费者 ⇒ 画面不变，见文件头）。
local phase_dot = scene.spawn({
    Circle { x = 5.5, y = 2.5, r = 1.2 },
    Fill { color = scene.rgba(255, 220, 90, 255) },
    PhaseAnim { phase = 0.0, omega = 0.0 },
    layer,
})
expr.bind {
    object = phase_dot,
    target = PhaseAnim.channel.phase,
    root = phase(1.0),
    clock = clock.scale(1.0),
    predicate = Predicate.Of { expr = clock.range(0.0, T) },
}

-- ③ 常量曲线：整段保持 0.6（对照上面两条"随时间变化"的曲线）。
local dim_dot = scene.spawn({
    Circle { x = 5.5, y = -2.5, r = 1.2 },
    Fill { color = scene.rgba(90, 190, 255, 255) },
    layer,
})
expr.bind {
    object = dim_dot,
    target = Opacity.channel.value,
    root = expr.pure(0.6),
    clock = clock.scale(1.0),
    predicate = Predicate.Of { expr = clock.range(0.0, T) },
}

-- ⑥ 相机运镜：3D 相机在 XY 平面绕原点公转。
--    `up = +Z` 不能省 —— 视线落在 XY 平面内，`up = +Y` 在 eye 转到 ±Y 轴时与视线
--    平行，look-at 退化（画面会出现 NaN / 全黑）。
local cam3 = scene.spawn({
    Camera3D {
        eye = scene.vec3(9.0, 0.0, 0.0),
        up = scene.vec3(0.0, 0.0, 1.0),
        fov_y = 0.7,
    },
})
-- ⑥ 相机运镜：`Orbit2D` 的算子化形态（f32 口径，与旧曲线逐位一致）。
expr.bind {
    object = cam3,
    target = Camera3D.channel.eye,
    root = sugar.orbit2d(9.0, 0.5, 0.0),
    clock = clock.scale(1.0),
    predicate = Predicate.Of { expr = clock.range(0.0, T) },
}

-- 立方体：Flat 着色不依赖光源 ⇒ 不需要 Light 组件。
scene.spawn({
    Cube { half_extent = 1.6, solid = true },
    Material3D {
        color = scene.rgba(235, 120, 120, 255),
        variant = ShaderVariant.Flat,
    },
    layer,
})

-- ② 生命周期：创建后立即取消（commit 期执行）。位置**故意放在视口内**：
--    若 despawn 失效，画面正中会多出一个白圆（下面的对拍就是靠这一点自证的）。
local ghost = scene.spawn({
    Circle { x = 0.0, y = -6.0, r = 1.5 },
    Fill { color = scene.rgba(255, 255, 255, 255) },
    layer,
})
scene.despawn(ghost)
