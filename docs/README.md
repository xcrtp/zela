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

## 规范性陈述 ↔ 代码断言（双向对照）

CJ 起的纪律：**每条规范性陈述必须能指到代码断言或 GAPS 条目；每条护栏必须在导航文档里有入口。**
下表是这张双向对照表本身——左列是文档里的说法，右列是可执行的落点（反向查找也用这一张）。

| 规范性陈述 | 代码 / 测试 / GAPS 落点 |
|---|---|
| 确定性三层口径（同目标可复现 / 跨端结构一致 / 跨端逐位相同不承诺） | [`guides/determinism.md`](guides/determinism.md) §5；`GAPS.md` 头部 |
| 结构指纹管「算得对不对」，像素哈希管「画得一样不一样」 | [`guides/determinism.md`](guides/determinism.md) §6；`cargo verify --step hashes`（`xtask/src/main.rs`） |
| `Frame` 是纯值、三条平行结构、不进 payload | `ze_interact::Frame`（`GAPS` CG-03）；`crates/kernel/ze-interact` |
| 三条反面约束（不做全链路埋点 / tag 不进 `SceneEvent` / 不靠 System 手工上报） | `zela_app::debug` 模块文档；`GAPS` CH-03 |
| 元规则 A（零消费者只约束实现、不约束形状）与 B（正确性优先于兼容性） | [`principles.md`](principles.md) §5；`GAPS` CI-04 / CI-07 |
| 六个未来口子的形状（源种类 / 标记 / 路径来源 / 策略 trait / 输入锚点 / 诊断内容） | `GAPS` CI-04；`docs/modules/` 对应 crate |
| `O1 / OPerInterval / OLogN / EventReplay` 已实现；`Replay` 是 AI-07 红线 | `animation::BindingTable::reject_replay`（`crates/scene/ze-scene-core/src/animation/binding.rs`）；`GAPS` BY-04 / CH |
| 输入事件只属于一帧（`Advance` 后清理，录制关闭也不残留） | `SceneFrame::begin`（`crates/ui/zela-egui/src/frame.rs`）；`GAPS` CH-05；提交 `dc8500d` |
| 录制关闭 + RPC 注入时 `FrameTag` 不残留 | `SceneFrame::begin` 帧末清理（同文件）；`GAPS` CJ-05（B3） |

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
