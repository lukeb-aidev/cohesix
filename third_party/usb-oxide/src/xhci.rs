// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
use crate::{
    reg,
    ring::{completion, trb_type, EventRing, PhysMem, Ring, Trb},
    Dma, Result, UsbError,
};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{
    hint::spin_loop,
    sync::atomic::{compiler_fence, AtomicUsize, Ordering},
};
use spin::Mutex;

const MMIO_INIT_SIZE: usize = 0x1000;
const MMIO_MAX_SIZE: usize = 0x20_0000;
const CMD_RING_SIZE: usize = 256;
const EVENT_RING_SIZE: usize = 256;
const STOP_WAIT_SPINS: usize = 10_000_000;
const RESET_WAIT_SPINS: usize = 10_000_000;
// On Pi 4 mailbox-reset handoff, the first live USBSTS read can race VL805
// while firmware is still finishing the reset boundary. Keep a short blind
// settle before the first CNR poll on that path.
const MAILBOX_RESET_POST_SETTLE_SPINS: usize = 1_000_000;
const READY_WAIT_SPINS: usize = 10_000_000;
const COMMAND_WAIT_SPINS: usize = 20_000_000;
const COMMAND_WAIT_OTHER_EVENT_LOGS: usize = 8;
const PORT_RESET_WAIT_SPINS: usize = 10_000_000;
const PORT_ENABLE_WAIT_SPINS: usize = 10_000_000;
const PORT_SETTLE_SPINS: usize = 100_000;
const PORT_POST_ACK_WAIT_SPINS: usize = 1_000_000;
const PORT_POST_ACK_TRANSITION_LOGS: usize = 8;
const DROP_HALT_WAIT_SPINS: usize = 1_000_000;
const CONFIG_MAX_SLOTS_MASK: u32 = 0xff;
// Pi 4 now reliably reaches trusted-handoff ring programming. At that point the
// DCBAAP prewrite read and zero rewrite are just ownership probes, and the
// board-freezing edge was the experimental atomic write64 publish. Keep the
// trusted-handoff path on the standard low / high dword sequence only and use
// later xHCI breadcrumbs to judge success/failure.
const TRUSTED_HANDOFF_DCBAAP_PREWRITE_READ_PROBE: bool = false;
const TRUSTED_HANDOFF_DCBAAP_ZERO_REWRITE_PROBE: bool = false;
const USBSTS_CLEAR_MASK: u32 =
    reg::USBSTS_EINT | reg::USBSTS_PCD | reg::USBSTS_HSE | reg::USBSTS_HCE;
const USBLEGACY_BIOS_OWNED: u32 = 1 << 16;
const USBLEGACY_OS_OWNED: u32 = 1 << 24;
const EXT_CAP_SCAN_LIMIT: usize = 64;
// Perform an explicit host controller reset after stop so ring/DCBAA
// programming starts from a deterministic post-firmware baseline on generic
// xHCI bring-up paths.
const SKIP_HCRST_DURING_INIT: bool = false;
// The Pi4 UEFI chain may already own xHCI; forcing BIOS/OS ownership handover
// can fault on some firmware paths.
const SKIP_LEGACY_OWNERSHIP_CLAIM: bool = true;
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

#[inline(always)]
const fn polling_iman_value() -> u32 {
    reg::IMAN_IP
}

#[inline(always)]
const fn masked_usbcmd(usbcmd: u32) -> u32 {
    usbcmd & !(reg::USBCMD_INTE | reg::USBCMD_HSEE)
}

#[inline(always)]
const fn halt_revalidation_needed(usbsts: u32) -> bool {
    (usbsts & reg::USBSTS_HCH) == 0
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
        XhciFirmwareHandoff::ResetlessReinit | XhciFirmwareHandoff::PreserveControllerState
    )
}

#[inline(always)]
const fn skip_constructor_polling_scrub_writes(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ColdStartFromSnapshot
            | XhciFirmwareHandoff::ResetlessReinit
            | XhciFirmwareHandoff::PreserveControllerState
    )
}

#[inline(always)]
const fn skip_init_pre_reset_scrub_writes(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(firmware_handoff, XhciFirmwareHandoff::ColdStartFromSnapshot)
        || skip_preinit_polling_scrub(firmware_handoff)
}

#[inline(always)]
const fn skip_legacy_ownership_claim_for_handoff(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ColdStartFromSnapshot
            | XhciFirmwareHandoff::ResetlessReinit
            | XhciFirmwareHandoff::PreserveControllerState
    )
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
    // Only the stronger mailbox-reset snapshot suppresses live post-reset ring
    // seed reads. The weaker stop-state snapshot has now proven it cannot
    // preserve runtime ownership state safely, so it falls back to the normal
    // cold-start read/reset/config/ring sequence after the early halt
    // revalidation is skipped.
    use_live_post_reset_seed_reads(firmware_handoff)
        && !runtime_snapshot_has_runtime_ring_seed(runtime_seed_snapshot)
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
const fn skip_live_post_reset_verification_readbacks(
    firmware_handoff: XhciFirmwareHandoff,
) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::ColdStartFromSnapshot | XhciFirmwareHandoff::ResetlessReinit
    )
}

#[inline(always)]
const fn skip_doorbell_readback_after_ring(firmware_handoff: XhciFirmwareHandoff) -> bool {
    !matches!(firmware_handoff, XhciFirmwareHandoff::None)
}

#[inline(always)]
const fn skip_config_write_during_init(firmware_handoff: XhciFirmwareHandoff) -> bool {
    matches!(
        firmware_handoff,
        XhciFirmwareHandoff::PreserveControllerState
    )
}

#[inline(always)]
const fn skip_config_write_during_init_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || skip_config_write_during_init(firmware_handoff)
}

#[inline(always)]
const fn skip_reset_during_init(firmware_handoff: XhciFirmwareHandoff) -> bool {
    // Resetless and preserve-state handoff modes are only safe when firmware
    // has already proven the controller halted and interrupt-quiesced. The
    // snapshot-driven cold-start path intentionally falls back to the normal
    // halt/reset/config sequence so runtime matches the known-good U-Boot
    // controller bring-up more closely while still avoiding a live CAP read.
    skip_preinit_polling_scrub(firmware_handoff) || SKIP_HCRST_DURING_INIT
}

#[inline(always)]
const fn skip_reset_during_init_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Only the stronger seeded snapshot paths remain fully reset-equivalent.
    // The weaker stop-state-only snapshot still skips the toxic live HCRST
    // store on Pi 4, but now also suppresses the live CONFIG seed read while
    // continuing with the standard ring bring-up sequence.
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || runtime_stop_state_snapshot_handoff(firmware_handoff, runtime_seed_snapshot)
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
const fn runtime_stop_state_snapshot_handoff(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    matches!(firmware_handoff, XhciFirmwareHandoff::ColdStartFromSnapshot)
        && runtime_snapshot_has_stop_state(runtime_seed_snapshot)
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
        || snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_mailbox_reset_needs_blind_settle(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[cfg(test)]
#[inline(always)]
const fn runtime_stop_state_needs_post_run_settle(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    let _ = firmware_handoff;
    let _ = runtime_seed_snapshot;
    false
}

#[cfg(test)]
#[inline(always)]
const fn skip_post_run_interrupter_zeroing_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    let _ = firmware_handoff;
    let _ = runtime_seed_snapshot;
    false
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
    runtime_deferred_ring_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn defer_erst_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    runtime_deferred_ring_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn use_atomic_erstba_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    // Keep the single 64-bit ERSTBA publish only on the stronger mailbox-reset
    // and resetless-snapshot paths, where runtime is replacing a live seeded
    // event-ring table base. The weaker stop-state snapshot path stages ERSTBA
    // from zero, so fall back to the split low/high replay there and make the
    // next diagnostic boundary the first live low-dword store rather than the
    // wider 64-bit MMIO transaction itself.
    runtime_mailbox_reset_handoff(firmware_handoff, runtime_seed_snapshot)
        || snapshot_resetless_reinit_handoff(firmware_handoff, runtime_seed_snapshot)
}

#[inline(always)]
const fn probe_live_dcbaap_before_staged_publish_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    !runtime_deferred_ring_handoff(firmware_handoff, runtime_seed_snapshot)
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
    skip_preinit_polling_scrub(firmware_handoff)
}

#[inline(always)]
const fn skip_live_halt_revalidation_with_snapshot(
    firmware_handoff: XhciFirmwareHandoff,
    runtime_seed_snapshot: Option<XhciRuntimeSeedSnapshot>,
) -> bool {
    skip_live_halt_revalidation(firmware_handoff)
        || runtime_snapshot_has_stop_state(runtime_seed_snapshot)
}

#[inline(always)]
const fn runtime_snapshot_has_stop_state(
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

static XHCI_DIAG_HOOK: AtomicUsize = AtomicUsize::new(0);

/// Installs or clears the xHCI probe diagnostic callback.
pub fn set_xhci_diag_hook(hook: Option<XhciDiagHook>) {
    let raw = hook.map_or(0usize, |f| f as usize);
    XHCI_DIAG_HOOK.store(raw, Ordering::Release);
}

#[inline(always)]
fn emit_xhci_diag(stage: u16, a: u64, b: u64, c: u64) {
    let raw = XHCI_DIAG_HOOK.load(Ordering::Acquire);
    if raw == 0 {
        return;
    }
    // SAFETY: `raw` is written only by `set_xhci_diag_hook` from a function
    // pointer with the exact `XhciDiagHook` ABI/signature.
    let hook: XhciDiagHook = unsafe { core::mem::transmute(raw) };
    hook(stage, a, b, c);
}

#[inline(always)]
fn ring_write_barrier() {
    compiler_fence(Ordering::Release);
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dmb oshst", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(Ordering::Release);
}

#[inline(always)]
fn mmio_write_barrier() {
    compiler_fence(Ordering::Release);
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

    if (db_offset & 0x3) != 0 || (rts_offset & 0x1f) != 0 {
        return None;
    }
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
    dcbaa: PhysMem<H>,
    scratchpad: Option<ScratchpadSet<H>>,
    cmd_ring: Mutex<Box<Ring<H>>>,
    event_ring: Mutex<Box<EventRing<H>>>,
    host: Arc<H>,
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

impl<H: Dma> XhciCtrl<H> {
    #[inline(always)]
    fn read_reg_at<T: Copy>(mmio: usize, offset: usize) -> T {
        let val = unsafe { ((mmio + offset) as *const T).read_volatile() };
        mmio_read_barrier();
        val
    }

    #[inline(always)]
    fn write_reg_at<T: Copy>(mmio: usize, offset: usize, val: T) {
        // Match U-Boot's `readl`/`writel` ordering discipline on ARM before
        // touching live controller registers after firmware handoff.
        mmio_write_barrier();
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
    fn write_reg_u64_at(mmio: usize, offset: usize, val: u64) {
        // xHCI 64-bit register pairs must be programmed low dword first, then
        // high dword. Replaying the low dword after the high dword is not part
        // of the xHCI-defined sequence and has proven unsafe on Pi 4 VL805
        // during the first post-handoff ownership transfer.
        let [low, high] = split_u64_reg_write_ops(offset, val);
        Self::write_reg_at::<u32>(mmio, low.0, low.1);
        Self::write_reg_at::<u32>(mmio, high.0, high.1);
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
    fn write_only_polling_scrub(mmio: usize, op_offset: usize, int_base: usize, skip_writes: bool) {
        if skip_writes {
            emit_xhci_diag(0x0209, reg::USBCMD as u64, 0, 1);
        } else {
            emit_xhci_diag(0x0205, reg::USBCMD as u64, 0, 1);
            Self::write_reg_at(mmio, op_offset + reg::USBCMD, 0u32);
        }
        if skip_writes {
            emit_xhci_diag(0x020a, reg::USBSTS as u64, USBSTS_CLEAR_MASK as u64, 1);
        } else {
            Self::write_reg_at(mmio, op_offset + reg::USBSTS, USBSTS_CLEAR_MASK);
        }
        if skip_writes {
            emit_xhci_diag(0x020b, (int_base + reg::IMOD) as u64, 0, 1);
        } else {
            Self::write_reg_at(mmio, int_base + reg::IMOD, 0u32);
        }
        if skip_writes {
            emit_xhci_diag(
                0x020c,
                (int_base + reg::IMAN) as u64,
                polling_iman_value() as u64,
                1,
            );
        } else {
            Self::write_reg_at(mmio, int_base + reg::IMAN, polling_iman_value());
        }
    }

    /// Create and initialize a new xHCI controller
    pub fn new(mmio_phys: usize, host: H) -> Result<Self> {
        emit_xhci_diag(0x0100, mmio_phys as u64, 0, 0);
        let host = Arc::new(host);

        // Initial map to read capability registers
        let init_mmio =
            unsafe { host.map_mmio(mmio_phys, MMIO_INIT_SIZE) }.ok_or(UsbError::MapFail)?;
        emit_xhci_diag(0x0101, init_mmio as u64, MMIO_INIT_SIZE as u64, 0);

        let cap_length = unsafe { (init_mmio as *const u8).read_volatile() };
        let hcs1: u32 = unsafe { ((init_mmio + reg::HCSPARAMS1) as *const u32).read_volatile() };
        let hcs2: u32 = unsafe { ((init_mmio + reg::HCSPARAMS2) as *const u32).read_volatile() };
        let hccparams1: u32 =
            unsafe { ((init_mmio + reg::HCCPARAMS1) as *const u32).read_volatile() };
        let db_offset: u32 = unsafe { ((init_mmio + reg::DBOFF) as *const u32).read_volatile() };
        let rts_offset: u32 = unsafe { ((init_mmio + reg::RTSOFF) as *const u32).read_volatile() };
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
        };

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
        let mmio = unsafe { host.map_mmio(mmio_phys, mmio_size) }.ok_or(UsbError::MapFail)?;
        emit_xhci_diag(0x0105, mmio as u64, mmio_size as u64, 0);

        let op_base = mmio + params.cap_length as usize;
        let rt_base = mmio + params.rts_offset as usize;
        let op_offset = op_base - mmio;
        let int_base = rt_base + 0x20;

        // Generic xHCI probing still needs to quiesce interrupt delivery
        // immediately after mapping, before any runtime register reads. On the
        // Pi4 trusted-handoff path, the first pre-reset operational write has
        // proven unsafe, so runtime stays read-only until live halt
        // revalidation/HCRST and only emits the skip breadcrumbs here.
        emit_xhci_diag(0x0106, op_offset as u64, rt_base as u64, int_base as u64);
        Self::write_only_polling_scrub(
            mmio,
            op_offset,
            int_base,
            skip_constructor_polling_scrub_writes(params.firmware_handoff),
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
            db_offset: params.db_offset,
            ctx_size_bytes,
            max_slots,
            max_ports,
            firmware_handoff: params.firmware_handoff,
            runtime_seed_snapshot: params.runtime_seed_snapshot,
            dcbaa,
            scratchpad,
            cmd_ring: Mutex::new(cmd_ring),
            event_ring: Mutex::new(event_ring),
            host,
        };

        emit_xhci_diag(0x010f, mmio as u64, op_base as u64, rt_base as u64);
        ctrl.init()?;
        emit_xhci_diag(0x0110, 0, 0, 0);
        Ok(ctrl)
    }

    fn init(&mut self) -> Result<()> {
        let preserve_firmware_state = preserve_firmware_handoff_config(self.firmware_handoff);
        let trusted_runtime_seed_snapshot = self.runtime_seed_snapshot;
        let defer_scratchpad_array_publish = defer_scratchpad_array_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let defer_dcbaap_publish = defer_dcbaap_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let defer_crcr_publish = defer_crcr_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let defer_erst_publish = defer_erst_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let atomic_erstba_publish = use_atomic_erstba_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let defer_erdp_publish = defer_erdp_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let probe_live_dcbaap_before_staged_publish =
            probe_live_dcbaap_before_staged_publish_with_snapshot(
                self.firmware_handoff,
                trusted_runtime_seed_snapshot,
            );
        let atomic_runtime_ring_publish = use_atomic_runtime_ring_publish_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let live_post_reset_seed_reads = use_live_post_reset_seed_reads_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let live_config_seed_reads = use_live_config_seed_reads_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        );
        let skip_post_reset_verification_readbacks =
            skip_live_post_reset_verification_readbacks(self.firmware_handoff);

        // Keep host-system and event interrupts masked during bring-up. Pi4
        // local-seat uses polling and does not install xHCI IRQ handlers in
        // this phase.
        if skip_init_pre_reset_scrub_writes(self.firmware_handoff) {
            emit_xhci_diag(0x0204, self.mmio as u64, self.firmware_handoff as u64, 0);
        } else {
            let mut usbcmd = self.read_op::<u32>(reg::USBCMD);
            let usbsts_start = self.read_op::<u32>(reg::USBSTS);
            emit_xhci_diag(0x0200, usbcmd as u64, usbsts_start as u64, self.mmio as u64);
            usbcmd = masked_usbcmd(usbcmd);
            emit_xhci_diag(0x0201, usbcmd as u64, 0, 0);
            self.write_op(reg::USBCMD, usbcmd);
            emit_xhci_diag(0x0202, self.read_op::<u32>(reg::USBCMD) as u64, 0, 0);
            self.write_op(reg::USBSTS, USBSTS_CLEAR_MASK);
            emit_xhci_diag(0x0203, self.read_op::<u32>(reg::USBSTS) as u64, 0, 0);
        }

        // Some firmware/UEFI stacks leave xHCI under legacy ownership until
        // the OS-owned semaphore is asserted.
        if !SKIP_LEGACY_OWNERSHIP_CLAIM
            && !skip_legacy_ownership_claim_for_handoff(self.firmware_handoff)
        {
            emit_xhci_diag(0x0210, 0, 0, 0);
            self.claim_legacy_ownership()?;
            emit_xhci_diag(0x0211, 0, 0, 0);
        } else {
            emit_xhci_diag(0x0212, self.firmware_handoff as u64, 0, 0);
        }

        // Resetless/preserve-state firmware modes already proved the
        // controller safe for takeover and can skip this halt check. The
        // trusted snapshot cold-start path instead mirrors U-Boot's
        // xhci_reset() entry more closely: live halt-state reads first,
        // then HCRST from the current USBCMD value.
        if skip_live_halt_revalidation_with_snapshot(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) {
            emit_xhci_diag(0x0224, 0, reg::USBSTS_HCH as u64, 1);
        } else {
            let usbsts = self.read_op::<u32>(reg::USBSTS);
            let usbcmd_raw = self.read_op::<u32>(reg::USBCMD);
            let usbcmd = masked_usbcmd(usbcmd_raw);
            emit_xhci_diag(0x0220, usbcmd as u64, usbsts as u64, usbcmd_raw as u64);
            if !SKIP_STOP_DURING_INIT && halt_revalidation_needed(usbsts) {
                if (usbcmd & reg::USBCMD_RUN) != 0 {
                    emit_xhci_diag(0x0221, usbcmd as u64, usbcmd_raw as u64, 0);
                    self.write_op(reg::USBCMD, usbcmd & !reg::USBCMD_RUN);
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

        if runtime_mailbox_reset_needs_blind_settle(
            self.firmware_handoff,
            trusted_runtime_seed_snapshot,
        ) {
            emit_xhci_diag(0x0227, MAILBOX_RESET_POST_SETTLE_SPINS as u64, 0, 0);
            for _ in 0..MAILBOX_RESET_POST_SETTLE_SPINS {
                spin_loop();
            }
            emit_xhci_diag(0x0228, MAILBOX_RESET_POST_SETTLE_SPINS as u64, 0, 0);
        }

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
            let usbcmd_before_reset = trusted_runtime_seed_snapshot
                .and_then(|snapshot| snapshot.usbcmd)
                .map(masked_usbcmd)
                .unwrap_or_else(|| self.read_op::<u32>(reg::USBCMD));
            emit_xhci_diag(
                0x0226,
                usbcmd_before_reset as u64,
                trusted_runtime_seed_snapshot.is_some() as u64,
                0,
            );
            let reset_cmd = usbcmd_before_reset | reg::USBCMD_HCRST;
            // Reset controller
            emit_xhci_diag(
                0x0230,
                reg::USBCMD as u64,
                reset_cmd as u64,
                self.firmware_handoff as u64,
            );
            self.write_op_u32_store_diag(
                reg::USBCMD,
                reset_cmd,
                0x0237,
                0x0235,
                self.firmware_handoff as u64,
            );
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
            // SAFETY: DCBAA entry 0 is the scratchpad array pointer owned by
            // controller init. On the trusted mailbox-reset path we keep it
            // clear until after DCBAAP / CRCR / ERST are published so runtime
            // matches U-Boot's safer handoff ordering more closely.
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
            emit_xhci_diag(0x0257, reg::DCBAAP as u64, dcbaap_offset as u64, snapshot_dcbaap);
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
        let publish_dcbaap = |this: &mut Self| {
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
            if atomic_runtime_ring_publish {
                emit_xhci_diag(0x0290, reg::DCBAAP as u64, dcbaa_phys, 1);
                Self::write_reg_u64_atomic_diag_at(
                    this.mmio,
                    dcbaap_offset,
                    dcbaa_phys,
                    0x0291,
                    0x0292,
                );
            } else {
                Self::write_reg_u64_done_diag_at(
                    this.mmio,
                    dcbaap_offset,
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
        if !defer_dcbaap_publish {
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
        } else if !live_post_reset_seed_reads {
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
        emit_xhci_diag(0x0250, crcr, 0, 0);
        emit_xhci_diag(0x0252, reg::CRCR as u64, crcr, 0);
        let crcr_offset = self.op_base - self.mmio + reg::CRCR;
        if !defer_crcr_publish {
            if atomic_runtime_ring_publish {
                emit_xhci_diag(0x0293, reg::CRCR as u64, crcr, 0);
                Self::write_reg_u64_atomic_diag_at(
                    self.mmio,
                    crcr_offset,
                    crcr,
                    0x0294,
                    0x0295,
                );
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
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        let (event_ring_phys, erst_phys) = event_ring.share_for_device(&*self.host)?;
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
        emit_xhci_diag(0x0266, (int_base + reg::ERDP) as u64, erdp, 0);
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
            1,
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
        emit_xhci_diag(0x0265, (int_base + reg::ERSTBA) as u64, erstba, 0);
        if defer_erst_publish {
            emit_xhci_diag(
                0x02c0,
                (int_base + reg::ERSTSZ) as u64,
                erst_size as u64,
                staged_current_erst_size as u64,
            );
            emit_xhci_diag(
                0x02c1,
                (int_base + reg::ERSTBA) as u64,
                erstba,
                staged_current_erstba,
            );
        } else {
            self.write_reg(int_base + reg::ERSTSZ, erst_size);
            if atomic_runtime_ring_publish {
                emit_xhci_diag(0x0299, (int_base + reg::ERSTBA) as u64, erstba, 0);
                Self::write_reg_u64_atomic_diag_at(
                    self.mmio,
                    int_base + reg::ERSTBA,
                    erstba,
                    0x029a,
                    0x029b,
                );
            } else {
                self.write_reg_u64_diag(int_base + reg::ERSTBA, erstba, 0x026e, 0x026f);
            }
        }
        if defer_erst_publish {
            // The trusted mailbox-reset snapshot path no longer lets ERSTSZ /
            // ERSTBA become the first live runtime event-ring ownership
            // stores. Publish ERSTBA first so the controller never observes a
            // non-zero ERSTSZ while the event-ring table base is still staged
            // at the bootloader snapshot value. The split low/high ERSTBA
            // replay still wedges VL805 on the first live low-dword publish,
            // so switch just this edge to a single 64-bit MMIO transaction.
            emit_xhci_diag(
                0x02c5,
                (int_base + reg::ERSTBA) as u64,
                staged_current_erstba,
                erstba,
            );
            if atomic_erstba_publish {
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
            emit_xhci_diag(
                0x02c2,
                (int_base + reg::ERSTSZ) as u64,
                staged_current_erst_size as u64,
                erst_size as u64,
            );
            Self::write_reg_u32_store_diag_at(
                self.mmio,
                int_base + reg::ERSTSZ,
                erst_size,
                0x02c3,
                0x02c4,
                staged_current_erst_size as u64,
            );
        }
        if defer_erdp_publish {
            // The trusted mailbox-reset snapshot path no longer lets ERSTSZ /
            // ERSTBA / ERDP become the first live runtime event-ring
            // ownership stores. Publish the table first, then hand ERDP to
            // the controller with the same staged low/high breadcrumbs used
            // for the later runtime ring registers.
            Self::write_reg_u64_done_diag_at(
                self.mmio,
                int_base + reg::ERDP,
                staged_current_erdp,
                0x02b7,
                0x02b8,
                0x02b9,
                0x02ba,
            );
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

        if defer_dcbaap_publish {
            // The stronger mailbox-reset snapshot no longer lets DCBAAP be the
            // first live runtime ring store. Publish ERDP / ERST first, then
            // hand DCBAAP to the controller before the final CRCR ownership
            // transfer so the next live edge stays isolated.
            emit_xhci_diag(0x025a, reg::DCBAAP as u64, dcbaa_phys, 1);
            Self::write_reg_u64_done_diag_at(
                self.mmio,
                dcbaap_offset,
                staged_current_dcbaap,
                0x02a1,
                0x02a2,
                0x02a3,
                0x02a4,
            );
            let [target_low, target_high] = split_u64_reg_write_ops(dcbaap_offset, dcbaa_phys);
            emit_xhci_diag(
                0x02a5,
                target_low.0 as u64,
                target_low.1 as u64,
                staged_current_dcbaap,
            );
            Self::write_reg_at::<u32>(self.mmio, target_low.0, target_low.1);
            emit_xhci_diag(
                0x02a6,
                target_low.0 as u64,
                target_low.1 as u64,
                staged_current_dcbaap,
            );
            emit_xhci_diag(
                0x02a7,
                target_high.0 as u64,
                target_high.1 as u64,
                staged_current_dcbaap,
            );
            Self::write_reg_at::<u32>(self.mmio, target_high.0, target_high.1);
            emit_xhci_diag(
                0x02a8,
                target_high.0 as u64,
                target_high.1 as u64,
                staged_current_dcbaap,
            );
            emit_xhci_diag(0x02a9, reg::DCBAAP as u64, dcbaa_phys, 1);
            publish_dcbaap(self);
        }
        if defer_crcr_publish {
            emit_xhci_diag(0x02aa, reg::CRCR as u64, crcr, current_crcr);
            emit_xhci_diag(
                0x02ab,
                current_crcr,
                crcr,
                u64_register_change_mask(current_crcr, crcr),
            );
            Self::write_reg_u64_done_diag_at(
                self.mmio,
                crcr_offset,
                current_crcr,
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
                current_crcr,
            );
            Self::write_reg_at::<u32>(self.mmio, target_low.0, target_low.1);
            emit_xhci_diag(
                0x02b1,
                target_low.0 as u64,
                target_low.1 as u64,
                crcr,
            );
            emit_xhci_diag(
                0x02b2,
                target_high.0 as u64,
                target_high.1 as u64,
                current_crcr,
            );
            Self::write_reg_at::<u32>(self.mmio, target_high.0, target_high.1);
            emit_xhci_diag(
                0x02b3,
                target_high.0 as u64,
                target_high.1 as u64,
                crcr,
            );
            emit_xhci_diag(0x02b4, reg::CRCR as u64, crcr, 1);
        }

        if let Some(scratchpad_array_phys) = deferred_scratchpad_array_phys {
            // SAFETY: DCBAA entry 0 remains controller-init-owned state. Once
            // runtime has published DCBAAP / CRCR / ERST on the trusted
            // mailbox-reset path, republishing the scratchpad array pointer
            // restores the standard xHCI layout before the controller runs.
            unsafe {
                self.dcbaa.as_ptr::<u64>().write_volatile(scratchpad_array_phys);
            }
            let _ = self.dcbaa.share_for_device(&*self.host, "xhci-dcbaa")?;
        }

        // Disable device notifications before the first command can observe
        // any stale firmware-originated notification state.
        emit_xhci_diag(0x0256, reg::DNCTRL as u64, 0, 0);
        self.write_op(reg::DNCTRL, 0u32);

        // The trusted snapshot cold-start path now follows U-Boot directly:
        // publish rings, start the controller, then zero IMOD/IMAN. Generic
        // polling bring-up still clears stale USBSTS before RUN so command
        // completions are observable on non-snapshot paths.
        if matches!(
            self.firmware_handoff,
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ) {
            emit_xhci_diag(
                0x0263,
                USBSTS_CLEAR_MASK as u64,
                self.firmware_handoff as u64,
                1,
            );
        } else {
            emit_xhci_diag(0x0269, reg::USBSTS as u64, USBSTS_CLEAR_MASK as u64, 0);
            self.write_op(reg::USBSTS, USBSTS_CLEAR_MASK);
            emit_xhci_diag(
                0x0263,
                USBSTS_CLEAR_MASK as u64,
                self.firmware_handoff as u64,
                0,
            );
        }
        // Start controller in polling mode (interrupt delivery remains masked).
        let run_usbcmd = if preserve_firmware_state || !live_post_reset_seed_reads {
            reg::USBCMD_RUN
        } else {
            emit_xhci_diag(0x0275, reg::USBCMD as u64, reg::USBCMD_RUN as u64, 0);
            let current = self.read_op::<u32>(reg::USBCMD);
            emit_xhci_diag(0x0271, current as u64, reg::USBCMD_RUN as u64, 0);
            current | reg::USBCMD_RUN
        };
        if preserve_firmware_state {
            emit_xhci_diag(0x0270, 0, 1, 0);
        } else if !skip_post_reset_verification_readbacks {
            emit_xhci_diag(0x0274, reg::USBSTS as u64, 0, 0);
            emit_xhci_diag(0x0270, self.read_op::<u32>(reg::USBSTS) as u64, 0, 0);
        }
        emit_xhci_diag(0x026a, reg::USBCMD as u64, run_usbcmd as u64, 0);
        self.write_op(reg::USBCMD, run_usbcmd);
        if preserve_firmware_state {
            emit_xhci_diag(0x0271, reg::USBCMD_RUN as u64, 1, 0);
        } else if !skip_post_reset_verification_readbacks {
            emit_xhci_diag(0x0275, reg::USBCMD as u64, 0, 1);
            emit_xhci_diag(0x0271, self.read_op::<u32>(reg::USBCMD) as u64, 0, 1);
        }
        // Wait for controller to be ready
        emit_xhci_diag(0x0276, reg::USBSTS as u64, reg::USBSTS_HCH as u64, 0);
        let mut waited = 0usize;
        while (self.read_op::<u32>(reg::USBSTS) & reg::USBSTS_HCH) != 0 {
            waited = waited.saturating_add(1);
            if waited >= READY_WAIT_SPINS {
                emit_xhci_diag(
                    0x0272,
                    waited as u64,
                    self.read_op::<u32>(reg::USBSTS) as u64,
                    0,
                );
                return Err(UsbError::Timeout);
            }
            spin_loop();
        }
        emit_xhci_diag(0x0273, self.read_op::<u32>(reg::USBSTS) as u64, 0, 0);

        // Match U-Boot's post-start ordering for interrupter state: zero
        // moderation and pending after the controller is running.
        if preserve_firmware_state {
            emit_xhci_diag(0x026b, (int_base + reg::IMOD) as u64, 0, 1);
            emit_xhci_diag(0x026c, (int_base + reg::IMAN) as u64, 0, 1);
        } else {
            emit_xhci_diag(0x0267, (int_base + reg::IMOD) as u64, 0, 1);
            self.write_reg(int_base + reg::IMOD, 0u32);
            emit_xhci_diag(0x0268, (int_base + reg::IMAN) as u64, 0, 1);
            self.write_reg(int_base + reg::IMAN, 0u32);
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
    fn write_reg_u64(&self, offset: usize, val: u64) {
        Self::write_reg_u64_at(self.mmio, offset, val);
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
        ring_write_barrier();
        self.write_reg(db, 0u32);
        if !skip_doorbell_readback_after_ring(self.firmware_handoff) {
            let _ = self.read_reg::<u32>(db);
        }
    }

    /// Ring device doorbell
    pub fn ring_doorbell(&self, slot: u8, target: u8) {
        let db = reg::doorbell(self.db_offset, slot);
        ring_write_barrier();
        self.write_reg(db, target as u32);
        if !skip_doorbell_readback_after_ring(self.firmware_handoff) {
            let _ = self.read_reg::<u32>(db);
        }
    }

    /// Update event ring dequeue pointer
    fn update_erdp(&self) {
        let event_ring = self.event_ring.lock();
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        self.write_reg_u64(
            int_base + reg::ERDP,
            event_ring.dequeue_ptr(&*self.host) | 0x8,
        );
        // Polling mode must leave the xHCI interrupter disabled while
        // acknowledging Event Handler Busy / pending state.
        self.write_reg(int_base + reg::IMAN, polling_iman_value());
        self.write_op(reg::USBSTS, reg::USBSTS_EINT);
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
        loop {
            let trb = {
                let mut event_ring = self.event_ring.lock();
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
                    self.read_op_u64(reg::CRCR),
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

    /// Poll for transfer events (non-blocking)
    pub fn poll_event(&self) -> Option<Trb> {
        let mut event_ring = self.event_ring.lock();
        let trb = event_ring.try_dequeue();
        drop(event_ring);
        if trb.is_some() {
            self.update_erdp();
        }
        trb
    }

    /// Submit a command TRB
    pub fn submit_command(&self, trb: Trb) -> Result<Trb> {
        emit_xhci_diag(
            0x0300,
            trb.param,
            ((trb.status as u64) << 32) | trb.control as u64,
            0,
        );
        let mut cmd_ring = self.cmd_ring.lock();
        let (enqueue_before, cycle_before) = cmd_ring.debug_state();
        let cmd_addr = cmd_ring.enqueue(&*self.host, trb);
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
        let offset = reg::port_reg_base(self.cap_length, port);
        self.read_reg(offset)
    }

    /// Write port status (for clearing change bits, reset, etc.)
    pub fn write_port_status(&self, port: u8, val: u32) {
        let offset = reg::port_reg_base(self.cap_length, port);
        self.write_reg(offset, val);
    }

    /// Reset a port
    pub fn reset_port(&self, port: u8) -> Result<()> {
        let offset = reg::port_reg_base(self.cap_length, port);
        let mut portsc: u32 = self.read_reg(offset);
        emit_xhci_diag(0x0280, port as u64, encode_port_diag(portsc), 0);
        if (portsc & reg::PORTSC_CCS) == 0 {
            emit_xhci_diag(0x028f, port as u64, encode_port_diag(portsc), 0);
            return Err(UsbError::DeviceNotFound);
        }

        // Clear stale change bits before asserting reset while preserving the
        // controller-owned neutral port state (power/link ownership bits).
        let clear_changes = port_state_neutral(portsc) | PORT_CHANGE_BITS;
        self.write_reg(offset, clear_changes);
        portsc = self.read_reg(offset);
        emit_xhci_diag(
            0x0281,
            port as u64,
            encode_port_diag(portsc),
            clear_changes as u64,
        );

        // Keep power enabled while requesting reset.
        let reset = port_state_neutral(portsc) | reg::PORTSC_PP | reg::PORTSC_PR;
        self.write_reg(offset, reset);
        emit_xhci_diag(
            0x0282,
            port as u64,
            reset as u64,
            encode_port_diag(self.read_reg(offset)),
        );

        // Wait for reset to complete
        let mut waited = 0usize;
        loop {
            portsc = self.read_reg(offset);
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
            portsc = self.read_reg(offset);
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
        self.write_reg(offset, ack_changes);
        portsc = self.read_reg(offset);
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
            portsc = self.read_reg(offset);
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
        let settled = self.read_reg(offset);
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
        unsafe {
            self.dcbaa
                .as_ptr::<u64>()
                .add(slot as usize)
                .write_volatile(phys);
        }
    }

    /// Reads the current DCBAA slot entry for diagnostics.
    pub fn device_context_entry(&self, slot: u8) -> u64 {
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
            portsc: self.port_status(port),
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
        compose_config, compose_crcr, compose_erst_base, compose_erst_size, compose_initial_erdp,
        defer_crcr_publish_with_snapshot, defer_dcbaap_publish_with_snapshot,
        defer_erdp_publish_with_snapshot, defer_erst_publish_with_snapshot,
        defer_scratchpad_array_publish_with_snapshot,
        halt_revalidation_needed, masked_usbcmd, parse_controller_params, polling_iman_value,
        port_ready_for_enumeration, preserve_firmware_handoff_config,
        probe_live_dcbaap_before_staged_publish_with_snapshot,
        runtime_mailbox_reset_handoff, runtime_mailbox_reset_needs_blind_settle,
        runtime_stop_state_needs_post_run_settle,
        runtime_stop_state_snapshot_handoff,
        skip_config_write_during_init, skip_config_write_during_init_with_snapshot,
        skip_constructor_polling_scrub_writes,
        skip_doorbell_readback_after_ring, skip_init_pre_reset_scrub_writes,
        skip_legacy_ownership_claim_for_handoff, skip_live_halt_revalidation,
        skip_live_halt_revalidation_with_snapshot, skip_live_post_reset_verification_readbacks,
        skip_post_run_interrupter_zeroing_with_snapshot,
        skip_post_reset_cnr_poll_with_snapshot, skip_preinit_polling_scrub, skip_reset_during_init,
        skip_reset_during_init_with_snapshot, split_u64_reg_write_ops, u64_register_change_mask,
        use_atomic_erstba_publish_with_snapshot, use_atomic_runtime_ring_publish_with_snapshot,
        use_live_config_seed_reads,
        use_live_config_seed_reads_with_snapshot, use_live_post_reset_seed_reads,
        use_live_post_reset_seed_reads_with_snapshot, XhciControllerParams, XhciFirmwareHandoff,
        XhciRuntimeSeedSnapshot, SKIP_HCRST_DURING_INIT,
        TRUSTED_HANDOFF_DCBAAP_PREWRITE_READ_PROBE, TRUSTED_HANDOFF_DCBAAP_ZERO_REWRITE_PROBE,
    };
    use crate::reg;

    #[test]
    fn parse_controller_params_rejects_all_ones() {
        assert!(
            parse_controller_params(0xff, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff)
                .is_none()
        );
    }

    #[test]
    fn parse_controller_params_accepts_reasonable_window() {
        let hcs1 = 32u32 | (8u32 << 24);
        let parsed = parse_controller_params(0x40, hcs1, 0, 0x1000, 0x2000);
        assert!(parsed.is_some());
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
        };
        let validated = params.validated().expect("validated controller params");
        assert_eq!(validated.4, 64);
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
    fn stop_state_only_snapshot_skips_reset_and_live_config_seed_reads() {
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
        assert!(use_live_post_reset_seed_reads_with_snapshot(
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
        assert!(runtime_stop_state_snapshot_handoff(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(skip_reset_during_init_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!skip_config_write_during_init_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!runtime_mailbox_reset_needs_blind_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_scratchpad_array_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_dcbaap_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_crcr_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_erdp_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_erst_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(probe_live_dcbaap_before_staged_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
    }

    #[test]
    fn stop_state_only_snapshot_does_not_use_post_run_shortcuts() {
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
        assert!(!runtime_stop_state_needs_post_run_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!skip_post_run_interrupter_zeroing_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!runtime_stop_state_needs_post_run_settle(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(!skip_post_run_interrupter_zeroing_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        ));
        assert!(!runtime_stop_state_needs_post_run_settle(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
        assert!(!skip_post_run_interrupter_zeroing_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
    }

    #[test]
    fn trusted_runtime_snapshot_defers_scratchpad_array_publish() {
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
        assert!(use_atomic_erstba_publish_with_snapshot(
            XhciFirmwareHandoff::ResetlessReinit,
            snapshot,
        ));
    }

    #[test]
    fn trusted_runtime_snapshot_defers_dcbaap_publish_until_after_other_ring_state() {
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
    }

    #[test]
    fn trusted_runtime_snapshot_defers_crcr_publish_until_after_other_ring_state() {
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
    }

    #[test]
    fn trusted_runtime_snapshot_defers_erdp_publish_until_after_erst_programming() {
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
        assert!(defer_erdp_publish_with_snapshot(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            snapshot,
        ));
        assert!(!defer_erdp_publish_with_snapshot(
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
            usbcmd: Some(0),
            usbsts: Some(reg::USBSTS_HCH),
            iman0: Some(0),
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
    }

    #[test]
    fn trusted_runtime_snapshot_skips_live_dcbaap_read_before_staged_publish() {
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
    fn u64_register_change_mask_reports_low_and_high_dword_changes_independently() {
        assert_eq!(u64_register_change_mask(0x0000_0001_0000_0002, 0x0000_0001_0000_0002), 0);
        assert_eq!(u64_register_change_mask(0x0000_0001_0000_0002, 0x0000_0001_0000_0003), 1);
        assert_eq!(u64_register_change_mask(0x0000_0001_0000_0002, 0x0000_0002_0000_0002), 2);
        assert_eq!(u64_register_change_mask(0x0000_0001_0000_0002, 0x0000_0002_0000_0003), 3);
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
        assert_eq!(polling_iman_value(), reg::IMAN_IP);
        assert_eq!(polling_iman_value() & reg::IMAN_IE, 0);
    }

    #[test]
    fn masked_usbcmd_clears_interrupt_enables_only() {
        let raw = reg::USBCMD_RUN | reg::USBCMD_INTE | reg::USBCMD_HSEE;
        assert_eq!(masked_usbcmd(raw), reg::USBCMD_RUN);
    }

    #[test]
    fn halt_revalidation_depends_on_live_halt_bit() {
        assert!(halt_revalidation_needed(0));
        assert!(halt_revalidation_needed(reg::USBSTS_CNR));
        assert!(!halt_revalidation_needed(reg::USBSTS_HCH));
    }

    #[test]
    fn only_resetless_firmware_handoffs_skip_live_halt_revalidation() {
        assert!(!skip_live_halt_revalidation(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
        assert!(skip_live_halt_revalidation(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(skip_live_halt_revalidation(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert!(!skip_live_halt_revalidation(XhciFirmwareHandoff::None));
    }

    #[test]
    fn only_trusted_mailbox_reset_handoff_uses_blind_post_reset_settle() {
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
        assert_eq!(
            skip_reset_during_init(XhciFirmwareHandoff::None),
            SKIP_HCRST_DURING_INIT
        );
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
    fn trusted_handoff_constructor_still_skips_early_polling_scrub_writes() {
        assert!(skip_constructor_polling_scrub_writes(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
        assert!(skip_constructor_polling_scrub_writes(
            XhciFirmwareHandoff::PreserveControllerState
        ));
        assert!(skip_constructor_polling_scrub_writes(
            XhciFirmwareHandoff::ResetlessReinit
        ));
        assert!(!skip_constructor_polling_scrub_writes(
            XhciFirmwareHandoff::None
        ));
    }

    #[test]
    fn trusted_handoff_init_still_skips_pre_reset_scrub_writes() {
        assert!(skip_init_pre_reset_scrub_writes(
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
    fn trusted_resetless_paths_skip_post_reset_verification_readbacks() {
        assert!(skip_live_post_reset_verification_readbacks(
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ));
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
        assert!(!skip_doorbell_readback_after_ring(
            XhciFirmwareHandoff::None
        ));
    }

    #[test]
    fn only_preserve_state_handoffs_skip_config_write_during_init() {
        assert!(!skip_config_write_during_init(
            XhciFirmwareHandoff::ColdStartFromSnapshot
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
    fn event_ring_publish_accepts_zero_seeds() {
        assert_eq!(compose_erst_size(0, 1), 1);
        let composed = compose_erst_base(0, 0x0404_0030_00);
        assert_eq!(composed & reg::ERST_PTR_MASK, 0);
        assert_eq!(composed & !reg::ERST_PTR_MASK, 0x0404_0030_00);
    }
}

impl<H: Dma> Drop for XhciCtrl<H> {
    fn drop(&mut self) {
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

        // Unmap MMIO
        unsafe {
            self.host.unmap_mmio(self.mmio, self.mmio_size);
        }
    }
}
