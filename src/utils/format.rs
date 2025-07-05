// CLASSIFICATION: COMMUNITY
// Filename: format.rs v0.1
// Date Modified: 2025-06-01
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix · Utility Format Helpers
//
// A small collection of frequently reused formatting helpers that
// do **not** pull in heavy dependencies.  Currently includes:
//
// * [`human_bytes`] – Pretty‑prints byte counts (e.g. `42.6 MiB`)
// * [`truncate_middle`] – Truncates long strings with “…” in the middle.
//
// These functions are *pure* and `no_std`‑friendly so they can be
// reused in early‑boot code if desired.
// ─────────────────────────────────────────────────────────────

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};

/// Return a human‑readable string for a byte count using IEC (MiB) units.
///
/// ```
/// use cohesix::utils::format::human_bytes;
/// assert_eq!(human_bytes(1_048_576), "1.0 MiB");
/// ```
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut value = bytes as f64;
    let mut idx = 0usize;

    while value >= 1024.0 && idx < UNITS.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", bytes, UNITS[idx])
    } else {
        format!("{:.1} {}", value, UNITS[idx])
    }
}

/// Truncate `s` to fit within `max_len`, inserting “…” in the middle.
///
/// If `s.len()` ≤ `max_len`, the original string is returned.
pub fn truncate_middle(s: &str, max_len: usize) -> String {
    let len = s.chars().count();
    if len <= max_len || max_len < 3 {
        return s.to_owned();
    }
    let keep = (max_len - 1) / 2;
    let start: String = s.chars().take(keep).collect();
    let end: String = s
        .chars()
        .rev()
        .take(keep)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{}…{}", start, end)
}

// ───────────────────────────── tests ─────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_formatting() {
        assert_eq!(human_bytes(500), "500 B");
        assert_eq!(human_bytes(1_048_576), "1.0 MiB");
    }

    #[test]
    fn truncate_logic() {
        let s = "abcdefghijklmnopqrstuvwxyz";
        assert_eq!(truncate_middle(s, 10), "abcd…wxyz");
        assert_eq!(truncate_middle(s, 5), "ab…yz");
        assert_eq!(truncate_middle(s, s.len()), s);
    }
}
