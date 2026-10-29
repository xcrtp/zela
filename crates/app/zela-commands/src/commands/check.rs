//! `check`：只做装配期检查、不渲染。
//!
//! 30 秒作品 1800 帧，写错要等渲完才知道 ⇒ 反馈循环太贵。本命令把"脚本能不能装配"
//! 从"渲完才知道"里拆出来。

use std::path::PathBuf;

use crate::commands::compile;
use crate::host::{CommandHost, Level};
use crate::response::{Fact, Response};
use crate::spec::{ArgSpec, ArgType, CommandArgs, CommandSpec};

pub const SPEC: CommandSpec = CommandSpec {
    name: "check",
    about: "只检查脚本能否装配（不渲染）",
    args: &[ArgSpec::positional("script", ArgType::Path, "脚本路径")],
};

/// 只取脚本路径。
pub fn script_from_args(args: &CommandArgs) -> Result<PathBuf, String> {
    args.path("script").ok_or("缺少脚本路径".to_string())
}

pub fn run(host: &dyn CommandHost, script: &std::path::Path) -> Result<Response, String> {
    // 装配期可能用到 Typst（content.text）⇒ 先保证确定字体，避免静默回退。
    let _ = crate::commands::fonts::ensure_font_dir();
    let (source, language) = compile::read_script(script)?;
    let compiled = compile::compile(&source, language)?;
    let meta = compiled.bundle.meta();
    host.diagnostic(
        Level::Info,
        &format!(
            "OK：{}（{} 个对象，静止时刻 {}）",
            script.display(),
            meta.object_count,
            compiled.still_time.map_or_else(
                || "不可推导".to_string(),
                |value| format!("≈ {value:.3}s")
            )
        ),
    );
    Ok(Response::facts(
        "check",
        vec![
            ("script".to_string(), Fact::Text(script.display().to_string())),
            ("objects".to_string(), Fact::Count(meta.object_count as u64)),
            (
                "still_time".to_string(),
                Fact::Text(compiled.still_time.map_or_else(
                    || "不可推导".to_string(),
                    |value| format!("{value:.3}"),
                )),
            ),
        ],
    ))
}
