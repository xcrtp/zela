// binding_snapshot.rs — BP-3 的**句柄级对照**（双跑等价）。
//
// `wgpu::BindGroup` 没有稳定 id，所以"改造前后绑的是同一个组"不能按句柄比较。
// 这里用两条可判定的事实把等价固定住（计划 §BP-3 的判据）：
//
//   1. 每个 `set_bind_group` 的**实参表达式**逐字节不变 —— 改造只允许在它下一行
//      加 `note_bind`，不许调整实参；
//   2. 每个帧级绑定的下一行就是它的记账，且记账里的组名映射回**同一个组号**。
//
// 为什么不能只靠像素：绑错一个"内容相同的另一个组"不会改像素。

use zela_render_vector::passes::RASTER_GROUPS;

const RENDER: &str = include_str!("../src/render.rs");
const VECTOR: &str = include_str!("../src/vector.rs");

/// 统计 `needle` 在 `source` 里出现的次数（逐字节，不含换行处理）。
fn count(source: &str, needle: &str) -> usize {
    source.matches(needle).count()
}

/// 实参快照：改造前（BP-2 提交）与 BP-3 之后必须逐条相同。
#[test]
fn set_bind_group_arguments_are_pinned() {
    let render: [(&str, usize); 10] = [
        // 帧级固定（已接入记账）
        ("set_bind_group(0, buffers.group.as_ref(), &slot)", 8),
        ("set_bind_group(3, lighting_group, &[])", 4),
        ("set_bind_group(3, &buffers.shadow_maps.group, &[])", 1),
        ("set_bind_group(2, append, &[])", 1),
        ("set_bind_group(1, &self.image_groups[&0], &[])", 1),
        // escape：per-iteration / 现场构造 / 参数传递 / per-draw
        ("set_bind_group(2, &s.groups[1], &[])", 1),
        ("set_bind_group(2, &s.groups[0], &[])", 1),
        ("set_bind_group(0, &group, &[])", 2),
        ("set_bind_group(1, white, &[])", 2),
        (
            "set_bind_group(1, self.image_groups.get(&draw.texture).unwrap_or(white), &[])",
            1,
        ),
    ];
    for (needle, expected) in render {
        assert_eq!(
            count(RENDER, needle),
            expected,
            "render.rs 的实参 {needle:?} 变了 —— 改造不许动实参，只许在下一行加 note_bind"
        );
    }

    let vector: [(&str, usize); 9] = [
        ("set_bind_group(0, &scene, &[])", 5),
        ("set_bind_group(0, &group, &[])", 1),
        ("set_bind_group(3, read, &[])", 1),
        ("set_bind_group(1, &images, &[])", 2),
        ("set_bind_group(2, &write, &[])", 1),
        (
            "set_bind_group(2, &bind(&self.groups_layout, &[(0, self.crossings.1.as_entire_binding())]), &[])",
            1,
        ),
        ("set_bind_group(1, &read, &[])", 1),
        ("set_bind_group(2, &written, &[])", 1),
        // 测试内联着色器（`area law`），不是生产路径
        ("set_bind_group(0, &bindings, &[])", 1),
    ];
    for (needle, expected) in vector {
        assert_eq!(
            count(VECTOR, needle),
            expected,
            "vector.rs 的实参 {needle:?} 变了（BP-3 不动 vector.rs 的绑定）"
        );
    }
}

/// 取出一行里所有 `"…"` 的内容。
fn quoted(line: &str) -> Vec<&str> {
    line.split('"').skip(1).step_by(2).collect()
}

/// 帧级绑定的下一行必须是记账，且记账名映射回同一个组号。
#[test]
fn frame_level_binds_are_ledgered_next_line() {
    let lines: Vec<&str> = RENDER.lines().collect();
    let mut ledgered = 0;
    let mut escapes = 0;
    for (index, line) in lines.iter().enumerate() {
        let Some(start) = line.find("set_bind_group(") else {
            continue;
        };
        let group: u32 = line[start + "set_bind_group(".len()..]
            .split(',')
            .next()
            .expect("组号")
            .trim()
            .parse()
            .expect("组号必须是字面量");
        // escape：实参随循环变量 / 现场构造 / 闭包入参 / per-draw 变化
        let escape = line.contains("&s.groups[")
            || line.contains("(0, &group,")
            || line.contains("white")
            || line.contains("image_groups.get");
        let next = lines.get(index + 1).copied().unwrap_or("");
        if escape {
            assert!(
                !next.contains("note_bind("),
                "escape 点不应记账（否则会把顺手绑定算进声明）：{line}"
            );
            escapes += 1;
            continue;
        }
        let names = quoted(next);
        assert_eq!(
            names.len(),
            2,
            "帧级绑定的下一行应是 note_bind(\"pass\", \"组\")：\n  绑定：{line}\n  下一行：{next}"
        );
        assert_eq!(
            RASTER_GROUPS.group_of(names[1]),
            Some(group),
            "记账名 {:?} 映射不到组 {group}（{:?}）",
            names[1],
            names
        );
        ledgered += 1;
    }
    assert_eq!(ledgered, 15, "帧级固定绑定应全部接入记账");
    assert_eq!(escapes, 7, "escape 点应保持 7 处（2 codegen + 2 slabs + 2 白图 + 1 per-draw）");
}
