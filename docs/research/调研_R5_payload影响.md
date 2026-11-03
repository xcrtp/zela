# 调研 R5 · payload 影响判定

> 问题：给 `VertexLayout` 加字段（如 `requirements`）要不要 bump `LIBRARY_VERSION`、要不要重录基线？
> 性质：只读调研，不改代码

## 1. 结论（二值）

**不进 payload。** 因此加字段**不需要** bump `LIBRARY_VERSION`，也**不需要**重录 `baselines.txt` / `structures.txt`。

唯一会变的是**进程内** pipeline 变体缓存（内存 HashMap），进程重启即重建，无迁移成本。

## 2. 证据

### 2.1 payload 是什么

`ZSB` 三段布局（[zela-scene-bundle/src/lib.rs:11-19](file:///d:/Users/xcdh/Desktop/xc/zela/crates/scene/zela-scene-bundle/src/lib.rs#L11-L19)）：

```text
Header（24 B）
Payload（hash 只覆盖这段）  ← SceneScript 的规范 JSON
Meta（不进 hash）
```

⇒ payload 的**唯一内容**是 `SceneScript` 的规范 JSON。顶点布局是否进 payload，等价于"`SceneScript` 里有没有 `VertexLayout`"。

### 2.2 `VertexLayout` 不在任何 scene 侧类型里

| 检查 | 结果 |
|---|---|
| `grep VertexLayout crates/scene/zela-scene-bundle` | 无引用 |
| `grep VertexLayout crates/scene/` | 无引用（`ze-scene-macros` 的 `layout:` 是 Pod 的 `ze_pod_core::Layout`，同名不同物） |
| `grep shading_normal` 全仓 | 仅 3 个文件：`ze-vertex/src/layout.rs`、`zela-render-backends/src/wgpu_3d.rs`、`docs/modules/ze-vertex.md` —— 没有第二个落盘格式带它 |

### 2.3 `VertexLayout` 的 serde 目前只被测试驱动

派生 `Serialize` / `Deserialize` 在 [layout.rs:156](file:///d:/Users/xcdh/Desktop/xc/zela/crates/kernel/ze-vertex/src/layout.rs#L156)，但运行时序列化点只有测试：

- [layout.rs:574](file:///d:/Users/xcdh/Desktop/xc/zela/crates/kernel/ze-vertex/src/layout.rs#L574) `serde_roundtrip_is_identical`
- [layout.rs:474](file:///d:/Users/xcdh/Desktop/xc/zela/crates/kernel/ze-vertex/src/layout.rs#L474)（3D 往返）
- [layout.rs:737](file:///d:/Users/xcdh/Desktop/xc/zela/crates/kernel/ze-vertex/src/layout.rs#L737) `instance_layout_serde_roundtrip`

⇒ serde 能力是**为将来**留的，现在没有持久化消费者。所以 `VertexAttrib.semantic`（`&'static str`）也不会进任何产物。

### 2.4 承载 `VertexLayout` 的两个类型都不持久化

| 类型 | serde | 用途 |
|---|---|---|
| [`PipelineDesc`](file:///d:/Users/xcdh/Desktop/xc/zela/crates/kernel/ze-render-ir/src/pipeline.rs#L34-L42) | **无**（只 derive `Debug/Clone/PartialEq/Eq/Hash`） | IR 意图声明；管线的 `vertex_layout` 经它传给后端 |
| [`PipelineKey`](file:///d:/Users/xcdh/Desktop/xc/zela/crates/render/zela-render-backends/src/wgpu_3d.rs#L222-L229) | **无** | 进程内 pipeline 缓存键（`self.pipelines: Mutex<HashMap<..>>`，[:1340-1345](file:///d:/Users/xcdh/Desktop/xc/zela/crates/render/zela-render-backends/src/wgpu_3d.rs#L1340-L1345)） |

`StaticConsistencyChecker` 的 `HashMap<(VertexLayout, u64), u64>`（[check.rs:82-86](file:///d:/Users/xcdh/Desktop/xc/zela/crates/kernel/ze-vertex/src/check.rs#L82-L86)）同样是进程内、且 release 下 no-op。

### 2.5 `LIBRARY_VERSION` 管的是另一件事

[`LIBRARY_VERSION = "24"`](file:///d:/Users/xcdh/Desktop/xc/zela/crates/ze/pod/ze-pod-func/src/expr_pod.rs#L75) 是 `zela.math` 函子库的**签名身份**，进表达式产物 ⇒ 算子签名一变就要 bump（历史先例见 GAPS `BE-03` / `BF-06`）。顶点布局不在它的覆盖范围内。

### 2.6 基线的两份清单（用于说明"重录"代价）

| 基线 | 位置 | 条目数（本次基线读数） |
|---|---|---|
| 像素序列哈希 | `crates/app/zela-app/scripts/baselines.txt` | 36 |
| 结构指纹 | `crates/app/zela-app/scripts/structures.txt` | 22 |

本次调研前后各跑一次，读数均为 **哈希 36 项 ok 36 / DIFF 0**、**结构全部通过**。

## 3. 一个必须分清的区别

**"不进 payload"不等于"像素不变"。**

- payload = `SceneScript` JSON ⇒ 不受顶点布局影响，加字段不必重录；
- 像素 = 顶点字节 + 布局 + shader ⇒ **改 `standard_3d()` 的既有字节（stride / offset / 属性顺序）会让像素动**，只是不会让 payload hash 动。

所以实施阶段的风险不在"要不要 bump"，而在这条：

> 加字段（`requirements`、`U8x4Norm` 等）是安全的；
> **改既有属性的顺序 / 类型 / offset 是不安全的**（像素，不是 payload）。

## 4. 判据核对表

| 判据 | 结果 |
|---|---|
| 二值结论 | 不进 payload |
| 代码证据 | §2.1–§2.5 全部可核对 |
| bump 代价 | 无（不需要） |
| 重录代价 | 无（不需要） |
| 例外 | 若把需求声明放进 `SceneScript`（不建议），才会进 payload 并需要 bump |
