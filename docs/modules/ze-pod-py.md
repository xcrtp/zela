# ze-pod-py

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`pod`（`crates/ze/pod`）
- 路径：`crates/ze/pod/ze-pod-py`
- 生产依赖（上游，dev-deps 不计）：`ze-pod-binding`、`ze-pod-core`、`ze-pod-macro`
- 被依赖（下游）：`ze-pod-tests`、`zela-app`、`zela-commands`
- 源文件：3 个 · 1926 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/export.rs` | 794 | — |
| `src/imp.rs` | 1039 | — |
| `src/lib.rs` | 93 | — |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/export.rs` | `pub fn pyi_files(capabilities: &[CapabilityDocs]) -> BTreeMap<String, String>` |
| `src/export.rs` | `pub fn export_docs(dir: &Path, capabilities: &[CapabilityDocs]) -> io::Result<Vec<PathBuf>>` |
| `src/imp.rs` | `pub struct PyPodValue` |
| `src/imp.rs` | `pub struct PyHandle` |
| `src/imp.rs` | `pub struct PyPod` |
| `src/imp.rs` | `pub fn registered_python_names(pod: &PyPod) -> Vec<String>` |
| `src/lib.rs` | `pub struct FloatList(pub Vec<f32>)` |
| `src/lib.rs` | `pub struct PythonApi<'a>` |
<!-- AUTO:END -->

## 职责

Python 适配器（pyo3，python feature 可选）：提供装配期批量能力与宿主到 Python 脚本的入口。

## 运行机制

能力声明只包含在区间上取点与在 Python 侧建对象两个函数：前者真实执行 Python 代码返回批量数据，后者经注入的场景能力在宿主里建对象而不绕过句柄检查。实现层用 Python 的 C 函数闭包直接捕获同步服务引用，因此提供方必须登记同步徽章，未登记返回明确错误；脚本经运行入口执行，入口服务的重置与收尾函数取回产物，导出层按命名空间生成 Python 类型存根。

## 边界与不做什么

- 不依赖任何 zela-*，领域能力由应用层注入。
- pyo3 必须留在可选 feature 之后，否则与 vendored Lua 的零运行时依赖目标冲突。
- 不做每帧函数求值与跨 Pod 批量调用，只覆盖装配期。
