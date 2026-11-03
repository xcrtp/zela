# ze-pod-lua

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`pod`（`crates/ze/pod`）
- 路径：`crates/ze/pod/ze-pod-lua`
- 生产依赖（上游，dev-deps 不计）：`ze-pod-binding`、`ze-pod-core`
- 被依赖（下游）：`ze-pod-tests`、`zela-app`、`zela-commands`
- 源文件：4 个 · 1374 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/binding.rs` | 525 | — |
| `src/export.rs` | 457 | — |
| `src/lib.rs` | 26 | — |
| `src/lua_pod.rs` | 366 | — |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/binding.rs` | `pub struct LuaHandle(pub ze_pod_core::Handle)` |
| `src/binding.rs` | `pub struct LuaPodValue(pub PodValue)` |
| `src/export.rs` | `pub fn lua_type_name(spec: &'static TypeSpec) -> &'static str` |
| `src/export.rs` | `pub fn lua_ls(capabilities: &[CapabilityDocs]) -> String` |
| `src/export.rs` | `pub fn export_docs(dir: &Path, capabilities: &[CapabilityDocs]) -> io::Result<[PathBuf; 2]>` |
| `src/lua_pod.rs` | `pub struct LuaPod` |
<!-- AUTO:END -->

## 职责

纯 Lua 适配器：把能力注册成 Lua 全局表与类型构造器，从宿主执行脚本，并导出 LuaLS 注解。

## 运行机制

装配期用语言无关的绑定计划把每个函数包成 Lua 闭包，参数与返回值经本 crate 的编解码实现转换，句柄与结构体值以 userdata 传递，时钟前缀的函数额外挂到 clock 表。脚本 Pod 通过注入器自述依赖并接收场景与内容两个服务，宿主经脚本运行入口传入源码，入口服务的重置与收尾函数负责交接产物；注解导出只读能力文档，不含领域知识。

## 边界与不做什么

- 不依赖任何 zela-*，不知道场景与内容的业务语义。
- 不做宿主侧装配（节点声明、Broker、注入），那属于应用层。
- 不提供每帧批量或跨 Pod 调用，只做装配期的一次性执行。
