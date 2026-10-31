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
