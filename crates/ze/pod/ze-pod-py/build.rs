//! 把 pyo3 **编译期**选中的解释器与版本传给运行时。
//!
//! # 为什么需要
//!
//! pyo3 的 `auto-initialize` 在 Windows 上按**本进程 exe** 推算前缀，于是找不到
//! `Lib/encodings`：CPython 直接 `Fatal Python error`（`abort()`），Rust 侧既捕获
//! 不到也没机会提示。要给出正确的 `PYTHONHOME`，运行时必须先知道"链接进来的那个
//! 解释器是谁"，而这个信息**只在构建期存在**：
//!
//! pyo3 通过 `links = "pyo3-python"` 把 `InterpreterConfig` 以
//! `DEP_PYO3_PYTHON_PYO3_CONFIG`（十六进制编码的 `键=值` 行）交给下游 build script。
//!
//! 消费方：`zela-app/src/python_env.rs`（装配前探测）。

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DEP_PYO3_PYTHON_PYO3_CONFIG");
    let Some(config) = interpreter_config() else {
        return;
    };
    for (key, value) in config {
        match key.as_str() {
            // `sys.executable`：链接时用的那个解释器。
            "executable" => println!("cargo:rustc-env=ZELA_PYO3_EXECUTABLE={value}"),
            // `X.Y`：用来滤掉本机**另一个版本**的解释器（否则前缀会指错）。
            "version" => println!("cargo:rustc-env=ZELA_PYO3_VERSION={value}"),
            _ => {}
        }
    }
}

fn interpreter_config() -> Option<Vec<(String, String)>> {
    let raw = std::env::var("DEP_PYO3_PYTHON_PYO3_CONFIG").ok()?;
    let text = String::from_utf8(unhex(&raw)?).ok()?;
    Some(
        text.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
            .collect(),
    )
}

/// pyo3 的 `escape`：每个字节写成两个小写十六进制字符。
fn unhex(input: &str) -> Option<Vec<u8>> {
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let bytes = input.trim().as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    bytes
        .chunks_exact(2)
        .map(|pair| Some(nibble(pair[0])? << 4 | nibble(pair[1])?))
        .collect()
}
