// CLASSIFICATION: COMMUNITY
// Filename: efistubs.c v0.1
// Author: Lukas Bower
// Date Modified: 2026-09-07
// SPDX-License-Identifier: MIT
#include <stddef.h>

size_t strlen(const char *s) {
    const char *p = s;
    while (*p) p++;
    return (size_t)(p - s);
}

char *strchr(const char *s, int c) {
    while (*s) {
        if (*s == (char)c) return (char *)s;
        s++;
    }
    return c == 0 ? (char *)s : NULL;
}

int snprintf(char *str, size_t size, const char *format, ...) {
    if (size && str) str[0] = '\0';
    (void)format;
    return 0;
}
