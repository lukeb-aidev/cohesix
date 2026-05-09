// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
use crate::{
    Dma, Result, UsbError, reg,
    ring::{EventRing, PhysMem, Ring, Trb, completion, trb_type},
};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{
    hint::spin_loop,
    sync::atomic::{Ordering, compiler_fence},
};
use spin::Mutex;

const MMIO_INIT_SIZE: usize = 0x1000;
const MMIO_MAX_SIZE: usize = 0x20_0000;
const CMD_RING_SIZE: usize = 256;
const EVENT_RING_SIZE: usize = 256;
const STOP_WAIT_SPINS: usize = 10_000_000;
const RESET_WAIT_SPINS: usize = 10_000_000;
// On Pi 4 mailbox-reset handoff, the first live USBSTS read or USBCMD reset
// write can race VL805 while firmware is still finishing the reset boundary.
// Keep a short blind settle before the first live reset/CNR touch on those
// paths, including the weaker stop-state-only snapshot.
const MAILBOX_RESET_POST_SETTLE_SPINS: usize = 1_000_000;
const READY_WAIT_SPINS: usize = 10_000_000;
const READY_WAIT_PROGRESS_SPINS: usize = 1_000_000;
const COMMAND_WAIT_SPINS: usize = 20_000_000;
const COMMAND_POLL_ONLY_WAIT_SPINS: usize = 64;
const COMMAND_PROMPT_SAFE_WAIT_POLLS: usize = 4;
const COMMAND_WAIT_LIVE_SNAPSHOT_SPINS: usize = 32;
const COMMAND_WAIT_OTHER_EVENT_LOGS: usize = 8;
const COMMAND_EVENT_RING_CPU_SYNC_INTERVAL_SPINS: usize = 1_000_000;
const COMMAND_EVENT_RING_DEBUG_TRBS: usize = 4;
const COMMAND_RING_DEBUG_TRBS: usize = 4;
const LINUX_COMMAND_PROBE_IMOD: u32 = 0x0000_00a0;
const LINUX_COMMAND_PROBE_USBCMD: u32 = reg::USBCMD_RUN | reg::USBCMD_INTE;
const PORT_RESET_WAIT_SPINS: usize = 10_000_000;
const PORT_ENABLE_WAIT_SPINS: usize = 10_000_000;
const PORT_SETTLE_SPINS: usize = 100_000;
const PORT_POST_ACK_WAIT_SPINS: usize = 1_000_000;
const PORT_POST_ACK_TRANSITION_LOGS: usize = 8;
const DROP_HALT_WAIT_SPINS: usize = 1_000_000;
const CONFIG_MAX_SLOTS_MASK: u32 = 0xff;
const XHCI_DBOFF_MASK: u32 = !0x3;
const XHCI_RTSOFF_MASK: u32 = !0x1f;
// Pi 4 now reliably reaches trusted-handoff ring programming. At that point the
// DCBAAP prewrite read and zero rewrite are just ownership probes, and the
// board-freezing edge was the experimental atomic write64 publish. Keep the
// trusted-handoff path on the standard low / high dword sequence only and use
// later xHCI breadcrumbs to judge success/failure.
const TRUSTED_HANDOFF_DCBAAP_PREWRITE_READ_PROBE: bool = false;
const TRUSTED_HANDOFF_DCBAAP_ZERO_REWRITE_PROBE: bool = false;
const USBSTS_CLEAR_MASK: u32 =
    reg::USBSTS_EINT | reg::USBSTS_PCD | reg::USBSTS_HSE | reg::USBSTS_HCE;
const USBCMD_INTERRUPT_DELIVERY_MASK: u32 = reg::USBCMD_INTE | reg::USBCMD_HSEE;
const USBLEGACY_BIOS_OWNED: u32 = 1 << 16;
const USBLEGACY_OS_OWNED: u32 = 1 << 24;
const XHCI_LEGACY_CONTROL_OFFSET: usize = 0x04;
const XHCI_LEGACY_DISABLE_SMI: u32 = (0x7 << 1) + (0xff << 5) + (0x7 << 17);
const XHCI_LEGACY_SMI_EVENTS: u32 = 0x7 << 29;
const EXT_CAP_SCAN_LIMIT: usize = 64;
const BRCM_XHCI_AXIWRA: usize = 0xC08;
const BRCM_XHCI_AXIRDA: usize = 0xC0C;
const BRCM_XHCI_USBAXI_CACHE: u32 = 0xF;
const BRCM_XHCI_USBAXI_PROT: u32 = 0x8;
const BRCM_XHCI_USBAXI_SA_MASK: u32 = 0x1FF;
const BRCM_XHCI_USBAXI_UA_MASK: u32 = 0x1FF << 16;
const BRCM_XHCI_USBAXI_SA_VAL: u32 = (BRCM_XHCI_USBAXI_CACHE << 4) | BRCM_XHCI_USBAXI_PROT;
const BRCM_XHCI_USBAXI_UA_VAL: u32 = BRCM_XHCI_USBAXI_SA_VAL << 16;
const BRCM_XHCI_USBAXI_SA_UA_MASK: u32 = BRCM_XHCI_USBAXI_UA_MASK | BRCM_XHCI_USBAXI_SA_MASK;
const BRCM_XHCI_USBAXI_SA_UA_VAL: u32 = BRCM_XHCI_USBAXI_UA_VAL | BRCM_XHCI_USBAXI_SA_VAL;
// Perform an explicit host controller reset after stop so ring/DCBAA
// programming starts from a deterministic post-firmware baseline on generic
// xHCI bring-up paths.
const SKIP_HCRST_DURING_INIT: bool = false;
// Stop the controller before ring/DCBAA programming so command state latches
// deterministically across firmware handoff states.
const SKIP_STOP_DURING_INIT: bool = false;
const MAX_REASONABLE_SLOTS: u8 = 255;
const MAX_REASONABLE_PORTS: u8 = 255;
// Pi4/VL805 uses a small scratchpad count; very large values are usually
// bogus capability reads from an incorrect MMIO candidate.
const MAX_REASONABLE_SCRATCHPAD: u16 = 256;
const PORT_CHANGE_BITS: u32 = reg::PORTSC_CSC
    | reg::PORTSC_PEC
    | reg::PORTSC_WRC
    | reg::PORTSC_OCC
    | reg::PORTSC_PRC
    | reg::PORTSC_PLC
    | reg::PORTSC_CEC;
const POST_START_POLLING_IRQ_QUIESCE_RETRY_SPINS: usize = 32;

/// Build a command-ring No Op TRB for bounded controller liveness probes.
pub const fn no_op_command_trb_for_probe() -> Trb {
    Trb {
        param: 0,
        status: 0,
        control: trb_type::NO_OP_CMD << 10,
    }
}

#[inline]
const fn command_wait_should_sync_event_ring(waited: usize) -> bool {
    waited == 0 || waited % COMMAND_EVENT_RING_CPU_SYNC_INTERVAL_SPINS == 0
}

#[inline]
const fn command_poll_only_should_sync_event_ring(_waited: usize) -> bool {
    true
}

// Keep only bits that are safe to mirror back across maintenance writes.
// PORTSC_PED must not be mirrored: on some USB2 paths it is RW1CS and writing
// a sampled 1 can clear the enabled state immediately after reset/ACK.
const PORTSC_NEUTRAL_MASK: u32 = reg::PORTSC_CCS
    | reg::PORTSC_OCA
    | reg::PORTSC_PLS_MASK
    | reg::PORTSC_PP
    | reg::PORTSC_SPEED_MASK
    | reg::PORTSC_PIC_MASK
    | reg::PORTSC_CAS
    | reg::PORTSC_WCE
    | reg::PORTSC_WDE
    | reg::PORTSC_WOE
    | reg::PORTSC_DR;

const RUN_WAIT_OBSERVABLE_USBSTS_MASK: u32 = reg::USBSTS_HCH
    | reg::USBSTS_CNR
    | reg::USBSTS_HSE
    | reg::USBSTS_HCE
    | reg::USBSTS_EINT
    | reg::USBSTS_PCD;

#[inline(always)]
const fn polling_iman_value() -> u32 {
    0
}

#[inline(always)]
const fn polling_iman_ack_value() -> u32 {
    // IMAN.IP is write-one-to-clear. The normal polling path keeps IMAN.IE clear
    // so command proofs observe event production from the event ring, not IRQs.
    reg::IMAN_IP
}

#[inline(always)]
const fn disable_interrupter_iman_value(iman: u32) -> u32 {
    // Linux and U-Boot disable an interrupter by clearing IE without writing
    // IP=1. IP is write-one-to-clear, so keep the pre-DCBAAP handoff from
    // acknowledging an event before runtime owns the event ring.
    iman & !(reg::IMAN_IP | reg::IMAN_IE)
}

#[inline(always)]
const fn masked_usbcmd(usbcmd: u32) -> u32 {
    usbcmd & !USBCMD_INTERRUPT_DELIVERY_MASK
}

#[inline(always)]
const fn linux_command_probe_usbcmd_seed() -> u32 {
    LINUX_COMMAND_PROBE_USBCMD
}

#[inline(always)]
const fn usbcmd_interrupt_delivery_enabled(usbcmd: u32) -> bool {
    (usbcmd & USBCMD_INTERRUPT_DELIVERY_MASK) != 0
}

#[inline(always)]
const fn post_start_polling_irq_quiesce_pending_bits(
    usbcmd: u32,
    usbsts: u32,
    iman: u32,
    skip_usbsts_clear_write: bool,
) -> u32 {
    let _ = (usbsts, iman, skip_usbsts_clear_write);
    usbcmd & (reg::USBCMD_INTE | reg::USBCMD_HSEE)
}

#[inline(always)]
const fn run_usbcmd_snapshot_seed(
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> Option<u32> {
    match runtime_seed_snapshot {
        Some(snapshot) => snapshot.usbcmd,
        None => None,
    }
}

#[inline(always)]
const fn run_usbcmd_needs_live_seed_read(
    preserve_firmware_state: bool,
    live_post_reset_seed_reads: bool,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    live_post_reset_seed_reads
        || (preserve_firmware_state && run_usbcmd_snapshot_seed(runtime_seed_snapshot).is_none())
}

#[inline(always)]
const fn run_usbcmd_prefers_snapshot_seed(
    firmware_handoff: XhciFirmwareHandoff,
    preserve_firmware_state: bool,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    preserve_firmware_state
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn post_start_polling_irq_quiesce_skip_usbsts_clear(preserve_firmware_state: bool) -> bool {
    let _ = preserve_firmware_state;
    true
}

#[inline(always)]
const fn compose_run_usbcmd(current_usbcmd: u32, merge_existing_bits: bool) -> u32 {
    if merge_existing_bits {
        masked_usbcmd(current_usbcmd) | reg::USBCMD_RUN
    } else {
        reg::USBCMD_RUN
    }
}

#[inline(always)]
const fn polling_event_generation_run_usbcmd(
    run_usbcmd: u32,
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> u32 {
    let _ = firmware_handoff;
    let _ = runtime_seed_snapshot;
    run_usbcmd & !reg::USBCMD_INTE
}

#[inline(always)]
const fn polling_event_generation_iman_value(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> u32 {
    let _ = firmware_handoff;
    let _ = runtime_seed_snapshot;
    polling_iman_value()
}

#[inline(always)]
const fn command_timeout_live_snapshot_enabled() -> bool {
    true
}

#[inline(always)]
const fn command_timeout_live_snapshot_spins() -> usize {
    COMMAND_WAIT_LIVE_SNAPSHOT_SPINS
}

#[inline(always)]
const fn polling_command_proof_dnctrl_value(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> u32 {
    let _ = firmware_handoff;
    let _ = runtime_seed_snapshot;
    0
}

#[inline(always)]
const fn halt_revalidation_needed(usbsts: u32) -> bool {
    (usbsts & reg::USBSTS_HCH) == 0
}

#[inline(always)]
const fn run_wait_progress_due(waited: usize) -> bool {
    waited == 1 || (waited % READY_WAIT_PROGRESS_SPINS) == 0
}

#[inline(always)]
const fn run_wait_observable_usbsts(usbsts: u32) -> u32 {
    usbsts & RUN_WAIT_OBSERVABLE_USBSTS_MASK
}

#[inline(always)]
const fn preserve_firmware_handoff_config(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::PreserveControllerState
    ) || matches!(firmware_handoff, XhciFirmwareHandoff::None) && SKIP_HCRST_DURING_INIT
}

#[inline(always)]
const fn skip_preinit_polling_scrub(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ResetlessReinit
            | XhciFirmwareHandoff::PlatformResetComplete
            | XhciFirmwareHandoff::PreserveControllerState
    )
}

#[inline(always)]
const fn skip_constructor_polling_scrub_writes(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ResetlessReinit
            | XhciFirmwareHandoff::PlatformResetComplete
            | XhciFirmwareHandoff::PreserveControllerState
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConstructorPollingScrubMode {
    Full,
    TrustedQuiesceOnly,
}

#[inline(always)]
const fn skip_constructor_polling_scrub_writes_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    skip_constructor_polling_scrub_writes(firmware_handoff)
        || runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn constructor_polling_scrub_mode(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> ConstructorPollingScrubMode {
    if matches!(firmware_handoff, XhciFirmwareHandoff::ColdStartFromSnapshot) {
        let _ = runtime_seed_snapshot;
        // Pi 4 bootloader-trusted cold-start lanes stay completely
        // write-free through constructor scrub and rebuild ownership from
        // reset instead of touching live status state first.
        ConstructorPollingScrubMode::TrustedQuiesceOnly
    } else if skip_constructor_polling_scrub_writes_with_snapshot(
        firmware_handoff,
        runtime_seed_snapshot,
    ) {
        ConstructorPollingScrubMode::TrustedQuiesceOnly
    } else {
        ConstructorPollingScrubMode::Full
    }
}

#[inline(always)]
const fn constructor_polling_scrub_mode_from_params(
    params: XhciControllerParams,
) -> ConstructorPollingScrubMode {
    if params.skip_constructor_live_scrub || params.skip_initial_live_operational_reads {
        ConstructorPollingScrubMode::TrustedQuiesceOnly
    } else {
        constructor_polling_scrub_mode(params.firmware_handoff, params.runtime_seed_snapshot)
    }
}

#[inline(always)]
const fn initial_live_operational_read_hazard(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    skip_initial_live_operational_reads: bool,
) -> bool {
    skip_initial_live_operational_reads
        && matches!(
            firmware_handoff,
            XhciFirmwareHandoff::None | XhciFirmwareHandoff::PlatformResetComplete
        )
        && runtime_seed_snapshot.is_none()
}

#[inline(always)]
const fn skip_init_pre_reset_scrub_writes(firmware_handoff: XhciFirmwareHandoff) -> bool {
    skip_preinit_polling_scrub(firmware_handoff)
}

#[inline(always)]
const fn skip_init_pre_reset_scrub_writes_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    skip_init_pre_reset_scrub_writes(firmware_handoff)
        || runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn skip_init_pre_reset_scrub_writes_for_init(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    skip_initial_live_operational_reads: bool,
) -> bool {
    skip_init_pre_reset_scrub_writes_with_snapshot(firmware_handoff, runtime_seed_snapshot)
        || initial_live_operational_read_hazard(
            firmware_handoff,
            runtime_seed_snapshot,
            skip_initial_live_operational_reads,
        )
}

#[inline(always)]
const fn skip_legacy_ownership_claim_for_handoff(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ColdStartFromSnapshot
            | XhciFirmwareHandoff::ResetlessReinit
            | XhciFirmwareHandoff::PlatformResetComplete
            | XhciFirmwareHandoff::PreserveControllerState
    )
}

#[inline(always)]
const fn skip_legacy_ownership_claim_for_handoff_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    skip_legacy_ownership_claim_for_handoff(firmware_handoff)
        || runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn claim_legacy_ownership_before_reset_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    !skip_legacy_ownership_claim_for_handoff_with_snapshot(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn claim_legacy_ownership_before_reset_for_init(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    skip_initial_live_operational_reads: bool,
) -> bool {
    claim_legacy_ownership_before_reset_with_snapshot(firmware_handoff, runtime_seed_snapshot)
        && !initial_live_operational_read_hazard(
            firmware_handoff,
            runtime_seed_snapshot,
            skip_initial_live_operational_reads,
        )
}

#[inline(always)]
const fn disable_legacy_smi_control_bits(val: u32) -> u32 {
    (val & XHCI_LEGACY_DISABLE_SMI) | XHCI_LEGACY_SMI_EVENTS
}

#[inline(always)]
const fn use_live_post_reset_seed_reads(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ColdStartFromSnapshot | XhciFirmwareHandoff::None
    )
}

#[inline(always)]
const fn use_live_config_seed_reads(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ColdStartFromSnapshot | XhciFirmwareHandoff::None
    )
}

#[inline(always)]
const fn use_live_post_reset_seed_reads_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // The weaker stop-state snapshots now replay the U-Boot-style reset/config
    // sequence again, but they still keep post-reset ring seed reads
    // suppressed. Use zero/snapshot seeds there instead of touching live
    // runtime ring registers before ownership has been rebuilt.
    use_live_post_reset_seed_reads(firmware_handoff)
        && !runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        && !runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
        && !runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot)
}

#[inline(always)]
const fn use_live_post_reset_seed_reads_for_init(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    skip_initial_live_operational_reads: bool,
) -> bool {
    use_live_post_reset_seed_reads_with_snapshot(firmware_handoff, runtime_seed_snapshot)
        && !initial_live_operational_read_hazard(
            firmware_handoff,
            runtime_seed_snapshot,
            skip_initial_live_operational_reads,
        )
}

#[inline(always)]
const fn use_live_config_seed_reads_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Any trusted runtime snapshot has already proven enough controller state
    // for Pi 4 local-seat takeover. A live CONFIG seed read is still a toxic
    // MMIO touch on the weaker stop-state path, so suppress it for every
    // snapshot-backed cold start and only keep it on the fully unseeded path.
    use_live_config_seed_reads(firmware_handoff) && runtime_seed_snapshot.is_none()
}

#[inline(always)]
const fn use_live_config_seed_reads_for_init(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    skip_initial_live_operational_reads: bool,
) -> bool {
    use_live_config_seed_reads_with_snapshot(firmware_handoff, runtime_seed_snapshot)
        && !initial_live_operational_read_hazard(
            firmware_handoff,
            runtime_seed_snapshot,
            skip_initial_live_operational_reads,
        )
}

#[inline(always)]
const fn skip_live_post_reset_verification_readbacks(
    firmware_handoff: XhciFirmwareHandoff,
) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ResetlessReinit | XhciFirmwareHandoff::PlatformResetComplete
    )
}

#[inline(always)]
const fn skip_live_post_reset_verification_readbacks_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    skip_live_post_reset_verification_readbacks(firmware_handoff)
        || runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn skip_live_post_reset_verification_readbacks_for_init(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    skip_initial_live_operational_reads: bool,
) -> bool {
    skip_live_post_reset_verification_readbacks_with_snapshot(
        firmware_handoff,
        runtime_seed_snapshot,
    ) || initial_live_operational_read_hazard(
        firmware_handoff,
        runtime_seed_snapshot,
        skip_initial_live_operational_reads,
    )
}

#[inline(always)]
const fn skip_usbsts_clear_before_run_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    let _ = (firmware_handoff, runtime_seed_snapshot);
    true
}

#[inline(always)]
const fn skip_doorbell_readback_after_ring(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ColdStartFromSnapshot
            | XhciFirmwareHandoff::PlatformResetComplete
            | XhciFirmwareHandoff::PreserveControllerState
            | XhciFirmwareHandoff::ResetlessReinit
    )
}

#[inline(always)]
const fn skip_config_write_during_init(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::PlatformResetComplete | XhciFirmwareHandoff::PreserveControllerState
    )
}

#[inline(always)]
const fn skip_config_write_during_init_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // After the Pi 4 mailbox reset, runtime owns fresh DCBAA/rings and must
    // replay CONFIG.MaxSlotsEn before RUN. The captured Linux controller state
    // has CONFIG=0x20; a skipped write left Cohesix with no command completions.
    if runtime_platform_reset_fresh_rings_handoff(firmware_handoff, runtime_seed_snapshot) {
        return false;
    }
    // Other stop-state lanes keep CONFIG untouched. On those paths, U-Boot's
    // stopped xHCI state has already programmed MaxSlotsEn, while runtime does
    // not have enough ownership to republish fresh rings.
    snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
        || skip_config_write_during_init(firmware_handoff)
}

#[inline(always)]
const fn skip_reset_during_init(firmware_handoff: XhciFirmwareHandoff) -> bool {
    // Resetless and preserve-state handoff modes are only safe when firmware
    // has already proven the controller halted and interrupt-quiesced.
    // PlatformResetComplete proves the Pi mailbox/VL805 reset boundary, but
    // fresh Cohesix-owned rings still need a local xHC HCRST before command
    // doorbells can be trusted.
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ResetlessReinit | XhciFirmwareHandoff::PreserveControllerState
    ) || SKIP_HCRST_DURING_INIT
}

#[inline(always)]
const fn skip_reset_during_init_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Pi 4 has two distinct stop-state lanes. The bootloader-owned
    // ColdStartFromSnapshot lane stays no-touch. The reset-owned None+stop-state
    // lane also skips HCRST: the stopped seed already proves HCH/USBCMD quiesce,
    // and current Pi 4/seL4 hardware traps IRQ 27 immediately after HCRST.
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
        || snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
        || skip_reset_during_init(firmware_handoff)
}

#[inline(always)]
const fn runtime_mailbox_reset_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    matches!(firmware_handoff, XhciFirmwareHandoff::ColdStartFromSnapshot)
        && runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_mailbox_reset_stop_state_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    matches!(firmware_handoff, XhciFirmwareHandoff::ColdStartFromSnapshot)
        && runtime_snapshot_has_stop_state_seed(runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_stop_state_only_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        && !runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_bootloader_owned_pollsafe_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // ColdStartFromSnapshot + stop-state-only is the diagnostic
    // bootloader-owned lane. The reset-owned None + stop-state-only lane is
    // handled separately because it has a different authority label.
    runtime_stop_state_only_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_pollsafe_no_fresh_ownership_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Stop-state-only seeds prove a bounded poll-safe state, but not enough
    // runtime ring ownership to publish a fresh DCBAAP on seL4. The Pi 4
    // platform-reset-complete witness is mailbox-reset ownership, so it stays
    // outside this poll-safe stop-seed set.
    runtime_bootloader_owned_pollsafe_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_seeded_full_reset_start_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    matches!(firmware_handoff, XhciFirmwareHandoff::None)
        && runtime_snapshot_has_stop_state_seed(runtime_seed_snapshot)
        && !runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_owned_fresh_rings_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    matches!(firmware_handoff, XhciFirmwareHandoff::ColdStartFromSnapshot)
        && !runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot)
        && !runtime_snapshot_has_stop_state_seed(runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_platform_reset_fresh_rings_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    matches!(firmware_handoff, XhciFirmwareHandoff::PlatformResetComplete)
        && !runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_unseeded_full_reset_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    matches!(firmware_handoff, XhciFirmwareHandoff::None)
        && !runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot)
        && !runtime_snapshot_has_stop_state_seed(runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_preserve_stop_state_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::PreserveControllerState
    ) && runtime_snapshot_has_stop_state_seed(runtime_seed_snapshot)
}

#[inline(always)]
const fn snapshot_resetless_reinit_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    matches!(firmware_handoff, XhciFirmwareHandoff::ResetlessReinit)
        && runtime_seed_snapshot.is_some()
}

#[inline(always)]
const fn runtime_deferred_ring_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_preserve_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_mailbox_reset_needs_blind_settle(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || matches!(firmware_handoff, XhciFirmwareHandoff::PlatformResetComplete)
        || snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_preserve_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn blind_settle_precedes_live_stop_revalidation(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_mailbox_reset_needs_blind_settle(firmware_handoff, runtime_seed_snapshot)
        && !skip_live_halt_revalidation_with_snapshot(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn pre_halt_source_quiesce_before_live_stop_revalidation(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        && match runtime_seed_snapshot {
            Some(snapshot) => snapshot.usbcmd.is_some(),
            None => false,
        }
}

#[inline(always)]
const fn reset_usbcmd_seed_before_hcrst(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> Option<u32> {
    match runtime_seed_snapshot {
        Some(snapshot) => match snapshot.usbcmd {
            Some(usbcmd) => Some(masked_usbcmd(usbcmd)),
            None => {
                if runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
                {
                    Some(0)
                } else {
                    None
                }
            }
        },
        None => None,
    }
}

#[inline(always)]
const fn runtime_handoff_needs_pre_run_settle(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Any Pi 4 mailbox-reset handoff that rebuilds runtime rings locally
    // still reaches the same RUN edge without trusted live ring seeds. Give
    // VL805 one short bounded settle after runtime ring publication so the
    // controller can observe the freshly published DMA state before RUN.
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
        || matches!(firmware_handoff, XhciFirmwareHandoff::PlatformResetComplete)
        || snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
        || matches!(firmware_handoff, XhciFirmwareHandoff::ColdStartFromSnapshot)
            && runtime_seed_snapshot.is_none()
}

#[inline(always)]
const fn runtime_handoff_needs_relaxed_run_write(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // The Pi 4 mailbox-reset stop-state path now reaches the same first live
    // `USBCMD.RUN` ownership edge as the runtime-ring handoff, after all of
    // the earlier controller-visible publishes were moved out of the way.
    // Keep the lighter helper only on the runtime-ring path; the weaker
    // stop-state path now switches back to the plain U-Boot-style `write_op`
    // sequence so the next hardware trace tells us whether VL805 still dies
    // on the live RUN store when the helper machinery is removed entirely.
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_handoff_needs_uboot_style_reset_write(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Fresh unseeded and Pi 4 platform-reset-complete paths use the same
    // direct HCRST edge as U-Boot/Linux before publishing fresh rings.
    runtime_owned_fresh_rings_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_unseeded_full_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || matches!(firmware_handoff, XhciFirmwareHandoff::PlatformResetComplete)
}

#[inline(always)]
const fn runtime_handoff_needs_release_only_run_write(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // The release-only helper isolated the failure to the live RUN edge
    // itself; the next corrective step is to replay the plain U-Boot-style
    // store sequence instead of keeping preserve-state on an experimental
    // helper branch.
    let _ = firmware_handoff;
    let _ = runtime_seed_snapshot;
    false
}

#[inline(always)]
const fn runtime_handoff_needs_release_only_dcbaap_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // The release-only probe answered its question: on Pi 4/VL805
    // platform-reset-complete, the first live DCBAAP low dword store itself
    // wedges before the completion breadcrumb. Do not keep another publish
    // variant on this path; require stronger runtime-ring ownership evidence.
    let _ = firmware_handoff;
    let _ = runtime_seed_snapshot;
    false
}

#[inline(always)]
const fn platform_reset_dcbaap_publish_blocked_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Pi 4/VL805 platform-reset-complete is only selected after local-seat has
    // pinned the high BAR, recorded COMMAND ownership, installed bounded IRQ
    // sinks, and received the mailbox reset ACK. Those are the seL4 ownership
    // preconditions for publishing Cohesix-owned DCBAA/rings; keep the earlier
    // breadcrumbs so the next hardware trace classifies the real publish edge.
    let _ = firmware_handoff;
    let _ = runtime_seed_snapshot;
    false
}

#[inline(always)]
const fn replay_staged_dcbaap_snapshot_before_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // The preserve-state bootloader-handoff lane has now proven the staged
    // current-value replay to be the first live DCBAAP edge that still wedges
    // under degraded IRQ27. Keep the replay on the other trusted snapshot
    // paths, but let preserve-state go straight to the real publish.
    !runtime_preserve_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
fn preserve_state_dcbaap_write_is_redundant(current: u64, desired: u64) -> bool {
    current == desired
}

#[inline(always)]
fn preserve_state_dcbaap_publish_seed(
    preserve_firmware_state: bool,
    staged_current_dcbaap: u64,
    desired_dcbaap: u64,
) -> u64 {
    if preserve_firmware_state {
        desired_dcbaap
    } else {
        staged_current_dcbaap
    }
}

#[inline(always)]
fn emit_preserve_state_dcbaap_skip_diag(dcbaap_offset: usize, current: u64, desired: u64) {
    emit_xhci_diag(0x0312, dcbaap_offset as u64, current, desired);
}

#[inline(always)]
fn preserve_state_crcr_write_is_redundant(current: u64, desired: u64) -> bool {
    current == desired
}

#[inline(always)]
fn preserve_state_crcr_publish_seed(
    preserve_firmware_state: bool,
    staged_current_crcr: u64,
    desired_crcr: u64,
) -> u64 {
    if preserve_firmware_state {
        desired_crcr
    } else {
        staged_current_crcr
    }
}

#[inline(always)]
fn emit_preserve_state_crcr_skip_diag(crcr_offset: usize, current: u64, desired: u64) {
    emit_xhci_diag(0x0313, crcr_offset as u64, current, desired);
}

#[inline(always)]
const fn runtime_handoff_needs_uboot_style_run_write(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_owned_fresh_rings_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_platform_reset_fresh_rings_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_handoff_skips_live_run_write(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Preserve-state handoff adopts firmware-owned runtime state and avoids a
    // redundant RUN store. Stop-state-only seeds without runtime-ring pointers
    // stay poll-safe until stronger ownership evidence exists.
    runtime_preserve_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_pollsafe_no_fresh_ownership_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_handoff_skips_live_drop_stop(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // A controller instance that did not publish fresh runtime ownership must
    // not acquire ownership later from Drop. The Pi 4 stop-seed path uses this
    // temporary controller only as a poll-safe progress witness before the
    // next live cold-start strategy is attempted. The platform-reset stop-seed
    // lane also avoids an implicit Drop-time stop because the live post-RUN
    // operational reads are intentionally kept behind explicit breadcrumbs.
    runtime_pollsafe_no_fresh_ownership_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_platform_reset_stop_seed_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_platform_reset_stop_seed_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    matches!(firmware_handoff, XhciFirmwareHandoff::PlatformResetComplete)
        && runtime_snapshot_has_stop_state_seed(runtime_seed_snapshot)
        && !runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_stop_state_needs_post_run_settle(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
        || matches!(firmware_handoff, XhciFirmwareHandoff::PlatformResetComplete)
            && runtime_snapshot_has_stop_state_seed(runtime_seed_snapshot)
            && !runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot)
}

#[inline(always)]
const fn skip_post_run_interrupter_zeroing_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    let _ = (firmware_handoff, runtime_seed_snapshot);
    true
}

#[inline(always)]
const fn runtime_needs_post_run_polling_irq_quiesce_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    let _ = (firmware_handoff, runtime_seed_snapshot);
    false
}

#[inline(always)]
const fn runtime_needs_post_init_polling_irq_quiesce_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
#[cfg(test)]
const fn pre_dcbaap_polling_irq_quiesce_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    pre_dcbaap_iman_disable_value_with_snapshot(firmware_handoff, runtime_seed_snapshot).is_some()
}

#[inline(always)]
const fn pre_dcbaap_iman_disable_value_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> Option<u32> {
    if runtime_pollsafe_no_fresh_ownership_handoff(firmware_handoff, runtime_seed_snapshot) {
        return None;
    }
    if runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot) {
        let iman = match runtime_seed_snapshot {
            Some(snapshot) => match snapshot.iman0 {
                Some(iman) => iman,
                None => 0,
            },
            None => 0,
        };
        Some(disable_interrupter_iman_value(iman))
    } else {
        None
    }
}

#[inline(always)]
const fn defer_scratchpad_array_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_deferred_ring_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn defer_dcbaap_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_deferred_ring_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn defer_crcr_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_deferred_ring_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn defer_erdp_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Runtime-deferred snapshot paths keep ERDP staged so earlier ownership
    // edges stay isolated. Stop-state-only seeds without runtime-ring pointers
    // now defer here too, then skip the fresh publish through the no-ownership
    // policy gate.
    runtime_deferred_ring_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_owned_fresh_rings_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn defer_erst_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_deferred_ring_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn deferred_erst_publish_uses_size_first_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    (snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_preserve_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot))
        && runtime_deferred_ring_handoff(firmware_handoff, runtime_seed_snapshot)
        && (!runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot)
            || snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
            || runtime_preserve_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
            || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
            || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot))
}

#[inline(always)]
const fn deferred_erdp_publish_precedes_erst_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Runtime-owned handoffs publish a fresh event ring table-first. Stop-state
    // only seeds are no longer in that set and publish no fresh event-ring
    // registers until runtime has stronger ring-ownership evidence.
    let _ = firmware_handoff;
    let _ = runtime_seed_snapshot;
    false
}

#[inline(always)]
const fn defer_event_ring_publish_until_after_run_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Keep the resetless snapshot path on the post-RUN event-ring ladder.
    // Stop-state-only seeds are filtered by skip_fresh_event_ring.
    snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn skip_fresh_event_ring_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_pollsafe_no_fresh_ownership_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn skip_fresh_runtime_ownership_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_pollsafe_no_fresh_ownership_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn defer_dcbaap_publish_until_after_run_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_stop_state_only_handoff(firmware_handoff, runtime_seed_snapshot)
        || snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn defer_crcr_publish_until_after_run_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_stop_state_only_handoff(firmware_handoff, runtime_seed_snapshot)
        || snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn defer_dnctrl_write_until_after_run_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_stop_state_only_handoff(firmware_handoff, runtime_seed_snapshot)
        || snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn skip_dnctrl_write_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // The trusted preserve-state Pi 4 lane now consistently reaches DNCTRL as
    // the first remaining controller-visible ownership edge. Preserve that
    // bootloader-owned notification state as-is instead of replaying another
    // redundant zero on the degraded IRQ27 path.
    runtime_preserve_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn use_atomic_erstba_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Pi 4 ERSTBA uses the 0x4_0000_0000 PCIe DMA alias. Keep the atomic write
    // only on runtime-ring snapshots; stop-state-only seeds now avoid fresh
    // event-ring publication entirely.
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn probe_live_dcbaap_before_staged_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    !runtime_deferred_ring_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn probe_live_crcr_before_staged_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    !defer_crcr_publish_with_snapshot(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn use_atomic_runtime_ring_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // The atomic ownership-publish probe answered its question: the first
    // runtime DCBAAP store is toxic even without the split low/high sequence.
    // Disable the atomic experiment and move the next diagnostic branch back
    // to the reset gate instead of repeating the same publish hazard.
    let _ = firmware_handoff;
    let _ = runtime_seed_snapshot;
    false
}

#[cfg(test)]
#[inline(always)]
const fn skip_post_reset_cnr_poll_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    let _ = firmware_handoff;
    let _ = runtime_seed_snapshot;
    false
}

#[inline(always)]
const fn skip_live_halt_revalidation(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ResetlessReinit
            | XhciFirmwareHandoff::PlatformResetComplete
            | XhciFirmwareHandoff::PreserveControllerState
    )
}

#[inline(always)]
const fn skip_live_halt_revalidation_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    skip_live_halt_revalidation(firmware_handoff)
        || runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_stop_state_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_seeded_full_reset_start_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn skip_live_halt_revalidation_for_init(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    skip_initial_live_operational_reads: bool,
) -> bool {
    skip_live_halt_revalidation_with_snapshot(firmware_handoff, runtime_seed_snapshot)
        || initial_live_operational_read_hazard(
            firmware_handoff,
            runtime_seed_snapshot,
            skip_initial_live_operational_reads,
        )
}

#[inline(always)]
const fn reset_usbcmd_seed_before_hcrst_for_init(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    skip_initial_live_operational_reads: bool,
) -> Option<u32> {
    if initial_live_operational_read_hazard(
        firmware_handoff,
        runtime_seed_snapshot,
        skip_initial_live_operational_reads,
    ) {
        Some(0)
    } else {
        reset_usbcmd_seed_before_hcrst(firmware_handoff, runtime_seed_snapshot)
    }
}

#[inline(always)]
const fn skip_reset_pre_usbsts_read_for_init(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    skip_initial_live_operational_reads: bool,
) -> bool {
    initial_live_operational_read_hazard(
        firmware_handoff,
        runtime_seed_snapshot,
        skip_initial_live_operational_reads,
    )
}

#[inline(always)]
const fn skip_reset_completion_poll_for_init(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    skip_initial_live_operational_reads: bool,
) -> bool {
    if skip_reset_pre_usbsts_read_for_init(
        firmware_handoff,
        runtime_seed_snapshot,
        skip_initial_live_operational_reads,
    ) {
        return true;
    }
    if matches!(firmware_handoff, XhciFirmwareHandoff::PlatformResetComplete) {
        return false;
    }
    false
}

#[inline(always)]
const fn runtime_snapshot_has_stop_state_seed(
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    match runtime_seed_snapshot {
        Some(snapshot) => {
            snapshot.usbcmd.is_some() || snapshot.usbsts.is_some() || snapshot.iman0.is_some()
        }
        None => false,
    }
}

#[inline(always)]
const fn runtime_snapshot_has_runtime_ring_seed(
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    match runtime_seed_snapshot {
        Some(snapshot) => {
            snapshot.dcbaap.is_some()
                || snapshot.crcr.is_some()
                || snapshot.erstba0.is_some()
                || snapshot.erdp0.is_some()
                || snapshot.erstsz0.is_some()
        }
        None => false,
    }
}

#[inline(always)]
const fn runtime_seed_snapshot_flag_bits(
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> u64 {
    let mut flags = 0u64;
    if runtime_seed_snapshot.is_some() {
        flags |= 1 << 0;
    }
    if runtime_snapshot_has_stop_state_seed(runtime_seed_snapshot) {
        flags |= 1 << 1;
    }
    if runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot) {
        flags |= 1 << 2;
    }
    flags
}

#[inline]
const fn port_ready_for_enumeration(portsc: u32) -> bool {
    if (portsc & reg::PORTSC_CCS) == 0 {
        return false;
    }

    let speed = reg::portsc_speed(portsc);
    if speed == 0 {
        return false;
    }

    // USB2 ports should be both connected and enabled before issuing
    // Address Device. For SuperSpeed, keep acceptance permissive because
    // controller-specific link states can transiently clear PED.
    if speed >= reg::SPEED_SUPER {
        true
    } else {
        (portsc & reg::PORTSC_PED) != 0
    }
}

/// Callback signature for xHCI probe diagnostics.
pub type XhciDiagHook = fn(stage: u16, a: u64, b: u64, c: u64);

/// Callback signature for platform-owned xHCI root-port register reads.
pub type XhciPortReadHook = fn(mmio: usize, offset: usize, port: u8, max_ports: u8) -> u32;

/// Callback signature for platform-owned xHCI root-port register writes.
pub type XhciPortWriteHook = fn(mmio: usize, offset: usize, port: u8, max_ports: u8, value: u32);

/// Callback signature for platform-owned xHCI posted-write flushes.
pub type XhciPostedWriteFlushHook = fn(mmio: usize, offset: usize, value: u32, stage: u16);

static XHCI_DIAG_HOOK: Mutex<Option<XhciDiagHook>> = Mutex::new(None);
static XHCI_PORT_READ_HOOK: Mutex<Option<XhciPortReadHook>> = Mutex::new(None);
static XHCI_PORT_WRITE_HOOK: Mutex<Option<XhciPortWriteHook>> = Mutex::new(None);
static XHCI_POSTED_WRITE_FLUSH_HOOK: Mutex<Option<XhciPostedWriteFlushHook>> = Mutex::new(None);

/// Installs or clears the xHCI probe diagnostic callback.
pub fn set_xhci_diag_hook(hook: Option<XhciDiagHook>) {
    *XHCI_DIAG_HOOK.lock() = hook;
}

/// Installs or clears platform-owned xHCI root-port register access callbacks.
pub fn set_xhci_port_access_hooks(
    read: Option<XhciPortReadHook>,
    write: Option<XhciPortWriteHook>,
) {
    *XHCI_PORT_READ_HOOK.lock() = read;
    *XHCI_PORT_WRITE_HOOK.lock() = write;
}

/// Installs or clears a platform-owned posted-write flush callback.
pub fn set_xhci_posted_write_flush_hook(hook: Option<XhciPostedWriteFlushHook>) {
    *XHCI_POSTED_WRITE_FLUSH_HOOK.lock() = hook;
}

#[inline(always)]
fn emit_xhci_diag(stage: u16, a: u64, b: u64, c: u64) {
    if let Some(hook) = *XHCI_DIAG_HOOK.lock() {
        hook(stage, a, b, c);
    }
}

#[inline(always)]
fn xhci_port_read_hook() -> Option<XhciPortReadHook> {
    *XHCI_PORT_READ_HOOK.lock()
}

#[inline(always)]
fn xhci_port_write_hook() -> Option<XhciPortWriteHook> {
    *XHCI_PORT_WRITE_HOOK.lock()
}

#[inline(always)]
fn flush_posted_write(mmio: usize, offset: usize, value: u32, stage: u16) {
    if let Some(hook) = *XHCI_POSTED_WRITE_FLUSH_HOOK.lock() {
        hook(mmio, offset, value, stage);
    }
}

#[inline(always)]
fn ring_write_barrier() {
    compiler_fence(Ordering::Release);
    #[cfg(target_arch = "aarch64")]
    // SAFETY: This emits a store-only data memory barrier and does not touch
    // registers, memory operands, or the stack.
    unsafe {
        core::arch::asm!("dmb oshst", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(Ordering::Release);
}

#[inline(always)]
fn mmio_write_relaxed_barrier() {
    compiler_fence(Ordering::Release);
    #[cfg(target_arch = "aarch64")]
    // SAFETY: This emits a store-only data memory barrier and does not touch
    // registers, memory operands, or the stack.
    unsafe {
        // The cold-start mailbox-reset runtime handoff still benefits from a
        // device-visible store barrier before touching live operational state,
        // but it does not need the stronger full-system drain.
        core::arch::asm!("dmb oshst", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(Ordering::Release);
}

#[inline(always)]
fn mmio_write_release_only_barrier() {
    // The resetless runtime seed path has already inherited a halted
    // controller and runtime-ring snapshot from firmware. At the final
    // `USBCMD RUN` edge, an AArch64 device drain itself can wedge VL805 before
    // the store lands, so keep compiler ordering but skip the hardware drain.
    compiler_fence(Ordering::Release);
}

#[inline(always)]
fn mmio_write_barrier() {
    compiler_fence(Ordering::Release);
    // SAFETY: Emits a single architectural barrier instruction, does not touch
    // memory or registers, and preserves flags/stack.
    #[cfg(target_arch = "aarch64")]
    unsafe {
        // Match U-Boot's ARM `writel()` ordering exactly on the trusted Pi 4
        // handoff path. Its `__iowmb()` expands to `dmb sy`, not `dmb osh`.
        core::arch::asm!("dmb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(Ordering::Release);
}

#[inline(always)]
fn mmio_read_barrier() {
    compiler_fence(Ordering::Acquire);
    // SAFETY: Emits a single architectural barrier instruction, does not touch
    // memory or registers, and preserves flags/stack.
    #[cfg(target_arch = "aarch64")]
    unsafe {
        // Match U-Boot's ARM `readl()` ordering exactly on the trusted Pi 4
        // handoff path. Its `__iormb()` expands to `dmb sy`, not `dmb osh`.
        core::arch::asm!("dmb sy", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(Ordering::Acquire);
}

#[inline]
const fn port_state_neutral(portsc: u32) -> u32 {
    portsc & PORTSC_NEUTRAL_MASK
}

#[inline]
const fn encode_port_diag(portsc: u32) -> u64 {
    let speed = reg::portsc_speed(portsc) as u64;
    let pls = reg::portsc_pls(portsc) as u64;
    let ped = ((portsc & reg::PORTSC_PED) != 0) as u64;
    let ccs = ((portsc & reg::PORTSC_CCS) != 0) as u64;
    (speed << 56) | (pls << 48) | (ped << 40) | (ccs << 32) | portsc as u64
}

#[inline]
const fn xhci_dboff_offset(raw: u32) -> u32 {
    raw & XHCI_DBOFF_MASK
}

#[inline]
const fn xhci_rtsoff_offset(raw: u32) -> u32 {
    raw & XHCI_RTSOFF_MASK
}

fn parse_controller_params(
    cap_length: u8,
    hcs1: u32,
    hcs2: u32,
    db_offset: u32,
    rts_offset: u32,
) -> Option<(u8, u8, u16, usize)> {
    if cap_length < 0x20 || (cap_length as usize) >= MMIO_INIT_SIZE || (cap_length & 0x3) != 0 {
        return None;
    }

    let max_slots = (hcs1 & 0xff) as u8;
    let max_ports = ((hcs1 >> 24) & 0xff) as u8;
    let max_scratchpad = (((hcs2 >> 27) & 0x1f) | (((hcs2 >> 21) & 0x1f) << 5)) as u16;
    if max_slots == 0 || max_slots > MAX_REASONABLE_SLOTS {
        return None;
    }
    if max_ports == 0 || max_ports > MAX_REASONABLE_PORTS {
        return None;
    }
    if max_scratchpad > MAX_REASONABLE_SCRATCHPAD {
        return None;
    }

    let db_offset = xhci_dboff_offset(db_offset);
    let rts_offset = xhci_rtsoff_offset(rts_offset);
    if db_offset < cap_length as u32 || rts_offset < cap_length as u32 {
        return None;
    }

    let mmio_size = (rts_offset as usize + 0x20 + 0x20)
        .max(db_offset as usize + (max_slots as usize + 1) * 4)
        .max(0x10000);
    if !(MMIO_INIT_SIZE..=MMIO_MAX_SIZE).contains(&mmio_size) {
        return None;
    }

    Some((max_slots, max_ports, max_scratchpad, mmio_size))
}

/// Firmware ownership contract for xHCI runtime bring-up.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XhciFirmwareHandoff {
    /// Runtime owns the full xHCI quiesce/reset sequence.
    None = 0,
    /// Firmware supplied a trusted capability snapshot, so runtime can drive
    /// a full halt/reset/config/ring init without a fresh live CAP probe.
    ColdStartFromSnapshot = 1,
    /// Firmware proved the controller safe, so runtime skips the fragile
    /// pre-init scrub/reset sequence but still republishes its own config and
    /// ring ownership state.
    ResetlessReinit = 2,
    /// Firmware proved and preserved a halted/quiesced controller state that
    /// runtime should adopt without rewriting preserved firmware-owned
    /// controller state.
    PreserveControllerState = 3,
    /// Platform firmware or mailbox reset proved PCIe/VL805 ownership.
    /// Runtime uses a blind local xHC HCRST after that boundary, then replays
    /// CONFIG/ring ownership locally before trusting command-ring completion.
    PlatformResetComplete = 4,
}

/// Bootloader-exported xHCI stop/ring seed snapshot for trusted handoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XhciRuntimeSeedSnapshot {
    /// Operational `USBCMD` captured before handoff.
    pub usbcmd: Option<u32>,
    /// Operational `USBSTS` captured before handoff.
    pub usbsts: Option<u32>,
    /// Interrupter 0 `IMAN` captured before handoff.
    pub iman0: Option<u32>,
    /// Operational `DCBAAP` captured before handoff.
    pub dcbaap: Option<u64>,
    /// Operational `CRCR` captured before handoff.
    pub crcr: Option<u64>,
    /// Runtime interrupter 0 `ERSTBA` captured before handoff.
    pub erstba0: Option<u64>,
    /// Runtime interrupter 0 `ERDP` captured before handoff.
    pub erdp0: Option<u64>,
    /// Runtime interrupter 0 `ERSTSZ` captured before handoff.
    pub erstsz0: Option<u32>,
}

/// Pre-validated xHCI capability register snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XhciControllerParams {
    /// Capability register length.
    pub cap_length: u8,
    /// Structural Parameters 1.
    pub hcs1: u32,
    /// Structural Parameters 2.
    pub hcs2: u32,
    /// Capability Parameters 1.
    pub hccparams1: u32,
    /// Doorbell offset.
    pub db_offset: u32,
    /// Runtime space offset.
    pub rts_offset: u32,
    /// Firmware ownership contract selected by platform bring-up.
    pub firmware_handoff: XhciFirmwareHandoff,
    /// Optional bootloader stop/ring seed snapshot for trusted handoff.
    pub runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    /// Apply the Broadcom generic xHCI-wrapper AXI attribute quirk.
    pub apply_brcm_axi_setup: bool,
    /// Skip the constructor's live USBCMD scrub read for platform paths where
    /// the first operational MMIO read is unsafe until later ownership setup.
    pub skip_constructor_live_scrub: bool,
    /// Skip early operational MMIO reads until after reset/config/RUN rebuild
    /// controller ownership from caller-provided capability evidence.
    pub skip_initial_live_operational_reads: bool,
    /// Permit direct root-port register access. Platform HALs may disable this
    /// when PORTSC reads are known to trap before a HAL-owned port path exists.
    pub port_register_access_allowed: bool,
}

impl XhciControllerParams {
    #[inline]
    fn validated(self) -> Option<(u8, u8, u16, usize, usize)> {
        let (max_slots, max_ports, max_scratchpad, mmio_size) = parse_controller_params(
            self.cap_length,
            self.hcs1,
            self.hcs2,
            self.db_offset,
            self.rts_offset,
        )?;
        let ctx_size_bytes = if (self.hccparams1 & (1 << 2)) != 0 {
            64
        } else {
            32
        };
        Some((
            max_slots,
            max_ports,
            max_scratchpad,
            mmio_size,
            ctx_size_bytes,
        ))
    }
}

/// xHCI Controller
pub struct XhciCtrl<H: Dma> {
    mmio: usize,
    mmio_size: usize,
    cap_length: u8,
    op_base: usize,
    rt_base: usize,
    db_offset: u32,
    ctx_size_bytes: usize,
    max_slots: u8,
    max_ports: u8,
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
    initialized: bool,
    dcbaa: PhysMem<H>,
    scratchpad: Option<ScratchpadSet<H>>,
    cmd_ring: Mutex<Box<Ring<H>>>,
    event_ring: Mutex<Box<EventRing<H>>>,
    host: Arc<H>,
    skip_initial_live_operational_reads: bool,
    port_register_access_allowed: bool,
}

/// Snapshot of xHCI command/event ring state for timeout diagnostics.
#[derive(Clone, Copy, Debug)]
pub struct XhciCommandDiag {
    /// Current value of `USBCMD`.
    pub usbcmd: u32,
    /// Current value of `USBSTS`.
    pub usbsts: u32,
    /// Current value of `CRCR`.
    pub crcr: u64,
    /// Current value of `DCBAAP`.
    pub dcbaap: u64,
    /// Current value of interrupter 0 `IMAN`.
    pub iman: u32,
    /// Current value of interrupter 0 `ERDP`.
    pub erdp: u64,
    /// Current value of interrupter 0 `ERSTBA`.
    pub erstba: u64,
    /// Current value of the selected port `PORTSC`.
    pub portsc: u32,
}

struct ScratchpadSet<H: Dma> {
    array: PhysMem<H>,
    buffers: Vec<PhysMem<H>>,
}

impl<H: Dma> ScratchpadSet<H> {
    fn build(host: &H, count: usize) -> Result<Self> {
        // xHCI spec requires 64-byte alignment for the scratchpad pointer array.
        let array = PhysMem::alloc(host, count * core::mem::size_of::<u64>(), 64)?;
        let mut buffers = Vec::new();
        buffers
            .try_reserve_exact(count)
            .map_err(|_| UsbError::OoRam)?;

        let array_ptr = array.as_ptr::<u64>();
        for index in 0..count {
            // Scratchpad buffers are page-sized and page-aligned.
            let page = PhysMem::alloc(host, host.page_size(), host.page_size())?;
            let phys = page.phys(host);
            // SAFETY: `array_ptr` points at the owned scratchpad pointer array
            // allocation and `index < count` by loop construction.
            unsafe {
                array_ptr.add(index).write_volatile(phys);
            }
            buffers.push(page);
        }

        Ok(Self { array, buffers })
    }

    fn share_for_device(&self, host: &H) -> Result<()> {
        for page in &self.buffers {
            let _ = page.share_for_device(host, "xhci-scratchpad-page")?;
        }
        let _ = self.array.share_for_device(host, "xhci-scratchpad-array")?;
        Ok(())
    }
}

#[inline(always)]
fn split_u64_reg_write_ops(offset: usize, val: u64) -> [(usize, u32); 2] {
    [(offset, val as u32), (offset + 4, (val >> 32) as u32)]
}

#[inline(always)]
fn dcbaap_reg_write_ops(offset: usize, _current: u64, target: u64) -> [(usize, u32); 2] {
    // Preserve the original usb-oxide split-write order. VL805 observes the
    // low/high pair directly; publishing the high dword first can expose a
    // transient high-only DCBAAP base on the Pi 4 PCIe DMA alias.
    split_u64_reg_write_ops(offset, target)
}

#[inline(always)]
fn u64_register_change_mask(current: u64, target: u64) -> u64 {
    let [current_low, current_high] = split_u64_reg_write_ops(0, current);
    let [target_low, target_high] = split_u64_reg_write_ops(0, target);
    u64::from(current_low.1 != target_low.1) | (u64::from(current_high.1 != target_high.1) << 1)
}

#[inline(always)]
fn compose_crcr(current: u64, ring_ptr: u64, cycle_state: bool) -> u64 {
    (current & reg::CMD_RING_RSVD_BITS)
        | (ring_ptr & !reg::CMD_RING_RSVD_BITS)
        | u64::from(cycle_state)
}

#[inline(always)]
fn compose_erst_size(current: u32, entries: u32) -> u32 {
    (current & reg::ERST_SIZE_MASK) | entries
}

#[inline(always)]
fn preserve_state_erstsz_write_is_redundant(current: u32, desired: u32) -> bool {
    current == desired
}

#[inline(always)]
fn preserve_state_erstsz_publish_seed(
    preserve_firmware_state: bool,
    staged_current_erst_size: u32,
    desired_erst_size: u32,
) -> u32 {
    if preserve_firmware_state {
        desired_erst_size
    } else {
        staged_current_erst_size
    }
}

#[inline(always)]
fn emit_preserve_state_erstsz_skip_diag(int_base: usize, current: u32, desired: u32) {
    emit_xhci_diag(
        0x02da,
        (int_base + reg::ERSTSZ) as u64,
        current as u64,
        desired as u64,
    );
}

#[inline(always)]
fn preserve_state_erstba_write_is_redundant(current: u64, desired: u64) -> bool {
    current == desired
}

#[inline(always)]
fn preserve_state_erstba_publish_seed(
    preserve_firmware_state: bool,
    staged_current_erstba: u64,
    desired_erstba: u64,
) -> u64 {
    if preserve_firmware_state {
        desired_erstba
    } else {
        staged_current_erstba
    }
}

#[inline(always)]
fn emit_preserve_state_erstba_skip_diag(int_base: usize, current: u64, desired: u64) {
    emit_xhci_diag(0x0310, (int_base + reg::ERSTBA) as u64, current, desired);
}

#[inline(always)]
fn preserve_state_erdp_write_is_redundant(current: u64, desired: u64) -> bool {
    current == desired
}

#[inline(always)]
fn preserve_state_erdp_publish_seed(
    preserve_firmware_state: bool,
    staged_current_erdp: u64,
    desired_erdp: u64,
) -> u64 {
    if preserve_firmware_state {
        desired_erdp
    } else {
        staged_current_erdp
    }
}

#[inline(always)]
fn emit_preserve_state_erdp_skip_diag(int_base: usize, current: u64, desired: u64) {
    emit_xhci_diag(0x0311, (int_base + reg::ERDP) as u64, current, desired);
}

#[inline(always)]
fn compose_erst_base(current: u64, base: u64) -> u64 {
    (current & reg::ERST_PTR_MASK) | (base & !reg::ERST_PTR_MASK)
}

#[inline(always)]
fn compose_config(current: u32, max_slots: u8) -> u32 {
    (current & !CONFIG_MAX_SLOTS_MASK) | u32::from(max_slots)
}

#[inline(always)]
fn compose_initial_erdp(event_ring_ptr: u64) -> u64 {
    event_ring_ptr & !reg::ERST_PTR_MASK
}

#[inline(always)]
fn compose_polling_erdp_ack(event_ring_ptr: u64) -> u64 {
    event_ring_ptr | reg::ERST_EHB
}

#[inline(always)]
const fn compose_brcm_usbaxi_attr(current: u32) -> u32 {
    (current & !BRCM_XHCI_USBAXI_SA_UA_MASK) | BRCM_XHCI_USBAXI_SA_UA_VAL
}

#[inline(always)]
const fn xhci_port_in_range(port: u8, max_ports: u8) -> bool {
    port < max_ports
}

#[inline(always)]
const fn xhci_slot_in_range(slot: u8, max_slots: u8) -> bool {
    slot <= max_slots
}

impl<H: Dma> XhciCtrl<H> {
    #[inline(always)]
    fn port_in_range(&self, port: u8) -> bool {
        xhci_port_in_range(port, self.max_ports)
    }

    #[inline(always)]
    fn slot_in_range(&self, slot: u8) -> bool {
        xhci_slot_in_range(slot, self.max_slots)
    }

    #[inline(always)]
    fn read_reg_at<T: Copy>(mmio: usize, offset: usize) -> T {
        // SAFETY: Callers pass offsets within the mapped xHCI MMIO aperture and
        // `T` is only used for register-width volatile loads.
        let val = unsafe { ((mmio + offset) as *const T).read_volatile() };
        mmio_read_barrier();
        val
    }

    #[inline(always)]
    fn write_reg_at<T: Copy>(mmio: usize, offset: usize, val: T) {
        // Match U-Boot's `readl`/`writel` ordering discipline on ARM before
        // touching live controller registers after firmware handoff.
        mmio_write_barrier();
        // SAFETY: Callers pass offsets within the mapped xHCI MMIO aperture and
        // `T` is only used for register-width volatile stores.
        unsafe {
            ((mmio + offset) as *mut T).write_volatile(val);
        }
    }

    #[inline(always)]
    fn write_reg_u32_store_diag_at(
        mmio: usize,
        offset: usize,
        val: u32,
        pre_stage: u16,
        done_stage: u16,
        diag_ctx: u64,
    ) {
        emit_xhci_diag(pre_stage, offset as u64, val as u64, diag_ctx);
        Self::write_reg_at::<u32>(mmio, offset, val);
        emit_xhci_diag(done_stage, offset as u64, val as u64, diag_ctx);
    }

    #[inline(always)]
    fn write_reg_u32_store_diag_at_with_barrier_phase(
        mmio: usize,
        offset: usize,
        val: u32,
        pre_stage: u16,
        barrier_done_stage: u16,
        pre_store_stage: u16,
        done_stage: u16,
        diag_ctx: u64,
    ) {
        emit_xhci_diag(pre_stage, offset as u64, val as u64, diag_ctx);
        mmio_write_barrier();
        emit_xhci_diag(barrier_done_stage, offset as u64, val as u64, diag_ctx);
        emit_xhci_diag(
            pre_store_stage,
            (mmio + offset) as u64,
            val as u64,
            diag_ctx,
        );
        // SAFETY: The register offset is within the mapped xHCI MMIO aperture;
        // this helper performs one volatile u32 store after the selected barrier.
        unsafe {
            ((mmio + offset) as *mut u32).write_volatile(val);
        }
        emit_xhci_diag(done_stage, offset as u64, val as u64, diag_ctx);
    }

    #[inline(always)]
    fn write_reg_u32_store_diag_relaxed_at_with_barrier_phase(
        mmio: usize,
        offset: usize,
        val: u32,
        pre_stage: u16,
        barrier_done_stage: u16,
        pre_store_stage: u16,
        done_stage: u16,
        diag_ctx: u64,
    ) {
        emit_xhci_diag(pre_stage, offset as u64, val as u64, diag_ctx);
        mmio_write_relaxed_barrier();
        emit_xhci_diag(barrier_done_stage, offset as u64, val as u64, diag_ctx);
        emit_xhci_diag(
            pre_store_stage,
            (mmio + offset) as u64,
            val as u64,
            diag_ctx,
        );
        // SAFETY: The register offset is within the mapped xHCI MMIO aperture;
        // this helper performs one volatile u32 store after the selected barrier.
        unsafe {
            ((mmio + offset) as *mut u32).write_volatile(val);
        }
        emit_xhci_diag(done_stage, offset as u64, val as u64, diag_ctx);
    }

    #[inline(always)]
    fn write_reg_u32_store_diag_release_only_at_with_barrier_phase(
        mmio: usize,
        offset: usize,
        val: u32,
        pre_stage: u16,
        barrier_done_stage: u16,
        pre_store_stage: u16,
        done_stage: u16,
        diag_ctx: u64,
    ) {
        emit_xhci_diag(pre_stage, offset as u64, val as u64, diag_ctx);
        mmio_write_release_only_barrier();
        emit_xhci_diag(barrier_done_stage, offset as u64, val as u64, diag_ctx);
        emit_xhci_diag(
            pre_store_stage,
            (mmio + offset) as u64,
            val as u64,
            diag_ctx,
        );
        // SAFETY: The register offset is within the mapped xHCI MMIO aperture;
        // this helper performs one volatile u32 store after the selected barrier.
        unsafe {
            ((mmio + offset) as *mut u32).write_volatile(val);
        }
        emit_xhci_diag(done_stage, offset as u64, val as u64, diag_ctx);
    }

    #[inline(always)]
    fn write_reg_u64_diag_at(
        mmio: usize,
        offset: usize,
        val: u64,
        low_stage: u16,
        high_stage: u16,
    ) {
        let [low, high] = split_u64_reg_write_ops(offset, val);
        emit_xhci_diag(low_stage, low.0 as u64, low.1 as u64, val);
        Self::write_reg_at::<u32>(mmio, low.0, low.1);
        emit_xhci_diag(high_stage, high.0 as u64, high.1 as u64, val);
        Self::write_reg_at::<u32>(mmio, high.0, high.1);
    }

    #[inline(always)]
    fn write_reg_u64_done_diag_at(
        mmio: usize,
        offset: usize,
        val: u64,
        low_stage: u16,
        low_done_stage: u16,
        high_stage: u16,
        high_done_stage: u16,
    ) {
        let [low, high] = split_u64_reg_write_ops(offset, val);
        emit_xhci_diag(low_stage, low.0 as u64, low.1 as u64, val);
        Self::write_reg_at::<u32>(mmio, low.0, low.1);
        emit_xhci_diag(low_done_stage, low.0 as u64, low.1 as u64, val);
        emit_xhci_diag(high_stage, high.0 as u64, high.1 as u64, val);
        Self::write_reg_at::<u32>(mmio, high.0, high.1);
        emit_xhci_diag(high_done_stage, high.0 as u64, high.1 as u64, val);
    }

    #[inline(always)]
    fn write_dcbaap_reg_u64_done_diag_at(
        mmio: usize,
        offset: usize,
        current: u64,
        val: u64,
        low_stage: u16,
        low_done_stage: u16,
        high_stage: u16,
        high_done_stage: u16,
    ) {
        for (reg_offset, reg_value) in dcbaap_reg_write_ops(offset, current, val) {
            let (pre_stage, done_stage) = if reg_offset == offset {
                (low_stage, low_done_stage)
            } else {
                (high_stage, high_done_stage)
            };
            emit_xhci_diag(pre_stage, reg_offset as u64, reg_value as u64, val);
            Self::write_reg_at::<u32>(mmio, reg_offset, reg_value);
            emit_xhci_diag(done_stage, reg_offset as u64, reg_value as u64, val);
        }
    }

    #[inline(always)]
    fn write_reg_u64_atomic_diag_at(
        mmio: usize,
        offset: usize,
        val: u64,
        write_stage: u16,
        done_stage: u16,
    ) {
        emit_xhci_diag(write_stage, offset as u64, val, 0);
        Self::write_reg_at::<u64>(mmio, offset, val);
        emit_xhci_diag(done_stage, offset as u64, val, 0);
    }

    #[inline(always)]
    fn write_only_polling_scrub(
        mmio: usize,
        op_offset: usize,
        int_base: usize,
        mode: ConstructorPollingScrubMode,
    ) {
        if matches!(mode, ConstructorPollingScrubMode::TrustedQuiesceOnly) {
            emit_xhci_diag(0x0209, reg::USBCMD as u64, 0, 1);
            // Trusted preserve/resetless and Pi 4 cold-start snapshot lanes
            // now keep constructor scrub completely write-free.
            emit_xhci_diag(0x020d, reg::USBSTS as u64, USBSTS_CLEAR_MASK as u64, 1);
            emit_xhci_diag(0x020e, (int_base + reg::IMOD) as u64, 0, 1);
            emit_xhci_diag(
                0x020f,
                (int_base + reg::IMAN) as u64,
                polling_iman_value() as u64,
                1,
            );
        } else {
            let usbcmd = Self::read_reg_at::<u32>(mmio, op_offset + reg::USBCMD);
            if usbcmd_interrupt_delivery_enabled(usbcmd) {
                let masked = masked_usbcmd(usbcmd);
                emit_xhci_diag(0x0205, usbcmd as u64, masked as u64, 1);
                Self::write_reg_at(mmio, op_offset + reg::USBCMD, masked);
            } else {
                emit_xhci_diag(0x0209, reg::USBCMD as u64, usbcmd as u64, 0);
            }
            emit_xhci_diag(0x0206, reg::USBSTS as u64, USBSTS_CLEAR_MASK as u64, 1);
            emit_xhci_diag(0x0207, (int_base + reg::IMOD) as u64, 0, 1);
            emit_xhci_diag(
                0x0208,
                (int_base + reg::IMAN) as u64,
                polling_iman_value() as u64,
                1,
            );
        }
    }

    #[inline(always)]
    fn write_pre_halt_source_quiesce_at(
        mmio: usize,
        op_offset: usize,
        seed_usbcmd: Option<u32>,
        diag_stage: Option<u16>,
    ) {
        let usbcmd =
            seed_usbcmd.unwrap_or_else(|| Self::read_reg_at::<u32>(mmio, op_offset + reg::USBCMD));
        let pre_halt = masked_usbcmd(usbcmd) & !reg::USBCMD_RUN;
        if let Some(stage) = diag_stage {
            emit_xhci_diag(
                stage,
                usbcmd as u64,
                pre_halt as u64,
                (usbcmd ^ pre_halt) as u64,
            );
        }
        if usbcmd != pre_halt {
            if diag_stage.is_some() {
                Self::write_reg_u32_store_diag_at(
                    mmio,
                    op_offset + reg::USBCMD,
                    pre_halt,
                    0x021a,
                    0x021b,
                    0,
                );
            } else {
                Self::write_reg_at::<u32>(mmio, op_offset + reg::USBCMD, pre_halt);
            }
        } else if diag_stage.is_some() {
            emit_xhci_diag(0x021c, reg::USBCMD as u64, pre_halt as u64, 0);
        }
    }

    #[inline(always)]
    fn write_polling_interrupt_quiesce_at(
        mmio: usize,
        op_offset: usize,
        int_base: usize,
        erdp: u64,
        diag_stage: Option<u16>,
    ) {
        let erdp_ack = compose_polling_erdp_ack(erdp);
        if let Some(stage) = diag_stage {
            emit_xhci_diag(stage, erdp_ack, polling_iman_value() as u64, 0);
        }
        let _ = op_offset;
        Self::write_reg_at::<u64>(mmio, int_base + reg::ERDP, erdp_ack);
    }

    #[inline(always)]
    fn write_post_start_polling_interrupt_quiesce_at(
        mmio: usize,
        op_offset: usize,
        int_base: usize,
        erdp: u64,
        seed_usbcmd: Option<u32>,
        skip_imod_write: bool,
        skip_erdp_write: bool,
        skip_iman_write: bool,
        skip_usbsts_clear_write: bool,
        diag_stage: Option<u16>,
    ) {
        if let Some(stage) = diag_stage {
            emit_xhci_diag(
                stage,
                erdp | 0x8,
                polling_iman_value() as u64,
                USBSTS_CLEAR_MASK as u64,
            );
        }
        // U-Boot's poll-only start path does not replay IMOD/IMAN, and Linux
        // only advances ERDP after software consumes events. Keep this helper
        // limited to masking command-level interrupt enables; event-ring
        // progression is driven by poll_event()/wait_command().
        let usbcmd =
            seed_usbcmd.unwrap_or_else(|| Self::read_reg_at::<u32>(mmio, op_offset + reg::USBCMD));
        let masked = masked_usbcmd(usbcmd);
        if diag_stage.is_some() {
            emit_xhci_diag(
                0x0320,
                usbcmd as u64,
                masked as u64,
                (usbcmd ^ masked) as u64,
            );
        }
        if usbcmd != masked {
            if diag_stage.is_some() {
                Self::write_reg_u32_store_diag_at(
                    mmio,
                    op_offset + reg::USBCMD,
                    masked,
                    0x0321,
                    0x0322,
                    0,
                );
            } else {
                Self::write_reg_at::<u32>(mmio, op_offset + reg::USBCMD, masked);
            }
        } else if diag_stage.is_some() {
            emit_xhci_diag(0x0323, reg::USBCMD as u64, masked as u64, 0);
        }
        let _ = (
            skip_imod_write,
            skip_erdp_write,
            skip_iman_write,
            skip_usbsts_clear_write,
        );
        if diag_stage.is_some() {
            emit_xhci_diag(0x0324, (int_base + reg::IMOD) as u64, 0, 1);
            emit_xhci_diag(
                0x032e,
                (int_base + reg::ERDP) as u64,
                erdp | reg::ERST_EHB,
                1,
            );
            emit_xhci_diag(
                0x032f,
                (int_base + reg::IMAN) as u64,
                polling_iman_value() as u64,
                1,
            );
            emit_xhci_diag(
                0x0330,
                (op_offset + reg::USBSTS) as u64,
                USBSTS_CLEAR_MASK as u64,
                1,
            );
        }
    }

    /// Create and initialize a new xHCI controller
    pub fn new(mmio_phys: usize, host: H) -> Result<Self> {
        emit_xhci_diag(0x0100, mmio_phys as u64, 0, 0);
        let host = Arc::new(host);

        // Initial map to read capability registers
        // SAFETY: The HAL maps the requested xHCI capability page as device
        // memory; the returned virtual address is unmapped before full remap.
        let init_mmio =
            unsafe { host.map_mmio(mmio_phys, MMIO_INIT_SIZE) }.ok_or(UsbError::MapFail)?;
        emit_xhci_diag(0x0101, init_mmio as u64, MMIO_INIT_SIZE as u64, 0);

        // SAFETY: The initial mapping covers the fixed xHCI capability header.
        let cap_length = unsafe { (init_mmio as *const u8).read_volatile() };
        // SAFETY: The initial mapping covers the fixed xHCI capability header.
        let hcs1: u32 = unsafe { ((init_mmio + reg::HCSPARAMS1) as *const u32).read_volatile() };
        // SAFETY: The initial mapping covers the fixed xHCI capability header.
        let hcs2: u32 = unsafe { ((init_mmio + reg::HCSPARAMS2) as *const u32).read_volatile() };
        // SAFETY: The initial mapping covers the fixed xHCI capability header.
        let hccparams1: u32 =
            unsafe { ((init_mmio + reg::HCCPARAMS1) as *const u32).read_volatile() };
        // SAFETY: The initial mapping covers the xHCI capability DBOFF register.
        let db_offset_raw: u32 =
            unsafe { ((init_mmio + reg::DBOFF) as *const u32).read_volatile() };
        // SAFETY: The initial mapping covers the xHCI capability RTSOFF register.
        let rts_offset_raw: u32 =
            unsafe { ((init_mmio + reg::RTSOFF) as *const u32).read_volatile() };
        let db_offset = xhci_dboff_offset(db_offset_raw);
        let rts_offset = xhci_rtsoff_offset(rts_offset_raw);
        emit_xhci_diag(
            0x0102,
            cap_length as u64,
            ((hcs1 as u64) << 32) | hcs2 as u64,
            ((db_offset as u64) << 32) | rts_offset as u64,
        );
        let params = XhciControllerParams {
            cap_length,
            hcs1,
            hcs2,
            hccparams1,
            db_offset,
            rts_offset,
            firmware_handoff: XhciFirmwareHandoff::None,
            runtime_seed_snapshot: None,
            apply_brcm_axi_setup: false,
            skip_constructor_live_scrub: false,
            skip_initial_live_operational_reads: false,
            port_register_access_allowed: true,
        };

        // SAFETY: `init_mmio` is the live mapping returned by `map_mmio` above
        // with the same size.
        unsafe {
            host.unmap_mmio(init_mmio, MMIO_INIT_SIZE);
        }
        Self::new_from_params_arc(mmio_phys, host, params)
    }

    /// Create and initialize a new xHCI controller using a caller-supplied
    /// capability snapshot captured from a prior safe probe.
    pub fn new_with_params(
        mmio_phys: usize,
        host: H,
        params: XhciControllerParams,
    ) -> Result<Self> {
        emit_xhci_diag(0x0100, mmio_phys as u64, 1, 0);
        Self::new_from_params_arc(mmio_phys, Arc::new(host), params)
    }

    fn new_from_params_arc(
        mmio_phys: usize,
        host: Arc<H>,
        params: XhciControllerParams,
    ) -> Result<Self> {
        let Some((max_slots, max_ports, max_scratchpad, mmio_size, ctx_size_bytes)) =
            params.validated()
        else {
            emit_xhci_diag(0x0103, 0, 0, 0);
            return Err(UsbError::MapFail);
        };
        emit_xhci_diag(
            0x0104,
            ((max_slots as u64) << 32) | max_ports as u64,
            max_scratchpad as u64,
            mmio_size as u64,
        );

        // Remap with full size
        // SAFETY: `mmio_size` is derived from validated xHCI capability
        // offsets and maps the full controller aperture through the HAL.
        let mmio = unsafe { host.map_mmio(mmio_phys, mmio_size) }.ok_or(UsbError::MapFail)?;
        emit_xhci_diag(0x0105, mmio as u64, mmio_size as u64, 0);
        Self::apply_brcm_axi_setup(mmio, params.apply_brcm_axi_setup);

        let db_offset = xhci_dboff_offset(params.db_offset);
        let rts_offset = xhci_rtsoff_offset(params.rts_offset);
        let op_base = mmio + params.cap_length as usize;
        let rt_base = mmio + rts_offset as usize;
        let op_offset = op_base - mmio;
        let int_base = rt_base + 0x20;

        // Generic xHCI probing still needs to quiesce interrupt delivery
        // immediately after mapping, before any runtime register reads. On the
        // Pi4 trusted-handoff path, the first pre-reset USBCMD write has
        // proven unsafe, so runtime skips only that store and still clears the
        // stale status / moderation / interrupter state before live halt
        // revalidation/HCRST.
        emit_xhci_diag(0x0106, op_offset as u64, rt_base as u64, int_base as u64);
        Self::write_only_polling_scrub(
            mmio,
            op_offset,
            int_base,
            constructor_polling_scrub_mode_from_params(params),
        );
        emit_xhci_diag(
            0x0107,
            USBSTS_CLEAR_MASK as u64,
            polling_iman_value() as u64,
            params.firmware_handoff as u64,
        );

        // Allocate DCBAA (Device Context Base Address Array)
        // xHCI spec requires 64-byte alignment for DCBAA
        emit_xhci_diag(0x0108, (max_slots as u64 + 1) * 8, 64, 0);
        let dcbaa = PhysMem::alloc(&*host, (max_slots as usize + 1) * 8, 64)?;
        emit_xhci_diag(0x0109, dcbaa.phys(&*host), 0, 0);

        // Allocate scratchpad if needed
        emit_xhci_diag(0x010a, max_scratchpad as u64, host.page_size() as u64, 0);
        let scratchpad = if max_scratchpad > 0 {
            let set = ScratchpadSet::build(&*host, max_scratchpad as usize)?;
            // SAFETY: DCBAA entry 0 is the scratchpad array pointer; `dcbaa`
            // is the owned controller DCBAA allocation.
            unsafe {
                dcbaa.as_ptr::<u64>().write_volatile(set.array.phys(&*host));
            }
            emit_xhci_diag(0x010b, set.array.phys(&*host), max_scratchpad as u64, 0);
            Some(set)
        } else {
            emit_xhci_diag(0x010c, 0, 0, 0);
            None
        };

        // Allocate rings on heap to reduce stack usage
        emit_xhci_diag(0x010d, CMD_RING_SIZE as u64, EVENT_RING_SIZE as u64, 0);
        let cmd_ring = Box::new(Ring::new(&*host, CMD_RING_SIZE)?);
        let event_ring = Box::new(EventRing::new(&*host, EVENT_RING_SIZE)?);
        emit_xhci_diag(
            0x010e,
            cmd_ring.phys(&*host),
            event_ring.ring_phys(&*host),
            event_ring.erst_phys(&*host),
        );

        let mut ctrl = Self {
            mmio,
            mmio_size,
            cap_length: params.cap_length,
            op_base,
            rt_base,
            db_offset,
            ctx_size_bytes,
            max_slots,
            max_ports,
            firmware_handoff: params.firmware_handoff,
            runtime_seed_snapshot: params.runtime_seed_snapshot,
            initialized: false,
            dcbaa,
            scratchpad,
            cmd_ring: Mutex::new(cmd_ring),
            event_ring: Mutex::new(event_ring),
            host,
            skip_initial_live_operational_reads: params.skip_initial_live_operational_reads,
            port_register_access_allowed: params.port_register_access_allowed,
        };

        emit_xhci_diag(0x010f, mmio as u64, op_base as u64, rt_base as u64);
        ctrl.init()?;
        ctrl.initialized = true;
        if runtime_needs_post_init_polling_irq_quiesce_with_snapshot(
            ctrl.firmware_handoff,
            ctrl.runtime_seed_snapshot,
        ) {
            ctrl.quiesce_polling_interrupts_post_init();
        }
        emit_xhci_diag(0x0110, 0, 0, 0);
        Ok(ctrl)
    }

    #[inline(always)]
    fn apply_brcm_axi_setup(mmio: usize, enabled: bool) {
        if !enabled {
            return;
        }

        emit_xhci_diag(0x0111, mmio as u64, BRCM_XHCI_AXIWRA as u64, 0);
        let axiwr_before = Self::read_reg_at::<u32>(mmio, BRCM_XHCI_AXIWRA);
        let axiwr_after = compose_brcm_usbaxi_attr(axiwr_before);
        emit_xhci_diag(
            0x0112,
            BRCM_XHCI_AXIWRA as u64,
            axiwr_before as u64,
            axiwr_after as u64,
        );
        Self::write_reg_at::<u32>(mmio, BRCM_XHCI_AXIWRA, axiwr_after);
        let axiwr_readback = Self::read_reg_at::<u32>(mmio, BRCM_XHCI_AXIWRA);
        emit_xhci_diag(
            0x0113,
            BRCM_XHCI_AXIWRA as u64,
            axiwr_readback as u64,
            axiwr_after as u64,
        );

        emit_xhci_diag(0x0116, mmio as u64, BRCM_XHCI_AXIRDA as u64, 0);
        let axird_before = Self::read_reg_at::<u32>(mmio, BRCM_XHCI_AXIRDA);
        let axird_after = compose_brcm_usbaxi_attr(axird_before);
        emit_xhci_diag(
            0x0114,
            BRCM_XHCI_AXIRDA as u64,
            axird_before as u64,
            axird_after as u64,
        );
        Self::write_reg_at::<u32>(mmio, BRCM_XHCI_AXIRDA, axird_after);
        let axird_readback = Self::read_reg_at::<u32>(mmio, BRCM_XHCI_AXIRDA);
        emit_xhci_diag(
            0x0115,
            BRCM_XHCI_AXIRDA as u64,
            axird_readback as u64,
            axird_after as u64,
        );
    }

    fn init(&mut self) -> Result<()> {
        let preserve_firmware_state = preserve_firmware_handoff_config(self.firmware_handoff);
        let trusted_runtime_seed_snapshot = self.runtime_seed_snapshot;
        let skip_initial_live_operational_reads = initial_live_operational_read_hazard(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
            self.skip_initial_live_operational_reads,
        );
        let defer_scratchpad_array_publish = defer_scratchpad_array_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let defer_dcbaap_publish = defer_dcbaap_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let defer_crcr_publish =
            defer_crcr_publish_with_snapshot(self.firmware_handoff, trusted_runtime_seed_snapshot);
        let defer_erst_publish =
            defer_erst_publish_with_snapshot(self.firmware_handoff, trusted_runtime_seed_snapshot);
        let deferred_erst_publish_uses_size_first =
            deferred_erst_publish_uses_size_first_with_snapshot(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            );
        let defer_event_ring_publish_until_after_run =
            defer_event_ring_publish_until_after_run_with_snapshot(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            );
        let defer_dcbaap_publish_until_after_run =
            defer_dcbaap_publish_until_after_run_with_snapshot(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            );
        let defer_crcr_publish_until_after_run = defer_crcr_publish_until_after_run_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let defer_dnctrl_write_until_after_run = defer_dnctrl_write_until_after_run_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let skip_fresh_event_ring_publish = skip_fresh_event_ring_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let skip_fresh_runtime_ownership_publish =
            skip_fresh_runtime_ownership_publish_with_snapshot(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            );
        let skip_dnctrl_write =
            skip_dnctrl_write_with_snapshot(self.firmware_handoff, trusted_runtime_seed_snapshot);
        let atomic_erstba_publish = use_atomic_erstba_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let defer_erdp_publish =
            defer_erdp_publish_with_snapshot(self.firmware_handoff, trusted_runtime_seed_snapshot);
        let probe_live_dcbaap_before_staged_publish =
            probe_live_dcbaap_before_staged_publish_with_snapshot(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            );
        let atomic_runtime_ring_publish = use_atomic_runtime_ring_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let live_post_reset_seed_reads = use_live_post_reset_seed_reads_for_init(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
            self.skip_initial_live_operational_reads,
        );
        let live_config_seed_reads = use_live_config_seed_reads_for_init(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
            self.skip_initial_live_operational_reads,
        );
        let skip_post_reset_verification_readbacks =
            skip_live_post_reset_verification_readbacks_for_init(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
                self.skip_initial_live_operational_reads,
            );
        let runtime_handoff_mask = u64::from(runtime_mailbox_reset_handoff(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        )) | (u64::from(runtime_mailbox_reset_stop_state_handoff(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        )) << 1)
            | (u64::from(snapshot_resetless_reinit_handoff(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            )) << 2)
            | (u64::from(runtime_preserve_stop_state_handoff(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            )) << 3);
        let publish_policy_mask = u64::from(defer_dcbaap_publish)
            | (u64::from(defer_dcbaap_publish_until_after_run) << 1)
            | (u64::from(defer_crcr_publish) << 2)
            | (u64::from(defer_crcr_publish_until_after_run) << 3)
            | (u64::from(defer_event_ring_publish_until_after_run) << 4)
            | (u64::from(defer_dnctrl_write_until_after_run) << 5)
            | (u64::from(defer_erdp_publish) << 6)
            | (u64::from(defer_erst_publish) << 7)
            | (u64::from(skip_fresh_event_ring_publish) << 8)
            | (u64::from(skip_fresh_runtime_ownership_publish) << 9);
        emit_xhci_diag(
            0x0117,
            self.firmware_handoff as u64,
            runtime_handoff_mask,
            publish_policy_mask,
        );
        if platform_reset_dcbaap_publish_blocked_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) {
            emit_xhci_diag(
                0x034c,
                self.firmware_handoff as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
                1,
            );
            return Err(UsbError::NotSupported);
        }

        // Keep host-system and event interrupts masked during bring-up. Pi4
        // local-seat uses polling and does not install xHCI IRQ handlers in
        // this phase.
        if skip_init_pre_reset_scrub_writes_for_init(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
            self.skip_initial_live_operational_reads,
        ) {
            emit_xhci_diag(
                0x0204,
                self.mmio as u64,
                self.firmware_handoff as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot)
                    | ((skip_initial_live_operational_reads as u64) << 8),
            );
        } else {
            let usbcmd_raw = self.read_op::<u32>(reg::USBCMD);
            let usbsts_start = self.read_op::<u32>(reg::USBSTS);
            emit_xhci_diag(
                0x0200,
                usbcmd_raw as u64,
                usbsts_start as u64,
                self.mmio as u64,
            );
            let usbcmd = masked_usbcmd(usbcmd_raw);
            emit_xhci_diag(
                0x0201,
                usbcmd_raw as u64,
                usbcmd as u64,
                u64::from(usbcmd_raw != usbcmd),
            );
            if usbcmd_raw != usbcmd {
                self.write_op(reg::USBCMD, usbcmd);
                emit_xhci_diag(0x0202, self.read_op::<u32>(reg::USBCMD) as u64, 0, 0);
            } else {
                emit_xhci_diag(0x0202, usbcmd_raw as u64, 0, 1);
            }
            emit_xhci_diag(0x0203, usbsts_start as u64, USBSTS_CLEAR_MASK as u64, 1);
        }

        // Some firmware/UEFI stacks leave xHCI under legacy ownership until
        // the OS-owned semaphore is asserted.
        if claim_legacy_ownership_before_reset_for_init(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
            self.skip_initial_live_operational_reads,
        ) {
            emit_xhci_diag(0x0210, 0, 0, 0);
            self.claim_legacy_ownership()?;
            emit_xhci_diag(0x0211, 0, 0, 0);
        } else {
            emit_xhci_diag(
                0x0212,
                self.firmware_handoff as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
                1 | ((skip_initial_live_operational_reads as u64) << 8),
            );
        }

        let settle_before_stop_revalidation = blind_settle_precedes_live_stop_revalidation(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        if settle_before_stop_revalidation {
            emit_xhci_diag(0x0227, MAILBOX_RESET_POST_SETTLE_SPINS as u64, 0, 0);
            for _ in 0..MAILBOX_RESET_POST_SETTLE_SPINS {
                spin_loop();
            }
            emit_xhci_diag(0x0228, MAILBOX_RESET_POST_SETTLE_SPINS as u64, 0, 0);
        }

        let pre_halt_source_quiesce = pre_halt_source_quiesce_before_live_stop_revalidation(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        if pre_halt_source_quiesce {
            Self::write_pre_halt_source_quiesce_at(
                self.mmio,
                self.op_base - self.mmio,
                trusted_runtime_seed_snapshot.and_then(|snapshot| snapshot.usbcmd),
                Some(0x0219),
            );
        }

        // Only the unseeded cold-start path still performs the first live halt
        // revalidation. Preserve/resetless, explicit full-reset-start, and the
        // seeded stop-state cold-start replay all stay on their trusted
        // snapshot contracts here.
        if skip_live_halt_revalidation_for_init(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
            self.skip_initial_live_operational_reads,
        ) {
            emit_xhci_diag(
                0x0217,
                self.firmware_handoff as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
                1 | ((skip_initial_live_operational_reads as u64) << 8),
            );
            emit_xhci_diag(
                0x0218,
                self.firmware_handoff as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
                1 | ((skip_initial_live_operational_reads as u64) << 8),
            );
            emit_xhci_diag(0x0224, 0, reg::USBSTS_HCH as u64, 1);
        } else {
            emit_xhci_diag(
                0x0217,
                self.firmware_handoff as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
                0,
            );
            emit_xhci_diag(
                0x0213,
                reg::USBSTS as u64,
                self.firmware_handoff as u64,
                trusted_runtime_seed_snapshot.is_some() as u64,
            );
            let usbsts = self.read_op::<u32>(reg::USBSTS);
            emit_xhci_diag(0x0214, usbsts as u64, reg::USBSTS_HCH as u64, 0);
            emit_xhci_diag(
                0x0215,
                reg::USBCMD as u64,
                self.firmware_handoff as u64,
                trusted_runtime_seed_snapshot.is_some() as u64,
            );
            let usbcmd_raw = self.read_op::<u32>(reg::USBCMD);
            emit_xhci_diag(0x0216, usbcmd_raw as u64, reg::USBCMD_RUN as u64, 0);
            let usbcmd = masked_usbcmd(usbcmd_raw);
            emit_xhci_diag(0x0220, usbcmd as u64, usbsts as u64, usbcmd_raw as u64);
            if !SKIP_STOP_DURING_INIT && halt_revalidation_needed(usbsts) {
                if pre_halt_source_quiesce || (usbcmd & reg::USBCMD_RUN) != 0 {
                    if !pre_halt_source_quiesce && (usbcmd & reg::USBCMD_RUN) != 0 {
                        emit_xhci_diag(0x0221, usbcmd as u64, usbcmd_raw as u64, 0);
                        self.write_op(reg::USBCMD, usbcmd & !reg::USBCMD_RUN);
                    } else {
                        emit_xhci_diag(0x021d, usbcmd as u64, usbsts as u64, usbcmd_raw as u64);
                    }
                    let mut waited = 0usize;
                    while halt_revalidation_needed(self.read_op::<u32>(reg::USBSTS)) {
                        waited = waited.saturating_add(1);
                        if waited >= STOP_WAIT_SPINS {
                            emit_xhci_diag(
                                0x0222,
                                waited as u64,
                                self.read_op::<u32>(reg::USBSTS) as u64,
                                0,
                            );
                            return Err(UsbError::Timeout);
                        }
                        spin_loop();
                    }
                    emit_xhci_diag(0x0223, self.read_op::<u32>(reg::USBSTS) as u64, 0, 0);
                } else {
                    emit_xhci_diag(0x0225, usbcmd as u64, usbsts as u64, usbcmd_raw as u64);
                }
            }
        }

        if runtime_mailbox_reset_needs_blind_settle(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) && !settle_before_stop_revalidation
        {
            emit_xhci_diag(0x0227, MAILBOX_RESET_POST_SETTLE_SPINS as u64, 0, 0);
            for _ in 0..MAILBOX_RESET_POST_SETTLE_SPINS {
                spin_loop();
            }
            emit_xhci_diag(0x0228, MAILBOX_RESET_POST_SETTLE_SPINS as u64, 0, 0);
        }

        let mut controller_reset_performed = false;
        if skip_reset_during_init_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) {
            emit_xhci_diag(
                0x0234,
                self.firmware_handoff as u64,
                trusted_runtime_seed_snapshot.is_some() as u64,
                0,
            );
        } else {
            let skip_reset_completion_poll = skip_reset_completion_poll_for_init(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
                self.skip_initial_live_operational_reads,
            );
            let usbcmd_before_reset = reset_usbcmd_seed_before_hcrst_for_init(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
                self.skip_initial_live_operational_reads,
            )
            .unwrap_or_else(|| self.read_op::<u32>(reg::USBCMD));
            emit_xhci_diag(
                0x0226,
                usbcmd_before_reset as u64,
                trusted_runtime_seed_snapshot.is_some() as u64,
                skip_reset_completion_poll as u64,
            );
            let skip_reset_pre_usbsts_read = skip_reset_pre_usbsts_read_for_init(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
                self.skip_initial_live_operational_reads,
            );
            let reset_pre_usbsts = if skip_reset_pre_usbsts_read {
                emit_xhci_diag(
                    0x022c,
                    reg::USBSTS as u64,
                    reg::USBSTS_HCH as u64,
                    self.firmware_handoff as u64,
                );
                reg::USBSTS_HCH
            } else {
                let reset_pre_usbsts = self.read_op::<u32>(reg::USBSTS);
                emit_xhci_diag(
                    0x0214,
                    reset_pre_usbsts as u64,
                    reg::USBSTS_HCH as u64,
                    1,
                );
                reset_pre_usbsts
            };
            if !skip_reset_pre_usbsts_read
                && !SKIP_STOP_DURING_INIT
                && halt_revalidation_needed(reset_pre_usbsts)
            {
                let stop_cmd = masked_usbcmd(usbcmd_before_reset) & !reg::USBCMD_RUN;
                emit_xhci_diag(
                    0x0221,
                    stop_cmd as u64,
                    usbcmd_before_reset as u64,
                    1,
                );
                self.write_op(reg::USBCMD, stop_cmd);
                let mut waited = 0usize;
                while halt_revalidation_needed(self.read_op::<u32>(reg::USBSTS)) {
                    waited = waited.saturating_add(1);
                    if waited >= STOP_WAIT_SPINS {
                        emit_xhci_diag(
                            0x0222,
                            waited as u64,
                            self.read_op::<u32>(reg::USBSTS) as u64,
                            1,
                        );
                        return Err(UsbError::Timeout);
                    }
                    spin_loop();
                }
                emit_xhci_diag(0x0223, self.read_op::<u32>(reg::USBSTS) as u64, 1, 0);
            } else if skip_reset_pre_usbsts_read {
                emit_xhci_diag(0x0225, reset_pre_usbsts as u64, reg::USBSTS_HCH as u64, 1);
            }
            let reset_cmd = usbcmd_before_reset | reg::USBCMD_HCRST;
            let uboot_style_reset_write = runtime_handoff_needs_uboot_style_reset_write(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            );
            // Reset controller
            if uboot_style_reset_write {
                self.write_op_u32_store_diag(
                    reg::USBCMD,
                    reset_cmd,
                    0x0230,
                    0x0235,
                    self.firmware_handoff as u64,
                );
            } else {
                self.write_op_u32_store_diag_with_barrier_phase(
                    reg::USBCMD,
                    reset_cmd,
                    0x0230,
                    0x023a,
                    0x0237,
                    0x0235,
                    self.firmware_handoff as u64,
                );
            }
            if skip_reset_completion_poll {
                emit_xhci_diag(0x0227, RESET_WAIT_SPINS as u64, reset_cmd as u64, 2);
                for _ in 0..RESET_WAIT_SPINS {
                    spin_loop();
                }
                emit_xhci_diag(0x0228, RESET_WAIT_SPINS as u64, reset_cmd as u64, 2);
                emit_xhci_diag(0x0229, reg::USBSTS_CNR as u64, reset_cmd as u64, 1);
                emit_xhci_diag(0x0233, 0, 0, 1);
            } else {
                let mut reset_state = self.read_op::<u32>(reg::USBCMD);
                emit_xhci_diag(0x0236, reset_state as u64, reg::USBCMD_HCRST as u64, 0);
                let mut waited = 0usize;
                while (reset_state & reg::USBCMD_HCRST) != 0 {
                    waited = waited.saturating_add(1);
                    if waited >= RESET_WAIT_SPINS {
                        emit_xhci_diag(0x0231, waited as u64, reset_state as u64, 0);
                        return Err(UsbError::Timeout);
                    }
                    spin_loop();
                    reset_state = self.read_op::<u32>(reg::USBCMD);
                }
                waited = 0;
                while (self.read_op::<u32>(reg::USBSTS) & reg::USBSTS_CNR) != 0 {
                    waited = waited.saturating_add(1);
                    if waited >= RESET_WAIT_SPINS {
                        emit_xhci_diag(
                            0x0232,
                            waited as u64,
                            self.read_op::<u32>(reg::USBSTS) as u64,
                            0,
                        );
                        return Err(UsbError::Timeout);
                    }
                    spin_loop();
                }
                emit_xhci_diag(0x0233, self.read_op::<u32>(reg::USBSTS) as u64, 0, 0);
            }
            controller_reset_performed = true;
        }

        if runtime_owned_fresh_rings_handoff(self.firmware_handoff, trusted_runtime_seed_snapshot)
            && !controller_reset_performed
            && !runtime_mailbox_reset_stop_state_handoff(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            )
        {
            emit_xhci_diag(
                0x023b,
                self.firmware_handoff as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
                0,
            );
            return Err(UsbError::NotSupported);
        }

        // Configure controller
        emit_xhci_diag(0x0240, self.max_slots as u64, 0, 0);
        if skip_config_write_during_init_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) {
            emit_xhci_diag(0x0245, reg::CONFIG as u64, self.max_slots as u64, 1);
            emit_xhci_diag(0x0241, self.max_slots as u64, 1, 0);
        } else {
            let current_config = if live_config_seed_reads {
                emit_xhci_diag(0x0246, reg::CONFIG as u64, 0, 0);
                let current = self.read_op::<u32>(reg::CONFIG);
                emit_xhci_diag(0x0241, current as u64, 0, 0);
                Some(current)
            } else {
                None
            };
            let config = current_config
                .map(|current| compose_config(current, self.max_slots))
                .unwrap_or(self.max_slots as u32);
            if current_config == Some(config) {
                // U-Boot preserves CONFIG's non-slot bits and can end up with
                // MaxSlotsEn already programmed before the final runtime
                // ownership pass. Avoid an unnecessary first live op write on
                // the trusted resetless path when the register is already
                // equivalent to the value we would publish.
                emit_xhci_diag(0x0245, reg::CONFIG as u64, config as u64, 2);
            } else {
                emit_xhci_diag(0x0243, reg::CONFIG as u64, config as u64, 0);
                self.write_op_u32_store_diag(
                    reg::CONFIG,
                    config,
                    0x0238,
                    0x0239,
                    self.firmware_handoff as u64,
                );
                if !skip_post_reset_verification_readbacks {
                    emit_xhci_diag(0x0246, reg::CONFIG as u64, 0, 1);
                    emit_xhci_diag(0x0241, self.read_op::<u32>(reg::CONFIG) as u64, 0, 1);
                }
            }
        }
        let deferred_scratchpad_array_phys = if defer_scratchpad_array_publish {
            self.scratchpad
                .as_ref()
                .map(|scratchpad| scratchpad.array.phys(&*self.host))
        } else {
            None
        };
        if let Some(scratchpad) = &self.scratchpad {
            scratchpad.share_for_device(&*self.host)?;
        }
        if deferred_scratchpad_array_phys.is_some() {
            // SAFETY: DCBAA entry 0 is the controller-owned scratchpad pointer;
            // it is intentionally kept clear until DCBAAP/CRCR/ERST are live.
            unsafe {
                self.dcbaa.as_ptr::<u64>().write_volatile(0);
            }
        }
        let dcbaa_phys = self.dcbaa.share_for_device(&*self.host, "xhci-dcbaa")?;
        let dcbaap_offset = self.op_base - self.mmio + reg::DCBAAP;
        let snapshot_dcbaap = trusted_runtime_seed_snapshot
            .and_then(|snapshot| snapshot.dcbaap)
            .unwrap_or(0);
        let live_dcbaap_before = if defer_dcbaap_publish {
            emit_xhci_diag(
                0x0257,
                reg::DCBAAP as u64,
                dcbaap_offset as u64,
                snapshot_dcbaap,
            );
            if probe_live_dcbaap_before_staged_publish {
                Some(self.read_reg_u64(dcbaap_offset))
            } else {
                None
            }
        } else {
            None
        };
        let staged_current_dcbaap = live_dcbaap_before.unwrap_or(snapshot_dcbaap);
        if defer_dcbaap_publish {
            emit_xhci_diag(
                0x0258,
                staged_current_dcbaap,
                dcbaa_phys,
                u64::from(live_dcbaap_before.is_some()),
            );
            emit_xhci_diag(
                0x02a0,
                u64_register_change_mask(staged_current_dcbaap, dcbaa_phys),
                0,
                0,
            );
        }
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        if let Some(pre_dcbaap_iman) = pre_dcbaap_iman_disable_value_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) {
            let snapshot_iman = trusted_runtime_seed_snapshot
                .and_then(|snapshot| snapshot.iman0)
                .unwrap_or(0);
            emit_xhci_diag(
                0x0332,
                snapshot_iman as u64,
                pre_dcbaap_iman as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
            );
            Self::write_reg_at::<u32>(self.mmio, int_base + reg::IMAN, pre_dcbaap_iman);
            emit_xhci_diag(
                0x0333,
                snapshot_iman as u64,
                pre_dcbaap_iman as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
            );
        }
        let release_only_dcbaap_publish =
            runtime_handoff_needs_release_only_dcbaap_publish_with_snapshot(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            );
        let current_dcbaap_publish = preserve_state_dcbaap_publish_seed(
            preserve_firmware_state,
            staged_current_dcbaap,
            dcbaa_phys,
        );
        let preserve_state_dcbaap_publish_is_redundant = preserve_firmware_state
            && preserve_state_dcbaap_write_is_redundant(current_dcbaap_publish, dcbaa_phys);
        let dcbaap_publish_policy_mask = u64::from(atomic_runtime_ring_publish)
            | (u64::from(release_only_dcbaap_publish) << 1)
            | (u64::from(preserve_firmware_state) << 2)
            | (u64::from(defer_dcbaap_publish) << 3)
            | (u64::from(defer_dcbaap_publish_until_after_run) << 4);
        let publish_dcbaap = |this: &mut Self| {
            emit_xhci_diag(
                0x02f7,
                dcbaap_publish_policy_mask,
                this.firmware_handoff as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
            );
            emit_xhci_diag(0x0244, reg::DCBAAP as u64, dcbaa_phys, 0);
            if preserve_firmware_state && TRUSTED_HANDOFF_DCBAAP_PREWRITE_READ_PROBE {
                emit_xhci_diag(0x024e, reg::DCBAAP as u64, dcbaap_offset as u64, 1);
                emit_xhci_diag(
                    0x024f,
                    this.read_reg_u64(dcbaap_offset),
                    dcbaap_offset as u64,
                    1,
                );
            }
            if preserve_firmware_state && TRUSTED_HANDOFF_DCBAAP_ZERO_REWRITE_PROBE {
                Self::write_reg_u64_atomic_diag_at(this.mmio, dcbaap_offset, 0, 0x024c, 0x024d);
            }
            if preserve_firmware_state {
                emit_xhci_diag(0x0259, reg::DCBAAP as u64, dcbaa_phys, 1);
            }
            if preserve_state_dcbaap_publish_is_redundant {
                emit_preserve_state_dcbaap_skip_diag(
                    dcbaap_offset,
                    current_dcbaap_publish,
                    dcbaa_phys,
                );
                if preserve_firmware_state {
                    emit_xhci_diag(0x0242, dcbaa_phys, 1, 0);
                } else if !skip_post_reset_verification_readbacks {
                    emit_xhci_diag(0x0247, reg::DCBAAP as u64, 0, 0);
                    emit_xhci_diag(0x0242, this.read_op_u64(reg::DCBAAP), 0, 0);
                }
                return;
            }
            if atomic_runtime_ring_publish {
                emit_xhci_diag(0x0290, reg::DCBAAP as u64, dcbaa_phys, 1);
                Self::write_reg_u64_atomic_diag_at(
                    this.mmio,
                    dcbaap_offset,
                    dcbaa_phys,
                    0x0291,
                    0x0292,
                );
            } else if release_only_dcbaap_publish {
                emit_xhci_diag(0x029c, reg::DCBAAP as u64, dcbaa_phys, 1);
                for (reg_offset, reg_value) in
                    dcbaap_reg_write_ops(dcbaap_offset, current_dcbaap_publish, dcbaa_phys)
                {
                    let (pre_stage, barrier_done_stage, pre_store_stage, done_stage) =
                        if reg_offset == dcbaap_offset {
                            (0x0248, 0x029d, 0x029e, 0x024a)
                        } else {
                            (0x0249, 0x029f, 0x02f6, 0x024b)
                        };
                    Self::write_reg_u32_store_diag_release_only_at_with_barrier_phase(
                        this.mmio,
                        reg_offset,
                        reg_value,
                        pre_stage,
                        barrier_done_stage,
                        pre_store_stage,
                        done_stage,
                        dcbaa_phys,
                    );
                }
            } else {
                Self::write_dcbaap_reg_u64_done_diag_at(
                    this.mmio,
                    dcbaap_offset,
                    current_dcbaap_publish,
                    dcbaa_phys,
                    0x0248,
                    0x024a,
                    0x0249,
                    0x024b,
                );
            }
            if preserve_firmware_state {
                emit_xhci_diag(0x0242, dcbaa_phys, 1, 0);
            } else if !skip_post_reset_verification_readbacks {
                emit_xhci_diag(0x0247, reg::DCBAAP as u64, 0, 0);
                emit_xhci_diag(0x0242, this.read_op_u64(reg::DCBAAP), 0, 0);
            }
        };
        if !defer_dcbaap_publish && !skip_fresh_runtime_ownership_publish {
            publish_dcbaap(self);
        }

        // Setup command ring. On the trusted mailbox-reset snapshot path, keep
        // CRCR from becoming the first live runtime ownership store: seed it
        // here, then publish it only after ERDP / ERST and deferred DCBAAP.
        let cmd_ring = self.cmd_ring.lock();
        // Avoid live CRCR reads before reset. Once the controller has reached
        // the post-reset U-Boot-style path, reuse the live reserved bits as
        // the seed for our CRCR publish while still skipping extra verification
        // reads on the trusted snapshot path.
        let current_crcr = if preserve_firmware_state {
            0
        } else if !probe_live_crcr_before_staged_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) || !live_post_reset_seed_reads
        {
            trusted_runtime_seed_snapshot
                .and_then(|snapshot| snapshot.crcr)
                .unwrap_or(0)
        } else {
            emit_xhci_diag(0x0253, reg::CRCR as u64, 0, 0);
            let current = self.read_op_u64(reg::CRCR);
            emit_xhci_diag(0x0251, current, 0, 0);
            current
        };
        let cmd_ring_phys = cmd_ring.share_for_device(&*self.host, "xhci-cmd-ring")?;
        let crcr = compose_crcr(current_crcr, cmd_ring_phys, true);
        let current_crcr_publish =
            preserve_state_crcr_publish_seed(preserve_firmware_state, current_crcr, crcr);
        let preserve_state_crcr_publish_is_redundant = preserve_firmware_state
            && preserve_state_crcr_write_is_redundant(current_crcr_publish, crcr);
        emit_xhci_diag(0x0250, crcr, 0, 0);
        emit_xhci_diag(0x0252, reg::CRCR as u64, crcr, 0);
        let crcr_offset = self.op_base - self.mmio + reg::CRCR;
        let publish_crcr = |this: &mut Self| {
            emit_xhci_diag(0x02aa, reg::CRCR as u64, crcr, current_crcr_publish);
            emit_xhci_diag(
                0x02ab,
                current_crcr_publish,
                crcr,
                u64_register_change_mask(current_crcr_publish, crcr),
            );
            if preserve_state_crcr_publish_is_redundant {
                emit_preserve_state_crcr_skip_diag(crcr_offset, current_crcr_publish, crcr);
                return;
            }
            Self::write_reg_u64_done_diag_at(
                this.mmio,
                crcr_offset,
                current_crcr_publish,
                0x02ac,
                0x02ad,
                0x02ae,
                0x02af,
            );
            let [target_low, target_high] = split_u64_reg_write_ops(crcr_offset, crcr);
            emit_xhci_diag(
                0x02b0,
                target_low.0 as u64,
                target_low.1 as u64,
                current_crcr_publish,
            );
            Self::write_reg_at::<u32>(this.mmio, target_low.0, target_low.1);
            emit_xhci_diag(0x02b1, target_low.0 as u64, target_low.1 as u64, crcr);
            emit_xhci_diag(
                0x02b2,
                target_high.0 as u64,
                target_high.1 as u64,
                current_crcr_publish,
            );
            Self::write_reg_at::<u32>(this.mmio, target_high.0, target_high.1);
            emit_xhci_diag(0x02b3, target_high.0 as u64, target_high.1 as u64, crcr);
            emit_xhci_diag(0x02b4, reg::CRCR as u64, crcr, 1);
        };
        if !defer_crcr_publish {
            if atomic_runtime_ring_publish {
                emit_xhci_diag(0x0293, reg::CRCR as u64, crcr, 0);
                Self::write_reg_u64_atomic_diag_at(self.mmio, crcr_offset, crcr, 0x0294, 0x0295);
            } else {
                self.write_op_u64_diag(reg::CRCR, crcr, 0x0254, 0x0255);
            }
            if preserve_firmware_state {
                emit_xhci_diag(0x0251, crcr, 1, 0);
            } else if !skip_post_reset_verification_readbacks {
                emit_xhci_diag(0x0253, reg::CRCR as u64, 0, 1);
                emit_xhci_diag(0x0251, self.read_op_u64(reg::CRCR), 0, 1);
            }
        }
        drop(cmd_ring);

        // Setup event ring
        let event_ring = self.event_ring.lock();
        let (event_ring_phys, erst_phys) = event_ring.share_for_device(&*self.host)?;
        let event_ring_dequeue = event_ring.dequeue_ptr(&*self.host);
        let erst_entries = event_ring.erst_entries();
        let staged_current_erst_size = trusted_runtime_seed_snapshot
            .and_then(|snapshot| snapshot.erstsz0)
            .unwrap_or(0);
        let staged_current_erstba = trusted_runtime_seed_snapshot
            .and_then(|snapshot| snapshot.erstba0)
            .unwrap_or(0);
        let staged_current_erdp = trusted_runtime_seed_snapshot
            .and_then(|snapshot| snapshot.erdp0)
            .unwrap_or(0);
        emit_xhci_diag(
            0x0260,
            int_base as u64,
            self.rt_base as u64,
            self.db_offset as u64,
        );

        // Prime ERDP to the first event TRB without setting EHB during init.
        let erdp = compose_initial_erdp(event_ring_phys);
        let current_erdp_publish =
            preserve_state_erdp_publish_seed(preserve_firmware_state, staged_current_erdp, erdp);
        emit_xhci_diag(0x0266, (int_base + reg::ERDP) as u64, erdp, 0);
        let publish_deferred_erdp_before_erst = defer_erdp_publish
            && !defer_event_ring_publish_until_after_run
            && deferred_erdp_publish_precedes_erst_with_snapshot(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            );
        if defer_erdp_publish {
            emit_xhci_diag(0x02b5, reg::ERDP as u64, erdp, staged_current_erdp);
            emit_xhci_diag(
                0x02b6,
                staged_current_erdp,
                erdp,
                u64_register_change_mask(staged_current_erdp, erdp),
            );
        } else if atomic_runtime_ring_publish {
            emit_xhci_diag(0x0296, (int_base + reg::ERDP) as u64, erdp, 0);
            Self::write_reg_u64_atomic_diag_at(
                self.mmio,
                int_base + reg::ERDP,
                erdp,
                0x0297,
                0x0298,
            );
        } else {
            self.write_reg_u64_diag(int_base + reg::ERDP, erdp, 0x0277, 0x0278);
        }
        if skip_fresh_event_ring_publish {
            emit_xhci_diag(
                0x032f,
                reg::ERDP as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
                1,
            );
        } else if publish_deferred_erdp_before_erst {
            let publish_deferred_erdp = |mmio: usize, include_staged_snapshot: bool| {
                if preserve_firmware_state
                    && preserve_state_erdp_write_is_redundant(current_erdp_publish, erdp)
                {
                    emit_preserve_state_erdp_skip_diag(int_base, current_erdp_publish, erdp);
                    return;
                }
                if include_staged_snapshot {
                    Self::write_reg_u64_done_diag_at(
                        mmio,
                        int_base + reg::ERDP,
                        staged_current_erdp,
                        0x02b7,
                        0x02b8,
                        0x02b9,
                        0x02ba,
                    );
                }
                // Match U-Boot's cold-start order on the Pi 4 stop-state
                // mailbox handoff: hand the deque pointer to the controller
                // before publishing ERSTSZ/ERSTBA when we are on the live
                // write path. Do not replay the staged snapshot value first:
                // U-Boot writes the live dequeue pointer directly, and the
                // extra snapshot prewrite is now the first Pi 4 runtime edge
                // to wedge.
                let [target_low, target_high] = split_u64_reg_write_ops(int_base + reg::ERDP, erdp);
                emit_xhci_diag(
                    0x02bb,
                    target_low.0 as u64,
                    target_low.1 as u64,
                    staged_current_erdp,
                );
                Self::write_reg_at::<u32>(mmio, target_low.0, target_low.1);
                emit_xhci_diag(0x02bc, target_low.0 as u64, target_low.1 as u64, erdp);
                emit_xhci_diag(
                    0x02bd,
                    target_high.0 as u64,
                    target_high.1 as u64,
                    staged_current_erdp,
                );
                Self::write_reg_at::<u32>(mmio, target_high.0, target_high.1);
                emit_xhci_diag(0x02be, target_high.0 as u64, target_high.1 as u64, erdp);
                emit_xhci_diag(0x02bf, reg::ERDP as u64, erdp, 1);
            };
            publish_deferred_erdp(self.mmio, false);
        }
        let (current_erst_size, current_erstba) = if preserve_firmware_state {
            (0, 0)
        } else if !live_post_reset_seed_reads {
            (staged_current_erst_size, staged_current_erstba)
        } else {
            emit_xhci_diag(
                0x026d,
                (int_base + reg::ERSTSZ) as u64,
                (int_base + reg::ERSTBA) as u64,
                0,
            );
            let current_erst_size = self.read_reg::<u32>(int_base + reg::ERSTSZ);
            let current_erstba = self.read_reg_u64(int_base + reg::ERSTBA);
            emit_xhci_diag(0x0261, current_erst_size as u64, current_erstba, 0);
            (current_erst_size, current_erstba)
        };
        let erst_size = compose_erst_size(
            if preserve_firmware_state || !live_post_reset_seed_reads {
                0
            } else {
                current_erst_size
            },
            erst_entries,
        );
        // Preserve-state is already trusting the bootloader's halted seed.
        // Use the desired size as the publish seed so this lane stays no-touch
        // unless a future non-preserve handoff needs an actual rewrite.
        let current_erstsz_publish = preserve_state_erstsz_publish_seed(
            preserve_firmware_state,
            staged_current_erst_size,
            erst_size,
        );
        emit_xhci_diag(0x0264, (int_base + reg::ERSTSZ) as u64, erst_size as u64, 0);
        let erstba = compose_erst_base(
            if preserve_firmware_state || !live_post_reset_seed_reads {
                0
            } else {
                current_erstba
            },
            erst_phys,
        );
        let current_erstba_publish = preserve_state_erstba_publish_seed(
            preserve_firmware_state,
            staged_current_erstba,
            erstba,
        );
        emit_xhci_diag(0x0265, (int_base + reg::ERSTBA) as u64, erstba, 0);
        if defer_erst_publish {
            emit_xhci_diag(
                0x02c0,
                (int_base + reg::ERSTSZ) as u64,
                erst_size as u64,
                current_erstsz_publish as u64,
            );
            emit_xhci_diag(
                0x02c1,
                (int_base + reg::ERSTBA) as u64,
                erstba,
                current_erstba_publish,
            );
        } else {
            emit_xhci_diag(
                0x02c2,
                (int_base + reg::ERSTSZ) as u64,
                current_erstsz_publish as u64,
                erst_size as u64,
            );
            if preserve_firmware_state
                && preserve_state_erstsz_write_is_redundant(current_erstsz_publish, erst_size)
            {
                emit_preserve_state_erstsz_skip_diag(int_base, current_erstsz_publish, erst_size);
            } else {
                Self::write_reg_u32_store_diag_at(
                    self.mmio,
                    int_base + reg::ERSTSZ,
                    erst_size,
                    0x02c3,
                    0x02c4,
                    current_erstsz_publish as u64,
                );
            }
            emit_xhci_diag(
                0x02c5,
                (int_base + reg::ERSTBA) as u64,
                current_erstba_publish,
                erstba,
            );
            if preserve_firmware_state
                && preserve_state_erstba_write_is_redundant(current_erstba_publish, erstba)
            {
                emit_preserve_state_erstba_skip_diag(int_base, current_erstba_publish, erstba);
            } else if atomic_runtime_ring_publish {
                emit_xhci_diag(0x0299, (int_base + reg::ERSTBA) as u64, erstba, 0);
                Self::write_reg_u64_atomic_diag_at(
                    self.mmio,
                    int_base + reg::ERSTBA,
                    erstba,
                    0x029a,
                    0x029b,
                );
            } else {
                Self::write_reg_u64_done_diag_at(
                    self.mmio,
                    int_base + reg::ERSTBA,
                    erstba,
                    0x02c6,
                    0x02c7,
                    0x02c8,
                    0x02c9,
                );
            }
        }
        if defer_erst_publish
            && !defer_event_ring_publish_until_after_run
            && !skip_fresh_event_ring_publish
        {
            // The stronger mailbox-reset path still needs ERSTBA-before-ERSTSZ
            // during the deferred publish ladder. The weaker stop-state-only
            // snapshot has now shown the opposite edge in live logs: its first
            // pre-RUN ERSTBA low-dword store wedges while ERSTSZ is still
            // zero. Seed ERSTSZ first on that halted stop-state branch so the
            // next live edge tests the fully-described table instead of the
            // zero-sized ERSTBA commit.
            if deferred_erst_publish_uses_size_first {
                emit_xhci_diag(
                    0x02c2,
                    (int_base + reg::ERSTSZ) as u64,
                    current_erstsz_publish as u64,
                    erst_size as u64,
                );
                if preserve_firmware_state
                    && preserve_state_erstsz_write_is_redundant(current_erstsz_publish, erst_size)
                {
                    emit_preserve_state_erstsz_skip_diag(
                        int_base,
                        current_erstsz_publish,
                        erst_size,
                    );
                } else {
                    Self::write_reg_u32_store_diag_at(
                        self.mmio,
                        int_base + reg::ERSTSZ,
                        erst_size,
                        0x02c3,
                        0x02c4,
                        current_erstsz_publish as u64,
                    );
                }
            }
            emit_xhci_diag(
                0x02c5,
                (int_base + reg::ERSTBA) as u64,
                current_erstba_publish,
                erstba,
            );
            if preserve_firmware_state
                && preserve_state_erstba_write_is_redundant(current_erstba_publish, erstba)
            {
                emit_preserve_state_erstba_skip_diag(int_base, current_erstba_publish, erstba);
            } else if atomic_erstba_publish {
                Self::write_reg_u64_atomic_diag_at(
                    self.mmio,
                    int_base + reg::ERSTBA,
                    erstba,
                    0x02c6,
                    0x02c7,
                );
            } else {
                Self::write_reg_u64_done_diag_at(
                    self.mmio,
                    int_base + reg::ERSTBA,
                    erstba,
                    0x02c6,
                    0x02c7,
                    0x02c8,
                    0x02c9,
                );
            }
            emit_xhci_diag(0x02ca, reg::ERSTBA as u64, erstba, 1);
            if !deferred_erst_publish_uses_size_first {
                emit_xhci_diag(
                    0x02c2,
                    (int_base + reg::ERSTSZ) as u64,
                    current_erstsz_publish as u64,
                    erst_size as u64,
                );
                if preserve_firmware_state
                    && preserve_state_erstsz_write_is_redundant(current_erstsz_publish, erst_size)
                {
                    emit_preserve_state_erstsz_skip_diag(
                        int_base,
                        current_erstsz_publish,
                        erst_size,
                    );
                } else {
                    Self::write_reg_u32_store_diag_at(
                        self.mmio,
                        int_base + reg::ERSTSZ,
                        erst_size,
                        0x02c3,
                        0x02c4,
                        current_erstsz_publish as u64,
                    );
                }
            }
        }
        if defer_erdp_publish
            && !defer_event_ring_publish_until_after_run
            && !publish_deferred_erdp_before_erst
            && !skip_fresh_event_ring_publish
        {
            // The trusted mailbox-reset snapshot path no longer lets ERSTSZ /
            // ERSTBA / ERDP become the first live runtime event-ring
            // ownership stores. Publish the table first, then hand ERDP to
            // the controller with the live dequeue pointer only. If the
            // trusted preserve-state seed already matches the desired queue
            // pointer, skip the write entirely and keep the lane no-touch.
            if preserve_firmware_state
                && preserve_state_erdp_write_is_redundant(current_erdp_publish, erdp)
            {
                emit_preserve_state_erdp_skip_diag(int_base, current_erdp_publish, erdp);
            } else {
                let [target_low, target_high] = split_u64_reg_write_ops(int_base + reg::ERDP, erdp);
                emit_xhci_diag(
                    0x02bb,
                    target_low.0 as u64,
                    target_low.1 as u64,
                    staged_current_erdp,
                );
                Self::write_reg_at::<u32>(self.mmio, target_low.0, target_low.1);
                emit_xhci_diag(0x02bc, target_low.0 as u64, target_low.1 as u64, erdp);
                emit_xhci_diag(
                    0x02bd,
                    target_high.0 as u64,
                    target_high.1 as u64,
                    staged_current_erdp,
                );
                Self::write_reg_at::<u32>(self.mmio, target_high.0, target_high.1);
                emit_xhci_diag(0x02be, target_high.0 as u64, target_high.1 as u64, erdp);
                emit_xhci_diag(0x02bf, reg::ERDP as u64, erdp, 1);
            }
        }
        if preserve_firmware_state {
            emit_xhci_diag(0x0262, int_base as u64, polling_iman_value() as u64, 1);
            emit_xhci_diag(
                0x0261,
                1,
                event_ring.erst_phys(&*self.host),
                event_ring.ring_phys(&*self.host) | 0x8,
            );
        } else if !skip_post_reset_verification_readbacks {
            emit_xhci_diag(0x0262, int_base as u64, polling_iman_value() as u64, 0);
            emit_xhci_diag(0x026d, int_base as u64, 0, 0);
            emit_xhci_diag(
                0x0261,
                self.read_reg::<u32>(int_base + reg::ERSTSZ) as u64,
                self.read_reg_u64(int_base + reg::ERSTBA),
                self.read_reg_u64(int_base + reg::ERDP),
            );
        }
        drop(event_ring);

        if skip_fresh_runtime_ownership_publish {
            emit_xhci_diag(
                0x0334,
                dcbaap_offset as u64,
                crcr_offset as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
            );
        } else if defer_dcbaap_publish && !defer_dcbaap_publish_until_after_run {
            // The stronger mailbox-reset snapshot no longer lets DCBAAP be the
            // first live runtime ring store. Publish ERDP / ERST first, then
            // hand DCBAAP to the controller before the final CRCR ownership
            // transfer so the next live edge stays isolated.
            emit_xhci_diag(0x025a, reg::DCBAAP as u64, dcbaa_phys, 1);
            if preserve_state_dcbaap_publish_is_redundant {
                emit_preserve_state_dcbaap_skip_diag(
                    dcbaap_offset,
                    current_dcbaap_publish,
                    dcbaa_phys,
                );
                emit_xhci_diag(0x0242, dcbaa_phys, 1, 0);
            } else {
                if replay_staged_dcbaap_snapshot_before_publish_with_snapshot(
                    self.firmware_handoff,
                    trusted_runtime_seed_snapshot,
                ) {
                    Self::write_reg_u64_done_diag_at(
                        self.mmio,
                        dcbaap_offset,
                        staged_current_dcbaap,
                        0x02a1,
                        0x02a2,
                        0x02a3,
                        0x02a4,
                    );
                }
                for (reg_offset, reg_value) in
                    dcbaap_reg_write_ops(dcbaap_offset, staged_current_dcbaap, dcbaa_phys)
                {
                    let (pre_stage, done_stage) = if reg_offset == dcbaap_offset {
                        (0x02a5, 0x02a6)
                    } else {
                        (0x02a7, 0x02a8)
                    };
                    emit_xhci_diag(
                        pre_stage,
                        reg_offset as u64,
                        reg_value as u64,
                        staged_current_dcbaap,
                    );
                    Self::write_reg_at::<u32>(self.mmio, reg_offset, reg_value);
                    emit_xhci_diag(
                        done_stage,
                        reg_offset as u64,
                        reg_value as u64,
                        staged_current_dcbaap,
                    );
                }
                emit_xhci_diag(0x02a9, reg::DCBAAP as u64, dcbaa_phys, 1);
                publish_dcbaap(self);
            }
        }
        if defer_crcr_publish
            && !defer_crcr_publish_until_after_run
            && !skip_fresh_runtime_ownership_publish
        {
            publish_crcr(self);
        }

        if let Some(scratchpad_array_phys) = deferred_scratchpad_array_phys {
            if !skip_fresh_runtime_ownership_publish {
                // SAFETY: DCBAA entry 0 remains controller-init-owned state and
                // is republished only after DCBAAP/CRCR/ERST are live.
                unsafe {
                    self.dcbaa
                        .as_ptr::<u64>()
                        .write_volatile(scratchpad_array_phys);
                }
                let _ = self.dcbaa.share_for_device(&*self.host, "xhci-dcbaa")?;
            }
        }

        // Disable device notifications before the first command can observe
        // any stale firmware-originated notification state.
        let dnctrl_value = polling_command_proof_dnctrl_value(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        if skip_dnctrl_write {
            emit_xhci_diag(0x0314, reg::DNCTRL as u64, 0, 1);
        } else if !defer_dnctrl_write_until_after_run && !skip_fresh_runtime_ownership_publish {
            emit_xhci_diag(0x0256, reg::DNCTRL as u64, dnctrl_value as u64, 0);
            self.write_op(reg::DNCTRL, dnctrl_value);
        }

        // The trusted snapshot paths follow U-Boot's polling order: publish
        // rings, start the controller, then mask the interrupter with IMOD=0
        // and IMAN=0 before root-task samples ports.
        let _ = skip_usbsts_clear_before_run_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        emit_xhci_diag(
            0x0263,
            USBSTS_CLEAR_MASK as u64,
            self.firmware_handoff as u64,
            1,
        );
        // Start controller in polling mode (interrupt delivery remains masked).
        let run_usbcmd = if let Some(snapshot_usbcmd) =
            run_usbcmd_snapshot_seed(trusted_runtime_seed_snapshot)
                .filter(|_| {
                    run_usbcmd_prefers_snapshot_seed(
                        self.firmware_handoff,
                        preserve_firmware_state,
                        trusted_runtime_seed_snapshot,
                    )
                })
                .map(masked_usbcmd)
        {
            // The Pi 4 trusted stop-state handoff must reuse the bootloader
            // snapshot as the RUN seed. A fresh live USBCMD read on these
            // seeded reset-start paths wedges VL805 before the write lands.
            let composed = compose_run_usbcmd(snapshot_usbcmd, true);
            emit_xhci_diag(
                0x02ee,
                snapshot_usbcmd as u64,
                composed as u64,
                run_usbcmd_prefers_snapshot_seed(
                    self.firmware_handoff,
                    preserve_firmware_state,
                    trusted_runtime_seed_snapshot,
                ) as u64,
            );
            composed
        } else if run_usbcmd_needs_live_seed_read(
            preserve_firmware_state,
            live_post_reset_seed_reads,
            trusted_runtime_seed_snapshot,
        ) {
            // Match U-Boot's RUN edge more closely on the preserved firmware
            // handoff path when no trusted stop-state seed exists, and keep
            // the generic live-seed path for fully unseeded controller bring-up.
            emit_xhci_diag(
                0x0275,
                reg::USBCMD as u64,
                reg::USBCMD_RUN as u64,
                preserve_firmware_state as u32 as u64,
            );
            let current = self.read_op::<u32>(reg::USBCMD);
            let composed = compose_run_usbcmd(current, true);
            emit_xhci_diag(0x02ec, current as u64, reg::USBCMD_RUN as u64, 0);
            emit_xhci_diag(0x0271, current as u64, reg::USBCMD_RUN as u64, 0);
            emit_xhci_diag(0x02ed, current as u64, composed as u64, 0);
            composed
        } else {
            emit_xhci_diag(
                0x02ef,
                reg::USBCMD_RUN as u64,
                self.firmware_handoff as u64,
                0,
            );
            reg::USBCMD_RUN
        };
        let event_generation_run_usbcmd = polling_event_generation_run_usbcmd(
            run_usbcmd,
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        if event_generation_run_usbcmd != run_usbcmd {
            emit_xhci_diag(
                0x0350,
                run_usbcmd as u64,
                event_generation_run_usbcmd as u64,
                runtime_seed_snapshot_flag_bits(trusted_runtime_seed_snapshot),
            );
        }
        let run_usbcmd = event_generation_run_usbcmd;
        if preserve_firmware_state {
            emit_xhci_diag(0x0270, 0, 1, 0);
        } else if !skip_post_reset_verification_readbacks {
            emit_xhci_diag(0x0274, reg::USBSTS as u64, 0, 0);
            emit_xhci_diag(0x0270, self.read_op::<u32>(reg::USBSTS) as u64, 0, 0);
        }
        if runtime_handoff_needs_pre_run_settle(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) {
            emit_xhci_diag(0x02e2, reg::USBCMD as u64, run_usbcmd as u64, 1);
            for _ in 0..MAILBOX_RESET_POST_SETTLE_SPINS {
                spin_loop();
            }
            emit_xhci_diag(0x02e3, reg::USBCMD as u64, run_usbcmd as u64, 1);
        }
        let skip_live_run_write = runtime_handoff_skips_live_run_write(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let relaxed_run_write = runtime_handoff_needs_relaxed_run_write(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let uboot_style_run_write = runtime_handoff_needs_uboot_style_run_write(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let release_only_run_write = runtime_handoff_needs_release_only_run_write(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let run_write_mode = if skip_live_run_write {
            4
        } else if release_only_run_write {
            2
        } else if uboot_style_run_write {
            3
        } else if relaxed_run_write {
            1
        } else {
            0
        };
        // Emit the full controller-visible ring picture immediately before the
        // RUN edge so the next Pi 4 serial trace tells us whether the board is
        // dying on bad DMA publication or only when VL805 transitions to RUN.
        let pre_run_publish_mask = u64::from(
            !defer_dcbaap_publish_until_after_run && !skip_fresh_runtime_ownership_publish,
        ) | (u64::from(
            !defer_crcr_publish_until_after_run && !skip_fresh_runtime_ownership_publish,
        ) << 1)
            | (u64::from(
                !defer_dnctrl_write_until_after_run && !skip_fresh_runtime_ownership_publish,
            ) << 2)
            | (u64::from(
                !defer_event_ring_publish_until_after_run && !skip_fresh_event_ring_publish,
            ) << 3)
            | (u64::from(
                deferred_scratchpad_array_phys.is_some() && !skip_fresh_runtime_ownership_publish,
            ) << 4);
        emit_xhci_diag(0x02f0, dcbaa_phys, cmd_ring_phys, event_ring_phys);
        emit_xhci_diag(0x02f1, erst_phys, crcr, erdp);
        emit_xhci_diag(
            0x02f2,
            staged_current_dcbaap,
            current_crcr,
            staged_current_erdp,
        );
        emit_xhci_diag(
            0x02f3,
            staged_current_erstba,
            staged_current_erst_size as u64,
            erst_size as u64,
        );
        emit_xhci_diag(
            0x02f4,
            pre_run_publish_mask,
            run_usbcmd as u64,
            run_write_mode,
        );
        emit_xhci_diag(
            0x02f5,
            dcbaap_offset as u64,
            crcr_offset as u64,
            int_base as u64,
        );
        emit_xhci_diag(
            0x02e4,
            reg::USBCMD as u64,
            run_usbcmd as u64,
            run_write_mode,
        );
        if skip_live_run_write {
            emit_xhci_diag(0x02e8, reg::USBCMD as u64, run_usbcmd as u64, 4);
        } else if release_only_run_write {
            self.write_op_u32_store_diag_release_only_with_barrier_phase(
                reg::USBCMD,
                run_usbcmd,
                0x026a,
                0x02eb,
                0x02e9,
                0x02e5,
                2,
            );
        } else if uboot_style_run_write {
            emit_xhci_diag(0x026a, reg::USBCMD as u64, run_usbcmd as u64, 3);
            emit_xhci_diag(0x02eb, reg::USBCMD as u64, run_usbcmd as u64, 3);
            mmio_write_barrier();
            emit_xhci_diag(0x02ea, reg::USBCMD as u64, run_usbcmd as u64, 3);
            emit_xhci_diag(
                0x02e9,
                (self.op_base + reg::USBCMD) as u64,
                run_usbcmd as u64,
                3,
            );
            // SAFETY: This is the trusted handoff RUN write to the live xHCI
            // USBCMD register inside the mapped operational register aperture.
            unsafe {
                ((self.op_base + reg::USBCMD) as *mut u32).write_volatile(run_usbcmd);
            }
            emit_xhci_diag(0x02e5, reg::USBCMD as u64, run_usbcmd as u64, 3);
        } else if relaxed_run_write {
            self.write_op_u32_store_diag_relaxed_with_barrier_phase(
                reg::USBCMD,
                run_usbcmd,
                0x026a,
                0x02eb,
                0x02e9,
                0x02e5,
                1,
            );
        } else {
            self.write_op_u32_store_diag_with_barrier_phase(
                reg::USBCMD,
                run_usbcmd,
                0x026a,
                0x02eb,
                0x02e9,
                0x02e5,
                0,
            );
        }
        if !skip_live_run_write {
            flush_posted_write(
                self.mmio,
                self.op_base - self.mmio + reg::USBCMD,
                run_usbcmd,
                0x02e5,
            );
        }
        let post_run_blind_settle = runtime_stop_state_needs_post_run_settle(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) || skip_initial_live_operational_reads;
        if skip_live_run_write {
            emit_xhci_diag(0x0273, 0, 0, 1);
        } else if post_run_blind_settle {
            emit_xhci_diag(0x02e0, reg::USBCMD as u64, run_usbcmd as u64, 1);
            let post_run_settle_spins = if skip_initial_live_operational_reads {
                READY_WAIT_SPINS
            } else {
                MAILBOX_RESET_POST_SETTLE_SPINS
            };
            for _ in 0..post_run_settle_spins {
                spin_loop();
            }
            emit_xhci_diag(
                0x02e1,
                reg::USBCMD as u64,
                run_usbcmd as u64,
                post_run_settle_spins as u64,
            );
            if skip_initial_live_operational_reads {
                emit_xhci_diag(0x0273, 0, 0, 2);
            }
        } else {
            let usbcmd_after_run = self.read_op::<u32>(reg::USBCMD);
            emit_xhci_diag(0x02db, usbcmd_after_run as u64, run_usbcmd as u64, 0);
            if preserve_firmware_state {
                emit_xhci_diag(0x0271, usbcmd_after_run as u64, 1, 0);
            } else if !skip_post_reset_verification_readbacks {
                emit_xhci_diag(0x0275, reg::USBCMD as u64, 0, 1);
                emit_xhci_diag(0x0271, usbcmd_after_run as u64, 0, 1);
            }
            // Wait for controller to be ready
            emit_xhci_diag(0x0276, reg::USBSTS as u64, reg::USBSTS_HCH as u64, 0);
            let mut waited = 0usize;
            let mut usbsts_after_run = self.read_op::<u32>(reg::USBSTS);
            emit_xhci_diag(0x02dc, usbsts_after_run as u64, reg::USBSTS_HCH as u64, 0);
            let mut last_observable_usbsts = run_wait_observable_usbsts(usbsts_after_run);
            while (usbsts_after_run & reg::USBSTS_HCH) != 0 {
                waited = waited.saturating_add(1);
                if run_wait_progress_due(waited) {
                    emit_xhci_diag(
                        0x02dd,
                        waited as u64,
                        usbsts_after_run as u64,
                        usbcmd_after_run as u64,
                    );
                }
                if waited >= READY_WAIT_SPINS {
                    emit_xhci_diag(
                        0x02de,
                        waited as u64,
                        usbsts_after_run as u64,
                        usbcmd_after_run as u64,
                    );
                    emit_xhci_diag(0x0272, waited as u64, usbsts_after_run as u64, 0);
                    return Err(UsbError::Timeout);
                }
                spin_loop();
                usbsts_after_run = self.read_op::<u32>(reg::USBSTS);
                let observable_usbsts = run_wait_observable_usbsts(usbsts_after_run);
                if observable_usbsts != last_observable_usbsts {
                    emit_xhci_diag(
                        0x02df,
                        waited as u64,
                        usbsts_after_run as u64,
                        last_observable_usbsts as u64,
                    );
                    last_observable_usbsts = observable_usbsts;
                }
            }
            emit_xhci_diag(0x0273, usbsts_after_run as u64, 0, 0);
        }

        // Match U-Boot's post-start ordering for interrupter state: preserve
        // firmware-owned moderation while clearing poll-visible pending state
        // after the controller is running.
        if runtime_needs_post_run_polling_irq_quiesce_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) {
            let skip_usbsts_clear_write =
                post_start_polling_irq_quiesce_skip_usbsts_clear(preserve_firmware_state);
            Self::write_post_start_polling_interrupt_quiesce_at(
                self.mmio,
                self.op_base - self.mmio,
                int_base,
                event_ring_dequeue,
                run_usbcmd_snapshot_seed(trusted_runtime_seed_snapshot)
                    .filter(|_| preserve_firmware_state),
                preserve_firmware_state,
                preserve_firmware_state,
                preserve_firmware_state,
                skip_usbsts_clear_write,
                Some(0x0316),
            );
            self.settle_post_start_polling_interrupt_quiesce(
                int_base,
                event_ring_dequeue,
                preserve_firmware_state,
                preserve_firmware_state,
                preserve_firmware_state,
                skip_usbsts_clear_write,
                0x0317,
                0x0318,
                0x0319,
            );
        } else if skip_post_run_interrupter_zeroing_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) {
            emit_xhci_diag(0x026b, (int_base + reg::IMOD) as u64, 0, 1);
            emit_xhci_diag(0x026c, (int_base + reg::IMAN) as u64, 0, 1);
        } else {
            emit_xhci_diag(0x0267, (int_base + reg::IMOD) as u64, 0, 1);
            emit_xhci_diag(0x0268, (int_base + reg::IMAN) as u64, 0, 1);
        }

        if defer_event_ring_publish_until_after_run && !skip_fresh_event_ring_publish {
            emit_xhci_diag(
                0x02cb,
                (int_base + reg::ERSTSZ) as u64,
                erst_size as u64,
                staged_current_erst_size as u64,
            );
            Self::write_reg_u32_store_diag_at(
                self.mmio,
                int_base + reg::ERSTSZ,
                erst_size,
                0x02cc,
                0x02cd,
                staged_current_erst_size as u64,
            );
            emit_xhci_diag(
                0x02ce,
                (int_base + reg::ERSTBA) as u64,
                staged_current_erstba,
                erstba,
            );
            Self::write_reg_u64_done_diag_at(
                self.mmio,
                int_base + reg::ERSTBA,
                erstba,
                0x02cf,
                0x02d0,
                0x02d1,
                0x02d2,
            );
            emit_xhci_diag(0x02d3, reg::ERSTBA as u64, erstba, 1);
            if preserve_firmware_state
                && preserve_state_erdp_write_is_redundant(current_erdp_publish, erdp)
            {
                emit_preserve_state_erdp_skip_diag(int_base, current_erdp_publish, erdp);
            } else {
                // Match the Linux/U-Boot event-ring handoff shape more closely:
                // once ERSTSZ/ERSTBA are live, publish the live dequeue pointer
                // directly. Replaying the staged snapshot value first is just an
                // extra controller-visible edge on the deferred stop-state and
                // resetless ladders.
                let [target_low, target_high] = split_u64_reg_write_ops(int_base + reg::ERDP, erdp);
                emit_xhci_diag(
                    0x02bb,
                    target_low.0 as u64,
                    target_low.1 as u64,
                    staged_current_erdp,
                );
                Self::write_reg_at::<u32>(self.mmio, target_low.0, target_low.1);
                emit_xhci_diag(0x02bc, target_low.0 as u64, target_low.1 as u64, erdp);
                emit_xhci_diag(
                    0x02bd,
                    target_high.0 as u64,
                    target_high.1 as u64,
                    staged_current_erdp,
                );
                Self::write_reg_at::<u32>(self.mmio, target_high.0, target_high.1);
                emit_xhci_diag(0x02be, target_high.0 as u64, target_high.1 as u64, erdp);
                emit_xhci_diag(0x02bf, reg::ERDP as u64, erdp, 1);
            }
        }
        if defer_dcbaap_publish_until_after_run && !skip_fresh_runtime_ownership_publish {
            emit_xhci_diag(0x02d4, reg::DCBAAP as u64, dcbaa_phys, 1);
            publish_dcbaap(self);
            emit_xhci_diag(0x02d5, reg::DCBAAP as u64, dcbaa_phys, 1);
        }
        if defer_crcr_publish_until_after_run && !skip_fresh_runtime_ownership_publish {
            emit_xhci_diag(0x02d6, reg::CRCR as u64, crcr, current_crcr);
            publish_crcr(self);
            emit_xhci_diag(0x02d7, reg::CRCR as u64, crcr, current_crcr);
        }
        if defer_dnctrl_write_until_after_run && !skip_fresh_runtime_ownership_publish {
            emit_xhci_diag(0x02d8, reg::DNCTRL as u64, dnctrl_value as u64, 1);
            self.write_op(reg::DNCTRL, dnctrl_value);
            emit_xhci_diag(0x02d9, reg::DNCTRL as u64, dnctrl_value as u64, 1);
        }

        Ok(())
    }

    fn claim_legacy_ownership(&self) -> Result<()> {
        let hccparams1 = self.read_reg::<u32>(reg::HCCPARAMS1);
        let mut ext_offset = (((hccparams1 >> 16) & 0xffff) as usize) * 4;
        if ext_offset == 0 || ext_offset >= self.mmio_size {
            return Ok(());
        }

        for _ in 0..EXT_CAP_SCAN_LIMIT {
            if ext_offset + core::mem::size_of::<u32>() > self.mmio_size {
                return Ok(());
            }
            let legacy = self.read_reg::<u32>(ext_offset);
            let cap_id = (legacy & 0xff) as u8;
            let next = ((legacy >> 8) & 0xff) as usize * 4;

            if cap_id == reg::ECAP_USB_LEGACY {
                if (legacy & USBLEGACY_BIOS_OWNED) == 0 {
                    let legacy_ctlsts =
                        self.read_reg::<u32>(ext_offset + XHCI_LEGACY_CONTROL_OFFSET);
                    emit_xhci_diag(0x022a, legacy_ctlsts as u64, ext_offset as u64, 0);
                    self.write_reg(
                        ext_offset + XHCI_LEGACY_CONTROL_OFFSET,
                        disable_legacy_smi_control_bits(legacy_ctlsts),
                    );
                    emit_xhci_diag(
                        0x022b,
                        disable_legacy_smi_control_bits(legacy_ctlsts) as u64,
                        ext_offset as u64,
                        0,
                    );
                    return Ok(());
                }
                self.write_reg(ext_offset, legacy | USBLEGACY_OS_OWNED);
                let mut waited = 0usize;
                while (self.read_reg::<u32>(ext_offset) & USBLEGACY_BIOS_OWNED) != 0 {
                    waited = waited.saturating_add(1);
                    if waited >= RESET_WAIT_SPINS {
                        return Err(UsbError::Timeout);
                    }
                    spin_loop();
                }
                let legacy_ctlsts = self.read_reg::<u32>(ext_offset + XHCI_LEGACY_CONTROL_OFFSET);
                emit_xhci_diag(0x022a, legacy_ctlsts as u64, ext_offset as u64, 1);
                self.write_reg(
                    ext_offset + XHCI_LEGACY_CONTROL_OFFSET,
                    disable_legacy_smi_control_bits(legacy_ctlsts),
                );
                emit_xhci_diag(
                    0x022b,
                    disable_legacy_smi_control_bits(legacy_ctlsts) as u64,
                    ext_offset as u64,
                    1,
                );
                return Ok(());
            }

            if next == 0 {
                return Ok(());
            }
            ext_offset = ext_offset.saturating_add(next);
            if ext_offset >= self.mmio_size {
                return Ok(());
            }
        }

        Ok(())
    }

    fn read_reg<T: Copy>(&self, offset: usize) -> T {
        Self::read_reg_at(self.mmio, offset)
    }

    fn write_reg<T: Copy>(&self, offset: usize, val: T) {
        Self::write_reg_at(self.mmio, offset, val);
    }

    #[inline(always)]
    fn write_reg_u64_diag(&self, offset: usize, val: u64, low_stage: u16, high_stage: u16) {
        Self::write_reg_u64_diag_at(self.mmio, offset, val, low_stage, high_stage);
    }

    fn read_op<T: Copy>(&self, offset: usize) -> T {
        self.read_reg(self.op_base - self.mmio + offset)
    }

    fn write_op<T: Copy>(&self, offset: usize, val: T) {
        self.write_reg(self.op_base - self.mmio + offset, val)
    }

    #[inline(always)]
    fn write_op_u32_store_diag(
        &self,
        offset: usize,
        val: u32,
        pre_stage: u16,
        done_stage: u16,
        diag_ctx: u64,
    ) {
        Self::write_reg_u32_store_diag_at(
            self.mmio,
            self.op_base - self.mmio + offset,
            val,
            pre_stage,
            done_stage,
            diag_ctx,
        );
    }

    #[inline(always)]
    fn write_op_u32_store_diag_with_barrier_phase(
        &self,
        offset: usize,
        val: u32,
        pre_stage: u16,
        barrier_done_stage: u16,
        pre_store_stage: u16,
        done_stage: u16,
        diag_ctx: u64,
    ) {
        Self::write_reg_u32_store_diag_at_with_barrier_phase(
            self.mmio,
            self.op_base - self.mmio + offset,
            val,
            pre_stage,
            barrier_done_stage,
            pre_store_stage,
            done_stage,
            diag_ctx,
        );
    }

    #[inline(always)]
    fn write_op_u32_store_diag_relaxed_with_barrier_phase(
        &self,
        offset: usize,
        val: u32,
        pre_stage: u16,
        barrier_done_stage: u16,
        pre_store_stage: u16,
        done_stage: u16,
        diag_ctx: u64,
    ) {
        Self::write_reg_u32_store_diag_relaxed_at_with_barrier_phase(
            self.mmio,
            self.op_base - self.mmio + offset,
            val,
            pre_stage,
            barrier_done_stage,
            pre_store_stage,
            done_stage,
            diag_ctx,
        );
    }

    #[inline(always)]
    fn write_op_u32_store_diag_release_only_with_barrier_phase(
        &self,
        offset: usize,
        val: u32,
        pre_stage: u16,
        barrier_done_stage: u16,
        pre_store_stage: u16,
        done_stage: u16,
        diag_ctx: u64,
    ) {
        Self::write_reg_u32_store_diag_release_only_at_with_barrier_phase(
            self.mmio,
            self.op_base - self.mmio + offset,
            val,
            pre_stage,
            barrier_done_stage,
            pre_store_stage,
            done_stage,
            diag_ctx,
        );
    }

    #[inline(always)]
    fn write_op_u64_diag(&self, offset: usize, val: u64, low_stage: u16, high_stage: u16) {
        Self::write_reg_u64_diag_at(
            self.mmio,
            self.op_base - self.mmio + offset,
            val,
            low_stage,
            high_stage,
        );
    }

    #[inline(always)]
    fn read_op_u64(&self, offset: usize) -> u64 {
        let lo = self.read_op::<u32>(offset) as u64;
        let hi = self.read_op::<u32>(offset + 4) as u64;
        (hi << 32) | lo
    }

    #[inline(always)]
    fn read_reg_u64(&self, offset: usize) -> u64 {
        let lo = self.read_reg::<u32>(offset) as u64;
        let hi = self.read_reg::<u32>(offset + 4) as u64;
        (hi << 32) | lo
    }

    /// Ring the command doorbell
    fn ring_cmd_doorbell(&self) {
        let db = reg::doorbell(self.db_offset, 0);
        let skip_readback = skip_doorbell_readback_after_ring(self.firmware_handoff);
        emit_xhci_diag(0x030f, db as u64, 0, u64::from(skip_readback));
        mmio_write_barrier();
        self.write_reg(db, 0u32);
        mmio_write_barrier();
        flush_posted_write(self.mmio, db, 0, 0x031f);
        emit_xhci_diag(0x031a, db as u64, 0, u64::from(skip_readback));
        emit_xhci_diag(0x031f, db as u64, 0, u64::from(skip_readback));
        if !skip_readback {
            let _ = self.read_reg::<u32>(db);
        }
    }

    /// Ring device doorbell
    pub fn ring_doorbell(&self, slot: u8, target: u8) {
        let db = reg::doorbell(self.db_offset, slot);
        ring_write_barrier();
        self.write_reg(db, target as u32);
        flush_posted_write(self.mmio, db, target as u32, 0x031f);
        if !skip_doorbell_readback_after_ring(self.firmware_handoff) {
            let _ = self.read_reg::<u32>(db);
        }
    }

    /// Update event ring dequeue pointer
    fn update_erdp(&self) {
        if runtime_pollsafe_no_fresh_ownership_handoff(
            self.firmware_handoff,
            self.runtime_seed_snapshot,
        ) {
            emit_xhci_diag(
                0x0335,
                reg::ERDP as u64,
                runtime_seed_snapshot_flag_bits(self.runtime_seed_snapshot),
                1,
            );
            return;
        }
        let event_ring = self.event_ring.lock();
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        Self::write_polling_interrupt_quiesce_at(
            self.mmio,
            self.op_base - self.mmio,
            int_base,
            event_ring.dequeue_ptr(&*self.host),
            None,
        );
    }

    /// Acknowledge inherited Event Handler Busy state before poll-only drains.
    pub fn clear_event_handler_busy_for_polling(&self) {
        let event_ring = self.event_ring.lock();
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        let erdp = compose_polling_erdp_ack(event_ring.dequeue_ptr(&*self.host));
        let iman_ack = polling_iman_ack_value();
        let iman_event_generation =
            polling_event_generation_iman_value(self.firmware_handoff, self.runtime_seed_snapshot);
        emit_xhci_diag(0x031b, (int_base + reg::ERDP) as u64, erdp, 0);
        let [target_low, target_high] = split_u64_reg_write_ops(int_base + reg::ERDP, erdp);
        Self::write_reg_at::<u32>(self.mmio, target_low.0, target_low.1);
        Self::write_reg_at::<u32>(self.mmio, target_high.0, target_high.1);
        emit_xhci_diag(0x031c, (int_base + reg::ERDP) as u64, erdp, 0);
        emit_xhci_diag(0x031d, (int_base + reg::IMAN) as u64, iman_ack as u64, 0);
        Self::write_reg_at::<u32>(self.mmio, int_base + reg::IMAN, iman_ack);
        emit_xhci_diag(0x031e, (int_base + reg::IMAN) as u64, iman_ack as u64, 0);
        if iman_event_generation != polling_iman_value() {
            emit_xhci_diag(
                0x0351,
                (int_base + reg::IMAN) as u64,
                iman_event_generation as u64,
                runtime_seed_snapshot_flag_bits(self.runtime_seed_snapshot),
            );
            Self::write_reg_at::<u32>(self.mmio, int_base + reg::IMAN, iman_event_generation);
            emit_xhci_diag(
                0x0352,
                (int_base + reg::IMAN) as u64,
                iman_event_generation as u64,
                runtime_seed_snapshot_flag_bits(self.runtime_seed_snapshot),
            );
        }
    }

    /// Drain inherited pending interrupter state on the polling runtime path.
    pub fn quiesce_polling_interrupts_post_init(&self) {
        let event_ring = self.event_ring.lock();
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        let preserve_firmware_state = preserve_firmware_handoff_config(self.firmware_handoff);
        let skip_usbsts_clear_write =
            post_start_polling_irq_quiesce_skip_usbsts_clear(preserve_firmware_state);
        let skip_imod_write = preserve_firmware_state;
        let skip_erdp_write = preserve_firmware_state;
        let skip_iman_write = preserve_firmware_state;
        Self::write_post_start_polling_interrupt_quiesce_at(
            self.mmio,
            self.op_base - self.mmio,
            int_base,
            event_ring.dequeue_ptr(&*self.host),
            None,
            skip_imod_write,
            skip_erdp_write,
            skip_iman_write,
            skip_usbsts_clear_write,
            Some(0x0315),
        );
        self.settle_post_start_polling_interrupt_quiesce(
            int_base,
            event_ring.dequeue_ptr(&*self.host),
            skip_imod_write,
            skip_erdp_write,
            skip_iman_write,
            skip_usbsts_clear_write,
            0x0317,
            0x0318,
            0x0319,
        );
    }

    fn settle_post_start_polling_interrupt_quiesce(
        &self,
        int_base: usize,
        erdp: u64,
        skip_imod_write: bool,
        skip_erdp_write: bool,
        skip_iman_write: bool,
        skip_usbsts_clear_write: bool,
        pending_stage: u16,
        settled_stage: u16,
        timeout_stage: u16,
    ) {
        for attempt in 0..POST_START_POLLING_IRQ_QUIESCE_RETRY_SPINS {
            let usbcmd = self.read_op::<u32>(reg::USBCMD);
            let usbsts = self.read_op::<u32>(reg::USBSTS);
            let iman = self.read_reg::<u32>(int_base + reg::IMAN);
            let pending = post_start_polling_irq_quiesce_pending_bits(
                usbcmd,
                usbsts,
                iman,
                skip_usbsts_clear_write,
            );
            let packed_state = ((usbsts as u64) << 32) | iman as u64;
            if pending == 0 {
                emit_xhci_diag(settled_stage, attempt as u64, usbcmd as u64, packed_state);
                return;
            }
            emit_xhci_diag(pending_stage, attempt as u64, usbcmd as u64, packed_state);
            Self::write_post_start_polling_interrupt_quiesce_at(
                self.mmio,
                self.op_base - self.mmio,
                int_base,
                erdp,
                None,
                skip_imod_write,
                skip_erdp_write,
                skip_iman_write,
                skip_usbsts_clear_write,
                None,
            );
            spin_loop();
        }

        let usbcmd = self.read_op::<u32>(reg::USBCMD);
        let usbsts = self.read_op::<u32>(reg::USBSTS);
        let iman = self.read_reg::<u32>(int_base + reg::IMAN);
        emit_xhci_diag(
            timeout_stage,
            POST_START_POLLING_IRQ_QUIESCE_RETRY_SPINS as u64,
            usbcmd as u64,
            ((usbsts as u64) << 32) | iman as u64,
        );
    }

    /// Wait for command completion.
    ///
    /// When `expected_cmd_trb` is provided, completion events for other
    /// command TRBs are ignored (with diagnostic breadcrumbs) until the
    /// expected command completes.
    pub fn wait_command(&self, expected_cmd_trb: Option<u64>) -> Result<Trb> {
        let mut waited = 0usize;
        let mut other_event_logs = 0usize;
        let mut last_non_command_event = None;
        let mut event_syncs = 0usize;
        loop {
            let trb = {
                let mut event_ring = self.event_ring.lock();
                if command_wait_should_sync_event_ring(waited) {
                    event_ring.sync_current_for_cpu(&*self.host, "xhci-event-ring-poll")?;
                    event_syncs = event_syncs.saturating_add(1);
                }
                event_ring.try_dequeue()
            };

            if let Some(trb) = trb {
                self.update_erdp();

                if trb.trb_type() == trb_type::COMMAND_COMPLETION as u8 {
                    let code = trb.completion_code();
                    let completion_ptr = trb.param & !0x0f;
                    if let Some(expected_ptr_raw) = expected_cmd_trb {
                        let expected_ptr = expected_ptr_raw & !0x0f;
                        let ptr_match = completion_ptr == expected_ptr;
                        emit_xhci_diag(
                            0x0304,
                            completion_ptr,
                            expected_ptr,
                            if ptr_match { 1 } else { 0 },
                        );
                        if !ptr_match {
                            emit_xhci_diag(
                                0x0305,
                                self.read_op_u64(reg::CRCR),
                                ((self.read_op::<u32>(reg::USBCMD) as u64) << 32)
                                    | self.read_op::<u32>(reg::USBSTS) as u64,
                                trb.control as u64,
                            );
                            continue;
                        }
                    }
                    emit_xhci_diag(
                        0x0301,
                        trb.param,
                        ((trb.status as u64) << 32) | trb.control as u64,
                        code as u64,
                    );
                    if code != completion::SUCCESS {
                        emit_xhci_diag(
                            0x0306,
                            self.read_op_u64(reg::CRCR),
                            self.read_op_u64(reg::DCBAAP),
                            ((self.read_op::<u32>(reg::USBCMD) as u64) << 32)
                                | self.read_op::<u32>(reg::USBSTS) as u64,
                        );
                        emit_xhci_diag(
                            0x0302,
                            code as u64,
                            trb.slot_id() as u64,
                            trb.endpoint_id() as u64,
                        );
                        return Err(UsbError::CmdFail(code));
                    }
                    return Ok(trb);
                }

                last_non_command_event = Some(trb);
                if other_event_logs < COMMAND_WAIT_OTHER_EVENT_LOGS {
                    emit_xhci_diag(
                        0x0308,
                        trb.param,
                        ((trb.status as u64) << 32) | trb.control as u64,
                        trb.trb_type() as u64,
                    );
                    other_event_logs = other_event_logs.saturating_add(1);
                }
            }

            waited = waited.saturating_add(1);
            if waited >= COMMAND_WAIT_SPINS {
                let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
                emit_xhci_diag(
                    0x0307,
                    waited as u64,
                    expected_cmd_trb.unwrap_or(0) & !0x0f,
                    event_syncs as u64,
                );
                emit_xhci_diag(
                    0x0309,
                    ((self.read_op::<u32>(reg::USBCMD) as u64) << 32)
                        | self.read_op::<u32>(reg::USBSTS) as u64,
                    ((self.read_reg::<u32>(int_base + reg::IMAN) as u64) << 32)
                        | self.read_reg::<u32>(int_base + reg::ERSTSZ) as u64,
                    self.read_op_u64(reg::DCBAAP),
                );
                if let Some(trb) = last_non_command_event {
                    emit_xhci_diag(
                        0x030a,
                        trb.param,
                        ((trb.status as u64) << 32) | trb.control as u64,
                        trb.trb_type() as u64,
                    );
                }
                return Err(UsbError::Timeout);
            }
            spin_loop();
        }
    }

    fn emit_command_event_ring_debug_snapshot(&self, base_stage: u16) {
        let event_ring = self.event_ring.lock();
        let _ = event_ring.sync_prefix_for_cpu(
            &*self.host,
            COMMAND_EVENT_RING_DEBUG_TRBS,
            "xhci-event-ring-debug-prefix",
        );
        let (dequeue, cycle) = event_ring.debug_state();
        for index in 0..COMMAND_EVENT_RING_DEBUG_TRBS {
            let state = ((index as u64) << 32) | ((dequeue as u64) << 1) | u64::from(cycle);
            let Some(trb) = event_ring.debug_trb_at(index) else {
                emit_xhci_diag(base_stage + index as u16, 0, 0, state | (1u64 << 63));
                continue;
            };
            emit_xhci_diag(
                base_stage + index as u16,
                trb.param,
                ((trb.status as u64) << 32) | trb.control as u64,
                state,
            );
        }
    }

    fn emit_command_ring_debug_snapshot(&self, base_stage: u16) {
        let cmd_ring = self.cmd_ring.lock();
        let (enqueue, cycle) = cmd_ring.debug_state();
        for index in 0..COMMAND_RING_DEBUG_TRBS {
            let state = ((index as u64) << 32) | ((enqueue as u64) << 1) | u64::from(cycle);
            let Some(trb) = cmd_ring.debug_trb_at(index) else {
                emit_xhci_diag(base_stage + index as u16, 0, 0, state | (1u64 << 63));
                continue;
            };
            emit_xhci_diag(
                base_stage + index as u16,
                trb.param,
                ((trb.status as u64) << 32) | trb.control as u64,
                state,
            );
        }
    }

    fn emit_command_gate_plan_snapshot(
        &self,
        base_stage: u16,
        expected_cmd_trb: Option<u64>,
        phase: u64,
        linux_event_generation: bool,
    ) {
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        let db = reg::doorbell(self.db_offset, 0);
        let expected_ptr = expected_cmd_trb.unwrap_or(0) & !0x0f;
        let expected_cmd_ring = expected_ptr & !((self.host.page_size() as u64).saturating_sub(1));
        let expected_dcbaap = self.dcbaa.phys(&*self.host);
        let expected_usbcmd = if linux_event_generation {
            linux_command_probe_usbcmd_seed()
        } else {
            polling_event_generation_run_usbcmd(
                reg::USBCMD_RUN,
                self.firmware_handoff,
                self.runtime_seed_snapshot,
            )
        };
        let expected_iman = if linux_event_generation {
            reg::IMAN_IE
        } else {
            polling_iman_value()
        };
        let expected_dnctrl =
            polling_command_proof_dnctrl_value(self.firmware_handoff, self.runtime_seed_snapshot);
        let (expected_erstba, expected_erstsz, expected_erdp) = {
            let event_ring = self.event_ring.lock();
            (
                event_ring.erst_phys(&*self.host),
                event_ring.erst_entries(),
                compose_polling_erdp_ack(event_ring.dequeue_ptr(&*self.host)),
            )
        };

        emit_xhci_diag(
            base_stage,
            (expected_usbcmd as u64) << 32,
            ((self.max_slots as u64) << 32) | expected_dnctrl as u64,
            expected_ptr,
        );

        emit_xhci_diag(
            base_stage + 1,
            expected_cmd_ring | 1,
            expected_dcbaap,
            ((self.db_offset as u64) << 32) | db as u64,
        );

        emit_xhci_diag(
            base_stage + 2,
            (expected_iman as u64) << 32,
            expected_erstsz as u64,
            phase,
        );

        emit_xhci_diag(
            base_stage + 3,
            expected_erstba,
            ((int_base as u64) << 32) | reg::ERDP as u64,
            expected_erdp,
        );
    }

    fn emit_command_gate_live_timeout_snapshot(&self, expected_cmd_trb: Option<u64>) {
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        let expected_ptr = expected_cmd_trb.unwrap_or(0) & !0x0f;
        let live_crcr = self.read_op_u64(reg::CRCR);
        let live_crcr_ptr = live_crcr & !0x0f;
        let live_usbcmd = self.read_op::<u32>(reg::USBCMD);
        let live_usbsts = self.read_op::<u32>(reg::USBSTS);
        let live_iman = self.read_reg::<u32>(int_base + reg::IMAN);
        let live_erstsz = self.read_reg::<u32>(int_base + reg::ERSTSZ);
        let live_erstba = self.read_reg_u64(int_base + reg::ERSTBA);
        let live_erdp = self.read_reg_u64(int_base + reg::ERDP);

        emit_xhci_diag(
            0x0374,
            live_crcr,
            expected_ptr,
            u64::from(live_crcr_ptr == expected_ptr),
        );
        emit_xhci_diag(
            0x0375,
            ((live_usbcmd as u64) << 32) | live_usbsts as u64,
            ((live_iman as u64) << 32) | live_erstsz as u64,
            self.read_op_u64(reg::DCBAAP),
        );
        emit_xhci_diag(0x0376, live_erstba, live_erdp, 0);
    }

    fn wait_command_poll_only(&self, expected_cmd_trb: Option<u64>) -> Result<Trb> {
        let mut waited = 0usize;
        let mut other_event_logs = 0usize;
        let mut last_non_command_event = None;
        let mut event_syncs = 0usize;
        let mut live_timeout_snapshot_emitted = false;
        loop {
            let trb = {
                let mut event_ring = self.event_ring.lock();
                if command_poll_only_should_sync_event_ring(waited) {
                    if event_syncs == 0 {
                        emit_xhci_diag(
                            0x037b,
                            expected_cmd_trb.unwrap_or(0) & !0x0f,
                            waited as u64,
                            0,
                        );
                    }
                    event_ring.sync_current_for_cpu(&*self.host, "xhci-event-ring-poll-fast")?;
                    event_syncs = event_syncs.saturating_add(1);
                    if event_syncs == 1 {
                        emit_xhci_diag(
                            0x037c,
                            expected_cmd_trb.unwrap_or(0) & !0x0f,
                            waited as u64,
                            event_syncs as u64,
                        );
                    }
                }
                event_ring.try_dequeue()
            };

            if let Some(trb) = trb {
                self.update_erdp();

                if trb.trb_type() == trb_type::COMMAND_COMPLETION as u8 {
                    let code = trb.completion_code();
                    let completion_ptr = trb.param & !0x0f;
                    if let Some(expected_ptr_raw) = expected_cmd_trb {
                        let expected_ptr = expected_ptr_raw & !0x0f;
                        let ptr_match = completion_ptr == expected_ptr;
                        emit_xhci_diag(
                            0x0304,
                            completion_ptr,
                            expected_ptr,
                            if ptr_match { 1 } else { 0 },
                        );
                        if !ptr_match {
                            emit_xhci_diag(
                                0x030c,
                                completion_ptr,
                                expected_ptr,
                                trb.control as u64,
                            );
                            continue;
                        }
                    }
                    emit_xhci_diag(
                        0x0301,
                        trb.param,
                        ((trb.status as u64) << 32) | trb.control as u64,
                        code as u64,
                    );
                    if code != completion::SUCCESS {
                        emit_xhci_diag(0x030d, code as u64, trb.slot_id() as u64, 0);
                        return Err(UsbError::CmdFail(code));
                    }
                    return Ok(trb);
                }

                last_non_command_event = Some(trb);
                if other_event_logs < COMMAND_WAIT_OTHER_EVENT_LOGS {
                    emit_xhci_diag(
                        0x0308,
                        trb.param,
                        ((trb.status as u64) << 32) | trb.control as u64,
                        trb.trb_type() as u64,
                    );
                    other_event_logs = other_event_logs.saturating_add(1);
                }
            }

            waited = waited.saturating_add(1);
            if command_timeout_live_snapshot_enabled()
                && !live_timeout_snapshot_emitted
                && waited >= command_timeout_live_snapshot_spins()
            {
                self.emit_command_gate_live_timeout_snapshot(expected_cmd_trb);
                live_timeout_snapshot_emitted = true;
            }
            if waited >= COMMAND_POLL_ONLY_WAIT_SPINS {
                self.emit_command_ring_debug_snapshot(0x0364);
                self.emit_command_event_ring_debug_snapshot(0x0357);
                self.emit_command_gate_plan_snapshot(0x036c, expected_cmd_trb, 2, false);
                emit_xhci_diag(
                    0x030b,
                    waited as u64,
                    expected_cmd_trb.unwrap_or(0) & !0x0f,
                    event_syncs as u64,
                );
                if command_timeout_live_snapshot_enabled() && !live_timeout_snapshot_emitted {
                    self.emit_command_gate_live_timeout_snapshot(expected_cmd_trb);
                } else if !command_timeout_live_snapshot_enabled() {
                    emit_xhci_diag(
                        0x0377,
                        expected_cmd_trb.unwrap_or(0) & !0x0f,
                        event_syncs as u64,
                        0,
                    );
                }
                if let Some(trb) = last_non_command_event {
                    emit_xhci_diag(
                        0x030e,
                        trb.param,
                        ((trb.status as u64) << 32) | trb.control as u64,
                        trb.trb_type() as u64,
                    );
                }
                return Err(UsbError::Timeout);
            }
            spin_loop();
        }
    }

    fn wait_command_prompt_safe(&self, expected_cmd_trb: Option<u64>) -> Result<Trb> {
        let mut event_syncs = 0usize;
        let mut last_non_command_event = None;
        for _ in 0..COMMAND_PROMPT_SAFE_WAIT_POLLS {
            let trb = {
                let mut event_ring = self.event_ring.lock();
                event_ring.sync_current_for_cpu(&*self.host, "xhci-event-ring-prompt-safe")?;
                event_syncs = event_syncs.saturating_add(1);
                event_ring.try_dequeue()
            };

            let Some(trb) = trb else {
                continue;
            };
            self.update_erdp();

            if trb.trb_type() == trb_type::COMMAND_COMPLETION as u8 {
                let code = trb.completion_code();
                let completion_ptr = trb.param & !0x0f;
                if let Some(expected_ptr_raw) = expected_cmd_trb {
                    let expected_ptr = expected_ptr_raw & !0x0f;
                    let ptr_match = completion_ptr == expected_ptr;
                    emit_xhci_diag(
                        0x0304,
                        completion_ptr,
                        expected_ptr,
                        if ptr_match { 1 } else { 0 },
                    );
                    if !ptr_match {
                        emit_xhci_diag(0x030c, completion_ptr, expected_ptr, trb.control as u64);
                        return Err(UsbError::Timeout);
                    }
                }
                emit_xhci_diag(
                    0x0301,
                    trb.param,
                    ((trb.status as u64) << 32) | trb.control as u64,
                    code as u64,
                );
                if code != completion::SUCCESS {
                    emit_xhci_diag(0x030d, code as u64, trb.slot_id() as u64, 0);
                    return Err(UsbError::CmdFail(code));
                }
                return Ok(trb);
            }

            last_non_command_event = Some(trb);
            emit_xhci_diag(
                0x0308,
                trb.param,
                ((trb.status as u64) << 32) | trb.control as u64,
                trb.trb_type() as u64,
            );
        }

        emit_xhci_diag(
            0x0378,
            expected_cmd_trb.unwrap_or(0) & !0x0f,
            event_syncs as u64,
            COMMAND_PROMPT_SAFE_WAIT_POLLS as u64,
        );
        self.emit_command_ring_debug_snapshot(0x0364);
        self.emit_command_event_ring_debug_snapshot(0x0357);
        self.emit_command_gate_plan_snapshot(0x036c, expected_cmd_trb, 2, false);
        emit_xhci_diag(
            0x0377,
            expected_cmd_trb.unwrap_or(0) & !0x0f,
            event_syncs as u64,
            1,
        );
        if let Some(trb) = last_non_command_event {
            emit_xhci_diag(
                0x030e,
                trb.param,
                ((trb.status as u64) << 32) | trb.control as u64,
                trb.trb_type() as u64,
            );
        }
        Err(UsbError::Timeout)
    }

    /// Poll for transfer events (non-blocking)
    pub fn poll_event(&self) -> Option<Trb> {
        let mut event_ring = self.event_ring.lock();
        if event_ring
            .sync_current_for_cpu(&*self.host, "xhci-event-ring-poll")
            .is_err()
        {
            return None;
        }
        let trb = event_ring.try_dequeue();
        drop(event_ring);
        if trb.is_some() {
            self.update_erdp();
        }
        trb
    }

    fn enable_linux_command_event_generation_for_probe(&self) {
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        let linux_usbcmd = linux_command_probe_usbcmd_seed();

        Self::write_reg_u32_store_diag_at(
            self.mmio,
            self.op_base - self.mmio + reg::USBCMD,
            linux_usbcmd,
            0x035b,
            0x035c,
            0,
        );
        Self::write_reg_u32_store_diag_at(
            self.mmio,
            int_base + reg::IMOD,
            LINUX_COMMAND_PROBE_IMOD,
            0x035d,
            0x035d,
            0,
        );
        Self::write_reg_u32_store_diag_at(
            self.mmio,
            int_base + reg::IMAN,
            reg::IMAN_IE,
            0x035e,
            0x035e,
            0,
        );
        emit_xhci_diag(
            0x035f,
            (linux_usbcmd as u64) << 32,
            ((reg::IMAN_IE as u64) << 32) | u64::from(LINUX_COMMAND_PROBE_IMOD),
            1,
        );
    }

    /// Submit a command TRB
    pub fn submit_command(&self, trb: Trb) -> Result<Trb> {
        if runtime_pollsafe_no_fresh_ownership_handoff(
            self.firmware_handoff,
            self.runtime_seed_snapshot,
        ) {
            emit_xhci_diag(
                0x0336,
                trb.param,
                ((trb.status as u64) << 32) | trb.control as u64,
                runtime_seed_snapshot_flag_bits(self.runtime_seed_snapshot),
            );
            return Err(UsbError::NotSupported);
        }
        emit_xhci_diag(
            0x0300,
            trb.param,
            ((trb.status as u64) << 32) | trb.control as u64,
            0,
        );
        let mut cmd_ring = self.cmd_ring.lock();
        let (enqueue_before, cycle_before) = cmd_ring.debug_state();
        let cmd_addr = cmd_ring.enqueue_and_sync(&*self.host, trb, "xhci-cmd-ring-submit")?;
        let (enqueue_after, cycle_after) = cmd_ring.debug_state();
        emit_xhci_diag(
            0x0303,
            cmd_addr,
            ((enqueue_before as u64) << 32) | enqueue_after as u64,
            ((cycle_before as u64) << 1) | (cycle_after as u64),
        );
        drop(cmd_ring);
        self.ring_cmd_doorbell();
        self.wait_command(Some(cmd_addr))
    }

    fn submit_command_poll_only(&self, trb: Trb) -> Result<Trb> {
        if runtime_pollsafe_no_fresh_ownership_handoff(
            self.firmware_handoff,
            self.runtime_seed_snapshot,
        ) {
            emit_xhci_diag(
                0x0336,
                trb.param,
                ((trb.status as u64) << 32) | trb.control as u64,
                runtime_seed_snapshot_flag_bits(self.runtime_seed_snapshot),
            );
            return Err(UsbError::NotSupported);
        }
        emit_xhci_diag(
            0x0300,
            trb.param,
            ((trb.status as u64) << 32) | trb.control as u64,
            1,
        );
        let mut cmd_ring = self.cmd_ring.lock();
        let (enqueue_before, cycle_before) = cmd_ring.debug_state();
        let cmd_addr = cmd_ring.enqueue_and_sync(&*self.host, trb, "xhci-cmd-ring-submit")?;
        let (enqueue_after, cycle_after) = cmd_ring.debug_state();
        emit_xhci_diag(
            0x0303,
            cmd_addr,
            ((enqueue_before as u64) << 32) | enqueue_after as u64,
            ((cycle_before as u64) << 1) | (cycle_after as u64),
        );
        drop(cmd_ring);
        self.emit_command_ring_debug_snapshot(0x0360);
        self.emit_command_event_ring_debug_snapshot(0x0353);
        self.emit_command_gate_plan_snapshot(0x0370, Some(cmd_addr), 0, false);
        self.ring_cmd_doorbell();
        self.emit_command_gate_plan_snapshot(0x0368, Some(cmd_addr), 1, false);
        self.wait_command_poll_only(Some(cmd_addr))
    }

    fn submit_command_prompt_safe_poll_only(&self, trb: Trb) -> Result<Trb> {
        if runtime_pollsafe_no_fresh_ownership_handoff(
            self.firmware_handoff,
            self.runtime_seed_snapshot,
        ) {
            emit_xhci_diag(
                0x0336,
                trb.param,
                ((trb.status as u64) << 32) | trb.control as u64,
                runtime_seed_snapshot_flag_bits(self.runtime_seed_snapshot),
            );
            return Err(UsbError::NotSupported);
        }
        emit_xhci_diag(
            0x0300,
            trb.param,
            ((trb.status as u64) << 32) | trb.control as u64,
            3,
        );
        let mut cmd_ring = self.cmd_ring.lock();
        let (enqueue_before, cycle_before) = cmd_ring.debug_state();
        let cmd_addr = cmd_ring.enqueue_and_sync(&*self.host, trb, "xhci-cmd-ring-submit")?;
        let (enqueue_after, cycle_after) = cmd_ring.debug_state();
        emit_xhci_diag(
            0x0303,
            cmd_addr,
            ((enqueue_before as u64) << 32) | enqueue_after as u64,
            ((cycle_before as u64) << 1) | (cycle_after as u64),
        );
        drop(cmd_ring);
        self.emit_command_ring_debug_snapshot(0x0360);
        self.emit_command_event_ring_debug_snapshot(0x0353);
        self.emit_command_gate_plan_snapshot(0x0370, Some(cmd_addr), 0, false);
        self.ring_cmd_doorbell();
        self.emit_command_gate_plan_snapshot(0x0368, Some(cmd_addr), 1, false);
        match self.wait_command_prompt_safe(Some(cmd_addr)) {
            Ok(trb) => Ok(trb),
            Err(UsbError::Timeout) => {
                emit_xhci_diag(0x037a, cmd_addr & !0x0f, 0, 0);
                self.enable_linux_command_event_generation_for_probe();
                if let Ok(trb) = self.wait_command_poll_only(Some(cmd_addr)) {
                    return Ok(trb);
                }
                emit_xhci_diag(
                    0x0379,
                    cmd_addr & !0x0f,
                    0,
                    COMMAND_PROMPT_SAFE_WAIT_POLLS as u64,
                );
                Err(UsbError::Timeout)
            }
            Err(err) => Err(err),
        }
    }

    fn submit_command_linux_event_generation_poll_only(&self, trb: Trb) -> Result<Trb> {
        if runtime_pollsafe_no_fresh_ownership_handoff(
            self.firmware_handoff,
            self.runtime_seed_snapshot,
        ) {
            emit_xhci_diag(
                0x0336,
                trb.param,
                ((trb.status as u64) << 32) | trb.control as u64,
                runtime_seed_snapshot_flag_bits(self.runtime_seed_snapshot),
            );
            return Err(UsbError::NotSupported);
        }
        emit_xhci_diag(
            0x0300,
            trb.param,
            ((trb.status as u64) << 32) | trb.control as u64,
            2,
        );
        self.enable_linux_command_event_generation_for_probe();
        let mut cmd_ring = self.cmd_ring.lock();
        let (enqueue_before, cycle_before) = cmd_ring.debug_state();
        let cmd_addr = cmd_ring.enqueue_and_sync(&*self.host, trb, "xhci-cmd-ring-submit")?;
        let (enqueue_after, cycle_after) = cmd_ring.debug_state();
        emit_xhci_diag(
            0x0303,
            cmd_addr,
            ((enqueue_before as u64) << 32) | enqueue_after as u64,
            ((cycle_before as u64) << 1) | (cycle_after as u64),
        );
        drop(cmd_ring);
        self.ring_cmd_doorbell();
        self.wait_command_poll_only(Some(cmd_addr))
    }

    fn submit_command_linux_event_generation_prompt_safe(&self, trb: Trb) -> Result<Trb> {
        if runtime_pollsafe_no_fresh_ownership_handoff(
            self.firmware_handoff,
            self.runtime_seed_snapshot,
        ) {
            emit_xhci_diag(
                0x0336,
                trb.param,
                ((trb.status as u64) << 32) | trb.control as u64,
                runtime_seed_snapshot_flag_bits(self.runtime_seed_snapshot),
            );
            return Err(UsbError::NotSupported);
        }
        emit_xhci_diag(
            0x0300,
            trb.param,
            ((trb.status as u64) << 32) | trb.control as u64,
            4,
        );
        self.enable_linux_command_event_generation_for_probe();
        let mut cmd_ring = self.cmd_ring.lock();
        let (enqueue_before, cycle_before) = cmd_ring.debug_state();
        let cmd_addr = cmd_ring.enqueue_and_sync(&*self.host, trb, "xhci-cmd-ring-submit")?;
        let (enqueue_after, cycle_after) = cmd_ring.debug_state();
        emit_xhci_diag(
            0x0303,
            cmd_addr,
            ((enqueue_before as u64) << 32) | enqueue_after as u64,
            ((cycle_before as u64) << 1) | (cycle_after as u64),
        );
        drop(cmd_ring);
        self.emit_command_ring_debug_snapshot(0x0360);
        self.emit_command_event_ring_debug_snapshot(0x0353);
        self.emit_command_gate_plan_snapshot(0x0370, Some(cmd_addr), 0, true);
        self.ring_cmd_doorbell();
        self.emit_command_gate_plan_snapshot(0x0368, Some(cmd_addr), 1, true);
        self.wait_command_prompt_safe(Some(cmd_addr))
    }

    /// Submit a command-ring No Op probe without touching root-port registers.
    pub fn probe_no_op_command(&self) -> Result<()> {
        self.submit_command_poll_only(no_op_command_trb_for_probe())?;
        Ok(())
    }

    /// Submit a command-ring No Op probe with no post-doorbell spin wait.
    pub fn probe_no_op_command_prompt_safe(&self) -> Result<()> {
        self.submit_command_prompt_safe_poll_only(no_op_command_trb_for_probe())?;
        Ok(())
    }

    /// Probe a No Op command using Linux-shaped event generation registers.
    pub fn probe_no_op_command_linux_event_generation(&self) -> Result<()> {
        self.submit_command_linux_event_generation_poll_only(no_op_command_trb_for_probe())?;
        Ok(())
    }

    /// Probe a No Op command with Linux-shaped event generation and bounded prompt-safe polling.
    pub fn probe_no_op_command_linux_event_generation_prompt_safe(&self) -> Result<()> {
        self.submit_command_linux_event_generation_prompt_safe(no_op_command_trb_for_probe())?;
        Ok(())
    }

    /// Enable a device slot
    pub fn enable_slot(&self) -> Result<u8> {
        let trb = Trb {
            param: 0,
            status: 0,
            control: trb_type::ENABLE_SLOT << 10,
        };
        let evt = match self.submit_command(trb) {
            Err(UsbError::Timeout) => return Err(UsbError::EnableSlotTimeout),
            Err(err) => return Err(err),
            Ok(evt) => evt,
        };
        Ok(evt.slot_id())
    }

    /// Disable a device slot
    pub fn disable_slot(&self, slot_id: u8) -> Result<()> {
        let trb = Trb {
            param: 0,
            status: 0,
            control: (trb_type::DISABLE_SLOT << 10) | ((slot_id as u32) << 24),
        };
        self.submit_command(trb)?;
        Ok(())
    }

    /// Reset a halted endpoint context.
    pub fn reset_endpoint(&self, slot_id: u8, endpoint_id: u8) -> Result<()> {
        let trb = Trb {
            param: 0,
            status: 0,
            control: (trb_type::RESET_ENDPOINT << 10)
                | ((endpoint_id as u32) << 16)
                | ((slot_id as u32) << 24),
        };
        self.submit_command(trb)?;
        Ok(())
    }

    /// Update the transfer-ring dequeue pointer for an endpoint.
    pub fn set_tr_dequeue(
        &self,
        slot_id: u8,
        endpoint_id: u8,
        dequeue_ptr: u64,
        dcs: bool,
    ) -> Result<()> {
        let trb = Trb {
            // Bits [63:4] are the dequeue pointer, bit [0] is the DCS bit.
            param: (dequeue_ptr & !0x0f) | u64::from(dcs),
            status: 0,
            control: (trb_type::SET_TR_DEQUEUE << 10)
                | ((endpoint_id as u32) << 16)
                | ((slot_id as u32) << 24),
        };
        self.submit_command(trb)?;
        Ok(())
    }

    /// Read port status
    pub fn port_status(&self, port: u8) -> u32 {
        if !self.port_in_range(port) {
            emit_xhci_diag(0x03f0, port as u64, self.max_ports as u64, 0);
            return 0;
        }
        if !self.port_register_access_allowed {
            emit_xhci_diag(0x03f5, port as u64, self.max_ports as u64, 0);
            return 0;
        }
        let offset = reg::port_reg_base(self.cap_length, port);
        if let Some(read_hook) = xhci_port_read_hook() {
            return read_hook(self.mmio, offset, port, self.max_ports);
        }
        self.read_reg(offset)
    }

    /// Write port status (for clearing change bits, reset, etc.)
    pub fn write_port_status(&self, port: u8, val: u32) {
        if !self.port_in_range(port) {
            emit_xhci_diag(0x03f1, port as u64, self.max_ports as u64, val as u64);
            return;
        }
        if !self.port_register_access_allowed {
            emit_xhci_diag(0x03f6, port as u64, self.max_ports as u64, val as u64);
            return;
        }
        let offset = reg::port_reg_base(self.cap_length, port);
        if let Some(write_hook) = xhci_port_write_hook() {
            write_hook(self.mmio, offset, port, self.max_ports, val);
            return;
        }
        self.write_reg(offset, val);
    }

    /// Reset a port
    pub fn reset_port(&self, port: u8) -> Result<()> {
        if !self.port_in_range(port) {
            emit_xhci_diag(0x03f2, port as u64, self.max_ports as u64, 0);
            return Err(UsbError::InvPort);
        }
        if !self.port_register_access_allowed {
            emit_xhci_diag(0x03f7, port as u64, self.max_ports as u64, 0);
            return Err(UsbError::NotSupported);
        }
        let mut portsc: u32 = self.port_status(port);
        emit_xhci_diag(0x0280, port as u64, encode_port_diag(portsc), 0);
        if (portsc & reg::PORTSC_CCS) == 0 {
            emit_xhci_diag(0x028f, port as u64, encode_port_diag(portsc), 0);
            return Err(UsbError::DeviceNotFound);
        }

        // Clear stale change bits before asserting reset while preserving the
        // controller-owned neutral port state (power/link ownership bits).
        let clear_changes = port_state_neutral(portsc) | PORT_CHANGE_BITS;
        self.write_port_status(port, clear_changes);
        portsc = self.port_status(port);
        emit_xhci_diag(
            0x0281,
            port as u64,
            encode_port_diag(portsc),
            clear_changes as u64,
        );

        // Keep power enabled while requesting reset.
        let reset = port_state_neutral(portsc) | reg::PORTSC_PP | reg::PORTSC_PR;
        self.write_port_status(port, reset);
        emit_xhci_diag(
            0x0282,
            port as u64,
            reset as u64,
            encode_port_diag(self.port_status(port)),
        );

        // Wait for reset to complete
        let mut waited = 0usize;
        loop {
            portsc = self.port_status(port);
            if (portsc & reg::PORTSC_PR) == 0 {
                break;
            }
            waited = waited.saturating_add(1);
            if waited >= PORT_RESET_WAIT_SPINS {
                emit_xhci_diag(0x028e, port as u64, encode_port_diag(portsc), waited as u64);
                return Err(UsbError::PortResetTimeout);
            }
            spin_loop();
        }
        emit_xhci_diag(0x0283, port as u64, encode_port_diag(portsc), waited as u64);

        // Wait for the link to settle and expose either PED or speed bits.
        waited = 0;
        loop {
            portsc = self.port_status(port);
            if port_ready_for_enumeration(portsc) {
                break;
            }
            waited = waited.saturating_add(1);
            if waited >= PORT_ENABLE_WAIT_SPINS {
                emit_xhci_diag(0x028d, port as u64, encode_port_diag(portsc), waited as u64);
                return Err(UsbError::PortEnableTimeout);
            }
            spin_loop();
        }
        emit_xhci_diag(0x0284, port as u64, encode_port_diag(portsc), waited as u64);

        // Acknowledge port status change bits after reset using RW1C bits only,
        // then ensure the port remains enumeration-ready.
        let portsc_before_ack = portsc;
        let ack_changes = port_state_neutral(portsc_before_ack) | PORT_CHANGE_BITS;
        self.write_port_status(port, ack_changes);
        portsc = self.port_status(port);
        emit_xhci_diag(
            0x0285,
            port as u64,
            encode_port_diag(portsc),
            ack_changes as u64,
        );
        let ack_write_flags = ((ack_changes & reg::PORTSC_PED != 0) as u64)
            | (((ack_changes & reg::PORTSC_PP != 0) as u64) << 1)
            | (((ack_changes & reg::PORTSC_LWS != 0) as u64) << 2)
            | (((ack_changes & reg::PORTSC_PR != 0) as u64) << 3)
            | (((ack_changes & reg::PORTSC_WPR != 0) as u64) << 4);
        emit_xhci_diag(
            0x028a,
            port as u64,
            ((portsc_before_ack as u64) << 32) | ack_changes as u64,
            ack_write_flags,
        );
        emit_xhci_diag(
            0x0288,
            port as u64,
            ((portsc_before_ack as u64) << 32) | portsc as u64,
            (portsc_before_ack ^ portsc) as u64,
        );
        emit_xhci_diag(
            0x0289,
            port as u64,
            ((portsc_before_ack as u64) << 32) | portsc as u64,
            (((portsc_before_ack & PORT_CHANGE_BITS) as u64) << 32)
                | (portsc & PORT_CHANGE_BITS) as u64,
        );

        let mut post_waited = 0usize;
        let mut transition_logs = 0usize;
        let mut last_post = portsc;
        loop {
            portsc = self.port_status(port);
            let major_change = reg::portsc_speed(portsc) != reg::portsc_speed(last_post)
                || reg::portsc_pls(portsc) != reg::portsc_pls(last_post)
                || (portsc & (reg::PORTSC_PED | reg::PORTSC_CCS))
                    != (last_post & (reg::PORTSC_PED | reg::PORTSC_CCS));
            if major_change && transition_logs < PORT_POST_ACK_TRANSITION_LOGS {
                emit_xhci_diag(
                    0x028b,
                    port as u64,
                    ((last_post as u64) << 32) | portsc as u64,
                    post_waited as u64,
                );
                transition_logs = transition_logs.saturating_add(1);
            }
            last_post = portsc;
            if port_ready_for_enumeration(portsc) {
                break;
            }
            post_waited = post_waited.saturating_add(1);
            if post_waited >= PORT_POST_ACK_WAIT_SPINS {
                emit_xhci_diag(
                    0x028c,
                    port as u64,
                    encode_port_diag(portsc),
                    post_waited as u64,
                );
                return Err(UsbError::PortEnableTimeout);
            }
            spin_loop();
        }
        emit_xhci_diag(
            0x0286,
            port as u64,
            encode_port_diag(portsc),
            post_waited as u64,
        );

        for _ in 0..PORT_SETTLE_SPINS {
            spin_loop();
        }
        let settled = self.port_status(port);
        emit_xhci_diag(
            0x0287,
            port as u64,
            encode_port_diag(settled),
            PORT_SETTLE_SPINS as u64,
        );

        Ok(())
    }

    /// Get port speed (after device is connected and port is enabled)
    pub fn port_speed(&self, port: u8) -> u8 {
        let portsc = self.port_status(port);
        ((portsc >> 10) & 0xf) as u8
    }

    /// Check if device is connected on port
    pub fn port_connected(&self, port: u8) -> bool {
        (self.port_status(port) & reg::PORTSC_CCS) != 0
    }

    /// Set device context in DCBAA
    pub fn set_device_context(&self, slot: u8, phys: u64) {
        if !self.slot_in_range(slot) {
            emit_xhci_diag(0x03f3, slot as u64, self.max_slots as u64, phys);
            return;
        }
        // SAFETY: `slot` is allocated by the controller and indexes the owned
        // DCBAA; the caller supplies the DMA address for that slot's context.
        unsafe {
            self.dcbaa
                .as_ptr::<u64>()
                .add(slot as usize)
                .write_volatile(phys);
        }
    }

    /// Reads the current DCBAA slot entry for diagnostics.
    pub fn device_context_entry(&self, slot: u8) -> u64 {
        if !self.slot_in_range(slot) {
            emit_xhci_diag(0x03f4, slot as u64, self.max_slots as u64, 0);
            return 0;
        }
        // SAFETY: `slot` is bounded by controller slot allocation when this
        // diagnostic is called; this performs a by-value volatile read.
        unsafe {
            self.dcbaa
                .as_ptr::<u64>()
                .add(slot as usize)
                .read_volatile()
        }
    }

    /// Get host reference
    pub fn host(&self) -> &H {
        &self.host
    }

    /// Get max slots
    pub fn max_slots(&self) -> u8 {
        self.max_slots
    }

    /// Get max ports
    pub fn max_ports(&self) -> u8 {
        self.max_ports
    }

    /// Returns whether direct root-port register MMIO is enabled for this controller.
    pub fn port_register_access_allowed(&self) -> bool {
        self.port_register_access_allowed
    }

    /// Returns the xHCI context stride in bytes (32 or 64).
    pub fn context_size_bytes(&self) -> usize {
        self.ctx_size_bytes
    }

    /// Captures key command/event-ring registers for timeout debugging.
    pub fn command_diag_for_port(&self, port: u8) -> XhciCommandDiag {
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        XhciCommandDiag {
            usbcmd: self.read_op::<u32>(reg::USBCMD),
            usbsts: self.read_op::<u32>(reg::USBSTS),
            crcr: self.read_op_u64(reg::CRCR),
            dcbaap: self.read_op_u64(reg::DCBAAP),
            iman: self.read_reg::<u32>(int_base + reg::IMAN),
            erdp: self.read_reg_u64(int_base + reg::ERDP),
            erstba: self.read_reg_u64(int_base + reg::ERSTBA),
            portsc: if self.port_register_access_allowed {
                self.port_status(port)
            } else {
                emit_xhci_diag(0x03f8, port as u64, self.max_ports as u64, 0);
                0
            },
        }
    }

    /// Emits an xHCI diagnostic sample through the configured hook.
    pub(crate) fn emit_diag(&self, stage: u16, a: u64, b: u64, c: u64) {
        emit_xhci_diag(stage, a, b, c);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BRCM_XHCI_USBAXI_SA_UA_MASK, BRCM_XHCI_USBAXI_SA_UA_VAL,
        COMMAND_EVENT_RING_CPU_SYNC_INTERVAL_SPINS, COMMAND_POLL_ONLY_WAIT_SPINS,
        COMMAND_PROMPT_SAFE_WAIT_POLLS, COMMAND_WAIT_SPINS, ConstructorPollingScrubMode,
        LINUX_COMMAND_PROBE_IMOD, READY_WAIT_PROGRESS_SPINS, SKIP_HCRST_DURING_INIT,
        TRUSTED_HANDOFF_DCBAAP_PREWRITE_READ_PROBE, TRUSTED_HANDOFF_DCBAAP_ZERO_REWRITE_PROBE,
        XHCI_LEGACY_DISABLE_SMI, XHCI_LEGACY_SMI_EVENTS, XhciControllerParams, XhciCtrl,
        XhciFirmwareHandoff, XhciRuntimeSeedSnapshot, blind_settle_precedes_live_stop_revalidation,
        claim_legacy_ownership_before_reset_for_init,
        claim_legacy_ownership_before_reset_with_snapshot, command_timeout_live_snapshot_enabled,
        command_poll_only_should_sync_event_ring, command_timeout_live_snapshot_spins,
        command_wait_should_sync_event_ring, compose_brcm_usbaxi_attr, compose_config,
        compose_crcr, compose_erst_base, compose_erst_size, compose_initial_erdp,
        compose_polling_erdp_ack, compose_run_usbcmd, constructor_polling_scrub_mode,
        constructor_polling_scrub_mode_from_params, dcbaap_reg_write_ops,
        defer_crcr_publish_until_after_run_with_snapshot, defer_crcr_publish_with_snapshot,
        defer_dcbaap_publish_until_after_run_with_snapshot, defer_dcbaap_publish_with_snapshot,
        defer_dnctrl_write_until_after_run_with_snapshot, defer_erdp_publish_with_snapshot,
        defer_erst_publish_with_snapshot, defer_event_ring_publish_until_after_run_with_snapshot,
        defer_scratchpad_array_publish_with_snapshot,
        deferred_erdp_publish_precedes_erst_with_snapshot,
        deferred_erst_publish_uses_size_first_with_snapshot, disable_interrupter_iman_value,
        disable_legacy_smi_control_bits, halt_revalidation_needed,
        initial_live_operational_read_hazard, linux_command_probe_usbcmd_seed, masked_usbcmd,
        no_op_command_trb_for_probe, parse_controller_params,
        platform_reset_dcbaap_publish_blocked_with_snapshot, polling_command_proof_dnctrl_value,
        polling_event_generation_iman_value, polling_event_generation_run_usbcmd,
        polling_iman_ack_value, polling_iman_value, port_ready_for_enumeration,
        post_start_polling_irq_quiesce_pending_bits,
        post_start_polling_irq_quiesce_skip_usbsts_clear,
        pre_dcbaap_iman_disable_value_with_snapshot, pre_dcbaap_polling_irq_quiesce_with_snapshot,
        pre_halt_source_quiesce_before_live_stop_revalidation, preserve_firmware_handoff_config,
        preserve_state_crcr_publish_seed, preserve_state_crcr_write_is_redundant,
        preserve_state_dcbaap_publish_seed, preserve_state_dcbaap_write_is_redundant,
        preserve_state_erdp_publish_seed, preserve_state_erdp_write_is_redundant,
        preserve_state_erstba_publish_seed, preserve_state_erstba_write_is_redundant,
        preserve_state_erstsz_publish_seed, preserve_state_erstsz_write_is_redundant,
        probe_live_crcr_before_staged_publish_with_snapshot,
        probe_live_dcbaap_before_staged_publish_with_snapshot,
        replay_staged_dcbaap_snapshot_before_publish_with_snapshot, reset_usbcmd_seed_before_hcrst,
        reset_usbcmd_seed_before_hcrst_for_init, run_usbcmd_needs_live_seed_read,
        run_usbcmd_prefers_snapshot_seed, run_usbcmd_snapshot_seed, run_wait_observable_usbsts,
        run_wait_progress_due, runtime_bootloader_owned_pollsafe_handoff,
        runtime_deferred_ring_handoff, runtime_handoff_needs_pre_run_settle,
        runtime_handoff_needs_relaxed_run_write,
        runtime_handoff_needs_release_only_dcbaap_publish_with_snapshot,
        runtime_handoff_needs_release_only_run_write,
        runtime_handoff_needs_uboot_style_reset_write, runtime_handoff_needs_uboot_style_run_write,
        runtime_handoff_skips_live_drop_stop, runtime_handoff_skips_live_run_write,
        runtime_mailbox_reset_handoff, runtime_mailbox_reset_needs_blind_settle,
        runtime_mailbox_reset_stop_state_handoff,
        runtime_needs_post_init_polling_irq_quiesce_with_snapshot,
        runtime_needs_post_run_polling_irq_quiesce_with_snapshot,
        runtime_owned_fresh_rings_handoff, runtime_platform_reset_fresh_rings_handoff,
        runtime_pollsafe_no_fresh_ownership_handoff, runtime_preserve_stop_state_handoff,
        runtime_seed_snapshot_flag_bits, runtime_seeded_full_reset_start_handoff,
        runtime_stop_state_needs_post_run_settle, runtime_unseeded_full_reset_handoff,
        skip_config_write_during_init, skip_config_write_during_init_with_snapshot,
        skip_constructor_polling_scrub_writes_with_snapshot, skip_dnctrl_write_with_snapshot,
        skip_doorbell_readback_after_ring, skip_fresh_event_ring_publish_with_snapshot,
        skip_fresh_runtime_ownership_publish_with_snapshot, skip_init_pre_reset_scrub_writes,
        skip_init_pre_reset_scrub_writes_for_init, skip_init_pre_reset_scrub_writes_with_snapshot,
        skip_legacy_ownership_claim_for_handoff,
        skip_legacy_ownership_claim_for_handoff_with_snapshot, skip_live_halt_revalidation,
        skip_live_halt_revalidation_for_init, skip_live_halt_revalidation_with_snapshot,
        skip_live_post_reset_verification_readbacks,
        skip_live_post_reset_verification_readbacks_with_snapshot,
        skip_post_reset_cnr_poll_with_snapshot, skip_post_run_interrupter_zeroing_with_snapshot,
        skip_preinit_polling_scrub, skip_reset_completion_poll_for_init,
        skip_reset_during_init, skip_reset_during_init_with_snapshot,
        skip_reset_pre_usbsts_read_for_init, skip_usbsts_clear_before_run_with_snapshot,
        snapshot_resetless_reinit_handoff, split_u64_reg_write_ops, u64_register_change_mask,
        usbcmd_interrupt_delivery_enabled, use_atomic_erstba_publish_with_snapshot,
        use_atomic_runtime_ring_publish_with_snapshot, use_live_config_seed_reads,
        use_live_config_seed_reads_for_init, use_live_config_seed_reads_with_snapshot,
        use_live_post_reset_seed_reads, use_live_post_reset_seed_reads_for_init,
        use_live_post_reset_seed_reads_with_snapshot, xhci_port_in_range, xhci_slot_in_range,
    };
    use crate::{Dma, reg, ring::trb_type};
    use alloc::vec;

    struct MockDma;

    impl Dma for MockDma {
        unsafe fn alloc(&self, _size: usize, _align: usize) -> Option<usize> {
            None
        }

        unsafe fn free(&self, _addr: usize, _size: usize, _align: usize) {}

        unsafe fn map_mmio(&self, _phys: usize, _size: usize) -> Option<usize> {
            None
        }

        unsafe fn unmap_mmio(&self, _virt: usize, _size: usize) {}

        fn virt_to_phys(&self, va: usize) -> usize {
            va
        }
    }

    #[test]
    fn parse_controller_params_rejects_all_ones() {
        assert!(
            parse_controller_params(0xff, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff)
                .is_none()
        );
    }

    #[test]
    fn no_op_command_probe_trb_has_no_slot_side_effect() {
        let trb = no_op_command_trb_for_probe();
        assert_eq!(trb.param, 0);
        assert_eq!(trb.status, 0);
        assert_eq!(trb.trb_type(), trb_type::NO_OP_CMD as u8);
        assert_eq!(trb.slot_id(), 0);
        assert_eq!(trb.endpoint_id(), 0);
    }

    #[test]
    fn parse_controller_params_accepts_reasonable_window() {
        let hcs1 = 32u32 | (8u32 << 24);
        let parsed = parse_controller_params(0x40, hcs1, 0, 0x1000, 0x2000);
        assert!(parsed.is_some());
    }

    #[test]
    fn xhci_port_and_slot_bounds_are_controller_derived() {
        assert!(xhci_port_in_range(0, 4));
        assert!(xhci_port_in_range(3, 4));
        assert!(!xhci_port_in_range(4, 4));
        assert!(xhci_slot_in_range(0, 32));
        assert!(xhci_slot_in_range(32, 32));
        assert!(!xhci_slot_in_range(33, 32));
    }

    #[test]
    fn parse_controller_params_masks_reserved_dboff_and_rtsoff_bits() {
        let hcs1 = 32u32 | (8u32 << 24);
        let parsed = parse_controller_params(0x40, hcs1, 0, 0x1003, 0x201f)
            .expect("reserved DBOFF/RTSOFF bits should be masked like U-Boot");
        assert_eq!(parsed.3, 0x10000);
    }

    #[test]
    fn controller_params_derive_context_size_from_hccparams1() {
        let params = XhciControllerParams {
            cap_length: 0x40,
            hcs1: 32u32 | (8u32 << 24),
            hcs2: 0,
            hccparams1: 1 << 2,
            db_offset: 0x1000,
            rts_offset: 0x2000,
            firmware_handoff: XhciFirmwareHandoff::None,
            runtime_seed_snapshot: None,
            apply_brcm_axi_setup: false,
            skip_constructor_live_scrub: false,
            skip_initial_live_operational_reads: false,
            port_register_access_allowed: true,
        };
        let validated = params.validated().expect("validated controller params");
        assert_eq!(validated.4, 64);
    }

    #[test]
    fn brcm_usbaxi_attr_matches_u_boot_masking() {
        assert_eq!(compose_brcm_usbaxi_attr(0), BRCM_XHCI_USBAXI_SA_UA_VAL);
        assert_eq!(
            compose_brcm_usbaxi_attr(0xffff_ffff),
            (0xffff_ffff & !BRCM_XHCI_USBAXI_SA_UA_MASK) | BRCM_XHCI_USBAXI_SA_UA_VAL
        );
    }

    #[test]
    fn trusted_runtime_seed_snapshot_skips_early_reads_and_post_reset_seed_reads() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: Some(0),
            crcr: Some(0),
            erstba0: Some(0),
            erdp0: Some(0),
            erstsz0: Some(0),
        });
        assert!(skip_live_halt_revalidation_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(runtime_mailbox_reset_handoff(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!use_live_post_reset_seed_reads_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!use_live_config_seed_reads_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!use_atomic_runtime_ring_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(use_atomic_erstba_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(skip_reset_during_init_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!skip_post_reset_cnr_poll_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
    }

    #[test]
    fn stop_state_only_snapshot_is_bootloader_owned_no_touch_without_ack() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(skip_live_halt_revalidation_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!runtime_mailbox_reset_handoff(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(runtime_mailbox_reset_stop_state_handoff(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!runtime_owned_fresh_rings_handoff(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(runtime_bootloader_owned_pollsafe_handoff(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(runtime_pollsafe_no_fresh_ownership_handoff(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!blind_settle_precedes_live_stop_revalidation(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!use_live_post_reset_seed_reads_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!use_live_config_seed_reads_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!use_atomic_runtime_ring_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!use_atomic_erstba_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(deferred_erst_publish_uses_size_first_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(skip_reset_during_init_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(runtime_mailbox_reset_needs_blind_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!blind_settle_precedes_live_stop_revalidation(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(defer_scratchpad_array_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(defer_dcbaap_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(defer_crcr_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(defer_erdp_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(defer_erst_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!deferred_erdp_publish_precedes_erst_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(skip_fresh_event_ring_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(skip_fresh_runtime_ownership_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_event_ring_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(defer_dcbaap_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(defer_crcr_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(defer_dnctrl_write_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!probe_live_dcbaap_before_staged_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!probe_live_crcr_before_staged_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!runtime_handoff_needs_uboot_style_reset_write(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!runtime_handoff_needs_uboot_style_run_write(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(runtime_handoff_skips_live_run_write(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(skip_usbsts_clear_before_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert_eq!(runtime_seed_snapshot_flag_bits(snapshot), 0b011);
    }

    #[test]
    fn usbcmd_only_seeded_stop_state_skips_live_halt_revalidation() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: None,
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(skip_live_halt_revalidation_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
    }

    #[test]
    fn runtime_seed_snapshot_flag_bits_mark_runtime_ring_seed() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: Some(0),
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert_eq!(runtime_seed_snapshot_flag_bits(snapshot), 0b111);
    }

    #[test]
    fn preserved_stop_state_snapshot_publishes_event_ring_before_run() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(runtime_preserve_stop_state_handoff(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(runtime_mailbox_reset_needs_blind_settle(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_scratchpad_array_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_dcbaap_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_crcr_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_erdp_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_erst_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(deferred_erst_publish_uses_size_first_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(!defer_event_ring_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(!defer_dcbaap_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(!defer_crcr_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(!defer_dnctrl_write_until_after_run_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(!probe_live_dcbaap_before_staged_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(!probe_live_crcr_before_staged_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
    }

    #[test]
    fn preserve_state_erstsz_skip_only_triggers_on_matching_live_size() {
        assert!(preserve_state_erstsz_write_is_redundant(1, 1));
        assert!(preserve_state_erstsz_write_is_redundant(0, 0));
        assert!(!preserve_state_erstsz_write_is_redundant(0, 1));
        assert!(!preserve_state_erstsz_write_is_redundant(1, 0));
    }

    #[test]
    fn preserve_state_erstba_skip_only_triggers_on_matching_trusted_seed_base() {
        assert!(preserve_state_erstba_write_is_redundant(0x1000, 0x1000));
        assert!(preserve_state_erstba_write_is_redundant(0, 0));
        assert!(!preserve_state_erstba_write_is_redundant(0, 0x1000));
        assert!(!preserve_state_erstba_write_is_redundant(0x1000, 0));
        assert_eq!(preserve_state_erstba_publish_seed(true, 0, 0x1234), 0x1234);
        assert_eq!(
            preserve_state_erstba_publish_seed(false, 0x5678, 0x1234),
            0x5678
        );
    }

    #[test]
    fn preserve_state_erdp_skip_only_triggers_on_matching_trusted_seed_pointer() {
        assert!(preserve_state_erdp_write_is_redundant(0x1000, 0x1000));
        assert!(preserve_state_erdp_write_is_redundant(0, 0));
        assert!(!preserve_state_erdp_write_is_redundant(0, 0x1000));
        assert!(!preserve_state_erdp_write_is_redundant(0x1000, 0));
        assert_eq!(preserve_state_erdp_publish_seed(true, 0, 0x1234), 0x1234);
        assert_eq!(
            preserve_state_erdp_publish_seed(false, 0x5678, 0x1234),
            0x5678
        );
    }

    #[test]
    fn preserve_state_erstsz_publish_seed_prefers_desired_size() {
        assert_eq!(preserve_state_erstsz_publish_seed(true, 0, 1), 1);
        assert_eq!(preserve_state_erstsz_publish_seed(true, 4, 1), 1);
        assert_eq!(preserve_state_erstsz_publish_seed(false, 0, 1), 0);
    }

    #[test]
    fn stop_state_snapshot_skips_live_run_and_post_ready_interrupter_mask() {
        let stop_state_snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        let runtime_ring_snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: Some(0x1000),
            crcr: Some(0x2000),
            erstba0: Some(0x3000),
            erdp0: Some(0x4000),
            erstsz0: Some(1),
        });
        assert!(!runtime_stop_state_needs_post_run_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(skip_post_run_interrupter_zeroing_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(runtime_handoff_needs_pre_run_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(runtime_handoff_needs_pre_run_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            runtime_ring_snapshot,
        ));
        assert!(runtime_handoff_needs_relaxed_run_write(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            runtime_ring_snapshot,
        ));
        assert!(!runtime_stop_state_needs_post_run_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(skip_post_run_interrupter_zeroing_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(runtime_handoff_needs_pre_run_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(!runtime_handoff_needs_relaxed_run_write(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_uboot_style_reset_write(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_uboot_style_run_write(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(runtime_handoff_skips_live_run_write(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(runtime_handoff_needs_uboot_style_run_write(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(runtime_unseeded_full_reset_handoff(
            XhciFirmwareHandoff::None,
            None,
        ));
        assert!(runtime_handoff_needs_uboot_style_reset_write(
            XhciFirmwareHandoff::None,
            None,
        ));
        assert!(
            !runtime_handoff_needs_release_only_dcbaap_publish_with_snapshot(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                stop_state_snapshot,
            )
        );
        assert!(runtime_handoff_needs_pre_run_settle(
            XhciFirmwareHandoff::None,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_relaxed_run_write(
            XhciFirmwareHandoff::None,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_uboot_style_reset_write(
            XhciFirmwareHandoff::None,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_uboot_style_run_write(
            XhciFirmwareHandoff::None,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_release_only_run_write(
            XhciFirmwareHandoff::None,
            stop_state_snapshot,
        ));
        assert!(
            !runtime_handoff_needs_release_only_dcbaap_publish_with_snapshot(
                XhciFirmwareHandoff::None,
                stop_state_snapshot,
            )
        );
        assert!(runtime_pollsafe_no_fresh_ownership_handoff(
            XhciFirmwareHandoff::None,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_pre_run_settle(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_relaxed_run_write(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_uboot_style_reset_write(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_uboot_style_run_write(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(runtime_handoff_skips_live_run_write(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_release_only_run_write(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(skip_post_run_interrupter_zeroing_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(skip_post_run_interrupter_zeroing_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            None,
        ));
        assert!(!runtime_needs_post_run_polling_irq_quiesce_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(!runtime_needs_post_init_polling_irq_quiesce_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(!runtime_needs_post_init_polling_irq_quiesce_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            None,
        ));
        assert!(runtime_stop_state_needs_post_run_settle(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(skip_post_run_interrupter_zeroing_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(!runtime_needs_post_run_polling_irq_quiesce_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(skip_post_run_interrupter_zeroing_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(!runtime_needs_post_run_polling_irq_quiesce_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(runtime_handoff_needs_pre_run_settle(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_relaxed_run_write(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_uboot_style_run_write(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_needs_release_only_run_write(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(
            !runtime_handoff_needs_release_only_dcbaap_publish_with_snapshot(
                XhciFirmwareHandoff::ResetlessReinit,
                stop_state_snapshot,
            )
        );
        assert!(!runtime_handoff_needs_release_only_run_write(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            runtime_ring_snapshot,
        ));
        assert!(
            !runtime_handoff_needs_release_only_dcbaap_publish_with_snapshot(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                runtime_ring_snapshot,
            )
        );
        assert!(runtime_stop_state_needs_post_run_settle(
            XhciFirmwareHandoff::PlatformResetComplete,
            stop_state_snapshot,
        ));
        assert!(!runtime_stop_state_needs_post_run_settle(
            XhciFirmwareHandoff::PlatformResetComplete,
            runtime_ring_snapshot,
        ));
        assert!(replay_staged_dcbaap_snapshot_before_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            runtime_ring_snapshot,
        ));
        assert!(replay_staged_dcbaap_snapshot_before_publish_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(!replay_staged_dcbaap_snapshot_before_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(preserve_state_dcbaap_write_is_redundant(0, 0));
        assert!(preserve_state_dcbaap_write_is_redundant(0x1000, 0x1000));
        assert!(!preserve_state_dcbaap_write_is_redundant(0, 0x1000));
        assert!(!preserve_state_dcbaap_write_is_redundant(0x1000, 0));
        assert_eq!(preserve_state_dcbaap_publish_seed(true, 0, 0x1234), 0x1234);
        assert_eq!(
            preserve_state_dcbaap_publish_seed(false, 0x5678, 0x1234),
            0x5678
        );
        assert!(preserve_state_crcr_write_is_redundant(0, 0));
        assert!(preserve_state_crcr_write_is_redundant(0x1000, 0x1000));
        assert!(!preserve_state_crcr_write_is_redundant(0, 0x1000));
        assert!(!preserve_state_crcr_write_is_redundant(0x1000, 0));
        assert_eq!(preserve_state_crcr_publish_seed(true, 0, 0x1234), 0x1234);
        assert_eq!(
            preserve_state_crcr_publish_seed(false, 0x5678, 0x1234),
            0x5678
        );
        assert!(!defer_dcbaap_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(defer_dcbaap_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(defer_dcbaap_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            runtime_ring_snapshot,
        ));
        assert!(defer_dcbaap_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(defer_crcr_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(!defer_crcr_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(defer_crcr_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(defer_dnctrl_write_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(!defer_dnctrl_write_until_after_run_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(defer_dnctrl_write_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(!skip_dnctrl_write_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(skip_dnctrl_write_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(!skip_dnctrl_write_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
    }

    #[test]
    fn trusted_runtime_snapshot_defers_scratchpad_array_publish() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: None,
            usbsts: None,
            iman0: None,
            dcbaap: Some(0),
            crcr: Some(0),
            erstba0: Some(0),
            erdp0: Some(0),
            erstsz0: Some(0),
        });
        assert!(defer_scratchpad_array_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_scratchpad_array_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(!defer_scratchpad_array_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_scratchpad_array_publish_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
        assert!(!use_atomic_runtime_ring_publish_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
        assert!(!use_atomic_erstba_publish_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
    }

    #[test]
    fn trusted_runtime_snapshot_defers_dcbaap_publish_until_after_other_ring_state() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: None,
            usbsts: None,
            iman0: None,
            dcbaap: Some(0),
            crcr: Some(0),
            erstba0: Some(0),
            erdp0: Some(0),
            erstsz0: Some(0),
        });
        assert!(defer_dcbaap_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_dcbaap_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(!defer_dcbaap_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_dcbaap_publish_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
        assert!(defer_dcbaap_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_dcbaap_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_dcbaap_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
    }

    #[test]
    fn trusted_runtime_snapshot_defers_crcr_publish_until_after_other_ring_state() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: None,
            usbsts: None,
            iman0: None,
            dcbaap: Some(0),
            crcr: Some(0),
            erstba0: Some(0),
            erdp0: Some(0),
            erstsz0: Some(0),
        });
        assert!(defer_crcr_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_crcr_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(!defer_crcr_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_crcr_publish_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
        assert!(defer_crcr_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(defer_crcr_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
        assert!(!probe_live_crcr_before_staged_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(probe_live_crcr_before_staged_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
    }

    #[test]
    fn trusted_runtime_snapshot_defers_erdp_publish_until_after_erst_programming() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: None,
            usbsts: None,
            iman0: None,
            dcbaap: Some(0),
            crcr: Some(0),
            erstba0: Some(0),
            erdp0: Some(0),
            erstsz0: Some(0),
        });
        assert!(defer_erdp_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(defer_erdp_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(!defer_erdp_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_erdp_publish_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
    }

    #[test]
    fn trusted_runtime_snapshot_defers_erst_publish_until_after_late_event_ring_handoff() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: None,
            usbsts: None,
            iman0: None,
            dcbaap: Some(0),
            crcr: Some(0),
            erstba0: Some(0),
            erdp0: Some(0),
            erstsz0: Some(0),
        });
        assert!(defer_erst_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!deferred_erst_publish_uses_size_first_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_erst_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(!defer_erst_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_erst_publish_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
        assert!(deferred_erst_publish_uses_size_first_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
        assert!(!deferred_erdp_publish_precedes_erst_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
        assert!(!defer_event_ring_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_event_ring_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(defer_event_ring_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
    }

    #[test]
    fn stop_state_snapshot_uses_bootloader_owned_pollsafe_event_ring() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(runtime_bootloader_owned_pollsafe_handoff(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(skip_fresh_event_ring_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(skip_fresh_runtime_ownership_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!deferred_erdp_publish_precedes_erst_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_event_ring_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
    }

    #[test]
    fn trusted_runtime_snapshot_skips_live_dcbaap_read_before_staged_publish() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: None,
            usbsts: None,
            iman0: None,
            dcbaap: Some(0),
            crcr: Some(0),
            erstba0: Some(0),
            erdp0: Some(0),
            erstsz0: Some(0),
        });
        assert!(!probe_live_dcbaap_before_staged_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(probe_live_dcbaap_before_staged_publish_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(!probe_live_dcbaap_before_staged_publish_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
    }

    #[test]
    fn trusted_stop_state_snapshot_defers_dcbaap_without_live_probe() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(!probe_live_dcbaap_before_staged_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
    }

    #[test]
    fn u64_register_change_mask_reports_low_and_high_dword_changes_independently() {
        assert_eq!(
            u64_register_change_mask(0x0000_0001_0000_0002, 0x0000_0001_0000_0002),
            0
        );
        assert_eq!(
            u64_register_change_mask(0x0000_0001_0000_0002, 0x0000_0001_0000_0003),
            1
        );
        assert_eq!(
            u64_register_change_mask(0x0000_0001_0000_0002, 0x0000_0002_0000_0002),
            2
        );
        assert_eq!(
            u64_register_change_mask(0x0000_0001_0000_0002, 0x0000_0002_0000_0003),
            3
        );
    }

    #[test]
    fn dcbaap_write_ops_publish_low_before_high_when_high_dword_changes() {
        assert_eq!(
            dcbaap_reg_write_ops(0x50, 0x0000_0000_0000_0000, 0x0000_0004_0400_3000),
            [(0x50, 0x0400_3000), (0x54, 0x0000_0004)]
        );
    }

    #[test]
    fn platform_reset_complete_dcbaap_publish_is_allowed_after_ownership_proof() {
        let stop_seed = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        let runtime_ring_seed = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: Some(0x4003_000),
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });

        assert!(
            !runtime_handoff_needs_release_only_dcbaap_publish_with_snapshot(
                XhciFirmwareHandoff::PlatformResetComplete,
                None,
            )
        );
        assert!(!platform_reset_dcbaap_publish_blocked_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(!platform_reset_dcbaap_publish_blocked_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            stop_seed,
        ));
        assert!(!platform_reset_dcbaap_publish_blocked_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            runtime_ring_seed,
        ));
        assert!(!platform_reset_dcbaap_publish_blocked_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
    }

    #[test]
    fn dcbaap_write_ops_keep_low_before_high_when_high_dword_is_stable() {
        assert_eq!(
            dcbaap_reg_write_ops(0x50, 0x0000_0004_0000_0000, 0x0000_0004_0400_3000),
            [(0x50, 0x0400_3000), (0x54, 0x0000_0004)]
        );
        assert_eq!(
            dcbaap_reg_write_ops(0x50, 0x0000_0000_0000_0000, 0x0000_0000_0400_3000),
            [(0x50, 0x0400_3000), (0x54, 0x0000_0000)]
        );
    }

    #[test]
    fn port_ready_requires_enabled_for_usb2() {
        let portsc_connected_full_speed = reg::PORTSC_CCS | ((reg::SPEED_FULL as u32) << 10);
        assert!(!port_ready_for_enumeration(portsc_connected_full_speed));

        let portsc_connected_enabled_full_speed = portsc_connected_full_speed | reg::PORTSC_PED;
        assert!(port_ready_for_enumeration(
            portsc_connected_enabled_full_speed
        ));
    }

    #[test]
    fn port_ready_accepts_superspeed_without_ped() {
        let portsc_connected_superspeed = reg::PORTSC_CCS | ((reg::SPEED_SUPER as u32) << 10);
        assert!(port_ready_for_enumeration(portsc_connected_superspeed));
    }

    #[test]
    fn port_ready_rejects_missing_speed_or_connect() {
        assert!(!port_ready_for_enumeration(0));
        assert!(!port_ready_for_enumeration(reg::PORTSC_CCS));
    }

    #[test]
    fn polling_mode_keeps_interrupter_disabled() {
        assert_eq!(polling_iman_value(), 0);
        assert_eq!(polling_iman_value() & reg::IMAN_IE, 0);
    }

    #[test]
    fn polling_iman_ack_clears_pending_without_enabling_interrupts() {
        assert_eq!(polling_iman_ack_value(), reg::IMAN_IP);
        assert_eq!(polling_iman_ack_value() & reg::IMAN_IE, 0);
    }

    #[test]
    fn reset_owned_stop_seed_leaves_iman_untouched_before_deferred_dcbaap() {
        let pending_enabled_snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(reg::IMAN_IP | reg::IMAN_IE),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert_eq!(
            disable_interrupter_iman_value(reg::IMAN_IP | reg::IMAN_IE),
            0
        );
        assert_eq!(
            disable_interrupter_iman_value(0xffff_ffff),
            0xffff_ffff & !(reg::IMAN_IP | reg::IMAN_IE)
        );
        assert_eq!(
            pre_dcbaap_iman_disable_value_with_snapshot(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                pending_enabled_snapshot,
            ),
            None
        );
        assert!(!pre_dcbaap_polling_irq_quiesce_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            pending_enabled_snapshot,
        ));
        assert!(!pre_dcbaap_polling_irq_quiesce_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(!pre_dcbaap_polling_irq_quiesce_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            pending_enabled_snapshot,
        ));

        let pending_disabled_snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(reg::IMAN_IP),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert_eq!(
            pre_dcbaap_iman_disable_value_with_snapshot(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                pending_disabled_snapshot,
            ),
            None
        );
        assert!(!pre_dcbaap_polling_irq_quiesce_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            pending_disabled_snapshot,
        ));
        let reset_owned_pending_disabled_seed = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(reg::IMAN_IP),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert_eq!(
            pre_dcbaap_iman_disable_value_with_snapshot(
                XhciFirmwareHandoff::None,
                reset_owned_pending_disabled_seed
            ),
            None
        );
        assert!(!pre_dcbaap_polling_irq_quiesce_with_snapshot(
            XhciFirmwareHandoff::None,
            reset_owned_pending_disabled_seed
        ));
        let reset_owned_pending_enabled_seed = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(reg::IMAN_IP | reg::IMAN_IE),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert_eq!(
            pre_dcbaap_iman_disable_value_with_snapshot(
                XhciFirmwareHandoff::None,
                reset_owned_pending_enabled_seed
            ),
            None
        );
        assert!(!pre_dcbaap_polling_irq_quiesce_with_snapshot(
            XhciFirmwareHandoff::None,
            reset_owned_pending_enabled_seed
        ));
        assert_eq!(
            pre_dcbaap_iman_disable_value_with_snapshot(XhciFirmwareHandoff::None, None),
            None
        );
    }

    #[test]
    fn masked_usbcmd_clears_interrupt_enables_only() {
        let raw = reg::USBCMD_RUN | reg::USBCMD_INTE | reg::USBCMD_HSEE;
        assert_eq!(masked_usbcmd(raw), reg::USBCMD_RUN);
        assert!(usbcmd_interrupt_delivery_enabled(raw));
        assert!(!usbcmd_interrupt_delivery_enabled(reg::USBCMD_RUN));
    }

    #[test]
    fn full_constructor_scrub_skips_redundant_usbcmd_write() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        let int_base = 0x180;
        XhciCtrl::<MockDma>::write_reg_at::<u32>(
            mmio_base,
            op_offset + reg::USBCMD,
            reg::USBCMD_RUN,
        );

        XhciCtrl::<MockDma>::write_only_polling_scrub(
            mmio_base,
            op_offset,
            int_base,
            ConstructorPollingScrubMode::Full,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD),
            reg::USBCMD_RUN
        );
    }

    #[test]
    fn full_constructor_scrub_masks_only_interrupt_delivery_bits() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        let int_base = 0x180;
        XhciCtrl::<MockDma>::write_reg_at::<u32>(
            mmio_base,
            op_offset + reg::USBCMD,
            reg::USBCMD_RUN | reg::USBCMD_INTE | reg::USBCMD_HSEE,
        );

        XhciCtrl::<MockDma>::write_only_polling_scrub(
            mmio_base,
            op_offset,
            int_base,
            ConstructorPollingScrubMode::Full,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD),
            reg::USBCMD_RUN
        );
    }

    #[test]
    fn preserve_handoff_run_usbcmd_prefers_snapshot_seed_when_available() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(reg::USBCMD_EWE | reg::USBCMD_INTE),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        let current =
            masked_usbcmd(run_usbcmd_snapshot_seed(snapshot).expect("snapshot preserves usbcmd"));
        assert!(!run_usbcmd_needs_live_seed_read(true, false, snapshot));
        assert_eq!(current, reg::USBCMD_EWE);
        assert_eq!(
            compose_run_usbcmd(current, true),
            reg::USBCMD_EWE | reg::USBCMD_RUN
        );
    }

    #[test]
    fn cold_start_snapshot_run_usbcmd_prefers_stop_state_seed_when_available() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(reg::USBCMD_EWE | reg::USBCMD_INTE),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        let current =
            masked_usbcmd(run_usbcmd_snapshot_seed(snapshot).expect("snapshot preserves usbcmd"));
        assert!(run_usbcmd_prefers_snapshot_seed(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            false,
            snapshot,
        ));
        assert_eq!(current, reg::USBCMD_EWE);
        assert_eq!(
            compose_run_usbcmd(current, true),
            reg::USBCMD_EWE | reg::USBCMD_RUN
        );
    }

    #[test]
    fn preserve_handoff_run_usbcmd_falls_back_to_live_seed_read_without_snapshot() {
        let current = reg::USBCMD_HSEE;
        assert!(run_usbcmd_needs_live_seed_read(true, false, None));
        assert_eq!(compose_run_usbcmd(current, true), reg::USBCMD_RUN);
    }

    #[test]
    fn blind_run_usbcmd_path_stays_bare_when_no_live_seed_read_is_allowed() {
        let current = reg::USBCMD_HSEE;
        assert!(!run_usbcmd_needs_live_seed_read(false, false, None));
        assert_eq!(compose_run_usbcmd(current, false), reg::USBCMD_RUN);
        assert_eq!(compose_run_usbcmd(current, true), reg::USBCMD_RUN);
    }

    #[test]
    fn platform_reset_stop_seed_keeps_command_proof_poll_only() {
        let stop_seed = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });

        assert_eq!(
            polling_event_generation_run_usbcmd(
                reg::USBCMD_RUN,
                XhciFirmwareHandoff::PlatformResetComplete,
                stop_seed,
            ),
            reg::USBCMD_RUN
        );
        assert_eq!(
            polling_event_generation_iman_value(
                XhciFirmwareHandoff::PlatformResetComplete,
                stop_seed,
            ),
            polling_iman_value()
        );
        assert_eq!(
            polling_event_generation_run_usbcmd(
                reg::USBCMD_RUN | reg::USBCMD_INTE,
                XhciFirmwareHandoff::PlatformResetComplete,
                stop_seed,
            ) & reg::USBCMD_INTE,
            0
        );
        assert_eq!(
            polling_event_generation_iman_value(
                XhciFirmwareHandoff::PlatformResetComplete,
                stop_seed,
            ) & reg::IMAN_IE,
            0
        );
        assert_eq!(
            polling_command_proof_dnctrl_value(
                XhciFirmwareHandoff::PlatformResetComplete,
                stop_seed,
            ),
            0
        );
        assert_eq!(
            polling_event_generation_run_usbcmd(reg::USBCMD_RUN, XhciFirmwareHandoff::None, None,),
            reg::USBCMD_RUN
        );
        assert_eq!(
            polling_event_generation_iman_value(XhciFirmwareHandoff::None, None),
            polling_iman_value()
        );
        assert_eq!(
            polling_command_proof_dnctrl_value(XhciFirmwareHandoff::None, None),
            0
        );
    }

    #[test]
    fn linux_command_probe_uses_linux_moderation_seed() {
        assert_eq!(LINUX_COMMAND_PROBE_IMOD, 0x0000_00a0);
        assert_eq!(
            linux_command_probe_usbcmd_seed(),
            reg::USBCMD_RUN | reg::USBCMD_INTE
        );
        assert_eq!(
            linux_command_probe_usbcmd_seed() & reg::USBCMD_HSEE,
            0,
            "Pi 4 command proof must not need a pre-write live USBCMD read to preserve interrupt-delivery bits"
        );
    }

    #[test]
    fn command_timeout_live_snapshot_is_enabled_for_command_ring_edge() {
        assert!(command_timeout_live_snapshot_enabled());
    }

    #[test]
    fn command_timeout_live_snapshot_runs_before_final_timeout() {
        assert!(command_timeout_live_snapshot_spins() > 0);
        assert_eq!(command_timeout_live_snapshot_spins(), 32);
        assert_eq!(COMMAND_POLL_ONLY_WAIT_SPINS, 64);
        assert_eq!(COMMAND_PROMPT_SAFE_WAIT_POLLS, 4);
        assert!(command_timeout_live_snapshot_spins() < COMMAND_POLL_ONLY_WAIT_SPINS);
        assert!(COMMAND_PROMPT_SAFE_WAIT_POLLS <= COMMAND_POLL_ONLY_WAIT_SPINS);
        assert!(COMMAND_POLL_ONLY_WAIT_SPINS < COMMAND_EVENT_RING_CPU_SYNC_INTERVAL_SPINS);
        assert!(COMMAND_POLL_ONLY_WAIT_SPINS < COMMAND_WAIT_SPINS);
    }

    #[test]
    fn command_poll_only_reinvalidates_event_ring_every_bounded_poll() {
        assert!(command_poll_only_should_sync_event_ring(0));
        assert!(command_poll_only_should_sync_event_ring(1));
        assert!(command_poll_only_should_sync_event_ring(
            COMMAND_POLL_ONLY_WAIT_SPINS - 1
        ));
        assert!(!command_wait_should_sync_event_ring(1));
        assert!(!command_wait_should_sync_event_ring(
            COMMAND_POLL_ONLY_WAIT_SPINS - 1
        ));
    }

    #[test]
    fn halt_revalidation_depends_on_live_halt_bit() {
        assert!(halt_revalidation_needed(0));
        assert!(halt_revalidation_needed(reg::USBSTS_CNR));
        assert!(!halt_revalidation_needed(reg::USBSTS_HCH));
    }

    #[test]
    fn run_wait_progress_due_is_sparse_and_deterministic() {
        assert!(run_wait_progress_due(1));
        assert!(!run_wait_progress_due(2));
        assert!(run_wait_progress_due(READY_WAIT_PROGRESS_SPINS));
        assert!(run_wait_progress_due(READY_WAIT_PROGRESS_SPINS * 2));
    }

    #[test]
    fn run_wait_observable_usbsts_masks_non_run_bits() {
        let raw =
            reg::USBSTS_HCH | reg::USBSTS_CNR | reg::USBSTS_HCE | reg::USBSTS_EINT | 0x8000_0000;
        assert_eq!(
            run_wait_observable_usbsts(raw),
            reg::USBSTS_HCH | reg::USBSTS_CNR | reg::USBSTS_HCE | reg::USBSTS_EINT
        );
    }

    #[test]
    fn resetless_and_preserve_firmware_handoffs_skip_live_halt_revalidation() {
        assert!(skip_live_halt_revalidation(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(skip_live_halt_revalidation(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert!(!skip_live_halt_revalidation(XhciFirmwareHandoff::None));
    }

    #[test]
    fn unseeded_cold_start_uses_live_halt_revalidation() {
        assert!(!skip_live_halt_revalidation_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
    }

    #[test]
    fn mailbox_reset_snapshot_paths_use_blind_post_reset_settle() {
        assert!(runtime_mailbox_reset_needs_blind_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            Some(XhciRuntimeSeedSnapshot {
                usbcmd: None,
                usbsts: None,
                iman0: None,
                dcbaap: Some(0),
                crcr: Some(0),
                erstba0: Some(0),
                erdp0: Some(0),
                erstsz0: Some(0),
            }),
        ));
        assert!(!runtime_mailbox_reset_needs_blind_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(!runtime_mailbox_reset_needs_blind_settle(
            XhciFirmwareHandoff::ResetlessReinit,
            None,
        ));
        assert!(runtime_mailbox_reset_needs_blind_settle(
            XhciFirmwareHandoff::ResetlessReinit,
            Some(XhciRuntimeSeedSnapshot {
                usbcmd: None,
                usbsts: None,
                iman0: None,
                dcbaap: None,
                crcr: None,
                erstba0: None,
                erdp0: None,
                erstsz0: None,
            }),
        ));
        assert!(runtime_mailbox_reset_needs_blind_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            Some(XhciRuntimeSeedSnapshot {
                usbcmd: Some(0),
                usbsts: Some(reg::USBSTS_HCH),
                iman0: Some(0),
                dcbaap: None,
                crcr: None,
                erstba0: None,
                erdp0: None,
                erstsz0: None,
            }),
        ));
        assert!(!runtime_mailbox_reset_needs_blind_settle(
            XhciFirmwareHandoff::PreserveControllerState,
            Some(XhciRuntimeSeedSnapshot {
                usbcmd: None,
                usbsts: None,
                iman0: None,
                dcbaap: None,
                crcr: None,
                erstba0: None,
                erdp0: None,
                erstsz0: None,
            }),
        ));
        assert!(runtime_mailbox_reset_needs_blind_settle(
            XhciFirmwareHandoff::PreserveControllerState,
            Some(XhciRuntimeSeedSnapshot {
                usbcmd: Some(0),
                usbsts: Some(reg::USBSTS_HCH),
                iman0: Some(0),
                dcbaap: None,
                crcr: None,
                erstba0: None,
                erdp0: None,
                erstsz0: None,
            }),
        ));
    }

    #[test]
    fn trusted_mailbox_reset_handoff_no_longer_skips_post_reset_cnr_poll() {
        assert!(!skip_post_reset_cnr_poll_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            Some(XhciRuntimeSeedSnapshot {
                usbcmd: None,
                usbsts: None,
                iman0: None,
                dcbaap: None,
                crcr: None,
                erstba0: None,
                erdp0: None,
                erstsz0: None,
            }),
        ));
        assert!(!skip_post_reset_cnr_poll_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(!skip_post_reset_cnr_poll_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            Some(XhciRuntimeSeedSnapshot {
                usbcmd: None,
                usbsts: None,
                iman0: None,
                dcbaap: None,
                crcr: None,
                erstba0: None,
                erdp0: None,
                erstsz0: None,
            }),
        ));
    }

    #[test]
    fn firmware_handoff_preserves_config_register_programming() {
        assert!(!preserve_firmware_handoff_config(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
        assert!(preserve_firmware_handoff_config(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(!preserve_firmware_handoff_config(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert_eq!(
            preserve_firmware_handoff_config(XhciFirmwareHandoff::None),
            SKIP_HCRST_DURING_INIT
        );
    }

    #[test]
    fn trusted_firmware_handoff_skips_reset_during_init() {
        assert!(!skip_reset_during_init(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
        assert!(skip_reset_during_init(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(skip_reset_during_init(XhciFirmwareHandoff::ResetlessReinit));
        assert!(!skip_reset_during_init(
            XhciFirmwareHandoff::PlatformResetComplete
        ));
        assert_eq!(
            skip_reset_during_init(XhciFirmwareHandoff::None),
            SKIP_HCRST_DURING_INIT
        );
    }

    #[test]
    fn platform_reset_complete_replays_config_before_fresh_ring_publish() {
        assert!(!skip_reset_during_init(
            XhciFirmwareHandoff::PlatformResetComplete
        ));
        assert!(!skip_reset_during_init_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(runtime_handoff_needs_uboot_style_reset_write(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(skip_config_write_during_init(
            XhciFirmwareHandoff::PlatformResetComplete
        ));
        assert!(!skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(skip_live_halt_revalidation(
            XhciFirmwareHandoff::PlatformResetComplete
        ));
        assert!(runtime_mailbox_reset_needs_blind_settle(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(runtime_handoff_needs_pre_run_settle(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(initial_live_operational_read_hazard(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
            true,
        ));
        assert_eq!(
            reset_usbcmd_seed_before_hcrst_for_init(
                XhciFirmwareHandoff::PlatformResetComplete,
                None,
                true,
            ),
            Some(0),
        );
        assert!(skip_reset_pre_usbsts_read_for_init(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
            true,
        ));
        assert!(skip_reset_completion_poll_for_init(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
            true,
        ));
        assert!(!runtime_deferred_ring_handoff(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(
            !runtime_handoff_needs_release_only_dcbaap_publish_with_snapshot(
                XhciFirmwareHandoff::PlatformResetComplete,
                None,
            )
        );
        assert!(runtime_platform_reset_fresh_rings_handoff(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(!platform_reset_dcbaap_publish_blocked_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(!runtime_pollsafe_no_fresh_ownership_handoff(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(!skip_fresh_runtime_ownership_publish_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(!skip_fresh_event_ring_publish_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(runtime_handoff_needs_uboot_style_run_write(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(runtime_handoff_needs_uboot_style_reset_write(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(!runtime_handoff_skips_live_run_write(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(!runtime_handoff_skips_live_drop_stop(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
        assert!(!snapshot_resetless_reinit_handoff(
            XhciFirmwareHandoff::PlatformResetComplete,
            None,
        ));
    }

    #[test]
    fn platform_reset_complete_with_stop_seed_replays_config_before_fresh_ring_publish() {
        let stop_state_only_snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });

        assert!(!skip_reset_during_init_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            stop_state_only_snapshot,
        ));
        assert!(!skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            stop_state_only_snapshot,
        ));
        assert!(!runtime_pollsafe_no_fresh_ownership_handoff(
            XhciFirmwareHandoff::PlatformResetComplete,
            stop_state_only_snapshot,
        ));
        assert!(!skip_fresh_runtime_ownership_publish_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            stop_state_only_snapshot,
        ));
        assert!(runtime_handoff_needs_uboot_style_run_write(
            XhciFirmwareHandoff::PlatformResetComplete,
            stop_state_only_snapshot,
        ));
        assert!(runtime_handoff_needs_uboot_style_reset_write(
            XhciFirmwareHandoff::PlatformResetComplete,
            stop_state_only_snapshot,
        ));
        assert!(runtime_stop_state_needs_post_run_settle(
            XhciFirmwareHandoff::PlatformResetComplete,
            stop_state_only_snapshot,
        ));
    }

    #[test]
    fn resetless_reinit_still_skips_preinit_polling_scrub() {
        assert!(!skip_preinit_polling_scrub(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
        assert!(skip_preinit_polling_scrub(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(skip_preinit_polling_scrub(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert!(!skip_preinit_polling_scrub(XhciFirmwareHandoff::None));
    }

    #[test]
    fn constructor_scrub_only_stays_trusted_for_snapshot_backed_handoffs() {
        assert_eq!(
            constructor_polling_scrub_mode(XhciFirmwareHandoff::ColdStartFromSnapshot, None),
            ConstructorPollingScrubMode::TrustedQuiesceOnly
        );
        assert_eq!(
            constructor_polling_scrub_mode(XhciFirmwareHandoff::PreserveControllerState, None),
            ConstructorPollingScrubMode::TrustedQuiesceOnly
        );
        assert_eq!(
            constructor_polling_scrub_mode(XhciFirmwareHandoff::ResetlessReinit, None),
            ConstructorPollingScrubMode::TrustedQuiesceOnly
        );
        assert_eq!(
            constructor_polling_scrub_mode(XhciFirmwareHandoff::None, None),
            ConstructorPollingScrubMode::Full
        );
        assert_eq!(
            constructor_polling_scrub_mode(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                Some(XhciRuntimeSeedSnapshot {
                    usbcmd: Some(0),
                    usbsts: Some(reg::USBSTS_HCH),
                    iman0: Some(0),
                    dcbaap: None,
                    crcr: None,
                    erstba0: None,
                    erdp0: None,
                    erstsz0: None,
                }),
            ),
            ConstructorPollingScrubMode::TrustedQuiesceOnly
        );
        assert_eq!(
            constructor_polling_scrub_mode(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                Some(XhciRuntimeSeedSnapshot {
                    usbcmd: Some(0),
                    usbsts: Some(reg::USBSTS_HCH),
                    iman0: Some(0),
                    dcbaap: Some(0x4003_000),
                    crcr: Some(0x4024_001),
                    erstba0: Some(0x4020_000),
                    erdp0: Some(0x4023_000),
                    erstsz0: Some(1),
                }),
            ),
            ConstructorPollingScrubMode::TrustedQuiesceOnly
        );
        assert_eq!(
            constructor_polling_scrub_mode(
                XhciFirmwareHandoff::None,
                Some(XhciRuntimeSeedSnapshot {
                    usbcmd: Some(0),
                    usbsts: Some(reg::USBSTS_HCH),
                    iman0: Some(0),
                    dcbaap: None,
                    crcr: None,
                    erstba0: None,
                    erdp0: None,
                    erstsz0: None,
                }),
            ),
            ConstructorPollingScrubMode::TrustedQuiesceOnly
        );
    }

    #[test]
    fn controller_param_override_skips_constructor_live_scrub_without_seed() {
        let params = XhciControllerParams {
            cap_length: 0x20,
            hcs1: 32u32 | (5u32 << 24),
            hcs2: 0,
            hccparams1: 1 << 2,
            db_offset: 0x100,
            rts_offset: 0x200,
            firmware_handoff: XhciFirmwareHandoff::None,
            runtime_seed_snapshot: None,
            apply_brcm_axi_setup: false,
            skip_constructor_live_scrub: true,
            skip_initial_live_operational_reads: true,
            port_register_access_allowed: true,
        };
        assert_eq!(
            constructor_polling_scrub_mode_from_params(params),
            ConstructorPollingScrubMode::TrustedQuiesceOnly
        );

        let live_params = XhciControllerParams {
            skip_constructor_live_scrub: false,
            skip_initial_live_operational_reads: false,
            ..params
        };
        assert_eq!(
            constructor_polling_scrub_mode_from_params(live_params),
            ConstructorPollingScrubMode::Full
        );
    }

    #[test]
    fn initial_live_read_hazard_uses_blind_full_reset_start() {
        assert!(initial_live_operational_read_hazard(
            XhciFirmwareHandoff::None,
            None,
            true,
        ));
        assert!(skip_init_pre_reset_scrub_writes_for_init(
            XhciFirmwareHandoff::None,
            None,
            true,
        ));
        assert!(!claim_legacy_ownership_before_reset_for_init(
            XhciFirmwareHandoff::None,
            None,
            true,
        ));
        assert!(skip_live_halt_revalidation_for_init(
            XhciFirmwareHandoff::None,
            None,
            true,
        ));
        assert_eq!(
            reset_usbcmd_seed_before_hcrst_for_init(XhciFirmwareHandoff::None, None, true),
            Some(0)
        );
        assert!(skip_reset_pre_usbsts_read_for_init(
            XhciFirmwareHandoff::None,
            None,
            true,
        ));
        assert!(skip_reset_completion_poll_for_init(
            XhciFirmwareHandoff::None,
            None,
            true,
        ));
        assert!(!use_live_post_reset_seed_reads_for_init(
            XhciFirmwareHandoff::None,
            None,
            true,
        ));
        assert!(!use_live_config_seed_reads_for_init(
            XhciFirmwareHandoff::None,
            None,
            true,
        ));
    }

    #[test]
    fn trusted_constructor_scrub_skips_live_usbsts_write_but_quiesces_interrupter() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        let int_base = 0x180;
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD, 0xa5a5_5a5a);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS, 0x55aa_cc33);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMOD, 0xffff_ffff);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMAN, 0);

        XhciCtrl::<MockDma>::write_only_polling_scrub(
            mmio_base,
            op_offset,
            int_base,
            ConstructorPollingScrubMode::TrustedQuiesceOnly,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD),
            0xa5a5_5a5a
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS),
            0x55aa_cc33
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMOD),
            0xffff_ffff
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMAN),
            0
        );
    }

    #[test]
    fn seeded_stop_state_handoff_needs_pre_halt_source_quiesce() {
        let stop_state_snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(reg::USBCMD_RUN | reg::USBCMD_INTE | reg::USBCMD_HSEE),
            usbsts: Some(reg::USBSTS_CNR),
            iman0: Some(reg::IMAN_IE),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(pre_halt_source_quiesce_before_live_stop_revalidation(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(!pre_halt_source_quiesce_before_live_stop_revalidation(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
    }

    #[test]
    fn seeded_stop_state_without_usbcmd_avoids_live_pre_reset_command_read() {
        let stop_state_snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: None,
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(skip_live_halt_revalidation_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(!pre_halt_source_quiesce_before_live_stop_revalidation(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert_eq!(
            reset_usbcmd_seed_before_hcrst(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                stop_state_snapshot,
            ),
            Some(0)
        );
    }

    #[test]
    fn pre_halt_source_quiesce_clears_run_and_irq_enable_bits_only() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        XhciCtrl::<MockDma>::write_reg_at::<u32>(
            mmio_base,
            op_offset + reg::USBCMD,
            reg::USBCMD_RUN | reg::USBCMD_INTE | reg::USBCMD_HSEE | reg::USBCMD_EWE,
        );

        XhciCtrl::<MockDma>::write_pre_halt_source_quiesce_at(mmio_base, op_offset, None, None);

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD),
            reg::USBCMD_EWE
        );
    }

    #[test]
    fn pre_halt_source_quiesce_prefers_seeded_usbcmd_without_live_read() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD, 0xa5a5_5a5a);

        XhciCtrl::<MockDma>::write_pre_halt_source_quiesce_at(
            mmio_base,
            op_offset,
            Some(reg::USBCMD_RUN | reg::USBCMD_INTE | reg::USBCMD_HSEE | reg::USBCMD_EWE),
            None,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD),
            reg::USBCMD_EWE
        );
    }

    #[test]
    fn trusted_constructor_scrub_keeps_constructor_state_write_free() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        let int_base = 0x180;
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD, 0xa5a5_5a5a);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS, 0);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMOD, 0xffff_ffff);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMAN, 0);

        XhciCtrl::<MockDma>::write_only_polling_scrub(
            mmio_base,
            op_offset,
            int_base,
            ConstructorPollingScrubMode::TrustedQuiesceOnly,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD),
            0xa5a5_5a5a
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS),
            0
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMOD),
            0xffff_ffff
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMAN),
            0
        );
    }

    #[test]
    fn full_constructor_scrub_masks_interrupt_delivery_without_ack_writes() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        let int_base = 0x180;
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD, 0xffff_ffff);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS, 0);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMOD, 0xffff_ffff);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMAN, 0);

        XhciCtrl::<MockDma>::write_only_polling_scrub(
            mmio_base,
            op_offset,
            int_base,
            ConstructorPollingScrubMode::Full,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD),
            masked_usbcmd(0xffff_ffff)
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS),
            0
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMOD),
            0xffff_ffff
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMAN),
            0
        );
    }

    #[test]
    fn polling_event_progression_updates_erdp_only_after_consumption() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        let int_base = 0x180;

        XhciCtrl::<MockDma>::write_reg_at::<u64>(mmio_base, int_base + reg::ERDP, 0);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMAN, 0xffff_ffff);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS, 0);

        XhciCtrl::<MockDma>::write_polling_interrupt_quiesce_at(
            mmio_base,
            op_offset,
            int_base,
            0x1234_5000,
            None,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u64>(mmio_base, int_base + reg::ERDP),
            0x1234_5008
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMAN),
            0xffff_ffff
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS),
            0
        );
    }

    #[test]
    fn post_start_polling_interrupt_quiesce_masks_usbcmd_only() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        let int_base = 0x180;

        XhciCtrl::<MockDma>::write_reg_at::<u32>(
            mmio_base,
            op_offset + reg::USBCMD,
            reg::USBCMD_INTE | reg::USBCMD_HSEE | reg::USBCMD_RUN,
        );
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMOD, 0xffff_ffff);
        XhciCtrl::<MockDma>::write_reg_at::<u64>(mmio_base, int_base + reg::ERDP, 0);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMAN, 0xffff_ffff);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS, 0);

        XhciCtrl::<MockDma>::write_post_start_polling_interrupt_quiesce_at(
            mmio_base,
            op_offset,
            int_base,
            0x1234_5000,
            None,
            false,
            false,
            false,
            false,
            None,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMOD),
            0xffff_ffff
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD),
            reg::USBCMD_RUN
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u64>(mmio_base, int_base + reg::ERDP),
            0
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMAN),
            0xffff_ffff
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS),
            0
        );
    }

    #[test]
    fn post_start_polling_interrupt_quiesce_uses_snapshot_seeded_usbcmd_when_available() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        let int_base = 0x180;

        XhciCtrl::<MockDma>::write_reg_at::<u32>(
            mmio_base,
            op_offset + reg::USBCMD,
            reg::USBCMD_RUN | reg::USBCMD_EWE | reg::USBCMD_INTE | reg::USBCMD_HSEE,
        );
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMOD, 0xffff_ffff);
        XhciCtrl::<MockDma>::write_reg_at::<u64>(mmio_base, int_base + reg::ERDP, 0);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMAN, 0xffff_ffff);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS, 0);

        XhciCtrl::<MockDma>::write_post_start_polling_interrupt_quiesce_at(
            mmio_base,
            op_offset,
            int_base,
            0x1234_5000,
            Some(reg::USBCMD_RUN | reg::USBCMD_INTE | reg::USBCMD_HSEE),
            false,
            false,
            false,
            false,
            None,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD),
            reg::USBCMD_RUN
        );
    }

    #[test]
    fn post_start_polling_irq_quiesce_pending_bits_tracks_usbcmd_irq_enables_only() {
        assert_eq!(
            post_start_polling_irq_quiesce_pending_bits(
                reg::USBCMD_INTE | reg::USBCMD_RUN,
                reg::USBSTS_PCD | reg::USBSTS_HCH,
                reg::IMAN_IE | reg::IMAN_IP,
                false,
            ),
            reg::USBCMD_INTE
        );
        assert_eq!(
            post_start_polling_irq_quiesce_pending_bits(reg::USBCMD_RUN, reg::USBSTS_HCH, 0, false),
            0
        );
    }

    #[test]
    fn post_start_polling_interrupt_quiesce_preserve_state_skips_imod_erdp_and_iman_writes() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        let int_base = 0x180;
        let preserved_erdp = 0xabcd_ef08;
        let preserved_iman = reg::IMAN_IP;

        XhciCtrl::<MockDma>::write_reg_at::<u32>(
            mmio_base,
            op_offset + reg::USBCMD,
            reg::USBCMD_INTE | reg::USBCMD_HSEE | reg::USBCMD_RUN,
        );
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMOD, 0x1234_5678);
        XhciCtrl::<MockDma>::write_reg_at::<u64>(mmio_base, int_base + reg::ERDP, preserved_erdp);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMAN, preserved_iman);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS, 0);

        XhciCtrl::<MockDma>::write_post_start_polling_interrupt_quiesce_at(
            mmio_base,
            op_offset,
            int_base,
            0x1234_5000,
            Some(reg::USBCMD_RUN | reg::USBCMD_INTE | reg::USBCMD_HSEE),
            true,
            true,
            true,
            true,
            None,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMOD),
            0x1234_5678
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD),
            reg::USBCMD_RUN
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u64>(mmio_base, int_base + reg::ERDP),
            preserved_erdp
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMAN),
            preserved_iman
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS),
            0
        );
    }

    #[test]
    fn post_init_preserve_state_skips_runtime_interrupter_registers() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        let int_base = 0x180;
        let preserved_erdp = 0xabcd_ef08;

        XhciCtrl::<MockDma>::write_reg_at::<u32>(
            mmio_base,
            op_offset + reg::USBCMD,
            reg::USBCMD_INTE | reg::USBCMD_HSEE | reg::USBCMD_RUN,
        );
        XhciCtrl::<MockDma>::write_reg_at::<u32>(mmio_base, int_base + reg::IMOD, 0x1234_5678);
        XhciCtrl::<MockDma>::write_reg_at::<u64>(mmio_base, int_base + reg::ERDP, preserved_erdp);
        XhciCtrl::<MockDma>::write_reg_at::<u32>(
            mmio_base,
            int_base + reg::IMAN,
            reg::IMAN_IE | reg::IMAN_IP,
        );
        XhciCtrl::<MockDma>::write_reg_at::<u32>(
            mmio_base,
            op_offset + reg::USBSTS,
            reg::USBSTS_PCD,
        );

        XhciCtrl::<MockDma>::write_post_start_polling_interrupt_quiesce_at(
            mmio_base,
            op_offset,
            int_base,
            0x1234_5000,
            Some(reg::USBCMD_RUN | reg::USBCMD_INTE | reg::USBCMD_HSEE),
            true,
            true,
            true,
            true,
            None,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMOD),
            0x1234_5678
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBCMD),
            reg::USBCMD_RUN
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u64>(mmio_base, int_base + reg::ERDP),
            preserved_erdp
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, int_base + reg::IMAN),
            reg::IMAN_IE | reg::IMAN_IP
        );
        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS),
            reg::USBSTS_PCD
        );
    }

    #[test]
    fn post_start_polling_interrupt_quiesce_preserve_state_skips_usbsts_clear() {
        let mut mmio = vec![0u8; 0x400];
        let mmio_base = mmio.as_mut_ptr() as usize;
        let op_offset = 0x40;
        let int_base = 0x180;

        XhciCtrl::<MockDma>::write_reg_at::<u32>(
            mmio_base,
            op_offset + reg::USBCMD,
            reg::USBCMD_INTE | reg::USBCMD_HSEE | reg::USBCMD_RUN,
        );
        XhciCtrl::<MockDma>::write_reg_at::<u32>(
            mmio_base,
            op_offset + reg::USBSTS,
            reg::USBSTS_PCD,
        );

        XhciCtrl::<MockDma>::write_post_start_polling_interrupt_quiesce_at(
            mmio_base,
            op_offset,
            int_base,
            0x1234_5000,
            Some(reg::USBCMD_RUN | reg::USBCMD_INTE | reg::USBCMD_HSEE),
            true,
            true,
            true,
            true,
            None,
        );

        assert_eq!(
            XhciCtrl::<MockDma>::read_reg_at::<u32>(mmio_base, op_offset + reg::USBSTS),
            reg::USBSTS_PCD
        );
    }

    #[test]
    fn post_start_polling_irq_quiesce_always_skips_usbsts_clear() {
        assert!(post_start_polling_irq_quiesce_skip_usbsts_clear(true));
        assert!(post_start_polling_irq_quiesce_skip_usbsts_clear(false));
    }

    #[test]
    fn post_start_polling_irq_quiesce_preserve_state_ignores_usbsts_pending_bits_when_clear_is_skipped()
     {
        assert_eq!(
            post_start_polling_irq_quiesce_pending_bits(reg::USBCMD_RUN, reg::USBSTS_PCD, 0, true),
            0
        );
    }

    #[test]
    fn runtime_seeded_full_reset_start_skips_constructor_polling_scrub_writes() {
        let stop_state_seed = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(1),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(skip_constructor_polling_scrub_writes_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(!skip_constructor_polling_scrub_writes_with_snapshot(
            XhciFirmwareHandoff::None,
            None,
        ));
    }

    #[test]
    fn resetless_and_preserve_handoffs_skip_pre_reset_scrub_writes() {
        assert!(!skip_init_pre_reset_scrub_writes(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
        assert!(skip_init_pre_reset_scrub_writes(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(skip_init_pre_reset_scrub_writes(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert!(!skip_init_pre_reset_scrub_writes(XhciFirmwareHandoff::None));
    }

    #[test]
    fn runtime_seeded_full_reset_start_skips_pre_reset_scrub_writes() {
        let stop_state_seed = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(1),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(skip_init_pre_reset_scrub_writes_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(skip_init_pre_reset_scrub_writes_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_seed,
        ));
    }

    #[test]
    fn runtime_seeded_stop_seed_start_skips_hcrst_and_fresh_ring_publish() {
        let stop_state_seed = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(1),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(runtime_seeded_full_reset_start_handoff(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(skip_reset_during_init_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(!runtime_handoff_needs_uboot_style_reset_write(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(!use_live_post_reset_seed_reads_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(defer_erdp_publish_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(defer_erst_publish_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(deferred_erst_publish_uses_size_first_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(!use_atomic_erstba_publish_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(!defer_event_ring_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(defer_scratchpad_array_publish_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(defer_dcbaap_publish_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(defer_crcr_publish_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(!defer_dcbaap_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(!defer_crcr_publish_until_after_run_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(!defer_dnctrl_write_until_after_run_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(!runtime_bootloader_owned_pollsafe_handoff(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(runtime_pollsafe_no_fresh_ownership_handoff(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(runtime_handoff_skips_live_run_write(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(runtime_handoff_skips_live_drop_stop(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(runtime_handoff_skips_live_drop_stop(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_seed,
        ));
        assert!(runtime_handoff_skips_live_drop_stop(
            XhciFirmwareHandoff::PlatformResetComplete,
            stop_state_seed,
        ));
        assert!(!runtime_handoff_skips_live_drop_stop(
            XhciFirmwareHandoff::None,
            None,
        ));
        assert!(!runtime_handoff_needs_uboot_style_reset_write(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(!runtime_handoff_needs_uboot_style_run_write(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(skip_fresh_event_ring_publish_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(skip_fresh_runtime_ownership_publish_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(skip_usbsts_clear_before_run_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
    }

    #[test]
    fn trusted_handoffs_skip_legacy_ownership_claim() {
        assert!(skip_legacy_ownership_claim_for_handoff(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
        assert!(skip_legacy_ownership_claim_for_handoff(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(skip_legacy_ownership_claim_for_handoff(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert!(!skip_legacy_ownership_claim_for_handoff(
            XhciFirmwareHandoff::None
        ));
    }

    #[test]
    fn runtime_seeded_full_reset_start_and_cold_start_stop_seed_skip_legacy_ownership_claim() {
        let stop_state_seed = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(1),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(skip_legacy_ownership_claim_for_handoff_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_seed,
        ));
        assert!(skip_legacy_ownership_claim_for_handoff_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_seed,
        ));
    }

    #[test]
    fn cold_start_stop_seed_skips_legacy_and_live_halt_revalidation() {
        let stop_state_seed = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(1),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(!claim_legacy_ownership_before_reset_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_seed,
        ));
        assert!(skip_live_halt_revalidation_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_seed,
        ));
    }

    #[test]
    fn legacy_smi_control_bits_match_linux_disable_and_clear_contract() {
        let current = 0xffff_ffffu32;
        assert_eq!(
            disable_legacy_smi_control_bits(current),
            (current & XHCI_LEGACY_DISABLE_SMI) | XHCI_LEGACY_SMI_EVENTS
        );
        assert_eq!(disable_legacy_smi_control_bits(0), XHCI_LEGACY_SMI_EVENTS);
    }

    #[test]
    fn cold_start_snapshot_uses_post_reset_seed_reads() {
        assert!(use_live_post_reset_seed_reads(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
        assert!(!use_live_post_reset_seed_reads(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(!use_live_post_reset_seed_reads(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert!(use_live_post_reset_seed_reads(XhciFirmwareHandoff::None));
    }

    #[test]
    fn only_cold_start_and_runtime_owned_paths_use_live_config_seed_reads() {
        assert!(use_live_config_seed_reads(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
        assert!(!use_live_config_seed_reads(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(!use_live_config_seed_reads(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert!(use_live_config_seed_reads(XhciFirmwareHandoff::None));
    }

    #[test]
    fn resetless_paths_skip_post_reset_verification_readbacks() {
        assert!(!skip_live_post_reset_verification_readbacks(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(skip_live_post_reset_verification_readbacks(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert!(!skip_live_post_reset_verification_readbacks(
            XhciFirmwareHandoff::None
        ));
    }

    #[test]
    fn trusted_preserve_and_runtime_ring_paths_skip_usbsts_clear_before_run() {
        let stop_state_snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(reg::IMAN_IP),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(skip_usbsts_clear_before_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(skip_usbsts_clear_before_run_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(skip_usbsts_clear_before_run_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(skip_usbsts_clear_before_run_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
        assert!(skip_usbsts_clear_before_run_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            None,
        ));
        assert!(skip_usbsts_clear_before_run_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            None,
        ));
        assert!(skip_usbsts_clear_before_run_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_snapshot,
        ));
    }

    #[test]
    fn snapshot_backed_coldstart_paths_skip_post_reset_verification_readbacks() {
        let stop_state_snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(reg::IMAN_IP),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(!skip_live_post_reset_verification_readbacks_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(skip_live_post_reset_verification_readbacks_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
    }

    #[test]
    fn preserve_state_handoff_skips_live_run_write() {
        let stop_state_snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(reg::USBCMD_RUN),
            usbsts: Some(0),
            iman0: Some(reg::IMAN_IE),
            dcbaap: Some(0x2000),
            crcr: Some(0x3000),
            erstba0: Some(0x4000),
            erdp0: Some(0x5000),
            erstsz0: Some(1),
        });
        assert!(runtime_handoff_skips_live_run_write(
            XhciFirmwareHandoff::PreserveControllerState,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_skips_live_run_write(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_snapshot,
        ));
        assert!(!runtime_handoff_skips_live_run_write(
            XhciFirmwareHandoff::ResetlessReinit,
            stop_state_snapshot,
        ));
    }

    #[test]
    fn trusted_handoff_skips_doorbell_readback_after_ring() {
        assert!(skip_doorbell_readback_after_ring(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
        assert!(skip_doorbell_readback_after_ring(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(skip_doorbell_readback_after_ring(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert!(skip_doorbell_readback_after_ring(
            XhciFirmwareHandoff::PlatformResetComplete
        ));
        assert!(!skip_doorbell_readback_after_ring(
            XhciFirmwareHandoff::None
        ));
    }

    #[test]
    fn preserve_handoff_skips_config_write_during_init() {
        assert!(!skip_config_write_during_init(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
        assert!(skip_config_write_during_init(
            XhciFirmwareHandoff::PlatformResetComplete
        ));
        assert!(skip_config_write_during_init(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(!skip_config_write_during_init(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert!(!skip_config_write_during_init(XhciFirmwareHandoff::None));
    }

    #[test]
    fn trusted_and_snapshot_resetless_handoffs_skip_config_write_during_init() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: Some(0),
            crcr: Some(0),
            erstba0: Some(0),
            erdp0: Some(0),
            erstsz0: Some(0),
        });
        assert!(skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::PreserveControllerState,
            snapshot,
        ));
        assert!(skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
        assert!(!skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::None,
            snapshot,
        ));
        let stop_state_only_snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert!(skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            stop_state_only_snapshot,
        ));
        assert!(skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::None,
            stop_state_only_snapshot,
        ));
        assert!(!skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::PlatformResetComplete,
            stop_state_only_snapshot,
        ));
    }

    #[test]
    fn split_u64_register_writes_use_low_then_high_order() {
        let writes = split_u64_reg_write_ops(0x30, 0x1122_3344_5566_7788);
        assert_eq!(writes, [(0x30, 0x5566_7788), (0x34, 0x1122_3344)]);
    }

    #[test]
    fn trusted_handoff_dcbaap_path_skips_experimental_probes() {
        assert!(!TRUSTED_HANDOFF_DCBAAP_PREWRITE_READ_PROBE);
        assert!(!TRUSTED_HANDOFF_DCBAAP_ZERO_REWRITE_PROBE);
    }

    #[test]
    fn crcr_publish_accepts_zero_seed() {
        let composed = compose_crcr(0, 0x0404_0020_00, true);
        assert_eq!(composed & reg::CMD_RING_RSVD_BITS, 1);
        assert_eq!(composed & !reg::CMD_RING_RSVD_BITS, 0x0404_0020_00);
    }

    #[test]
    fn config_updates_preserve_non_slot_bits() {
        assert_eq!(compose_config(0xabcd_ff00, 32), 0xabcd_ff20);
    }

    #[test]
    fn crcr_updates_preserve_reserved_bits() {
        let composed = compose_crcr(0x3e, 0x0404_0020_00, true);
        assert_eq!(composed & reg::CMD_RING_RSVD_BITS, 0x3f);
        assert_eq!(composed & !reg::CMD_RING_RSVD_BITS, 0x0404_0020_00);
    }

    #[test]
    fn erst_base_preserves_low_reserved_bits() {
        let composed = compose_erst_base(0xf, 0x0404_0030_00);
        assert_eq!(composed & reg::ERST_PTR_MASK, reg::ERST_PTR_MASK);
        assert_eq!(composed & !reg::ERST_PTR_MASK, 0x0404_0030_00);
    }

    #[test]
    fn initial_erdp_clears_ehb() {
        let erdp = compose_initial_erdp(0x0404_0040_08);
        assert_eq!(erdp & reg::ERST_EHB, 0);
        assert_eq!(erdp & !reg::ERST_PTR_MASK, 0x0404_0040_00);
    }

    #[test]
    fn polling_erdp_ack_sets_ehb() {
        let erdp = compose_polling_erdp_ack(compose_initial_erdp(0x0404_0040_08));
        assert_eq!(erdp & reg::ERST_EHB, reg::ERST_EHB);
        assert_eq!(erdp & !reg::ERST_PTR_MASK, 0x0404_0040_00);
    }

    #[test]
    fn event_ring_publish_accepts_zero_seeds() {
        assert_eq!(compose_erst_size(0, 1), 1);
        let composed = compose_erst_base(0, 0x0404_0030_00);
        assert_eq!(composed & reg::ERST_PTR_MASK, 0);
        assert_eq!(composed & !reg::ERST_PTR_MASK, 0x0404_0030_00);
    }
}

impl<H: Dma> Drop for XhciCtrl<H> {
    fn drop(&mut self) {
        if self.initialized
            && !runtime_handoff_skips_live_drop_stop(
                self.firmware_handoff,
                self.runtime_seed_snapshot,
            )
        {
            // Stop controller
            let usbcmd = self.read_op::<u32>(reg::USBCMD);
            self.write_op(reg::USBCMD, usbcmd & !reg::USBCMD_RUN);

            // Wait for halt
            let mut waited = 0usize;
            while (self.read_op::<u32>(reg::USBSTS) & reg::USBSTS_HCH) == 0 {
                waited = waited.saturating_add(1);
                if waited >= DROP_HALT_WAIT_SPINS {
                    break;
                }
                spin_loop();
            }
        } else if self.initialized {
            emit_xhci_diag(
                0x0335,
                self.firmware_handoff as u64,
                runtime_seed_snapshot_flag_bits(self.runtime_seed_snapshot),
                1,
            );
        } else {
            emit_xhci_diag(0x0331, self.firmware_handoff as u64, self.mmio as u64, 0);
        }

        // Unmap MMIO
        // SAFETY: `self.mmio` is the mapping created during controller
        // construction with `self.mmio_size`, and drop runs exactly once.
        unsafe {
            self.host.unmap_mmio(self.mmio, self.mmio_size);
        }
    }
}
