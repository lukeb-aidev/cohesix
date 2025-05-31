// CLASSIFICATION: PRIVATE
// Filename: mod.rs · Cohesix boot subsystem
// Date Modified: 2025-05-31
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix Boot Subsystem – Root Module
//
// This module groups all boot‑time functionality such as secure
// measurements, firmware validation, and first‑stage hardware
// bring‑up.
//
// ## Current sub‑modules
// * `measure` – TPM‑style PCR extension helpers.
//
// ## Planned
// * `verify`  – signature chain & image authentication.
// * `init`    – early hardware initialisation (UART, watchdog).
// ─────────────────────────────────────────────────────────────

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// PCR extension helpers based on SHA‑256.
/// Re‑export [`measure::extend_pcr`] for convenience.
pub mod measure;
pub use measure::extend_pcr;
