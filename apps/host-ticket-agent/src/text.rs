// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Bound untrusted host-executor text without splitting UTF-8 characters.
// Author: Lukas Bower
#![forbid(unsafe_code)]

//! UTF-8-safe bounded text helpers shared by ticket executors and receipts.

/// Return at most `max_bytes` from `input`, ending on a UTF-8 character boundary.
#[must_use]
pub fn truncate_utf8(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_owned();
    }
    let mut boundary = max_bytes.min(input.len());
    while boundary > 0 && !input.is_char_boundary(boundary) {
        boundary -= 1;
    }
    input[..boundary].to_owned()
}

/// Lossily decode, flatten line breaks, trim, and bound untrusted process output.
#[must_use]
pub fn bounded_utf8_lossy(bytes: &[u8], max_bytes: usize) -> String {
    let flattened = String::from_utf8_lossy(bytes).replace(['\n', '\r'], " ");
    truncate_utf8(flattened.trim(), max_bytes)
}

/// Replace control characters with spaces, trim, and bound an existing UTF-8 string.
#[must_use]
pub fn bounded_single_line(input: &str, max_bytes: usize) -> String {
    let sanitized: String = input
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    truncate_utf8(sanitized.trim(), max_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_preserves_multibyte_boundaries() {
        assert_eq!(truncate_utf8("ab🙂cd", 2), "ab");
        assert_eq!(truncate_utf8("ab🙂cd", 5), "ab");
        assert_eq!(truncate_utf8("ab🙂cd", 6), "ab🙂");
        assert_eq!(truncate_utf8("ab🙂cd", 8), "ab🙂cd");
    }

    #[test]
    fn lossy_capture_is_flattened_and_bounded() {
        let captured = bounded_utf8_lossy("🔥火\n🔥".as_bytes(), 7);
        assert!(captured.is_char_boundary(captured.len()));
        assert!(captured.len() <= 7);
        assert!(!captured.contains('\n'));
    }

    #[test]
    fn single_line_encoder_removes_non_newline_controls() {
        assert_eq!(bounded_single_line("  ok\t🙂\0tail  ", 7), "ok 🙂");
    }
}
