# 术语表

> 这是 **hub**，不是字典：每条只回答「它是什么 · 在哪 · 和谁相邻」，不复述文档。
> 每条必须挂锚点——能挂符号就挂「crate 名 + `::` + 符号名」（用符号名定位，不用行号），
> 无符号可挂时挂 GAPS 编号或决策记录。锚点腐烂是响亮的（grep 无结果），行号腐烂则没有提示。

## 确定性谱系（L0–L3）

**一句话**：确定性在时间上是连续的，不是布尔；四层按「能否从 `(初始态, 输入流)` 重建」划分。

- **锚点**：`decisions/001-确定性谱系.md` · L0 `state = f(t)` / L1 `state = f(t, 输入流)` / L2 `state = fold(seed, 输入流)` / L3 依赖环境
- **所属层**：理念层（跨 kernel / scene）
- **相关**：[L1](#l1) · [L2](#l2) · [seek 代价](#seek-代价) · [决策 001](decisions/001-确定性谱系.md)

## L1

**一句话**：项目所在层——`state = f(t, 输入流)`，任意 seek O(1)，拿起它是「更坚实」。

- **锚点**：`decisions/001-确定性谱系.md` · 判据：能从 `(初始态, 输入流)` 重建且无跨帧记忆
- **所属层**：理念层
- **相关**：[L2](#l2) · [seek 代价](#seek-代价) · [SeekKind](#seekkind)

## L2

**一句话**：`state = fold(seed, 输入流)`，可复现但 seek 需要重放；求解器 / 弹簧 / 粒子 / `NatRec` 都是它的成员。

- **锚点**：`decisions/001-确定性谱系.md` · 成员清单：`GAPS AI-07`、`AP-10`
- **所属层**：理念层
- **相关**：[L1](#l1) · [SeekKind](#seekkind) · [NatRec](#natrec)

## seek 代价

**一句话**：L1 与 L2 的区别不在于「能不能复现」，而在于定位到任意一帧要付 O(1) 还是 O(n)。

- **锚点**：`decisions/001-确定性谱系.md` · 由引擎推导的 `SeekKind` 承载，不进脚本、不进产物
- **所属层**：scene / 绑定
- **相关**：[SeekKind](#seekkind) · [决策 001](decisions/001-确定性谱系.md)

## 开环控制

**一句话**：动画 = 无反馈、单变量的开环控制系统；通用化的每一步都是在这个式子上逐个去掉限定词。

- **锚点**：`decisions/008-定位与控制系统视角.md` · 「只借检查清单与命名，不照理论重构」
- **所属层**：理念层
- **相关**：[check_read_order](#check_read_order) · [决策 008](decisions/008-定位与控制系统视角.md)

## 确定性第四判据

**一句话**：确定性 = 输入相同 ⇒ 输出相同，**且无隐式可变状态**；检查项是全局可变静态 / 墙钟 / 随机 / 系统环境 / 进程计数。

- **锚点**：GAPS `AF-01` · 把 `MeshGeometry.id` 的 `AtomicU64`、系统字体、预记 `random/time` 三个坑统一为同一规则的实例
- **所属层**：全仓
- **相关**：[确定性第四判据](#确定性第四判据) · [结构指纹](#结构指纹)

## Pod

**一句话**：语言无关的黑箱状态机，提供一组方法；Channel 是它的能力面。

- **代码锚点**：`ze_pod_core::pod::Pod`（trait）· `ze_pod_core::pod::ChannelDecl`
- **所属层**：`crates/ze/pod/`
- **相关**：[Channel](#channel) · [MethodDecl](#methoddecl) · [决策 007](decisions/007-作者侧.md)

## Channel

**一句话**：Pod 对外暴露的一个可读写能力面，带名字、类型与所属组件；不是 Pod 本身的一部分。

- **代码锚点**：`ze_pod_core::pod::ChannelDecl` · `ze_pod_func::api::ChannelSpec`
- **所属层**：`crates/ze/pod/ze-pod-core/`
- **相关**：[Pod](#pod) · [ChannelRef](#channelref) · [FieldSpec](#fieldspec)

## ChannelDecl

**一句话**：Pod 在装配期自述的通道声明——名字、类型、所属组件，不含读写实现。

- **代码锚点**：`ze_pod_core::pod::ChannelDecl`
- **所属层**：`crates/ze/pod/ze-pod-core/`
- **相关**：[Channel](#channel) · [ChannelSpec](#channelspec)

## ChannelSpec

**一句话**：脚本侧通道选择键的数据形态（`namespace` + `name` + 值类型），落在 `zela.math`，由 `Pod::channels()` 导出到三语言常量。

- **代码锚点**：`ze_pod_func::api::ChannelSpec`
- **所属层**：`crates/ze/pod/ze-pod-func/`
- **相关**：[ChannelDecl](#channeldecl) · [ReadSpec](#readspec) · [ChannelRef](#channelref)

## ChannelRef

**一句话**：表达式里对一个通道的身份引用（`namespace` + `name`，可选 `clock`），按 bits 参与相等与哈希。

- **代码锚点**：`ze_comb::expr::ChannelRef`
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[ChannelType](#channeltype) · [ClockForest](#clockforest)

## ChannelType

**一句话**：通道值类型的封闭集（`Unknown` / `Number` / `Bool` / `Vec2` / `Vec3` / `Path`），由 `TypeSpec` 投影而来。

- **代码锚点**：`ze_comb::expr::ChannelType`
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[ChannelRef](#channelref) · [TypeSpec](#typespec)

## Expr

**一句话**：高层表达式 IR——只有 `id` 和 `node`，没有注解位；所有派生信息必须走 side table。

- **代码锚点**：`ze_comb::expr::Expr` · `ze_comb::expr::Node`
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[Step](#step) · [PassRegistry](#passregistry) · [决策 004](decisions/004-Pass架构.md)

## Step

**一句话**：低层 IR，已 lowering 成程序的值表与根索引；后端（WGSL / C / 节点图）只认它。

- **代码锚点**：`ze_comb::program::Step`
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[Expr](#expr) · [CompiledExpr](#compiledexpr)

## CompiledExpr

**一句话**：装配期末序程序 + 帧内 memo，无跨帧缓存；离散层成员额外标记「需要重放」。

- **代码锚点**：`ze_comb::program::CompiledExpr`
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[Step](#step) · [SeekKind](#seekkind)

## Value

**一句话**：求值期的纯数据值（`F64` / `I64` / `Bool` / `Text` / `Vec2` / `Vec3` / `Bytes`），不含句柄、不含引用。

- **代码锚点**：`ze_comb::program::Value`
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[AnimValue](#animvalue) · [CodataSpec](#codataspec)

## Mono

**一句话**：宿主注入的环境单子（时间 / 帧 / 事件 / 组件 / 环境）；脚本只提交结构，取值由宿主给。

- **代码锚点**：`ze_comb::program::Mono`（trait）
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[SceneMono](#scenemono) · [ReadSpec](#readspec)

## SceneMono

**一句话**：`Mono` 的场景实现——同时持局部时钟与全局时间，按叶子时钟链在全局时间上反算。

- **代码锚点**：`zela_scene_ecs::expr_mono::SceneMono`
- **所属层**：`crates/scene/zela-scene-ecs/`
- **相关**：[Mono](#mono) · [ClockForest](#clockforest)

## PassRegistry

**一句话**：pass 的登记与顺序表；分析 pass 只读 IR 写 side table，变换 pass 才写 IR。

- **代码锚点**：`ze_comb::pass::PassRegistry` · `ze_comb::pass::kernel_registry`
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[AnalysisPass](#analysispass) · [TransformPass](#transformpass) · [决策 004](decisions/004-Pass架构.md)

## AnalysisPass

**一句话**：只读 IR、产出 side table 的 pass；结果不进产物，因为派生量不该有第二个来源。

- **代码锚点**：`ze_comb::pass::AnalysisPass`（trait）
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[TransformPass](#transformpass) · [PassRegistry](#passregistry)

## TransformPass

**一句话**：会改写 IR 的 pass；语义变化必须显式声明（`Semantics`），否则对拍失去意义。

- **代码锚点**：`ze_comb::pass::TransformPass`（trait）· `ze_comb::pass::Semantics`
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[AnalysisPass](#analysispass) · [PassRegistry](#passregistry)

## AnimValue

**一句话**：可动画量的值域（`Scalar` / `Vec2` / `Vec3` / `Color` / `Path`），内核层的唯一来源。

- **代码锚点**：`ze_time::curve::AnimValue`
- **所属层**：`crates/kernel/ze-time/`
- **相关**：[Value](#value) · [FieldSpec](#fieldspec) · [结构指纹](#结构指纹)

## ClockForest

**一句话**：多时钟共存的森林——每个时钟是「父链 + 仿射变换」的扁平链，局部时间由它反解。

- **代码锚点**：`ze_time::clock::ClockForest` · `ze_time::clock::ClockNode`
- **所属层**：`crates/kernel/ze-time/`
- **相关**：[ChannelRef](#channelref) · [SceneMono](#scenemono) · [决策 001](decisions/001-确定性谱系.md)

## Director

**一句话**：帧号 → 时间的唯一折算点：`t = base_t + (frame − base_frame) × dt × speed`。

- **代码锚点**：`ze_time::director::Director`
- **所属层**：`crates/kernel/ze-time/`
- **相关**：[确定性第四判据](#确定性第四判据) · [SeekKind](#seekkind)

## pointwise

**一句话**：`f: ℝⁿ → V` 且逐点独立、可 O(1) 求值；声明之后靠污染传播（与 `Send`/`Sync` 同构）保守扩张。

- **锚点**：GAPS `AL-03` · `PointwiseReport` 记录污染源；误判为 pointwise 会算错值，所以传播只能取并集
- **所属层**：`crates/kernel/ze-func/`
- **相关**：[PointwiseReport](#pointwisereport) · [codata](#codata) · [决策 002](decisions/002-计算内核双层模型.md)

## PointwiseReport

**一句话**：一次表达式分析里「谁把点态性污染掉了」的报告，含污染源与位置。

- **代码锚点**：`ze_pod_func::infer::PointwiseReport`
- **所属层**：`crates/ze/pod/ze-pod-func/`
- **相关**：[pointwise](#pointwise) · [OpSig](#opsig)

## codata

**一句话**：生产性展开的无限结构（如无限细分曲面）——有 `depth` 观察者与节点预算，不生产即显式失败。

- **锚点**：GAPS `AF-08` · 零缓存 ⇒ 清空重算逐位相同是结构事实
- **所属层**：`crates/kernel/ze-comb/src/codata.rs`
- **相关**：[CodataSpec](#codataspec) · [原始递归 / 全函数](#原始递归--全函数)

## CodataSpec

**一句话**：codata 的可序列化上限（`depth` / `max_nodes`）——上界进产物，超出即显式失败。

- **代码锚点**：`ze_comb::codata::CodataSpec`
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[codata](#codata) · [决策 003](decisions/003-图灵完备边界.md)

## 原始递归 / 全函数

**一句话**：结构递归 = 原始递归 = 全函数；保证停机但没有无界搜索，因此不是图灵完备。

- **锚点**：GAPS `AF-02` · 组合子（Functor / Applicative / Monad，无 `fix`）的 bind 次数由项的语法结构决定
- **所属层**：表达式层 / 语言模型
- **相关**：[codata](#codata) · [NatRec](#natrec) · [决策 003](decisions/003-图灵完备边界.md)

## NatRec

**一句话**：有界自然数递归原语（`nat_rec` / `rec_prev` / `rec_index`）；装配期强制非负整数，绑定推导为 `Replay`。

- **锚点**：GAPS `AP-10` · 用 `rec_prev` / `rec_index` 表达 `step`，避免跨 Pod 回调
- **所属层**：`crates/ze/pod/ze-pod-func/`
- **相关**：[SeekKind](#seekkind) · [L2](#l2)

## TypeSpec

**一句话**：类型的纯代数表示（`Scalar` / `Struct` / `Enum` / `Array` / `Opaque` / `Var` / `App`），`no_std + alloc`。

- **代码锚点**：`ze_type::ty::TypeSpec`
- **所属层**：`crates/kernel/ze-type/`
- **相关**：[Layout](#layout) · [名义类型](#名义类型) · [决策 005](decisions/005-类型与反射.md)

## Layout

**一句话**：结构的内存布局，收进 `TypeSpec::Struct` 做字段（不再平行存在）；`None` 是安全缺省。

- **代码锚点**：`ze_type::layout::Layout`
- **所属层**：`crates/kernel/ze-type/`
- **相关**：[TypeSpec](#typespec) · [名义类型](#名义类型)

## 名义类型

**一句话**：`Struct` 同名不同布局即冲突——因为 `namespace + name` 是跨 Pod 的契约，不是实现细节。

- **锚点**：GAPS `AM-02` · 冲突诊断统一收敛到 `ReflectError`（GAPS `AM-03`）
- **所属层**：`crates/kernel/ze-type/`
- **相关**：[TypeSpec](#typespec) · [TypeRegistry](#typeregistry)

## OpSig

**一句话**：一条算子的槽位签名，供装配期合一（`Subst` + kind 检查 + occurs check）与重载解析。

- **代码锚点**：`ze_type::ty::OpSig` · `ze_pod_func::signature::signature_of`
- **所属层**：`crates/kernel/ze-type/`
- **相关**：[OpDef](#opdef) · [PointwiseReport](#pointwisereport)

## OpDef

**一句话**：算子目录里的一项——签名 + pointwise/targets + 版本标注（`since` / `changed`），唯一目录 `ze_func::OP_DEFS`。

- **代码锚点**：`ze_type::ty::OpDef` · `ze_func::OP_DEFS`
- **所属层**：`crates/kernel/ze-func/`
- **相关**：[OpSig](#opsig) · [PassRegistry](#passregistry)

## TypeRegistry

**一句话**：运行时类型登记表；同名重复由 `ReflectError` 统一诊断，不自动覆盖。

- **代码锚点**：`ze_reflect::codec::TypeRegistry`
- **所属层**：`crates/kernel/ze-reflect/`
- **相关**：[名义类型](#名义类型) · [SymbolTable](#symboltable)

## SlotTable

**一句话**：句柄槽位表——句柄 = 运行时对象的符号，`FunctionId` 带 owner + generation。

- **代码锚点**：`ze_reflect::handle::SlotTable`
- **所属层**：`crates/kernel/ze-reflect/`
- **相关**：[SymbolTable](#symboltable) · [Handle](#handle) · [FunctionId](#functionid)

## SymbolTable

**一句话**：符号 → 对象的有序表；与句柄表是同一数据结构的两面（句柄是运行时的符号，符号是编译期的句柄）。

- **代码锚点**：`ze_reflect::sym::SymbolTable`
- **所属层**：`crates/kernel/ze-reflect/`
- **相关**：[SlotTable](#slottable) · [慢路径 / 快路径](#慢路径--快路径)

## 慢路径 / 快路径

**一句话**：慢路径可序列化、需校验；一次性 resolve 后进进程内快路径。零拷贝是协商成功的奖励，不是唯一路径。

- **锚点**：GAPS `AM-04` · 删掉慢路径 ⇒ 跨 wasm 立刻死
- **所属层**：`crates/kernel/ze-type/` + `crates/kernel/ze-reflect/`
- **相关**：[Codec](#codec) · [MonoCache](#monocache) · [决策 005](decisions/005-类型与反射.md)

## Codec

**一句话**：慢路径的编解码契约；给「可序列化、需校验」的那条路一个正式名分。

- **代码锚点**：`ze_reflect::codec::Codec`（trait）
- **所属层**：`crates/kernel/ze-reflect/`
- **相关**：[慢路径 / 快路径](#慢路径--快路径) · [TypeRegistry](#typeregistry)

## MonoCache

**一句话**：单态化结果的缓存，按 `MAX_INSTANCES` 设上界；缓存可清空、可重建。

- **代码锚点**：`ze_reflect::cache::MonoCache`
- **所属层**：`crates/kernel/ze-reflect/`
- **相关**：[慢路径 / 快路径](#慢路径--快路径) · [TypeSpec](#typespec)

## Topology

**一句话**：图算法唯一依赖的最小接口——`nodes()` + `successors()`；算法不该知道什么是 Node。

- **代码锚点**：`ze_graph_core::topology::Topology`（trait）
- **所属层**：`crates/kernel/ze-graph-core/`
- **相关**：[topo_sort](#topo_sort) · [find_cycle](#find_cycle) · [决策 006](decisions/006-图基础设施.md)

## topo_sort

**一句话**：确定性拓扑排序，输出按 id 升序（`BTree*`，同一张图两次逐位相同），环则返回完整环路径。

- **代码锚点**：`ze_graph_core::sort::topo_sort`
- **所属层**：`crates/kernel/ze-graph-core/`
- **相关**：[Topology](#topology) · [find_cycle](#find_cycle) · [升序输出](#升序输出)

## find_cycle

**一句话**：返回一条完整环路径（不是只报「有环」）；表达式图与 Pod 装配图都走它。

- **代码锚点**：`ze_graph_core::scc::find_cycle` · `ze_graph_core::scc::strongly_connected`
- **所属层**：`crates/kernel/ze-graph-core/`
- **相关**：[topo_sort](#topo_sort) · [Topology](#topology)

## 升序输出

**一句话**：图的输出一律用 `BTree*` 而非 `HashMap`，把确定性落在输出边而不是容器实现上。

- **锚点**：GAPS `AN-02` · 表达式图与拓扑路径的 `HashMap` 门禁见 GAPS `AO-05`
- **所属层**：`crates/kernel/ze-graph-core/`
- **相关**：[Topology](#topology) · [决策 006](decisions/006-图基础设施.md)

## NodeMeta

**一句话**：节点图的元数据，只留 `{ type_name, version }`；展示数据已拆到 `NodePresentation`，不再进 core trait。

- **代码锚点**：`ze_node_core::node::NodeMeta`
- **所属层**：`crates/ze/graph/ze-node-core/`
- **相关**：节点图 `Node` · [Topology](#topology)

## Node（节点图）

**一句话**：节点图里一个有输入/输出 socket 的单元；算法只经 `Graph::input_count/output_count` 间接看它。

- **代码锚点**：`ze_node_core::node::Node`（trait）· `ze_node_core::graph::Graph`
- **所属层**：`crates/ze/graph/ze-node-core/`
- **相关**：[NodeMeta](#nodemeta) · [Topology](#topology)

## ObjectId

**一句话**：场景对象的稳定身份，遍历与指纹都按它升序；销毁后重建不复用旧身份。

- **代码锚点**：`ze_scene_contract::id::ObjectId`
- **所属层**：`crates/kernel/ze-scene-contract/`
- **相关**：[结构指纹](#结构指纹) · [WorldTransforms](#worldtransforms)

## FieldSpec

**一句话**：一个可动画字段的完整规格——持久化名 / 规范地址 / 组件 / 字段 / 类型 / 读 / 写；读写同源。

- **代码锚点**：`ze_scene_core::animation::target::FieldSpec`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[Target](#target) · [TARGETS](#targets) · [AnimValue](#animvalue)

## Target

**一句话**：字段的读写协议（`read` / `write` + 类型不符显式报错）；目标表是它的注册表。

- **代码锚点**：`ze_scene_core::animation::target::Target`（trait）· `ze_scene_core::animation::target::TargetError`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[FieldSpec](#fieldspec) · [SceneWrite](#scenewrite) · [决策 007](decisions/007-作者侧.md)

## TARGETS

**一句话**：场景可写字段的唯一登记表（手写 + `#[channel]` 派生），组合选择键带 `is_composite` 标记。

- **代码锚点**：`ze_scene_core::animation::target::TARGETS` · `ze_scene_core::animation::target::TargetEntry`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[FieldSpec](#fieldspec) · [METHODS](#methods)

## MethodSpec

**一句话**：场景方法的规格（参数 / 返回 / 静态声明的读集 / 求值），取代原先只读通道的特殊路径。

- **代码锚点**：`ze_scene_core::animation::target::MethodSpec`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[METHODS](#methods) · [MethodDecl](#methoddecl) · [决策 007](decisions/007-作者侧.md)

## METHODS

**一句话**：场景方法表；`reads` 静态声明参与帧内定序检查，未注册方法在装配期显式失败。

- **代码锚点**：`ze_scene_core::animation::target::METHODS`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[MethodSpec](#methodspec) · [check_read_order](#check_read_order)

## MethodDecl

**一句话**：Pod 对方法的自述（`component` / `name` / `qualified` / `arity` / `ret`），与 `channels()` 同构，供三语言绑定挂到构造器上。

- **代码锚点**：`ze_pod_core::pod::MethodDecl`
- **所属层**：`crates/ze/pod/ze-pod-core/`
- **相关**：[MethodSpec](#methodspec) · [Pod](#pod)

## MethodRef

**一句话**：表达式里对方法调用的引用（方法名 + 对象句柄），参数个数与存在性在装配期校验。

- **代码锚点**：`ze_comb::expr::MethodRef`
- **所属层**：`crates/kernel/ze-comb/`
- **相关**：[MethodDecl](#methoddecl) · [Mono](#mono)

## SeekKind

**一句话**：引擎推导出的定位代价（`O1` / `Replay`），不进脚本、不进产物；有记忆的演化必然是 `Replay`。

- **代码锚点**：`ze_scene_core::animation::binding::SeekKind`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[seek 代价](#seek-代价) · [Binding](#binding) · [L2](#l2)

## Binding

**一句话**：一条表达式绑定：根表达式 + 时钟 + predicate / reset + 目标；可分析时携带区间集，`Hold` 退化成纯函数。

- **代码锚点**：`ze_scene_core::animation::binding::Binding`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[IntervalSet](#intervalset) · [SeekKind](#seekkind)

## BindingTable

**一句话**：全部绑定的有序集合；装配期定序、运行期按序应用，不依赖容器迭代序。

- **代码锚点**：`ze_scene_core::animation::binding::BindingTable`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[Binding](#binding) · [BindingApplySystem](#bindingapplysystem)

## IntervalSet

**一句话**：predicate 映射出的局部时间区间集（有序、互不相交、端点开闭规范化），支持交 / 并 / 补。

- **代码锚点**：`ze_scene_core::animation::predicate::IntervalSet`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[Binding](#binding) · [still_time](#still_time)

## SceneScript

**一句话**：脚本装配的产物脚本（创建 + 绑定 + 时钟），`still_time` 与读序检查都在这里收敛。

- **代码锚点**：`ze_scene_core::scene_script::SceneScript`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[check_read_order](#check_read_order) · [still_time](#still_time) · [ZSB](#zsb)

## SceneRead

**一句话**：场景的只读视图（按地址读字段），读端与写端对称，不再是第二张表。

- **代码锚点**：`ze_scene_contract::scene::SceneRead`（trait）· `ze_scene_core::scene_write::SceneRead`
- **所属层**：`crates/kernel/ze-scene-contract/`
- **相关**：[SceneWrite](#scenewrite) · [FieldSpec](#fieldspec)

## SceneWrite

**一句话**：场景的写端协议；写失败必须显式（`TypeMismatch`），不丢弃。

- **代码锚点**：`ze_scene_core::scene_write::SceneWrite`（trait）
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[SceneRead](#sceneread) · [Target](#target)

## check_read_order

**一句话**：帧内定序检查——同一 `(地址, 对象)` 的写者时钟起点晚于读者则装配期显式拒绝（等价于代数环 / 直接馈通检测）。

- **代码锚点**：`ze_scene_core::scene_script::check_read_order`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[开环控制](#开环控制) · [METHODS](#methods)

## still_time

**一句话**：从「只依赖时间」的 predicate 反解静止时刻；不可分析返回 `None`，不拿 0 冒充。

- **代码锚点**：`ze_scene_core::scene_script::still_time`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[IntervalSet](#intervalset) · [SceneScript](#scenescript)

## SceneApi

**一句话**：脚本侧的场景能力面（构造器 / 通道常量 / 方法），由 `SceneServicePod` 注册并按指纹固定。

- **代码锚点**：`ze_scene_api::api::SceneApi` · `ze_scene_api::api::SCENE_API_FINGERPRINT`
- **所属层**：`crates/scene/ze-scene-api/`
- **相关**：[ContentApi](#contentapi) · [PodAssembly](#podassembly)

## ContentApi

**一句话**：脚本侧的内容能力面（文本 / 路径 / 排版回读）；内容以路径形态进渲染 IR。

- **代码锚点**：`ze_scene_api::api::ContentApi`
- **所属层**：`crates/scene/ze-scene-api/`
- **相关**：[SceneApi](#sceneapi) · [Path2D](#path2d)

## Path2D

**一句话**：二维几何基元（折线 + 闭合标记），弧长展平是 `Reveal` 与 `Bounds` 的共同口径。

- **代码锚点**：`ze_notion::path::Path2D`
- **所属层**：`crates/kernel/ze-notion/`
- **相关**：[ContentApi](#contentapi) · [Bounds](#bounds)

## Bounds

**一句话**：对象的二维包围盒聚合入口；空 / 退化几何一律 `None`，不拿零矩形冒充。

- **锚点**：GAPS `AP-08` · `object_bounds` 读取顺序固定（Circle → Path → Line → Arrow → Rect → Reveal）
- **所属层**：`crates/scene/ze-scene-core/src/bounds.rs`
- **相关**：[Path2D](#path2d) · [METHODS](#methods)

## Query / bind_each

**一句话**：按组件类型组合在装配期展开成 N 条具体绑定；`it`（`expr.query_self`）是哨兵，不是闭包。

- **锚点**：GAPS `AP-09` · 结果集按 `ObjectId` 升序，产物里既没有 `it` 也没有 query
- **所属层**：`crates/ze/pod/ze-pod-func/` + `crates/app/zela-app/`
- **相关**：[Binding](#binding) · [ObjectId](#objectid)

## 零回读上屏

**一句话**：矢量帧渲进 `Rgba8Unorm` 中间纹理、注册成 egui 原生纹理直接采样，像素不经 CPU 往返（对照：`render_at` → `ColorImage` → `set/load_texture`）。

- **锚点**：GAPS `AX-01` · 宿主在拿到 `wgpu_render_state` 时 `init_gpu(device, queue, adapter)`，此后矢量后端不自建 device；device features 取"已注册且可用后端之并"
- **所属层**：`crates/app/zela-gui/` + `crates/render/zela-render-vector/`
- **相关**：[像素基线](#像素基线) · [门禁](#门禁)

## 结构指纹

**一句话**：全量对象 × 全量字段的逐位摘要（含 `WorldTransforms`），每帧独立、不链式，能定位到具体帧。

- **锚点**：GAPS `AQ-03` · `AnimValue::Path` 只写占位并计入 `derived_excluded`（见 GAPS `AR-08` 的盲区记账）
- **所属层**：`crates/scene/zela-scene-ecs/src/fingerprint.rs`
- **相关**：[像素基线](#像素基线) · [确定性第四判据](#确定性第四判据)

## 像素基线

**一句话**：渲染管线的端到端守门员——抽样关键帧的像素哈希；跨端逐位相同明确不做承诺。

- **锚点**：GAPS `AQ-04` · 结构指纹走等式、性能走不等式，两者不合并判据
- **所属层**：`crates/app/zela-app/scripts/`
- **相关**：[结构指纹](#结构指纹) · [门禁](#门禁)

## disposition

**一句话**：每个脚本对「有没有像素基线」的显式声明（`baseline` 或带理由的豁免），未登记 / 假声明都 fail。

- **锚点**：GAPS `AQ-07` · 登记文件 `crates/app/zela-app/scripts/dispositions.txt`，由 `check_dispositions` 核对
- **所属层**：`crates/app/zela-app/scripts/`
- **相关**：[像素基线](#像素基线) · [门禁](#门禁)

## 门禁

**一句话**：机器可执行的判据（结构 / 指纹 / 覆盖 / 文档 / 能力表 / 重名白名单），把纪律从注释变成必过步骤。

- **锚点**：GAPS `AQ-06` · 等式比对 `compare_structures`，不等式比对 `compare_bench`（20% 警告 / 50% 失败）
- **所属层**：`xtask/`
- **相关**：[结构指纹](#结构指纹) · [disposition](#disposition)

## PodAssembly

**一句话**：宿主侧的装配器——按对象名把 Pod 提交的绑定并入 `SceneScript`，是句柄转换的唯一入口。

- **代码锚点**：`zela_app::assembly::PodAssembly`
- **所属层**：`crates/app/zela-app/`
- **相关**：[SceneApi](#sceneapi) · [SceneScript](#scenescript)

## BindingApplySystem

**一句话**：每帧按绑定局部时间求值并写场景；`Hold` 在可分析时是纯函数，seek 不需要重放前面的帧。

- **代码锚点**：`zela_scene_ecs::animation::system::BindingApplySystem`
- **所属层**：`crates/scene/zela-scene-ecs/`
- **相关**：[BindingTable](#bindingtable) · [SeekKind](#seekkind)

## WorldTransforms

**一句话**：父子层级累乘后的世界变换（存 3D，`get2d` 是投影）；2D 与 3D 共用同一层级。

- **代码锚点**：`ze_scene_core::world_transforms::WorldTransforms`
- **所属层**：`crates/scene/ze-scene-core/`
- **相关**：[结构指纹](#结构指纹) · [ObjectId](#objectid)

## Bundle

**一句话**：已解析的 ZSB 产物（payload 保持原始字节，hash 因此可复算）。

- **代码锚点**：`zela_scene_bundle::lib::Bundle`
- **所属层**：`crates/scene/zela-scene-bundle/`
- **相关**：[ZSB](#zsb) · [InputStream](#inputstream)

## ZSB

**一句话**：`Zela Scene Bundle`——脚本在 Native 侧执行后的纯数据产物（`.zsb`），浏览器只回放它、不跑脚本。

- **锚点**：GAPS `AI-04` · 输入流用 `Pairing { zsb_payload_hash, libraries }` 记录「为谁而录」，不匹配显式 fail
- **所属层**：`crates/scene/zela-scene-bundle/`
- **相关**：[Bundle](#bundle) · [InputStream](#inputstream)

## InputStream

**一句话**：按帧号索引的稀疏事件流（`zela.input/1`），空帧不写；回放前先与 ZSB 配对校验。

- **代码锚点**：`zela_scene_bundle::input_stream::InputStream`
- **所属层**：`crates/scene/zela-scene-bundle/`
- **相关**：[Recorder](#recorder) · [ZSB](#zsb)

## Recorder

**一句话**：输入流录制器，默认关（`capture` 是 no-op），显式 `recording(pairing)` 才录。

- **代码锚点**：`zela_scene_bundle::input_stream::Recorder`
- **所属层**：`crates/scene/zela-scene-bundle/`
- **相关**：[InputStream](#inputstream)

## Scheduler

**一句话**：ECS 调度框架——系统按资源读写冲突排序执行，telemetry 只服务 UI，不进指纹线。

- **代码锚点**：`zela_schedule::scheduler::Scheduler`
- **所属层**：`crates/schedule/zela-schedule/`
- **相关**：[BindingApplySystem](#bindingapplysystem) · [结构指纹](#结构指纹)

## DrawCommand

**一句话**：渲染 IR 的一条绘制指令（几何 + 样式 + 图层 + z 序），后端只消费它。

- **代码锚点**：`ze_render_ir::draw::DrawCommand` · `ze_render_ir::draw::ZOrder`
- **所属层**：`crates/kernel/ze-render-ir/`
- **相关**：[ResourceCache](#resourcecache) · [VertexLayout](#vertexlayout)

## ResourceCache

**一句话**：GPU 资源缓存，按 `GpuResourceId` 复用；失效与重建必须可观察。

- **代码锚点**：`ze_resource::resource_cache::ResourceCache` · `ze_resource::render_backend::GpuResourceId`
- **所属层**：`crates/kernel/ze-resource/`
- **相关**：[DrawCommand](#drawcommand) · [VertexLayout](#vertexlayout)

## VertexLayout

**一句话**：顶点格式契约（属性 + 步长 + 输入描述），后端与顶点生产侧的唯一边界。

- **代码锚点**：`ze_vertex::layout::VertexLayout` · `ze_vertex::layout::VertexInputDesc`
- **所属层**：`crates/kernel/ze-vertex/`
- **相关**：[DrawCommand](#drawcommand) · [ResourceCache](#resourcecache)

## Handle

**一句话**：Pod 内部的句柄（槽位下标）；产物里只有脚本局部句柄，跨到场景时才重写成对象身份。

- **代码锚点**：`ze_pod_core::handle::Handle`
- **所属层**：`crates/ze/pod/ze-pod-core/`
- **相关**：[SlotTable](#slottable) · [PodAssembly](#podassembly)

## FunctionId

**一句话**：Pod 函数身份（owner + generation），避免卸载后旧句柄命中断言。

- **代码锚点**：`ze_pod_core::pod::FunctionId`
- **所属层**：`crates/ze/pod/ze-pod-core/`
- **相关**：[Handle](#handle) · [SlotTable](#slottable)

## ReadSpec

**一句话**：脚本侧读端规格（通道 + 显式对象 + 类型）；「读自己」没有隐式默认。

- **代码锚点**：`ze_pod_func::api::ReadSpec`
- **所属层**：`crates/ze/pod/ze-pod-func/`
- **相关**：[Mono](#mono) · [ChannelSpec](#channelspec)
