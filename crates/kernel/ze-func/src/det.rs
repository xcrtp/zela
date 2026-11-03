//! **确定性超越函数**：算子语义里唯一的 sin / cos / … 实现。
//!
//! # 为什么不能直接用 `f64::sin`
//!
//! `std` 的超越函数落到**平台 libm**（MSVC / glibc / musl 各一份）。同一个输入在
//! 两份 libm 上可能差 1 ulp ⇒ 同一份脚本在不同平台算出不同的 f64 ⇒ 产物不逐位一致。
//! 加减乘除、`sqrt`、`floor` / `ceil` / `trunc` / `round` 由 IEEE 754 规定为
//! 正确舍入，不需要替换；**只锁超越函数**（依赖面最小）。
//!
//! # 实现
//!
//! 统一走 [`libm`]（musl 的纯 Rust 移植，no_std 可用，wasm 与 native 同一份代码）。
//! 这是本仓新增的第 27 个外部依赖，理由与验证结果记在 GAPS `BB-05`：
//! 用例 [`det_transcendentals_are_bit_stable`] 把一批输入的**结果位模式**钉成常量，
//! 换实现（或换 libm 版本）会直接红。
//!
//! # 覆盖边界
//!
//! 本模块只覆盖 **`zela.math` 算子语义**（含单态实例与 `rotate_rodrigues`）。
//! 几何细分（`ze-notion` / `zela-render` / `zela-render-vector`）仍调用平台 std：
//! 那些是**组装期**的派生物，换成 libm 会改像素，属于另一个决策（GAPS `BB-05`）。

/// `sin`。
pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

/// `cos`。
pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

/// `tan`。
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}

/// `asin`。
pub fn asin(x: f64) -> f64 {
    libm::asin(x)
}

/// `acos`。
pub fn acos(x: f64) -> f64 {
    libm::acos(x)
}

/// `atan`。
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}

/// `atan2(y, x)`。
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

/// `exp`。
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// `exp2`。
pub fn exp2(x: f64) -> f64 {
    libm::exp2(x)
}

/// `ln`（自然对数）。
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

/// `log2`。
pub fn log2(x: f64) -> f64 {
    libm::log2(x)
}

/// `pow`（`x^y`）。
pub fn pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FNV-1a（与 `ze_pod_core::capability_fingerprint` 同一算法，跨 Rust 版本稳定）。
    fn fold(hash: u64, bits: u64) -> u64 {
        (hash ^ bits).wrapping_mul(0x0000_0100_0000_01b3)
    }

    /// **跨构建逐位相同**的判据：一批输入的结果位模式折叠成一个常量。
    ///
    /// 换掉 libm（或换到会改动舍入的版本）⇒ 这里红。这不是"结果对不对"的用例
    /// （那由 `ops` 的语义用例负责），是"任何平台都算出同一串位"的用例。
    #[test]
    fn det_transcendentals_are_bit_stable() {
        let inputs = [
            0.0,
            0.5,
            1.0,
            -1.0,
            2.5,
            1e-3,
            std::f64::consts::PI,
            1e6,
            -0.25,
            0.999_999_9,
        ];
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for x in inputs {
            let unit = x.clamp(-1.0, 1.0);
            let positive = x.abs().max(1e-9);
            for bits in [
                sin(x).to_bits(),
                cos(x).to_bits(),
                tan(x).to_bits(),
                asin(unit).to_bits(),
                acos(unit).to_bits(),
                atan(x).to_bits(),
                atan2(x, 0.75).to_bits(),
                exp(x).to_bits(),
                exp2(x).to_bits(),
                ln(positive).to_bits(),
                log2(positive).to_bits(),
                pow(positive + 0.5, 1.7).to_bits(),
            ] {
                hash = fold(hash, bits);
            }
        }
        assert_eq!(hash, 14_333_979_151_148_114_719);
    }
}
