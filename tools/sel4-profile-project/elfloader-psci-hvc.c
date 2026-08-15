/*
 * Copyright 2020, Data61, CSIRO (ABN 41 687 119 230)
 *
 * SPDX-License-Identifier: GPL-2.0-only
 */

/*
 * Copyright 2026 Lukas Bower
 *
 * Author: Lukas Bower
 * Purpose: Start QEMU HVF secondary CPUs through the DTB-selected PSCI conduit.
 * Cohesix modifications: Add DTB-selected HVC dispatch while retaining SMC.
 */

#include <autoconf.h>
#include <elfloader/gen_config.h>
#include <elfloader_common.h>
#include <devices_gen.h>
#include <drivers/common.h>
#include <drivers/smp.h>
#include <armv/machine.h>
#include <psci.h>
#include <armv/smp.h>

#include <printf.h>
#include <types.h>

#ifdef CONFIG_ARCH_AARCH64
#define PSCI_FID_CPU_ON 0xc4000003u
#else
#error "The Cohesix QEMU HVF PSCI conduit requires AArch64"
#endif

extern int cohesix_psci_hvc(unsigned int id, unsigned long param1,
                            unsigned long param2, unsigned long param3);

static int cohesix_smp_psci_cpu_on(UNUSED struct elfloader_device *dev,
                                   struct elfloader_cpu *cpu, void *entry,
                                   void *stack)
{
#if CONFIG_MAX_NUM_NODES > 1
    int ret;

    secondary_data.entry = entry;
    secondary_data.stack = stack;
    dmb();
    if (cpu->extra_data == PSCI_METHOD_HVC) {
        ret = cohesix_psci_hvc(PSCI_FID_CPU_ON, cpu->cpu_id,
                               (unsigned long)&secondary_startup, 0);
    } else {
        ret = psci_cpu_on(cpu->cpu_id, (unsigned long)&secondary_startup, 0);
    }
    if (ret != PSCI_SUCCESS) {
        printf("Failed PSCI CPU_ON for core 0x%x with conduit %s: %d\n",
               cpu->cpu_id,
               cpu->extra_data == PSCI_METHOD_HVC ? "hvc" : "smc", ret);
        return -1;
    }
    return 0;
#else
    return -1;
#endif
}

static int cohesix_smp_psci_init(struct elfloader_device *dev,
                                 UNUSED void *match_data)
{
    smp_register_handler(dev);
    return 0;
}

static const struct dtb_match_table cohesix_smp_psci_matches[] = {
    { .compatible = "arm,psci-0.2" },
    { .compatible = "arm,psci-1.0" },
    { .compatible = NULL },
};

static const struct elfloader_smp_ops cohesix_smp_psci_ops = {
    .enable_method = "psci",
    .cpu_on = &cohesix_smp_psci_cpu_on,
};

static const struct elfloader_driver cohesix_smp_psci = {
    .match_table = cohesix_smp_psci_matches,
    .type = DRIVER_SMP,
    .init = &cohesix_smp_psci_init,
    .ops = &cohesix_smp_psci_ops,
};

ELFLOADER_DRIVER(cohesix_smp_psci);
