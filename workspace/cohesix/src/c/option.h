// CLASSIFICATION: COMMUNITY
// Filename: option.h v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31
// SPDX-License-Identifier: MIT

#ifndef COH_OPTION_H
#define COH_OPTION_H

#include <stddef.h>

#define OPTION_DEFINE(type, name)                                             \
    typedef struct {                                                          \
        int is_some;                                                          \
        type value;                                                           \
    } name;                                                                  \
    static inline name name##_some(type v) { return (name){1, v}; }            \
    static inline name name##_none(void) { return (name){0}; }                \
    static inline int name##_is_some(const name *o) { return o->is_some; }    \
    static inline type name##_unwrap(const name *o) { return o->value; }

#endif /* COH_OPTION_H */
