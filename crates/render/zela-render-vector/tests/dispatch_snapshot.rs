// dispatch_snapshot.rs — BO-0 的**现状快照**（dispatch 参数计算）。
//
// # 这是"先固化现状，再改造"的那份基准
//
// 15 处 `dispatch_workgroups` 分散在 GPU 代码路径里（要设备才能跑），无法直接快照它们的
// 实参。因此这里用**与现状逐字对应的纯函数**把算法复刻一遍，并把代表值固定下来。
//
// 它的价值在 BO-2：改造后 `groups_for(shape, extent)` 必须对这些输入给出**完全相同的**
// `(x, y, z)`。任何一处形状/除数写错，这里先红 —— 像素判据抓不到 dispatch 算错
// （多派的工作组会被 shader 的边界检查吃掉，只是变慢）。
//
// # 为什么现在必然通过
//
// 这些断言比对的是"复刻的算法 vs 固定的数字"，与产品代码无关。BO-2 会追加
// "产品实现 vs 复刻算法"的等价断言，那一步才是真正的判据。

/// 二维铺满：`ceil(w/tile)` × `ceil(h/tile)`。
///
/// 对应：bloom（tile 8/8/8/16）、occlusion（8/8/8/16/16）、render 的 frag codegen（8）、
/// vector 的深度解析（16）与 composite/keep/bound_slabs（TILE=16）。
fn old_tiles(w: u32, h: u32, tile: u32) -> [u32; 3] {
    [w.div_ceil(tile), h.div_ceil(tile), 1]
}

/// frag codegen 的现状写法：两维各自 `min(65535)`。
fn old_frag_tiles(w: u32, h: u32) -> [u32; 3] {
    [w.div_ceil(8).min(65535), h.div_ceil(8).min(65535), 1]
}

/// 一维列表：`ceil(n/64)` 个工作组，超过 65535 折到 y 维。
///
/// 对应：vector 的 flatten（fill / stroke）与测试里的 area 累积。
fn old_linear(n: u32) -> [u32; 3] {
    let slots = n.div_ceil(64);
    [slots.min(65535), slots.div_ceil(65535), 1]
}

/// 立方体环境：`ceil(side/8)` × `ceil(side/8)` × 6 面。
fn old_cube(side: u32) -> [u32; 3] {
    let groups = side.div_ceil(8);
    [groups, groups, 6]
}

#[test]
fn tiles_snapshot() {
    assert_eq!(old_tiles(1920, 1080, 16), [120, 68, 1]);
    assert_eq!(old_tiles(1920, 1080, 8), [240, 135, 1]);
    assert_eq!(old_tiles(1280, 720, 16), [80, 45, 1]);
    assert_eq!(old_tiles(1, 1, 16), [1, 1, 1]);
    assert_eq!(old_tiles(0, 0, 16), [0, 0, 1], "0 尺寸不派工作组");
}

#[test]
fn frag_tiles_snapshot() {
    assert_eq!(old_frag_tiles(1920, 1080), [240, 135, 1]);
    assert_eq!(old_frag_tiles(8, 8), [1, 1, 1]);
}

#[test]
fn linear_snapshot() {
    assert_eq!(old_linear(0), [0, 0, 1]);
    assert_eq!(old_linear(1), [1, 1, 1]);
    assert_eq!(old_linear(64), [1, 1, 1]);
    assert_eq!(old_linear(65), [2, 1, 1]);
    // 恰好 65535 个工作组：不折。
    assert_eq!(old_linear(65535 * 64), [65535, 1, 1]);
    // 多一个：折到 y 维。
    assert_eq!(old_linear(65536 * 64), [65535, 2, 1]);
}

#[test]
fn cube_snapshot() {
    assert_eq!(old_cube(128), [16, 16, 6]);
    assert_eq!(old_cube(64), [8, 8, 6]);
}

// ─── BO-2：产品实现 vs 上面复刻的旧算法（双跑等价） ────────────────────────
//
// 像素判据抓不到 dispatch 算错（多派的工作组被 shader 的边界检查吃掉，只是变慢），
// 所以这一组断言是本批唯一能证明"参数没变"的判据。

use zela_render_vector::dispatch::{Dispatch, Extent, GroupSize, groups_for};

/// 旧算法没有截断/折叠的上限问题；这里对"上限之内"的输入逐个比对。
#[test]
fn tiles_match_the_old_algorithm() {
    let sizes = [(1920, 1080), (1280, 720), (256, 256), (1, 1), (7, 9), (0, 0), (4096, 2160)];
    let entries = [("spread", 8), ("down", 8), ("up", 8), ("show", 16), ("composite", 16), ("keep", 16), ("bound_slabs", 16), ("nearest_of_samples", 16), ("sao", 8)];
    for (width, height) in sizes {
        for (entry, tile) in entries {
            let shape = Dispatch::Tiles {
                group: GroupSize::Entry(entry),
            };
            assert_eq!(
                groups_for(shape, Extent::px(width, height)),
                old_tiles(width, height, tile),
                "{entry} 在 {width}x{height} 上与旧算法不一致"
            );
        }
    }
}

#[test]
fn frag_tiles_match_the_old_algorithm() {
    for (width, height) in [(1920, 1080), (8, 8), (17, 3)] {
        let shape = Dispatch::Tiles {
            group: GroupSize::Literal(8),
        };
        assert_eq!(
            groups_for(shape, Extent::px(width, height)),
            old_frag_tiles(width, height),
            "frag codegen 在 {width}x{height} 上与旧算法不一致"
        );
    }
}

#[test]
fn linear_matches_the_old_algorithm() {
    let shape = Dispatch::Linear {
        group: GroupSize::Entry("flatten_fill"),
    };
    for count in [0, 1, 63, 64, 65, 4096, 65535 * 64, 65536 * 64] {
        assert_eq!(
            groups_for(shape, Extent::len(count)),
            old_linear(count),
            "flatten 在 {count} 个槽位上与旧算法不一致"
        );
    }
}

#[test]
fn cube_matches_the_old_algorithm() {
    let shape = Dispatch::Cube {
        group: GroupSize::Entry("to_cube"),
        faces: 6,
    };
    for side in [16, 64, 128, 512] {
        assert_eq!(
            groups_for(shape, Extent::square(side)),
            old_cube(side),
            "environment 在边长 {side} 上与旧算法不一致"
        );
    }
}

/// 唯一一处**行为变化**（只在上限之外）：旧写法是两维各自截断（会少派工作组），
/// 新写法把溢出折到第二维。这里把它写成断言，避免"悄悄改了"。
#[test]
fn folding_differs_from_clamping_only_beyond_the_limit() {
    let width = 65535u32 * 8 + 8; // 超过 65535 个工作组
    let shape = Dispatch::Tiles {
        group: GroupSize::Literal(8),
    };
    let [x, y, _] = groups_for(shape, Extent::px(width, 8));
    assert_eq!([x, y], [65535, 2], "折到第二维");
    assert_eq!(old_frag_tiles(width, 8), [65535, 1, 1], "旧写法会少派一层");
}
