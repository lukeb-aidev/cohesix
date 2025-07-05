// CLASSIFICATION: COMMUNITY
// Filename: test_coherr_macro.rs v0.1
// Author: Lukas Bower
// Date Modified: 2027-02-02

use cohesix::coherr;

#[test]
fn coherr_macro_compiles() {
    coherr!("coherr macro test {}", 1);
}
