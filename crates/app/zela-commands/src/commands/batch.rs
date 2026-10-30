//! `batch`：**批量渲染 + 对拍**（门禁用；也是批量出片的一等能力）。
//!
//! ```text
//! 输入  一组 (作品脚本, 尺寸, 后端, fps)
//! 输出  每组的 { payload hash, 像素序列 hash, 结构摘要 } + 可选观测产物
//! ```
//!
//! # 为什么在这一层
//!
//! 它同时服务两件事：门禁对拍（`zela-verify`）与**批量出片**（未来的
//! `zela render --batch` / 编辑器）。所以帧体放在 `zela-commands`（宿主无关），
//! 二进制只剩参数解析与退出码。
//!
//! # 一次 GPU 初始化
//!
//! `RenderContext` 只建一次并复用到所有作品；每件作品各自新建 runtime 与后端
//! （新 World 是必须的）。逐作品的进程启动开销（~0.5s GPU 初始化 + ~0.7s 构帧准备）
//! 因此被摊平。
//!
//! # 与单进程路径的关系（不得漂移）
//!
//! 帧循环**只此一份**（[`zela_app::frame_loop`]），与 `zela-render` 共用；
//! 本模块只是"同一循环 + 一个哈希回调"，不重写渲染代码。
//! `xtask` 保留 `--independent` 开关走旧的独立进程路径 —— 两者必须逐位相同（B9 §4.3）。
//!
//! # 输出（机器可读，一行一个作品）
//!
//! ```text
//! [decision] {"work":"fourier","pass":"script","backend":"Wgpu","reason":"…","rejected":[]}
//! [verify] {"work":"fourier","size":"640x480","frames":216,"script":"<64 hex>","bundle":"<64 hex>","payload":"<64 hex>"}
//! ```
//!
//! 失败时**指出**作品与路径（`work · script|bundle`），不报"批量失败"。

use std::path::{Path, PathBuf};

use zela_app::frame_loop::{
    FrameLoopOptions, FrameRun, FrameStats, FrameTiming, LoopBackend, TIMING_WINDOW, accumulate,
    frames_for, run_frames, summary_body, timing_json,
};
use zela_app::{
    ScriptLanguage, build_bundle_runtime, build_script_runtime, compile_script, language_from_path,
};
use zela_render_backends::{HeadlessTarget, RenderContext, RoutingDecision, StandardBackends};
use zela_scene_bundle::frame_hash::FrameSequenceHash;
use zela_scene_bundle::{Bundle, MetaInput};

/// 一个待渲染 / 待对拍的作品。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub backend: LoopBackend,
}

/// 一次批量渲染的输入。
#[derive(Debug, Clone)]
pub struct BatchOptions {
    /// 作品脚本目录（`--scripts`）。
    pub scripts: PathBuf,
    /// 中间产物目录（ZSB 与 sink-bypass 的 PNG）。
    pub tmp: PathBuf,
    pub fps: f64,
    pub works: Vec<Work>,
}

/// 一件作品的产出（结构化；CLI 只负责格式化）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkOutcome {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub script_hash: String,
    pub bundle_hash: String,
    pub payload_hash: String,
}

/// 批量渲染（串行版）：逐作品编译 → script 路径 → bundle 路径 → 结构等式比对。
pub fn run_batch(ctx: &RenderContext, options: &BatchOptions) -> Result<Vec<WorkOutcome>, String> {
    std::fs::create_dir_all(&options.tmp)
        .map_err(|e| format!("创建目录 {} 失败：{e}", options.tmp.display()))?;
    let env = zela_app::frame_loop::environment_json(ctx.adapter());
    let mut outcomes = Vec::with_capacity(options.works.len());
    for work in &options.works {
        outcomes.push(run_work(ctx, options, work, &env)?);
    }
    Ok(outcomes)
}

/// 单件作品：编译 → 两条路径 → 结构比对 → 打印 `[verify]` 行。
fn run_work(
    ctx: &RenderContext,
    options: &BatchOptions,
    work: &Work,
    env: &str,
) -> Result<WorkOutcome, String> {
    let (script_path, language) = locate_script(&options.scripts, &work.name)?;
    let source = std::fs::read_to_string(&script_path)
        .map_err(|e| format!("[{}] 读取脚本 {} 失败：{e}", work.name, script_path.display()))?;

    // ── 编译（也在同一进程里，避免逐作品 `zela compile` 的启动开销）──
    let payload = compile_and_write(
        &work.name,
        &source,
        language,
        &options.tmp.join(format!("{}.zsb", work.name)),
    )?;

    // ── 路径 1：脚本 ──
    let mut runtime = build_script_runtime(ctx.clone(), source, language)
        .map_err(|e| format!("[{}] 脚本装配失败：{e}", work.name))?;
    let script_hash = render_hash(ctx, &mut runtime, work, options.fps, "script", env)?;
    drop(runtime);

    // ── 路径 2：同一份 ZSB ──
    let bytes = std::fs::read(options.tmp.join(format!("{}.zsb", work.name)))
        .map_err(|e| format!("[{}] 读取 ZSB 失败：{e}", work.name))?;
    let bundle =
        Bundle::from_bytes(&bytes).map_err(|e| format!("[{}] ZSB 解析失败：{e}", work.name))?;
    let mut runtime = build_bundle_runtime(ctx.clone(), &bundle)
        .map_err(|e| format!("[{}] bundle 装配失败：{e}", work.name))?;
    let bundle_hash = render_hash(ctx, &mut runtime, work, options.fps, "bundle", env)?;

    // 阶段 1：脚本 / bundle 两条路径的**结构**必须逐位相同（不只像素）。
    assert_structure_matches(&work.name, &script_hash.1, &bundle_hash.1)?;

    let frames = frames_for(&runtime, options.fps);
    println!(
        "[verify] {{\"work\":\"{}\",\"size\":\"{}x{}\",\"frames\":{},\"script\":\"{}\",\
         \"bundle\":\"{}\",\"payload\":\"{}\"}}",
        work.name, work.width, work.height, frames, script_hash.0, bundle_hash.0, payload,
    );
    Ok(WorkOutcome {
        name: work.name.clone(),
        width: work.width,
        height: work.height,
        frames,
        script_hash: script_hash.0,
        bundle_hash: bundle_hash.0,
        payload_hash: payload,
    })
}

/// 脚本 → ZSB（与 `zela-compile` 同一装配路径），返回 payload hash。
fn compile_and_write(
    name: &str,
    source: &str,
    language: ScriptLanguage,
    out: &Path,
) -> Result<String, String> {
    let compiled = compile_script(source, language)
        .map_err(|e| format!("[{name}] 编译失败：{e}"))?;
    let bundle = Bundle::encode(
        &compiled.script,
        MetaInput {
            capabilities: compiled.capabilities.clone(),
            required_capabilities: compiled.required_capabilities.clone(),
            pods: compiled.pods.clone(),
            required_pods: compiled.required_pods.clone(),
            downgrades: Vec::new(),
            source_sha256: Some(compiled.source_sha256.clone()),
            render_params: Default::default(),
        },
    )
    .map_err(|e| format!("[{name}] 编码 ZSB 失败：{e}"))?;
    std::fs::write(out, bundle.to_bytes())
        .map_err(|e| format!("[{name}] 写 {} 失败：{e}", out.display()))?;
    Ok(bundle.payload_hash_hex())
}

/// 按 `.lua` / `.py` / `.js` 顺序探测作品脚本（B7 Step 0.1：不再硬编码扩展名）。
///
/// B7 Step 3：作品名**自带扩展名**时按显式文件名取（`surface_matrix.js` 与
/// `surface_matrix.lua` 并存，探测顺序选不出后者）。
///
/// 语义不变：**找不到 ⇒ 显式报错并列出试过的扩展名**（不静默跳过）。
/// 语言判断复用 [`language_from_path`]，不另写一套。
fn locate_script(dir: &Path, name: &str) -> Result<(PathBuf, ScriptLanguage), String> {
    if let Some(extension) = Path::new(name).extension().and_then(|ext| ext.to_str()) {
        if matches!(extension, "lua" | "py" | "js") {
            let path = dir.join(name);
            if path.is_file() {
                let language = language_from_path(&path).map_err(|e| e.to_string())?;
                return Ok((path, language));
            }
            return Err(format!(
                "[{name}] 显式指定了扩展名，但 {} 不存在",
                path.display()
            ));
        }
    }
    let mut tried = Vec::new();
    for ext in ["lua", "py", "js"] {
        let path = dir.join(format!("{name}.{ext}"));
        if path.is_file() {
            let language = language_from_path(&path).map_err(|e| e.to_string())?;
            return Ok((path, language));
        }
        tried.push(ext);
    }
    Err(format!(
        "[{name}] 找不到脚本：{} 下没有 .{} 中的任何一个",
        dir.display(),
        tried.join(" / .")
    ))
}

/// **V3-12 检查**（并入本进程，B7 Step 0.3）：挂 / 不挂哈希时的像素产出必须逐位相同。
///
/// 比 PNG **字节**而不是序列哈希 —— 不挂哈希时本来就没有哈希值可拿，比它是自证。
pub fn run_sink_bypass(
    ctx: &RenderContext,
    scripts: &Path,
    tmp: &Path,
    fps: f64,
    env: &str,
) -> Result<(), String> {
    let work = Work {
        name: "vector_paths".to_string(),
        width: 640,
        height: 480,
        backend: LoopBackend::Vector,
    };
    let (script_path, language) = locate_script(scripts, &work.name)?;
    let source = std::fs::read_to_string(&script_path)
        .map_err(|e| format!("读取脚本 {} 失败：{e}", script_path.display()))?;
    let root = tmp.join("sink-bypass");
    let mut sizes = Vec::new();
    for (name, hash) in [("with-hash", true), ("no-hash", false)] {
        let dir = root.join(name);
        sizes.push(render_png(ctx, &source, language, &work, fps, &dir, hash, env)?.len());
    }
    if sizes[0] != sizes[1] {
        return Err(format!(
            "V3-12 失败：挂 / 不挂 HashSink 的 PNG 字节不同（{} B vs {} B）",
            sizes[0], sizes[1]
        ));
    }
    // 比字节本身：只比长度不够，读回来再比一次。
    let frame = "frame_00000.png";
    let a = std::fs::read(root.join("with-hash").join(frame))
        .map_err(|e| format!("读取 with-hash 帧失败：{e}"))?;
    let b = std::fs::read(root.join("no-hash").join(frame))
        .map_err(|e| format!("读取 no-hash 帧失败：{e}"))?;
    if a != b {
        return Err("V3-12 失败：挂 / 不挂 HashSink 的 PNG 字节不同".to_string());
    }
    println!(
        "[verify] {{\"sink_bypass\":\"ok\",\"bytes\":{}}}",
        a.len()
    );
    Ok(())
}

/// B12 Step 2：路由决策落盘（每作品每遍一行，字段与 `zela-render` 一致）。
///
/// 拒绝理由非空由 `run_frames` 内的 `validate()` 保证 —— 这里不再重复判断。
fn report_decisions(work: &str, pass: &str, decisions: &[RoutingDecision]) {
    for decision in decisions {
        println!("{}", decision.json(work, pass));
    }
}

/// B12 Step 3：**结构化落盘**（计时 + 决策 + 环境指纹，一行一个作品/一遍）。
///
/// 目标 = `ZELA_FRAME_REPORT=<path>`（JSONL，追加）。不设 ⇒ 完全不写（门禁无副作用）。
/// 环境指纹由调用方预先算好（`frame_loop::environment_json`）且**必填**：
/// 没有 GPU / profile / 架构的计时数字不可比（B8 的 dev-release 与 X1-11 是同一类教训）。
///
/// **执行计划阶段 3**：`ZELA_OBSERVE=<path>` 时同一份数据再写一行**统一观测产物**
/// （`schema = zela.observation/1`，`kind = frame`）。旧格式保持不变（已有工具在读）。
fn report_work(work: &Work, pass: &str, frames: u32, env: &str, run: &FrameRun) {
    let frame_path = report_path("ZELA_FRAME_REPORT");
    let observe_path = report_path("ZELA_OBSERVE");
    if frame_path.is_none() && observe_path.is_none() {
        return;
    }
    let (all, all_n) = accumulate(&run.timings, false, None);
    let (window, window_n) = accumulate(&run.timings, true, Some(TIMING_WINDOW));
    let decisions: Vec<String> = run
        .decisions
        .iter()
        .map(|decision| decision.object_json(&work.name, pass))
        .collect();
    // B17 Step 0：资源曲线（逐帧增量的首 / 末 / 峰值）。修好后每帧应为 0，曲线持平。
    let (first, last) = (
        run.resource_frames.first().copied().unwrap_or_default(),
        run.resource_frames.last().copied().unwrap_or_default(),
    );
    let peak = |f: fn(&zela_render_vector::ResourceTotals) -> usize| {
        run.resource_frames.iter().map(f).max().unwrap_or(0)
    };
    // **AW-07（E-G3）**：几何数组逐帧上传字节（首帧 / 稳态总和 / 峰值）。
    let upload_first = run.store_uploads.first().copied().unwrap_or(0);
    let upload_steady: usize = run.store_uploads.iter().skip(1).sum();
    let upload_max = run.store_uploads.iter().copied().max().unwrap_or(0);
    let upload = format!(
        "{{\"frames\":{},\"first\":{upload_first},\"steady\":{upload_steady},\"max\":{upload_max}}}",
        run.store_uploads.len()
    );
    let all_body = summary_body(&all, "all", all_n);
    let window_body = summary_body(&window, &format!("1..{window_n}"), window_n);
    let gpu = run.gpu_json();
    let cache = run.cache.json();

    if let Some(path) = &frame_path {
        let line = format!(
            "{{\"work\":\"{}\",\"pass\":\"{pass}\",\"size\":\"{}x{}\",\"frames\":{frames},\
             \"env\":{env},\"timings\":{{\"all\":{all_body},\"window\":{window_body}}},\"gpu\":{gpu},\
             \"cache\":{cache},\"textures_created\":{},\"textures_created_scope\":\"wgpu-only\",\
             \"vector_resources\":{},\"vector_resource_curve\":{{\"frames\":{},\"first\":{},\"last\":{},\
             \"max_textures\":{},\"max_texture_bytes\":{},\"max_buffers\":{},\"max_buffer_bytes\":{}}},\
             \"geometry_upload\":{upload},\
             \"vector_overflow\":{},\"vector_shadows\":{},\
             \"verify_note\":\"verify_ms 未启用（GPU 校验 pass 未实现，B12 Step 1.3）\",\
             \"decisions\":[{}]}}",
            work.name,
            work.width,
            work.height,
            run.textures_created,
            run.vector_resources.json(),
            run.resource_frames.len(),
            first.json(),
            last.json(),
            peak(|r| r.textures),
            peak(|r| r.texture_bytes),
            peak(|r| r.buffers),
            peak(|r| r.buffer_bytes),
            run.vector_overflow.json(),
            run.vector_shadows.json(),
            decisions.join(","),
        );
        append_jsonl(path, &line);
    }

    if let Some(path) = &observe_path {
        let data = format!(
            "{{\"timings\":{{\"all\":{all_body},\"window\":{window_body}}},\"gpu\":{gpu},\
             \"cache\":{cache},\"textures_created\":{},\"vector_resources\":{},\
             \"geometry_upload\":{upload},\
             \"vector_overflow\":{},\"vector_shadows\":{},\"decisions\":[{}]}}",
            run.textures_created,
            run.vector_resources.json(),
            run.vector_overflow.json(),
            run.vector_shadows.json(),
            decisions.join(","),
        );
        append_jsonl(
            path,
            &observation_line("frame", work, pass, env, &data),
        );
    }
}

/// **结构指纹报告**（执行计划阶段 1）：门禁 stdout 一行 + 可选统一产物一行。
///
/// stdout 行是**等式比对器**的输入（xtask 解析后与 `structures.txt` 比对）；
/// 摘要按「逐帧摘要再摘要」汇总，帧仍可反推（`ZELA_OBSERVE` 里带逐帧值）。
fn report_structure(work: &Work, pass: &str, env: &str, run: &FrameRun) {
    let Some(last) = run.structure.last() else {
        return;
    };
    let digest = zela_scene_ecs::sequence_digest(&run.structure);
    let backend = match work.backend {
        LoopBackend::Wgpu => "wgpu",
        LoopBackend::Vector => "vector",
        LoopBackend::Auto => "auto",
        LoopBackend::AutoFrame => "auto-frame",
    };
    println!(
        "[structure] {{\"work\":\"{}\",\"backend\":\"{backend}\",\"pass\":\"{pass}\",\
         \"frames\":{},\"objects\":{},\"fields\":{},\"derived_excluded\":{},\
         \"digest\":\"{digest:016x}\"}}",
        work.name,
        run.structure.len(),
        last.objects,
        last.fields,
        last.derived_excluded,
    );
    let Some(path) = report_path("ZELA_OBSERVE") else {
        return;
    };
    let frames = run
        .structure
        .iter()
        .map(|frame| format!("\"{:016x}\"", frame.digest))
        .collect::<Vec<_>>()
        .join(",");
    let data = format!(
        "{{\"objects\":{},\"fields\":{},\"derived_excluded\":{},\"digest\":\"{digest:016x}\",\
         \"frames\":[{frames}]}}",
        last.objects, last.fields, last.derived_excluded,
    );
    append_jsonl(&path, &observation_line("structure", work, pass, env, &data));
}

/// 统一观测产物的**信封**（阶段 3）：`schema / kind / work / pass / size / frames / env / data`。
fn observation_line(kind: &str, work: &Work, pass: &str, env: &str, data: &str) -> String {
    format!(
        "{{\"schema\":\"zela.observation/1\",\"kind\":\"{kind}\",\"work\":\"{}\",\"pass\":\"{pass}\",\
         \"size\":\"{}x{}\",\"env\":{env},\"data\":{data}}}",
        work.name, work.width, work.height,
    )
}

/// 读取一个"报告路径"环境变量；未设置 / 空 ⇒ `None`（门禁无副作用）。
fn report_path(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|path| !path.trim().is_empty())
}

fn append_jsonl(path: &str, line: &str) {
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(mut file) => {
            use std::io::Write;
            if let Err(error) = writeln!(file, "{line}") {
                eprintln!("[report] 写 {path} 失败：{error}");
            }
        }
        Err(error) => eprintln!("[report] 打开 {path} 失败：{error}"),
    }
}

/// B12 Step 1.2：`zela-verify` 是**唯一默认门禁路径**，必须与 `zela-render` 同字段计时。
///
/// 默认不打印（门禁输出要保持可读）；`ZELA_FRAME_TIMING=1` 时逐帧 `[timing]` + 每遍
/// `[timing-summary]`，字段名与 `zela-render` 完全一致。
fn report_timings(work: &str, pass: &str, timings: &[FrameTiming]) -> Result<(), String> {
    let print = std::env::var("ZELA_FRAME_TIMING").is_ok_and(|value| value != "0");
    let mut stats = FrameStats::new();
    for (index, timing) in timings.iter().enumerate() {
        if print {
            eprintln!("{}", timing_json(timing, index as u32));
        }
        stats.record(index as u32, timing);
    }
    if print {
        eprintln!("[timing-work] {work} · {pass}");
        if let Err(error) = stats.report() {
            eprintln!("[timing] {error}");
        }
    }
    // B12 Step 6：**量级门禁恒开**（单帧 > 500ms ⇒ 退出码非 0）；打印才是可选的。
    stats.check().map_err(|error| format!("[{work}] {pass}：{error}"))
}

/// 渲一个作品并把 PNG 写进 `dir`；`hash` 决定是否同时累积序列哈希。
fn render_png(
    ctx: &RenderContext,
    source: &str,
    language: ScriptLanguage,
    work: &Work,
    fps: f64,
    dir: &Path,
    hash: bool,
    env: &str,
) -> Result<Vec<u8>, String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("创建目录 {} 失败：{e}", dir.display()))?;
    let mut runtime = build_script_runtime(ctx.clone(), source, language)
        .map_err(|e| format!("[{}] 装配失败：{e}", work.name))?;
    let frames = frames_for(&runtime, fps);
    let mut backends = StandardBackends::new(ctx);
    let mut target = HeadlessTarget::new();
    let mut sequence = FrameSequenceHash::new(work.width, work.height);
    let mut first = Vec::new();
    let options = FrameLoopOptions {
        width: work.width,
        height: work.height,
        fps,
        backend: work.backend,
        start_frame: 0,
        end_frame: None,
        vector_params: None,
    };
    let run = run_frames(
        ctx,
        &mut runtime,
        &mut backends,
        &mut target,
        options,
        true,
        &mut |index, frame| {
            // 只写第 0 帧：V3-12 只比较首帧字节，旧实现每帧编码 + 写盘，
            // 899/900 次结果无人读取（执行计划 §1.2）。编码参数与 `PngSink` 完全相同。
            if index == 0 {
                let path = dir.join(format!("frame_{index:05}.png"));
                zela_app::export::write_png(&path, work.width, work.height, frame.rgba)
                    .map_err(|e| e.to_string())?;
                first = std::fs::read(&path)
                    .map_err(|e| format!("读回 {} 失败：{e}", path.display()))?;
            }
            if hash {
                sequence.push_frame(frame.rgba);
            }
            Ok(std::time::Duration::ZERO)
        },
    )
    .map_err(|e| format!("[{}] 渲染失败：{e}", work.name))?;
    report_decisions(&work.name, "png", &run.decisions);
    report_timings(&work.name, "png", &run.timings)?;
    report_work(work, "png", frames, env, &run);
    report_structure(work, "png", env, &run);
    Ok(first)
}

/// 一个作品的一遍渲染 → `(序列哈希, 逐帧结构摘要)`。
///
/// **每个作品新建 `StandardBackends`**（§4.2 的安全取舍）：`wgpu_3d` 的离屏 / 深度目标
/// 按 `LayerId` 索引，复用后端可能让残留目标串味。GPU 初始化（大头）已在外面摊平。
fn render_hash(
    ctx: &RenderContext,
    runtime: &mut zela_playback::Runtime,
    work: &Work,
    fps: f64,
    path: &str,
    env: &str,
) -> Result<(String, Vec<u64>), String> {
    let mut backends = StandardBackends::new(ctx);
    let mut target = HeadlessTarget::new();
    let mut hash = FrameSequenceHash::new(work.width, work.height);
    let frames = frames_for(runtime, fps);
    let options = FrameLoopOptions {
        width: work.width,
        height: work.height,
        fps,
        backend: work.backend,
        start_frame: 0,
        end_frame: None,
        vector_params: None,
    };
    let run = run_frames(
        ctx,
        runtime,
        &mut backends,
        &mut target,
        options,
        true,
        &mut |_index, frame| {
            // 与 `zela-render --hash` 完全相同的输入与口径。
            hash.push_frame(frame.rgba);
            Ok(std::time::Duration::ZERO)
        },
    )
    .map_err(|e| format!("[{}] {path} 渲染失败：{e}", work.name))?;
    report_decisions(&work.name, path, &run.decisions);
    report_timings(&work.name, path, &run.timings)?;
    report_work(work, path, frames, env, &run);
    report_structure(work, path, env, &run);
    Ok((
        hash.hex(),
        run.structure.iter().map(|frame| frame.digest).collect(),
    ))
}

/// **等式比对器（进程内）**：脚本 / bundle 两条路径的逐帧结构必须完全相同。
///
/// 与像素哈希的区别：这里是 `state = f(t)` 的等式，不含 GPU / 字体 / 编码器；
/// 失败时**点名第一个分歧帧**（旧像素链式哈希做不到）。
fn assert_structure_matches(work: &str, script: &[u64], bundle: &[u64]) -> Result<(), String> {
    if script.len() != bundle.len() {
        return Err(format!(
            "[{work}] 结构指纹帧数不同：script {} / bundle {}",
            script.len(),
            bundle.len()
        ));
    }
    for (index, (a, b)) in script.iter().zip(bundle.iter()).enumerate() {
        if a != b {
            return Err(format!(
                "[{work}] 结构指纹在**第 {index} 帧**分歧：script {a:016x} / bundle {b:016x}\n  \
                 该帧状态不同 ⇒ 优先查表达式绑定 / 装配顺序，而不是渲染"
            ));
        }
    }
    Ok(())
}

/// **能力自测**（不依赖 GPU / 不依赖门禁主流程）：脚本定位的三种语义。
///
/// 定位错 ⇒ 找错作品或静默跳过，是批量渲染最危险的一类错误，所以单独守。
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zela-batch-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("创建临时目录");
        dir
    }

    #[test]
    fn locate_script_prefers_explicit_extension() {
        let dir = temp_dir("explicit");
        std::fs::write(dir.join("surface_matrix.lua"), "-- lua\n").unwrap();
        std::fs::write(dir.join("surface_matrix.js"), "// js\n").unwrap();

        // 名字自带扩展名 ⇒ 按显式文件名取（不再按 .lua/.py/.js 顺序探测）。
        let (path, language) = locate_script(&dir, "surface_matrix.js").expect("显式扩展名");
        assert!(path.ends_with("surface_matrix.js"));
        assert_eq!(language, ScriptLanguage::JavaScript);

        let (path, language) = locate_script(&dir, "surface_matrix").expect("按顺序探测");
        assert!(path.ends_with("surface_matrix.lua"), "探测顺序仍以 .lua 优先");
        assert_eq!(language, ScriptLanguage::Lua);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn locate_script_fails_explicitly_when_missing() {
        let dir = temp_dir("missing");
        let error = locate_script(&dir, "nope").expect_err("缺失必须报错");
        assert!(error.contains("找不到脚本"), "错误必须说明找不到：{error}");
        assert!(error.contains("lua"), "错误必须列出试过的扩展名：{error}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
