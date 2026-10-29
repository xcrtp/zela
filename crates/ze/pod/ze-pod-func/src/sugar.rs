//! **糖层的穿透文档**（计划 §5.5）：每个糖都必须写出等价的 core 写法。
//!
//! # 为什么这是门禁
//!
//! 糖太好用时，作者就学不会 `Mix` / `Scale` / `Clamp` 的组合；一旦遇到非线性
//! （ease-in-out、自定义曲线）糖就不够用了。所以糖必须是**可穿透的**：
//! 每个糖的文档都要给出"展开成什么"，作者照着写就能继续往下走。
//!
//! 与术语表"每个条目必须挂锚点"同一招：**不检查文字质量，只检查挂载点存在**。
//! 用例 [`tests::every_sugar_documents_its_core_form`] 做两件事：
//!
//! 1. `zela.sugar` 能力表里的每个函数都在这张表里（加糖漏文档 ⇒ 红）；
//! 2. 每条 `core` 非空（删掉等价写法 ⇒ 红）。
//!
//! # 为什么不自动生成
//!
//! 等价写法是**给人读的**，不是从实现反向打印的：`sugar_ramp` 的实现细节
//! （先算哪个子树）会变，而作者需要的 core 形态不会。两者由本文件的用例
//! 保证"语义同一"（`ramp` 的展开由 `expr_pod` 的快慢路径对拍覆盖）。

/// 一个糖的穿透条目。
pub struct SugarDoc {
    /// 糖的函数名（= `SugarApi` 的字段名 = 脚本里的 `sugar.<name>`）。
    pub name: &'static str,
    /// 等价的 core 写法（作者可以直接抄进脚本）。
    pub core: &'static str,
}

/// 全部糖的穿透文档（顺序 = 能力声明顺序）。
pub const SUGAR: &[SugarDoc] = &[
    SugarDoc {
        name: "ramp",
        core: "Mix { a = expr.pure(from), b = expr.pure(to), \
               t = Clamp { x = Scale { k = 1.0 / dur, of = clock.time() }, \
                           lo = expr.pure(0.0), hi = expr.pure(1.0) } }",
    },
    SugarDoc {
        name: "ramp2",
        core: "Mix { a = Vec2Of { x = expr.pure(ax), y = expr.pure(ay) }, \
               b = Vec2Of { x = expr.pure(bx), y = expr.pure(by) }, \
               t = Clamp { x = Scale { k = 1.0 / dur, of = clock.time() }, \
                           lo = expr.pure(0.0), hi = expr.pure(1.0) } }",
    },
    SugarDoc {
        name: "orbit2d",
        core: "Vec3FromVec2 { v = Vec2Of { \
                 x = round_f32(Mul { a = round_f32(expr.pure(radius)), \
                                     b = round_f32(Cos { of = angle }) }), \
                 y = round_f32(Mul { a = round_f32(expr.pure(radius)), \
                                     b = round_f32(Sin { of = angle }) }) }, \
               z = expr.pure(0.0) }  （angle = speed·t + phase，逐步 round_f32）",
    },
    SugarDoc {
        name: "wave",
        core: "round_f32(Add { a = round_f32(expr.pure(base)), \
                 b = round_f32(Mul { a = round_f32(expr.pure(amp)), \
                                     b = round_f32(Sin { of = round_f32(Mul { \
                                         a = round_f32(expr.pure(omega)), \
                                         b = clock.time() }) }) }) })",
    },
    SugarDoc {
        name: "f32",
        core: "RoundF32 { of = x }",
    },
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ze_pod_core::Capability;

    use super::SUGAR;
    use crate::api::SugarApi;

    /// **穿透门禁**：糖 ↔ 文档一一对应，且每条都写了等价 core 写法。
    #[test]
    fn every_sugar_documents_its_core_form() {
        let declared: BTreeSet<&str> = SugarApi::<'static>::signatures()
            .iter()
            .map(|signature| signature.name)
            .collect();
        let documented: BTreeSet<&str> = SUGAR.iter().map(|doc| doc.name).collect();
        assert_eq!(
            declared, documented,
            "糖的能力表与穿透文档必须一一对应（新增糖 → 补 `SUGAR` 条目）"
        );
        for doc in SUGAR {
            assert!(
                !doc.core.trim().is_empty(),
                "糖 `{}` 缺等价 core 写法（糖必须可穿透）",
                doc.name
            );
        }
    }
}
