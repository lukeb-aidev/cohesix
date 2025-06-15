// CLASSIFICATION: COMMUNITY
// Filename: bootloader.c v0.6
// Author: Lukas Bower
// Date Modified: 2025-07-31
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
#include <fcntl.h>
#include <sys/stat.h>
#include "boot_trampoline.h"

#define COH_BOOT_ROLE_BUF       32
#define COH_BOOT_WATCHDOG_SECS  15
#define COH_PATH_COHROLE        "/srv/cohrole"
#define COH_PATH_BOOT_LOG       "/log/bootloader_init.log"
#define COH_PATH_BOOT_ERROR     "/state/boot_error"

extern int boot_trampoline_crc_ok;

static void watchdog_handler(int sig)
{
    (void)sig;
    FILE *f = fopen(COH_PATH_BOOT_ERROR, "a");
    if (f) {
        fprintf(f, "%ld watchdog timeout\n", (long)time(NULL));
        fclose(f);
    }
    _exit(1);
}

static const char *detect_role(void) {
    const char *role = getenv("cohrole");
    static char buf[COH_BOOT_ROLE_BUF];
    FILE *f = NULL;

    if (role && *role)
        return role;

    f = fopen(COH_PATH_COHROLE, "r");
    if (f) {
        if (fgets(buf, sizeof(buf), f)) {
            buf[strcspn(buf, "\r\n")] = '\0';
            fclose(f);
            if (strcmp(buf, "QueenPrimary") == 0 ||
                strcmp(buf, "KioskInteractive") == 0 ||
                strcmp(buf, "InteractiveAIBooth") == 0 ||
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

static void emit_console(const char *msg)
{
    int fd = open("/dev/console", O_WRONLY);
    if (fd >= 0) {
        write(fd, msg, strlen(msg));
        write(fd, "\n", 1);
        close(fd);
    }
}

static void boot_success(void)
{
    emit_console("BOOT_OK");
    int fd = open(COH_BOOT_SUCCESS_PATH, O_WRONLY | O_CREAT, 0644);
    if (fd >= 0) {
        write(fd, "ok\n", 3);
        close(fd);
    }
}

static void boot_fail(const char *reason)
{
    char buf[64];
    snprintf(buf, sizeof(buf), "BOOT_FAIL:%s", reason);
    emit_console(buf);
}

static const char *script_for_role(const char *role) {
    if (strcmp(role, "DroneWorker") == 0)
        return "/init/worker.rc";
    if (strcmp(role, "KioskInteractive") == 0)
        return "/init/kiosk.rc";
    if (strcmp(role, "InteractiveAIBooth") == 0)
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
    alarm(COH_BOOT_WATCHDOG_SECS);

    const char *role = detect_role();
    FILE *f;
    char srv_path[] = COH_PATH_COHROLE;
    f = fopen(srv_path, "w");
    if (f) {
        fprintf(f, "%s", role);
        fclose(f);
    }

    f = fopen(COH_PATH_BOOT_LOG, "a");
    if (f) {
        fprintf(f, "%ld, %s, %d\n", (long)time(NULL), role,
                boot_trampoline_crc_ok);
        fclose(f);
    }

    assign_caps(role);

    if (access("/srv/validator/live.sock", F_OK) == 0)
        boot_success();
    else
        boot_fail("validator_missing");

    const char *script = script_for_role(role);
    alarm(0); /* boot init succeeded */
    const char *argv[] = {"/bin/rc", script, NULL};
    execv(argv[0], (char *const *)argv);
    perror("execv rc");
    return 1;
}
