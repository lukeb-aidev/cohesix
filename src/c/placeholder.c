// CLASSIFICATION: COMMUNITY
// Filename: placeholder.c v0.2
// Author: Lukas Bower
// Date Modified: 2026-12-31
// SPDX-License-Identifier: MIT

#include "option.h"

OPTION_DEFINE(int, IntOption);

int placeholder() {
    IntOption opt = IntOption_some(1);
    if (IntOption_is_some(&opt)) {
        return IntOption_unwrap(&opt);
    }
    return 0;
}
