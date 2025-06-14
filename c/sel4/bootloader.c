// CLASSIFICATION: COMMUNITY
// Filename: bootloader.c v0.4
// Author: Lukas Bower
// Date Modified: 2025-07-22
// SPDX-License-Identifier: MIT
//
// Cohesix OS bootloader (seL4 root task)
// Assigns capability slots per role and launches role-specific init script.

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <time.h>
#include <signal.h>

#define BOOT_ROLE_BUF       32
#define BOOT_WATCHDOG_SECS  15
#define PATH_COHROLE        "/srv/cohrole"
#define PATH_BOOT_LOG       "/log/bootloader_init.log"
#define PATH_BOOT_ERROR     "/state/boot_error"

extern int boot_trampoline_crc_ok;

static void watchdog_handler(int sig)
{
    (void)sig;
    FILE *f = fopen(PATH_BOOT_ERROR, "a");
    if (f) {
        fprintf(f, "%ld watchdog timeout\n", (long)time(NULL));
        fclose(f);
    }
    _exit(1);
}

static const char *detect_role(void) {
    const char *role = getenv("cohrole");
    static char buf[BOOT_ROLE_BUF];
    FILE *f = NULL;

    if (role && *role)
        return role;

    f = fopen(PATH_COHROLE, "r");
    if (f) {
        if (fgets(buf, sizeof(buf), f)) {
            buf[strcspn(buf, "\r\n")] = '\0';
            fclose(f);
            if (strcmp(buf, "QueenPrimary") == 0 ||
                strcmp(buf, "KioskInteractive") == 0 ||
                strcmp(buf, "DroneWorker") == 0 ||
                strcmp(buf, "GlassesAgent") == 0 ||
                strcmp(buf, "SensorRelay") == 0 ||
                strcmp(buf, "SimulatorTest") == 0)
                return buf;
        } else {
            fclose(f);
        }
    }

    return "DroneWorker";
}

static void assign_caps(const char *role) {
    (void)role;
    // Stub: in real build this would configure seL4 cspace slots.
    printf("[bootloader] assign caps for %s\n", role);
}

static const char *script_for_role(const char *role) {
    if (strcmp(role, "DroneWorker") == 0)
        return "/init/worker.rc";
    if (strcmp(role, "KioskInteractive") == 0)
        return "/init/kiosk.rc";
    if (strcmp(role, "SensorRelay") == 0)
        return "/init/sensor.rc";
    if (strcmp(role, "SimulatorTest") == 0)
        return "/init/simtest.rc";
    return "/init/queen.rc";
}

/*
 * Boot phases:
 * 1) detect_role() determines CohRole.
 * 2) Write role to /srv/cohrole and log boot information.
 * 3) assign_caps() sets capability slots per role.
 * 4) Execute role-specific init script via rc.
 */
int main(void) {
    signal(SIGALRM, watchdog_handler);
    alarm(BOOT_WATCHDOG_SECS);

    const char *role = detect_role();
    FILE *f;
    char srv_path[] = PATH_COHROLE;
    f = fopen(srv_path, "w");
    if (f) {
        fprintf(f, "%s", role);
        fclose(f);
    }

    f = fopen(PATH_BOOT_LOG, "a");
    if (f) {
        fprintf(f, "%ld, %s, %d\n", (long)time(NULL), role,
                boot_trampoline_crc_ok);
        fclose(f);
    }

    assign_caps(role);

    const char *script = script_for_role(role);
    alarm(0); /* boot init succeeded */
    const char *argv[] = {"/bin/rc", script, NULL};
    execv(argv[0], (char *const *)argv);
    perror("execv rc");
    return 1;
}
