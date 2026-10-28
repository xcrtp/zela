# 运行时：装配期 → 每帧循环 → 产物

> 以源码为准。凡本篇没写死的地方，去对应文件的 `//!` 注释读——那里是唯一真源，
> 本篇不复述易腐烂的签名。

## 一句话

一次运行只有三段：**装配期**把脚本或产物变成 `Scene` + 系统图 + Driver；
**每帧**只有一条 `Advance` 命令推进六阶段并渲染；**产物**是 ZSB、哈希、结构指纹、计时与决策日志。

## 一、装配期

两条入口，**同一条尾**（防双轨）：

```
脚本路径：Lua / JS / Py → 脚本调用 zela.scene → SceneEncoder → SceneScript
产物路径：.zsb → Bundle.script() → SceneScript
                     ↓ 同一条尾
SceneScript::commit(&mut Scene) → SceneRecipe{ clock 树, BindingTable, 句柄映射 }
                     ↓
assemble_runtime → Runtime{ DriverPod, RenderContext, duration/record_hint }
```

- **脚本只提交结构**：`SceneScript` 是命令序列，句柄是脚本内的局部下标；`commit` 是**唯一**
  把局部句柄映射成 `ObjectId` 的地方，也是唯一解析「组件里存另一个对象句柄」的地方。
- **时钟树与绑定表**在 `commit` 时产出，随后作为 Resource 插进调度器；绑定表的 target 在
  `commit` 时按真实 id 生成。
- **装配尾只有一份**：`assemble_runtime` 负责资源插入、System 注册（场景 + 渲染录制）、
  Driver pod 构造。`zela-app`（脚本 / 内置作品）与浏览器回放都走它。
- 装配期错误一律显式：句柄越界、未知目标、表达式非法、方法参数不符都在这里失败，不留给运行期。

细节见 [`zela-playback`](../../crates/app/zela-playback/src/lib.rs)、
[`scene_script.rs`](../../crates/scene/ze-scene-core/src/scene_script.rs)。

## 二、每帧循环

**唯一实现**：[`frame_loop.rs`](../../crates/app/zela-app/src/frame_loop.rs) 的 `run_frames`——
`zela-render`（导出）与 `zela-verify`（校验）都调它，不允许有第二条渲染循环。

单帧顺序（同时也是计时字段的划分）：

| 段 | 做什么 |
|---|---|
| `advance` | `submit(Advance { dt }).wait()` —— 导出用固定 `dt = 1/fps`，GUI 用墙钟 |
| （无计时） | 结构指纹：**推进之后、渲染之前**，读组件字段 + 世界矩阵 |
| （无计时） | 路由决策：作品级，第一帧决定后固定（诊断开关 `AutoFrame` 每帧重算） |
| `build` | 取帧快照（矢量路径）或构造 `SceneView` + 渲染回调（wgpu 路径） |
| `prepare` / `paint` / `submit` / `read` | 后端渲染与回读（矢量路径的 `prepare` 含整帧渲染） |
| `sink` / `hash` | 交给调用方输出或校验；回调返回其中的哈希耗时，循环据此拆出 `hash` |
| `end` | `end_frame`：帧末回收（缓存、延迟销毁） |
| `verify` | GPU 校验 pass **未启用** ⇒ 恒 0；`verify_ms = 0` 表示「没有校验」，不是「校验不耗时」 |

⚠️ 两条不变量：**首帧计时不计入门禁**（含管线创建）；单帧超过量级阈值即失败，且该门禁恒开。

## 三、命令-事件

| | 命令队列 | 事件 topic |
|---|---|---|
| 方向 | 控制 | 通知 |
| 语义 | FIFO、顺序确定 | 异步、可丢 |
| 参与帧推进 | **是** | **否** |
| 回执 | `Receipt::wait()` | — |

**铁律：任何影响场景状态的都必须走命令队列；事件只用于观测。** 两者是**不同的泛型类型**
（命令 vs 事件），把事件当命令提交是编译错误——若退化成同一个值类型承载，约束就只剩约定。

GUI 与 headless 共用同一个 Driver pod，差别只在 `dt` 从哪来；因此不存在「另写一套简化帧循环」的分叉。
展开说明见 [`ARCHITECTURE.md`](../../ARCHITECTURE.md) 的「命令 / 事件机制」。

## 四、调度流水线（六阶段）

阶段枚举与执行顺序由 [`scheduler.rs`](../../crates/schedule/zela-schedule/src/scheduler.rs) 固定：
`Input → LogicUpdate → SceneSync → Prepare → RenderCommit → FrameEnd`，每阶段独立命令缓冲，阶段末 flush。

注册点有两处，app 只按层次顺序调用：

| 阶段 | 场景侧（[`register.rs`](../../crates/scene/zela-scene-ecs/src/register.rs)） | 渲染侧（[`record_systems.rs`](../../crates/render/zela-render/src/record_systems.rs)） |
|---|---|---|
| Input | 时间推进 · 相机驱动 · 相机输入 | — |
| LogicUpdate | 时间控制 · 参数面板 | — |
| SceneSync | 绑定应用 | — |
| Prepare | 重播清理 · 合成器 · 父子变换累乘 · 坐标轴刷新 · 函数图刷新 | — |
| RenderCommit | — | 2D+3D 录制 · 内容纹理 · 形变 · 书写 |
| FrameEnd | 延迟命令应用 · 生命周期求差 | — |

- `debug` 下注册时检查同阶段 Resource 读写冲突并 `panic`（带系统名），执行时用借用追踪器核对实际借用；
  release 下两者完全消失，零开销。
- ⚠️ 声明的只是 **Resource** 读写集；**组件级访问既未声明也不可声明**（GAPS `AR-01`）。
  因此「某个 System 写了哪个组件」不能从类型签名读出来。

## 五、动画数据流

```
脚本（曲线 / 表达式）→ BindingTable（target = 数据化地址「组件.字段」+ 读写入口）
   → BindingApplySystem（SceneSync）每帧求值
   → Target::apply 写组件字段
   → RenderCommit 的录制 System 把场景读成绘制队列
   → 后端出帧；同时在 advance 之后算结构指纹
```

- 绑定系统只做「遍历 + 求值 + 派发」，**不认识任何具体目标** ⇒ 新增可动画字段不需要改 System。
- 应用顺序按段的起点升序 + 稳定排序；段前不写，段后照写（`Hold` 已钳到末值）。
- 谓词可分析时退化为**区间集判定**（纯函数，不逐帧求值）；不可分析才逐帧跑表达式。
- seek 口径是推导出来的：无记忆 ⇒ O(1)，有记忆 ⇒ `Replay`（弹簧 / 数值积分等，GAPS `AR-11` / `AH-07`）。

## 六、产物

| 产物 | 是什么 | 谁比对 |
|---|---|---|
| `.zsb` | 装配后的**纯数据**：Header + Payload（规范 JSON）+ Meta；内容 hash 只覆盖 Payload | `cargo verify --step demo`（manifest payload） |
| 像素哈希基线 | 逐作品逐后端的帧序列哈希 | `cargo verify --step hashes` |
| 结构指纹 | 逐帧的**纯数值状态**摘要（组件字段 + 世界矩阵） | `cargo verify --step hashes`（`structures.txt`） |
| 计时 / 决策 JSON 行 | 分段耗时、后端选择与拒绝理由、环境指纹 | 人工 / 落盘归档 |

## 七、本页不写的东西

- 动画系统的现状结论：专项治理未完成，见 [`animation.md`](../guides/animation.md) 占位说明。
- 缺口、未接线项：见 [`GAPS.md`](../../GAPS.md) 对应段（`AR` 段是场景侧组件与 API 的摸底记账）。
