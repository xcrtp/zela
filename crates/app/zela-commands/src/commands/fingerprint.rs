//! `fingerprint`：跨运行时对拍的指纹工具（单帧 / 结构清单 / 目录对拍）。
//!
//! 帧体搬自 `zela-playback` 的 `fingerprint` example；判定口径不变：
//! 结构（帧数 / 尺寸 / 结构清单）⇒ 不一致立即失败；数值（单帧单通道平均绝对差）
//! 与分歧帧比例 ⇒ 容差判定。

use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::path::{Path, PathBuf};

use zela_playback::{build_bundle_runtime, end_frame, set_view_rect, step_frame};
use zela_render_backends::RenderContext;
use zela_scene_bundle::Bundle;

use crate::host::{CommandHost, Level};
use crate::response::{Fact, Response};
use crate::spec::{ArgSpec, ArgType, CommandArgs, CommandSpec};

pub const DEFAULT_FPS: f64 = 30.0;
pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 480;

pub const SPEC: CommandSpec = CommandSpec {
    name: "fingerprint",
    about: "跨运行时对拍：单帧指纹 / 结构清单 / 帧目录比较",
    args: &[
        ArgSpec::positional("target", ArgType::Text, "bundle.zsb | manifest | compare"),
        ArgSpec::positional("first", ArgType::Text, "第二个位置参数"),
        ArgSpec::positional("second", ArgType::Text, "第三个位置参数"),
        ArgSpec::positional("third", ArgType::Text, "第四个位置参数"),
        ArgSpec::option("max-mad", ArgType::Number, "1.0", "单帧单通道平均绝对差阈值"),
        ArgSpec::option(
            "max-divergent-ratio",
            ArgType::Number,
            "0.01",
            "分歧帧比例阈值",
        ),
    ],
};

/// 三种用法（与原 example 同形）。
#[derive(Debug, Clone, PartialEq)]
pub enum Invocation {
    Single { bundle: String, frame: u32, fps: f64 },
    Manifest { bundle: String, out: String, fps: f64 },
    Compare { reference: String, candidate: String, max_mad: f64, max_ratio: f64 },
}

/// 参数集合 → 用法（默认值：fps = 30、容差 = 1.0、分歧比例 = 0.01）。
pub fn invocation_from_args(args: &CommandArgs) -> Result<Invocation, String> {
    let target = args.text("target").unwrap_or_default();
    let first = args.text("first").map(str::to_string);
    let second = args.text("second").map(str::to_string);
    let third = args.text("third").map(str::to_string);
    let fps_or = |value: Option<String>| {
        value
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_FPS)
    };
    match target {
        "manifest" => Ok(Invocation::Manifest {
            bundle: first.ok_or("usage: manifest <bundle.zsb> <out.txt> [fps]")?,
            out: second.ok_or("usage: manifest <bundle.zsb> <out.txt> [fps]")?,
            fps: fps_or(third),
        }),
        "compare" => Ok(Invocation::Compare {
            reference: first.ok_or("usage: compare <ref_dir> <cand_dir> [options]")?,
            candidate: second.ok_or("usage: compare <ref_dir> <cand_dir> [options]")?,
            max_mad: args.number("max-mad").unwrap_or(1.0),
            max_ratio: args.number("max-divergent-ratio").unwrap_or(0.01),
        }),
        _ => {
            let bundle = if target.is_empty() {
                return Err(
                    "usage: fingerprint <bundle.zsb> <frame> [fps]\n       fingerprint manifest <bundle> <out> [fps]\n       fingerprint compare <ref_dir> <cand_dir> [options]"
                        .to_string(),
                );
            } else {
                target.to_string()
            };
            let frame: u32 = first
                .as_deref()
                .and_then(|value| value.parse().ok())
                .ok_or("缺少 frame 序号")?;
            Ok(Invocation::Single {
                bundle,
                frame,
                fps: fps_or(second),
            })
        }
    }
}

/// 执行指纹工具。
pub fn run(host: &dyn CommandHost, invocation: &Invocation) -> Result<Response, String> {
    match invocation {
        Invocation::Manifest { bundle, out, fps } => manifest(host, bundle, Path::new(out), *fps),
        Invocation::Compare {
            reference,
            candidate,
            max_mad,
            max_ratio,
        } => compare(
            host,
            Path::new(reference),
            Path::new(candidate),
            *max_mad,
            *max_ratio,
        ),
        Invocation::Single { bundle, frame, fps } => single_frame(host, bundle, *frame, *fps),
    }
}

fn fnv(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn build_runtime(bundle_path: &str) -> Result<zela_playback::Runtime, String> {
    let ctx = RenderContext::headless().map_err(|e| e.to_string())?;
    let bytes = fs::read(bundle_path).map_err(|e| format!("读取 {bundle_path} 失败：{e}"))?;
    let bundle = Bundle::from_bytes(&bytes).map_err(|e| e.to_string())?;
    let runtime = build_bundle_runtime(ctx, &bundle).map_err(|e| e.to_string())?;
    set_view_rect(&runtime, WIDTH as f32, HEIGHT as f32);
    Ok(runtime)
}

fn single_frame(
    host: &dyn CommandHost,
    bundle_path: &str,
    target: u32,
    fps: f64,
) -> Result<Response, String> {
    let runtime = build_runtime(bundle_path)?;
    let dt = 1.0 / fps as f32;
    for index in 0..=target {
        let callback = step_frame(&runtime, dt).ok_or("本帧没有可渲染回调")?;
        if index == target {
            let fingerprint = callback.debug_fingerprint();
            let structure = callback.debug_structure();
            host.diagnostic(
                Level::Info,
                &format!("frame={index} fingerprint={fingerprint}"),
            );
            host.diagnostic(Level::Info, &format!("frame={index} structure={structure}"));
            return Ok(Response::facts(
                "fingerprint",
                vec![
                    ("frame".to_string(), Fact::Count(u64::from(index))),
                    ("fingerprint".to_string(), Fact::Text(fingerprint)),
                    ("structure".to_string(), Fact::Text(structure)),
                ],
            ));
        }
        end_frame(&runtime);
    }
    Err("没有走到目标帧".to_string())
}

fn manifest(
    host: &dyn CommandHost,
    bundle_path: &str,
    out: &Path,
    fps: f64,
) -> Result<Response, String> {
    let runtime = build_runtime(bundle_path)?;
    let frames = ((runtime.duration * fps).ceil() as u32).max(1);
    let dt = 1.0 / fps as f32;

    let mut lines = Vec::with_capacity(frames as usize);
    for index in 0..frames {
        let callback = step_frame(&runtime, dt).ok_or("本帧没有可渲染回调")?;
        let objects = runtime
            .driver
            .with_scheduler(|scheduler| scheduler.world().len());
        let structure = callback.debug_structure().replace(';', " ");
        lines.push(format!("frame={index} objects={objects} {structure}"));
        end_frame(&runtime);
    }

    let text = lines.join("\n") + "\n";
    let mut file = BufWriter::new(
        File::create(out).map_err(|e| format!("创建 {} 失败：{e}", out.display()))?,
    );
    file.write_all(text.as_bytes())
        .map_err(|e| format!("写 {} 失败：{e}", out.display()))?;
    let hash = fnv(text.trim_end());
    host.diagnostic(
        Level::Info,
        &format!("structure-summary frames={frames} hash={hash:016x}"),
    );
    Ok(Response::facts(
        "fingerprint",
        vec![
            ("frames".to_string(), Fact::Count(u64::from(frames))),
            ("structure_hash".to_string(), Fact::Text(format!("{hash:016x}"))),
            ("out".to_string(), Fact::Text(out.display().to_string())),
        ],
    ))
}

fn png_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("读取目录 {} 失败：{e}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("png"))
        .collect();
    files.sort();
    Ok(files)
}

fn decode_png(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = File::open(path).map_err(|e| format!("打开 {} 失败：{e}", path.display()))?;
    let decoder = png::Decoder::new(file);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("解析 {} 失败：{e}", path.display()))?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|e| format!("解码 {} 失败：{e}", path.display()))?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(format!(
            "{} 不是 RGBA8（{:?}/{:?}）",
            path.display(),
            info.color_type,
            info.bit_depth
        ));
    }
    buffer.truncate(info.buffer_size());
    Ok((info.width, info.height, buffer))
}

fn compare(
    host: &dyn CommandHost,
    reference: &Path,
    candidate: &Path,
    max_mad: f64,
    max_divergent_ratio: f64,
) -> Result<Response, String> {
    let reference_frames = png_files(reference)?;
    let candidate_frames = png_files(candidate)?;

    // ── 第一层：结构（硬失败）────────────────────────────────────────
    if reference_frames.len() != candidate_frames.len() {
        return Err(format!(
            "结构失败：帧数不同（ref={} cand={}）",
            reference_frames.len(),
            candidate_frames.len()
        ));
    }
    let reference_manifest = reference.join("structure.txt");
    let candidate_manifest = candidate.join("structure.txt");
    if !reference_manifest.is_file() || !candidate_manifest.is_file() {
        return Err(format!(
            "结构失败：缺少 structure.txt（ref={} cand={}）—— 先对两侧运行 `fingerprint manifest`",
            reference_manifest.display(),
            candidate_manifest.display()
        ));
    }
    let a = fs::read_to_string(&reference_manifest)
        .map_err(|e| format!("读取 {} 失败：{e}", reference_manifest.display()))?;
    let b = fs::read_to_string(&candidate_manifest)
        .map_err(|e| format!("读取 {} 失败：{e}", candidate_manifest.display()))?;
    if a != b {
        let first = a
            .lines()
            .zip(b.lines())
            .position(|(x, y)| x != y)
            .unwrap_or_else(|| a.lines().count().min(b.lines().count()));
        return Err(format!(
            "结构失败：structure.txt 第 {} 行不同（ref={:?} cand={:?}）—— 这是逻辑错误，不是舍入",
            first + 1,
            a.lines().nth(first),
            b.lines().nth(first)
        ));
    }
    host.diagnostic(
        Level::Info,
        &format!(
            "structure: OK（{} 帧，manifest 完全一致，hash={:016x}）",
            reference_frames.len(),
            fnv(a.trim_end())
        ),
    );

    // ── 第二层：数值容差 ─────────────────────────────────────────────
    let mut divergent = 0usize;
    let mut out_of_tolerance = 0usize;
    let mut max_seen = 0.0f64;
    let mut max_at = 0usize;

    for (index, (ref_path, cand_path)) in reference_frames
        .iter()
        .zip(candidate_frames.iter())
        .enumerate()
    {
        let (ref_w, ref_h, ref_px) = decode_png(ref_path)?;
        let (cand_w, cand_h, cand_px) = decode_png(cand_path)?;
        if (ref_w, ref_h) != (cand_w, cand_h) {
            return Err(format!(
                "结构失败：第 {index} 帧尺寸不同（ref={ref_w}×{ref_h} cand={cand_w}×{cand_h}）"
            ));
        }
        if ref_w != WIDTH || ref_h != HEIGHT {
            return Err(format!(
                "结构失败：第 {index} 帧尺寸 {ref_w}×{ref_h} ≠ 基线 {WIDTH}×{HEIGHT}"
            ));
        }
        if ref_px.len() != cand_px.len() {
            return Err(format!("结构失败：第 {index} 帧像素字节数不同"));
        }

        let mut sums = [0u64; 4];
        let mut diff_pixels = 0usize;
        for (a, b) in ref_px.chunks_exact(4).zip(cand_px.chunks_exact(4)) {
            let mut any = false;
            for channel in 0..4 {
                let delta = i32::from(a[channel]) - i32::from(b[channel]);
                if delta != 0 {
                    any = true;
                    sums[channel] += delta.unsigned_abs() as u64;
                }
            }
            if any {
                diff_pixels += 1;
            }
        }
        let pixels = f64::from(ref_w) * f64::from(ref_h);
        let mad = [
            sums[0] as f64 / pixels,
            sums[1] as f64 / pixels,
            sums[2] as f64 / pixels,
            sums[3] as f64 / pixels,
        ];
        let mad_rgb = (mad[0] + mad[1] + mad[2]) / 3.0;
        let is_divergent = sums.iter().any(|&sum| sum > 0);
        let over = mad_rgb > max_mad;
        if is_divergent {
            divergent += 1;
        }
        if over {
            out_of_tolerance += 1;
        }
        if mad_rgb > max_seen {
            max_seen = mad_rgb;
            max_at = index;
        }
        if is_divergent || over {
            host.diagnostic(
                Level::Info,
                &format!(
                    "frame={index} mad=({:.6},{:.6},{:.6},{:.6}) mad_rgb={:.6} diff_px={} status={}",
                    mad[0],
                    mad[1],
                    mad[2],
                    mad[3],
                    mad_rgb,
                    diff_pixels,
                    if over { "OUT_OF_TOLERANCE" } else { "OK" }
                ),
            );
        }
    }

    let ratio = divergent as f64 / reference_frames.len() as f64;
    let verdict_ok = out_of_tolerance == 0 && ratio <= max_divergent_ratio;
    host.diagnostic(
        Level::Info,
        &format!(
            "summary: frames={} structure=OK divergent={} ({:.2}%) out_of_tolerance={} max_mad_rgb={:.6}@frame{} ratios(limit={:.2}%) verdict={}",
            reference_frames.len(),
            divergent,
            ratio * 100.0,
            out_of_tolerance,
            max_seen,
            max_at,
            max_divergent_ratio * 100.0,
            if verdict_ok { "OK" } else { "FAIL" }
        ),
    );

    if out_of_tolerance > 0 {
        return Err(format!(
            "数值容差失败：{out_of_tolerance} 帧 mad_rgb > {max_mad}"
        ));
    }
    if ratio > max_divergent_ratio {
        return Err(format!(
            "分歧帧比例失败：{:.2}% > {:.2}%",
            ratio * 100.0,
            max_divergent_ratio * 100.0
        ));
    }
    Ok(Response::facts(
        "fingerprint",
        vec![
            (
                "frames".to_string(),
                Fact::Count(reference_frames.len() as u64),
            ),
            ("divergent".to_string(), Fact::Count(divergent as u64)),
            ("max_mad_rgb".to_string(), Fact::Number(max_seen)),
            ("verdict".to_string(), Fact::Text("OK".to_string())),
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

    #[test]
    fn defaults_match_the_legacy_example() {
        let invocation =
            invocation_from_args(&parse(&["a.zsb", "7"]).expect("可解析")).expect("可构造");
        match invocation {
            Invocation::Single { bundle, frame, fps } => {
                assert_eq!(bundle, "a.zsb");
                assert_eq!(frame, 7);
                assert_eq!(fps, 30.0);
            }
            _ => panic!("应解析为单帧"),
        }

        let invocation =
            invocation_from_args(&parse(&["compare", "ref", "cand"]).expect("可解析"))
                .expect("可构造");
        match invocation {
            Invocation::Compare {
                max_mad,
                max_ratio,
                ..
            } => {
                assert_eq!(max_mad, 1.0);
                assert_eq!(max_ratio, 0.01);
            }
            _ => panic!("应解析为 compare"),
        }
    }

    #[test]
    fn malformed_usage_fails_explicitly() {
        assert!(invocation_from_args(&parse(&["manifest", "a.zsb"]).expect("可解析")).is_err());
        assert!(invocation_from_args(&parse(&["a.zsb"]).expect("可解析")).is_err());
    }
}
