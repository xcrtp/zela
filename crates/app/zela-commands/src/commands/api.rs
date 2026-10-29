//! `api`：导出 domain 能力的机器可读文档（JSON 唯一来源 + LuaLS + TypeScript + `.pyi`）。
//!
//! 帧体搬自 `export_scene_api` example；产物默认写到 `target/scene_api/`。
//! 写脚本有没有补全是天壤之别 ⇒ 这条命令是使用者旅程里的高价值项。

use std::path::PathBuf;

use ze_pod_binding::CapabilityDocs;
use ze_pod_core::Capability;
use ze_pod_func::MathApi;
use zela_scene_ecs::pod::{ContentApi, SceneApi};

use crate::host::{CommandHost, Level};
use crate::response::{Fact, Response};
use crate::spec::{ArgSpec, ArgType, CommandArgs, CommandSpec};

pub const SPEC: CommandSpec = CommandSpec {
    name: "api",
    about: "导出 LuaLS / TypeScript / Python 补全声明",
    args: &[ArgSpec::positional(
        "dir",
        ArgType::Path,
        "输出目录（缺省 target/scene_api）",
    )],
};

/// 产物目录：位置参数；缺省 ⇒ `target/scene_api`。
pub fn output_dir(args: &CommandArgs) -> PathBuf {
    args.path("dir")
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(default_dir)
}

pub fn default_dir() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("target")
        .join("scene_api")
}

/// 执行导出。
pub fn run(host: &dyn CommandHost, dir: &std::path::Path) -> Result<Response, String> {
    // `capabilities` 只在开启 python feature 时追加 PythonApi。
    #[cfg_attr(not(feature = "python"), allow(unused_mut))]
    let mut capabilities = vec![
        CapabilityDocs {
            name: "scene",
            calls: SceneApi::calls(),
            docs: SceneApi::docs(),
            meta: SceneApi::meta(),
            channels: zela_scene_ecs::pod::channels(),
            methods: zela_scene_ecs::pod::methods(),
            ops: &[],
        },
        CapabilityDocs {
            name: "content",
            calls: ContentApi::calls(),
            docs: ContentApi::docs(),
            meta: ContentApi::meta(),
            channels: &[],
            methods: &[],
            ops: &[],
        },
        CapabilityDocs {
            name: "expr",
            calls: MathApi::calls(),
            docs: MathApi::docs(),
            meta: MathApi::meta(),
            channels: &[],
            methods: &[],
            ops: ze_pod_func::SIGNATURES,
        },
        CapabilityDocs {
            name: "python",
            calls: ze_pod_py::PythonApi::calls(),
            docs: ze_pod_py::PythonApi::docs(),
            meta: ze_pod_py::PythonApi::meta(),
            channels: &[],
            methods: &[],
            ops: &[],
        },
    ];

    let [json, lua] = ze_pod_lua::export_docs(dir, &capabilities)
        .map_err(|e| format!("写导出文件失败：{e}"))?;
    host.diagnostic(Level::Info, &format!("JSON  : {}", json.display()));
    host.diagnostic(Level::Info, &format!("LuaLS : {}", lua.display()));

    #[cfg(feature = "js")]
    {
        let paths = ze_pod_js::export_docs(dir, &capabilities)
            .map_err(|e| format!("写 TypeScript 声明失败：{e}"))?;
        for path in paths {
            host.diagnostic(Level::Info, &format!("DTS   : {}", path.display()));
        }
    }
    // `.pyi` 生成器是纯 Rust（不启用 pyo3）⇒ 文档基线在所有机器上都能生成。
    let paths = ze_pod_py::export_docs(dir, &capabilities)
        .map_err(|e| format!("写 Python 类型存根失败：{e}"))?;
    for path in paths {
        host.diagnostic(Level::Info, &format!("PYI   : {}", path.display()));
    }

    let _ = &mut capabilities;
    Ok(Response::facts(
        "api",
        vec![
            ("dir".to_string(), Fact::Text(dir.display().to_string())),
            (
                "scene_functions".to_string(),
                Fact::Count(SceneApi::signatures().len() as u64),
            ),
            (
                "content_functions".to_string(),
                Fact::Count(ContentApi::signatures().len() as u64),
            ),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dir_is_target_scene_api() {
        let dir = output_dir(&CommandArgs::default());
        assert!(dir.ends_with("target/scene_api"), "实际 {dir:?}");
    }
}
