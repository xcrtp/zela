// SPDX-FileCopyrightText: 2026 Academa, Inc.
// SPDX-License-Identifier: MIT
// Modified by Zela: BO 批新建 —— compute 的 `workgroup_size` 与 dispatch 形状。

//! `workgroup_size` 的单一来源，以及 dispatch 形状的描述。
//!
//! # 两个概念，不要混
//!
//! - **除数**（`workgroup_size`）：写在 shader 的 `@compute @workgroup_size(…)` 里，
//!   本模块的 [`WORKGROUP`] 是它的**受门禁保护的镜像**。改 shader 不改表 ⇒ 门禁红；
//!   改表不改 shader ⇒ 门禁红。
//! - **被除数**（要铺多少像素 / 多少个槽）：运行时算出来的，见 [`Extent`]。
//!
//! 声明里能写死的只有"形状"（[`Dispatch`]），写不死参数值——
//! `environment` 的三个 entry 都是 `(8,8,1)`，但被除数分别是 `SIDE` / `SIDE>>mip` / `SIDE>>l`。
//!
//! # 为什么这层必须存在
//!
//! 改造前，15 处 `dispatch_workgroups` 各自把除数写成字面量（`div_ceil(8)` /
//! `div_ceil(16)` / `div_ceil(64)`）。改 `bloom.wgsl` 的 `show` 而 Rust 侧仍传 16 ⇒
//! **零报错**，只是多派或少派工作组；多派的工作组会被 shader 的边界检查吃掉，
//! 像素完全一致，只是变慢 —— 像素判据抓不到这类错误。
//!
//! # 门禁
//!
//! `#[cfg(test)]` 里解析五个 shader 的 `@compute @workgroup_size(…)` 与紧随的
//! `fn` 名，与本表逐项比对；**两侧任一方多出条目也判红**。形态与 BM 批的
//! `wgsl_requirements.rs` 同源（不检查文字，只检查挂载点）。

// ─── 单一来源：21 个 compute entry 的 workgroup_size ──────────────────────

/// `(entry 名, (x, y, z))`。顺序与 shader 文件内的出现顺序无关（比对按名字）。
///
/// 缺维补 1：`(8, 8)` 记为 `(8, 8, 1)`，`(64)` 记为 `(64, 1, 1)`。
pub const WORKGROUP: [(&str, (u32, u32, u32)); 21] = [
    // bloom.wgsl（前三个 8×8，show 是 16×16 —— 同文件内异 size，最容易漂移的地方）
    ("spread", (8, 8, 1)),
    ("down", (8, 8, 1)),
    ("up", (8, 8, 1)),
    ("show", (16, 16, 1)),
    // depth.wgsl
    ("nearest_of_samples", (16, 16, 1)),
    ("single_sample", (16, 16, 1)),
    // environment.wgsl
    ("to_cube", (8, 8, 1)),
    ("halve", (8, 8, 1)),
    ("prefilter", (8, 8, 1)),
    // occlusion.wgsl（前三个 8×8，两个模糊 16×16）
    ("linearize", (8, 8, 1)),
    ("depth_mip", (8, 8, 1)),
    ("sao", (8, 8, 1)),
    ("blur_h", (16, 16, 1)),
    ("blur_v", (16, 16, 1)),
    // vector_compute.wgsl
    ("flatten_fill", (64, 1, 1)),
    ("flatten_stroke", (64, 1, 1)),
    ("composite", (16, 16, 1)),
    ("keep", (16, 16, 1)),
    ("bound_slabs", (16, 16, 1)),
    ("count_crossings", (1, 1, 1)),
    ("settle_crossings", (64, 1, 1)),
];

/// entry 名 → `workgroup_size`。未登记的名字 **panic 并点名**（不静默给 1）。
pub fn workgroup(entry: &str) -> (u32, u32, u32) {
    WORKGROUP
        .iter()
        .find(|(name, _)| *name == entry)
        .map(|(_, size)| *size)
        .unwrap_or_else(|| panic!("entry {entry:?} 没有登记 workgroup_size（WORKGROUP 表缺项）"))
}

/// shader 的 tile 常量（`vector.wgsl` 的 `const TILE: u32 = 16u;`）。
///
/// Rust 侧唯一的一份在这里；`vector.rs` 从本模块引入，不再自己写 16。
pub const TILE: u32 = 16;

// ─── dispatch 形状 ───────────────────────────────────────────────────────

/// 除数的来源：要么是一个已登记的 entry（跟随 shader），要么是生成着色器的字面量。
///
/// 生成着色器（fill / frag codegen）的 `@workgroup_size` 在运行期由 codegen 拼进源码，
/// 不在 [`WORKGROUP`] 里 —— 对它们只能用 `Literal`，并在调用点写明来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupSize {
    /// 从 [`WORKGROUP`] 查（单一来源）。
    Entry(&'static str),
    /// 生成着色器的字面量。
    Literal(u32),
}

impl GroupSize {
    /// 该维度上的工作组边长。
    pub fn size(self) -> u32 {
        match self {
            Self::Entry(entry) => workgroup(entry).0,
            Self::Literal(tile) => tile,
        }
    }
}

/// 被除数：这一帧要铺多少。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent {
    pub width: u32,
    pub height: u32,
}

impl Extent {
    /// 一串元素（`Linear` 的被除数）。
    pub const fn len(count: u32) -> Self {
        Self {
            width: count,
            height: 1,
        }
    }

    /// 一张二维目标。
    pub const fn px(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// 一张正方形目标（环境立方体的每个面）。
    pub const fn square(side: u32) -> Self {
        Self::px(side, side)
    }
}

/// 一个 compute pass 的 dispatch **形状**。
///
/// 形状描述"被除数怎么算"，不写死参数值 —— 参数由 [`groups_for`] 在运行时求。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// 光栅化，不是 compute（没有 dispatch）。
    Draw,
    /// 恰好一个工作组。
    Fixed,
    /// 二维铺满 `ceil(w/g) × ceil(h/g)`。
    Tiles { group: GroupSize },
    /// 一维列表 `ceil(n/g)`，溢出折到第二维。
    Linear { group: GroupSize },
    /// 立方体的六个面：`ceil(side/g) × ceil(side/g) × faces`。
    Cube { group: GroupSize, faces: u32 },
    /// 由 GPU 侧写的 indirect 参数决定。
    Indirect { source: &'static str },
    /// **声明专用**：一个逻辑 pass 内多次 dispatch（例如泛光的 spread/down/up/show）。
    ///
    /// [`groups_for`] 不接受它 —— 它没有单一的三元组。
    Sequence(&'static [Dispatch]),
}

/// wgpu 单维上限。
const MAX_GROUPS: u32 = 65535;

/// 把一维的溢出折到第二维：`x` 截到上限，`y` 乘上"折了几层"。
///
/// 折层数 = `ceil(x / 65535)`：`x = 0` 时为 0（没有工作组要派），`1..=65535` 时为 1，
/// 之后线性增长。这与改造前 `flatten` 的 `(min(slots,65535), ceil(slots/65535))` 逐位一致。
fn fold_x(x: u32, y: u32) -> (u32, u32) {
    (x.min(MAX_GROUPS), y.saturating_mul(x.div_ceil(MAX_GROUPS)))
}

/// 形状 + 被除数 → 实际派发的 `(x, y, z)`。
///
/// `Draw` / `Indirect` / `Sequence` 没有直接的三元组：前两者分别由光栅化与 GPU 决定，
/// 最后一个是声明专用的复合形状。对它们调用本函数是**调用方的错**，显式 panic。
pub fn groups_for(shape: Dispatch, extent: Extent) -> [u32; 3] {
    match shape {
        Dispatch::Fixed => [1, 1, 1],
        Dispatch::Tiles { group } => {
            let (x, y) = fold_x(
                extent.width.div_ceil(group.size()),
                extent.height.div_ceil(group.size()),
            );
            [x, y, 1]
        }
        Dispatch::Linear { group } => {
            let (x, y) = fold_x(extent.width.div_ceil(group.size()), 1);
            [x, y, 1]
        }
        Dispatch::Cube { group, faces } => {
            let side = extent.width.div_ceil(group.size());
            let (x, y) = fold_x(side, side);
            [x, y, faces]
        }
        Dispatch::Draw => panic!("Draw 是光栅化 pass 的形状，没有 dispatch 参数"),
        Dispatch::Indirect { source } => {
            panic!("Indirect({source}) 的组数由 GPU 侧的 indirect buffer 决定，CPU 不计算")
        }
        Dispatch::Sequence(_) => {
            panic!("Sequence 是声明用的复合形状，没有单一的三元组；请对每一项分别求值")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 解析与门禁 ───────────────────────────────────────────────────────

    const SHADERS: [(&str, &str); 5] = [
        ("bloom.wgsl", include_str!("bloom.wgsl")),
        ("depth.wgsl", include_str!("depth.wgsl")),
        ("environment.wgsl", include_str!("environment.wgsl")),
        ("occlusion.wgsl", include_str!("occlusion.wgsl")),
        ("vector_compute.wgsl", include_str!("vector_compute.wgsl")),
    ];

    /// 解析源码里全部 `@compute @workgroup_size(…)` 入口：`(entry 名, (x, y, z))`。
    ///
    /// 只认 `@compute @workgroup_size(` 这个组合前缀（单独的 `@workgroup_size` 不算）；
    /// 维度不定，缺维补 1；入口名取其后第一个 `fn` 的名字。
    fn parse_compute_entries(src: &str) -> Vec<(String, (u32, u32, u32))> {
        const PREFIX: &str = "@compute @workgroup_size(";
        let mut entries = Vec::new();
        let mut rest = src;
        while let Some(found) = rest.find(PREFIX) {
            let after = &rest[found + PREFIX.len()..];
            let close = match after.find(')') {
                Some(index) => index,
                None => break,
            };
            let mut dims: Vec<u32> = after[..close]
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect();
            dims.resize(3, 1);
            let dimensions = (dims[0], dims[1], dims[2]);

            // 入口名：其后第一个 `fn <name>`
            let tail = &after[close..];
            let name = tail.find("fn ").and_then(|at| {
                let after_fn = &tail[at + 3..];
                let end = after_fn.find('(')?;
                Some(after_fn[..end].trim().to_string())
            });
            if let Some(name) = name {
                entries.push((name, dimensions));
            }
            rest = tail;
        }
        entries
    }

    /// 门禁本体：解析结果的集合必须与 [`WORKGROUP`] 逐项相等（双向）。
    fn gate(parsed: &[(String, (u32, u32, u32))]) -> Result<(), String> {
        for (name, dimensions) in parsed {
            match WORKGROUP.iter().find(|(known, _)| known == name) {
                None => return Err(format!("shader 里的 entry {name:?} 没在 WORKGROUP 表里登记")),
                Some((_, known)) if known != dimensions => {
                    return Err(format!(
                        "entry {name:?}：shader 的 @workgroup_size {dimensions:?} ≠ 表里的 {known:?}"
                    ))
                }
                Some(_) => {}
            }
        }
        for (name, _) in WORKGROUP {
            if !parsed.iter().any(|(parsed, _)| parsed == name) {
                return Err(format!("WORKGROUP 表登记的 {name:?} 在 shader 源码里找不到"));
            }
        }
        Ok(())
    }

    /// 五个 shader 合计 21 个 compute entry，与表逐项一致。
    #[test]
    fn workgroup_table_matches_every_shader() {
        let parsed: Vec<(String, (u32, u32, u32))> = SHADERS
            .iter()
            .flat_map(|(name, src)| {
                parse_compute_entries(src)
                    .into_iter()
                    .map(move |entry| (format!("{name}:{}", entry.0), entry.1))
            })
            .collect();
        assert_eq!(parsed.len(), 21, "解析到的 compute entry 数不是 21");

        // 门禁用"纯入口名"比对：去掉文件前缀。
        let bare: Vec<(String, (u32, u32, u32))> = parsed
            .iter()
            .map(|(name, size)| {
                let short = name.split_once(':').map_or(name.as_str(), |(_, short)| short);
                (short.to_string(), *size)
            })
            .collect();
        gate(&bare).unwrap_or_else(|error| panic!("workgroup_size 门禁失败：{error}"));
    }

    /// 门禁自证：shader 侧多出一个 entry ⇒ 红。
    #[test]
    fn gate_rejects_extra_shader_entry() {
        let mut parsed = parsed_entries();
        parsed.push(("brand_new".to_string(), (8, 8, 1)));
        assert!(gate(&parsed).unwrap_err().contains("没在 WORKGROUP 表里登记"));
    }

    /// 门禁自证：表侧多出一个 entry ⇒ 红。
    #[test]
    fn gate_rejects_entry_missing_from_shader() {
        let mut parsed = parsed_entries();
        parsed.retain(|(name, _)| name != "show");
        assert!(gate(&parsed).unwrap_err().contains("找不到"));
    }

    /// 门禁自证：任一侧改一个数 ⇒ 红（这里改 shader 侧解析出的值）。
    #[test]
    fn gate_rejects_changed_size() {
        let mut parsed = parsed_entries();
        for entry in parsed.iter_mut() {
            if entry.0 == "show" {
                entry.1 = (8, 8, 1);
            }
        }
        let error = gate(&parsed).unwrap_err();
        assert!(error.contains("show"), "错误要点名出问题的 entry：{error}");
    }

    fn parsed_entries() -> Vec<(String, (u32, u32, u32))> {
        SHADERS
            .iter()
            .flat_map(|(_, src)| parse_compute_entries(src))
            .collect()
    }

    /// 解析：1 / 2 / 3 维各一例，缺维补 1。
    #[test]
    fn parse_handles_every_dimension_count() {
        let one = parse_compute_entries("@compute @workgroup_size(64)\nfn a() {}");
        assert_eq!(one, vec![("a".to_string(), (64, 1, 1))]);
        let two = parse_compute_entries("@compute @workgroup_size(8, 8)\nfn b() {}");
        assert_eq!(two, vec![("b".to_string(), (8, 8, 1))]);
        let three = parse_compute_entries("@compute @workgroup_size(8, 8, 1)\nfn c() {}");
        assert_eq!(three, vec![("c".to_string(), (8, 8, 1))]);
    }

    /// 单独的 `@workgroup_size`（不属于 `@compute`）不被当成入口。
    #[test]
    fn parse_ignores_non_compute_annotations() {
        let parsed = parse_compute_entries("@workgroup_size(8, 8)\nfn d() {}");
        assert!(parsed.is_empty());
    }

    /// TILE 与 shader 侧的常量一致（第二份来源在这里被消除）。
    #[test]
    fn tile_matches_the_shader() {
        let src = include_str!("vector.wgsl");
        let marker = "const TILE: u32 = ";
        let at = src.find(marker).expect("vector.wgsl 里没有 TILE 常量");
        let rest = &src[at + marker.len()..];
        let end = rest.find('u').expect("TILE 常量缺 u 后缀");
        let shader: u32 = rest[..end].trim().parse().expect("TILE 解析失败");
        assert_eq!(TILE, shader, "Rust 的 TILE 与 vector.wgsl 的 TILE 不一致");
    }

    // ── 形状 → 参数 ─────────────────────────────────────────────────────

    #[test]
    fn groups_fixed_is_one_group() {
        assert_eq!(groups_for(Dispatch::Fixed, Extent::px(1920, 1080)), [1, 1, 1]);
    }

    #[test]
    fn groups_tiles_rounds_up() {
        let shape = Dispatch::Tiles {
            group: GroupSize::Entry("composite"),
        };
        assert_eq!(groups_for(shape, Extent::px(1920, 1080)), [120, 68, 1]);
        assert_eq!(groups_for(shape, Extent::px(1, 1)), [1, 1, 1]);
        assert_eq!(groups_for(shape, Extent::px(0, 0)), [0, 0, 1]);
    }

    /// 除数跟随 shader：同一个形状换 entry，参数随之变化（这就是要消除的双来源）。
    #[test]
    fn groups_tiles_follow_the_entry() {
        let tiny = Dispatch::Tiles {
            group: GroupSize::Entry("spread"),
        };
        let big = Dispatch::Tiles {
            group: GroupSize::Entry("show"),
        };
        assert_eq!(groups_for(tiny, Extent::px(1920, 1080)), [240, 135, 1]);
        assert_eq!(groups_for(big, Extent::px(1920, 1080)), [120, 68, 1]);
        // 生成着色器没有表项，用字面量。
        let literal = Dispatch::Tiles {
            group: GroupSize::Literal(8),
        };
        assert_eq!(groups_for(literal, Extent::px(1920, 1080)), [240, 135, 1]);
    }

    #[test]
    fn groups_linear_folds_beyond_the_limit() {
        let shape = Dispatch::Linear {
            group: GroupSize::Entry("flatten_fill"),
        };
        assert_eq!(groups_for(shape, Extent::len(0)), [0, 0, 1]);
        assert_eq!(groups_for(shape, Extent::len(64)), [1, 1, 1]);
        assert_eq!(groups_for(shape, Extent::len(65)), [2, 1, 1]);
        assert_eq!(
            groups_for(shape, Extent::len(65535 * 64)),
            [65535, 1, 1],
            "恰好到上限不折"
        );
        assert_eq!(
            groups_for(shape, Extent::len(65536 * 64)),
            [65535, 2, 1],
            "超一个工作组就折到 y 维"
        );
    }

    #[test]
    fn groups_cube_keeps_six_faces() {
        let shape = Dispatch::Cube {
            group: GroupSize::Entry("to_cube"),
            faces: 6,
        };
        assert_eq!(groups_for(shape, Extent::square(128)), [16, 16, 6]);
        assert_eq!(groups_for(shape, Extent::square(64)), [8, 8, 6]);
    }

    /// `Draw` / `Indirect` / `Sequence` 求值是调用方的错，必须显式报出来。
    #[test]
    #[should_panic(expected = "没有 dispatch 参数")]
    fn groups_for_draw_panics() {
        groups_for(Dispatch::Draw, Extent::px(1, 1));
    }

    #[test]
    #[should_panic(expected = "由 GPU 侧的 indirect buffer 决定")]
    fn groups_for_indirect_panics() {
        groups_for(
            Dispatch::Indirect {
                source: "crossing groups",
            },
            Extent::px(1, 1),
        );
    }

    #[test]
    #[should_panic(expected = "声明用的复合形状")]
    fn groups_for_sequence_panics() {
        groups_for(Dispatch::Sequence(&[]), Extent::px(1, 1));
    }

    /// 未登记的 entry ⇒ panic 并点名。
    #[test]
    #[should_panic(expected = "没有登记 workgroup_size")]
    fn unknown_entry_panics() {
        workgroup("no_such_entry");
    }
}
