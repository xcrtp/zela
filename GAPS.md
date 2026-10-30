# 缺口总账（GAPS.md）

> **唯一记账入口**。散落在 `FOURIER_GAPS.md`、`WASM_PORT_SURVEY.md` 与各 crate
> README / `ARCHITECTURE.md` 的「仍缺 / 已知未接线」统一汇总到这里。
>
> 更新：2026-10-19（融合前盘点与收尾）；2026-10-20（S1：G-101 关闭 + R-21 记账）。
> `FOURIER_GAPS.md` 保留为 **fourier 作品快照**；`WASM_PORT_SURVEY.md` 保留为
> **当时的探测证据**，两者正文都注明总账在本文件。
>
> **确定性口径**（详见
> [`FRAME_HASH_BASELINES.md`](crates/app/zela-app/scripts/FRAME_HASH_BASELINES.md#L117-L128)）：
> 同目标可复现 ✅（最硬）· 跨端结构一致 ✅ · 跨端逐位相同 ❌（已知可接受）。
> 本文件谈「确定性」时沿用这三层，不混用。
>
> **第四判据（S0，2026-10-26）**：确定性 = 输入相同 ⇒ 输出相同，**且无隐式可变状态**。
> 「逐位相同」是测试夹具，不是定义本身。本判据可直接变成检查项：全局可变静态 / 墙钟 /
> 随机 / 系统环境 / 进程计数。它把三个分头修过的坑统一为同一规则的实例
> （W1-01 `MeshGeometry.id` 的 `AtomicU64`、X1-11 系统字体、#93 预记 `random/time`），
> 详见 AF 段（AF-01..AF-05）。

## 状态定义（四类 + 两个补充 + 已记录）

| 状态 | 含义 | 判定要求 |
|---|---|---|
| **可清** | 代码里已经做了，或范围已消失 | 必须给出具体代码位置 |
| **描述不准，需改写** | 事实已变，条目本身误导 | 给出新事实与证据 |
| **仍成立** | 缺口仍存在 | 写清影响层与证据 |
| **升格 / 降为工效项** | 成为前置阻塞（P0）或降为工效项 | 写清理由 |
| 范围外 | 本仓库/浏览器路线明确不做 | 写清替代路线 |
| 待定 | 证据不足或依赖未定决策 | 不得为了清而清 |
| **已记录** | 实现选择不同，**两边都对**（B19 V-08） | 写清参照物、不再收敛的理由，并声明不构成门禁失败 |

---

## P0 — 2D/3D 融合前置

| 编号 | 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| **G-101** | `WorldTransforms` 只有 2D 仿射；3D 录制只读 local `Transform`，3D 父子层级未接线 | 场景层 → 渲染层 | **升格（原 F-02）→ S1 已关闭** | 关闭于 S1：结构层提交 `84cabeb`（[`transform_hierarchy.rs`](crates/scene/zela-scene-ecs/src/transform_hierarchy.rs#L39-L65) 改存 `Transform3D` + `get2d` 投影）、消费方接线提交 `3ed493f`（[`record_systems.rs`](crates/render/zela-render/src/record_systems.rs) 3D 录制 `Reads = (WorldTransforms,)`）；像素对拍 fourier / storyboard 与基线逐位一致（冻结态通过） |
| **G-101 补证（S0-1，2026-10-20）** | S1 零扰动已实证：h_after（HEAD `ba75687`，S1 后）== h_before（`ff2751d`，S1 前）== 同一对哈希 fourier `b6eaa864933847ff…` / storyboard `6de42d6b15943c…`（均用当前 ZELA 口径重跑）。注：计划原定 h_before 取 `84cabeb^`，但该中间提交 `3ed493f` 不可编译（record_systems 已接线而 draw.rs 仍传 Mat3），故取真正可编译的 S1 前 `ff2751d`（`3ed493f^`）—— 以代码为准，见执行报告 §六.4 |

> **消息误标注明（S0-4）**：`3ed493f` 的提交消息写「WorldTransforms 换型为 3D（结构层）」，实际内容只是 `record_systems.rs` 的消费方接线（结构层本体在 `84cabeb`）。该提交已在 `origin/feat/2d3d-fusion` 上（共享分支），故不改写历史，仅在此记账。

> **为什么是 P0**：只要融合要求 2D 与 3D 对象进入**同一父子层级**（或 3D 对象跟随
> 父对象变换），当前 3D 录制不消费 `WorldTransforms` 就直接失效。若融合只做
> 「同屏按层叠加」，则现有层合成已够用 —— 这一点需要融合形态确认，但
> **3D 层级本身是确定性缺口**，不依赖融合也存在。

**待定（不作为 P0）**：跨 2D/3D 的统一相机 / 深度合成模型。当前 2D 与 3D
各自持有相机、按 `SceneView::layers` 顺序合成（无跨层深度）。是否需要统一，
取决于融合是「同屏叠加」还是「同一深度空间互遮挡」—— 后者才构成前置。

---

## A. 作品快照（原 `FOURIER_GAPS.md` 14 条）

| 编号 | 一句话描述 | 影响层 | 状态 | 证据 / 说明 |
|---|---|---|---|---|
| F-01 | 脚本 `Curve.Keyframes` 只有标量 | 脚本层 | **描述不准，已改写** | 内核 [`AnimValue`](crates/kernel/ze-time/src/curve.rs#L16-L23) 已支持 `Scalar/Vec2/Vec3/Color/Path`，`Curve::Keyframes` 也用 `AnimValue`；缺口只在脚本入口 [`types.rs`](crates/scene/ze-scene-api/src/types.rs#L468) 的 `Vec<ScalarKey>` |
| F-02 | `WorldTransforms` 只有 2D 仿射 | 场景/渲染 | **升格 P0（G-101）** | 同上 |
| F-03 | `MorphRecordSystem` / `ContentTextureRecordSystem` 不消费世界变换 | 渲染 | 仍成立 | [`record_systems.rs`](crates/render/zela-render/src/record_systems.rs#L133-L175)、[`L249-L272`](crates/render/zela-render/src/record_systems.rs#L249-L272) 的 `Reads` 均无 `WorldTransforms` |
| F-04 | `Opacity` 不沿 `Parent` 传播 | 渲染 | 仍成立 | [`draw.rs`](crates/render/zela-render/src/draw.rs#L482-L487) 只读对象自身组件；无祖先链查询 |
| F-05 | 父子环路只写 `TransformHierarchyError` 资源 | 场景 | 仍成立 | 生产侧仅 [`zela-playback`](crates/app/zela-playback/src/lib.rs#L196-L198) 插入默认值；无宿主升级为帧错误 |
| F-06 | `Reveal` 世界变换在 CPU 侧逐帧折算路径 | 渲染性能 | 仍成立 | [`record_systems.rs`](crates/render/zela-render/src/record_systems.rs#L216-L235) 每帧 `path.transformed(...)`（克隆 + 变换） |
| F-07 | `clip_transform` 是独立函数，不是 `Target.Transform` 形态 | 脚本 API | **可清** | 能力已由 [`api.rs`](crates/scene/ze-scene-api/src/api.rs#L108) 提供；计划中的枚举形态从未落地，语义等价且可并行写不同通道 |
| F-08 | Text 缺颜色 / 对齐 / 尺寸回读 | 内容/脚本 | 仍成立 | [`Text`](crates/scene/ze-scene-api/src/types.rs#L421-L426) 只有 `source/font_size/x/y`；[`ContentApi`](crates/scene/ze-scene-api/src/api.rs#L142) 返回 `PathList`，无排版度量回读 |
| F-09 | `Axes2D` 无刻度标签 | 对象层 | 仍成立 | [`axes.rs`](crates/scene/ze-scene-core/src/object/axes.rs#L204-L221) 只产 `x_ticks/y_ticks/grid_lines/axis_lines`，无文字 |
| F-10 | 无 ZOrder 脚本入口 | 脚本层 | 仍成立 | `ZOrder` 组件存在（[`draw.rs`](crates/kernel/ze-render-ir/src/draw.rs#L23)），[`SceneApi`](crates/scene/ze-scene-api/src/api.rs#L37) 无构造器 |
| F-11 | 无 `fit_bounds` 脚本入口 | 脚本层 | 仍成立 | [`Camera2D::fit_bounds`](crates/scene/ze-scene-core/src/object/camera.rs#L117) 已存在；脚本 API 无对应函数 |
| F-12 | Python `from` 关键字迫使位置参数 | 语言适配器 | 仍成立 | 脚本内注明：[`fourier.py`](crates/app/zela-app/scripts/fourier.py#L18) |
| F-13 | 无虚线 / 线帽 / 渐变 | 渲染 IR | 仍成立 | [`Style2D`](crates/kernel/ze-render-ir/src/shape.rs#L181-L194) 只有 fill/stroke/width/space |
| F-14 | `Parent` 只接受脚本句柄 | 脚本层 | 仍成立 | [`Parent`](crates/scene/ze-scene-api/src/types.rs#L238-L240) 为 `Handle`；无按名字/路径重挂 API |

**A 段小计**：可清 1 · 描述不准 1 · 升格 P0 1 · 仍成立 11。

---

## B. WASM 移植探测（`WASM_PORT_SURVEY.md`）

| 编号 | 原项 | 状态 | 证据 / 说明 |
|---|---|---|---|
| W-01 | P1-1 字体按路径读盘 | **可清** | 浏览器只回放 ZSB；字形在 native 已烘焙为路径，`zela-web` 无字体代码 |
| W-02 | P1-2 同步 GPU 回读 | **可清** | [`read_texture_async`](crates/render/zela-render-backends/src/render_context.rs#L171) |
| W-03 | P1-3 上下文创建 `block_on` | **可清** | [`request_adapter_and_device`](crates/render/zela-render-backends/src/render_context.rs#L63)；`blocking` feature 隔离 |
| W-04 | P1-4 导出 fs / ffmpeg | 范围外 | 浏览器不导出；ZSB 路线绕开 |
| W-05 | P1-5 `zela-gui` 关不掉下层默认 features | **降为工效项** | `zela-gui` 不进浏览器 ⇒ 与浏览器无关，仅工效项 |
| W-06 | P2-1 `gpu_test` 的 libc / sleep / spawn | **可清** | 归入 `blocking` feature；wasm 构建不编译 |
| W-07 | P2-2 `Receipt::wait` 忙等 | **可清** | [`DriverPod::submit`](crates/scene/zela-scene-ecs/src/driver.rs#L132) 同步跑完 `run_frames` 后 `complete()`，`yield_now` 走不到 |
| W-08 | P2-3 CI wasm gate 未覆盖 | **可清** | [`.github/workflows/architecture.yml`](.github/workflows/architecture.yml) 的 Playback runtime gate |
| W-09 | P2-4 `ze-pod-py` wasm | 范围外 | 浏览器不启用 Python Pod |

**B 段小计**：可清 6 · 降为工效项 1 · 范围外 2。

---

## C. README / ARCHITECTURE 散落项

> 盘点结果：真正带「仍缺 / 已知未接线」正文的 README 是 **6 个**
> （`ze-time`、`ze-scene-contract`、`zela-render`、`ze-pod-lua`、`ze-pod-py`），
> 其余 README 没有缺口小节；`ARCHITECTURE.md` 另有一张「已知未接线项」表。

| 编号 | 缺口 | 来源 | 状态 | 证据 / 说明 |
|---|---|---|---|---|
| R-01 | `Curve::PathMorph` 已就绪未接线 | `ze-time` README | 仍成立 | 只有定义与单测（[`curve.rs`](crates/kernel/ze-time/src/curve.rs#L151)），无 `render/`、`app/` 引用 |
| R-02 | `ObjectId::is_stale` 无生产消费方 | `ze-scene-contract` README | 仍成立 | 仅单测；销毁-重建流程未引入 |
| R-03 | `record_scene` 不消费 `Descriptors` | `ze-scene-contract` README | 仍成立 | 消费方是构造期 `apply`；录制层直接吃 `Style2D` |
| R-04 | `Shape2D` 无 `Text/Image` 变体 | `zela-render` README | 仍成立 | 内容以路径形式进入 `Path/Polyline` |
| R-05 | `Parent` 零生产消费方 | `ze-scene-contract` README / `ARCHITECTURE.md` | **可清（文档已修）** | S5 已接线：[`transform_hierarchy.rs`](crates/scene/zela-scene-ecs/src/transform_hierarchy.rs)、[`record_systems.rs`](crates/render/zela-render/src/record_systems.rs#L86) |
| R-06 | `Path3D` 零消费方 | `ARCHITECTURE.md` | 仍成立 | 与 `Path2D` 不同构，接入是破坏性变更 |
| R-07 | `glam::` 未完全收敛到 `ze-notion` | `ARCHITECTURE.md` | 仍成立 | 2026-10-19 实测 **25 次**（口径：`*.rs` 中 `glam::` 出现次数，含注释、含 3 个豁免 crate；不含 `.md` 与根 `Cargo.toml`）；`zela-scene` 代码引用已归零 |
| R-08 | 节点编辑器冻结（已排期） | `ARCHITECTURE.md` | 仍成立（排期项） | 零生产消费方；等 UI 接口稳定 |
| R-09 | 2D / 3D 录制 System 分居 | `ARCHITECTURE.md` | 仍成立 | 现已同住 `zela-render::record_systems`，但仍是两个 System / 两个队列 |
| R-10 | CPU 回放路径不支持真合成 | `ARCHITECTURE.md` | **可清（文档已修）** | 旧 `egui_2d` 已在 S4.1 删除（[`zela-egui` README](crates/ui/zela-egui/README.md#L140-L145)） |
| R-11 | `formula.rs` 归位未定 | `ARCHITECTURE.md` | 仍成立 | 归位目标 crate 有争议 |
| R-12 | `tessellate` 下沉未做 | `ARCHITECTURE.md` | 仍成立 | 目前与 2D 后端同层 |
| R-13 | 非原生 Pod 的 `FunctionX2Y` 适配器 | `ARCHITECTURE.md` | 仍成立 | 脚本只承担装配期，不参与每帧采样 |
| R-14 | 每层多一次全屏 blit | `ARCHITECTURE.md` | 仍成立 | 层数少时可接受 |
| R-15 | 3D 离屏无 MSAA（1x） | `ARCHITECTURE.md` | 仍成立 | `wgpu_3d` / `compositor` 的 `sample_count: 1` |
| R-16 | `opacity × blend` 未穷举验证 | `ARCHITECTURE.md` | 仍成立 | 测试覆盖缺口 |
| R-17 | 3D 无深度缓冲 | `ARCHITECTURE.md` | **可清（文档已修）** | [`DEPTH_FORMAT = Depth32Float`](crates/render/zela-render-backends/src/wgpu_3d.rs#L98) 且两个 pass 都挂深度附件 |
| R-18 | `ze-pod-lua` / `ze-pod-py` wasm 未做 | 两个 README | 范围外 | 浏览器只回放 ZSB，不运行脚本；不进 wasm gate |
| R-19 | 节点图组直接依赖 glam（数学入口第二套）：`zela-node-editor`(10) / `zela-node-egui`(6) / `zela-node-egui-ui`(1) | 节点图组 | 仍成立（不违规，记账） | 三者不依赖 `ze-notion`，按 M0 不违规；CI 已显式豁免并注明理由。**触发条件：合成图（ComposerGraph）接入场景前，必须先统一到 `ze-notion`**；此前不改代码（当前零消费方，避免为未发生的需求付代价） |
| R-20 | `Shape2D::Custom` 在结构指纹 topo 中统一输出 `"custom"`，不同 `SpecId` 的自定义图元在结构层不可区分 | 结构对拍 | 仍成立（记账，不修代码） | 现状可接受：spec 是具名字符串（非注册序号），且当前仓库**没有 Custom 的生产者**（只有测试），不影响对拍。**触发条件：Custom 出现真实生产者时，须把 spec 名写进 topo** |
| R-21 | `Transform3D::to_affine2d` 是**投影**：`z` 平移与 xy↔z 耦合被丢弃（D4） | 理念层 `ze-notion` | 仍成立（记账，行为如此） | [`transform.rs`](crates/kernel/ze-notion/src/transform.rs#L115-L134) doc 已写明"投影，非同构"；2D 对象挂 3D 父下是 S6 的目标形态，本轮不报错也不兜底。**消亡条件：S6 之后复核**（2D 进 3D 后该投影是否还有消费方） |

**C 段小计**：可清 3（含 2 条纯文档漂移 + `Parent`）· 仍成立 16 · 排期 1（计入仍成立）·
范围外 1。

---

## D. B1 批次记账（光照数据化与 PBR，2026-10-21）

> 计划《S1收尾与B1_光照数据化与PBR执行计划.md》B1-A~F 已全部落地（逐笔提交）。
> 本段：**已关闭**（硬编码 / 无提示兜底类缺口）与**新增能力**。像素为演进态
> （`baselines.txt` 的 reason + 提交号见 §统计后的维护约定与录制段）。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| B-01 | `cube.wgsl` 硬编码光源（`LIGHT_DIR` / `AMBIENT`）→ 场景级 light uniform buffer 驱动 | 着色器 | **已关闭（B1-A）** | 提交 `5743460`；`Light` 组件不带 position（唯一来源，位置走 `Transform` → `WorldTransforms`） |
| B-02 | 点光缺失：新增 Point light（`range` 窗口衰减避免 1/d² 爆炸）；数量超编译期上限 ⇒ **显式报错** | 渲染 | 新增能力（B1-B） | 提交 `8efc515`；位置走 `Transform` → `WorldTransforms`（S1 已通） |
| B-03 | PBR 缺失：新增 `ShaderVariant::Pbr`（metallic-roughness，线性空间）；法线贴图走**屏幕空间 TBN**（Mikkelsen） | 渲染 | 新增能力（B1-C） | 提交 `1970c2d`；未改顶点格式（无 tangent 属性槽） |
| B-04 | 脚本层无 3D 入口（3D 场景硬编码在 `authored/demo3d.rs`）→ `camera3d` / `cube` / `parametric_surface` / `material3d` / `light` **参数化构造器** | 脚本 → 场景 | **已关闭（B1-D）** | 提交 `6d4a1c9`；不传顶点数组（`MeshGeometry` 无 `Serialize`，撞"图元可序列化"硬限制）⇒ 3D 首入 `--script == --bundle` 对拍网；demo 入册提交 `6c3376e` |
| B-05 | pipeline `(topology, variant)` **笛卡尔积预建**（3×2=6 条，随 variant 数爆炸）→ `get_or_create_pipeline` **按需构建 + 缓存** | 渲染后端 | **已关闭（B1-E）** | 提交 `797c914`；key 四维定齐 `(topology, variant, cull_mode, blending)` —— B2 只增取值不再动机制；结构判据 `test_b1e_pipelines_built_on_demand`（0→1→1→2→3） |
| B-06 | pipeline 未命中**自动回退 Lambert**（仅 debug 断言）+ 点列表**无提示丢弃** | 渲染后端 | **已关闭（B1-E/F）** | `797c914` 结构性删除回退（get_or_create 无"漏建"状态）；`7a9299f` 点列表改 `panic!` 显式报错（失败必须显式），`test_b1f_points_topology_panics_explicitly` |

**D 段小计**：已关闭 4 · 新增能力 2（不计入缺口类别）。

> **澄清（B2-0e）**：B1 验收里「像素未变」仅指 **2D 基线集**（`fourier` / `storyboard` /
> `smoke` 均无 3D 内容）；3D 回归由 **23 条 GPU 像素单测**保障，**3D 尚未进
> frame hash 对拍网**（`baselines.txt` 无 3D 条目）。3D 的逐位对拍在 S6/后续批次
> 接入基线窗口前，缺口保持可见。

---

## E. B2 批次记账（管线状态 / 队列统一 / 2D 进 3D，2026-10-21）

> 计划《B2执行计划_管线状态队列统一与2D进3D.md》B2-0 ~ B2-5 已落地（逐笔提交）。
> 本段记录**已关闭项**与**新增/仍成立的缺口**；B2-6 ~ B2-9 完成后继续追加。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| E-01 | `PipelineKey` 含 wgpu 类型（`Option<wgpu::Face>` / `BlendState`）→ 改为 IR `CullMode` / `LayerBlend`，并补第五维 `VertexLayout`（D15） | 渲染后端 | **已关闭（B2-2）** | `grep -n "struct PipelineKey" -A 8` 零 wgpu 类型；`test_b2_2_pipeline_state_dimensions`（1→2→3 条） |
| E-02 | `cull_mode` 由拓扑推导、`blend` 硬编码 `REPLACE`（#2/#3）→ 意图层 `Material3D.double_sided` / `transparent` 驱动 | 渲染 / 场景组件 | **已关闭（B2-2）** | `test_b2_2_double_sided_visible_from_back`（背面可见） |
| E-03 | 2D / 3D 两个队列、3D 无层序（#11/D2）→ **一个 `DrawQueue`**（2D 形状 + 3D 网格/线段共存）+ 统一 `ZOrder`（IR 唯一声明，`ze_scene_core::ZOrder` re-export） | 渲染管线 | **已关闭（B2-1）** | `test_queue_stable_within_equal_zorder`（R-F 显式锁定）、`test_unified_queue_mixes_2d_and_3d_by_zorder` |
| E-04 | 2D 图元无法参与 3D（融合缺口）→ `derive_mesh`（Rect/Circle/Ellipse/Polygon/Line/**Path**/**Group** → 平面网格，Flat + double_sided 材质默认化） | 渲染管线 | **新增能力（B2-3；B3 Step 1 扩到 7 种）** | `test_b2_3_derived_2d_rect_renders_in_3d`；B3 Step 1 起统一走 `tessellate_with`（lyon），手写三角化已删；`Polyline`/`Custom` 仍显式报错（不做无提示跳过） |
| E-05 | 透明物体无排序（无透明桶）→ 不透明先画、透明按 `ZOrder` 升序后画；透明 pipeline **不写深度**（R-A） | 渲染后端 | **已关闭（B2-3）** | `test_b2_3_transparent_bucket_orders_by_zorder`（对调 ZOrder 后主导色对调） |
| E-06 | 共面 z-fighting → `ZOrder × Z_ORDER_EPSILON` 的 **normal offset**（不用常量 depth bias，D4） | 渲染后端 | **已关闭（B2-4）** | `test_b2_4_epsilon_from_zorder_resolves_coplanar`；`DepthBiasState::default()` 未改 |
| E-07 | 3D 无 AA（1x）vs 2D 4x（#6/#8）→ 3D MSAA 4x（`DEFAULT_MSAA_SAMPLES`），采样数经 `new_with_samples` 由能力侧给定 | 渲染后端 | **已关闭（B2-5）** | `test_b2_5_msaa_4x_produces_partial_coverage`（4x 部分覆盖像素更多） |
| E-08 | **纹理走样**：MSAA 治不了纹理采样走样（`Material3D.texture` / `ContentTexture` 进 3D 后会有） | 渲染后端 | **仍成立（已知缺口）** | 需要 **mipmap + 各向异性过滤**；本轮不做（计划 B2-5 记账要求）。B3 Step 4b 起 `SurfaceTexture` 显式声明 `filter` / `mipmap`（Fixed `texel_size`），但**mipmap 生成未实现** ⇒ 2D 内容进 3D 同样受本条约束 |
| E-09 | 硬件 MSAA **resolve 的跨端确定性**是否为 `[browser]` 段"1 帧 1 像素"差异的贡献者 | 对拍 | **待验证（不是断言）** | 本轮无浏览器环境（D5：跨端重录推迟）；`[browser]` 段仍仅存档 |
| E-10 | 2D 派生网格仅覆盖可直接三角化的图元（`Path` 需要 lyon；`Group` 需要递归） | 渲染管线 | **已关闭（B3 Step 1）** | 复用 `tessellate_with`：7 种图元可派生；剩余 `Polyline` / `Custom` 仍 `ShapeMeshError::Unsupported` 显式报错 |
| E-11 | 双槽基线**没有差分视图**（两个孤立值，无法看出"当前值落在哪一槽"） | 对拍工具 | **已关闭（B2-6）** | xtask `render_diff_report`（诊断旁路，不参与门禁）；`diff_report_tests` 3 条 |
| E-12 | 帧级归因缺**分层标签**（无法定位首帧分歧） | 对拍 / 可观测性 | **已关闭（B2-6）** | `ZELA_FRAME_DIGEST_SAMPLE=N` 逐帧 digest 采样；**不进序列哈希链**（D7） |
| E-13 | 脚本看不到 B2 的意图层（`double_sided` / `transparent`） | 脚本 API | **已关闭（B2-7）** | pod `Material3D` 增两字段（D19：不暴露 GPU 机制）；`zela.scene` version 5 → **6** |
| E-14 | 新能力缺展示脚本 | Demo | **已关闭（B2-7）** | `fusion.lua`（双面 + 透明相交体）入册 `demo/manifest.txt`，payload-hash `-` |
| E-15 | 3D **未进 frame hash 对拍网**（B1 遗留） | 对拍 | **已关闭（B2-8）** | `demo3d` 升级为 fixture（Q4 决议）；hashes 覆盖 fourier / storyboard / demo3d；`[browser]` 仍仅存档 |
| E-16 | 能力 API 改了可能**忘记递增 version** | 脚本 API | **已关闭（B2-9）** | `capability_fingerprint`（FNV-1a，跨 Rust 稳定）+ 锁定常量；`cargo verify --step structure` 内跑护栏（变而未递增 ⇒ **fail**） |
| E-17 | 逐帧 digest 旁路**仅 native**（`zela-render` 二进制）；浏览器路径未接 | 可观测性 | **仍成立（范围限制）** | wasm 无环境变量；浏览器归因留待跨端重录（D5） |

**E 段小计（B2-0 ~ B2-9）**：已关闭 **13**（含 E-10，B3 Step 1）· 新增能力 1 · 仍成立/待验证 **3**（E-08 mipmap、E-09 跨端 resolve、E-17 旁路仅 native）。

---

## F. B3 批次记账（顶点生产与后端解耦，2026-10-21）

> 计划《B3-0执行计划_顶点生产与后端解耦.md》：纯结构调整，**像素冻结**。
> 目标是把"生产 + 打包"从 backend 移到渲染无关层，使组合从 N×M 降为 N+M。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| F-01 | 3D 顶点生产（`interleaved_pos_uv_normal` / `fallback_uvs` / line 内联）**全在后端私有函数** | 渲染后端 | **已关闭（B3-0b）** | 迁至 `zela-render::vertex_record`；`grep fn interleaved_pos_uv_normal\|fn fallback_uvs crates/render/zela-render-backends/` = 0；line 走 `line_vertex_buffer`（B7） |
| F-02 | 2D 三角化调用与顶点打包在后端（`tessellate` + `bytes_of(&vertices)`） | 渲染后端 | **已关闭（B3-0c）** | `tessellate_to_buffers` / `morph_endpoint_buffers` / `tessellate_reveal_to_buffer`；后端只拼接字节（`CachedGeometry { data: VertexBufferData }`） |
| F-03 | 三角化失败 `eprintln + continue`（跳过且不报错） | 渲染后端 | **已关闭（B3-0d）** | 改显式 panic；`gpu_unregistered_custom_shape_panics_explicitly`（`should_panic`） |
| F-04 | 缓存结构 2D 两级 / 3D 一级**未统一** | 渲染后端 | **仍成立（B4：本批不动）** | 2D 仍 `geom_cache → batch_cache`；3D 仍一级 `CachedMeshBucket` |
| F-05 | 顶点生产**未提前**到场景录制（2D 容差依赖相机 scale） | 渲染管线 | **仍成立（有意）** | 生产仍在渲染阶段执行，只是挪出 backend crate（计划 §一 的明确约束） |

**F 段小计（B3-0）**：已关闭 3 · 仍成立 2（均为计划明确范围外/有意保留）。

---

## G. B3-0.5 批次记账（Backend 抽象与 Pod 化，2026-10-21）

> 计划《B3-0.5执行计划_Backend抽象与Pod化.md》：把"跨后端契约"从 wgpu 类型里剥出来，
> 并纠正像素口径（**同后端 + 同机器 = 测试夹具**，不再承诺跨端逐像素一致）。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| G-01 | `PodValue` 无法承载 bulk 字节（只能 `Array(U8)`，与 `Vec<u8>` 语义撞车） | Pod 层 | **已关闭（B3-0.5a）** | `PodBytes` newtype + `PodValue::Bytes`；`describe()` / `mismatch()` 补分支；`vec_u8_still_uses_array` 防回归；脚本绑定层显式报错（报错而非跳过） |
| G-02 | 渲染目标描述（尺寸 / 格式 / 采样数）留在后端，IR 侧无契约 | 渲染 IR | **已关闭（B3-0.5b）** | `ze_render_ir::target::RenderTargetDesc`（serde + Hash + `validate()`）；`ColorFormat` 补 Bgra 两型；旧 `RenderTarget` 仍在 backend 持 GPU 句柄 |
| G-03 | 四个 render capability（2d / 3d / mixed / shapes）没有签名集与指纹 | 渲染 IR | **已关闭（B3-0.5c）** | `ze_render_ir::caps`：`SIGNATURES` / `signatures_of` / `fingerprints` / `PINNED_FINGERPRINTS` / `negotiate`；`structure` 步跑 `render_fingerprints_are_pinned` |
| G-04 | 后端"实现了哪些 capability"没有显式声明，部分实现会失败且无提示 | 渲染后端 | **已关闭（B3-0.5d）** | `zela_render_backends::render_backend::{IMPLEMENTED_NAMESPACES, implemented_capabilities, negotiate_capabilities}`；`negotiation_fails_explicitly_for_shapes` |
| G-05 | `zela-render` 带 `feature = "wgpu"`，GPU 侧模块与渲染无关层混居 | 分层 | **已关闭（B3-0.5e）** | 四个模块迁入 backends；`default = []` 且无 wgpu 依赖；`cargo tree -p zela-render \| grep -c wgpu`：**12 → 0** |
| G-06 | `zela.render.shapes` **wgpu 未实现**（需要直接吃 `Path2D` 控制点的解析抗锯齿 / 矢量化渲染） | 渲染后端 | **仍成立（范围限制）** | `implemented_capabilities().shapes == false`；请求它 ⇒ 显式 `CapabilityNegotiationError`（不假装实现） |
| G-07 | `Capability` trait 的 `from_negotiated` 与 **typed 快路径**（`Pod::resolve()`）未落地；当前契约是签名集 + 后端自述集合，跨边界调用仍走 typed trait | Pod / 后端 | **仍成立（D4.4 后续）** | 本轮只做契约与协商；`PodBytes` 是契约层表示，typed 快路径是可选优化（不改变语义） |
| G-08 | `Compositor` 未 pod 化、`COMPOSITE_QUAD` 未外迁 | 渲染后端 | **明确不做（§九）** | 随 Compositor 单独批次处理 |
| G-09 | `FRAME_HASH_BASELINES.md` 旧标题/首段把像素一致性写成了跨运行时承诺 | 文档 | **已纠正（B3-0.5e）** | 标题改「同后端 + 同机器」；写明像素 = 测试夹具、跨端不追求一致、`[browser]` 仅存档；`baselines.txt` 注释同步 |

**G 段小计（B3-0.5）**：已关闭 6 · 仍成立 2（G-06 / G-07）· 明确不做 1（G-08）。

---

## H. B3-1 批次记账（场景层分层重构，2026-10-21）

> 计划《B3-1执行计划_场景层分层重构.md》：把 12k 行的 `zela-scene` 分成
> **概念（core）/ 实现（ecs）/ 能力（api）**，作品移出。**纯结构搬迁，像素必须不变**。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| H-01 | `pod/types.rs` / `scene_service.rs` 反向依赖 ECS 内部（`crate::animation` / `crate::scene::components` / `crate::scene_script`） | 场景层 | **已关闭（B3-1a/b）** | `grep "^use crate::" ze-scene-api/src` = 0（仅剩 crate 内 `crate::types` / `crate::content_service`） |
| H-02 | `ze-object` 与 `zela-scene` 组件/纯逻辑混居 | 分层 | **已关闭（B3-1b/f）** | `ze-scene-core::object` 承接全部对象；`ze-object` crate 已删除（`cargo tree` 中无该包） |
| H-03 | 动画目标 `Target::apply(&mut WorldScene)` 把 hecs 适配器写进"数据层" | 核心层 | **已关闭（B3-1b）** | 新增 `ze_scene_core::scene_write::{SceneWrite, with_component}`（object-safe 擦除写）；ECS 用组件登记表实现分发 |
| H-04 | 组件登记表 / persist / math_pod 写在 ECS 实现里，脚本数据（core）却要依赖它 | 核心层 | **已关闭（B3-1b）** | `registry` / `persist` / `math_pod` 下沉 core；ECS 的 `add_known` 与 `SceneWrite` 共用导出的 `known_component_table!` |
| H-05 | `pod/` 与实现同 crate，语言边界不独立 | 场景层 | **已关闭（B3-1c）** | 新 crate `ze-scene-api`；`cargo tree` 中 `zela-` / `hecs` = **0**；能力指纹未变 |
| H-06 | `zela-scene` 自称"契约实现"却有 11k 行 | 场景层 | **已关闭（B3-1d）** | 实现改名 `zela-scene-ecs`；`zela-scene` 变壳层（re-export） |
| H-07 | `authored/`（分镜 / demo）住在场景层 | 场景层 | **已关闭（B3-1e）** | 独立 `zela-authored`；`zela-scene-ecs` 内无 `authored` |
| H-08 | `ze-scene-core` / `ze-scene-api` 的"不能依赖实现"只是约定 | CI | **已关闭（B3-1f）** | `structure` 步新增 jq 判据（`JQ_SCENE_LAYERS`） |
| H-09 | `Target` 的 `apply` 签名变化（`&mut dyn SceneWrite`）会波及下游实现者 | 公开 API | **仍成立（有意）** | 仓内只有 `authored` 测试实现；这是分层必要的破坏性变更，未留兼容 shim |
| H-10 | `scene_script.rs` 的 `commit` 用例需要真实 hecs 场景 | 测试布局 | **已关闭（B3-1b）** | 用例迁至 `zela-scene-ecs/src/scene_script_tests.rs`（实现与用例分离，语义不变） |
| H-11 | `zela-scene` 壳层与 `ze-object` 壳层的删除时机 | 兼容层 | **已关闭（B3-1 收尾）** | `ze-object` 与 `zela-scene` 壳层均已删除；237 处旧 `zela_scene` 路径改为 `zela_scene_ecs::`；ze-macro 派生代码指向 `ze_scene_core::transform`（不再需要任何 `zela_scene` 路径） |
| H-12 | `WGPUDevice` / `CPUDevice` 仍在组件层（D7） | 场景核心 | **已关闭（B3-1 收尾）** | 全仓零消费方 ⇒ 删除 `components::marker` 整块；`zela-render::components` 的转发同步移除 |
| H-13 | `zela-render` 生产依赖场景 **ECS 实现**（`zela-scene-ecs`，45 处引用；含 `WorldView` / `WorldTransforms`） | 分层 | **已关闭（B3-1 收尾）** | `WorldTransforms` 下沉 core；新增 `ze-scene-hecs`（`SceneRead` 只读适配器 + `ObjectId↔Entity`）；`cargo tree -p zela-render \| grep zela-scene-ecs` = **0** |

**H 段小计（B3-1）**：已关闭 12 · 仍成立 1（H-09）。

---

## H-bis. B3-P 批次记账（JS Pod，2026-10-21）

> 计划《B3-P执行计划_JS_Pod.md》：新增 `ze-pod-js`（boa，纯 Rust），
> 与 Lua / Python 同形地复用 `ze-pod-binding`；**只接装配期**，不碰回放。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| P-01 | `host-js`（浏览器原生引擎）**未接线** | JS 适配器 | **明确不做（B3-Pe 可选）** | `ze-pod-js/src/lib.rs` 两条 `compile_error!`：非 wasm 启用 ⇒ 编译期报错（J3）；wasm 启用 ⇒ 显式"尚未接线"（不做无提示降级） |
| P-02 | wasm 体积尚未基线化 | 构建 | 仍成立 | `js` feature 默认不开；`zela-web` 未引用 `ze-pod-js` ⇒ 浏览器产物体积不变（未变即无需记录前后值） |
| P-03 | JS 只有 `number`（f64） | JS 适配器 | **已处理** | `exact_integer()` 显式校验整值 / 范围（i32 / u32 / u8 / u64 各自边界）；u64 上限取 `2^53-1`（精确整数）；越界报错不截断 |
| P-04 | 语言适配器不在 wasm gate 名单 | CI | **已关闭（A1）** | xtask 新增 `OPTIONAL_WASM_CRATES = ["ze-pod-js"]`，`step_wasm` 对其实跑 `cargo check --target wasm32-unknown-unknown`；lua / py 仍不进（mlua 不适配 wasm 内核路径、pyo3 编不到） |
| P-05 | **wasm 运行时对拍未做** | JS 适配器 | 仍成立 | 本机无浏览器宿主：只做了 `cargo check --target wasm32`（编译期）与 native 运行时用例；"同一脚本在浏览器里行为一致"尚未被任何用例覆盖 |

**H-bis 段小计（B3-P）**：已关闭 1（P-04）· 已处理 1（P-03）· 明确不做 1（P-01）· 仍成立 2（P-02 / P-05）。

---

## I. B3-2 批次记账（Render Pod 契约探针，2026-10-21）

> 计划《Pod 线：JS Pod 收尾 + Render Pod 探针》B 组：`zela.render2d` 的四条签名
> （`ze_render_ir/src/caps.rs:24-29`）从未被实现，却已被 `PINNED_FINGERPRINTS`
> 锁定。本段记录**真实实现**（`zela-render-backends/src/render_pod_probe.rs`，
> 仅 `#[cfg(test)]`）得到的结论。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| R-01 | 四条签名从未被实现却被指纹门禁冻结 | 渲染 IR | **已闭合（B3-2）** | `RenderProbePod` 真实现 `capabilities` / `create_target` / `upload` / `draw_2d`；typed 直调与 Pod 边界两条路径**逐字节相同**（`pod_boundary_matches_typed_path`） |
| R-02 | `upload(PodBytes) -> Handle` **每次调用新开槽**；契约无 release / 原地更新 | 渲染 IR | **仍成立（有意保留）** | `upload_allocates_a_new_slot_per_call` 取证：两次 upload 得到不同槽位、旧句柄仍存活；生产接线前必须补 `release(Handle)`（或 `upload_into(Handle, PodBytes)`）。本轮**不改签名**：没有第二个消费方时冻结错误形状的风险高于泄漏风险 |
| R-03 | `RenderTargetDesc.format` / `sample_count` 被 2D 后端忽略 | 渲染 IR / 后端 | **仍成立（探针显式收窄）** | `wgpu_2d.rs:630-651`：离屏固定 `OFFSCREEN_FORMAT` + `MSAA_SAMPLES`。探针对非 `Rgba8Unorm` / 非 4x 的 desc **显式报错**（`target_desc_is_narrowed_explicitly`）；契约侧应改用 `capabilities()` 查询（返回类型目前未定义） |
| R-04 | `draw_2d` 无法表达**视口 / 相机 / 清屏** | 渲染 IR | **仍成立** | `create_target` 只有尺寸 / 格式（无视口 rect、无 pixel ratio）；2D 离屏固定清透明（`wgpu_2d.rs:70-75`），背景归合成器；相机变换只能预烘焙进载荷 ⇒ 探针约定"整目标 = 视口、1pt = 1px、坐标已是屏幕点" |
| R-05 | `Handle` 无法同时表达"目标 + 批次" | 渲染 IR | **仍成立** | `draw_2d(Handle, PodBytes)` 只有一个 Handle；探针把批次句柄编码进 `ProbeFrame { batch_slot, batch_generation }` —— 即 `PodBytes` 缺 `Handle` 的表示规则（只能自定义编码） |
| R-06 | `Handle ↔ GpuHandle` 映射 | 渲染 IR / 资源 | **可行（有前提）** | `ze_pod_core::HandleTable<Slot>` 承载映射（世代防 ABA）；`GpuHandle` 是 `Box<dyn Any>`（`ze-resource/src/render_backend.rs:78-84`，`!Send + !Sync`）⇒ 只能留在 Pod 内部；`Handle.slot` 可与 `GpuResourceId.index()` 对齐，`generation` 对齐 |
| R-07 | `Send + Sync` 是否硬阻塞 | Pod / 后端 | **平台相关：native 不阻塞，wasm 阻塞** | `wgpu_2d.rs:352-362` 有 `#[cfg(not(target_arch = "wasm32"))] unsafe impl Send/Sync for WgpuBackend2D`（S1 徽章，`GpuHandle` 本身是 `Box<dyn Any>` ⇒ `!Send + !Sync`）。native：`ProbeState: Send + Sync` ⇒ `RefCell` 换 `Mutex` 即可登记 `SyncPod`（`probe_state_is_send_and_sync_on_native`）；wasm：徽章不存在 ⇒ `examples/send_probe.rs` 在 `--target wasm32` 下必须失败 |
| R-08 | `DrawCommand` / `Style2D` 无 serde | 渲染 IR | **仍成立（独立决策）** | 探针用自定义可序列化载荷（`ProbeBatch` / `ProbeFrame`）跑通边界 ⇒ serde **不是**"边界可行"的前提；但生产接线需要稳定编码（补 serde 或定义显式二进制格式）。`Shape2D` 已有 serde，补齐的收益集中在 `Style2D` |

### B3-2d 结论（逐条）

1. **`draw_2d` 四条够不够**：**单帧够用，生命周期不够**。四条能跑通"建目标 → 上传 → 画一帧"；
   但缺 `release` / 原地更新（R-02）、缺视口与相机语义（R-04）、缺清屏（R-04）。
   ⇒ **不改签名**（触发条件：出现第一个真消费方且需要 >1 帧或 buffer 更新）。
2. **`upload` 返回 Handle vs 写入已有 Handle**：**返回 Handle 是对的** —— 它与本仓
   "句柄 = 不可变资源标识 + 世代"的模型一致；"写入已有"会要求 Handle 参数 + 可变语义，
   与 `HandleTable::get_mut` 的借用模型冲突。**但必须配套 release**（契约缺，R-02）。
3. **`Handle ↔ GpuHandle` 映射**：**可行**（R-06）。规则：`Handle.slot` ↔ `GpuResourceId.index`、
   `Handle.generation` ↔ `GpuResourceId.generation`；`GpuHandle` 永不出 Pod。
4. **"Send+Sync 是硬阻塞"**：**分平台**（R-07）。`Pod` 自身不要求 `Send + Sync`
   （`PodNode::new` 合法）；native 上 2D 后端带 S1 徽章（`unsafe impl Send/Sync`）
   ⇒ `ProbeState: Send + Sync`，把 `RefCell` 换 `Mutex` 就能登记 `SyncPod`；
   **wasm 上徽章不存在**（wgpu 资源含 `Rc<RefCell<..>>`）⇒ 跨平台统一的 SyncPod 不成立。
5. **`DrawCommand` / `Style2D` 要不要补 serde**：**要，但不是本批**（R-08）。探针证明边界
   可以靠自定义编码跑通；生产接线前需要稳定编码，届时补齐 `Style2D`（`Shape2D` 已有）。

**I 段小计（B3-2）**：已闭合 1（R-01）· 可行 1（R-06）· 成立 1（R-08）· 平台相关 1（R-07）· 仍成立 4（R-02 / R-03 / R-04 / R-05）。
签名**未改** ⇒ `PINNED_FINGERPRINTS` 不变。

---

## J. B3 批次记账（Surface 双路径，2026-10-21）

> 计划《B3执行计划_Surface双路径》及后续 Step 4 / Step 5-6 计划。本段记录**实际落地**部分：
> Step 0-4c 已全部落地；Step 5-6 落地为「两个脚本作品 + 对拍注册」。fourier / storyboard
> 迁移纹理路径（S5-01）已**否决**（决策留痕，不是待执行缺口）。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| S0-01 | `zela-render` 断 ECS 依赖后缺 CI 门禁 | CI | **已关闭（B3 Step 0）** | 新增 `JQ_RENDER_LAYER`；临时加违规依赖验证能捕获后撤销 |
| S0-02 | 四条基线 `@ f2c8778` 是**回填值**，易被误读为"产生时提交" | 对拍 | **已关闭（B3 Step 0）** | `FRAME_HASH_BASELINES.md` 增加回填值警告（跨批次不得采信） |
| S0-03 | `B3-R` / `B4` / `B3-2d` 三名并存 | 文档 | **已关闭（B3 Step 0）** | GAPS 段标题 / 结论标题 / 代码注释统一为 B3-2 / B3-2d；`grep B3-R` = 0 |
| S0-04 | 可选的 hecs 隔离判据（"hecs 只在 `ze-scene-hecs` / `zela-scene-ecs`"） | CI | **不成立（未采纳）** | 实测 `zela-schedule` / `zela-egui` / `zela-app` / `zela-playback` 的生产边仍直接依赖 hecs；判据前提与代码不符 |
| S1-01 | 派生桥手写 ellipse / polygon / line_ribbon（第二个三角化来源） | 渲染管线 | **已关闭（B3 Step 1）** | 统一走 `tessellate_with`（lyon）；7 种图元（含 Path / Group）；三个手写函数已删 |
| S1-02 | 2D 的边法线（attrib0）/ advance（attrib2）在 3D 布局里无对应槽 | 渲染管线 | **仍成立（显式限制）** | 转换时丢弃边法线，着色法线恒 +Z；Direct 对 `Reveal` 显式报错 |
| S1-03 | `Polyline` / `Custom` 仍不可派生 | 渲染管线 | **仍成立（显式报错）** | `ShapeMeshError::Unsupported`；Direct 路径未声明支持 ⇒ 直接报错 |
| S1-04 | Direct 单材质：`fill` + `stroke` 同时存在时只派生填充 | 渲染管线 | **已关闭（B3 Step 4a-A）** | `derive_mesh → Vec<DerivedMesh>`：fill / stroke 各一份，各自材质颜色；不再丢描边 |
| S2-01 | Direct 路径的触发条件不能靠内容类型推断 | 渲染管线 | **已关闭（B3 Step 2）** | 只由 `SurfaceRenderer::Renderer3D` 决定（`surface_route.rs`）；无 Surface ⇒ 默认 2D |
| S2-02 | Direct 失败无处表达（`System::run` 无返回值） | 渲染管线 | **已关闭（B3 Step 2）** | `DrawQueue::surface_errors()` 收集显式错误；不影响其它对象录制 |
| S2-03 | Direct / Texture + `SurfaceMapping::Screen` 的 3D 语义未定义 | 渲染管线 | **仍成立（显式拒绝）** | Direct：`ScreenMappingUnsupported`；Texture：`ScreenTextureUnsupported`（`content_texture_specs` 只接受 `World`）。触发条件：先定义 HUD 近平面 / 代理平面世界位置语义（见 B3 Step 4d，明确"不做"） |
| S3-01 | 无 Surface 组件 / 归属判定 | 场景核心 | **已关闭（B3 Step 3）** | `Surface`（无 position）+ `SurfaceRenderer`；沿 `Parent` 链解析、visited 环检测；缺 renderer 显式错误 |
| S3-02 | 默认 renderer 策略 | 场景核心 | **未拍（按计划延后）** | 计划要求用 Step 1 实测覆盖度反推；当前 7/9 图元可 Direct，但 Step 4/5 未做，暂不设默认 |
| S4-01 | Texture 路径多实例（2D 离屏 → 纹理 → 代理平面） | 渲染后端 | **已关闭（B3 Step 4c-1..4c-5）** | 容器 / id 链 / `fit_margin` / demo3d 迁移 / 双 Surface 互不串扰验证全部落地 |
| S4-02 | `Renderer2D` 没有可表达的固定纹理契约 | 场景核心 | **已关闭（B3 Step 4b）** | `SurfaceTexture { texel_size, filter, mipmap }` + `camera_for`；缺声明 / 尺寸非法 / 宽高比不一致 ⇒ 显式错误 |
| S4-03 | `surface_errors` 只收集、无生产消费方（验收 O-2） | 宿主 / 帧管理 | **已关闭（B3 Step 4c-1）** | `zela_render::frame::end_frame` 非空 ⇒ `panic!`；egui / playback / web 三个宿主共用该帧末路径 |
| S4-04 | `ContentTextureSpec` / `ContentTextureQueue` 单份；`LayerId` 用图层位置分配 | 渲染 IR / 帧管理 | **已关闭（B3 Step 4c-2）** | 容器按 `LayerId` 索引；`ObjectId → LayerId → TextureHandle` 单一映射（`content_layer_id` / `content_texture_handle`）；视口图层用固定 id；`grep "LayerId(view.layers.len()" = 0` |
| S4-05 | Surface 纹理相机只能 1:1，无法表达 demo3d 的 0.8 留边 | 场景核心 | **已关闭（B3 Step 4c-3）** | `SurfaceTexture.fit_margin`；`Some(m)` 直接调用 `Camera2D::fit_bounds`（逐位相同）；非法 margin ⇒ 显式错误 |
| S4-06 | demo3d 走硬编码 `CONTENT_TEXTURE_ID` / 手算内容相机（与 Surface 路径**两个来源**） | 作品 / 装配 | **已关闭（B3 Step 4c-4）** | demo3d 改为 `Surface(Renderer2D) + SurfaceTexture(fit_margin=0.8)`，内容实体挂 `Parent(surface)`；材质纹理由 `content_texture_handle` 派生；**四基线逐位不变（零差异）** |
| S4-07 | 多 Surface 只是"签名存在"，无验证 | 测试 | **已关闭（B3 Step 4c-5）** | `two_surfaces_record_into_independent_layers`：两个 Surface 各自一层、纹理句柄不同、内容跨度互不串扰 |
| S4-08 | `SurfaceTexture.filter` / `mipmap` 已进契约，但后端采样仍用全局默认 | 渲染后端 | **仍成立（未接线）** | 为保持 demo3d 零差异，4c 未改后端 sampler；mipmap 生成本就未实现（E-08）。触发条件：出现对过滤敏感的 Surface 内容 / 引入 mipmap 生成 |
| S4-09 | 内容层录制只支持 `(Path, StrokeSolid)`，其它组合走 Surface 会无提示丢内容 | 渲染管线 | **已关闭（B3 Step 5-1）** | 新增 `record_scene_for_surface`：复用 9 组全组合 + Parent 归属过滤；`ContentTextureItem` 标记**不再必需**（组件保留登记以兼容旧数据）；demo3d 零差异 |
| S4-10 | Surface 归属的 2D 内容被**屏幕视口重复绘制**（同一对象出现两次且带偏移） | 渲染管线 | **已关闭（B3 Step 5 修正）** | `record_group` 引入 `SurfaceScope::{Viewport, Content}`：视口层跳过归属任一 `Surface` 的对象（内容只进该 Surface 的纹理）；test `renderer2d_content_is_not_drawn_in_viewport`；`surface_matrix` 重录 `baafe1aa… → 7bed8a31…`，既有四基线不变 |
| S4-11 | `composite = true` 把整张纹理**铺满视口**（无 letterbox / 无目标矩形）⇒ 纹素宽高比 ≠ 输出宽高比时画面被拉伸（圆变椭圆） | 渲染后端 | **已关闭（B4 验收）** | `CompositeOp.fit: Option<(u32,u32)>`：`Some` ⇒ 按源纹理宽高比 **contain + 居中**（`fit_rect` + `rect_to_ndc`，顶点阶段按 per-layer uniform 的 `rect` 重映射四边形）；`None` ⇒ 铺满（视口层，离屏尺寸 == 视口尺寸 ⇒ 恒等）。验收：`gpu_fit_letterboxes_instead_of_stretching`（2:1 源进 4×4 ⇒ 上下各 1 行黑边）+ 3 条纯函数用例；既有 5 条 native 基线**逐位不变**（作品 A/B 本就是 4:3 = 4:3，rect 换算为精确的 `(-1,1,-1,1)`） |
| S4-12 | `Planar` 兜底 UV 的 V 轴方向反了：贴图在无 UV 几何上**上下倒转** | 渲染管线 | **已关闭（B3 Step 5 修正）** | `fallback_uvs` Planar 改为 `v = 1 - y`（纹理行首在模型 +y，与 `parametric_surface` 手写的 `1.0 - v` 同口径）；test `planar_fallback_uv_puts_image_top_at_positive_y` |
| S4-13 | `solid_cube` 只有 8 个共享顶点、**无 UV / 无法线**：Planar 兜底在侧面（x=常量）退化成 0 面积 ⇒ PBR 法线 TBN 除以 `det≈0` 得 NaN（右脸随机噪点）；面法线恒 +Z ⇒ 三个面同亮度 | 渲染管线 | **已关闭（B3 Step 5 修正）** | 改 24 顶点（每面 4 个）：显式 UV（图像行首在 `+y`）+ 面法线；`cube.wgsl` 的 TBN 在 `abs(det) ≤ 1e-6` 时跳过法线贴图；tests `test_solid_cube_faces_have_non_degenerate_uv`；demo3d 重录 `dc8b1388… → ddd69b3e…`（噪点消失 + 面法线正确着色，逐条可解释） |
| S5-01 | 既有 2D 作品迁移纹理路径 | 作品 | **已否决（不改写）** | 既有 2D 作品**不迁**纹理路径：无 3D 消费者，强行 `composite` 会多一次全屏重采样且无收益。纹理路径的验证改由新增作品承担（`surface_matrix` / `atlas`）。本条不是"待执行的缺口"，是决策留痕 |
| S6-01 | 脚本无法声明 Surface / 无法把内容纹理挂到材质 | 脚本 API | **已关闭（B3 Step 5-0）** | `scene.surface(zela.Surface)` + `Material3D.texture = Texture.Surface{..}`；`SurfaceTextureRef` 在 commit 解析；`zela.scene@7`（指纹 `15_573_081_187_359_822_637`） |
| S6-02 | `zela.Material3D` 加 `texture` 字段而指纹不变（字段结构不进指纹） | 脚本 API | **仍成立（已知）** | 加**方法**会变指纹、加**字段**不会 —— 看守力度不一致；本轮靠版本递增 + 手工记账 |
| S6-03 | Rust 版 `authored/demo3d.rs` 不进任何对拍 / 测试（验证盲区） | 作品 / 测试 | **仍成立（记账）** | `build_script_runtime` 用 `with_demo3d = false`，`--script` / `--bundle` 都不构建它；`tests/` 只覆盖能力 demo 与分镜 ⇒ 该模块的改动无人验（4c-4 的"零差异"对它是平凡真）。触发条件：给它加对拍入口，或显式降级为"仅 GUI 示例" |
| S6-04 | 脚本可表达的图元是 9 组录制表的**真子集** | 脚本 API | **仍成立（已知）** | `ze-scene-api` 只有 `Circle` / `Polyline` / `Arrow` / `PathShape`（无 Rect / Line / Ellipse / Polygon）；作品 A 因此只能覆盖 Circle / Arrow / Path × {Fill, Stroke}，Rect / Line 组合**无法用脚本表达** |

**J 段小计（B3，已执行部分）**：已关闭 20（S0-01/02/03、S1-01、S1-04、S2-01/02、S3-01、S4-01/02/03/04/05/06/07/09/10/11/12/13、S6-01）· 仍成立/显式拒绝 7（S1-02/03、S2-03、S4-08、S6-02/03/04）· 不成立未采纳 1（S0-04）· 未拍 1（S3-02）· 已否决 1（S5-01）。

---

## K. B4 批次记账（矢量后端移植与渲染参数契约，2026-10-22）

> 计划《B4执行计划_矢量后端移植与渲染参数契约》。上游 `manimgx @ dcfe01a`（本地快照在仓库内 `manimgx/`，已 gitignore）。
> B4 Step 1 / 2 / 3 / 4 / 5 / 6 与 B6 Step 1 / 2.1 / 3 / 4 已落地；矢量后端**能画、能对拍、能出作品基线**，
> 但 `shapes` 契约与后端可注册仍未做（V2-12 / V2-18），渐变 / 纹理 / dash 仍缺概念（V2-13）。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| V1-01 | 渲染参数（分辨率 / 采样 / 过滤 / 后处理）不在任何契约里 | 渲染 IR | **已关闭（B4 Step 1）** | `ze_render_ir::RenderParams`（size / supersample / msaa / filter / tone / bloom / ao / env）；`Default` = 当前行为；`validate()` / `is_supported_by(&Capabilities)`；`target_desc()` 是 `RenderTargetDesc` 的唯一上游投影 |
| V1-02 | 后端能力没有"后处理 / IBL"维度，无法表达"不支持" | 渲染 IR | **已关闭（B4 Step 1）** | `PostCaps`（tone / bloom / ao / env）进 `Capabilities`；wgpu 后端声明 `none()` ⇒ 一旦 Meta 要求后处理就**显式失败** |
| V1-03 | ZSB 不承载渲染参数，回放端各写各的默认值 | 装配产物 / 回放 | **已关闭（B4 Step 2）** | `Meta.render_params`（`#[serde(default)]`，不进内容 hash）；`zela-playback::bundle_render_params` + `check_render_params_supported`；`zela-render` 开渲前校验 |
| V1-04 | 许可证基线未立：上游多数文件**自身没有 SPDX 头** | 依赖 / 合规 | **已关闭（B4 Step 3）** | 新 crate `zela-render-vector`：`LICENSES/{MIT,Apache-2.0}.txt` 逐字复制；`UPSTREAM.md` 记录 SHA + 逐文件许可（light/occlusion/tone/bloom + dfg.bin 为 Apache-2.0）；只开 `render` feature |
| V1-05 | `RenderCapabilitySet::ALL` 把"只有一个后端"写进类型；无后端注册/选择点 | 渲染后端 | **已关闭（B4 Step 4）** | 删除 `ALL`，新增 `union()`；`BackendKind::{Wgpu, Vector}` 各自自述；`BackendRegistry` 注册 / 选择 / 协商（未实现或未注册 ⇒ 显式错误）；测试锁住"wgpu-only 协商 shapes 必须失败" |
| V2-01 | 矢量后端本身尚未移植（`vector.*` / `render.rs` 公共类型 / 着色栈 / 光栅后端，约 9.9k 行） | 渲染后端 | **已关闭（B4 Step 5）** | `zela-render-vector` 收录上游源码（`vector.*` + `vector_buffers/compute` + `render.rs` 公共类型 + `blend*` / `depth` / `paint` / `mesh` / `shadow` / `bloom` / `occlusion` / `environment` + 着色栈 wgsl + `dfg.bin`）；逐个补 SPDX 头；宿主适配层 `api::FilledPath` / `render_2d`。验证：`cargo test -p zela-render-vector` 26 passed（含上游自带用例 + `filled_square_draws`） |
| V2-02 | 双后端共存验证 + 矢量作品新基线 | 测试 / 对拍 | **已关闭（B4 Step 6，新基线拆到 V2-07）** | `zela-render-backends/tests/vector_vs_wgpu.rs`：同一进程内 wgpu 与矢量各渲一帧，判据 = 非空 + 覆盖率相对差 ≤ 10% + 逐像素 RGB MAE ≤ 24/255。实测：轴对齐矩形 4096/4096 像素、MAE 0.00；非 45° 斜边三角形 MAE 12.18/255、覆盖率相对差 0.005。**⚠️ 本条 12.18 / 24 是夹具 bug 修正前（B4 验收 P1-1）的旧读数，已被 V2-09 取代（全图 0.09 / 阈值 8，内部区 ≤ 2）；此处仅作血缘留痕** |
| V2-03 | 上游是 `m1/m2` 双变换 + **GPU 插值**；本仓是 CPU 每帧烘单矩阵 | 渲染后端 | **记账（移植时已知非等价）** | 移植时令 `m1 = m2` 可跑通，但**丢掉"一次上传、GPU 插值"的性能模型**；确定性分镜不受影响（逐帧渲染） |
| V2-04 | 三角化资产（`tessellate.rs` / `VertexBufferData` / `shape_to_mesh`）在矢量路线下不是唯一来源 | 文档 / 分层 | **记账（性质变更）** | 不删除、不破坏分层；在 ARCHITECTURE 写明"wgpu 后端专用路径"，避免后人误以为全局唯一 |
| V2-05 | 旧 ZSB 没有 `render_params` 字段 | 装配产物 | **已按兼容策略关闭（V1-03）** | `#[serde(default)]` ⇒ 缺字段按 `RenderParams::default()`（= 当前行为）；不升容器版本（该字段不进 hash） |
| V2-06 | 矢量后端**未接进作品管线**：`DrawCommand` / `Path2D` → `Record` 的全量映射（描边 / 渐变 / 图片 / 圆角）未做；`zela.render.shapes` 未实现；`BackendKind::Vector` 仍 `is_available() == false` | 渲染后端 / 装配 | **部分关闭（B6：填充 + 描边已映射，且已进作品基线）** | `zela-render-vector::scene` 把 `Shape2D + Transform2D + Style2D` 映射成填充记录（圆/椭圆 4 段三次曲线、矩形/多边形、路径含 Quad/Cubic/Arc）；B6 Step 2.1 补上**描边记录**（`Record::stroked`）。真实 `DrawQueue` 帧对拍：填充 4568 vs 4580 像素、MAE 0.05/255；描边三角形 934 vs 954、MAE 0.20/255。B6 V2-07 起 `--backend vector` 用同一份快照出作品帧并进 `[vector]` 基线。此外 **仍缺**：纹理图片（V2-13）与 `shapes` 签名（V2-12）；渐变 / dash **已由 PD 暴露后端接口、PE 接通场景入口**（见 V2-13） |
| V2-07 | 矢量作品新基线（计划 §6.3）依赖 V2-06 | 验收 / 基线 | **已关闭（B6 Step 3）** | `zela-render --backend vector`：同一份 `DrawQueue` 快照 → `render_scene_2d`（纯 2D 帧；含 3D / 内容图层的帧**显式报错**，不做无提示跳过）。作品 `vector_paths.lua`（Circle / Arrow × Fill / Stroke、`Reveal` 折线描边、`content.text` → 路径填充，静态 1 帧）实测 14 对象、1 帧、`d19962e7…`。`baselines.txt` 新增 `[vector]` 段；`xtask` 的 `hashes` / `--record` / `--rebind` / `+dirty` 门禁经 `is_managed_section` 覆盖该段，并有 3 条解析器用例 |
| V2-08 | 基线归因：`--record` 在**脏工作树**上把值标成 `@ <HEAD>`，但该提交并不包含这份状态（6 条值全标 `@ da9ea93`，实际由 `642b9d4` 携带）；slotA → slotB 的差异也无逐条说明 | 验收 / 基线 | **已关闭（B4 验收 P1-4，口径 D）** | `--record` 检测脏树 ⇒ 写 `@ <sha>+dirty`；新 `cargo verify --rebind`（要求干净树）把 `<sha>+dirty` **整段**换成携带录制的提交并自校验；`smoke` / `hashes` 在见到未归因 `+dirty` 时**直接失败**；`# delta:` 逐条留痕（slotA→slotB 前缀 + 原因指针）；存量 `baselines.txt` 已按真实血缘重写（demo3d `dc8b1388→ddd69b3e`、atlas `efe5bf39→93632049`，@ `642b9d4`）。文档：`FRAME_HASH_BASELINES.md` 口径 D；测试：`xtask/tests/parser_tests.rs` 5 条 |
| V2-09 | 双后端对拍的**口径**问题（B4 验收 P1-1/2/3）：① MAE 12.18 无法区分"AA 方式不同"与"色彩管线不同"；② 矢量自测在起不来 GPU 时无提示 `return`（仍然 passed）；③ 对拍注释声称"验证注册机制"，实际绕过 `BackendRegistry` | 测试 / 对拍 | **已关闭（B4 验收）** | ① MAE 拆「内部区 / 边缘带」并输出计数，内部区门禁 ≤ 2/255；**诊断出真 bug**：斜边夹具把屏幕坐标（y 向下）当成世界坐标（y 向上）⇒ 比的是镜像形状。修正后实测全图 MAE 0.09（内部区 0.00、边缘带 3.44），全局阈值由 24 收紧到 8。② `api.rs` 起不来即 `panic!`（与对拍同口径）。③ 注释改为"本对拍不经过 `BackendRegistry`，注册机制由 `registry.rs` 单测覆盖" |

| V2-10 | 描边 `cap` / `join` 曾是 **lyon 的隐默默认**（不在任何契约里）：换后端 / 换 lyon 版本都会无提示改画面 | 渲染 IR | **已关闭（B5 Step 1）** | `StrokeCap` / `StrokeJoin` / `StrokeShape` 进 `ze-render-ir::style`；`StrokeSolid` 带 `#[serde(default)]`（默认 butt + miter ⇒ 旧 ZSB 逐位不变）；`Style2D.stroke_shape` 是**后端唯一消费点**；`tessellate.rs` 的 `stroke_options()` 显式驱动 lyon（`StrokeOptions::default()` 不再裸用）。实测锁定 `lyon_tessellation 1.0.22` 默认 `Butt / Miter / 4.0`，测试断言"默认选项逐字段相等 + 改 cap/join 真的改变顶点数" |
| V2-11 | miter limit 两边不一致：lyon 默认 **4.0**，上游矢量**硬编码 10**（`vector.wgsl:497` / `render.rs:330`） | 渲染后端 | **已记录·参数性（PB Step 3 评估；不改）** | 实测（`miter_limit_difference_on_a_needle_angle`，128×128，本机 RTX 4060）：1° 尖角覆盖率 wgpu 345 / 矢量 350（+1.4%），四象限"最远亮像素"完全相同；20°（miter 比 5.76，正好落在 4 与 10 之间）561 / 576（+2.7%），最远点同为 9.90px。⇒ **差异只出现在极端尖角，量级为个位数像素、无结构性差异** ⇒ 按 PA 的分层判据（V-08）记录为参数性差异，不回改 wgsl 语义。若将来矢量侧解析 AA 的 join 改动，本条重测 |
| V2-12 | `zela.render.shapes` 签名只有 `capabilities` / `submit_shapes`：**没有** `create_target` / `upload`；且 `PathList` 表达不了 `Circle` / `Rect` / `Group` | 能力契约 / 装配 | **未执行（B6 复核后仍不改）** | 改签名必须同步 `PINNED_FINGERPRINTS`（`shapes = 1_097_982_529_554_556_752`）。**为什么不改**：`submit_shapes` 在仓库里**没有任何实现也没有任何调用点**（`IMPLEMENTED_NAMESPACES` 不含它、wgpu 协商 shapes 本就显式失败），脚本层也没有 `create_target` / `upload` 的绑定表；此时把入参改成尚不存在的 `ShapeList` 只会得到一个**没有实现可校验的 ABI 字符串**（违反"不得为凑可注册而填未验证的编码"）。触发：真正要接 `shapes` 时，先定 target 生命周期与 `Shape2D` 列表的宿主载体，再一次性改签名 + 指纹 |
| V2-13 | 渐变笔刷行 / 纹理图片 / dash 未映射 | 渲染后端 | **部分关闭（PE Step 1–2：渐变 / dash / 双 paint / 背景描边 / 曲线窗口接通；纹理仍缺）** | **事实更正**："缺概念"只对纹理成立。渐变与 dash 在 PD 已由后端公开面暴露（`PathOptions`，6 条 GPU 用例），PE 把类型下沉到 `ze-render-ir::paint` 并接上**场景层组件**（`Gradient` / `Dash` / `SecondPaint` / `BackgroundStroke` / `CurveWindow`），由 `record_scene` 汇成 `Style2D.paint`、`zela-render-vector::scene` 写进记录槽位。**仍缺**：纹理图片（要设计"纹理源" + `TEXTURED` 标志，与 PB-04 路线 B 同一件事，属 PD-03 顺延 PF） |
| V2-14 | 矢量后端的**描边记录**未接 | 渲染后端 | **已关闭（B6 Step 2.1）** | ⚠️ B5 的阻塞理由**不成立**：`Curves.miter` 由 `render.rs:476 Store::add_path` 自动计算，与 `vector.rs` 无关；真正缺的只是 `Record` 的描边构造。现在 `Record::stroked`（`stroke` + `params.z` = **宽度** + `flags >> STYLE` = `cap \| joint << 2`）已落地，`scene::scene_records` 按"先填充后描边"产出。**事实更正**：`params.w` 是背景描边的宽度，不是 miter；miter limit 硬编码在 wgsl（见 V2-11）。宽度语义由实测锁定（传半宽会画出一半宽）。对拍：描边三角形 934 vs 954 像素、MAE 0.20/255、内部区 0.29 |
| V2-15 | 着色栈的**结构事实**：`tone` 由 bloom 的合成 pass 应用，而那条 pass 只在**存在 3D layered 计划**时运行（`render.rs:2419`）；`occlusion_of()` 还要求存在环境光（类型 4）或全向光（类型 0），否则 AO 直接不生效、没有任何提示 | 渲染后端 | **已关闭（B6 Step 1 验收用例；B10 Step 4 升级为作品级证据）** | 结论不变（B6 §2.5 说 "bloom/tone 独立于光照" **不成立**）。B6 的 3 条 3D 用例（`AgX ≠ clip` / bloom 外溢 / AO 需环境光）已被 **B10 的作品级证据**取代并加强：`shaded_meshes` 走 `--backend vector`，同一份 ZSB 只切换 `vector_params`（`tests/vector3d_shading.rs`）⇒ tone 差 >2000 像素、bloom 开更亮、AO 开更暗；帧序列哈希进 `[vector]` 基线（`58ab1f0e…`）。另记：受光/材质编码来自上游 `feed.py:1134`（`shade_in_3d` → `LIT`）与 `:1150`（`material[3] = 1`），`plan.lit` 按 `material[3] > 0` 判定 |
| V2-16 | `ToneCurve` 语义错配（B4 建契约时按常见命名猜的）：`Reinhard` / `Aces`，而 `tone.wgsl` 只有 `0 = clip` / `1 = AgX` | 渲染 IR | **已关闭（B6 Step 1.1）** | 改为 `ToneCurve { Clip, #[default] AgX }`；`default_for(caps)` 按后端能力取默认（wgpu 全关 / 矢量全开），显式请求不支持项仍报错 —— 与"静默降级"的区别写进 `RenderParams::default_for` 文档 |
| V2-17 | 校验口径：静态单帧测不出**闪烁**（时域不稳定） | 测试 / 对拍 | **已关闭（B6 Step 4）** | `per_frame_tolerance_holds_across_an_animation`：8 帧旋转 + 平移矩形，逐帧走**同一套**门禁（非空 + 覆盖率 ≤10% + 全图 MAE ≤ 8 + 内部区 ≤ 2），再对帧间 MAE 序列取"最大 ≤ 3×均值"。实测：逐帧 MAE 0.03–0.08，帧间均值 1.31 / 最大 1.54 ⇒ 无异常跳变。阈值来自实测数据（不是先设一个宽到永远绿的数） |
| V2-18 | `BackendKind::Vector` 仍 `is_available() == false`（B6 Step 2.3 未转 true） | 渲染后端 / 装配 | **未执行（B6 复核：无可自述 namespace）** | 注册的意义是"给系统能力加 namespace"。矢量后端**没有**可自述的：`zela.render2d` / `render3d` 的 `upload(PodBytes)` 吃三角化顶点（矢量后端按路径解析渲染，不吃），`zela.render.shapes` 未定稿（V2-12）。⇒ 注册它不会带来任何能力，翻开关就是凑数（违反 §四.4）。`--backend vector` 是 `zela-render` 的**进程级开关**（同一份 `DrawQueue` 快照），不经注册表；`post_caps()` 已能自述（`Wgpu = none()` / `Vector = all()`） |
| V2-19 | "开着色栈（tone + bloom + AO + env）的作品"无法用脚本表达 | 作品 / 脚本 API | **部分关闭（B10 Step 4）** | **B6 的结论已被 B10 推翻一半**：`--backend vector` 现在支持含 3D 相机的帧（`frame_loop` 的 3D 分支），`shaded_meshes` 就是"开着色栈的作品"，帧哈希进 `[vector]` 基线。**仍缺**：脚本层仍不能自己表达曝光 / tone 曲线 / bloom 强度（现在吃的是"矢量后端默认全开"），`env` 也没有脚本入口（X1-04） |
| S4-14 | 孤儿 Surface：`Renderer2D + composite=false` 且无 `Material3D.texture` 引用 ⇒ 内容既不在视口画（S4-10 起跳过）、也不参与合成、也没有代理平面 ⇒ **完全不可见且不报错** | 渲染管线 | **已关闭（B5 A-2）** | `content_texture_specs` 在同一趟里 `for_each::<Material3D>` 查引用；无人引用 ⇒ `SurfaceError::UnconsumedSurface`（报错文案给出修法：设 `composite=true` 或让 3D 材质引用）。测试 `unconsumed_content_surface_errors_explicitly`（无人引用报错 / 有引用通过 / composite=true 通过） |

**K 段小计（B4 + B5 + B6）**：已关闭 16（V1-01..05、V2-01/02、V2-07、V2-08/09、V2-10、V2-14、V2-15、V2-16/17、S4-14）· 部分关闭 3（V2-06、V2-13、V2-19）· 记账 2（V2-03/04）· 未执行 2（V2-12、V2-18）· 已记录 1（V2-11）。

---

## L. B8 批次记账（性能优化与可观测性，2026-10-23）

> 计划《B8执行计划_性能优化与可观测性》+ 更正件《B8补充更正_GPU校验Pass与tile指纹》
> + 验收报告《B8_Step1执行验收报告_性能实测与方向修正》。
> 本批是**纯性能改动**：像素逐位不变是唯一正确性证明（V3-01 原口径）。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| V3-01 | 手写 `sha256` 被用在不匹配的负载上：它自己的文档写"只服务 payload hash"（几十 KB、每次装配一次），实际**每帧**哈希 1.2 MB 帧缓冲；朴素 compress + 每次调用 1.2 MB `Vec` 拷贝 ⇒ 实测 22 MB/s、占 45% 帧时间 | 性能 / 装配 | **已关闭（B8 Step 3′0）** | 新增 `sha256::sha256_fast`：非 wasm 走 `sha2`（**target-specific 依赖**，wasm 侧仍用手写实现、格式 crate 保持极轻）；`frame_hash` 只在大输入上走快路径。对拍 `fast_path_matches_the_hand_written_one`（空 / 63 / 64 / 65 B / 1.2 MB + FIPS 向量）。实测（dev）54.3 → 26.5 ms/帧；**release 0.64 ms/帧**；六条基线值不变 |
| V3-02 | PNG 用 crate 默认压缩档（zlib level 6 + 逐行 `Adaptive` 滤波），1.2 MB ≈ 57 ms/帧 | 性能 / 导出 | **已关闭（B8 Step 3′00）** | `Compression::Fast` + `FilterType::Sub`。**`NoFilter` 实测更差**（46 ms 且文件大 4.3 倍：419 KB vs 97 KB）⇒ 选 `Sub`。只改 `.png` 文件字节；序列哈希吃像素缓冲（`frame_hash` 模块说明早已写明这点），六条基线不变。dev 57.3 → 41.5 ms/帧；release 1.8 ms/帧 |
| V3-03 | §0.3「实际渲染 110 ms/帧」的推算被实测推翻 | 计划 / 决策 | **已关闭（B8 Step 1 实测）** | fourier 渲染核心 **8.9 ms/帧**（dev）、demo3d 4.1 ms；93% 是哈希 + 编码。⇒ 由"每帧贵"推出的「新作品 ≤60 帧」「纳管成本 3.3×」「分级阈值」全部失据（影响 B7 的成本模型） |
| V3-04 | 双缓冲流水线（原 Step 4） | 性能 / 流水线 | **已关闭（取消）** | `t_read`（含 GPU 等待）实测 dev 1.3–4.4%、release 17–32%，但整帧已降到 3.7–5.2 ms（release）⇒ 绝对值 ≤1.7 ms。按更正件 §3.2 的 30% 线直接取消；理由记账：夹具路径的毫秒级回读不值得一套双缓冲复杂度 |
| V3-05 | GPU 校验 pass / tile 指纹（原 Step 3′B） | 性能 / 校验 | **记账（推迟，不是否决）** | GPU 校验的真实收益是省 CPU 哈希（54 ms dev），而该代价已被 Step 3′0 的库级修复覆盖（release 0.64 ms）⇒ "为加速而换基线格式 + 全量重录"的理由不成立。触发条件：release 下哈希重新成为主瓶颈，或需要"差异定位到 tile"的诊断能力（那种场景 tile 指纹确实比 32B 全局摘要强） |
| V3-06 | sink 抽象（原 Step 2）：主流程被迫为测试夹具付哈希代价 | 装配 / 导出 | **已关闭（B8 Step 2）** | `FrameSink`（`PngSink` / `FfmpegSink` / `HashSink`）；`--hash` 显式挂、`--hash-out` 隐含，**默认不挂**；xtask 对拍显式传 `--hash`。V3-12：同帧 PNG **字节**在挂/不挂哈希时逐位相同（fourier `frame_00100` 与 `vector_paths` 各验一次） |
| V3-07 | dev 与 release 的 20–40× 差距没有任何记账，容易被当成"优化没生效" | 性能 / 构建 | **已关闭（B9 Step A + Step 1）** | 同机同码：hash 26.5 → 0.64 ms、PNG 41.5 → 1.8 ms、整帧 77.0 → 5.2 ms。B9 用实验证明两者**像素逐位一致**（release 下 fourier `b6eaa864…`、demo3d `ddd69b3e…`、vector_paths `98d61c1a…` 全部等于基线）⇒ 门禁可以改用 `[profile.verify]`（opt-level 3 + debug=false，仍继承 dev 的检查） |
| V3-08 | §0.3「实际渲染 110 ms/帧」的推算被实测推翻后，没有回答"切 profile 会不会改变像素" | 性能 / 构建 | **已关闭（B9 Step A）** | 三个作品（2D 路径 / 3D PBR / 矢量后端）的 release 序列哈希与基线逐位相同；无需 `fp-contract=off`，也没有分歧帧。 |
| V3-09 | `cargo verify` 全程跑在未优化构建上（全仓无 `[profile]`，xtask 的 `cargo run` 没有 `--release`） | 性能 / 构建 | **已关闭（B9 Step 1）** | 根 `Cargo.toml` 新增 `[profile.verify]`（`inherits = "dev"` + opt-level 3 + debug=false + incremental=false）；xtask 抽 `run_args()` 统一给 `zela-compile` / `zela-render` 加 `--profile verify`，`check` / `test` 仍走 dev。实测：`hashes` 52.48s（profile 切换后、仍是 19 次进程）；编译：增量 9.03s。 |
| V3-10 | 每次渲染进程约 4s 的固定开销没有拆分（"不知道贵在哪"） | 性能 / 构建 | **已关闭（B9 Step 2 探针 + B7 Step 0.2 复测修正）** | verify profile 下实测（fourier / vector_paths）：GPU 初始化 **531 / 502 ms**、脚本运行时构建（含 typst）**718 / 668 ms**、`StandardBackends::new` **5 ms**、首帧 18 ms / 439 ms（后者含矢量管线创建）、其余帧 4.5 ms/帧。**B7 修正**：批量进程内逐作品复测 runtime 构建 = **0–1 ms** ⇒ 那 718 ms 是 **typst 世界 / 字体加载，进程级一次**，已天然跨作品复用；B9 报告结论②「runtime 构建每作品仍要付」**不成立**（这是好消息：不需要 hoist）。 |
| V3-11 | 19 次进程调用 ≈ 46s 固定开销（占 profile 切换后 `hashes` 的 ~88%） | 性能 / 装配 | **已关闭（B9 Step 3）** | 新 bin `zela-verify`：**一次** GPU 初始化，逐作品新建 Runtime 与 `StandardBackends`（`wgpu_3d` 的离屏/深度目标按 `LayerId` 索引，复用会串味 ⇒ 选安全方案），编译也在同进程（§4.5）。帧循环抽到 `zela_app::frame_loop`，与 `zela-render` **共用一份**（不是第二条渲染路径）。实测：批量 **6.46s** vs 独立进程 **53.16s**，6 个作品哈希逐位相同；`--independent` 保留为对照组。 |
| V3-12 | 「挂 / 不挂 HashSink 像素逐位相同」（B8 的 V3-12）只有人工比对，会腐烂 | 测试 / 门禁 | **已关闭（B9 Step 5）** | `xtask::assert_sink_bypass` 并入 `--step hashes`：`vector_paths`（1 帧）分别挂/不挂哈希各渲一次，比 **PNG 字节**（不比序列哈希 —— 不挂时没有哈希值，比哈希是自证）。实测 39504 B 完全相同。 |
| V3-13 | `opt-level = 3` 会让编译变慢、与"验证要快"存在张力 | 性能 / 构建 | **记账（本批末测量，未引入 sccache / lld）** | 首次 verify-profile 全量构建 + 六步全跑 = **530.57s**（含 test/wasm/structure/smoke/hashes，编译时间未单独隔离）；改一个 zela-app 源文件后的增量重建 = **9.03s**。未引入 sccache（外部依赖 + CI 配置）与换链接器（需 `.cargo/config.toml` 分 target），理由见 B9 §5.2 的风险栏。触发：若首次构建时间成为日常瓶颈，再按 §5.3/§5.4 逐项评估。 |

**L 段小计（B8 + B9）**：已关闭 11（V3-01/02/03/04/06/07/08/09/10/11/12）· 记账 2（V3-05、V3-13）。

---

## M. B7 批次记账（内容丰满期，2026-10-23）

> 计划《B7执行计划_内容丰满期》。Step 0 / 1 / 2 已落地（Step 3–5 见后续提交）。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| W1-01 | `MeshGeometry.id` 取自**进程内**全局计数器（`ze-scene-core/components/3d.rs` 的 `next_geometry_id`），且随 serde 序列化进 ZSB payload ⇒ 同一脚本的 payload hash 取决于"本进程此前造过多少几何体"，ZSB 不是源码的纯函数 | 装配产物 / 可复现性 | **已关闭（B10 Step 0）** | 修法：`#[serde(skip, default = "next_geometry_id")]` —— **运行态字段不进产物，解码时重新分配**（计数器推进；置 0 会让解码体撞号，见代码注释的取舍说明）。单测 2 条：同内容不同时刻构造的几何序列化**逐字节相同**（且 JSON 里没有 `id`）；两次解码 / 解码后再新建三者 id 互不相同。实测：15 件 payload 在"同进程批量（zela-verify）"与"一进程一作品（zela-compile）"两种布局下**逐位相同**；manifest 15 条逐件断言通过；15 条帧基线逐位不变（`hashes` 全绿）。副作用（预期）：7 件含网格的作品 payload 值变化（`demo3d` / `fusion` / `pbr_*` / `atlas` / `capability_gaps`），manifest 已同步更新 |
| W1-02 | manifest 的 payload 断言长期空转：13 件里 12 件写 `-`（等于没断言），唯一有值的 fourier 也已不可复现 | 装配产物 / 门禁 | **已关闭（B7 Step 2）** | fourier 的 `f8ed3eae…` 停在 45b0f85（改名 + ZSB 容器 v3）；当前代码下**任一字体配置都复现不出**（系统字体 `dad48069…`、`ZELA_TYPST_FONT_DIR` 确定字体 `a5bdfbe7…`）⇒ 该断言今日必然失败（不断言它从哪次提交开始坏）。Step 2 用 build.ps1 的同一命令逐件重算 13 条并逐条断言通过；含文本的 5 件（fourier / storyboard / surface_matrix / atlas / vector_paths）必须带确定字体才可复现，已写进 manifest 表头 |

| W1-03 | JS 适配器（boa）实现完整但**零作品**：没有一件脚本走 JS，等于没有兑现证据 | 脚本 / Pod 装配 | **已关闭（B7 Step 3）** | `surface_matrix.js` 是 `surface_matrix.lua` 的逐语句翻译（同组件、同字段、同枚举、同顺序）。判据与实测：① 分镜 JSON **逐字节相同** ⇒ `payload hash` 两件都是 `e0951fd4…`（带确定字体）；② 帧序列哈希两件都是 `928cd5f9…`（640×480，1 帧，`script == bundle`）⇒「同一场景、两种语言、同一像素」成立。落地配套：`js` 进 xtask 对拍 features（`typst,js`，纯 Rust，不让对拍依赖本机 Python）、`build.ps1` 的编译命令同开 `js`（不进 wasm —— 网页只回放 ZSB）、`zela-verify` 支持"作品名自带扩展名"（同名 `.lua` / `.js` 并存时探测顺序选不出来） |
| W1-04 | `[browser]` 段定位待改：JS 是唯一能进 wasm 的脚本语言，但真值仍未录制 | 基线 / 浏览器 | **记账（B7 Step 3 探测）** | `surface_matrix.js` 的 ZSB 与其它作品**同一容器格式**（回放端只解释分镜，不关心源语言）⇒ 浏览器侧第一次具备"可比对"的前提；`build.ps1` 已带 `?compare=1` 对拍入口。本机跑不出浏览器产物（需要 `wasm-pack` + Chrome WebGPU）⇒ 本批不录值，`[browser]` 段维持"仅存档、不参与自动比对"。触发：在 Chrome WebGPU 打开 `index.html?compare=1` 跑一次即可补录（`[browser]` 只增不改语义） |

| W1-05 | 6 项"零使用能力"里只有 3 项在脚本层**可达**（`scene.despawn` / `Curve.Const` / `Target.CameraEye`）；`Group` / `Target.Phase` / `Target.MorphT` 是**没有入口**，不是没人用：`Group` 不在 `SceneApi` 签名里、`PhaseAnim` 只由函数图像对象内部产生、`MorphShape` 只在渲染/测试里构造 | 脚本 API / 能力兑现 | **已关闭（B7 Step 4，`zela.scene@8→@9`）** | 新增三个入口：`scene.group(zela.Group{id})` → `Group` 标签、`scene.phase_anim(zela.PhaseAnim{phase,omega})` → `PhaseAnim`、`scene.morph(zela.Morph{from,to,closed,t,samples})` → `MorphShape`（折线点表走 `Path2D::from_polyline`，与 `reveal`/`polyline` 同口径）；指纹 `15_573_081_187_359_822_637 → 10_059_647_094_731_395_853`。**`Group` 判断结论**：语义 = 纯组合标签、不含变换、不嵌套，与 `Parent`（父子变换累乘）分工清晰 ⇒ **补，不删**。作品 `capability_gaps.lua`（90 帧）一次覆盖 6 项，`script == bundle`；`despawn` 有效性用"去掉那一行"的对照脚本实测：哈希 `d66b333a…` ≠ `29713c3d…`（白圆出现在视口内） |
| W1-06 | `Target.Phase` 写入的 `phase` **没有消费者**：脚本层没有创建函数图像对象的入口（`FunctionX2Y` + `Sampler1D` 不暴露，且函数是 Rust 闭包，ZSB 无法序列化） | 脚本 API / 动画目标 | **记账（B7 Step 4）** | `capability_gaps.lua` 里 `phase` 每帧被 `BindingApplySystem` 写入（组件值确实变了），但 `SceneRecordSystem` 只在函数图像对象上取 `x + phase` ⇒ **像素不变**。触发：要暴露函数图像对象时必须先解决"函数怎么持久化"（命名函数表 vs 表达式子集），不是加一个组件构造器就够 |
| W1-07 | `scene.despawn` **没有时间参数**：在 commit 期执行 ⇒ 只能表达"创建后取消"，无法表达"动画中途移除" | 脚本 API / 生命周期 | **记账（B7 Step 4）** | `scene_service::scene_despawn` → `SceneEncoder::despawn`，写入 SceneScript 的 `despawns`，commit 时一次性应用。作品用它做"条件性不生成"（对照实测见 W1-05）。触发：需要中途销毁时，得给 `despawn` 加时间轴参数（属动画语义扩展，不是补一个字段） |
| W1-08 | A6「`zela.scene` 指纹只变一次」被 Step 4 打破（`@7→@8` 后又 `@8→@9`） | 验收标准 | **记账（用户明确豁免）** | 根因：计划 §6.1 把 `Group` / `Target.Phase` / `Target.MorphT` 当成"可达但零使用"，实际按代码是"不可达"（W1-05）；Step 1 已提交，无法合并。用户在 Step 4 明确选择"开 @8→@9，六项全补" ⇒ 本条只留痕，不作为违规 |
| W1-09 | 「新增能力必须在作品中体现」只是一句口号，没有任何检查 —— 与 GAPS 行号腐烂、`optional_missing` 无人消费同类 | 测试 / 门禁 | **已关闭（B7 Step 5）** | 新 step `cargo verify --step coverage`：能力清单**从源码抽**（`SceneApi` / `ContentApi` 方法 + `Target` / `Curve` / `TransformChannel` 变体，不维护第二张手写表），与 manifest 作品脚本（**去注释**，否则一句注释就能把能力标成已用）比对。未覆盖 ⇒ 必须在 `demo/capability_exemptions.txt` 写 `kind:name -> W1-xx`，且该 `W1-xx` 必须是 GAPS 里真实存在的表格行；豁免过期（能力已被覆盖）也 fail。首次运行：**49/49 覆盖 · 豁免 0**。单测 6 条（抽取 / 两种写法 / 未覆盖项被指出 / 假 GAPS 行 / 过期豁免 / 格式） |
| W1-10 | `Target.TransformValue` + `TransformChannel.Auto` 是**零使用**的默认值兜底：按 `AnimValue` 类型猜写哪一列、猜不中跳过且不报错，且**没有 Rotation 分支**（想转的动画永远不动、还不报错） | 场景 API / 动画 | **已关闭（B7 Step 1，`zela.scene@7→@8`）** | 删除后唯一入口是显式的 `scene.clip_transform(channel, curve)`（Position / Rotation / Scale 三条通道互不覆盖）。六条既有基线**逐位不变**（`hashes` 全绿）⇒ 纯能力面收缩，不动像素。指纹常量**未变**（`FnSig` 是名字级，不覆盖枚举变体）⇒ 版本常量手动升 `@8` 并在 `api.rs` 留注释 |
| W1-11 | manifest 里 7 个作品只有"像素好看"，没有任何回归门禁（fusion / pbr_materials / pbr_lights / pbr_scene / pendulum / orbit / spring） | 测试 / 门禁 | **已关闭（B7 Step 2）** | 全部进 `baselines.txt` 的 `[native]` 段（`HASHES_WORKS` 13 条，批量与独立两条路径**共用**一张表）；`--record` 的门禁要求每件 `--script == --bundle`。顺带修掉 `--record` 的**手写录制清单**会漏录新增作品的缺陷（改为从 `HASHES_WORKS` 派生）。`hashes` 19.53s（A5 ≤30s） |

**M 段小计（B7）**：已关闭 7（W1-01、W1-02、W1-03、W1-05、W1-09、W1-10、W1-11）· 记账 4（W1-04、W1-06、W1-07、W1-08）。

---

## N. B10 批次记账（着色栈生效与 3D 记录接入，2026-10-23）

> 计划《B10执行计划_着色栈生效与3D记录接入》。**路线 A**（矢量后端自己承载 3D 网格）。
> 一句话：把"已经移植进来、但被 `pub(crate)` 挡住"的 3D 能力接出来，让着色栈第一次
> 出现在作品画面上。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| X1-01 | 渲染能力契约只描述 namespace（标量 / 枚举），**无法表达"这个后端接受哪种 3D 网格"** | 渲染 IR | **已关闭（B10 Step 1）** | `RenderCapabilitySet` 新增 `mesh: MeshForms`（`MeshForm::Tri` / `Line`）+ `require_mesh()`：不支持所请求形态 ⇒ **显式报错**（不换成另一种）。`from_names` **不猜**（留空集），两个后端各自显式自述 `tri`（wgpu：`standard_3d()`；矢量：`Upload::mesh` 的 `triangles`）。单测 3 条：`from_names_does_not_guess_mesh_forms` / `unsupported_mesh_form_fails_explicitly` / 并集 |
| X1-02 | 上游的 `Upload::{Mesh, Texture, Environment}` 与 `Record` 的 mesh 记录都在，但 `pub(crate)` 挡住 ⇒ 宿主无法提交 3D 网格 | 渲染后端 / 宿主 | **已关闭（B10 Step 2/3）** | 新公开入口：`MeshRecord`（f64 顶点 / 可选 UV·法线 / u32 三角形 / 材质 / 模型矩阵）+ `PathRecord::Mesh` + `render_3d_with(width,height,samples,view,records)`；`Record::mesh` 写 `params=[0, 三角形数]` 与 `material[3]=1`（受光的关键）。⚠️ **计划 §5.1 与代码不符**：计划说"顶点解析按 f32/u32 硬编码、缺 Float2x3，是全批唯一需发明处"；本仓的 `mesh::Mesh::new` 实际吃 **f64 三元组**（`read::<[f64;3]>`）并在内部压成 `[pos,v]/[normal,u]` ⇒ **不需要新增顶点格式分支**，Step 3 退化为"加一个记录变体" |
| X1-03 | `Material3D` 没有 upstream 的 reflectance 分量（材质四元组是 `[metallic, roughness, reflectance, 1]`） | 场景组件 / 材质映射 | **记账（B10 Step 2；PA Step 3 同步文字）** | 宿主桥取常数 **0.04**（电介质 F0；B14 Step 2 从旧值 0.5 改来），写在 `zela_playback::vector3d::DEFAULT_REFLECTANCE`（B11 Step 5 从 `zela-app` 移入）。触发：作品需要金属/电介质强区分时，把 reflectance 进 `Material3D`（会改场景 API 版本） |
| X1-04 | **环境图（IBL）**：上游 `Upload::Environment` 与 `Light::Environment{id}` 已接出，但场景脚本没有环境图入口，`RenderParams.env` 也没有任何代码桥接到上传 | 渲染后端 / 脚本 | **部分关闭（PG Step 3：内置影棚走通；自定义 .hdr 仍记账）** | PG 实测真因更深一层：环境灯槽位只写了 `kind` / `id`，`color.rgb` 与 `axis.w` 为 0 ⇒ `light.wgsl` 的 kind-4 分支把整项乘成 0（**接了也不发光**）。PG 补齐编码（id 移到 `cone.x`）并接上 `RenderParams.env` + 宿主开关 `ZELA_IBL`，内置影棚（程序化生成、零资产）实测金属亮度 47.5 → 93.1。**仍缺**：自定义 .hdr 的上传出口（`add_environment` 仍 `pub(crate)`）与脚本层表达力 |
| X1-05 | 着色栈的**作品级证据**（V2-15 的收尾）：此前只有单元测试，没有一件作品在着色栈下出帧 | 作品 / 基线 | **已关闭（B10 Step 4）** | 新作品 `shaded_meshes.lua`（墙 + 地板 + 中央亮立方体，全部是真网格）+ `--backend vector`：帧哈希 **`58ab1f0e…`** 进 `[vector]` 基线。判据（`tests/vector3d_shading.rs`，同一份 ZSB 只切 `vector_params`）：tone AgX≠clip（差 >2000 像素）、bloom 关掉更暗、AO 关掉更亮。`frame_loop` 的 3D 分支：`Camera3D → View3`、`LightQueue → Light`（方向取反 + 环境 0.2）、`DrawQueue::meshes → MeshRecord` |
| X1-06 | 2D 图元与 3D 网格**混排帧**仍不支持：有 3D 相机时若队列里还有 2D 图元 ⇒ 显式报错 | 渲染后端 / 装配 | **已关闭（PF Step 2）** | 2D 记录以 `PathOptions.overlay`（`Record.flags` 的 OVERLAY 位）叠进 3D 帧：`scene_records` 产出的像素偏移乘 `frame_w/width` 换到 overlay 坐标系；路由不再拒绝混排并在理由里写明「图层级：N 条 2D 记录以屏幕平面叠加进 3D 帧」。能力用例 `overlay_path_lands_on_the_screen_plane_in_a_3d_frame`（E-3：z=3 的 overlay 记录不随透视放大）；作品 `cap_mix`（cube + 2D 圆/箭头，新 `[vector]` 基线）。H3：网格类 2D 记录不进 overlay ⇒ 显式报错 |
| X1-07 | 矢量后端的 2D 入口**不吃** `RenderParams`（`render_scene_2d` 仍用 `ViewOptions::default()`）⇒ 同一份 `params` 在 2D/3D 两条路径上的含义不一致 | 渲染后端 / 宿主 | **记账（B10 Step 4）** | 实测：`vector_paths` 的帧哈希在 B10 前后**逐位不变**（`98d61c1a…`）—— 2D 分支不读 params，所以"着色栈默认开"只影响 3D 帧。触发：2D 也要 tone/bloom 时，把 `ViewOptions` 从 `RenderParams` 投影过去（会改 2D 基线） |
| X1-08 | **网页黑屏**：B8 给逐帧计时加了 `std::time::Instant::now()`，而它在 `wasm32-unknown-unknown` 上是 `panic!("time not implemented on this platform")` ⇒ 页面加载完产物、第一次出帧即 abort（`select` 停在 `disabled`，canvas 全黑） | 渲染后端 / wasm | **已关闭（B10 修复）** | `zela-render-backends` 引入 `web-time`，四处计时起点改走 `clock()`（wasm 用 `performance.now()`，native 转发 `std::time::Instant`）。实测（Chrome，`python -m http.server` 起的地产）：fourier `播放完成：216 帧`、`select.disabled = false`、canvas `toDataURL` 28.5 KB（非黑）；切到 `shaded_meshes` 正常播放 1 帧。**教训**：`--step wasm` 只编译不运行，抓不到这类运行期 panic（WASM_PORT_SURVEY 的 P0-2 曾被"telemetry 换 ZeroClock"判为已解，B8 又引入一处） |
| X1-09 | `zela-compile` 在 **dev profile** 下编译 JS 脚本会栈溢出（`0xC00000FD`）：boa 的解析 / 编译在未优化构建里递归很深，`build.ps1` 因此卡在 `surface_matrix.js` | 装配 / 构建脚本 | **已关闭（B10 修复）** | `zela_app::compile_script` 把"装配 + 执行"整体放到 **64 MiB 栈**的专用线程（`boa::Context` 是 `!Send`，只能整根线程内建；只把纯数据 `CompiledScript` join 回来）。修复后 dev profile 编 `surface_matrix.js` 成功，payload 与优化构建一致；`build.ps1` 全量通过、16 件 payload 全部与 manifest 相符 |
| X1-10 | **字形孔洞被填**：`ze_content::stroke_outlines` 按**子轮廓**产出多条 `StrokeOutline` ⇒ `content.text` 给每个 counter（`o`/`e` 的内圈）一个独立 `Path`，脚本逐条 `Fill` ⇒ 内圈被当作实心形状填满（wgpu / 矢量后端表现一致，与渲染器无关） | 内容路径 / 矢量后端作品 | **已关闭（字形孔洞修复）** | `stroke_outlines` 改为**每个字形一条** `Path2D`（全部子轮廓 `MoveTo` 分段 + `Close`，闭合时去掉重复尾点），填充按 nonzero 规则挖孔。证据：`vector_paths` 修复前后同帧仅 **424 px** 变化，bbox x 281–369 / y 419–440（正是 `e`、`o` 的 counter），其余笔画逐像素不变；`ze-content` 新增 `counter_contour_stays_in_the_same_path`，矢量后端既有 `concave_ring_keeps_its_hole` 证明多子路径 nonzero 正确。payload 随字形数变化（fourier 63→50 对象），`[vector] vector_paths` 重录为 `07bd4301…` |
| X1-11 | **4 条 `[native]` 帧基线相对 HEAD 不可复现**（fourier / storyboard / surface_matrix / surface_matrix.js）：实测 `45669d03…` / `e6ed98b6…` / `9a34bf69…`，基线 `b6eaa864…` / `6de42d6b…` / `928cd5f9…` | 基线 / 验证 | **已关闭（B13 Step 0）** | 病因 = **同一作品两套字体口径**：payload 断言在 `build.ps1` 的 `ZELA_TYPST_FONT_DIR` 下做，帧基线却在系统字体下录（`grep ZELA_TYPST_FONT_DIR xtask/` 无引用）。修法：`tools/typst-fonts.txt` 成为唯一来源（版本 + 文件名 + SHA-256），`build.ps1` 与 xtask 共读；xtask 的 `cargo_command`（`xtask/src/main.rs`）对**所有**对拍子进程注入 `ZELA_TYPST_FONT_DIR`，字体缺失**显式报错**（不回退到系统字体）。关闭判据 = **可复现**：同代码连续两次录制 16 条哈希逐位相同（`target/xtask/b13-font-run1|2.log`）。4 条新值 `0cb8b4a1…` / `be286560…` / `a6e4394c…`（`surface_matrix` 与 `_js` 相同）；另外 atlas `d180e51f…`、vector_paths `c34f7a3d…` 同步变化（两件也含文本）。此前排除项（回退 `stroke.rs` diff = 0、旧代码+确定字体 = payload `a5bdfbe7…` 等）逐条保留，见 git 历史 `25c1d3a` 的提交说明 |

**N 段小计（B10）**：已关闭 8（X1-01、X1-02、X1-05、X1-06、X1-08、X1-09、X1-10、X1-11）· 部分关闭 1（X1-04）· 记账 2（X1-03、X1-07）。

---

## O. B11 批次记账（Device 注入与动态后端路由，2026-10-23）

> 计划《B11执行计划_Device注入与动态后端路由》。一句话：后端不再由启动参数静态决定，
> 而由「帧内容 × 后端自述能力」在运行时决定；两个后端共享一块 device。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| X2-01 | device features 曾由每个后端各自 request（wgpu 空集、矢量要 `FLOAT32_BLENDABLE` + `DEPTH32FLOAT_STENCIL8`）⇒ 同一块 device 无法共享 | 渲染后端 / 装配 | **已关闭（B11 Step 0/1）** | `BackendKind::required_features(adapter)` 自述 `BackendFeatures { required, preferred }`；`BackendRegistry::required_device_features` 只对**已注册且可用**后端取并集（C2）。守护测试 `union_is_empty_when_only_wgpu_is_registered`：仅 wgpu 注册 ⇒ `Features::empty()`，与改动前逐位相同。实测：`zela-render --backend vector` 注入后 `vector_paths` 仍为 `07bd4301…` |
| X2-02 | `required` / `preferred` 混为一谈会把"有则更好"的 feature 变成硬门槛 | 渲染后端 | **已关闭（Step 0）** | 矢量 required = `DEPTH32FLOAT_STENCIL8` + adapter 自述的 `FLOAT32_BLENDABLE`；preferred = `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`（只影响 MSAA 档位）。单测覆盖：wgpu 显式空集、缺 float32 混合、缺深度模板、required ⊆ adapter |
| X2-03 | 矢量后端在浏览器上**能编译但不能出帧**：`Player::pixels` 用 `map_async` + `getMappedRange` 同步回读，浏览器里 map 完成前就取 ⇒ `OperationError: getMappedRange failed` | 渲染后端 / wasm | **已关闭（B15 Step 1/2/3）** | 病因 = `Player::pixels` 的三连同步回读（`map_async` → `poll(wait)` → `getMappedRange`）；WebGPU 上 `poll` 是 no-op、map 未完成即取。修法：① 浏览器专走 `Player::pixels_async`（`map_async` 回调接 oneshot，`await` 后再取；native 同步签名不变）；② 显示路径改 `render_2d/3d_to_texture_async`（合成进宿主 `Rgba8Unorm` 目标，GPU 内 blit 到 surface，**零 CPU 往返**）；③ `BackendKind::is_available_on_facts` / `unavailable_reason_for` 去掉 wasm 拒绝。浏览器实测（本地 `http.server`）：`?zsb=./vector_paths.zsb` + 后端"矢量" ⇒ 状态栏 `后端：选择 Vector（显式指定矢量）`，画面出帧（截图为矢量作品：橙/蓝/绿路径 + "vector" 字形）；`pbr_lights` 走矢量连续播放。补测（独立 Edge + CDP，同一构建）：3D **510/510 帧**播完、零异常；`?compare=1` 输出 `3b94c49a…`（X4-04）；vector→wgpu→vector 双向切换各重播一次、无异常。内嵌浏览器自身的 GPU 崩溃见 X6-03（环境问题，非应用缺陷） |
| X2-04 | 路由粒度：帧级切换会让同一作品中途换画风 | 路由 | **记账（Step 4）** | 默认**作品级**（第一帧决定后固定）；帧级留了 `--backend auto-frame`，默认关闭。触发：需要逐帧比对两种后端时 |
| X2-05 | 并排分屏对比未做 | Demo 页 | **记账（Step 5）** | 单 canvas；本批只做"切换 + 状态栏给决策理由"（页面 `?compare=1` 仍输出序列哈希）。触发：需要同帧并排比像素差时 |
| X2-06 | 3D 归属未定（矢量做 2D+3D / 分立协作） | 架构 | **记账（B11 §9）** | Device 注入对两条路都是前置，本批不预设立场；等"矢量 3D 在 WebGPU 上跑"的探测结果再定 |
| X2-07 | `--step wasm` 只 `cargo check` 不运行，抓不到运行期 panic（X1-08 的教训） | 验证 | **记账（B11 §8）** | 本批 Step 5 用**浏览器实测**定案（Chrome + 本地 `http.server`）：① 工具栏新增"后端：自动 / wgpu / 矢量"，切到"矢量"触发重播（同一块 device、不重新 fetch、不整页重载），`fourier` 重播 **216 帧**；② `vector_paths` 画面非黑（`canvas.toDataURL` 29 214 B）；③ 状态栏常驻决策行：`后端：选择 Wgpu（自动路由：Wgpu 能力覆盖本帧）`、`后端：选择 Wgpu（首选 Vector 被拒 ⇒ 回退到 Wgpu）；Vector 被拒：浏览器侧矢量后端还没有异步回读…`。矢量在浏览器**不能出帧**（X2-03）⇒ 本批止步于"接口就绪 + 记账"（计划 §7.1 的退路）。headless Chrome + WebGPU 的 smoke 未进 CI（GPU 依赖会 flaky），触发：CI 有稳定 GPU 时补。**B15 更新**：矢量在浏览器已能出帧（X2-03 关闭），但 `--step wasm` 仍只编译；本批把实测要点落成可复现检查点（`adapter_facts()` + 截图 + 状态栏），**未接进 CI** |

**O 段小计（B11）**：已关闭 3（X2-01、X2-02、X2-03）· 记账 4（X2-04、X2-05、X2-06、X2-07）。

---

## P. B12 批次记账（可观测性专题，2026-10-23）

> 计划《B12执行计划_可观测性专题》。一句话：**把已经存在但看不见的接出来，把不存在但需要的建起来**；
> 本批不优化性能、不改渲染语义。
> 四个入口：`ZELA_LOG` / `ZELA_LOG_FILE`（日志）、`ZELA_FRAME_TIMING`（逐帧计时打印）、
> `ZELA_FRAME_REPORT=<path>`（结构化 JSONL）、`ZELA_GPU_TIMING=1`（GPU timestamp）。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| X3-01 | **唯一默认门禁路径零计时能力**：`zela-verify` 里 `timing/Instant/elapsed` 全空，而 B9 之后它是默认门禁 | 验证 / 计时 | **已关闭（Step 1）** | `frame_loop::FrameStats` / `timing_json` / `summary_json` 下沉为**唯一实现**，`zela-render` 删掉本地副本、`zela-verify` 新增 `report_timings`；`ZELA_FRAME_TIMING=1` 时两边字段名**完全一致**（`advance/build/prepare/paint/submit/read/sink/hash/verify/end/total`）。实测：fourier 216 帧 avg_total 2.335ms、pendulum 300 帧 1.746ms |
| X3-02 | **采了没人消费**：telemetry 默认开、每帧有成本，但 headless 入口从不初始化 subscriber（唯一调用点在 `zela-gui`） | 日志 / 接线 | **已关闭（Step 1）** | 新增 `init_tracing_headless()`（默认 `warn`、`ZELA_LOG` 控级别、`ZELA_LOG_FILE` 控目标，打开失败回落 stderr 并**报出来**）；接入 `zela-render` / `zela-verify` / `zela-playback`（浏览器侧同样生效）。不再"付了钱拿不到东西" |
| X3-03 | **决策过程未落盘**：B11 的 `BackendRouter` 只 `eprintln!` 一行，产物里查不到"走的哪个后端、为什么" | 路由 / 验证 | **已关闭（Step 2）** | `RoutingDecision::{validate, json, object_json}`；`run_frames` 对每条决策先 `validate()`（空拒绝理由 = 静默决策 ⇒ **显式失败**）；`zela-verify` 每作品每遍 `[decision]`，并进结构化报告。已知的不报错路径一并修掉：`zela-web` compare 上下文补 `with_adapter`（此前 `caps = None` ⇒ 参数按 `Default` 兜底，能力校验不再发生且无提示） |
| X3-04 | 没有可解析、可 diff、可归档的结构化输出；B8 的 dev/release 教训说明**脱离环境的性能数没有意义** | 观测 / 落盘 | **已关闭（Step 3）** | `ZELA_FRAME_REPORT=<path>` 每作品每遍追加一行 JSON：`timings.all` / `timings.window` / `decisions` / **`env`（GPU / vendor / device / driver / API / os / arch / debug_assertions，必填）** / `cache` / `textures_created` / `gpu`。实测 env 记录为 `NVIDIA GeForce RTX 4060 Laptop GPU / Vulkan / windows / x86_64` |
| X3-05 | **GPU 侧完全黑盒**（24 处 `timestamp_writes` 全是 `None`）：B8 的分级计时全是 CPU wall-clock，测不出 GPU 执行时间 | 渲染 / 归因 | **已关闭（Step 4）** | `zela_render_backends::gpu_timing`：三态 `disabled / unsupported / measured`，`GpuSpan` 在主帧前后各提交一个空 compute pass 写 timestamp（不改任何现有 pass）。实测 `--backend vector`：script 遍 424.995ms（首帧含建管线）、bundle 遍 2.919ms；未请求 feature 时输出 `unsupported: device 未请求 TIMESTAMP_QUERY`（**不填 0**）。单测 3 条。⚠️ 浏览器侧只做到**显式探测**（`gpu_timing::probe(&adapter)` 在页面加载时打 `[gpu-timing]` 到控制台），WebGPU 的实际探测结果**未实测**（矢量在 wasm 还不能出帧，X2-03）——不假设、也不记成"可用" |
| X3-06 | telemetry 自身成本**没有精确 A/B**：它是编译期 feature（`zela-schedule` 默认开），而 7 个 crate 依赖它 ⇒ cargo 特性合并使"单点关闭"无效 | 观测 / 成本 | **记账（Step 6）** | 上界实测（`ZELA_FRAME_TIMING=1`）：pendulum `advance` 0.058–0.074ms/帧（3.3–3.9%，总 1.75–1.88ms）、fourier 0.164–0.167ms/帧（7.0–7.1%，总 2.34ms）。telemetry 采集是 `advance` 的**子集** ⇒ 其成本低于该占比（B8 的 93% 在 hash+sink，不在采集）。触发：需要精确数字时，从 7 个 manifest 同时 `default-features = false` 再对比（跨 manifest 改动，未并入本批） |
| X3-07 | `FrameCacheStats` 有定义、有填充、**零外部消费者** | 渲染 / 观测 | **已关闭（Step 5）** | `diagnostics::{CacheTotals, record_cache, take_cache_totals}`（取走即清零）；`scene_callback` 两个 2D finish 点上报；实测 pendulum 300 帧 `geom 600/600`（动画内容每帧几何都变，符合预期）、`reveal 600/297`（区间变化不触发 miss，与文档口径一致） |
| X3-08 | 量级门禁只在 `ZELA_FRAME_TIMING=1` 时"呈报"，不进退出码 | 验证 / 门禁 | **已关闭（Step 6）** | `FrameStats::check()` 从 `report()` 拆出并**恒开**（单帧 > 500ms ⇒ 失败；首帧含建管线，按既有口径不计）；`zela-verify` 两遍渲染都接上，打印仍可选 |
| X3-09 | **buffer 数与显存未计数**：wgpu 没有全局资源查询，要自己包一层 `create_buffer` / 纹理尺寸累加才拿得到 | 渲染 / 观测 | **部分关闭（B17 Step 0：矢量侧已计数）** | 矢量的全部创建口收进 `render::counted_texture` / `counted_buffer`（纹理按 格式×尺寸×层×mip×采样数 估字节；buffer 自存 `desc.size`）。`ZELA_FRAME_REPORT` 新增 `vector_resources` 与逐帧 `vector_resource_curve`（first/last/峰值），页面状态栏显示累计。**仍记账**：wgpu 侧的 buffer 与两端"实际显存占用"（分配器口径）未做；触发：需要显存曲线时再包 wgpu 侧的 `create_buffer` |
| X3-10 | xtask 自身不初始化 tracing subscriber | 验证 / 日志 | **记账（Step 1）** | xtask 只**拉起子进程**，每个子进程各自 `init_tracing_headless()`；xtask 自己的 `println!` 已足够。触发：xtask 内部需要 `tracing::warn!` 时再补（否则会无提示丢弃） |
| X3-11 | `t_verify` 字段恒 0：GPU 校验 pass 未实现（B8 占位） | 验证 / 计时 | **记账（Step 1）** | 保留字段（字段名与 B8 一致，前后可比），但 `FrameStats::report` 固定打印 `[timing-note] verify_ms 恒 0 是因为 GPU 校验 pass **未启用**`，结构化报告也带 `verify_note`。触发：实现 GPU 校验 pass 时删掉这两处提示 |

**P 段小计（B12）**：已关闭 7（X3-01、X3-02、X3-03、X3-04、X3-05、X3-07、X3-08）· 部分关闭 1（X3-09）· 记账 3（X3-06、X3-10、X3-11）。

---

## Q. B13 批次记账（矢量作品对拍与字体口径，2026-10-23）

> 计划《B13执行计划_矢量作品对拍与字体口径》。一句话：给 11 件"能渲染但零证据"的矢量作品
> 建立作品级对拍证据与基线，并把"payload 用确定字体、帧基线用系统字体"的口径分裂修掉。
> 对拍入口：`zela-parity --scripts crates/app/zela-app/scripts --fps 30 --sample 30 --post match --dump --out <dir>`。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| X4-01 | 矢量后端**丢 2D 开放折线与坐标轴网格**：`fn zela_render_vector::scene::push_shape` 把 `Shape2D::Polyline` 一律当**闭合多边形**（`points.len() < 3` 直接不产出），`Shape2D::Line` 直接返回空；`Axes2D` 生成的网格线因此整片消失。另有更宽的一层：`RevealQueue` / `MorphQueue` **不在 `DrawQueue` 里**，而矢量快照只取了 `DrawQueue` | 渲染后端 / 2D 映射 | **已关闭（B14 Step 1）** | 两处修法：① `push_shape` 拆成 `Line` / `Polyline`（开放，`ranges` 闭合位 0）/ `Polygon`（闭合）三支，新增 `open_path`；单测 4 条（开放位为 0、2 点折线、`Line` 产出几何、`Polygon` 仍闭合）＋ GPU 用例 `open_subpath_fills_implicitly_and_strokes_without_closing`（填充隐式闭合、描边不回连）。② `zela_playback::vector_frame` 把 `RevealQueue` 按**归一化弧长**裁成开放折线（`clip_reveal`，单测 `reveal_clip_keeps_the_parameterized_span`）并追加在 DrawQueue 之后；`MorphQueue` 非空 ⇒ 路由**显式拒绝**（新增 `FrameContent::has_morphs` + 单测）。实测 `--post match`：fourier 覆盖率 0.7239 → **0.0195**、MAE 4.45 → **0.54**、内部区 100.78 → 3.01；storyboard 0.6394 → 0.0039 / 7.25 → 0.62 / 101.76 → 6.96；pendulum 0.3614 → 0.0258 / 0.35 → 0.06 / 61.91 → **2.00**；orbit 0.0131 / MAE 0.09（全绿）、spring 0.0144 / 0.04（全绿）。剩余细差见 X5-04 |
| X4-02 | 矢量 3D 输出**系统性偏暗**：同一份 `RenderParams` 下，fusion / shaded_meshes / pbr_* 的整体亮度显著低于 wgpu | 渲染后端 / 3D 光照 | **部分关闭（B14 Step 0/2：亮度项已对齐；剩余差异归因到取景与高光/点光衰减）** | 归因（Step 0，只测量）：H1 tone 排除（`--post match` 已关 tone，差异仍在）；H3 sRGB 排除（白/灰两种 albedo 下比值恒定 2.26 ⇒ 不是传递曲线）；**H2 光照口径成立**。真因两条：① 桥里把光源方向**多取反了一次**（旧注释称 `LightQueue.direction` 是"传播方向"，而 wgpu `fs_pbr` 直接 `dot(n, dirs[i].xyz)` 当"指向光源"）⇒ 矢量侧所有正面收不到方向光，只剩 ambient（实测 white=124/灰=60，正好 = 0.2·albedo）；② 强度归一化不同：wgpu `(1−ambient)·I·NdotL·(kd·albedo/π+spec)`，矢量 `I·NdotL` ⇒ 桥里乘 `(1−ambient)/π`；③ `DEFAULT_REFLECTANCE` 0.5 → **0.04**（与 wgpu 电介质 F0 对齐）。修后（`--post match`）：pbr_scene 内部区 26.11 → **7.63**、MAE 0.73；pbr_lights 24.22 → **4.59**、MAE 0.54；shaded_meshes 30.53 → 23.83、MAE 19.48；pbr_materials 35.60 → 24.33、MAE 4.56；fusion 149 → 136.25。未达门限的部分见 X5-02 / X5-03 / X5-05（取景、点光衰减与高光、透明合成），**门限未放宽** |
| X4-03 | **后处理政策差异**混进差异里：矢量按自身能力默认开 tone/bloom/ao，wgpu headless 默认全关 | 渲染后端 / 口径 | **记账（B13 Step 1）** | 同一批作品两种口径各跑一次：2D 作品（fourier / storyboard / pendulum / orbit / spring / vector_paths）**逐项完全相同**（2D 分支不读 params，见 X1-07）；3D 作品数值变化：fusion 全图 MAE 14.15→15.09、pbr_materials 9.18→6.61、pbr_lights 4.27→2.22、pbr_scene 3.34→2.47、shaded_meshes 23.08→24.94。⇒ 差异里有一块是**政策**不是算法。两份记录在：`target/xtask/parity/`（backend-default）与 `target/xtask/parity-match/`（match） |
| X4-04 | 浏览器侧无法做同一套对拍 | 验证 | **已关闭（B15 补测）** | 依赖解除：B15 把矢量回读改成异步（`Player::pixels_async`），`zela-web` 的 compare 分支改走 `VectorFrame::render_async`（与 native 共用 `scene_records` / `render_2d_async`）。浏览器实测（独立 Edge + CDP，`?zsb=./vector_paths.zsb&compare=1` + 后端"矢量"）：`对拍完成：3b94c49a137f8a9f98ee0c91577adc4e4b51049c1d047b2af41dbb88dca768e0（1 帧）`，控制台同时输出 `[frame-hash] sequence = …` ⇒ **可行性成立**（本批只验证可行性，不批量录浏览器基线） |
| X4-05 | **字体口径分裂**（X1-11 的病因）：payload 断言用 `ZELA_TYPST_FONT_DIR`，帧基线用系统字体 | 验证 / 可复现性 | **已关闭（B13 Step 0）** | `tools/typst-fonts.txt` = 唯一来源（版本 + 文件名 + SHA-256），`build.ps1` 与 xtask 共读；xtask 给所有对拍子进程注入 `ZELA_TYPST_FONT_DIR`，缺失即报错。验证：连续两次录制 16 条逐位相同；含文本 6 条基线重录（fourier `0cb8b4a1…`、storyboard `be286560…`、surface_matrix(_js) `a6e4394c…`、atlas `d180e51f…`、vector_paths `c34f7a3d…`） |
| X4-06 | `[vector]` 段只有 2 条（`vector_paths` 2D + `shaded_meshes` 3D），9 件矢量可渲作品零门禁 | 基线 / 门禁 | **已关闭（B13 Step 3；B14 已按修复重录）** | `HASHES_WORKS` 扩到 25 条（同一作品在 `[native]` 与 `[vector]` 各一条，靠 `--work` 顺序配对、标签 `name#vector` 区分）；`[vector]` 11 条。B13 首录锁定的是"当时行为"；**B14 在 X4-01 / X4-02 / X5-01 修完后一次性重录**（6 条 2D + 5 条 3D），现在锁的是修复后的画面（剩余已知差异见 X5-02/03/05） |

**Q 段小计（B13）**：已关闭 4（X4-01、X4-04、X4-05、X4-06）· 部分关闭 1（X4-02）· 记账 1（X4-03）。

---

## R. B14 批次记账（矢量 2D 开放折线与 3D 亮度口径，2026-10-23）

> 计划《B14执行计划_矢量2D开放折线与3D亮度口径》。一句话：把 B13 对拍暴露的两类差异
> 修到"可解释、且大部分收敛"，并把过程中挖出的第四层原因（灯从未生效）一并关掉。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| X5-01 | **`LightQueue` 资源从未注入**：`SceneRecord3DSystem` 的 `with_world_and_resource_mut::<LightQueue>` 在资源缺席时不生效且无提示 ⇒ 脚本里的 `Light` 组件被两个后端**一起忽略**，一直只用默认灯（`default_light_bytes` / 桥的默认 Sun） | 装配 / 3D 光照 | **已关闭（B14 Step 2）** | 实测证据：`zela-parity` 在两处 dump `resources().get::<LightQueue>()` 都是 `None`（wgpu 输入侧与矢量 `VectorFrame::take`/`render` 侧）；`fn zela_playback::assemble_runtime` 的资源注入只插了 DrawQueue/DrawQueue3D/MorphQueue/RevealQueue，**漏了 LightQueue**。修法：该函数补 `LightQueue::new()`。影响（用户明确豁免 §1.1）：含 `Light` 的 **7 件 `[native]`**（demo3d / atlas / fusion / pbr_materials / pbr_lights / pbr_scene / …）与 **5 件 `[vector]`** 基线一并重录；不含灯的作品逐位不变 |
| X5-02 | **3D 取景不一致**：同一相机（eye/target/up/fov）下，矢量侧的物体比 wgpu 大约 8%、并整体偏移（≈35px） | 渲染后端 / 相机映射 | **已记录·参数性（B19 Step 1；非缺陷）** | `shaded_meshes` 最差帧：亮立方体 bbox wgpu x 204..337（133 px）vs 矢量 236..380（144 px），中心 270 vs 308；墙面左缘 y=60 行 102 vs 122。亮度已对齐（中位 Δ=0.33、有符号偏差≈0）⇒ 残差来自**几何位置**，不是颜色；两边相机映射不同（`vector3d::view3` 的 frame/center vs `Camera3D::view_projection(aspect)`），均自洽。<br>**类别**：已记录（参数性差异，非缺陷）。<br>**参照物**：wgpu —— 已按 V-04 **冻结**，不再改动。<br>**处置**：不再收敛。若将来 wgpu 解冻或矢量成为唯一后端，本条重新评估。<br>**门禁**：`zela-parity` 不进 `cargo verify` 七步，本条**不构成任何门禁失败** |
| X5-03 | **点光衰减与高光实现不同**：wgpu 点光 `att = clamp(1−d²/r²,0,1)` 再乘 `(1−ambient)`；矢量是 `window²/max(d²,1e-4)`；高光虽同为 GGX 但 `spec` 归一化、能量补偿不同 | 渲染后端 / 3D 光照 | **已记录·参数性（B19 Step 1；非缺陷）** | 含点光/低粗糙度的作品残差明显大于只有方向光的作品：pbr_scene 内部区 7.63、pbr_lights 4.59（近门限）vs pbr_materials 24.33、shaded_meshes 23.83（低粗糙亮立方体/金属材质）。方向光部分已收敛（pbr_scene MAE 0.73）。两个衰减模型**都对**，只是选择不同。<br>**类别**：已记录（参数性差异，非缺陷）。<br>**参照物**：wgpu —— 已按 V-04 **冻结**，不再改动。<br>**处置**：不再收敛。若将来 wgpu 解冻或矢量成为唯一后端，本条重新评估。<br>**门禁**：`zela-parity` 不进 `cargo verify` 七步，本条**不构成任何门禁失败** |
| X5-04 | 2D 细笔画 / 字形边缘的内部区残差 | 渲染后端 / 2D AA | **已记录·参数性（B19 Step 1；非缺陷）** | fourier 内部区 3.01、storyboard 6.96（门限 2；全图 MAE 0.54 / 0.62 已达标），pendulum 2.00 卡线。覆盖率已全部 ≤0.026。原因：reveal 曲线按弧长裁剪 vs wgpu 预细分 + 着色器丢弃、以及**解析 AA vs MSAA** 在 1–2px 笔画上的差异——两者都对。<br>**类别**：已记录（参数性差异，非缺陷）。<br>**参照物**：wgpu —— 已按 V-04 **冻结**，不再改动。<br>**处置**：不再收敛。若将来 wgpu 解冻或矢量成为唯一后端，本条重新评估。<br>**门禁**：`zela-parity` 不进 `cargo verify` 七步，本条**不构成任何门禁失败** |
| X5-05 | `fusion` 的**透明面板合成**差异（内部区 MAE ≈136，全图 13.8） | 渲染后端 / 2D-3D 合成 | **重定性：结构性（非参数性）；PF Step 1 已修根因一，口径差异记账（PF-02）** | **原判定错在前提**：分层记录要求"两个实现都对、只是选了不同参数"；这里矢量侧当时**缺少变体映射**（Flat/Lambert/Pbr 输出逐位相同，实测 `d2cf72af…` ×3），属结构性缺失 ⇒ 按新判据「能力在某一后端完全不存在 ⇒ 不适用参数性记录」。PF 修正后同一帧：中心均值 77.7 → **128.6**、峰值 137 → **222**（wgpu 213.6 / 20.9）。**剩余差异**：wgpu 的透明 pipeline 用 `PREMULTIPLIED_ALPHA_BLENDING`，而 `cube.wgsl` 三个变体输出的是**直色**（未预乘）⇒ wgpu 侧偏亮；矢量 OIT 的 alpha 合成是对的。该 wgpu 侧修正属 V-04 的"正确性修复"允许范围，本批不动（记 PF-02）。<br>**门禁**：`zela-parity` 不进 `cargo verify` 七步 |
| X5-06 | `LightQueue.direction` 的**注释口径错误**：文档写"传播方向"，而代码（wgpu `dot(n, dirs)`）按"指向光源"消费；桥据此多做一次取反 | 文档 / 光照 | **已关闭（B14 Step 2）** | 与 B10 的 `params.z` 半宽、B14 的 `scene.rs` 注释是同一型（"注释宣称与另一侧一致"）。改法：`vector3d` 头注与 `lights()` 改为"指向光源、原样透传"，并加单测 `lights_pass_direction_through_and_scale_intensity` 锁定方向与 `(1−ambient)/π` 强度换算 |
| X5-07 | `scene.rs` 里"线段没有面积：填充语义为空（**与 wgpu 侧三角化一致**）"是错误注释 | 文档 / 2D 映射 | **已关闭（B14 Step 1）** | wgpu 的 `contour_of(pts, false)` 对 `Line`/`Polyline` **有开放轮廓几何**（描边用），只是填充面积为 0。注释改为指向可执行证据（`open_path` + 4 条单测 + 1 条 GPU 用例），不再写"与 X 侧一致" |
| X5-08 | GAPS「统计与维护约定」的总表只统计 A/B/C 段，未纳入 D–Q | 文档 | **已关闭（B14 §7.2）** | 在总表前明确标注"总表只统计 A/B/C；D 及之后见各段自己的汇总"，不再假装覆盖全文件 |
| X5-09 | `cargo_command` 的约束写得太宽（"不得在别处直接 `Command::new`"），而 `cargo_capture`（只跑 `cargo metadata`）并不需要字体注入 | 工具 / 文档 | **已关闭（B14 §7.3）** | 收窄为"**涉及对拍 / 渲染的子进程**不得直接 `Command::new`；只读工具不受此限" |
| X5-10 | 诊断脚本会随 `cargo clean` 消失（B12 Step 0 的治理项） | 工具 | **已关闭（确认）** | 本批复用前确认 `tools/diag/{README.txt,export-vector.ps1,encode-vector.ps1,holes.py,diff.py}` 均已在版本控制中（B12 Step 0 纳管） |

**R 段小计（B14）**：已关闭 6（X5-01、X5-06、X5-07、X5-08、X5-09、X5-10）· 已记录 3（X5-02、X5-03、X5-04）· 记账 1（X5-05，PF 重定性为结构性；剩余差异见 PF-02）。

---

## S. B15 批次记账（矢量后端进 wasm，2026-10-23）

> 计划《B15执行计划_矢量后端进wasm》。一句话：**让矢量后端在浏览器里真正出帧**——
> 根因只有一处同步回读，而异步容器（rAF + `spawn_local`）早已存在。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| X6-01 | Step 0：wasm 侧 adapter 能力此前是**推断**，不是实测（证据只有"Player 建起来了"） | 渲染 / wasm | **已关闭（B15 Step 0）** | `zela-web` 在 `create_display` 落一行 `[adapter-facts]`，并暴露 `adapter_facts()`（控制台 / 页面可复制、可 diff，不含时间 / 负载）。本机 Chrome 实测：`features=[0, ff8f] can_compute=true indirect_first_instance=true float32_blendable=true depth32float_stencil8=true adapter_specific_formats=false r32float_blendable=true rgba32float_storage=true rgba8unorm_storage=true surface_format=Bgra8Unorm`。required 全齐 ⇒ 矢量可用；`INDIRECT_FIRST_INSTANCE` 也有（TriMesh / LineMesh 二选一不受限） |
| X6-02 | 乙的"零拷贝"口径会误导：计划 §1.5 称合成目标是 `Rgba32Float`，实测是 `Rgba8Unorm`（`zela_render_vector::render::COLOR`；`Rgba32Float` 只是 3D slab bounds 的中间纹理） | 文档 / 口径 | **已关闭（B15 Step 2）** | 事实：surface 是 `Bgra8Unorm`，WebGPU 核心不允许它当 storage texture ⇒ 矢量**永远无法直接写 surface**。乙的真实形态 = **零 CPU 往返**（去掉 1.2 MB 回读 + 1.2 MB `write_texture`），保留一步 GPU→GPU resolve（`Rgba8Unorm → surface`）。实现：`zela_render_vector::api::render_2d_to_texture[_async]` / `render_3d_to_texture[_async]` + `zela-web::VectorTarget` / `Blit` |
| X6-03 | Trae 内嵌浏览器的 WebGPU 在 3D 长播后崩溃（`webgpu found no adapters`），曾阻断 compare / 切后端实测 | 验证 / 环境 | **已关闭（B15 补测：换独立 Edge）** | 现象：内嵌浏览器播放 `pbr_lights` 到 301/510 帧后，`navigator.gpu.requestAdapter()`（普通与 `forceFallbackAdapter`）都返回 `null`；重启其 GPU 进程、换 origin、开新标签页均未恢复。补测：用**独立 Edge + CDP**（同一份 wasm / 同一 URL）跑同一构建：① 2D/3D 矢量出帧，`pbr_lights` **510/510 帧**播完、零异常（截图 `out/b15-edge-3d-vector-50s.png`）；② `?compare=1` 输出 `3b94c49a…`（见 X4-04）；③ backend 双向切换（vector→wgpu→vector）均重播且只有一条循环。⇒ 崩溃属**内嵌浏览器环境**，不是应用缺陷。⚠️ "环境"结论只针对那次崩溃；**churn 候选根因并未被否定**，B17 把它单独实证并关闭（X7-01） |
| X6-04 | `--step wasm` 仍只 `cargo check`（X2-07） | 验证 | **记账（B15 §7）** | 本批把浏览器实测落成可复现检查点（`adapter_facts()` + 截图 + 状态栏），但**没有**接进 CI；本批只记账，不实现 |

**S 段小计（B15）**：已关闭 3（X6-01、X6-02、X6-03）· 记账 1（X6-04）。

---

## T. B17 批次记账（矢量后端资源生命周期，2026-10-24）

> 计划《B17执行计划_矢量后端资源生命周期与移动端可用性》。一句话：**每帧重建整套 GPU 资源
> （churn）才是移动端崩溃的嫌疑，先修生命周期、把"每帧建了多少"变成数，再谈移动端判定。**
> 本批止于 Step 2；Step 3/4（移动端实测与按实测补判定）等设备数据。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| X7-01 | **每帧新建一整套 `Player`**（`prepare_view` 内 `Player::new`）⇒ 画布 / 附件 / 缓冲 / 列表 / bloom / AO 每帧分配+释放；上游"常驻 Player + eviction/packing"的设计被架空 | 渲染 / 资源生命周期 | **已关闭（B17 Step 1）** | `Gpu` 新增 `players: HashMap<(w,h,samples), Player>`；`take_player` 取出并 `Player::reset`（清 `Store` / 待上传图像，保留画布 / 附件 / 缓冲 / 列表 / 目标），`put_player` 归还（上限 4 份）。`Store::clear` 推进 `generation` ⇒ 数组从 0 逐字节重传，键从 1 重分配（无上一帧残影）。`targets` 记录 `external`，读回/外部目标模式切换时重建。**25 条基线逐位不变**；实测 `pbr_lights`（510 帧，矢量）首帧 38 纹理 / 33.5 MB / 31 buffer，**第 2–510 帧全为 0**；浏览器同一构建 510/510 帧，状态栏 `tex 37 / buf 30（34.0 MB）` 从 421 帧到播完不变。⚠️ 本条只覆盖 `Targets` 的"外部/内部"切换；**同一目标身份不同**（重播换 surface）是另一层，见 V-09 |
| X7-02 | **矢量后端资源计数始终为零**：`textures_created` 只覆盖 wgpu 后端（`wgpu_2d.rs` / `wgpu_3d.rs` 三个点），走矢量时恒为 0，像是"零分配" | 渲染 / 观测 | **已关闭（B17 Step 0）** | 本 crate 的全部创建口收进 `counted_texture` / `counted_buffer`（28 处，含 vector / bloom / occlusion / shadow / environment）；出口 `resource_totals()` / `take_resource_totals()` / `ResourceTotals::{json,since,plus}`。`ZELA_FRAME_REPORT` 每作品每遍加 `vector_resources` + `vector_resource_curve`（frames / first / last / 峰值），并给旧字段加 `"textures_created_scope":"wgpu-only"`（不把两套语义混成一个"全局"数）。页面状态栏在矢量播放时显示 `矢量资源 tex/buf（MB）` |
| X7-03 | 上游 `evict` / `pack_if_worthwhile` / `generation` 这套淘汰与打包机制**从未被真正执行**（每帧 `Player::new` ⇒ `dead` 永远 0） | 渲染 / 资源生命周期 | **已关闭（B17 Step 1）** | 复用后 `Store.dead` 仍是 0（本批逐帧复位，不启用多帧在飞的 `evict`——它依赖 `settle` 语义）；但 `generation` 第一次真正参与工作（每次复位 +1，驱动 `Buffers.sent` 归零）。**packing 未被首次触发**（`dead = 0`）⇒ 无新行为、无像素变化。`evict` 保留原样，不本批启用 |
| X7-04 | 移动端可用性判定可能**过宽**：`rgba32float_storage` / `rgba8unorm_storage` / `r32float_blendable` 已探测但未进 `is_available_on_facts`；若移动端不支持某项，会"直接放行 ⇒ 创建存储纹理 ⇒ 卡死" | 路由 / 能力判定 | **已关闭（PA Step 1：数据已到，命中第一分支）** | 实到数据（vivo V2270A / Adreno 660）：`[adapter-facts] features=[0, e7f3] can_compute=true indirect_first_instance=true float32_blendable=true depth32float_stencil8=true adapter_specific_formats=false r32float_blendable=true rgba32float_storage=true rgba8unorm_storage=true surface_format=Rgba8Unorm` ⇒ required 三项**全 true**；B17 修完 churn 后**不崩溃**；B18 修完 AO 的 workgroup 读回（X8-07）后**条纹消失**。三条分支命中第一条 ⇒ **不卡 ⇒ 收尾、不补判定**。🌟 反事实：当时一度准备"补判定，让移动端回退 wgpu"——若当时凭猜测补了，移动端就永远拿不到矢量画面，而实际两个问题（churn、驱动 bug）**都不是能力问题**。判定本来就是窄的、是正确的；不是所有移动端问题都该往"判定不全"上归因（与 V-13 ①「先有数据再改判据」同一条方法论）。强制约束 ③ 仍成立：本条是在**数据到达之后**关闭，不是绕过；若换设备复现崩溃，**新开条目**，不复活 X7-04 |

**T 段小计（B17）**：已关闭 4（X7-01、X7-02、X7-03、X7-04）。

---

## U. B18 批次记账（移动端 3D 条纹与 MSAA 能力判据，2026-10-24）

> 计划《B18执行计划_移动端3D条纹与MSAA能力判据》。一句话：**移动端因为没有
> `adapter_specific_formats`，`max_samples` 走了"不查硬件、硬编码 4"的分支，从而在不支持
> 4x MSAA 的设备上创建了 4x 附件（规则横向条纹）。** 修法是删掉常量分支，两条路径问同一个源。
> 本批代码已就位；条纹是否消失需移动端复测。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| X8-01 | 移动端 3D 出现规则横向条纹：**根因待定**（H1「4x MSAA 不被支持」已被源码证据证伪，见 X8-06） | 渲染 / MSAA | **已关闭（B18 收尾，见 X8-07）** | 判别开关：`?msaa=1` 强制 1x。条纹消失 ⇒ 与 MSAA 有关（采样表/驱动）；仍在 ⇒ 非 MSAA（行分块 H2 / 链表溢出 H3，X8-04 已可观测）。`[adapter-facts]` 已带 `max_samples` / `max_storage_buffer_binding_size`，无需再靠拍屏。**实测：`?msaa=1` 条纹不变 ⇒ 与 MSAA 无关；最终根因是 AO 低通的 workgroup 内存读回（X8-07）** |
| X8-02 | `max_samples` 两条路径判据不一致：未启用 specific 时**硬编码 4**，启用时才查格式能力 | 渲染 / 能力判定 | **已关闭（B18 Step 1）** | `Gpu::from_parts` 改调 `msaa_samples(adapter)`（只查 `get_texture_format_features(COLOR/DEPTH)` 的 `MULTISAMPLE_X4/X8`），不再有无条件常量分支。单测：x8 ⇒ 8、x4 ⇒ 4、空 ⇒ 1。⚠️ 在 WebGPU 上与旧值同为 4（X8-06）；native 启用 specific 时行为不变（25 条基线逐位不变） |
| X8-03 | 移动端 3D 可能降为 **1x MSAA**（边缘锯齿变明显） | 渲染 / 画质 | **已关闭（B19 V-10：触发条件已证伪）** | 原取舍是"若 `?msaa=1` 能消条纹，则移动端要在 1x 锯齿与 4x 条纹之间选"。B18 实测 **`?msaa=1` 条纹不变**，且 X8-06 证明移动端 `max_samples` 本来就是 4 ⇒ 这个取舍**不存在**；条纹根因是 AO 低通（X8-07）。该 feature 由 adapter 决定是否提供，不是应用能请求的 |
| X8-04 | 链表溢出无提示：`overflowed()` 在容量已到 device 上限时直接 `return false`，该帧装不下的 see-through 片元被丢弃且无任何出口 | 渲染 / 观测 | **已关闭（B18 Step 2）** | 新增 `OverflowTotals`（events / appended / capacity / most），`overflowed` 撞上限时记录最坏一次；`ZELA_FRAME_REPORT` 新增 `vector_overflow`；浏览器播放结束落 `[vector-overflow]`（vConsole 可见）。单测覆盖 `plus` 的最坏值合并 |
| X8-05 | `[adapter-facts]` 完全不打印 limits（本轮最大的观测盲区：`max_samples` / 溢出上限都看不见） | 观测 / 能力 | **已关闭（B18 Step 0）** | 一行内新增 `max_samples`（与 `Gpu` 同源）、`max_storage_buffer_binding_size`（溢出上限 = 该值 / 16）、`max_texture_dimension_2d` |
| X8-06 | 计划 H1 的因果链不成立："没有 `adapter_specific_formats` ⇒ 查询退化为规范最小集 ⇒ 1x" | 渲染 / 依据 | **已关闭（B18 Step 0 源码证据）** | wgpu 30 的 WebGPU 后端 `get_texture_format_features` 直接返回 `format.guaranteed_format_features(adapter.features())`（`backend/webgpu.rs`）；`Rgba8Unorm` 与 `Depth32FloatStencil8` 的保证位都含 `MULTISAMPLE_X4`。⇒ 移动端 `max_samples` **本来就是 4**，旧硬编码 4 在 WebGPU 上与查询同值。H1 就此排除，条纹根因转 H2（行分块）/ H3（溢出）或驱动 |
| X8-07 | **横向条纹的根因**：AO 低通（`occlusion.wgsl` 原 `blur`）把水平结果暂存到 `var<workgroup> rows`，`workgroupBarrier()` 之后再按行读回；在 Adreno 660（`ro.board.platform=lahaina`，骁龙 888）上该读回不可靠——读到的行错乱/未初始化，于是 AO 每 16 行（tile 高 `T`）分带，最终表现为物体表面的横向明暗条纹 | 渲染 / AO（`occlusion.wgsl`） | **已关闭（B18 收尾）** | 逐层判据见下方「X8-07 调试纪要」。修复：可分离低通改为**两趟独立 compute**（`blur_h` 写中间纹理、`blur_v` 读它写 `occlusion`），新增一张 R32Float `occlusion across`；不再使用 workgroup 内存与 barrier。数学同序（先水平后垂直、同一高斯核与深度权重），桌面像素不变 |

**U 段小计（B18）**：已关闭 7（X8-01、X8-02、X8-03、X8-04、X8-05、X8-06、X8-07）。

### X8-07 调试纪要：移动端 3D 横向条纹（AO 低通）

**设备与现象**

- 设备：vivo V2270A，`ro.board.platform=lahaina`（骁龙 888 / Adreno 660），Android + Edge/Chromium。
- 现象：矢量后端 3D 场景（`pbr_lights`，PBR 光照实验室）所有物体表面出现**规则横向明暗条纹**，跨物体在同一屏幕行上对齐。
- 对照：同一份 wasm 在桌面 Edge 强制矢量后端干净；同机 wgpu 后端干净。
- 量化：截图自相关峰值 **27 物理像素 = 16.00 逻辑像素**（画布 640×480，CSS 放大 1.6875）。

**排查过程（每一步的判据与结论）**

| 判据 | 做法 | 结论 |
|---|---|---|
| 调度 | 合成 compute 的 `@workgroup_size` 由 16×16 改 16×4，dispatch 同步改 | 周期仍 16 ⇒ 与 compute 调度无关 |
| tile 表 | 强制 `bare()` 恒 false（全像素走 `composite`） | 条纹仍在 ⇒ 与 tile 列表 / `keep` 分支无关 |
| 输出存储 | 向 `view_out` 写已知平滑渐变 | 无条带 ⇒ 该纹理写入/读取正常 |
| 通道定位 | 分别只输出 `m[0]`（paint）/ `m[1]`（light） | paint 全黑（lit 网格的 paint 本就置 0）；light 有彩色条纹 ⇒ **条纹在 light 通道** |
| 值与存储 | light 通道写常量 0.5 vs 写真实值 | 常量无条纹、真实值有条纹 ⇒ 与**值**相关，不是格式/存储 |
| 着色入口 | `fs_mesh_with_light` 直接返回洋红 | 物体全变洋红 ⇒ 确认 raster base 的 MRT 着色入口 |
| 计算层 | `lit_light` 的 radiance 换常量 | 无条纹 ⇒ 在 `reflected` 内 |
| AO | `textureLoad(occlusion, …)` 不采样 | **条纹消失** |
| 阴影 / DFG / 法线 / 世界坐标 / `dpdx`+`dpdy` | 逐项替换成常量或直出 | 条纹均仍在 ⇒ 逐个排除 |
| AO 纹理内容 | 把 AO 值当颜色直出 | 物体上明暗交替、周期 16 ⇒ **AO 纹理本身有问题** |
| AO 分趟 | 跳过低通直接用 `sao` 输出 | 平滑 ⇒ 条纹由低通引入 |
| 低通内部 | 只做水平段 / 直出共享内存里装载的原始 AO | 前者仍有条纹；后者平滑（装载正确） |
| 写读通路 | 把行号写进 `rows` 再读回 | 出现 16 行周期而**不是**平滑渐变 ⇒ `rows` 写读错乱 |
| 坐实 | 每像素直接从 `tile_ao`/`tile_z` 重算二维低通（不经 `rows`） | 平滑 ⇒ 根因确认 |

**根因**

`occlusion.wgsl` 的低通是"tile + apron 装进 `var<workgroup>` → 水平加权写 `rows` → `workgroupBarrier()` → 垂直段读 `rows`"。在 Adreno 660 上，**barrier 之后从 workgroup 内存按行读回不可靠**：部分行读到错位/未初始化的值。因为 `rows` 是逐行暂存，错乱就按 tile 高（`T = 16`）分带，正好解释了 16 逻辑像素的周期。与 MSAA、能力判定、tile 列表、compute 调度、输出纹理格式**都无关**。

**修复**

- `occlusion.wgsl`：删掉 tile/apron 与 `var<workgroup>`，把低通拆成两趟独立 compute：`blur_h`（读 `sao` 结果，横向加权写中间纹理）与 `blur_v`（读中间纹理，纵向加权写 `occlusion`）。
- `occlusion.rs`：新增一张 R32Float `occlusion across`；`blur` 的 pipeline 拆成 `blur_h` / `blur_v`（复用同一 bind group layout），`encode` 里 `dispatch(3)` 与 `dispatch(4)` 各一次。
- 权重与顺序与原实现一致（同一高斯核、同一深度权重、先水平后垂直），桌面像素不变。

**可复用的调试手段**

- **adb 全链路**：`adb reverse tcp:8080 tcp:8080` 让手机用 `localhost`（`navigator.gpu` 只在 https/localhost 存在，局域网 http 会直接报"不支持 WebGPU"）；`adb shell am start -a android.intent.action.VIEW -d <url>` 打开页面；`adb shell screencap -p /sdcard/x.png` + `adb pull` 取图。
- **屏幕必须常亮**：`adb shell svc power stayon usb`、`adb shell settings put system screen_off_timeout 1800000`；否则测试跑到一半息屏，截图是纯黑，会被误读成"渲染全黑"。
- **周期测量**：Python + PIL 取一列亮度做自相关，峰值 lag ÷ 1.6875 得逻辑像素，直接把条纹周期与代码里的 tile/workgroup 尺寸对上。
- **判据开关形态**：一律写成 shader 里的 `const DEBUG_*: bool`，一次只翻一个；每轮构建后先看"画面是否仍正常"，再读结论。写洋红/常量是最快的"新代码确实在跑"的存活证明。
- **二分顺序**：先证通道（paint/light）→ 再证"存储 vs 内容"（常量/渐变 vs 真实值）→ 最后才进算法；每一步都要有一个**能看见**的输出，不靠推理。

**缓存与环境坑（下次先排这三项，别再浪费时间）**

1. **wasm 被浏览器缓存**：`python -m http.server` 不发 `Cache-Control`，Chrome 会按 `Last-Modified` 启发式缓存，重编后**仍跑旧 wasm**。本轮多次"改动看似无效"甚至"洋红也不出现"都是它造成的误判。对策：起一个带 `Cache-Control: no-store` 的临时静态服务器；并用 `adb reverse tcp:8080 tcp:8081` **保持 origin 仍是 `localhost:8080`**，否则 `localStorage` 里的 `zela-backend=vector` 会丢，页面会无提示路由到 wgpu 后端，测的就不是矢量路径了。
2. **手机息屏/锁屏**：截图纯黑（PNG 仅 ~15 KB）先怀疑屏幕状态，不要怀疑渲染。查 `adb shell dumpsys power | Select-String mWakefulness`（要 `Awake`）与 `adb shell dumpsys window | Select-String mDreamingLockscreen`（要 `false`）。有密码/图案时 adb 无法代解锁，需人工解锁一次。
3. **target 缓存损坏 / rustc ICE**：`cargo verify` 出现过 `no entry found for key` 与 dep-info 写不进去（`.fingerprint` 目录损坏）。删 `target/debug/.fingerprint` 并设 `CARGO_INCREMENTAL=0` 即可恢复；`--full` 不是有效参数（`cargo verify` 默认即 full）。

**下次的直觉**

- "移动端独有 + 固定周期（8/16/32）+ 只出现在某个 pass 的输出上" ⇒ **先怀疑 workgroup 共享内存 + `workgroupBarrier()`**，尤其当周期正好等于代码里的 tile 尺寸时；MSAA、纹理格式、compute 调度的优先级都排在它之后。
- 同一段 shader 里"用共享内存做可分离滤波"在桌面成立、在移动端不成立时，最稳的替代是**多趟 pass + 中间纹理**，而不是继续调整 tile 尺寸或 barrier 数量。


---

## V. B19 跨批补记（讨论过但没进账的东西，2026-10-24）

> 计划《B19执行计划_渲染后端收尾与转向》Step 0。**本段不新增缺陷**：三条默认值兜底
> （V-01..V-03）、五条已拍板的定位（V-04..V-08）、四处滞后记账（V-09..V-12）与一次计划
> 失误（V-13）。之所以单列一段，是因为这些都是"已经发生、却没进 GAPS"的事——
> 没进这里的东西，下一批会重新被讨论一遍。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| V-01 | **阴影整块被关**（能力消失，不是参数丢失）：桥给 `Light::Sun` / `Light::Spot` 一律写 `shadows: false`，而后端侧阴影是完整实现 | 渲染 / 3D 光照 | **部分关闭（PB Step 1：开关与超限出口就位，默认仍关）** | `fn zela_playback::vector3d::lights` 不再写字面量，改读 `shadows()`；显式开启 = native `ZELA_SHADOWS=1`、浏览器 `?shadows=1`，**默认关**（PB §1.3：不要一步到位改默认）。能力侧 `const zela_render_vector::shadow::SIZE = 2048`、`MOST = 4`，3×3 PCF、法线偏移 1.5 texel、半径按 2^(1/4) 取整 + 中心对齐整纹素。⚠️ 成本描述（**PC 第二次修正**）：原文"67 MiB/**帧**"不成立（PA 已记），"一次性 ≈67 MiB"也不准确。实际是**按投射光数量**：每盏 **≈16.7 MiB**（2048² × 4 B，`shadow::SIZE`），`MOST = 4` 时上限才是 67 MiB；`shadow_maps` 只在 `layers > 0` 且容量不足时按 `layers` 扩容，且随 `Player` 常驻 ⇒ **一次性、非每帧、多数作品 1–2 盏（增量 17–34 MiB）**。PB 实测开 1 盏：51.5 − 33.5 = 18 MB。超限不再无提示：`shadow::fit` 返回 `requested`，`ShadowTotals`（events / requested / cast）经 `ZELA_FRAME_REPORT.vector_shadows` 与浏览器 `[vector-shadows]` 输出。默认是否切换留待"实测通过 + 重录"评估 |
| V-02 | **bloom 三参只接一个**：`Bloom { threshold, intensity, radius }` 里只有 `intensity` 被透传，threshold / radius 无提示丢弃 | 渲染 / 后处理 | **已关闭（PB Step 2：三参全部接通）** | `View3` 新增 `bloom_threshold` / `bloom_radius`，编码进视图的 `occlusion.z/.w`（着色器只读 `.x/.y`，这是仅剩的空槽）；`bloom.wgsl` 的 `spread` 先按最大通道切门槛（施加到整色、保色相），`bloom.rs` 用半径决定金字塔级数（最粗一级纹素 ≈ radius 像素；默认 4.0 在 640×480 上仍是 6 级 ⇒ 半径不改变现状）。GPU 用例 `bloom_threshold_stops_low_light_from_spreading`：门槛 10 与关掉 bloom 同亮。默认 `Bloom::default().threshold = 1.0` ⇒ 只有真正高光发光，3D `[vector]` 基线因此重录 |
| V-03 | **其余失效契约字段**（归零） | 渲染 / 契约 | **已关闭（PG-07 划掉 `env`；PH Step 4 删 `supersample` / `filter`、`msaa` 归类为能力声明）** | 原 4 项：`env` / `msaa` / `supersample` / `filter`。① `env` 已由 PG Step 3 接通（`RenderParams.env` + `ZELA_IBL`，槽位编码修复）；② `supersample` / `filter` 经复核**零消费者** ⇒ PH Step 4 **删除字段**（破坏性变更，见 PH-05）；③ `msaa` **不是失效项**：它由 `is_supported_by` / `RenderTargetDesc::with_sample_count` 消费，是**能力声明**而非逐帧命令（实际采样数由宿主选，`frame_loop` = 4 / 预览 = 1）。定义见 `ze_render_ir::params::RenderParams` |
| V-04 | **wgpu 冻结定位**（B19 落账，PA 界定范围）：**冻结 = 渲染能力扩张；兼容必需改动允许** | 架构 / 后端关系 | **记账（B19 Step 0；PA Step 2 界定）** | 三层：① **冻结**：后处理栈 / IBL / AO / 阴影等**渲染能力扩张**一律不补——wgpu 的角色是"抽象的第二消费者"，义务是证明同一套抽象能容纳异构后端，而不是追功能；由此 X5-02/03/04/05 的参照物被固定，不再驱动矢量收敛（Step 1 记录）。② **允许**：正确性修复、Rust / wgpu 版本与契约变更的被动跟随，以及**兼容必需的接线改动**（surface 格式、深度格式、MSAA 能力查询、Cargo feature 如 `webgl`）；**冻结 ≠ 放任编译不过**。③ **仍然禁止**：任何让"抽象被迫向 wgpu 看齐"的语义让步。B16 ①（让 wgpu 走 WebGL2）落在此条**允许**范围内，与 V-06 不冲突：V-06 是"不做**矢量**的 WebGL 降级"（解析 AA 依赖 `atomic` storage buffer，WebGL2 无法表达），而 wgpu 后端不依赖 compute / storage（已核实：0 处 storage texture、0 处 storage buffer，仅 `gpu_timing.rs` 一个空 compute pass 且注释写明"不做任何工作"）。B16 ②（换默认后端）已由 B19 Step 3 吸收完成 ⇒ **B16 ① 状态 = 保留待做（依赖本条界定，本批不实现）** |
| V-05 | **E1/E2 结论**（辩了三轮，结论从未落账）：矢量为主 + wgpu 冻结兼容 ⇒ 融合/分立之争消解 | 架构 / 路由 | **已关闭（PH Step 1/2 后归零）** | 原 5 件作品矢量被拒；PC 的**叠加**接住 `surface_matrix` 与其 `_js`，PG Step 2 的**采样**接住 `atlas`（材质采样的内容层当相机视图先渲）⇒ 本批之前为 **2 件**（demo3d Lines / capability_gaps Morph）走冻结的 wgpu。PH Step 1 映射 Morph、Step 2 把 LineList 转细条三角形 ⇒ 两件均进矢量，**被拒作品归零**；两条新基线已随 PH 批次重录（`69b5a888…` / `ff3ad9a2…`，见 PH-02 / PH-03） |
| V-06 | **不做兼容渲染器**（WebGL2） | 架构 / 路线 | **记账（B19 Step 0）** | 解析 AA 依赖**每像素链表**（`atomic` storage buffer），WebGL2 无法表达 ⇒ "矢量画质 + 全兼容"这个组合**不存在**；compute 是 WebGPU 核心功能，去掉它也换不来兼容性。故 B16 的 WebGL 接线不以"兼容"为目标（见 §7 本批不做） |
| V-07 | **产品化三问**冻结为"成人礼"，非路线图项 | 路线 | **记账（B19 Step 0）** | 24/7 功耗、陌生人脚本安全、画面值不值钱——三者都要真实用户与真实负载才能回答，当前数据不足，不作为待办 |
| V-08 | **分层判据**（结构性 vs 参数性）只停在上一轮讨论，未落成文字与工具行为 | 验收 / 对拍 | **已关闭（B19 Step 1）** | 判据：**结构性**（内容缺失 / 系统性错误）必修；**参数性**（实现选择不同、两边都对）记录不修。落地为 X5-02/03/04/05 的"已记录·参数性"终态 + `zela-parity` 输出的 `kind` 列（见 R 段与 §3.3） |
| V-09 | **B18 批次二完全未记账**：重播黑屏与切换作品重复初始化 WebGPU 的修复在 GAPS 里 grep 无引用 | 文档 / 记账 | **已关闭（B19 Step 0 补记）** | 已修已验（提交 `6dfdebc`）。补记两条**可复用病因**：① "只比有没有、不比是哪一个"——`Targets` 缓存只比较是否存在外部目标，不比较是哪一个，重播换了 surface 仍复用旧目标 ⇒ 黑屏；修法是全局目标代数 `TARGET_GENERATION` + `zela_render_vector::forget_player_targets`。② "资源看起来与目标无关，但绑定里藏着目标"——`Bloomed` 的最后一组 bind group 在创建时绑死了当时的 `canvas.color`，随目标一起搬运就会指向旧纹理；修法是只搬 `raster` / `occluded`，bloom 重建。③ 网页侧删掉 `reloadWith()`，切换作品走同一个 device |
| V-10 | 三处描述已过时 | 文档 | **已关闭（B19 Step 0/4 改写）** | ① T 段 `X7-01` 未含"目标代数"（现补进 V-09 并回指）；② U 段 `X8-03` 的触发条件已失效——`?msaa=1` 实测条纹不变 + 移动端 `max_samples` 本来就是 4 ⇒ "1x 锯齿 vs 4x 条纹"的取舍**不存在**（X8-06 / X8-07 已证）；③ `ARCHITECTURE.md` 的 B18 节仍写"等复测"，Step 4 改为"已复测通过" |
| V-11 | 统计表口径注仍写 **D–R**，而实际已有 S / T / U 三段 | 文档 | **已关闭（B19 Step 0）** | 口径注扩到 D–V，并注明 `check_gaps_subtotals` 的实际覆盖段（K…V） |
| V-12 | 页面状态栏「矢量资源 tex 38 → 51」是**累计创建数**不是当前占用（重播 +13 是稳定的一次性开销，不是泄漏），但极易被读成泄漏 | 文档 / 观测 | **已关闭（B19 Step 0 改文案）** | `fn zela_web::vector_resource_note` 的文案改为"累计创建"，与 `zela_render_vector::resource_totals` 的文档口径一致；数字本身不动（它仍是 churn 的判据） |
| V-13 | **计划自身的失误**：B18 计划把 H1（MSAA 能力判据）列为"强候选"并给了三处吻合（只在移动端 / 只在 3D / 规则横向），三处都成立却**没有一处是因果**——X8-06 的源码证据在第一环就打断链条。B19 计划又犯了同型的一处：§5.4 断言"换默认不需要重录任何基线"，只看了 `HASHES_WORKS` 的显式对表，**漏了 smoke**（它调 `zela-render` 时不传 `--backend` ⇒ 吃默认值） | 流程 / 计划 | **记账（B19 Step 0）** | 对比：计划的形态是"强候选 + 三处吻合"，执行的形态是"判别实验 + 逐层收敛"；前者照此改一遍还得重录基线，后者每步都能证伪。smoke 的处置见 ARCHITECTURE B19（显式钉 `--backend wgpu`，基线 2847735c 逐位不变）。**规则**：① 计划中的根因候选**必须**配一个能证伪它的判别开关，否则只能写"待验证"；② 声称"不需要重录基线"时，必须列出**所有**吃默认值的入口，不只是显式对表 |

**V 段小计（B19）**：已关闭 8（V-02、V-03、V-05、V-08、V-09、V-10、V-11、V-12）· 部分关闭 1（V-01）· 记账 4（V-04、V-06、V-07、V-13）。

---

## W. PA 批次记账（记账收尾与机制化，2026-10-24）

> 计划《PA执行计划_记账收尾与机制化》。一句话：V 段刚写下"没进这里的东西，下一批会重新被
> 讨论一遍"，转头就漏了 Render Attachment 三层 —— **所以本段一半在补账，一半在给"记账"
> 这个动作本身补机制**。编号用 `PA-`（`W-xx` 已被 B 段占用，见 PA-05）。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| PA-01 | **Render Attachment 三层**（辩了两轮却零记账）：L1 画面语义进产物；L2 宿主能力不进；L3 执行旋钮**绝不进** | 渲染 / 契约分层 | **记账（PA Step 0；本批不实现）** | 分层与判定问句：L1 = tone / bloom 三参 / ao / env / 阴影 / dash / gradient / 材质（改这个值**画面变不变**）⇒ ✅ 作品的一部分；L2 = 现 `RenderParams` + 能力协商（变不变，但决定**能不能跑**）⇒ ❌；L3 = tile / atlas / bloom 级数 / AO 采样 / msaa / 阴影贴图尺寸（不变，只影响**快慢与内存**）⇒ ❌ **绝不**。**L3 绝不进产物**的硬理由：`tile` / `atlas` 进 payload ⇒ "低端设备降规格"会让同一作品产出不同 payload，直接破坏 W1-01（产物确定性）与 25 条基线的逐位可比性。挂载点 `Camera3D`（3D 视口）/ `Surface`（2D 视口）正好对应两条路径的分界，顺带解决 X1-07（2D 入口把着色栈整个丢了）暴露的 2D/3D 口径分裂。冲突点：`params.rs` 现写明"不进 Pod 边界"，挂组件 ⇒ 进 ECS ⇒ 进 payload ⇒ **建议新增 `RenderAttachment` 类型，而不是改 `RenderParams` 的约定**。实现要等内容表达力的实测数据（先有数据再定结构） |
| PA-02 | **锚点扫描只读 GAPS.md**：`crates/...` 路径校验的作用域只有一份文件，ARCHITECTURE / README / 历史报告全在盲区 | 文档 / 门禁 | **已关闭（PA Step 4）** | 实测连接路径：GAPS 16（B19 已修，0 死链）、ARCHITECTURE 4（1 条含省略号，按构造不可解析）、README / CONTRIBUTING 0、WASM_PORT_SURVEY 12（1 条已漂移）。处置：扫描范围扩到**仓库根所有 `.md`**；`WASM_PORT_SURVEY.md` **显式排除**（历史探测快照，路径漂移是它本来就该保留的信息）**并在文件头加可见声明**；含 `...` 的路径不参与定位、但**打印提示**。排除必须**响亮**——无提示排除 = 又造一个"看起来全绿"的门禁 |
| PA-03 | **vConsole 已引入但未记账**（提交 `1b447b6`） | 观测 / 移动端 | **已关闭（PA Step 0 补记）** | `crates/web/zela-web/www/vendor/vconsole.min.js` 本地内置：移动端 UA 默认开、`?vconsole=1/0` 覆盖。两条决策理由：① 移动端没有桌面控制台，页面状态栏只够放一行；② **不装 network 插件**——它包装 `window.fetch`，wasm-bindgen 的 `instantiateStreaming` 会拿到不可用的 Response（实测报 `compile ... must be a Response`）⇒ "观测工具反过来破坏被观测对象"的典型 |
| PA-04 | **记账没有机制保障**（四次漏账）：① 三条默认值兜底（阴影 / bloom / env）到 B19 Step 0 才 grep 到；② B18 批次二（重播黑屏 + 重复初始化 WebGPU）由 B19 补记（V-09）；③ 锚点扫描作用域提了三次才修；④ Render Attachment 三层辩了两轮仍无引用 | 流程 / 记账 | **部分关闭（PA Step 4：2 项自动化 + 1 项纪律）** | 能自动化的两项已落地：**A1** 锚点扫描扩到根目录所有 `.md`（WASM 显式排除且文件头声明）；**A2** 条目结构校验（K..W 段条目必须满 5 列且证据列非空，缺则报错带行号）。**不能自动化的那项要诚实写出来**："本轮讨论过的专有名词都进 GAPS 了吗"只能是人工纪律 ⇒ 写进 `CONTRIBUTING.md` 的收尾检查表（4 项）并承认它靠人。同时遵守 V-13 ②：新增段必须**双登记**（`check_gaps_subtotals` 的 prefix + `is_item` 白名单） |
| PA-05 | **编号命名空间碰撞**：B 段（WASM 移植探测）已占用 `W-01..W-09`，新段若用 `W-xx` 会与它撞 | 文档 / 编号 | **已关闭（PA Step 0）** | 段字母 `## W.` 可用，条目编号改用 **`PA-01..PA-05`**（唯一、可 grep、不撞既有前缀）。`check_gaps_subtotals` 的 `prefix` 列表与 `is_item` 白名单**必须同时登记**——只加段不加白名单 ⇒ 条目不参与小计重算，又是一处无提示的缺口（与 PA-04 同型） |

**W 段小计（PA）**：已关闭 3（PA-02、PA-03、PA-05）· 部分关闭 1（PA-04）· 记账 1（PA-01）。

> **V-01 的"67 MiB/帧"已由 PC-07 第二次修正**（不再是"每帧"，也不是笼统的"一次性"）：
> 每盏投射光 ≈16.7 MiB、`MOST = 4` 时上限 67 MiB，按 `layers` 扩容且随 `Player` 常驻。
> 这段旧注记保留只为解释当时的判断依据，**成本口径以 PC-07 为准**。

---

## X. PB 批次记账（矢量参数开启与内容纹理层，2026-10-24）

> 计划《PB执行计划_矢量参数开启与内容纹理层》。一句话：把矢量契约里两个**被丢弃且无提示**的参数真正接上
> （阴影开关、bloom 三参），给阴影超限加出口，并把 V2-11 的差异量出来记录；
> **内容纹理层（PB-B）按计划 §4.4 的退路剥离到 PC 批** —— 原因与已完成的部分见 PB-04。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| PB-01 | **路由粒度过宽**（整帧 vs 图层级）：`routing::reject_reason` 见 `has_content_texture` 就拒绝整个矢量后端，而 `content_backend_2d` 的注释本身证明"同一帧不同图层可以用不同后端实例" | 路由 / 能力判定 | **记账（PB Step 5：只修错引，不改实现）** | 原拒绝理由写「GAPS V2-12/V2-13」，但 V2-12 是 shapes 契约签名、V2-13 是渐变 / 纹理 / dash 的宿主入口 —— **指向了存在但不相关的东西**（与 B10 的「`params.z` 是半宽，已核对 wgsl:807」同型，读者会以为已核对）。已改为指向本条。代价：3 件作品（`surface_matrix` / `_js` / `atlas`）因此只能走冻结的 wgpu 上。正确粒度是图层级：内容层继续用 wgpu 渲、视口层走矢量 |
| PB-02 | V2-11（miter limit 4 vs 10）的评估结论 | 渲染 / 2D 描边 | **已记录·参数性（PB Step 3）** | 见 K 段 V2-11：1° 与 20° 两个尖角的覆盖率差 1.4% / 2.7%，四象限"最远亮像素"完全相同或差 0.3px ⇒ 无结构性差异，不改 wgsl 语义。测量用例 `miter_limit_difference_on_a_needle_angle`（`#[ignore]`，本机手工跑） |
| PB-03 | **阴影光源上限（>4）的行为**：`shadow::fit` 只取前 `MOST` 个投射光源，第 5 个起**无提示丢弃** | 渲染 / 阴影 | **已关闭（PB Step 1：显式计数出口）** | `fit` 现在返回 `(maps, requested)`；请求数超过 `MOST` 时 `ShadowTotals { events, requested, cast }` 记录最坏一帧，经 `ZELA_FRAME_REPORT.vector_shadows` / 浏览器 `[vector-shadows]` / `zela-verify` 输出。单测：`more_lights_than_most_are_counted`（6 盏 ⇒ requested = 6、maps = 4）与 `shadow_totals_keep_the_worst_drop` |
| PB-04 | **ContentTexture 图层在矢量后端仍不可用**：公开面没有多视图 / 内容纹理采样入口 | 渲染 / 图层合成 | **记账（PB Step 4 按 §4.4 剥离到 PC 批）** | 已核实：内部模型**是**同构的（`Player::views` 的 `CameraView` 先渲进画布、`image_groups` 按 key 可采样、`Record.texture` + `TEXTURED` 标志都在），但**全是 `pub(crate)`**；公开的 `render_2d*` / `render_3d*` 只接单视图 + `PathRecord::{Fill,Stroke,Mesh}`。缺的是三处**新公开面**：① 多视图入口（content key → canvas）；② 带纹理的路径 / 网格记录；③ 屏幕空间合成（`LayerBlend` + contain fit）的矢量等价物 —— `Blit` 只做全屏 present，不带图层变换与混合（计划 §4.2① 已点明"这一层没有现成"）。⇒ 按 §4.4 剥离，PB-A 照常落地；剥离代价 = 基线重录分两次（可接受） |

**X 段小计（PB）**：已关闭 1（PB-03）· 已记录 1（PB-02）· 记账 2（PB-01、PB-04）。

---

## Y. PC 批次记账（内容纹理层与记账残余，2026-10-24）

> 计划《PC执行计划_内容纹理层与记账残余》。一句话：**内容纹理层走"图层级"而不是"整帧"** ——
> 内容层自己渲成纹理、再叠到视口上；只有被 3D 材质采样的层仍属路线 B。附带三处记账 / 注释残余。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| PC-01 | **路线选择**：A（图层级叠加）vs B（矢量原生多视图 / 纹理采样）；本批选 A | 渲染 / 图层合成 | **已关闭（PC Step 1：A 落地）** | A = 内容层与视口层各自渲染、再按 contain + Alpha 叠加；B = 三处新公开面（多视图 / 带纹理记录 / 屏幕合成），见 PB-04。A 复用了矢量既有的 2D 能力与 B15 的纹理路径，**不需要** `StandardBackends` 参与；B 继续记账。实现：`VectorFrame::composite_content`（含 `bilinear` / `composite_layer`）；零拷贝的 `render_to_texture` 对含内容层的帧**显式报错**，而不是只画视口层 |
| PC-02 | **矢量路径不经过合成器**：`run_frames` 的矢量分支直接拿 RGBA8（`snap.render`），`Compositor` 只服务 egui / wgpu 路径 | 渲染 / 架构 | **记账（PC Step 0 核实）** | 所以路线 A 的叠加要么走 GPU 合成器（需把视口纹理也搬到 GPU），要么在 CPU 像素上做。本批选 CPU：确定性好、无需跨后端句柄；代价是这两件作品在浏览器上要付一次回读（web 显示路径对含内容层的帧改走 `render_async` + 上传，其余帧仍零拷贝） |
| PC-03 | **三个未验证点的实测结论**（V-1 / V-2 / V-3） | 渲染 / 路由 | **已关闭（PC Step 0）** | V-1 **成立**：`ContentTextureSpecs` + `ContentTextureQueue` 在 `assemble_runtime` 里**与后端无关**地注入，矢量快照能取到；V-2 **不需要**：内容层用矢量自己的 2D 路径渲染，`StandardBackends` 不进矢量分支；V-3 **成立**：`LayerKind::ContentTexture { size_px, composite }` 给尺寸与合成开关，`ContentTextureQueue[LayerId]` 给该层 draw。⚠️ 新发现：`atlas` 的内容层是 `composite = false`（被 `Material3D.texture` 采样）⇒ 路线 A **做不到**，它继续按"材质采样"被拒并回退 wgpu；所以新增基线是 **2 条**，不是计划写的 3 条 |
| PC-04 | **[vector] 标签含义变化**：新增的两条是**混合后端** | 基线 / 文档 | **已关闭（PC Step 2）** | `baselines.txt` 的 `[vector]` 段头已写明两种含义（纯矢量 11 条 / 混合 2 条：`surface_matrix`、`surface_matrix.js`），并注明 `atlas` 没有 `[vector]` 条目的原因。决策日志带「（图层级：内容层由矢量渲染后叠加）」，不用猜 |
| PC-05 | **`shadow_maps` 只扩不缩 + `Player` 缓存键不含 shadows 状态** | 渲染 / 资源生命周期 | **记账（PC Step 1；当前无触发路径）** | `shadow_maps` 只有"容量不足就按 `layers` 扩容"分支；`players: HashMap<(w, h, samples), Player>` 不含 shadows 状态。当前不触发（开关是加载期参数），但 `set_shadows` 是 `pub` —— **若将来运行期切换，缓存键必须带上该状态**，否则就是 B18「只比有没有、不比是哪一个」的同型 |
| PC-06 | **wgsl 注释滞后**：`occlusion` 的 `.z/.w` 已承载 bloom 参数，注释仍只说 AO | 文档 / 着色器 | **已关闭（PC-A）** | `blend.wgsl` 与 `vector.wgsl` 的 `Frame` 结构体注释改为「x/y: AO；z/w: bloom threshold / radius」，并指向 `api.rs` 的 `view_3d` 打包说明（不在注释里重述数值） |
| PC-07 | **V-01 成本描述第二次修正** | 文档 / 阴影 | **已关闭（PC-A）** | 每盏投射光 ≈16.7 MiB，`MOST = 4` 上限 67 MiB，按 `layers` 扩容且随 `Player` 常驻（一次性、非每帧）；多数作品 1–2 盏。PB 实测 1 盏：51.5 − 33.5 = 18 MB。见 V-01 条目 |
| PC-08 | **浏览器实测阴影**（PB 唯一未完成项） | 验证 / 浏览器 | **已关闭（PC Step 3）** | 独立 Edge 156 + CDP（无头起不来，改有头；X6-03 禁内嵌浏览器）。`pbr_lights.zsb`：`?shadows=1` 与 `=0` 都 **510/510 帧**走矢量；截图差 **3148 px**（阈值 16/255），bbox 落在物体上 ⇒ 阴影可见；资源 `?shadows=1` 恒为 **39 tex / 30 buf / 52.0 MB**，`=0` 为 **38 / 30 / 35.3 MB** ⇒ 差值正好 1 张 2048² 阴影贴图（16.7 MB），同一配置多次运行数字不变（无 churn）。判据与 native（5390 px 差）同向 |

**Y 段小计（PC）**：已关闭 6（PC-01、PC-03、PC-04、PC-06、PC-07、PC-08）· 记账 2（PC-02、PC-05）。

---

## Z. PD 批次记账（引擎能力普查与 API 暴露，2026-10-25）

> 计划《PD执行计划_引擎能力普查与API暴露》。一句话：**先普查，再暴露；暴露即验证。**
> 自底向上以 `Record` / `Upload` / wgsl 为基准 —— 两处上轮的判断（"IR 没有图元"、
> "多 stop 会绑架内核"）都被源码推翻，这恰好证明普查必须先于设计。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| PD-01 | **覆盖范围从未被声明**：27 条基线与 `zela-parity` 只覆盖公开出来的子集 | 文档 / 验证 | **已关闭（PD Step 1）** | 普查对照表落进 `ARCHITECTURE.md` 的 PD 节：后端能力 / 真实语义 / 公开面 / 场景层四列，未覆盖项逐条列明。已核实的反例：`Shape2D` 10 个变体**全有且全在用**（缺的是场景对象）；`paint.wgsl` 的 `stops(first, second, count, m, t)` **原生支持多 stop** |
| PD-02 | **已实现但未暴露的参数**（标量 / 双 paint / dash / gradient / 逐顶点色） | 渲染 / 公开面 | **已关闭（PD Step 2 + 3）** | 新增 `PathOptions { background, background_width, second, dash, curve_window, gradient }` 与 `MeshRecord::{colors, second}`；每个字段都指向 `Record` 的既有槽位（`apply_options`）。逐参 GPU 用例：`background_stroke_changes_pixels_outside_the_fill`、`second_paint_replaces_the_fill_at_mix_one`、`dash_breaks_a_stroke`、`curve_window_shows_only_the_first_curve`、`gradient_stops_vary_along_the_axis`（含 2 vs 3 stop 中点差异）、`per_vertex_colors_replace_the_mesh_fill`；错误路径 `option_errors_are_explicit`（色标 < 2 / period = 0 / 窗口反向 / 顶点色数不符） |
| PD-03 | **剥离到 PE 的能力**：纹理 / morph / IBL / 场景图元 / 点云逐点色 | 渲染 / 公开面 | **记账（PD §8 退路）** | 各自的具体阻塞：**纹理** = 需要先设计"纹理源"（图像 or 内容层 key）+ `TEXTURED` 标志，与 PB-04 的路线 B 是同一件事；**morph** = `clip = C1·S1 + C2·S2`，第二项是**差值形状**且权重烤进 `m2`（`dash[3]` 只管 paint2），"传目标几何 + mix"的 API 形状与后端语义不符，需要先定形状再暴露；**IBL** = **零资产**：内置影棚 `STUDIO` 是程序化生成的（`environment.rs:216 studio()`，1024×512 + 4 个柔光箱，`render.rs:3581` 惰性 `or_insert_with`），缺的只是公开面出口 + 场景层接线（PG 已改此条，原文"需要真实 radiance 资产"不成立）；**场景图元** = `Ellipse`/`Polygon`/`Polyline` 是场景对象，改的是组件注册表与 `zela.scene` 指纹，不是 Record 暴露；**点云逐点色** = 公开面还没有 Points 记录类型，逐点色无处可挂。**每项都必须先写出"改变输出"的用例才许进公开面（P3）** |
| PD-04 | **单位与坐标口径的陷阱**：`dash` 的 period 是**曲线参数 u**（1 = 一条曲线），不是像素；渐变的轴端点在**场景空间**、不吃 `model` | 公开面 / 语义 | **已关闭（PD Step 2：写进类型文档 + 用例锁定）** | `Dash` 文档写明 u 单位；`Gradient` 文档写明轴端点要传"模型变换之后"的位置（上游 `paint()` 直接投影 `gradient_a/b`，不乘记录矩阵）；`curve_window` 的 `[lo, hi]` 也是曲线索引 + t（`vector.wgsl` 的 `piece()` 先减曲线号再 clamp）。三条都各有用例或显式报错，不靠口头约定 |
| PD-05 | **指纹一次付清**：`Style2D` / `Shape2D` / 场景组件本批是否需要动 | 契约 / 版本 | **已关闭（PD Step 2）** | 都没有动：新参数挂在矢量 crate 的公开结构（`PathOptions` / `MeshRecord`）上，`Record` 结构体尺寸与布局不变（336 字节断言仍在）。⇒ `zela.scene` 指纹**不变**，27 条基线 DIFF 0。`Record` 只把 10 个字段的可见性放宽到 `pub(crate)`（同 crate 内），不产生行为差异 |
| PD-06 | **记账残余**：`MeshRecord` 文档仍写"本批不带纹理"、V-03 仍列 6 个失效字段 | 文档 | **已关闭（PD Step 4）** | `MeshRecord` 的"不带纹理"仍然成立（PD-03）；V-03 已在 PB 划掉 bloom 两项、现剩 4 项（`env` / `msaa` / `supersample` / `filter`），本轮复核无变化 |

**Z 段小计（PD）**：已关闭 5（PD-01、PD-02、PD-04、PD-05、PD-06）· 记账 1（PD-03）。

---

## AA. PE 批次记账（能力入口与展示作品，2026-10-25）

> 计划《PE执行计划_能力入口与展示作品》。一句话：**接一个、证明一个**。
> PD 的验收不得不写"本批没有改变任何一件作品的画面"—— 那句 caveat 就是
> "先暴露、再建作品"这个顺序的失败产物；本批按"当轮闭环"纠正。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| PE-01 | **类型住在错误的层 ⇒ 入口不存在**：`Gradient` / `Dash` / `SecondPaint` / `PathOptions` 定义在 `zela-render-vector`，而场景层不依赖它 | 分层 / 公开面 | **已关闭（Step 1）** | 下沉到 `ze-render-ir::paint`（矢量 crate → IR 是单向依赖）；矢量 crate 改为 `pub use`，公开面名字不变；`PathOptions` 带 serde（组件持久化需要）。分层判据仍成立：IR 只依赖 `ze-notion` / `ze-vertex` / `ze-pod-core` |
| PE-02 | **wgpu 无提示忽略新样式**（V-01 / V-02 的同型，只是发生在 wgpu 侧） | 路由 / 可观测 | **已关闭（Step 3）** | `FrameContent.vector_only_styles` 位掩码由帧快照统计；选到 wgpu（显式或回退）时决策理由追加「wgpu 忽略本帧仅矢量支持的样式：…」。用例：`wgpu_must_say_which_vector_only_styles_it_ignores`（正向指出 + 无特性不误报 + 选矢量不出现"忽略"）、`fallback_to_wgpu_also_reports_ignored_styles` |
| PE-03 | **`Style2D` 不再 `Copy`**（渐变 stops 是 `Vec`）：所有 `*style` / `..draw.style` / 循环内复用都会编译失败 | 渲染 IR / 后端 | **已关闭（Step 1–2）** | 逐点改成显式 `clone()`；`wgpu_2d::snapshot_queue` / `push` / `tessellate_pass` / 测试夹具、`zela-playback::vector_frame` 都已修。类型文档写明"这是能力进样式的必然后果，不是疏忽" |
| PE-04 | **能力面付一次账**：`zela.scene` 新增 5 个构造函数 | 契约 / 版本 | **已关闭（Step 2）** | `@9 → @10`；指纹 `10_059_647_094_731_395_853 → 83_729_276_200_230_347`（`api.rs` 里版本与指纹成对更新，D14 用例红/绿即判据）；`cargo verify --step coverage` 从 `49/49` 变为 `54/54`（5 个新入口都被 manifest 作品用到，豁免 0） |
| PE-05 | **`cap_colors` 无法作品化**：脚本层没有"带逐顶点色的网格"构造入口（只有 `Cube` / `Circle` / `PathShape` 等） | 验证 / 作品 | **记账（§5 ③ 退路，顺延 PF）** | 顶点色能力本身**已有**证据（PD 的 `per_vertex_colors_replace_the_mesh_fill` + `option_errors_are_explicit`）；缺的只是作品载体 —— 要给脚本加网格构造入口，属"场景图元 / 纹理"那条线（PD-03 / PF），不在本批硬塞 |
| PE-06 | **新样式组件不进产物指纹**：`object_fingerprint` 是逐条枚举已知组件的硬编码清单（X1-11 同型） | 对拍 / 指纹 | **已关闭（Step 4）** | 5 个组件都加入指纹（渐变按色标逐项量化 + 两个端点）；`script_storyboard_matches_hardcoded` / Python 版继续逐对象比较。⚠️ 诚实的边界：这两条用例的事故面是**分镜**；新作品的"脚本 vs bundle"由 `--record` 的门禁（逐作品 script == bundle）覆盖，而组件可持久化由 `zela-scene-ecs::tests::persist`（**32 个登记组件**逐条 serde 往返）锁定 |
| PE-07 | **计数写在注释里就会过时**：`baselines.txt` 三处 reason 写「native 16 / vector 11」，实测 14 / 13 | 文档 | **已关闭（Step 0 F3）** | 三处 reason 改为「native 14 与既有 vector 11」；同型的还有 ARCHITECTURE 的「25 条 / 被拒 5 件」（PD 已修）与 api.rs 的「纹理 / 渐变入口仍缺」（本轮改写） |

**AA 段小计（PE）**：已关闭 6（PE-01、PE-02、PE-03、PE-04、PE-06、PE-07）· 记账 1（PE-05）。

---

## AB. PF 批次记账（接缝弥合：透明 / 相机统一 / 2D-3D 混排，2026-10-25）

> 计划《PF执行计划_接缝弥合_透明桶与相机统一与混排》。**Step 0 的 E-2 判伪**（把 alpha
> 140→255 后画面**变暗**而不是变亮）⇒ 原假设"alpha 被当成 coverage、没进混合"不成立；
> 追加判别找到真根因：**`ShaderVariant` 在 3D 桥里被吞掉**（Flat/Lambert/Pbr 输出逐位相同）。
> 本轮按"修正后的 Step 1"执行：先映变体，再谈透明。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| PF-01 | **`ShaderVariant` 未映射**：`vector3d::mesh_record` 无条件写 `material: Some(...)` ⇒ Flat（无光照）也被 PBR 照亮 | 接缝 / 3D 着色 | **已关闭（Step 1）** | 判别：同一立方体只改变体，矢量侧三条哈希**完全相同**（`d2cf72af…` ×3），wgpu 三条不同（`dfc1f7e3…` / `4cc2fb7f…` / `fa90743d…`）。修后：Flat → 无材质（平涂）、Lambert → `[0,1,0.04]`（纯漫反射近似）、Pbr → 透传；三条矢量哈希变为 `9c51bff1…` / `a4293307…` / `d2cf72af…`。单测 `shader_variant_maps_to_material_semantics`。fusion 中心均值 77.7 → 128.6、峰值 137 → 222 |
| PF-02 | **透明合成口径**：wgpu 透明 pipeline 用 `PREMULTIPLIED_ALPHA_BLENDING`，而 `cube.wgsl` 三个变体输出**直色** ⇒ wgpu 侧偏亮；矢量 OIT 的 alpha 合成是对的 | 后端关系 / 合成 | **已关闭（PI-05）** | `wgpu_3d::to_wgpu_blend` 的注释写"cube.wgsl 输出预乘色"，但 `fs_flat` 返回 `albedo`（直色）、`fs_lambert` 返回 `color*lit`（直色）——注释与代码不符。PI 批在 wgsl 侧修（新增 `fs_*_pm` 预乘入口，`LayerBlend::Alpha` 时选用），影响面实测枚举：只有 `fusion` 的 `[native]` 变（`dc61e7cf…` → `e5608dac…`），vector 不变 |
| PF-03 | **2D 相机占位约束从渲染层泄漏到脚本层**：纯 3D 场景必须手写 `Camera2D`，否则装配期报错 | 装配 / 脚本 API | **已关闭（Step 2）** | `SceneAssembly.camera2d: Option<Camera2DHandle>`；无相机时只跳过 2D 录制。**H3**：无相机但**确有 2D 图元** ⇒ 显式错误（判据复用 `record_scene` 的 8 组组合，不另维护清单）。`demo3d.lua` / `pbr_lights.lua` / `fusion.lua` 三处占位已删除；三件 `[native]` 哈希**逐位不变**（`983c0d88…` / `1d1ea6b4…` / `dc61e7cf…`），纯 3D 无相机脚本的矢量哈希与带相机版本相同（`9c51bff1…`） |
| PF-04 | **`double_sided` 在矢量侧不可表达**：后端没有 culling 开关，两面都画 | 接缝 / 3D | **记账** | 不透明网格两面深度遮挡 ⇒ 无差别；**半透明 + `double_sided = false`** 会多出背面。矢量桥已注释写明；要支持需给后端加三角形剔除（新增后端行为，违反本批边界） |
| PF-05 | **overlay 坐标系与 2D 记录不同**：overlay 矩阵是 3D 视图的 frame 单位，而 `scene_records` 给的是屏幕像素偏移 | 接缝 / 2D-3D | **已关闭（Step 2）** | 实测症状：不换算时 2D 内容整体放大到画外（只剩网格）。修法：`overlay_records` 统一乘 `frame_w / width`（几何 / 描边宽度 / 渐变轴端点 / 背景宽度）。E-3 用例 `overlay_path_lands_on_the_screen_plane_in_a_3d_frame`：z=3 的 overlay 记录不随透视放大 |
| PF-06 | **`capability_gaps` 仍不能进矢量**：混排已放行，但它还含 `Morph`（B14 记账、PD-03 顺延） | 作品 / 验证 | **记账（退路 §7）** | 决策日志实测：`首选 Vector 被拒 ⇒ 回退到 Wgpu`，原因「本帧含形变（Morph）项」。⇒ 混排证据改由新作品 `cap_mix`（cube + 2D 圆/箭头，静态 1 帧）承担，新增 1 条 `[vector]` 基线；`capability_gaps` 的矢量化要等 Morph 映射（PG） |

**AB 段小计（PF）**：已关闭 4（PF-01、PF-02、PF-03、PF-05）· 记账 2（PF-04、PF-06）。

---

## AC. PG 批次记账（渲染能力全开放与自封闭，2026-10-25）

> 计划《PG执行计划_渲染能力全开放与自封闭》。一句话：**把后端已写好、没有出口的能力开出出口；
> 把渲染层强加给调用方的约束拆掉**，仍不碰 `ze-scene-*`。
> 判别逻辑按计划 §1.4 硬执行：每个能力先做判别实验，做不到就剥离。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| PG-01 | **占位相机（自封闭反例）**：纯 3D 作品必须手写 `Camera2D` | 装配 / 脚本 | **已关闭（Step 0）** | 删 4 处：`pbr_materials` / `pbr_scene` / `shaded_meshes` / `atlas`。`[native]` 逐位不变（`pbr_materials 834ad967…`、`pbr_scene ff2ae135…`、`atlas e330bbc0…`），`shaded_meshes [vector] 1bfa7b43…` 不变。**第 5 处 `surface_matrix` 经判别实验保留**：删掉后 `[native] a6e4394c…→9a34bf69…`、`[vector] 3e9d4b0e…→e313809d…` 都变 ⇒ 它参与内容层的 2D 落地，不是占位（注释已改写） |
| PG-02 | **PD-03 的 IBL 成本描述过时**："用例需要真实 radiance 资产" | 文档 | **已关闭（Step 0）** | 内置影棚 `environment.rs::studio()` 是**程序化生成**（1024×512 + 4 个柔光箱，`render.rs` 首次引用时惰性造），零资产。PD-03 条目已改写（同 V-01 的"过时成本会让下一个人不敢动"） |
| PG-03 | **双向普查**（A：场景层要、渲染层没有；B：后端有、公开面没有） | 公开面 / 记账 | **已关闭（Step 1）** | 见下方"Step 1 普查表"：A 方向 4 项（`env` 本批补齐；`msaa`/`supersample`/`filter` 仍是失效字段；`double_sided` 记账 PF-04）；B 方向 5 项（多视图 / `Upload::{Points,Texture}` / `STUDIO` 本批补齐；`Grow`、`Environment` 上传仍无公开出口） |
| PG-04 | **2D 内容进 3D（路线 B）不可达**：`views()` 的 cameras 四处传 `Vec::new()`，记录也没有纹理字段 | 公开面 / 3D | **已关闭（Step 2）** | 新增公开面：`CameraView { content_2d / scene_3d }`（额外视图先渲进 canvas，主帧按 `key` 采样）、`MeshRecord.texture ∈ { View(key), Image{..} }`；四个 3D 入口加 `cameras` 参数。判别实验 G-1（`a_camera_view_is_sampled_by_a_mesh`）：相机视图红、网格绿 ⇒ 采样后红；不带宽 ⇒ 绿；引用不存在的 key ⇒ `unknown texture`。`atlas` 因此进矢量（`cddaad13…`，见 PG-05 的图） |
| PG-05 | **`atlas` 的 3D 材质采样与 UV 兜底** | 接缝 / 3D | **已关闭（Step 2）** | 内容层当相机视图（尺寸 = 图层 texel，2D 视图口径与 `render_content` 完全一致）；网格无 UV 时用 `zela_render::vertex_record::fallback_uvs`（**与 wgpu 同一个函数**，Planar 的 `1 − y` 不会反转）。实测帧：平面正立、公式可读；`[native]` 不变、新增 `[vector]` 基线 |
| PG-06 | **纹理的合成语义**：上游是"画面即表面"（`texel.rgb / texel.a` 取代材质色），wgpu 是"纹理作为 decal 叠在受光材质色上" | 后端行为 / 语义 | **记账（已按 wgpu 语义改，但只覆盖材质路径）** | 透明的内容层（Surface 的文字画布）在旧语义下让整块表面消失、只剩墨迹（实测第一版：平面全黑）。`blend.wgsl::lit_light` 改为 `texel.rgb + color.rgb·(1 − texel.a)`，口径与 `cube.wgsl` 相同；**不透明画面（`texel.a = 1`）两项都退化为 `texel.rgb`** ⇒ 无既有行为变化。⚠️ 未改的：无材质的 `unlit` 仍按"画面即表面"（`Upload::Texture` 的将来消费者会看到两套语义）；wgpu 侧墨迹显示为浅蓝（与 PF-02 同源，wgpu 冻结） |
| PG-07 | **IBL 永远不发光**：`Light::Environment` 的槽位只写了 kind 与 id | 渲染 / 3D | **已关闭（Step 3）** | 真因不是"没接场景层"：`light.wgsl` 的 kind-4 分支把整项乘 `color.rgb`（未写 = 0）与 `axis.w`（未写 = 0）⇒ 即使视图里有环境灯，贡献恒为 0。补齐编码（`place`=图 +x、`axis`=图 up、`color.rgb`=1、`color.w`=最粗一级、`axis.w`=单位光强），并把 **id 移到 `cone.x`**（`color.w` 归着色器读作最粗一级，两者不能同槽；`environment_of` / `with_sun` 同步）。接线：`RenderParams.env`（IR 既有字段）+ 宿主开关 `ZELA_IBL=1` / `set_environment`。判据实测（pbr_materials 第 60 帧，受光像素均值）：**47.5 → 93.1** |
| PG-08 | **点云与纹理资产无出口**（PD-03 顺延） | 公开面 | **已关闭（Step 4）** | `PointsRecord`（每点一个圆盘；**半径是每个对象一个** —— 后端把半径放实例槽 `extra.x`，逐点半径需改后端槽位，不做）+ `TextureSource::Image`（内联 RGBA8，与相机视图共用 `image_groups`）。各一条"改变输出"用例：`per_point_colors_change_a_point_cloud`（白点 → 左红右蓝）、`an_inline_texture_changes_the_mesh`（绿网格贴 1×1 红图 → 红；字节数不符显式报错）。`Upload::Points` / `Upload::Texture` 的死代码警告随之消失 |
| PG-09 | **键空间**：相机视图记录、brush rows、内联图像三者共用 `u64` 键 | 后端内部 | **已关闭（Step 2/4）** | 主帧记录 `1..=n`；相机视图记录 `(i+1)<<48`（否则同一 `key` 在两份记录里指向同一条 shape）；rows = `1<<40 + key`；内联图像 = `1<<41 + key`；相机视图键 = 内容层的 `content_texture_handle`（`1<<32 + index`）。冲突会让后上传的渐变 / 图像覆盖前面的 |
| PG-10 | **`has_material_content` 拒绝分支移除** | 路由 | **已关闭（Step 2）** | 现在只有"材质的纹理**没有生产方**"才拒绝（`FrameContent::has_unserved_material_texture`，理由写明"缺纹理上传入口"）；材质采样的层在决策理由里写"**按纹理采样**"（与 PC 的"叠加"区分）。用例：`material_sampled_content_is_served_by_a_camera_view` / `vector_falls_back_to_wgpu_with_a_reason` |
| PG-11 | **`RenderParams` 仍失效的 3 个字段** | 契约 / 后端 | **记账（Step 1 结论）** | `msaa` / `supersample` / `filter`：矢量后端没有对应旋钮（采样数由 adapter + `?msaa=` 排查开关决定；无超采样；采样器固定 linear）。要接就先定语义，不在本批硬做 |
| PG-12 | **demo payload 断言只在 CI 上跑** ⇒ 脚本改动后 push 才红 | 验证 / 门禁 | **已关闭（PG 补丁：新增验证步骤）** | 两次同型事故：PF 的 `demo3d`（删占位相机）、PG 的 `pbr_materials`（同因）。根因：Pages 工作流只跑 `build.ps1`，而 `cargo verify` 的七步里没有任何一步编译 manifest 里的作品 ⇒ 删一个场景对象改了 ZSB，本地全绿、CI 当场红（`payload hash 不符（pbr_materials / linux）：实测 e1688f07… 期望 6eeed7ff…`）。修法：新增 `cargo verify --step demo`（在 `check…coverage` 之后、`smoke` 之前），逐条按 manifest 编译并断言 `payload hash`，字体目录由 `cargo_command` 统一注入；口径与 `build.ps1` 逐字一致（`-` 跳过、`windows:…,linux:…` 只断言本机平台）。连带修正 4 条 manifest 值：`pbr_materials e1688f07…`、`pbr_scene 75bb7816…`、`shaded_meshes 7d7081ba…`、`atlas 1233e169…`（都是删占位相机导致的 ZSB 变化；`pbr_lights` / `surface_matrix` 等未变）。本机实测 22 条断言通过 |

| PG-13 | **内嵌 Python 初始化失败会让整个进程死掉**（`cargo run` 报 `Fatal Python error: Failed to import encodings module`） | 装配 / 环境 | **已关闭** | 真因：`pyo3` 的 `auto-initialize` 在 Windows 上按**本进程 exe** 推算前缀 ⇒ 找不到 `Lib/encodings`；而这是 CPython 的 **Fatal**（`abort()`），Rust 侧捕获不到 ⇒ 只能**前置探测**，不能靠 `Result`。修法：`ze-pod-py/build.rs` 从 pyo3 的 `links` 配置取出**编译期解释器**（`ZELA_PYO3_EXECUTABLE` / `ZELA_PYO3_VERSION`）；`zela-app/src/python_env.rs` 在第一次碰 pyo3 之前用它（备选 `PYO3_PYTHON` → `python`/`python3`/`py -3` → PATH 扫描）跑 `import encodings` 并取 `sys.prefix` 写 `PYTHONHOME`，且**版本必须与编译期一致**（实测 PATH 里 msys64 的 3.14 排在 anaconda 3.13 之前，指错前缀照样 Fatal）；找不到 ⇒ 打印可操作提示并**跳过 Python Pod**（其余 Pod 照常），不再 abort。实测（不设任何 Python 环境变量）：`zela-render demo3d.lua` 装配 `scene -> content -> python -> lua`、渲染 8 帧完成 |

**AC 段小计（PG）**：已关闭 11（PG-01…PG-05、PG-07…PG-10、PG-12、PG-13）· 记账 2（PG-06、PG-11）。

### Step 1 普查表（A 方向：场景层已经要给、渲染层丢掉；B 方向：后端已有、公开面没有）

| 方向 | 项 | 结论 | 理由 / 证据 |
|---|---|---|---|
| A | `RenderParams.env` | **补齐**（PG-07） | IR 字段一直在，矢量桥无引用；补齐后金属亮度翻倍 |
| A | `RenderParams.msaa` / `supersample` / `filter` | **记账**（PG-11） | 后端无对应旋钮，硬接等于编一套语义 |
| A | `Material3D.double_sided` | **沿用 PF-04** | 后端无 culling 开关 |
| A | `Material3D.texture` | **补齐**（PG-04/05） | 内容层 → 相机视图，网格按句柄采样 |
| B | `views()` 多视图 | **补齐**（PG-04） | `CameraView` 公开面 |
| B | `Upload::Points` | **补齐**（PG-08） | `PointsRecord` |
| B | `Upload::Texture` | **补齐**（PG-08） | `TextureSource::Image` |
| B | 内置影棚 `STUDIO` | **补齐**（PG-07） | `Light::Environment{ id: BUILTIN_ENVIRONMENT }` |
| B | `Upload::Grow` / `Upload::Environment`（自定义 .hdr 上传） | **剥离到 PH** | `Grow` 服务曲线续接（morph/形变的同族）；自定义环境要 RGBE 解码入口 + 句柄管理，本批只做"零资产的影棚" |

---

## AD. PH 批次记账（能力缺口归零：Morph / Lines / 自定义环境，2026-10-25）

> 计划《PH执行计划_能力缺口归零_Morph与Lines与自定义环境》。一句话：**把最后两条被拒路由
> 变成映射，并把两个 `pub(crate)` 出口开给宿主**。此批之后矢量后端相对 wgpu 无已知能力缺失，
> 22/22 作品全部可走矢量（基线见 PH-02 / PH-03）。
>
> ⚠️ **破坏性变更**（H5 允许）：PH-05 删除 `RenderParams` 的两个字段。API 尚未稳定，
> 已在条目里显式标注。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| PH-01 | **`baselines.txt` 段头事实错误**：`[vector]` 段头仍写"`atlas` 仍回退 wgpu、没有 `[vector]` 条目"，而条目区已有 `atlas = cddaad13…`（PG Step 2 的头条成果） | 文档 / 基线 | **已关闭（Step 0）** | 段头改为三类口径：**纯矢量 17 · 叠加 2（surface_matrix / _js）· 采样 1（atlas）**。⚠️ 计划 §0.1 写"纯矢量 18 · 叠加 2 · 采样 1"（和 = 21）与条目数 20 不符 ⇒ 按条目重算为 17 / 2 / 1，并在段头写明这次修正。同时把"段级叙述同步"写进 `CONTRIBUTING.md` 收尾检查表第 5 项（第 5 次同型） |
| PH-02 | **Morph 未映射**（路由 `reject_reason` 的 `has_morphs` 分支） | 接缝 / 2D | **已关闭（Step 1）** | 后端语义是"首形状 + **差值形状** + 权重烤进 `m2`"（PD-03）⇒ 接缝层算 `delta = to − from`，权重进 `m2 = mix · I`（`api.rs::attach_morph` / `render.rs::Record::morph` / `scene.rs::morph_records`）。H-1 判别（`morph_mix_changes_the_shape_and_zero_equals_the_start`）：t = 0 **逐位等于**起点、t = 0.5 / 1 各与两端不同（不是"整项乘 0"）；两端曲线数不一致 ⇒ `SceneMapError::UnalignedMorph` 显式拒绝（`unaligned_morph_endpoints_are_rejected`）。缓存键 = `(from 摘要, to 摘要, samples)`（`vector_frame::sampled_endpoints`，含 `samples` 与两端标识，避开 PC-05 的"只比有没有"）。`same_2d_frame` 纳入 `t` ⇒ 动画不会被像素缓存冻住。新增 `[vector] capability_gaps` 基线 |
| PH-03 | **线框未映射**（`MeshForm::Lines` 无引用；`mesh_record` 对非 Tri 显式 Err） | 接缝 / 3D | **已关闭（Step 2）** | 后端**没有**线框路径（H-2a：`MeshForm` 枚举只有 `Tri` / `Line`，矢量 mesh 分支只接受三角形）⇒ 唯一合法解是接缝层把 `LineList` 转**细条三角形**（`vector3d::line_ribbons`：每段 4 顶点 / 2 三角形，法线取"指向眼睛"）。位置输出为世界坐标 + `model = 恒等`，宽度 `1 px`（`view.frame.1 / height`）。H-2b：场景层**没有 `line_width` 字段** ⇒ 宽度不可由作者控制（PH-06 记账）。用例 `line_topology_becomes_a_thin_ribbon`（宽 2 世界单位 ⇒ ±1 展开 + 法线朝相机）。新增 `[vector] demo3d` 基线 |
| PH-04 | **自定义 `.hdr` 环境无出口**：`read_hdr` / `add_environment` / `STUDIO` 全是 `pub(crate)` | 公开面 / 3D | **已关闭（Step 3）** | `zela_render_vector::{read_hdr, add_environment, STUDIO}` 公开；`add_environment(id, w, h, rgbe)` 进进程级表，`Player` 按**代数**重装（同 id 再上传 = 替换；不重算 harmonics）。id 0 保留给内置影棚（显式拒绝）。判据 `an_uploaded_environment_actually_lights_the_scene`：**只**给环境灯，上半天亮 / 下半天黑的自定义图与内置影棚的受光均值必须不同（实测通过）；`environment_upload_errors_are_explicit` 锁定错误路径 |
| PH-05 | **契约字段显式化**（`RenderParams` 里挂了没效果的三项） | 契约 / 后端 | **已关闭（Step 4；破坏性变更）** | 判别后逐项处置：① `supersample` 除 `target_size()` 外**零消费者** ⇒ **删除字段**（连同上限 / 三个错误变体 / `target_size`）；② `filter` 两个后端都不读 `RenderParams.filter`（矢量里的 `.filter` 是迭代器；采样器按纹理各自设置）⇒ **删除字段**；`TextureFilter` 保留（`SurfaceTexture.filter` 是真消费者）；③ `msaa` **保留**：它由 `is_supported_by` 的 `require_sample_count` 与 `target_desc` 消费，是**能力声明**而非逐帧命令（实际采样数由宿主选：`frame_loop` 4 / 预览 1）。⚠️ ①② 是破坏性变更（H5）；默认值变化不影响像素（34 条基线逐位不变） |
| PH-06 | **线框宽度语义**：场景层没有 `line_width`，接缝层只能取 1 px | 场景 / 3D | **记账（Step 2）** | `MeshGeometry` 只有 positions / normals / uvs / indices / topology。矢量侧宽度 = 1 屏幕像素（焦距平面处换算成世界长度），**作者不可控**；wgpu 的 `LineList` 同样是 1 px（不支持 linewidth）⇒ 此参数上两边一致。但矢量细条**能**控宽（计划 §4.3 的"好消息"），将来加 `line_width` 时只需把它接进 `LineStyle`。`demo3d` 的 vector 与 native 因此是**结构性差异**（线宽/抗锯齿不同），不是回归 |
| PH-07 | **能力归零后，"默认回退 wgpu"是否仍划算？** | 架构 / 路由 | **记账（本批只回答方向，不实现）** | 回退会同时丢掉：后处理栈（tone / bloom / AO）· 阴影 · 渐变 / dash / 双 paint 全家；而它换来的只是"能力不足兜底"。PH 之后被拒作品为 0 ⇒ 回退分支只剩"adapter 能力不足"与"纹理没有生产方"两种触发，**默认是否仍要保留 wgpu 回退**在阶段二拍（与"默认打开阴影 / IBL"同一决策面）。⚠️ 默认仍关：`ZELA_SHADOWS` / `ZELA_IBL` 都是关，能力全开但默认画面里看不到 |
| PH-08 | **ARCHITECTURE「明确不可用」两条过时**（"被拒 3 件" / "IBL 无脚本入口"） | 文档 | **已关闭（Step 0/5）** | 改为按 routing.rs 实测口径：被拒曾是 **2 件**（demo3d 线框 / capability_gaps Morph），`atlas` 已由 PG Step 2 的采样接住；IBL 已由 PG Step 3 打开（`ZELA_IBL` + 槽位编码修复）。PH Step 1/2 后两条也进矢量 ⇒ 该节最终写"**0 件被拒**"（见 Step 5 的同步） |
| PH-09 | **FPS（3D 第一人称）相机控制的五处实测缺陷**（PH 交付后由用户报告：一操作就不对 / WASD 像按世界坐标） | 场景 / UI | **已关闭** | ① **取景焦距塌陷**：`apply_fps` 把 `target` 固定成 `eye + forward`（距离 1），而矢量后端取景尺度就是 `focal = |eye − target|`（`vector3d::view3`）⇒ 一拖拽画面缩到 1/d（demo3d 5 → 1）、切回 Orbit 时枢轴也只剩 1 单位。改为**保留命令前的 `|eye − target|`**（`fps_preserves_the_focal_distance`，并修正 `fps_move_follows_current_orientation` 里把旧行为当预期的断言）。② **朝向跨模式残留**：`FpsView` 是会话内累积量，Orbit / Path 改过相机后旧值仍被复用 ⇒ 切回 FPS 第一下操作跳回上次朝向。`CameraDriveSystem::write_mode` 在模式变化时作废 `FpsView`，且 `CameraInputSystem` 只在 FPS 模式回写（`entering_fps_discards_the_previous_heading` / `staying_in_fps_keeps_the_heading`）。③ **移动速度按帧**（`0.08/帧`）⇒ 60/144 Hz 差 2.4 倍，且 WASD 要求指针悬停、文本输入会抢键。改为 `stable_dt × 4.8 单位/秒`、不要求悬停、文本输入时让位（`zela-egui::host`）。④ **脚本的相机绑定没让位**：`CameraDriveSystem` 只记录装配期的自动轨道，而脚本作品（`capability_gaps` 等）用 `Target.CameraEye` 自己驱动相机、`Camera3DHandle` 又是宿主侧才登记 ⇒ 切到 Orbit/Fps 后相机仍被脚本每帧写回（"控制完全没反应"）。改为按目标 id **动态移出所有写这台相机的绑定**、切回时原样放回（`script_camera_binding_is_suspended_and_restored`）。⑤ **WASD 沿世界轴走**：`FpsView` 的 yaw/pitch 固定按世界 Y 解释、移动基也用常量 Y ⇒ 对 `up = +Z` 的场景（`capability_gaps`）表现为"D 往上、W 往侧面"，像按世界坐标算的。`FpsView` 新增 `up`（进入 FPS 时取自 `Camera3D.up`，用确定性正交基 `(a, b, up)`；`up = +Y` 时与旧实现逐位一致），朝向与 (右/上/前) 基都从它构造（`fps_move_uses_the_cameras_up_axis`） |

**AD 段小计（PH）**：已关闭 7（PH-01、PH-02、PH-03、PH-04、PH-05、PH-08、PH-09）· 记账 2（PH-06、PH-07）。

---

## AE. PI 批次记账（渲染层收尾与成本度量，2026-10-25）

> 计划《PI执行计划_渲染层收尾与成本度量》。四件事：**弥合最后一处纹理出口**、**把成本测出来**、
> **按数据拍默认开关**、**收尾两项记账项**（外加一项 wasm CI 判别实验，不直接进 CI）。
> 本段的核心是**数字**：M-1..M-5 全部落在这里，避免下一批在无历史的情况下重测（PJ 的可观测性 L1）。

| 编号 | 缺口 / 变更 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| PI-01 | **过时拒绝理由**：`routing.rs` 对"网格材质纹理不是内容层"仍写"矢量侧缺少该纹理的**上传入口**（PG Step 4）"，而 PG Step 4 正是**开这个入口**的那一步 | 路由 / 文档 | **已关闭** | 前半句（无生产方 ⇒ 显式拒绝）保留；后半句改为「上传入口已开放（PG-08 的 `TextureSource::Image` + `add_texture`），当前缺的是**场景层的非内容层纹理来源**（图片资产概念，属阶段三）」。判据：被拒作品数仍为 0（该分支的触发条件未变，只是把理由说对了） |
| PI-02 | **F1 定性更正**：`TextureSource::Image` 的"零生产消费者"曾被定为阶段一未弥合接缝 | 契约 / 分层 | **已关闭（结论已记录）** | 方向是**底层往上** ⇒ 按方向性原则**充分暴露、无需求方也保留**。消费者需场景层有图片资产概念（`Shape2D` 无 Image 变体、无图片组件）⇒ **阶段三**。接缝层唯一生产者是 `content_texture_handle`（`CONTENT_TEXTURE_ID_BASE`）⇒ 只能映射 `View`，映射 `Image` 在结构上不可达。**记此条是为了防止下一批再以「零消费者」为由讨论删它** |
| PI-03 | **默认开关拍板**（依赖 PI-2） | 渲染 / 决策 | **已关闭** | **两个都开**（用户按 M-3/M-4 数据拍板）：`shadows()` / `environment()` 默认 `true`，`ZELA_SHADOWS=0` / `ZELA_IBL=0` 仍可关（浏览器 `?shadows=0`）。依据：阴影 +16.77 MB **一次性**、每帧无可测变化（M-3）；IBL +0.42 ms/帧（+11.6%）、实际 ≈+35.5 MB，用户确认接受（M-4）。⚠️ 只动 `[vector]` 基线（wgpu 侧 grep `shadow` / `env` 无引用，V-04 冻结） |
| PI-04 | **PC-05 落地**：`shadow_maps` 只扩不缩 + `Player` 缓存键不含 shadows 状态 | 渲染 / 资源生命周期 | **已关闭** | `players` 键改为 `(w, h, samples, shadow_generation)`；`set_shadow_state(on)` 只在状态**真的变化**时推进代数（同值重复调用不换代，避免无谓重建）；`take_player` 先 `retain` 掉同 `(w,h,s)` 的旧代数再取新键 ⇒ 切换不再复用"上一种阴影状态"的 player。同 B18「只比有没有、不比是哪一个」的修法 |
| PI-05 | **PF-02 落地**：wgpu 透明 pipeline 用 `PREMULTIPLIED_ALPHA_BLENDING`，而 `cube.wgsl` 三变体输出**直色** | 后端 / 合成 | **已关闭** | 在 wgsl 侧修（V-04 允许的正确性修复）：`cube.wgsl` 拆出 `color_*` 与 `fs_*_pm`（`premultiply(c) = vec4(c.rgb*c.a, c.a)`），`wgpu_3d::variant_entry_point(variant, blend)` 在 `LayerBlend::Alpha` 时选 `fs_*_pm`。**影响面实测枚举**（不是假设）：只有 `fusion` 变 —— native `dc61e7cf…` → `e5608dac…`，vector `8226d53b…` 不变；其余 13 条 `[native]` 逐位不变 |
| PI-06 | **M-1 长跑**：资源是否随帧数增长 | 验证 / 资源 | **已关闭（有边界）** | `fourier` **12240 帧**、`fusion` **10200 帧**（320×240，`--fps` 放大帧数的压力代理）。两件都 `first == last`，末帧增量 0 ⇒ **无 churn**（B17 判据成立）。fourier 另有 2 个 buffer 的**一次性**增长（首帧 22 → 总 24）。⚠️ 边界：`帧数 = duration × fps` 是压力代理，**回答不了"能不能挂 24 小时"**（没有真实时间老化 / GC / 驱动长期行为） |
| PI-07 | **M-2 内容纹理帧的每帧耗时**（PC 路线 A 引入，F4 从未补） | 验证 / 性能 | **已关闭** | `atlas` **120 帧**（剔除首帧）median **2.556 ms**、p99 **4.575 ms**；`surface_matrix` 只有 **1 帧**（`duration = 0`）⇒ **81.874 ms 是冷启动**，不能当稳态数字。口径：`ZELA_FRAME_TIMING=1` 的 `[timing]`，剔除 sink/hash 项 |
| PI-08 | **M-3 / M-4 开阴影与开 IBL 的增量** | 验证 / 成本 | **已关闭** | **M-3**（`pbr_lights`）：关 GPU 3.645 ms / 39 tex 34.70 MB；开 GPU 3.604 ms / **40 tex 51.48 MB（+16.77 MB）** ⇒ 帧时间无可测变化（与 PC-07 的 16.7 MiB 独立吻合）。**M-4**（`pbr_materials`）：关 3.618 ms / 39 tex 34.70 MB；开 **4.039 ms（+0.42 ms / +11.6%）** / 42 tex，账面 +203.4 MB 但**实际 ≈ +35.5 MB**（见 PI-10） |
| PI-09 | **M-5 浏览器侧回读增量**（PC 08 之后零数据） | 验证 / 浏览器 | **已关闭（有边界）** | Edge `--headless=new` + **真实适配器**（`nvidia/lovelace`，`--enable-unsafe-swiftshader --ignore-gpu-blocklist --enable-gpu-rasterization` ⇒ `navigator.gpu` 可用）、640×480、CDP 读状态行：`atlas`（内容层 + 材质纹理）**40 tex / 36 buf / 50.2 MB，7.22 ms/帧（138 fps）**；`surface_matrix`（内容层叠加）17 tex / 34 buf / 15.1 MB（1 帧）；对照 `pbr_scene`（纯 3D）38 tex / 29 buf / 35.3 MB，20.33 ms/帧（49 fps）、`fourier`（纯 2D）14 tex / 22 buf / 35.5 MB，29.36 ms/帧（34 fps）。⇒ ① **资源量级**与 PC-08（39 tex / 52.0 MB）同量级；② 回读走的是**每帧临时** staging buffer（不进常驻 totals）；③ ⚠️ **边界：跨作品对照被工作量差异淹没**（内容纹理作品反而最快）⇒ 本项只给同方法论量级对照，**没有隔离出"回读增量"这个单一数字** |
| PI-10 | **资源计数过计**：`ResourceTotals::texture_bytes()` 对 mipmapped 纹理按 `size × mip_level_count` 求和 | 可观测 / 资源 | **记账** | M-4 因此记出 +203.4 MB，而同一份资源按"每级实际字节"求和只有 ≈ +35.5 MB（1024×512 影棚 + 预滤 mip 链，约 4/3 而非 6 倍）。⇒ 计数口径是"上限估计"，**不能当显存占用量引用**；修法需要按级尺寸逐级累加（改的是计数函数，不是渲染），留给可观测性批 |
| PI-11 | **G-2 路径级纹理判别**（`FilledPath` / `StrokedPath` 无 texture 字段） | 契约 / 2D | **记账（不开）** | 判别结论：**无效**。① 两个路径记录类型**没有** texture 字段（只有 `MeshRecord.texture`，其文档已写明"纹理只对网格生效；路径记录没有采样器入口"）；② 后端唯一绑定图像的地方是 `Draw::draw` 的 `Kind::Mesh` 分支（`pass.set_bind_group(1, image_groups[..])`），`Kind::Path` 走解析覆盖度合成（`vector.wgsl` 无逐记录图像采样、无 uv）⇒ 即便硬写 `Record.texture` 也**画不出来**。按"路径无 uv 映射 ⇒ 记账不开"处置（与 V-13 ① 同型：先判别，再决定） |
| PI-12 | **W-1 wasm CI 判别实验**：`step_wasm` 全是 `cargo check --target wasm32` ⇒ 现有"wasm 门禁"是编译门禁 | 验证 / CI | **已关闭（维持手动）** | 本地代理实验：Edge `--headless=new` + SwiftShader 组合**能**拿到 adapter（`nvidia/lovelace`；强制 `--use-webgpu-adapter=swiftshader` 反而 `no-adapter`），说明"headless 无 GPU"不是硬障碍。但 **ubuntu-latest 的 Chrome 未实测**（本机无法代表 CI 镜像），且既有政策已覆盖该边界（`architecture.yml`：托管 runner 无 GPU ⇒ 只断言 `--script == --bundle`、不比基线）。⇒ 结论写进 `WASM_PORT_SURVEY.md`：**维持手动浏览器实测**，不新建 CI 链路。边界照记：跨端浮点分歧首次出现在 fourier **第 103 帧**，而 smoke 只渲前 15 帧 ⇒ smoke 绿 ≠ 画面一致 |
| PI-13 | **功耗零数据** | 验证 / 成本 | **记账** | 仓库内无任何功耗支持（无外部工具、无采样点）⇒ 本批如实记为「**无数据**」，不以帧时间代功耗 |

**AE 段小计（PI）**：已关闭 10（PI-01、PI-02、PI-03、PI-04、PI-05、PI-06、PI-07、PI-08、PI-09、PI-12）· 记账 3（PI-10、PI-11、PI-13）。

---

## AF. 表达式层立约（S0，2026-10-26）

> 计划《表达式层执行计划：组合子库、单子注入与 API 导出》。本段只**立约与记账**，不改代码：
> 组合子内核的实现在 S1（[`ze-comb`](crates/kernel/ze-comb/src/lib.rs)）。
> 口径：能力面不再只由「手写方法 + 静态数据」构成；脚本只提交**结构**，
> 时间 / 帧 / 事件 / 组件 / 环境由宿主以单子 `M` 注入。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AF-01 | **确定性第四判据：无隐式可变状态** | 全仓 | **立约（S0）** | 判据与三层结果口径并列（见文件头）。检查项：全局可变静态 / 墙钟 / 随机 / 系统环境 / 进程计数。已关闭的三个实例：W1-01（`MeshGeometry.id` 曾用进程内 `AtomicU64` 全局计数）、X1-11（系统字体不在版本控制）、#93（预记 `random/time`）。三者此前分头修，现统一为同一规则 |
| AF-02 | **拒绝 `fix`、接受 codata** | 表达式层 / 语言模型 | **立约（S0）** | 组合子（Functor / Applicative / Monad，无 `fix`）不是图灵完备的：bind 次数由项的语法结构决定；游戏需要的累积状态 `stateₙ₊₁ = f(stateₙ, inputₙ)` 是 primitive recursion，保证停机。真需要无界搜索时走 **bounded loop 且上限进产物**。无限细分曲面是 codata（生产性展开），不是无界计算 —— **缺的不是 `fix`，是 codata**（S4） |
| AF-03 | **f64 向量 / floating origin 是双曲漫游的前置（G6）** | `ze-notion` / 3D 相机 | **记账（本批不实现）** | 实测：[`Camera3D.eye`](crates/scene/ze-scene-core/src/object/camera.rs) 是 `Vec3`（f32）；[`ze-notion`](crates/kernel/ze-notion/src/vec.rs) 无 f64 向量；全仓无 floating origin。双曲空间面积随半径指数增长：f32 上限 ~3.4e38 ⇒ 半径约 89 处溢出；尾数 24 位 ⇒ 距原点 1e6 时分辨率已 ~0.06，漫游肉眼可见抖动。**表达力救不了精度，这是数学层的事**，与组合子库无关 |
| AF-04 | **运行时脚本执行 = 延后非不做** | 脚本 / 产物 | **记账（S0）** | 它与提案 #2（ZSB v5 烘焙）是**同一能力的两种实现**，判据是 `--script` / `--bundle` / `--baked` 三条路径像素逐位相同。本计划不实施 #2（26 条基线全重录），但**不否决**运行时脚本执行 |
| AF-05 | **交互 / 输入方向记账（G4）** | 输入 / 宿主 | **记账（S0）** | `ViewportClick` / `ViewportHover` 由 [`zela-egui`](crates/ui/zela-egui/src/host.rs) 产出，**零消费方**。此前「交互 / 游戏 / 用户输入」在本文件 grep 无引用（第 5 次漏账，PA 批刚记过这个模式）。S3 的 `Event` 通道是它们的第一个消费方 |
| AF-06 | **字面量自动提升未落地（表构造器已落地）** | 脚本 API | **部分关闭（S2 尾巴）** | **已落地**：`Sin { of = ... }` / `Mul { a = ..., b = ... }` / `Scale { k, of }` / `Sum { terms }` 由已注册函子签名**自动生成**（`component_ctor` 路径，LuaLS / `.d.ts` 里均有构造器）；`expr.sin{...}` 也可直接调用。**未落地**：数字→`Pure` 的自动提升 —— `Mul { a = t, b = k }` 仍需写 `b = expr.pure(k)`。**边界**：产物格式不受影响（字面量已是 `Pure` 叶子），只是作者书写形态。 |
| AF-07 | **S3 已落地 / 未落地** | 时间 / 求值 / 绑定 | **记账（S3）** | **已落地**：`Director` 折叠时间（`t = base_t + (frame−base_frame)×dt×speed`，固定 fps 下严格等于 `frame×dt`）、`TimeState.frame` 一等通道、`CompiledExpr`（装配期末序程序 + 帧内 memo，无跨帧缓存）、`Mono` / `SceneMono`（Time / Frame 通道 + `zela.math` 标量算子语义）、**表达式 → 场景目标绑定**（`expr.bind{}` 登记 → 宿主 `PodAssembly` 按对象名并入 `SceneScript` → `BindingDriver::Expr` 在 `BindingApplySystem` 求值；对象名 / 目标名 / 通道 / 算子都在**装配期**校验，JSON 往返保留 ⇒ bundle 路径可用）。**验证**：36 条像素基线（native + vector）逐位不变；验收作品 [`expr_demo`](crates/app/zela-app/scripts/expr_demo.lua)（半径 / 透明度由表达式驱动）payload `f343e2ba…`，`--script` 与 `--bundle` 240 帧序列哈希同为 `f20ba3f4…`，0/60/150 帧 PNG 各不相同（动画确实由 M 驱动）。**未落地**：`Event` / `ComponentState` / `Env` 通道仍显式缺失；`Reveal.current_pos` 的 read 通道（G1 读方向）未接线；相机 pos 未建模为 `M<Vec3>`。⚠️ 语义边界：表达式读**全局** `Time`，`duration` 只做区间门控（`Hold`）—— 需要段内局部时间时应新增通道，不能偷改 `Time` 语义。⚠️ 验收里的 `--baked` 路径不存在（提案 #2 未实施），当前只能验证 `--script == --bundle`。 |
| AF-08 | **S4 codata 已落地 / 未落地** | 表达式层 / 几何 | **记账（S4）** | **已落地**：[`ze-func::codata`](crates/kernel/ze-comb/src/codata.rs) 的生产性展开器（`Unfold`）、`depth` 观察者（`observe` / `Expanded::frontier`）、**上限进产物**（`CodataSpec { depth, max_nodes }` 可序列化）、节点预算实时检查（超出即显式错误，不做无提示截断）、**零缓存**（每次展开从头算 ⇒ 清空重算逐位相同是结构事实）、不生产（`step` 返回空）显式失败。**未落地**：场景侧无限细分曲面消费方（网格生成 / LOD 选择）未接线；`CodataSpec` 尚未进入 `SceneScript`（当前只是内核 API）；"无限曲面 + 有限帧数"的帧成本实测未做。 |

| AF-09 | **声明文件防腐烂已落地（§7.5 第 1/2/3 条）** | 导出 / 门禁 | **已关闭** | ① `capabilities_json` 顶层加 `schema_version`（`DOCS_SCHEMA_VERSION = 1`，与 capability `version` 分开）；② 新增 `cargo verify --step docs`：用 `export_scene_api` 生成 JSON / LuaLS / `.d.ts` / `.pyi` 到 `target/declarations/{a,b}` **连续两次**并逐字节比对（缺 / 差 / 多都 fail，报错给出重生成命令）—— 生成物**不进仓库**（AG-07 定案）；③ `export_scene_api` 纳入 `MathApi`，声明文件里出现 `Sin` / `Mul` / `Scale` / `Sum` 等表达式构造器。当前共 **9 个文件**（JSON / LuaLS / `.d.ts` + 6 个 `.pyi`），均在 `typst,js` 组合下生成。`.pyi` 生成器是纯 Rust（`ze-pod-py` 的 `export` 模块不依赖 pyo3，作为 dev-dependency 引入）⇒ 文档基线不再需要 Python 解释器；`PythonApi` 也已纳入导出。 |

**AF 段小计**：立约 2（AF-01、AF-02）· 部分关闭 1（AF-06）· 记账 5（AF-03、AF-04、AF-05、AF-07、AF-08）· 已关闭 1（AF-09）。

---

## AG. 提案 #6 批次记账（寻址数据化与脚本 API 便捷化，2026-10-26）

> 计划《寻址与构造器执行计划：提案 #6 落地 + 脚本 API 便捷化》（一批四次提交 A/B/C/D）。
> 口径：**语义（A/B）与糖（C）分开**；`zela.scene@10 → @11` 一次付清。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AG-01 | **寻址数据化已落地（A 批）** | 场景 / 动画 | **已关闭（A）** | `TargetKind` 枚举删除，改为 [`FieldSpec`](crates/scene/ze-scene-core/src/animation/target.rs)：`name`（持久化名）/ `address`（规范地址 `Component.field`）/ `component`（组件登记表名字）/ `field` / `ty` / `write` / `read`。**读写同源**：read 不再是第二张表（[`SceneRead`](crates/scene/ze-scene-core/src/scene_write.rs) 与 `SceneWrite` 对称）。**类型不符显式报错**：`apply` 返回 `Result<(), TargetError>`，`TypeMismatch { target, expected, actual }`（用例 `type_mismatch_is_an_explicit_error`）。**coercion 独立成层**：`target::coerce`（`rotation_z` / `splat3` / `position3`）。**命名唯一来源**：`component` 必须等于 `known_type_name::<_>()`（用例 `component_names_come_from_the_registry`）。`SceneApi::clip` 的 target 改为地址字符串 ⇒ `@11`，指纹 `16274693131949166351` 已固定。**验收**：36 条像素基线逐位不变；`cargo verify --step demo` 23 条 payload 与 manifest 一致 |
| AG-02 | **规范地址与旧选择键并存（记账，非缺口）** | 脚本 API / 覆盖门禁 | **记账（A）** | 旧 payload 的 target 名沿用旧名（`CircleRadius` / `RevealRange` / …）⇒ 老产物逐字节不变；规范地址（`Circle.radius` / `Reveal.range` / …）作为**新选择键**同时可用，写同一字段。组合选择键（`Camera2DFraming` = `Framing2D.scale` ∪ `Framing2D.center`；`RevealRange` = `Reveal.end` ∪ `Reveal.range`）没有单一字段地址，`address` 与持久化名相同。**后果**：4 个单字段选择键当前没有作品直接使用（字段写入已被旧组合选择键覆盖）⇒ 在 [`capability_exemptions.txt`](crates/app/zela-app/scripts/demo/capability_exemptions.txt) 豁免并指向本行；B/C 批改用规范地址后应逐条删除（过期豁免会让门禁失败） |
| AG-03 | **A 批偏差：字段访问器用声明宏而非 derive 宏** | `ze-scene-core` | **记账（A）** | 计划 §7 写「字段访问器（derive 宏）」。实际用 `scalar_spec!` / `vec2_spec!` / `vec3_spec!` 三个声明宏：**新增字段仍只写一行**（`scalar_spec!(CIRCLE_RADIUS, "CircleRadius", "Circle.radius", Circle, radius)`），且不必引入 proc-macro 依赖。`ze-scene-core` 已不依赖 `ze-pod-macro`。若将来字段路径需要嵌套（`a.b.c`），再评估 derive 形态 |
| AG-04 | **`Select` 只留格式已落地（D 批）** | 表达式层 | **已关闭（D）** | `Node::Select { cond, then, else }` 的序列化形状固定（命名子表达式，`{"type":"select","data":{…}}`），`children()` 覆盖三个分支 ⇒ 悬空 `Ref` / 环 / 通道校验照常生效。**求值不实现**：`CompiledExpr::compile` 返回 `ExprError::UnsupportedNode("select")`（显式未实现，不给出错误结果）。用例：`select_format_is_pinned`（JSON 形状 + round-trip）、`select_is_explicitly_unimplemented`。**未落地**：`Reveal.current_pos` 的读通道仍未接进宿主 M（AF-07 同一条），所以值分支暂时没有真实条件可用 |
| AG-06 | **三语言构造器便捷化已落地（C 批）** | 脚本绑定 / 声明文件 | **已关闭（C）** | ① 构造器从"函数"变**可调用对象**：Lua 是带 `__call` 元表的表、Python 是 `functools.partial`（`PyCFunction` 不能挂属性）、JS 是原生函数对象 ⇒ 三语言都能把通道常量挂上去（`Circle.channel.radius`，来自 `Pod::channels()`，值是 `{ namespace, name }` 数据）。② **位置参数 + 具名双形态**：唯一实参是 table/对象/kwargs ⇒ 具名；否则按字段顺序取位置参数。⚠️ **偏差记账**：计划 §8 强制约束 5 要求「同一构造器只一种参数形态」，但既有 251 处脚本全用具名，单形态会全部破坏；实际判别规则只看**首参是不是容器**（位置参数永不接受 table/object），Lua 空表歧义（`Vec2()` vs `Vec2{}`）因此不存在。③ 默认值白名单唯一来源在 [`defaults.rs`](crates/ze/pod/ze-pod-binding/src/defaults.rs)：`Color.a = 255`；**不进产物**（构造完即完整值），但进 LuaLS / `.d.ts` / `.pyi`（可选字段 / `a = 255`）。**验收**：Lua 与 JS 的等价脚本（位置 / 具名 / 省 `a` / `Circle.channel.radius`）产出**逐字节相同**的 `SceneScript`（用例 `positional_constructors_and_channel_constants_agree_across_languages`）；23 条 demo payload 逐条不变；声明基线 9 文件重生成后逐字节一致。⚠️ Python 路径在本机只有**编译校验**（`PYO3_PYTHON` 指向 msys Python 时 `cargo check -p ze-pod-py --features python` 通过），链接缺 `libpython3.14.lib` ⇒ 用例 `python_positional_constructors_match_lua` 在 CI 的 MSVC Python 环境下执行 |
| AG-05 | **handle 绑定 + 通道数据化已落地（B 批）** | 表达式层 / 脚本 API | **已关闭（B）** | ① `ze_func::Channel` 枚举删除，改为 [`ChannelRef { namespace, name }`](crates/kernel/ze-comb/src/expr.rs)：基础通道空 namespace（`time` / `frame`），组件字段 `"zela.scene" + 地址`；`SCHEMA_VERSION` 1→2（受影响作品只有 `expr_demo`）。② `expr.bind{ object = <spawn 句柄> }`：`BindSpec.object` 改为 pod `Handle`，`SceneScript::bind_expr(handle, …)` 取代 `bind_expr_named`，`find_named` / 重名歧义错误删除（`Name{}` 仍在，但只当分组标签）。产物不存 pod 句柄 —— `SceneServicePod::finish` 快照 pod→脚本句柄，`PodAssembly::merge_expr_bindings` 是**唯一转换点**（`handle` 越界是显式错误）。③ `expr.bind{ target = Circle.radius }`：`zela.math.Bind.target` 是 `ChannelSpec { namespace, name }`（`zela.math@3 → @4`）；`Pod::channels()` 从场景地址表导出单字段规格（组合选择键不导出）。**验收**：`expr_demo` 去掉 `Name{}` 与 target 字符串，payload 重录为 `2611dda9…`，23 条 demo payload 断言通过；`--script` 与 `--bundle` 各 80 帧（640×480 / 10fps / vector）序列哈希同为 `0f77479c…` |
| AG-07 | **定案：声明文件移出仓库 + 通道地址去裸字符串** | 导出 / 脚本 API | **已关闭（定案）** | ① **生成的 API 不进仓库**：删除 `crates/app/zela-app/api/**` 与 `.gitattributes` 里对应的 `-text` 规则；`cargo verify --step docs` 改为**连续生成两次**到 `target/declarations/{a,b}`，断言文件集合非空、每个文件非空、两次输出逐字节相同（生成物只活在构建产物区）。AF-09 的"9 文件基线"口径由本行取代。② **通道地址不再有裸字符串入口**：删除 `zela.math::channel(address: String)`（`zela.math@4 → @5`），通道选择键只剩**组件的 `channel` 字段**（`Circle.channel.radius` / `Opacity.channel.value`，由 `Pod::channels()` 挂到可调用构造器上）；写错常量名在绑定层立即暴露。`BindSpec.target` 仍是 `ChannelSpec` 数据（namespace + name），但作者侧不出现地址字符串。③ **常量进声明文件**（否则编辑器把 `Opacity.channel.value` 诊断成 undefined-field）：`ChannelSpec` 增加 `value_type`，三语言声明都标出常量 —— 常量收在组件的 `channel` 字段下（组件字段可能与地址同名，如 `Opacity.value`，平铺会撞名）；LuaLS 输出 `---@field channel XChannel` + `---@class XChannel`（构造器保持 `function X(spec) end` 形态，`---@operator call(...)` 方案已撤销），`.d.ts` 输出 `interface XChannel` + `namespace X { const channel: XChannel }`，`.pyi` 输出 `class _XChannel` 并在构造器 Protocol 上声明 `channel: _XChannel`。**影响**：`expr_demo` 改用常量；产物里的库版本 `4 → 5` ⇒ payload 重录为 `94d731b4…`（像素不变） |

**AG 段小计（提案 #6 A–D + 定案）**：已关闭 5（AG-01、AG-04、AG-05、AG-06、AG-07）· 记账 2（AG-02、AG-03）。

---

## AH. 时钟显式化批次记账（分布式时间源 + predicate / reset，2026-10-26）

> 计划《时钟显式化执行计划：分布式时间源 + predicate 绑定》（一批四次提交 A/B/C/D）。
> 口径：**A 比较算子 → B clock 显式化 → C predicate / reset → D 去 cursor 与时长**；
> 旧 `Curve` 路径保持可用，36 条基线不得重录。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AH-01 | **比较 / 布尔算子已落地（A 批）** | 表达式层 / 函子库 | **已关闭（A）** | `zela.math` 的 [`OPS`](crates/ze/pod/ze-pod-func/src/ops.rs) 增加 `lt` / `gt` / `le` / `ge` / `eq` / `ne` / `and` / `or` / `not`；对应 PodType（`Lt` … `Not`）在 [`api.rs`](crates/ze/pod/ze-pod-func/src/api.rs) 声明、在 [`expr_pod.rs`](crates/ze/pod/ze-pod-func/src/expr_pod.rs) 注册为值算子（纯结构、无状态）。比较算子产出 `Value::Bool`，**不经过 `anim_value`**（root 侧语义不变：Bool 仍是显式不可写，`comparisons_evaluate_to_bool_but_are_not_animatable`）。`eq` / `ne` 只支持数值与布尔（不做隐式真值转换，U2）；`and` / `or` / `not` 只接受 Bool（`boolean_combinators_require_bool`）。⚠️ Lua / Python 里 `and` / `or` / `not` 是关键字 ⇒ 作者侧用表构造器 `And {}` / `Or {}` / `Not {}`（JS 可用 `expr.and(...)`）。`LIBRARY_VERSION` 仍为 5：既有表达式语义与 payload 逐位不变 |
| AH-02 | **clock 显式化已落地（B 批）** | 表达式层 / 时钟 / 脚本 API | **已关闭（B）** | ① 脚本侧 `clock.time()` / `clock.shift(s)` / `clock.scale(r)` / `clock.nest(parent, child)`：`Clock` 是**数据**（`zela.math.Clock { links }`，从根到本时钟的扁平链），三语言绑定把 `clock_` 前缀的函数同时挂到 `clock` 表 / 对象 / namespace（[Lua](crates/ze/pod/ze-pod-lua/src/binding.rs) / [JS](crates/ze/pod/ze-pod-js/src/binding.rs) / [Python](crates/ze/pod/ze-pod-py/src/imp.rs)），声明文件同步输出 `clock`。② `Bind.clock` 收它（缺省 = 空链 = 全局，经 [`defaults.rs`](crates/ze/pod/ze-pod-binding/src/defaults.rs)）；`PodAssembly::merge_expr_bindings` 转成 [`ClockProgram`](crates/kernel/ze-time/src/clock.rs)，`Step::Expr` 携带 `clock`（`skip_serializing_if = is_global` ⇒ 全局时钟的 payload 逐字节不变）。③ 编译期展开成 `ClockNode` 链（[`timeline.rs`](crates/scene/ze-scene-core/src/animation/timeline.rs)），运行期 `BindingApplySystem` 用**该绑定的局部时间**构造 `SceneMono`（[`system.rs`](crates/scene/zela-scene-ecs/src/animation/system.rs)）⇒ 表达式 `time` 与 `Curve` 的 `eval_local` 同源，G1 被**消灭**而非修复。④ `LIBRARY_VERSION 5 → 6`、`zela.math@5 → @6`。**验收**：`clock_constructors_agree_across_languages`（Lua / JS 逐字节相同，含 `"clock"` 字段）、`expression_binding_reads_its_own_clock_local_time`、`nested_clocks_compose_linearly`（`local = t × 0.5 − 1`） |
| AH-03 | **predicate / reset 已落地（C 批）** | 表达式层 / 绑定 / 产物 | **已关闭（C）** | ① `Bind` 增加 `predicate` / `reset`：`Predicate.Always`（缺省）或 `Predicate.Of { expr = <Bool 表达式> }`；`Reset.Hold` / `Reset.Reset` / `Reset.Skip`（计划里的 `none` 在 Python 里是字面量 ⇒ 改名 `Skip`，语义相同：不写）。② `duration` 降为**糖**：`> 0` ⇒ `predicate = Lt { time, d }` + `reset = hold`（[`scene_script.rs`](crates/scene/ze-scene-core/src/scene_script.rs) 的 `duration_predicate`）；产物里**不再有** `duration`（`Step::Expr.duration` 只为解码历史 payload 保留）。③ 运行期 predicate 与 root **分开求值**（[`system.rs`](crates/scene/zela-scene-ecs/src/animation/system.rs)）；`reset = Reset` 在 `commit` 时经 `Target::read` 从场景快照初值（[`scene_script.rs`](crates/scene/ze-scene-core/src/scene_script.rs)），失效时写回；hold / skip 都不写。④ 引擎**推导** `SeekKind`：`predicate + hold` ⇒ `Replay`（保守：单调性分析未实现），其余 `O1` —— 不进脚本（强制约束 2）。⑤ 装配期不再沉默：predicate 与 duration 同时给出 ⇒ `ExprBindError::ConflictingGate`；predicate 非 Bool 在 C 批是运行期显式记录并按"不生效"处理（**B 批已把这条检查前移到装配期**：`infer_predicate`）。**验收**：`duration_sugar_equals_predicate_and_hold`（payload 逐字节相同）、`duration_sugar_holds_after_the_window`、`reset_restores_the_initial_value`、`skip_leaves_the_field_untouched`；`expr_demo` 的光晕绑定改用显式 `Predicate.Of` + `Reset.Hold`（payload 重录；该作品不在 36 条像素基线内） |
| AH-04 | **去 cursor 与时长已落地（D 批）** | 场景脚本 / 产物 / 日志 | **已关闭（D）** | ① 删除 `SceneScript::duration()` 与 `Timeline::total_duration()`，改为 [`still_time() -> Option<f64>`](crates/scene/ze-scene-core/src/animation/timeline.rs)：只对**纯 Time 依赖**的 predicate（`Lt { time, Pure(d) }`）求上界，时钟链按仿射 `local = rate·t + shift` 反解（`rate > 0`）；`Play` 段按 `start + duration` 计入。② 不可分析（恒生效 / `Gt` 等非可解形状 / 非正速率）⇒ `None`，调用方**显式说明**，不拿 0 冒充（强制约束 4）。③ `compile_script` 判空改用 `creates` 非空；日志改为「静止时刻 ≈ Xs」/「静止时刻不可推导：含非时间依赖或恒生效的 predicate」。④ ZSB 的 `Meta.duration` **删除**（U3：静止时刻不进产物 —— 它是派生量、可能不可求）；`zela-compile` / `zela-render` / `zela-web` 的状态行改为**现算**的静止时刻（或"不可推导"）。录制长度由 `ExportSpec.frames` 决定。**验收**：`still_time_is_derived_from_time_only_predicates`（`scale(0.5)+shift(1)` & 窗口 2 ⇒ 6.0）、`still_time_is_none_when_not_analyzable`；bundle / persist / scene_script 用例改用 `still_time`；zela-app 44 条全绿 |
| AH-05 | `still_time` 的**粒度**（原：全有或全无 + `unwrap_or(0.0)`） | 场景脚本 / 时长 | **已关闭（C）** | ① `still_time` 改为**逐条绑定独立判定**：可分析且有限右端 ⇒ 计入；不可分析 ⇒ 跳过（不再用 `?` 短路整部作品）；全部不可分析才 `None`；恒生效 / 区间延伸到 `+∞` 仍显式 `None`（永远在动）。② `zela-playback` 的 `SceneAssembly.duration: f64` 改为 `record_hint: Option<f64>`（[`lib.rs`](crates/app/zela-playback/src/lib.rs)）—— `None` 才是"不可推导"，由 `forest_duration(...)` 兜底；[`zela-app`](crates/app/zela-app/src/lib.rs) 的 `still_time().unwrap_or(0.0)` 删除，直接透传 `Option`。**验收**：`still_time_skips_unanalyzable_bindings`（可分析 2.0 + 不可分析 ⇒ `Some(2.0)`）、`still_time_is_none_when_every_predicate_is_unanalyzable`、`still_time_understands_interval_windows` |
| AH-06 | 时间窗口只认单个 `Lt`，`And` 组合一用即失去可分析性 | 表达式层 / 可分析性 | **已关闭（C）** | 新增 [`predicate.rs`](crates/scene/ze-scene-core/src/animation/predicate.rs)：`Lt/Gt/Le/Ge/Eq/Ne`（`time` 与字面量比较，两侧顺序都接受）→ 半无限区间 / 点集；`And` = 交、`Or` = 并、`Not` = 补；`IntervalSet` 规范化（有序、互不相交、端点开闭），并提供 `contains` / `last_active` / `finite_max_end`。`still_time` 与运行期 Hold 共用同一份分析结果。**验收**：`And{Lt{t,4},Gt{t,1}}` ⇒ `Some(4.0)`（`still_time_understands_interval_windows`）；`last_active` 在空隙取**右端点**（U3）；`Not`/`Or` 的端点开闭有单测锁定 |
| AH-07 | `Reset.Hold` 靠"不写"实现，`Replay` 已推导但**无重放路径** | 绑定 / seek | **已关闭（C）** | ① `Binding` 增加 `active: Option<IntervalSet>`（局部时间）；可分析时 `BindApplySystem` 不再逐帧求 predicate，且 `Hold` 改为**纯函数** `f(last_active(t))` —— seek 到任意一帧直接算，不需要先跑前面的帧，也与绑定应用顺序解耦。② `bind_expr_gated` 的 `SeekKind` 推导：`predicate + Hold + 可分析` ⇒ `O1`（强制约束 6）；脚本装配期对 `Hold + 不可分析` **显式拒绝**（`ExprBindError::UnanalyzableHold`），只有历史 payload 的不可分析 Hold 才保守保留 `Replay`。③ `SeekKind::Replay` 本身保留给真正有记忆的 root（弹簧 / 数值积分，本批不实现但**不得删除**）。**验收**：`hold_is_pure_when_the_predicate_is_an_interval_window`（单帧 seek 到窗口后 = `f(4)`；首次生效前不写）；`expression_binding_reaches_the_recipe_through_lua` 改为断言 `O1` + `active.is_some()`；36 条像素基线 ok 36 / DIFF 0 |

**AH 段小计（时钟显式化 + 类型/输入流）**：已关闭 7（AH-01…AH-07）· 待处理 0。

---

## AI. 类型系统与输入流批次记账（L2 类型化 + 双层确定性，2026-10-26）

> 计划《类型系统与输入流执行计划：L2 类型化 + 双层确定性》（A/B/C/D 四批）。
> 口径：**A 类型数据 → B L2 合一 → C 区间代数 → D 输入流**；
> 36 条像素基线是 `Render(ZSB, ∅)`，本批不得重录。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AI-01 | **L2 类型推导已落地（B 批）** | 表达式层 / 函子库 / 装配 | **已关闭（B）** | [`infer.rs`](crates/ze/pod/ze-pod-func/src/infer.rs)：每条 [`OpSig`](crates/ze/pod/ze-pod-core/src/typespec.rs) 独立实例化，`Subst` 做槽位合一 + kind 检查（`Num` / `Vector` / `Bool`）+ occurs check；重载**逐条尝试**，0 条 ⇒ 错误带「签名 + 第 N 个参数」，>1 条 ⇒ **歧义错误**（强制约束 4）。叶子类型封闭（字面量 / 通道 → 数值 / 布尔 / Vec2 / Vec3），`Ref` 继承定义类型。`bind` 在装配期调用 `infer_root` / `infer_predicate`：Bool 根与非 Bool predicate 都**不再留到运行期**（AH-03 的运行期检查前移），`PendingBinding.root_type` 带到宿主，[`assembly.rs`](crates/app/zela-app/src/assembly.rs) 用 `target_entry(...).spec.ty` 核对目标字段（`Circle.radius` = F32 ⇒ 数值）。**验收**：`and_rejects_numbers_with_a_source_location`（`And{f64,f64}` 装配期拒绝）、`vec2_times_number_is_vec2`（`Vec2*F64 → Vec2` 两侧都对）、`ambiguous_overloads_are_explicit`、`bind_rejects_bool_root_at_assembly`、`bind_rejects_non_bool_predicate_at_assembly`；`zela.math@6` 不变，23 条 payload 与 `expr_demo` 哈希逐位不变（推导结果不进产物） |
| AI-02 | 绑定目标通道的类型来自**宿主地址表**，不在 `zela.math.Channel` 常量里 | 绑定 / 通道常量 | **已记录（设计选择）** | 三语言绑定把 `Circle.channel.radius` 挂成 `{ namespace, name }`（[`attach_channels`](crates/ze/pod/ze-pod-lua/src/binding.rs)）；值类型留在 `Pod::channels()` 的 [`ChannelSpec.ty`](crates/ze/pod/ze-pod-core/src/pod.rs)，装配期由宿主反查 [`TARGETS`](crates/scene/ze-scene-core/src/animation/target.rs)。好处：`zela.math` 保持场景无关（分层铁律），加类型不必改脚本面 schema；代价：不在 `TARGETS` 里的第三方目标会**跳过**根/目标核对（仍由 `Target::write` 在运行期报 `TypeMismatch`）。若将来要求通道常量自带类型，需要给 `zela.math.Channel` 加可选 `ty` 字段并递增库版本 |
| AI-03 | **区间代数 + Hold 反算已落地（C 批）** | 表达式层 / 绑定 / seek | **已关闭（C）** | [`predicate.rs`](crates/scene/ze-scene-core/src/animation/predicate.rs) 把 predicate 映射成**局部时间区间集**（`Lt/Gt/Le/Ge/Eq/Ne` + `And` 交 / `Or` 并 / `Not` 补，端点开闭规范化）；`last_active(t) = sup{x ≤ t}` 是 O(区间数) 的零状态查询。`Binding.active` 携带它：运行期生效判定不再逐帧求 predicate，`Hold` 改为纯函数 `f(last_active(t))`（AH-07），`predicate + Hold` 退回 `SeekKind::O1`（不可分析的 Hold 在脚本装配期被 `ExprBindError::UnanalyzableHold` 拒绝）。`still_time` 复用同一份区间集：有限右端按仿射时钟反解，恒生效 / `+∞` ⇒ `None`，不可分析逐条跳过（AH-05 / AH-06）。**验收**：`And{Lt{t,4},Gt{t,1}}` 可分析且静止时刻 `Some(4.0)`；单帧 seek 到窗口后得到 `f(4)`；36 条像素基线 ok 36 / DIFF 0，23 条 demo payload 逐位不变（区间集不进产物） |
| AI-04 | **输入流产品已落地（D 批）** | 输入 / 产物 / 回放 | **已关闭（D）** | [`input_stream.rs`](crates/scene/zela-scene-bundle/src/input_stream.rs)：独立文件 `zela.input/1`，**按帧号索引**的稀疏事件（`{ frame, events }`），空帧不写；`Pairing { zsb_payload_hash, libraries }` 记录"为谁而录"，`validate` 对 payload hash / 函子库身份**显式 fail**（不让 desync 无提示地发生）。`Recorder` **默认关**（`Default` = disabled，`capture` 是 no-op），`Recorder::recording(pairing)` 才录。`to_json` 规范输出（帧升序、同帧合并）+ round-trip 逐字节检查。事件跨进程：`SceneEvent` / `CameraCommand` / `WidgetId` 加 serde，`SceneScript::libraries()` / `Timeline::libraries()` 提供配对身份。回放侧 [`apply_input_frame`](crates/app/zela-playback/src/input_replay.rs) 在 `Advance` 前整帧替换 `InputEvents`；`zela-render --bundle … --input-stream FILE` 先配对再逐帧重放。**验收**：`pairing_rejects_a_different_zsb`（ZSB 变更 ⇒ 显式 fail）、`stream_is_frame_indexed_and_byte_stable`、`recorder_is_off_by_default`、`replay_twice_is_bit_identical`、`replaying_the_same_stream_twice_is_bit_identical`（Director 状态逐位比较）；强制约束 1/2/3 全部满足 |
| AI-05 | 宿主侧**录制入口**未接线（只有 API 级开关） | 输入 / 宿主 | **已记录（待接）** | `Recorder` 已具备"默认关 + 显式开启 + 每帧 `capture`"的完整语义，但 GUI（`zela-egui`）与 headless（`zela-render`）目前都**不调用** `capture`，也没有 `--record-input` CLI；`--input-stream` 只做回放，且只与 `--bundle` 搭配（配对需要 ZSB payload hash）。⇒ 现在能录的只有**测试 / 将来宿主**；真实交互录制需要一个 UI 侧"录制开关 + 保存流"的入口。回放侧不受影响（流格式与 `apply_input_frame` 已稳定） |
| AI-06 | `last_active` 遍历依赖「区间有序不相交」不变式，**无断言锁定** | 表达式层 / 区间代数 | **待处理** | [`IntervalSet::new`](crates/scene/ze-scene-core/src/animation/predicate.rs#L108-L140) 排序 + 合并保证"有序、互不相交、已合并"，但消费方（[`last_active`](crates/scene/ze-scene-core/src/animation/predicate.rs#L221-L239) / `still_time` / `intersect`）都是**零状态遍历**，一旦上游构造出非规范区间即无提示给出错值；无 `debug_assert` 锁定该不变式。**拟修**：`IntervalSet::new` 末尾加 `debug_assert!(is_normalized)`（O(n) 线性核查，仅 debug） |
| AI-07 | 历史 payload 的 `Replay` 绑定**无消费者**：seek 中间帧 Hold 值错**且不报错** | 绑定 / seek | **待处理（优先）** | `SeekKind::Replay` 只由[历史 payload 的 `Hold`](crates/scene/ze-scene-core/src/animation/binding.rs#L157-L171) 推导出来，引擎当前只实现 `O1`（纯函数）；任何带 `predicate + Hold` 的历史绑定 seek 到中间帧都会无提示拿到错值，无运行期 fail（AI-03 后新绑定已被 `UnanalyzableHold` 装配期拒绝，**旧产物不受保护**）。**拟修**：回放侧遇到 `SeekKind::Replay` 的绑定且无记忆演化器 ⇒ 显式报错而非照旧跑 |
| AI-08 | 输入流只有半条路：**能放不能录**（GUI / headless 都不调 `capture`，无 `--record-input`） | 输入 / 宿主 | **待处理** | 与 AI-05 同源：`Recorder` 的 `capture` 无任何宿主调用点，`zela-render` 无 `--record-input` CLI（[`input_stream.rs`](crates/scene/zela-scene-bundle/src/input_stream.rs) 的 `Recorder` 默认关）；回放侧（`apply_input_frame`）已稳定。**拟修**：`zela-render` 加 `--record-input FILE`（配对 + 每帧 capture），GUI 加录制开关 |

**AI 段小计**：已关闭 3（AI-01、AI-03、AI-04）· 已记录 2（AI-02、AI-05）· 待处理 3（AI-06、AI-07、AI-08）· B / C / D 三批完成。

---

## AJ. 通道读端与字段注册批次记账（`ze-scene-macros`，2026-10-27）

> 计划《通道读端与字段注册执行计划：新建 `ze-scene-macros` + 显式注册》（E → F → G → H，每批一次提交）。
> 口径：**注册去三字符串 → 字段通道 derive → 读端 monad → Select**；36 条像素基线不得重录，
> `zela.scene` 指纹不动（E/F 只改"怎么注册"，不改注册了什么）。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AJ-01 | **组件注册去三字符串已落地（E 批）** | api / 注册 | **已关闭（E）** | 新建 [`ze-scene-macros`](crates/scene/ze-scene-macros/src/lib.rs)（proc-macro，**不带 `zela-` 前缀**，避开 xtask 判据 2b 的生产依赖门禁）。`#[derive(SceneComponent)]` 从 `#[pod_type(name = "...")]` 取 `POD_TYPE_NAME`、从 ident 的 snake_case（仅"小写→大写"插 `_`，`Camera2D → camera2d`）推导 `CTOR_NAME`；`RegisterSceneComponent::register_component::<T>(ctor)` 内部调 `register_fn1_named::<T, Handle>`，25 条注册点从 5 行降到 1 行，删掉 75 个手写字符串。`ze-scene-api` 顶部 `extern crate self as ze_scene_api;` 供宏生成的全路径引用。**验收**：25 个组件名测试锁定（`component_names_match_registered_ctors`）、声明文件逐字节一致、`SCENE_API_FINGERPRINT = 16274693131949166351` 不变 |
| AJ-02 | **`Channels` derive + 字段全量注册（F 批）** | 场景 / 动画 | **已关闭（F）** | `#[derive(Channels)]` + `#[channel(name = "...", field: Ty)]`（`name` 缺省 = 规范地址 `Component.field`；显式名保留旧持久化名 `CircleRadius` / `Phase` / `MorphT` / `CameraEye` / `Reveal.end` 等，**老 payload 一字不改**）。类型映射**封闭**：`f32 → Scalar(F32)`、`Vec2 → zela.Vec2`、`Vec3 → zela.Vec3`、`bool → Scalar(Bool)`，其它类型**编译期报错**。10 个既有组件加 derive（18 条派生通道），另手写 7 条（`Dash.period/duty/phase`、`CurveWindow.lo/hi`、`Gradient.from/to` —— 类型在 `ze-render-ir`，core 不能反向依赖）；`TARGETS` 现共 32 条（手写 14 + 派生 18），`Gradient.stops` 因无 `AnimValue` 表示不注册。组合键 `Camera2DFraming` / `RevealRange` 保留手写并标 `FieldSpec.is_composite`，`channels()` 的筛选从字符串拼比对改为 `!is_composite`（强制约束 3）。新增字段：`Circle.center`、`Reveal.start`、`Arrow.from/to/head_length/head_width`、`Light.intensity`、`Material3D.metallic/roughness`（`Camera2D.center/scale` 因 `Camera2D` 是派生视图类型而**不注册**，见 AJ-05）。**验收**：`derived_channels_in_targets`（地址无重复 + 旧名齐全）、`derived_write_read_roundtrip`、`composite_specs_flagged`；`SCENE_API_FINGERPRINT` 不变（函数签名未动）。`xtask` 的覆盖门禁同步从 `#[channel(...)]` 属性抽通道，并把作者侧常量路径（`Gradient.channel.from`）算作使用（否则 derive 掉的通道 / 表达式常量都会从门禁里无提示消失） |
| AJ-03 | **通道读端 monad + 帧内定序（G 批）** | 表达式 / 场景 / 产物 | **已关闭（G）** | ① [`SceneMono`](crates/scene/zela-scene-ecs/src/expr_mono.rs) 持 `&'a dyn SceneRead`，`Mono::read_object(channel, object)` 按 `channel.name` 查 `TARGETS` → 调 `spec.read`；协议值 `AnimValue → Value`。② `ze-func` 新增 `Node::Read { channel, object }` / `Step::Read`；`Mono::read_object` 的**默认实现显式失败**（`MissingChannel`），既有宿主实现不破。③ 脚本侧 `expr.read { channel = Circle.channel.radius, ty = "Number", object = curve }`（`ReadSpec`，**对象显式传入**，无隐式"读自己"），`zela.math@6 → @7`（H 批再升到 `@8`）、`MATH_API_FINGERPRINT` 更新。④ `is_injected()` 改**查注册表**（`zela.scene` 地址在 `TARGETS` 里即放行），装配期 `Node::Read` 也过同一检查；`Read.object` 越界 ⇒ `UnknownObject`。⑤ 产物里 `Read.object` 仍是**脚本局部句柄下标**（与 `handle_ref` 惯例一致），`commit` 时 `Timeline::rewrite_read_objects` 重写为 `ObjectId` 打包值再编译。⑥ **帧内定序**：`check_read_order` 在装配期扫每条表达式绑定的读集（含 `Select` 分支递归），若同一 `(地址, 对象)` 的写者时钟起点晚于读者 ⇒ `ExprBindError::ReadOrderConflict`（显式报错，而不是默默读到旧值）。**验收**：`read_object_reads_component_field_through_scene`、`read_object_fails_explicitly_for_unregistered_and_missing_component`、`read_without_host_impl_fails_explicitly`，以及读先于写/顺序正确的场景脚本用例 |
| AJ-04 | **`Select` 求值（H 批）** | 表达式层 | **已关闭（H）** | [`program.rs`](crates/kernel/ze-comb/src/program.rs) 实现 `Step::Select { cond, then, else }`：`cond` 编进外层程序，两个分支各自是**独立子程序**（独立值表 + 根索引）⇒ 运行期只 `eval` 被选中那一支，未选中支的通道 / 算子**一次都不求值**（惰性是结构性的，不靠短路标记）。`cond` 不是 `Bool` ⇒ `EvalError::SelectCondition`（显式，不默默当成真 / 假）。删除 `first_unimplemented`（`ExprError::UnsupportedNode` 保留为公开枚举变体）。**验收**：`select_picks_branch_by_condition`、`select_is_lazy_on_the_unselected_branch`（未选中支读缺失通道不报错）、`select_condition_must_be_bool`；`check_read_order` 的读集收集同步递归进分支。**脚本入口**：`zela.math.Select { cond, ["then"], otherwise }`（`SelectSpec`；`then` / `else` 在 Lua / Python 里是关键字 ⇒ 脚本侧第三个字段叫 `otherwise`，产物节点仍是 `cond/then/else`），`zela.math@7 → @8`，`MATH_API_FINGERPRINT = 8308800684048990846`，用例 `select_builds_a_value_branch` |
| AJ-05 | **偏差：`Camera2D.center/scale` 通道不注册** | 场景 / 动画 | **记账（F 批偏差）** | 计划 §5 要求批量注册 `Camera2D.center/scale`。实测 `Camera2D` 是**派生视图类型**（[`Framing2D::to_camera_2d`](crates/scene/ze-scene-core/src/object/camera.rs) 现算，渲染侧只吃 `Transform2D`），场景里**不存储**该组件 ⇒ 挂 `#[channel]` 会生成"写永远命不中"的无提示 no-op（`with_component` 未命中按约定跳过）。**处理**：不注册，2D 相机的可写通道仍由存储组件提供 —— `Framing2D.center` / `Framing2D.scale`（旧组合键 `Camera2DFraming` 同源）；`cap_channels2d.lua` 直接驱动这两个地址。若将来把 `Camera2D` 落成存储组件（渲染层改吃它），再补注册 |
| AJ-06 | **`Reveal.current_pos`：只读派生通道（笔尖跟随）** | 场景 / 表达式 | **已关闭（补充）** | 计划 §5 G「解锁：`Reveal.current_pos`、笔尖跟随」/ W2 的落账。[`REVEAL_CURRENT_POS`](crates/scene/ze-scene-core/src/animation/target.rs)：`read` = `Reveal::current_pos()`（`Path2D::flatten` + 累计弦长，与渲染侧 `clip_reveal` 同口径），`write` 恒 `TargetError::ReadOnly`。它**不进 `TARGETS`**，只经 `read_only_entry` 提供给读端 ⇒ `scene.clip` / `expr.bind` 选不中它（装配期 `UnknownTarget`，写不出假值）；`channels()` 仍导出常量 `Reveal.channel.current_pos`（脚本可读）。[`sine_trace.lua`](crates/app/zela-app/scripts/sine_trace.lua) 因此是**纯管道**：`Curve.Ramp → Reveal.end`，`expr.read(Reveal.current_pos) → Circle.center`，没有逐段动画循环，也没有"按采样密度对齐"的巧合。**验收**：`reveal_current_pos_is_a_read_only_derived_channel`（`end = 0.5` ⇒ 线段中点；只读项不在写表；写 ⇒ 显式错误）、`read_reveal_current_pos_through_the_pipeline`（经 `SceneRead` 读出 `Vec2`）；frame 60（t=2s）/ frame 179（t=6s）渲染图中小球都正好在笔尖上 |

**AJ 段小计（E–H 四批）**：已关闭 5（AJ-01…AJ-04、AJ-06）· 记账 1（AJ-05 F 批偏差）· 待处理 0 · 36 条像素基线未重录 · `zela.scene` 指纹不变（版本仍 `@11`；`Reveal.current_pos` 是**通道**，不增函数签名）。
> **作品级证据**：`cap_channels2d.lua`（2D 字段通道逐条驱动）、`cap_channels3d.lua`（PBR 金属度 / 粗糙度 / 光照强度）、`expr_read.lua`（读端 Vec2 / Number / Vec3 + `Select`）、`sine_trace.lua`（`Reveal.current_pos` 读端管道，小球画正弦）；能力覆盖 **73/73**，豁免清单清零。
> AI-06 / AI-07 / AI-08 仍为**待处理**（`last_active` 不变式、历史 `Replay` 无消费者、输入流能放不能录）—— 与 E–H 无交集，本轮未动。


---

## AK. 算子补齐与老 API 退役批次记账（2026-10-27）

> 计划《算子补齐与老 API 退役执行计划》（6 批：1 算子 → 2 类型去字符串化 → 3 类型扩展 → 4 函数级版本 → 5a 逐脚本迁移对拍 → 5b 退役重录）。
> 口径：**先证明等价、再删**；第 1–4 批是纯增量，36 条像素基线必须逐位不变。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AK-01 | **算子补齐已落地（第 1 批）** | 表达式层 | **已关闭（1）** | `zela.math@8 → @9`：新增 `mix(a,b,t)`（`Num \| Vector` 同型；`Ramp` / `Ramp2` 的算子化形态）、`clamp(x,lo,hi)`、`vec2(x,y)` / `vec3(x,y,z)`（显式构造，不做隐式提升）、`x\|y\|z(v)`（`Vec2` 上取 z ⇒ 显式错误）、`rotate3(v,axis,angle)`（罗德里格斯；轴先归一化，零轴退回 `Y` —— 与旧 `Curve::Orbit` 同口径）。签名全部进 [`signature.rs`](crates/ze/pod/ze-pod-func/src/signature.rs)（`signatures_cover_every_op_once` 门禁覆盖），`MATH_API_FINGERPRINT = 9644935882278304908`。**验收**：`mix_interpolates_numbers_and_vectors`、`clamp_bounds_both_ends`、`vector_constructors_and_components`、`rotate3_matches_the_orbit_convention`；既有算子语义未动 ⇒ 36 条像素基线不受影响（只有 `expr_demo` / `expr_read` / `sine_trace` 三件表达式作品的 payload 因库版本变化重录） |

| AK-02 | **类型去字符串化 + 可调用语法糖（第 2 批）** | 表达式 / 绑定 / 三语言 | **已关闭（2，Lua 糖）** | `zela.math@9 → @10`：`Channel` 增加结构化 `ty`（`zela.math.ChannelType`：`Unknown/Number/Bool/Vec2/Vec3`），`Read` 删除 `ty` 字符串；`ChannelTypeSpec → ChannelType` 一一映射，**没有默认分支**（未知不再默默当成 `Number`）。三个语言绑定都用 [`channel_type_variant`](crates/ze/pod/ze-pod-binding/src/lib.rs) 把 `FieldSpec.ty` 写进常量（封闭集之外 ⇒ `Unknown`，不是 `Number`）。**Lua 常量可调用**：`Reveal.channel.current_pos(curve)` ≡ `expr.read { channel, object }`（调用时才解析 `expr.read`，不依赖 capability 装配顺序）。**对账**：`SceneMono::read_object` 用 `channel.ty` 核对运行期 `Value` 标签，不符 ⇒ `EvalError::ChannelTypeMismatch`；`Unknown` 跳过（老 payload 不做推导，强制约束 7）。**偏差**：JS / Python 常量已带 `ty`，但可调用糖只做了 Lua（两件作品都是 Lua）—— 记为后续项。**验收**：`read_builds_a_read_leaf`（类型来自常量）、`declared_channel_type_must_match_the_runtime_value`、`expr_read.lua` / `sine_trace.lua` 用糖编译通过；36 条基线不变（仅 3 件表达式作品 payload 重录） |

| AK-03 | **类型系统扩展 + 转换算子族（第 3 批）** | 表达式 / 类型系统 | **已关闭（3）** | `zela.math@10 → @11`：三处类型补 `Path` —— `Value::Path { points, closed }`（纯数据点列，不把 `ze_notion::Path2D` 拉进 kernel 层）、`ChannelType::Path`、`Kind::Path`；**Path 通道本批不接**（计划 W1：脚本侧零使用，只扩类型系统）。`anim_to_value` 的 `None` 改为显式 `EvalError::UnsupportedValue { channel, kind }`：`Color` / `Path` 不再伪装成 `MissingComponent`（强制约束 4）。新增转换算子 `splat(v)`（`F64 → Vec3`）、`vec3_from_vec2(v, z)`、`vec2_from_vec3(v)`（**显式**丢 z）、`length(v)` / `normalize(v)`（`Vector → F64` / `Vector`；零向量保持零，不产出 NaN）。签名全部进 [`signature.rs`](crates/ze/pod/ze-pod-func/src/signature.rs)，`MATH_API_FINGERPRINT = 6327976336490177055`。**验收**：`conversion_operators_are_explicit`、`unsupported_component_value_types_are_explicit`；36 条像素基线逐位不变（5 件表达式作品 payload 因 `LIBRARY_VERSION` 变化重录） |

| AK-04 | **函数级版本标注（第 4 批）** | 表达式 / 产物 / 回放 | **已关闭（4）** | `OpDef` 增加 `since` / `changed`（revision 取算子库版本序号：既有 = 1、第 1 批 = 9、第 3 批 = 11）；`LibraryRef` 增加 `op_revisions: BTreeMap<String,(u32,u32)>`（`#[serde(default)]`：老 payload 解成空表 ⇒ 回放路径合法）。装配期由 `ze_pod_func::collect_op_revisions`（实现在函子库内，算子身份与 revision 同源）把**每条绑定实际用到的值算子**（含 predicate）写进产物；加载期 `zela-playback::build_bundle_runtime` 在 `bundle.script()` 之后逐个比对，不符 ⇒ `check_op_revisions` **指出算子**（不是笼统的库版本不符），也不整库作废。验收：`op_revisions_follow_the_batches`、`stale_op_revision_names_the_operator`（改 `changed` / 删算子都指出）。 |

| AK-05 | **逐脚本迁移 + 旧/新帧哈希对拍（第 5a 批）** | 脚本 / 表达式 / 校验 | **已关闭（5）** | 15 个脚本改为 `expr.bind` + 显式 clock + predicate 区间：`smoke` / `atlas` / `fusion` / `storyboard` / `capability_gaps` / `demo3d` / `pendulum` / `spring` / `orbit` / `fourier` / `pbr_lights` / `pbr_materials` / `pbr_scene` / `cap_channels2d` / `cap_channels3d` / `expr_read`。映射口径：旧 `scene.animate` 是**顺序段** ⇒ `clock.shift(start)` + `in_segment(dur)`（段外由 `Hold` 求值 `last_active`，与旧 `eval_local` 的段外钳制同值）；`parallel` 同窗口；`wait` 只推进游标。**新增 `round_f32`**（`zela.math@11 → @12`）：旧曲线每步都在 f32 上取整，而表达式层是 f64 ⇒ `Sin` / `Phase` / `Orbit2D` 的迁移必须在同位置显式落位，否则差 1 ulp（pendulum 第 16 帧、spring 第 159 帧实测会翻 1 像素）。`mix` 的**端点返回原值**（旧 `Keyframes` 的端点早退），避免 `a+(b−a)×1` 的舍入。**对拍**：36 条序列哈希全绿（逐位不变）；非基线作品用「旧脚本 vs 新脚本」序列哈希比对（`expr_read` 去掉 always-on 读者后比 writer —— 两者相同）。**Python 绑定修复**：`zela.math` 的**类型 home 包**曾在 `register_types` 里覆盖服务模块（`bind` / `pure` 全部丢失）⇒ 同命名空间时复用服务模块；`clock` 同时挂到 `zela` 与 `zela.math`。**偏差**：`Framing2D` / `MorphShape` 是存储组件、没有脚本侧常量表 ⇒ 用显式 `Channel { namespace, name, ty }` 构造键（记录为后续项）。**未做**：老 API 未删、基线未重录（顺序铁律：留到 5b）。 |

**AK 段小计**：已关闭 5（AK-01…AK-05）· 待处理 0（第 5b 批尚未开始）。

---

## AL. 计算内核独立化批次记账（ze-type / ze-func 切分，2026-10-26/27）

> 计划《表达式层 S0–S4》。口径：**先证明等价、再删**；36 条像素基线逐位不变。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AL-01 | **阶段 0：切分 ze-comb 与 ze-func** | kernel | **已关闭（0）** | 表达式内核（`ze-comb`，只 serde + thiserror）与基础函子库（`ze-func`：算子语义 + 元数据）分离；两者均过 wasm gate。 |
| AL-02 | **阶段 1：类型地基（I64 / Layout / Path）** | kernel | **已关闭（1）** | `I64` 离散索引、`Layout` 注册、`zela.Path` 迁出 `Value`；36 条基线不变。 |
| AL-03 | **阶段 2：多维连续变量与 pointwise 传播** | kernel | **已关闭（2）** | `Vec2` / `Vec3` 通道、区间代数、`PointwiseReport` 的污染源传播。 |
| AL-04 | **阶段 3：离散层 NatRec + 显式 Cache** | kernel | **已关闭（3）** | `NatRec` 求值 + 可序列化缓存 + 递归/环区分；上界进产物。 |
| AL-05 | **阶段 4：跨语言 fixture 门禁** | kernel / 三语言 | **已关闭（4）** | `ze-func/tests/fixtures` 跨语言对拍；符号化简与 targets 占位。 |

**AL 段小计**：已关闭 5（AL-01…AL-05）。

---

## AM. 统一解析协议批次记账（ze-type / ze-reflect，2026-10-27）

> 计划《ze-type / ze-reflect 执行计划》。核心：把可序列化的慢形式一次性解析成进程内快形式，用 generation 保证不失效。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AM-01 | **阶段 0：代数层迁至 ze-type** | kernel | **已关闭（0）** | `typespec.rs` 的十一个类型迁入 [`ze-type`](crates/kernel/ze-type/src/lib.rs)（`no_std + alloc`）；`ze-pod-core` 只 re-export，`ErasedFn` 留在 pod 层（运行时绑定入口，避免反向边）。 |
| AM-02 | **阶段 1：合并类型描述** | kernel | **已关闭（1）** | `Layout` 收进 `TypeSpec::Struct.layout`、`Array { elem, len }`；`ChannelType` 降级为 `TypeSpec` 视图（`from_type_spec`）；同名不同布局 = 冲突。 |
| AM-03 | **阶段 2：冲突检测统一** | kernel | **已关闭（2）** | [`ReflectError`](crates/kernel/ze-type/src/conflict.rs) 收敛重名 / 类型 / 布局 / 版本 / 能力五类诊断并带污染源；`ExprError::DuplicateId` / `PassRegistryError::Duplicate` / `TypeRegistry::register` 全部改走它。 |
| AM-04 | **阶段 3：符号表与句柄统一** | kernel / pod / scene | **已关闭（3）** | `HandleCore` + `SlotTable`（`HandleTable` 变薄包装）；`FunctionId` 补 owner + generation；`OPS` / `SIGNATURES` / `TARGETS` 装配期解析一次、查询 O(1)。 |
| AM-05 | **阶段 4：单态化上界 + Codec 归位** | kernel | **已关闭（4）** | `ze-type::r#gen` 按需单态化（`MAX_INSTANCES`）、`ze-reflect::MonoCache`、`Codec` 给慢路径正式名分；基准见 `ze-func/benches/lookup_bench.rs` 等。 |

**AM 段小计**：已关闭 5（AM-01…AM-05）。

---

## AN. 图算法独立化批次（ze-graph-core，2026-10-27）

> 计划《图算法独立执行计划》。纪律：每阶段可测试；阶段 0 先还接口欠账，阶段 1 先修确定性。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AN-01 | **阶段 0：接口欠账 + crate 能力表 + 重名门禁** | kernel / pod / scene / xtask | **已关闭（0）** | `OpDef` 合并为单一目录（`ze-func::OP_DEFS`，签名 + pointwise/targets）；Pod 侧 `ChannelSpec → ChannelDecl`；`seek_kind` 纳入离散层（`CompiledExpr::requires_replay`）；本文件补 AL/AM/AN 与 crate 能力表；xtask 新增"新 crate 必须登记"与"跨 crate 同名 `pub struct/enum` 白名单"两道门禁。 |
| AN-02 | **阶段 1：ze-graph-core 纯拓扑层 + 确定性收敛** | kernel | **已关闭（1）** | 新建 [`ze-graph-core`](crates/kernel/ze-graph-core/src/lib.rs)：`Topology` / `topo_sort` / `strongly_connected` / `find_cycle` / `reachable` / `dirty_downstream`，全部输出有序（`BTree*`，同一张图两次逐位相同）。`DirtyTracker::collect_dirty` 由 `HashSet` 改 `Vec`（升序）；`ze-graph-algo` 的 Kahn / Tarjan / BFS 全部改 `BTree*` 并补确定性用例。基准 `graph_bench`（quick，128/1024/8192）：topo_sort 22 µs / 332 µs / 4.0 ms，SCC 45 µs / 708 µs / 9.0 ms，reachable 9.2 µs / 106 µs / 1.35 ms。 |
| AN-03 | **阶段 2：NodeMeta 拆家 + Node&lt;D&gt; 泛型化 + socket 外置** | graph / ui | **部分关闭（2）** | ① `NodeMeta` 只留 `{ type_name, version }`；展示数据改为 `NodePresentation`（`display_name` + `NodeInteraction`），由 `zela-node-editor::PresentationTable` 按 `type_name` 持有，UI 只经 `Editor::presentation_of` 读取；core trait 上不再有 UI 字段。② `Graph` 新增 `input_count` / `output_count`（默认读节点 socket 表，可被注册表实现覆盖），算法可不再直接摸 `Node`。③ **偏差（记账）**：`Node<D = Erased>` 泛型化与"节点不再持有 socket 表"未做——它要求所有节点 impl 携带 data 字段与 socket 注册表，牵动 1807 行 trait/用例与 schema 宏；本批先把 UI 字段与算法入口解耦，泛型化留待 AN-04 之后单独一批。 |
| AN-04 | **阶段 3：算法降层 + eval 切换 + 7 处环检测收敛** | kernel / graph / ui | **部分关闭（3）** | ① `ze-graph-algo` 新增 `GraphTopology` 适配器（只经 `nodes/successors/predecessors`，socket 计数走 `Graph::input_count/output_count`）；`KahnSorter` / `TarjanSccDetector` / `BfsDirtyTracker` 全部委托 `ze-graph-core`（不再直接摸 `Node`）。`zela-node-editor::ConnectionValidator::check_no_cycle` 因此自动走同一实现。② **偏差（记账）**：其余 5 处环检测仍是手写遍历——`ze-comb::expr`（字符串 `Ref` id）、`ze-time::clock`（`ClockId` 父子）、`ze-scene-core::surface` 与 `zela-scene-ecs::transform_hierarchy`（`ObjectId`/`Entity`）、`ze-pod-core::broker`（节点下标）。它们的 id 空间与语义各自不同，收敛需要先统一 id 投影；本批只登记，不硬改。 |
| AN-05 | **阶段 4：TypeTag / NodeValue 收敛到 ze-type** | kernel / graph | **部分关闭（4）** | ① `TypeTag` / `NamedTypeTag` / `TypeCast` / `DefaultTypeCast` / `CastError` 迁入 [`ze-type::tag`](crates/kernel/ze-type/src/tag.rs)（`no_std + alloc`，手写 `Display`/`Error`，无 thiserror）；`ze-node-core::type_tag` 只 re-export，路径不变。② **偏差（记账）**：`NodeValue` 未迁（它带运行时值容器，属于 `ze-reflect` 的语义，不应进纯代数层）；`clone_box` 5→1 未做——五个 trait（`Component` / `EdgeData` / `DirtyFn` / `TypeTag` / `NodeValue`）各自返回不同的 `Box<dyn Trait>`，没有安全且无依赖的统一实现；收敛需要引入 `dyn-clone` 一类依赖或 trait upcasting + 类型擦除设计，属于独立决策，本批不硬做。 |

**AN 段小计**：已关闭 2（AN-01、AN-02）· 部分关闭 3（AN-03、AN-04、AN-05）· 未执行 0。

---

## AO. 图基础设施收敛批次（遍历器抽象与环检测收敛，2026-10-28）

> 计划《图基础设施收敛执行计划》。顺序约定：遍历器先于环检测（统一 id 投影是遍历的副产品）。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AO-01 | **阶段 0：记账归位 + 顶层文档** | kernel / 文档 | **已关闭（0）** | ① `NodeInteraction` 仍定义在 `ze-node-core::presentation`（schema 宏与 UI 共用），本批只离开 trait 签名——已知偏差，不进一步拆；② 重名白名单精确为 **37 条**（`Curve` 已登记，是已声明的债）；③ AN-04 第 5 处定位为 `zela_scene_ecs::transform_hierarchy::world_of`（`TransformHierarchyError(Option<String>)`，`visiting: HashSet<ObjectId>`）；④ 根 `README.md` / `ARCHITECTURE.md` 补 `ze-type` / `ze-reflect` / `ze-graph-core`。 |
| AO-02 | **阶段 1：遍历器抽象 + Expr 图视图** | kernel | **已关闭（1）** | `ze-graph-core::visitor` 提供 `Visitor` / `traverse` / `preorder` / `postorder`（确定性：根与后继按 id 升序、共享节点只访问一次、`enter` 可剪枝、无 HashMap）；`ze_comb::ExprGraphView` 实现 `Topology`：`Def` 算一个节点、`Ref` 算一条边（W-C），字符串 id 只用于解析 Ref，投影副产品含 `definitions` / `duplicate_ids` / `unresolved_refs`。生产边新增 `ze-comb → ze-graph-core`。单测覆盖共享 Def、后序、确定性、重复 id、悬空 Ref。**W-B 偏差（诚实）**：payload 的 `Def` / `Ref` 仍是字符串 id（未改序列化格式）；"整数槽位"只存在于图视图内部（`BTreeMap<&str, usize>` 投影）。 |
| AO-03 | **阶段 2：环检测收敛（完整路径）** | kernel / scene / pod | **已关闭（2）** | 5 处全部给出**完整环路径**：① `ze-comb::expr::check_cycles` 改走 `ExprGraphView` + `ze_graph_core::find_cycle`，`ExprError::Cycle(Vec<Id>)`；② `ze-pod-core::broker::topological_order` 删手写 DFS，改 `AssemblyGraph: Topology` + `topo_sort`/`find_cycle`（`BrokerError::Cycle { chain }` 首尾相同），生产边新增 `ze-pod-core → ze-graph-core`；③ `ze-scene-core::surface::resolve_surface` 完整链 `ParentCycle { chain }`；④ `zela_scene_ecs::transform_hierarchy::world_of` `visiting: Vec`，错误串带完整链；⑤ `ze-time::clock::set_parent` `CycleDetected { chain }`（自环 = `[a, a]`）。**偏差（诚实）**：③④⑤ 是「沿 parent 链的单点增量检查」，直接走上溯链即天然拿到完整路径，不建全图、不调 `find_cycle`；只有 ①② 是图算法接入点。 |
| AO-04 | **阶段 3：pass 迁移到遍历器（11 处）** | kernel / scene / pod | **部分关闭（3）** | 遍历骨架收敛到两处：`Expr::walk`（前序 `enter` 可剪枝 + 后序 `leave`，带词法父节点）与 `Expr::map_children`；节点自省收敛到 `Node::{channel,read,literal,call,rec_leaf}` + `Literal::as_real`。已迁移 7 处：`expr` 校验（`collect_defs` / `check_dangling` / `definitions` / `check_recursive_leaves`）、`simplify` 重写（子节点递归改 `map_children`）、`timeline::rewrite_reads`、`scene_script` 两处、`predicate` 两处、`expr_pod::validate_binding`。**剩余 4 处 `match &expr.node` 是派发型解释器**（`program::compile_node`、`reference::eval_node`、`simplify::eval_closed`、`infer::type_of`）：它们有短路 `Select`、memo、词法作用域，图/树遍历器无法表达而没有语义风险 ⇒ 保留 `match`，本批**未归零**（诚实偏差）。 |
| AO-05 | **阶段 4：新能力 + HashMap 门禁** | kernel / xtask | **已关闭（4）** | ① `ze_comb::analysis` 把 `ze-graph-core` 三能力接到表达式 DAG：`shared_definitions`（入度 → 共享 Def 的消费者数）、`affected_consumers`（反向图 `dirty_downstream` → 谁要重算）、`unreachable`（`reachable` 补集）、`reference_groups`（SCC）；各一个不依赖渲染的 fixture（单元测试内联表达式）。② xtask `structure` 新增门禁 `check_no_hashmap_in_graph_paths`：表达式图与拓扑路径（ze-graph-core / ze-comb / simplify / scene_script / predicate / timeline / expr_pod / infer）禁用 `HashMap` / `HashSet`，注释行豁免；`infer` 的 `memo` / `visiting` 已先迁 `BTreeMap` / `BTreeSet`。③ 基准：`ze-graph-core/benches/graph_bench.rs`（新增 `traverse/preorder`、`traverse/postorder`）、`ze-comb/benches/expr_bench.rs`（`walk` / `map_children` / `graph_view` / `shared` / `affected` / `validate`）、`ze-comb/benches/eval_bench.rs`（`compile` / `lowered` / `reference` / `shared` / `nat_rec` 冷启动 vs 缓存续算）、`ze-pod-core/benches/assembly_bench.rs`（`validate` / 环检测）。 |

### AO 遍历点清单（阶段 3 的迁移清单与结果）

| # | 位置（符号名） | 用途 | 阶段 3 结果 |
|---|---|---|---|
| 1 | `ze_comb::expr::Expr::validate` | 结构校验遍历 | 已迁移（`Expr::walk`） |
| 2 | `ze_comb::program::compile_node` | lowering 派发 | 派发型解释器，保留 `match` |
| 3 | `ze_comb::reference` 参考求值器 | 参考求值 | 派发型解释器，保留 `match` |
| 4 | `ze_func::simplify`（transform） | 化简重写 | 已迁移（`map_children` + 本节点折叠） |
| 5 | `ze_func::simplify`（eval） | 化简求值 | 派发型解释器，保留 `match` |
| 6 | `ze_scene_core::animation::predicate` 区间代数（分析） | 区间代数 | 已迁移（`Node` 访问器） |
| 7 | `ze_scene_core::animation::predicate` 区间代数（求值） | 区间代数 | 已迁移（`Node` 访问器） |
| 8 | `ze_scene_core::animation::timeline` 扫描 | 时间轴扫描 | 已迁移（`map_children`） |
| 9 | `ze_scene_core::scene_script` 注入通道检查 | 装配期校验 | 已迁移（`Expr::walk`） |
| 10 | `ze_pod_func::expr_pod` 三语言注册 | 绑定生成 | 已迁移（`Expr::walk`） |
| 11 | `ze_pod_func::infer` 类型推导 | L2 推导 | 派发型解释器，保留 `match` |

**AO 段小计**：已关闭 4（AO-01、AO-02、AO-03、AO-05）· 部分关闭 1（AO-04）· 未执行 0。

---

## AP. 作者侧兑现批次（5b 退役 + 时钟身份化 + 方法/Query/Bounds，2026-10-28）

> 计划《作者侧兑现执行计划_5b退役与时钟身份化》。顺序约定：先证明等价、后重录基线；
> A（纯减法）先于 B（作者侧兑现）。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AP-01 | **阶段 0：记账归位** | scene / xtask / fixtures | **已关闭（0）** | ① `TransformHierarchyError(Option<String>)` → `{ chain: Option<Vec<ObjectId>> }`（与其余四处环诊断同口径，`message()` 只留调试用）；② 该文件的 `locals` / `parents` / `out` 从 `HashMap` 改 `BTreeMap`，并把 `crates/scene/zela-scene-ecs/src/transform_hierarchy.rs` 纳入确定性门禁（`WorldTransforms::from_map` 改收 `IntoIterator`）——从"注释保证"变成"机器保证"；③ **W5 实测**：Rust `f64::sin` 与 JS `Math.sin` 在 6283 个采样点上有 **160 处差 1 ulp**（cos 195 处），首个反例 `x = 0.117`；新增 fixture `transcendental.json`（`cross_language: false`），JS/Lua runner 显式跳过并回报跳过数，Rust harness 照常守护；④ 累计项 AN-03 / AN-05 / AO-04 / W-B 保持原状态（不在本批做）。 |

| AP-02 | **子阶段 A1：删脚本侧旧动画 API** | scene-api / app / xtask | **已关闭（A1）** | `SceneApi` 删除 `clip` / `clip_transform` / `animate` / `parallel` / `wait` 五个字段（`zela.scene` 11 → **12**，指纹重钉 `133_947_965_974_789_799`）；`scene_service` 删除 `Clip` / `ClipTarget` / `clips` 句柄表与五个注册/实现；`capability_exemptions.txt` 的 5 条 `scene:*` 一并清空（coverage 56/68 + 豁免 12）；仅存的两条使用方测试（`python_keyframes_curve_matches_lua` / `js_and_lua_produce_the_same_scene_script`）随 API 删除。Rust 侧 `SceneEncoder` / `Timeline` 旧接口与 `Name` 组件保留到 A3（脚本已不调用）。 |

| AP-03 | **子阶段 A2：删双 `Curve`** | scene-api / xtask | **已关闭（A2）** | 删除 `ze-scene-api::Curve`（脚本侧曲线枚举）及其 `to_native`、两条映射测试；`xtask` 能力抽取去掉 `Curve` 类别（`collect_capabilities` 不再要求该种类非空）；豁免再清 7 条 `Curve:*`（现 5 条：`TransformChannel` ×3 + `Target` ×2）。**全仓 `pub enum Curve` 只剩 `ze-time/src/curve.rs` 一处**（`Timeline`/`SceneEncoder` 一直用的是它，不受影响）。 |

| AP-04 | **子阶段 A3：退役顺序编排** | scene-core / app | **已关闭（A3）** | `Timeline` 删除 `play` / `parallel` / `wait` / `play_pending` / `parallel_pending` 与 `Step::Play` / `Step::Wait`、`Action`、`ActionTarget::Ready` —— 只剩 `Step::Expr`（`PendingTarget` 是唯一可持久化形态）；`SceneEncoder` 删除 `animate*` / `parallel*` / `wait`，`SceneScript::append` 的时间接续改为**在时钟链最外层前插 `shift`**（不再有顺序游标，`append` 的 O(1) 语义保住）；硬编码分镜与能力 demo 改用表达式绑定（顺序 = `clock.shift`、并行 = 共用同一时钟、窗口 = `[0, dur)`），与 Lua 版逐项同构；同步重写 5 个受影响的测试文件。**36/36 像素基线逐位不变**。 |

| AP-05 | **子阶段 A4：清 AK-05 豁免、收窄能力表面** | scene-api / xtask | **已关闭（A4）** | 删除脚本侧 Pod 类型 `zela.TransformChannel`（A1 后无入口）；能力抽取不再从 `types.rs` 抽动画枚举，复合目标（`Camera2DFraming` / `RevealRange`）只剩旧 payload 解码用途 ⇒ `collect_targets` 跳过 `is_composite`；`capability_exemptions.txt` **清空**（AK-05 九条全部兑现），同步删掉 `TransformChannel` 的跨 crate 重名白名单与相关夹具。coverage **55/55、0 豁免**。 |

| AP-06 | **子阶段 B1：时钟身份化 + 窗口函子** | ze-comb / pod-func / ecs / scripts | **已关闭（B1）** | `ChannelRef` 增加 `clock: Option<ClockShape>`（随 payload 走，`Eq`/`Hash` 按 bits 纳入身份）；`SceneMono` 同时持局部与全局时间，`c.time()` 按叶子时钟链在**全局时间**上反算（不再"时间只有一个数"）；新增 `clock.time_of(c)` / `clock.range(lo,hi)` / `clock.range_of(c,lo,hi)`，删除 `expr.time()`（`zela.math` 12 → 13，指纹 `13_878_659_082_635_502_658`）；强制约束：predicate 只允许**绑定时钟**，`clock.time_of` 出现在 predicate 里装配期显式拒绝；12 个脚本的 `in_segment` helper 全部收敛为 `Predicate.Of { expr = clock.range(…) }`。**未决落地**：W-B 不做 `play` 糖（保留 `clock.range`）、W-C 不提供 `range_inclusive`。 |

| AP-07 | **子阶段 B2：`MethodSpec` 取代只读通道** | ze-comb / scene-core / ecs / pod-func | **已关闭（B2）** | `ze-comb` 新增 `MethodRef` / `Node::Method` / `Step::Method` / `Mono::call_method`（默认 `EvalError::MissingMethod`：未注册方法**显式失败**）；`ze-scene-core` 新增 `MethodSpec { params, ret, reads, eval }` 与 `METHODS`，`Reveal.current_pos` 改为方法（`reads = Reveal.end`）；**删除** `TargetError::ReadOnly` / `READ_ONLY_SPECS` / `read_only_entry` 与 4 条特殊路径，`channels()` 不再导出派生量；装配期未知方法 / 参数个数不符 ⇒ `ExprBindError::UnknownMethod`，`method.object` 与 `Read.object` 同一处句柄重写，`reads` 进 `check_read_order`；`zela.math@15` 新增 `method`，`infer` 用 `METHOD_SIGNATURES` 定型（与 `METHODS` 逐条一致性有专门用例守）。 |

| AP-08 | **子阶段 B3：Bounds（方法的第一个应用）** | ze-scene-core | **部分关闭（B3）** | `Bounds2D` trait（`Circle` / `Path` / `Line` / `Arrow` / `Rect` / `Path2D`）与聚合入口 `object_bounds` 就位；**空/退化几何一律 `None`**（不拿零矩形冒充），聚合读取顺序固定（Circle → Path → Line → Arrow → Rect → Reveal），不依赖容器迭代序；`METHODS` 增加 `Object.bounds` / `Circle.bounds`（`reads` 静态声明）。**残余**：两者返回 `Rect2D`，在脚本边界要以 `Bytes + zela.Rect2D` 表示 —— 值系统收敛（W-A）之前**只有核心侧**可达，跨层一致性用例显式列出这两个"仅核心侧"方法。用例锁定：笔尖（`Reveal.current_pos`）与包围盒同一 `flatten` 采样 ⇒ 必落在盒内。 |

| AP-09 | **子阶段 B4：Query + `it` + `exists`** | pod-func / app | **已关闭（B4）** | `bind_each` 在**装配期**展开成 N 条具体绑定：结果集按 `ObjectId` 升序（脚本句柄下标 = create 序 ⇒ 天然稳定，不碰任何 `HashMap`），产物里**既没有 `it` 也没有 query**；`it` 的形态是 `expr.query_self(channel)`（装配期哨兵 `u64::MAX`，无闭包、无 `where` 对象 ⇒ 不违反禁闭包纪律）；过滤条件只认**组件类型组合**（`requires` / `excludes`），未知组件名装配期显式失败；展开数 **N 可见**（`N = 0` 打印但不报错，与 `Sum{n=0}` 同类）；`exists` 为 `Node::Exists` + **装配期常量折叠**成 `Bool` —— `predicate::analyze` 同时支持"依赖时间（区间代数）"与"完全静态（常量）"两类，折叠先做；漏折到求值期是 `EvalError::UnfoldedExists`（不默默算错）。字段名用 `requires`/`excludes` 而非 `with`/`without`（`with` 是 Python 关键字，同 `Select.otherwise` 之于 `else`）。`zela.math@16→@17`，指纹 `4_840_136_137_552_424_654`。验收用例：两次装配逐位一致、`N = 0`、`exists` 折叠为常量（含未知组件名报错）。 |

| AP-10 | **子阶段 B5：`NatRec` 三语言入口** | pod-func | **已关闭（B5）** | `zela.math@14` 新增 `nat_rec` / `rec_prev` / `rec_index`（Lua / JS / Python 同一张签名表）；`count` 过 f64 边界但在装配期**强制非负整数**（非整数显式报错，强制作者写 `Floor` —— 刀刃取整）；`step` 用 `rec_prev()` / `rec_index()` 表达，避免跨 Pod 回调；Lua 用例锁定产物形状（`node type = nat_rec`，`count` 是 `i64` 字面量）；`ze-scene-ecs` 用例锁定：含 `NatRec` 的绑定 `seek_kind == Replay`。 |

| AP-11 | **W5 明确记账：超越函数跨端一致性不转绿** | ze-func / 代码生成 | **仍成立（记账）** | 阶段 0 已让 fixture `transcendental.json`（`cross_language: false`）变红并留档：Rust `f64::sin` 与 JS `Math.sin` 在 6283 个采样点上 160 处差 1 ulp（`cos` 195 处，首个反例 `x = 0.117`）。本批**不转绿**：两条路线（① 自带可移植算子实现 ② 限制可代码生成的算子集，见 W-E）都需要独立批次的设计与验证成本，且现有 7 个 fixture 无一依赖超越函数。⇒ 保持 `cross_language: false`，是**已声明的债**而不是漏抓；代码生成路线开工前必须先收掉它。 |

| AP-12 | **通道常量挂错组件（无提示丢失）** | scene-core / scene-api / 三语言绑定 / scripts | **已关闭（收尾）** | 反例：`Framing2D.center` / `MorphShape.t` 的 `FieldSpec.component` 是**存储组件**，而脚本世界没有 `Framing2D` / `MorphShape` 这两个名字 ⇒ 三种绑定的 `attach_channels` 都走 `continue`（**无提示**丢掉常量），脚本只能手写 `Channel { namespace, name, ty }` 字符串。修复：`target::channel_home` 把存储组件映射回脚本组件（`Framing2D → Camera2D`、`MorphShape → Morph`，**地址与 payload 不变**），`channels()` 用它做常量归属；新增门禁用例 `every_channel_constant_hangs_on_a_script_component`（常量必须挂在 `SceneApi` 的构造器上，否则将来再挂错就 red）；fourier / storyboard / spring / cap_channels2d / capability_gaps 与 fourier.py / storyboard.py 全部改用 `Camera2D.channel.scale|center` / `Morph.channel.t`（payload 逐字节不变，五件作品实测 hash 与 manifest 一致）。 |

| AP-13 | **方法没有作者侧调用形态**（只能手写 `expr.method { … }`） | ze-pod-core / 三语言绑定 / 声明文件 / scripts | **已关闭（收尾）** | 反例：`Reveal.current_pos(curve)` 这种最自然的写法**不可用** —— 方法只能写成 `expr.method { name = "Reveal.current_pos", object = curve, args = {} }`（表名 / 句柄 / 空 `args` 三处都靠手写）。修复：`Pod::methods()` 自述面（`MethodDecl { component, name, qualified, arity, ret }`，与 `channels()` 同构）+ `ze-scene-api::methods()`（只导出返回值有表达式层表示的方法：`Object.bounds` / `Circle.bounds` 是 `Rect2D`，等 W-A）；Lua / JS / Py 三种绑定把方法挂到组件构造器上（调用时组 `MethodCallSpec` 走 `expr.method`，未注册方法仍由装配期拒绝），声明文件同步出 `current_pos`（`fun(object: Handle): Expr<Vec2>` / `Callable[[Handle], Expr[Vec2]]`）。等价性证据：`sine_trace.lua` 改用 `Reveal.current_pos(curve)` 后 payload 仍为 `771dc4cf…`（与 `expr.method` 逐字节相同）。 |

**AP 段小计**：已关闭 11（AP-01..AP-07、AP-09、AP-10、AP-12、AP-13）· 部分关闭 1（AP-08）· 记账 1（AP-11）· 未执行 0。

---

## AQ. 可观测性与性能基准批次（结构指纹 + 统一门禁，2026-10-28）

> 计划《可观测性与性能基准执行计划_结构指纹与统一门禁》。
> 核心设计：**一套框架，两个比对器** —— 结构指纹是**等式**（逐位相等），性能是**不等式**（20% 警告 / 50% 失败）。
> 合并的是工具，不是判据：把性能硬做成「逐位一致」会让 CI 天天红然后被关掉。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AQ-01 | **阶段 0：砍纯浪费** —— 每帧无条件 PNG 编码 + 写盘，只有第 0 帧被读 | zela-verify | **已关闭（0）** | `render_png` 回调只在 `index == 0` 时 `write_png` + 读回；`run_sink_bypass` 的 V3-12 判据只比较首帧字节，899/900 次编码结果无人读取。像素基线不动（36/36 逐位不变），证明砍的是纯浪费。 |
| AQ-02 | **W-B：`WorldSnapshot` 的 `HashMap`**（迭代序不确定 / Debug 截断 / 未注册时无提示降级） | zela-schedule telemetry | **已关闭（0）** | `TypeId` 不实现 `Ord`，注册表本身无法换 `BTreeMap` ⇒ 确定性固定在**输出边**：实体按 `id`、组件与 Resource 按 `type_name` 升序（排序键显式写死，`snapshot_json_is_deterministic_across_component_order` 锁定逐字节相同）。文件头改为显式声明 **UI 专用、不参与指纹线**；结构指纹另有纯数值入口（AQ-03）。 |
| AQ-03 | **阶段 1：结构指纹（主力回归网）** —— 没有「全量对象 × 全量字段」入口 | ze-scene-core / zela-scene-ecs / zela-app | **已关闭（0）** | `zela_scene_ecs::fingerprint`：`fingerprint_world(world, Option<&WorldTransforms>)` 枚举 `ObjectId` 升序 × `TARGETS` 全部 read 端（精确 f32 位）× `WorldTransforms`（父子累乘后 16×f32）；派生几何（`AnimValue::Path`）只写占位标记并计入 `derived_excluded`（强制约束 1）。每帧独立、不链式 ⇒ 首次能**定位到具体帧**：`assert_structure_matches` 在 script / bundle 之间逐帧比对并指出第一个分歧帧。`zela-verify` 打印 `[structure]` 行；xtask 与 `structures.txt` 做等式比对（`compare_structures`：wgpu / vector × script / bundle 必须同值）。 |
| AQ-04 | **阶段 2：像素层缩水（每作品 3–5 关键帧）** | zela-verify / baselines.txt | **未执行（3）** | 机制与验收都要求**重录 36 条像素基线**；`--record` 要求干净工作树 + 提交后 `--rebind`，本批工作树带着 AQ 改动 ⇒ 录制流程不可执行。**判据不变，也不悄悄放宽**：像素仍是全帧序列哈希，结构指纹已提供全量覆盖，等提交后单独一批做「旧值 → 关键帧集合」。 |
| AQ-05 | **阶段 3：统一观测产物与报告** —— 三种产物（结构 / 帧 / bench）各说各话 | zela-verify / xtask | **已关闭（0）** | 统一信封 `{"schema":"zela.observation/1","kind":"structure" 或 "frame",...}` 写入 `ZELA_OBSERVE`（hashes 步固定写 `target/xtask/hashes/observe.jsonl`，每次重写）；结构产物带逐帧摘要数组（可反推帧）。旧 `ZELA_FRAME_REPORT` 格式**逐字段保留**（已有工具在读）。bench 侧不重造框架：criterion 的 `target/criterion/**/new/estimates.json` 就是数据源（AQ-06）。 |
| AQ-06 | **阶段 4：双比对器与分档告警** | xtask | **已关闭（0）** | 等式：`compare_structures`（缺基线 / 跨后端不一致 / 与基线不符都 fail）。不等式：`compare_bench` + `BENCH_WARN_RATIO = 1.20` / `BENCH_FAIL_RATIO = 1.50`；新 step `cargo verify --step bench`（已进 full 列表）。**CI 只拦 >50%**；`>20%` 打印 `[bench][warn]` 不阻断。没跑过 `cargo bench` ⇒ 打印「**未测**，跳过（不是通过）」；产物存在但基线未登记 ⇒ 同样只打印不阻断（换机 / 新 bench 不该红）。基线 `bench_baselines.txt` 已按本机录制 **88 条**（单位 = 纳秒，criterion `estimates.json` 的 `median.point_estimate` 原生单位；`ZELA_RECORD_DEVICE` 记设备），不承诺跨设备。**结构指纹的 CI 边界与像素一致**：`--skip-baseline` 时只断言跨 pass / 跨后端一致（`script == bundle`、`wgpu == vector`），不比对 `structures.txt` —— 跨 OS libm 的 1 ulp 分歧尚未排除（与 AP-11 同类），不拿未经验证的判据去红 CI。 |
| AQ-07 | **阶段 5：覆盖率补洞** —— 改坏了不会红且没人知道 | scripts / xtask | **部分关闭（1）** | `scripts/dispositions.txt` + `check_dispositions`：30 个脚本 **23 有像素基线 + 7 显式豁免（理由必填）**，未登记 / 假声明 `baseline` / 空理由都 fail（`--step hashes` 恒开）。**残余**：cap_channels2d / cap_channels3d / expr_demo / expr_read / sine_trace 5 件 demo 只有 manifest payload 断言，补 `[vector]` 基线需一次录制设备运行（与 AQ-04 同一批）。 |
| AQ-08 | **阶段 0 记账**：9 个 bench 与 248 处 `println!`/`eprintln!` 登记 | 记账 | **已关闭（0）** | bench（9 个 / 7 crate）：`ze-comb` eval_bench + expr_bench、`ze-func` lookup_bench、`ze-graph-core` graph_bench、`ze-reflect` reflect_bench、`zela-schedule` scheduler_bench、`ze-pod-core` assembly_bench + function_lookup、`ze-pod-tests` pod_bench（criterion，已接 AQ-06）。`println!`/`eprintln!` 实测 **248** 处（计划写 221，按代码重算）；其中 `xtask` + `zela-app` 是 CLI 输出，本来就该给人读 ⇒ **不全改**，只治理「想被机器消费的那部分」（B12 起已有 `[decision]` / `[verify]` / `[timing]` / `[structure]` / `[observe]` 五种机器行）。`tracing::` 13 处 / 7 个文件，kernel 仍**零 tracing**（no_std 门禁守着，强制约束 3）。 |
| AQ-09 | **verify 默认必须覆盖 Python（pyo3）** —— 门禁固定 `--features typst`，`ze-pod-py` 的 `python` feature 长期没被编译 | xtask / ze-pod-py | **已关闭（0）** | 反例：AP-13 的 Python 方法绑定里 `PyCFunction::new_closure` 的 `&'static CStr` 签名与闭包生命周期错误，在 `--step test` 全绿的情况下存在了整整一批；只有手动打开默认 features 才暴露。修复：`check` / `test` 在无解释器时**默认失败**（`--allow-no-python` 才降级，且降级必须显式），`FeaturePlan` 的能力检测保留；同时修掉那两处绑定错误。`hashes` 仍固定 typst-only（CI 的 hashes job 不装 `python3-dev`，且像素基线就是该组合下录的）。本机解释器改用 uv 托管 CPython 3.13.11 + 用户级 `PYO3_PYTHON`。 |

**AQ 段小计**：已关闭 7（AQ-01、AQ-02、AQ-03、AQ-05、AQ-06、AQ-08、AQ-09）· 部分关闭 1（AQ-07）· 记账 0 · 未执行 1（AQ-04）· 已记录 0。

---

## AR. 场景侧组件与 API 专项治理 · 摸底记账（2026-10-28）

> 计划《场景侧组件与API专项治理_摸底计划》。本段只记**摸底结论与实验结局**，不记落地；
> 报告正文见仓库根 `场景侧组件与API专项治理_摸底报告.md`。
> 每条实验按计划 §0.3 标注结局：A 转正 / B 删除（负面结论）/ C 待核。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AR-01 | **P0-1 更正：`Reads`/`Writes` 只是 Resource 元组，组件级访问既未声明也不可声明** | zela-schedule / 全部 System | **记账（1）** | `zela-schedule/src/system.rs` 的 `System::Reads: ResourceTuple` / `Writes: ResourceTuple` 只展开成 `TypeId` 列表做 Resource 冲突检测；`ExecutorContext::borrow_world` / `borrow_world_mut`（`context.rs`）**不经过 `BorrowTracker`**。⇒ `MorphApplySystem` 的 `type Writes = ()` 与它写 `Path` 并不矛盾（组件写根本不进任何声明），`BindingApplySystem` 同理。「把声明改对」无法执行，真正动作是**新增组件级访问声明机制**——超出摸底批量级，转下一批（触发条件：需要静态判定"哪些组件被谁写"时，例如并行调度或结构指纹的写集推导）。 |
| AR-02 | **P0-2：上一轮"通道常量 / 方法作者侧命名"主题的 20 文件补记** | 记账 | **已关闭（0）** | 实测该主题落在两个提交：`4c364e0`（12 文件，通道常量挂到脚本组件）+ `0cf5b2c`（18 文件，`Pod::methods` + 三语言绑定），并集 **27 文件**（含 `GAPS.md` / `xtask`）。两者已分别由 **AP-12 / AP-13** 记账并给出等价性证据（payload 逐字节不变），无需在 AR 段重复；本行只补交叉引用。 |
| AR-03 | **P0-3：摸底参照基线（`*.explore-baseline`）** | scripts | **已关闭（0）** | 已复制 `crates/app/zela-app/scripts/structures.txt.explore-baseline` 与 `baselines.txt.explore-baseline`（SHA-256 与源文件一致：`EF8C6452…` / `D159E205…`）。实验前 `cargo verify --step coverage` = **55/55 使用、0 豁免**；实验后逐文件 `git checkout` 回滚，再跑 coverage（55/55）与 structure（双比对器 + 渲染指纹 + 能力指纹全绿）**逐位回到原状态**。 |
| AR-04 | **端点普查：动画不是四份实现，是"每帧变量归谁"的两族 + 四条专用通路** | scene-core / ecs / render | **记账（2）** | 六处实测：① `channel`（值在 `Curve`，写字段，30 条 `TARGETS`）；② `MorphShape{t}` + `MorphApplySystem`（CPU，Headless 用）；③ `MorphShape{t}` + `MorphRecordSystem`（GPU：双 vertex buffer + `t` uniform，App 用，`shape_2d_morph.wgsl` 的 `mix(from,to,t)`）；④ `Reveal{start,end}` + `RevealRecordSystem`（沿弧长的区间，专属队列）；⑤ `PhaseAnim.phase` + `FunctionGraphRefreshSystem`（`zela-scene-ecs/src/geometry.rs`，每帧重采样并**覆盖写 `Path`**）；⑥ `Curve::PathMorph`（值域已实现、单测覆盖、**零接线**）。另有两处**非声明式**控制流常被误计入动画：`CameraDriveSystem`（绑定在位/让位）与 `CameraInputSystem`+`FpsView`（跨帧累积）。**修正计划口径**：`record_group::<S,A,B>` 实测 **18 处**（9 Viewport + 9 Content），计划写 20。 |
| AR-05 | **「配置 / 游标」混装：3 个组件确认 + 另 3 类混装被计划漏掉** | scene-core | **记账（2）** | 确认 3 个：`PhaseAnim{omega=配置, phase=游标}`、`MorphShape{from/to/samples=配置, t=游标}`、`Reveal{path=配置, start/end=游标}`。计划的两分法漏掉三类：① **状态+配置**（`Camera3D{eye/target=状态, up/projection=配置}`）；② **累积状态**（`FpsView{yaw,pitch}`，不是 f(t)，见 AR-11）；③ **运行时字段**（`MeshGeometry.id` 是 GPU 缓存键，显式不进产物 W1-01）。⇒ 类型级标记若只做"配置/游标"两分，仍需第三、四类；**不建议**用命名约定，建议用「可写通道 = 只有游标与数据」这一条判据（与 `TARGETS` 现状一致：`from`/`to`/`path`/`func` 都不在表里）。 |
| AR-06 | **值系统三套：`TypeSpec` 7 变体 / `Value` 7 变体 / `AnimValue` 5 变体，且 `zela.Path` 在投影层缺口** | ze-type / ze-comb / ze-time | **记账（2）** | `ze-type/src/ty.rs` 的 `TypeSpec` = Scalar/Struct/Enum/Array/Opaque/**Var**/**App**（7）；`ze-comb/src/program.rs` 的 `Value` = F64/I64/Bool/Text/Vec2/Vec3/Bytes（7）；`ze-time/src/curve.rs` 的 `AnimValue` = Scalar/Vec2/Vec3/Color/Path（5）。缺口逐项：`AnimValue::Color` 在 `Value` 无对应（`anim_to_value` 显式报 `UnsupportedValue{kind:"color"}`）；`Value::I64` 在 `AnimValue` 无对应（`anim_value` 返回 `None`，即"不可动画"）；`Path` 已双向迁移（`Value::Bytes{ty:"zela.Path"}` ↔ `AnimValue::Path`）。**关键缺口**：`ChannelType::from_type_spec`（`ze-comb/src/expr.rs`）只认 `zela.Vec2`/`zela.Vec3`，`zela.Path` 落到 `_ => None` ⇒ 即使给 `Path` 加了通道，脚本侧类型投影与声明文件仍会拒绝它（`ChannelType::Bytes` 已存在，缺的是一条映射臂）。 |
| AR-07 | **E1 · `Path` 通道化：机制成立，但 `#[derive(Channels)]` 不可用，且单独转正会更糟** | ze-scene-macros / ze-scene-core | **记账（C 待核）** | 实测两步：① 给 `Path` 加 `#[channel(path: Path2D)]` ⇒ **编译期 E0609**（`no field 'path' on type &mut Path`）——宏的写体是 `c.#field = v`，只能寻址**可命名字段**（`Circle.radius` 靠 `DerefMut` 进内层），元组组件没有可命名字段。② 改手写 `FieldSpec`（`try_path()` → `c.0 = path.clone()`，`TypeSpec::Struct{name:"zela.Path", layout:None}`）⇒ 编译通过，单测 `path_channel_writes_the_whole_geometry` **通过**（写回整条几何 + 读回 + 类型不符显式报错）。改动量：宏 `MappedKind` 加一臂 ≈28 行、手写规格 + 表项 ≈30 行。**但**：新 Target 立刻撞 `cargo verify --step coverage`（`Target:Path.path 没有被任何 manifest 作品使用`），且引入结构指纹盲区（AR-08）⇒ 回滚代码，结论待核。触发条件：AR-06 的投影臂 + AR-08 的判据改造同批完成。 |
| AR-08 | **E1/E4 · 结构指纹按 `AnimValue::Path` 排除 ⇒ 一旦 `Path` 可动画，摘要对几何失明** | zela-scene-ecs | **记账（C 待核）** | 实测单测：同一世界两条半宽 1.0 / 3.0 的闭合路径，`derived_excluded == 1` 且 **`digest` 完全相同**（`path_read_endpoint_makes_geometry_invisible_to_the_digest`）。根因在 `fingerprint.rs::hash_anim_value` 的 `AnimValue::Path(_) => { byte(b'p'); 1 }`：它排除的是**变体**，不是"来源"。⇒ 判据必须改成「按来源排除派生几何」（作者给的静态 `Path` 进摘要、由 `flatten`/字形产出的不进）或给读端加"来源"标记；否则 Path 通道化会让回归网在几何层面留洞，只剩像素基线兜底。这条与 AQ-03 的跨设备承诺直接冲突，**必须与 E1 同批**。改名/加标记/加来源字段三条路线的成本本批未实测（未执行部分）。 |
| AR-09 | **E2 · `PathMorph` 替代 `MorphShape`：值层逐位等价，像素层不成立** | ze-time / zela-scene-ecs / render-backends | **已关闭（B 负面结论）** | 值层实测（单测 `path_morph_curve_equals_the_cpu_fallback_bit_for_bit`）：`Curve::PathMorph` 与 `MorphShape + MorphApplySystem` 在 t ∈ {0, .25, .37, .5, .93, 1} 上产出**逐位相同**的 `Path2D`（同源于 `Path2D::morph_points` + `from_polyline(.., true)`）。像素层**必然不同**：App 走 GPU 形变（`MorphRecordSystem` → 双 buffer + `mix(from,to,t)`，几何帧间不变、零上传），CPU 路径是「每帧形变 → 重新三角化 → 重新上传」；`MorphApplySystem` 之所以在 App 里不注册（`morph.rs` 头注释）正是为此。⇒ 「用 `PathMorph` 消灭一份实现且像素逐位一致」**不成立**；等价替换会用**最热路径的性能**换机制统一。结论：端点可以进值域，但**必须保留一条"端点静态 ⇒ 渲染层降级为 GPU 形变"的路径**，否则是负优化。 |
| AR-10 | **E3 · "morph = A、B 都不显示、显示差值" 命题前提与现状不符** | scene-core / zela-render | **未执行（C 待核）** | 实测三条：① 现状 morph 的 A/B **不是显示对象**——`from`/`to` 是 `MorphShape` 的**字段**，作者写 `Morph{from,to}` 内联两条几何，场上只有一个对象 ⇒ "disable A + disable B + 第三条绑定"这条组合没有可作用的 A/B。② `Visibility{tags: Vec<VisibilityTag>}` 是**并集语义**且 `Global` tag 恒命中，无法承载"布尔开关"（`Vec` 也不在 `#[channel]` 类型白名单里）。③ `Reveal` 是**沿弧长**的参数区间（`start/end ∈ [0,1]`），不是布尔可见性；一条 bool 通道表达不了 `0..0.37`。⇒ 用户命题作为"morph 的实现机制"不成立；但 `Visible(bool)` 作为**独立的作者能力**（时间轴显隐）仍值得做：新组件 + `bool` 通道（宏已支持 bool）+ `visible_for` 单一闸口 4 行；注意 `Light` / `Arc<MeshGeometry>` 两条录制通路不经过 `visible_for`。本批未写代码（渲染层改动 + 像素影响需另批验证）。 |
| AR-11 | **E5 · 累积动画：边界由现成反例证明，作者侧接入未执行** | zela-scene-ecs / ze-comb | **未执行（C 待核）** | 反例在仓内已存在：`CameraInputSystem` 用 `FpsView{yaw,pitch}` **跨帧累加**（`crates/scene/zela-scene-ecs/src/camera_input.rs` 的 `apply_fps` + 回写组件），它既不是 `f(t)` 也进不了 `BindingTable`；`CameraDriveSystem` 只能靠"把绑定移出表"来让位。表达式层 `NatRec`/`rec_prev` 三语言入口已就位（AP-10），但 **30 个脚本零使用**（实测 grep 无引用），且 `seek_kind` 推导会把含 `NatRec` 的绑定记为 `Replay`。⇒ 「绑定表表达一切」的边界就是**有记忆的演化**：它需要"状态 + 步进"两个原语，绑定表只有"值 + 时间"。把 `nat_rec` 接到作者侧（做一个"拖拽后继续漫游"）本批未做——它同时要回答与 `CameraDriveSystem` 让位机制的冲突，属独立批次。 |
| AR-12 | **E6 · `{STRUCT}_CHANNELS` 消费：假设"拼接会打断门禁"不成立，真正的债换成了手工文件清单** | xtask / ze-scene-macros | **已关闭（B 负面结论）** | 实测：① 16 条 `#[channel]` **全部**已在 `TARGETS`（30 项里 16 项派生 + 14 项手写，其中 2 项复合），拼接今天**语义等价**、零行为变化。② 门禁**不会**因拼接失效：能力抽取有两条独立路径——`collect_targets`（解析 `target.rs` 源码的 `scalar_spec!` / `FieldSpec` 字面量）与 `collect_channel_capabilities`（解析各组件文件里的 `#[channel(...)]`），后者不依赖 `target.rs`。③ 真正的债：`xtask/src/main.rs::step_coverage` 里 `channel_files` 是**手工 4 文件**清单（`object/primitives.rs`、`object/camera.rs`、`components.rs`、`components/3d.rs`）⇒ 新组件把 `#[channel]` 写在别的文件里，覆盖门禁**无提示漏检**（"漏了不红"）。修法：改成扫描 `crates/**/*.rs`（与 `collect_rs_files` 同源），成本小、收益是消掉这条无提示失败。 |
| AR-13 | **E7 / F-2 · 加组件 9 处：实测分类** | 全仓 | **未执行（3）** | 未走完"加一个完整组件"（建议的 `Disable` 与 E3 复用，本批未落地）。已用 E1 实测**加 Target** 这条路径：定义 1 处 + `TARGETS` 1 处即可，**只撞 coverage 1 处门禁**（报错指出 `target.rs:613`），脚本侧常量与地址由 `TARGETS` 自动派生（`channels()`）⇒ 计划里"第 3/4 处（Pod 包装 / register_component）"只对**组件**成立，对**通道**不成立。按性质分类见报告 §F-2 表（可自动化 3/手工 4/可门禁化 2）。 |
| AR-14 | **组件分类表（41 × 有无通道 × 该不该动画）** | scene-core / ze-scene-api | **已关闭（0）** | 按 `known_component_table!` 实测 **41** 个已登记类型；**16 条通道 / 9 个组件**（Circle 2、Arrow 4、Camera3D 1、Framing2D 2、Light 1、Material3D 2、MorphShape 1、PhaseAnim 1、Reveal 2）。无通道的 32 个按"该不该能动画"分四堆（类型白名单挡 / 值系统挡 / 本来不该 / 已在别处动画），逐项清单见报告 §C-2。**修正计划口径**：计划写"9 个有通道 / 16 条"正确，但把 `Camera2D` 当组件计数——它是**派生视图**（`Framing2D::to_camera_2d` 现算），登记表里有名字、世界里没有存储实例。 |
| AR-15 | **结构性债务四项（与动画专题同因）** | xtask / macros / schedule | **记账（2）** | ① `{STRUCT}_CHANNELS`：宏生成 9 个数组、**零消费**（全仓仅宏文档与 `target.rs` 注释提到）。② 跨 crate 重名白名单实测 **35 条**（计划写 36），其中 **27 条是"同一概念两处定义"**（18 条 Pod 包装 vs 组件/契约、9 条绘制参数 vs 组件视图）。③ `CHANNEL_HOMES` 2 条（`Framing2D→Camera2D`、`MorphShape→Morph`）——存储组件 ≠ 脚本组件。④ 组件级借用检查缺失（见 AR-01）。这四条都指向同一件事：**"组件"与"脚本可见的名字"没有单一来源**。 |

**AR 段小计**：已关闭 5（AR-02、AR-03、AR-09、AR-12、AR-14）· 部分关闭 0 · 记账 7（AR-01、AR-04、AR-05、AR-06、AR-07、AR-08、AR-15）· 未执行 3（AR-10、AR-11、AR-13）· 已记录 0。

---

## AS. 文档工程升级批次（docs/ 目录与模块索引，2026-10-28）

> 计划《文档工程升级执行计划_docs目录与模块索引》。本批是**文档 + 门禁**，不改生产行为。
> 核心判据：会腐烂的（文件清单 / 依赖 / 行数 / API）自动生成 + 漂移 fail；不会腐烂的（职责 /
> 边界 / 决策理由）才手写。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AS-01 | `architecture.json` **零消费者**且内容已漂移（仍登记不存在的 `ze-object` / `zela-scene`，缺 8 个真实 crate） | xtask / 文档 | **已关闭（0）** | 新增 `check_architecture`：`cargo metadata --no-deps` 与 `architecture.json` **双向**比对（登记了不存在的 / 漏登记 / 层路径不符 / 重复登记），并入 `cargo verify --step structure`。同批修正声明：补 `ze-comb` / `ze-func` / `ze-pod-func` / `zela-render-vector` / app 层四件 / 新增 web 层，删除已并入的 object 层，scene 层改为 6 个真实 crate。 |
| AS-02 | `docs/` 不在文档检查范围：`root_markdown_docs()` 只扫仓库根 ⇒ 新文档反而零校验 | xtask | **已关闭（0）** | 改为 `repository_markdown_docs`（根 + `docs/**` 递归），锚点（`crate::Item` 存在性）与路径（`crates/...` 链接存在性）两条检查同时覆盖 `docs/`（主计划 6.4 / 6.5）。 |
| AS-03 | 50 个 crate 零模块索引；46 个同名文件名 / 159 个文件无「crate 内怎么分工」文档 | 文档 / xtask | **已关闭（0）** | 新增 `xtask docs-gen`（零第三方依赖，自带极简 JSON 解析器）：生成 50 篇 `docs/modules/<crate>.md` 自动区（层 / 路径 / 生产依赖双向 / 文件清单 + 行数 + `//!` 首行 / 文件级 `use` 图 / 模块级 `pub` 项）。11 个重点 crate 手写四段；文件数 ≥ 15 的 4 个（`zela-scene-ecs` 24 / `zela-render-backends` 21 / `zela-render` 17 / `ze-pod-core` 16）与 `ze-scene-core`（41）在「运行机制」里回答了内部文件分工。 |
| AS-04 | README 与 `docs/modules/` 两个来源（33 个 crate 有 README，深度内容仍在 README） | 文档 | **记账（2）** | **5.3 已做**：32 个 crate README 的 H1 下各加一行 `docs/modules/<crate>.md` 指针（按目录层级算相对路径，逐条校验可解析；`crates/ze/graph/README.md` 是分组目录、无 Cargo.toml，跳过）。**5.1 未做**：深度内容仍在 README，同一事实可能两处各有一份；搬迁需逐篇核对归属，属独立一批。 |
| AS-05 | 手写区必填段无门禁（模板占位也能"绿"） | xtask | **已关闭（0）** | `check_handwritten_sections`：`## 职责` 与 `## 边界与不做什么` 必须存在且非空；源文件数 ≥ 15 的 crate 另需 `## 运行机制`；占位文案 `（待补` 视为未填。并入 `--step structure`。 |
| AS-06 | 上一批「通道常量 / 方法作者侧命名」主题是否未记账存疑（主计划 0.2） | 记账 | **已关闭（0）** | 核实 `GAPS AP-12`（通道常量挂错存储组件 ⇒ 三语言绑定无提示丢弃）与 `AP-13`（方法没有作者侧调用形态 ⇒ 只能手写 `expr.method{}`）均已定案并各带等价性证据（payload 逐字节相同），本批无需重复记账。 |

**AS 段小计**：已关闭 5（AS-01、AS-02、AS-03、AS-05、AS-06）· 记账 1（AS-04）。

---

## AT. 文档工程升级补充批次（glossary 与 decisions，2026-10-28）

> 补充计划《补充计划_docs术语表与决策记录》。判据：散文不可校验，**挂载点可以**。
> 备忘录中的「未落地清单 / 下一阶段起点 / 关键原话」一律不搬，只留指向 `GAPS.md` 的指针。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AT-01 | 全仓零术语表 / 零决策记录；专有名词密度高但无 hub | 文档 | **已关闭（0）** | `docs/glossary.md` 87 条（每条必带 `- **代码锚点**：` 或 `- **锚点**：`）；`docs/decisions/` 8 篇（确定性谱系 / 计算内核双层 / 图灵完备边界 / Pass 架构 / 类型与反射 / 图基础设施 / 007 / 定位）+ README 索引。搬运时逐条按 `GAPS.md` 校验时效性（备忘录 §五「可观测性计划未执行」已过时，AQ 段已执行）。 |
| AT-02 | 术语 / 决策无门禁 ⇒ 写了也守不住 | xtask | **已关闭（0）** | 7.1 复用 docs 锚点扫描；7.2 `check_glossary`；7.3–7.5 `check_decision`（状态 / 锚点 / 被谁取代 / 决策·理由·反例三段）；7.6 复用 `linked_crate_paths`；7.7 孤儿术语只告警且**恒打印计数**（0 条也打印）。全部并入 `--step structure`。 |
| AT-03 | 补充计划写「备忘录 §七 共 7 条」，实际只有 5 条 | 文档 | **已关闭（0）** | 按实际搬运 5 条进 `CONTRIBUTING.md`「协作注意事项」；数字差 2 在此记账，**不猜补**（补两条编造纪律比少两条更糟）。 |
| AT-04 | 备忘录 §四 / §五 未落地清单与下一阶段起点 | 文档 | **已关闭（0）** | `docs/README.md` 的「未落地项 / 下一阶段去哪看」只放指针（`GAPS.md` / `ARCHITECTURE.md` / `architecture.json`），无复制内容；决策记录里剩余缺口只写「见 GAPS 对应段」。 |

**AT 段小计**：已关闭 4（AT-01、AT-02、AT-03、AT-04）。

---

## AU. 类型与算子地基批次 · 阶段 1（值类型参数化，2026-10-28）

> 计划《组合子库算子库类型改造与优化_执行计划》，代号**类型与算子地基批次**。
> 硬纪律：① 参数化不得削弱穷尽匹配门禁（新增标量类型未处理 ⇒ 编译失败或测试失败）；
> ② 每阶段挂在可验证产出上。本段只记**阶段 1**（全批阻塞项），阶段 2–5 未执行。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AU-01 | **阶段 1：值类型参数化**（`Value` 六变体 → 四变体） | ze-comb / ze-func / scene | **已关闭（1）** | `Value` 由 `F64 / I64 / Vec2 / Vec3` 四个变体收敛为 `Scalar(Scalar)`：`Scalar { ty: ScalarKind, dim: u8, data: [u8; 32] }`（`ty × 1..=4`，零堆分配，字段私有 ⇒ 只能经构造函数产生）。同时补上 `Vec4` 与 `F32` 标量的表达能力（旧枚举无法表达）。调用点全部迁移：`ze-func::ops`（含 `mix` / `x|y|z` / `length` / `normalize` / 转换族）、`simplify`、`reference`、`pass`、`zela-scene-ecs::expr_mono`、`ze-scene-core::target`。**规范字节（`canonical_bytes`）对既有四种组合逐位不变**（标签 `0 / 6 / 3 / 4` 保留），新组合走通用标签 `7`——值序列哈希因此不动，fixture 与像素 / 结构基线无需重录。 |
| AU-02 | **值侧 `I64` 在类型侧没有对应种类**（`ScalarKind` 缺 `I64`） | ze-type / ze-pod-* | **已关闭（1）** | 反例：`Value::I64` 是离散层索引，而 `ScalarKind` 只有 `F32/F64/I32/U32/U64/U8/Bool/String` ⇒ 参数化后该值没有类型可填。补 `ScalarKind::I64` 并让**穷尽匹配把它逼出来**：`ze-type::unify`（布局 8/8）、`gen::static_of`、`ze-pod-core::ScalarValue::I64` + `impl_pod_type_scalar!(i64)`、三语言绑定（Lua `integer` / Python `int` / JS 精确整数 `MAX_EXACT_INTEGER` 内）与声明文件类型名。这类"新增种类 ⇒ 4 个 crate 编译失败"正是 §0.1 要的门禁。 |
| AU-03 | **`ChannelType::Bytes` 是死臂**：`zela.Path` 的 schema 投影不到 `Bytes` | ze-comb / ze-pod-func | **已关闭（1）** | 旧 `from_type_spec` 只认 `zela.Vec2` / `zela.Vec3`，`zela.Path`（`Struct { layout: None }`）落到 `None` ⇒ 通道化会在类型投影处被拒。按**定长 / 变长第一判据**接上：`Struct` 有 `Layout` ⇒ 具体数值类型（`Vec2/Vec3/Vec4`），无 `Layout` ⇒ `ChannelType::Bytes`（走 `negotiate_bytes` 同一协商入口）。另补 `ChannelType::Vec4`（`kind_of` → `Kind::Vector`），与值侧 `Vec4` 对齐。 |
| AU-04 | **精度口径：标量 `f64`、向量 `f32` 不统一成单一宽度** | ze-comb | **记账（1）** | 风险表第 4 条要求"明确选哪个"。裁决：**保持两种显式类型**——`F64 × 1` 是表达式层的数（与 `sin` / `mul` 等 f64 算术逐位一致），`F32 × 2/3/4` 是向量（与 `ze-type::layout` 的 `zela.Vec2/Vec3` 的 f32 字段逐位一致）。统一成 f32 会改全部标量语义、统一成 f64 会改 `mix` / `normalize` / `length` 的 f32 取整 ⇒ 两者都会改像素基线（本批门禁禁止）。参数化的意义是把这层差异**写在类型里**（`ty` 显式），而不是藏在一个无类型标签的枚举里。 |
| AU-05 | **跨 crate 重名白名单**：`Scalar`（ze-comb vs ze-type） | xtask | **已关闭（1）** | `ze-type::layout::Scalar` 是布局标量 trait，`ze_comb::Scalar` 是运行时值（`ty × dim`）——角色不同，按既有机制登记真实理由。**口径写死**：白名单共 **37 条 = 36 条真实重复 + 1 条 `OpDef` 哨兵**（哨兵条目是"已收敛到 `ze-type`，保留条目让第二份立刻失败"，不是重复本身）；`Scalar` 条目只补理由，**不新增条目**（此前"37 → 38"是手写错值）。`--step structure` 全绿。 |
| AU-06 | **阶段 2：`Text` 下沉 `Bytes`** | ze-comb / ze-func | **已关闭（2）** | `Value::Text(String)` 删除 ⇒ 文本是 `Bytes + zela.Text`（变长 ⇒ 没有 `Layout` ⇒ 永远慢路径），与 `zela.Path` / `acme.Tensor` 走同一套协商机制。规范编码保留标签 `2` ⇒ 值序列哈希与阶段 1 之前**逐位相同**；`kind()` 仍报 `text`（诊断词汇不变）；`simplify` 的常量折叠可逆还原 `Literal::Text`。`Value` 变体 5 → 5（阶段 1 已 −2；本阶段再 −1 后由阶段 3 的 +2 抵消，见 AU-07 的说明）。 |
| AU-07 | **阶段 3：`Iter` + `Stream` 是两个类型** | ze-comb / ze-func | **已关闭（3）** | `ze_comb::iter`：`ValueIter`（长度已知 ⇒ `fold` / `map` / `sum`，归纳侧）与 `ValueStream`（余归纳前沿，`depth` 上界**随值走**，越界 `StreamDepth` 显式失败、`StreamShape` 拒绝层数错配）；`ValueStream::from_expanded` 直接吃 `codata::expand` 的产物（同一份 `depth`，第五次"上界进产物"）。对拍 `ze-func/tests/iter_equivalence.rs`：`fold` / `sum` / `map` 与 `NatRec` / `zela.math::sum` **逐位一致**（不重录基线）。**偏差（记账）**：`Value` 变体数为 5（+`Iter` / `Stream`）—— 计划§1.3 的"变体数减少"指**标量四变体合并**（阶段 1 已达成）；可迭代类型是新能力，不能靠合并换数量。 |
| AU-08 | **阶段 5：Mono 元素身份 + 通道三分法** | ze-comb | **已关闭（5）** | `Mono::read_element(channel, element)`（默认 `MissingElementChannel` 显式失败）；`ChannelRef::class()` 是**地址的函数**（`Frame` / `Element` / `HostState`，机器可检查）；`Step::Channel` 按类别分派 —— 逐元素通道无上下文即 `ElementContext`；`CompiledExpr::eval_elements(mono, len)` 把"每顶点"变成对迭代器的 map（结果 `ValueIter`），参考求值器有同口径入口对拍。用例锁定：`uv.position` 按元素下标读出不同值、lowered == reference、无元素源/无上下文都显式失败、每帧通道在逐元素求值下**广播**。**偏差（诚实）**：场景侧真正的逐顶点数据源（顶点 attribute 注入）属下游批次，本批只交付机制。 |
| AU-09 | **阶段 4：算子注册表化** | ze-func / ze-pod-func | **部分关闭（4）** | ① `ze-func`：`math_ops!` 一次展开同时产出 `DEFS`（元数据）/ `OPS`（名字）/ `EVALS`（名称→实现），`ops::eval` 改为**符号表查表**（`match op.name` 已删除）；少写实现 = 编译失败，漏登记 = 算子不存在。② 门禁：`every_def_has_an_eval_implementation`（目录 ↔ 注册表逐条）+ `every_operator_is_reachable_from_scripts`（目录 ↔ 作者侧，缺字段/注册/`call` 分支即指出并红）。③ **"加一个算子只改 1 处"未达成（诚实）**：作者侧仍需 spec 结构 + `MathApi` 字段 + ctor + 注册 + `call` 分支（`#[capability]` 的字段无法由外部宏注入）。④ **实测发现的硬冲突**：新增算子必改 `signatures()` ⇒ 必须递增 version ⇒ `LIBRARY_VERSION` 进表达式产物 ⇒ **payload 逐位变化**，与本批"不重录基线"互斥 —— 因此本批**不新增算子**（曾以 `floor` 全程实测并回滚，指纹/版本回到 17 / `4_840_…`）。彻底"1 处"需要算子 codegen（内核 + 作者侧同一份源），属独立批次。 |

| AU-10 | **W-A（值系统三套收敛）显式关闭** | ze-comb / ze-time | **记账（2）** | 计划 §3 判据 6 要求"W-A 显式关闭或显式说明理由"。本批的阶段 1/2 就是它的答案：`Value` 与 `TypeSpec` 已同构（`Scalar{ty,dim}` ↔ `Scalar` / `App`，文本与路径统一为 `Bytes + 命名类型`），`ChannelType` 是 `TypeSpec` 的降级视图（唯一投影点）。`AnimValue`（ze-time 的动画侧值）按计划 §5 明确**不在本批**，留待阶段 1/2 之后的独立评估。 |

| AU-11 | **签名声明了向量重载，求值只吃标量**（`add` / `mul` / `scale` / `sum`） | ze-func / app / scripts | **已关闭（收尾）** | 实现：`add` 同型逐分量；`scale` / `mul` 支持「向量 × 数」与「数 × 向量」两侧（**f32 域**，与向量分量同宽）；`sum` 同型累加（空序列仍是标量零元）；混型一律**显式失败**（不隐式提升、也不默默跳过）。这是**语义变更**（此前这些输入 100% 报错）⇒ 按纪律 bump `changed` 1 → 18（`since` 与签名不变），旧产物在加载时被指出并拒绝。代价与证据：17 件作品的 ZSB payload 因 revision 记录变化而更新 `demo/manifest.txt` 哈希（`demo` 27 条断言重绿）；**像素 36/36 与结构指纹逐位不变**（渲染结果与 revision 无关）。用例：`vector_overloads_are_implemented`、`vector_overloads_landed_at_revision_18`。 |
| AU-12 | **`Vec4` / `F32` 标量能存能传，但算子算不了** | ze-comb / ze-func | **记账（3）** | `Scalar::f32_vec` / `as_vec4` 已具备（`Vec4` 可作通道值与 payload 往返），但：① `ze-comb::typed::SlotType` 没有 `Vec4`，单态化遇 `Vec4` 整棵回落 `CompiledExpr`（`SlotType::from_channel` 对 `Vec4` 返回 `None`）；② `ze-func` 的 29 个算子没有 `Vec4` 签名，`add` / `mul` / `scale` / `sum` 的向量重载只覆盖 `Vec2` / `Vec3`。**裁决：只做通道承载，不做运算**——当前无任何作品 / fixture 使用 `Vec4` 算术，补签名要给四个算子各写分支并扩 `TypedOp` 16 个变体，收益为零；一旦出现 `Vec4` 算术需求（触发条件），按 AU-11 的同一路径补签名 + typed 实例并 bump revision。 |
| AU-13 | **`Iter` / `Stream` / `read_element` 在作者侧零入口** | ze-comb / ze-func / pod | **记账（4）** | 机制已交付（AU-07 / AU-08：`ValueIter` / `ValueStream`、`Mono::read_element`、`CompiledExpr::eval_elements`、`iter_equivalence` 对拍），但脚本层没有任何构造 `Iter` / `Stream` 或注入逐元素数据源的入口——`ze-pod-func` 里没有 `iter` / `stream` / `read_element` 的绑定，`zela.math` 也不暴露 `fold` / `map`。**裁决：机制先到，出口单独排**（计划 §8 明确不把作者侧入口塞进本批）；触发条件是"某个作品需要逐顶点 / 逐元素求值"，届时补 Pod 构造器与能力覆盖，并同步 `cargo verify --step coverage` 的作品矩阵。 |

**AU 段小计**：已关闭 8（AU-01、AU-02、AU-03、AU-05、AU-06、AU-07、AU-08、AU-11）· 部分关闭 1（AU-09）· 记账 4（AU-04、AU-10、AU-12、AU-13）· 未执行 0 · 已记录 0。

---

## AV. 表达式求值单态化批次（typed fast path，2026-10-28）

> 用户立约：**编译器做单态化，按类型分槽，直接解析指针**；且**类型转换不该由算子库
> 做**，应由底层按注册的类型自动完成。
>
> 现状病根：`CompiledExpr` 的值表是 `Vec<Value>` —— 每步一次装箱值、每个算子一次
> `Vec<Value>` 实参（clone）、一次符号表查找，算子里还要 `as_f64` / `as_vec2` 做**运行期**
> 类型判断。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AV-01 | **编译期单态化 + 按类型分槽 + 函数指针直呼** | ze-comb | **已关闭（1）** | 新增 [`ze_comb::typed`](crates/kernel/ze-comb/src/typed.rs)：`SlotType`（F64 / I64 / Bool / Vec2 / Vec3）、`SlotRef`（编译期分配）、`Slots`（SoA 值池：`Vec<f64>` / `Vec<[f32;2]>` / …）、`TypedOp`（**精确类型签名**的函数指针枚举）、`TypedStep`（常量 / 通道 / 调用）、`TypedProgram::compile/eval`。求值期：无符号表查找、无 `Value` 装箱、无实参 `Vec`；每步一次枚举分派 + 裸指针调用。槽池按 `SlotLayout` **一次性预分配**。**回落策略也在内核**：[`ze_comb::compiled`](crates/kernel/ze-comb/src/compiled.rs) 的 `ExprProgram { lowered, typed: Option<_> }`（`compile(expr, resolver)` / `lowered_only` / `eval`）+ `NoMonomorphizer` —— 这两者只认识 `OpResolver`，不认识任何算子词汇。 |
| AV-02 | **类型转换下沉到调度层**（算子不再做 `as_*`） | ze-func | **已关闭（2）** | [`ze_func::mono`](crates/kernel/ze-func/src/mono.rs)：`MathMonomorphizer` 实现 `ze_comb::OpResolver` —— 输入 `(算子, 实参槽类型)`，按**注册的签名**选出唯一实例（`F64xF64ToF64` / `Vec2xF64ToVec2` / `MixVec3` / …）；`instances` 模块只写**精确类型**的公式，没有 `as_f64`。签名声明了但没登记的类型组合 ⇒ `None`（整棵回落），不静默挑近似实例。29 个算子全部覆盖（含向量重载与 Bool 比较）。 |
| AV-03 | **三路逐位对拍** | ze-func | **已关闭（3）** | `ze-func/tests/typed_equivalence.rs`：42 条算子用例 + 整树（通道 / 共享节点）+ 4 条回落用例，断言 `typed == lowered == reference` **逐位相同**；`sum` 的累加顺序与动态路径同为 `0.0` 起左折（`Vec2` / `Vec3` 变体亦然）。 |
| AV-04 | **性能实测**（同一条 `Σ sin(t)` 深链） | ze-func bench | **已关闭（4）** | `ze-func/benches/typed_bench.rs`（criterion，warm-up 0.5s / measure 2s，本机 median）：16 项 **2.23 µs → 0.415 µs（5.4×）**；64 项 **9.26 µs → 1.60 µs（5.8×）**；256 项 **37.8 µs → 6.23 µs（6.1×）**。项数越多差距越大 ⇒ 砍掉的是**每步固定开销**（装箱 / 参数 Vec / 符号表 / 动态类型判断）。 |
| AV-05 | **宿主接入（装配期单态化，运行期快路径）** | ze-comb / ze-scene-core / ecs | **已关闭（5）** | `BindingDriver::Expr(CompiledExpr)` → `Expr(ze_comb::ExprProgram)`（场景层只 `pub use`，**不自己实现回落策略**）；`Timeline::commit` 用 `ExprProgram::compile(expr, ze_func::monomorphizer())` —— 内核只依赖 `OpResolver` 接口，解析器实例由场景层装配时注入。求值点 `BindingApplySystem` 无需改动（`ExprProgram::eval` 与旧签名同形）。`predicate` / 读集分析仍用 lowered 语义基准。**端到端证据**：像素 **36/36** 与结构指纹**逐位不变**，`demo` 27 条 payload 断言不变。 |
| AV-06 | **未覆盖的节点与值类型** | ze-comb / ze-func | **记账（1）** | 本批 typed 只做**逐点数值 / 向量 / Bool**：`Select`（惰性分支）/ `Read`（对象）/ `Method` / `NatRec`（递归）/ `Exists` / `Bytes` / `Text` / `Iter` / `Stream` / `Vec4` / 类型未知的通道一律**回落** `CompiledExpr`（`TypedProgram::compile` 返回 `None`）。回落是整棵而非混跑 —— 部分 typed 会让"同一条表达式两种语义"变得不可对拍。 |
| AV-07 | **残余开销** | ze-comb | **记账（2）** | ① 变参 `sum` 在 typed 里仍收集一次 `Vec<f64>`（每帧一次小分配）；② 通道读仍经 `Mono::read` 返回 `Value` 再解包（`time` / `frame` 每帧 1–2 次，占比小；后续可加 `Mono::read_f64` 默认方法让宿主覆写）；③ 槽池每帧 `Slots::new` 分配 5 个 Vec（可按帧复用，需 `&mut` 求值入口）。三者都**不影响正确性**，属下一轮微优化。 |

**AV 段小计**：已关闭 5（AV-01…AV-05）· 部分关闭 0 · 记账 2（AV-06、AV-07）· 未执行 0 · 已记录 0。

---

## AW. 参数化几何与 shader codegen 批次（2026-10-29）

> 计划《参数化几何与 shader codegen_执行计划》，代号**参数化几何与 codegen 批次**。
> 硬纪律：① payload 边界外不保证跨端逐位一致，但**本机逐次一致仍必须保**；
> ② 危险能力（GPU 求值）必须有语法/值级的分级标记；③ 每个实验必须有 A/B/C 结局。
> 编号用 `AW-`（`AV` 已被单态化批次占用，`AU` 已被类型地基占用 —— 不复用）。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AW-01 | **阶段 1：参数化几何**（`MorphGeometry` → `kind + params`） | render-vector / playback | **已关闭（阶段 1）** | [`api.rs`](crates/render/zela-render-vector/src/api.rs) 的 `ParamGeometry { kind: ParamKind, delta, params }`（`Lerp` / `Window` / `Grow`）+ `ParamGeometry::{lerp,window,grow,param_digest}`；接缝统一在 `zela_render_vector::scene::param_records`，`morph_records` / `window_records` / `grow_records` 都是它的薄入口。`attach_param` 把 `Lerp` 接第二形状、`Window` / `Grow` 写 `params[0..2]`；窗口与 `PathOptions::curve_window` 同时给出**显式报错**。`Grow(n)` 就是 `Window(0, n)`（用例 `grow_is_a_window_prefix` 锁定像素逐位相同）。 |
| AW-02 | **E1：三种动画是同一 idiom，且结构指纹只哈希参数** | scene / render-vector | **已关闭（E1）** | `ParamGeometry::param_digest` 只哈希 `kind + params`：同参数两次求值同摘要、只改 `delta` 摘要不动、参数变摘要变（用例 `param_digest_hashes_the_numbers_not_the_geometry`）。几何仍是派生量（GAPS AR-05 / AR-08 的 `Path` 盲区不变），但**逐帧真正变化的只有参数** ⇒ 等式判据不再依赖求值结果。`Window` 与 `curve_window` 逐位相同（`window_param_and_curve_window_render_alike`）。**边界（诚实）**：`base` 几何仍在每帧被重新打包并上传 —— `Player::reset` 每帧清 `Store`，真正的"上传一次"需要持久前缀 + 增量上传，见 AW-07。 |
| AW-03 | **阶段 2：Reveal 归约为曲线窗口** | playback / render-vector | **已关闭（阶段 2）** | [`vector_frame.rs`](crates/app/zela-playback/src/vector_frame.rs) 的 `reveal_window`（归一化弧长 → 曲线索引 + t）+ `reveal_base`（完整折线）；`two_d_records` 不再用 CPU 裁剪，改发 `window_records`。旧裁剪实现保留为 `#[cfg(test)]` 的 A/B 参考：`reveal_window_matches_the_cpu_clip` 判据 = 最大通道差 ≤ 4 且差异像素 < 200。**像素基线变更（逐条列出，全部来自 Reveal）**：`[vector] fourier` / `pendulum` / `storyboard` 三条序列哈希变化 —— 书写脉冲的**笔尖端点**由着色器裁剪（没有 CPU 侧的端点 cap）；已在录制设备（`ZELA_RECORD_DEVICE`）重录并 `--rebind`，`hashes` 36/36 全绿，其余 33 条逐位不变。 |
| AW-04 | **E2：过采样 + 窗口的精度** | ze-scene-core | **已关闭（E2）** | `FunctionGraph::{extended_path,window_for}`（两侧各扩一个 `span`，相位只移动 `CurveWindow`；超边距才回落重采样，显式 `None`）。实测 `e2_sampling_error_at_four_times_zoom`：`sin` 在 `[0, 2π]` 按 120 段采样，最大弦高 **< 1e-4 世界单位**，4×（4 px/单位）下 **< 0.5 px** ⇒ 4× 放大无可见台阶（结局 **A**）。**边界**：采样密度由 `step` 固定、不随缩放增长；临界倍率 ≈ 0.5 / 弦高（数千 px/单位）之后会露台阶，那时走 4-B 的 GPU 求值。FunctionGraph 目前**无脚本入口**（GAPS W1-06），本阶段不改变任何作品像素。 |
| AW-05 | **E3：select 粒度 —— uniform（per-draw）vs 属性（per-vertex）** | render-vector | **部分关闭（E3 数据）** | 实验 `e3_expression_clustered_draws_keep_depth_compositing`：三个不透明方块（z = −1 / 0 / +1）按"声明序"与"按表达式聚簇、组间按最小 z 排"两种顺序渲染，**像素逐位相同** ⇒ vector 后端逐像素深度合成对 draw 顺序不敏感，`uniform select` 可行（结局 **A**）。**未测（记账）**：GPU 侧"每次 draw 换表达式"的耗时差 —— 需要先加 shader 分支（4-A 的 `concat` 接缝），留给 codegen 接入时用同一场景复测。 |
| AW-06 | **阶段 4-A：着色 codegen（插 `concat()` 的 source 位）** | ze-comb / ze-func | **已关闭（4-A）** | [`codegen.rs`](crates/kernel/ze-comb/src/codegen.rs) 的 `ze_comb::codegen::emit_wgsl`：结构递归 + 类型映射 + `BTreeMap` 参数表，输出 `fn eval(p: Params, index: u32) -> T` 纯函数；`HostState` 通道 / 离散层节点 / 非有限字面量**显式失败**。算子规则在 [`wgsl.rs`](crates/kernel/ze-func/src/wgsl.rs)（`ze_func::wgsl::backend`），签名校验复用单态解析器（生成规则与求值实例同源）；`every_operator_has_a_wgsl_rule` 从 `OP_DEFS` 展开、29 个算子全覆盖。**E4**：`zela-render-backends/tests/wgsl_codegen.rs` 把生成的 WGSL 包进最小 compute pass，7 条表达式 × 3 个时间点与参考求值**容差内一致**（f32 vs f64，容差 2e-5）；同 `Expr` 两次生成逐字节相同（`emission_is_byte_identical_across_runs`）。**修正的口径坑**：`round_f32` 是"f64 收窄到 f32"，在 shader 里是恒等，不是 `round()` 到整数。 |
| AW-07 | **阶段 4-B：几何 codegen 接进矢量后端**（持久前缀 + 增量上传） | render-vector | **部分关闭（E-G3：①②④ 已接；③ 不需要）** | ① `Player::reset` 不再 `Store::clear`：改为 `Store::retire`（保留本帧用过的键，淘汰其余），几何数组跨帧保留；② 上传只走 `Buffers.sent` 游标（本就有）+ **内容复用**：`add_path` / `add_points` / `add_mesh` 在同键、同内容（控制点 / subpath / 顶点 / 法线 / 三角形逐位相等）时直接返回已有偏移，不追加、不产生 dead；同键换内容时旧字节记入 `dead`（否则只增不减），由 `pack_if_worthwhile` 回收。④ 拓扑变化 = 内容不同 ⇒ append 新偏移，`grow_path` 的原地写与 `ranges` 同步沿用 `dirty`（上界超容量仍由 `buffer_room` 显式失败）。③ **记账为不需要**：`ParamGeometry::Lerp` 已经是"两套控制点 + shader 按 `params[3]` 混合"（`Record::morph`），compute 写 `ctrl` 对现有参数化几何是冗余路径（AW-08 的机制保留为实验）。**判据（E-G3）**：新增 `take_store_upload_bytes()`（几何数组逐帧上传字节）并接进 `FrameRun.store_uploads` 与 `ZELA_OBSERVE` 的 `geometry_upload`。实测（`--step hashes`，vector 一路）：pbr_scene 660 帧 3.64 MB → **0**、pbr_materials 480 帧 15.8 MB → **0**、pbr_lights 510 帧 3.37 MB → **0**、fusion 180 帧 1.36 MB → **0**；fourier 216 帧 13.8 MB **不变**（Reveal 每帧 `points` 真的不同）、storyboard / pendulum / orbit 同理（曲线每帧重算）。**边界（诚实）**：键仍是"记录下标 + 基址"⇒ 对象顺序/数量变化时下标平移、命中率下降；内容每帧变化的作品（Reveal 增长、3D 曲线重算）不复用。这两者要靠"内容哈希键"解决，触发条件：某作品的 `prepare` 被实测为主瓶颈（fourier 当前 `prepare` 占 22.5%）。**`FEATURE_CODEGEN` 仍未加**（AW-11）：几何没有新的生产入口。 |
| AW-08 | **E5：compute pass 写 `ctrl` 后 flatten 结果正确 + 本机逐次一致** | render-backends（实验） | **已关闭（E5 机制）** | `gpu_geometry_experiments.rs` 的 `e5_compute_pass_written_controls_flatten_correctly`：GPU 对 8 个控制点做 `mix(base, target, t)` 写入 `out`，CPU 按 `Path2D` 三次曲线展开；与"先 mix 再展开"逐点最大距离 < 1e-3；同一输入连跑两次的读回字节**逐位相同**。**边界（诚实）**：这是独立 compute pass 实验，未接渲染链路（接入是 AW-07 的 C）。 |
| AW-09 | **阶段 5 / E6：GPU bounds reduce 逐位确定** | render-backends（实验） | **部分关闭（E6 数据）** | `e6_gpu_bounds_reduce_is_bit_exact`：单 workgroup 逐点 `min/max` 归约与 CPU 参考**逐位相同**（含 `-0.0`：两端都做 `x + 0.0` 归一）。min/max 只有比较、不涉及舍入 ⇒ 树形归约与顺序无关，可留在 payload 内、进结构指纹（不是容差判据）。**未做（记账）**：接进 `object_bounds` / 相机取景的生产路径；控制点 bounds ≠ 曲线 bounds（凸包偏大，取景会留白），需要紧包时另做"导函数求极值"。 |
| AW-10 | **阶段 0.6：`bench_baselines.txt` 重录** | xtask | **未执行（需干净树 + 录制设备）** | 本批开发期间工作树持续有改动，按纪律**不在脏树上重录**；且 bench 基线与本批功能无关。触发条件：本批合入后的第一个干净树 + 录制设备（`ZELA_RECORD_DEVICE`）。 |
| AW-11 | **`FEATURE_CODEGEN = 1 << 5` 未加** | ze-render-ir / routing | **记账（随 AW-07 一起做）** | 计划 §阶段 4 要求几何 codegen 是 vector-only 能力、路由到 wgpu 时必须写明"已忽略"。当前**没有几何 codegen 的生产入口** ⇒ 先加能力位等于登记一个永远不会为真的条件（死位）。触发条件与 AW-07 相同：compute pass 写入生产链路的那次提交同时加位、加 `PathOptions::feature_names` 条目、加 routing 的"已忽略"文案与用例。 |

**AW 段小计**：已关闭 6（AW-01、AW-02、AW-03、AW-04、AW-06、AW-08）· 部分关闭 3（AW-05、AW-07、AW-09）· 记账 1（AW-11）· 未执行 1（AW-10）· 已记录 0。

---

## AX. GUI 上屏与 codegen 生产化批次（2026-10-29）

> 计划《GUI 上屏与 codegen 生产化_执行计划》，代号**为 30 秒产品清路批次**。
> 硬纪律沿用 AW：① 改 GUI 上屏**不碰 headless**（判据 = 像素 36/36 与结构 22 项逐位不变）；
> ② `reusable`（静止帧不重渲）必须保留；③ 每个实验必须有 A/B/C 结局。
> 本段按 §2 的阶段顺序记账；**未兑现的阶段不写成"已关闭"**。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AX-01 | **阶段 1 / E-G1：GUI 矢量上屏零回读** | zela-gui / zela-playback / zela-render-vector | **已关闭（E-G1 = A · 原生纹理注册）** | `App::launch` 注入 `init_gpu(device, queue, adapter)`（矢量后端不再自建 device）；帧由 [`render_to_texture_at`](crates/app/zela-playback/src/vector_frame.rs) 直接写进 `Rgba8Unorm` 中间纹理（`RENDER_ATTACHMENT \| STORAGE_BINDING \| TEXTURE_BINDING`），`register_native_texture` 后 `painter().image` ⇒ 每帧**没有** `Vec<u8>`、`ColorImage` 与 `load_texture` 三跳。用例 `vector_gui_zero_copy`：纹理目标与 `render_2d` 的 CPU 读回**逐位相同**，且同帧连渲两次逐位相同（§1.4）。实测（`ZELA_GUI_TIMING=1`，381×829，ppp 1.5）：vector_paths / fourier / surface_matrix / demo3d 四件首帧 `渲染 4–16 ms + 上传 0.0 ms`，其后 `reusable` 生效（矢量 0.0 ms）⇒ 静止作品仍近乎零成本。`cargo verify --step hashes` 36/36 ok（headless 未动）。**边界（诚实）**：① 含 `composite` 内容纹理层的帧仍走 CPU 叠加（GAPS PC-01 未解），surface_matrix 是静止帧、只付首帧；② 宿主 device 的 features 改成"已注册且可用后端之并"（`zela-gui/src/main.rs::request_vector_device`），否则矢量建管线时 `R32Float` 不可混合 —— 这是"注入 device"的**必要条件**，不是可选优化。 |
| AX-02 | **阶段 2 / E-G2：着色 codegen 进生产 + 最小用例脚本** | ze-comb / ze-func / scene / render-vector / ze-pod-func | **已关闭（E-G2）** | 已落地的三段：① **后端**（`c11b6cf`）：`FilledPath.shader` + `ShaderFill{source, params}`，compute pass 在光栅前把 `eval` 的颜色写进 `Object`（解析路径）与 `Instance`（点云 / 网格）的填充槽，只写 r/g/b（alpha 保持原值）；管线按生成文本摘要缓存，`Params` 走 uniform。② **脚本入口**：`expr.shader { object, root }`（`zela.math@18`）→ `PendingShader` → 装配期 `SceneScript::shader_fill` → 组件 `ShaderPaint` → `paint_of` → `PathOptions.shader`（`FEATURE_CODEGEN = 1<<5` + `feature_names` 条目 ⇒ 路由到 wgpu 时理由里会写"已忽略：着色 codegen"）。③ **作品 / 门禁**：`cap_codegen.lua`（左右同几何、右侧颜色由 GPU 求值）+ manifest 条目 + `dispositions.txt` 豁免 + `--step coverage` 55/55、`--step hashes` 36/36、`--step structure` 全绿。用例 `codegen_fill`：GPU 求值与 CPU 参考容差 4/255 内一致、连渲两次逐位相同、无程序时填充色逐位等于记录值。**后续切片（本批已接）**：④ **外部注入按类型推导**（`aa4962c`）：`Params` 的 uniform 布局由生成器的参数表按 std140 推导（标量 / 向量 / 矩阵同一套规则，不按通道名特判），宿主每帧按参数表填字节；⑤ **对象级内建**（`77a1b0b`）：`zela.shader::center` / `size` 归 `ChannelClass::Element`，不进 uniform，后端从 `Object.rect` 现读；⑥ **逐片元内建**（`93fd0c7`）：`fragment.pos2` / `local` / `uv` 走 `Rgba32Float` 颜色画布（rgb = 颜色，a = 程序标签），合成时按"对象 → 标签"认领，标签不匹配退回普通填充；⑦ **内建 mono**（`a4dccbc`）：`distance` / `smoothstep` / `fract`，CPU / 类型化 / WGSL 三路同源。`zela.math` 因此到 `@21`。**连带影响（需重录）**：`zela.math` API 升到 `@18` ⇒ 表达式产物的 `library.version` 变化 ⇒ 含表达式绑定的作品（expr_demo / cap_channels2d / cap_channels3d / expr_read / storyboard 等）的 manifest payload-hash 需要在录制设备上重录；重录与验证见 AX-07。 |
| AX-03 | **阶段 3 / E-G3：AW-07 几何 codegen 接生产** | zela-render-vector | **部分关闭（E-G3 · ①②④ 已接；③ 不需要）** | 见 AW-07 的证据与实测：持久前缀（`Store::retire`）+ 内容复用（path / points / mesh）+ 既有的 `sent` 增量上传；`take_store_upload_bytes()` 是可复现判据（`FrameRun.store_uploads` / `ZELA_OBSERVE.geometry_upload`）。四个重负载 3D 作品的稳态几何上传从 3.4–15.8 MB 降到 **0**；Reveal / 3D 曲线这类"每帧内容不同"的作品不变（诚实边界），内容哈希键留待 `prepare` 成为主瓶颈时再做。compute 写 `ctrl` 记账为不需要（morph 已由 shader 混合）。 |
| AX-04 | **E-G4：E3 性能补测（uniform select vs 属性 select）** | zela-render-vector | **未执行** | AX-02 已给出着色入口（每对象一个 compute dispatch + 逐片元颜色画布），但"每次 draw 换表达式"的耗时差仍未测：需要同一场景下 A/B（uniform 参数 vs 属性参数）的成对计时，且按纪律要在干净树上录。AW-05 的"待补"状态不变。 |
| AX-05 | **口径修正：`param_digest` / `grow_records` 不是"已关闭"** | ze-render-ir / render-vector / GAPS | **记账（机制已备、出口未接）** | `ParamGeometry::param_digest` 的调用点只有 `api.rs` 自己的用例（`param_digest_hashes_the_numbers_not_the_geometry`），生产代码零消费者；`grow_records` 只有 `lib.rs` 导出 + 测试，`param_records` 的 `Grow` 分支不走它。二者与 AW-11 同型：**机制在，出口没接** ⇒ 在 AW-01 / AW-02 的"已关闭"之外明确记这一笔，避免把"有用例"读成"有生产路径"。 |
| AX-06 | **`bench_baselines.txt` 重录（AW-10）** | xtask | **未执行** | 本批开发期间工作树仍有改动，按纪律不在脏树上重录。触发条件不变：合入后的第一个干净树 + 录制设备（`ZELA_RECORD_DEVICE`）。 |
| AX-07 | **manifest payload 重录（AX-02 的连带项）** | zela-app/scripts、xtask | **已关闭（0）** | `zela.math` 17 → 21 使 `LibraryRef.version` 进 payload ⇒ 含表达式绑定的 manifest 记录需要新 hash。做法：`target/verify/zela.exe compile`（注入 `ZELA_TYPST_FONT_DIR`）逐条取实测值，不猜；18 条变化与计划列出的清单逐条一致（fourier … cap_codegen）。`--step demo` 28/28 通过（`cap_codegen` 由 `-` 提升为断言）。**像素不变**：manifest 只断言 ZSB 内容；`--step hashes` 36/36 与结构 22 项不变（见 BA-02 的读数）。 |

**AX 段小计**：已关闭 3（AX-01、AX-02、AX-07）· 部分关闭 1（AX-03）· 记账 1（AX-05）· 未执行 2（AX-04、AX-06）· 已记录 0。

---

## 附录. crate 能力表（阶段 0.5 门禁）

> **门禁 1**：`crates/**/Cargo.toml` 的每个 crate 必须在本表登记（新增 crate 未登记 ⇒ `cargo verify --step structure` 失败）。
> **门禁 2**：跨 crate 同名 `pub struct/enum` 不在白名单 ⇒ 失败（一个概念只允许一处定义；重复必须登记理由）。

| crate | 职责 | 可复用能力（领域无关者显式标记） |
|---|---|---|
| ze-type | 纯代数类型层 | `TypeSpec` / `Layout` / `unify` / `ReflectError` / 单态化上界（no_std） |
| ze-reflect | 运行时反射 | `SlotTable` / `SymbolTable` / `TypeRegistry` / `Codec` / `MonoCache` |
| ze-graph-core | 纯拓扑图算法 | `Topology` / `topo_sort` / `scc` / `reachable` / `dirty_downstream`（只认节点 id + 边） |
| ze-notion | 数学与几何基元 | `Path2D` / `Transform` / `Vec` 运算 |
| ze-render-ir | 渲染 IR 契约 | `DrawOp` / `Paint` / `UniformLayout` |
| ze-scene-contract | 场景契约 | `ObjectId` / `Describable` / `Scene` 契约 |
| ze-content | 内容编译 | `ContentIr` / SVG / 笔画 |
| ze-prerender | 预排版 | `PrerenderCache` / Typst 适配（feature-gated） |
| ze-time | 时钟与曲线 | `ClockId` / `Curve` / `Timeline` 纯数据 |
| ze-comb | 组合子表达式内核 | `Expr` / `CompiledExpr` / `PassRegistry` / `Value` |
| ze-func | 基础函子库 | 算子目录 `OP_DEFS` / `eval` / `TypeRegistry` 门面 |
| ze-vertex | 顶点格式 | `VertexLayout` / `VertexInputDesc` |
| ze-resource | GPU 资源抽象 | `GpuResourceId` / `ResourceCache` |
| ze-macro | 通用派生宏 | `#[derive(Describable)]` |
| ze-pod-core | Pod 抽象 | `Pod` / `Handle` / `FunctionId` / `ChannelDecl` |
| ze-pod-binding | 语言无关绑定驱动 | 能力计划 / JSON 文档 |
| ze-pod-macro | Pod 派生宏 | `#[capability]` / `#[derive(PodType)]` |
| ze-pod-func | `zela.math` Pod | 三语言接线 + 装配期类型推导 |
| ze-pod | Pod 门面 | re-export core + binding + macro |
| ze-pod-lua | Lua 适配器 | Lua 绑定生成（平台相关） |
| ze-pod-py | Python 适配器 | Python 绑定生成（平台相关） |
| ze-pod-js | JS 适配器 | boa 绑定（native + wasm） |
| ze-pod-tests | Pod 测试与基准 | 集成用例 / criterion 基准（publish = false） |
| ze-scene-core | 场景核心 | 组件 / 动画 target 表 `TARGETS` / 脚本数据 |
| ze-scene-api | 场景 Pod 能力层 | `SceneApi` / `ContentApi` |
| ze-scene-macros | 场景派生宏 | `#[derive(Channels)]` / `#[derive(SceneComponent)]` |
| ze-scene-hecs | 场景 ECS 契约 | 只依赖 hecs 的薄层 |
| ze-node-core | 节点图 trait 层 | `Graph` / `Node` / `NodeMeta` / socket |
| ze-graph-algo | 图算法实现 | Kahn / Tarjan / BFS dirty |
| ze-graph-eval | 图求值策略 | `LazyPullEval` |
| ze-node-schema | 动态节点与属性 | `DynamicNode` / `Property` |
| ze-node-schema-macros | 动态节点派生宏 | `#[derive(NodeSchema)]` |
| ze-node | 节点图门面 | re-export core + schema + algo + eval |
| zela-scene-ecs | 场景 ECS 实现 | 系统 / 绑定应用 / 表达式单子 |
| zela-scene-bundle | 产物与输入流 | ZSB 编解码 / 帧哈希 / 输入流 |
| zela-schedule | ECS 调度框架 | `Scheduler` / `Resource` / telemetry |
| zela-render | 渲染抽象 | 绘制队列 / 三角化 / 视口 |
| zela-render-backends | wgpu 后端 | 2D / 3D / 合成器 |
| zela-render-vector | 矢量后端 | 移植自 manimgx；native 同步读回，wasm32 只开 `*_async`（B15） |
| zela-egui | egui 前端 | 画面宿主 / 健康检查 |
| zela-node-editor | 节点编辑器状态机 | 命中测试 / 布局 / undo |
| zela-node-egui | 节点编辑器 egui 视图 | canvas / painter |
| zela-node-egui-ui | 节点编辑器 UI 组件 | 菜单 / 视图 |
| zela-app | 应用组装 | 唯一可执行入口 / 导出 |
| zela-authored | 内置作品 | demo3d / storyboard / capability demo |
| zela-playback | 回放 | 输入流 / 矢量帧 |
| zela-commands | 命令帧体（宿主无关） | `CommandSpec` / `CommandHost` / `FrameSink` / `Response`（不含 clap / egui） |
| zela-cli | `zela` 伞命令 | `CommandSpec → clap` 适配 / 终端宿主 / PNG / mp4 sink |
| zela-gui | GUI 入口 | eframe 宿主 |
| zela-web | wasm 门面 | 浏览器入口 |
| zela | 应用层门面 | re-export ze-pod + ze-node |

---

## AY. 仓库门面批次（命令层 / 断点续渲 / doctor / examples 门禁，2026-10-29）

> 计划《仓库门面批次_执行计划》。判据：命令层不依赖 UI；默认值不改像素；
> 断点续渲与一次性渲染逐位相同。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AY-01 | 命令层独立 crate + 声明式 `CommandSpec` | app | **已关闭（0）** | `crates/app/zela-commands`：`CommandSpec` / `CommandHost` / `FrameSink` / `Response`；其 Cargo.toml 无 clap / egui / eframe；`render` / `list` 经内存宿主与 CLI 宿主跑通（`cargo test -p zela-commands -p zela-cli`） |
| AY-02 | 默认值快照回归网 | app / gui | **已关闭（0）** | `zela-render` / `zela-verify` / `zela-parity` / GUI 启动项各带 `defaults_snapshot`：锁定 fps 30、640×480、`out.mp4`、`auto`、`wgpu`（verify 的 `--work` 缺省）等；`zela-compile` 的默认值由 `zela-commands::compile` 用例承接 |
| AY-03 | `zela` 伞命令 + 删 `zela-compile` crate 与 `zela-gui` bin | app / xtask / web | **已关闭（0）** | `zela-cli`（bin `zela`）暴露 render / frames / compile / check / inspect / api / fingerprint / doctor / fonts / list / gui；xtask 的 `zela-compile` 调用（demo / compile_bundle）与 `build.ps1` 改走 `zela compile`，`export_scene_api` 改走 `zela api` |
| AY-04 | 断点续渲 | app | **已关闭（0）** | `--frames` 写 `.zela-render.json`（参数指纹 + 每帧 SHA-256）；恢复时 `Seek` 到边界并回渲 1 帧校验逐位一致，指纹不符拒绝续渲。实测：`frames smoke.lua` 渲满 15 帧后把 manifest 截到 5 条，重跑输出「从第 5 帧继续」且边界帧哈希与首次渲染相同，manifest 回到 15 条 |
| AY-05 | `doctor` 查出字体静默回退 | app / kernel | **已关闭（0）** | `zela doctor` 在缺字体时 Warn 并指向 `zela fonts`；`fonts` 用 curl 下载 + SHA-256 校验，并自动设置 `ZELA_TYPST_FONT_DIR` |
| AY-06 | 使用者命令补全 + 帧范围 | app | **已关闭（0）** | `check` / `inspect` / `list` / `play` 与 `--from` / `--to`；`render` 与 `frames` 共用同一帧体（`run_frames`） |
| AY-07 | examples 记录门禁 | xtask | **已关闭（0）** | `check_examples_recorded` 纳入 `--step structure`：每个 example 必须在 README / docs 有记录，或用 `required-features` 隔离（`send_probe`） |
| AY-08 | `dev verify` / `dev parity` 子命令 | app | **未执行（3）** | 两个内部 bin 仍在 `zela-app`，xtask 继续调用；迁入 `zela dev` 需先评估门禁调用点（本轮按"GUI/codegen 批次优先"让路） |
| AY-09 | README 面向使用者重写 + docs 角色入口 | docs | **已关闭（0）** | README 删除架构章节，改为定位 / 5 分钟 / 第一个动画 / 能力索引 / 角色入口；角色表唯一定义在 `docs/README.md` 的「按角色阅读」 |
| AY-10 | 全批像素与结构逐位回归 | render | **已关闭（0）** | 本机实测：`cargo verify --step hashes` 合计 36 项全部 ok（DIFF 0）；`--step structure` 全绿（含新门禁）；`--step docs` / `--step check` 全绿 |

**AY 段小计**：已关闭 9（AY-01–AY-07、AY-09、AY-10）· 部分关闭 0 · 未执行 1（AY-08）。

---

## AZ. 文档用语统一（补充计划，2026-10-29）

> 计划《补充计划_文档用语统一》。判据：只改叙述、不改事实；模块文档自动区零变化；
> 禁止词门禁真的会红。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| AZ-01 | 禁止词门禁 | xtask / docs | **已关闭（0）** | `xtask/src/lib.rs` 的 `FORBIDDEN_TERMS` + `forbidden_term_hits`；挂在 `--step docs`，另有单跑入口 `cargo run -p xtask -- terms`；命中打印文件、行号、建议改写；白名单 `FORBIDDEN_TERM_ALLOW` 按词 + 文件 + 理由登记（现行唯一一条见 BA-04） |
| AZ-02 | 来源用词统一 | docs / 根 md / 代码注释 | **已关闭（0）** | 唯一来源 / 第二个来源 / 定义在 `X` 三种说法；叙述范围（docs + 根目录 `.md`）清零；代码注释只改来源类（57 处），约束词按分档保留；保留 1 处错误信息字符串（非叙述）。模块文档自动区只随 `//!` 首行同步变化，无其他自动区改动 |
| AZ-03 | 自造词替换 | docs / 根 md | **已关闭（0）** | GAPS 第 4 类状态词由「档案化」改为「已记录」，`check_gaps_subtotals` 的桶判定 / 标签 / 小计行同步；其余替换为锁定、收敛 / 定案、指出、待核、无引用、作者侧、强制约束、门禁（逐词见 `FORBIDDEN_TERMS`） |
| AZ-04 | 优先级下调词与 degrade 消歧 | GAPS / docs | **已关闭（0）** | 优先级下调统一写「降为工效项」；degrade 的「降级」保留（由 `ze-render-ir::degrade` 支撑），两者不再混用 |
| AZ-05 | 「静默」收量 | docs / 根 md | **已关闭（0）** | 文档口径 127 → 12 处，只在「静默失败 vs 显式失败」对立处保留；门禁超 20 只警告（术语表计数豁免），不阻断 |
| AZ-06 | 代码注释的分档 | crates/**/*.rs | **已关闭（0）** | 只改「来源」类用词（57 处注释）；「强制约束 / 必须」等失效模式说明保留——文档是叙述，注释是约束 |
| AZ-07 | 加粗密度与语气 | docs / 根 md | **已关闭（0）** | `docs/principles.md` 加粗 56 → 0；CONTRIBUTING「铁律摘要」→「约定摘要」；ARCHITECTURE「任何影响…必须」→「凡影响…都」；docs 人称代词 0 |
| AZ-08 | 「记账」的裁决 | GAPS / docs | **已关闭（0）** | 保留：它是本仓核心工作方式；核查后未混入需要统一的同义词（记录 / 登记语义不同） |
| AZ-09 | 门禁误报处理 | docs / xtask | **已关闭（0）** | 决策 007 的文件名含旧词 ⇒ 改名为 `007-作者侧.md` 并同步 4 处链接；其余无误报（本批为此新增的白名单见 BA-04） |

**AZ 段小计**：已关闭 9（AZ-01–AZ-09）· 部分关闭 0 · 未执行 0。

---

## BA. 验收收尾批次（2026-10-29）

> 计划《验收收尾批次_执行计划》，依据验收报告的 P1 / P2 / P5。判据同前：
> 只改叙述、不改事实；像素与结构逐位不变；门禁真的会红。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| BA-01 | 「铁律 N」编号体系的定义已消失（27 处引用） | docs / 代码注释 / GAPS | **已关闭（0）** | 定案：**去掉编号**（选项 ②）。旧版 CONTRIBUTING 的摘要本就是 5 条无编号列表，编号没有任何可指回的定义；重建编号等于把「约定」做成第二张必须维护的表。27 处逐条改写为语义内联：`spec.md` / `tasks.md` 的「铁律 2/3/4/5」→ 四条约定的名字，`GAPS.md` B-03 / B-06、`ARCHITECTURE.md` 的「铁律 4」→「未改顶点格式」。xtask 的「编号引用按行豁免」同步删除（`numbered_rule_reference` 移除）⇒ 任何新出现的「铁律」都直接红，不再有「看起来合法」的编号形态 |
| BA-02 | 条目正文里的附带义务必须有独立编号 | GAPS / xtask | **已关闭（0）** | AX-02 正文原有的「需重录」不进小计 ⇒ AX-07 补编号并进 AX 小计；纪律写入本文件「统计与维护约定」。读数：`--step demo` 28/28、`--step hashes` 36/36、结构 22 项不变（本批末实测） |
| BA-03 | 禁止词表不限于清单：近义词会漏网 | docs / xtask | **已关闭（0）** | 本批补入「钉」字根的近义词并逐处改写，按「钉 / 锚 / 锁 / 真」扫过一遍（其余命中按语境处理）；纪律写入 `docs/README.md` 的维护约定（每批收尾扫一次字根） |
| BA-04 | 白名单现状 | xtask / GAPS | **已关闭（0）** | `FORBIDDEN_TERM_ALLOW` 仅一条：`GAPS.md` 豁免「铁律」（历史记账引用原名，改名会丢失可追溯性）；其余禁止词无豁免 |

**BA 段小计**：已关闭 4（BA-01、BA-02、BA-03、BA-04）· 部分关闭 0 · 记账 0 · 未执行 0 · 已记录 0。

---

## BB. 算子扩充、糖层与能力面批次（2026-10-29）

> 计划《算子扩充糖层与能力面_执行计划》。定位：做作品之前的最后一批地基。
> 口径：**每阶段可交付可测试**；像素 38 条与结构 22 项逐位不变（阶段 2 的 manifest 除外）；
> 加算子只 bump 一次 `LIBRARY_VERSION`。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| BB-01 | crate README 不在用语门禁范围内（33 个 README 零扫描） | xtask / docs | **已关闭（阶段 0）** | [`repository_markdown_docs`](xtask/src/main.rs) 追加 `crates/**/README.md`；清掉 19 处命中（强制约束 9、约定 8、定案 2；`ze-notion` 的「验收方式」原写作含该串的近义词，一并改写）。可验证：往任一 crate README 插一个禁止词 ⇒ `terms` 红 |
| BB-02 | AF–AK 六段不在 `check_gaps_subtotals` 的前缀表里 | xtask / GAPS | **已关闭（阶段 0）** | 前缀表补 `AF.`–`AK.`，`is_item` 补 `AF-`–`AK-`（两处缺一即漏账）。纳入后第一次运行即抓到真错：AF 段把小计里的 `AF-06`（实际部分关闭）记成记账 ⇒ 改为 `部分关闭 1 · 记账 5` |
| BB-03 | 用语门禁的扫描范围只在代码里，`WASM_PORT_SURVEY.md` 的排除理由没有对外说明 | docs | **已关闭（阶段 0）** | [`docs/README.md`](docs/README.md) 的维护约定新增「扫描范围」段：根 `*.md` + `docs/**` + `crates/**/README.md`，唯一排除项 `WASM_PORT_SURVEY.md`（历史快照，文件头有可见声明），并约定范围变更必须同步该段 |
| BB-04 | BA-01 定案：去编号 | docs / xtask | **已关闭（阶段 0）** | 27 处「铁律 N」逐条语义内联（`spec.md` / `tasks.md` / `GAPS.md` B 段 / `ARCHITECTURE.md`）；xtask 的编号豁免函数 `numbered_rule_reference` 删除 ⇒ 编号形态不再被当成合法写法。BA-01 条目已同步关闭 |
| BB-05 | 超越函数落到平台 libm ⇒ 同一表达式跨平台可能差 1 ulp | kernel / 表达式 | **已关闭（阶段 1）** | 新增 [`ze-func::det`](crates/kernel/ze-func/src/det.rs)：`sin` / `cos` / `tan` / `asin` / `acos` / `atan` / `atan2` / `exp` / `exp2` / `ln` / `log2` / `pow` 一律走 `libm`（新增第 27 个外部依赖，0.2.16，wasm 同一份代码）。**实测 1 ulp 确实存在**：`libm::sin(0.117) = 0x3fbde23ae6d763c7`，std（MSVC libm）`= …c6` ⇒ 表达式层与参考求值器统一到 `det`。判据 [`det_transcendentals_are_bit_stable`](crates/kernel/ze-func/src/det.rs)：120 个结果位模式折叠成常量 `14333979151148114719`，换实现即红。**边界（明说）**：只覆盖 `zela.math` 算子语义 + `rotate_rodrigues`；几何细分（`ze-notion` / `zela-render` / `zela-render-vector`）仍用平台 std —— 换掉会改像素，属另一个决策（不在本批）。CPU / GPU 口径：**CPU 逐位确定，GPU 容差内，二者不对拍到等式**。⚠️ 顺带发现：`serde_json` 的 f64 解析对 17 位十进制会差 1 ulp（`0.11673324714446585` 解成 `…c6`）⇒ `transcendental.json` 的输入改为十进制可精确表达的点（`0` / `0.5`），期望值随 libm 重算 |
| BB-06 | 算子面缺 28 个 WGSL 标准库算子（`sqrt` / `pow` / `step` … 全部没有） | 表达式 / 函子库 | **已关闭（阶段 2）** | 一次加完、**只 bump 一次**：`LIBRARY_VERSION 21 → 22`、`zela.math@21 → @22`、`MATH_API_FINGERPRINT = 5482242184379522694`。新增 29 条目录行（28 个算子 + `narrow_f32`）：代数与舍入 `sqrt` / `inversesqrt` / `abs` / `sign` / `min` / `max` / `floor` / `ceil` / `trunc` / `round` / `modulo`，指数与三角 `exp` / `exp2` / `log` / `log2` / `pow` / `tan` / `asin` / `acos` / `atan` / `atan2` / `degrees` / `radians`，向量与阶跃 `step` / `dot` / `cross` / `reflect` / `faceforward`。**三处强制同步**：① WGSL 生成规则逐条补齐（`every_operator_has_a_wgsl_rule` 遍历目录，缺一条即红；`dot` / `distance` 复用 `length(a - b)` 同式）；② 三路逐位对拍（`typed_equivalence` 新增 34 个用例，含两个 `step` 边界与 `sign(0)`）；③ 声明文件两次生成逐字节一致（`--step docs`）。**作者侧一次展开**：`ze-pod-func` 的 `numeric_ops!` 宏用一行同时产出 ctor / 注册 / 慢路径分派，`api.rs` 的 `numeric_specs!` 用一行产出参数结构体 ⇒ AU-11「改 1 处」的愿望在**内核侧**达成（PodType 与字段仍要各写一次，记在这里）。**更名**：`round_f32` → `narrow_f32`（旧名保留为废弃别名，`changed` 仍为 12），WGSL 侧两者都是恒等。**命名偏差**：`mod` 在 Rust 里是关键字、能力字段名又必须等于算子名（D14 门禁）⇒ 算子名取 `modulo`（WGSL 侧没有 `mod` 函数，生成 `%`，口径不受影响）。**验收**：`--step demo` 28/28（表达式作品 payload 因库版本重录，像素不变）|

| BB-07 | 每个 Demo 都要把参数结构体写满（`Material3D` 每次 7 个字段、`Camera2D` 每次 2 个） | 脚本绑定 / 作者侧 | **已关闭（阶段 3）** | `field_default` 从 7 条扩到 **18 条**：`Camera2D.center`（17/21 个作品写 `(0,0)`）、`Camera3D.target` / `up`（20/21 写 `(0,0,0)` / `(0,1,0)`）、`Transform.z`（14/18 写 `0.0`）、`Material3D` 六项（`variant=Lambert` 6/19、`metallic=0.0` 16/19、`roughness=0.5` 8/19、`double_sided=false` 16/19、`transparent=false` 16/19、`texture=None` 17/19）。**判据（全批验收 #6）**：把 26 个脚本里与默认值相同的字段删掉后，`--step demo` **28/28 payload 逐位不变**（默认值在装配期补进 `PodValue::Struct`，字段顺序由类型声明决定 ⇒ 与手写版同字节）。**明确不做**：`Light` 不加默认 —— 15 处光源的颜色 / 强度逐个不同，没有"普遍值"可对齐（写一个没人用的默认值等于未测代码）。声明文件同步把可选字段标成 `?` 并写明默认值 |
| BB-08 | 13 个脚本各写一份 `ramp` / `ramp2` / `sin_curve` / `orbit2d` / `f32` / `rgba`（同一概念 N 份实现） | 表达式 / 脚本 API | **已关闭（阶段 4）** | 糖层用 Rust 写（三语言共用），挂在**独立能力** `zela.sugar`（`ramp` / `ramp2` / `orbit2d` / `wave` / `f32`）与 `zela.scene`（`rgba` / `vec2` / `vec3`）。**为什么独立 namespace**：加进 `zela.math` 会改它的 `signatures()` ⇒ 必须 bump `LIBRARY_VERSION` ⇒ 所有表达式 payload 变化，与"糖不进 payload"冲突。为此三个脚本 Pod（Lua / JS / Python）的绑定表从「Service → 一个能力」改为「Service → 多个能力」，新增 `register_capability_as(service, global, required)`（同一 Pod 可以暴露两个表）。**判据（全批验收 #5）**：把 22 个脚本里的 helper 全部换成糖（另把 110 处单行 `Vec2{…}` / `Vec3{…}` 字面量换成 `scene.vec2/vec3`，净删 405 行）后，`--step demo` **28/28 payload 逐位不变**；用例 `sugar_expands_to_the_hand_written_core_form` 直接比对"糖"与"手写 core"的表达式 JSON 逐字节相同。**穿透门禁**：`crate::sugar::SUGAR` 每条带等价 core 写法，用例要求「能力表 ↔ 文档表一一对应且 `core` 非空」（删一条 ⇒ 红）。**命名**：脚本里写 `sugar.ramp(...)`（不是裸 `ramp(...)`）—— 与表构造器形式的差异一眼可见；`mod` 式的关键字冲突也一并避开。**偏差**：糖 `f32(x)` 展开成旧算子名 `round_f32`（保 payload 逐位）；等下一次允许重录时再切到 `narrow_f32`（触发条件：任何一次会改 payload 的批次） |

| BB-09 | 存根的参数侧一律 `Expr<any>`（= 隐式接受任意表达式） | 导出 / 脚本 API | **已关闭（阶段 5，方案 a）** | 新增 [`lua_op_param`](crates/ze/pod/ze-pod-lua/src/export.rs)：算子 spec 的每个字段按 `OpSig` 逐条展开 —— `Kind::Num ⇒ Expr<number>`、`Kind::Vector ⇒ Vec2 或 Vec3`（写成 `Expr<Vec2>` 与 `Expr<Vec3>` 的联合）、具体 `Vec2` / `Vec3` / `Bool` 同理 ⇒ `Mul` 的参数是 `Expr<number>` / `Expr<Vec2>` / `Expr<Vec3>` 三种候选，`Dot` 是 `Expr<Vec2>` / `Expr<Vec3>`；`---@field` 与 `---@overload` 用**同一份**口径（否则表构造器与位置参数两种写法会诊断不一致）。用例 `lua_stub_expands_vector_parameters` 断言具体候选存在且 `fun(a: Expr<any>, b: Expr<any>)` 已消失。**范围**：只做 LuaLS 存根；`.d.ts` / `.pyi` 仍是 `any` —— 方案 b（按 `sigs` 逐条展开笛卡尔积、处理多个 `VAR_A`）继续记账。**产物不受影响**（存根是导出物，不进 payload） |

| BB-10 | `Bind.duration` 的展开是 `t < d`，与"段"的语义差一个左端点；62 处绑定手写 `Predicate.Of { clock.range(0, d) }` 样板 | 场景 / 表达式 / 脚本 | **已关闭（阶段 6）** | `duration > 0` 现在展开成 **`clock.range(0, d)`**（`And [Ge(t,0), Lt(t,d)]`，见 [`duration_predicate`](crates/scene/ze-scene-core/src/scene_script.rs)）——与脚本手写逐字节相同，因此把 62 处样板 predicate 换成 `duration = d` 后 payload **逐位不变**（`--step demo` 28/28 通过）。**展开位置**：在 `zela-app` 的装配期、**收集算子 revision 之前**（否则产物里会少 `and` / `ge` / `lt` 三条 revision —— 实测首次尝试就踩到：`fusion` 的 payload 因此变化）。**语义差异**：旧的 `Lt(t,d)` 在 `clock.shift` 造成的负局部时间里也算"生效"，新形状多了 `Ge(t,0)` ⇒ 左端闭合，段前不再误写。**payload 影响**：3 个作品（`fusion` / `atlas` / `expr_demo`）原本就用 `duration`，展开换了形状 ⇒ manifest 重录（像素不变）。**明确不做**：`at = { start, duration }` 合成糖、`sequence` 时序糖（与 5b 退役边界近，计划 §12 明确不做） |

| BB-11 | 能力面：Stroke 线宽 / Fill 颜色 / ZOrder / Visibility / Surface 系列都不能被动画驱动 | 场景 / 动画 / 脚本 | **部分关闭（阶段 7）** | **已暴露**：`Stroke.width`（`scalar_spec!` 一行 + `CHANNEL_HOMES` 把存储组件 `StrokeSolid` 映射回脚本组件 `Stroke`）⇒ 常量 `Stroke.channel.width`、目标地址 `Stroke.width`、`expr.bind` 可直接驱动；作品级证据 [`cap_stroke_width.lua`](crates/app/zela-app/scripts/cap_stroke_width.lua)（两个环的线宽分别由糖 `sugar.ramp` 与新算子 `abs` / `pow` 驱动），登记进 manifest（payload 断言 29/29）与 dispositions（`exempt` + 理由）；`--step coverage` **58/58**。**未暴露（逐条记账，不是漏掉）**：① `Fill.color` —— `AnimValue::Color` 存在，但表达式层的 `ChannelType` 只有 Number / Bool / Vec2 / Vec3 ⇒ 通道常量与 `expr.read` 没有 Color 类型，补它要改 `zela.math` 签名（bump + 全量重录）⇒ 下一批；② `Visibility` —— 需要 `AnimValue::Bool`（曲线插值语义要重新定义）⇒ 未执行；③ `ZOrder` —— 存储是整数、`AnimValue` 只有 f32 标量（取整 coercion 可做），但改绘制顺序会让像素基线语义需要重审 ⇒ 单独立项；④ `Surface` 系列 —— 字段是**资源创建参数**（texel 尺寸 / 过滤 / mipmap），运行期改需重建纹理，不是"每帧写字段" ⇒ 明确不做。触发条件：①/② 与下次允许重录同批；③ 独立计划；④ 等 GPU 侧资源热替换。**结构基线重录（如实记账）**：`fingerprint_world` 逐帧遍历 `TARGETS` ⇒ 新增一个通道**必然**改变含该组件作品的每帧结构摘要（本批只有 `atlas` 有 `Stroke`）⇒ `structures.txt` 用 `ZELA_STRUCTURE_RECORD=1` 重录一次；**像素基线 36/36 不动**（新增通道不改渲染结果）。这与"结构 22 项逐位不变"的字面要求不同：那份要求写在"只加 Demo、不加通道"的前提下，而阶段 7 的本体就是加通道 ⇒ 重录是本批唯一诚实的处置 |

**BB 段小计**：已关闭 10（BB-01…BB-10）· 部分关闭 1（BB-11）· 记账 0 · 未执行 0 · 已记录 0。

---

## BC. Parent 多维度连续继承批次（2026-10-30）

> 方案《方案_Parent多维度连续继承.md》。定位：把 `Parent` 从"只传变换"扩成
> 变换三维 + 样式三维的连续继承；默认权重让现有作品逐位不变。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| BC-01 | `Parent` 只有一个字段，继承权重无处表达 | 契约 / 脚本 API | **已关闭** | `ze_scene_contract::relation::ParentInherit` 六个 `f32`（`position` / `rotation` / `scale` 默认 `1.0`，`opacity` / `fill` / `stroke` 默认 `0.0`）+ `none()` + `full_transform()`；`ze_scene_api::types::Parent` 加同名字段；`ze_pod_binding::defaults::{field_default, field_default_literal}` 各加 6 条 ⇒ 13 处 `Parent { parent = h }` 与 26 个脚本源码零改动。组件登记表加入 `ParentInherit`（29 → 30 个类型，`clone_known_covers_every_registered_type` 同步）；`scene_service::scene_parent` 一次插入 `Parent` + `ParentInherit` 两个组件 |
| BC-02 | 派生态只产世界矩阵，父链的样式不继承 | 派生 / ECS | **已关闭** | `ze_scene_core::world_inherit::WorldInherit`（`opacity` / `fill` / `stroke`）挂在 `WorldTransforms::{with_inherit, inherit}` 上，与矩阵在**同一次**父链遍历里产出（不新增第二个资源：`zela_schedule::ResourceTuple` 没有 5 元组）。`zela_scene_ecs::transform_hierarchy::{mix_transform, inherit_style_of, inherit_color}`：position 线性、rotation 四元数 slerp、scale 线性；三个变换权重全 `1.0` 时整份复制父矩阵 ⇒ 逐位等于旧行为；不透明度 `local × mix(1, parent, w)` 乘性累积；颜色在 `w > 0` 时按权重混合，子缺席则继承父色。用例：`opacity_inherits_when_weight_is_one` / `opacity_not_inherited_by_default` / `fill_inherits_when_weight_is_one` / `full_transform_weights_match_manual_composition` / `zero_transform_weights_ignore_parent` / `half_position_weight_halves_translation` |
| BC-03 | 消费层只看本地 `Opacity` 与填充 / 描边色 | 渲染 | **已关闭** | `zela_render::draw::opacity_of` 增加继承入参（缺席回退本地 `Opacity`）；新增 `apply_inherit` 先覆盖填充 / 描边色，再调用 `apply_opacity` —— 全仓唯一施加点不变；`surface_route::try_record_direct` 同步；`MorphRecordSystem` / `RevealRecordSystem` / `ContentTextureRecordSystem` 经 `WorldTransforms.inherit()` 取结果。默认权重下继承是空操作 ⇒ 渲染结果不变 |
| BC-04 | 六个继承权重不可动画 | 动画 / 脚本 | **已关闭** | `ze_scene_core::animation::target` 新增 6 条 `scalar_spec!` + 6 条 `make_target!` + `TARGETS` 6 项（地址 `Parent.position` / `Parent.rotation` / `Parent.scale` / `Parent.opacity` / `Parent.fill` / `Parent.stroke`）。作品 [`cap_parent_inherit.lua`](crates/app/zela-app/scripts/cap_parent_inherit.lua)：子 A 驱动 `opacity` / `fill` / `stroke`，子 B 驱动 `position` / `rotation` / `scale`；登记进 `demo/manifest.txt` 做 payload 断言，并在 `dispositions.txt` 记显式豁免理由。`--step coverage` **70/70**；`--step demo` **30/30**。父链作品（fourier / orbit / pendulum / atlas / surface_matrix / surface_matrix_js）payload 因新增 `ParentInherit` 组件重录；`structures.txt` 因登记表与六条通道重录；像素基线逐位不变（默认权重不改变任何渲染输入） |
| BC-05 | 描边线宽与 ZOrder 的继承本批不做 | 渲染 / 动画 | **记账** | `stroke` 权重只管颜色（`ze_render_ir::StrokeSolid::color`）；线宽要独立权重，会继续膨胀 `Parent` 字段；`ZOrder` 继承会改绘制顺序，与 BB-11 ③ 同因。触发条件：文字组落地时评估线宽继承；ZOrder 单独立项 |
| BC-06 | 覆盖门禁的能力抽取只认五参形态，宏写法的通道被无提示漏掉 | xtask / 门禁 | **已关闭** | `xtask::parse_spec_invocation` 同时接受五参（手写）与六参（`scalar_spec!`：组件名 + 组件类型）两种形态；六个 `Parent.*` 通道因此进入覆盖矩阵，并连带补回此前漏掉的 `Dash.*` / `CurveWindow.*` / `Stroke.width`（宏形态通道共 12 条）。修复后 `--step coverage` 从 58/58 变为 **70/70**（全部被现有作品使用，豁免 0 条）——旧口径下这些通道"没有义务"，正是门禁要防的安静漏检 |

**BC 段小计**：已关闭 5 · 记账 1。

---

## BD. 动画层补全 · 阶段 0（收尾与值类型核查，2026-10-30）

> 计划《动画层补全_执行计划》阶段 0。定位：把上一批的遗留项收干净，并核查
> `Fill.color` / 组可见性两个"看起来只差一行"的值类型缺口。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| BD-01 | 代码 / Cargo / 脚本里仍有「铁律 N」编号引用（BA-01 只清理了文档） | 全仓代码注释 | **已关闭** | 逐处改为语义内联（"不静默" / "唯一来源" / "依赖约定" / "不得改顶点格式" / "不暴露后端细节"），共 53 处：Rust 注释、14 个 `Cargo.toml` 依赖段、4 个脚本注释、`zela-web/build.ps1`、`.github/workflows/pages.yml`。`rg 铁律 crates` 无结果；GAPS 历史记账里的原名保留（可追溯性） |
| BD-02 | `narrow_f32` 零使用（脚本与糖仍走旧名 `round_f32`） | 表达式 / 糖 / 脚本 | **已关闭** | `sugar_narrow` 改调 `narrow_f32`；`sugar.rs` 的 core 文档同步为新名；9 个脚本的说明注释同步。只换算子身份、不换实现（`round_f32` 仍委托 `narrow_f32`）⇒ 像素不变；受影响的 10 件作品（fourier / orbit / pendulum / pbr_materials / pbr_lights / pbr_scene / capability_gaps / cap_channels2d / cap_stroke_width / expr_read）payload 随算子 revision 重录 |
| BD-03 | `Fill.color` 通道 | 表达式 / 动画 | **记账** | 值映射可做（`AnimValue::Color` ↔ `Value::vec4`），但**没有可写的根表达式**：`ze_comb::Literal` 到 `Vec3` 为止，`expr.pure` 只收 f64，表达式层也没有 `Vec4Of` 算子 ⇒ 通道常量即使挂上也写不出值。触发条件：加 `Vec4Of` / `Vec4` 字面量（会改 `zela.math` 签名，需 bump 与全量重录）时同批暴露 |
| BD-04 | 组可见性开关（bool） | 动画 / 渲染 | **记账** | 两个前提都不成立：`AnimValue` 没有 `Bool`；`GroupVisibility` 是**全局 Resource**（`hidden: HashSet<GroupId>`），不是对象组件，而 `Target` 只能按 `ObjectId` 写组件字段。要做成可动画需要新增"组开关组件 + 折叠进 Resource 的 System"，属独立机制。触发条件：文字组 / 组级淡出需要驱动整组开关时单独立项；对象级淡出继续用 `Opacity` |
| BD-05 | `cap_codegen` payload 未变，确认是否真的走 codegen | 渲染 / Demo | **已关闭** | 作品用 `expr.shader { object, root }` 提交 `ShaderPaint`；`zela_render::draw::paint_of` 把它放进 `style.paint.shader`，后端在光栅前用 compute pass 写填充槽（`emit_wgsl` 生成 `eval`）。payload 未变是因为该作品不含本批改名的 `narrow_f32` / `round_f32`，不是"没走 codegen" |
| BD-06 | BB-11 的结构基线申报只提 atlas | 记账 / 基线 | **已关闭** | BB-11 原文按"只有 atlas 有 Stroke"申报不准确：结构指纹遍历 `TARGETS`，任何含 `StrokeSolid` 的作品都会变。BC 已用 `ZELA_STRUCTURE_RECORD=1` 全量重录（22 件），本批再确认结构基线未变 |
| BD-07 | Parent 产出复核 | 契约 / 派生 / 通道 | **已关闭** | 六个 `Parent.*` 在 `TARGETS` 且被 `cap_parent_inherit.lua` 使用；覆盖门禁 70/70；现有 26 个脚本源码零改动；`--step hashes` 像素 36/36 不变（BC 批实测） |

**BD 段小计**：已关闭 5 · 记账 2。

---

## BE. 动画层补全 · 阶段 1（plot 采样，2026-10-30）

> 计划《动画层补全_执行计划》阶段 1。定位：把 AU 阶段 5 建好的逐元素求值
> （`CompiledExpr::eval_elements` + `Mono::read_element`）接出第一个真实用途 ——
> **函数图像在装配期采样成折线**。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| BE-01 | 能力形态：计划写的是 `ze-scene-api` 的 `Plot` 组件，实际改为 `expr.plot` | 表达式 / 场景装配 | **已关闭（偏差）** | 新增 `zela.math::plot { object, lo, hi, samples, y }`：脚本先 `spawn{ Stroke{…} }`，再由宿主逐点求值并在 `SceneScript::attach_polyline` 挂成 `Reveal::visible`（与手写 `Polyline` 同一条 `Path2D::from_polyline` 路径）。理由：`Plot` 组件的消费端（采样）只能在宿主侧做，做成组件会多一层"组件→采样器"的间接；改成登记式与 `expr.shader` 同构，且不需要新图元 |
| BE-02 | 自变量通道：`plot.x` 是逐元素通道 | kernel / 表达式 | **已关闭** | `ChannelRef::class` 把 namespace `plot` 归入 `ChannelClass::Element`；`expr.plot_x()` 产出 `("plot","x", Number)` 叶子，求值时经 `Mono::read_element` 拿 `lo + (hi-lo)*i/(samples-1)` |
| BE-03 | `zela.math` 签名增至 `@23`，payload 随之变化 | Pod / 产物 | **已关闭** | `LIBRARY_VERSION` 22→23、`MATH_API_VERSION` 22→23、指纹更新；`cap_plot` 之外 **19 件含表达式的作品** payload 重录（像素不变）；`zela-authored` 的两处版本常量同步 |
| BE-04 | 与手写折线**逐位相同**（阶段判据 1） | 装配 / 测试 | **已关闭** | 用例 `plot_sampling_matches_a_handwritten_polyline`：同一函数、同一结合顺序（`K*(x*x)+B`）、同一采样公式 ⇒ 两条 `Path2D.commands` 相等（不是容差比较） |
| BE-05 | `samples` 装配期固定、运行期不重采样（阶段判据 2） | 装配 | **已关闭** | 采样发生在 `PodAssembly::sample_plots`（脚本结束后一次），产物里只有顶点；`samples < 2` / 纵坐标非 Number / 表达式读了 `plot.x` 之外的通道 都是装配期显式失败 |
| BE-06 | 阶段 0 漏改 Python 侧：`fourier.py` 的 `f32` 糖仍产 `RoundF32` | 脚本 / 语言对拍 | **已关闭** | `python_fourier_matches_lua` 因此红（Lua 侧已切 `narrow_f32`）；改 `expr.NarrowF32` 后两侧逐项相等。教训同 BD-02：改名必须两语言一起扫 |

**BE 段小计**：已关闭 6。
> 本段读数（`cargo verify` 全量）：demo 31/31 · 覆盖 70/70（豁免 0）· 结构 22 件一致 · 像素 36 项 ok 36 / DIFF 0。

---

## BF. 动画层补全 · 阶段 2（路径跟随，2026-10-30）

> 计划《动画层补全_执行计划》阶段 2。定位：把「对象沿一条路径按归一化弧长移动」
> 做成场景能力 —— 原语（`Path2D::point_at` / `tangent_at`）、组件（`PathFollow`）、
> 方法（`Reveal.point_at` / `Reveal.tangent_at`）三层共用**一份**弧长实现。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| BF-01 | 能力形态：计划写 `PathFollow { path = p }`，实际是折线点表 `PathFollow { points, closed, t, orient }` | 场景 / 装配 | **已关闭（偏差）** | 点表与 `Reveal` / `Polyline` / `Morph` 同一口径（`Path2D::from_polyline`），不引入"内容路径句柄"这条额外依赖；`scene_path_follow` 就是一次 `from_polyline` |
| BF-02 | 原语层命名：计划写 `Path.point_at(t)` / `Path.tangent_at(t)`，实际挂在 `Reveal` 上 | 表达式 / 方法目录 | **已关闭（偏差）** | `Path` 在脚本侧是**值类型**（`zela.Path`），不挂构造器；方法只能挂脚本能 spawn 的组件（`Pod::methods` 的 `component`）。`Reveal` 已持有 `Path2D` 且已有 `current_pos` 先例 ⇒ `Reveal.point_at(obj, t)` / `Reveal.tangent_at(obj, t)`（arity 1） |
| BF-03 | 弧长逻辑**只有一份**（判据 1：`Reveal.current_pos` 逐位不变） | kernel `ze-notion` | **已关闭** | 抽取到 `Path2D::point_at(t, tolerance)` / `tangent_at(t, tolerance)`；`Reveal::current_pos` 改为 `point_at(end, REVEAL_TOLERANCE)`（`0.25` 常量，运算顺序与旧实现逐项一致）。像素 36 项全绿即逐位不变 |
| BF-04 | 判据 2：`t` 从 0 到 1 与手写逐帧 `Transform` 逐位相同 | ECS / 测试 | **已关闭** | `follow_matches_handwritten_transforms_bitwise`：同一份 `point_at` 的手写参照 ⇒ `x` / `y` 用 `assert_eq!` 逐位比较，`z` 保留原值 |
| BF-05 | 判据 3：`orient = false` 只写位置 | ECS / 测试 | **已关闭** | `orient_false_leaves_rotation_alone`（旋转仍是 `IDENTITY`）；`orient_true_turns_along_the_tangent` 对拍 `Quat::from_rotation_z(angle_at(t))` |
| BF-06 | 版本与基线：新增脚本组件 + 通道 + 方法 ⇒ payload 全量重录 | Pod / 产物 | **已关闭** | `zela.scene` 13→14（指纹更新）、`LIBRARY_VERSION` / `zela.math` 23→24（算子签名不变 ⇒ 指纹不动）；manifest 20 行 payload 更新（19 件既有作品重录 + 新作品首次写值，像素全绿）；`zela-authored` 两处版本常量同步 |
| BF-07 | `PathFollow.t` 进 `TARGETS` 是否改既有结构基线 | 基线 | **已关闭** | 结构摘要只对**读得到**的 `(对象, 地址)` 入哈希（缺组件直接跳过）⇒ 既有 22 件结构指纹不变；新作品无像素基线（`exempt`），故不新增结构基线 |

**BF 段小计**：已关闭 7。
> 本段读数（`cargo verify` 全量）：demo 32/32 · 覆盖 72/72（豁免 0）· 结构 22 件一致 · 像素 36 项 ok 36 / DIFF 0。

---

## BG. 动画层补全 · 阶段 3（文字 / 公式排版糖，2026-10-30）

> 计划《动画层补全_执行计划》阶段 3。定位：把四个文字作品里手写的
> 「`content.text` 编译 + 逐字形 spawn」收成一行 `scene.text(…)`，且**产物一字不改**。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| BG-01 | 落点：糖做成**前置块**（脚本级）而非能力函数 | 脚本绑定 | **已关闭（偏差）** | `ze-scene-api::LUA_PRELUDE`（`lua_prelude.lua`）+ 通用入口 `LuaPod::with_prelude`（宿主给一段脚本，绑定层不认识领域名）。做成能力会改 `zela.scene` 签名 ⇒ 全部作品 payload 改名，与"改写法不改产物"直接冲突 |
| BG-02 | 判据 1：四个作品 payload 逐位不变 | 装配 / 产物 | **已关闭** | `--step demo` 逐条断言：fourier `b2ad6beb…` / storyboard `42b5847a…` / surface_matrix `098aff13…` / vector_paths `b2f6f4f8…` 均未变；另加 `text_sugar_matches_the_handwritten_loop`（手写 vs 糖的 `fingerprint_world` 摘要逐位相等） |
| BG-03 | 判据 2：组级淡出只动容器一个字段 | 场景 / 渲染 | **已关闭** | `cap_text_group.lua`：组 A 只写 `Opacity.value`、组 B 只写 `Transform.scale`；字形的 `Parent` 由糖生成。注意样式权重缺省为 0 ⇒ 需要 `inherit_opacity = true`（`Parent { opacity = 1.0 }`）才能继承，这是 BC 批的既有口径，不是本批新增 |
| BG-04 | 判据 3：默认排版参数与手写逐位一致 | 脚本 / 排版 | **已关闭** | 位置口径写成显式规则：有组 / 有 `glyph_transform` ⇒ 内容在原点烘焙（位置由容器承担）；否则内容按 `(x, y)` 烘焙（`parent` 只管层级）。四个手写版的三种形态都由这套规则**逐位**覆盖，糖不引入新的默认偏移 |
| BG-05 | 计划标题含"公式"，本批只做文字 | 表达式 / 内容 | **记账** | `content.formula` 与 `content.text` 的参数同形，糖可以同构扩展；但当前无任何作品用公式走这套排版（现有公式都走 `RevealPath` 书写），无判据可对拍 ⇒ 等第一个消费方出现再加 |
| BG-06 | 语言面：糖只在 Lua | 语言绑定 | **记账** | 四件文字作品都是 Lua；`LuaPod::with_prelude` 是 Lua 侧的通用入口，Python / JS 没有对应机制。触发条件：出现 Python/JS 文字作品且重复手写循环时，按同样方式给对应 Pod 加前置块 |

**BG 段小计**：已关闭 4 · 记账 2。
> 本段读数（`cargo verify` 全量）：demo 33/33 · 覆盖 72/72（豁免 0）· 结构 22 件一致 · 像素 36 项 ok 36 / DIFF 0。

---

## BH. 动画层补全 · 阶段 4（时序糖 sequence，2026-10-30）

> 计划《动画层补全_执行计划》阶段 4。定位：把"手写游标 + 逐段 `expr.bind`"收成
> `scene.sequence { … }`，**纯展开**成现有的 `clock.shift(起点)` + `duration`。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| BH-01 | 落点：与 `scene.text` 同一条前置块（脚本级糖） | 脚本绑定 | **已关闭** | `ze-scene-api::LUA_PRELUDE` 增加 `scene.sequence`；不动能力签名 ⇒ 不 bump、不重录 |
| BH-02 | 判据：展开结果与手写版 **AST 逐字节相同** | 装配 / 产物 | **已关闭** | `spring.lua` 的 18 处手写绑定改为一张 `scene.sequence` 表后，payload 仍为 `a1e3bcbd…`（`--step demo` 断言；像素基线 `spring` 亦不变） |
| BH-03 | 不恢复旧 `scene.animate`、不引入隐式全局游标 | 脚本 / 设计 | **已关闭** | 游标是 `scene.sequence` 的**局部量**：`at` 显式起点 ⇒ 不推进；缺省 ⇒ 推进 `duration`；`wait` 只推进不绑定；返回值 / `start` 是唯一的跨调用接续方式（无模块级状态） |
| BH-04 | 并行段（同段多目标）怎么表达 | 脚本 / 糖 | **已关闭（偏差）** | 计划的示例只有"一段一条绑定"。`spring` 的一个段同时写 position / scale / opacity ⇒ 新增 `at`：显式起点、且**不推进游标**（同段共享起点）。三只球的错峰间隔仍是 `cursor = cursor + 0.4`（与手写版同一算术，浮点位不变） |
| BH-05 | 新作品 `cap_sequence.lua` 覆盖三种段形态 | Demo | **已关闭** | 顺序推进 / 并行（`at`）/ 空档（`wait`）各一条；payload `d7ad9c31…` 由 manifest 断言，像素基线待录制设备批次（`exempt`） |

**BH 段小计**：已关闭 5。
> 本段读数（`cargo verify` 全量）：demo 34/34 · 覆盖 72/72（豁免 0）· 结构 22 件一致 · 像素 36 项 ok 36 / DIFF 0。

---

## BI. 动画层补全 · 阶段 5（GPU plot：只登记，2026-10-30）

> 计划《动画层补全_执行计划》阶段 5 明确要求：**本阶段不实现**，只写清触发条件。
> 现状（阶段 1）：`expr.plot` 在**装配期**逐点求值（`CompiledExpr::eval_elements`），
> 采样结果作为 `Path2D` 顶点进产物；运行期零重采样。

| 编号 | 立约 / 缺口 | 影响层 | 类别 | 证据 |
|---|---|---|---|---|
| BI-01 | 触发条件：什么情况下才值得把 plot 搬到 GPU | 渲染 / 表达式 | **记账** | 「需要**放大看曲线**」时：装配期采样点数是**静态**的（`samples` 进产物），放大后线段可见。若作品只在固定取景下播放，CPU 采样即够 ⇒ 不启用 |
| BI-02 | 机制依赖 | 表达式 / 后端 | **记账** | 复用阶段 1 的同一套机制（逐元素求值 + `plot.x`），只把**求值位置**从 CPU 换到 GPU（每个控制点按屏幕尺度重算，或逐片元 SDF 求值）。不是新语义，是同一语义的搬家 |
| BI-03 | 与"上界进产物"的口径冲突 | 产物 / 契约 | **记账** | 若 GPU 侧重采样，`samples` 不再固定 ⇒ 产物里不能再写采样点数（否则同一产物在不同取景下不一致）。落地时必须同时改 `expr.plot` 的**声明**（去掉 `samples` 或改成"屏幕尺度提示"）⇒ 属能力签名变化，需 bump 与全量重录 |
| BI-04 | 本阶段的判据 | — | **已关闭** | 无代码改动：`expr.plot` / `cap_plot.lua` / manifest 哈希 / 像素 36 项全部保持阶段 1 的状态（本次 `cargo verify` 即为证据） |

**BI 段小计**：已关闭 1 · 记账 3。
> 本段读数（`cargo verify` 全量）：demo 34/34 · 覆盖 72/72（豁免 0）· 结构 22 件一致 · 像素 36 项 ok 36 / DIFF 0。

---

## 统计与维护约定

> **口径（B14 §7.2，B19 V-11 扩，PA / PB / PC / PD / PE / PF / PG / PH / PI 再扩）**：下表只统计 A / B / C 三段（最初 44 条）。D 段及之后
> （**D–BI**，按批次追加）**不并入本表**，见各段自己的小计（由 `check_gaps_subtotals`
> 自动核对 K–BI 段，含"部分关闭"与"已记录"；立约段只核对可识别的状态词）。

| 类别 | 数量 |
|---|---|
| 升格为 P0（融合前置） | 1（F-02 / G-101，**G-101 已关闭**） |
| 可清 | 11（A 1 + B 6 + C 3 + G-101） |
| 描述不准，已改写 | 1（F-01） |
| 降为工效项 | 1（W-05） |
| 仍成立 | 28（A 11 + C 16 + 排期 1） |
| 范围外 | 3（B 2 + C 1） |
| 待定 | 1（统一相机 / 深度合成模型） |

共 **44 条**：A 段 14（作品）+ B 段 9（WASM 探测）+ C 段 21（README/ARCHITECTURE）；G-101 是 F-02 的升格形态，不重复计数。
D 段（B1 批次记账）6 条：已关闭 4 · 新增能力 2 —— 不计入缺口类别（见上表下方 D 段小计）。

维护约定：

- 新增缺口必须先落到本文件，再在 crate README 留一行指针；不得只写 README。
- 条目正文里的「需重录」「待补」必须**有独立编号**，否则段小计看不到（BA-02 的教训）。
- 禁止词表不限于清单：每批收尾按字根（钉 / 锚 / 锁 / 真）扫一遍（BA-03）。
- 「可清」必须附代码位置；无法给出证据的标「待定」，不得为了清而清。
- 每次清理后同步更新 `WASM_PORT_SURVEY.md` 的状态列与本文件统计。

### 语义锚点（B12 Step 0.2）

代码引用一律用**符号名**定位，不用行号：

```
<kind> <crate>::<mod>::<Item>[::<member>]
例：struct ze_render_ir::style::StrokeCap
    fn     zela_render::frame::end_frame
    const  zela_render_backends::wgpu_3d::DEFAULT_MSAA_SAMPLES
```

行号可以写，但必须标 `≈` 且**不参与定位**：行号腐烂是静默的（永远指向某一行，只是不是目标那行），
符号名腐烂是响亮的（grep 0 命中）。

`cargo verify --step structure` 会扫本文件里所有 `crate::…` 形式的反引号引用：
crate 找不到 ⇒ 改名 / 已删除；符号找不到 ⇒ 改名 / 已删除。**只查存在性，不查语义**
（"常量还在但缺口已关闭"是 `verified` 字段的事，不归锚点检查——一个机制不包打天下）。
