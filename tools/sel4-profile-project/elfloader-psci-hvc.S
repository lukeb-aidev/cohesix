/*
 * Copyright 2026 Lukas Bower
 *
 * SPDX-License-Identifier: GPL-2.0-only
 *
 * Author: Lukas Bower
 * Purpose: Invoke PSCI through HVC using the standard AArch64 calling convention.
 */

#include <assembler.h>

.text

BEGIN_FUNC(cohesix_psci_hvc)
    hvc #0
    ret
END_FUNC(cohesix_psci_hvc)
