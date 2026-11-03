# BU 前置核实（资源系统与可观测性）

> 对象：《资源系统与可观测性专项治理_执行计划》BU-0
> 性质：只读核实 + 基线读数（不改产品代码）
> 结论直接决定 BU-1 的计量单位与 BU-3 的清单范围

## 0. 结论速览

| 项 | 结论 | 影响 |
|---|---|---|
| 基线读数 | 像素 **39 项 ok 39 / DIFF 0** · 结构 22 · demo 37 · 覆盖 72/72 · docs · smoke 全绿（`--full` 298.2s） | BU 各阶段的红线 |
| `RES_*` 语义 | **分配频次**（create 时累加，读取即清零；无 destroy hook） | BU-1 必须拆出 held / peak |
| 纹理计数口 | `counted_texture` **16 处**（render 8 · vector 4 · environment 2 · bloom 1 · occlusion 1） | 与计划一致 |
| 缓冲计数口 | `counted_buffer` **14 处**（render 9 · environment 2 · bloom 1 · occlusion 1 · vector 1） | 计划只写了"另计" |
| **关键事实** | 16 处纹理口里 **15 处立刻 `create_view` 并丢弃纹理句柄**（视图在 wgpu 里持有底层纹理） | **按句柄算"持有"会恒为 0** ⇒ BU-1 必须按"分配租约 + 视图共享"计 |
| 中间目标 | `Canvas` 4 字段 · `Attachments` 6 · `Slabs` 4 · `ShadowMaps` 4 + `Vector` 的 14 个目标字段；`RenderTargetId` 17 变体；`IMAGE_SLOTS` 25 | BU-3 的"加字段才有的"清单 |
| 调度 | 5 张静态 `PassPlan`；`validate` 只校验（`find_cycle`），不推导 | BU-4/5 的落点 |
| 过时注释 | `render.rs` 的「为什么不是 `ze_resource::ResourceCache`」理由已被 BQ-01 推翻 | BU-2 一并改写 |

## 1. 基线读数（BU 起点）

```
cargo verify --full     # 全部通过；耗时汇总 298.2s
  hashes    39 项 ok 39 · DIFF 0 · no-slot 0
  structure 22 件作品与基线一致；渲染能力指纹 OK
  demo      37 条 payload 断言通过
  coverage  能力覆盖 72/72（豁免 0 条）
  docs      51 个模块文档与生成结果一致
  smoke     320×240 15 帧，script == bundle（wgpu 口径）
  check/test/wasm/bench/fixtures 全绿（bench 1 条 >20% 警告，不阻断）
```

`--workers` 没有独立的字节门禁：批量对拍由 `batch_hashes` 用 `batch_workers()`
（`xtask/src/main.rs:3457/3496`）跑，`--step smoke` 只比 script/bundle。
BU-A 的"驱逐与否不改变字节"需要新建采集装置（见 §7），不能靠现有步骤。

## 2. `RES_*` 现状

| 位置 | 现状 |
|---|---|
| `render.rs:55-58` | `RES_TEXTURES / RES_TEXTURE_BYTES / RES_BUFFERS / RES_BUFFER_BYTES`（`AtomicUsize`） |
| `render.rs:918/925` | `counted_texture` / `counted_buffer` 是**唯一**创建口，做 `fetch_add` |
| `render.rs:105-127` | `resource_totals()`（非破坏读）/ `take_resource_totals()`（`swap(0)`） |
| 无 destroy hook | 纹理 / 缓冲被 drop 时没有任何记录 |

⇒ 读数是「**距上次读取以来新建了多少**」，名字（`ResourceTotals`）读起来却是
「当前持有多少」。稳定复用的池会显示持续高值；泄漏（只增不减）与高频分配**无法区分**。

消费者（3 处，都是只读）：

| 消费者 | 用法 |
|---|---|
| `zela-app/src/frame_loop.rs:712-713` | 逐帧 `take_resource_totals()` 累加进 `RunReport.vector_resources` |
| `zela-app/src/frame_loop.rs:373` | `RunReport` 字段（`ResourceTotals`） |
| `zela-web/src/lib.rs:798` | 浏览器面板显示 `resource_totals()` |
| `zela-commands/src/commands/batch.rs:467/495` | 把 `vector_resources.json()` 写进 `observe.jsonl` |

`observe.jsonl` 的实际形状（fourier / vector / script 一轮）：

```json
"vector_resources":{"textures":21,"texture_bytes":20135012,"buffers":4,"buffer_bytes":64}
```

## 3. 计数口调用点（生命周期事实）

### 3.1 纹理：16 处，但句柄几乎不存活

| 文件 | 处数 | 形态 |
|---|---|---|
| `render.rs` | 8 | `Canvas::new`（color）、`Canvas::attach`（depth/msaa/msaa_light）、`attach_slabs`（bounds/slabs）、`Gpu::new`（dfg / no_environment / no_occlusion）、`ShadowMaps::new` |
| `vector.rs` | 4 | `image()`（返回 `TextureView`，纹理句柄当场 drop）、`prepare_flatten` 的 records、`stroke_depths`、`no_slabs` |
| `environment.rs` | 2 | `environment picture`（Uint 源图）与预滤波输出（多 mip） |
| `bloom.rs` / `occlusion.rs` | 各 1 | 逐级纹理，随即 `create_view` |

**15/16 是 `counted_texture(...).create_view(...)` 或先建后建多个视图**；wgpu 的
`TextureView` 持有底层纹理的引用，因此**丢弃 `Texture` 句柄不等于释放显存**。
⇒ BU-1 若包装 `wgpu::Texture` 并在 drop 时减 held，读数会≈0（误导）。

### 3.2 缓冲：14 处，句柄基本长期持有

`render.rs` 9（`readback`、`Lists` 的 3 条、`Buffers` 的 views/instances/sprites/statics×7）、
`vector.rs` 1（`buffer()` 帮助函数，覆盖 Vector 的全部缓冲）、`environment.rs` 2、
`bloom.rs` / `occlusion.rs` 各 1（`params`）。绑定组（`wgpu::BindGroup`）同样会持有
缓冲引用，但它与字段的生命周期在本 crate 里同生同灭（重建字段时同时重建组）。

## 4. 中间目标穷举（BU-3 的清单）

| 容器 | 字段 | 创建点 / 生命周期 |
|---|---|---|
| `Canvas`（`render.rs:3097`） | `width` / `height` / `color` / `raster` / `occluded` / `bloomed` | 每画布一次；`color` 可由外部目标给（`covered`） |
| `Attachments`（3108） | `msaa` / `msaa_light` / `depth` / `depth_only` / `append` / `slabs` | 首次光栅时 `get_or_insert_with` |
| `Slabs`（3120） | `bounds` / `layers[4]` / `slabs` / `groups[2]` | 首次有 see-through 层时建 |
| `ShadowMaps`（3302） | `layers` / `array` / `size` / `group` / `environment` | 阴影层数变化时重建 |
| `Vector`（`vector.rs:671`） | `colors` / `no_colors` / `tags` / `no_tags` / `atlas` / `scratch` / `carried` / `no_light_out` / `empty` / `base` / `base_light` / `base_depth` / `far` / `no_samples` / `no_slabs` | 按视图尺寸 / 组数懒建 |
| `bloom::Bloomed` / `occlusion::Occluded` | 各自的分级纹理与视图 | `Canvas` 的 `Option` 字段 |

名字侧：`RenderTargetId` **17 个变体**（`passes.rs`），`IMAGE_SLOTS` **25**（BT-02 扩过）。
⇒ 加一个中间目标 = 加字段（上表）+ 加 `RenderTargetId` 变体 + 加创建分支 + 可能加槽位。

## 5. 过时注释（BU-2 必须改写）

`render.rs` 的「# 为什么不是 `ze_resource::ResourceCache`」理由是
「`GpuHandle` 不 `Send` ⇒ `Gpu` 不再是 `Send`」。**BQ-01 已按平台拆箱**：

- native：`Box<dyn Any + Send>`（自动 `Send`，无 `unsafe`）
- wasm：`Box<dyn Any>`（语义零变化）

且编译探针已断言 `assert_send::<ze_resource::ResourceCache<()>>()`。
⇒ 该结论现在是错的，留着会让下一个人重新得出"不能接入"。

## 6. 调度侧（BU-4/5 的落点）

- 5 张静态 `PassPlan`：`RASTER_PLAN` / `RASTER_LAYERED_PLAN` / `VECTOR_PLAN` /
  `ENVIRONMENT_PLAN` / `CODEGEN_PLAN`（`passes.rs`）
- `validate`：`ze_graph_core::find_cycle` + 生产者先于消费者，**不推导顺序**；
  `topo_sort` 未使用（文件头写明原因）
- `PassSchedule` 按声明下标推进；`command_flow()` 已有（BS-0）
- `check_bindings` 校验"声明 vs 实际绑定"，**没有** `inputs ∩ outputs` 检查

## 7. BU-1 的计量单位（本批的实现决定）

**分配租约（allocation lease）**，不是 wgpu 句柄：

- `counted_texture` 返回 `TrackedTexture`（句柄 + `Arc<Lease>`）
- 由它派生视图用 `TrackedTexture::view(desc)` → `TrackedView`（视图 + 同一个 `Arc<Lease>`）
- 最后一个引用（句柄或任一视图）drop 时，`Lease::drop` 减 `held_*`
- `counted_buffer` 返回 `TrackedBuffer`（缓冲 + `Arc<Lease>`；绑定组与字段同生同灭，按字段算）
- 旧 `RES_*` 改名 `ALLOC_*`，语义 = **分配频次**（保留原用途）
- 新增 `HeldTotals`：`held_*`（当前持有）+ `peak_*`（高水位 = 真正的泄漏信号）

**BU-A 的采集装置**（BU-1 只建装置，BU-2 有驱逐后才生效）：

| 采集物 | 判据 |
|---|---|
| `observe.jsonl` | 同一次渲染在"驱逐开/关"两种容量下逐字节相同 |
| `baselines.txt` / `structures.txt` | 同上（不写入新值，只比对） |
| `held_*` / `peak_*` | 允许不同（它们就是性能读数），**不得**进 observe 的确定性字段 |

## 8. 本阶段能证明什么 / 不能证明什么

- 能：基线读数、计数口全表、中间目标全表、BU-1 计量单位的选择依据。
- 不能：`--workers` 的字节门禁（不存在，需 BU-A 新建）；
  `held_*` 的数值正确性（BU-1 落地后用"建了又放不推高高水位"用例证明）。
