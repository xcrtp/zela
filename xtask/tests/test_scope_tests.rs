//! 阶段 5（按依赖选测试）的门禁级用例：变更分类 + 影响集。
//!
//! 这组函数的错误代价不对称 —— 少算一个下游 crate 就会**静默少跑测试**，
//! 所以三条硬约束（非 crate 输入全量 / 影响集显式打印 / CI 全量）都要有用例守着。

use xtask::{TestScope, is_crate_source, test_scope};

/// 造一份够 `cargo metadata --no-deps` 口径的 JSON：`(包名, 包目录, 内部依赖名…)`。
fn metadata(packages: &[(&str, &str, &[&str])]) -> String {
    let members: Vec<String> = packages
        .iter()
        .map(|(name, dir, _)| {
            format!("\"path+file://{dir}#{name}@0.1.0\"")
        })
        .collect();
    let bodies: Vec<String> = packages
        .iter()
        .map(|(name, dir, deps)| {
            let dependencies: Vec<String> = deps
                .iter()
                .map(|dep| format!("{{\"name\":\"{dep}\",\"path\":\"/repo/crates/{dep}/Cargo.toml\"}}"))
                .collect();
            format!(
                "{{\"id\":\"path+file://{dir}#{name}@0.1.0\",\"name\":\"{name}\",\
                 \"manifest_path\":\"{dir}/Cargo.toml\",\"dependencies\":[{}]}}",
                dependencies.join(",")
            )
        })
        .collect();
    format!(
        "{{\"workspace_members\":[{}],\"packages\":[{}]}}",
        members.join(","),
        bodies.join(",")
    )
}

fn changed(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|value| value.to_string()).collect()
}

#[test]
fn only_crate_rust_sources_are_narrowable() {
    assert!(is_crate_source("crates/kernel/ze-notion/src/path.rs"));
    assert!(is_crate_source("crates\\app\\zela-app\\src\\lib.rs"), "反斜杠也要认");
    // 以下都**不进**依赖图 ⇒ 必须全量。
    for path in [
        "crates/app/zela-app/Cargo.toml",
        "scripts/fourier.lua",
        "crates/render/zela-render/src/shader.wgsl",
        "xtask/src/main.rs",
        "docs/modules/zela.md",
        "crates/app/zela-app/tests/baselines.txt",
    ] {
        assert!(!is_crate_source(path), "{path} 不该被当作可缩小范围的变更");
    }
}

#[test]
fn empty_or_non_crate_changes_fall_back_to_full() {
    let data = metadata(&[("a", "/repo/crates/a", &[])]);
    match test_scope(&data, "/repo", &[]).expect("可计算") {
        TestScope::Full(reason) => assert!(reason.contains("没有变更"), "{reason}"),
        other => panic!("没有变更必须全量：{other:?}"),
    }

    let paths = changed(&["crates/a/src/lib.rs", "scripts/fourier.lua"]);
    match test_scope(&data, "/repo", &paths).expect("可计算") {
        TestScope::Full(reason) => assert!(reason.contains("scripts/fourier.lua"), "{reason}"),
        other => panic!("非 crate 源文件必须全量：{other:?}"),
    }
}

#[test]
fn change_propagates_to_all_downstream_packages() {
    // c 依赖 b，b 依赖 a ⇒ 改 a 影响 a/b/c，改 c 只影响 c。
    let data = metadata(&[
        ("a", "/repo/crates/a", &[]),
        ("b", "/repo/crates/b", &["a"]),
        ("c", "/repo/crates/c", &["b"]),
        ("standalone", "/repo/crates/standalone", &[]),
    ]);

    match test_scope(&data, "/repo", &changed(&["crates/a/src/lib.rs"])).expect("可计算") {
        TestScope::Affected { packages, skipped } => {
            assert_eq!(packages, vec!["a", "b", "c"], "下游闭包要全带上");
            assert_eq!(skipped, 1, "standalone 不依赖 a");
        }
        other => panic!("应缩小范围：{other:?}"),
    }

    match test_scope(&data, "/repo", &changed(&["crates/c/src/lib.rs"])).expect("可计算") {
        TestScope::Affected { packages, skipped } => {
            assert_eq!(packages, vec!["c"], "没有下游 ⇒ 只跑自己");
            assert_eq!(skipped, 3);
        }
        other => panic!("应缩小范围：{other:?}"),
    }
}

#[test]
fn dev_only_edges_still_pull_the_tested_crate() {
    // `zela-app` 通过 dev-dependencies 拉 `zela-render-backends`：改后者必须带上前者。
    let data = metadata(&[
        ("zela-render-backends", "/repo/crates/render/zela-render-backends", &[]),
        ("zela-app", "/repo/crates/app/zela-app", &["zela-render-backends"]),
    ]);
    match test_scope(
        &data,
        "/repo",
        &changed(&["crates/render/zela-render-backends/src/wgpu_2d.rs"]),
    )
    .expect("可计算")
    {
        TestScope::Affected { packages, .. } => {
            assert_eq!(packages, vec!["zela-app", "zela-render-backends"]);
        }
        other => panic!("dev 边也要算：{other:?}"),
    }
}

#[test]
fn nested_package_dirs_use_the_longest_prefix() {
    // `crates/app` 本身不是包，但若将来出现父子同名前缀，必须落到最深的那一个。
    let data = metadata(&[
        ("zela-app", "/repo/crates/app/zela-app", &[]),
        ("zela-playback", "/repo/crates/app/zela-playback", &[]),
    ]);
    match test_scope(&data, "/repo", &changed(&["crates/app/zela-playback/src/lib.rs"]))
        .expect("可计算")
    {
        TestScope::Affected { packages, .. } => assert_eq!(packages, vec!["zela-playback"]),
        other => panic!("应缩小范围：{other:?}"),
    }
}

#[test]
fn paths_outside_any_package_fall_back_to_full() {
    let data = metadata(&[("a", "/repo/crates/a", &[])]);
    match test_scope(&data, "/repo", &changed(&["crates/not-a-package/src/lib.rs"]))
        .expect("可计算")
    {
        TestScope::Full(reason) => assert!(reason.contains("不在任何 workspace 包内"), "{reason}"),
        other => panic!("映射不到包必须全量：{other:?}"),
    }
}
