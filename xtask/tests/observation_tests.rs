// 执行计划（结构指纹 + 统一门禁）的纯函数用例：等式判据 / 不等式判据 / 覆盖处置。
use std::collections::{BTreeMap, BTreeSet};

use xtask::{
    Disposition, StructureObservation, check_dispositions, compare_bench, compare_structures,
    criterion_median_nanos, parse_bench_baselines, parse_dispositions, parse_structures,
    render_structures,
};

fn observation(work: &str, backend: &str, pass: &str, digest: &str) -> StructureObservation {
    StructureObservation {
        work: work.to_string(),
        backend: backend.to_string(),
        pass: pass.to_string(),
        digest: digest.to_string(),
    }
}

#[test]
fn structures_roundtrip_and_reject_bad_digest() {
    let text = "# 注释\nfourier = 0123456789abcdef\natlas = fedcba9876543210 @ deadbee\n";
    let parsed = parse_structures(text).unwrap();
    assert_eq!(parsed.get("fourier").unwrap(), "0123456789abcdef");
    assert_eq!(parsed.get("atlas").unwrap(), "fedcba9876543210");

    let rendered = render_structures(&parsed);
    assert_eq!(parse_structures(&rendered).unwrap(), parsed);

    assert!(parse_structures("fourier = 1234\n").is_err());
    assert!(parse_structures("fourier = zzzzzzzzzzzzzzzz\n").is_err());
}

#[test]
fn structures_require_equality_across_pass_and_backend() {
    let baseline: BTreeMap<String, String> = [("fourier".to_string(), "0123456789abcdef".to_string())]
        .into_iter()
        .collect();
    let same = vec![
        observation("fourier", "wgpu", "script", "0123456789abcdef"),
        observation("fourier", "wgpu", "bundle", "0123456789abcdef"),
        observation("fourier", "vector", "script", "0123456789abcdef"),
    ];
    assert_eq!(compare_structures(&baseline, &same).unwrap(), 1);

    // 后端之间不同 ⇒ 状态被渲染路径改过（真问题）。
    let mut differs = same.clone();
    differs.push(observation("fourier", "vector", "bundle", "ffffffffffffffff"));
    let error = compare_structures(&baseline, &differs).unwrap_err();
    assert!(error.contains("不一致"), "{error}");

    // 基线不同 ⇒ 状态变了。
    let mut changed = same;
    changed[0].digest = "0000000000000000".to_string();
    changed[1].digest = "0000000000000000".to_string();
    changed[2].digest = "0000000000000000".to_string();
    let error = compare_structures(&baseline, &changed).unwrap_err();
    assert!(error.contains("与基线不符"), "{error}");

    // 没有基线 ⇒ 不许静默放过。
    let error = compare_structures(&BTreeMap::new(), &changed).unwrap_err();
    assert!(error.contains("缺少"), "{error}");
}

#[test]
fn criterion_median_is_parsed_from_estimates_json_in_nanos() {
    let json = r#"{
      "mean": { "confidence_interval": { "lower_bound": 1.0 }, "point_estimate": 9.9 },
      "median": { "confidence_interval": { "lower_bound": 2.0 }, "point_estimate": 0.000123, "standard_error": 0.1 },
      "median_abs_dev": { "point_estimate": 3.3 }
    }"#;
    let nanos = criterion_median_nanos(json).unwrap();
    assert!((nanos - 0.000123).abs() < 1e-12, "got {nanos}");
}

#[test]
fn bench_thresholds_are_20_percent_warn_and_50_percent_fail() {
    let baseline: BTreeMap<String, f64> = [
        ("ok".to_string(), 1.0),
        ("warn".to_string(), 1.0),
        ("fail".to_string(), 1.0),
    ]
    .into_iter()
    .collect();
    let current: BTreeMap<String, f64> = [
        ("ok".to_string(), 1.1),
        ("warn".to_string(), 1.25),
        ("fail".to_string(), 1.6),
    ]
    .into_iter()
    .collect();

    let (warnings, failures) = compare_bench(&baseline, &current);
    assert_eq!(
        warnings.iter().map(|delta| delta.key.as_str()).collect::<Vec<_>>(),
        vec!["warn"]
    );
    assert_eq!(
        failures.iter().map(|delta| delta.key.as_str()).collect::<Vec<_>>(),
        vec!["fail"]
    );
}

#[test]
fn bench_baselines_parse_rejects_non_positive() {
    let parsed = parse_bench_baselines("# dev\nze-comb/eval_bench/eval = 0.00042\n").unwrap();
    assert!((parsed["ze-comb/eval_bench/eval"] - 0.00042).abs() < 1e-12);
    assert!(parse_bench_baselines("x = 0\n").is_err());
    assert!(parse_bench_baselines("x = -1\n").is_err());
}

#[test]
fn dispositions_must_cover_every_script_and_be_real() {
    let text = "\
fourier.lua = baseline
sine_trace.lua = exempt: 表达式读端探针，无像素基线；结构指纹覆盖
";
    let dispositions = parse_dispositions(text).unwrap();
    assert_eq!(dispositions["fourier.lua"], Disposition::Baseline);

    let scripts = vec!["fourier.lua".to_string(), "sine_trace.lua".to_string()];
    let baselines: BTreeSet<String> = ["fourier".to_string()].into_iter().collect();
    let (covered, exempted) = check_dispositions(&dispositions, &scripts, &baselines).unwrap();
    assert_eq!((covered, exempted), (1, 1));

    // 声明了 baseline 但基线里没有 ⇒ 失败。
    let lie = parse_dispositions("sine_trace.lua = baseline\n").unwrap();
    assert!(check_dispositions(&lie, &scripts[1..], &baselines).is_err());

    // 没登记的脚本 ⇒ 失败（不许「无保护且不声明」）。
    assert!(check_dispositions(&BTreeMap::new(), &scripts[1..], &baselines).is_err());

    // 豁免必须带理由。
    assert!(parse_dispositions("sine_trace.lua = exempt:\n").is_err());
}
