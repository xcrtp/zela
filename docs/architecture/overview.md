# 系统总览

> **依赖图见 [`ARCHITECTURE.md`](../../ARCHITECTURE.md) 的自动区**（`cargo-workspace-analyzer` 产物）。
> 本篇只写三件事：分层、依赖方向、**合规校验在哪**。不复制依赖图——复制即第二真源。

## 一句话

`crates/` 按 10 层垂直切分，依赖只能自上而下；分层声明由 [`architecture.json`](../../architecture.json)
写成机器可读数据，与 `cargo metadata` **双向比对**，任何一边漏登记都红。

## 分层表（以 `architecture.json` 为准）

| 层 | 路径 | 前缀 | 一句话职责 |
|---|---|---|---|
| kernel | `crates/kernel/` | `ze-` | 纯计算 / 数据内核：类型、反射、图算法、几何、时间、组合子、渲染 IR |
| pod | `crates/ze/pod/` | `ze-` | 领域无关的 Pod 层：核心组（wasm 必须可编）+ 语言适配器（lua / py，不进 wasm gate） |
| graph | `crates/ze/graph/` | `ze-` | 图论 + 节点结构契约；`ze-node` 是门面（不含 UI） |
| schedule | `crates/schedule/` | `zela-` | 通用 ECS 调度框架：Resource / System / 六阶段 Scheduler / Telemetry，不含场景概念 |
| scene | `crates/scene/` | `ze-`·`zela-` | 场景核心（组件 / target 表 / 对象几何）+ Pod 能力层 + 派生宏 + ECS 契约与实现 + 产物 |
| render | `crates/render/` | `zela-` | 渲染抽象（trait / 队列 / 三角化）+ 具体后端（wgpu / 矢量） |
| ui | `crates/ui/` | `zela-` | egui 前端 + 编辑器状态机 |
| app | `crates/app/` | `zela-` | 应用组装层（唯一可执行入口）+ 内置作品 / 回放 / GUI 宿主 / 编译入口 |
| web | `crates/web/` | `zela-` | 浏览器（wasm）门面：只做装配与入口导出，不含业务 |
| zela | `crates/zela/` | `zela` | 应用层门面：真实 re-export `ze-pod` + `ze-node`（不是空壳） |

`ARCHITECTURE.md` 的自动区里还留着 `object` 与 `script` 两行历史痕迹：前者已并入 `scene`
（`ze-scene-core` 的 `object` 模块），后者已并入 `pod`。**以 `architecture.json` 为准**。

## 依赖方向

```
kernel → pod → graph → schedule → scene → render → ui → app
```

- 反向禁止：`ze-*` 不得依赖任何 `zela-*` / egui / wgpu；任何 crate 不得依赖 `zela-app`。
- `kernel` / `pod` 是底座：`render` / `ui` / `app` 也可以**直接**依赖它们（不必逐层穿透）。
- `schedule` 无内部依赖（只依赖 hecs），因此谁都能依赖它；`scene` 依赖它。
- `web` 是旁路门面：只依赖 `playback` / 渲染后端 / 产物格式，不被任何 crate 依赖。
- `zela` 是应用层门面，不属于原生命令链。
- workspace 内必须是 DAG（dev-dependencies 不计入生产边）。

## 合规校验在哪

### 1. `cargo verify --step structure`

一次跑完下列判据，任一失败即整体失败：

| 判据 | 拦什么 |
|---|---|
| crate 登记双向比对 | 新 crate 未登记在 GAPS 附录的 crate 能力表；表里写了仓库不存在的名字 |
| 跨 crate 同名 `pub struct/enum` 白名单 | 一个概念被定义两遍；新同名类型未经登记 |
| 图路径禁 `HashMap` / `HashSet` | 表达式图与拓扑路径上出现迭代序不确定的容器 |
| GAPS 小计重算 | 段内条目状态与小计不一致（手写数字腐烂） |
| 文档符号锚点 + `crates/` 链接存在性 | `crate::mod::Item` 形式的引用找不到定义；markdown 链接指向不存在的路径 |
| `architecture.json` ↔ `cargo metadata` 双向比对 | 层里少 crate / 多了不存在的 crate / crate 落进两层 |
| 模块文档覆盖 + 手写段 | `docs/modules/<crate>.md` 缺失或多余；手写段缺失、留占位 |
| 术语表 / 决策记录 | 术语无锚点；决策记录缺状态 / 锚点 / 三段正文 |
| 5 条 jq 判据（跑在 `cargo metadata` 输出上） | pod 层不含 `zela-*`；backends 不含 `zela-scene`；场景核心层不含 `zela-*` / hecs；`zela-render` 不含 `zela-scene-ecs`；未豁免处不得直接引 glam |
| 能力指纹 × 版本 | `signatures()` 变了而 `version` / 指纹常量没同步（两条钉住用例） |

> 判据实现见 [`xtask/src/main.rs`](../../xtask/src/main.rs) 的 `step_structure` 与其调用的各 `check_*`。
> 未安装 `jq` 时 jq 段会被**跳过并打印**（不静默通过），其余不依赖 jq 的判据照常跑。

### 2. `cargo verify --step docs`

用 `export_scene_api` **连续生成两次**声明文件到构建产物区并逐字节比对，再与已提交的模块文档
自动区比对。生成物不进仓库，因此不存在「手写声明忘更新」的腐烂路径。

### 3. 编译期

反向依赖的第一道闸是 Cargo 本身：`ze-*` 的 `Cargo.toml` 里写不出 `zela-*` 依赖，
写了就编译失败。`architecture.json` 的 `rules` 段逐条声明了「由什么保证」。

## 新增一个 crate 要同时改几处

1. 根 `Cargo.toml` 的 `[workspace].members`；
2. `architecture.json` 对应层的 `crates` 数组；
3. `docs/modules/<crate>.md`（手写段必填）；
4. GAPS 附录的 crate 能力表。

四处缺一即 structure 门禁失败——这是「登记表双向比对」存在的唯一理由。

## 相关

- 运行时（装配期 → 每帧 → 产物）：[runtime.md](runtime.md)
- 每个 crate 的事实与边界：[`modules/`](../modules/)
- 分层规则与依赖图：[`ARCHITECTURE.md`](../../ARCHITECTURE.md)
