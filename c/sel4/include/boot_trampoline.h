// CLASSIFICATION: PRIVATE
// Filename: boot_trampoline.h v0.2
// Date Modified: 2025-06-01
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix · seL4 Boot Trampoline Header
//
// Provides the public prototype for the tiny C trampoline that
// jumps from verified seL4 ASM into the Rust `early_init()`.
// Added conventional include‑guards for tool‑chain portability
// (some static‑analysis tools discourage `#pragma once`).
// ─────────────────────────────────────────────────────────────

#ifndef COHESIX_BOOT_TRAMPOLINE_H
#define COHESIX_BOOT_TRAMPOLINE_H

#ifdef __cplusplus
extern "C" {
#endif

/**
 * @brief Entry invoked by verified seL4 assembly.
 *
 * The function is implemented in `boot_trampoline.c` and performs
 * minimal register/stack setup before tail‑calling the Rust
 * `rust_early_init()` symbol.
 */
void boot_trampoline(void);

#ifdef __cplusplus
}
#endif

#endif /* COHESIX_BOOT_TRAMPOLINE_H */
