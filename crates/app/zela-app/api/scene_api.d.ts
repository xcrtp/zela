// 自动生成：zela 脚本能力声明（TypeScript）
// JS 只有 number ⇒ i32/u32/u64/u8 一律映射为 number，整数语义在运行时校验。

/** zela 场景句柄（组件 / 对象 / 曲线 / 动作） */
export type Handle = { readonly __zela_handle: unique symbol };

export interface Arrow {
  from: Vec2;
  to: Vec2;
  head_length: number;
  head_width: number;
}
export declare function Arrow(spec: Arrow): Handle;

export interface Axes2D {
  min_px: number;
  grid: LineStyle;
  axis: LineStyle;
}
export declare function Axes2D(spec: Axes2D): Handle;

export interface BackgroundStroke {
  color: Color;
  width: number;
}
export declare function BackgroundStroke(spec: BackgroundStroke): Handle;

export interface Camera2D {
  center: Vec2;
  scale: number;
}
export declare function Camera2D(spec: Camera2D): Handle;

export interface Camera3D {
  eye: Vec3;
  target: Vec3;
  up: Vec3;
  fov_y: number;
}
export declare function Camera3D(spec: Camera3D): Handle;

export interface Circle {
  x: number;
  y: number;
  r: number;
}
export declare function Circle(spec: Circle): Handle;

export interface Color {
  r: number;
  g: number;
  b: number;
  a: number;
}
export declare function Color(spec: Color): Color;

export interface Cube {
  half_extent: number;
  solid: boolean;
}
export declare function Cube(spec: Cube): Handle;

export type Curve = Curve.Value;
export declare namespace Curve {
  interface Value { readonly __zela_enum: unique symbol }
  function Const(spec: { value: number }): Value;
  function Sin(spec: { base: number; amp: number; omega: number }): Value;
  function Phase(spec: { omega: number }): Value;
  function Ramp(spec: { from: number; to: number; dur: number }): Value;
  function Ramp2(spec: { from: Vec2; to: Vec2; dur: number }): Value;
  function Orbit(spec: { radius: number; speed: number; initial_angle: number }): Value;
  function Keyframes(spec: { keys: ScalarKey[]; interpolation: KeyframeInterp }): Value;
}

export interface CurveWindow {
  lo: number;
  hi: number;
}
export declare function CurveWindow(spec: CurveWindow): Handle;

export interface Dash {
  period: number;
  duty: number;
  phase: number;
}
export declare function Dash(spec: Dash): Handle;

export interface Fill {
  color: Color;
}
export declare function Fill(spec: Fill): Handle;

export interface Formula {
  source: string;
  font_size: number;
  x: number;
  y: number;
}
export declare function Formula(spec: Formula): Formula;

export interface Gradient {
  stops: Color[];
  from: Vec3;
  to: Vec3;
}
export declare function Gradient(spec: Gradient): Handle;

export interface Group {
  id: number;
}
export declare function Group(spec: Group): Handle;

export type KeyframeInterp = KeyframeInterp.Value;
export declare namespace KeyframeInterp {
  interface Value { readonly __zela_enum: unique symbol }
  const Linear: Value;
  const Step: Value;
}

export interface Light {
  kind: LightKind;
  color: Color;
  intensity: number;
}
export declare function Light(spec: Light): Handle;

export type LightKind = LightKind.Value;
export declare namespace LightKind {
  interface Value { readonly __zela_enum: unique symbol }
  function Directional(spec: { direction: Vec3 }): Value;
  function Point(spec: { range: number }): Value;
}

export interface LineStyle {
  width: number;
  color: Color;
}
export declare function LineStyle(spec: LineStyle): LineStyle;

export interface Material3D {
  color: Color;
  variant: ShaderVariant;
  metallic: number;
  roughness: number;
  double_sided: boolean;
  transparent: boolean;
  texture: Texture;
}
export declare function Material3D(spec: Material3D): Handle;

export interface Morph {
  from: Vec2[];
  to: Vec2[];
  closed: boolean;
  t: number;
  samples: number;
}
export declare function Morph(spec: Morph): Handle;

export interface Name {
  value: string;
}
export declare function Name(spec: Name): Handle;

export interface Opacity {
  value: number;
}
export declare function Opacity(spec: Opacity): Handle;

export interface Parent {
  parent: Handle;
}
export declare function Parent(spec: Parent): Handle;

export interface Path {
  path: Handle;
  color: Color;
  width: number;
}
export declare function Path(spec: Path): Path;

export interface PathShape {
  path: Path;
}
export declare function PathShape(spec: PathShape): Handle;

export interface PhaseAnim {
  phase: number;
  omega: number;
}
export declare function PhaseAnim(spec: PhaseAnim): Handle;

export interface Polyline {
  points: Vec2[];
  closed: boolean;
}
export declare function Polyline(spec: Polyline): Handle;

export interface Reveal {
  points: Vec2[];
  closed: boolean;
}
export declare function Reveal(spec: Reveal): Handle;

export interface RevealPath {
  path: Path;
}
export declare function RevealPath(spec: RevealPath): Handle;

export interface ScalarKey {
  t: number;
  value: number;
}
export declare function ScalarKey(spec: ScalarKey): ScalarKey;

export interface SecondPaint {
  fill: Color;
  stroke: Color;
  background: Color;
  mix: number;
}
export declare function SecondPaint(spec: SecondPaint): Handle;

export type ShaderVariant = ShaderVariant.Value;
export declare namespace ShaderVariant {
  interface Value { readonly __zela_enum: unique symbol }
  const Flat: Value;
  const Lambert: Value;
  const Pbr: Value;
}

export interface Stroke {
  width: number;
  color: Color;
}
export declare function Stroke(spec: Stroke): Handle;

export interface Surface {
  min_x: number;
  min_y: number;
  max_x: number;
  max_y: number;
  mapping: SurfaceMapping;
  anchor_x: number;
  anchor_y: number;
  renderer: SurfaceRenderer;
  texel_width: number;
  texel_height: number;
  filter: TextureFilter;
  mipmap: boolean;
  fit_margin: number;
  composite: boolean;
}
export declare function Surface(spec: Surface): Handle;

export type SurfaceMapping = SurfaceMapping.Value;
export declare namespace SurfaceMapping {
  interface Value { readonly __zela_enum: unique symbol }
  const World: Value;
  const Screen: Value;
}

export type SurfaceRenderer = SurfaceRenderer.Value;
export declare namespace SurfaceRenderer {
  interface Value { readonly __zela_enum: unique symbol }
  const Renderer2D: Value;
  const Renderer3D: Value;
}

export interface Text {
  source: string;
  font_size: number;
  x: number;
  y: number;
}
export declare function Text(spec: Text): Text;

export type Texture = Texture.Value;
export declare namespace Texture {
  interface Value { readonly __zela_enum: unique symbol }
  const None: Value;
  function Surface(spec: { surface: Handle }): Value;
}

export type TextureFilter = TextureFilter.Value;
export declare namespace TextureFilter {
  interface Value { readonly __zela_enum: unique symbol }
  const Nearest: Value;
  const Linear: Value;
}

export interface Transform {
  x: number;
  y: number;
  z: number;
}
export declare function Transform(spec: Transform): Handle;

export interface Transform3D {
  position: Vec3;
  rotation: Vec4;
  scale: Vec3;
}
export declare function Transform3D(spec: Transform3D): Handle;

export type TransformChannel = TransformChannel.Value;
export declare namespace TransformChannel {
  interface Value { readonly __zela_enum: unique symbol }
  const Position: Value;
  const Rotation: Value;
  const Scale: Value;
}

export interface Vec2 {
  x: number;
  y: number;
}
export declare function Vec2(spec: Vec2): Vec2;

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}
export declare function Vec3(spec: Vec3): Vec3;

export interface Vec4 {
  x: number;
  y: number;
  z: number;
  w: number;
}
export declare function Vec4(spec: Vec4): Vec4;

export interface Add {
  a: Handle;
  b: Handle;
}
export declare function Add(spec: Add): Handle;

export interface Bind {
  object: Handle;
  target: Channel;
  root: Handle;
  duration: number;
}
export declare function Bind(spec: Bind): Bind;

export interface Channel {
  namespace: string;
  name: string;
}
export declare function Channel(spec: Channel): Channel;

export interface Cos {
  of: Handle;
}
export declare function Cos(spec: Cos): Handle;

export interface Mul {
  a: Handle;
  b: Handle;
}
export declare function Mul(spec: Mul): Handle;

export interface Scale {
  k: number;
  of: Handle;
}
export declare function Scale(spec: Scale): Handle;

export interface Sin {
  of: Handle;
}
export declare function Sin(spec: Sin): Handle;

export interface Sum {
  terms: Handle[];
}
export declare function Sum(spec: Sum): Handle;

export declare const scene: {
  circle(shape: Circle): Handle;
  stroke(style: Stroke): Handle;
  camera2d(camera: Camera2D): Handle;
  camera3d(camera: Camera3D): Handle;
  cube(cube: Cube): Handle;
  material3d(material: Material3D): Handle;
  light(light: Light): Handle;
  transform3d(transform: Transform3D): Handle;
  reveal(reveal: Reveal): Handle;
  polyline(polyline: Polyline): Handle;
  parent(parent: Parent): Handle;
  name(name: Name): Handle;
  opacity(opacity: Opacity): Handle;
  axes2d(axes: Axes2D): Handle;
  arrow(arrow: Arrow): Handle;
  fill(fill: Fill): Handle;
  transform(transform: Transform): Handle;
  reveal_path(shape: RevealPath): Handle;
  path_shape(shape: PathShape): Handle;
  spawn(components: Handle[]): Handle;
  despawn(object: Handle): void;
  clip(target: string, curve: Curve): Handle;
  clip_transform(channel: TransformChannel, curve: Curve): Handle;
  animate(object: Handle, clip: Handle, duration: number): void;
  parallel(object: Handle, clip: Handle): void;
  wait(seconds: number): void;
  surface(surface: Surface): Handle;
  group(group: Group): Handle;
  phase_anim(phase: PhaseAnim): Handle;
  morph(morph: Morph): Handle;
  gradient(gradient: Gradient): Handle;
  dash(dash: Dash): Handle;
  second_paint(second: SecondPaint): Handle;
  background_stroke(background: BackgroundStroke): Handle;
  curve_window(window: CurveWindow): Handle;
};

export declare const content: {
  formula(spec: Formula): Path[];
  text(spec: Text): Path[];
};

export declare const expr: {
  pure(value: number): Handle;
  time(): Handle;
  frame(): Handle;
  event(): Handle;
  component_state(): Handle;
  env(): Handle;
  channel(address: string): Channel;
  sin(spec: Sin): Handle;
  cos(spec: Cos): Handle;
  add(spec: Add): Handle;
  mul(spec: Mul): Handle;
  scale(spec: Scale): Handle;
  sum(spec: Sum): Handle;
  expression(root: Handle): string;
  bind(spec: Bind): void;
};

export declare const python: {
  linspace(from: number, to: number, samples: number): number[];
  make_dot(x: number, y: number, r: number): void;
};

