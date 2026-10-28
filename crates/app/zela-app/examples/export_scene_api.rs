//! 导出 domain 能力的机器可读文档（JSON 真源 + LuaLS + TypeScript + Python `.pyi`）。
//!
//! ```text
//! cargo run -p zela-app --example export_scene_api
//! ```
//!
//! 产物默认写到 `target/scene_api/`（第一个命令行参数可改目录）：
//! - `scene_api.json` / `scene_api.lua`（`ze-pod-lua`）；
//! - `scene_api.d.ts`（`ze-pod-js`，`js` feature）；
//! - `zela/__init__.pyi` / `zela/scene.pyi` / `zela/content.pyi` / `zela/python.pyi`
//!   （`ze-pod-py`，路径与 capability 命名空间一致）。
//!
//! `cargo verify --step docs` 连续生成两次到 `target/declarations/`，断言输出
//! 逐字节一致 —— 声明文件是**生成物**，不进仓库（提案 #6 收口）。

use std::path::PathBuf;

use ze_pod_binding::CapabilityDocs;
use ze_pod_core::Capability;
use ze_pod_func::MathApi;
use zela_scene_ecs::pod::{ContentApi, SceneApi};

fn main() {
    let dir = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..")
            .join("target")
            .join("scene_api")
    });

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

    let [json, lua] = ze_pod_lua::export_docs(&dir, &capabilities).expect("写导出文件");
    println!("JSON  : {}", json.display());
    println!("LuaLS : {}", lua.display());

    #[cfg(feature = "js")]
    {
        let paths = ze_pod_js::export_docs(&dir, &capabilities).expect("写 TypeScript 声明");
        for path in paths {
            println!("DTS   : {}", path.display());
        }
    }
    // `.pyi` 生成器是纯 Rust（不启用 pyo3）⇒ 文档基线在所有机器上都能生成。
    let paths = ze_pod_py::export_docs(&dir, &capabilities).expect("写 Python 类型存根");
    for path in paths {
        println!("PYI   : {}", path.display());
    }

    println!(
        "函数数: scene {} / content {}",
        SceneApi::signatures().len(),
        ContentApi::signatures().len()
    );
}
