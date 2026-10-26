//! `zela.math` 的**标量算子语义**（纯函数，无状态）。
//!
//! # 为什么在这里
//!
//! 算子的**身份**（namespace + name）由本 crate 的 capability 声明；
//! 算子的**语义**必须与身份同源，否则表达式层与求值层会各说各话。
//!
//! # 边界
//!
//! 只做标量（`f64`）。向量 / 路径算子仍留在场景侧；错误里带算子限定名，
//! 便于把「脚本作者看到的写法」映射回「函子库里的那一条签名」。

use ze_func::{EvalError, OpRef, Value};

/// 本函子库认识的算子（装配期校验用；未知算子直接拒绝）。
///
/// # 为什么有 `and` / `or` / `not`
///
/// predicate（`bind` 的生效条件）是 Bool 表达式，必须有比较与布尔组合子；
/// 它们是**值算子**，与 `sin` 等同层。Bool 结果不进 `anim_value`（root 侧语义不变）。
pub const OPS: &[&str] = &[
    "sin", "cos", "add", "mul", "scale", "sum", "lt", "gt", "le", "ge", "eq", "ne", "and", "or",
    "not",
];

/// `op` 是否属于本库（namespace 必须一致，名字在 [`OPS`] 里）。
pub fn is_known(op: &OpRef) -> bool {
    op.namespace == crate::LIBRARY_NAMESPACE && OPS.contains(&op.name.as_str())
}

/// 标量算子求值。
///
/// 未知算子 / 实参类型不符 ⇒ [`EvalError`]，不静默返回 0。
pub fn eval(op: &OpRef, args: &[Value]) -> Result<Value, EvalError> {
    let number = |index: usize| -> Result<f64, EvalError> {
        args.get(index)
            .and_then(Value::as_f64)
            .ok_or_else(|| EvalError::op(op, format!("第 {index} 个实参不是数值")))
    };
    let boolean = |index: usize| -> Result<bool, EvalError> {
        match args.get(index) {
            Some(Value::Bool(value)) => Ok(*value),
            _ => Err(EvalError::op(
                op,
                format!("第 {index} 个实参不是布尔（Bool 由比较算子产出）"),
            )),
        }
    };

    match op.name.as_str() {
        "sin" => Ok(Value::F64(number(0)?.sin())),
        "cos" => Ok(Value::F64(number(0)?.cos())),
        "add" => Ok(Value::F64(number(0)? + number(1)?)),
        "mul" => Ok(Value::F64(number(0)? * number(1)?)),
        // `scale(k, of)`：k 在产物里是字面量叶子，这里只是普通乘法。
        "scale" => Ok(Value::F64(number(0)? * number(1)?)),
        "sum" => {
            let mut total = 0.0;
            for index in 0..args.len() {
                total += number(index)?;
            }
            Ok(Value::F64(total))
        }
        "lt" => Ok(Value::Bool(number(0)? < number(1)?)),
        "gt" => Ok(Value::Bool(number(0)? > number(1)?)),
        "le" => Ok(Value::Bool(number(0)? <= number(1)?)),
        "ge" => Ok(Value::Bool(number(0)? >= number(1)?)),
        "eq" | "ne" => {
            let equal = match (args.first(), args.get(1)) {
                (Some(Value::Bool(a)), Some(Value::Bool(b))) => a == b,
                (Some(a), Some(b)) => match (a.as_f64(), b.as_f64()) {
                    (Some(a), Some(b)) => a == b,
                    _ => {
                        return Err(EvalError::op(
                            op,
                            "只支持数值或布尔比较（不做隐式真值转换）",
                        ))
                    }
                },
                _ => return Err(EvalError::op(op, "需要两个实参")),
            };
            Ok(Value::Bool(if op.name == "eq" { equal } else { !equal }))
        }
        "and" => Ok(Value::Bool(boolean(0)? && boolean(1)?)),
        "or" => Ok(Value::Bool(boolean(0)? || boolean(1)?)),
        "not" => Ok(Value::Bool(!boolean(0)?)),
        other => Err(EvalError::op(op, format!("未知算子 `{other}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn math(name: &str) -> OpRef {
        OpRef::new("zela.math", name)
    }

    #[test]
    fn scalar_operators_match_std_math() {
        let x = Value::F64(0.5);
        assert_eq!(
            eval(&math("sin"), std::slice::from_ref(&x)).unwrap(),
            Value::F64(0.5_f64.sin())
        );
        assert_eq!(
            eval(&math("mul"), &[x.clone(), Value::F64(3.0)]).unwrap(),
            Value::F64(1.5)
        );
        assert_eq!(
            eval(&math("scale"), &[Value::F64(4.0), x.clone()]).unwrap(),
            Value::F64(2.0)
        );
        assert_eq!(
            eval(&math("sum"), &[Value::F64(1.0), x, Value::F64(2.0)]).unwrap(),
            Value::F64(3.5)
        );
        assert_eq!(eval(&math("sum"), &[]).unwrap(), Value::F64(0.0));
    }

    #[test]
    fn type_errors_name_the_operator() {
        let err = eval(&math("sin"), &[Value::Text("x".to_string())]).unwrap_err();
        assert_eq!(
            err,
            EvalError::Op {
                op: "zela.math::sin".to_string(),
                message: "第 0 个实参不是数值".to_string(),
            }
        );
    }

    #[test]
    fn unknown_operator_is_explicit() {
        let err = eval(&math("tan"), &[Value::F64(1.0)]).unwrap_err();
        assert!(matches!(err, EvalError::Op { ref op, .. } if op == "zela.math::tan"));
    }

    /// 比较算子产出 `Bool`（predicate 的原料）；顺序与边界都按 IEEE 比较。
    #[test]
    fn comparisons_yield_bool() {
        let one = Value::F64(1.0);
        let two = Value::F64(2.0);
        assert_eq!(eval(&math("lt"), &[one.clone(), two.clone()]).unwrap(), Value::Bool(true));
        assert_eq!(eval(&math("gt"), &[one.clone(), two.clone()]).unwrap(), Value::Bool(false));
        assert_eq!(eval(&math("le"), &[two.clone(), two.clone()]).unwrap(), Value::Bool(true));
        assert_eq!(eval(&math("ge"), &[one.clone(), two.clone()]).unwrap(), Value::Bool(false));
        assert_eq!(eval(&math("eq"), &[two.clone(), two.clone()]).unwrap(), Value::Bool(true));
        assert_eq!(eval(&math("ne"), &[one, two]).unwrap(), Value::Bool(true));
    }

    /// 布尔组合子只接受 Bool：不做 `0 = false` 的隐式真值转换（U2）。
    #[test]
    fn boolean_combinators_require_bool() {
        assert_eq!(
            eval(&math("and"), &[Value::Bool(true), Value::Bool(false)]).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            eval(&math("or"), &[Value::Bool(true), Value::Bool(false)]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(eval(&math("not"), &[Value::Bool(true)]).unwrap(), Value::Bool(false));

        let err = eval(&math("and"), &[Value::F64(1.0), Value::Bool(true)]).unwrap_err();
        assert!(matches!(err, EvalError::Op { ref op, .. } if op == "zela.math::and"));
    }

    /// `eq` / `ne` 支持数值与布尔两种比较，其他类型显式报错。
    #[test]
    fn equality_compares_numbers_and_bools_only() {
        assert_eq!(
            eval(&math("eq"), &[Value::Bool(true), Value::Bool(true)]).unwrap(),
            Value::Bool(true)
        );
        let err = eval(&math("eq"), &[Value::Text("a".into()), Value::Text("a".into())])
            .unwrap_err();
        assert!(matches!(err, EvalError::Op { ref op, .. } if op == "zela.math::eq"));
    }
}
