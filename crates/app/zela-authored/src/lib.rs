// authored — **场景内容作者**（原住在 app）。
//
// 归位理由：场景怎么摆、有哪些对象、时序怎么排，属于场景层；app 只应做装配、
// 命令提交与事件订阅。
//
// - [`storyboard`]：§1.1 六条分镜的声明处，同时是 `storyboard.lua` / `.py` 的
//   等价性参照基准（生产路径已统一走脚本，见 app 的 `SceneChoice`）。
// - [`capability_demo`]：N 线能力测试 demo（`--demo`）。
// - [`demo3d`]：3D 线框立方体 + 参数曲面 + 内容纹理公式（启动即见的 3D 场景）。

pub mod capability_demo;
pub mod demo3d;
/// CE-e：两个 View + 两个参数面板（场景对象；`ze-interact` 的真消费者）。
pub mod panels;
/// CF-d：节点状态 → 场景 `State` 的映射与校验（弹簧走 BZ-a 门禁）。
pub mod node_state;
/// CF-d：弹簧节点（Vec2 状态）+ lowering 规则表。
pub mod spring;
pub mod storyboard;
