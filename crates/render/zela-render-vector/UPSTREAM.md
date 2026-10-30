# 上游快照与许可清单

```text
upstream: manimgx @ dcfe01a932118263c238a50563d1bb6fab09e08e
path:     manimgx/rust/engine/src/
licenses: MIT（多数） + Apache-2.0（light / occlusion / tone / bloom + dfg.bin）
```

- **SHA 是唯一凭据**：同步上游或回答"这段代码哪来的"时必须引用它。
- 上游 `REUSE.toml` 的 `path = "**"` 兜底声明"未被其它注解认领的文件都是
  `2026 Academa, Inc.` / MIT"，因此**多数文件自身没有 SPDX 头**。
- `LICENSES/MIT.txt` / `LICENSES/Apache-2.0.txt` 逐字复制自上游 `LICENSES/`。

## Apache-2.0 文件（AOSP 版权，改编自 Google Filament）

| 文件 | SPDX-FileCopyrightText（除 Academa 外） |
|---|---|
| `light.wgsl` | The Android Open Source Project |
| `occlusion.wgsl` | 2021 The Android Open Source Project |
| `occlusion.rs` | 2016 The Android Open Source Project |
| `tone.wgsl` | 2021 The Android Open Source Project |
| `bloom.wgsl` | The Android Open Source Project |
| `dfg.bin` | 2015 The Android Open Source Project |

这 6 个文件**恰好是着色栈核心**（PBR 直接光 / AO / 色调映射 / 泛光 / DFG 查表）。

## 移植标注规则（B4 §三，强制）

1. **Apache-2.0 文件**：保留上游四行原文，追加一行修改说明：

   ```text
   // SPDX-FileCopyrightText: 2026 Academa, Inc.
   // SPDX-FileCopyrightText: The Android Open Source Project
   // SPDX-License-Identifier: Apache-2.0
   // Modified by Academa, Inc.
   // Modified by Zela: <一句话说明改了什么>
   ```

2. **MIT 文件（上游无头）**：由我们补齐头部：

   ```text
   // SPDX-FileCopyrightText: 2026 Academa, Inc.
   // SPDX-License-Identifier: MIT
   // Modified by Zela: <一句话说明改了什么>
   ```

3. 多处修改 ⇒ 多行 `// Modified by Zela: …`，保持可追溯；**不允许**只写一句笼统说明。
   **例外（BO 批起）**：若整个文件被**大幅重写**（改动覆盖大部分行、逐处标注会被淹没），
   允许在该文件头部写一行
   `// Modified by Zela: 大幅重写，原始版本见 <upstream SHA>:<path>`，
   **并在本文件登记该文件**（见下节"大幅重写登记"）。未登记就套用这条 = 违反规则。

   约束（仅 Apache-2.0 那 6 个文件）：上游四行原文必须**保留**，只允许追加，不允许替换——
   Apache-2.0 §4(b) 要求"显著说明你改过"，那条义务不因"大幅重写"而消失。

## 大幅重写登记

| 文件 | 批次 | 状态 |
|---|---|---|
| （暂无） | — | BO 批的改动是逐处的参数计算与调度，未出现大幅重写；因此本批走第 3 条主规则（逐处标注） |

## 依赖边界

上游 `Cargo.toml` 的 feature 里，本 crate **只开 `render`**：

```text
render = ["dep:wgpu", "dep:ash", "dep:libc", "dep:dxc", "dep:windows-sys"]
```

`python` / `typeset` / `export` / `window` / `web` 全部不开 ⇒ pyo3 / typst /
**x264(GPL)** / ffmpeg(LGPL) / winit / cpal 一个都不进来。

wgpu 版本：我方 `"30"`，上游 `"30.0.1"`（同版本），shader 可直接使用。

## 移植范围（Step 5）

| 内容 | 说明 |
|---|---|
| `vector.wgsl` | 三 pass 核心：flatten / raster / composite |
| `vector.rs` | Rust 侧装配 |
| `render.rs` | 部分（`Record` / `Store` / `Camera` / `Shape` / `Paint` 等公共类型） |
| `vector_buffers.wgsl` / `vector_compute.wgsl` | pass 0 / pass 2 |
| `environment.rs` / `shadow.rs` / `bloom.rs` / `occlusion.rs` | 3D 着色栈 |
| `blend*.wgsl` / `mesh.rs` / `depth.wgsl` | 点云与网格光栅 |

**剔除**：`python.rs` / `typeset.rs` / `text.rs` / `export.rs` / `mp4.rs` / `aac.rs` /
`audio.rs` / `speaker.rs` / `chrome.rs` / `web.rs` / `player.rs` / `take.rs` /
`project.rs` / `stage.rs` / `pack.rs` / `encode.rs` / `nv12.wgsl` / `lavapipe.rs` / `dxc.rs`
