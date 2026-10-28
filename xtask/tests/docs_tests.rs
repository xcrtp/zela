//! 文档工程升级（2026-10-28）的门禁级用例：JSON 解析 / architecture 比对 /
//! 模块文档合并 / 术语表与决策记录的必填字段。

use std::collections::BTreeMap;

use xtask::{
    ArchLayer, ModuleDocFacts, architecture_layers, check_decision, check_glossary,
    compare_architecture, merge_module_doc, parse_json,
};

#[test]
fn json_parses_objects_arrays_and_escapes() {
    let value = parse_json(r#"{"a": [1, 2.5, true, null], "b": "x\ny"}"#).expect("合法 JSON");
    assert_eq!(value.get("b").and_then(|value| value.as_str()), Some("x\ny"));
    let items = value
        .get("a")
        .and_then(|value| value.as_array())
        .expect("数组");
    assert_eq!(items.len(), 4);
    assert!(items[3].is_null());
}

#[test]
fn json_rejects_trailing_content() {
    assert!(parse_json(r#"{"a": 1} trailing"#).is_err());
}

fn layer(name: &str, path: &str, crates: &[&str]) -> ArchLayer {
    ArchLayer {
        name: name.to_string(),
        path: path.to_string(),
        crates: crates.iter().map(|name| name.to_string()).collect(),
    }
}

#[test]
fn architecture_comparison_reports_both_directions() {
    let layers = vec![
        layer("kernel", "crates/kernel", &["ze-a", "ze-gone"]),
        layer("render", "crates/render", &["zela-b"]),
    ];
    let mut members = BTreeMap::new();
    members.insert("ze-a".to_string(), "crates/kernel/ze-a".to_string());
    members.insert("zela-b".to_string(), "crates/scene/zela-b".to_string());
    members.insert("ze-new".to_string(), "crates/kernel/ze-new".to_string());

    let problems = compare_architecture(&layers, &members);
    assert!(
        problems.iter().any(|line| line.contains("ze-gone")),
        "登记了不存在的 crate 必须报出来：{problems:?}"
    );
    assert!(
        problems.iter().any(|line| line.contains("ze-new")),
        "漏登记必须报出来：{problems:?}"
    );
    assert!(
        problems
            .iter()
            .any(|line| line.contains("zela-b") && line.contains("crates/scene")),
        "层路径不符必须报出来：{problems:?}"
    );
}

#[test]
fn architecture_layers_reads_name_path_and_crates() {
    let json = parse_json(
        r#"{"layers": [{"name": "kernel", "path": "crates/kernel", "crates": ["ze-a"]}]}"#,
    )
    .expect("合法 JSON");
    let layers = architecture_layers(&json).expect("layers");
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].crates, vec!["ze-a".to_string()]);
}

fn facts() -> ModuleDocFacts {
    ModuleDocFacts {
        crate_name: "ze-demo".to_string(),
        layer: "kernel".to_string(),
        layer_path: "crates/kernel".to_string(),
        crate_path: "crates/kernel/ze-demo".to_string(),
        upstream: vec!["ze-a".to_string()],
        downstream: Vec::new(),
        files: vec![("src/lib.rs".to_string(), 10, Some("演示 crate".to_string()))],
        edges: Vec::new(),
        api: vec![("src/lib.rs".to_string(), "pub struct Demo".to_string())],
    }
}

#[test]
fn module_doc_merge_replaces_only_the_auto_region() {
    let facts = facts();
    let first = merge_module_doc(None, &facts).expect("新建");
    assert!(first.contains("## 职责"));
    assert!(first.contains("演示 crate"));

    let handwritten = first.replace(
        "（待补：一句话定位 + 它持有什么）",
        "只做演示；不持有状态。",
    );
    let facts_changed = ModuleDocFacts {
        upstream: vec!["ze-a".to_string(), "ze-b".to_string()],
        ..facts
    };
    let merged = merge_module_doc(Some(&handwritten), &facts_changed).expect("合并");
    assert!(
        merged.contains("只做演示；不持有状态。"),
        "手写区必须逐字节保留"
    );
    assert!(merged.contains("`ze-b`"), "自动区必须刷新");
}

#[test]
fn module_doc_merge_requires_markers() {
    let facts = facts();
    assert!(merge_module_doc(Some("# 没有标记\n"), &facts).is_err());
}

#[test]
fn glossary_each_entry_needs_an_anchor() {
    let text = "# 术语表\n\n## Pod\n\n**一句话**：黑箱状态机。\n\n- **代码锚点**：`ze_pod_core::pod::Pod`\n\n## 裸术语\n\n**一句话**：没有锚点。\n";
    let problems = check_glossary(text);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("裸术语"));
}

#[test]
fn glossary_rejects_empty_file() {
    assert!(!check_glossary("# 术语表\n").is_empty());
}

#[test]
fn decision_requires_status_anchor_and_three_sections() {
    let good = "# 001 · 示例\n\n> **状态**：已采纳\n> **锚点**：GAPS AF-01\n> **被谁取代**：—\n\n## 决策\n选 A。\n\n## 理由\n因为 B。\n\n## 反例 / 边界\n当 C 时不成立。\n";
    assert!(check_decision(good, "docs/decisions/001.md").is_empty());

    let missing = "# 001 · 示例\n\n## 决策\n选 A。\n";
    let problems = check_decision(missing, "docs/decisions/001.md");
    assert!(problems.iter().any(|line| line.contains("状态")));
    assert!(problems.iter().any(|line| line.contains("锚点")));
    assert!(problems.iter().any(|line| line.contains("理由")));
}

#[test]
fn decision_superseded_requires_a_replacement() {
    let text = "# 002 · 示例\n\n> **状态**：已被取代\n> **锚点**：GAPS AF-01\n> **被谁取代**：—\n\n## 决策\n选 A。\n\n## 理由\n因为 B。\n\n## 反例 / 边界\n当 C 时不成立。\n";
    let problems = check_decision(text, "docs/decisions/002.md");
    assert!(
        problems.iter().any(|line| line.contains("被谁取代")),
        "{problems:?}"
    );
}
