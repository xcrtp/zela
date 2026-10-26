# 由 zela-app 的 export_scene_api 生成；与 scene_api.json 同源。
from typing import Any, List

class Handle:
    """不透明句柄（组件 / 对象 / 曲线 / 动作）。"""

from . import content as content
from . import math as math
from . import python as python
from . import scene as scene

def Arrow(from_: Vec2, to: Vec2, head_length: float, head_width: float) -> Handle: ...

def Axes2D(min_px: float, grid: LineStyle, axis: LineStyle) -> Handle: ...

def BackgroundStroke(color: Color, width: float) -> Handle: ...

def Camera2D(center: Vec2, scale: float) -> Handle: ...

def Camera3D(eye: Vec3, target: Vec3, up: Vec3, fov_y: float) -> Handle: ...

def Circle(x: float, y: float, r: float) -> Handle: ...

class Color:
    r: int
    g: int
    b: int
    a: int
    def __init__(self, r: int, g: int, b: int, a: int) -> None: ...

def Cube(half_extent: float, solid: bool) -> Handle: ...

class Curve:
    @staticmethod
    def Const(value: float) -> Curve: ...
    @staticmethod
    def Sin(base: float, amp: float, omega: float) -> Curve: ...
    @staticmethod
    def Phase(omega: float) -> Curve: ...
    @staticmethod
    def Ramp(from_: float, to: float, dur: float) -> Curve: ...
    @staticmethod
    def Ramp2(from_: Vec2, to: Vec2, dur: float) -> Curve: ...
    @staticmethod
    def Orbit(radius: float, speed: float, initial_angle: float) -> Curve: ...
    @staticmethod
    def Keyframes(keys: List[ScalarKey], interpolation: KeyframeInterp) -> Curve: ...

def CurveWindow(lo: float, hi: float) -> Handle: ...

def Dash(period: float, duty: float, phase: float) -> Handle: ...

def Fill(color: Color) -> Handle: ...

class Formula:
    source: str
    font_size: float
    x: float
    y: float
    def __init__(self, source: str, font_size: float, x: float, y: float) -> None: ...

def Gradient(stops: List[Color], from_: Vec3, to: Vec3) -> Handle: ...

def Group(id: int) -> Handle: ...

class KeyframeInterp:
    Linear: KeyframeInterp
    Step: KeyframeInterp

def Light(kind: LightKind, color: Color, intensity: float) -> Handle: ...

class LightKind:
    @staticmethod
    def Directional(direction: Vec3) -> LightKind: ...
    @staticmethod
    def Point(range: float) -> LightKind: ...

class LineStyle:
    width: float
    color: Color
    def __init__(self, width: float, color: Color) -> None: ...

def Material3D(color: Color, variant: ShaderVariant, metallic: float, roughness: float, double_sided: bool, transparent: bool, texture: Texture) -> Handle: ...

def Morph(from_: List[Vec2], to: List[Vec2], closed: bool, t: float, samples: int) -> Handle: ...

def Name(value: str) -> Handle: ...

def Opacity(value: float) -> Handle: ...

def Parent(parent: Handle) -> Handle: ...

class Path:
    path: Handle
    color: Color
    width: float
    def __init__(self, path: Handle, color: Color, width: float) -> None: ...

def PathShape(path: Path) -> Handle: ...

def PhaseAnim(phase: float, omega: float) -> Handle: ...

def Polyline(points: List[Vec2], closed: bool) -> Handle: ...

def Reveal(points: List[Vec2], closed: bool) -> Handle: ...

def RevealPath(path: Path) -> Handle: ...

class ScalarKey:
    t: float
    value: float
    def __init__(self, t: float, value: float) -> None: ...

def SecondPaint(fill: Color, stroke: Color, background: Color, mix: float) -> Handle: ...

class ShaderVariant:
    Flat: ShaderVariant
    Lambert: ShaderVariant
    Pbr: ShaderVariant

def Stroke(width: float, color: Color) -> Handle: ...

def Surface(min_x: float, min_y: float, max_x: float, max_y: float, mapping: SurfaceMapping, anchor_x: float, anchor_y: float, renderer: SurfaceRenderer, texel_width: int, texel_height: int, filter: TextureFilter, mipmap: bool, fit_margin: float, composite: bool) -> Handle: ...

class SurfaceMapping:
    World: SurfaceMapping
    Screen: SurfaceMapping

class SurfaceRenderer:
    Renderer2D: SurfaceRenderer
    Renderer3D: SurfaceRenderer

class Target:
    CircleRadius: Target
    Phase: Target
    CameraEye: Target
    MorphT: Target
    Camera2DFraming: Target
    RevealRange: Target
    OpacityValue: Target

class Text:
    source: str
    font_size: float
    x: float
    y: float
    def __init__(self, source: str, font_size: float, x: float, y: float) -> None: ...

class Texture:
    None: Texture
    @staticmethod
    def Surface(surface: Handle) -> Texture: ...

class TextureFilter:
    Nearest: TextureFilter
    Linear: TextureFilter

def Transform(x: float, y: float, z: float) -> Handle: ...

def Transform3D(position: Vec3, rotation: Vec4, scale: Vec3) -> Handle: ...

class TransformChannel:
    Position: TransformChannel
    Rotation: TransformChannel
    Scale: TransformChannel

class Vec2:
    x: float
    y: float
    def __init__(self, x: float, y: float) -> None: ...

class Vec3:
    x: float
    y: float
    z: float
    def __init__(self, x: float, y: float, z: float) -> None: ...

class Vec4:
    x: float
    y: float
    z: float
    w: float
    def __init__(self, x: float, y: float, z: float, w: float) -> None: ...

