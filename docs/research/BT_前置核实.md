# BT 前置核实（程序化材质与 SDF）

> 对象：《程序化材质与SDF_执行计划》BT-0
> 性质：只读核实 + 基线读数（除本节记录外不改产品代码）
> 结论只影响 BT-6 的形态与 BT-4/5/6 的验收方式，不改任何既有判据

## 0. 结论速览

| 项 | 结论 | 影响 |
|---|---|---|
| 基线读数 | 像素 36 条 · 结构 22 件 · demo 34 条 payload · 覆盖 72/72 · docs 全绿 | BT 各阶段以这份读数为红线 |
| 3D 内建（`world_pos` / `normal`） | **没有 per-pixel 3D 属性** | BT-6 走屏幕空间内建，`world_pos` / `normal` 记账 |
| 新 Demo 像素基线 | 本环境 `ZELA_RECORD_DEVICE` 为空 ⇒ **不是录制设备** | BT-4/5/6 的新脚本只能标「待录」，不许就地造数 |
| `eval` 产物快照 | `codegen_fill.rs` 已有"生成 + 执行 + 像素对拍"的形态 | BT-3 的手写入口复用同一契约，不另造格式 |

## 1. 基线读数（BT-0 起点）

在 BS 批收尾、BT-1 之前的提交上跑：

```
cargo verify --step hashes      # 合计 36 项：ok 36 · DIFF 0 · no-slot 0；结构指纹 22 件与基线一致
cargo verify --step demo        # 34 条 payload 断言通过
cargo verify --step structure   # 22 件作品与基线一致（等式；wgpu/vector × script/bundle 同值）
cargo verify --step coverage    # 能力覆盖 72/72（豁免 0 条）
cargo verify --step docs        # 文档工程全绿
```

这些数字是 BT 批的贯穿判据（计划 §3）：已有作品的像素、结构、payload 一条都不许变；
新 Demo 只**新增**条目。

## 2. 3D 内建核实（决定 BT-6 的形态）

### 2.1 着色内建只有 5 个，全是 vec2

`ze-func/src/wgsl.rs` 的 `builtin()` 是表达式通道 → 着色器标识符的**唯一**映射表：

```rust
("zela.shader", "center")          => zela_center   // Vec2
("zela.shader", "size")            => zela_size     // Vec2
("zela.shader", "fragment.pos2")   => zela_pos2     // Vec2
("zela.shader", "fragment.local")  => zela_local    // Vec2
("zela.shader", "fragment.uv")     => zela_uv       // Vec2
```

没有 `world_pos` / `normal`，也没有 vec3 内建。

### 2.2 两条入口尾部都只声明这 5 个 `var<private>`

`render.rs` 的 `FILL_TAIL`（对象级，行 1091–1095）与 `FRAG_TAIL`（逐片元，行 1142–1146）
各声明 `zela_center / zela_size / zela_pos2 / zela_local / zela_uv`，并由入口在每线程开头赋值；
**没有第三组坐标**可供 `eval` 读。

### 2.3 合成阶段也没有 per-pixel 3D 属性

- `vector.wgsl` 的 `shaded_fill(o: u32, up: vec2<f32>)` 只吃**2D 屏幕坐标**（y 向上），
  返回颜色画布上的颜色；
- 组 1 的 24 个槽位里与 3D 有关的只有 `base_samples`（7）/ `base_sample_depths`（8）/
  `base_light_samples`（18）——颜色与深度，**没有法线 G-buffer**；
- 每个 `Object` 确实带 `normal` / `place`（3D 受光填充用），但那是**合成 compute**
  读的对象缓冲；逐片元着色的绑定（`frag_layout`）只有 Params / 颜色画布 / 离屏标签 /
  `ZelaFrag` 四个绑定，**读不到对象缓冲，也读不到 `FrameUniform.inverse`**
  ⇒ 无法从 `pos2` 反算世界坐标或法线。

### 2.4 结论

BT-6 **走屏幕空间内建**：`zela_local` / `zela_pos2` / `zela_uv` 在 3D 视图同样可用
（它们由对象包围盒与视图像素定义，与相机无关）。`world_pos` / `normal` 作为缺口记账，
后续要做得先给逐片元入口补 per-pixel 3D 属性（需要新的绑定点，属于另一批）。

## 3. 新 Demo 的像素基线（§0.8 的确认）

本机实测：

```powershell
"ZELA_RECORD_DEVICE=" + $env:ZELA_RECORD_DEVICE   # => 空
```

`cargo verify --record` 要求 `ZELA_RECORD_DEVICE` 非空（xtask 会显式报错）。
⇒ 本环境**不是录制设备**，BT-4/5/6 新增的脚本：

- `dispositions.txt` 写 `exempt: …；像素基线待录（录制设备）`；
- `manifest.txt` 的 payload hash 由批量编译器生成（与设备无关，可以就地更新）；
- **不新增 `baselines.txt` 条目**，也不改已有的 36 条像素基线。

## 4. `eval` 产物的现有快照形态

`crates/render/zela-render-backends/tests/codegen_fill.rs` 已经固定了一条完整链路：

1. `ze_comb::emit_wgsl` 生成 `vec3<f32>(sin(time), 0.25, 0.5)` 的 WGSL，
   并断言 `emitted.ret == Vec3`、`emitted.params.len() == 1`；
2. 按 std140 打包 uniform 字节（`time` 在 `Params` 的前 4 字节）；
3. 经 `zela_render_vector` 渲染后与 CPU 参考比对（容差 4/255）；
4. 同一帧连跑两次逐位相同；没有程序时与普通填充逐位相同。

BT-3 的手写入口直接复用这个契约：**`struct Params { … }` + `fn eval(p: Params, index: u32) -> T`**，
后端只吃 `source` 字符串 ⇒ 不需要为"手写"另造一套格式，也没有第二套入口尾部。
