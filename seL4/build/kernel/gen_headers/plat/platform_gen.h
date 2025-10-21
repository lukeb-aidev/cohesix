/*
 * Copyright 2020, Data61, CSIRO (ABN 41 687 119 230)
 *
 * SPDX-License-Identifier: GPL-2.0-only
 */

#pragma once

#include <config.h>

#define TIMER_CLOCK_HZ ULL_CONST(62500000)
#define CLK_MAGIC 4611686019llu
#define CLK_SHIFT 58u
#define TIMER_PRECISION 0
#define TIMER_OVERHEAD_TICKS 0

enum IRQConstants {
    maxIRQ                      = 159
};

#define IRQ_CNODE_SLOT_BITS (8)

#include <arch/machine/gic_v2.h>
#include <drivers/timer/arm_generic.h>

/* #undef CONFIGURE_SMMU */
#if (defined(CONFIGURE_SMMU) && defined(CONFIG_TK1_SMMU))
#include CONFIGURE_SMMU
#endif

/* #undef CONFIGURE_SMMU */
#if (defined(CONFIGURE_SMMU) && defined(CONFIG_ARM_SMMU))
#include CONFIGURE_SMMU

#define SMMU_MAX_SID  
#define SMMU_MAX_CB  

#endif

#ifdef CONFIG_KERNEL_MCS
static inline CONST time_t getKernelWcetUs(void)
{
    return 10u;
}
#endif
