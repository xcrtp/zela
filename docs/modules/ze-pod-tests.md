# ze-pod-tests

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`pod`（`crates/ze/pod`）
- 路径：`crates/ze/pod/ze-pod-tests`
- 生产依赖（上游，dev-deps 不计）：`ze-pod-binding`、`ze-pod-core`、`ze-pod-js`、`ze-pod-lua`、`ze-pod-macro`、`ze-pod-py`
- 被依赖（下游）：（无）
- 源文件：1 个 · 773 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 773 | Pod 层集成测试与基准的**共用设施**（mini pod，领域无关）。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/lib.rs` | `pub enum ClockCmd` |
| `src/lib.rs` | `pub enum ClockEvent` |
| `src/lib.rs` | `pub struct ClockPod` |
| `src/lib.rs` | `pub struct LogPod` |
| `src/lib.rs` | `pub struct StatsPod` |
| `src/lib.rs` | `pub struct ControllerPod` |
| `src/lib.rs` | `pub struct Dot` |
| `src/lib.rs` | `pub enum Kind` |
| `src/lib.rs` | `pub struct MiniApi<'a>` |
| `src/lib.rs` | `pub struct MiniOutput` |
| `src/lib.rs` | `pub struct MiniRecorderPod` |
| `src/lib.rs` | `pub enum CounterCmd` |
| `src/lib.rs` | `pub struct CounterPod` |
| `src/lib.rs` | `pub struct MathApi<'a>` |
| `src/lib.rs` | `pub fn math_registry() -> RegistryPod` |
<!-- AUTO:END -->

## 职责

Pod 层测试与基准的共用设施：一组领域无关的 mini Pod、录制器与命令 / 事件类型。

## 运行机制

时钟 Pod 提供命令队列与事件广播，日志与统计 Pod 作为事件接收方验证扇出，控制器 Pod 通过注入器消费命令；录制器带重置与收尾函数，把建对象、发值与记类型累积成输出供断言。命令只能提交进对应命令类型的队列、事件只能发布到对应事件类型的广播，两条误用路径各有编译失败用例守住；数学能力由注册表 Pod 提供，供基准对照 typed 与擦除两条路径。

## 边界与不做什么

- 只服务测试与基准：不发布，也不被任何生产 crate 依赖。
- 不引入任何 zela-* 或真实领域能力。
- 不提供生产可用的 Pod 实现，也不做性能承诺。
