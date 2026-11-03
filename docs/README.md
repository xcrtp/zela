# docs · 入口

> 本目录是**入门文档**：稳定知识放这里，缺口 / 未接线项一律指向 [`GAPS.md`](../GAPS.md)。
> 会腐烂的内容（文件清单、依赖、行数、API 签名）由机器生成；不会腐烂的内容（职责、边界、
> 决策理由）由人手写。**自动区勿手改**——见每篇模块文档开头的声明。

## 按角色阅读

README 的阅读导览只指向这里 —— 同一份定义，不复制第二份。

| 角色 | 先看 |
|---|---|
| 想做动画的 | 脚本怎么写（[`guides/animation.md`](guides/animation.md)）、有哪些能力（`zela api` 生成的补全声明）、`expr.bind` 怎么用 |
| 想改引擎的 | [加一个组件要改哪几处](guides/adding-a-component.md)、[门禁有哪些](principles.md) |
| 想接后端的 | [后端契约与路由](guides/render-backends.md)、`feature_bits`、路由口径 |
| 想复现论文的 | [确定性四层口径](guides/determinism.md)、基线怎么重录 |

## 阅读顺序

1. [设计原则与判据清单](principles.md) —— 先看判据，再看结论
2. [系统总览](architecture/overview.md) —— 分层 / 依赖方向 / 合规校验
3. [运行时](architecture/runtime.md) —— 装配期 → 每帧循环 → 产物
4. 需要动手时：[加一个组件要改哪几处](guides/adding-a-component.md)
5. 查词：[术语表](glossary.md)（每条都挂着代码符号或 GAPS 编号）

## 目录

| 位置 | 内容 | 性质 |
|---|---|---|
| [`principles.md`](principles.md) | 设计原则 + 判据清单 + 反复出现的错误模式 | 手写 |
| [`architecture/overview.md`](architecture/overview.md) | 分层 / 依赖图 / 合规校验（与 `ARCHITECTURE.md` 同源，**引用不复制**） | 手写 + 指针 |
| [`architecture/runtime.md`](architecture/runtime.md) | 帧循环、命令-事件、调度流水线、动画数据流 | 手写 |
| [`modules/`](modules/) | 每个 crate 一篇：自动区（事实）+ 手写区（职责 / 运行机制 / 边界） | 自动 + 手写 |
| [`guides/animation.md`](guides/animation.md) | 动画系统（跨 6 个 crate） | 手写（占位，等专项治理结论） |
| [`guides/determinism.md`](guides/determinism.md) | 确定性四层口径 | 手写 |
| [`guides/render-backends.md`](guides/render-backends.md) | 后端选型与路由 | 手写 |
| [`guides/adding-a-component.md`](guides/adding-a-component.md) | 「加一个组件要改 9 处」的操作指南 | 手写 |
| [`glossary.md`](glossary.md) | 术语 hub（网状入口，不是字典） | 手写（每条必带锚点） |
| [`decisions/`](decisions/) | 决策记录（选了什么 + 为什么 + 什么情况下不成立） | 手写（状态段可校验） |

## 未落地项 / 下一阶段去哪看

这里不维护清单（复制 = 第二个来源 = 必然腐烂）：

- 缺口、未接线、已声明的债 → [`GAPS.md`](../GAPS.md)（按批次分段，小计由 `check_gaps_subtotals` 机器核对）
- 架构规则与依赖图 → [`ARCHITECTURE.md`](../ARCHITECTURE.md)（自动区由 `cargo-workspace-analyzer` 生成）
- 分层声明（机器可校验）→ [`architecture.json`](../architecture.json)
- 验证入口 → 仓库根 `cargo verify`（`--step structure` 含文档门禁）

## 维护约定

- 改代码后刷新模块文档自动区：`cargo run -p xtask -- docs-gen`，然后提交
- 漂移会被 `cargo verify --step docs` 拦下（生成两次逐字节比对 + 与已提交内容比对）
- 新增 crate 必须建 `docs/modules/<crate>.md`，并在 `architecture.json` 登记（两处缺一即 fail）
- 术语表每条必须有 `- **代码锚点**：` 或 `- **锚点**：`；决策记录必须有状态 / 锚点 / 三段正文
- 文档用语：禁止词表定义在 `xtask/src/lib.rs` 的 `FORBIDDEN_TERMS`；单跑检查
  `cargo run -p xtask -- terms`（也挂在 `--step docs`）。确有合理场合时在
  `FORBIDDEN_TERM_ALLOW` 按「词 + 文件 + 理由」登记
- 禁止词表不限于清单：每批收尾按字根（钉 / 锚 / 锁 / 真）扫一遍，近义词同样改写（GAPS `BA-03`）
- **用语门禁的扫描范围**（唯一实现在 `xtask/src/main.rs` 的 `repository_markdown_docs`）：
  根目录 `*.md` + `docs/**/*.md` + `crates/**/README.md`。改动这个范围时必须同步本段文字，
  否则范围又回到"只有代码知道"。唯一排除项是 `WASM_PORT_SURVEY.md`：它是历史探测快照，
  路径漂移本来就是它保留的信息，文件头有一行可见声明说明这一点；新增排除项同样要写在这里
