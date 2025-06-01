// CLASSIFICATION: COMMUNITY
// Filename: const_eval.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Constant Expression Evaluator
//!
//! Provides utility support for parsing and evaluating constant expressions at compile time
//! or runtime as needed. Intended for use in macros, config parsers, or unit test expressions.

/// Evaluates a simple constant arithmetic expression string and returns the result as i64.
/// Supports `+`, `-`, `*`, and `/` operators with integer literals.
pub fn eval(expr: &str) -> Result<i64, String> {
    let sanitized = expr.replace(" ", "");
    let tokens: Vec<&str> = sanitized.split(|c| c == '+' || c == '-' || c == '*' || c == '/').collect();
    if tokens.len() < 2 {
        return Err("Expression must contain at least two operands.".into());
    }

    // TODO(cohesix): Implement full parser and operator precedence handling
    Err("Const eval stub: parser not yet implemented.".into())
}

/// Returns true if the input string appears to be a valid const expression.
pub fn is_valid_expression(expr: &str) -> bool {
    expr.chars().all(|c| c.is_digit(10) || "+-*/() ".contains(c))
}