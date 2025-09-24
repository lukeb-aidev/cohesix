// CLASSIFICATION: COMMUNITY
// Filename: bootloader.c v0.9
// Author: Lukas Bower
// Date Modified: 2027-12-31
// SPDX-License-Identifier: MIT
//
// Cohesix OS bootloader (seL4 root task)
// Assigns capability slots per role and launches role-specific init script.

#include <stdint.h>
#include <string.h>
#include <sel4/sel4.h>
#include "boot_trampoline.h"

#define COH_BOOT_ROLE_BUF       32
#define COH_BOOT_STATUS_TAG     "[bootloader] "

static volatile uint8_t *const UART0 =
    (volatile uint8_t *)COH_BOOT_TRAMPOLINE_UART_BASE;

extern int boot_trampoline_crc_ok;

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

static void console_log(const char *msg)
{
    console_write(COH_BOOT_STATUS_TAG);
    console_write_line(msg);
}

static void console_write_hex(unsigned int value)
{
    char buf[2 + sizeof(unsigned int) * 2 + 1];
    static const char hex[] = "0123456789abcdef";
    buf[0] = '0';
    buf[1] = 'x';
    for (size_t i = 0; i < sizeof(unsigned int) * 2; ++i) {
        size_t shift = (sizeof(unsigned int) * 2 - 1 - i) * 4;
        buf[2 + i] = hex[(value >> shift) & 0xFu];
    }
    buf[2 + sizeof(unsigned int) * 2] = '\0';
    console_write(buf);
}

static const char *detect_role(void) {
    extern trampoline_hdr_t __trampoline_hdr;
    static char buf[COH_BOOT_ROLE_BUF];
    size_t len = strnlen(__trampoline_hdr.role_hint,
                         sizeof(__trampoline_hdr.role_hint));
    if (len > 0) {
        if (len >= sizeof(buf))
            len = sizeof(buf) - 1;
        memcpy(buf, __trampoline_hdr.role_hint, len);
        buf[len] = '\0';
        return buf;
    }

    return "DroneWorker";
}

/*
 * Provision the init process with a basic capability layout.
 * We copy the init thread's TCB capability into slot 1 of its CSpace
 * so sel4utils_copy_path_to_process can succeed when new threads are
 * created from the loader.
 */
static void assign_caps(const char *role)
{
    (void)role;
    seL4_Error err;

    err = seL4_TCB_SetSpace(seL4_CapInitThreadTCB,
                            seL4_CapNull,
                            seL4_CapInitThreadCNode, 0,
                            seL4_CapInitThreadVSpace, 0);
    if (err != seL4_NoError) {
        console_write(COH_BOOT_STATUS_TAG);
        console_write("TCB_SetSpace failed err=");
        console_write_hex((unsigned int)err);
        console_putc('\n');
    } else {
        console_log("init CSpace root installed");
    }

    err = seL4_CNode_Copy(seL4_CapInitThreadCNode,
                          1,
                          seL4_WordBits,
                          seL4_CapInitThreadCNode,
                          seL4_CapInitThreadTCB,
                          seL4_WordBits,
                          seL4_AllRights);
    if (err != seL4_NoError) {
        console_write(COH_BOOT_STATUS_TAG);
        console_write("cap copy failed err=");
        console_write_hex((unsigned int)err);
        console_putc('\n');
    } else {
        console_log("caps assigned");
    }
}

static void boot_success(void)
{
    console_write_line("BOOT_OK");
}

static void boot_fail(const char *reason)
{
    console_write("BOOT_FAIL:");
    console_write(reason);
    console_putc('\n');
}

static void handoff_to_kernel(void)
{
    console_log("handoff to seL4");
    /* The actual hand-off is performed by the trampoline and Rust loader. */
    for (;;) {
        /* Busy wait to avoid returning to firmware. */
#if defined(__aarch64__)
        __asm__ volatile("wfe" ::: "memory");
#else
        __asm__ volatile("hlt");
#endif
    }
}

/*
 * Boot phases:
 * 1) detect_role() determines CohRole.
 * 2) Log boot role information for diagnostics.
 * 3) assign_caps() sets capability slots per role.
 * 4) Emit boot status and hand off to the Rust trampoline.
 */
int main(void) {
    const char *role = detect_role();

    console_log("bootloader start");
    console_log("assign capabilities");
    assign_caps(role);

    if (boot_trampoline_crc_ok)
        boot_success();
    else
        boot_fail("trampoline_crc");

    console_log("role detected");
    console_write_line(role);

    handoff_to_kernel();
    return 0;
}
