# 加一个组件要改哪几处

> 结论先行：**9 处**，但只有一部分会在编译期或门禁上红，其余靠「有人记得」。
> 下面每处都标了漏掉后的真实后果（红 / 静默）。口径来自当前代码，不是计划稿。
> 计划期的「9 处 / 6 处不红」估计未逐项走完，GAPS `AR-13` 记录了本轮只实测了「加通道」这条路。

## 一句话

真正危险的不是「改得多」，是「漏了不红」：组件未登记会让对象**安静地少一个**，
通道写错文件会让覆盖门禁**静默漏检**。

## 九处

| # | 改哪 | 真实位置 | 漏了会怎样 |
|---|---|---|---|
| 1 | **存储组件定义** | `crates/scene/ze-scene-core/src/components.rs`（或 `object/primitives.rs`、`components/3d.rs`） | 编译红（类型不存在） |
| 2 | **组件登记表** | [`registry.rs`](../../crates/scene/ze-scene-core/src/registry.rs) 的 `known_component_table!` 的 `clone:` 段（类型需 `Clone`；要持久化还需 serde） | **静默**：`spawn_dyn` 返回 `None`，延迟创建路径**丢弃返回值** ⇒ 对象少一个、不报错；直到写产物时才炸 `UnregisteredComponent` |
| 3 | **通道声明**（要可动画才需要） | 组件类型上 `#[derive(Channels)]` + `#[channel(字段: 类型)]`；类型只支持 `f32` / `Vec2` / `Vec3` / `bool` | 写错类型 ⇒ 编译红；**忘了写 ⇒ 静默**（字段就是不可动画，没有任何东西报错） |
| 4 | **目标表** | [`target.rs`](../../crates/scene/ze-scene-core/src/animation/target.rs) 的 `TARGETS`：**逐项**登记 spec + 构造函数（stable Rust 不支持数组展开，派生通道也要手写这一项） | **静默**：该目标根本不存在（脚本绑不上、结构指纹也看不见这个字段）。`coverage` 只会报「登记了但没作品用」，**不会**报「没登记」 |
| 5 | **脚本侧 Pod 类型** | `crates/scene/ze-scene-api/src/types.rs`：`derive(PodType, SceneComponent)` + `#[pod_type(name = "...")]` | 编译红 |
| 6 | **脚本侧构造器（两处）** | [`api.rs`](../../crates/scene/ze-scene-api/src/api.rs) 的 `SceneApi` 字段；[`scene_service.rs`](../../crates/scene/ze-scene-api/src/scene_service.rs) 的 `register_component` + 构造闭包 | 漏 `api.rs` 字段 ⇒ 指纹**不变**（不红），脚本一调用就在装配期报「未知函数」；漏注册 ⇒ 能力协商时 provides 不足（装配期红） |
| 7 | **能力指纹与版本** | `api.rs` 的 `version` 声明 + 版本常量 + 指纹常量 | 红（`scene_api_fingerprint_is_pinned_to_version`） |
| 8 | **覆盖门禁** | `scripts/demo/manifest.txt` 里至少一个作品用到，或在 `demo/capability_exemptions.txt` 豁免并指向**真实存在的** GAPS 条目 | 红（`cargo verify --step coverage`）；过期豁免同样红 |
| 9 | **文档** | `docs/modules/<crate>.md` 手写区；GAPS 附录的 crate 能力表（新增 crate 才需要） | 结构门禁只校验「段存在」，**语义是否更新不红** |

## 三处隐性陷阱（不在 9 处里，但会咬人）

1. **`channel_files` 是手工清单**：`xtask` 的覆盖门禁只从**四个固定文件**里抽 `#[channel(...)]`。
   把通道写在这四个文件之外 ⇒ 该能力**静默漏检**（GAPS `AR-12` 记的就是这条）。
2. **`CHANNEL_HOMES`**：存储组件名与脚本组件名不同时（如 `Framing2D` → `Camera2D`），必须加一行；
   否则通道常量挂不到作者侧的构造器上，作者**看不到**这个通道（历史形态是静默跳过）。
3. **跨 crate 同名**：脚本侧类型与存储组件同名（`Circle` / `Gradient` …）必须在门禁白名单登记理由，
   漏了是红。这条是刻意的：一个概念只允许一处定义，重复必须留理由。

## 建议顺序

1. 存储组件 + （可选）通道声明；
2. 脚本侧 Pod 类型 → `SceneApi` 字段 → `register_component` + 闭包（三处一起改，避免中间态）；
3. 登记表 → `TARGETS`（若可动画）；
4. 指纹与版本（`signatures()` 变了就必须动）；
5. 作品 / 豁免 → 文档。

## 两条路径，别混

- **只给已有组件加一个可动画字段**：`#[channel]`（3）+ `TARGETS`（4）+ 覆盖（8）。若该字段要出现在脚本构造器上，第 5、6 处也要改；能力签名不变，指纹（7）不动。
- **加一个全新的脚本可见组件**：9 处全走。中间态很难编译通过，建议按「存储侧 → 脚本侧 → 登记表」的顺序一次改完。

## 最小可编译路径

只想先让它能跑起来，最少要动：存储组件（1）→ 脚本侧 Pod 类型（5）→ `SceneApi` 字段 + 注册（6）→ 登记表（2）。
少第 2 处时能编译，但对象**静默不存在**；少第 8 处则在 `coverage` 上红——「能跑」不等于「能交付」。

## 验证入口

```
cargo verify --step structure     # 登记表、白名单、指纹、文档段
cargo verify --step coverage      # 每个能力都有作品用到或显式豁免
cargo verify --step demo          # 作品 payload 与 manifest 一致
cargo verify --step docs          # 脚本 API 变了 ⇒ 声明文件重新生成并逐字节比对
```

`structure` 里的指纹校验只跑两条钉住用例：`signatures()` 变了而版本 / 指纹常量没同步就失败，
**不是警告**。

## 边界：这条路径的已知盲区

- 「加一个完整组件」本轮**没有走完**（GAPS `AR-13`），已实测的是「加一个可动画目标」：
  定义 1 处 + `TARGETS` 1 处，只撞 `coverage` 1 处门禁。
- 组件级读写访问**不可声明**（GAPS `AR-01`）⇒ 第 2、4 处的遗漏没有任何类型层面的兜底，
  只能靠产物序列化或画面现象发现。

## 相关

- 判据与错误模式：[设计原则](../principles.md)
- 场景侧组件的摸底与挂账：GAPS `AR` 段
- 每帧谁读谁写：[运行时](../architecture/runtime.md)
