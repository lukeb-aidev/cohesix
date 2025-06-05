// CLASSIFICATION: COMMUNITY
// Filename: const_eval_test.rs v0.1
// Date Modified: 2025-06-05
// Author: Cohesix Codex

use cohesix::utils::const_eval;

#[test]
fn eval_simple_expressions() {
    assert_eq!(const_eval::eval("1 + 2 * 3").unwrap(), 7);
    assert_eq!(const_eval::eval("10 / (2 + 3)").unwrap(), 2);
}
