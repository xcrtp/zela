//! 构造器参数的**默认值白名单**（提案 #6 C）。
//!
//! # 唯一真源
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
        _ => None,
    }
}

/// 声明文件里的默认值字面量（`None` = 必填）。
pub fn field_default_literal(type_name: &str, field_name: &str) -> Option<&'static str> {
    match (type_name, field_name) {
        ("zela.Color", "a") => Some("255"),
        _ => None,
    }
}
