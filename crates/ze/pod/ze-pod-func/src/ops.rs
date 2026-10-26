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
pub const OPS: &[&str] = &["sin", "cos", "add", "mul", "scale", "sum"];

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
}
