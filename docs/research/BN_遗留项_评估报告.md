# BN 遗留项 · 评估报告

> 对象：《Pass 声明化与 Pod 可插拔_执行计划》里未完成或缩水做的部分
> 用途：供评估决策。每项给「核实到的代码事实」→「为什么这一批没做」→「可选方案（代价 / 风险 / 判定方式）」→「建议」
> 性质：只读评估，不含代码改动

## 0. 结论速览

| 项 | 状态 | 一句话原因 | 建议 |
|---|---|---|---|
| 阶段 2 目标纹理池 | **没做**（只做身份化） | 后端根本没有 per-frame 临时目标，池化的对象不存在 | 保持现状，触发条件写清 |
| 阶段 5 自动绑定 | **没做**（逃生舱已做） | 缺"帧内按名字取 BindGroup"的注册表，硬写是第二份定义 | 先做只含前两组的小版本 |
| 阶段 4 Pod 接线 | 接口做了，**装配没接** | 生产构建里没有渲染 Pod，且 `upload` 无释放语义（GAPS R-02） | 等资源租约落地 |
| 阶段 3 的粒度 | **缩水**：30 label → 11/10/2 条 | 阴影按 layer、矢量按 group 重复，静态顺序表表达不了 N 次 | 保持帧级；需要单开关时再做 |
| 阶段 2.2 双跑等价 | **没做** | 旧路径已删，没有可对比的第二条路径 | 值得补，但先定基线存放规则 |
| BN-07 作者可编程 | 按计划只记账 | 缺"完整 pass 生成"与"声明携带表达式引用" | 先做后者 |

**先修正我上一条回复里的一处说法**：我说过"架构里没有渲染 Pod 的位置"，更准确的说法是——**存在**一个渲染 Pod，但它只在测试构建里（`render_pod_probe`），生产构建没有。差别见 §3。

## 1. 阶段 2 · 目标纹理池：没有可池化的对象

### 1.1 核实到的事实

**所有目标都挂在 `Canvas` / `Vector` 上，只在尺寸变化时重建，不是每帧新建：**

| 目标 | 创建点 | 生命周期 |
|---|---|---|
| `Canvas.color` | `render.rs` 的 `Canvas::new` | 每个画布一次 |
| `Attachments.msaa` / `depth` / `msaa_light` | `Canvas::attach` | 每个画布一次（`get_or_insert_with`） |
| `Slabs.bounds` / `layers` / `slabs` | `Canvas::attach_slabs` | 每个画布一次（`if attached.slabs.is_some() { return; }`） |
| `Vector.base` / `base_light` / `base_depth` | `vector.rs` 的 `Vector::base` / `base_light` | 尺寸不变就复用（`is_none_or(|(_, have)| *have != size)`） |
| `bloomed` | `canvas.bloomed.is_none()` 时建 | 每个画布一次；一次调用建 `2 + (2L−1)` 张纹理 |
| `occluded` | `canvas.occluded.is_none()` 时建 | 每个画布一次 |

⇒ **没有"每帧申请、用完即弃"的纹理**。池化的收益（少建纹理）在这里是零，因为本来就不重建。

**Clear 语义是三态，不是"全 Clear"**（`Canvas::pass` 的 `color: Option<(LoadOp<Color>, bool)>`）：

| pass | 传入 | 含义 |
|---|---|---|
| 基线 / 不透明 / atlas 栅格 | `Some((Clear(…), …))` | 清屏后画 |
| 复合 | `Some((Load, true))` | **要叠在 base 上**，不能清 |
| append（链表追加） | `None` | **没有颜色附件**，只读深度 |

⇒ 计划里"强制 `LoadOp::Clear`"这条规则如果照字面落地，会直接改坏复合与 append。

### 1.2 为什么这一批没做

两个条件同时成立：**没有池化对象**（上表）+ 计划自己把它标为最高风险（残留 bug 只在特定顺序下复现，像素门禁未必抓得到）。此时做池化只会引入一个零消费者、且带已知风险的抽象。

### 1.3 可选方案

| 方案 | 做法 | 代价 | 风险 | 判定方式 |
|---|---|---|---|---|
| **A1（现状）** | 只做 `RenderTargetId` 身份化，声明里能读到目标名，纹理沿用现有缓存 | 无 | 无 | 已满足：声明可读、像素 36/36 |
| **A2 受限池** | 只池"同尺寸同格式的离屏颜色目标"（`base` / `base_light`），键 = (size, format, role) | 需要把"写前必须 Clear"变成入参约束（现在已是 `Option<LoadOp>`，可复用） | 中：复合用 `Load`，池化后 base 的内容来源变化会无声改像素 | 36 项 + 两条新用例（连续两帧不残留 / 单帧内复用不串色） |
| **A3 全池 + 附件区块化** | 目标收进 `TargetPool`，pass 声明 `outputs: [{target, load}]`，附件由框架生成 | 要把 `Canvas::pass` 的 msaa / resolve / kept 逻辑搬进框架（现在集中在 `attachment` 闭包里） | 最高 | 同上 + 附件配置只有一处 |

### 1.4 建议

保持 **A1**。触发条件：出现"每帧新建的目标"（例如新的后处理链）时，先做 A2，不要直接跳 A3。

## 2. 阶段 5 · 自动绑定：缺一个注册表

### 2.1 核实到的事实

- **21 个 pass 开始点**：`render.rs` 11 处、`vector.rs` 7 处、`bloom.rs` / `occlusion.rs` / `environment.rs` 各 1 处（计划写 30，是 label 口径：含循环内重复与模块内多阶段）。
- **14 个 `create_bind_group` 点**：`render.rs` 8、`vector.rs` 3、`bloom` / `occlusion` / `environment` 各 1。
- **绑哪个组是分支内决定的**，不是 pass 的静态属性：
  - 复合 pass：`set_bind_group(1, &self.image_groups[&0])`
  - append pass：`set_bind_group(2, attached.append)`
  - slabs pass：`set_bind_group(2, &s.groups[0])` 或 `[1]`
  - 普通光栅 pass：`set_bind_group(0, scene)` + `set_bind_group(3, lighting)`
- 组索引本身在管线创建时固定（scene=0 / images=1 / appends=2 / lighting=3），但**具体是哪个 BindGroup**要看分支。

⇒ 声明里写 `binds: [scene, images]` 需要"帧内按名字 → BindGroup"的映射；没有这个映射，声明只能复制分支判断，成为第二份定义（计划红线 4 要避免的"推导"）。

### 2.2 可选方案

| 方案 | 做法 | 代价 | 风险 | 判定方式 |
|---|---|---|---|---|
| **B1 注册表 + 声明** | 帧前把所有 BindGroup 收进 `BindGroups { name → &BindGroup }`；pass 声明 `binds: &[&str]`；框架统一 `set_bind_group(槽位, …)` | 14 个创建点要起名登记；`append` 每帧按 `lists.generation` 重建，注册表要同步刷新 | 中：绑错组会立刻触发 wgpu 校验错误，不会无声通过 | 绑定序列快照（每个 pass 绑了哪几个组）+ 36 项 |
| **B2 只自动前两组** | group0=scene、group1=images 对所有光栅 pass 一致，可先自动；group2/3 保留手写 | 小 | 低 | 同上，覆盖范围小 |
| **B3（现状）** | 不自动 | 无 | 无 | — |

### 2.3 建议

先做 **B2**（收益/风险比最好）；B1 与阶段 2 的池一起做——池化天然需要"按名字取目标/取组"的注册表，两件事同源。

## 3. 阶段 4 · Pod 接线：接口做了，装配没接

### 3.1 核实到的事实

- 渲染侧的 Pod **存在**，但只在测试构建：`mod render_pod_probe;` 挂着 `#[cfg(all(test, feature = "blocking", any(feature = "wgpu", feature = "egui-wgpu")))]`（`zela-render-backends/src/lib.rs:100`），文件头写明"契约证伪，不是生产接线"。
- 该探针的结论已经记账：`upload` 每次新开槽、契约里没有 release（GAPS R-02）；`Handle` 表达不了"目标 + 批次"（R-04 / R-05）。
- 生产装配路径是三件独立的东西：`StandardBackends` / `SceneCompositorCallback` / `assemble_runtime`，都不经过 Pod。

⇒ 把 `PassProvider` 做成 capability 再接进去，会原样继承 R-02：**资源没有释放语义**。而 pass 的资源恰恰是"一帧内生产-消费"，没有 release 的句柄模型装不下。

### 3.2 可选方案

| 方案 | 做法 | 代价 | 风险 | 判定方式 |
|---|---|---|---|---|
| **C1 先补资源租约** | 契约加 `lease(frame)` / `release(Handle)`（对应 R-02），再把 `PassProvider` 做成 capability | 要改 `ze_render_ir::caps` 的签名 ⇒ 能力指纹变化、需重录 | 中 | 指纹门禁 + 探针双路径对拍 |
| **C2（现状）** | 只保留纯函数 `negotiate`，宿主直接调用 | 无 | 无 | 已满足：同名/缺资源整批失败有 3 条用例 |
| **C3 只声明不替换** | 定义 `PassProvider` capability，协商期只读取"提供了哪些计划"，不允许运行时替换 | 小 | 低 | 协商失败即整批失败（与现状同一纪律） |

### 3.3 建议

**C2 现状** + 在装配期打印一句"本后端提供了 N 张计划"（零风险、可观测）。C1 等 Render Pod 的生命周期契约立项再做。

## 4. 阶段 3 的粒度：30 label → 帧级调度单元

### 4.1 核实到的事实

21 个 pass 开始点里，重复与嵌套是结构性的：

- **阴影按 layer 重复**：`for (layer, s) in shadows.iter()` 里 begin，层数由数据决定；
- **矢量按 group 重复**：`for (g, (offset, draws)) in groups.iter()` 里 begin；
- **一次调用内含多个 compute**：`vector.encode` 内部有 flatten / composite / slab bounds（`vector.rs` 三处 `begin_compute_pass`）；`bloomed` 一次建 4 个 compute pass。

静态顺序表只能表达"出现一次"。硬拆成 30 条会让"声明 1 次 vs 实际 N 次"对不上，只能把校验放宽成"出现次数 ≥ 1"，等于削弱判据。

### 4.2 可选方案

| 方案 | 做法 | 代价 | 收益 |
|---|---|---|---|
| **D1（现状）** | 一个声明 = 一次逻辑调用，N 次重复算一次 | 模块内子 pass 的缺席不可见 | 顺序与缺失判据严格 |
| **D2 声明带重复语义** | `PassDecl.repeats: Repeats::{Once, PerLayer, PerGroup}`，`finish()` 改计数 | 校验逻辑变复杂；要定义"≥1"与"=0 时的显式跳过" | 模块内子 pass 可单独开关/观测 |

### 4.3 建议

**D1**。只在出现"需要单独关掉某个模块内 pass"的真实需求时做 D2（例如定位 `slab bounds` 的移动端问题）。

## 5. 阶段 2.2 的第二层测试（双跑等价）没做

### 5.1 事实

计划要求"每个阶段保留旧路径，新旧各录一遍命令、断言逐字节相同，验收后再删旧路径"。实际做法是直接替换：`image_layout` 由声明表生成、`encode_frame` 的 pass 顺序改为声明驱动，中间没有可控开关可切回旧路径。

### 5.2 现有替代

- **声明序逐步核对**：顺序不符 / 被关掉却执行 / 启用项既没记录也没显式跳过，三种都变成帧错误；
- **36 个作品**跑真实录制序（任何一处声明错误都会让 demo 失败）。

⇒ 它证明"最终命令流与声明一致"，但不证明"新命令流与旧命令流逐字节相同"。

### 5.3 方案

| 方案 | 做法 | 代价 | 风险 |
|---|---|---|---|
| **E1（现状）** | 靠声明序核对 + 像素 | 无 | 漏掉"两者都错但恰好一致"的情形 |
| **E2 命令流快照** | `#[cfg(test)]` 包一层 encoder，记录 (pass label, 附件配置, 绑定的组)，把 36 个作品的序列存基线 | 需要记录器 + 36 份基线文件 + 更新规则 | 基线会随场景演进而腐烂，需要"何时更新"的明文约定 |

### 5.4 建议

**E2 值得做**，但要先定一件事：基线变更的判定口径（改场景就更新、改实现不更新）。否则会变成第三份需要人工维护的清单。

## 6. BN-07 作者可编程：按计划只记账

### 6.1 事实

`emit_wgsl(expr, backend) -> WgslFunction { params, source, ret }`（`ze-comb/src/codegen.rs`）生成的是**一个表达式的函数体**；调用方是 `ShaderFill` 的 fill / frag codegen，只产片元颜色与一个计算阶段的骨架。

⇒ 缺两件：① 生成顶点阶段与 pass 骨架（vs + fs + dispatch 形状）；② pass 声明能携带"逐像素表达式"的引用（现在 `entry` 是字符串）。

### 6.2 方案

| 方案 | 做法 | 前置 |
|---|---|---|
| **F1** | 声明里放 `ShaderFill` 引用，`entry` 由框架拼；codegen 程序作为一个 pass 的片元 | 无（复用现有管线） |
| **F2** | codegen 生成完整 pass（vs + fs + dispatch） | F1 + `ze-comb` 扩到顶点阶段 |

### 6.3 建议

**F1 是 F2 的必经步骤**，且能立刻让"声明式 pass"与"作者表达式"接上。

## 7. 优先级

| 顺序 | 项 | 收益 | 风险 | 前置 |
|---|---|---|---|---|
| 1 | BN-07 F1（声明携带表达式） | 让声明式与作者可编程接上 | 低 | 无 |
| 2 | 阶段 5 B2（前两组自动绑定） | 减少重复的 `set_bind_group` | 低 | 无 |
| 3 | 阶段 2.2 E2（命令流快照） | 补上"中间步骤"的判据 | 低（但要定基线规则） | 无 |
| 4 | 阶段 5 B1 + 阶段 2 A2（注册表 + 受限池） | 两件事同源，一起做才顺 | 中 | B2 |
| 5 | 阶段 4 C1（资源租约 → Pod 接线） | 真正的可插拔 | 中（动能力指纹） | R-02 立项 |
| 6 | 阶段 2 A3 / D2 | 附件与子 pass 区块化 | 最高 | A2 / D1 稳定后 |

## 8. 一条建议补进 GAPS 的事实

BN-02 现在的证据是"没有引入新的临时目标"，但更硬的表述是：

> **后端没有 per-frame 临时目标**：所有离屏目标都挂在 `Canvas` / `Vector` 上，只在尺寸变化时重建（`Canvas::new` / `attach` / `attach_slabs` 都有"已有则返回"的保护；`bloomed` / `occluded` 只在 `is_none()` 时建）。池化的对象不存在。

这条比"怕风险"更能解释为什么不做，建议补进 BN-02 的证据列。
