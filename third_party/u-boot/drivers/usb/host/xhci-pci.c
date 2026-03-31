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
#ifndef PCI_MSIX_FLAGS
#define PCI_MSIX_FLAGS		2
#endif
#ifndef PCI_MSIX_FLAGS_MASKALL
#define PCI_MSIX_FLAGS_MASKALL	0x4000
#endif
#ifndef PCI_MSIX_FLAGS_ENABLE
#define PCI_MSIX_FLAGS_ENABLE	0x8000
#endif

struct xhci_pci_plat {
	struct reset_ctl reset;
};

static const char *xhci_pci_env_or_absent(const char *name)
{
	const char *value = env_get(name);

	return value ? value : "absent";
}

static void xhci_pci_emit_breadcrumb(struct udevice *dev, const char *stage,
					 int ret)
{
	pci_dev_t bdf = dm_pci_get_bdf(dev);

	printf("[cohesix:xhci-pci] stage=%s bdf=%02x:%02x.%x mmio_raw=%s mmio=%s cmd=%s usbcmd=%s usbsts=%s iman0=%s ready=%s irq=%s halted=%s safe=%s ret=%d\n",
	       stage, PCI_BUS(bdf), PCI_DEV(bdf), PCI_FUNC(bdf),
	       xhci_pci_env_or_absent("coh_xhci_mmio_raw"),
	       xhci_pci_env_or_absent("coh_xhci_mmio"),
	       xhci_pci_env_or_absent("coh_xhci_pci_cmd"),
	       xhci_pci_env_or_absent("coh_xhci_usbcmd"),
	       xhci_pci_env_or_absent("coh_xhci_usbsts"),
	       xhci_pci_env_or_absent("coh_xhci_iman0"),
	       xhci_pci_env_or_absent("coh_xhci_handoff_ready"),
	       xhci_pci_env_or_absent("coh_xhci_irq_quiesced"),
	       xhci_pci_env_or_absent("coh_xhci_halted"),
	       xhci_pci_env_or_absent("coh_xhci_handoff_safe"), ret);
}

static void xhci_pci_export_handoff_ready(int ready)
{
	env_set("coh_xhci_handoff_ready", ready ? "1" : NULL);
}

static void xhci_pci_export_irq_quiesced(int ready)
{
	env_set("coh_xhci_irq_quiesced", ready ? "1" : NULL);
}

static void xhci_pci_export_halted(int halted)
{
	env_set("coh_xhci_halted", halted ? "1" : NULL);
}

static void xhci_pci_export_handoff_safe(int safe)
{
	env_set("coh_xhci_handoff_safe", safe ? "1" : NULL);
}

static void xhci_pci_export_capability_snapshot(struct xhci_hccr *hccr)
{
	u32 capbase = xhci_readl(&hccr->cr_capbase);

	env_set_hex("coh_xhci_cap_length", HC_LENGTH(capbase));
	env_set_hex("coh_xhci_hci_version", HC_VERSION(capbase));
	env_set_hex("coh_xhci_hcs1", xhci_readl(&hccr->cr_hcsparams1));
	env_set_hex("coh_xhci_hcs2", xhci_readl(&hccr->cr_hcsparams2));
	env_set_hex("coh_xhci_hccparams1", xhci_readl(&hccr->cr_hccparams));
	env_set_hex("coh_xhci_dboff", xhci_readl(&hccr->cr_dboff));
	env_set_hex("coh_xhci_rtsoff", xhci_readl(&hccr->cr_rtsoff));
}

static int xhci_pci_map_runtime_regs(struct udevice *dev, struct xhci_hccr **ret_hccr,
				     struct xhci_hcor **ret_hcor)
{
	struct xhci_hccr *hccr;
	struct xhci_hcor *hcor;

	hccr = (struct xhci_hccr *)dm_pci_map_bar(dev, PCI_BASE_ADDRESS_0, 0, 0,
						  PCI_REGION_TYPE, PCI_REGION_MEM);
	if (!hccr)
		return -EIO;

	hcor = (struct xhci_hcor *)((uintptr_t)hccr +
			HC_LENGTH(xhci_readl(&hccr->cr_capbase)));
	*ret_hccr = hccr;
	*ret_hcor = hcor;
	return 0;
}

static int xhci_pci_capture_handoff_state(struct udevice *dev, int scrub_irqs,
					  int *ret_safe)
{
	struct xhci_hccr *hccr;
	struct xhci_hcor *hcor;
	struct xhci_run_regs *run_regs;
	struct xhci_intr_reg *ir_set;
	u32 usbcmd;
	u32 usbsts;
	u32 iman0;
	int halted;
	int command_irqs_quiesced;
	int interrupter_quiesced;
	int safe;
	int ret;

	ret = xhci_pci_map_runtime_regs(dev, &hccr, &hcor);
	if (ret)
		return ret;

	run_regs = (struct xhci_run_regs *)((uintptr_t)hccr +
		   (xhci_readl(&hccr->cr_rtsoff) & RTSOFF_MASK));
	ir_set = &run_regs->ir_set[0];

	usbcmd = xhci_readl(&hcor->or_usbcmd);
	if (scrub_irqs) {
		usbcmd &= ~XHCI_IRQS;
		xhci_writel(&hcor->or_usbcmd, usbcmd);
		usbcmd = xhci_readl(&hcor->or_usbcmd);
	}
	env_set_hex("coh_xhci_usbcmd", usbcmd);

	iman0 = xhci_readl(&ir_set->irq_pending);
	if (scrub_irqs) {
		iman0 = ER_IRQ_DISABLE(iman0);
		xhci_writel(&ir_set->irq_pending, iman0);
		iman0 = xhci_readl(&ir_set->irq_pending);
	}
	env_set_hex("coh_xhci_iman0", iman0);

	usbsts = xhci_readl(&hcor->or_usbsts);
	env_set_hex("coh_xhci_usbsts", usbsts);

	halted = !!(usbsts & STS_HALT);
	command_irqs_quiesced = !(usbcmd & XHCI_IRQS);
	interrupter_quiesced = !(iman0 & 0x2);
	safe = halted && command_irqs_quiesced && interrupter_quiesced;

	xhci_pci_export_halted(halted);
	xhci_pci_export_handoff_safe(safe);
	if (ret_safe)
		*ret_safe = safe;

	return 0;
}

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

static int xhci_pci_quiesce_interrupt_modes(struct udevice *dev)
{
	int cap;
	u16 flags;
	int ret;

	xhci_pci_export_irq_quiesced(0);

	cap = dm_pci_find_capability(dev, PCI_CAP_ID_MSI);
	if (cap > 0) {
		ret = dm_pci_read_config16(dev, cap + PCI_MSI_FLAGS, &flags);
		if (ret)
			return ret;
		flags &= ~PCI_MSI_FLAGS_ENABLE;
		ret = dm_pci_write_config16(dev, cap + PCI_MSI_FLAGS, flags);
		if (ret)
			return ret;
		ret = dm_pci_read_config16(dev, cap + PCI_MSI_FLAGS, &flags);
		if (ret)
			return ret;
		if (flags & PCI_MSI_FLAGS_ENABLE)
			return -EIO;
	}

	cap = dm_pci_find_capability(dev, PCI_CAP_ID_MSIX);
	if (cap > 0) {
		ret = dm_pci_read_config16(dev, cap + PCI_MSIX_FLAGS, &flags);
		if (ret)
			return ret;
		flags = (flags | PCI_MSIX_FLAGS_MASKALL) & ~PCI_MSIX_FLAGS_ENABLE;
		ret = dm_pci_write_config16(dev, cap + PCI_MSIX_FLAGS, flags);
		if (ret)
			return ret;
		ret = dm_pci_read_config16(dev, cap + PCI_MSIX_FLAGS, &flags);
		if (ret)
			return ret;
		if (flags & PCI_MSIX_FLAGS_ENABLE)
			return -EIO;
	}

	xhci_pci_export_irq_quiesced(1);
	return 0;
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
	int state_safe;

	if (xhci_pci_map_runtime_regs(dev, &hccr, &hcor)) {
		printf("xhci-pci init cannot map PCI mem bar\n");
		return -EIO;
	}

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
	xhci_pci_export_capability_snapshot(hccr);

	*ret_hccr = hccr;
	*ret_hcor = hcor;

	/*
	 * Cohesix consumes the BAR again after U-Boot's `usb stop`, so keep
	 * memory decoding and bus mastering enabled while masking INTx. U-Boot
	 * polls xHCI and does not rely on legacy PCI interrupts here.
	 */
	cmd = xhci_pci_configure_command(dev);
	debug("XHCI-PCI command configured for Cohesix handoff: %x\n", cmd);
	if (!xhci_pci_capture_handoff_state(dev, 0, &state_safe))
		xhci_pci_export_handoff_safe(state_safe);
	xhci_pci_emit_breadcrumb(dev, "init", 0);
	return 0;
}

static int xhci_pci_probe(struct udevice *dev)
{
	struct xhci_pci_plat *plat = dev_get_plat(dev);
	struct xhci_hccr *hccr;
	struct xhci_hcor *hcor;
	const char *fail_stage = "probe-entry";
	int ret;

	xhci_pci_export_handoff_ready(0);
	xhci_pci_export_irq_quiesced(0);
	xhci_pci_export_halted(0);
	xhci_pci_export_handoff_safe(0);
	xhci_pci_emit_breadcrumb(dev, "probe-entry", 0);

	fail_stage = "reset-get";
	ret = reset_get_by_index(dev, 0, &plat->reset);
	if (ret && ret != -ENOENT && ret != -ENOTSUPP) {
		dev_err(dev, "failed to get reset\n");
		xhci_pci_emit_breadcrumb(dev, fail_stage, ret);
		return ret;
	}

	if (reset_valid(&plat->reset)) {
		fail_stage = "reset-assert";
		ret = reset_assert(&plat->reset);
		if (ret)
			goto err_reset;

		fail_stage = "reset-deassert";
		ret = reset_deassert(&plat->reset);
		if (ret)
			goto err_reset;
	}

	fail_stage = "init";
	ret = xhci_pci_init(dev, &hccr, &hcor);
	if (ret)
		goto err_reset;

	fail_stage = "register";
	ret = xhci_register(dev, hccr, hcor);
	if (ret)
		goto err_reset;

	fail_stage = "irq-quiesce";
	ret = xhci_pci_quiesce_interrupt_modes(dev);
	if (ret)
		goto err_reset;

	xhci_pci_export_handoff_ready(1);
	xhci_pci_emit_breadcrumb(dev, "probe-ready", 0);
	return 0;

err_reset:
	xhci_pci_emit_breadcrumb(dev, fail_stage, ret);
	if (reset_valid(&plat->reset))
		reset_free(&plat->reset);

	return ret;
}

static int xhci_pci_remove(struct udevice *dev)
{
	struct xhci_pci_plat *plat = dev_get_plat(dev);
	u16 cmd;
	int safe;
	int ret;

	xhci_pci_emit_breadcrumb(dev, "remove-entry", 0);
	xhci_pci_export_handoff_ready(0);
	xhci_pci_export_irq_quiesced(0);
	xhci_pci_export_halted(0);
	xhci_pci_export_handoff_safe(0);
	ret = xhci_deregister(dev);
	if (ret) {
		xhci_pci_emit_breadcrumb(dev, "remove-deregister", ret);
		if (reset_valid(&plat->reset))
			reset_free(&plat->reset);
		return ret;
	}
	cmd = xhci_pci_configure_command(dev);
	debug("XHCI-PCI stop preserved command bits for Cohesix handoff: %x\n",
	      cmd);
	ret = xhci_pci_quiesce_interrupt_modes(dev);
	if (ret) {
		xhci_pci_emit_breadcrumb(dev, "remove-irq-quiesce", ret);
		if (reset_valid(&plat->reset))
			reset_free(&plat->reset);
		return ret;
	}
	ret = xhci_pci_capture_handoff_state(dev, 1, &safe);
	if (ret) {
		xhci_pci_emit_breadcrumb(dev, "remove-state-snapshot", ret);
		if (reset_valid(&plat->reset))
			reset_free(&plat->reset);
		return ret;
	}
	if (!safe) {
		xhci_pci_emit_breadcrumb(dev, "remove-handoff-unsafe", -EIO);
		if (reset_valid(&plat->reset))
			reset_free(&plat->reset);
		return -EIO;
	}
	xhci_pci_export_handoff_ready(1);
	xhci_pci_emit_breadcrumb(dev, "remove-ready", 0);
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
