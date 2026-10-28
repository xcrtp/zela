//! xtask：本机验证入口（`cargo verify`）。
//!
//! ```text
//! cargo verify               # full：check + test + wasm + structure + docs + coverage + demo + smoke + hashes
//! cargo verify --fast        # 开发循环：check + test + 短对拍（smoke）
//! cargo verify --step <name> # 单步：check | test | wasm | structure | docs | coverage | demo | smoke | hashes
//! cargo verify --record --reason <text>
//!                            # 录制设备重录基线（见 lib.rs：两槽窗口 / 强制 reason）
//! ```
//!
//! 设计约束（见「本机验证入口 xtask 执行计划」）：
//! - 零第三方依赖（不引 clap / toml），保证 xtask 自身编译快；
//! - 本机与 CI 的差异必须**显式打印**：pyo3 不可用时降级、jq 缺失时跳过；
//! - `baselines.txt` 的 `[native]` 段自动比对，`[browser]` 段**仅留档**；
//! - 门禁级逻辑（解析器 / 选项 / 重录改写）在 `lib.rs`，`tests/` 配用例。
//!
//! 本 crate 是**独立 workspace**（`xtask/Cargo.toml` 中的空 `[workspace]`），
//! 因此不会被根 workspace 的 `--workspace` 或 wasm gate 收进去。

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use xtask::{
    Baselines, Options, Step, apply_record, assert_attributed, load_baselines, rebind_dirty,
};

/// wasm gate 的 kernel 组（与 CI 名单逐字一致）。
const KERNEL_CRATES: &[&str] = &[
    "ze-type",
    "ze-reflect",
    "ze-graph-core",
    "ze-notion",
    "ze-render-ir",
    "ze-scene-contract",
    "ze-content",
    "ze-time",
    "ze-comb",
    "ze-func",
    "ze-vertex",
    "ze-resource",
    "ze-macro",
    "ze-prerender",
    "ze-pod-core",
    "ze-pod-binding",
    "ze-pod-func",
];

/// 门面 crate：必须与核心组一样保持 wasm32 可编译（不含 lua / py / UI）。
const FACADE_CRATES: &[&str] = &["ze-pod", "ze-node", "zela"];

/// 可选 wasm 能力**组**：语言适配器里**能**编到 wasm32 的那些。
///
/// lua / py 不需要 wasm（mlua 的解释器不适合 wasm 内核路径；pyo3 根本编不到），
/// 而 `ze-pod-js` 的存在理由正是"native 与 wasm 跑同一个 JS 引擎"（B3-P §一）——
/// 不给它门禁，那条理由就没有回归保护（A1）。
///
/// ⚠️ **native-only（不进任何 wasm 名单）**：`zela-render-vector` 依赖 `pollster` + wgpu
/// 的同步构造，编不到 wasm32；把它加进上面的 kernel / facade 名单会得到一条难懂的
/// 编译错误。它的正确性由 native 侧的 `zela-render-backends` 对拍覆盖（B4）。
const OPTIONAL_WASM_CRATES: &[&str] = &["ze-pod-js"];

/// 结构性判据 1：`crates/ze/pod/` 的生产依赖边不得出现任何 `zela-*`。
const JQ_POD_LAYER: &str = r#"
.packages[]
| select(.name | IN("ze-pod-core", "ze-pod-binding", "ze-pod-macro",
                    "ze-pod-lua", "ze-pod-py", "ze-pod-js", "ze-pod-tests", "ze-pod",
                    "ze-pod-func"))
| .name as $n
| .dependencies[]
| select(.kind != "dev")
| select(.name | startswith("zela-"))
| "\($n) -> \(.name)"
"#;

/// 结构性判据 2：`zela-render-backends` 的生产依赖边不得出现场景实现（S2）。
const JQ_BACKENDS: &str = r#"
.packages[]
| select(.name == "zela-render-backends")
| .name as $n
| .dependencies[]
| select(.kind != "dev")
| select(.name == "zela-scene-ecs")
| "\($n) -> \(.name)"
"#;

/// 结构性判据 2b（B3-1）：场景核心层 / Pod 能力层不得依赖 `zela-*` 或 hecs。
///
/// 光靠人记住"core 不能依赖实现"不行 —— 让 CI 守住（约定式不变量的教训）。
const JQ_SCENE_LAYERS: &str = r#"
.packages[]
| select(.name | IN("ze-scene-core", "ze-scene-api"))
| .name as $n
| .dependencies[]
| select(.kind != "dev")
| select((.name | startswith("zela-")) or .name == "hecs" or .name == "zela-schedule")
| "\($n) -> \(.name)"
"#;

/// 结构性判据 2c（B3 收尾）：`zela-render` 的生产依赖边不得出现场景 **ECS 实现**。
///
/// B3-1 收尾已把这条边断开（渲染层改依赖 `ze-scene-hecs`）；没有门禁时，
/// 后来者加一行 `zela-scene-ecs = {...}` 就能让分层悄悄回退。
/// ⚠️ `ze-scene-hecs` / `zela-schedule` 是**设计意图**，不在禁止之列。
const JQ_RENDER_LAYER: &str = r#"
.packages[]
| select(.name == "zela-render")
| .name as $n
| .dependencies[]
| select(.kind != "dev")
| select(.name == "zela-scene-ecs")
| "\($n) -> \(.name)"
"#;

/// 结构性判据 3：数学入口唯一（M0）—— 除豁免组外不得直接依赖 glam。
const JQ_GLAM: &str = r#"
.packages[]
| select(.name | IN("ze-notion",
                    "zela-node-editor",
                    "zela-node-egui",
                    "zela-node-egui-ui") | not)
| .name as $n
| .dependencies[]
| select(.kind != "dev")
| select(.name == "glam")
| "\($n) -> glam"
"#;

/// pyo3（Python Pod）在本机是否可用。
///
/// 检测顺序与 pyo3 自身一致：`PYO3_PYTHON` → `python3` → `python` → `py -3`。
/// 找不到可用解释器时，`cargo check/test --workspace` 无法构建 pyo3，
/// 因此降级到 `--no-default-features --features typst`。
struct FeaturePlan {
    interpreter: Option<String>,
}

impl FeaturePlan {
    fn detect() -> Self {
        if let Some(path) = env::var_os("PYO3_PYTHON") {
            let path = path.to_string_lossy().into_owned();
            if probe_python(&path, &[]) {
                return Self {
                    interpreter: Some(format!("PYO3_PYTHON={path}")),
                };
            }
            println!("[xtask] PYO3_PYTHON={path} 不可用，继续探测 PATH");
        }
        for (program, prefix) in [
            ("python3", &[][..]),
            ("python", &[][..]),
            ("py", &["-3"][..]),
        ] {
            if probe_python(program, prefix) {
                let label = if prefix.is_empty() {
                    program.to_string()
                } else {
                    format!("{program} {}", prefix.join(" "))
                };
                return Self {
                    interpreter: Some(label),
                };
            }
        }
        Self { interpreter: None }
    }

    fn pyo3_available(&self) -> bool {
        self.interpreter.is_some()
    }

    /// 本机与 CI 的差异：必须打印，不得静默。
    fn announce(&self) {
        match &self.interpreter {
            Some(label) => println!(
                "[xtask] Python 解释器可用（{label}）⇒ 使用默认 features（与 CI 一致）"
            ),
            None => {
                println!(
                    "[xtask] 检测到默认 features 不可用（pyo3），降级为 --no-default-features --features typst"
                );
                println!(
                    "[xtask] 注意：zela-gui 的依赖边强制 zela-app 默认 features（含 pyo3），本机将跳过该 crate"
                );
                println!(
                    "[xtask] 如需与 CI 完全一致（覆盖 zela-gui）：设置 PYO3_PYTHON（必要时连同 PYTHONHOME）指向 Python 3 解释器"
                );
            }
        }
    }

    /// `-p zela-app` / `-p zela-compile` 使用的 feature 参数。
    fn app_args(&self) -> Vec<String> {
        if self.pyo3_available() {
            Vec::new()
        } else {
            strings(&["--no-default-features", "--features", "typst"])
        }
    }
}

fn probe_python(program: &str, prefix: &[&str]) -> bool {
    let output = Command::new(program)
        .args(prefix)
        .args(["-c", "import sys; print(sys.version_info[0])"])
        .output();
    matches!(output, Ok(out) if out.status.success()
        && String::from_utf8_lossy(&out.stdout).trim() == "3")
}

fn main() {
    let raw: Vec<String> = env::args().skip(1).collect();
    let options = match Options::parse(&raw) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    if let Err(message) = run(&options) {
        eprintln!("[xtask] 失败：{message}");
        std::process::exit(1);
    }
}

fn run(options: &Options) -> Result<(), String> {
    let root = repo_root()?;
    if options.rebind {
        return run_rebind(&root);
    }
    if options.record {
        return run_record(&root, options);
    }
    let plan = FeaturePlan::detect();

    println!("[xtask] 仓库根：{}", root.display());
    plan.announce();
    if options.skip_baseline {
        println!(
            "[xtask] --skip-baseline：对拍只断言 --script == --bundle，不比对基线（用于无匹配 GPU 的 CI）"
        );
    }

    let steps = options.steps();
    println!(
        "[xtask] 步骤：{}",
        steps
            .iter()
            .map(|step| step.name())
            .collect::<Vec<_>>()
            .join(" → ")
    );

    for step in steps {
        println!("\n[xtask] ===== {} =====", step.name());
        match step {
            Step::Check => step_check(&root, &plan)?,
            Step::Test => step_test(&root, &plan)?,
            Step::Wasm => step_wasm(&root)?,
            Step::Structure => step_structure(&root)?,
            Step::Docs => step_docs(&root)?,
            Step::Coverage => step_coverage(&root)?,
            Step::Demo => step_demo(&root)?,
            Step::Smoke => step_smoke(&root, options.skip_baseline)?,
            Step::Hashes => step_hashes(&root, options.skip_baseline, options.independent)?,
            Step::Fixtures => step_fixtures(&root)?,
        }
    }

    println!("\n[xtask] 全部通过");
    Ok(())
}

fn repo_root() -> Result<PathBuf, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("无法从 {} 推断仓库根", manifest.display()))
}

// ─── step: record（基线重录，仅录制设备） ────────────────────────────────────

/// `--record --reason <text>`：重录 `baselines.txt`（基线策略 §九）。
///
/// 流程：
/// 1. 设备门禁：`ZELA_RECORD_DEVICE` 未设置 ⇒ 显式报错，不静默；
/// 2. 硬门禁：smoke + hashes 全量对拍，`--script == --bundle` 必须绿；
/// 3. 新值的 commit 写**产生时 HEAD**（口径 B），轮换 slotA/slotB（纯逻辑在
///    `xtask::apply_record`，有 tests 覆盖）；旧版 `pending` 行按最后已知有效提交兜底；
/// 4. 写入后重新解析自校验。
fn run_record(root: &Path, options: &Options) -> Result<(), String> {
    let device = env::var("ZELA_RECORD_DEVICE").unwrap_or_default();
    if device.trim().is_empty() {
        return Err(format!(
            "--record 只允许在录制设备执行（基线策略 §四.1：沙盒 / CI / lavapipe / 其他 GPU 一律不得写入基线）\n  \
             本机未设置 ZELA_RECORD_DEVICE ⇒ 拒绝写入，不静默。\n  \
             若确认本机是录制设备（Windows / RTX 4060 Laptop），设置后重跑：\n  \
             \x20 $env:ZELA_RECORD_DEVICE = \"windows / RTX 4060 Laptop\""
        ));
    }
    let reason = options.reason.as_deref().unwrap_or_default();

    let baseline_path = scripts_dir(root).join("baselines.txt");
    prepare_hashes_dir(root)?;
    announce_baselines(&baseline_path);
    println!("[xtask] --record：检测到 ZELA_RECORD_DEVICE={device}，仅录制设备可执行");

    // 1) 硬门禁：同一跑次 script == bundle 必须绿（smoke + hashes 全量）。
    // B19 Step 3：与 `step_smoke` 同口径 —— smoke 钉 wgpu，否则重录会把矢量默认写进 `[smoke]`。
    let smoke_hash =
        compare_script_vs_bundle(root, "smoke", "320x240", None, Some("wgpu"))?;
    // B9 Step 3：完整对拍走批量模式（`--independent` 可切回独立进程做对照）。
    let hashes: std::collections::BTreeMap<String, String> = if options.independent {
        println!("[xtask] 对拍模式：独立进程（--independent 诊断路径）");
        let mut map = std::collections::BTreeMap::new();
        for (work, backend) in HASHES_WORKS {
            // 标签而不是作品名：同一作品两个后端各要一条（B13 Step 3）。
            map.insert(
                work_label(work, *backend),
                compare_script_vs_bundle(root, work, "640x480", None, *backend)?,
            );
        }
        map
    } else {
        println!("[xtask] 对拍模式：批量（一次 GPU 初始化）");
        batch_hashes(root)?.into_iter().collect()
    };
    let recorded = |work: &str| -> Result<String, String> {
        hashes
            .get(work)
            .cloned()
            .ok_or_else(|| format!("对拍结果缺少 `{work}`（批量模式未产出该作品）"))
    };

    // 2) 回填 commit：当前 HEAD（重录发生在提交前）。
    let head = git_short_head(root)?;
    // 口径 D：重录前工作树必然是脏的（改代码 → 录值 → 一起提交）。
    // 所以 `head` 并不包含这份状态 —— 标 `+dirty`，提交后用 `--rebind` 归因。
    let dirty = worktree_dirty(root)?;
    if dirty {
        println!(
            "[xtask] --record：工作树有未提交改动 ⇒ 值标注为 `{head}+dirty`；\
             \n         录完必须：① 把 baselines.txt 与代码**一起提交**，② 立刻跑 `cargo verify --rebind`。\
             \n         未归因的 `+dirty` 会被 smoke / hashes 步骤拒绝。"
        );
    } else {
        println!("[xtask] --record：工作树干净 ⇒ 值直接归因到 {head}");
    }

    // 3) 改写文件（纯函数，逻辑在 lib.rs，tests/ 覆盖）。
    let text = fs::read_to_string(&baseline_path)
        .map_err(|error| format!("读取 {} 失败：{error}", baseline_path.display()))?;
    // 录制清单**派生自** `HASHES_WORKS`：新增作品只需改那张表，不再漏录
    // （B7 Step 2：手写清单曾让 7 个已纳管作品算完就被丢掉）。
    let mut records: Vec<(String, String, String)> =
        vec![("smoke".to_string(), "smoke".to_string(), smoke_hash)];
    for (work, backend) in HASHES_WORKS {
        let label = work_label(work, *backend);
        records.push((
            baseline_section(*backend).to_string(),
            (*work).to_string(),
            recorded(&label)?,
        ));
    }
    let next = apply_record(&text, &device, reason, &head, dirty, &records)?;
    fs::write(&baseline_path, next)
        .map_err(|error| format!("写入 {} 失败：{error}", baseline_path.display()))?;

    // 4) 自校验：重录后的文件必须能重新解析。
    let baselines = load_baselines(&baseline_path)?;
    for (key, hash) in &baselines.native {
        println!("[xtask] [native] {key} = {hash}");
    }
    for (key, hash) in &baselines.smoke {
        println!("[xtask] [smoke] {key} = {hash}");
    }
    for (key, hash) in &baselines.vector {
        println!("[xtask] [vector] {key} = {hash}");
    }
    println!(
        "[xtask] 已重录 {}：reason 已写入{}",
        baseline_path.display(),
        if dirty {
            "；下一步 = 提交（含 baselines.txt）→ `cargo verify --rebind`"
        } else {
            ""
        }
    );
    Ok(())
}

/// `--rebind`：把 `@ <sha>+dirty` 归因到当前 HEAD（提交之后立刻跑）。
///
/// 前置：工作树干净且 baselines.txt 无未提交改动 —— 否则"当前 HEAD"还不是
/// 携带这份录制的提交，归因会再次失真（P1-4 的成因）。
fn run_rebind(root: &Path) -> Result<(), String> {
    if worktree_dirty(root)? {
        return Err(
            "--rebind 要求干净工作树：还有未提交改动时，当前 HEAD 不含这份录制，归因会失真。\n  \
             先 `git add` + `git commit`（把 baselines.txt 一起提交），再重跑 `cargo verify --rebind`。"
                .to_string(),
        );
    }
    let baseline_path = scripts_dir(root).join("baselines.txt");
    let text = fs::read_to_string(&baseline_path)
        .map_err(|error| format!("读取 {} 失败：{error}", baseline_path.display()))?;
    let head = git_short_head(root)?;
    let (next, rewritten) = rebind_dirty(&text, &head)?;
    fs::write(&baseline_path, next)
        .map_err(|error| format!("写入 {} 失败：{error}", baseline_path.display()))?;
    println!("[xtask] --rebind：{rewritten} 行 `+dirty` 已归因到 {head}");
    // 自校验：归因后不得再有未归因的录制。
    let text = fs::read_to_string(&baseline_path)
        .map_err(|error| format!("读取 {} 失败：{error}", baseline_path.display()))?;
    assert_attributed(&text)?;
    load_baselines(&baseline_path)?;
    println!("[xtask] --rebind 完成：baselines.txt 归因一致");
    Ok(())
}

/// 工作树是否有未提交改动（含未跟踪文件；`git status --porcelain` 非空）。
fn worktree_dirty(root: &Path) -> Result<bool, String> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("无法运行 git：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git status --porcelain 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn git_short_head(root: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("无法运行 git：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git rev-parse --short HEAD 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if head.is_empty() {
        return Err("git rev-parse --short HEAD 输出为空".to_string());
    }
    Ok(head)
}

// ─── step: docs（声明文件生成门禁）───────────────────────────────────────

/// **提案 #6 收口**：声明文件是**生成物**，不进仓库（用户决定）。
///
/// 门禁不再比对仓库内基线，改为「生成器可用 + 输出确定」：
///
/// 1. 连续生成到两个目录（`target/declarations/a`、`b`，属构建产物区）；
/// 2. 两次输出**逐字节相同** —— 生成器若把时间戳 / 哈希表顺序写进文件，
///    这里就会红（同一实现、同一输入必须得到同一文档）；
/// 3. 文件集合非空、每个文件非空（避免"生成成功但什么也没写"）。
///
/// 为什么去掉基线：基线本身也是生成物，留在仓库里等于让构建产物参与版本控制；
/// 而声明文件由 capability **自动派生**，不存在"手写声明忘更新"的腐烂路径。

/// 删除目录：Windows 上 `remove_dir_all` 偶发 `ERROR_DIR_NOT_EMPTY`
/// （杀毒 / 索引 / 上一轮宿主进程短暂持有句柄）。逐层清只读属性 + 有限次重试，
/// 最终仍失败就显式返回错误（不静默当作已清理）。
fn remove_dir_all_robust(path: &Path) -> std::io::Result<()> {
    let mut last = None;
    for attempt in 0..10u32 {
        match remove_tree(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                last = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(50 * u64::from(attempt + 1)));
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("清理目录失败")))
}

fn remove_tree(path: &Path) -> std::io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            remove_tree(&entry?.path())?;
        }
        match fs::remove_dir(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn step_docs(root: &Path) -> Result<(), String> {
    let base = root.join("target").join("declarations");
    if base.exists() {
        remove_dir_all_robust(&base)
            .map_err(|error| format!("清理 {} 失败：{error}", base.display()))?;
    }
    let first = base.join("a");
    let second = base.join("b");
    for dir in [&first, &second] {
        fs::create_dir_all(dir)
            .map_err(|error| format!("创建 {} 失败：{error}", dir.display()))?;
        export_scene_api(root, dir)?;
    }

    let expected = collect_files(&first)?;
    let actual = collect_files(&second)?;
    if expected.is_empty() {
        return Err("声明文件生成为空（生成器没有写出任何文件）".to_string());
    }
    let mut problems = Vec::new();
    for (path, bytes) in &expected {
        match actual.get(path) {
            None => problems.push(format!("第二次生成缺少：{path}")),
            Some(other) if other != bytes => problems.push(format!("两次生成不一致：{path}")),
            Some(_) => {}
        }
    }
    for path in actual.keys() {
        if !expected.contains_key(path) {
            problems.push(format!("第二次生成多出：{path}"));
        }
    }
    for (path, bytes) in &expected {
        if bytes.is_empty() {
            problems.push(format!("空文件：{path}"));
        }
    }
    if !problems.is_empty() {
        return Err(format!(
            "声明文件生成不确定（{} 项）：\n  {}",
            problems.len(),
            problems.join("\n  ")
        ));
    }
    println!(
        "[xtask] 声明文件：{} 个文件，两次生成逐字节一致（生成物，未入库）",
        expected.len()
    );
    Ok(())
}

/// 跑一次 `export_scene_api`（输出到给定目录）。
fn export_scene_api(root: &Path, out: &Path) -> Result<(), String> {
    let args = [
        strings(&[
            "run",
            "-q",
            "--profile",
            "verify",
            "-p",
            "zela-app",
            "--example",
            "export_scene_api",
            "--no-default-features",
            "--features",
            "typst,js",
            "--",
        ]),
        vec![out.to_string_lossy().into_owned()],
    ]
    .concat();
    cargo(root, &args)
}

/// 递归收集目录下的文件（相对路径用 `/` 分隔，值 = 原始字节）。
fn collect_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> Result<(), String> {
        let entries =
            fs::read_dir(dir).map_err(|error| format!("读取 {} 失败：{error}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("遍历 {} 失败：{error}", dir.display()))?;
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out)?;
                continue;
            }
            let relative = path
                .strip_prefix(base)
                .map_err(|error| format!("相对路径失败：{error}"))?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes =
                fs::read(&path).map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
            out.insert(relative, bytes);
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out)?;
    Ok(out)
}

// ─── step: check ────────────────────────────────────────────────────────────

fn step_check(root: &Path, plan: &FeaturePlan) -> Result<(), String> {
    if plan.pyo3_available() {
        return cargo(
            root,
            &strings(&["check", "--workspace", "--all-targets"]),
        );
    }

    // pyo3 不可用：`--no-default-features` 只影响“被选中的包”，
    // 而 zela-gui 对 zela-app 的依赖边会重新打开 zela-app 的默认 features
    // （含 pyo3）。因此先把不依赖 zela-app 的 crate 用默认 features 检查，
    // 再单独检查两个壳（zela-gui 只能跳过并打印）。
    cargo(
        root,
        &strings(&[
            "check",
            "--workspace",
            "--all-targets",
            "--exclude",
            "zela-app",
            "--exclude",
            "zela-gui",
            "--exclude",
            "zela-compile",
        ]),
    )?;
    cargo(
        root,
        &[
            strings(&["check", "-p", "zela-app", "--all-targets"]),
            plan.app_args(),
        ]
        .concat(),
    )?;
    cargo(
        root,
        &[
            strings(&["check", "-p", "zela-compile", "--all-targets"]),
            plan.app_args(),
        ]
        .concat(),
    )?;
    println!("[xtask] 跳过 zela-gui：无 Python 时其依赖边无法排除 pyo3");
    Ok(())
}

// ─── step: test ─────────────────────────────────────────────────────────────

fn step_test(root: &Path, plan: &FeaturePlan) -> Result<(), String> {
    // 本机页面文件有限：编译并行度压到 2，避免 rustc mmap 失败（os error 1455）；
    // 测试线程仍按仓库既有做法单线程。
    if plan.pyo3_available() {
        return cargo(
            root,
            &strings(&[
                "test",
                "--workspace",
                "-j",
                "2",
                "--",
                "--test-threads=1",
            ]),
        );
    }

    cargo(
        root,
        &strings(&[
            "test",
            "--workspace",
            "-j",
            "2",
            "--exclude",
            "zela-app",
            "--exclude",
            "zela-gui",
            "--exclude",
            "zela-compile",
            "--",
            "--test-threads=1",
        ]),
    )?;
    cargo(
        root,
        &[
            strings(&["test", "-p", "zela-app", "-j", "2"]),
            plan.app_args(),
            strings(&["--", "--test-threads=1"]),
        ]
        .concat(),
    )?;
    cargo(
        root,
        &[
            strings(&["test", "-p", "zela-compile", "-j", "2"]),
            plan.app_args(),
            strings(&["--", "--test-threads=1"]),
        ]
        .concat(),
    )?;
    println!("[xtask] 跳过 zela-gui：无 Python 时其依赖边无法排除 pyo3");
    Ok(())
}

// ─── step: wasm ─────────────────────────────────────────────────────────────

fn step_wasm(root: &Path) -> Result<(), String> {
    for name in KERNEL_CRATES {
        println!("[xtask] == {name} (wasm32-unknown-unknown) ==");
        cargo(
            root,
            &strings(&["check", "-p", name, "--target", "wasm32-unknown-unknown"]),
        )?;
    }

    // 门面必须保持 wasm32 能力：不能吞 lua / py / UI。
    for name in FACADE_CRATES {
        println!("[xtask] == {name} facade (wasm32-unknown-unknown) ==");
        cargo(
            root,
            &strings(&["check", "-p", name, "--target", "wasm32-unknown-unknown"]),
        )?;
    }

    // 可选 wasm 能力：只有"同引擎覆盖 native+wasm"的适配器才在这里。
    for name in OPTIONAL_WASM_CRATES {
        println!("[xtask] == {name} optional-wasm (wasm32-unknown-unknown) ==");
        cargo(
            root,
            &strings(&["check", "-p", name, "--target", "wasm32-unknown-unknown"]),
        )?;
    }

    println!("[xtask] == zela-scene-bundle (wasm32-unknown-unknown) ==");
    cargo(
        root,
        &strings(&[
            "check",
            "-p",
            "zela-scene-bundle",
            "--target",
            "wasm32-unknown-unknown",
        ]),
    )?;

    println!("[xtask] == zela-render-backends (wgpu, no blocking) ==");
    cargo(
        root,
        &strings(&[
            "check",
            "-p",
            "zela-render-backends",
            "--target",
            "wasm32-unknown-unknown",
            "--no-default-features",
            "--features",
            "wgpu",
        ]),
    )?;

    println!("[xtask] == zela-schedule (default features) ==");
    cargo(
        root,
        &strings(&[
            "check",
            "-p",
            "zela-schedule",
            "--target",
            "wasm32-unknown-unknown",
        ]),
    )?;

    println!("[xtask] == zela-render (wasm32-unknown-unknown) ==");
    cargo(
        root,
        &strings(&[
            "check",
            "-p",
            "zela-render",
            "--target",
            "wasm32-unknown-unknown",
        ]),
    )?;

    println!("[xtask] == zela-playback (web config) ==");
    cargo(
        root,
        &strings(&[
            "check",
            "-p",
            "zela-playback",
            "--target",
            "wasm32-unknown-unknown",
            "--no-default-features",
        ]),
    )?;

    println!("[xtask] == zela-web ==");
    cargo(
        root,
        &strings(&["check", "-p", "zela-web", "--target", "wasm32-unknown-unknown"]),
    )?;

    Ok(())
}

// ─── step: structure ────────────────────────────────────────────────────────

/// **门禁 1（阶段 0.5）**：`crates/**/Cargo.toml` 的每个 crate 必须在 GAPS 的
/// crate 能力表里登记；表里出现仓库没有的名字同样失败（打错字也是缺口）。
fn check_crate_registry(root: &Path) -> Result<(), String> {
    let mut crates: Vec<(String, PathBuf)> = Vec::new();
    collect_crates(&root.join("crates"), &mut crates)?;
    let text = fs::read_to_string(root.join("GAPS.md"))
        .map_err(|error| format!("读取 GAPS.md 失败：{error}"))?;
    let start = text.find("## 附录. crate 能力表").ok_or_else(|| {
        "GAPS.md 缺少 `## 附录. crate 能力表`（阶段 0.5 的登记表）".to_string()
    })?;
    let body = text[start..]
        .split("\n## ")
        .next()
        .unwrap_or(&text[start..]);

    let mut registered = std::collections::BTreeSet::new();
    for line in body.lines() {
        let cells = table_cells(line);
        if cells.len() < 3 {
            continue;
        }
        let first = cells[0].trim().trim_matches('`');
        if first.is_empty() || first == "crate" || first.starts_with('-') {
            continue;
        }
        registered.insert(first.to_string());
    }

    let known: std::collections::BTreeSet<&str> =
        crates.iter().map(|(name, _)| name.as_str()).collect();
    let mut missing: Vec<&str> = crates
        .iter()
        .filter(|(name, _)| !registered.contains(name))
        .map(|(name, _)| name.as_str())
        .collect();
    missing.sort_unstable();
    if !missing.is_empty() {
        return Err(format!(
            "以下 crate 未在 GAPS「crate 能力表」登记（新增 crate 必须登记）：{}",
            missing.join(", ")
        ));
    }
    let mut unknown: Vec<&str> = registered
        .iter()
        .filter(|name| !known.contains(name.as_str()))
        .map(String::as_str)
        .collect();
    unknown.sort_unstable();
    if !unknown.is_empty() {
        return Err(format!(
            "GAPS「crate 能力表」登记了仓库里不存在的 crate（拼写错误？）：{}",
            unknown.join(", ")
        ));
    }
    println!(
        "[xtask] OK: crate 能力表覆盖 {} 个 crate",
        crates.len()
    );
    Ok(())
}

/// **跨 crate 重名白名单**（计划 §0.5 / 硬约束 4）。
///
/// 只登记**允许存在**的同名 `pub struct/enum`，每条必须写清理由；
/// 新出现的同名类型必须在 `cargo verify --step structure` 里显式登记后才通过。
const DUPLICATE_PUB_NAME_ALLOWLIST: &[(&str, &str)] = &[
    ("OpDef", "已收敛到 ze-type；保留条目让『再次出现第二份』立刻失败"),
    ("ChannelSpec", "Pod 声明是 ChannelDecl，脚本值类型是 ChannelSpec —— 角色不同"),
    ("NodeSpec", "装配计划（ze-pod-core）与节点 schema 各自的领域类型"),
    // ── 历史领域重复（本批只登记，收敛属后续领域去重批次）────────────────
    ("Arrow", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("Axes2D", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("BackgroundStroke", "render-ir 的绘制参数 vs scene-api 的组件视图"),
    ("Camera2D", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("Camera3D", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("Circle", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("ContentHandle", "pre-render 的句柄 vs scene-core 的内容句柄"),
    ("Curve", "scene-api 的曲线参数 vs ze-time 的曲线内核"),
    ("CurveWindow", "render-ir 的绘制参数 vs scene-api 的组件视图"),
    ("Dash", "render-ir 的绘制参数 vs scene-api 的组件视图"),
    ("EvalError", "ze-comb 的表达式错误 vs ze-node-core 的图求值错误"),
    ("Gradient", "render-ir 的绘制参数 vs scene-api 的组件视图"),
    ("Group", "scene-api 的 Pod 侧包装 vs scene-contract 的契约类型"),
    ("Handle", "pod-core 的资源句柄 vs scene-core 的脚本局部句柄"),
    ("Light", "scene-api / scene-core 的组件 vs render-vector 的 GPU 光照"),
    ("LightKind", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("LineStyle", "scene-api 的组件视图 vs playback 的矢量线样式"),
    ("Material3D", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("Name", "scene-api 的 Pod 侧包装 vs scene-contract 的契约类型"),
    ("Opacity", "render-ir 的绘制参数 vs scene-api 的组件视图"),
    ("Parent", "scene-api 的 Pod 侧包装 vs scene-contract 的契约类型"),
    ("Path", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("PhaseAnim", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("Rect", "scene-core 的几何组件 vs node-editor 的编辑框"),
    ("Reveal", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("SecondPaint", "render-ir 的绘制参数 vs scene-api 的组件视图"),
    ("ShaderVariant", "render-ir 的着色参数 vs scene-api 的组件视图"),
    ("Surface", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("SurfaceMapping", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("SurfaceRenderer", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("TextureFilter", "render-ir 的采样参数 vs scene-api 的组件视图"),
    ("Transform", "scene-api 的 Pod 侧包装 vs scene-core 的组件定义"),
    ("Transform3D", "ze-notion 的数学变换 vs scene-api 的组件视图"),
];

/// 测试专用 crate 不参与跨 crate 重名门禁（ publish = false，不进生产图）。
const DUPLICATE_SCAN_EXCLUDED_CRATES: &[&str] = &["ze-pod-tests"];

/// **门禁 2（阶段 0.5）**：跨 crate 同名 `pub struct/enum` 必须在白名单里。
fn check_duplicate_public_names(root: &Path) -> Result<(), String> {
    use std::collections::{BTreeMap, BTreeSet};

    let mut crates: Vec<(String, PathBuf)> = Vec::new();
    collect_crates(&root.join("crates"), &mut crates)?;
    let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for (crate_name, path) in &crates {
        if DUPLICATE_SCAN_EXCLUDED_CRATES.contains(&crate_name.as_str()) {
            continue;
        }
        let mut files = Vec::new();
        collect_rs_files(&path.join("src"), &mut files)?;
        for file in files {
            let text = match fs::read_to_string(&file) {
                Ok(text) => text,
                Err(_) => continue,
            };
            for line in text.lines() {
                let trimmed = line.trim_start();
                let rest = trimmed
                    .strip_prefix("pub struct ")
                    .or_else(|| trimmed.strip_prefix("pub enum "));
                let Some(rest) = rest else { continue };
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name.is_empty() {
                    continue;
                }
                seen.entry(name).or_default().insert(crate_name.clone());
            }
        }
    }

    let allow: BTreeSet<&str> = DUPLICATE_PUB_NAME_ALLOWLIST
        .iter()
        .map(|(name, _)| *name)
        .collect();
    let mut conflicts: Vec<String> = Vec::new();
    for (name, crates) in &seen {
        if crates.len() > 1 && !allow.contains(name.as_str()) {
            conflicts.push(format!(
                "{name}: {}",
                crates.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    if !conflicts.is_empty() {
        conflicts.sort();
        return Err(format!(
            "跨 crate 同名 `pub struct/enum`（一个概念只允许一处定义；确属不同角色请登记白名单）：\n  {}",
            conflicts.join("\n  ")
        ));
    }
    println!(
        "[xtask] OK: 跨 crate 同名 pub 类型（{} 个名字，{} 条白名单）",
        seen.len(),
        DUPLICATE_PUB_NAME_ALLOWLIST.len()
    );
    Ok(())
}

/// **阶段 4 门禁**：遍历 / 求值 / 装配路径禁用 `HashMap` / `HashSet`。
///
/// 为什么：这些路径的输出顺序会被下游继承（拓扑序、诊断次序、pass 结果），
/// 一旦用哈希容器，顺序就随种子漂移 —— 而"确定性"是本项目的产物承诺。
/// 名单只覆盖**表达式图**与拓扑算法；场景 ECS 的容器不在本门禁范围内。
fn check_no_hashmap_in_graph_paths(root: &Path) -> Result<(), String> {
    const NO_HASHMAP_PATHS: &[&str] = &[
        "crates/kernel/ze-graph-core/src",
        "crates/kernel/ze-comb/src",
        "crates/kernel/ze-func/src/simplify.rs",
        "crates/scene/ze-scene-core/src/scene_script.rs",
        "crates/scene/ze-scene-core/src/animation/predicate.rs",
        "crates/scene/ze-scene-core/src/animation/timeline.rs",
        "crates/ze/pod/ze-pod-func/src/expr_pod.rs",
        "crates/ze/pod/ze-pod-func/src/infer.rs",
        // 阶段 0（AP-01）：父子变换遍历此前只靠注释保证"不依赖 HashMap 顺序"。
        "crates/scene/zela-scene-ecs/src/transform_hierarchy.rs",
    ];

    let mut offenders = Vec::new();
    for entry in NO_HASHMAP_PATHS {
        let path = root.join(entry);
        let mut files = Vec::new();
        if path.is_dir() {
            collect_rs_files(&path, &mut files)?;
        } else {
            files.push(path);
        }
        for file in files {
            let text = fs::read_to_string(&file)
                .map_err(|error| format!("读取 {} 失败：{error}", file.display()))?;
            for (index, line) in text.lines().enumerate() {
                // 文档注释里出现 `HashMap` 是在**说明禁令**，不是使用。
                if line.trim_start().starts_with("//") {
                    continue;
                }
                if line.contains("HashMap") || line.contains("HashSet") {
                    offenders.push(format!("{}:{}: {}", file.display(), index + 1, line.trim()));
                }
            }
        }
    }

    if offenders.is_empty() {
        println!("[xtask] OK: 遍历 / 求值 / 装配路径不含 HashMap / HashSet");
        return Ok(());
    }
    offenders.sort();
    eprintln!(
        "[xtask] 遍历 / 求值 / 装配路径出现 HashMap / HashSet（输出顺序会随哈希种子漂移；请改 BTreeMap / BTreeSet / Vec）："
    );
    for offender in &offenders {
        eprintln!("  {offender}");
    }
    Err("确定性门禁未通过：表达式图路径禁用 HashMap / HashSet".to_string())
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let entry = entry.map_err(|error| format!("读取 {} 失败：{error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// GAPS 各托管段的"小计"必须与表格里的条目状态一致（B7 Step 0.2）。
///
/// 为什么要有这条检查：手写小计已连续错三次（B6 写 16 实际 17、B9 写 12 实际 11、
/// B9-L 列举 11 写 12）。这里是**从条目状态重算**再与小计行比对，错了就红；
/// 报错文案会给出重算后的正确小计，修起来是机械动作。
fn check_gaps_subtotals(root: &Path) -> Result<(), String> {
    let path = root.join("GAPS.md");
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
    // K 段（B4+B5+B6）、L 段（B8+B9）、M 段（B7）、N 段（B10）、O 段（B11）、
    // P 段（B12）、Q 段（B13）、R 段（B14）、S 段（B15）、T 段（B17）、U 段（B18）、
    // V 段（B19 跨批补记）、W 段（PA 记账收尾）、X 段（PB 参数开启）、Y 段（PC 内容纹理层）、
    // Z 段（PD 能力普查）、AA 段（PE 能力入口）、AB 段（PF 接缝）、AC 段（PG 全开放）、
    // AD 段（PH 能力缺口归零）、AE 段（PI 渲染层收尾与成本度量）
    // 都是"编号 | 缺口 | 影响层 | 类别 | 证据"。
    for prefix in [
        "## K.", "## L.", "## M.", "## N.", "## O.", "## P.", "## Q.", "## R.", "## S.", "## T.",
        "## U.", "## V.", "## W.", "## X.", "## Y.", "## Z.", "## AA.", "## AB.", "## AC.",
        "## AD.", "## AE.", "## AL.", "## AM.", "## AN.", "## AO.", "## AP.",
    ] {
        let Some(body) = gaps_section(&text, prefix) else {
            continue;
        };
        let base_line = text
            .find(prefix)
            .map(|at| text[..at].lines().count())
            .unwrap_or(0);
        // 0 = 已关闭，1 = 部分关闭，2 = 记账，3 = 未执行，4 = 落档（参数性差异，终态）
        let mut counts = [0usize; 5];
        for (index, line) in body.lines().enumerate() {
            let cells = table_cells(line);
            let Some(first) = cells.first() else {
                continue;
            };
            let id = first.trim_matches('*').trim();
            let is_item = id.contains('-')
                && (id.starts_with('V')
                    || id.starts_with("PA-")
                    || id.starts_with("PB-")
                    || id.starts_with("PC-")
                    || id.starts_with("PD-")
                    || id.starts_with("PE-")
                    || id.starts_with("PF-")
                    || id.starts_with("PG-")
                    || id.starts_with("PH-")
                    || id.starts_with("PI-")
                    || id.starts_with("S4-")
                    || id.starts_with("W1-")
                    || id.starts_with("X1-")
                    || id.starts_with("X2-")
                    || id.starts_with("X3-")
                    || id.starts_with("X4-")
                    || id.starts_with("X5-")
                    || id.starts_with("X6-")
                    || id.starts_with("X7-")
                    || id.starts_with("X8-")
                    || id.starts_with("AL-")
                    || id.starts_with("AM-")
                    || id.starts_with("AN-")
                    || id.starts_with("AO-")
                    || id.starts_with("AP-"));
            if !is_item {
                continue;
            }
            // PA A2：条目必须满 5 列且证据列非空。此前 `cells.len() < 4 { continue }`
            // 会把"缺证据列"的行**静默跳过**——又一个"看起来全绿"的缺口。
            if cells.len() < 5 || cells[4].trim().is_empty() {
                return Err(format!(
                    "{} 第 {} 行的条目 `{id}` 缺证据列（必须满 5 列且第 5 列非空）：\n  {}",
                    prefix,
                    base_line + index + 1,
                    line.trim()
                ));
            }
            let status = cells[3];
            let bucket = if status.contains("部分关闭") {
                1
            } else if status.contains("已关闭") {
                0
            } else if status.contains("落档") {
                4
            } else if status.contains("记账") {
                2
            } else if status.contains("未执行") {
                3
            } else {
                continue;
            };
            counts[bucket] += 1;
        }

        let summary = body
            .lines()
            .find(|line| line.contains("段小计"))
            .ok_or_else(|| format!("{prefix} 段缺少小计行"))?;
        let labels = [
            ("已关闭", 0usize),
            ("部分关闭", 1),
            ("记账", 2),
            ("未执行", 3),
            ("落档", 4),
        ];
        for (label, bucket) in labels {
            match number_after(summary, label) {
                Some(written) if written == counts[bucket] => {}
                Some(written) => {
                    return Err(format!(
                        "{prefix} 段小计与条目不符：`{label}` 写 {written}，实际 {}。\n  \
                         正确的小计片段：{}\n  （请按表格重算，不要手写）",
                        counts[bucket],
                        subtotal_line(&counts),
                    ));
                }
                None if counts[bucket] == 0 => {}
                None => {
                    return Err(format!(
                        "{prefix} 段小计漏写 `{label}`（实际 {}）。正确的小计片段：{}",
                        counts[bucket],
                        subtotal_line(&counts),
                    ));
                }
            }
        }
        println!(
            "[xtask] GAPS {prefix} 小计一致：已关闭 {} · 部分关闭 {} · 记账 {} · 未执行 {} · 落档 {}",
            counts[0], counts[1], counts[2], counts[3], counts[4]
        );
    }
    Ok(())
}

/// **文档锚点**校验（B12 Step 0.3 起）：`crate::mod::Item` 形式的引用必须有定义，
/// markdown 链接里的 `crates/...` 路径必须存在（B19 Step 2 加路径，PA Step 4 扩域）。
///
/// - 只查**存在性**，不查语义（"常量还在但缺口已关闭"是 `verified` 字段的事，不归这里）；
/// - 只校验首段是**本仓 crate**（`crates/**/Cargo.toml` 的 name，`-`/`_` 等价）的锚点；
///   首段像 crate（`ze_*` / `zela_*`）但找不到 ⇒ **crate 改名 / 已删除**；
/// - 末段在本 crate 的 `src/**` 里搜不到 ⇒ **符号改名 / 已删除**。
///
/// 为什么用符号名而不是行号：行号腐烂是**静默**的（永远指向某一行，只是不是你要的那行），
/// 符号名腐烂是**响亮**的（grep 0 命中）。
///
/// **B19 Step 2**：除符号锚点外，还要校验 markdown 链接里的 `crates/...` **路径**形态
/// （`crates/.../file.rs#L259-L275`）。此前只认 `::`，A/C 段的路径引用**全部被静默过滤**，
/// 于是 7 条死链在门禁全绿的情况下存在了三批。`#L` 只是提示，**不参与定位**。
///
/// **PA Step 4（PA-02）**：路径校验的作用域从 `GAPS.md` 扩到**仓库根所有 `.md`** ——
/// 符号锚点（`crate::mod::Item`）仍只在 GAPS 里查，它是 GAPS 的记账约定；路径是通用的。
/// `WASM_PORT_SURVEY.md` **显式排除**（历史探测快照，路径漂移是它本来就该保留的信息；
/// 文件头有可见声明 ⇒ 排除是响亮的，不是静默的）。含 `...` 的路径按构造不可解析，
/// 不参与定位，但会**打印出来**让人决定是改成真实路径还是改写表述。
fn check_gaps_anchors(root: &Path) -> Result<(), String> {
    let mut crates: Vec<(String, PathBuf)> = Vec::new();
    collect_crates(&root.join("crates"), &mut crates)?;

    let text = fs::read_to_string(root.join("GAPS.md"))
        .map_err(|error| format!("读取 GAPS.md 失败：{error}"))?;
    let mut checked = 0usize;
    for token in backticked_anchor_tokens(&text) {
        let mut segments = token.split("::");
        let head = segments.next().unwrap_or_default();
        let normalized = head.replace('_', "-");
        let Some((_, dir)) = crates.iter().find(|(name, _)| *name == normalized) else {
            if head.starts_with("ze_") || head.starts_with("zela_") {
                return Err(format!(
                    "GAPS 锚点 `{token}`：crate `{head}` 不在本仓（改名或已删除？）"
                ));
            }
            continue;
        };
        let last = token
            .rsplit("::")
            .next()
            .unwrap_or_default()
            .trim_end_matches("()");
        if !ident_exists_in(&dir.join("src"), last)? {
            return Err(format!(
                "GAPS 锚点 `{token}`：在 {} 的 src/ 里找不到符号 `{last}`（改名或已删除？）",
                dir.display()
            ));
        }
        checked += 1;
    }
    println!("[xtask] GAPS 锚点：{checked} 条 crate 限定引用全部有定义");

    let mut linked = 0usize;
    let mut missing: Vec<String> = Vec::new();
    let mut abbreviated: Vec<String> = Vec::new();
    for document in root_markdown_docs(root)? {
        let name = document
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("?");
        let body = fs::read_to_string(&document)
            .map_err(|error| format!("读取 {} 失败：{error}", document.display()))?;
        for token in linked_crate_paths(&body) {
            if token.contains("...") {
                // 按构造不可解析；由下面的全文扫描统一报告（链接只是其中一种形态）。
                continue;
            }
            if root.join(&token).exists() {
                linked += 1;
            } else {
                missing.push(format!("{name}: {token}"));
            }
        }
        // 省略号路径可能出现在链接里，也可能只出现在行内代码 / shell 示例里
        // （`ARCHITECTURE.md` 的 `grep ... crates/app/.../lib.rs`）。
        for token in abbreviated_crate_paths(&body) {
            let entry = format!("{name}: {token}");
            if !abbreviated.contains(&entry) {
                abbreviated.push(entry);
            }
        }
    }
    if !missing.is_empty() {
        // 一次列全（只报第一条会让人以为改完就绿了）。
        return Err(format!(
            "文档链接有 {} 条指向不存在的路径：\n  {}\n  \
             修法：改指向新位置；能力确实没了就改指向**可执行证据**（单测名 / 对拍用例），\
             不要删掉引用。",
            missing.len(),
            missing.join("\n  ")
        ));
    }
    if !abbreviated.is_empty() {
        println!(
            "[xtask] 文档链接：{} 条含省略号（按构造不可解析，未定位；请人工确认是真实路径还是改写表述）：",
            abbreviated.len()
        );
        for item in &abbreviated {
            println!("  {item}");
        }
    }
    println!("[xtask] 文档链接：{linked} 条 `crates/` 路径全部存在");
    Ok(())
}

/// 文档里**含省略号**的 `crates/...` 片段：按构造不可解析 ⇒ 只提示、不参与定位。
///
/// 这类片段可能出现在 markdown 链接里，也可能只出现在行内代码 / shell 示例里
/// （`ARCHITECTURE.md` 的 `grep ... crates/app/.../lib.rs`）⇒ 不能只扫链接。
fn abbreviated_crate_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("crates/") {
        let tail = &rest[at..];
        let end = tail
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '`' | '"' | '\'' | ')' | '>' | '|' | '，' | '。')
            })
            .unwrap_or(tail.len())
            .max(1);
        let token = tail[..end].trim_end_matches([',', ';']);
        // 只报"省略号后面还有真实片段"的（`crates/app/.../lib.rs`）；
        // 纯泛指 `crates/...` 不算路径，报出来只会训练人忽略输出。
        let tail_after = token.rsplit("...").next().unwrap_or_default().trim_start_matches('/');
        if token.contains("...") && !tail_after.is_empty() {
            out.push(token.to_string());
        }
        rest = &tail[end..];
    }
    out
}

/// 仓库根目录的 `.md` 文档（按文件名排序）。
///
/// `WASM_PORT_SURVEY.md` **显式排除**：它是历史探测快照，路径漂移是它本来就该保留的信息。
/// 该文件头部有一行可见声明（"本报告为历史快照，文中路径可能已漂移…"），所以这个排除
/// 是**响亮**的；静默排除等于又造一个"看起来全绿"的门禁（PA-02）。
fn root_markdown_docs(root: &Path) -> Result<Vec<PathBuf>, String> {
    const EXCLUDED: &[&str] = &["WASM_PORT_SURVEY.md"];
    let mut out = Vec::new();
    let entries =
        fs::read_dir(root).map_err(|error| format!("读取 {} 失败：{error}", root.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("遍历 {} 失败：{error}", root.display()))?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if EXCLUDED.contains(&name) {
            continue;
        }
        out.push(path);
    }
    out.sort();
    Ok(out)
}

/// 递归收集 `crates/**/Cargo.toml` 的 `name` → crate 目录。
fn collect_crates(dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|error| format!("读取 {} 失败：{error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("遍历 {} 失败：{error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_crates(&path, out)?;
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) != Some("Cargo.toml") {
            continue;
        }
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
        let name = text
            .lines()
            .find_map(|line| {
                let line = line.trim();
                line.strip_prefix("name")
                    .and_then(|rest| rest.trim_start().strip_prefix('='))
                    .map(|rest| rest.trim().trim_matches('"').to_string())
            })
            .ok_or_else(|| format!("{} 里没有 name 字段", path.display()))?;
        if let Some(parent) = path.parent() {
            out.push((name, parent.to_path_buf()));
        }
    }
    Ok(())
}

/// GAPS 里的反引号 token → 只保留 `a::b::c` 形态（纯标识符段）。
fn backticked_anchor_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        let token = &after[..close];
        rest = &after[close + 1..];
        if !token.contains("::") {
            continue;
        }
        let clean = token.ends_with("()");
        let body = token.trim_end_matches("()");
        let ok = body.split("::").all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        if ok {
            out.push(if clean { format!("{body}()") } else { body.to_string() });
        }
    }
    out
}

/// GAPS 里的 markdown 链接目标 → 只保留 `crates/...` 形态的仓库内路径。
///
/// `#Lxx-Lyy` 只是提示，**不参与定位**（行号会腐烂且静默；路径不存在才是响亮的）。
fn linked_crate_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("](") {
        let after = &rest[open + 2..];
        let Some(close) = after.find(')') else { break };
        let target = after[..close].trim();
        rest = &after[close + 1..];
        let Some(tail) = target.strip_prefix("crates/") else {
            continue;
        };
        let path = tail.split('#').next().unwrap_or_default().trim_end_matches('/');
        if path.is_empty() {
            continue;
        }
        out.push(format!("crates/{path}"));
    }
    out
}

/// 在 `dir` 下的所有 `.rs` 里找独立标识符 `ident`（ASCII 单词边界）。
fn ident_exists_in(dir: &Path, ident: &str) -> Result<bool, String> {
    if ident.is_empty() {
        return Ok(false);
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(false),
    };
    for entry in entries {
        let entry = entry.map_err(|error| format!("遍历 {} 失败：{error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            if ident_exists_in(&path, ident)? {
                return Ok(true);
            }
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
        if has_ident(&text, ident) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_ident(text: &str, ident: &str) -> bool {
    let bytes = text.as_bytes();
    let mut offset = 0usize;
    while let Some(pos) = text[offset..].find(ident) {
        let start = offset + pos;
        let end = start + ident.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        offset = end;
    }
    false
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// 取 `prefix` 开头的一节（直到下一个 `## `）。
fn gaps_section<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let start = text.find(prefix)?;
    let rest = &text[start..];
    let end = rest[3..]
        .find("\n## ")
        .map(|offset| offset + 3)
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// 把一行 markdown 表格拆成单元格（去掉首尾的 `|`）。
fn table_cells(line: &str) -> Vec<&str> {
    let trimmed = line.trim();
    let Some(inner) = trimmed.strip_prefix('|').and_then(|rest| rest.strip_suffix('|')) else {
        return Vec::new();
    };
    inner.split('|').collect()
}

/// `label` 之后第一个数字（`已关闭 11（…）` ⇒ `Some(11)`）。
fn number_after(line: &str, label: &str) -> Option<usize> {
    let rest = &line[line.find(label)? + label.len()..];
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// 重算后的小计片段（用于报错文案）。
fn subtotal_line(counts: &[usize; 5]) -> String {
    let mut parts = Vec::new();
    for (label, count) in [
        ("已关闭", counts[0]),
        ("部分关闭", counts[1]),
        ("记账", counts[2]),
        ("未执行", counts[3]),
        ("落档", counts[4]),
    ] {
        if count > 0 {
            parts.push(format!("{label} {count}"));
        }
    }
    parts.join(" · ")
}

/// **能力覆盖矩阵**（B7 Step 5）：每个脚本可见能力都必须被 manifest 作品用到，
/// 否则必须在 `demo/capability_exemptions.txt` 里豁免、并指向一条真实存在的 GAPS 记账。
///
/// 为什么读源码而不是维护清单：手写清单是"第二张真相表"，加一个能力忘改一处就失真
/// —— 这正是本批要消灭的失败模式（见 `xtask/src/lib.rs` 的同一段注释）。
fn step_coverage(root: &Path) -> Result<(), String> {
    let read = |relative: &str| -> Result<String, String> {
        let path = root.join(relative);
        fs::read_to_string(&path).map_err(|error| format!("读取 {} 失败：{error}", path.display()))
    };
    let api = read("crates/scene/ze-scene-api/src/api.rs")?;
    let types = read("crates/scene/ze-scene-api/src/types.rs")?;
    let target = read("crates/scene/ze-scene-core/src/animation/target.rs")?;
    // F 批：字段通道改由 `#[derive(Channels)]` 生成 ⇒ 也要从组件源码的
    // `#[channel(...)]` 属性里抽（否则 derive 掉的那些通道会从门禁里安静消失）。
    let channel_files = [
        "crates/scene/ze-scene-core/src/object/primitives.rs",
        "crates/scene/ze-scene-core/src/object/camera.rs",
        "crates/scene/ze-scene-core/src/components.rs",
        "crates/scene/ze-scene-core/src/components/3d.rs",
    ];
    let mut channel_sources = Vec::new();
    for relative in channel_files {
        channel_sources.push((relative, read(relative)?));
    }
    let channel_sources: Vec<(&str, &str)> = channel_sources
        .iter()
        .map(|(relative, source)| (*relative, source.as_str()))
        .collect();
    let manifest = read("crates/app/zela-app/scripts/demo/manifest.txt")?;
    let gaps = read("GAPS.md")?;

    let exemptions_path = scripts_dir(root).join("demo").join("capability_exemptions.txt");
    let exemptions = if exemptions_path.is_file() {
        xtask::parse_exemptions(
            &fs::read_to_string(&exemptions_path)
                .map_err(|error| format!("读取 {} 失败：{error}", exemptions_path.display()))?,
        )?
    } else {
        Vec::new()
    };

    let capabilities = xtask::collect_capabilities(&api, &types, &target, &channel_sources)?;
    let scripts_root = scripts_dir(root);
    let mut scripts = String::new();
    for relative in xtask::manifest_scripts(&manifest) {
        let path = scripts_root.join(&relative);
        scripts.push_str(
            &fs::read_to_string(&path)
                .map_err(|error| format!("读取作品脚本 {} 失败：{error}", path.display()))?,
        );
        scripts.push('\n');
    }

    let report = xtask::check_coverage(&capabilities, &scripts, &exemptions, &gaps)?;
    println!(
        "[xtask] 能力覆盖：{}/{} 被 manifest 作品使用（豁免 {} 条）",
        report.covered.len(),
        report.total,
        report.exempted.len()
    );
    for exemption in &report.exempted {
        println!(
            "[xtask] 豁免 {} → {}（GAPS 已记账）",
            exemption.capability, exemption.gaps
        );
    }
    Ok(())
}

fn step_structure(root: &Path) -> Result<(), String> {
    // 阶段 0.5 门禁：新 crate 必须登记 / 跨 crate 同名 pub 类型必须进白名单。
    // 这两条不依赖 jq，先跑。
    check_crate_registry(root)?;
    check_duplicate_public_names(root)?;
    // 阶段 4：表达式图 / 拓扑路径的确定性门禁（禁用 HashMap / HashSet）。
    check_no_hashmap_in_graph_paths(root)?;
    // B7 Step 0.2：GAPS 的小计必须与条目状态一致（不依赖 jq，先跑）。
    check_gaps_subtotals(root)?;
    // B12 Step 0.3：GAPS 里的 crate 限定语义锚点必须能在源码里找到定义。
    check_gaps_anchors(root)?;
    let jq_available = Command::new("jq")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !jq_available {
        println!("[xtask] 跳过结构性判据：未找到 jq");
        return Ok(());
    }

    let output = cargo_capture(
        root,
        &strings(&["metadata", "--format-version", "1", "--no-deps"]),
    )?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let metadata = String::from_utf8_lossy(&output.stdout).into_owned();

    let checks = [
        (
            "crates/ze/pod 生产依赖不含 zela-*",
            JQ_POD_LAYER,
            "crates/ze/pod 的生产依赖边中出现了 zela-*（违反领域无关规则）：",
        ),
        (
            "zela-render-backends 生产依赖不含 zela-scene（S2）",
            JQ_BACKENDS,
            "zela-render-backends 的生产依赖边中出现了 zela-scene（违反 S2 分层规则）：",
        ),
        (
            "ze-scene-core / ze-scene-api 不依赖 zela-* / hecs（B3-1）",
            JQ_SCENE_LAYERS,
            "场景核心层或 Pod 能力层出现了 zela-* / hecs 生产依赖（违反分层规则）：",
        ),
        (
            "zela-render 生产依赖不含 zela-scene-ecs（B3-1 收尾）",
            JQ_RENDER_LAYER,
            "zela-render 的生产依赖边中出现了 zela-scene-ecs（渲染层只能依赖 ze-scene-hecs）：",
        ),
        (
            "除 ze-notion 与节点图组豁免外无直接 glam（M0）",
            JQ_GLAM,
            "依赖 ze-notion 的 crate 不得直接引 glam（数学入口唯一）：",
        ),
    ];

    for (title, program, violation) in checks {
        let violations = jq_filter(&metadata, program)?;
        if violations.trim().is_empty() {
            println!("[xtask] OK: {title}");
        } else {
            eprintln!("[xtask] {violation}");
            eprintln!("{violations}");
            return Err(format!("结构性判据未通过：{title}"));
        }
    }

    // B2-9（D14）：能力指纹 × version 配对校验。
    // 只跑这两个护栏用例：它们断言 `hash(signatures())` 与 `version` 的钉住基线，
    // API 变了而未递增 version / 未更新指纹 ⇒ **fail**（不是警告）。
    println!("[xtask] $ cargo test -p ze-scene-api --lib fingerprint_is_pinned");
    let status = Command::new("cargo")
        .args([
            "test",
            "-q",
            "-p",
            "ze-scene-api",
            "--lib",
            "fingerprint_is_pinned",
        ])
        .current_dir(root)
        .status()
        .map_err(|error| format!("启动能力指纹校验失败：{error}"))?;
    if !status.success() {
        return Err(
            "能力指纹校验失败：signatures 变了但 version/指纹常量未同步（D14）".to_string(),
        );
    }
    println!("[xtask] OK: 能力指纹与 version 配对（zela.scene / zela.content）");

    // B3-0.5c：四个 render capability 的指纹同样钉住——
    // 签名集（ze-render-ir::caps）变了而未更新 PINNED_FINGERPRINTS ⇒ fail。
    println!("[xtask] $ cargo test -p ze-render-ir --lib render_fingerprints_are_pinned");
    let status = Command::new("cargo")
        .args([
            "test",
            "-q",
            "-p",
            "ze-render-ir",
            "--lib",
            "render_fingerprints_are_pinned",
        ])
        .current_dir(root)
        .status()
        .map_err(|error| format!("启动渲染能力指纹校验失败：{error}"))?;
    if !status.success() {
        return Err(
            "渲染能力指纹校验失败：caps 签名变了但 PINNED_FINGERPRINTS 未同步（B3-0.5c）"
                .to_string(),
        );
    }
    println!("[xtask] OK: 渲染能力指纹（zela.render2d / render3d / mixed / shapes）");
    Ok(())
}

fn jq_filter(metadata: &str, program: &str) -> Result<String, String> {
    let mut child = Command::new("jq")
        .args(["-r", program, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("启动 jq 失败：{error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(metadata.as_bytes())
            .map_err(|error| format!("写入 jq 失败：{error}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("等待 jq 失败：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "jq 执行失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// ─── step: demo（manifest payload 断言） ────────────────────────────────────

/// **PG 记账 AD-01**：`demo/manifest.txt` 的 payload 断言。
///
/// 为什么需要这一步：Pages 工作流只跑 `build.ps1`，而它不在本机门禁链里 —— 脚本改动
/// （删占位相机、改动画）会静默改掉 ZSB payload，直到 push 之后才在 CI 红掉。
/// 这类失败已经发生过两次（PF 的 demo3d、PG 的 pbr_materials）。
///
/// 口径与 `build.ps1` 逐字一致：同一条 `zela-compile`、同一个 `ZELA_TYPST_FONT_DIR`
/// （由 [`cargo_command`] 注入）、同一条 `payload hash = …` 行；`-` 跳过、
/// `windows:…,linux:…` 只断言本机平台。
fn step_demo(root: &Path) -> Result<(), String> {
    let manifest = root.join("crates/app/zela-app/scripts/demo/manifest.txt");
    let text = fs::read_to_string(&manifest)
        .map_err(|error| format!("读取 {} 失败：{error}", manifest.display()))?;
    let scripts = root.join("crates/app/zela-app/scripts");
    let out_dir = root.join("target").join("xtask").join("demo");
    fs::create_dir_all(&out_dir)
        .map_err(|error| format!("创建 {} 失败：{error}", out_dir.display()))?;
    let platform = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };

    let mut checked = 0usize;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            return Err(format!(
                "manifest.txt:{} 期望 ≥3 列（id title… script [payload-hash]）：{line}",
                index + 1
            ));
        }
        let id = parts[0];
        let (script, expected) = if parts.len() == 3 {
            (parts[2], "-")
        } else {
            (parts[parts.len() - 2], parts[parts.len() - 1])
        };
        if expected == "-" {
            println!("[xtask] demo {id}: 不校验（manifest 写 `-`）");
            continue;
        }
        let want = if let Some((want, skipped)) = expected_hash_for(expected, platform)? {
            if let Some(note) = skipped {
                println!("[xtask] demo {id}: 跳过断言（{note}）");
                continue;
            }
            want
        } else {
            continue;
        };
        let script_path = scripts.join(script);
        if !script_path.is_file() {
            return Err(format!(
                "manifest.txt:{} 脚本不存在：{}",
                index + 1,
                script_path.display()
            ));
        }
        let out = out_dir.join(format!("{id}.zsb"));
        let args = [
            run_args("zela-compile", None),
            strings(&[
                &script_path.to_string_lossy(),
                "--out",
                &out.to_string_lossy(),
            ]),
        ]
        .concat();
        println!("[xtask] $ cargo {}", args.join(" "));
        let output = cargo_command(root)?
            .args(&args)
            .output()
            .map_err(|error| format!("启动 cargo 失败：{error}"))?;
        let log = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !output.status.success() {
            eprint!("{log}");
            return Err(format!("demo {id}：zela-compile 失败（{}）", output.status));
        }
        let Some(actual) = log
            .lines()
            .find_map(|line| line.trim().strip_prefix("payload hash = "))
            .map(|hash| hash.split_whitespace().next().unwrap_or_default().to_string())
        else {
            return Err(format!("demo {id}：编译输出里没有 `payload hash = …`"));
        };
        if actual != want {
            return Err(format!(
                "demo {id}：payload hash 不符（{platform}）\n  实测 = {actual}\n  期望 = {want}\n  \
                 ⇒ 脚本 / 场景装配改了 ZSB 内容：同步更新 `crates/app/zela-app/scripts/demo/manifest.txt`\n     \
                 （只允许在设置 ZELA_TYPST_FONT_DIR 时取新值；本步骤已自动注入）"
            ));
        }
        println!("[xtask] demo {id}: payload 与 manifest 一致");
        checked += 1;
    }
    println!("[xtask] demo：{checked} 条 payload 断言通过");
    Ok(())
}

/// 解析 manifest 的 payload-hash 列：`-` / 64 位 hex / `windows:hex,linux:hex`。
///
/// 返回 `(值, 跳过原因)`；`None` 表示 `-`（整条跳过）。
fn expected_hash_for(
    spec: &str,
    platform: &str,
) -> Result<Option<(String, Option<String>)>, String> {
    if spec == "-" {
        return Ok(None);
    }
    if spec.len() == 64 && spec.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(Some((spec.to_ascii_lowercase(), None)));
    }
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    for item in spec.split(',') {
        let Some((name, hash)) = item.split_once(':') else {
            return Err(format!(
                "manifest payload-hash 非法：{spec}（期望 -、64 位 hex，或 windows:hash,linux:hash）"
            ));
        };
        if !matches!(name, "windows" | "macos" | "linux") {
            return Err(format!("manifest payload-hash 平台非法：{name}"));
        }
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("manifest payload-hash 非法（{name}）：{hash}"));
        }
        pairs.push((name, hash));
    }
    match pairs.iter().find(|(name, _)| *name == platform) {
        Some((_, hash)) => Ok(Some((hash.to_ascii_lowercase(), None))),
        None => Ok(Some((
            String::new(),
            Some(format!("manifest 未提供 {platform} 的 payload hash")),
        ))),
    }
}

// ─── step: smoke / hashes ──────────────────────────────────────────────────

/// 短对拍（PR / `--fast`）：15 帧 @ 320×240，仍覆盖 `--script` 与 `--bundle` 两条路径。
///
/// **边界（不得省略）**：短路径抓不到跨端浮点分歧 —— 已知首次分歧出现在
/// fourier 第 103 帧；本步骤只渲 0.5s ⇒ 它绿只说明链路没断，
/// **不代表**跨端确定性没问题。完整基线（fourier / storyboard）由 `hashes` 负责，
/// 必须在 main push / nightly 定期跑。
fn step_smoke(root: &Path, skip_baseline: bool) -> Result<(), String> {
    let baseline_path = scripts_dir(root).join("baselines.txt");
    // 口径 D 门禁：未归因的 `+dirty` 不得进入对拍（否则差异无法回溯到提交）。
    assert_attributed(&fs::read_to_string(&baseline_path).map_err(|error| {
        format!("读取 {} 失败：{error}", baseline_path.display())
    })?)?;
    let baselines = load_baselines(&baseline_path)?;
    prepare_hashes_dir(root)?;
    announce_baselines(&baseline_path);

    let expected = if skip_baseline {
        None
    } else {
        Some(
            baselines
                .smoke
                .get("smoke")
                .ok_or_else(|| "baselines.txt 的 [smoke] 段缺少 `smoke`".to_string())?
                .as_str(),
        )
    };
    // B19 Step 3：smoke 的 `[smoke]` 序列哈希是**在旧默认（wgpu）下录的**。默认切成矢量后
    // 不传 `--backend` 会走到矢量 ⇒ 这里显式钉 wgpu，保住"25 条 + smoke 基线逐位不变"的硬约束。
    // 新默认路径本身由 `--step hashes` 的 11 条 `[vector]` 与 routing 单测守护，不是没人看。
    let _ = compare_script_vs_bundle(root, "smoke", "320x240", expected, Some("wgpu"))?;
    Ok(())
}

fn step_hashes(root: &Path, skip_baseline: bool, independent: bool) -> Result<(), String> {
    let baseline_path = scripts_dir(root).join("baselines.txt");
    // 口径 D 门禁：未归因的 `+dirty` 不得进入对拍（否则差异无法回溯到提交）。
    assert_attributed(&fs::read_to_string(&baseline_path).map_err(|error| {
        format!("读取 {} 失败：{error}", baseline_path.display())
    })?)?;
    let baselines = load_baselines(&baseline_path)?;
    prepare_hashes_dir(root)?;
    announce_baselines(&baseline_path);

    // B2-6：对拍结果同时喂给差分报告器（诊断旁路，不参与门禁）。
    let baseline_text = fs::read_to_string(&baseline_path)
        .map_err(|error| format!("读取 {} 失败：{error}", baseline_path.display()))?;
    let mut current: Vec<(&str, &str, &str)> = Vec::new();

    let hashes = if independent {
        println!("[xtask] 对拍模式：独立进程（--independent 诊断路径）");
        let mut out = Vec::new();
        for (work, backend) in HASHES_WORKS {
            let expected = if skip_baseline {
                None
            } else {
                Some(baseline_for(&baselines, work, *backend)?)
            };
            let hash = compare_script_vs_bundle(
                root,
                work,
                "640x480",
                expected.as_deref(),
                *backend,
            )?;
            out.push((work_label(work, *backend), hash));
        }
        out
    } else {
        println!("[xtask] 对拍模式：批量（一次 GPU 初始化；--independent 可切回独立进程）");
        let hashes = batch_hashes(root)?;
        if !skip_baseline {
            for ((work, backend), (label, hash)) in HASHES_WORKS.iter().zip(hashes.iter()) {
                let expected = baseline_for(&baselines, work, *backend)?;
                if *hash != expected {
                    return Err(format!(
                        "{label}: 序列哈希与基线不符（批量）\n  实测 = {hash}\n  基线 = {expected}"
                    ));
                }
                println!("[xtask] {label}: 基线（{hash}）");
            }
        }
        hashes
    };

    // B9 Step 5：V3-12 自动化（挂/不挂 HashSink 的像素产出逐位相同）。
    assert_sink_bypass(root)?;

    // 同一作品可能同时纳管两个后端 ⇒ section 取自条目本身，不按名字反查。
    for ((work, backend), (_, hash)) in HASHES_WORKS.iter().zip(hashes.iter()) {
        current.push((baseline_section(*backend), work, hash.as_str()));
    }
    // smoke 的当前值在 `--step hashes` 里不重算（由 `--step smoke` 负责）；
    // 报告只呈现本次实测到的作品，未实测项不伪造。
    print!("{}", xtask::render_diff_report(&xtask::build_diff_rows(&baseline_text, &current)));
    Ok(())
}

fn prepare_hashes_dir(root: &Path) -> Result<(), String> {
    let dir = root.join("target").join("xtask").join("hashes");
    fs::create_dir_all(&dir)
        .map_err(|error| format!("创建 {} 失败：{error}", dir.display()))
}

fn scripts_dir(root: &Path) -> PathBuf {
    root.join("crates").join("app").join("zela-app").join("scripts")
}

fn announce_baselines(path: &Path) {
    println!(
        "[xtask] 基线：{}（[browser] 段仅留档，不参与自动比对）",
        path.display()
    );
    println!(
        "[xtask] 对拍固定使用 --no-default-features --features typst（与 FRAME_HASH_BASELINES.md 的复现命令一致，绕开 pyo3 运行时）"
    );
}

/// 完整对拍的作品表（顺序 = 报告顺序；`None` = wgpu，`Some` = `--backend`）。
///
/// B9 Step 3：批量与独立两条路径**共用**这张表，避免两处清单漂移。
const HASHES_WORKS: &[(&str, Option<&str>)] = &[
    // B2-8（Q4 决议）：`demo3d` 让 3D 进对拍网。
    ("fourier", None),
    ("storyboard", None),
    ("demo3d", None),
    // B3 Step 5：Surface 作品（能力矩阵 + 复杂 Demo）。
    ("surface_matrix", None),
    // B7 Step 3：同一场景的 JS 翻译（像素必须与上面那件逐位相同）。
    ("surface_matrix.js", None),
    ("atlas", None),
    // B6 V2-07：矢量后端作品。
    ("vector_paths", Some("vector")),
    // B7 Step 2：此前零回归保护的 7 个 manifest 作品。
    ("fusion", None),
    ("pbr_materials", None),
    ("pbr_lights", None),
    ("pbr_scene", None),
    ("pendulum", None),
    ("orbit", None),
    ("spring", None),
    // B7 Step 4：零使用能力补齐（despawn / Curve.Const / Phase / CameraEye / MorphT / Group）。
    ("capability_gaps", None),
    // B10 Step 4：真 3D 网格 + 着色栈的作品级证据（必须走矢量后端才有着色栈）。
    ("shaded_meshes", Some("vector")),
    // B13 Step 3：9 件此前只有 wgpu 门禁的作品补上**矢量基线**。
    // 同一作品在两个 section 各一条（`[native]` 保既有门禁，`[vector]` 是新证据）；
    // 批量模式下靠 `--work` 的出现顺序配对，名称重复不是问题。
    ("fourier", Some("vector")),
    ("storyboard", Some("vector")),
    ("fusion", Some("vector")),
    ("pbr_materials", Some("vector")),
    ("pbr_lights", Some("vector")),
    ("pbr_scene", Some("vector")),
    ("pendulum", Some("vector")),
    ("orbit", Some("vector")),
    ("spring", Some("vector")),
    // PC Step 2：内容纹理层（composite = true）走矢量图层级叠加后的基线。
    // ⚠️ 这两条是**混合后端**（内容层矢量渲染 + CPU 叠加），不是纯矢量；见 GAPS PC-04。
    ("surface_matrix", Some("vector")),
    ("surface_matrix.js", Some("vector")),
    // PE Step 2：能力展示作品**只进 `[vector]`**。给它们录 `[native]` 基线等于把
    // "wgpu 静默丢弃渐变 / 虚线 / 双 paint / 背景描边 / 曲线窗口"固化成基线 ——
    // 正是 PE 要杀的模式（见 ARCHITECTURE 的 PE 节 / GAPS PE-02）。
    ("cap_gradient", Some("vector")),
    ("cap_dash", Some("vector")),
    ("cap_second", Some("vector")),
    ("cap_background", Some("vector")),
    ("cap_window", Some("vector")),
    // PF Step 2：2D/3D 混排（2D 记录以屏幕平面 overlay 叠进 3D 帧）。
    ("cap_mix", Some("vector")),
    // PG Step 2（路线 B）：内容层当相机视图、3D 材质按纹理采样 ⇒ atlas 进矢量。
    ("atlas", Some("vector")),
    // PH Step 1/2：两条此前被拒的作品第一次进矢量（Morph / 线框映射）。
    // `[native]` 条目已存在（上面的 wgpu 门禁），这里只补矢量证据。
    ("capability_gaps", Some("vector")),
    ("demo3d", Some("vector")),
];

/// 报告 / 重录用的唯一标签：同一作品同时纳管两个后端时不能只用作品名。
fn work_label(work: &str, backend: Option<&str>) -> String {
    match backend {
        Some(backend) => format!("{work}#{backend}"),
        None => work.to_string(),
    }
}

/// 取某作品在**指定后端**段里的基线值（`Some(_)` ⇒ `[vector]`，`None` ⇒ `[native]`）。
fn baseline_for(
    baselines: &Baselines,
    work: &str,
    backend: Option<&str>,
) -> Result<String, String> {
    let name = baseline_section(backend);
    let section = if name == "vector" {
        &baselines.vector
    } else {
        &baselines.native
    };
    section
        .get(work)
        .cloned()
        .ok_or_else(|| format!("baselines.txt 的 [{name}] 段缺少 `{work}`"))
}

/// 基线段：由条目自身的 backend 决定（同一作品在两个段各有一条）。
fn baseline_section(backend: Option<&str>) -> &'static str {
    if backend.is_some() { "vector" } else { "native" }
}

/// 从 `[verify] {...}` 的 JSON 行里取一个字符串字段（只含本仓自产的扁平 JSON）。
fn json_field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("\"{name}\":\"");
    let start = line.find(&key)? + key.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// **批量对拍**（B9 Step 3）：一次进程跑完全部作品（脚本 + bundle 两条路径）。
///
/// 渲染循环与 `zela-render` **共用**（`zela_app::frame_loop`），这里只负责拼参数、
/// 解析结果、以及"script == bundle"的断言（§4.4：失败要点名作品与路径）。
fn batch_hashes(root: &Path) -> Result<Vec<(String, String)>, String> {
    prepare_hashes_dir(root)?;
    let scripts = scripts_dir(root);
    let tmp = root.join("target").join("xtask").join("hashes");
    let mut args = strings(&["run", "-q", "--profile", "verify", "-p", "zela-app"]);
    args.extend(hashes_app_args());
    args.extend(strings(&["--bin", "zela-verify", "--"]));
    args.extend(strings(&[
        "--scripts",
        &scripts.display().to_string(),
        "--tmp",
        &tmp.display().to_string(),
        "--fps",
        "30",
    ]));
    for (work, backend) in HASHES_WORKS {
        let spec = match backend {
            Some(backend) => format!("{work}=640x480:{backend}"),
            None => format!("{work}=640x480"),
        };
        args.extend(strings(&["--work", &spec]));
    }

    println!("[xtask] $ cargo {}", args.join(" "));
    let output = cargo_command(root)?
        .args(&args)
        .output()
        .map_err(|error| format!("启动 cargo 失败：{error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        eprint!("{stdout}{stderr}");
        return Err("批量对拍失败（zela-verify 退出码非 0；上面的错误点明了作品与路径）".to_string());
    }

    let mut verified: Vec<(String, String, String)> = Vec::new();
    for line in stdout.lines() {
        // B12 Step 2 / B13：决策日志原样转出（每作品每遍一条，含拒绝理由）。
        if let Some(rest) = line.strip_prefix("[decision] ") {
            println!("[xtask] [decision] {rest}");
            continue;
        }
        let Some(rest) = line.strip_prefix("[verify] ") else {
            continue;
        };
        let work = json_field(rest, "work").ok_or_else(|| format!("无法解析 work：{line}"))?;
        let script = json_field(rest, "script").ok_or_else(|| format!("无法解析 script：{line}"))?;
        let bundle = json_field(rest, "bundle").ok_or_else(|| format!("无法解析 bundle：{line}"))?;
        verified.push((work.to_string(), script.to_string(), bundle.to_string()));
    }
    // 名称会重复（B13 Step 3：同一作品同时纳管 wgpu 与矢量）⇒ 按 `--work` 的出现顺序配对；
    // 名字不符说明两边顺序漂了，必须显式报错而不是错位比对。
    if verified.len() != HASHES_WORKS.len() {
        return Err(format!(
            "批量对拍产出了 {} 行 [verify]，HASHES_WORKS 有 {} 条（zela-verify 漏跑或多跑）",
            verified.len(),
            HASHES_WORKS.len()
        ));
    }
    let mut hashes: Vec<(String, String)> = Vec::with_capacity(verified.len());
    for ((work, backend), (name, script, bundle)) in HASHES_WORKS.iter().zip(verified.iter()) {
        if name != work {
            return Err(format!(
                "zela-verify 输出顺序与 HASHES_WORKS 不一致：期望 `{work}`，得到 `{name}`"
            ));
        }
        if script != bundle {
            return Err(format!(
                "{name}: --script 与 --bundle 哈希不一致（批量）\n  script = {script}\n  bundle = {bundle}"
            ));
        }
        let label = work_label(work, *backend);
        println!("[xtask] {label}: script == bundle（{script}）");
        hashes.push((label, script.clone()));
    }
    Ok(hashes)
}

/// **V3-12 自动化**（B9 Step 5 → B7 Step 0.3 并入批进程）：挂 / 不挂 `HashSink` 时，
/// 输出 PNG 的**字节**必须相同。
///
/// 为什么比 PNG 字节而不是序列哈希：不挂哈希时本来就没有哈希值可拿，比哈希是自证。
/// B7 起这件事在 `zela-verify --sink-bypass` 里做 —— 同一次 GPU 初始化，不再多两次
/// `cargo run`（B9 记账第 1 条：那两次曾让 `hashes` 从 6.46s 回到 13.07s）。
fn assert_sink_bypass(root: &Path) -> Result<(), String> {
    prepare_hashes_dir(root)?;
    let scripts = scripts_dir(root);
    let tmp = root.join("target").join("xtask").join("hashes");
    let mut args = strings(&["run", "-q", "--profile", "verify", "-p", "zela-app"]);
    args.extend(hashes_app_args());
    args.extend(strings(&["--bin", "zela-verify", "--"]));
    args.extend(strings(&[
        "--scripts",
        &scripts.display().to_string(),
        "--tmp",
        &tmp.display().to_string(),
        "--fps",
        "30",
        "--sink-bypass",
    ]));
    println!("[xtask] $ cargo {}", args.join(" "));
    let output = cargo_command(root)?
        .args(&args)
        .output()
        .map_err(|error| format!("启动 cargo 失败：{error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        eprint!("{stdout}{stderr}");
        return Err("V3-12 检查失败（zela-verify --sink-bypass 退出码非 0）".to_string());
    }
    let line = stdout
        .lines()
        .find(|line| line.starts_with("[verify] ") && line.contains("\"sink_bypass\":\"ok\""))
        .ok_or_else(|| format!("V3-12 检查没有产出结果行：{stdout}{stderr}"))?;
    let bytes = line
        .split("\"bytes\":")
        .nth(1)
        .and_then(|rest| rest.trim_end_matches('}').trim().parse::<usize>().ok())
        .ok_or_else(|| format!("V3-12 结果里没有 bytes：{line}"))?;
    println!(
        "[xtask] V3-12：挂 / 不挂 HashSink 的 PNG 字节相同（{bytes} B，vector_paths 1 帧）"
    );
    Ok(())
}

/// 渲染 `--script` 与 `--bundle` 两条路径，断言：
/// 1. 两者逐位相同；2. `expected = Some` 时等于给定基线。
///
/// `expected = None`（`--skip-baseline`）只做同机自洽断言：
/// 用于 CI 的软件 Vulkan（lavapipe），它与基线录制设备不同，像素哈希不可比。
///
/// `backend`：显式 `--backend wgpu|vector`，`None` = 走 CLI 默认（B19 起为 `auto`）。
/// B6 V2-07：矢量作品与 wgpu 基线共用同一套"脚本 == bundle == 基线"门禁，
/// 只是出帧后端不同 —— 两条路径都必须逐位一致。
fn compare_script_vs_bundle(
    root: &Path,
    work: &str,
    size: &str,
    expected: Option<&str>,
    backend: Option<&str>,
) -> Result<String, String> {
    let script_rel = format!("crates/app/zela-app/scripts/{work}.lua");
    let zsb_rel = format!("target/xtask/hashes/{work}.zsb");
    let script_path = root.join(&script_rel);
    if !script_path.is_file() {
        return Err(format!("脚本不存在：{}", script_path.display()));
    }

    println!("\n[xtask] -- {work}（{size}）--");
    compile_bundle(root, &script_rel, &zsb_rel)?;

    let script_hash = render(
        root,
        &["--script", &script_rel],
        &format!("target/xtask/hashes/{work}-script"),
        size,
        backend,
    )?;
    let bundle_hash = render(
        root,
        &["--bundle", &zsb_rel],
        &format!("target/xtask/hashes/{work}-bundle"),
        size,
        backend,
    )?;

    if script_hash != bundle_hash {
        return Err(format!(
            "{work}: --script 与 --bundle 哈希不一致\n  script = {script_hash}\n  bundle = {bundle_hash}"
        ));
    }
    match expected {
        Some(expected) => {
            if script_hash != expected {
                return Err(format!(
                    "{work}: 序列哈希与基线不符\n  实测 = {script_hash}\n  基线 = {expected}"
                ));
            }
            println!("[xtask] {work}: script == bundle == 基线（{script_hash}）");
        }
        None => {
            println!("[xtask] {work}: script == bundle（{script_hash}）");
        }
    }
    Ok(script_hash)
}

fn compile_bundle(root: &Path, script: &str, out: &str) -> Result<(), String> {
    let args = [run_args("zela-compile", None), strings(&[script, "--out", out])].concat();
    cargo(root, &args)
}

/// 对拍路径固定关闭默认 features（含 pyo3）：本机基线就是在该组合下录的，
/// 且不依赖本机 Python 运行时（`PYTHONHOME` 等）。
fn hashes_app_args() -> Vec<String> {
    // B7 Step 3：`js`（boa，纯 Rust，无外部运行时）进对拍 —— 不进 CI 就会腐烂。
    strings(&[
        "--no-default-features",
        "--features",
        "typst,js",
    ])
}

/// `cargo run` 的公共前缀（B9 Step 1）：**统一**走 `[profile.verify]`。
///
/// 抽出来是为了避免 `compile_bundle` / `render` 两处各自拼参数漂移
/// （与之前"两份 view 布局漂移"同类）。注意 `--profile` 与 `--release` 不能同用。
fn run_args(package: &str, bin: Option<&str>) -> Vec<String> {
    let mut args = strings(&["run", "-q", "--profile", "verify", "-p", package]);
    args.extend(hashes_app_args());
    if let Some(bin) = bin {
        args.extend(strings(&["--bin", bin]));
    }
    args.extend(strings(&["--"]));
    args
}

fn render(
    root: &Path,
    source: &[&str],
    frames: &str,
    size: &str,
    backend: Option<&str>,
) -> Result<String, String> {
    let mut args = run_args("zela-app", Some("zela-render"));
    args.extend(source.iter().map(|value| value.to_string()));
    args.extend(strings(&["--frames", frames, "--fps", "30", "--size", size]));
    // B8 Step 2：哈希是可选的 sink，对拍必须显式挂上（主流程默认不挂）。
    args.extend(strings(&["--hash"]));
    if let Some(backend) = backend {
        args.extend(strings(&["--backend", backend]));
    }

    // 清掉上一轮的帧，避免目录里残留过期 PNG。
    let frames_path = root.join(frames);
    if frames_path.exists() {
        remove_dir_all_robust(&frames_path)
            .map_err(|error| format!("清理 {} 失败：{error}", frames_path.display()))?;
    }

    println!("[xtask] $ cargo {}", args.join(" "));
    let output = cargo_command(root)?
        .args(&args)
        .output()
        .map_err(|error| format!("启动 cargo 失败：{error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        eprint!("{stdout}{stderr}");
        return Err(format!("渲染失败（{}）", source.join(" ")));
    }

    for line in stderr.lines().chain(stdout.lines()) {
        if line.starts_with("[zsb]")
            || line.starts_with("[frame-hash]")
            || line.starts_with("[decision]")
        {
            println!("[xtask] {line}");
        }
    }

    parse_frame_hash(&stderr)
        .or_else(|| parse_frame_hash(&stdout))
        .ok_or_else(|| "输出中没有找到 `[frame-hash] sequence = …`".to_string())
}

fn parse_frame_hash(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some(rest) = line.split("[frame-hash] sequence =").nth(1) else {
            continue;
        };
        // 行尾还有中文说明（“（N 帧，…）”），只取开头的十六进制串。
        let token: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if token.len() == 64 {
            return Some(token.to_ascii_lowercase());
        }
    }
    None
}

/// `baselines.txt` 的解析与重录改写已迁至 `lib.rs`（门禁级代码，`tests/` 配用例）。

// ─── 进程工具 ───────────────────────────────────────────────────────────────

fn cargo_program() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

// ─── 确定字体（B13 Step 0 / GAPS X1-11） ────────────────────────────────────

/// 确定字体的唯一真源：`build.ps1`（下载 + 校验）与本文件（对拍环境）都读它。
const FONT_MANIFEST: &str = "tools/typst-fonts.txt";
/// 与 `ze_prerender::typst::FONT_DIR_ENV` 同名。xtask 是独立 workspace（零依赖），
/// 不能依赖 `ze-prerender` ⇒ 这里重复的是**环境变量名**，不是路径 / 版本 / 哈希。
const FONT_DIR_ENV: &str = "ZELA_TYPST_FONT_DIR";

/// 解析 `tools/typst-fonts.txt` ⇒ `(版本, [(文件名, sha256)])`。
fn load_font_manifest(root: &Path) -> Result<(String, Vec<(String, String)>), String> {
    let path = root.join(FONT_MANIFEST);
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
    let mut version: Option<String> = None;
    let mut assets: Vec<(String, String)> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let name = parts.next().unwrap_or_default();
        let rest: Vec<&str> = parts.collect();
        if name == "version" {
            if rest.len() != 1 {
                return Err(format!("{} 的 version 行非法：{line}", path.display()));
            }
            version = Some(rest[0].to_string());
            continue;
        }
        if rest.len() != 1
            || rest[0].len() != 64
            || !rest[0].chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err(format!(
                "{} 的条目非法（期望 `<文件名> <64 位 sha256>`）：{line}",
                path.display()
            ));
        }
        assets.push((name.to_string(), rest[0].to_ascii_lowercase()));
    }
    let version = version.ok_or_else(|| format!("{} 缺少 version 行", path.display()))?;
    if assets.is_empty() {
        return Err(format!("{} 没有任何字体条目", path.display()));
    }
    Ok((version, assets))
}

/// 对拍子进程必须用的字体目录（缺失 ⇒ **显式报错**，不静默退回系统字体）。
///
/// 静默退回系统字体正是 X1-11 的成因：payload 用确定字体、帧哈希用系统字体，
/// 于是基线"看起来存在"却谁也复现不出来。
fn font_dir(root: &Path) -> Result<PathBuf, String> {
    let (version, assets) = load_font_manifest(root)?;
    let dir = root
        .join("target")
        .join("fonts")
        .join(format!("typst-assets-v{version}"));
    let missing: Vec<String> = assets
        .iter()
        .filter(|(name, _)| !dir.join(name).is_file())
        .map(|(name, _)| name.clone())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "确定字体缺失（{}）：{}\n  ⇒ 先运行 `pwsh crates/web/zela-web/build.ps1`\
             （它按 {FONT_MANIFEST} 下载并校验 SHA-256）",
            dir.display(),
            missing.join(" / "),
        ));
    }
    Ok(dir)
}

/// 所有对拍子进程的统一起点：`current_dir` + `ZELA_TYPST_FONT_DIR`。
///
/// **涉及对拍 / 渲染的子进程**不得直接 `Command::new(cargo_program())` —— 会漏掉字体注入，
/// 让含文本作品回到"随机器字体漂移"的状态。只读工具（如 `cargo metadata`，走
/// [`cargo_capture`]）不渲染任何像素，不受此限。
fn cargo_command(root: &Path) -> Result<Command, String> {
    let mut command = Command::new(cargo_program());
    command.current_dir(root).env(FONT_DIR_ENV, font_dir(root)?);
    Ok(command)
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn cargo(root: &Path, args: &[String]) -> Result<(), String> {
    println!("[xtask] $ cargo {}", args.join(" "));
    let status = cargo_command(root)?
        .args(args)
        .status()
        .map_err(|error| format!("启动 cargo 失败：{error}"))?;
    if !status.success() {
        return Err(format!(
            "`cargo {}` 失败（{status}）",
            args.first().map(String::as_str).unwrap_or("")
        ));
    }
    Ok(())
}

fn cargo_capture(root: &Path, args: &[String]) -> Result<Output, String> {
    println!("[xtask] $ cargo {}", args.join(" "));
    Command::new(cargo_program())
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|error| format!("启动 cargo 失败：{error}"))
}

// ─── step: fixtures（阶段 4：跨语言同一套 fixture） ──────────────────────────

/// 独立 fixture 驱动：Rust 内核 + JS / Lua 各跑同一套 JSON（表达式 + 输入 + 期望哈希）。
///
/// 解释器缺失时**显式打印跳过**（与 pyo3 缺失同一口径），不静默当作通过。
fn step_fixtures(root: &Path) -> Result<(), String> {
    let status = Command::new(cargo_program())
        .args(["test", "-p", "ze-func", "--test", "fixtures"])
        .current_dir(root)
        .status()
        .map_err(|error| format!("启动 cargo test 失败：{error}"))?;
    if !status.success() {
        return Err(format!("Rust fixture harness 失败（{status}）"));
    }

    let dir = root.join("crates/kernel/ze-func/tests/fixtures");
    let mut fixtures: Vec<std::path::PathBuf> = fs::read_dir(&dir)
        .map_err(|error| format!("读取 {} 失败：{error}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    fixtures.sort();
    if fixtures.is_empty() {
        return Err(format!("fixture 目录为空：{}", dir.display()));
    }
    for (program, script) in [("node", "run.mjs"), ("lua", "run.lua")] {
        println!("[xtask] fixtures: {program} {script}");
        let output = Command::new(program)
            .arg(dir.join(script))
            .args(&fixtures)
            .current_dir(root)
            .output();
        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stdout.is_empty() {
                    print!("{stdout}");
                }
                if !output.status.success() {
                    return Err(format!(
                        "{program} fixture runner 失败（{}）：\n{stderr}",
                        output.status
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                println!("[xtask] fixtures: 未找到 `{program}`，显式跳过（不静默当作通过）");
            }
            Err(error) => return Err(format!("启动 {program} 失败：{error}")),
        }
    }
    Ok(())
}
