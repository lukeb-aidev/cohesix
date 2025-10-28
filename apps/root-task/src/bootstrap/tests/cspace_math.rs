// Author: Lukas Bower

use crate::bootstrap::cspace::{slot_advance, slot_in_empty_window};

#[test]
fn slot_window_bounds() {
    let start = 0x0140u32;
    let end = 0x2000u32;
    assert!(!slot_in_empty_window(start - 1, start, end));
    assert!(slot_in_empty_window(start, start, end));
    assert!(slot_in_empty_window(start + 1, start, end));
    assert!(!slot_in_empty_window(end, start, end));
}

#[test]
fn slot_sequence_progresses_monotonically() {
    let start = 0x0140u32;
    let end = 0x2000u32;
    let mut current = start;
    let mut count = 0u32;
    while current < start + 256 {
        assert!(slot_in_empty_window(current, start, end));
        current = slot_advance(current).expect("slot_advance should not overflow within window");
        count += 1;
    }
    assert_eq!(current, start + count);
}

#[test]
fn slot_advance_overflow_returns_none() {
    let max = u32::MAX;
    assert!(slot_advance(max).is_none());
}
