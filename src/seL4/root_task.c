// CLASSIFICATION: COMMUNITY
// Filename: root_task.c v0.3
// Author: Lukas Bower
// Date Modified: 2026-12-31

/*
 * Simplified seL4 root task stub for Cohesix.
 * Creates /srv/cohrole based on boot parameters and environment.
 */

#ifndef MINIMAL_UEFI
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
#endif

static void write_role(void) {
#ifndef MINIMAL_UEFI
    const char *role = getenv("COH_ROLE");
    if (!role) role = "Unknown";
    mkdir("/srv", 0755);
    FILE *f = fopen("/srv/cohrole", "w");
    if (f) {
        fprintf(f, "%s", role);
        fclose(f);
    }
#else
    (void)0; /* TODO: implement role exposure via seL4 RPC */
#endif
}

int main(int argc, char **argv) {
    (void)argc; (void)argv;
    write_role();
    return 0;
}

#include <stddef.h>

extern void coh_load_namespace(void);
extern void coh_expose_role(const char *role);
extern const char *coh_boot_role(void);

void root_task_start(void) {
    const char *role = coh_boot_role();
    coh_expose_role(role);
    coh_load_namespace();
    // kernel continues in Rust
}

