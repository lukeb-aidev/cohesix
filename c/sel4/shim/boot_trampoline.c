// CLASSIFICATION: PRIVATE
// Filename: boot_trampoline.c v0.3
// Date Modified: 2025-07-15
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

static volatile uint8_t *const UART0 = (volatile uint8_t *)0x09000000;
static char fallback_log[128];
static size_t log_pos;

int boot_trampoline_crc_ok = 0;

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
            crc = (crc >> 1) ^ (0xEDB88320 & (-(int)(crc & 1)));
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

    uint32_t calc = crc32_calc((const uint8_t *)&rust_early_init,
                               __trampoline_hdr.length);
    boot_trampoline_crc_ok = (calc == __trampoline_hdr.crc);
    _trampoline_log((uintptr_t)&rust_early_init, boot_trampoline_crc_ok);
    if (!boot_trampoline_crc_ok)
        panic_uart("panic: trampoline CRC mismatch\n");

    rust_early_init();
    for (;;)
        ;
}
