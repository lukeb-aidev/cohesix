// CLASSIFICATION: COMMUNITY
// Filename: bootloader.c v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-17
// SPDX-License-Identifier: MIT
//
// Cohesix OS bootloader (seL4 root task)
// Detects role from boot params or /boot/cohrole.txt and
// launches role-specific init script. Writes role to /srv/cohrole.

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static const char *detect_role(void) {
    const char *role = getenv("cohrole");
    static char buf[32];
    FILE *f;

    if (role && *role)
        return role;

    f = fopen("/boot/cohrole.txt", "r");
    if (f) {
        if (fgets(buf, sizeof(buf), f)) {
            buf[strcspn(buf, "\r\n")] = '\0';
            fclose(f);
            return buf;
        }
        fclose(f);
    }

    return "QueenPrimary"; // default
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

int main(void) {
    const char *role = detect_role();
    FILE *f;
    char srv_path[] = "/srv/cohrole";
    f = fopen(srv_path, "w");
    if (f) {
        fprintf(f, "%s", role);
        fclose(f);
    } else {
        perror("open /srv/cohrole");
    }

    const char *script = script_for_role(role);
    const char *argv[] = {"/bin/rc", script, NULL};
    execv(argv[0], (char *const *)argv);
    perror("execv rc");
    return 1;
}
