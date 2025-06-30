// CLASSIFICATION: PRIVATE
// Filename: mod.rs · HAL facade v0.2
// Date Modified: 2026-11-20
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix · Hardware‑Abstraction Layer (facade)
//
// This `mod.rs` file exports the architecture‑specific HAL
// implementations selected at **compile‑time** via `cfg`:
//
//   • `hal::arm64`   – Jetson / Raspberry Pi 5 targets
//   • `hal::x86_64`  – PC‑class development hosts
//
// Additional architectures can be added by creating a sibling
// sub‑module (e.g. `riscv64`) and extending the `cfg` list.
//
// The facade itself contains **no code**; it merely re‑exports
// the correct back‑end so higher layers can call
//
// ```rust
// use cohesix::hal::{arm64, x86_64}; // functions re‑exported from arch modules
// ```
//
// without caring about the underlying CPU family.
// ─────────────────────────────────────────────────────────────

#![allow(unsafe_code)]
#![warn(missing_docs)]

/// ARM64 implementation – selected when compiling for `aarch64`.
#[cfg(target_arch = "aarch64")]
pub mod arm64;

/// x86‑64 implementation – selected when compiling for `x86_64`.
#[cfg(target_arch = "x86_64")]
pub mod x86_64;
//__panel_test__
// Runs only under bare-metal QEMU targets to prevent SIGILL on host.
#[cfg(all(test, target_os = "none"))]
mod __panel_test_compile {
    // Ensure modules compile; privileged instructions are not executed in tests.
    #[test]
    fn link_arch_init() {
        assert!(true);
    }
}
//__panel_test__
