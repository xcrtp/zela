# BP-0 · 绑定点普查清单

> 本批（BP）要把 `PassDecl.inputs/outputs` 里已经写好的名字与帧循环里实际绑定的资源连起来。
> 本文只记录现状，不改产品代码。

## 1. 基线读数（BP 批起点，2026-10-31）

| 门禁 | 读数 |
|---|---|
| `cargo verify --step hashes` | 合计 36 项：ok 36 · DIFF 0 · no-slot 0 |
| `cargo verify --step structure` | 全部通过 |

基线提交：`b23c184`（BO-3 之后）。

## 2. 名字约定（不新造）

三套名字全部已在磁盘上，本批只复用：

| 名字来源 | 覆盖 | 定义处 |
|---|---|---|
| `IMAGE_SLOTS` 的 24 个名字 | 组 1（vector images）的全部槽位 | `passes.rs` |
| `RenderTargetId::name()` 的 17 个名字 | 被写入的目标 | `passes.rs` |
| 着色器 `@group(0)` 的 binding 名 | `instances` / `ctrl` / `subs` / `objects` / `rows` / `tiles` | `blend_buffers.wgsl`（组 0 = `Buffers::group`） |

组 0 的六个名字与 `PassDecl.inputs` 里用的词完全一致（`instances` / `rows` / `ctrl` / `subs` / `objects` / `tiles`），
组 3 的六个名字与 `IMAGE_SLOTS` 一致（`shadow_maps` / `shadow_compare` / `dfg_table` / `dfg_linear` / `environment` / `occlusion`），
组 2 的组名就是 `IMAGE_SLOTS` 的 `crossings` / `stroke_depths_in` / `slab_bounds`，组 1 即 24 槽位。

⇒ **一个名字 → 一个组**，组内资源集合由既有 layout 决定，不在声明里重复写。

## 3. 全量普查（`set_bind_group` 40 处生产代码 + 1 处测试）

分类口径：

- **帧级固定**：实参表达式在本帧的同一 pass 里恒定，且来源是 `Gpu` / `Buffers` 上的常驻对象；
- **per-iteration**：实参随「组 / 层 / 级」变化，是循环变量；
- **现场构造**：实参是就地 `create_bind_group` 的临时组，没有名字；
- **参数传递**：实参是上层传进来的 `&wgpu::BindGroup`（闭包入参），调用点不知道它是谁。

### 3.1 `render.rs`（21 处）

| # | 行 | 实参 | 组 | 类别 | 名字化 | 理由 |
|---|---|---|---|---|---|---|
| 1 | 2024 | `&group` | 0 | 现场构造 | ❌ escape | `fill codegen`：`words/params/target` 每个程序现建，layout 是 codegen 自己的 |
| 2 | 2135 | `&group` | 0 | 现场构造 | ❌ escape | `frag codegen`：`params/out/target` 每个程序现建 |
| 3 | 3601 | `buffers.group` | 0 | 帧级固定 | ✅ `scene` | 阴影 pass，组 0 六项 |
| 4 | 3634 | `buffers.group` | 0 | 帧级固定 | ✅ `scene` | 遮蔽深度 |
| 5 | 3702 | `buffers.group` | 0 | 帧级固定 | ✅ `scene` | `base` |
| 6 | 3703 | `lighting_group` | 3 | 帧级固定（**分支**） | ✅ `lighting` | `own_lighting`（本视图的遮蔽组）或 `buffers.shadow_maps.group`；两候选各自登记名字，失效条件都是帧内 generation |
| 7 | 3727 | `buffers.group` | 0 | 帧级固定 | ✅ `scene` | `opaque` |
| 8 | 3728 | `lighting_group` | 3 | 帧级固定（分支） | ✅ `lighting` | 同上 |
| 9 | 3744 | `buffers.group` | 0 | 帧级固定 | ✅ `scene` | `append` |
| 10 | 3745 | `lighting_group` | 3 | 帧级固定（分支） | ✅ `lighting` | 同上 |
| 11 | 3746 | `append` | 2 | 帧级固定 | ✅ `list_appended` | 来源是 `attached.append`，随 canvas generation 更换（名字与 `RenderTargetId::ListAppended` 同词） |
| 12 | 3759 | `buffers.group` | 0 | 帧级固定 | ✅ `scene` | `slabs` |
| 13 | 3760 | `&s.groups[1]` | 2 | per-iteration | ❌ escape | 每帧按 slab 层数现建（`Slabs::groups`） |
| 14 | 3768 | `buffers.group` | 0 | 帧级固定 | ✅ `scene` | `composite sprites` |
| 15 | 3769 | `lighting_group` | 3 | 帧级固定（分支） | ✅ `lighting` | 同上 |
| 16 | 3770 | `&self.image_groups[&0]` | 1 | 帧级固定 | ✅ 白图 | 白图（key 0）在 `image_groups` 里；BP-4 收敛进注册表 |
| 17 | 3773 | `&s.groups[0]` | 2 | per-iteration | ❌ escape | 同上（`Slabs::groups`） |
| 18 | 3797 | `buffers.group` | 0 | 帧级固定 | ✅ `scene` | `atlas raster` |
| 19 | 3798 | `&buffers.shadow_maps.group` | 3 | 帧级固定 | ✅ `lighting` | 纯矢量视图没有自己的遮蔽组 |
| 20 | 3856 | `white` | 1 | 参数传递 | ❌ escape | `draw()` 内部；每次 `draw` 前恢复白图 |
| 21 | 3894 | `self.image_groups.get(&draw.texture).unwrap_or(white)` | 1 | per-draw 动态 | ❌ escape | 按 `draw.texture` 取；BP-4 把 `image_groups` 收敛进注册表，调用形态不变 |
| 22 | 3904 | `white` | 1 | 参数传递 | ❌ escape | 同一 `draw()` 内画完带纹理的网格后恢复白图 |

### 3.2 `vector.rs`（13 处生产 + 1 处测试）

| # | 行 | 实参 | 组 | 类别 | 名字化 | 理由 |
|---|---|---|---|---|---|---|
| 23 | 1160 | `&group` | 0 | 现场构造 | ⚠️ 可（但留 escape） | `base depth`：`base_samples` / `base_sample_depths` / `base_depth` 三项现建；只有一个 pass 用，先按 escape 记账 |
| 24 | 1416 | `&scene` | 0 | 帧级固定 | ✅ `scene` | `accumulate`（渲染进图集） |
| 25 | 1417 | `read` | 3 | 参数传递 | ❌ escape | 闭包入参（`read` 组按 group 现建） |
| 26 | 1447 | `&scene` | 0 | 帧级固定 | ✅ `scene` | `nearest` |
| 27 | 1448 | `&images` | 1 | 参数传递 | ❌ escape | 闭包入参（`Out::Depth` / `Out::Opaque` 各一套） |
| 28 | 1473 | `&scene` | 0 | 帧级固定 | ✅ `scene` | `flatten` |
| 29 | 1474 | `&write` | 2 | 现场构造 | ❌ escape | `write` / `read` 两套 layout 现建 |
| 30 | 1508 | `&scene` | 0 | 帧级固定 | ✅ `scene` | `composite` |
| 31 | 1509 | `&images` | 1 | 现场构造 | ❌ escape | 24 个槽位一个表达式里现建（BP-1 的符号表可覆盖，但本次只记） |
| 32 | 1519 | `&bind(…crossings…)` | 2 | 现场构造 | ❌ escape | 交叉计数组 |
| 33 | 1536 | `&scene` | 0 | 帧级固定 | ✅ `scene` | `slab bounds` |
| 34 | 1537 | `&read` | 1 | 现场构造 | ❌ escape | `bounds` 的读组 |
| 35 | 1538 | `&written` | 2 | 现场构造 | ❌ escape | `bounds` 的写组 |
| — | 1721 | `&bindings` | 0 | 测试 | — | `area law` 内联着色器，不在生产路径 |

### 3.3 子模块（5 处）

| # | 文件:行 | 实参 | 组 | 类别 | 名字化 | 理由 |
|---|---|---|---|---|---|---|
| 36 | `bloom.rs:120` | `group` | 0 | per-iteration | ❌ escape | 泛光自己的 layout，按级现建 |
| 37 | `occlusion.rs:141` | `group` | 0 | per-iteration | ❌ escape | 遮蔽自己的 layout，按级现建 |
| 38 | `environment.rs:484` | `bind(…)` | 0 | 现场构造 | ❌ escape | 立方体源，逐 mip 现建 |
| 39 | `environment.rs:489` | `bind(…)` | 0 | 现场构造 | ❌ escape | 半分辨率链，逐 mip 现建 |
| 40 | `environment.rs:501` | `bind(…)` | 0 | 现场构造 | ❌ escape | 预滤波，逐级现建 |

## 4. 汇总

| 类别 | 处数 | 去向 |
|---|---|---|
| 帧级固定、可直接名字化（`scene`） | 13 | BP-3 |
| 帧级固定、分支（`lighting`） | 5 | BP-3（两候选各登记） |
| 帧级固定、单点（`list_appended` / 白图） | 2 | BP-3 / BP-4 |
| per-iteration | 4 | BP-4 记账 |
| 现场构造 | 11 | BP-4 记账（其中 `base depth` 与 `images` 组标了「可名字化但留 escape」） |
| 参数传递 | 4 | BP-4 记账 |
| per-draw 动态 | 1 | BP-4 记账（随 `image_groups` 收敛一并处理） |
| 测试 | 1 | 不计入 |

生产代码合计 40 处：**20 处进 BP-3**（计划写 23，实测按「实参是否随循环变量变化」重新归类后
可名字化处为 20：`scene` 13 + `lighting` 5 + `list_appended` 1 + 白图 1），**20 处留 escape**，1 处测试不计。

> 与计划的差额：计划的 23 把 per-iteration 的 `s.groups[1]`、`s.groups[0]`、`append` 与
> `shadow_maps.group` 混入了「帧级固定」。本清单把 `s.groups[*]` 归 per-iteration（2 处），
> `append` 与 `shadow_maps.group` 归帧级（2 处），`draw()` 内的两处白图归参数传递，其余不变。
> 改造判据仍按计划执行：**句柄逐项相同 + 像素 36 项不变**。

## 5. 对 BP-2 的直接结论

- 组 0 的资源名与 `PassDecl.inputs` 同词 ⇒ 注册表可以按「组名 → 资源名集合」登记，
  实际绑定集合由 **`bind(pass, name)` 的调用行为**产生，不另写一张表；
- `lighting` 与白图这两处有多个候选来源，注册表要允许**同名多值**（按帧内 generation 失效）；
- 21 处 escape 必须在 BP-4 逐条记账（触发条件 + 为什么不能名字化）。
