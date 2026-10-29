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

use ze_pod_core::{PodValue, ScalarValue};

/// 缺失字段的默认值（`None` = 必填）。
pub fn field_default(type_name: &str, field_name: &str) -> Option<PodValue> {
    match (type_name, field_name) {
        ("zela.Color", "a") => Some(PodValue::Scalar(ScalarValue::U8(255))),
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
