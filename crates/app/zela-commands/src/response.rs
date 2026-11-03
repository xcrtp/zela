//! 结构化返回值。
//!
//! 命令**返回结构**，CLI 负责格式化，GUI 负责显示。
//! 若返回打印好的字符串，GUI 就只能 parse stdout ⇒ 抽象白做。

use std::path::PathBuf;

/// 一条内置作品（`list` 的行，也是 GUI 下拉的数据源）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoRow {
    pub id: String,
    pub title: String,
    pub script: String,
    pub language: String,
}

/// 一次渲染的摘要（`render` / `frames`）。
#[derive(Debug, Clone, PartialEq)]
pub struct RenderSummary {
    pub frames: u32,
    pub size: (u32, u32),
    pub fps: f64,
    pub backend: String,
    /// ZSB 载入时的 payload hash（脚本路径 ⇒ `None`）。
    pub payload_hash: Option<String>,
    /// mp4 输出（`--out`）。
    pub out: Option<PathBuf>,
    /// PNG 序列目录（`--frames`）。
    pub frames_dir: Option<PathBuf>,
    /// 是否因取消而提前结束（已渲帧保留）。
    pub cancelled: bool,
    /// 序列帧已完整（manifest 覆盖全部帧）⇒ 本次没有渲染。
    pub skipped: bool,
}

/// 作品清单摘要（`list`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListSummary {
    pub demos: Vec<DemoRow>,
}

/// 通用事实（阶段 2 迁移其余命令时逐步收窄为专用结构）。
#[derive(Debug, Clone, PartialEq)]
pub enum Fact {
    Text(String),
    Number(f64),
    Count(u64),
    Flag(bool),
}

impl Fact {
    fn json(&self) -> String {
        match self {
            Fact::Text(value) => format!("\"{}\"", escape(value)),
            Fact::Number(value) => format!("{value}"),
            Fact::Count(value) => format!("{value}"),
            Fact::Flag(value) => format!("{value}"),
        }
    }
}

/// 命令的统一返回。
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    Render(RenderSummary),
    List(ListSummary),
    Facts {
        command: &'static str,
        facts: Vec<(String, Fact)>,
    },
}

impl Response {
    /// 通用事实响应的构造器。
    pub fn facts(command: &'static str, facts: Vec<(String, Fact)>) -> Self {
        Response::Facts { command, facts }
    }

    pub fn command(&self) -> &'static str {
        match self {
            Response::Render(_) => "render",
            Response::List(_) => "list",
            Response::Facts { command, .. } => command,
        }
    }

    /// 产出的文件（CLI 据此打印 "完成：…"；GUI 据此打开结果）。
    pub fn files(&self) -> Vec<PathBuf> {
        match self {
            Response::Render(summary) => {
                let mut files = Vec::new();
                if let Some(dir) = &summary.frames_dir {
                    files.push(dir.clone());
                }
                if let Some(out) = &summary.out {
                    files.push(out.clone());
                }
                files
            }
            Response::List(_) | Response::Facts { .. } => Vec::new(),
        }
    }

    /// 机器可读输出（一行 JSON）。
    pub fn to_json(&self) -> String {
        match self {
            Response::Render(summary) => format!(
                "{{\"command\":\"render\",\"frames\":{},\"size\":\"{}x{}\",\"fps\":{},\
                 \"backend\":\"{}\",\"payload\":{},\"out\":{},\"frames_dir\":{},\"cancelled\":{},\
                 \"skipped\":{}}}",
                summary.frames,
                summary.size.0,
                summary.size.1,
                summary.fps,
                escape(&summary.backend),
                opt_string(summary.payload_hash.as_deref()),
                opt_path(summary.out.as_deref()),
                opt_path(summary.frames_dir.as_deref()),
                summary.cancelled,
                summary.skipped,
            ),
            Response::List(summary) => {
                let rows: Vec<String> = summary
                    .demos
                    .iter()
                    .map(|row| {
                        format!(
                            "{{\"id\":\"{}\",\"title\":\"{}\",\"script\":\"{}\",\"language\":\"{}\"}}",
                            escape(&row.id),
                            escape(&row.title),
                            escape(&row.script),
                            escape(&row.language),
                        )
                    })
                    .collect();
                format!("{{\"command\":\"list\",\"demos\":[{}]}}", rows.join(","))
            }
            Response::Facts { command, facts } => {
                let pairs: Vec<String> = facts
                    .iter()
                    .map(|(name, fact)| format!("\"{}\":{}", escape(name), fact.json()))
                    .collect();
                format!(
                    "{{\"command\":\"{}\",{}}}",
                    escape(command),
                    pairs.join(",")
                )
            }
        }
    }
}

fn opt_string(value: Option<&str>) -> String {
    match value {
        Some(value) => format!("\"{}\"", escape(value)),
        None => "null".to_string(),
    }
}

fn opt_path(value: Option<&std::path::Path>) -> String {
    match value {
        Some(value) => format!("\"{}\"", escape(&value.to_string_lossy())),
        None => "null".to_string(),
    }
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}
