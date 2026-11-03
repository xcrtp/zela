//! `inspect`：读取 `.zsb` 的 Meta（container 版本 / payload 格式 / 能力 / 降级 / 来源）。
//!
//! 用户拿到 `.zsb` 想知道"它是什么、能不能在这台机器回放"——此前没有任何命令能看到。

use std::path::PathBuf;

use zela_scene_bundle::Bundle;

use crate::host::{CommandHost, Level};
use crate::response::{Fact, Response};
use crate::spec::{ArgSpec, ArgType, CommandArgs, CommandSpec};

pub const SPEC: CommandSpec = CommandSpec {
    name: "inspect",
    about: "读取 ZSB 的元信息与可还原性",
    args: &[ArgSpec::positional("file", ArgType::Path, ".zsb 产物路径")],
};

pub fn file_from_args(args: &CommandArgs) -> Result<PathBuf, String> {
    args.path("file").ok_or("缺少 .zsb 路径".to_string())
}

pub fn run(host: &dyn CommandHost, file: &std::path::Path) -> Result<Response, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("读取 {} 失败：{e}", file.display()))?;
    let bundle =
        Bundle::from_bytes(&bytes).map_err(|e| format!("ZSB 解析失败（{}）：{e}", file.display()))?;
    let meta = bundle.meta();

    let restorable = bundle.check_restorable(&zela_app::available_runtime_caps());
    match &restorable {
        Ok(()) => host.diagnostic(Level::Info, "可还原：本机运行时具备所需能力 / Pod"),
        Err(error) => host.diagnostic(
            Level::Warn,
            &format!("不可还原：{error}（这份产物在别的机器上可能能播）"),
        ),
    }

    Ok(Response::facts(
        "inspect",
        vec![
            ("file".to_string(), Fact::Text(file.display().to_string())),
            (
                "container".to_string(),
                Fact::Count(u64::from(meta.container)),
            ),
            (
                "payload_format".to_string(),
                Fact::Text(meta.payload_format.clone()),
            ),
            (
                "objects".to_string(),
                Fact::Count(meta.object_count as u64),
            ),
            (
                "required_capabilities".to_string(),
                Fact::Text(
                    meta.required_capabilities
                        .iter()
                        .map(|c| format!("{}@{:?}", c.name, c.version))
                        .collect::<Vec<_>>()
                        .join(","),
                ),
            ),
            (
                "required_pods".to_string(),
                Fact::Text(
                    meta.required_pods
                        .iter()
                        .map(|p| format!("{}@{}", p.name, p.version))
                        .collect::<Vec<_>>()
                        .join(","),
                ),
            ),
            (
                "downgrades".to_string(),
                Fact::Count(meta.downgrades.len() as u64),
            ),
            (
                "source_sha256".to_string(),
                Fact::Text(meta.source_sha256.clone().unwrap_or_else(|| "—".to_string())),
            ),
            (
                "payload_sha256".to_string(),
                Fact::Text(meta.payload_sha256.clone()),
            ),
            (
                "restorable".to_string(),
                Fact::Flag(restorable.is_ok()),
            ),
        ],
    ))
}
