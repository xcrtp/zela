//! 内嵌 Python 的**前置探测**（`feature = "python"`）。
//!
//! # 为什么必须提前
//!
//! `pyo3` 的 `auto-initialize` 走的是 CPython 的 `Py_InitializeEx`。Windows 上它把
//! **本进程的 exe** 当作 `sys.executable` 去推算前缀，于是找不到 `Lib/encodings`：
//!
//! ```text
//! Could not find platform independent libraries <prefix>
//! Fatal Python error: Failed to import encodings module
//! ```
//!
//! 这是 CPython 的 **Fatal**（`abort()`），Rust 侧既捕获不到、也没机会提示 ——
//! `cargo run -p zela-app` 会整个进程死掉（实测，2026-10-25）。所以在**第一次碰 pyo3
//! 之前**先做两件事：
//!
//! 1. 找一个真能 `import encodings` 的解释器，**首选 pyo3 编译期选中的那个**
//!    （`ze_pod_py::BUILD_PYTHON`，由 `build.rs` 从 pyo3 的 `links` 配置取出）；
//! 2. 若 `PYTHONHOME` 未设置，把它指到该解释器的 `sys.prefix`（用户 / CI 显式设置的不覆盖）。
//!
//! 一个都找不到 ⇒ 返回**可操作**的提示：调用方据此跳过 Python Pod（提示 + 其余 Pod 照常），
//! 而不是让 CPython 把进程打死。
//!
//! # 版本必须对齐
//!
//! 只"能 import encodings"不够：`PYTHONHOME` 指向 **另一个 CPython 版本**的 stdlib 时，
//! 内嵌初始化照样 Fatal（实测：本机 PATH 里 msys64 的 3.14 排在 anaconda 的 3.13 之前，
//! 前缀指过去就直接崩）。所以候选解释器的 `X.Y` 必须与 pyo3 编译期链接的版本一致；
//! 不一致的候选只进提示，不设 `PYTHONHOME`。
//!
//! # 边界
//!
//! 探测只保证"有一个与 pyo3 同版本的 Python 3"；若用户显式设了 `PYTHONHOME`，本模块
//! 不校验（那是他们的显式选择）。

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

static PROBE: OnceLock<Result<(), String>> = OnceLock::new();

/// 第一次碰 pyo3 **之前**调用；结果进程内缓存（探测要起子进程，只付一次）。
pub fn ensure() -> Result<(), String> {
    PROBE.get_or_init(probe).clone()
}

fn probe() -> Result<(), String> {
    // 用户 / CI 已显式配置 ⇒ 不覆盖（他们通常连同 PYO3_PYTHON 一起对齐了版本）。
    if std::env::var_os("PYTHONHOME").is_some() {
        return Ok(());
    }
    let expected = ze_pod_py::BUILD_PYTHON_VERSION;
    let mut failed = Vec::new();
    let mut mismatched = Vec::new();
    for (program, prefix_args) in candidates() {
        match python_home(&program, &prefix_args) {
            Some((home, version)) => match expected {
                Some(expected) if version != expected => {
                    mismatched.push(format!("{program} = {version}（本程序内嵌 {expected}）"));
                }
                _ => {
                    eprintln!(
                        "[zela-app] Python 前缀 = {}（来自 {program}）",
                        home.display()
                    );
                    // SAFETY: 装配期（`assembly::assemble` 的第一件事）、进程尚未起线程、
                    // 且在任何 pyo3 调用之前 —— 不存在并发读写环境变量的情形。
                    unsafe { std::env::set_var("PYTHONHOME", &home) };
                    return Ok(());
                }
            },
            None => failed.push(format!("{program} {}", prefix_args.join(" ")).trim().to_string()),
        }
    }

    let mut message = format!("找不到可用的 Python 解释器（试过：{}）", failed.join(" / "));
    if !mismatched.is_empty() {
        message.push_str(&format!(
            "\n  ⇒ 版本不匹配、不能用来推 PYTHONHOME：{}",
            mismatched.join("；")
        ));
    }
    message.push_str(
        "\n  ⇒ 装一个对应版本的 Python 3；或设置 `PYO3_PYTHON` 指向解释器、`PYTHONHOME` 指向它的前缀；\n  \
         ⇒ 或者关掉 Python Pod：`cargo run -p zela-app --no-default-features --features typst`",
    );
    Err(message)
}

fn candidates() -> Vec<(String, Vec<&'static str>)> {
    let mut out: Vec<(String, Vec<&'static str>)> = Vec::new();
    // 1. pyo3 **编译期**选中的解释器：唯一能保证与链接进来的 `pythonXY.dll` 同版本。
    if let Some(path) = ze_pod_py::BUILD_PYTHON {
        out.push((path.to_string(), Vec::new()));
    }
    // 2. 用户显式指定。
    if let Some(path) = std::env::var_os("PYO3_PYTHON") {
        out.push((path.to_string_lossy().into_owned(), Vec::new()));
    }
    // 3. 常规命令。
    out.push(("python".to_string(), Vec::new()));
    out.push(("python3".to_string(), Vec::new()));
    out.push(("py".to_string(), vec!["-3"]));
    // 4. Windows 上 `python` 常被 **Microsoft Store 的占位符**截胡（实测本机
    //    `C:\…\WindowsApps\python.exe` 且真正的解释器在 PATH 更后面）
    //    ⇒ 再按 PATH 顺序逐个试目录里的 `python.exe` / `python3.exe`。
    for dir in std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default()
    {
        for name in ["python.exe", "python3.exe"] {
            let exe = dir.join(name);
            if exe.is_file() {
                out.push((exe.to_string_lossy().into_owned(), Vec::new()));
            }
        }
    }
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// `python -c "import sys, encodings; …"` ⇒ `(sys.prefix, "X.Y")`。
///
/// 判据是 `import encodings` 成功 —— 那正是内嵌解释器崩掉的那一步；`sys.prefix` 则是
/// `PYTHONHOME` 需要的值，`X.Y` 用来与编译期版本对齐。
fn python_home(program: &str, prefix_args: &[&str]) -> Option<(PathBuf, String)> {
    let output = Command::new(program)
        .args(prefix_args)
        .arg("-c")
        .arg("import sys, encodings; print(sys.prefix); print('%d.%d' % tuple(sys.version_info[:2]))")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines().map(str::trim).filter(|line| !line.is_empty());
    let home = PathBuf::from(lines.next()?);
    let version = lines.next()?.to_string();
    home.is_dir().then_some((home, version))
}
