//! 声明式命令描述：`CommandSpec` / `ArgSpec`。
//!
//! 与 `OpDef` / `Capability` 同构：**元数据声明 + 多宿主消费**。
//! - `zela`（CLI）：`CommandSpec → clap::Command`（映射在 `zela-cli`，不在本 crate）；
//! - GUI / 未来编辑器：`CommandSpec → 表单控件`；
//! - 测试 / 脚本：直接 [`CommandSpec::parse`] 或 [`CommandSpec::from_pairs`]。
//!
//! # 默认值是契约
//!
//! 每个 `ArgSpec.default` 都直接进渲染参数（fps / 画幅 / 输出路径 / 后端）⇒
//! 改动会改像素。阶段 0 的快照用例锁住这些值，迁移时必须逐条复现。

use std::collections::BTreeMap;

/// 自定义校验闭包（宿主无关：只吃原始字符串）。
pub type Validator = fn(&str) -> Result<(), String>;

/// 参数类型。`Choice` 携带允许值集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgType {
    /// 文件 / 目录路径（按文本保存，由命令层决定是否要求存在）。
    Path,
    /// 任意文本。
    Text,
    /// 浮点数。
    Number,
    /// `WxH`（如 `640x480`）。
    Size,
    /// 布尔开关（出现即 true）。
    Flag,
    /// 枚举（允许值来自 spec）。
    Choice(&'static [&'static str]),
}

impl ArgType {
    /// 帮助里显示的占位符。
    pub fn placeholder(self) -> &'static str {
        match self {
            ArgType::Path => "PATH",
            ArgType::Text => "TEXT",
            ArgType::Number => "N",
            ArgType::Size => "WxH",
            ArgType::Flag => "",
            ArgType::Choice(_) => "VALUE",
        }
    }
}

/// 一个参数的声明。
#[derive(Debug, Clone, Copy)]
pub struct ArgSpec {
    /// 规范名（同时是 CLI 长参数名与 GUI 字段名）。
    pub name: &'static str,
    pub ty: ArgType,
    /// 缺省值（以字符串声明，按 `ty` 解析；`Flag` 缺省为 `"false"`）。
    pub default: Option<&'static str>,
    pub help: &'static str,
    /// 命令层要求的必填项（缺省值不算必填）。
    pub required: bool,
    /// 位置参数（最多一个；如 `render <script>`）。
    pub positional: bool,
    /// 可重复（如 `--work`）；值存进 [`ArgValue::Many`]。
    pub repeatable: bool,
    /// 额外长参数名（如 `render` 的 `--script` 与位置参数同义）。
    pub alias: Option<&'static str>,
    /// 自定义校验（宿主无关）。
    pub validator: Option<Validator>,
}

impl ArgSpec {
    pub const fn option(
        name: &'static str,
        ty: ArgType,
        default: &'static str,
        help: &'static str,
    ) -> Self {
        Self {
            name,
            ty,
            default: Some(default),
            help,
            required: false,
            positional: false,
            repeatable: false,
            alias: None,
            validator: None,
        }
    }

    /// 可选但没有缺省值（如 `--from` / `--to`：不给就是 `None`）。
    pub const fn optional(name: &'static str, ty: ArgType, help: &'static str) -> Self {
        Self {
            name,
            ty,
            default: None,
            help,
            required: false,
            positional: false,
            repeatable: false,
            alias: None,
            validator: None,
        }
    }

    pub const fn required(name: &'static str, ty: ArgType, help: &'static str) -> Self {
        Self {
            name,
            ty,
            default: None,
            help,
            required: true,
            positional: false,
            repeatable: false,
            alias: None,
            validator: None,
        }
    }

    pub const fn positional(name: &'static str, ty: ArgType, help: &'static str) -> Self {
        Self {
            name,
            ty,
            default: None,
            help,
            required: false,
            positional: true,
            repeatable: false,
            alias: None,
            validator: None,
        }
    }

    pub const fn flag(name: &'static str, help: &'static str) -> Self {
        Self {
            name,
            ty: ArgType::Flag,
            default: Some("false"),
            help,
            required: false,
            positional: false,
            repeatable: false,
            alias: None,
            validator: None,
        }
    }

    pub const fn with_alias(mut self, alias: &'static str) -> Self {
        self.alias = Some(alias);
        self
    }

    pub const fn repeatable(mut self) -> Self {
        self.repeatable = true;
        self
    }

    pub const fn with_validator(mut self, validator: Validator) -> Self {
        self.validator = Some(validator);
        self
    }
}

/// 一条命令的声明。
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    pub name: &'static str,
    pub about: &'static str,
    pub args: &'static [ArgSpec],
}

/// 一个参数的解析结果。
#[derive(Debug, Clone, PartialEq)]
pub enum ArgValue {
    Flag(bool),
    Text(String),
    Number(f64),
    Size((u32, u32)),
    Many(Vec<String>),
}

/// 解析后的参数集合（键 = [`ArgSpec::name`]）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CommandArgs {
    values: BTreeMap<&'static str, ArgValue>,
}

impl CommandArgs {
    pub fn get(&self, name: &str) -> Option<&ArgValue> {
        self.values.get(name)
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        match self.values.get(name) {
            Some(ArgValue::Text(value)) => Some(value.as_str()),
            _ => None,
        }
    }

    pub fn path(&self, name: &str) -> Option<std::path::PathBuf> {
        self.text(name).map(std::path::PathBuf::from)
    }

    pub fn number(&self, name: &str) -> Option<f64> {
        match self.values.get(name) {
            Some(ArgValue::Number(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn size(&self, name: &str) -> Option<(u32, u32)> {
        match self.values.get(name) {
            Some(ArgValue::Size(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn flag(&self, name: &str) -> bool {
        matches!(self.values.get(name), Some(ArgValue::Flag(true)))
    }

    /// 重复参数（如 `--work`）；单值参数返回 0 或 1 项。
    pub fn many(&self, name: &str) -> Vec<String> {
        match self.values.get(name) {
            Some(ArgValue::Many(values)) => values.clone(),
            Some(ArgValue::Text(value)) => vec![value.clone()],
            _ => Vec::new(),
        }
    }
}

impl CommandSpec {
    /// **手写宿主解析**（`--help` 与错误信息也走这里）。
    pub fn parse(&self, raw: &[String]) -> Result<CommandArgs, String> {
        if raw.is_empty() || raw.iter().any(|arg| arg == "--help" || arg == "-h") {
            return Err(self.usage());
        }
        let mut seen: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
        let mut positionals = self.args.iter().filter(|arg| arg.positional);

        let mut index = 0;
        while index < raw.len() {
            let token = &raw[index];
            if let Some(long) = token.strip_prefix("--") {
                let arg = self
                    .args
                    .iter()
                    .find(|arg| arg.name == long || arg.alias == Some(long))
                    .ok_or_else(|| format!("未知参数：{token}"))?;
                if arg.ty == ArgType::Flag {
                    seen.entry(arg.name).or_default().push("true".to_string());
                } else {
                    index += 1;
                    let value = raw
                        .get(index)
                        .ok_or_else(|| format!("--{long} 需要一个参数"))?;
                    seen.entry(arg.name).or_default().push(value.clone());
                }
            } else {
                let slot = positionals
                    .next()
                    .ok_or_else(|| format!("多余的参数：{token}"))?;
                seen.entry(slot.name).or_default().push(token.clone());
            }
            index += 1;
        }

        let pairs: Vec<(&'static str, Option<String>)> = seen
            .into_iter()
            .map(|(name, mut values)| {
                let value = if values.len() > 1 {
                    // 重复参数用 `\n` 连接，由 `finish` 展开成 `Many`。
                    Some(values.join("\n"))
                } else {
                    values.pop()
                };
                (name, value)
            })
            .collect();
        self.from_pairs(&pairs)
    }

    /// **另一条宿主的前端**（clap / GUI 表单）只提供「名字 → 值」，类型与默认值在这里统一。
    ///
    /// 这是两条解析路径不漂移的结构保证：`parse` 也最终落到本函数。
    pub fn from_pairs(
        &self,
        pairs: &[(&'static str, Option<String>)],
    ) -> Result<CommandArgs, String> {
        let provided: BTreeMap<&'static str, &str> = pairs
            .iter()
            .filter_map(|(name, value)| value.as_deref().map(|value| (*name, value)))
            .collect();
        let mut values = BTreeMap::new();
        for arg in self.args {
            let raw = provided.get(arg.name).copied().or(arg.default);
            let Some(raw) = raw else {
                if arg.required {
                    return Err(format!("缺少 --{}", arg.name));
                }
                continue;
            };
            if let Some(validator) = arg.validator {
                validator(raw)?;
            }
            values.insert(arg.name, parse_value(arg, raw)?);
        }
        Ok(CommandArgs { values })
    }

    /// 用法 / 帮助文本（手写宿主的 `--help`；CLI 由 clap 生成更完整的版本）。
    pub fn usage(&self) -> String {
        let mut text = format!("用法：zela {}\n\n{}", self.name, self.about);
        if !self.args.is_empty() {
            text.push_str("\n\n参数：\n");
            for arg in self.args {
                let head = if arg.ty == ArgType::Flag {
                    format!("  --{}", arg.name)
                } else if arg.positional {
                    format!("  <{}>", arg.name)
                } else {
                    format!("  --{} <{}>", arg.name, arg.ty.placeholder())
                };
                text.push_str(&head);
                if let Some(default) = arg.default {
                    if default != "false" {
                        text.push_str(&format!("（默认 {default}）"));
                    }
                }
                text.push_str(&format!("  {}", arg.help));
                text.push('\n');
            }
        }
        text
    }
}

fn parse_value(arg: &ArgSpec, raw: &str) -> Result<ArgValue, String> {
    if arg.repeatable {
        let values: Vec<String> = raw
            .split('\n')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
        return Ok(ArgValue::Many(values));
    }
    Ok(match arg.ty {
        ArgType::Flag => ArgValue::Flag(raw == "true"),
        ArgType::Path | ArgType::Text => ArgValue::Text(raw.to_string()),
        ArgType::Number => ArgValue::Number(
            raw.parse()
                .map_err(|_| format!("--{} 必须是数字", arg.name))?,
        ),
        ArgType::Size => {
            let (width, height) = raw
                .split_once('x')
                .ok_or_else(|| format!("--{} 必须是 WxH（如 640x480）", arg.name))?;
            let width = width
                .parse()
                .map_err(|_| format!("--{} 宽度不是数字", arg.name))?;
            let height = height
                .parse()
                .map_err(|_| format!("--{} 高度不是数字", arg.name))?;
            ArgValue::Size((width, height))
        }
        ArgType::Choice(allowed) => {
            if !allowed.contains(&raw) {
                return Err(format!(
                    "--{} 只接受 {}（收到 {raw:?}）",
                    arg.name,
                    allowed.join(" | ")
                ));
            }
            ArgValue::Text(raw.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKENDS: &[&str] = &["auto", "auto-frame", "wgpu", "vector"];

    const SPEC: CommandSpec = CommandSpec {
        name: "demo",
        about: "测试命令",
        args: &[
            ArgSpec::positional("script", ArgType::Path, "脚本").with_alias("script"),
            ArgSpec::option("fps", ArgType::Number, "30", "帧率"),
            ArgSpec::option("size", ArgType::Size, "640x480", "画幅"),
            ArgSpec::option("backend", ArgType::Choice(BACKENDS), "auto", "后端"),
            ArgSpec::flag("hash", "逐帧哈希"),
            ArgSpec::option("work", ArgType::Text, "", "作品").repeatable(),
        ],
    };

    fn raw(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn defaults_are_applied_by_the_spec_not_the_host() {
        let parsed = SPEC.parse(&raw(&["demo.lua"])).expect("可解析");
        assert_eq!(parsed.text("script"), Some("demo.lua"));
        assert_eq!(parsed.number("fps"), Some(30.0));
        assert_eq!(parsed.size("size"), Some((640, 480)));
        assert_eq!(parsed.text("backend"), Some("auto"));
        assert!(!parsed.flag("hash"));
        assert!(!parsed.text("work").is_some_and(|v| !v.is_empty()));
    }

    #[test]
    fn both_frontends_produce_the_same_values() {
        let hand = SPEC.parse(&raw(&["--script", "a.lua", "--size", "1x2"])).expect("手写解析");
        let host = SPEC
            .from_pairs(&[("script", Some("a.lua".to_string())), ("size", Some("1x2".to_string()))])
            .expect("宿主解析");
        assert_eq!(hand, host, "两条前端必须落到同一份 CommandArgs");
    }

    #[test]
    fn repeatable_values_are_kept() {
        let parsed = SPEC
            .parse(&raw(&["a.lua", "--work", "x=1x1", "--work", "y=2x2"]))
            .expect("可解析");
        assert_eq!(parsed.many("work"), vec!["x=1x1", "y=2x2"]);
    }

    #[test]
    fn invalid_values_fail_explicitly() {
        assert!(SPEC.parse(&raw(&["a.lua", "--fps", "x"])).is_err());
        assert!(SPEC.parse(&raw(&["a.lua", "--size", "640"])).is_err());
        assert!(SPEC.parse(&raw(&["a.lua", "--backend", "cpu"])).is_err());
        assert!(SPEC.parse(&raw(&["a.lua", "--bogus"])).is_err());
        assert!(SPEC.parse(&raw(&["a.lua", "b.lua"])).is_err());
    }

    #[test]
    fn help_returns_usage() {
        let usage = SPEC.parse(&raw(&["--help"])).expect_err("--help 走用法分支");
        assert!(usage.contains("用法：zela demo"));
        assert!(usage.contains("--size"));
    }
}
