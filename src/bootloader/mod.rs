// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.2
// Date Modified: 2025-06-01
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix · Bootloader Sub‑crate (root module)
//
// This module stitches together all boot‑loader–specific logic
// required before the main Rust kernel enters execution.
//
// Current sub‑modules
// -------------------
// * `args` – tiny key=value parser for firmware cmdline
// * `init` – early HAL bring‑up + boot argument collection
//
// Future work
// -----------
// * `verify`  – signature & measurement chain
// * `memory`  – physical memory map construction
// * `handoff` – trampoline to kernel entry
// ─────────────────────────────────────────────────────────────

#[forbid(unsafe_code)]
#[warn(missing_docs)]

/// Command‑line parser used by the bootloader.
pub mod args;

/// Early initialisation entry‑point.
///
/// See [`init::early_init`].
pub mod init;
