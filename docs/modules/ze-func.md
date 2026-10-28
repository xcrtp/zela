# ze-func

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`kernel`（`crates/kernel`）
- 路径：`crates/kernel/ze-func`
- 生产依赖（上游，dev-deps 不计）：`ze-comb`、`ze-reflect`、`ze-type`
- 被依赖（下游）：`ze-pod-func`、`ze-scene-core`、`zela-scene-ecs`
- 源文件：7 个 · 2330 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/layout.rs` | 102 | **Layout / Codec 的门面**：实现已迁至 `ze-type`（纯代数）与 `ze-reflect`（运行时注册表）。 |
| `src/lib.rs` | 41 | ze-func：**基础函子库**（namespace `zela.math`）。 |
| `src/mono.rs` | 347 | **单态解析器**：把 `(算子, 实参槽类型)` 解析成具体类型的函数指针。 |
| `src/op.rs` | 479 | **算子目录**：签名 + 语义元数据，**唯一一张表**。 |
| `src/ops.rs` | 762 | `zela.math` 的**标量算子语义**（纯函数，无状态）。 |
| `src/simplify.rs` | 328 | **符号化简**（阶段 4）：常量折叠 + **层间迁移**（离散层 → 连续层）。 |
| `src/wgsl.rs` | 271 | **WGSL codegen 后端**（阶段 4-A）：把 `zela.math` 的算子渲染成 WGSL 表达式。 |

### 模块内关系（`use crate::…` / `use super::…`）

```mermaid
graph LR
    f1["src/ops.rs"]
    f0["src/simplify.rs"]
    f0 --> f1
```

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/layout.rs` | `pub fn encode_path(points: &[[f32; 2]], closed: bool) -> ValueBytes` |
| `src/layout.rs` | `pub fn negotiate_bytes(` |
| `src/mono.rs` | `pub struct MathMonomorphizer` |
| `src/mono.rs` | `pub fn monomorphizer() -> &'static MathMonomorphizer` |
| `src/op.rs` | `pub fn op_def(name: &str) -> Option<&'static OpDef>` |
| `src/op.rs` | `pub fn signature_of(name: &str) -> Option<&'static OpDef>` |
| `src/op.rs` | `pub fn op_revision(name: &str) -> Option<(u32, u32)>` |
| `src/op.rs` | `pub fn is_pointwise(name: &str) -> Option<bool>` |
| `src/op.rs` | `pub fn targets(name: &str) -> Option<&'static [Target]>` |
| `src/op.rs` | `pub fn qualified(def: &OpDef) -> String` |
| `src/ops.rs` | `pub fn is_known(op: &OpRef) -> bool` |
| `src/ops.rs` | `pub fn eval(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn sin(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn cos(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn add(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn mul(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn scale(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn sum(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn mix(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn clamp(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn vec2(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn vec3(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn x(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn y(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn z(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn rotate3(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn splat(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn vec3_from_vec2(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn vec2_from_vec3(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn length(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn normalize(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn round_f32(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn lt(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn gt(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn le(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn ge(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn eq(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn ne(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn and(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn or(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/ops.rs` | `pub fn not(op: &OpRef, args: &[Value]) -> Result<Value, EvalError>` |
| `src/simplify.rs` | `pub fn simplify(expr: &Expr) -> Expr` |
| `src/simplify.rs` | `pub struct Simplify` |
| `src/simplify.rs` | `pub fn register_passes(registry: &mut PassRegistry)` |
| `src/wgsl.rs` | `pub struct MathWgsl` |
| `src/wgsl.rs` | `pub fn backend() -> &'static MathWgsl` |
<!-- AUTO:END -->

## 职责

`ze-func` 是**基础函子库**（namespace `zela.math`），算子的**身份与语义同源**的地方。它持有唯一一张算子目录 `OP_DEFS`（签名 + `since` / `changed` 版本 + `pointwise` + `targets`）与纯函数实现 `eval`，不持有任何求值状态。

## 运行机制

`ze-comb` 的算子调用节点进来后：`src/ops.rs` 的 `is_known` 先校验 namespace 与符号（首次查询解析进 `ze-reflect` 的符号表，之后是整数槽位，查表 O(1)）→ `eval` 按算子引用分派到标量 / 向量纯函数，逐位结果与运行期一致。`src/simplify.rs` 是语义保持的 transform pass：自底向上折叠 —— 实参全是字面量的 `Call` 直接用**同一份** `eval` 求值；闭合的 `NatRec`（不含通道 / 组件读 / `Ref`）在装配期展开成常量，从离散层降为连续层，`SIMPLIFY_REC_LIMIT` 是成本护栏，不闭合的原样保留。`src/layout.rs` 只是门面：布局代数在 `ze-type`，运行时注册表在 `ze-reflect`。

内部文件分工：`op.rs` 是目录（唯一真源），`ops.rs` 是算子语义与 `eval`，`simplify.rs` 是符号化简，`layout.rs` 是 Layout / Codec 门面，`lib.rs` 是对外 re-export 面。

## 边界与不做什么

- **不引入 pod 机制、不依赖任何宿主 crate**：只依赖 kernel 内的 `ze-comb` / `ze-type` / `ze-reflect`；表达式内核因此能在没有 pod / 场景 / 渲染时独立测试。
- **不做三语言接线与装配期类型推导**：那是 `ze-pod-func`（capability + `infer` + revision 校验）。
- **不持有布局 / Codec 的真源**：路径等私有类型只以 `Value` 的字节块 + 命名类型流经；布局代数在 `ze-type`，运行时注册表在 `ze-reflect`。
- **不做递归控制与求值调度**：那是 `ze-comb` 的 `CompiledExpr` 与宿主 `Mono`。
- **不做私有类型算子**：当前算子只覆盖标量 / 向量，且全部声明为 pointwise（离散层由 `NatRec` 承担）。
