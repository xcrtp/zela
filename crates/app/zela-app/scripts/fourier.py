# fourier.py — 傅里叶旋转向量（epicycles）绘制方波（S7 重做版，与 fourier.lua 逐项对应）。
#
# 画面：
#   左：五个旋转向量（1/3/5/7/9 次谐波）首尾相接，每个向量带一个半径圆；
#   右：这些向量的 y 之和 = 方波的部分和曲线，脉冲随主段逐段写出；
#   根节点上有一个黄色相位点，用 `Curve.Orbit` + `Position` 通道绕根公转；
#   主段结束后相机**推近 → 停留 → 拉回**（S1 的段前不写语义）。
#
# 能力使用：
#   S5 Parent 层级：向量/圆/末端点/标题字形整体跟随父变换；
#   S3 Rotation 通道：每个向量只写自己的 rotation，互不覆盖；
#   S2 静态折线 + Reveal：曲线是 Polyline，脉冲是可见初态外的 Reveal；
#   S4 Orbit：黄色相位点用 2D 公转；
#   S6 Opacity：曲线淡入。
#
# 数值量化到 1e-4，保证 Lua / Python 两侧曲线关键帧逐位相同。
#
# 注：`from` 是 Python 关键字，Arrow / Curve.Ramp / Curve.Ramp2 的该字段用位置参数。

import math

import zela
import zela.content as content
import zela.math as expr
import zela.scene as scene
from zela import (
    Arrow,
    Axes2D,
    Camera2D,
    Circle,
    Color,
    Fill,
    LineStyle,
    Name,
    Opacity,
    Parent,
    PathShape,
    Polyline,
    Reveal,
    Stroke,
    Text,
    Transform,
    Vec2,
)

PI = math.pi
TAU = 2.0 * PI
T = 4.0
FADE = 0.8
ZOOM = 0.8
HOLD = 0.8
BASE_X = -3.5
CURVE_X0 = 1.0
CURVE_X1 = 5.0
CURVE_SAMPLES = 240
HARMONICS = [1, 3, 5, 7, 9]


def q(v):
    return math.floor(v * 10000.0 + 0.5) / 10000.0


def rgba(r, g, b, a):
    return Color(r=r, g=g, b=b, a=a)


# 动画值域是 f32，而表达式层是 f64：`RoundF32` 在旧曲线的同一位置落位
# （第 5a 批：迁移后的每一帧必须与旧 `Curve` 逐位相同）。
def f32(v):
    return expr.RoundF32(of=v)


def ramp(from_, to, dur):
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


def orbit2d(radius, speed, initial_angle):
    """`Orbit2D` ⇒ `(r cos θ, r sin θ, 0)`，`θ = speed · t + initial_angle`（f32 口径）。"""
    angle = expr.Add(
        a=expr.Mul(f32(expr.pure(speed)), expr.clock.time()),
        b=f32(expr.pure(initial_angle)),
    )
    return expr.Vec3FromVec2(
        v=expr.Vec2Of(
            x=f32(expr.Mul(f32(expr.pure(radius)), f32(expr.Cos(of=angle)))),
            y=f32(expr.Mul(f32(expr.pure(radius)), f32(expr.Sin(of=angle)))),
        ),
        z=expr.pure(0.0),
    )


def in_segment(dur):
    """旧 `Step::Play` 区间：局部时间 `[0, dur)`（段外由 Hold 钳到两端）。"""
    return expr.Predicate.Of(
        expr=expr.And(
            a=expr.Ge(a=expr.clock.time(), b=expr.pure(0.0)),
            b=expr.Lt(a=expr.clock.time(), b=expr.pure(dur)),
        )
    )


MAIN_START = FADE
ZOOM_IN_START = FADE + T
ZOOM_OUT_START = ZOOM_IN_START + ZOOM + HOLD


def amplitude(k):
    return 4.0 / (PI * k)


def fourier(t):
    total = 0.0
    for k in HARMONICS:
        total = total + math.sin(k * t) / k
    return (4.0 / PI) * total


def curve_points():
    points = []
    for i in range(CURVE_SAMPLES + 1):
        t = TAU * i / CURVE_SAMPLES
        x = CURVE_X0 + (CURVE_X1 - CURVE_X0) * i / CURVE_SAMPLES
        points.append(Vec2(x=q(x), y=q(fourier(t))))
    return points


def place_text_group(source, size, x, y):
    group = scene.spawn(
        [Name(value="text-group"), Transform(x=x, y=y, z=0.0)]
    )
    glyphs = content.text(Text(source=source, font_size=size, x=0.0, y=0.0))
    for i in range(len(glyphs)):
        scene.spawn([PathShape(path=glyphs[i]), Parent(parent=group)])
    return group


# ── 0. 网格 + 相机 ────────────────────────────────────────────────────
scene.spawn(
    [
        Axes2D(
            min_px=12.0,
            grid=LineStyle(width=1.0, color=rgba(48, 48, 62, 255)),
            axis=LineStyle(width=2.0, color=rgba(145, 145, 165, 255)),
        )
    ]
)

camera = scene.spawn(
    [Camera2D(center=Vec2(x=0.5, y=0.0), scale=55.0)]
)

place_text_group("Fourier epicycles: square wave", 0.40, -3.3, 2.75)

# ── 1. 右侧曲线：静态 Polyline + 脉冲 Reveal ─────────────────────────
points = curve_points()
curve = scene.spawn(
    [
        Polyline(points=points, closed=False),
        Stroke(width=2.0, color=rgba(120, 220, 120, 255)),
        Opacity(value=0.0),
    ]
)
pulse = scene.spawn(
    [
        Reveal(points=points, closed=False),
        Stroke(width=4.0, color=rgba(255, 255, 235, 255)),
        Opacity(value=0.0),
    ]
)

# ── 2. 旋转向量链（Parent 累乘）──────────────────────────────────────
root = scene.spawn(
    [Name(value="fourier-root"), Transform(x=BASE_X, y=0.0, z=0.0)]
)

# 黄色相位点：绕根公转（S4 Orbit + S3 Position）。
phase_dot = scene.spawn(
    [
        Circle(x=0.0, y=0.0, r=0.06),
        Fill(color=rgba(255, 230, 120, 255)),
        Transform(x=0.0, y=0.0, z=0.0),
        Parent(parent=root),
    ]
)

arrow_colors = [
    rgba(0, 210, 210, 255),
    rgba(255, 190, 70, 255),
    rgba(230, 100, 200, 255),
    rgba(130, 230, 130, 255),
    rgba(255, 255, 255, 255),
]

guide = rgba(85, 85, 110, 255)
joints = []
arrows = []
radii = []

for i in range(len(HARMONICS)):
    k = HARMONICS[i]
    r = amplitude(k)
    radii.append(r)

    parent = root
    local_x = 0.0
    if i > 0:
        parent = arrows[i - 1]
        local_x = radii[i - 1]

    joints.append(
        scene.spawn(
            [
                Name(value="joint-" + str(k)),
                Transform(x=local_x, y=0.0, z=0.0),
                Parent(parent=parent),
            ]
        )
    )

    scene.spawn(
        [
            Circle(x=0.0, y=0.0, r=r),
            Stroke(width=1.2, color=guide),
            Transform(x=0.0, y=0.0, z=0.0),
            Parent(parent=joints[i]),
        ]
    )

    arrows.append(
        scene.spawn(
            [
                Arrow(Vec2(x=0.0, y=0.0), Vec2(x=r, y=0.0), 0.08, 0.08),
                Fill(color=arrow_colors[i]),
                Transform(x=0.0, y=0.0, z=0.0),
                Parent(parent=joints[i]),
            ]
        )
    )

# 末端点：最后一个向量的尖端（局部位置 r_last）。
scene.spawn(
    [
        Circle(x=0.0, y=0.0, r=0.06),
        Fill(color=rgba(255, 255, 255, 255)),
        Transform(x=radii[-1], y=0.0, z=0.0),
        Parent(parent=arrows[-1]),
    ]
)

# ── 3. 曲线淡入（S6 Opacity，段 [0, FADE)）────────────────────────────
expr.bind(
    expr.Bind(
        object=curve,
        target=Opacity.channel.value,
        root=ramp(0.0, 1.0, FADE),
        clock=expr.clock.shift(0.0),
        predicate=in_segment(FADE),
    )
)
expr.bind(
    expr.Bind(
        object=pulse,
        target=Opacity.channel.value,
        root=ramp(0.0, 1.0, FADE),
        clock=expr.clock.shift(0.0),
        predicate=in_segment(FADE),
    )
)


# ── 4. 主段：所有向量自转（S3 Rotation），相位点公转（S4 Orbit），脉冲书写
def rotation_expr(delta):
    return ramp(0.0, TAU * delta, T)


expr.bind(
    expr.Bind(
        object=arrows[0],
        target=Transform.channel.rotation,
        root=rotation_expr(1),
        clock=expr.clock.shift(MAIN_START),
        predicate=in_segment(T),
    )
)
for i in range(1, len(HARMONICS)):
    expr.bind(
        expr.Bind(
            object=arrows[i],
            target=Transform.channel.rotation,
            root=rotation_expr(HARMONICS[i] - HARMONICS[i - 1]),
            clock=expr.clock.shift(MAIN_START),
            predicate=in_segment(T),
        )
    )
expr.bind(
    expr.Bind(
        object=phase_dot,
        target=Transform.channel.position,
        root=orbit2d(0.18, TAU / T, 0.0),
        clock=expr.clock.shift(MAIN_START),
        predicate=in_segment(T),
    )
)
expr.bind(
    expr.Bind(
        object=pulse,
        target=Reveal.channel.end,
        root=ramp(0.0, 1.0, T),
        clock=expr.clock.shift(MAIN_START),
        predicate=in_segment(T),
    )
)

# ── 5. 相机推近 → 停留 → 拉回（S1 多段语义）─────────────────────────
CAMERA_SCALE = expr.Channel(
    namespace="zela.scene", name="Framing2D.scale", ty=expr.ChannelType.Number
)
CAMERA_CENTER = expr.Channel(
    namespace="zela.scene", name="Framing2D.center", ty=expr.ChannelType.Vec2
)
expr.bind(
    expr.Bind(
        object=camera,
        target=CAMERA_SCALE,
        root=ramp(55.0, 90.0, ZOOM),
        clock=expr.clock.shift(ZOOM_IN_START),
        predicate=in_segment(ZOOM),
    )
)
expr.bind(
    expr.Bind(
        object=camera,
        target=CAMERA_CENTER,
        root=ramp2(0.5, 0.0, -1.5, 0.0, ZOOM),
        clock=expr.clock.shift(ZOOM_IN_START),
        predicate=in_segment(ZOOM),
    )
)
expr.bind(
    expr.Bind(
        object=camera,
        target=CAMERA_SCALE,
        root=ramp(90.0, 55.0, ZOOM),
        clock=expr.clock.shift(ZOOM_OUT_START),
        predicate=in_segment(ZOOM),
    )
)
expr.bind(
    expr.Bind(
        object=camera,
        target=CAMERA_CENTER,
        root=ramp2(-1.5, 0.0, 0.5, 0.0, ZOOM),
        clock=expr.clock.shift(ZOOM_OUT_START),
        predicate=in_segment(ZOOM),
    )
)
