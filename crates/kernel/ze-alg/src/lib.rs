//! `ze-alg` — 抽象代数 trait 与**纯数值算法**的正当去处。
//!
//! # 设计原则：trait 描述"如何计算"，不描述数据结构
//!
//! 本 crate 只回答「一个数学对象**怎样计算**」，不引入"矩阵类型 / 多项式类型"这类
//! 数据容器。矩阵就是原生数组 `[[F; N]; N]`（`F` 上的自由构造），算法写成 trait +
//! 泛型实现：新增一个 `Field` 实现即可复用全部算法。
//!
//! # 唯一的 struct：`Complex<F>`
//!
//! Durand–Kerner 求根**必须在复数域上做**（实域没有闭包）。`Complex<F>` 与
//! `[[F; N]; N]` 同类，是 **`F` 上的自由构造**，不是"描述数学对象"的容器 ——
//! 看到 struct 不等于破了上面那条戒。
//!
//! 不接受把求根也 trait 化（`trait RootFinder` 由实现自行提升数域）：那只是把
//! struct 挪进实现里，复杂度更高。
//!
//! # 边界
//!
//! - **零 `glam`**：与 M0（"渲染向量入口唯一"）无关；M0 管渲染，不管纯算法。
//! - **零 `ze-notion` 依赖**：那边是 f32 + glam，用不上 f64 数值分析。
//! - 只抽**当前有消费者**的算法（`affine.rs` 的仿射识别 / 谱半径 / 快速幂）。
//!   第二个实现出现时才扩展 trait，不预先泛化。
//!
//! # const generics 的两点口径（实测约束）
//!
//! - `[F; N + 1]` 需要 `generic_const_exprs`（未稳定）⇒ 特征多项式统一返回
//!   字面常量上界 `[F; MAX_DIM + 1]`；
//! - 当前消费者 `affine.rs` 的宽度是**运行期**决定的（`Vec<Vec<f64>>`）⇒
//!   同时提供 `*_slice` 运行期入口，const generic 版本留给第二个实现出现时。
//!
//! # f64 / f32 口径（CB-c）
//!
//! 本 crate 的算法泛型于 [`Field`] / [`RealField`]，当前唯一实现是 `f64`。
//! 场景侧的 `State` **内部一律 f64**；向 `f32` 的降级只在构造 `EvalValue` 的边界
//! 发生**一次**，不得在中间步骤反复降级（见 `ze-scene-core::animation::affine`
//! 的模块文档与 `state_demotes_to_f32_only_at_the_eval_boundary` 用例）。

#![forbid(unsafe_code)]

/// 支持的最大方阵宽度（特征多项式的数组上界 `MAX_DIM + 1`）。
pub const MAX_DIM: usize = 4;

// ---------------------------------------------------------------------------
// 代数 trait
// ---------------------------------------------------------------------------

/// 域：加 / 减 / 乘 / 除 / 零 / 一。
///
/// 只描述运算，不描述表示。`Copy` 是算法（如矩阵快速幂）所需的实际约束，
/// 不是数学定义的一部分。
pub trait Field: Copy + PartialEq + core::fmt::Debug {
    /// 加法单位元。
    const ZERO: Self;
    /// 乘法单位元。
    const ONE: Self;

    fn add(self, rhs: Self) -> Self;
    fn sub(self, rhs: Self) -> Self;
    fn mul(self, rhs: Self) -> Self;
    fn div(self, rhs: Self) -> Self;
}

/// 实域：在 [`Field`] 之上补开方 / 绝对值 / 全序比较（谱半径需要）。
pub trait RealField: Field + PartialOrd {
    /// 非负平方根。
    fn sqrt(self) -> Self;
    /// 绝对值。
    fn abs(self) -> Self;
    /// `sqrt(a² + b²)`（与 `f64::hypot` 同口径：避免中间溢出）。
    fn hypot(self, other: Self) -> Self;
    /// 从整数构造（Faddeev–LeVerrier 里的 `/ s`）。
    fn from_usize(value: usize) -> Self;
    /// 从 `f64` 常量构造（收敛阈值 / 闭式系数）。
    fn from_f64(value: f64) -> Self;
}

/// 复数域：关联出实部域，并提供提升 / 模长 / 开方。
pub trait ComplexField: Field {
    /// 实部所在的域。
    type Real: RealField;

    /// 从实部提升（虚部为零）。
    fn from_real(re: Self::Real) -> Self;
    /// 直接从实部 / 虚部构造（自由构造，与 [`from_real`](Self::from_real) 同类）。
    fn from_parts(re: Self::Real, im: Self::Real) -> Self;
    /// 复数平方根（主值）。
    fn sqrt(self) -> Self;
    /// 模长 `|z|`。
    fn abs(self) -> Self::Real;
}

// ---------------------------------------------------------------------------
// 唯一的 struct
// ---------------------------------------------------------------------------

/// 极简复数：`F` 上的自由构造（见 crate 文档）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Complex<F> {
    pub re: F,
    pub im: F,
}

impl<F> Complex<F> {
    /// 直接构造。
    pub const fn new(re: F, im: F) -> Self {
        Self { re, im }
    }
}

impl<F: RealField> Field for Complex<F> {
    const ZERO: Self = Self::new(F::ZERO, F::ZERO);
    const ONE: Self = Self::new(F::ONE, F::ZERO);

    fn add(self, rhs: Self) -> Self {
        Self::new(self.re.add(rhs.re), self.im.add(rhs.im))
    }

    fn sub(self, rhs: Self) -> Self {
        Self::new(self.re.sub(rhs.re), self.im.sub(rhs.im))
    }

    fn mul(self, rhs: Self) -> Self {
        Self::new(
            self.re.mul(rhs.re).sub(self.im.mul(rhs.im)),
            self.re.mul(rhs.im).add(self.im.mul(rhs.re)),
        )
    }

    fn div(self, rhs: Self) -> Self {
        let denom = rhs.re.mul(rhs.re).add(rhs.im.mul(rhs.im));
        Self::new(
            self.re.mul(rhs.re).add(self.im.mul(rhs.im)).div(denom),
            self.im.mul(rhs.re).sub(self.re.mul(rhs.im)).div(denom),
        )
    }
}

impl<F: RealField> ComplexField for Complex<F> {
    type Real = F;

    fn from_real(re: Self::Real) -> Self {
        Self::new(re, F::ZERO)
    }

    fn from_parts(re: Self::Real, im: Self::Real) -> Self {
        Self::new(re, im)
    }

    fn sqrt(self) -> Self {
        if self.re == F::ZERO && self.im == F::ZERO {
            return Self::ZERO;
        }
        let modulus = self.abs();
        let two = F::from_f64(2.0);
        let re = modulus.add(self.re).div(two).sqrt();
        let im = modulus.sub(self.re).div(two).sqrt();
        if self.im < F::ZERO {
            Self::new(re, F::ZERO.sub(im))
        } else {
            Self::new(re, im)
        }
    }

    fn abs(self) -> F {
        // 与旧实现同口径：`hypot`（不是 sqrt(re²+im²)，末位舍入不同）。
        self.re.hypot(self.im)
    }
}

impl Field for f64 {
    const ZERO: Self = 0.0;
    const ONE: Self = 1.0;

    fn add(self, rhs: Self) -> Self {
        self + rhs
    }

    fn sub(self, rhs: Self) -> Self {
        self - rhs
    }

    fn mul(self, rhs: Self) -> Self {
        self * rhs
    }

    fn div(self, rhs: Self) -> Self {
        self / rhs
    }
}

impl RealField for f64 {
    fn sqrt(self) -> Self {
        f64::sqrt(self)
    }

    fn abs(self) -> Self {
        f64::abs(self)
    }

    fn hypot(self, other: Self) -> Self {
        f64::hypot(self, other)
    }

    fn from_usize(value: usize) -> Self {
        value as f64
    }

    fn from_f64(value: f64) -> Self {
        value
    }
}

// ---------------------------------------------------------------------------
// 方阵运算（const generic）
// ---------------------------------------------------------------------------

/// 方阵的乘 / 单位元 / 快速幂。
///
/// **求和顺序固定为升序**（与旧 `affine.rs` 一致）⇒ 浮点结果逐位可复现；
/// 不得改成并行 / 树形归约。
pub trait SquareOps<F: Field, const N: usize> {
    fn mul(&self, rhs: &Self) -> Self;
    fn identity() -> Self;
    fn pow(&self, exponent: u64) -> Self;
}

impl<F: Field, const N: usize> SquareOps<F, N> for [[F; N]; N] {
    fn mul(&self, rhs: &Self) -> Self {
        let mut out = [[F::ZERO; N]; N];
        for (i, row) in out.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                let mut acc = F::ZERO;
                for t in 0..N {
                    acc = acc.add(self[i][t].mul(rhs[t][j]));
                }
                *cell = acc;
            }
        }
        out
    }

    fn identity() -> Self {
        let mut out = [[F::ZERO; N]; N];
        for (i, row) in out.iter_mut().enumerate() {
            row[i] = F::ONE;
        }
        out
    }

    fn pow(&self, exponent: u64) -> Self {
        let mut acc = Self::identity();
        let mut base = *self;
        let mut exp = exponent;
        while exp > 0 {
            if exp & 1 == 1 {
                acc = acc.mul(&base);
            }
            exp >>= 1;
            if exp > 0 {
                base = base.mul(&base);
            }
        }
        acc
    }
}

/// 特征多项式 `λ^n + c[1]·λ^(n-1) + … + c[n]` 的系数（`c[0] = 1`）。
///
/// Faddeev–LeVerrier：`c_s = −tr(A·M_s) / s`，`M_s = A·M_(s−1) + c_(s−1)·I`，
/// `M_1 = I`。`n ≤ MAX_DIM`（`[F; N+1]` 未稳定 ⇒ 用字面上界）。
pub fn char_poly<F: RealField, const N: usize>(a: &[[F; N]; N]) -> [F; MAX_DIM + 1] {
    debug_assert!(N <= MAX_DIM, "N 不得超过 MAX_DIM");
    let mut c = [F::ZERO; MAX_DIM + 1];
    c[0] = F::ONE;
    let mut m = <[[F; N]; N] as SquareOps<F, N>>::identity();
    for s in 1..=N {
        if s > 1 {
            let mut next = a.mul(&m);
            for (i, row) in next.iter_mut().enumerate() {
                row[i] = row[i].add(c[s - 1]);
            }
            m = next;
        }
        let am = a.mul(&m);
        let mut trace = F::ZERO;
        for (i, row) in am.iter().enumerate() {
            trace = trace.add(row[i]);
        }
        c[s] = F::ZERO.sub(trace).div(F::from_usize(s));
    }
    c
}

// ---------------------------------------------------------------------------
// Durand–Kerner 求根
// ---------------------------------------------------------------------------

/// 首一多项式的值（Horner）。
fn polynomial_at<C: ComplexField>(coefficients: &[C::Real], z: C) -> C {
    let mut acc = C::ZERO;
    for &coefficient in coefficients {
        acc = acc.mul(z).add(C::from_real(coefficient));
    }
    acc
}

/// Durand–Kerner 求根，返回各根的**模长**（首一多项式）。
///
/// 初值是固定的几何序列（`0.4 + 0.9i` 的幂）⇒ 同一输入永远给出同一输出
/// （不依赖随机 / 迭代顺序）；固定 500 次上限与 `1e-14` 收敛阈值。
///
/// `degree` 是多项式次数；`coefficients.len()` 必须是 `degree + 1`。
pub fn root_moduli<F: RealField>(coefficients: &[F], degree: usize) -> Vec<F> {
    root_moduli_generic::<Complex<F>>(coefficients, degree)
}

fn root_moduli_generic<C: ComplexField>(coefficients: &[C::Real], degree: usize) -> Vec<C::Real> {
    let n = degree;
    debug_assert_eq!(coefficients.len(), n + 1, "首一多项式需要 n+1 个系数");
    let seed = C::from_parts(C::Real::from_f64(0.4), C::Real::from_f64(0.9));
    let mut roots = Vec::with_capacity(n);
    let mut current = C::from_real(<C::Real as Field>::ONE);
    for _ in 0..n {
        roots.push(current);
        current = current.mul(seed);
    }
    let epsilon = C::Real::from_f64(1e-14);
    for _ in 0..500 {
        let mut max_delta = <C::Real as Field>::ZERO;
        for i in 0..n {
            let mut denom = C::ONE;
            for (j, root) in roots.iter().enumerate() {
                if j != i {
                    denom = denom.mul(roots[i].sub(*root));
                }
            }
            if denom.abs() == <C::Real as Field>::ZERO {
                continue;
            }
            let delta = polynomial_at::<C>(coefficients, roots[i]).div(denom);
            roots[i] = roots[i].sub(delta);
            if delta.abs() > max_delta {
                max_delta = delta.abs();
            }
        }
        if max_delta <= epsilon {
            break;
        }
    }
    roots.iter().map(|root| root.abs()).collect()
}

/// **谱半径** `|λ|max`。
///
/// 1 / 2 阶用解析式；3 / 4 阶由特征多项式（Faddeev–LeVerrier）+
/// Durand–Kerner 求根。只在装配期调用，**不在**每帧路径上。
///
/// 非有限值（NaN / inf 来自非法常量）原样传播 ⇒ 调用方的 `< 1` 判定会拒绝。
pub fn spectral_radius<F: RealField, const N: usize>(a: &[[F; N]; N]) -> F {
    match N {
        1 => a[0][0].abs(),
        2 => {
            let (x, y) = (a[0][0], a[0][1]);
            let (z, w) = (a[1][0], a[1][1]);
            let trace = x.add(w);
            let det = x.mul(w).sub(y.mul(z));
            let disc = trace.mul(trace).sub(F::from_f64(4.0).mul(det));
            if disc >= F::ZERO {
                let root = disc.sqrt();
                let two = F::from_f64(2.0);
                let first = trace.add(root).div(two).abs();
                let second = trace.sub(root).div(two).abs();
                if first > second { first } else { second }
            } else {
                // 共轭复根：模长 = sqrt(det)（此时 det > 0）。
                det.abs().sqrt()
            }
        }
        _ => {
            let c = char_poly(a);
            let moduli = root_moduli(&c[..=N], N);
            let mut max = F::ZERO;
            for modulus in moduli {
                if modulus > max {
                    max = modulus;
                }
            }
            max
        }
    }
}

// ---------------------------------------------------------------------------
// slice 运行期入口（当前消费者 affine.rs：宽度运行期决定）
// ---------------------------------------------------------------------------

/// `n × n` 单位矩阵（运行期宽度）。
pub fn identity_slice(n: usize) -> Vec<Vec<f64>> {
    let mut out = vec![vec![0.0; n]; n];
    for (i, row) in out.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    out
}

/// 方阵乘法（运行期宽度；求和顺序升序 ⇒ 逐位可复现）。
pub fn mat_mul_slice(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    let mut out = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            out[i][j] = (0..n).map(|t| a[i][t] * b[t][j]).sum();
        }
    }
    out
}

/// 方阵快速幂（运行期宽度；与 [`SquareOps::pow`] **逐位相同**）。
pub fn mat_pow_slice(a: &[Vec<f64>], exponent: u64) -> Vec<Vec<f64>> {
    let mut acc = identity_slice(a.len());
    let mut base = a.to_vec();
    let mut exp = exponent;
    while exp > 0 {
        if exp & 1 == 1 {
            acc = mat_mul_slice(&acc, &base);
        }
        exp >>= 1;
        if exp > 0 {
            base = mat_mul_slice(&base, &base);
        }
    }
    acc
}

/// 特征多项式系数（运行期宽度；返回长度 `n + 1`）。
pub fn char_poly_slice(a: &[Vec<f64>]) -> Vec<f64> {
    let n = a.len();
    let mut c = vec![0.0; n + 1];
    c[0] = 1.0;
    let mut m = identity_slice(n);
    for s in 1..=n {
        if s > 1 {
            let mut next = mat_mul_slice(a, &m);
            for (i, row) in next.iter_mut().enumerate() {
                row[i] += c[s - 1];
            }
            m = next;
        }
        let am = mat_mul_slice(a, &m);
        let trace: f64 = (0..n).map(|i| am[i][i]).sum();
        c[s] = -trace / s as f64;
    }
    c
}

/// 谱半径（运行期宽度；与 [`spectral_radius`] **逐位相同**）。
pub fn spectral_radius_slice(a: &[Vec<f64>]) -> f64 {
    let k = a.len();
    match k {
        1 => a[0][0].abs(),
        2 => {
            let (x, y) = (a[0][0], a[0][1]);
            let (z, w) = (a[1][0], a[1][1]);
            let trace = x + w;
            let det = x * w - y * z;
            let disc = trace * trace - 4.0 * det;
            if disc >= 0.0 {
                let root = disc.sqrt();
                ((trace + root) / 2.0).abs().max(((trace - root) / 2.0).abs())
            } else {
                det.abs().sqrt()
            }
        }
        _ => {
            let coefficients = char_poly_slice(a);
            root_moduli(&coefficients, k)
                .into_iter()
                .fold(0.0, f64::max)
        }
    }
}

// ---------------------------------------------------------------------------
// 仿射递推 `x ↦ A·x + b`（当前消费者：scene-core 的 `AffineMap`）
// ---------------------------------------------------------------------------

/// 增广矩阵 `[[A, b], [0, 1]]` 的 `n` 次幂（幂表：可跨实例复用）。
pub fn affine_powered_matrix(a: &[Vec<f64>], b: &[f64], n: u64) -> Vec<Vec<f64>> {
    let k = a.len();
    let mut base = vec![vec![0.0; k + 1]; k + 1];
    for i in 0..k {
        base[i][..k].copy_from_slice(&a[i]);
        base[i][k] = b[i];
    }
    base[k][k] = 1.0;
    mat_pow_slice(&base, n)
}

/// 用已算好的幂表把 `init` 推进一步（`O(k²)`）。
pub fn affine_apply_powered(powered: &[Vec<f64>], init: &[f64]) -> Vec<f64> {
    let k = init.len();
    (0..k)
        .map(|i| (0..k).map(|j| powered[i][j] * init[j]).sum::<f64>() + powered[i][k])
        .collect()
}

/// **闭式定义**：`State(n) := (A, b)^n(init)`。
///
/// 纯函数 ⇒ seek 与路径无关；只用 `+ − ×` ⇒ IEEE754 逐位可复现。
pub fn affine_apply(a: &[Vec<f64>], b: &[f64], init: &[f64], n: u64) -> Vec<f64> {
    assert_eq!(init.len(), b.len(), "init 宽度必须与仿射图一致");
    let powered = affine_powered_matrix(a, b, n);
    affine_apply_powered(&powered, init)
}

/// 多实例共享**一次**矩阵幂（每个结果与 [`affine_apply`] 逐位相同）。
pub fn affine_apply_many(
    a: &[Vec<f64>],
    b: &[f64],
    inits: &[Vec<f64>],
    n: u64,
) -> Vec<Vec<f64>> {
    let powered = affine_powered_matrix(a, b, n);
    inits
        .iter()
        .map(|init| {
            assert_eq!(init.len(), b.len(), "init 宽度必须与仿射图一致");
            affine_apply_powered(&powered, init)
        })
        .collect()
}

/// 逐次重放（**近似实现 / 测试 oracle**，不是定义）。
pub fn affine_replay(a: &[Vec<f64>], b: &[f64], init: &[f64], n: u64) -> Vec<f64> {
    let mut state = init.to_vec();
    for _ in 0..n {
        state = (0..a.len())
            .map(|i| {
                (0..a.len())
                    .map(|j| a[i][j] * state[j])
                    .sum::<f64>()
                    + b[i]
            })
            .collect();
    }
    state
}

/// 在 `0..=horizon` 上比对闭式定义与重放（**容差内**，不是逐位）。
pub fn affine_replay_within(
    a: &[Vec<f64>],
    b: &[f64],
    init: &[f64],
    horizon: u64,
    tolerance: f64,
) -> bool {
    (0..=horizon).all(|n| {
        affine_apply(a, b, init, n)
            .iter()
            .zip(affine_replay(a, b, init, n))
            .all(|(x, y)| !x.is_nan() && !y.is_nan() && (x - y).abs() <= tolerance)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(rows: &[&[f64]]) -> Vec<Vec<f64>> {
        rows.iter().map(|row| row.to_vec()).collect()
    }

    #[test]
    fn complex_arithmetic_matches_hand_computation() {
        let a = Complex::new(1.0, 2.0);
        let b = Complex::new(3.0, -1.0);
        assert_eq!(a.add(b), Complex::new(4.0, 1.0));
        assert_eq!(a.mul(b), Complex::new(5.0, 5.0));
        assert_eq!(a.div(b), Complex::new(0.1, 0.7));
        assert!((a.abs() - 5f64.sqrt()).abs() < 1e-15);
        let root = Complex::new(-1.0, 0.0).sqrt();
        assert!(root.re.abs() < 1e-15);
        assert!((root.im - 1.0).abs() < 1e-15);
    }

    #[test]
    fn square_ops_pow_matches_repeated_multiplication() {
        let a: [[f64; 2]; 2] = [[1.0, 1.0], [-0.02, 0.97]];
        let mut repeated = <[[f64; 2]; 2] as SquareOps<f64, 2>>::identity();
        for _ in 0..7 {
            repeated = repeated.mul(&a);
        }
        // 快速幂与逐次乘法的舍入路径不同 ⇒ 只要求容差内一致。
        let powered = a.pow(7);
        for row in 0..2 {
            for col in 0..2 {
                assert!((powered[row][col] - repeated[row][col]).abs() < 1e-12);
            }
        }
        assert_eq!(a.pow(0), <[[f64; 2]; 2] as SquareOps<f64, 2>>::identity());

        // 对拍判据：slice 入口与 const generic 入口**逐位相同**（同一个快速幂算法）。
        let slice = mat_pow_slice(&matrix(&[&[1.0, 1.0], &[-0.02, 0.97]]), 7);
        assert_eq!(slice, vec![powered[0].to_vec(), powered[1].to_vec()]);
    }

    #[test]
    fn characteristic_polynomial_of_spring_matrix() {
        // λ² − 1.97λ + 0.99
        let a: [[f64; 2]; 2] = [[1.0, 1.0], [-0.02, 0.97]];
        let c = char_poly(&a);
        assert_eq!(c[0], 1.0);
        assert!((c[1] + 1.97).abs() < 1e-15, "c1 = {}", c[1]);
        assert!((c[2] - 0.99).abs() < 1e-15, "c2 = {}", c[2]);
        assert_eq!(&char_poly_slice(&matrix(&[&[1.0, 1.0], &[-0.02, 0.97]]))[..=2], &c[..=2]);
    }

    #[test]
    fn root_moduli_finds_cubic_roots() {
        // (x−1)(x−2)(x−3) = x³ − 6x² + 11x − 6
        let moduli = root_moduli(&[1.0, -6.0, 11.0, -6.0], 3);
        let mut sorted = moduli.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((sorted[0] - 1.0).abs() < 1e-9);
        assert!((sorted[1] - 2.0).abs() < 1e-9);
        assert!((sorted[2] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn spectral_radius_covers_all_widths() {
        let one: [[f64; 1]; 1] = [[-0.5]];
        assert_eq!(spectral_radius(&one), 0.5);

        let two: [[f64; 2]; 2] = [[1.0, 1.0], [-0.02, 0.97]];
        assert!((spectral_radius(&two) - 0.99f64.sqrt()).abs() < 1e-12);

        let three: [[f64; 3]; 3] = [[0.5, 0.0, 0.0], [0.0, 0.25, 0.0], [0.0, 0.0, 0.75]];
        assert!((spectral_radius(&three) - 0.75).abs() < 1e-9);

        // slice 入口与 const generic 入口逐位相同。
        assert_eq!(
            spectral_radius(&three),
            spectral_radius_slice(&matrix(&[
                &[0.5, 0.0, 0.0],
                &[0.0, 0.25, 0.0],
                &[0.0, 0.0, 0.75],
            ]))
        );
    }

    #[test]
    fn affine_recurrence_closed_form_and_replay_agree() {
        let a = matrix(&[&[1.0, 1.0], &[-0.02, 0.97]]);
        let b = vec![0.0, 0.0];
        let init = [1.5, 0.0];

        // 多实例共享一次幂表：每个结果与单点逐位相同。
        let many = affine_apply_many(&a, &b, &vec![init.to_vec(); 4], 500);
        assert!(
            many.iter()
                .all(|value| value == &affine_apply(&a, &b, &init, 500))
        );

        // 闭式定义 vs 逐次重放：只在容差内一致（舍入路径不同）。
        assert!(affine_replay_within(&a, &b, &init, 200, 1e-3));
        assert!(!affine_replay_within(&a, &b, &init, 200, 0.0));
    }
}
