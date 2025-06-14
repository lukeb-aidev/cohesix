// CLASSIFICATION: PRIVATE
// Filename: boot_trampoline.h v0.5
// Date Modified: 2025-07-22
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

#include <stdint.h>

/* Boot constants used by the trampoline */
#define BOOT_TRAMPOLINE_UART_BASE      0x09000000u
#define BOOT_TRAMPOLINE_CRC_POLYNOMIAL 0xEDB88320u
#define BOOT_TRAMPOLINE_LOG_SIZE       128
#include "boot_success.h"

/**
 * @brief Entry invoked by verified seL4 assembly.
 *
 * The function is implemented in `boot_trampoline.c` and performs
 * minimal register/stack setup before tail‑calling the Rust
 * `rust_early_init()` symbol.
 */
void boot_trampoline(void);

typedef struct {
    uint32_t crc;
    uint32_t length;
    char role_hint[16];
} trampoline_hdr_t;

extern int boot_trampoline_crc_ok;

#ifdef __cplusplus
}
#endif

#endif /* COHESIX_BOOT_TRAMPOLINE_H */
