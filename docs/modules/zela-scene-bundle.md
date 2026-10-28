# zela-scene-bundle

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`scene`（`crates/scene`）
- 路径：`crates/scene/zela-scene-bundle`
- 生产依赖（上游，dev-deps 不计）：`ze-render-ir`、`zela-scene-ecs`
- 被依赖（下游）：`zela-app`、`zela-compile`、`zela-playback`、`zela-web`
- 源文件：4 个 · 1534 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/frame_hash.rs` | 144 | — |
| `src/input_stream.rs` | 448 | **输入流**（D 批）：与 ZSB 分离的独立产物。 |
| `src/lib.rs` | 769 | — |
| `src/sha256.rs` | 173 | — |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/frame_hash.rs` | `pub struct FrameSequenceHash` |
| `src/input_stream.rs` | `pub enum InputError` |
| `src/input_stream.rs` | `pub struct LibraryId` |
| `src/input_stream.rs` | `pub struct Pairing` |
| `src/input_stream.rs` | `pub struct FrameEvents` |
| `src/input_stream.rs` | `pub struct InputStream` |
| `src/input_stream.rs` | `pub struct Recorder` |
| `src/lib.rs` | `pub struct Capability` |
| `src/lib.rs` | `pub struct Pod` |
| `src/lib.rs` | `pub struct Downgrade` |
| `src/lib.rs` | `pub struct Meta` |
| `src/lib.rs` | `pub struct MetaInput` |
| `src/lib.rs` | `pub struct Available` |
| `src/lib.rs` | `pub enum BundleError` |
| `src/lib.rs` | `pub struct RestoreError` |
| `src/lib.rs` | `pub struct Bundle` |
| `src/sha256.rs` | `pub fn sha256(data: &[u8]) -> [u8; 32]` |
| `src/sha256.rs` | `pub fn hex(bytes: &[u8]) -> String` |
| `src/sha256.rs` | `pub fn sha256_fast(data: &[u8]) -> [u8; 32]` |
| `src/sha256.rs` | `pub fn sha256_fast(data: &[u8]) -> [u8; 32]` |
<!-- AUTO:END -->

## 职责

ZSB（Zela Scene Bundle）产物的格式与编解码：定义「脚本 → 已装配场景」的规范产物，并持有帧序列哈希与独立输入流两个配套件。

## 运行机制

产物是 Header（24 字节：魔数 + 容器版本 + 段长）/ Payload（SceneScript 规范 JSON，内容 hash 只覆盖这段）/ Meta（能力与 Pod 版本、降级记录、源脚本 hash）三段；编码后会做一次「反序列化再序列化」的逐字节等价比对，NaN / Infinity 在这一步暴露为错误。回放端先按 Meta 做可还原性检查（逐项确认能力与 Pod 版本），再取 payload 还原场景 —— hash 对得上不代表能还原。输入流是另一份文件：按帧号稀疏记录事件（索引是帧号不是时间），并写下 zsb_payload_hash 与函子库身份，加载时不匹配即显式失败。自有最小 SHA-256 服务 payload hash，帧序列那种大体量哈希走快路径，frame_hash 把逐帧像素摘要累积成序列指纹。

## 边界与不做什么

- 不依赖任何语言适配器（mlua / pyo3），因此可以单独带进浏览器。
- 不做场景还原后的推进与出帧，也不执行脚本 —— 那是 zela-playback 与 zela-app 的事。
- 不记录用户输入：确定性只由「纯函数 + 无隐式状态」保证，输入另走 InputStream。
