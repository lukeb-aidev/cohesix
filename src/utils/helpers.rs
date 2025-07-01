// CLASSIFICATION: COMMUNITY
// Filename: helpers.rs v0.1
// Date Modified: 2025-06-01
// Author: Lukas Bower

use crate::prelude::*;
#[forbid(unsafe_code)]
#[warn(missing_docs)]

//
// ─────────────────────────────────────────────────────────────
// Cohesix · Miscellaneous Utility Helpers
//
// This module collects *tiny*, dependency‑free helpers that don’t
// warrant a separate file yet but are commonly reused.
//
// * [`sleep_ms`]      – Cross‑platform sleep in milliseconds.
// * [`hex_dump`]      – Hex‑string representation of byte slices.
// * Re‑exports of selected helpers from `utils::format` ‑‑ so users
//   can `use cohesix::utils::helpers::*;` and get the basics.
//
// All code is `unsafe`‑free and works in a `std` context; if we
// need `no_std`, we’ll add conditional compilation.
// ─────────────────────────────────────────────────────────────

use std::{fmt::Write, thread, time::Duration};

/// Sleep `ms` milliseconds (wrapper over `std::thread::sleep`).
pub fn sleep_ms(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

/// Return a hex‑encoded string (`lowercase`) of `bytes`.
///
/// ```
/// use cohesix::utils::helpers::hex_dump;
/// assert_eq!(hex_dump(&[0xDE, 0xAD, 0xBE, 0xEF]), "deadbeef");
/// ```
pub fn hex_dump(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{:02x}", b).unwrap();
    }
    s
}

// Re‑export selected helpers from `utils::format`
pub use super::format::{human_bytes, truncate_middle};

// ───────────────────────────── tests ─────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_output_correct() {
        assert_eq!(hex_dump(&[0, 1, 255]), "0001ff");
    }

    #[test]
    fn sleep_non_panicking() {
        sleep_ms(1);
    }
}
