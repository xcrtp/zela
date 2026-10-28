# 渲染后端：选型与路由

> 每个后端的实现细节看模块文档：[`zela-render-backends`](../modules/zela-render-backends.md)、
> [`zela-render-vector`](../modules/zela-render-vector.md)、[`zela-web`](../modules/zela-web.md)。
> 本篇只写「谁负责什么、怎么选、各自的约束」。

## 一句话

三个后端共用**同一份路由决策**（决策在后端层，不在宿主）；默认**矢量优先**，
被拒才回退 wgpu，且回退**必带理由**——`cargo verify --step hashes` 不走路由。

## 一、三个后端

| 后端 | crate | 负责什么 |
|---|---|---|
| wgpu 后端 | [`zela-render-backends`](../../crates/render/zela-render-backends/src/lib.rs) | 2D / 3D 离屏渲染、N 图层合成、合成后滤镜链；`RenderContext`、后端注册、GPU 计时、诊断计数 |
| 矢量后端 | [`zela-render-vector`](../../crates/render/zela-render-vector/src/lib.rs) | 解析抗锯齿的路径光栅化 + 3D 着色栈（tone / bloom / AO / 阴影 / 环境光照）；原生 2D+3D、四边形网格、形变 |
| 浏览器薄壳 | [`zela-web`](../../crates/web/zela-web/src/lib.rs) | 全仓唯一碰 DOM / WebGPU 初始化的地方：取产物、起循环、出帧 |

三者都**不知道场景语义**：不读 ECS 组件、不判可见性，只消费录制层产出的队列与图层声明。
wgpu 后端另有一份「后端自持有」的装配（不依赖 egui），让 headless 与 egui 两条宿主路径跑**同一份**帧体。

## 二、怎么选

路由在 [`routing.rs`](../../crates/render/zela-render-backends/src/routing.rs)，输入只有两样：
**本帧内容事实** + **adapter 事实**（没有负载、没有墙钟、没有随机 ⇒ 决策本身可复现）。

| 偏好（`--backend`） | 尝试顺序 | 语义 |
|---|---|---|
| `auto`（默认） | 矢量 → wgpu | 作品级：第一帧决定后全程固定 |
| `auto-frame` | 矢量 → wgpu | 每帧重算，**诊断专用**（同一作品内可能换画风） |
| `wgpu` | 仅 wgpu | 显式基线路径，不取帧快照、不回退、逐位等同改造前 |
| `vector` | 矢量 → wgpu | 偏好矢量；被拒时回退并打印理由 |

决策本身是**可观测对象**（选了谁 / 为什么 / 谁被拒 / 拒绝理由），拒绝理由为空即视为静默决策并直接失败。

## 三、矢量当前会被什么拒掉

- **adapter 硬前置不齐**（浮点可混合、深度模板格式等）；上下文**没有 adapter** 也拒——
  不允许假设能力可用。
- 本帧有网格材质的纹理**不是本帧内容层**（没有生产方）。
- 有 3D 相机但**没有任何网格**。

已经**不再是**拒绝理由的（历史上曾是）：线框（转细条三角形）、形变（差值形状 + 权重烤进第二组几何）、
2D/3D 混排（2D 记录以屏幕平面叠加进 3D 帧）。详见 GAPS 的 `AD` / `AB` 段。

## 四、平台约束

**矢量后端**：公开面按目标切两套。native 有同步入口（`render_2d` / `render_3d` / `render_scene_2d`），
wasm32 上**只有 `*_async`**（同步签名不存在，避免出现第二条不可用的路径）。
「零 CPU 往返」的入口是把帧合成进宿主给的纹理，但 `Rgba8Unorm → surface` 的 GPU 内 resolve 仍在
——**零 CPU 往返 ≠ 零拷贝**。它不在 xtask 的 wasm gate 名单里，wasm 可编与否由 `zela-web` 的构建保证。

**wgpu 后端**：wasm 构建要用 `--no-default-features --features wgpu`（去掉阻塞式读回）。

**浏览器薄壳**：整个 crate 以目标架构收口，不参与 native 构建；不编译脚本、不提供编辑器。
播放用固定 1/30 时间步，调速只改每次回调推进的帧数；切后端走「重播」而不是整页重载。

## 五、对拍关系

| 事实 | 后果 |
|---|---|
| 显式 `wgpu` 不取帧快照 | 该路径与引入路由前逐位相同，是冻结基线 |
| 基线按「作品 × 后端」**显式对表** | 门禁不经过路由，路由改了不会动基线 |
| 同一个脚本换 `--backend` 不换底色 | 背景 clear 在两条路径同源 |
| 矢量有 tone / bloom / IBL，wgpu 没有 | 两者像素**本来就不该一致**，对拍是各自对各自基线 |

跨后端的等价性只在**能等价的地方**断言（属性测试在
[`vector_vs_wgpu.rs`](../../crates/render/zela-render-backends/tests/vector_vs_wgpu.rs)），
不等价的地方写成显式差异，而不是抹平。

## 六、什么时候该显式指定后端

- **要从冻结基线取证**：用 `--backend wgpu`，该路径不取快照、不回退，与引入路由前逐位相同。
- **要复现「只差某一项」**：用 `vector`，让被拒理由打到日志里，而不是让 `auto` 悄悄回退。
- **要排查同作品内换后端**：用 `auto-frame`（诊断开关），生产不用。

`auto` 才是默认路径；显式值是为了**取证**，不是为了让作品依赖某个后端。

## 相关

- 每帧怎么走：[运行时](../architecture/runtime.md)
- 缺口与已声明的债：[`GAPS.md`](../../GAPS.md)（`S` / `T` / `U` 段是矢量后端的批次记账）
