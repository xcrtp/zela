# ze-alg

> **本文件的「事实（自动生成）」一节由 `cargo verify --step docs` 生成，勿手动编辑。**
> 手写内容放在 `## 职责` 及之后的章节：生成器只替换自动区，手写区逐字节保留。
> 导航与阅读顺序见 [`docs/README.md`](../README.md)。

<!-- AUTO:BEGIN -->
## 事实（自动生成）

- 层：`kernel`（`crates/kernel`）
- 路径：`crates/kernel/ze-alg`
- 生产依赖（上游，dev-deps 不计）：（无）
- 被依赖（下游）：`ze-scene-core`
- 源文件：1 个 · 669 行

### 文件清单

| 文件 | 行数 | `//!` 首行 |
|---|---|---|
| `src/lib.rs` | 669 | `ze-alg` — 抽象代数 trait 与**纯数值算法**的正当去处。 |

### 模块内关系（`use crate::…` / `use super::…`）

（未解析到文件级 `use` 边）

### 对外 API（模块级 `pub` 项；impl 方法见源码）

| 文件 | 项 |
|---|---|
| `src/lib.rs` | `pub trait Field: Copy + PartialEq + core::fmt::Debug` |
| `src/lib.rs` | `pub trait RealField: Field + PartialOrd` |
| `src/lib.rs` | `pub trait ComplexField: Field` |
| `src/lib.rs` | `pub struct Complex<F>` |
| `src/lib.rs` | `pub trait SquareOps<F: Field, const N: usize>` |
| `src/lib.rs` | `pub fn char_poly<F: RealField, const N: usize>(a: &[[F; N]; N]) -> [F; MAX_DIM + 1]` |
| `src/lib.rs` | `pub fn root_moduli<F: RealField>(coefficients: &[F], degree: usize) -> Vec<F>` |
| `src/lib.rs` | `pub fn spectral_radius<F: RealField, const N: usize>(a: &[[F; N]; N]) -> F` |
| `src/lib.rs` | `pub fn identity_slice(n: usize) -> Vec<Vec<f64>>` |
| `src/lib.rs` | `pub fn mat_mul_slice(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>>` |
| `src/lib.rs` | `pub fn mat_pow_slice(a: &[Vec<f64>], exponent: u64) -> Vec<Vec<f64>>` |
| `src/lib.rs` | `pub fn char_poly_slice(a: &[Vec<f64>]) -> Vec<f64>` |
| `src/lib.rs` | `pub fn spectral_radius_slice(a: &[Vec<f64>]) -> f64` |
| `src/lib.rs` | `pub fn affine_powered_matrix(a: &[Vec<f64>], b: &[f64], n: u64) -> Vec<Vec<f64>>` |
| `src/lib.rs` | `pub fn affine_apply_powered(powered: &[Vec<f64>], init: &[f64]) -> Vec<f64>` |
| `src/lib.rs` | `pub fn affine_apply(a: &[Vec<f64>], b: &[f64], init: &[f64], n: u64) -> Vec<f64>` |
| `src/lib.rs` | `pub fn affine_apply_many(` |
| `src/lib.rs` | `pub fn affine_replay(a: &[Vec<f64>], b: &[f64], init: &[f64], n: u64) -> Vec<f64>` |
| `src/lib.rs` | `pub fn affine_replay_within(` |
<!-- AUTO:END -->

## 职责

kernel 级的抽象代数层与纯数值算法库：`Field` / `RealField` / `ComplexField` 三组 trait 描述「数学对象如何计算」，`SquareOps` 给任意 `[[F; N]; N]` 提供乘 / 单位元 / 快速幂，并提供特征多项式（Faddeev–LeVerrier）、Durand–Kerner 求根与谱半径。当前唯一 `Field` 实现是 `f64`；同时暴露运行期宽度的 `*_slice` 入口，供 `ze-scene-core` 的仿射识别直接使用。

## 运行机制

矩阵不引入自定义类型：原生数组 `[[F; N]; N]` 就是矩阵，算法通过 trait 泛型化。唯一的 struct 是 `Complex<F>`——Durand–Kerner 必须在复数域上做，而它与 `[[F; N]; N]` 同类，是 `F` 上的**自由构造**，不是「描述数学对象」的容器。`char_poly` 返回 `[F; MAX_DIM + 1]`（`generic_const_exprs` 未稳定，`[F; N+1]` 编译不过）。所有矩阵乘法按**下标升序**累加，浮点结果逐位可复现；`*_slice` 与 const generic 两条入口共享同一算法与同一求和顺序。

## 边界与不做什么

- **零 `glam`**：与 M0 无关（M0 管渲染向量入口唯一，不管纯算法）。
- **零 `ze-notion` 依赖**：那边是 f32 + glam，用不上这里的 f64 数值分析（也不制造零消费者依赖）。
- 只抽当前有消费者的算法（矩阵幂 / 特征多项式 / 求根 / 谱半径 / 仿射递推）；**第二个 `Field` 实现出现时才扩展**，不预先泛化。
- 不做表达式识别、不做调度、不做持久化：`Expr` 相关的仿射识别留在 `ze-scene-core`。
- 精度口径：内部一律 f64；向 f32 的降级由消费方在边界做一次（见 `ze-scene-core::animation::affine` 模块文档）。
