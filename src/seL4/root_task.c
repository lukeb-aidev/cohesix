// CLASSIFICATION: COMMUNITY
// Filename: root_task.c v0.2
// Author: Lukas Bower
// Date Modified: 2025-06-20

/*
 * Simplified seL4 root task stub for Cohesix.
 * Creates /srv/cohrole based on boot parameters and environment.
 */

#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>

static void write_role(void) {
    const char *role = getenv("COH_ROLE");
    if (!role) role = "Unknown";
    mkdir("/srv", 0755);
    FILE *f = fopen("/srv/cohrole", "w");
    if (f) {
        fprintf(f, "%s", role);
        fclose(f);
    }
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

