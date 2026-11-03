# Contributing

## 约定摘要

- 分层单向依赖：`kernel → graph → render → ui → app`，反向禁止
- `ze-*`（kernel/graph）禁止依赖 egui / wgpu / 任何 `zela-*` crate
- `crates/ze/graph` 层禁止引用领域类型（FunctionDef, AST, AnimationClip 等）
- 新 crate 先判命名：与动画/渲染无关 → `ze-`，否则 `zela-`
- 新增 crate 必须同步更新 `architecture.json` 对应层的 `crates` 数组

## 加新 crate

1. 在 `crates/<layer>/` 下创建目录（按上表选层 + 前缀）
2. `cargo new --lib` 初始化
3. 根 `Cargo.toml` 的 `[workspace].members` 追加路径
4. 所有 `[dependencies]` 的 `path` 按跨层修正（同层 `../`，跨层 `../../layer/name`）
5. `architecture.json` 对应层的 `crates` 数组追加 crate 名
6. 建 `docs/modules/<crate>.md`：写 `## 职责` 与 `## 边界与不做什么`（源文件数 ≥ 15 时还要写
   `## 运行机制`），自动区由 `cargo run -p xtask -- docs-gen` 生成
7. 跑完整校验（见下）

## 校验命令

每次增删依赖后都要跑一遍：

```shell
# 1. 编译 + 类型检查（最基本，依赖方向错误会直接编译失败）
cargo check --workspace --all-targets

# 2. 依赖分析 + 循环检测（dev-deps 不计入生产边）
cargo-workspace-analyzer -d .

# 3. 完整测试
cargo test --workspace

# 4. clippy 零警告
cargo clippy --workspace --all-targets
```

> `cargo-archtest` TBD：当前版本 (v0.2.3) 为单 crate 内 `src/` 模块分层设计，
> 不适用于本 workspace 的 crate 级分层架构。待上游适配后接入。

## 文档引用

- 入门文档入口（模块索引 / 术语表 / 决策记录 / 指南）→ `docs/README.md`
- 架构总览 + 依赖图 + 合规校验 → `ARCHITECTURE.md`（自动生成区 + 手写设计说明）
- 分层声明 → `architecture.json`（workspace 根；`cargo verify --step structure` 做双向比对）
- 强制 CI → `.github/workflows/architecture.yml`

## 协作注意事项

> 以下 5 条来自本轮对话的教训（补充计划原写 7 条，实际只有 5 条；差额已在 GAPS `AT-03` 记账）。

1. 计划文档要实际写入文件，不能只写回复。
2. 数字要精确核实（出现过「37 条数成 34」的学费；引用数字前重算）。
3. 每阶段要可测试：不依赖渲染、不依赖场景、不重录基线。
4. 记账要诚实：写理由而不是含糊带过；「部分关闭」不写成「已关闭」。
5. 不确定就说不确定；采信实跑记录时标明「无法复现」的部分。

## 批次收尾检查表（人工纪律）

> 这四项没有自动化（见 GAPS PA-04 的诚实边界）：机器能查"路径存在 / 条目结构"，
> 查不了"本轮讨论过的名词都进账了吗"。每批收尾前逐项过一遍。

1. 专有名词追溯：把本轮对话 / 计划里出现过的专有名词逐个 `grep GAPS.md`；
   没有命中的要补条目，或显式写下"不需要记账"及理由。
2. 状态追溯：本轮改过代码后，`grep` 相关 GAPS 条目，确认描述没有变成过时的
   （V-01 的"67 MiB/帧"就是这么烂掉的），并确认没有"同一事实两个值"的自相矛盾。
3. 新增段要双登记：`check_gaps_subtotals` 的 `prefix` 列表 + `is_item` 白名单，
   缺一不可——只加段 ⇒ 条目不参与小计重算，门禁全绿地漏账（PA-05）。
4. 常量改值的文字同步：改常量时 `grep` 该值的文字出现（含 `//!` / `///`
   与其他 `.md`），确认没有第二处还写着旧值——这类不引用行号，只是"改了常量没改文字"
   （`vector3d.rs` 的 reflectance 0.5 → 0.04 就是这么漏的）。
5. 段级叙述同步（PH 新增，第 5 次同型）：新增 / 关闭条目后，要检查该段
   段头叙述、其他段的引用、ARCHITECTURE 对应节。`baselines.txt` 的
   `[vector]` 段头连续 5 次与条目不符（条目改了、段头没改），说明只检查"条目"不够。

> 已知四次漏账（三条隐式默认值、B18 批次二、锚点作用域、Render Attachment）
> 都至少能被其中一项抓住一次。见 GAPS PA-04。

## 提交规范

Conventional Commits：`<type>(<scope>): <subject>`。type 用 `feat` / `fix` / `refactor` / `docs` / `test` / `chore`。
