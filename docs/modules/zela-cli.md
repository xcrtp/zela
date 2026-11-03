# zela-cli

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`app`（`crates/app`）
- 路径：`crates/app/zela-cli`
- 生产依赖（上游，dev-deps 不计）：`zela-app`、`zela-commands`、`zela-gui`、`zela-render-backends`、`zela-scene-bundle`
- 被依赖（下游）：（无）
- 源文件：2 个 · 596 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 580 | `zela` 伞命令的宿主实现（clap 适配 + CLI 帧 sink）。 |
| `src/main.rs` | 16 | `zela` 可执行入口：宿主实现全在 `zela_cli`（lib），这里只做退出码映射。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/lib.rs` | `pub fn spec_to_clap(spec: &CommandSpec) -> clap::Command` |
| `src/lib.rs` | `pub fn collect_args(` |
| `src/lib.rs` | `pub struct CliHost` |
| `src/lib.rs` | `pub struct CliSinkFactory` |
| `src/lib.rs` | `pub fn root() -> clap::Command` |
| `src/lib.rs` | `pub fn dispatch(raw: Vec<String>) -> Result<Response, String>` |
| `src/lib.rs` | `pub fn format_human(response: &Response) -> String` |
| `src/lib.rs` | `pub fn matches_for(raw: &[&str]) -> Result<clap::ArgMatches, String>` |
<!-- AUTO:END -->

## 职责

`zela` 伞命令的**宿主实现**（package 名 `zela-cli`，bin 名 `zela`；`zela` 包名已被
`crates/zela/zela` 占用）。

- `CommandSpec → clap::Command`：参数声明只在 `zela-commands` 一处，CLI 不写第二份；
- `CommandArgs`：clap 匹配结果经 `from_pairs` 落到同一份类型化参数（默认值与手写前端一致）；
- 终端宿主：进度（1% 节流）/ 诊断（级别前缀）；
- 帧 sink：PNG 序列、mp4（ffmpeg 管道）、逐帧序列哈希。

## 边界与不做什么

- 不实现命令逻辑：命令帧体在 `zela-commands`；
- 不做 GUI：GUI 宿主在 `zela-gui` / `zela-egui`（`gui` / `play` 只转发 `launch`）；
- 已接入：`render` / `frames` / `compile` / `check` / `inspect` / `api` /
  `fingerprint` / `doctor` / `fonts` / `list` / `play` / `gui`；
  `dev verify` / `dev parity` 仍是 `zela-app` 的内部 bin（GAPS AY-08）。

## 运行机制

`zela render <script> --out x.mp4`：clap 解析 → `collect_args`（类型 / 默认值由 spec 决定）
→ `RenderOptions::from_args`（交叉校验）→ `render::run(CliHost, CliSinkFactory, options)`
→ `run_frames` → 每帧写 sink → `Response` → `format_human` 打印。
