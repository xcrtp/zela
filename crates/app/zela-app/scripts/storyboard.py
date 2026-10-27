# storyboard.py — 现有分镜（zela-app/src/storyboard.rs）的 **Python 完整脚本化版本**。
#
# 与 storyboard.lua 逐项对应（覆盖全部 6 个镜头 + T2 箭头 + T3 标注文字）：
#   1. 常驻 Axes2D（容器；几何由 AxesRefreshSystem 每帧生成）
#   2. 2D 相机对象
#   3. 曲线书写 0–2s
#   4. wait 0.5s
#   5. 镜头推进 2.5–3.5s（scale + center 并行）
#   6. 公式书写 3.5–4.5s（逐轮廓依次；顺序由内容服务决定）
#
# 数值与 storyboard.rs 逐项对应：
#   CAMERA_START = center (0, 0), scale 40
#   CAMERA_PEAK  = (π/2, 1), scale 110
#   曲线采样     = [-π, π] 上 128 段（129 个点）
#   公式         = "y = sin x"，字号 0.8，锚点 (1, 2)
#   标注文字     = "math animation"，字号 0.55，Transform (-1.5, 3, 0)
#   箭头         = (0.2, 2.1) → (π/2 − 0.15, 1.15)，头长 / 头宽 0.25
#
# 注：`from` 是 Python 关键字，Arrow / Curve.Ramp 的该字段用**位置参数**传入。
#
# capability 是**普通 Python 模块**：可以 `import zela.scene` / `from zela import ...`，
# 运行前由宿主把 `zela` 包树安装到 `sys.modules`。

import math

import zela
import zela.content as content
import zela.math as expr
import zela.scene as scene
from zela import (
    Arrow,
    Axes2D,
    Camera2D,
    Color,
    Fill,
    Formula,
    LineStyle,
    PathShape,
    Reveal,
    RevealPath,
    Stroke,
    Text,
    Transform,
    Vec2,
)

N = 128
FORMULA_WRITE_SECONDS = 1.0


def color(r, g, b, a):
    return Color(r=r, g=g, b=b, a=a)


def ramp(from_, to, dur):
    """`Ramp` 的算子化形态：`mix(from, to, clamp(t / dur, 0, 1))`（显式时钟）。"""
    return expr.Mix(
        a=expr.pure(from_),
        b=expr.pure(to),
        t=expr.Clamp(
            x=expr.Scale(k=1.0 / dur, of=expr.clock.time()),
            lo=expr.pure(0.0),
            hi=expr.pure(1.0),
        ),
    )


def ramp2(ax, ay, bx, by, dur):
    return expr.Mix(
        a=expr.Vec2Of(x=expr.pure(ax), y=expr.pure(ay)),
        b=expr.Vec2Of(x=expr.pure(bx), y=expr.pure(by)),
        t=expr.Clamp(
            x=expr.Scale(k=1.0 / dur, of=expr.clock.time()),
            lo=expr.pure(0.0),
            hi=expr.pure(1.0),
        ),
    )


def in_segment(dur):
    """旧 `Step::Play` 区间：局部时间 `[0, dur)`（段外由 Hold 钳到两端）。"""
    return expr.Predicate.Of(
        expr=expr.And(
            a=expr.Ge(a=expr.clock.time(), b=expr.pure(0.0)),
            b=expr.Lt(a=expr.clock.time(), b=expr.pure(dur)),
        )
    )


# ── 分镜 1：常驻网格与坐标轴（Axes2D::unbounded() 的默认样式）──────────
scene.spawn(
    [
        Axes2D(
            min_px=10.0,
            grid=LineStyle(width=1.0, color=color(50, 50, 50, 255)),
            axis=LineStyle(width=2.0, color=color(255, 255, 255, 255)),
        )
    ]
)

# ── 分镜 2：2D 相机 ────────────────────────────────────────────────────
camera = scene.spawn([Camera2D(center=Vec2(x=0.0, y=0.0), scale=40.0)])

# ── 分镜 3：曲线 y = sin x 的书写动画 ─────────────────────────────────
points = []
for i in range(N + 1):
    x = -math.pi + i * (2 * math.pi / N)
    points.append(Vec2(x=x, y=math.sin(x)))

curve = scene.spawn(
    [
        Reveal(points=points, closed=False),
        Stroke(width=2.0, color=color(0, 200, 200, 255)),
    ]
)

# ── 分镜 6：公式书写（每个描边子轮廓一个对象；顺序 = 内容服务的阅读顺序）──
formula = content.formula(Formula(source="y = sin x", font_size=0.8, x=1.0, y=2.0))
formula_handles = []
for i in range(len(formula)):
    formula_handles.append(scene.spawn([RevealPath(path=formula[i])]))

# ── T3：标注文字（静态，经 Transform 摆放）────────────────────────────
text = content.text(Text(source="math animation", font_size=0.55, x=0.0, y=0.0))
for i in range(len(text)):
    scene.spawn(
        [
            PathShape(path=text[i]),
            Transform(x=-1.5, y=3.0, z=0.0),
        ]
    )

# ── T2：箭头（实心，指向曲线峰值）────────────────────────────────────
scene.spawn(
    [
        Arrow(
            Vec2(x=0.2, y=2.1),
            Vec2(x=math.pi / 2 - 0.15, y=1.0 + 0.15),
            0.25,
            0.25,
        ),
        Fill(color=color(255, 220, 120, 255)),
    ]
)

# ── 时间编排（显式时钟；旧 scene.animate / parallel / wait 的段序）──────
CAMERA_SCALE = expr.Channel(
    namespace="zela.scene", name="Framing2D.scale", ty=expr.ChannelType.Number
)
CAMERA_CENTER = expr.Channel(
    namespace="zela.scene", name="Framing2D.center", ty=expr.ChannelType.Vec2
)

# 0–2s：曲线书写（phase(0.5) × 2s = 1）
expr.bind(
    expr.Bind(
        object=curve,
        target=Reveal.channel.end,
        root=expr.Scale(k=0.5, of=expr.clock.time()),
        clock=expr.clock.scale(1.0),
        predicate=in_segment(2.0),
    )
)
# 2–2.5s：停（没有绑定生效 ⇒ 上一步的值保持）
# 2.5–3.5s：镜头推进（scale 与 center 同区间 —— 旧 parallel 的两条 action）
expr.bind(
    expr.Bind(
        object=camera,
        target=CAMERA_SCALE,
        root=ramp(40.0, 110.0, 1.0),
        clock=expr.clock.shift(2.5),
        predicate=in_segment(1.0),
    )
)
expr.bind(
    expr.Bind(
        object=camera,
        target=CAMERA_CENTER,
        root=ramp2(0.0, 0.0, math.pi / 2, 1.0, 1.0),
        clock=expr.clock.shift(2.5),
        predicate=in_segment(1.0),
    )
)
# 3.5–4.5s：公式逐轮廓依次书写（1s 平摊；轮廓数可能为 0）
if len(formula_handles) > 0:
    each = FORMULA_WRITE_SECONDS / len(formula_handles)
    for index, handle in enumerate(formula_handles):
        start = 3.5 + index * each
        expr.bind(
            expr.Bind(
                object=handle,
                target=Reveal.channel.end,
                root=ramp(0.0, 1.0, each),
                clock=expr.clock.shift(start),
                predicate=in_segment(each),
            )
        )
