# ze-pod-macro

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`pod`（`crates/ze/pod`）
- 路径：`crates/ze/pod/ze-pod-macro`
- 生产依赖（上游，dev-deps 不计）：（无）
- 被依赖（下游）：`ze-pod`、`ze-pod-func`、`ze-pod-py`、`ze-pod-tests`、`ze-scene-api`、`zela-scene-ecs`
- 源文件：1 个 · 962 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 962 | `#[capability]`：把一个"函数集合" struct 变成运行时协商出的 typed 句柄。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/lib.rs` | `pub fn capability(attr: TokenStream, item: TokenStream) -> TokenStream` |
| `src/lib.rs` | `pub fn derive_pod_type(input: TokenStream) -> TokenStream` |
<!-- AUTO:END -->

## 职责

Pod 层的过程宏：把「函数集合」struct 变成运行时协商出的 typed 句柄，并从类型定义生成可跨边界的类型描述。

## 运行机制

能力属性宏解析命名空间、版本、描述与绑定开关，剥掉每个字段的首个服务形参，为字段生成裸函数指针与同名方法（调用即一次间接寻址），再实现签名声明与「从协商结果构造」入口；类型名缺省取类型自身的可命名常量，未实现即编译错误，可用类型属性按参数或返回值覆盖。类型派生宏为具名结构体与枚举生成类型规格与双向值转换，不支持泛型与元组风格字段。

## 边界与不做什么

- 不链接任何语言运行时，也不生成语言绑定本身，那交给绑定驱动。
- 不校验命名空间是否已被占用，协商在运行时做。
- 不做类型推断：值转换只按声明中的字段类型逐项生成。
