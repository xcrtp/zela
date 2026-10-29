//! `list`：内置作品清单（GUI 下拉与 CLI 共用同一份真源）。
//!
//! 真源是 `zela_app::demos::DEMOS`（编译期 `include_str!`，换机器 / 换目录都能播）。
//! 命令层只做投影，不复制清单。

use zela_app::demos::DEMOS;

use crate::host::{CommandHost, Level};
use crate::response::{DemoRow, ListSummary, Response};
use crate::spec::CommandSpec;

pub const SPEC: CommandSpec = CommandSpec {
    name: "list",
    about: "列出内置作品（id / 标题 / 脚本 / 语言）",
    args: &[],
};

/// 执行清单查询（无帧、无 GPU）。
pub fn run(host: &dyn CommandHost) -> Result<Response, String> {
    let demos: Vec<DemoRow> = DEMOS
        .iter()
        .map(|demo| DemoRow {
            id: demo.id.to_string(),
            title: demo.title.to_string(),
            script: demo.script.to_string(),
            language: format!("{:?}", demo.language),
        })
        .collect();
    host.diagnostic(Level::Info, &format!("内置作品 {} 件", demos.len()));
    Ok(Response::List(ListSummary { demos }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryHost;

    /// 两种宿主的验收点：内存宿主（测试）与 CLI 宿主（`zela-cli` 的用例）都只调本函数。
    #[test]
    fn lists_every_builtin_without_duplicates() {
        let host = MemoryHost::new();
        let response = run(&host).expect("清单命令不该失败");
        let Response::List(summary) = response else {
            panic!("list 必须返回 List");
        };
        assert_eq!(summary.demos.len(), DEMOS.len());
        let mut ids: Vec<&str> = summary.demos.iter().map(|row| row.id.as_str()).collect();
        ids.sort_unstable();
        let mut deduped = ids.clone();
        deduped.dedup();
        assert_eq!(ids, deduped, "id 不得重复");
        assert!(!host.diagnostics().is_empty(), "宿主必须收到一条诊断");
    }
}
