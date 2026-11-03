# BV 前置核实（光追 pass 与资源系统收尾）

> 对象：《光追pass与资源系统收尾_执行计划》BV-0
> 性质：只读核实 + 基线读数（不改产品代码）
> 结论直接决定 BV-2 的容量上限、BV-3 的绑定方案与 BV-5 的 Demo 分辨率

## 0. 结论速览

| 项 | 结论 | 影响 |
|---|---|---|
| 基线读数 | 像素 **39 项 ok 39 / DIFF 0** · 结构 25 件 · demo 37/37 · 覆盖 72/72 · `--full` 207.3s（BU-6 实测） | 本批红线 |
| 组 0 | 7 项：`0 view · 1 statics[0] · 2 statics[1] · 4 instances · 5 statics[3] · 6 statics[4] · 7 sprites`；**3 空着** | BV-2 补 indices 不占组 1 的 25 槽位 |
| **容量上限** | `store_limit(limits, 2) == max_buffer_size`（indices 的**索引缓冲**口径）；补 storage 绑定后必须压到 `max_storage_buffer_binding_size` | **BV-2 必须改这一处**，否则超限时建组失败（运行时，不是编译期） |
| 实例 → 索引范围 | **没有**：`Instance.ids = [vertex base 1, vertex base 2, flags, 0]`；范围在 CPU 的 `Draw.fill: Range<u32>`（= `Shape.raster.fill`） | BV-2 另传范围表，不动 `Instance` 布局 |
| `shaded()` | 用 `dpdx`/`dpdy`（`blend.wgsl:557-558`）⇒ **compute 不能直接调** | BV-3 调 `reflected()`（零导数，已核实） |
| `reflected()` 的依赖 | `light_count` / `light_at`（组 0 的 `View` uniform）、`shadowed` / `dfg_at` / `environment_at`（组 3） | BV-3 可**直接复用**组 0 的 view uniform 与组 3 的 lighting 组 |
| brute-force 实测 | 本机 RTX 4060：320×240×8192 tris = **10.4 ms**；96×72×8192 = 2.1 ms；96×72×64 = 0.25 ms | Demo 取 **96×72 + ≤256 三角形**（CI lavapipe 慢 1–2 个数量级） |
| 设备上限（本机） | `max_storage_buffer_binding_size` 2047 MiB · `max_buffer_size` 4094 MiB · `max_texture_dimension_2d` 32768 | CI 的 lavapipe 按默认 128 / 256 MiB 计 |
| 老目标迁移点 | `bloom::Bloom::bloomed`（自建 5 张纹理 + 金字塔）、`occlusion::Occlusion::occluded`（自建 6 张）、`environment::Prefilter::prepare`（自建 2 张 + 源图） | BV-1 逐个迁，每个单独跑 `--step hashes` |

## 1. 基线读数（BV 起点）

```
cargo verify --full     # 全部通过；耗时汇总 207.3s（BU-6 提交 38999f8）
  hashes    39 项 ok 39 · DIFF 0 · no-slot 0
  structure 25 件作品与基线一致（等式；wgpu/vector × script/bundle 同值）
  demo      37 条 payload 断言通过
  coverage  能力覆盖 72/72（豁免 0 条）
  docs      51 个模块文档与生成结果一致
  smoke     320×240 15 帧，script == bundle == 基线
  check/test/wasm/bench/fixtures 全绿
```

## 2. 组 0 的 binding 现状

| 项 | 现状（`render.rs`） |
|---|---|
| layout | `layout("scene", &[view, storage(1), storage(2), storage(4), storage(5), storage(6), storage(7)])`（`RasterLayouts.scene`） |
| bind group | `scene_group`：`(0 view) (1 statics[0]) (2 statics[1]) (4 instances) (5 statics[3]) (6 statics[4]) (7 sprites)` |
| shader 声明 | `blend_buffers.wgsl`：`1 vertices · 2 links · 4 instances · 5 rows · 6 extras`（`view` 在 `blend.wgsl:60`） |
| statics 下标 | `0 vertices · 1 links · 2 indices · 3 rows · 4 extras · 5 ctrl · 6 ranges` |
| indices 的 usage | `storage \| INDEX \| COPY_DST`（`Buffer` 闭包）—— **已经带 STORAGE**，只差 layout / group 各一项 |

**容量上限的坑（BV-2 必修）**：`store_limit(limits, k)` 对 `k == 2` 特例返回 `max_buffer_size`，
其余返回 `min(max_storage_buffer_binding_size, max_buffer_size)`。indices 今天只作 index buffer，
所以那条特例是对的；一旦把它同时绑成 `var<storage, read>`，绑定尺寸上限就变成
`max_storage_buffer_binding_size`（本机 2047 MiB，CI lavapipe 默认 128 MiB）。
⇒ BV-2 要去掉 `k == 2` 特例（或显式取两者的 min），并加一条
"`statics[2]` 的分配上限 ≤ 组 0 的 storage 绑定上限"的用例。

## 3. 实例 → 索引范围（生产端）

```
Shape.raster.fill: Range<u32>    // 网格三角形的索引区间（add_mesh 里 start..end）
Shape.raster.stroke: Range<u32>  // 描边：低 3 位是边号（(moved(code>>3)<<3) | (code&7)）
Draw.fill = Shape.raster.shown(...)  // 每个实例拿同一段区间（实例只差 ids[0] 的顶点基址）
Instance.ids = [a.base, b.base|NONE, flags | index<<PLACE, 0]
```

- `indices[fill]` 里存的顶点下标**已经加过 `base`**（`add_mesh` 里 `i + base`）⇒ 直接拿去索引 `vertices` 即可；
- 同一形状被多个实例引用时，`fill` 区间相同、`ids[0]` 不同 ⇒ 范围表按**实例**记 `(fill.start, fill.len())`；
- 光追只取 `fill`（三角形），`stroke` 不解码。

## 4. 着色：`reflected()` 可用、`shaded()` 不可

```
blend.wgsl:557-558   let across = dpdx(n); let down = dpdy(n);      // ← compute 里没有导数
                     let roughness = over_pixel(m.y, dot(across,across)+dot(down,down));
                     let radiance = reflected(linear(base.rgb), m.x, roughness, m.z, n, in.world, v, ao);
```

`light.wgsl` 的 `reflected(base, metallic, perceptual_roughness, reflectance, normal, p, v, ao)`
全文件零 `dpdx/dpdy/fwidth`（已核实）。它读：

| 依赖 | 绑定 |
|---|---|
| `light_count` / `light_at(i)` | 组 0 binding 0 的 `View`（`lights: array<Light,8>`） |
| `shadowed` | 组 3 binding 0/1（shadow maps + comparison sampler） |
| `dfg_at` | 组 3 binding 2/3（DFG 表 + 线性 sampler） |
| `environment_at` | 组 3 binding 4（环境立方体） |
| `harmonic`（环境漫射） | `View.harmonics: array<vec4,9>` |

⇒ 光追 compute pass 只要绑**同一份 view uniform**（按视图槽位偏移）与**同一份组 3**，
就能原样调 `reflected()`；只需自己补"相机逆投影"（`View` 里没有 `inverse`，见 §5）。

## 5. 相机与光线重建

`blend.wgsl` 的 `View` 有 `pixels` / `light` / `toward` / `eye`（w = 1 透视），**没有逆投影**。
`vector.wgsl` 的 `FrameUniform` 有 `inverse`（clip → world，列主序）。

两条路都不改现有布局：

1. 光追自己的 uniform（新 binding）里放 `inverse` + `eye` + `toward` + 视图尺寸；
2. 或者把光追放进 vector 的 compute 家族（它有 `FrameUniform`），但那样拿不到组 0 的
   `vertices` / `instances` / `extras`（vector 的组 0 是另一套）。

**BV-3 取 1**：新建一个 `RayParams` uniform（`inverse` 4×4 + `eye` + `toward` + `size` + `counts`），
组 0 = `[view(0), vertices(1), indices(3), instances(4), extras(6), ray(8)]`（只声明用到的），
组 1 = 输出 storage 纹理，组 3 = 复用的 lighting 组。既不碰既有 layout，也不复制资源。

## 6. brute-force 实测（本机 GPU；CI 口径）

测法：一次性探针（**不入库**）——一个 compute 入口，每像素对 N 个三角形做 Möller–Trumbore，
取最小 t，写 `rgba32float` storage；每个组合跑 3 次取最小值（`queue.submit` + `poll(wait)`）。

```
设备：NVIDIA GeForce RTX 4060 Laptop GPU（Vulkan，本机；无 lavapipe）
96x72   ·   64 tris · 0.25 ms
160x120 ·   64 tris · 0.19 ms
320x240 ·   64 tris · 0.36 ms
96x72   · 1024 tris · 0.40 ms
160x120 · 1024 tris · 0.46 ms
320x240 · 1024 tris · 1.09 ms
96x72   · 8192 tris · 2.12 ms
160x120 · 8192 tris · 2.78 ms
320x240 · 8192 tris · 10.38 ms
```

换算成"每帧每百万次求交"：8192 tris × 96×72 px = 56.6M 次 → 2.1 ms ⇒ 约 **37 ns / 千次**。

CI 是 lavapipe（CPU 软件 Vulkan），同类负载通常慢 **50–200 倍**。按 100 倍估：

| Demo 配置 | 本机 | CI 估计（×100） | 216 帧一遍 |
|---|---|---|---|
| 96×72 · 64 tris | 0.25 ms | 25 ms | ≈5.4 s |
| 96×72 · 1024 tris | 0.40 ms | 40 ms | ≈8.6 s |
| 160×120 · 1024 tris | 0.46 ms | 46 ms | ≈10 s |
| 320×240 · 8192 tris | 10.4 ms | 1.0 s | **≈216 s（不可接受）** |

**结论**：Demo 取 **96×72（或 128×96）+ ≤256 三角形**，进 `hashes`（计划 §4 的第一方案）。
若 CI 实测仍超预算，退到 `smoke` 方案并在 GAPS 写明理由（BV-10 记账）。

## 7. BV-1 的三个迁移点

| 目标 | 现状 | 迁移后 |
|---|---|---|
| bloom | `Bloom::bloomed` 自建 `paint` / `light` / 金字塔 `down` / `up`（共 2 + 2L 张）+ 一张 params buffer | 逐张走 `Canvas::target(TargetDesc)` |
| occlusion | `Occlusion::occluded` 自建 `depth` / `distances` / `raw` / `horizontal` / `out` / 金字塔（6 张）+ params | 同上 |
| environment | `Prefilter::prepare` 自建源图 + 输出立方体 + 每级 mip 视图 | 同上（立方体要 `layers = 6`，`TargetDesc.layers` 已覆盖） |

三处都是**像素敏感路径**：`TargetDesc` 只改"怎么建"，不改"建什么"，所以像素与 `held_*` 读数都应逐位不变；
每个迁移单独跑 `--step hashes` 验证。

**注意**：`Canvas::target` 现在返回 `&TrackedView`，而这三处需要**多视图共享同一张纹理**
（bloom 的 `down[l]`、occlusion 的 `level[l]`、environment 的每级 mip 与立方体视图）。
`TargetDesc` 的 `labels/dimension/aspect/mip` 还不够 ⇒ BV-1 需要给 `TargetDesc` 加
`dimension` / `base_mip` / `mip_count` / `base_layer` / `layer_count` / `aspect`（或加一个
`view_of(desc)`），这是 BV-1 的唯一接口扩展点。

## 8. 本阶段能证明什么 / 不能证明什么

- 能：基线读数、组 0 的现状与补 3 的容量代价、实例→索引范围的生产端事实、
  `reflected()` 的绑定需求、brute-force 的耗时量级、三个老目标的迁移点。
- 不能：CI（lavapipe）的真实耗时——本机没有 lavapipe 适配器，只能按倍数外推（BV-5 的
  分辨率因此取保守值，并把"CI 实测"写成 BV-10 的触发条件）。
