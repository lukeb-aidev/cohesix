// CLASSIFICATION: COMMUNITY
// Filename: const_eval.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Constant Expression Evaluator
///
/// Provides utility support for parsing and evaluating constant expressions at compile time
/// or runtime as needed. Intended for use in macros, config parsers, or unit test expressions.
/// Evaluates a simple constant arithmetic expression string and returns the result as i64.
/// Supports `+`, `-`, `*`, and `/` operators with integer literals.
pub fn eval(expr: &str) -> Result<i64, String> {
    // Shunting-yard style evaluation supporting + - * / and parentheses.
    let mut values: Vec<i64> = Vec::new();
    let mut ops: Vec<char> = Vec::new();
    let mut num = String::new();
    let push_num = |num: &mut String, values: &mut Vec<i64>| -> Result<(), String> {
        if !num.is_empty() {
            let v = num.parse::<i64>().map_err(|_| "invalid number")?;
            values.push(v);
            num.clear();
        }
        Ok(())
    };

    fn precedence(op: char) -> i32 {
        match op {
            '+' | '-' => 1,
            '*' | '/' => 2,
            _ => 0,
        }
    }

    fn apply_op(values: &mut Vec<i64>, op: char) -> Result<(), String> {
        if values.len() < 2 {
            return Err("malformed expression".into());
        }
        let b = values.pop().unwrap();
        let a = values.pop().unwrap();
        let res = match op {
            '+' => a + b,
            '-' => a - b,
            '*' => a * b,
            '/' => {
                if b == 0 {
                    return Err("division by zero".into());
                }
                a / b
            }
            _ => return Err("unknown operator".into()),
        };
        values.push(res);
        Ok(())
    }

    for ch in expr.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
        } else if ch == ' ' {
            continue;
        } else if ch == '(' {
            push_num(&mut num, &mut values)?;
            ops.push(ch);
        } else if ch == ')' {
            push_num(&mut num, &mut values)?;
            while let Some(op) = ops.pop() {
                if op == '(' {
                    break;
                }
                apply_op(&mut values, op)?;
            }
        } else if "+-*/".contains(ch) {
            push_num(&mut num, &mut values)?;
            while let Some(&op2) = ops.last() {
                if precedence(op2) >= precedence(ch) {
                    ops.pop();
                    apply_op(&mut values, op2)?;
                } else {
                    break;
                }
            }
            ops.push(ch);
        } else {
            return Err(format!("invalid character '{}'", ch));
        }
    }
    push_num(&mut num, &mut values)?;
    while let Some(op) = ops.pop() {
        apply_op(&mut values, op)?;
    }
    if values.len() == 1 {
        Ok(values[0])
    } else {
        Err("malformed expression".into())
    }
}

/// Returns true if the input string appears to be a valid const expression.
pub fn is_valid_expression(expr: &str) -> bool {
    expr.chars()
        .all(|c| c.is_ascii_digit() || "+-*/() ".contains(c))
}
