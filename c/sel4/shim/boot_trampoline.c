// CLASSIFICATION: PRIVATE
// Filename: boot_trampoline.c v0.4
// Date Modified: 2025-07-22
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix · seL4 Boot Trampoline (C)
// 
// This minimal C function is invoked by the verified seL4 assembly
// start‑up code.  It performs **no** dynamic allocation, assumes an
// already‑valid stack, and immediately tail‑calls the Rust
// `rust_early_init()` symbol exported by the second‑stage loader.
//
// Any register or MMU manipulation must be done in assembly prior to
// calling this trampoline; doing it here would break the seL4 proof
// assumptions.
//
// Compile with  `-ffreestanding -fno-builtin` and link early in the
// boot image so the symbol is easily discoverable.
// ─────────────────────────────────────────────────────────────

#include "boot_trampoline.h"
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>

/*
 * Boot stages:
 * 1) Called from verified assembly with stack ready.
 * 2) Verify Rust entry checksum.
 * 3) Emit telemetry marker for successful hand-off.
 * 4) Jump to rust_early_init(); never returns.
 */

static volatile uint8_t *const UART0 = (volatile uint8_t *)BOOT_TRAMPOLINE_UART_BASE;
static char fallback_log[BOOT_TRAMPOLINE_LOG_SIZE];
static size_t log_pos;

int boot_trampoline_crc_ok = 0;

/* Write a success marker for Fabric OS/validator */
static void emit_success_telemetry(void)
{
    int fd = open(BOOT_SUCCESS_PATH, O_WRONLY | O_CREAT, 0644);
    if (fd >= 0) {
        write(fd, "ok\n", 3);
        close(fd);
    }
}

static void uart_putc(char c)
{
    *UART0 = (uint8_t)c;
}

static void uart_write(const char *s)
{
    while (*s)
        uart_putc(*s++);
}

static void log_write(const char *s)
{
    uart_write(s);
    while (*s && log_pos < sizeof(fallback_log) - 1)
        fallback_log[log_pos++] = *s++;
    fallback_log[log_pos] = '\0';
}

static void panic_uart(const char *msg)
{
    log_write(msg);
    for (;;)
        ;
}

static uint32_t crc32_calc(const uint8_t *data, size_t len)
{
    uint32_t crc = ~0u;
    for (size_t i = 0; i < len; ++i) {
        crc ^= data[i];
        for (int j = 0; j < 8; ++j)
            crc = (crc >> 1) ^ (BOOT_TRAMPOLINE_CRC_POLYNOMIAL & (-(int)(crc & 1)));
    }
    return ~crc;
}

static void _trampoline_log(uintptr_t entry, int ok)
{
    char buf[64];
    snprintf(buf, sizeof(buf), "trampoline %p crc %s\n", (void *)entry,
             ok ? "ok" : "fail");
    log_write(buf);
}

void boot_trampoline(void)
{
    extern void rust_early_init(void);
    extern trampoline_hdr_t __trampoline_hdr;

    /* Phase 1: verify Rust entry and log result */
    uint32_t calc = crc32_calc((const uint8_t *)&rust_early_init,
                               __trampoline_hdr.length);
    boot_trampoline_crc_ok = (calc == __trampoline_hdr.crc);
    _trampoline_log((uintptr_t)&rust_early_init, boot_trampoline_crc_ok);
    if (!boot_trampoline_crc_ok)
        panic_uart("panic: trampoline CRC mismatch\n");

    /* Phase 2: emit boot success before hand-off */
    emit_success_telemetry();

    /* Phase 3: transfer control to Rust early init */
    rust_early_init();
    for (;;)
        ;
}
