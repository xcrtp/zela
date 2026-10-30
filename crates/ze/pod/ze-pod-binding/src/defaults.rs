//! 构造器参数的**默认值白名单**（提案 #6 C）。
//!
//! # 唯一来源
//!
//! 三个语言绑定（Lua / Python / JS）在"字段缺失"时都读这里；声明文件生成器
//! 也从这里取"哪些字段可选、默认是多少"。手写第二份就会漂移。
//!
//! # 为什么默认值不进产物
//!
//! 默认值只是**书写便利**：构造完的值一定是完整的（缺字段在边界就补上），
//! 所以产物里没有任何"默认"痕迹。payload 与显式写满的脚本逐字节相同。
//!
//! # 白名单只有一个
//!
//! `Color.a = 255` —— 其余字段"天然有序/必填"（计划 §5.3 的实测：字段数 1–3）。
//!
//! # 第 BB 批（阶段 3）：把"每个 Demo 都写满参数结构体"消掉
//!
//! 只给**现在大家都写同一个值**的字段加默认值（判据：用默认值重写现有脚本后
//! payload 逐位不变）。实测分布见 GAPS `BB-07`：`Transform.z` 14/18 写 `0.0`、
//! `Camera2D.center` 17/21 写 `(0, 0)`、`Material3D.metallic` 16/19 写 `0.0` 等。
//! **`Light` 不加默认**：颜色与强度逐个作品都不同，没有"普遍值"可对齐。

use ze_pod_core::{PodValue, ScalarValue};

/// `zela.Vec2` 常量（默认值用；与脚本写 `Vec2 { x = …, y = … }` 同一形状）。
fn vec2(x: f32, y: f32) -> PodValue {
    PodValue::Struct(
        "zela.Vec2",
        vec![
            ("x", PodValue::Scalar(ScalarValue::F32(x))),
            ("y", PodValue::Scalar(ScalarValue::F32(y))),
        ],
    )
}

/// `zela.Vec3` 常量。
fn vec3(x: f32, y: f32, z: f32) -> PodValue {
    PodValue::Struct(
        "zela.Vec3",
        vec![
            ("x", PodValue::Scalar(ScalarValue::F32(x))),
            ("y", PodValue::Scalar(ScalarValue::F32(y))),
            ("z", PodValue::Scalar(ScalarValue::F32(z))),
        ],
    )
}

/// 缺失字段的默认值（`None` = 必填）。
pub fn field_default(type_name: &str, field_name: &str) -> Option<PodValue> {
    match (type_name, field_name) {
        ("zela.Color", "a") => Some(PodValue::Scalar(ScalarValue::U8(255))),
        // 2D 相机：绝大多数作品都居中取景。
        ("zela.Camera2D", "center") => Some(vec2(0.0, 0.0)),
        // 3D 相机：看向原点、上方向 +Y 是默认取景。
        ("zela.Camera3D", "target") => Some(vec3(0.0, 0.0, 0.0)),
        ("zela.Camera3D", "up") => Some(vec3(0.0, 1.0, 0.0)),
        // 2D 摆放：z 只用于少量分层场景，多数作品写 0。
        ("zela.Transform", "z") => Some(PodValue::Scalar(ScalarValue::F32(0.0))),
        // 父子继承权重：变换三维默认全继承（旧行为），样式三维默认不继承（旧行为）。
        ("zela.Parent", "position") => Some(PodValue::Scalar(ScalarValue::F32(1.0))),
        ("zela.Parent", "rotation") => Some(PodValue::Scalar(ScalarValue::F32(1.0))),
        ("zela.Parent", "scale") => Some(PodValue::Scalar(ScalarValue::F32(1.0))),
        ("zela.Parent", "opacity") => Some(PodValue::Scalar(ScalarValue::F32(0.0))),
        ("zela.Parent", "fill") => Some(PodValue::Scalar(ScalarValue::F32(0.0))),
        ("zela.Parent", "stroke") => Some(PodValue::Scalar(ScalarValue::F32(0.0))),
        // 3D 材质：Lambert + 非金属 + 中等粗糙 + 不双面 + 不透明 + 无纹理。
        ("zela.Material3D", "variant") => Some(PodValue::Enum(
            "zela.ShaderVariant",
            "Lambert",
            Vec::new(),
        )),
        ("zela.Material3D", "metallic") => Some(PodValue::Scalar(ScalarValue::F32(0.0))),
        ("zela.Material3D", "roughness") => Some(PodValue::Scalar(ScalarValue::F32(0.5))),
        ("zela.Material3D", "double_sided") => Some(PodValue::Scalar(ScalarValue::Bool(false))),
        ("zela.Material3D", "transparent") => Some(PodValue::Scalar(ScalarValue::Bool(false))),
        ("zela.Material3D", "texture") => Some(PodValue::Enum("zela.Texture", "None", Vec::new())),
        // 绑定的默认时钟 = 空链（全局时钟）；显式写 `clock = clock.shift(…)` 才生效。
        ("zela.math.Bind", "clock") => Some(PodValue::Struct(
            "zela.math.Clock",
            vec![("links", PodValue::Array(Vec::new()))],
        )),
        // 空时钟（`Clock {}`）等价于全局时钟。
        ("zela.math.Clock", "links") => Some(PodValue::Array(Vec::new())),
        // 绑定缺省：恒真 predicate + hold + 无 duration 糖。
        ("zela.math.Bind", "predicate") => Some(PodValue::Enum(
            "zela.math.Predicate",
            "Always",
            Vec::new(),
        )),
        ("zela.math.Bind", "reset") => {
            Some(PodValue::Enum("zela.math.Reset", "Hold", Vec::new()))
        }
        ("zela.math.Bind", "duration") => Some(PodValue::Scalar(ScalarValue::F64(0.0))),
        // B4：批量绑定与单条绑定共用同一组默认（时钟 / predicate / reset）。
        // `with` / `without` **没有默认**：过滤条件必须显式写（否则等于全选）。
        ("zela.math.BindEach", "clock") => Some(PodValue::Struct(
            "zela.math.Clock",
            vec![("links", PodValue::Array(Vec::new()))],
        )),
        ("zela.math.BindEach", "predicate") => Some(PodValue::Enum(
            "zela.math.Predicate",
            "Always",
            Vec::new(),
        )),
        ("zela.math.BindEach", "reset") => {
            Some(PodValue::Enum("zela.math.Reset", "Hold", Vec::new()))
        }
        _ => None,
    }
}

/// 声明文件里的默认值字面量（`None` = 必填）。
pub fn field_default_literal(type_name: &str, field_name: &str) -> Option<&'static str> {
    match (type_name, field_name) {
        ("zela.Color", "a") => Some("255"),
        // 2D / 3D 取景与摆放的普遍值（见 GAPS BB-07 的实测分布）。
        ("zela.Camera2D", "center") => Some("Vec2(0, 0)"),
        ("zela.Camera3D", "target") => Some("Vec3(0, 0, 0)"),
        ("zela.Camera3D", "up") => Some("Vec3(0, 1, 0)"),
        ("zela.Transform", "z") => Some("0"),
        ("zela.Parent", "position") => Some("1"),
        ("zela.Parent", "rotation") => Some("1"),
        ("zela.Parent", "scale") => Some("1"),
        ("zela.Parent", "opacity") => Some("0"),
        ("zela.Parent", "fill") => Some("0"),
        ("zela.Parent", "stroke") => Some("0"),
        ("zela.Material3D", "variant") => Some("ShaderVariant.Lambert"),
        ("zela.Material3D", "metallic") => Some("0"),
        ("zela.Material3D", "roughness") => Some("0.5"),
        ("zela.Material3D", "double_sided") => Some("false"),
        ("zela.Material3D", "transparent") => Some("false"),
        ("zela.Material3D", "texture") => Some("Texture.None"),
        // 结构体默认值：三种语言都能解析的构造表达式（`.pyi` 里是合法的默认值写法）。
        ("zela.math.Bind", "clock") => Some("Clock()"),
        ("zela.math.Clock", "links") => Some("[]"),
        ("zela.math.Bind", "predicate") => Some("Predicate.Always"),
        ("zela.math.Bind", "reset") => Some("Reset.Hold"),
        ("zela.math.Bind", "duration") => Some("0"),
        ("zela.math.BindEach", "clock") => Some("Clock()"),
        ("zela.math.BindEach", "predicate") => Some("Predicate.Always"),
        ("zela.math.BindEach", "reset") => Some("Reset.Hold"),
        _ => None,
    }
}
