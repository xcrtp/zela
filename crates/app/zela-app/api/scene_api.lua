---@meta

---@class Handle zela 场景句柄（组件 / 对象 / 曲线 / 动作）

---@class Arrow
---@field from Vec2
---@field to Vec2
---@field head_length number
---@field head_width number

---@param spec Arrow
---@return Handle
function Arrow(spec) end

---@class Axes2D
---@field min_px number
---@field grid LineStyle
---@field axis LineStyle

---@param spec Axes2D
---@return Handle
function Axes2D(spec) end

---@class BackgroundStroke
---@field color Color
---@field width number

---@param spec BackgroundStroke
---@return Handle
function BackgroundStroke(spec) end

---@class Camera2D
---@field center Vec2
---@field scale number

---@param spec Camera2D
---@return Handle
function Camera2D(spec) end

---@class Camera3D
---@field eye Vec3
---@field target Vec3
---@field up Vec3
---@field fov_y number

---@param spec Camera3D
---@return Handle
function Camera3D(spec) end

---@class Circle
---@field x number
---@field y number
---@field r number

---@param spec Circle
---@return Handle
function Circle(spec) end

---@class Color
---@field r integer
---@field g integer
---@field b integer
---@field a integer

---@param spec Color
---@return Color
function Color(spec) end

---@class Cube
---@field half_extent number
---@field solid boolean

---@param spec Cube
---@return Handle
function Cube(spec) end

---@class Curve
Curve = {}

---@class Curve.Const
---@field value number
---@param spec Curve.Const
---@return Curve
function Curve.Const(spec) end

---@class Curve.Sin
---@field base number
---@field amp number
---@field omega number
---@param spec Curve.Sin
---@return Curve
function Curve.Sin(spec) end

---@class Curve.Phase
---@field omega number
---@param spec Curve.Phase
---@return Curve
function Curve.Phase(spec) end

---@class Curve.Ramp
---@field from number
---@field to number
---@field dur number
---@param spec Curve.Ramp
---@return Curve
function Curve.Ramp(spec) end

---@class Curve.Ramp2
---@field from Vec2
---@field to Vec2
---@field dur number
---@param spec Curve.Ramp2
---@return Curve
function Curve.Ramp2(spec) end

---@class Curve.Orbit
---@field radius number
---@field speed number
---@field initial_angle number
---@param spec Curve.Orbit
---@return Curve
function Curve.Orbit(spec) end

---@class Curve.Keyframes
---@field keys table
---@field interpolation KeyframeInterp
---@param spec Curve.Keyframes
---@return Curve
function Curve.Keyframes(spec) end

---@class CurveWindow
---@field lo number
---@field hi number

---@param spec CurveWindow
---@return Handle
function CurveWindow(spec) end

---@class Dash
---@field period number
---@field duty number
---@field phase number

---@param spec Dash
---@return Handle
function Dash(spec) end

---@class Fill
---@field color Color

---@param spec Fill
---@return Handle
function Fill(spec) end

---@class Formula
---@field source string
---@field font_size number
---@field x number
---@field y number

---@param spec Formula
---@return Formula
function Formula(spec) end

---@class Gradient
---@field stops table
---@field from Vec3
---@field to Vec3

---@param spec Gradient
---@return Handle
function Gradient(spec) end

---@class Group
---@field id integer

---@param spec Group
---@return Handle
function Group(spec) end

---@class KeyframeInterp
KeyframeInterp = {}

---@type KeyframeInterp
KeyframeInterp.Linear = {}

---@type KeyframeInterp
KeyframeInterp.Step = {}

---@class Light
---@field kind LightKind
---@field color Color
---@field intensity number

---@param spec Light
---@return Handle
function Light(spec) end

---@class LightKind
LightKind = {}

---@class LightKind.Directional
---@field direction Vec3
---@param spec LightKind.Directional
---@return LightKind
function LightKind.Directional(spec) end

---@class LightKind.Point
---@field range number
---@param spec LightKind.Point
---@return LightKind
function LightKind.Point(spec) end

---@class LineStyle
---@field width number
---@field color Color

---@param spec LineStyle
---@return LineStyle
function LineStyle(spec) end

---@class Material3D
---@field color Color
---@field variant ShaderVariant
---@field metallic number
---@field roughness number
---@field double_sided boolean
---@field transparent boolean
---@field texture Texture

---@param spec Material3D
---@return Handle
function Material3D(spec) end

---@class Morph
---@field from table
---@field to table
---@field closed boolean
---@field t number
---@field samples integer

---@param spec Morph
---@return Handle
function Morph(spec) end

---@class Name
---@field value string

---@param spec Name
---@return Handle
function Name(spec) end

---@class Opacity
---@field value number

---@param spec Opacity
---@return Handle
function Opacity(spec) end

---@class Parent
---@field parent Handle

---@param spec Parent
---@return Handle
function Parent(spec) end

---@class Path
---@field path Handle
---@field color Color
---@field width number

---@param spec Path
---@return Path
function Path(spec) end

---@class PathShape
---@field path Path

---@param spec PathShape
---@return Handle
function PathShape(spec) end

---@class PhaseAnim
---@field phase number
---@field omega number

---@param spec PhaseAnim
---@return Handle
function PhaseAnim(spec) end

---@class Polyline
---@field points table
---@field closed boolean

---@param spec Polyline
---@return Handle
function Polyline(spec) end

---@class Reveal
---@field points table
---@field closed boolean

---@param spec Reveal
---@return Handle
function Reveal(spec) end

---@class RevealPath
---@field path Path

---@param spec RevealPath
---@return Handle
function RevealPath(spec) end

---@class ScalarKey
---@field t number
---@field value number

---@param spec ScalarKey
---@return ScalarKey
function ScalarKey(spec) end

---@class SecondPaint
---@field fill Color
---@field stroke Color
---@field background Color
---@field mix number

---@param spec SecondPaint
---@return Handle
function SecondPaint(spec) end

---@class ShaderVariant
ShaderVariant = {}

---@type ShaderVariant
ShaderVariant.Flat = {}

---@type ShaderVariant
ShaderVariant.Lambert = {}

---@type ShaderVariant
ShaderVariant.Pbr = {}

---@class Stroke
---@field width number
---@field color Color

---@param spec Stroke
---@return Handle
function Stroke(spec) end

---@class Surface
---@field min_x number
---@field min_y number
---@field max_x number
---@field max_y number
---@field mapping SurfaceMapping
---@field anchor_x number
---@field anchor_y number
---@field renderer SurfaceRenderer
---@field texel_width integer
---@field texel_height integer
---@field filter TextureFilter
---@field mipmap boolean
---@field fit_margin number
---@field composite boolean

---@param spec Surface
---@return Handle
function Surface(spec) end

---@class SurfaceMapping
SurfaceMapping = {}

---@type SurfaceMapping
SurfaceMapping.World = {}

---@type SurfaceMapping
SurfaceMapping.Screen = {}

---@class SurfaceRenderer
SurfaceRenderer = {}

---@type SurfaceRenderer
SurfaceRenderer.Renderer2D = {}

---@type SurfaceRenderer
SurfaceRenderer.Renderer3D = {}

---@class Text
---@field source string
---@field font_size number
---@field x number
---@field y number

---@param spec Text
---@return Text
function Text(spec) end

---@class Texture
Texture = {}

---@type Texture
Texture.None = {}

---@class Texture.Surface
---@field surface Handle
---@param spec Texture.Surface
---@return Texture
function Texture.Surface(spec) end

---@class TextureFilter
TextureFilter = {}

---@type TextureFilter
TextureFilter.Nearest = {}

---@type TextureFilter
TextureFilter.Linear = {}

---@class Transform
---@field x number
---@field y number
---@field z number

---@param spec Transform
---@return Handle
function Transform(spec) end

---@class Transform3D
---@field position Vec3
---@field rotation Vec4
---@field scale Vec3

---@param spec Transform3D
---@return Handle
function Transform3D(spec) end

---@class TransformChannel
TransformChannel = {}

---@type TransformChannel
TransformChannel.Position = {}

---@type TransformChannel
TransformChannel.Rotation = {}

---@type TransformChannel
TransformChannel.Scale = {}

---@class Vec2
---@field x number
---@field y number

---@param spec Vec2
---@return Vec2
function Vec2(spec) end

---@class Vec3
---@field x number
---@field y number
---@field z number

---@param spec Vec3
---@return Vec3
function Vec3(spec) end

---@class Vec4
---@field x number
---@field y number
---@field z number
---@field w number

---@param spec Vec4
---@return Vec4
function Vec4(spec) end

---@class Add
---@field a Handle
---@field b Handle

---@param spec Add
---@return Handle
function Add(spec) end

---@class Bind
---@field object Handle
---@field target Channel
---@field root Handle
---@field duration number

---@param spec Bind
---@return Bind
function Bind(spec) end

---@class Channel
---@field namespace string
---@field name string

---@param spec Channel
---@return Channel
function Channel(spec) end

---@class Cos
---@field of Handle

---@param spec Cos
---@return Handle
function Cos(spec) end

---@class Mul
---@field a Handle
---@field b Handle

---@param spec Mul
---@return Handle
function Mul(spec) end

---@class Scale
---@field k number
---@field of Handle

---@param spec Scale
---@return Handle
function Scale(spec) end

---@class Sin
---@field of Handle

---@param spec Sin
---@return Handle
function Sin(spec) end

---@class Sum
---@field terms table

---@param spec Sum
---@return Handle
function Sum(spec) end

---@class SceneApi
scene = {}

---构造圆组件。
---@param shape Circle
---@return Handle
function scene.circle(shape) end

---构造描边组件（颜色是结构体，不再是打包的 `u32`）。
---@param style Stroke
---@return Handle
function scene.stroke(style) end

---构造 2D 相机取景组件。
---@param camera Camera2D
---@return Handle
function scene.camera2d(camera) end

---构造 3D 相机组件（B1-D）。
---@param camera Camera3D
---@return Handle
function scene.camera3d(camera) end

---构造立方体网格组件（B1-D，**参数化**：只传半边长，顶点由宿主生成）。
---@param cube Cube
---@return Handle
function scene.cube(cube) end

---构造 3D 材质组件（B1-D；`metallic` / `roughness` 仅 Pbr 变体消费）。
---@param material Material3D
---@return Handle
function scene.material3d(material) end

---构造光源组件（B1-D；**不带 position** —— 位置由同对象的 `Transform` 提供）。
---@param light Light
---@return Handle
function scene.light(light) end

---构造 3D 摆放组件（B1-D；位置 / 四元数旋转 / 缩放，映射到原生 `Transform`）。
---@param transform Transform3D
---@return Handle
function scene.transform3d(transform) end

---构造书写动画组件（点表是 `Array<Vec2>`）。
---@param reveal Reveal
---@return Handle
function scene.reveal(reveal) end

---构造**静态折线**（S2：可见初态，仍可继续被 RevealRange 书写）。
---@param polyline Polyline
---@return Handle
function scene.polyline(polyline) end

---父对象引用组件（S5：声明期句柄，commit 时解析）。
---@param parent Parent
---@return Handle
function scene.parent(parent) end

---对象名组件。
---@param name Name
---@return Handle
function scene.name(name) end

---对象级不透明度组件（S6）。
---@param opacity Opacity
---@return Handle
function scene.opacity(opacity) end

---构造坐标网格组件（容器；几何由 `AxesRefreshSystem` 每帧生成）。
---@param axes Axes2D
---@return Handle
function scene.axes2d(axes) end

---构造箭头图元。
---@param arrow Arrow
---@return Handle
function scene.arrow(arrow) end

---构造实心填充样式。
---@param fill Fill
---@return Handle
function scene.fill(fill) end

---构造逐对象摆放组件。
---@param transform Transform
---@return Handle
function scene.transform(transform) end

---由内容路径构造书写对象（`Reveal` + `StrokeSolid` 组件组）。
---@param shape RevealPath
---@return Handle
function scene.reveal_path(shape) end

---由内容路径构造静态对象（`Path` + `StrokeSolid` 组件组）。
---@param shape PathShape
---@return Handle
function scene.path_shape(shape) end

---创建对象：组件句柄列表（异构 ⇒ 保持 `Array<Opaque>`）。
---@param components table
---@return Handle
function scene.spawn(components) end

---销毁对象（`commit` 时执行）。
---@param object Handle
function scene.despawn(object) end

---构造动画动作：**目标地址** + **曲线枚举**（未知地址在装配期即报错）。
---
---地址可写旧名（`CircleRadius`）或规范形式（`Circle.radius`）。
---@param target string
---@param curve Curve
---@return Handle
function scene.clip(target, curve) end

---构造 **Transform 通道**动作（S3：Position / Rotation / Scale）。
---
---显式指定写哪一列，因此 `Position` 与 `Rotation` 可以在 `parallel` 里并存。
---B7 起这是写 `Transform` 的**唯一**入口（`Target.TransformValue` 已删除）。
---@param channel TransformChannel
---@param curve Curve
---@return Handle
function scene.clip_transform(channel, curve) end

---顺序播放一段动画（动作 + 时长，秒）。
---@param object Handle
---@param clip Handle
---@param duration number
function scene.animate(object, clip, duration) end

---与**上一段**动画同区间的额外动作。
---@param object Handle
---@param clip Handle
function scene.parallel(object, clip) end

---推进时序游标（不产生动画段）。
---@param seconds number
function scene.wait(seconds) end

---构造 Surface（B3 Step 0）：`Surface` + `SurfaceRenderer` + `SurfaceTexture` 组件组。
---@param surface Surface
---@return Handle
function scene.surface(surface) end

---构造组标签组件（B7 Step 4）：**纯组合**，不做父子变换累乘（与 `Parent` 分工）。
---@param group Group
---@return Handle
function scene.group(group) end

---构造函数图像相位组件（B7 Step 4）：`Target.Phase` 每帧写 `phase`。
---@param phase PhaseAnim
---@return Handle
function scene.phase_anim(phase) end

---构造可变形形状组件（B7 Step 4）：`Target.MorphT` 每帧写 `t`。
---@param morph Morph
---@return Handle
function scene.morph(morph) end

---构造渐变组件（PE）：色标沿两个端点的轴均匀分布（多 stop 原生支持）。
---@param gradient Gradient
---@return Handle
function scene.gradient(gradient) end

---构造虚线组件（PE）：单位是**曲线参数 u**，不是像素。
---@param dash Dash
---@return Handle
function scene.dash(dash) end

---构造第二套 paint 组件（PE）：`mix` 是混合进度。
---@param second SecondPaint
---@return Handle
function scene.second_paint(second) end

---构造背景描边组件（PE）：填充之下的第三色。
---@param background BackgroundStroke
---@return Handle
function scene.background_stroke(background) end

---构造曲线窗口组件（PE）：单位是曲线索引 + t。
---@param window CurveWindow
---@return Handle
function scene.curve_window(window) end

---@class ContentApi
content = {}

---编译公式源码（Typst）为描边路径。
---@param spec Formula
---@return table
function content.formula(spec) end

---编译纯文本为描边路径（正文外观）。
---@param spec Text
---@return table
function content.text(spec) end

---@class ExprApi
expr = {}

---数值字面量（装配期算术的结果落在这里）。
---@param value number
---@return Handle
function expr.pure(value) end

---时间通道（零元组合子；运行期由宿主注入）。
---@return Handle
function expr.time() end

---帧号通道（一等通道）。
---@return Handle
function expr.frame() end

---输入事件通道（`ViewportClick` / `ViewportHover` 的第一个消费方）。
---@return Handle
function expr.event() end

---组件状态通道（经 `SceneRead` 读取）。
---@return Handle
function expr.component_state() end

---运行环境通道（`RuntimeEval`）。
---@return Handle
function expr.env() end

---**通道选择键**：地址 → 通道引用（组件字段 `Circle.radius` 与基础通道同形）。
---
---C 批会把组件构造器变成可调用对象，把 `Circle.radius` 直接挂成常量；
---在那之前这是脚本侧唯一的显式入口。
---@param address string
---@return Channel
function expr.channel(address) end

---`Sin { of = ... }`（表构造器；与 `spawn{}` 同构）。
---@param spec Sin
---@return Handle
function expr.sin(spec) end

---`Cos { of = ... }`。
---@param spec Cos
---@return Handle
function expr.cos(spec) end

---`Add { a = ..., b = ... }`。
---@param spec Add
---@return Handle
function expr.add(spec) end

---`Mul { a = ..., b = ... }`。
---@param spec Mul
---@return Handle
function expr.mul(spec) end

---`Scale { k = ..., of = ... }`（`k` 进产物时是字面量叶子）。
---@param spec Scale
---@return Handle
function expr.scale(spec) end

---`Sum { terms = { ... } }`（空数组 = 零元 `Pure(0)`）。
---@param spec Sum
---@return Handle
function expr.sum(spec) end

---把表达式导出为 JSON（**产物侧证据**：含库 namespace + version）。
---@param root Handle
---@return string
function expr.expression(root) end

---登记一条**表达式动画**（S3）：对象句柄 + 目标通道 + 根句柄 + 时长。
---
---这里只记录；宿主在脚本结束后把它并入 `SceneScript`（句柄在装配期转换，
---产物只存脚本内的局部句柄）。
---
---⚠️ 语义：表达式读**全局** `Time` / `Frame`；`duration` 只决定段区间（`Hold`）。
---需要段内局部时间时应新增通道，而不是改 `Time` 的含义。
---@param spec Bind
function expr.bind(spec) end

---@class PythonApi
python = {}

---在 `[from, to]` 上均匀取 `samples` 个点（Python 执行）。
---@param from number
---@param to number
---@param samples integer
---@return table
function python.linspace(from, to, samples) end

---Python 侧调用注入的 `scene.*` 建一个圆（**真实 Pod → 宿主调用**）。
---@param x number
---@param y number
---@param r number
function python.make_dot(x, y, r) end

