// CLASSIFICATION: COMMUNITY
// Filename: posix_translation.rs v0.1
// Date Modified: 2025-07-11
// Author: Lukas Bower

use cohesix::posix::translate_syscall;

#[test]
fn translate_known_syscall() {
    assert_eq!(translate_syscall("open"), Some("coh_open"));
}

#[test]
fn translate_unknown_syscall() {
    assert_eq!(translate_syscall("fork"), None);
}
