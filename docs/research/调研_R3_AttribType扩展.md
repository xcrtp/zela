# 调研 R3 · `AttribType` 扩展需求

> 现状：[layout.rs:54-59](file:///d:/Users/xcdh/Desktop/xc/zela/crates/kernel/ze-vertex/src/layout.rs#L54-L59) `enum AttribType { F32, F32x2, F32x3, F32x4 }`
> 性质：只读调研，不改代码

## 1. 结论（二值）

**本批不加 `U8x4Norm`。** 依值为：全仓当前没有任何消费者（骨骼未落地），加了只是把 `AttribType` 的枚举面与 serde 面提前扩大；而它落地时是**三处穷尽 match 各加一行**，成本不随推迟而上涨。

同时给出将来加时的**确定取值**（不是待定），供骨骼批直接照抄。

## 2. 若加，确定取值

```rust
// ze-vertex/src/layout.rs
pub enum AttribType {
    F32,
    F32x2,
    F32x3,
    F32x4,
    U8x4Norm,   // 归一化 u8 ×4（骨骼 JOINTS_0 / WEIGHTS_0 的常规形态）
}

impl AttribType {
    pub const fn size(self) -> u32 {
        match self {
            Self::F32 => 4,
            Self::F32x2 => 8,
            Self::F32x3 => 12,
            Self::F32x4 => 16,
            Self::U8x4Norm => 4,      // 4 × 1 字节
        }
    }
    // alignment() 仍是常量 4（见 §3），无需改动
}
```

| 项 | 取值 | 依据 |
|---|---|---|
| `size()` | `4` | 4 个 u8 |
| `alignment()` | `4`（沿用现有常量返回） | 后端最小合法对齐是 4（见 §3） |
| `to_wgpu_format`（[wgpu_3d.rs:119](file:///d:/Users/xcdh/Desktop/xc/zela/crates/render/zela-render-backends/src/wgpu_3d.rs#L117-L124)） | `wgpu::VertexFormat::Unorm8x4` | 该变体在 wgpu 30 存在（`wgpu-30.0.1/src/backend/webgpu.rs:549` 映射到 `GpuVertexFormat::Unorm8x4`） |
| `attrib_format`（[vertex_layout.rs:21](file:///d:/Users/xcdh/Desktop/xc/zela/crates/render/zela-render-backends/src/vertex_layout.rs#L19-L26)） | 同上 `Unorm8x4` | 这是**第二处**穷尽 match，改 `AttribType` 必须同时补 |

## 3. 对齐与既有布局的影响

- 项目侧的对齐规则是"后端最小合法对齐（4）"（[layout.rs:72-81](file:///d:/Users/xcdh/Desktop/xc/zela/crates/kernel/ze-vertex/src/layout.rs#L72-L81)），与 wgpu 的 `VERTEX_ALIGNMENT = 4`（`wgpu-types-30.0.1/src/lib.rs:138`）一致。`U8x4Norm` 的 size 是 4，正好是 4 的倍数 ⇒ **stride 与 offset 都天然合法，不需要任何特例**。
- 既有布局全部只用 `F32*` ⇒ `compute_layout` 的输出逐位不变（`standard()` 0/8/16/20 stride 24；`standard_3d()` 0/12/20 stride 32）。
- 因此"加 `U8x4Norm`"在**字节层面是纯增量**，不会让任何基线像素或字节发生变化。

## 4. 带宽取舍（决定本批做不做）

| 承载方式 | 每顶点字节 | 相对 |
|---|---|---|
| `F32x4` ×2（索引 + 权重） | 32 | 基准 |
| `U8x4Norm` ×2 | 8 | **1/4** |

4 倍差距只在"每顶点都要带骨骼索引与权重"时才有意义。当前 3D 顶点格式固定 stride 32，加两个槽后：`F32x4` 方案 stride 64，`U8x4Norm` 方案 stride 40。差别显著，但前提是骨骼系统存在。

## 5. 附带发现

`AttribType` 的穷尽 match 有 **3 处**，实施时漏任何一处都编译失败（这是好事，但排期时要算进改动面）：

1. `layout.rs:63-70` `size()`
2. `wgpu_3d.rs:117-124` `to_wgpu_format`
3. `vertex_layout.rs:19-26` `attrib_format`

后两处是**同一映射的第二份实现**（3D 后端自带一份，2D 翻译层自带一份）。这不是这次的调研目标，但新增类型时必须两处同改；若将来要收敛，应让 `wgpu_3d.rs` 直接复用 `vertex_layout::attrib_format`。
