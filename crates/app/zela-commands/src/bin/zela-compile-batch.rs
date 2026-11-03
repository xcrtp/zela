//! `zela-compile-batch`：一次进程编完一组脚本（门禁 demo 步骤与批量出片共用的壳）。
//!
//! 帧体在 [`zela_commands::commands::compile::run_many`]；本文件只做参数解析与退出码。
//!
//! ```text
//! zela-compile-batch --out-dir DIR --job id=script [--job ...]
//! ```
//!
//! # 输出（机器可读，一行一件，顺序与 `--job` 一致）
//!
//! ```text
//! [payload] {"id":"fourier","payload":"<64 hex>"}
//! ```
//!
//! 输出格式是 `xtask` 的解析契约：**不得改**（`--step demo` 逐行读取）。

use std::path::PathBuf;

use zela_commands::commands::compile::{self, CompileJob};

#[derive(Debug)]
struct Args {
    out_dir: PathBuf,
    jobs: Vec<CompileJob>,
}

impl Args {
    /// 纯解析入口（默认值快照的被测点）。
    fn parse_from(raw: &[String]) -> Result<Self, String> {
        if raw.is_empty() || raw.iter().any(|a| a == "--help" || a == "-h") {
            return Err(
                "用法：zela-compile-batch --out-dir DIR --job id=script [--job ...]\n    \
                 一件一行 `[payload] {\"id\":…,\"payload\":…}`，顺序与 --job 一致"
                    .to_string(),
            );
        }
        let mut out_dir: Option<PathBuf> = None;
        let mut jobs = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            match raw[i].as_str() {
                "--out-dir" => {
                    i += 1;
                    out_dir = Some(PathBuf::from(
                        raw.get(i).ok_or("--out-dir 需要一个目录参数")?,
                    ));
                }
                "--job" => {
                    i += 1;
                    jobs.push(parse_job(raw.get(i).ok_or("--job 需要 id=script")?)?);
                }
                other => return Err(format!("未知参数：{other}")),
            }
            i += 1;
        }
        Ok(Self {
            out_dir: out_dir.ok_or("缺少 --out-dir")?,
            // 空批量等于"什么都没做还返回成功"—— 清单解析出错时最容易走到这里。
            jobs: if jobs.is_empty() {
                return Err("--job 至少要给一件（空批量会什么都不做）".to_string());
            } else {
                jobs
            },
        })
    }
}

/// `fourier=<path>/fourier.lua`。
fn parse_job(spec: &str) -> Result<CompileJob, String> {
    let (id, script) = spec
        .split_once('=')
        .ok_or_else(|| format!("--job 需要 id=script：{spec}"))?;
    if id.is_empty() {
        return Err(format!("--job 的 id 不能为空：{spec}"));
    }
    if script.is_empty() {
        return Err(format!("--job 的脚本路径不能为空：{spec}"));
    }
    Ok(CompileJob {
        id: id.to_string(),
        script: PathBuf::from(script),
    })
}

fn main() {
    if let Err(message) = run() {
        eprintln!("zela-compile-batch: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = Args::parse_from(&raw)?;
    compile::run_many(&args.out_dir, &args.jobs)?;
    Ok(())
}

/// **默认值快照**：参数缺省与切分口径决定 demo 步骤的断言对象。
#[cfg(test)]
mod defaults_snapshot {
    use super::*;

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn jobs_are_split_on_first_equals_only() {
        let parsed = Args::parse_from(&args(&[
            "--out-dir",
            "tmp",
            "--job",
            "atlas=a=b/atlas.lua",
        ]))
        .expect("可解析");
        assert_eq!(parsed.out_dir, PathBuf::from("tmp"));
        assert_eq!(parsed.jobs.len(), 1);
        assert_eq!(parsed.jobs[0].id, "atlas");
        assert_eq!(parsed.jobs[0].script, PathBuf::from("a=b/atlas.lua"));
    }

    #[test]
    fn required_values_fail_explicitly() {
        assert!(Args::parse_from(&args(&["--job", "a=b"])).is_err(), "缺 --out-dir");
        assert!(Args::parse_from(&args(&["--out-dir", "t"])).is_err(), "缺 --job");
        assert!(parse_job("fourier").is_err(), "缺 =");
        assert!(parse_job("=x.lua").is_err(), "空 id");
        assert!(parse_job("a=").is_err(), "空脚本");
        assert!(Args::parse_from(&args(&["--help"])).is_err(), "--help 走用法分支");
    }
}
