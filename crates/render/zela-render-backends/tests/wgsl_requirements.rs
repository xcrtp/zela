// wgsl_requirements.rs — 顶点入口 × 语义需求表的**源码门禁**。
//
// # 为什么要有这条门禁
//
// "哪个顶点入口读哪些 `@location`"的事实来源是 shader 源码；IR 的
// [`ze_render_ir::vs_requirements`] 是它的第二份表达。没有校验，两份必然漂移 ——
// 这正是本项目反复出现的同族问题（同一事实的第二份定义）。
//
// # 检查口径
//
// **不检查文字，只检查挂载点**：解析 `fn vs_*` 的参数表里 `@location(n)` 的
// **数量与顺序**，与需求表的条目逐项比对；两侧任一方多出条目也判红。
// 参数名（`pos` / `uv` / `normal`）不参与比对 —— 语义与名字的对应由
// `ze-render-ir` 的 `vs_main_requirements_match_standard_3d_layout` 负责。

const CUBE_WGSL: &str = include_str!("../shaders/cube.wgsl");

/// 解析源码里全部 `fn vs_*(...)` 入口：返回 `(入口名, 参数表里 @location 的编号序列)`。
fn parse_vertex_entries(src: &str) -> Vec<(String, Vec<u32>)> {
    let mut entries = Vec::new();
    let mut rest = src;
    while let Some(found) = rest.find("fn vs_") {
        // 定位入口名（`vs_` 之后到 `(` 之前，去掉空白）。
        let after_fn = &rest[found + "fn ".len()..];
        let open = match after_fn.find('(') {
            Some(index) => index,
            None => break,
        };
        let name = after_fn[..open].trim().to_string();
        let body = &after_fn[open..];

        // 扫描参数表：括号深度回到 0 即结束（参数里也有括号，如 `@location(0)`）。
        let bytes = body.as_bytes();
        let mut depth = 0i64;
        let mut index = 0usize;
        let mut locations = Vec::new();
        while index < bytes.len() {
            match bytes[index] as char {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                '@' => {
                    if let Some(location) = parse_location(&body[index..]) {
                        locations.push(location);
                    }
                }
                _ => {}
            }
            index += 1;
        }
        entries.push((name, locations));
        rest = body;
    }
    entries
}

/// 从形如 `@location(12) pos: vec3<f32>` 的片段里取出 `12`。
fn parse_location(at: &str) -> Option<u32> {
    let rest = at.strip_prefix("@location(")?;
    let close = rest.find(')')?;
    rest[..close].trim().parse().ok()
}

/// 每个 `fn vs_*` 都必须在需求表里登记，且 location 数量一致、编号从 0 连续递增。
#[test]
fn every_shader_vertex_entry_matches_requirement_table() {
    let entries = parse_vertex_entries(CUBE_WGSL);
    assert!(
        !entries.is_empty(),
        "cube.wgsl 里没有解析到任何 `fn vs_*` 顶点入口（门禁失效）"
    );

    for (name, locations) in &entries {
        let requirements = ze_render_ir::vs_requirements(name).unwrap_or_else(|| {
            panic!("shader 里的顶点入口 {name:?} 没有在 vs_requirements 表里登记")
        });
        assert_eq!(
            locations.len(),
            requirements.semantics.len(),
            "入口 {name:?}：shader 的 @location 数 {} ≠ 需求表条目数 {}",
            locations.len(),
            requirements.semantics.len()
        );
        // location 编号必须从 0 起连续递增 —— 需求表的"顺序"就是靠它成立的。
        let expected: Vec<u32> = (0..locations.len() as u32).collect();
        assert_eq!(
            locations, &expected,
            "入口 {name:?}：@location 编号必须是从 0 起的连续序列"
        );
    }
}

/// 需求表里的每个入口必须在 shader 源码里真实存在（反向：表不能多出条目）。
#[test]
fn requirement_table_has_no_orphan_entry() {
    let entries = parse_vertex_entries(CUBE_WGSL);
    let names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
    for entry in [ze_render_ir::VS_MAIN, ze_render_ir::VS_LINE] {
        assert!(
            names.contains(&entry),
            "需求表登记了 {entry:?}，但 cube.wgsl 里没有这个 `fn`（表多出了条目）"
        );
    }
    // 反向：源码里的入口也必须都在表里（由 `every_shader_vertex_entry_matches_requirement_table` 覆盖）。
    for name in names {
        assert!(
            ze_render_ir::vs_requirements(name).is_some(),
            "cube.wgsl 的顶点入口 {name:?} 未在需求表里登记"
        );
    }
}

/// 拓扑 → 入口的映射必须指向一个**已登记且源码里存在**的入口。
#[test]
fn topology_mapping_points_to_a_real_entry() {
    let names: Vec<String> = parse_vertex_entries(CUBE_WGSL)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    for topology in [
        ze_render_ir::PrimitiveTopology::Triangles,
        ze_render_ir::PrimitiveTopology::Lines,
        ze_render_ir::PrimitiveTopology::Points,
    ] {
        let entry = ze_render_ir::vs_entry_for(topology);
        assert!(
            names.iter().any(|name| name == entry),
            "{topology:?} 映射到 {entry:?}，但 cube.wgsl 里没有这个顶点入口"
        );
        assert!(
            ze_render_ir::vs_requirements(entry).is_some(),
            "{topology:?} 映射到 {entry:?}，但需求表没有登记它"
        );
    }
}

/// 门禁自身有效性的反例：把 shader 参数砍到一个 location，比对必须不相等。
///
/// 这条用例守的是**门禁会不会失效**——如果哪天解析器退化成恒等，这里先红。
#[test]
fn gate_would_catch_a_mismatched_table() {
    let synthetic = "fn vs_main(\n    @location(0) pos: vec3<f32>,\n) -> VertexOutput {\n";
    let entries = parse_vertex_entries(synthetic);
    assert_eq!(entries.len(), 1);
    let (name, locations) = &entries[0];
    let requirements = ze_render_ir::vs_requirements(name).expect("vs_main 已登记");
    assert_eq!(locations.len(), 1);
    assert_ne!(
        locations.len(),
        requirements.semantics.len(),
        "只有一个 location 的 vs_main 必须与三语义的需求表不一致（否则门禁失效）"
    );
}
