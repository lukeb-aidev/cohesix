// CLASSIFICATION: COMMUNITY
// Filename: root_task.c v0.1
// Date Modified: 2025-06-18
// Author: Lukas Bower

/*
 * Cohesix seL4 root task integration.
 *
 * During early boot this function is invoked by the assembly entry point.
 * It loads the Plan 9 namespace using the Rust helper and exposes the
 * CohRole string as `/srv/cohrole` via the in-memory 9P filesystem.
 */

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

