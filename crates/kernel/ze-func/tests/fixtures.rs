//! **独立 fixture harness**（阶段 0）。
//!
//! fixture = `(表达式, 输入序列, 期望值序列哈希)`，JSON 文件、可跨语言复用。
//! 每个 fixture 都同时跑：
//! 1. lowered 求值器（`CompiledExpr::eval`）；
//! 2. 参考求值器（tree-walking）。
//!
//! 两者逐位一致 + 值序列哈希等于 `expect_hash` 才算通过。这样"化简改了结果"
//! 会在这里直接变成红色，而不是等到重录 36 条像素基线才发现。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use ze_comb::{ChannelRef, CompiledExpr, EvalError, Expr, Mono, OpRef, Value, eval_reference, hash_values};

#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    expr: Expr,
    /// 通道名 → 每个采样点的标量值（`time` / `frame` / 任意连续变量）。
    #[serde(default)]
    channels: BTreeMap<String, Vec<f64>>,
    /// 对象打包值（十进制字符串）→ 地址 → 标量（`Node::Read` 用）。
    #[serde(default)]
    objects: BTreeMap<String, BTreeMap<String, f64>>,
    /// 期望值（可读性用；与哈希一起校验）。
    expect: Vec<Expected>,
    /// 期望值序列哈希（FNV-1a 64，十六进制字符串，跨语言可复用）。
    expect_hash: String,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(untagged)]
enum Expected {
    Number(f64),
    Bool(bool),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Text(String),
}

impl Expected {
    fn to_value(&self) -> Value {
        match self {
            Expected::Number(value) => Value::f64(*value),
            Expected::Bool(value) => Value::Bool(*value),
            Expected::Vec2(value) => Value::vec2(*value),
            Expected::Vec3(value) => Value::vec3(*value),
            Expected::Text(value) => Value::text(value.clone()),
        }
    }
}

/// 按采样点从固定输入序列取值的宿主单子。
struct FixtureMono<'a> {
    sample: usize,
    channels: &'a BTreeMap<String, Vec<f64>>,
    objects: &'a BTreeMap<String, BTreeMap<String, f64>>,
}

impl Mono for FixtureMono<'_> {
    fn read(&self, channel: &ChannelRef) -> Result<Value, EvalError> {
        self.channels
            .get(&channel.name)
            .and_then(|series| series.get(self.sample))
            .map(|value| Value::f64(*value))
            .ok_or_else(|| EvalError::MissingChannel {
                channel: channel.clone(),
            })
    }

    fn read_object(&self, channel: &ChannelRef, object: u64) -> Result<Value, EvalError> {
        self.objects
            .get(&object.to_string())
            .and_then(|fields| fields.get(&channel.name))
            .map(|value| Value::f64(*value))
            .ok_or_else(|| EvalError::MissingComponent {
                object,
                channel: channel.clone(),
            })
    }

    fn call(&self, op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
        ze_func::eval(op, args)
    }
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn parse_hash(text: &str) -> u64 {
    let text = text.trim();
    let digits = text.strip_prefix("0x").unwrap_or(text);
    u64::from_str_radix(digits, 16).unwrap_or_else(|_| panic!("非法哈希：{text}"))
}

#[test]
fn fixtures_match_both_evaluators_and_the_expected_hash() {
    let dir = fixture_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("fixture 目录 {} 不可读：{error}", dir.display()))
        .map(|entry| entry.expect("目录项可读").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "fixture 目录不能为空");

    for path in files {
        let raw = fs::read_to_string(&path).expect("fixture 可读");
        let fixture: Fixture =
            serde_json::from_str(&raw).unwrap_or_else(|error| panic!("{} 解析失败：{error}", path.display()));

        let samples = fixture
            .channels
            .values()
            .map(Vec::len)
            .chain(std::iter::once(fixture.expect.len()))
            .max()
            .unwrap_or(0);
        for series in fixture.channels.values() {
            assert_eq!(
                series.len(),
                samples,
                "fixture `{}` 的通道长度必须与采样点数一致",
                fixture.name
            );
        }

        fixture.expr.validate().unwrap_or_else(|error| {
            panic!("fixture `{}` 的表达式非法：{error}", fixture.name)
        });
        let program = CompiledExpr::compile(&fixture.expr)
            .unwrap_or_else(|error| panic!("fixture `{}` 无法 lowering：{error}", fixture.name));

        let mut values = Vec::with_capacity(samples);
        for sample in 0..samples {
            let mono = FixtureMono {
                sample,
                channels: &fixture.channels,
                objects: &fixture.objects,
            };
            let lowered = program
                .eval(&mono)
                .unwrap_or_else(|error| panic!("fixture `{}` 第 {sample} 帧 lowered 失败：{error}", fixture.name));
            let reference = eval_reference(&fixture.expr, &mono).unwrap_or_else(|error| {
                panic!("fixture `{}` 第 {sample} 帧参考求值失败：{error}", fixture.name)
            });
            assert_eq!(
                lowered, reference,
                "fixture `{}` 第 {sample} 帧两个求值器不一致",
                fixture.name
            );
            assert_eq!(
                lowered,
                fixture.expect[sample].to_value(),
                "fixture `{}` 第 {sample} 帧值与 expect 不符",
                fixture.name
            );
            values.push(lowered);
        }

        let actual = hash_values(&values);
        let expected = parse_hash(&fixture.expect_hash);
        assert_eq!(
            actual, expected,
            "fixture `{}` 的值序列哈希不符：实际 0x{actual:016x}，期望 0x{expected:016x}",
            fixture.name
        );
    }
}
