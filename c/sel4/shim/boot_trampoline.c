// CLASSIFICATION: PRIVATE
// Filename: boot_trampoline.c v0.6
// Date Modified: 2025-07-23
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
#include "boot_success.h"
#include <stdint.h>
#include <stddef.h>

/*
 * Boot stages:
 * 1) Called from verified assembly with stack ready.
 * 2) Verify Rust entry checksum.
 * 3) Emit telemetry marker for successful hand-off.
 * 4) Jump to rust_early_init(); never returns.
 */

static volatile uint8_t *const UART0 =
    (volatile uint8_t *)COH_BOOT_TRAMPOLINE_UART_BASE;
static char fallback_log[COH_BOOT_TRAMPOLINE_LOG_SIZE];
static size_t log_pos;

int boot_trampoline_crc_ok = 0;

/* Write a success marker for Fabric OS/validator */
static void console_putc(char c)
{
    *UART0 = (uint8_t)c;
}

static void console_write(const char *msg)
{
    while (*msg) {
        console_putc(*msg++);
    }
}

static void console_write_line(const char *msg)
{
    console_write(msg);
    console_putc('\n');
}

static void emit_fail_console(const char *reason)
{
    console_write("BOOT_FAIL:");
    console_write(reason);
    console_putc('\n');
}

static void emit_success_telemetry(void)
{
    console_write_line("BOOT_OK");
}

static void log_write(const char *s)
{
    const char *p = s;
    while (*p) {
        *UART0 = (uint8_t)*p;
        p++;
    }
    p = s;
    while (*p && log_pos < sizeof(fallback_log) - 1) {
        fallback_log[log_pos++] = *p++;
    }
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
            crc = (crc >> 1) ^
                  (COH_BOOT_TRAMPOLINE_CRC_POLYNOMIAL & (-(int)(crc & 1)));
    }
    return ~crc;
}

static void write_hex(uintptr_t value)
{
    char buf[2 + sizeof(uintptr_t) * 2 + 1];
    static const char hex[] = "0123456789abcdef";
    buf[0] = '0';
    buf[1] = 'x';
    for (size_t i = 0; i < sizeof(uintptr_t) * 2; ++i) {
        size_t shift = (sizeof(uintptr_t) * 2 - 1 - i) * 4;
        buf[2 + i] = hex[(value >> shift) & 0xFu];
    }
    buf[2 + sizeof(uintptr_t) * 2] = '\0';
    log_write(buf);
}

static void log_status(uintptr_t entry, int ok)
{
    log_write("trampoline ");
    write_hex(entry);
    log_write(" crc ");
    log_write(ok ? "ok" : "fail");
    log_write("\n");
}

void boot_trampoline(void)
{
    extern void rust_early_init(void);
    extern trampoline_hdr_t __trampoline_hdr;

    /* Phase 1: verify Rust entry and log result */
    uint32_t calc = 0;
    if (__trampoline_hdr.length != 0) {
        calc = crc32_calc((const uint8_t *)&rust_early_init,
                          __trampoline_hdr.length);
    }
    boot_trampoline_crc_ok = (__trampoline_hdr.length == 0) ||
                             (calc == __trampoline_hdr.crc);
    log_status((uintptr_t)&rust_early_init, boot_trampoline_crc_ok);
    if (!boot_trampoline_crc_ok) {
        emit_fail_console("crc_mismatch");
        panic_uart("panic: trampoline CRC mismatch\n");
    }

    /* Phase 2: emit boot success before hand-off */
    emit_success_telemetry();

    /* Phase 3: transfer control to Rust early init */
    rust_early_init();
    for (;;)
        ;
}
