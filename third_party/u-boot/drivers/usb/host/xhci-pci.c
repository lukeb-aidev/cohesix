// SPDX-License-Identifier: GPL-2.0
/*
 * Copyright 2026 Lukas Bower
 * Purpose: Bring up PCI-backed xHCI controllers and export the active BAR for Cohesix handoff.
 * Author: Lukas Bower
 */
/*
 * Copyright (c) 2015, Google, Inc
 * Written by Simon Glass <sjg@chromium.org>
 * All rights reserved.
 */

#include <dm.h>
#include <dm/device_compat.h>
#include <env.h>
#include <init.h>
#include <log.h>
#include <mapmem.h>
#include <pci.h>
#include <reset.h>
#include <usb.h>
#include <usb/xhci.h>

#ifndef PCI_COMMAND_INTX_DISABLE
#define PCI_COMMAND_INTX_DISABLE	0x0400
#endif

struct xhci_pci_plat {
	struct reset_ctl reset;
};

static u16 xhci_pci_configure_command(struct udevice *dev)
{
	u16 cmd;

	dm_pci_read_config16(dev, PCI_COMMAND, &cmd);
	cmd |= PCI_COMMAND_MEMORY | PCI_COMMAND_MASTER | PCI_COMMAND_INTX_DISABLE;
	dm_pci_write_config16(dev, PCI_COMMAND, cmd);
	dm_pci_read_config16(dev, PCI_COMMAND, &cmd);
	env_set_hex("coh_xhci_pci_cmd", cmd);

	return cmd;
}

static ulong xhci_pci_bar0_addr(struct udevice *dev)
{
	u32 bar0, bar1 = 0;
	u64 base;

	dm_pci_read_config32(dev, PCI_BASE_ADDRESS_0, &bar0);
	if (bar0 == 0xffffffff || (bar0 & PCI_BASE_ADDRESS_SPACE_IO))
		return 0;

	base = bar0 & PCI_BASE_ADDRESS_MEM_MASK;
	if ((bar0 & PCI_BASE_ADDRESS_MEM_TYPE_MASK) == PCI_BASE_ADDRESS_MEM_TYPE_64) {
		dm_pci_read_config32(dev, PCI_BASE_ADDRESS_1, &bar1);
		base |= (u64)bar1 << 32;
	}

	return (ulong)base;
}

static ulong xhci_pci_bar0_phys(struct xhci_hccr *hccr)
{
	return (ulong)map_to_sysmem(hccr);
}

static int xhci_pci_init(struct udevice *dev, struct xhci_hccr **ret_hccr,
			 struct xhci_hcor **ret_hcor)
{
	struct xhci_hccr *hccr;
	struct xhci_hcor *hcor;
	ulong bar0_addr;
	ulong bar0_phys;
	u16 cmd;

	hccr = (struct xhci_hccr *)dm_pci_map_bar(dev,
			PCI_BASE_ADDRESS_0, 0, 0, PCI_REGION_TYPE,
			PCI_REGION_MEM);
	if (!hccr) {
		printf("xhci-pci init cannot map PCI mem bar\n");
		return -EIO;
	}

	hcor = (struct xhci_hcor *)((uintptr_t) hccr +
			HC_LENGTH(xhci_readl(&hccr->cr_capbase)));

	debug("XHCI-PCI init hccr %p and hcor %p hc_length %d\n",
	      hccr, hcor, (u32)HC_LENGTH(xhci_readl(&hccr->cr_capbase)));

	bar0_addr = xhci_pci_bar0_addr(dev);
	bar0_phys = xhci_pci_bar0_phys(hccr);
	if (bar0_addr)
		env_set_hex("coh_xhci_mmio_raw", bar0_addr);
	if (bar0_phys) {
		if (!env_set_hex("coh_xhci_mmio", bar0_phys))
			debug("XHCI-PCI exported Cohesix BAR0 raw=%lx phys=%lx\n",
			      bar0_addr, bar0_phys);
	}

	*ret_hccr = hccr;
	*ret_hcor = hcor;

	/*
	 * Cohesix consumes the BAR again after U-Boot's `usb stop`, so keep
	 * memory decoding and bus mastering enabled while masking INTx. U-Boot
	 * polls xHCI and does not rely on legacy PCI interrupts here.
	 */
	cmd = xhci_pci_configure_command(dev);
	debug("XHCI-PCI command configured for Cohesix handoff: %x\n", cmd);
	return 0;
}

static int xhci_pci_probe(struct udevice *dev)
{
	struct xhci_pci_plat *plat = dev_get_plat(dev);
	struct xhci_hccr *hccr;
	struct xhci_hcor *hcor;
	int ret;

	ret = reset_get_by_index(dev, 0, &plat->reset);
	if (ret && ret != -ENOENT && ret != -ENOTSUPP) {
		dev_err(dev, "failed to get reset\n");
		return ret;
	}

	if (reset_valid(&plat->reset)) {
		ret = reset_assert(&plat->reset);
		if (ret)
			goto err_reset;

		ret = reset_deassert(&plat->reset);
		if (ret)
			goto err_reset;
	}

	ret = xhci_pci_init(dev, &hccr, &hcor);
	if (ret)
		goto err_reset;

	ret = xhci_register(dev, hccr, hcor);
	if (ret)
		goto err_reset;

	return 0;

err_reset:
	if (reset_valid(&plat->reset))
		reset_free(&plat->reset);

	return ret;
}

static int xhci_pci_remove(struct udevice *dev)
{
	struct xhci_pci_plat *plat = dev_get_plat(dev);
	u16 cmd;

	xhci_deregister(dev);
	cmd = xhci_pci_configure_command(dev);
	debug("XHCI-PCI stop preserved command bits for Cohesix handoff: %x\n",
	      cmd);
	if (reset_valid(&plat->reset))
		reset_free(&plat->reset);

	return 0;
}

static const struct udevice_id xhci_pci_ids[] = {
	{ .compatible = "xhci-pci" },
	{ }
};

U_BOOT_DRIVER(xhci_pci) = {
	.name	= "xhci_pci",
	.id	= UCLASS_USB,
	.probe = xhci_pci_probe,
	.remove	= xhci_pci_remove,
	.of_match = xhci_pci_ids,
	.ops	= &xhci_usb_ops,
	.plat_auto	= sizeof(struct xhci_pci_plat),
	.priv_auto	= sizeof(struct xhci_ctrl),
	.flags	= DM_FLAG_OS_PREPARE | DM_FLAG_ALLOC_PRIV_DMA,
};

static struct pci_device_id xhci_pci_supported[] = {
	{ PCI_DEVICE_CLASS(PCI_CLASS_SERIAL_USB_XHCI, ~0) },
	{},
};

U_BOOT_PCI_DEVICE(xhci_pci, xhci_pci_supported);
