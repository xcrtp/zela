// command_flow.rs — **BS-0** 的命令流快照。
//
// 像素只能证明"画面没变"，证明不了"命令流没变"：绑到另一个*内容相同*的组、
// 或两个 pass 的顺序局部互换，像素都可能是绿的。这里把每个视图**实际录到**的
// `(pass, 组名序列)` 顺序固定成夹具 —— 由 `PassSchedule::command_flow` 产生，
// 只记录事实（记录 / 跳过 / 逃生舱），不申报期望。
//
// 与 `binding_snapshot.rs` 分工：那个固定**源码里的实参**（改造不许动实参），
// 这个固定**运行时的命令序列**（改造不许动顺序与归属）。

#![cfg(not(target_arch = "wasm32"))]

use zela_render_vector::{
    FilledPath, Light, MeshRecord, PathRecord, View3, render_2d, render_3d,
    start_command_flow, take_command_flow,
};

/// 跑一次渲染并取回命令流（每次调用都清空旧记录）。
///
/// 命令流是进程级槽位，用例默认并行 ⇒ 这里用一把锁把它们串起来，
/// 否则两个用例会互相清空 / 取走对方的数据。
fn flow(run: impl FnOnce()) -> Vec<String> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    start_command_flow();
    run();
    take_command_flow()
}

/// **BS-4 终检**：计划里声明过的每个 pass 都有对应的驱动分支。
///
/// `encode_frame` 已改成 `for decl in plan { match decl.name { … } }`（光栅两张计划与
/// 矢量计划都是）。所以"加一个 pass"最少要两步：加一条声明 + 加一个分支 —— 漏掉分支
/// 会在运行时 panic，这里把它提前到测试里。反向也成立：驱动里不允许出现计划外的分支。
#[test]
fn every_declared_pass_has_a_branch() {
    use zela_render_vector::passes::{RASTER_LAYERED_PLAN, RASTER_PLAN, VECTOR_PLAN};

    let render = include_str!("../src/render.rs");
    for (name, plan) in [
        ("RASTER_PLAN", RASTER_PLAN),
        ("RASTER_LAYERED_PLAN", RASTER_LAYERED_PLAN),
        ("VECTOR_PLAN", VECTOR_PLAN),
    ] {
        for decl in plan {
            let arm = format!("\"{}\" =>", decl.name);
            assert!(
                render.contains(&arm),
                "{name} 声明了 pass {:?}，但 encode_frame 的驱动循环里没有 {arm} 分支",
                decl.name
            );
        }
    }
}

/// **BS-4 终检（2）**：`manual()` 逃生舱只有一个实现者（`VectorCompositeNode`）。
///
/// 逃生舱一旦变成正门，"加一个 pass"就不再是"声明 + 分支"了。这里把"只有一处"固定下来。
#[test]
fn only_the_vector_composite_takes_the_manual_escape() {
    let render = include_str!("../src/render.rs");
    assert_eq!(
        render.matches("fn manual(&self) -> bool").count(),
        1,
        "`manual()` 只允许 VectorCompositeNode 实现（其余节点走 execute / execute_at）"
    );
}

/// 纯矢量视图（`VECTOR_PLAN`）：一个填充方块。
#[test]
fn vector_plan_flow_is_pinned() {
    let square = FilledPath::polygon(
        &[
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
        ],
        [1.0, 0.0, 0.0, 1.0],
    );
    let flows = flow(|| {
        render_2d(64, 64, 1, [0.0, 0.0, 0.0, 1.0], &[PathRecord::from(square)])
            .expect("2D 后端起不来");
    });
    assert_eq!(
        flows,
        vec!["atlas raster[] > vector composite[]"],
        "VECTOR_PLAN 的命令流变了：纯路径视图没有光栅对象（atlas 起 0 个子 pass），\
         合成仍要跑（组名在 BS-1 接入 encode 的绑定记账后会填上）"
    );
}

/// 光栅视图（`RASTER_PLAN`）：一个受光网格。
#[test]
fn raster_plan_flow_is_pinned() {
    let mesh = MeshRecord {
        positions: vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]],
        uvs: None,
        normals: Some(vec![[0.0, 0.0, 1.0]; 4]),
        triangles: vec![0, 1, 2, 0, 2, 3],
        fill: [0.8, 0.8, 0.8, 1.0],
        model: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
        material: Some([0.0, 0.9, 0.0]),
        colors: None,
        second: None,
        texture: None,
    };
    let view = View3 {
        frame: (4.0, 4.0),
        center: [0.0, 0.0, 0.0],
        axes: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        zoom: 1.0,
        focal: 8.0,
        size: (64, 64),
        background: [0.0, 0.0, 0.0, 1.0],
        light_source: [0.0, -1.0, 0.0],
        exposure: 2.0,
        tone: Some(ze_render_ir::ToneCurve::AgX),
        bloom: 0.0,
        bloom_threshold: 0.0,
        bloom_radius: 0.0,
        ao: None,
        lights: vec![Light::Sun {
            direction: [0.0, 0.0, -1.0],
            color: [1.0, 1.0, 1.0],
            intensity: 4.0,
            shadows: false,
        }],
    };
    let flows = flow(|| {
        render_3d(&view, &[PathRecord::from(mesh)]).expect("3D 后端起不来");
    });
    assert_eq!(
        flows,
        vec![
            "shadow[] > occlusion depth(-) > occlusion(-) > base[scene,lighting] \
             > base depth[] > vector composite[scene,images] > bloom(-)"
        ],
        "RASTER_PLAN 的命令流变了：没有阴影投射者 / 遮蔽源 / 泛光时，三步都必须显式跳过（`(-)`），\
         不能静默消失；`vector composite` 的组名由 BS-1 的 encode 记账给出"
    );
}
