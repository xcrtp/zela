//! `compile`：脚本 → ZSB 装配产物（不渲染、不需要 GPU）。
//!
//! 帧体搬自 `zela-compile` bin；差别只在诊断走 [`CommandHost`]、返回 [`Response`]。

use std::path::PathBuf;

use zela_app::{ScriptLanguage, compile_script, language_from_path};
use zela_scene_bundle::{Bundle, MetaInput};

use crate::host::{CommandHost, Level};
use crate::response::{Fact, Response};
use crate::spec::{ArgSpec, ArgType, CommandArgs, CommandSpec};

pub const SPEC: CommandSpec = CommandSpec {
    name: "compile",
    about: "把脚本编译成 ZSB 装配产物（.zsb）",
    args: &[
        ArgSpec::positional("script", ArgType::Path, "脚本路径（.lua / .py / .js）"),
        ArgSpec::required("out", ArgType::Path, "输出的 .zsb 路径"),
        ArgSpec::option("json", ArgType::Path, "", "额外写出 payload 明文（调试 diff）"),
    ],
};

/// 编译请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOptions {
    pub script: PathBuf,
    pub out: PathBuf,
    pub json: Option<PathBuf>,
}

impl CompileOptions {
    pub fn from_args(args: &CommandArgs) -> Result<Self, String> {
        Ok(Self {
            script: args.path("script").ok_or("缺少脚本路径")?,
            out: args.path("out").ok_or("缺少 --out")?,
            json: args.path("json").filter(|path| !path.as_os_str().is_empty()),
        })
    }
}

/// 编译结果（结构化；CLI 只负责格式化）。
pub struct Compiled {
    pub bundle: Bundle,
    pub still_time: Option<f64>,
}

/// 只做装配（不写盘）——`compile` 与 `check` 共用。
pub fn compile(source: &str, language: ScriptLanguage) -> Result<Compiled, String> {
    let compiled = compile_script(source, language).map_err(|e| e.to_string())?;
    let bundle = Bundle::encode(
        &compiled.script,
        MetaInput {
            capabilities: compiled.capabilities.clone(),
            required_capabilities: compiled.required_capabilities.clone(),
            pods: compiled.pods.clone(),
            required_pods: compiled.required_pods.clone(),
            // 当前脚本不使用 Shape2D::Custom ⇒ 无降级记录。
            downgrades: Vec::new(),
            source_sha256: Some(compiled.source_sha256.clone()),
            // 编译期不猜画质（档 1 默认值 = 当前行为）；回放端可覆盖。
            render_params: Default::default(),
        },
    )
    .map_err(|e| format!("编码 ZSB 失败：{e}"))?;
    Ok(Compiled {
        still_time: compiled.script.still_time(),
        bundle,
    })
}

/// 读脚本并按扩展名推断语言（`.py` 未开 feature ⇒ 明确报错）。
pub fn read_script(script: &std::path::Path) -> Result<(String, ScriptLanguage), String> {
    let source = std::fs::read_to_string(script)
        .map_err(|e| format!("读取脚本 {} 失败：{e}", script.display()))?;
    let language = language_from_path(script).map_err(|e| e.to_string())?;
    if language == ScriptLanguage::Python && !cfg!(feature = "python") {
        return Err(
            "脚本是 .py，但本二进制编译时未启用 `python` feature（请重新构建或改用 .lua）"
                .to_string(),
        );
    }
    if language == ScriptLanguage::JavaScript && !cfg!(feature = "js") {
        return Err(
            "脚本是 .js，但本二进制编译时未启用 `js` feature（请重新构建或改用 .lua）"
                .to_string(),
        );
    }
    Ok((source, language))
}

/// 批量编译的一件任务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileJob {
    /// 输出名（写 `<out_dir>/<id>.zsb`），也是 `[payload]` 行的键。
    pub id: String,
    pub script: PathBuf,
}

/// **批量编译**：一个进程编完一组脚本（门禁 demo 步骤与批量出片共用）。
///
/// # 省的是什么
///
/// 编译本身是 CPU 绑定、单件 0.2–0.5s；一件一个 `cargo run` 时，每件还要付
/// cargo 解析 + 进程启动 + 装配期字体探测。34 件在一处编完，这笔开销只付一次。
/// 因此这里是**串行**：并行的收益远小于多进程编译时的内存与诊断串行化代价。
///
/// # 输出（机器可读，一行一件）
///
/// ```text
/// [payload] {"id":"fourier","payload":"<64 hex>"}
/// ```
///
/// # 失败语义
///
/// 与 [`crate::commands::batch::run_batch`] 一致：单件失败不影响其余，全部跑完后
/// 汇总，错误逐件指出 id 与脚本路径。
pub fn run_many(out_dir: &std::path::Path, jobs: &[CompileJob]) -> Result<Vec<(String, String)>, String> {
    let _ = crate::commands::fonts::ensure_font_dir();
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("创建目录 {} 失败：{e}", out_dir.display()))?;
    let mut done = Vec::with_capacity(jobs.len());
    let mut failures = Vec::new();
    for job in jobs {
        let out = out_dir.join(format!("{}.zsb", job.id));
        match compile_one(&job.script, &out) {
            Ok(hash) => {
                println!("[payload] {{\"id\":\"{}\",\"payload\":\"{hash}\"}}", job.id);
                done.push((job.id.clone(), hash));
            }
            Err(error) => failures.push(format!("{}（{}）：{error}", job.id, job.script.display())),
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "{} / {} 件编译失败（其余作品的产出已打印，可先行排查）：\n  {}",
            failures.len(),
            jobs.len(),
            failures.join("\n  ")
        ));
    }
    Ok(done)
}

/// 单件：读脚本 → 装配 → 写 ZSB，返回 payload hash。
fn compile_one(script: &std::path::Path, out: &std::path::Path) -> Result<String, String> {
    let (source, language) = read_script(script)?;
    let compiled = compile(&source, language)?;
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录 {} 失败：{e}", parent.display()))?;
        }
    }
    std::fs::write(out, compiled.bundle.to_bytes())
        .map_err(|e| format!("写 {} 失败：{e}", out.display()))?;
    Ok(compiled.bundle.payload_hash_hex().to_string())
}

/// 执行编译并写盘。
pub fn run(host: &dyn CommandHost, options: &CompileOptions) -> Result<Response, String> {
    // 装配期可能用到 Typst（content.text）⇒ 先保证确定字体，避免静默回退。
    let _ = crate::commands::fonts::ensure_font_dir();
    let (source, language) = read_script(&options.script)?;
    let compiled = compile(&source, language)?;
    let bytes = compiled.bundle.to_bytes();
    if let Some(parent) = options.out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录 {} 失败：{e}", parent.display()))?;
        }
    }
    std::fs::write(&options.out, &bytes)
        .map_err(|e| format!("写 {} 失败：{e}", options.out.display()))?;
    if let Some(json_path) = &options.json {
        std::fs::write(json_path, compiled.bundle.payload())
            .map_err(|e| format!("写 {} 失败：{e}", json_path.display()))?;
    }

    let meta = compiled.bundle.meta();
    // 逐件编译的既有输出（`build.ps1` 与手工调用按这一行看结果）；门禁的 demo 步骤改走
    // [`run_many`] 的 `[payload]` 行，两者都由同一个 `payload_hash_hex` 产出。
    host.diagnostic(
        Level::Info,
        &format!("payload hash = {}", compiled.bundle.payload_hash_hex()),
    );
    host.diagnostic(
        Level::Info,
        &format!(
            "[compile] {} → {}（{} 个对象，静止时刻 {}）",
            options.script.display(),
            options.out.display(),
            meta.object_count,
            compiled.still_time.map_or_else(
                || "不可推导".to_string(),
                |value| format!("≈ {value:.3}s")
            )
        ),
    );

    Ok(Response::facts(
        "compile",
        vec![
            (
                "payload_hash".to_string(),
                Fact::Text(compiled.bundle.payload_hash_hex().to_string()),
            ),
            ("objects".to_string(), Fact::Count(meta.object_count as u64)),
            (
                "capabilities".to_string(),
                Fact::Text(
                    meta.capabilities
                        .iter()
                        .map(|c| format!("{}@{:?}", c.name, c.version))
                        .collect::<Vec<_>>()
                        .join(","),
                ),
            ),
            (
                "pods".to_string(),
                Fact::Text(
                    meta.pods
                        .iter()
                        .map(|p| format!("{}@{}", p.name, p.version))
                        .collect::<Vec<_>>()
                        .join(","),
                ),
            ),
            ("out".to_string(), Fact::Text(options.out.display().to_string())),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<CommandArgs, String> {
        let raw: Vec<String> = values.iter().map(|value| value.to_string()).collect();
        SPEC.parse(&raw)
    }

    /// 阶段 0 回归网：`--json` 默认不写、必填项缺失必须显式失败。
    #[test]
    fn defaults_and_required_values_match_legacy() {
        let options =
            CompileOptions::from_args(&parse(&["a.lua", "--out", "a.zsb"]).expect("可解析"))
                .expect("可构造");
        assert_eq!(options.json, None);
        assert_eq!(options.script, PathBuf::from("a.lua"));
        assert_eq!(options.out, PathBuf::from("a.zsb"));

        let missing_out = CompileOptions::from_args(&CommandArgs::default());
        assert!(missing_out.is_err(), "空参数必须报缺脚本");
    }
}
