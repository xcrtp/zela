//! `zela-verify`：**批量**对拍入口（B9 Step 3）。
//!
//! ```text
//! zela-verify --scripts DIR --tmp DIR --work name=WxH[:vector] [--work ...]
//! ```
//!
//! # 为什么需要它
//!
//! `cargo verify --step hashes` 原先每个作品要付两次进程启动（每次 ~0.5s GPU 初始化
//! + ~0.7s 运行时构帧准备）。19 次进程调用 ≈ 46s，与"渲染本身"同量级。
//! 本二进制把 GPU 初始化压成**一次**，每个作品只重建自己的 Runtime（新 World 是必须的）。
//!
//! # 与单进程路径的关系（不得漂移）
//!
//! 帧循环**只此一份**（[`zela_app::frame_loop`]），与 `zela-render` 共用；
//! 本二进制只是"同一循环 + 一个哈希回调"，不重写渲染代码。
//! `xtask` 保留 `--independent` 开关走旧的独立进程路径 —— 两者必须逐位相同（B9 §4.3）。
//!
//! # 输出（机器可读，一行一个作品）
//!
//! ```text
//! [decision] {"work":"fourier","pass":"script","backend":"Wgpu","reason":"…","rejected":[]}
//! [verify] {"work":"fourier","size":"640x480","frames":216,"script":"<64 hex>","bundle":"<64 hex>","payload":"<64 hex>"}
//! ```
//!
//! `[decision]` 是 B12 Step 2 的决策日志：B13 对拍出现差异时，第一现场就是它。
//!
//! 失败时**点名**作品与路径（`work · script|bundle`），不报"批量失败"。

use std::path::{Path, PathBuf};

use zela_app::frame_loop::{
    FrameLoopOptions, FrameRun, FrameStats, FrameTiming, LoopBackend, TIMING_WINDOW, accumulate,
    environment_json, frames_for, run_frames, summary_body, timing_json,
};
use zela_app::{
    ScriptLanguage, build_bundle_runtime, build_script_runtime, compile_script, language_from_path,
};
use zela_render_backends::{HeadlessTarget, RenderContext, RoutingDecision, StandardBackends};
use zela_scene_bundle::frame_hash::FrameSequenceHash;
use zela_scene_bundle::{Bundle, MetaInput};

/// 一个待对拍的作品。
#[derive(Debug)]
struct Work {
    name: String,
    width: u32,
    height: u32,
    backend: LoopBackend,
}

#[derive(Debug)]
struct Args {
    scripts: PathBuf,
    tmp: PathBuf,
    fps: f64,
    works: Vec<Work>,
    /// B7 Step 0.3：把 V3-12（挂/不挂 HashSink 的像素一致性）并入本进程。
    sink_bypass: bool,
}

impl Args {
    /// 纯解析入口（默认值快照的被测点）。
    fn parse_from(raw: &[String]) -> Result<Self, String> {
        if raw.is_empty() || raw.iter().any(|a| a == "--help" || a == "-h") {
            return Err(
                "用法：zela-verify --scripts DIR --tmp DIR --work name=WxH[:vector] [--work ...]\n\
                 　　：zela-verify --scripts DIR --tmp DIR --sink-bypass（V3-12 检查）"
                    .to_string(),
            );
        }
        let mut scripts: Option<PathBuf> = None;
        let mut tmp: Option<PathBuf> = None;
        let mut fps = 30.0f64;
        let mut works = Vec::new();
        let mut sink_bypass = false;
        let mut i = 0;
        while i < raw.len() {
            match raw[i].as_str() {
                "--scripts" => {
                    i += 1;
                    scripts = Some(PathBuf::from(
                        raw.get(i).ok_or("--scripts 需要一个目录参数")?,
                    ));
                }
                "--tmp" => {
                    i += 1;
                    tmp = Some(PathBuf::from(raw.get(i).ok_or("--tmp 需要一个目录参数")?));
                }
                "--fps" => {
                    i += 1;
                    fps = raw
                        .get(i)
                        .ok_or("--fps 需要一个数字参数")?
                        .parse()
                        .map_err(|_| "--fps 必须是数字".to_string())?;
                }
                "--work" => {
                    i += 1;
                    works.push(parse_work(raw.get(i).ok_or("--work 需要 name=WxH[:vector]")?)?);
                }
                "--sink-bypass" => sink_bypass = true,
                other => return Err(format!("未知参数：{other}")),
            }
            i += 1;
        }
        Ok(Self {
            scripts: scripts.ok_or("缺少 --scripts")?,
            tmp: tmp.ok_or("缺少 --tmp")?,
            fps,
            works,
            sink_bypass,
        })
    }
}

/// `fourier=640x480` / `vector_paths=640x480:vector`。
fn parse_work(spec: &str) -> Result<Work, String> {
    let (name, rest) = spec
        .split_once('=')
        .ok_or_else(|| format!("--work 需要一个 name=WxH[:backend]：{spec}"))?;
    let (size, backend) = match rest.split_once(':') {
        Some((size, "vector")) => (size, LoopBackend::Vector),
        Some((size, "wgpu")) => (size, LoopBackend::Wgpu),
        Some((_, other)) => return Err(format!("--work 的后端只接受 wgpu|vector：{other}")),
        None => (rest, LoopBackend::Wgpu),
    };
    let (w, h) = size
        .split_once('x')
        .ok_or_else(|| format!("--work 的尺寸必须是 WxH：{size}"))?;
    Ok(Work {
        name: name.to_string(),
        width: w.parse().map_err(|_| format!("宽度不是数字：{w}"))?,
        height: h.parse().map_err(|_| format!("高度不是数字：{h}"))?,
        backend,
    })
}

fn main() {
    // B12 Step 1.1：门禁路径也接上订阅器（此前默认采集、默认丢弃）。
    zela_schedule::telemetry::init_tracing_headless();
    if let Err(message) = run() {
        eprintln!("zela-verify: {message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = Args::parse_from(&raw)?;
    std::fs::create_dir_all(&args.tmp)
        .map_err(|e| format!("创建目录 {} 失败：{e}", args.tmp.display()))?;

    // **一次** GPU 初始化：19 次进程调用的固定开销在这里被摊平。
    //
    // B12 Step 4：只有显式开启 GPU 计时才请求 `TIMESTAMP_QUERY`（默认空集，B11 C2）。
    // 请求失败 ⇒ 退回不请求，让 `run_frames` 把状态标成 `unsupported` 并带理由。
    let want_gpu_timing = std::env::var("ZELA_GPU_TIMING").is_ok_and(|value| value != "0");
    let ctx = if want_gpu_timing {
        match RenderContext::headless_with_format_and_features(
            zela_render_backends::DEFAULT_TARGET_FORMAT,
            zela_render_backends::gpu_timing::TIMING_FEATURE,
        ) {
            Ok(ctx) => ctx,
            Err(error) => {
                eprintln!(
                    "[gpu-timing] 无法请求 TIMESTAMP_QUERY：{error}（退回不测，**不填 0**）"
                );
                RenderContext::headless()
                    .map_err(|e| format!("无法创建 headless GPU 上下文：{e}"))?
            }
        }
    } else {
        RenderContext::headless().map_err(|e| format!("无法创建 headless GPU 上下文：{e}"))?
    };
    // B12 Step 3：环境指纹**必填**（GPU / API / os / arch / debug 断言），报告里逐条带上。
    let env = environment_json(ctx.adapter());

    if args.sink_bypass {
        return run_sink_bypass(&ctx, &args, &env);
    }

    for work in &args.works {
        let (script_path, language) = locate_script(&args.scripts, &work.name)?;
        let source = std::fs::read_to_string(&script_path)
            .map_err(|e| format!("[{}] 读取脚本 {} 失败：{e}", work.name, script_path.display()))?;

        // ── 编译（§4.5：也在同一进程里，避免 7 次 `zela-compile` 的启动开销）──
        let payload = compile_and_write(
            &work.name,
            &source,
            language,
            &args.tmp.join(format!("{}.zsb", work.name)),
        )?;

        // ── 路径 1：脚本 ──
        let mut runtime = build_script_runtime(ctx.clone(), source, language)
            .map_err(|e| format!("[{}] 脚本装配失败：{e}", work.name))?;
        let script_hash = render_hash(&ctx, &mut runtime, work, args.fps, "script", &env)?;
        drop(runtime);

        // ── 路径 2：同一份 ZSB ──
        let bytes = std::fs::read(args.tmp.join(format!("{}.zsb", work.name)))
            .map_err(|e| format!("[{}] 读取 ZSB 失败：{e}", work.name))?;
        let bundle = Bundle::from_bytes(&bytes)
            .map_err(|e| format!("[{}] ZSB 解析失败：{e}", work.name))?;
        let mut runtime = build_bundle_runtime(ctx.clone(), &bundle)
            .map_err(|e| format!("[{}] bundle 装配失败：{e}", work.name))?;
        let bundle_hash = render_hash(&ctx, &mut runtime, work, args.fps, "bundle", &env)?;

        // 阶段 1：脚本 / bundle 两条路径的**结构**必须逐位相同（不只像素）。
        assert_structure_matches(&work.name, &script_hash.1, &bundle_hash.1)?;

        println!(
            "[verify] {{\"work\":\"{}\",\"size\":\"{}x{}\",\"frames\":{},\"script\":\"{}\",\
             \"bundle\":\"{}\",\"payload\":\"{}\"}}",
            work.name,
            work.width,
            work.height,
            frames_for(&runtime, args.fps),
            script_hash.0,
            bundle_hash.0,
            payload,
        );
    }
    Ok(())
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
fn run_sink_bypass(ctx: &RenderContext, args: &Args, env: &str) -> Result<(), String> {
    let work = Work {
        name: "vector_paths".to_string(),
        width: 640,
        height: 480,
        backend: LoopBackend::Vector,
    };
    let (script_path, language) = locate_script(&args.scripts, &work.name)?;
    let source = std::fs::read_to_string(&script_path)
        .map_err(|e| format!("读取脚本 {} 失败：{e}", script_path.display()))?;
    let root = args.tmp.join("sink-bypass");
    let mut sizes = Vec::new();
    for (name, hash) in [("with-hash", true), ("no-hash", false)] {
        let dir = root.join(name);
        sizes.push(
            render_png(ctx, &source, language, &work, args.fps, &dir, hash, env)?.len(),
        );
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

/// **阶段 0：默认值快照**（仓库门面批次 §阶段 0）。
///
/// `zela-verify` 的默认值（fps、`--work` 的尺寸切分与后端缺省）直接决定 36 条像素
/// 基线与结构指纹的口径 ⇒ 迁移到 `zela-commands` 后必须逐条复现。
#[cfg(test)]
mod defaults_snapshot {
    use super::*;

    fn args(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|value| value.to_string()).collect()
    }

    fn base(raw: &[&str]) -> Vec<String> {
        let mut out = args(&["--scripts", "scripts", "--tmp", "tmp"]);
        out.extend(args(raw));
        out
    }

    #[test]
    fn fps_defaults_to_30_and_sink_bypass_is_off() {
        let parsed = Args::parse_from(&base(&["--work", "fourier=640x480"])).expect("可解析");
        assert_eq!(parsed.fps, 30.0, "默认 30 fps");
        assert!(!parsed.sink_bypass, "--sink-bypass 默认关");
        assert_eq!(parsed.works.len(), 1);
        assert_eq!(parsed.works[0].name, "fourier");
        assert_eq!((parsed.works[0].width, parsed.works[0].height), (640, 480));
        assert_eq!(
            parsed.works[0].backend,
            LoopBackend::Wgpu,
            "--work 未写后端 ⇒ wgpu（批量对拍基线口径）"
        );
    }

    #[test]
    fn work_backend_suffix_is_parsed() {
        let parsed = Args::parse_from(&base(&["--work", "vector_paths=640x480:vector"]))
            .expect("可解析");
        assert_eq!(parsed.works[0].backend, LoopBackend::Vector);
        assert!(parse_work("x=640x480:metal").is_err(), "未知后端必须失败");
        assert!(parse_work("x=640").is_err(), "缺高度必须失败");
        assert!(parse_work("x").is_err(), "缺尺寸必须失败");
    }

    #[test]
    fn required_directories_fail_explicitly() {
        assert!(Args::parse_from(&args(&["--tmp", "tmp", "--work", "a=1x1"])).is_err());
        assert!(Args::parse_from(&args(&["--scripts", "s", "--work", "a=1x1"])).is_err());
    }

    #[test]
    fn help_is_an_error_with_usage() {
        let error = Args::parse_from(&args(&["--help"])).expect_err("--help 走用法分支");
        assert!(error.contains("用法：zela-verify"));
    }
}
