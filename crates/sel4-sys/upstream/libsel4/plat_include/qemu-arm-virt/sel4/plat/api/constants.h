/* Author: Lukas Bower */
/*
 * Copyright 2020, Data61, CSIRO (ABN 41 687 119 230)
 *
 * SPDX-License-Identifier: BSD-2-Clause
 */

/* The QEMU virt platform can emulate various cores */

#if defined(CONFIG_ARM_CORTEX_A53)
#include <sel4/arch/constants_cortex_a53.h>
#elif defined(CONFIG_ARM_CORTEX_A35)
#include <sel4/arch/constants_cortex_a35.h>
#elif defined(CONFIG_ARM_CORTEX_A72)
#include <sel4/arch/constants_cortex_a72.h>
#elif defined(CONFIG_ARM_CORTEX_A57)
#include <sel4/arch/constants_cortex_a57.h>
#elif defined(CONFIG_ARM_CORTEX_A55)
#include <sel4/arch/constants_cortex_a55.h>
#elif defined(CONFIG_ARM_CORTEX_A15)
#include <sel4/arch/constants_cortex_a15.h>
#elif defined(CONFIG_ARM_CORTEX_A9)
#include <sel4/arch/constants_cortex_a9.h>
#elif defined(CONFIG_ARM_CORTEX_A8)
#include <sel4/arch/constants_cortex_a8.h>
#elif defined(CONFIG_ARM_CORTEX_A7)
#include <sel4/arch/constants_cortex_a7.h>
#else
#error "Unsupported QEMU ARM virt core configuration"
#endif
