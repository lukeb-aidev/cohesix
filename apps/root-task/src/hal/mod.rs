// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Hardware abstraction layer façade for drivers and platform helpers.
// Author: Lukas Bower

//! Lightweight hardware abstraction used by the root task to decouple
//! low-level seL4 primitives from driver code.
//!
//! The abstraction intentionally exposes only the operations that the
//! current driver set depends on. This keeps the surface area small while
//! providing a structured location for future peripherals.

#![allow(unsafe_code)]

use core::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "kernel")]
use core::{
    fmt,
    ptr::{self, NonNull},
};

// Early Pi HDMI bootstrap admits the engine and owner descriptor only. The
// steady local-seat retained-frame path owns the first framebuffer mutation,
// its completion receipt, and every later redraw.

#[cfg(any(feature = "kernel", feature = "cache-maintenance"))]
pub mod cache;

#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
/// Generation-revocable construction and transport for the isolated TCP console service.
pub mod console_network;
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
/// Construction of the six restricted critical children plus init root-control accounting.
pub mod critical_tcb;
pub mod driver_task;
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
/// Generation-revocable passive parser boundary for the target NineDoor service.
pub mod ninedoor_service;
/// Compiler-bounded executable-slot reservations and revoke sequencing.
pub mod resource_pool;
/// W^X planning and admission for separately packaged Worker child images.
pub mod worker_image;
#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
/// Real seL4 MCS object construction and containment for Worker children.
pub mod worker_task;

#[cfg(any(feature = "kernel", feature = "cache-maintenance"))]
pub mod dma;

#[cfg(feature = "kernel")]
pub mod pci;
#[cfg(feature = "kernel")]
pub mod pi4_pcie;
#[cfg(feature = "kernel")]
pub mod pi4_wifi;
#[cfg(feature = "kernel")]
pub mod uart;
#[cfg(feature = "kernel")]
pub mod virtio_mmio;

#[cfg(feature = "kernel")]
use crate::affinity::{self, DriverAffinityTarget};
#[cfg(feature = "kernel")]
use crate::hal::driver_task::{
    DriverTaskBootstrapFailure, DriverTaskBootstrapFailureReason, DriverTaskBootstrapReport,
    DriverTaskContract, DriverTaskContractError, DriverTaskRuntimeProof,
    CYW43_WIFI_DRIVER_TASK_CONTRACT, GENET_DRIVER_TASK_CONTRACT, HDMI_TEXT_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT, RTL8139_DRIVER_TASK_CONTRACT, SDIO_HOST_DRIVER_TASK_CONTRACT,
    SERIAL_DRIVER_TASK_CONTRACT, USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    VIRTIO_NET_DRIVER_TASK_CONTRACT,
};
#[cfg(feature = "kernel")]
use crate::rust_alloc::vec::Vec as AllocVec;
#[cfg(feature = "kernel")]
use crate::sel4::{
    self, DeviceCoverage, DeviceFrame, KernelEnv, KernelEnvSnapshot, RamFrame, UnmappedRamFrame,
    VSpaceTableTracker,
};
#[cfg(feature = "kernel")]
use pci::{PciAddress, PciTopology};
#[cfg(feature = "kernel")]
use pi4_driver_abi::{
    DriverRuntimeBusLinkDescriptor, DriverRuntimeInitDescriptor, DriverRuntimeIrqDescriptor,
    DriverRuntimePageDescriptor, DriverRuntimeResourceRangeDescriptor,
    DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO, DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE,
    DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE,
    DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_SLOT, DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT,
    DRIVER_RUNTIME_BUS_LINK_FLAG_DPC_EVENT_RING, DRIVER_RUNTIME_BUS_LINK_FLAG_NOTIFICATIONS,
    DRIVER_RUNTIME_BUS_LINK_FLAG_OWNER, DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
    DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE,
    DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_SLOT, DRIVER_RUNTIME_DPC_EVENT_RING_BYTES,
    DRIVER_RUNTIME_DPC_EVENT_RING_DEPTH, DRIVER_RUNTIME_DPC_EVENT_RING_OFFSET,
    DRIVER_RUNTIME_FRAMEBUFFER_VADDR, DRIVER_RUNTIME_GENET_IRQ, DRIVER_RUNTIME_GENET_IRQ_BADGE,
    DRIVER_RUNTIME_INIT_FLAG_BUS_ADDRESSING, DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS,
    DRIVER_RUNTIME_INIT_FLAG_DIRECT_GENET, DRIVER_RUNTIME_INIT_FLAG_DMA_PADDRS,
    DRIVER_RUNTIME_INIT_FLAG_FRAMEBUFFER, DRIVER_RUNTIME_INIT_FLAG_IRQS_BOUND,
    DRIVER_RUNTIME_INIT_FLAG_MMIO_MAPPED, DRIVER_RUNTIME_INIT_FLAG_POINTER_FREE,
    DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY, DRIVER_RUNTIME_INIT_FLAG_ROOT_CONTEXT_FORBIDDEN,
    DRIVER_RUNTIME_INIT_FLAG_SHARED_PADDRS, DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT,
    DRIVER_RUNTIME_PCIE_TIMER_IRQ, DRIVER_RUNTIME_PCIE_TIMER_IRQ_BADGE,
    DRIVER_RUNTIME_PI4_SYSTEM_TIMER_PADDR, DRIVER_RUNTIME_RESERVED_ROOT_BADGE,
    DRIVER_RUNTIME_RESOURCE_FLAG_CPU_ONLY, DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
    DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS, DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED,
    DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS, DRIVER_RUNTIME_RESOURCE_KIND_DMA,
    DRIVER_RUNTIME_RESOURCE_KIND_FRAMEBUFFER, DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
    DRIVER_RUNTIME_RESOURCE_KIND_SHARED, DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
    DRIVER_RUNTIME_RESOURCE_TAG_BCM2835_DMA, DRIVER_RUNTIME_RESOURCE_TAG_CYW43_CONTROL,
    DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA, DRIVER_RUNTIME_RESOURCE_TAG_GENET_DIRECT_LINK,
    DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS, DRIVER_RUNTIME_RESOURCE_TAG_HDMI_FRAMEBUFFER,
    DRIVER_RUNTIME_RESOURCE_TAG_HDMI_REGS, DRIVER_RUNTIME_RESOURCE_TAG_PCIE_HOST,
    DRIVER_RUNTIME_RESOURCE_TAG_PI4_SYSTEM_TIMER, DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
    DRIVER_RUNTIME_RESOURCE_TAG_SERIAL_MINI_UART, DRIVER_RUNTIME_RESOURCE_TAG_SHARED_CONTROL,
    DRIVER_RUNTIME_RESOURCE_TAG_USB_XHCI, DRIVER_RUNTIME_RESOURCE_TAG_WIFI_PWRSEQ,
    DRIVER_RUNTIME_RESOURCE_TAG_WIFI_PWRSEQ_REQUEST, DRIVER_RUNTIME_SDIO_DMA_IRQ,
    DRIVER_RUNTIME_SDIO_DMA_IRQ_BADGE, DRIVER_RUNTIME_SDIO_IRQ, DRIVER_RUNTIME_SDIO_IRQ_BADGE,
    DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_PAGES, DRIVER_RUNTIME_SERIAL_IRQ,
    DRIVER_RUNTIME_SERIAL_IRQ_BADGE, DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
    DRIVER_TASK_CHILD_PCIE_TIMER_IRQ_HANDLER_SLOT, DRIVER_TASK_CHILD_SDIO_DMA_IRQ_HANDLER_SLOT,
};
#[cfg(feature = "kernel")]
use sel4_sys::{seL4_ARM_VMAttributes, seL4_CPtr, seL4_Error, seL4_NoError, seL4_Word};

/// Timebase exists to unify timing for event pump + smoltcp; wiring will follow.
pub trait Timebase {
    /// Returns the current time in milliseconds.
    fn now_ms(&self) -> u64;
}

/// Supported SDIO bus widths for the Pi 4 Wi-Fi transport.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SdioBusWidth {
    OneBit,
    FourBit,
}

impl SdioBusWidth {
    /// Returns the wire-width in bits.
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Self::OneBit => 1,
            Self::FourBit => 4,
        }
    }
}

/// Addressable SDIO functions used by the CYW43455.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SdioFunction {
    Function0,
    Function1,
    Function2,
}

impl SdioFunction {
    /// Returns the numeric SDIO function identifier.
    #[must_use]
    pub const fn number(self) -> u8 {
        match self {
            Self::Function0 => 0,
            Self::Function1 => 1,
            Self::Function2 => 2,
        }
    }
}

const fn control_plane_startup_link_rescue_limit() -> u8 {
    2
}

const fn control_plane_startup_link_rescue_budget_exhausted(next_cycle: u8) -> bool {
    next_cycle >= control_plane_startup_link_rescue_limit()
}

/// HAL-owned power state for the Pi 4 Wi-Fi device.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WifiPowerState {
    Off,
    On,
}

/// HAL-owned reset line state for the Pi 4 Wi-Fi device.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WifiResetState {
    Asserted,
    Deasserted,
}

/// Compact Wi-Fi transport snapshot exposed to the root console debug path.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiDebugSnapshot {
    pub power_state: WifiPowerState,
    pub reset_state: WifiResetState,
    pub current_clock_hz: u32,
    pub preferred_data_clock_hz: u32,
    pub bus_width: SdioBusWidth,
    pub card_ready: bool,
    pub card_rca: u16,
    pub card_ocr: u32,
    pub io_enable: Option<u8>,
    pub io_ready: Option<u8>,
    pub chipclkcsr: Option<u8>,
    pub wakeupctrl: Option<u8>,
    pub sleepcsr: Option<u8>,
    pub cardcap: Option<u8>,
    pub programmed_backplane_window: Option<u32>,
    pub shadow_backplane_window: Option<u32>,
    pub shadow_backplane_fn_addr: Option<u32>,
    pub control_plane_frame_recovery_stage: Option<&'static str>,
    pub control_plane_frame_recovery_policy: Option<&'static str>,
    pub control_plane_frame_recovery_write: Option<bool>,
    pub control_plane_frame_recovery_drained: Option<bool>,
    pub control_plane_frame_recovery_count: Option<u16>,
    pub control_plane_bootstrap_phase: &'static str,
    pub control_plane_reply_mode: &'static str,
    pub control_plane_reply_attempts: u8,
    pub control_plane_reply_empty_polls: u8,
    pub control_plane_no_ht_transport: bool,
    pub control_plane_probe_pending: bool,
    pub control_plane_startup_link_stable: bool,
    pub control_plane_startup_profile_locked: bool,
    pub control_plane_startup_profile_reason: &'static str,
    pub control_plane_promoted_probe_pending: bool,
    pub debug_snapshot_source: &'static str,
    pub debug_snapshot_stage: &'static str,
    pub control_plane_startup_link_rescue_cycles: u8,
    pub control_plane_startup_link_rescue_limit: u8,
    pub control_plane_passive_startup_link_empty_poll_limit: u8,
    pub control_plane_f2_state: &'static str,
    pub control_plane_sdhci_read_diag: &'static str,
    pub control_plane_exact_error: &'static str,
}

/// Number of cached HT phase records exposed by Wi-Fi diagnostics.
#[cfg(feature = "kernel")]
pub const WIFI_HT_PHASE_RECORD_CAPACITY: usize = 4;

/// Passive HT clock state record captured from bounded firmware phases.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiHtPhaseRecord {
    pub stage: &'static str,
    pub status: &'static str,
    pub chipclkcsr: Option<u8>,
    pub wakeupctrl: Option<u8>,
    pub sleepcsr: Option<u8>,
    pub cardcap: Option<u8>,
}

#[cfg(feature = "kernel")]
impl WifiHtPhaseRecord {
    pub const EMPTY: Self = Self {
        stage: "n/a",
        status: "n/a",
        chipclkcsr: None,
        wakeupctrl: None,
        sleepcsr: None,
        cardcap: None,
    };
}

/// Cached firmware upload/release proof tuple for Wi-Fi diagnostics.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiFirmwareProofTrace {
    pub source: &'static str,
    pub upload_state: &'static str,
    pub nvram_tail_state: &'static str,
    pub reset_vector_state: &'static str,
    pub cpuhalt_state: &'static str,
    pub precondition_state: &'static str,
    pub readback_status: &'static str,
    pub verified: bool,
    pub armcr4_release_attempts: u8,
    pub upload_clock_hz: u32,
}

/// Number of cached bounded transport phase records exposed by diagnostics.
#[cfg(feature = "kernel")]
pub const WIFI_BOUNDED_PHASE_RECORD_CAPACITY: usize = 4;

/// Passive bounded no-HT/control-plane phase record for Wi-Fi diagnostics.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiBoundedPhaseRecord {
    pub stage: &'static str,
    pub action: &'static str,
    pub mode: &'static str,
    pub current_clock_hz: u32,
    pub bus_width: &'static str,
    pub no_ht_transport: bool,
}

#[cfg(feature = "kernel")]
impl WifiBoundedPhaseRecord {
    pub const EMPTY: Self = Self {
        stage: "n/a",
        action: "n/a",
        mode: "n/a",
        current_clock_hz: 0,
        bus_width: "n/a",
        no_ht_transport: false,
    };
}

/// Firmware-release contract evidence for the Pi 4 Wi-Fi debug path.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiFirmwareContractTrace {
    pub firmware_len: usize,
    pub nvram_len: usize,
    pub clm_len: Option<usize>,
    pub firmware_sha256: &'static str,
    pub nvram_sha256: &'static str,
    pub clm_sha256: &'static str,
    pub board_type: &'static str,
    pub reset_vector: Option<u32>,
    pub firmware_download_verified: bool,
    pub armcr4_release_attempts: u8,
    pub sr_kso_clock_ready: bool,
    pub alp_request: u8,
    pub ht_request: u8,
    pub ht_retry_request: u8,
    pub force_ht_after_proof_request: Option<u8>,
    pub chipclkcsr: Option<u8>,
    pub wakeupctrl: Option<u8>,
    pub sleepcsr: Option<u8>,
    pub cardcap: Option<u8>,
    pub f1_state: &'static str,
    pub f2_state: &'static str,
    pub current_clock_hz: u32,
    pub preferred_data_clock_hz: u32,
    pub blocker: &'static str,
    pub next_step: &'static str,
    pub proof: Option<WifiFirmwareProofTrace>,
    pub ht_summary: &'static str,
    pub function2_gate: &'static str,
    pub ht_phase_count: u8,
    pub ht_phase_records: [WifiHtPhaseRecord; WIFI_HT_PHASE_RECORD_CAPACITY],
}

/// Raw SDHCI contract evidence for the current Wi-Fi control-plane frontier.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiSdhciContractTrace {
    pub current_diag: &'static str,
    pub preserved_diag: &'static str,
    pub resolved_diag: &'static str,
    pub current_cmd: Option<u16>,
    pub current_arg: Option<u32>,
    pub current_present: Option<u32>,
    pub current_int_status: Option<u32>,
    pub preserved_cmd: Option<u16>,
    pub preserved_arg: Option<u32>,
    pub preserved_present: Option<u32>,
    pub preserved_int_status: Option<u32>,
}

/// Raw Wi-Fi control-plane register and cache evidence for console traces.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiControlPlaneTrace {
    pub cccr_io_enable: Option<u8>,
    pub cccr_io_ready: Option<u8>,
    pub cccr_int_enable: Option<u8>,
    pub f1_rframe_lo: Option<u8>,
    pub f1_rframe_hi: Option<u8>,
    pub f1_watermark: Option<u8>,
    pub f1_device_ctl: Option<u8>,
    pub f1_mesbusyctl: Option<u8>,
    pub block_size_shadow: u32,
    pub transfer_mode_shadow: u32,
    pub backplane_window_low: u8,
    pub backplane_window_mid: u8,
    pub backplane_window_high: u8,
    pub cached_source: &'static str,
    pub cached_stage: &'static str,
    pub cached_exact_error: &'static str,
    pub cached_sdhci_read_diag: &'static str,
    pub cached_f2_state: &'static str,
    pub cached_cccr_io_enable: Option<u8>,
    pub cached_cccr_io_ready: Option<u8>,
    pub cached_cccr_int_enable: Option<u8>,
    pub cached_cccr_bus_interface: Option<u8>,
    pub cached_cccr_speed: Option<u8>,
    pub cached_cccr_cardcap: Option<u8>,
    pub cached_fbr1_block_size: Option<u16>,
    pub cached_fbr2_block_size: Option<u16>,
    pub bounded_phase_count: u8,
    pub bounded_phase_records: [WifiBoundedPhaseRecord; WIFI_BOUNDED_PHASE_RECORD_CAPACITY],
}

/// Root-console Wi-Fi debug hooks backed by the kernel HAL.
#[cfg(feature = "kernel")]
pub trait WifiDebugOps {
    fn dump_state(&mut self, stage: &'static str) -> Result<WifiDebugSnapshot, HalError>;
    fn firmware_contract_trace(&mut self) -> Option<WifiFirmwareContractTrace> {
        None
    }
    fn sdhci_contract_trace(&mut self) -> Option<WifiSdhciContractTrace> {
        None
    }
    fn control_plane_trace(&mut self) -> Option<WifiControlPlaneTrace> {
        None
    }
}

/// HAL-provided firmware payloads for the Pi 4 Wi-Fi path.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WifiFirmwareBundle<'a> {
    pub firmware: &'a [u8],
    pub nvram: &'a [u8],
    pub clm_blob: Option<&'a [u8]>,
    pub board_type: &'a str,
}

impl<'a> WifiFirmwareBundle<'a> {
    /// Constructs a firmware bundle view backed by HAL-owned storage.
    #[must_use]
    pub const fn new(
        firmware: &'a [u8],
        nvram: &'a [u8],
        clm_blob: Option<&'a [u8]>,
        board_type: &'a str,
    ) -> Self {
        Self {
            firmware,
            nvram,
            clm_blob,
            board_type,
        }
    }

    /// Validates the presence of the bounded firmware assets expected by CYW43455.
    pub fn validate(self) -> Result<(), &'static str> {
        if self.firmware.is_empty() {
            return Err("missing-firmware");
        }
        if self.nvram.is_empty() {
            return Err("missing-nvram");
        }
        if self.board_type.trim().is_empty() {
            return Err("missing-board-type");
        }
        Ok(())
    }
}

/// Lightweight IRQ identifier used across drivers.
#[cfg(feature = "kernel")]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Irq(pub u32);

/// Trigger mode requested when deriving an IRQHandler capability.
#[cfg(feature = "kernel")]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum IrqTrigger {
    /// Level-triggered interrupt line. This is the correct mode for Pi 4 SDIO
    /// and PCIe INTx-style lines.
    Level,
    /// Edge-triggered interrupt line.
    Edge,
}

#[cfg(feature = "kernel")]
impl IrqTrigger {
    #[must_use]
    const fn arm_trigger_word(self) -> seL4_Word {
        match self {
            Self::Level => 0,
            Self::Edge => 1,
        }
    }
}

/// HAL-owned seL4 IRQ binding.
///
/// Drivers may inspect this for diagnostics, but acquisition and release stay
/// behind [`KernelHal`] / [`DeviceHal`]. Device-specific clearing stays in the
/// driver because xHCI, SDIO, GENET, and future devices all clear different
/// source registers.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelIrqBinding {
    irq: Irq,
    trigger: IrqTrigger,
    handler_slot: seL4_CPtr,
    notification_slot: seL4_CPtr,
    badged_notification_slot: seL4_CPtr,
    badge: seL4_Word,
    owns_notification: bool,
}

#[cfg(feature = "kernel")]
impl KernelIrqBinding {
    /// Returns the IRQ number covered by this binding.
    #[must_use]
    pub const fn irq(self) -> Irq {
        self.irq
    }

    /// Returns the seL4 trigger mode used when deriving the IRQHandler cap.
    #[must_use]
    pub const fn trigger(self) -> IrqTrigger {
        self.trigger
    }

    /// Returns the HAL-minted notification badge for diagnostics.
    #[must_use]
    pub const fn badge(self) -> seL4_Word {
        self.badge
    }

    /// Returns the IRQHandler cap slot for diagnostics only.
    #[must_use]
    pub const fn handler_slot_for_diagnostics(self) -> seL4_CPtr {
        self.handler_slot
    }

    /// Returns the notification object cap slot for diagnostics only.
    #[must_use]
    pub const fn notification_slot_for_diagnostics(self) -> seL4_CPtr {
        self.notification_slot
    }

    pub(crate) fn ack_from_hal(&self) -> Result<(), HalError> {
        let err = sel4::irq_handler_ack(self.handler_slot);
        if err == seL4_NoError {
            Ok(())
        } else {
            Err(HalError::Sel4(err))
        }
    }

    pub(crate) fn poll_and_service_from_hal<F>(
        &self,
        clear_device_source: F,
    ) -> Result<IrqServiceOutcome, HalError>
    where
        F: FnOnce() -> Result<(), HalError>,
    {
        let mut badge = 0;
        let _ = sel4::poll(self.notification_slot, &mut badge);
        if badge == 0 {
            return Ok(IrqServiceOutcome::Idle);
        }

        clear_device_source()?;
        self.ack_from_hal()?;
        Ok(IrqServiceOutcome::Serviced { badge })
    }

    pub(crate) fn wait_and_service_from_hal<F>(
        &self,
        clear_device_source: F,
    ) -> Result<seL4_Word, HalError>
    where
        F: FnOnce() -> Result<(), HalError>,
    {
        let mut badge = 0;
        let _ = sel4::wait(self.notification_slot, &mut badge);
        clear_device_source()?;
        self.ack_from_hal()?;
        Ok(badge)
    }
}

/// Result from a non-blocking IRQ service attempt.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IrqServiceOutcome {
    /// No notification was pending.
    Idle,
    /// A notification was observed, the device source was cleared, and the
    /// IRQHandler was acknowledged.
    Serviced { badge: seL4_Word },
}

/// Abstraction over IRQ controller behaviour.
#[cfg(feature = "kernel")]
pub trait IrqCtl {
    /// Returns the next pending IRQ when available.
    fn poll(&self) -> Option<Irq>;

    /// Acknowledges a previously observed IRQ.
    fn ack(&self, irq: Irq);
}

#[cfg(feature = "kernel")]
#[must_use]
const fn irq_notification_badge(irq: Irq) -> seL4_Word {
    irq.0 as seL4_Word + 1
}

/// Deterministic, pump-driven timebase suitable for dev-virt.
#[derive(Debug)]
pub struct MonotonicTimebase {
    counter_ms: AtomicU64,
}

impl MonotonicTimebase {
    /// Constructs a new timebase seeded at zero.
    pub const fn new() -> Self {
        Self {
            counter_ms: AtomicU64::new(0),
        }
    }

    /// Advances the timebase by the supplied delta in milliseconds.
    pub fn advance_ms(&self, delta_ms: u64) {
        self.counter_ms.fetch_add(delta_ms, Ordering::Relaxed);
    }

    /// Sets the timebase to an absolute value in milliseconds.
    pub fn set(&self, now_ms: u64) {
        self.counter_ms.store(now_ms, Ordering::Relaxed);
    }
}

impl Default for MonotonicTimebase {
    fn default() -> Self {
        Self::new()
    }
}

impl Timebase for MonotonicTimebase {
    fn now_ms(&self) -> u64 {
        self.counter_ms.load(Ordering::Relaxed)
    }
}

static DEFAULT_TIMEBASE: MonotonicTimebase = MonotonicTimebase::new();
#[cfg(test)]
static TIMEBASE_SET_COUNT: AtomicU64 = AtomicU64::new(0);

/// Returns the shared default timebase for the root task.
pub fn default_timebase() -> &'static dyn Timebase {
    &DEFAULT_TIMEBASE
}

/// Returns the active timebase used by the root task.
pub fn timebase() -> &'static dyn Timebase {
    default_timebase()
}

/// Sets the shared default timebase to an absolute value.
pub fn set_timebase_now_ms(now_ms: u64) {
    DEFAULT_TIMEBASE.set(now_ms);
    #[cfg(test)]
    TIMEBASE_SET_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Returns how many times tests published the shared default timebase.
#[cfg(test)]
pub fn timebase_set_count() -> u64 {
    TIMEBASE_SET_COUNT.load(Ordering::Relaxed)
}

/// Advances the shared default timebase by the provided delta.
pub fn advance_default_timebase(delta_ms: u64) {
    DEFAULT_TIMEBASE.advance_ms(delta_ms);
}

/// Mapping permissions used by the HAL when creating virtual regions.
#[cfg(feature = "kernel")]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MapPerms {
    pub read: bool,
    pub write: bool,
}

#[cfg(feature = "kernel")]
impl MapPerms {
    pub const R: Self = Self {
        read: true,
        write: false,
    };

    pub const RW: Self = Self {
        read: true,
        write: true,
    };
}

/// HAL-managed mapping of device memory returned to drivers.
#[cfg(feature = "kernel")]
#[derive(Clone)]
pub struct MappedRegion {
    frame: DeviceFrame,
    size: usize,
    perms: MapPerms,
}

#[cfg(feature = "kernel")]
impl MappedRegion {
    /// Constructs a mapped region from an existing device frame.
    #[must_use]
    pub const fn new(frame: DeviceFrame, size: usize, perms: MapPerms) -> Self {
        Self { frame, size, perms }
    }

    /// Returns the permissions assigned to this mapping.
    #[must_use]
    pub const fn perms(&self) -> MapPerms {
        self.perms
    }

    /// Returns the size of the mapped region in bytes.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }

    /// Returns the underlying virtual pointer backing the mapping.
    #[must_use]
    pub fn ptr(&self) -> NonNull<u8> {
        self.frame.ptr()
    }

    /// Returns the physical base address for the mapping.
    #[must_use]
    pub fn paddr(&self) -> usize {
        self.frame.paddr()
    }

    fn checked_register_ptr<T>(&self, offset: usize, write: bool) -> Result<*mut T, HalError> {
        if write && !self.perms.write {
            return Err(HalError::Unsupported("mapped-region-write-denied"));
        }
        if !write && !self.perms.read {
            return Err(HalError::Unsupported("mapped-region-read-denied"));
        }
        let width = core::mem::size_of::<T>();
        let align = core::mem::align_of::<T>();
        let Some(end) = offset.checked_add(width) else {
            return Err(HalError::Unsupported("mapped-region-register-overflow"));
        };
        if width == 0 || end > self.size || offset % align != 0 {
            return Err(HalError::Unsupported("mapped-region-register-out-of-range"));
        }
        // SAFETY: `end <= self.size` bounds the register to the HAL-owned
        // mapping, and the caller receives only a typed pointer for volatile
        // register access inside this module.
        Ok(unsafe { self.ptr().as_ptr().add(offset).cast::<T>() })
    }

    /// Reads an 8-bit register from the mapped region.
    pub fn read_u8(&self, offset: usize) -> Result<u8, HalError> {
        let ptr = self.checked_register_ptr::<u8>(offset, false)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        Ok(unsafe { ptr::read_volatile(ptr.cast_const()) })
    }

    /// Reads a 16-bit register from the mapped region.
    pub fn read_u16(&self, offset: usize) -> Result<u16, HalError> {
        let ptr = self.checked_register_ptr::<u16>(offset, false)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        Ok(unsafe { ptr::read_volatile(ptr.cast_const()) })
    }

    /// Reads a 32-bit register from the mapped region.
    pub fn read_u32(&self, offset: usize) -> Result<u32, HalError> {
        let ptr = self.checked_register_ptr::<u32>(offset, false)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        Ok(unsafe { ptr::read_volatile(ptr.cast_const()) })
    }

    /// Writes an 8-bit register in the mapped region.
    pub fn write_u8(&self, offset: usize, value: u8) -> Result<(), HalError> {
        let ptr = self.checked_register_ptr::<u8>(offset, true)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        unsafe { ptr::write_volatile(ptr, value) };
        Ok(())
    }

    /// Writes a 16-bit register in the mapped region.
    pub fn write_u16(&self, offset: usize, value: u16) -> Result<(), HalError> {
        let ptr = self.checked_register_ptr::<u16>(offset, true)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        unsafe { ptr::write_volatile(ptr, value) };
        Ok(())
    }

    /// Writes a 32-bit register in the mapped region.
    pub fn write_u32(&self, offset: usize, value: u32) -> Result<(), HalError> {
        let ptr = self.checked_register_ptr::<u32>(offset, true)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        unsafe { ptr::write_volatile(ptr, value) };
        Ok(())
    }
}

#[cfg(feature = "kernel")]
const HAL_PAGE_SIZE: usize = 1 << sel4_sys::seL4_PageBits;

/// Bounded MMIO register window derived from a HAL-owned mapping.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MappedRegisterWindow {
    base_vaddr: usize,
    size: usize,
}

#[cfg(feature = "kernel")]
impl MappedRegisterWindow {
    fn new(base_vaddr: usize, size: usize) -> Result<Self, HalError> {
        if base_vaddr == 0 || size == 0 {
            return Err(HalError::Unsupported("register-window-empty"));
        }
        if base_vaddr & (HAL_PAGE_SIZE - 1) != 0 {
            return Err(HalError::Unsupported("register-window-base-unaligned"));
        }
        Ok(Self { base_vaddr, size })
    }

    #[cfg(test)]
    fn new_test(base_vaddr: usize, size: usize) -> Result<Self, HalError> {
        if base_vaddr == 0 || size == 0 {
            return Err(HalError::Unsupported("register-window-empty"));
        }
        if !base_vaddr.is_multiple_of(core::mem::align_of::<u32>()) {
            return Err(HalError::Unsupported("register-window-base-unaligned"));
        }
        Ok(Self { base_vaddr, size })
    }

    fn checked_register_ptr<T>(&self, offset: usize) -> Result<*mut T, HalError> {
        let width = core::mem::size_of::<T>();
        let align = core::mem::align_of::<T>();
        let Some(end) = offset.checked_add(width) else {
            return Err(HalError::Unsupported("register-window-offset-overflow"));
        };
        if width == 0 || offset % align != 0 {
            return Err(HalError::Unsupported("register-window-offset-unaligned"));
        }
        let page_offset = offset % HAL_PAGE_SIZE;
        if end > self.size || page_offset + width > HAL_PAGE_SIZE {
            return Err(HalError::Unsupported("register-window-offset-out-of-range"));
        }
        let Some(vaddr) = self.base_vaddr.checked_add(offset) else {
            return Err(HalError::Unsupported("register-window-vaddr-overflow"));
        };
        // SAFETY: the window is constructed from a HAL-owned MMIO mapping and
        // the offset arithmetic above bounds access to one mapped page.
        Ok(ptr::with_exposed_provenance_mut::<u8>(vaddr).cast::<T>())
    }

    /// Reads a 32-bit register from the mapped MMIO window.
    pub fn read_u32(&self, offset: usize) -> Result<u32, HalError> {
        let ptr = self.checked_register_ptr::<u32>(offset)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned MMIO window.
        Ok(unsafe { ptr::read_volatile(ptr.cast_const()) })
    }

    /// Writes a 32-bit register to the mapped MMIO window.
    pub fn write_u32(&self, offset: usize, value: u32) -> Result<(), HalError> {
        let ptr = self.checked_register_ptr::<u32>(offset)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned MMIO window.
        unsafe { ptr::write_volatile(ptr, value) };
        Ok(())
    }
}

/// HAL-owned contiguous device register pages.
#[cfg(feature = "kernel")]
#[derive(Clone)]
pub struct MappedRegisterPages<const N: usize> {
    base_paddr: usize,
    pages: heapless::Vec<DeviceFrame, N>,
}

#[cfg(feature = "kernel")]
impl<const N: usize> MappedRegisterPages<N> {
    /// Constructs a register mapping from already mapped contiguous pages.
    pub fn new(base_paddr: usize, pages: heapless::Vec<DeviceFrame, N>) -> Result<Self, HalError> {
        if pages.is_empty() {
            return Err(HalError::Unsupported("empty-register-pages"));
        }
        if base_paddr & (HAL_PAGE_SIZE - 1) != 0 {
            return Err(HalError::Unsupported("register-pages-base-unaligned"));
        }
        for (index, frame) in pages.iter().enumerate() {
            let Some(offset) = index.checked_mul(HAL_PAGE_SIZE) else {
                return Err(HalError::Unsupported("register-pages-offset-overflow"));
            };
            let Some(expected_paddr) = base_paddr.checked_add(offset) else {
                return Err(HalError::Unsupported("register-pages-paddr-overflow"));
            };
            if frame.paddr() != expected_paddr {
                return Err(HalError::Unsupported("register-pages-noncontiguous"));
            }
        }
        Ok(Self { base_paddr, pages })
    }

    /// Constructs a single-page register mapping.
    pub fn single(frame: DeviceFrame) -> Result<Self, HalError> {
        let base_paddr = frame.paddr();
        let mut pages = heapless::Vec::new();
        pages
            .push(frame)
            .map_err(|_| HalError::Unsupported("register-page-capacity"))?;
        Self::new(base_paddr, pages)
    }

    /// Returns the physical base address for the first register page.
    #[must_use]
    pub const fn base_paddr(&self) -> usize {
        self.base_paddr
    }

    /// Returns the virtual base pointer for the first register page.
    #[must_use]
    pub fn base_ptr(&self) -> NonNull<u8> {
        self.pages[0].ptr()
    }

    /// Returns the mapped register span in bytes.
    #[must_use]
    pub fn size(&self) -> usize {
        self.pages.len() * HAL_PAGE_SIZE
    }

    /// Returns the virtual address corresponding to a register offset.
    #[must_use]
    pub fn vaddr_for_offset(&self, offset: usize) -> Option<usize> {
        self.checked_register_ptr::<u8>(offset, false)
            .ok()
            .map(|ptr| ptr as usize)
    }

    /// Returns a bounded MMIO window for safe storage outside HAL-owned setup.
    pub fn register_window(&self) -> Result<MappedRegisterWindow, HalError> {
        MappedRegisterWindow::new(self.base_ptr().as_ptr() as usize, self.size())
    }

    /// Returns the physical address corresponding to a register offset.
    #[must_use]
    pub fn paddr_for_offset(&self, offset: usize) -> Option<usize> {
        if offset >= self.size() {
            None
        } else {
            self.base_paddr.checked_add(offset)
        }
    }

    fn checked_register_ptr<T>(&self, offset: usize, _write: bool) -> Result<*mut T, HalError> {
        let width = core::mem::size_of::<T>();
        let align = core::mem::align_of::<T>();
        let Some(end) = offset.checked_add(width) else {
            return Err(HalError::Unsupported("register-pages-offset-overflow"));
        };
        if width == 0 || offset % align != 0 {
            return Err(HalError::Unsupported("register-pages-offset-unaligned"));
        }
        let page = offset / HAL_PAGE_SIZE;
        let page_offset = offset % HAL_PAGE_SIZE;
        if end > self.size() || page_offset + width > HAL_PAGE_SIZE {
            return Err(HalError::Unsupported("register-pages-offset-out-of-range"));
        }
        let Some(frame) = self.pages.get(page) else {
            return Err(HalError::Unsupported("register-pages-page-missing"));
        };
        // SAFETY: offset arithmetic is bounded to a single HAL-owned mapped
        // page above, so the returned pointer addresses only mapped MMIO.
        Ok(unsafe { frame.ptr().as_ptr().add(page_offset).cast::<T>() })
    }

    /// Reads an 8-bit register from the mapped pages.
    pub fn read_u8(&self, offset: usize) -> Result<u8, HalError> {
        let ptr = self.checked_register_ptr::<u8>(offset, false)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        Ok(unsafe { ptr::read_volatile(ptr.cast_const()) })
    }

    /// Reads a 16-bit register from the mapped pages.
    pub fn read_u16(&self, offset: usize) -> Result<u16, HalError> {
        let ptr = self.checked_register_ptr::<u16>(offset, false)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        Ok(unsafe { ptr::read_volatile(ptr.cast_const()) })
    }

    /// Reads a 32-bit register from the mapped pages.
    pub fn read_u32(&self, offset: usize) -> Result<u32, HalError> {
        let ptr = self.checked_register_ptr::<u32>(offset, false)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        Ok(unsafe { ptr::read_volatile(ptr.cast_const()) })
    }

    /// Writes an 8-bit register to the mapped pages.
    pub fn write_u8(&self, offset: usize, value: u8) -> Result<(), HalError> {
        let ptr = self.checked_register_ptr::<u8>(offset, true)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        unsafe { ptr::write_volatile(ptr, value) };
        Ok(())
    }

    /// Writes a 16-bit register to the mapped pages.
    pub fn write_u16(&self, offset: usize, value: u16) -> Result<(), HalError> {
        let ptr = self.checked_register_ptr::<u16>(offset, true)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        unsafe { ptr::write_volatile(ptr, value) };
        Ok(())
    }

    /// Writes a 32-bit register to the mapped pages.
    pub fn write_u32(&self, offset: usize, value: u32) -> Result<(), HalError> {
        let ptr = self.checked_register_ptr::<u32>(offset, true)?;
        // SAFETY: `checked_register_ptr` verified bounds and alignment for the
        // HAL-owned mapping.
        unsafe { ptr::write_volatile(ptr, value) };
        Ok(())
    }
}

/// PCI command register flags manipulated by the HAL.
#[cfg(feature = "kernel")]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct PciCommandFlags {
    bits: u16,
}

#[cfg(feature = "kernel")]
impl PciCommandFlags {
    pub const IO_SPACE: Self = Self { bits: 1 << 0 };
    pub const MEMORY_SPACE: Self = Self { bits: 1 << 1 };
    pub const BUS_MASTER: Self = Self { bits: 1 << 2 };

    /// Returns an empty flag set.
    #[must_use]
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    /// Returns the raw bitfield representation.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.bits
    }

    /// Returns true when all flags in `other` are present in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.bits & other.bits) == other.bits
    }

    /// Returns a new flag set containing all bits from both operands.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }
}

#[cfg(feature = "kernel")]
impl core::ops::BitOr for PciCommandFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

#[cfg(feature = "kernel")]
impl core::ops::BitOrAssign for PciCommandFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.bits |= rhs.bits;
    }
}

/// Errors surfaced by hardware accessors.
#[cfg(feature = "kernel")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalError {
    /// seL4 system call failure while manipulating capabilities or mappings.
    Sel4(seL4_Error),
    /// The requested platform does not expose PCI.
    NoPci,
    /// The requested PCI address is invalid or not present in the topology.
    InvalidPciAddress,
    /// The requested BAR is missing.
    PciBarUnavailable,
    /// Requested operation is unsupported by the current platform.
    Unsupported(&'static str),
    /// HAL driver-task contract rejected a hardware service path.
    DriverTaskContract(DriverTaskContractError),
    /// Manifest-driven TCB affinity failed validation or application.
    Affinity(affinity::AffinityError),
}

#[cfg(feature = "kernel")]
impl fmt::Display for HalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sel4(err) => write!(f, "seL4 error {err:?}"),
            Self::NoPci => f.write_str("pci unavailable"),
            Self::InvalidPciAddress => f.write_str("invalid pci address"),
            Self::PciBarUnavailable => f.write_str("pci bar unavailable"),
            Self::Unsupported(reason) => write!(f, "unsupported operation: {reason}"),
            Self::DriverTaskContract(err) => f.write_str(err.reason()),
            Self::Affinity(err) => write!(f, "affinity error: {err}"),
        }
    }
}

#[cfg(feature = "kernel")]
impl From<seL4_Error> for HalError {
    fn from(value: seL4_Error) -> Self {
        Self::Sel4(value)
    }
}

/// Core device-memory contract implemented by hardware providers used inside
/// the VM.
#[cfg(feature = "kernel")]
pub trait DeviceHal {
    /// Error type emitted by the hardware provider.
    type Error;

    /// Maps the physical device page at `paddr` into the device window.
    fn map_device(&mut self, paddr: usize) -> Result<DeviceFrame, Self::Error>;

    /// Allocates a DMA-capable frame and maps it into the DMA window.
    fn alloc_dma_frame(&mut self) -> Result<RamFrame, Self::Error>;

    /// Allocates a DMA-capable frame from the lowest-address RAM untyped.
    ///
    /// Platforms may route this to the default DMA allocator when no low-memory
    /// distinction is required.
    fn alloc_dma_frame_low(&mut self) -> Result<RamFrame, Self::Error> {
        self.alloc_dma_frame()
    }

    /// Allocates a DMA-capable frame with the requested cache attribute.
    fn alloc_dma_frame_attr(
        &mut self,
        attr: seL4_ARM_VMAttributes,
    ) -> Result<RamFrame, Self::Error> {
        let _ = attr;
        self.alloc_dma_frame()
    }

    /// Allocates a low-address DMA-capable frame with the requested cache attribute.
    fn alloc_dma_frame_low_attr(
        &mut self,
        attr: seL4_ARM_VMAttributes,
    ) -> Result<RamFrame, Self::Error> {
        let _ = attr;
        self.alloc_dma_frame_low()
    }

    /// Reserves an unmapped guard page in the DMA window and returns its base.
    fn reserve_dma_guard_page(&mut self) -> Result<usize, Self::Error>;

    /// Returns device coverage information for diagnostics.
    fn device_coverage(&self, paddr: usize, size_bits: usize) -> Option<DeviceCoverage>;

    /// Snapshot of allocator usage for debugging.
    fn snapshot(&self) -> KernelEnvSnapshot;

    /// Creates a HAL-owned IRQHandler notification binding for a device IRQ.
    #[cfg(feature = "kernel")]
    fn bind_irq_notification(
        &mut self,
        irq: Irq,
        trigger: IrqTrigger,
    ) -> Result<KernelIrqBinding, Self::Error>
    where
        Self::Error: From<HalError>,
    {
        let _ = irq;
        let _ = trigger;
        Err(HalError::Unsupported("irq-notification").into())
    }

    /// Acknowledges a HAL-owned IRQ binding after the device source is clear.
    #[cfg(feature = "kernel")]
    fn ack_irq_notification(&mut self, binding: &KernelIrqBinding) -> Result<(), Self::Error>
    where
        Self::Error: From<HalError>,
    {
        binding.ack_from_hal().map_err(Self::Error::from)
    }
}

/// PCI capability layer for platforms that expose discoverable PCI/MMIO BARs.
#[cfg(feature = "kernel")]
pub trait PciHal: DeviceHal {
    /// Returns the discovered PCI topology for the platform when available.
    fn pci_topology(&self) -> Option<&PciTopology>;

    /// Maps the specified BAR for the supplied PCI address into virtual memory.
    fn map_pci_bar(
        &mut self,
        addr: PciAddress,
        bar_index: u8,
        perms: MapPerms,
    ) -> Result<MappedRegion, Self::Error>
    where
        Self::Error: From<HalError>,
    {
        let _ = addr;
        let _ = bar_index;
        let _ = perms;
        Err(HalError::NoPci.into())
    }

    /// Configures the PCI command register for the supplied device.
    fn configure_pci_device(
        &mut self,
        addr: PciAddress,
        command_flags: PciCommandFlags,
    ) -> Result<(), Self::Error>
    where
        Self::Error: From<HalError>,
    {
        let _ = addr;
        let _ = command_flags;
        Err(HalError::NoPci.into())
    }
}

/// CYW43-over-SDIO capability layer used by the Pi 4 Wi-Fi transport.
#[cfg(feature = "kernel")]
pub trait Cyw43Hal: DeviceHal {
    /// Returns the firmware assets required for the Pi 4 Wi-Fi path.
    fn wifi_firmware_bundle(&self) -> Result<WifiFirmwareBundle<'static>, Self::Error>
    where
        Self::Error: From<HalError>,
    {
        Err(HalError::Unsupported("wifi-firmware-bundle").into())
    }
}

/// Compatibility façade for callers that still need the current full HAL
/// surface. New code should prefer the narrowest capability trait it needs:
/// [`DeviceHal`] for generic MMIO/DMA access, [`PciHal`] for PCI-backed
/// devices, and [`Cyw43Hal`] for the Pi 4 CYW43 transport path.
#[cfg(feature = "kernel")]
pub trait Hardware: PciHal + Cyw43Hal {}

#[cfg(feature = "kernel")]
impl<T> Hardware for T where T: PciHal + Cyw43Hal {}

/// seL4-backed hardware provider that owns the [`KernelEnv`].
#[cfg(feature = "kernel")]
pub struct KernelHal<'a> {
    env: KernelEnv<'a>,
    root_control_wake_notification_origin: seL4_CPtr,
    driver_tasks: heapless::Vec<KernelDriverTaskHandle, MAX_KERNEL_DRIVER_TASKS>,
    dormant_driver_tcbs: heapless::Vec<(DriverTaskContract, seL4_CPtr), 3>,
    driver_task_report: DriverTaskBootstrapReport,
}

#[cfg(feature = "kernel")]
const MAX_KERNEL_DRIVER_TASKS: usize = 9;

#[cfg(feature = "kernel")]
const DRIVER_TASK_BOOTSTRAP_CONTRACTS: &[DriverTaskContract] = &[
    SERIAL_DRIVER_TASK_CONTRACT,
    SDIO_HOST_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT,
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    HDMI_TEXT_DRIVER_TASK_CONTRACT,
    GENET_DRIVER_TASK_CONTRACT,
    CYW43_WIFI_DRIVER_TASK_CONTRACT,
    RTL8139_DRIVER_TASK_CONTRACT,
    VIRTIO_NET_DRIVER_TASK_CONTRACT,
];

#[cfg(feature = "kernel")]
const PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_WIFI_SELECTED: &[DriverTaskContract] = &[
    SERIAL_DRIVER_TASK_CONTRACT,
    HDMI_TEXT_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT,
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    SDIO_HOST_DRIVER_TASK_CONTRACT,
    CYW43_WIFI_DRIVER_TASK_CONTRACT,
];

#[cfg(feature = "kernel")]
const PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_WIRED_SELECTED: &[DriverTaskContract] = &[
    SERIAL_DRIVER_TASK_CONTRACT,
    HDMI_TEXT_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT,
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    GENET_DRIVER_TASK_CONTRACT,
];

#[cfg(feature = "kernel")]
const PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_BASE: &[DriverTaskContract] = &[
    SERIAL_DRIVER_TASK_CONTRACT,
    HDMI_TEXT_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT,
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
];

#[cfg(feature = "kernel")]
const PHYSICAL_PI_DRIVER_TASK_FAULT_CONTRACTS: &[DriverTaskContract] = &[
    SERIAL_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT,
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    HDMI_TEXT_DRIVER_TASK_CONTRACT,
    SDIO_HOST_DRIVER_TASK_CONTRACT,
    CYW43_WIFI_DRIVER_TASK_CONTRACT,
    GENET_DRIVER_TASK_CONTRACT,
];

/// Conservative non-payload capability allowance for each isolated runtime.
///
/// This covers the task objects, command/recovery caps, translation
/// tables for every declared virtual region, IRQ/bus-link caps, and bounded
/// growth in those fixed structures. Page-backed resources are accounted for
/// separately below.
#[cfg(feature = "kernel")]
const DRIVER_TASK_CSPACE_FIXED_CAPS_PER_RUNTIME: usize = 64;

/// Root-task capability headroom retained after all selected Pi runtimes exist.
///
/// Recovery, operator service, later IRQ setup, and network operation must not
/// begin with a nearly exhausted init CNode. This reserve is deliberately part
/// of admission rather than an optimistic post-bootstrap observation.
#[cfg(feature = "kernel")]
const DRIVER_TASK_CSPACE_POST_BOOT_RESERVE: usize = 2048;

/// Root slots consumed by one suspended fault-only driver identity.
///
/// This covers the CNode, TCB, command endpoint origin and badge, VSpace,
/// Reply, completion notification origin and receive cap, two fault caps, and
/// scheduling context. Child-CNode slots do not consume root CSpace slots.
#[cfg(feature = "kernel")]
const DORMANT_DRIVER_FAULT_IDENTITY_ROOT_SLOTS: usize = 11;

/// Maximum framebuffer span accepted by the HDMI runtime mapper.
#[cfg(feature = "kernel")]
const DRIVER_TASK_CSPACE_MAX_FRAMEBUFFER_PAGES: usize = 2048;

/// A conservative source-plus-mapping allowance for aliasable page resources.
///
/// Code, stack, and framebuffer pages now transfer one cap directly into the
/// child VSpace. Budgeting two slots nevertheless prevents a future root-alias
/// regression from silently restoring the CSpace exhaustion fixed here.
#[cfg(feature = "kernel")]
const DRIVER_TASK_CSPACE_CAPS_PER_ALIASABLE_PAGE: usize = 2;

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DriverTaskCspaceBudget {
    required_slots: usize,
    available_slots: usize,
    reserve_slots: usize,
    contract_count: usize,
    dormant_contract_count: usize,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DriverTaskCspacePreflightError {
    MissingRuntimeImage(&'static str),
    InvalidRuntimeImage(&'static str),
    ArithmeticOverflow,
    Insufficient {
        required_slots: usize,
        available_slots: usize,
    },
}

#[cfg(feature = "kernel")]
impl fmt::Display for DriverTaskCspacePreflightError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRuntimeImage(contract) => {
                write!(f, "runtime-image-missing contract={contract}")
            }
            Self::InvalidRuntimeImage(contract) => {
                write!(f, "runtime-image-invalid contract={contract}")
            }
            Self::ArithmeticOverflow => f.write_str("capability-budget-overflow"),
            Self::Insufficient {
                required_slots,
                available_slots,
            } => write!(
                f,
                "capability-budget-insufficient required={required_slots} available={available_slots}",
            ),
        }
    }
}

#[cfg(feature = "kernel")]
fn checked_cspace_page_slots(pages: usize, caps_per_page: usize) -> Option<usize> {
    pages.checked_mul(caps_per_page)
}

#[cfg(feature = "kernel")]
fn isolated_runtime_cspace_upper_bound(
    spec: driver_task::DriverTaskRuntimeImageSpec,
    linked_code_pages: usize,
    include_framebuffer: bool,
) -> Option<usize> {
    use driver_task::DriverTaskRuntimeRegionKind as Region;

    let stack_pages = usize::from(spec.region_pages(Region::Stack));
    let mmio_pages = usize::from(spec.region_pages(Region::Mmio));
    let dma_pages = usize::from(spec.region_pages(Region::Dma));
    let shared_pages = usize::from(spec.region_pages(Region::SharedBuffer));
    let transport_pages = linked_code_pages.checked_add(stack_pages)?.checked_add(2)?;
    let mut slots =
        checked_cspace_page_slots(transport_pages, DRIVER_TASK_CSPACE_CAPS_PER_ALIASABLE_PAGE)?;
    slots = slots.checked_add(checked_cspace_page_slots(
        mmio_pages,
        DRIVER_TASK_CSPACE_CAPS_PER_ALIASABLE_PAGE,
    )?)?;
    slots = slots.checked_add(dma_pages)?;
    slots = slots.checked_add(checked_cspace_page_slots(
        shared_pages,
        DRIVER_TASK_CSPACE_CAPS_PER_ALIASABLE_PAGE,
    )?)?;
    if include_framebuffer {
        slots = slots.checked_add(checked_cspace_page_slots(
            DRIVER_TASK_CSPACE_MAX_FRAMEBUFFER_PAGES,
            DRIVER_TASK_CSPACE_CAPS_PER_ALIASABLE_PAGE,
        )?)?;
    }
    slots.checked_add(DRIVER_TASK_CSPACE_FIXED_CAPS_PER_RUNTIME)
}

#[cfg(feature = "kernel")]
fn physical_pi_driver_task_bootstrap_contracts() -> &'static [DriverTaskContract] {
    match driver_task::pi4_pre_root_net_bootstrap_selection() {
        driver_task::Pi4PreRootNetBootstrapSelection::Wired => {
            PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_WIRED_SELECTED
        }
        driver_task::Pi4PreRootNetBootstrapSelection::Wifi => {
            PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_WIFI_SELECTED
        }
        driver_task::Pi4PreRootNetBootstrapSelection::Disabled => {
            PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_BASE
        }
    }
}

#[cfg(feature = "kernel")]
fn selected_pi4_early_child_mmio_pages(
    selection: driver_task::Pi4PreRootNetBootstrapSelection,
) -> &'static [usize] {
    match selection {
        driver_task::Pi4PreRootNetBootstrapSelection::Wifi => {
            PI4_DRIVER_RUNTIME_BCM2835_DMA_MMIO_BASES
        }
        driver_task::Pi4PreRootNetBootstrapSelection::Wired
        | driver_task::Pi4PreRootNetBootstrapSelection::Disabled => &[],
    }
}

#[cfg(feature = "kernel")]
fn classify_driver_task_bootstrap_failure(err: HalError) -> DriverTaskBootstrapFailureReason {
    match err {
        HalError::Unsupported("driver-runtime-sdio-dma-mmio-pre-admission-missing") => {
            DriverTaskBootstrapFailureReason::SdioDmaMmioPreAdmissionMissing
        }
        HalError::Unsupported("driver-runtime-sdio-dma-mmio-not-covered") => {
            DriverTaskBootstrapFailureReason::SdioDmaMmioNotCovered
        }
        HalError::Unsupported("driver-runtime-sdio-owner-handle-missing") => {
            DriverTaskBootstrapFailureReason::SdioOwnerHandleMissing
        }
        _ => DriverTaskBootstrapFailureReason::Other,
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug)]
struct KernelDriverTaskHandle {
    contract: DriverTaskContract,
    role_bit: usize,
    tcb: seL4_CPtr,
    cnode: seL4_CPtr,
    command_endpoint_origin: seL4_CPtr,
    command_endpoint: seL4_CPtr,
    command_reply: seL4_CPtr,
    completion_notification_origin: seL4_CPtr,
    completion_notification: seL4_CPtr,
    sched_context: seL4_CPtr,
    standard_fault_endpoint: seL4_CPtr,
    timeout_fault_endpoint: seL4_CPtr,
    notification: seL4_CPtr,
    root_notification: seL4_CPtr,
    root_wake_notification: seL4_CPtr,
    runtime_irqs: [Option<RuntimeIrqBinding>; DRIVER_RUNTIME_IRQ_BINDING_CAPACITY],
    reciprocal_link_caps: [Option<InstalledChildCap>; 2],
    fault_slot: seL4_CPtr,
    ipc_frame: seL4_CPtr,
    stack_frame: seL4_CPtr,
    ring_frame: Option<seL4_CPtr>,
    vspace: Option<seL4_CPtr>,
    code_frame: Option<seL4_CPtr>,
    runtime_image_spec: Option<driver_task::DriverTaskRuntimeImageSpec>,
    runtime_image_declared_region_mask: u16,
    runtime_image_mapped_region_mask: u16,
    runtime_image_acceptance_eligible: bool,
    runtime_image_non_acceptance_reason: &'static str,
    code_vaddr: usize,
    ipc_vaddr: usize,
    ring_vaddr: usize,
    stack_top: usize,
    affinity_core: Option<u8>,
    vspace_isolated: bool,
    pointer_free_ipc: bool,
    started: bool,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug)]
struct DriverTaskMcsObjects {
    command_endpoint: seL4_CPtr,
    command_reply: seL4_CPtr,
    completion_notification_origin: seL4_CPtr,
    completion_notification: seL4_CPtr,
    sched_context: seL4_CPtr,
    standard_fault_endpoint: seL4_CPtr,
    timeout_fault_endpoint: seL4_CPtr,
}

#[cfg(feature = "kernel")]
impl DriverTaskMcsObjects {
    #[cfg(not(sel4_config_kernel_mcs))]
    const fn classic(command_endpoint: seL4_CPtr) -> Self {
        Self {
            command_endpoint,
            command_reply: 0,
            completion_notification_origin: 0,
            completion_notification: 0,
            sched_context: 0,
            standard_fault_endpoint: 0,
            timeout_fault_endpoint: 0,
        }
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug)]
struct RuntimeIrqBinding {
    kernel: KernelIrqBinding,
    child_cnode: seL4_CPtr,
    child_handler_slot: seL4_CPtr,
    child_depth: u8,
}

#[cfg(feature = "kernel")]
fn release_runtime_irq_binding(binding: RuntimeIrqBinding) -> Result<(), seL4_Error> {
    let root_cnode = sel4_sys::seL4_CapInitThreadCNode;
    let root_depth = sel4::word_bits() as u8;
    let mut first_error = sel4::cnode_delete(
        binding.child_cnode,
        binding.child_handler_slot,
        binding.child_depth,
    );
    for err in [
        sel4::irq_handler_clear(binding.kernel.handler_slot),
        sel4::cnode_delete(
            root_cnode,
            binding.kernel.badged_notification_slot,
            root_depth,
        ),
        sel4::cnode_delete(root_cnode, binding.kernel.handler_slot, root_depth),
    ] {
        if err != seL4_NoError && first_error == seL4_NoError {
            first_error = err;
        }
    }
    if first_error == seL4_NoError {
        Ok(())
    } else {
        Err(first_error)
    }
}

#[cfg(feature = "kernel")]
fn release_runtime_irq_bindings(
    bindings: [Option<RuntimeIrqBinding>; DRIVER_RUNTIME_IRQ_BINDING_CAPACITY],
) -> Result<(), seL4_Error> {
    let mut first_error = seL4_NoError;
    for binding in bindings.into_iter().flatten() {
        if let Err(err) = release_runtime_irq_binding(binding) {
            if first_error == seL4_NoError {
                first_error = err;
            }
        }
    }
    if first_error == seL4_NoError {
        Ok(())
    } else {
        Err(first_error)
    }
}

#[cfg(feature = "kernel")]
struct RuntimeIrqInstallGuard {
    bindings: [Option<RuntimeIrqBinding>; DRIVER_RUNTIME_IRQ_BINDING_CAPACITY],
}

#[cfg(feature = "kernel")]
const DRIVER_RUNTIME_IRQ_BINDING_CAPACITY: usize = 2;

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug)]
struct InstalledChildCap {
    cnode: seL4_CPtr,
    slot: seL4_CPtr,
    depth: u8,
}

#[cfg(feature = "kernel")]
pub(crate) struct ReciprocalLinkCapGuard {
    caps: [Option<InstalledChildCap>; 2],
}

#[cfg(feature = "kernel")]
impl ReciprocalLinkCapGuard {
    const fn empty() -> Self {
        Self { caps: [None; 2] }
    }

    fn push(&mut self, cap: InstalledChildCap) -> Result<(), HalError> {
        let Some(slot) = self.caps.iter_mut().find(|slot| slot.is_none()) else {
            return Err(HalError::Unsupported(
                "driver-runtime-reciprocal-cap-overflow",
            ));
        };
        *slot = Some(cap);
        Ok(())
    }

    fn commit(mut self) -> [Option<InstalledChildCap>; 2] {
        core::mem::replace(&mut self.caps, [None; 2])
    }
}

#[cfg(feature = "kernel")]
impl Drop for ReciprocalLinkCapGuard {
    fn drop(&mut self) {
        for cap in self.caps.iter_mut().filter_map(Option::take) {
            let _ = sel4::cnode_delete(cap.cnode, cap.slot, cap.depth);
        }
    }
}

#[cfg(feature = "kernel")]
impl RuntimeIrqInstallGuard {
    const fn empty() -> Self {
        Self {
            bindings: [None; DRIVER_RUNTIME_IRQ_BINDING_CAPACITY],
        }
    }

    fn push(&mut self, binding: RuntimeIrqBinding) -> Result<(), HalError> {
        let Some(slot) = self.bindings.iter_mut().find(|slot| slot.is_none()) else {
            let _ = release_runtime_irq_binding(binding);
            return Err(HalError::Unsupported("driver-runtime-irq-binding-overflow"));
        };
        *slot = Some(binding);
        Ok(())
    }

    fn root_handler_slots(&self) -> [usize; DRIVER_RUNTIME_IRQ_BINDING_CAPACITY] {
        self.bindings
            .map(|binding| binding.map_or(0, |binding| binding.kernel.handler_slot as usize))
    }

    fn commit(mut self) -> [Option<RuntimeIrqBinding>; DRIVER_RUNTIME_IRQ_BINDING_CAPACITY] {
        core::mem::replace(
            &mut self.bindings,
            [None; DRIVER_RUNTIME_IRQ_BINDING_CAPACITY],
        )
    }
}

#[cfg(feature = "kernel")]
impl Drop for RuntimeIrqInstallGuard {
    fn drop(&mut self) {
        let bindings = core::mem::replace(
            &mut self.bindings,
            [None; DRIVER_RUNTIME_IRQ_BINDING_CAPACITY],
        );
        let _ = release_runtime_irq_bindings(bindings);
    }
}

#[cfg(feature = "kernel")]
fn add_driver_task_handle_to_report(
    report: &mut DriverTaskBootstrapReport,
    handle: &KernelDriverTaskHandle,
) {
    report.configured_count = report.configured_count.saturating_add(1);
    if handle.started {
        report.live_tcb_count = report.live_tcb_count.saturating_add(1);
        report.live_tcb_role_mask |= handle.role_bit;
    }
    if handle.affinity_core.is_some() {
        report.affinity_configured_count = report.affinity_configured_count.saturating_add(1);
        report.affinity_applied_count = report.affinity_applied_count.saturating_add(1);
    }
    if handle.vspace_isolated {
        report.isolated_vspace_count = report.isolated_vspace_count.saturating_add(1);
    }
    if handle.pointer_free_ipc {
        report.pointer_free_ipc_count = report.pointer_free_ipc_count.saturating_add(1);
    }
    if let Some(spec) = handle.runtime_image_spec {
        report.runtime_image_declared_count = report.runtime_image_declared_count.saturating_add(1);
        report.runtime_image_declared_hot_path_mask |= spec.hot_path.owner_state_bit();
        if driver_task::driver_task_runtime_owner_state_registered(spec.hot_path) {
            report.owner_state_role_mask |= handle.role_bit;
            report.owner_state_hot_path_mask |= spec.hot_path.owner_state_bit();
        }
        if handle.runtime_image_mapped_region_mask
            & driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK
            == driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK
        {
            report.runtime_image_transport_mapped_count = report
                .runtime_image_transport_mapped_count
                .saturating_add(1);
            report.runtime_image_transport_mapped_hot_path_mask |= spec.hot_path.owner_state_bit();
        }
        if handle.runtime_image_acceptance_eligible {
            report.runtime_image_acceptance_count =
                report.runtime_image_acceptance_count.saturating_add(1);
        }
    }
}

#[cfg(feature = "kernel")]
fn runtime_image_non_acceptance_reason(
    spec: Option<driver_task::DriverTaskRuntimeImageSpec>,
) -> &'static str {
    match spec {
        Some(spec) => spec.non_acceptance_reason().unwrap_or("acceptance-ready"),
        None => "qemu-compatibility-or-non-pi-contract",
    }
}

#[cfg(feature = "kernel")]
fn runtime_image_declared_region_mask(
    spec: Option<driver_task::DriverTaskRuntimeImageSpec>,
) -> u16 {
    spec.map(driver_task::DriverTaskRuntimeImageSpec::declared_region_mask)
        .unwrap_or(0)
}

#[cfg(feature = "kernel")]
fn runtime_image_acceptance_eligible(
    spec: Option<driver_task::DriverTaskRuntimeImageSpec>,
) -> bool {
    spec.map(driver_task::DriverTaskRuntimeImageSpec::acceptance_eligible)
        .unwrap_or(false)
}

#[cfg(feature = "kernel")]
fn runtime_image_transport_pointer_free_ipc_ready(
    mapped_region_mask: u16,
    ipc_abi: driver_task::DriverTaskIpcAbi,
) -> bool {
    ipc_abi.is_pointer_free()
        && mapped_region_mask & driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK
            == driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK
}

#[cfg(feature = "kernel")]
const fn remote_tcb_ipc_buffer_frame_cap(
    _root_mapped_frame: seL4_CPtr,
    child_mapped_frame: seL4_CPtr,
) -> seL4_CPtr {
    child_mapped_frame
}

#[cfg(feature = "kernel")]
fn emit_driver_task_ipc_bind_caps(
    contract: driver_task::DriverTaskContract,
    root_mapped_frame: seL4_CPtr,
    child_mapped_frame: seL4_CPtr,
    ipc_vaddr: usize,
) {
    let mut line = heapless::String::<192>::new();
    let _ = fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "DRIVER_TASK_IPC_BIND contract={} root_frame=0x{:04x} child_frame=0x{:04x} ipc_vaddr=0x{:08x} source=child-vspace-mapped-cap",
            contract.name, root_mapped_frame, child_mapped_frame, ipc_vaddr,
        ),
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
}

#[cfg(feature = "kernel")]
fn runtime_image_stack_pages(spec: Option<driver_task::DriverTaskRuntimeImageSpec>) -> usize {
    spec.map(|spec| {
        usize::from(spec.region_pages(driver_task::DriverTaskRuntimeRegionKind::Stack)).max(1)
    })
    .unwrap_or(1)
}

#[cfg(feature = "kernel")]
fn runtime_image_stack_top(
    spec: Option<driver_task::DriverTaskRuntimeImageSpec>,
) -> Result<usize, HalError> {
    let pages = runtime_image_stack_pages(spec);
    let stack_bytes = pages
        .checked_mul(1usize << sel4::PAGE_BITS)
        .ok_or(HalError::Unsupported("driver-runtime-stack-size"))?;
    driver_task::DRIVER_TASK_STACK_BOTTOM_VADDR
        .checked_add(stack_bytes)
        .ok_or(HalError::Unsupported("driver-runtime-stack-vaddr"))
}

#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_SERIAL_MMIO_BASES: &[usize] = &[uart::PI4_MINI_UART_PADDR];
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_USB_XHCI_MMIO_BASES: &[usize] = &[0x0000_0006_0000_0000];
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_HDMI_MMIO_BASES: &[usize] = &[];
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_GENET_MMIO_BASES: &[usize] = &[0xFD58_0000, 0x7D58_0000, 0xFE58_0000];
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_SDIO_MMIO_BASES: &[usize] = &[0xFE30_0000];
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_WIFI_PWRSEQ_MMIO_BASES: &[usize] = &[0xFE00_B000];
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_BCM2835_DMA_MMIO_BASES: &[usize] = &[0xFE00_7000];
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_BCM2835_DMA_AVAILABLE_CHANNEL_MASK: u16 = 0x07f5;
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_BCM2835_DMA_CHANNEL: usize = 4;
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_BCM2835_DMA_CHANNEL_STRIDE: usize = 0x100;
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_NO_MMIO_BASES: &[usize] = &[];
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_PCIE_MMIO_BASES: &[usize] = &[0xFD50_0000, 0xFE50_0000, 0x7D50_0000];
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_PCIE_HOST_MMIO_PAGES: usize = 10;
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_PCIE_TIMER_MMIO_PAGES: usize = 1;
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_PCIE_TOTAL_MMIO_PAGES: usize =
    PI4_DRIVER_RUNTIME_PCIE_HOST_MMIO_PAGES + PI4_DRIVER_RUNTIME_PCIE_TIMER_MMIO_PAGES;
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_SYSTEM_TIMER_PADDR: usize = DRIVER_RUNTIME_PI4_SYSTEM_TIMER_PADDR as usize;

#[cfg(feature = "kernel")]
fn runtime_mmio_candidate_bases(hot_path: driver_task::DriverTaskHotPath) -> &'static [usize] {
    match hot_path {
        driver_task::DriverTaskHotPath::SerialConsole => PI4_DRIVER_RUNTIME_SERIAL_MMIO_BASES,
        driver_task::DriverTaskHotPath::UsbKeyboard => PI4_DRIVER_RUNTIME_USB_XHCI_MMIO_BASES,
        driver_task::DriverTaskHotPath::HdmiText => PI4_DRIVER_RUNTIME_HDMI_MMIO_BASES,
        driver_task::DriverTaskHotPath::GenetNic => PI4_DRIVER_RUNTIME_GENET_MMIO_BASES,
        driver_task::DriverTaskHotPath::Cyw43Wifi => PI4_DRIVER_RUNTIME_NO_MMIO_BASES,
        driver_task::DriverTaskHotPath::SdioHost => PI4_DRIVER_RUNTIME_SDIO_MMIO_BASES,
        driver_task::DriverTaskHotPath::PcieRoot => PI4_DRIVER_RUNTIME_PCIE_MMIO_BASES,
    }
}

#[cfg(feature = "kernel")]
fn runtime_candidate_covers_pages(env: &KernelEnv<'_>, base: usize, pages: usize) -> bool {
    let page_bytes = 1usize << sel4::PAGE_BITS;
    for page in 0..pages {
        let Some(offset) = page.checked_mul(page_bytes) else {
            return false;
        };
        let Some(paddr) = base.checked_add(offset) else {
            return false;
        };
        if !env.device_page_available_for_child(paddr) {
            return false;
        }
    }
    true
}

#[cfg(feature = "kernel")]
fn runtime_region_page_vaddr(
    region: driver_task::DriverTaskRuntimeRegion,
    page: usize,
) -> Option<usize> {
    let page_bytes = 1usize << sel4::PAGE_BITS;
    region.vaddr.checked_add(page.checked_mul(page_bytes)?)
}

#[cfg(feature = "kernel")]
fn runtime_region_paddr_is_contiguous(
    first_paddr: usize,
    page: usize,
    page_bytes: usize,
    paddr: usize,
) -> bool {
    first_paddr.checked_add(page.saturating_mul(page_bytes)) == Some(paddr)
}

#[cfg(feature = "kernel")]
const BCM2711_SDIO_IRQ: u32 = DRIVER_RUNTIME_SDIO_IRQ;
#[cfg(feature = "kernel")]
const BCM2711_SDIO_IRQ_BADGE: u32 = DRIVER_RUNTIME_SDIO_IRQ_BADGE;
#[cfg(feature = "kernel")]
const BCM2711_SDIO_DMA_IRQ: u32 = DRIVER_RUNTIME_SDIO_DMA_IRQ;
#[cfg(feature = "kernel")]
const BCM2711_SDIO_DMA_IRQ_BADGE: u32 = DRIVER_RUNTIME_SDIO_DMA_IRQ_BADGE;
#[cfg(feature = "kernel")]
const BCM2711_MINI_UART_IRQ: u32 = DRIVER_RUNTIME_SERIAL_IRQ;
#[cfg(feature = "kernel")]
const BCM2711_MINI_UART_IRQ_BADGE: u32 = DRIVER_RUNTIME_SERIAL_IRQ_BADGE;
#[cfg(feature = "kernel")]
const BCM2711_GENET_IRQ: u32 = DRIVER_RUNTIME_GENET_IRQ;
#[cfg(feature = "kernel")]
const BCM2711_GENET_IRQ_BADGE: u32 = DRIVER_RUNTIME_GENET_IRQ_BADGE;
#[cfg(feature = "kernel")]
const BCM2711_PCIE_TIMER_IRQ: u32 = DRIVER_RUNTIME_PCIE_TIMER_IRQ;
#[cfg(feature = "kernel")]
const BCM2711_PCIE_TIMER_IRQ_BADGE: u32 = DRIVER_RUNTIME_PCIE_TIMER_IRQ_BADGE;
#[cfg(feature = "kernel")]
const DRIVER_RUNTIME_IRQ_TRIGGER_LEVEL: u16 = 0;
#[cfg(feature = "kernel")]
const DRIVER_RUNTIME_IRQ_TRIGGER_EDGE: u16 = 1;

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug)]
struct GeneratedCyw43SdioTopology {
    irqs: [crate::generated::DriverRuntimeIrqSpec; DRIVER_RUNTIME_IRQ_BINDING_CAPACITY],
    link: crate::generated::DriverRuntimeBusLinkSpec,
}

#[cfg(feature = "kernel")]
fn generated_cyw43_sdio_topology() -> Result<GeneratedCyw43SdioTopology, HalError> {
    let policy = crate::generated::driver_runtime_image_policy();
    if !policy.required || policy.bus_links.len() != 1 {
        return Err(HalError::Unsupported(
            "driver-runtime-cyw43-sdio-topology-count",
        ));
    }
    let mut sdio_irqs = policy
        .irqs
        .iter()
        .copied()
        .filter(|irq| irq.hot_path == driver_task::DriverTaskHotPath::SdioHost.as_str());
    let Some(sdhci_irq) = sdio_irqs.next() else {
        return Err(HalError::Unsupported(
            "driver-runtime-cyw43-sdio-topology-missing",
        ));
    };
    let Some(dma_irq) = sdio_irqs.next() else {
        return Err(HalError::Unsupported(
            "driver-runtime-cyw43-sdio-dma-topology-missing",
        ));
    };
    if sdio_irqs.next().is_some() {
        return Err(HalError::Unsupported(
            "driver-runtime-cyw43-sdio-topology-duplicate",
        ));
    }
    let link = policy.bus_links[0];
    let sdhci_irq_valid = sdhci_irq.irq == BCM2711_SDIO_IRQ
        && sdhci_irq.badge == BCM2711_SDIO_IRQ_BADGE
        && u32::from(sdhci_irq.handler_slot)
            == pi4_driver_abi::DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT
        && u32::from(sdhci_irq.notification_slot) == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
        && matches!(
            sdhci_irq.trigger,
            crate::generated::DriverRuntimeIrqTrigger::Level
        );
    let dma_irq_valid = dma_irq.irq == BCM2711_SDIO_DMA_IRQ
        && dma_irq.badge == BCM2711_SDIO_DMA_IRQ_BADGE
        && u32::from(dma_irq.handler_slot) == DRIVER_TASK_CHILD_SDIO_DMA_IRQ_HANDLER_SLOT
        && u32::from(dma_irq.notification_slot) == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
        && matches!(
            dma_irq.trigger,
            crate::generated::DriverRuntimeIrqTrigger::Level
        )
        && sdhci_irq.handler_slot != dma_irq.handler_slot
        && sdhci_irq.badge & dma_irq.badge == 0;
    let link_valid = link.channel == "cyw43-sdio"
        && link.client_hot_path == driver_task::DriverTaskHotPath::Cyw43Wifi.as_str()
        && link.owner_hot_path == driver_task::DriverTaskHotPath::SdioHost.as_str()
        && u32::from(link.client_notification_slot) == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
        && u32::from(link.owner_notification_slot) == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
        && u32::from(link.client_to_owner_slot) == DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_SLOT
        && u32::from(link.owner_to_client_slot) == DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_SLOT
        && link.shared_offset == DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32
        && link.shared_len == driver_task::DRIVER_TASK_SDIO_BUS_SHARED_DATA_BYTES as u32
        && link.link_epoch != 0
        && link.event_offset == DRIVER_RUNTIME_DPC_EVENT_RING_OFFSET
        && link.event_len == DRIVER_RUNTIME_DPC_EVENT_RING_BYTES
        && usize::from(link.event_depth) == DRIVER_RUNTIME_DPC_EVENT_RING_DEPTH;
    if !sdhci_irq_valid || !dma_irq_valid || !link_valid {
        return Err(HalError::Unsupported(
            "driver-runtime-cyw43-sdio-topology-invalid",
        ));
    }
    Ok(GeneratedCyw43SdioTopology {
        irqs: [sdhci_irq, dma_irq],
        link,
    })
}

#[cfg(feature = "kernel")]
fn generated_serial_runtime_irq() -> Result<crate::generated::DriverRuntimeIrqSpec, HalError> {
    let policy = crate::generated::driver_runtime_image_policy();
    if !policy.required {
        return Err(HalError::Unsupported(
            "driver-runtime-generated-irq-topology-count",
        ));
    }
    let mut matches = policy
        .irqs
        .iter()
        .copied()
        .filter(|irq| irq.hot_path == driver_task::DriverTaskHotPath::SerialConsole.as_str());
    let irq = matches.next().ok_or(HalError::Unsupported(
        "driver-runtime-generated-irq-missing",
    ))?;
    if matches.next().is_some() {
        return Err(HalError::Unsupported(
            "driver-runtime-generated-irq-duplicate",
        ));
    }
    let valid = irq.irq == BCM2711_MINI_UART_IRQ
        && irq.badge == BCM2711_MINI_UART_IRQ_BADGE
        && u32::from(irq.handler_slot) == pi4_driver_abi::DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT
        && u32::from(irq.notification_slot) == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
        && matches!(
            irq.trigger,
            crate::generated::DriverRuntimeIrqTrigger::Level
        );
    if !valid {
        return Err(HalError::Unsupported(
            "driver-runtime-generated-irq-invalid",
        ));
    }
    Ok(irq)
}

#[cfg(feature = "kernel")]
fn generated_genet_runtime_irq() -> Result<crate::generated::DriverRuntimeIrqSpec, HalError> {
    let policy = crate::generated::driver_runtime_image_policy();
    if !policy.required {
        return Err(HalError::Unsupported(
            "driver-runtime-generated-genet-irq-topology-count",
        ));
    }
    let mut matches = policy
        .irqs
        .iter()
        .copied()
        .filter(|irq| irq.hot_path == driver_task::DriverTaskHotPath::GenetNic.as_str());
    let irq = matches.next().ok_or(HalError::Unsupported(
        "driver-runtime-generated-genet-irq-missing",
    ))?;
    if matches.next().is_some() {
        return Err(HalError::Unsupported(
            "driver-runtime-generated-genet-irq-duplicate",
        ));
    }
    let valid = irq.irq == BCM2711_GENET_IRQ
        && irq.badge == BCM2711_GENET_IRQ_BADGE
        && u32::from(irq.handler_slot) == pi4_driver_abi::DRIVER_TASK_CHILD_IRQ_HANDLER_BASE_SLOT
        && u32::from(irq.notification_slot) == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
        && matches!(
            irq.trigger,
            crate::generated::DriverRuntimeIrqTrigger::Level
        );
    if !valid {
        return Err(HalError::Unsupported(
            "driver-runtime-generated-genet-irq-invalid",
        ));
    }
    Ok(irq)
}

#[cfg(feature = "kernel")]
fn generated_pcie_timer_runtime_irq_valid(irq: crate::generated::DriverRuntimeIrqSpec) -> bool {
    irq.irq == BCM2711_PCIE_TIMER_IRQ
        && irq.badge == BCM2711_PCIE_TIMER_IRQ_BADGE
        && u32::from(irq.handler_slot) == DRIVER_TASK_CHILD_PCIE_TIMER_IRQ_HANDLER_SLOT
        && u32::from(irq.notification_slot) == DRIVER_RUNTIME_LOCAL_NOTIFICATION_SLOT
        && matches!(
            irq.trigger,
            crate::generated::DriverRuntimeIrqTrigger::Level
        )
}

#[cfg(feature = "kernel")]
fn generated_pcie_timer_runtime_irq() -> Result<crate::generated::DriverRuntimeIrqSpec, HalError> {
    let policy = crate::generated::driver_runtime_image_policy();
    if !policy.required {
        return Err(HalError::Unsupported(
            "driver-runtime-generated-pcie-timer-irq-topology-count",
        ));
    }
    let mut matches = policy
        .irqs
        .iter()
        .copied()
        .filter(|irq| irq.hot_path == driver_task::DriverTaskHotPath::PcieRoot.as_str());
    let irq = matches.next().ok_or(HalError::Unsupported(
        "driver-runtime-generated-pcie-timer-irq-missing",
    ))?;
    if matches.next().is_some() {
        return Err(HalError::Unsupported(
            "driver-runtime-generated-pcie-timer-irq-duplicate",
        ));
    }
    if !generated_pcie_timer_runtime_irq_valid(irq) {
        return Err(HalError::Unsupported(
            "driver-runtime-generated-pcie-timer-irq-invalid",
        ));
    }
    Ok(irq)
}

#[cfg(feature = "kernel")]
const fn cyw43_sdio_peer_notification_badge(send_slot: u32) -> Option<u32> {
    match send_slot {
        DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_SLOT => {
            Some(DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE)
        }
        DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_SLOT => {
            Some(DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE)
        }
        _ => None,
    }
}

#[cfg(feature = "kernel")]
const CYW43_SDIO_BUS_LINK_DIAGNOSTIC_CAPACITY: usize = 384;

#[cfg(feature = "kernel")]
fn format_cyw43_sdio_bus_link_diagnostic(
    contract: DriverTaskContract,
    topology: GeneratedCyw43SdioTopology,
    client_to_owner_badge: u32,
    owner_to_client_badge: u32,
) -> Result<heapless::String<CYW43_SDIO_BUS_LINK_DIAGNOSTIC_CAPACITY>, HalError> {
    let mut line = heapless::String::new();
    fmt::write(
        &mut line,
        format_args!(
            "DRIVER_TASK_BUS_LINK contract={} owner={} channel={} client_doorbell=notification client_send_slot=0x{:04x} client_badge={} owner_doorbell=notification owner_send_slot=0x{:04x} owner_badge={} rights=send-only ring_vaddr=0x{:08x} data_vaddr=0x{:08x} shared_len={} link_epoch={}",
            contract.name,
            SDIO_HOST_DRIVER_TASK_CONTRACT.name,
            topology.link.channel,
            topology.link.client_to_owner_slot,
            client_to_owner_badge,
            topology.link.owner_to_client_slot,
            owner_to_client_badge,
            driver_task::DRIVER_TASK_SDIO_BUS_RING_VADDR,
            driver_task::DRIVER_TASK_SDIO_BUS_RING_VADDR
                + driver_task::DRIVER_TASK_RING_PAGE_BYTES,
            topology.link.shared_len,
            topology.link.link_epoch,
        ),
    )
    .map_err(|_| HalError::Unsupported("driver-runtime-cyw43-sdio-bus-link-log-overflow"))?;
    Ok(line)
}

#[cfg(feature = "kernel")]
fn generated_irq_trigger_word(trigger: crate::generated::DriverRuntimeIrqTrigger) -> u16 {
    match trigger {
        crate::generated::DriverRuntimeIrqTrigger::Level => DRIVER_RUNTIME_IRQ_TRIGGER_LEVEL,
        crate::generated::DriverRuntimeIrqTrigger::Edge => DRIVER_RUNTIME_IRQ_TRIGGER_EDGE,
    }
}

#[cfg(feature = "kernel")]
fn generated_irq_trigger(trigger: crate::generated::DriverRuntimeIrqTrigger) -> IrqTrigger {
    match trigger {
        crate::generated::DriverRuntimeIrqTrigger::Level => IrqTrigger::Level,
        crate::generated::DriverRuntimeIrqTrigger::Edge => IrqTrigger::Edge,
    }
}

#[cfg(feature = "kernel")]
fn generated_cyw43_sdio_bus_link_descriptor(
    hot_path: driver_task::DriverTaskHotPath,
    topology: GeneratedCyw43SdioTopology,
) -> Result<DriverRuntimeBusLinkDescriptor, HalError> {
    let (flags, peer_hot_path, local_notification_slot, peer_notification_slot) = match hot_path {
        driver_task::DriverTaskHotPath::Cyw43Wifi => (
            DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
            driver_task::DriverTaskHotPath::SdioHost.as_u32(),
            topology.link.client_notification_slot,
            topology.link.client_to_owner_slot,
        ),
        driver_task::DriverTaskHotPath::SdioHost => (
            DRIVER_RUNTIME_BUS_LINK_FLAG_OWNER | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
            driver_task::DriverTaskHotPath::Cyw43Wifi.as_u32(),
            topology.link.owner_notification_slot,
            topology.link.owner_to_client_slot,
        ),
        _ => {
            return Err(HalError::Unsupported(
                "driver-runtime-cyw43-sdio-topology-role",
            ));
        }
    };
    let descriptor = DriverRuntimeBusLinkDescriptor::new(
        driver_task::DriverTaskHotPath::SdioHost.as_u32(),
        DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
        topology.link.shared_offset,
        topology.link.shared_len,
        flags,
    )
    .with_notification_dpc(
        peer_hot_path,
        u32::from(local_notification_slot),
        u32::from(peer_notification_slot),
        topology.link.link_epoch,
    );
    if descriptor.event_offset != topology.link.event_offset
        || descriptor.event_len != topology.link.event_len
        || descriptor.event_depth != topology.link.event_depth
    {
        return Err(HalError::Unsupported(
            "driver-runtime-cyw43-sdio-event-geometry",
        ));
    }
    if !descriptor.valid() {
        return Err(HalError::Unsupported(
            "driver-runtime-cyw43-sdio-descriptor-invalid",
        ));
    }
    Ok(descriptor)
}

#[cfg(feature = "kernel")]
const PI4_VL805_DMA_BUS_ALIAS_OR: u64 = 0x0000_0004_0000_0000;
#[cfg(feature = "kernel")]
const PI4_VL805_DMA_BUS_ALIAS_AND: u64 = 0x0000_0000_ffff_ffff;
#[cfg(feature = "kernel")]
const PI4_SDIO_DMA_BUS_ALIAS_OR: u64 = 0x0000_0000_c000_0000;
#[cfg(feature = "kernel")]
const PI4_SDIO_DMA_BUS_ALIAS_AND: u64 = 0x0000_0000_3fff_ffff;
#[cfg(feature = "kernel")]
const PI4_SDIO_DMA_PRIVATE_PAGE_COUNT: usize = 2 + DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_PAGES;

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeInitDescriptorBuilder {
    descriptor: DriverRuntimeInitDescriptor,
    expected_mmio_pages: u16,
    expected_dma_pages: u16,
    expected_shared_pages: u16,
    task_key: u32,
    artifact_hash: u32,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeInitSchedulerConfig {
    scheduling_context_slot: u32,
    scheduling_context_bits: u8,
    sched_control_core: u8,
    max_refills: u8,
    affinity_core: u8,
    budget_us: u32,
    period_us: u32,
    standard_fault_badge: u64,
    timeout_fault_badge: u64,
}

#[cfg(feature = "kernel")]
impl RuntimeInitDescriptorBuilder {
    fn new(
        spec: driver_task::DriverTaskRuntimeImageSpec,
        role_bit: usize,
        task_key: usize,
        artifact_hash: u32,
    ) -> Result<Self, HalError> {
        let temporal = driver_task::driver_task_temporal_config(spec.hot_path)
            .ok_or(HalError::Unsupported("driver-runtime-temporal-config"))?;
        let standard_fault_badge = crate::critical_tcb::generated_standard_fault_badge(temporal.id)
            .ok_or(HalError::Unsupported(
                "driver-runtime-temporal-standard-fault-badge",
            ))?;
        Self::new_with_scheduler(
            spec,
            role_bit,
            task_key,
            artifact_hash,
            RuntimeInitSchedulerConfig {
                scheduling_context_slot: temporal.scheduling_context_slot,
                scheduling_context_bits: temporal.scheduling_context_bits,
                sched_control_core: temporal.sched_control_core,
                max_refills: temporal.max_refills,
                affinity_core: temporal.core,
                budget_us: temporal.budget_us,
                period_us: temporal.period_us,
                standard_fault_badge,
                timeout_fault_badge: temporal.timeout_badge,
            },
        )
    }

    fn new_with_scheduler(
        spec: driver_task::DriverTaskRuntimeImageSpec,
        role_bit: usize,
        task_key: usize,
        artifact_hash: u32,
        scheduler: RuntimeInitSchedulerConfig,
    ) -> Result<Self, HalError> {
        if scheduler.affinity_core != scheduler.sched_control_core
            || scheduler.budget_us > scheduler.period_us
        {
            return Err(HalError::Unsupported(
                "driver-runtime-temporal-config-mismatch",
            ));
        }
        let mut descriptor = DriverRuntimeInitDescriptor::empty().with_mcs_scheduler(
            task_key as u32,
            scheduler.scheduling_context_slot,
            scheduler.scheduling_context_bits,
            scheduler.sched_control_core,
            scheduler.max_refills,
            scheduler.affinity_core,
            scheduler.budget_us,
            scheduler.period_us,
            scheduler.standard_fault_badge,
            scheduler.timeout_fault_badge,
        );
        descriptor.hot_path = spec.hot_path.as_u32();
        descriptor.role_bit = role_bit as u32;
        descriptor.flags = DRIVER_RUNTIME_INIT_FLAG_POINTER_FREE
            | DRIVER_RUNTIME_INIT_FLAG_BUS_ADDRESSING
            | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY
            | DRIVER_RUNTIME_INIT_FLAG_ROOT_CONTEXT_FORBIDDEN;
        descriptor.mmio_vaddr_base = driver_task::DRIVER_TASK_DEVICE_MMIO_VADDR as u64;
        descriptor.dma_vaddr_base = driver_task::DRIVER_TASK_DMA_BUFFER_VADDR as u64;
        descriptor.shared_vaddr_base = driver_task::DRIVER_TASK_SHARED_BUFFER_VADDR as u64;
        if let Some((slot, badge)) = driver_runtime_root_wake_route(spec)? {
            descriptor.root_wake_notification_slot = u32::from(slot);
            descriptor.root_wake_notification_badge = badge;
        }
        match spec.hot_path {
            driver_task::DriverTaskHotPath::UsbKeyboard => {
                descriptor.bus_alias_or = PI4_VL805_DMA_BUS_ALIAS_OR;
                descriptor.bus_alias_and = PI4_VL805_DMA_BUS_ALIAS_AND;
                descriptor.bus_link_count = 1;
                descriptor.bus_links[0] = DriverRuntimeBusLinkDescriptor::new(
                    driver_task::DriverTaskHotPath::PcieRoot.as_u32(),
                    DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE,
                    0,
                    DRIVER_RUNTIME_RESOURCE_PAGE_BYTES as u32,
                    DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
                );
                descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS;
            }
            driver_task::DriverTaskHotPath::Cyw43Wifi => {
                let topology = generated_cyw43_sdio_topology()?;
                descriptor.bus_link_count = 1;
                descriptor.bus_links[0] =
                    generated_cyw43_sdio_bus_link_descriptor(spec.hot_path, topology)?;
                descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS;
            }
            driver_task::DriverTaskHotPath::SdioHost => {
                let topology = generated_cyw43_sdio_topology()?;
                descriptor.bus_alias_or = PI4_SDIO_DMA_BUS_ALIAS_OR;
                descriptor.bus_alias_and = PI4_SDIO_DMA_BUS_ALIAS_AND;
                descriptor.bus_link_count = 1;
                descriptor.bus_links[0] =
                    generated_cyw43_sdio_bus_link_descriptor(spec.hot_path, topology)?;
                descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS;
            }
            driver_task::DriverTaskHotPath::GenetNic => {
                descriptor = descriptor.with_direct_genet();
            }
            _ => {}
        }
        Ok(Self {
            descriptor,
            expected_mmio_pages: spec.region_pages(driver_task::DriverTaskRuntimeRegionKind::Mmio),
            expected_dma_pages: spec.region_pages(driver_task::DriverTaskRuntimeRegionKind::Dma),
            expected_shared_pages: spec
                .region_pages(driver_task::DriverTaskRuntimeRegionKind::SharedBuffer),
            task_key: task_key as u32,
            artifact_hash,
        })
    }

    fn add_irq(&mut self, irq: crate::generated::DriverRuntimeIrqSpec) -> Result<(), HalError> {
        let index = usize::from(self.descriptor.irq_count);
        let Some(slot) = self.descriptor.irqs.get_mut(index) else {
            return Err(HalError::Unsupported("driver-runtime-init-irq-overflow"));
        };
        *slot = DriverRuntimeIrqDescriptor {
            irq: irq.irq,
            badge: irq.badge,
            handler_slot: u32::from(irq.handler_slot),
            notification_slot: u32::from(irq.notification_slot),
            trigger: generated_irq_trigger_word(irq.trigger),
            flags: 0,
            reserved: 0,
        };
        self.descriptor.irq_count = self.descriptor.irq_count.saturating_add(1);
        self.descriptor.flags &= !DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY;
        self.descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_IRQS_BOUND;
        Ok(())
    }

    fn add_mmio_page(&mut self, paddr: usize) -> Result<(), HalError> {
        let index = usize::from(self.descriptor.mmio_page_count);
        if let Some(slot) = self.descriptor.mmio_pages.get_mut(index) {
            *slot = DriverRuntimePageDescriptor::new(paddr);
            self.descriptor.mmio_page_count = self.descriptor.mmio_page_count.saturating_add(1);
        }
        self.descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_MMIO_MAPPED;
        Ok(())
    }

    fn add_dma_page(&mut self, paddr: usize) -> Result<(), HalError> {
        let index = usize::from(self.descriptor.dma_page_count);
        if let Some(slot) = self.descriptor.dma_pages.get_mut(index) {
            *slot = DriverRuntimePageDescriptor::new(paddr);
            self.descriptor.dma_page_count = self.descriptor.dma_page_count.saturating_add(1);
        }
        self.descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_DMA_PADDRS;
        Ok(())
    }

    fn add_shared_page(&mut self, paddr: usize) -> Result<(), HalError> {
        let index = usize::from(self.descriptor.shared_page_count);
        if let Some(slot) = self.descriptor.shared_pages.get_mut(index) {
            let descriptor_paddr =
                if self.descriptor.flags & DRIVER_RUNTIME_INIT_FLAG_DIRECT_GENET != 0 {
                    0
                } else {
                    paddr
                };
            *slot = DriverRuntimePageDescriptor::new(descriptor_paddr);
            self.descriptor.shared_page_count = self.descriptor.shared_page_count.saturating_add(1);
        }
        self.descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_SHARED_PADDRS;
        Ok(())
    }

    fn set_framebuffer(&mut self, framebuffer: pi4_driver_abi::DriverRuntimeFramebufferDescriptor) {
        self.descriptor.framebuffer = framebuffer;
        self.descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_FRAMEBUFFER;
    }

    fn set_framebuffer_region(
        &mut self,
        framebuffer: pi4_driver_abi::DriverRuntimeFramebufferDescriptor,
        page_base: usize,
        bytes: usize,
        pages: usize,
    ) -> Result<(), HalError> {
        self.set_framebuffer(framebuffer);
        self.add_resource_range(DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_FRAMEBUFFER,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE
                | DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED,
            DRIVER_RUNTIME_RESOURCE_TAG_HDMI_FRAMEBUFFER,
            framebuffer.vaddr,
            page_base as u64,
            bytes as u64,
            u16::try_from(pages)
                .map_err(|_| HalError::Unsupported("driver-runtime-init-fb-range-pages"))?,
            0,
        ))
    }

    fn add_mmio_resource_range(
        &mut self,
        hot_path: driver_task::DriverTaskHotPath,
        vaddr: usize,
        paddr: usize,
        pages: usize,
        first_page_index: u16,
    ) -> Result<(), HalError> {
        self.add_tagged_mmio_resource_range(
            runtime_mmio_resource_tag(hot_path),
            vaddr,
            paddr,
            pages,
            first_page_index,
        )
    }

    fn add_tagged_mmio_resource_range(
        &mut self,
        tag: u32,
        vaddr: usize,
        paddr: usize,
        pages: usize,
        first_page_index: u16,
    ) -> Result<(), HalError> {
        let page_count = u16::try_from(pages)
            .map_err(|_| HalError::Unsupported("driver-runtime-init-mmio-range-pages"))?;
        self.add_resource_range(DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
            tag,
            vaddr as u64,
            paddr as u64,
            (pages as u64).saturating_mul(DRIVER_RUNTIME_RESOURCE_PAGE_BYTES),
            page_count,
            first_page_index,
        ))
    }

    fn add_buffer_resource_range(
        &mut self,
        hot_path: driver_task::DriverTaskHotPath,
        kind: u16,
        vaddr: usize,
        first_paddr: usize,
        pages: usize,
        first_page_index: u16,
        paddr_contiguous: bool,
    ) -> Result<(), HalError> {
        if kind == DRIVER_RUNTIME_RESOURCE_KIND_DMA {
            let tag = if hot_path == driver_task::DriverTaskHotPath::SdioHost {
                DRIVER_RUNTIME_RESOURCE_TAG_WIFI_PWRSEQ_REQUEST
            } else {
                DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA
            };
            return self.add_tagged_dma_resource_range(
                tag,
                vaddr,
                first_paddr,
                pages,
                first_page_index,
                paddr_contiguous,
            );
        }
        let page_count = u16::try_from(pages)
            .map_err(|_| HalError::Unsupported("driver-runtime-init-buffer-range-pages"))?;
        let (tag, flags) = match kind {
            DRIVER_RUNTIME_RESOURCE_KIND_SHARED
                if hot_path == driver_task::DriverTaskHotPath::GenetNic =>
            {
                if pages
                    != usize::from(pi4_driver_abi::DRIVER_RUNTIME_DIRECT_GENET_SHARED_PAGE_COUNT)
                    || first_page_index != 0
                    || vaddr != driver_task::DRIVER_TASK_SHARED_BUFFER_VADDR
                {
                    return Err(HalError::Unsupported(
                        "driver-runtime-init-direct-genet-range",
                    ));
                }
                (
                    DRIVER_RUNTIME_RESOURCE_TAG_GENET_DIRECT_LINK,
                    DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                        | DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED
                        | DRIVER_RUNTIME_RESOURCE_FLAG_CPU_ONLY,
                )
            }
            DRIVER_RUNTIME_RESOURCE_KIND_SHARED => (
                if hot_path == driver_task::DriverTaskHotPath::Cyw43Wifi {
                    DRIVER_RUNTIME_RESOURCE_TAG_CYW43_CONTROL
                } else {
                    DRIVER_RUNTIME_RESOURCE_TAG_SHARED_CONTROL
                },
                DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                    | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE
                    | DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED,
            ),
            _ => return Err(HalError::Unsupported("driver-runtime-init-buffer-kind")),
        };
        let range_paddr = if hot_path == driver_task::DriverTaskHotPath::GenetNic {
            0
        } else {
            first_paddr as u64
        };
        self.add_resource_range(DriverRuntimeResourceRangeDescriptor::new(
            kind,
            flags,
            tag,
            vaddr as u64,
            range_paddr,
            (pages as u64).saturating_mul(DRIVER_RUNTIME_RESOURCE_PAGE_BYTES),
            page_count,
            first_page_index,
        ))
    }

    fn add_tagged_dma_resource_range(
        &mut self,
        tag: u32,
        vaddr: usize,
        first_paddr: usize,
        pages: usize,
        first_page_index: u16,
        paddr_contiguous: bool,
    ) -> Result<(), HalError> {
        let page_count = u16::try_from(pages)
            .map_err(|_| HalError::Unsupported("driver-runtime-init-dma-range-pages"))?;
        let mut flags = DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
            | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE;
        if paddr_contiguous {
            flags |= DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS;
        }
        self.add_resource_range(DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_DMA,
            flags,
            tag,
            vaddr as u64,
            first_paddr as u64,
            (pages as u64).saturating_mul(DRIVER_RUNTIME_RESOURCE_PAGE_BYTES),
            page_count,
            first_page_index,
        ))
    }

    fn add_resource_range(
        &mut self,
        range: DriverRuntimeResourceRangeDescriptor,
    ) -> Result<(), HalError> {
        let index = usize::from(self.descriptor.resource_range_count);
        let Some(slot) = self.descriptor.resource_ranges.get_mut(index) else {
            return Err(HalError::Unsupported(
                "driver-runtime-init-resource-overflow",
            ));
        };
        *slot = range;
        self.descriptor.resource_range_count =
            self.descriptor.resource_range_count.saturating_add(1);
        Ok(())
    }

    fn finish(self) -> Result<DriverRuntimeInitDescriptor, HalError> {
        let descriptor = self
            .descriptor
            .with_sealed_identity(self.task_key, self.artifact_hash);
        if descriptor.valid_for_resources(
            descriptor.hot_path,
            descriptor.role_bit,
            self.expected_mmio_pages,
            self.expected_dma_pages,
            self.expected_shared_pages,
        ) && descriptor.sealed_identity_valid_for_task(self.task_key)
            && runtime_init_descriptor_expected_bus_links(&descriptor, self.task_key)
        {
            Ok(descriptor)
        } else {
            Err(HalError::Unsupported("driver-runtime-init-invalid"))
        }
    }
}

#[cfg(feature = "kernel")]
fn runtime_init_descriptor_expected_bus_links(
    descriptor: &DriverRuntimeInitDescriptor,
    task_key: u32,
) -> bool {
    match driver_task::DriverTaskHotPath::from_u32(descriptor.hot_path) {
        Some(driver_task::DriverTaskHotPath::UsbKeyboard) => {
            descriptor.bus_link_count == 1
                && descriptor.has_sealed_pointer_free_bus_link(
                    task_key,
                    driver_task::DriverTaskHotPath::PcieRoot.as_u32(),
                    DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE,
                )
        }
        Some(driver_task::DriverTaskHotPath::Cyw43Wifi) => {
            let topology = match generated_cyw43_sdio_topology() {
                Ok(topology) => topology,
                Err(_) => return false,
            };
            descriptor.bus_link_count == 1
                && descriptor.has_sealed_pointer_free_bus_link(
                    task_key,
                    driver_task::DriverTaskHotPath::SdioHost.as_u32(),
                    DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
                )
                && descriptor.bus_links[0].notification_dpc_valid()
                && descriptor.bus_links[0].flags
                    == (DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT
                        | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE
                        | DRIVER_RUNTIME_BUS_LINK_FLAG_NOTIFICATIONS
                        | DRIVER_RUNTIME_BUS_LINK_FLAG_DPC_EVENT_RING)
                && descriptor.bus_links[0].shared_epoch == topology.link.link_epoch
        }
        Some(driver_task::DriverTaskHotPath::SdioHost) => {
            let topology = match generated_cyw43_sdio_topology() {
                Ok(topology) => topology,
                Err(_) => return false,
            };
            descriptor.bus_link_count == 1
                && descriptor.has_sealed_pointer_free_bus_link(
                    task_key,
                    driver_task::DriverTaskHotPath::SdioHost.as_u32(),
                    DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
                )
                && descriptor.bus_links[0].notification_dpc_valid()
                && descriptor.bus_links[0].flags
                    == (DRIVER_RUNTIME_BUS_LINK_FLAG_OWNER
                        | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE
                        | DRIVER_RUNTIME_BUS_LINK_FLAG_NOTIFICATIONS
                        | DRIVER_RUNTIME_BUS_LINK_FLAG_DPC_EVENT_RING)
                && descriptor.bus_links[0].shared_epoch == topology.link.link_epoch
        }
        Some(_) => descriptor.bus_link_count == 0,
        None => false,
    }
}

#[cfg(feature = "kernel")]
fn bootstrap_linked_runtime_engine_for_early_console(
    contract: DriverTaskContract,
    spec: driver_task::DriverTaskRuntimeImageSpec,
) -> Result<bool, HalError> {
    if spec.hot_path != driver_task::DriverTaskHotPath::HdmiText
        || !driver_task::physical_pi_driver_task_only_owner_state_active()
    {
        return Ok(false);
    }

    driver_task::emit_driver_task_resource_init_status(
        contract,
        driver_task::DriverTaskHotPath::HdmiText,
        "hdmi-engine-init",
        "ready",
        None,
    );
    let owner_state = driver_task::register_driver_task_runtime_owner_state(
        driver_task::DriverTaskHotPath::HdmiText,
    );
    driver_task::emit_driver_task_endpoint_lifetime_checkpoint(contract, "early-draw");
    let mut line = heapless::String::<192>::new();
    let _ = fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "DRIVER_TASK_HDMI_EARLY_READY contract=hdmi-text engine_init=yes owner_state={} banner=child-owned-bounded action=steady-local-seat-retained-frame",
            if owner_state { "yes" } else { "no" },
        ),
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
    Ok(owner_state)
}

#[cfg(feature = "kernel")]
fn runtime_mmio_resource_tag(hot_path: driver_task::DriverTaskHotPath) -> u32 {
    match hot_path {
        driver_task::DriverTaskHotPath::SerialConsole => {
            DRIVER_RUNTIME_RESOURCE_TAG_SERIAL_MINI_UART
        }
        driver_task::DriverTaskHotPath::UsbKeyboard => DRIVER_RUNTIME_RESOURCE_TAG_USB_XHCI,
        driver_task::DriverTaskHotPath::HdmiText => DRIVER_RUNTIME_RESOURCE_TAG_HDMI_REGS,
        driver_task::DriverTaskHotPath::GenetNic => DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS,
        driver_task::DriverTaskHotPath::Cyw43Wifi => DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
        driver_task::DriverTaskHotPath::SdioHost => DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
        driver_task::DriverTaskHotPath::PcieRoot => DRIVER_RUNTIME_RESOURCE_TAG_PCIE_HOST,
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeElfLoad {
    entry: usize,
    code_vaddr: usize,
    root_write_aliases_unmapped: bool,
}

#[cfg(feature = "kernel")]
fn validate_runtime_load_for_resume(load: RuntimeElfLoad) -> Result<(), HalError> {
    if !load.root_write_aliases_unmapped {
        return Err(HalError::Unsupported(
            "driver-runtime-executable-root-alias",
        ));
    }
    Ok(())
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeElfSegment {
    offset: usize,
    vaddr: usize,
    filesz: usize,
    memsz: usize,
    flags: u32,
}

#[cfg(feature = "kernel")]
impl RuntimeElfSegment {
    const fn empty() -> Self {
        Self {
            offset: 0,
            vaddr: 0,
            filesz: 0,
            memsz: 0,
            flags: 0,
        }
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeElfLoadPlan {
    entry: usize,
    base_vaddr: usize,
    page_count: usize,
    segment_count: usize,
    segments: [RuntimeElfSegment; MAX_RUNTIME_ELF_LOAD_SEGMENTS],
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RuntimeElfPageFill {
    writable: bool,
    executable: bool,
}

#[cfg(feature = "kernel")]
struct RuntimeElfPageMapping {
    rights: sel4_sys::seL4_CapRights,
    attributes: sel4_sys::seL4_ARM_VMAttributes,
}

#[cfg(feature = "kernel")]
const fn runtime_cacheable_xn_attributes() -> sel4_sys::seL4_ARM_VMAttributes {
    sel4::vm_attributes_with_execute_never(sel4_sys::seL4_ARM_Page_Default)
}

#[cfg(feature = "kernel")]
const fn runtime_uncached_xn_attributes() -> sel4_sys::seL4_ARM_VMAttributes {
    sel4::vm_attributes_with_execute_never(sel4_sys::seL4_ARM_Page_Uncached)
}

#[cfg(feature = "kernel")]
const fn driver_runtime_local_notification_receive_rights() -> sel4_sys::seL4_CapRights {
    // The child may poll/receive its TCB-bound notification, but only root and
    // separately minted badged peer/IRQ caps may signal it. With write removed,
    // the child cannot manufacture an unbadged wake or any service badge.
    sel4_sys::seL4_CapRights::new(0, 0, 1, 0)
}

#[cfg(feature = "kernel")]
const fn driver_runtime_root_notification_send_rights() -> sel4_sys::seL4_CapRights {
    // Root may only signal this badged scheduling cap. It cannot receive from
    // the child-bound object or manufacture an unbadged service wake.
    sel4_sys::seL4_CapRights::new(0, 0, 0, 1)
}

#[cfg(feature = "kernel")]
const fn driver_runtime_child_root_wake_send_rights() -> sel4_sys::seL4_CapRights {
    // A runtime may only signal a compiler-declared, badged root wake cap. It
    // cannot receive from either root-owned object or mint another authority.
    sel4_sys::seL4_CapRights::new(0, 0, 0, 1)
}

#[cfg(feature = "kernel")]
fn driver_runtime_root_wake_route(
    spec: driver_task::DriverTaskRuntimeImageSpec,
) -> Result<Option<(u8, u32)>, HalError> {
    let slot = spec.root_wake_notification_slot;
    let badge = spec.root_wake_notification_badge;
    if spec.hot_path == driver_task::DriverTaskHotPath::Cyw43Wifi {
        if u32::from(slot) != pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT
            || badge != pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE
        {
            return Err(HalError::Unsupported(
                "driver-runtime-cyw43-root-wake-route",
            ));
        }
        return Ok(Some((slot, badge)));
    }
    if slot != 0 || badge != 0 {
        return Err(HalError::Unsupported(
            "driver-runtime-unexpected-root-wake-route",
        ));
    }
    Ok(None)
}

#[cfg(feature = "kernel")]
fn driver_runtime_needs_root_notification(contract: DriverTaskContract) -> bool {
    contract == CYW43_WIFI_DRIVER_TASK_CONTRACT
        || (driver_task::physical_pi_driver_task_only_owner_state_active()
            && contract == driver_task::SERIAL_DRIVER_TASK_CONTRACT)
}

#[cfg(feature = "kernel")]
const fn driver_runtime_command_endpoint_receive_rights() -> sel4_sys::seL4_CapRights {
    // The child owns the receive side of its command endpoint. Root retains
    // the ordinary send cap (and, for linked-pair recovery, its separately
    // admitted recovery copy), so the child must not be able to send commands
    // to itself or manufacture a second producer.
    sel4_sys::seL4_CapRights::new(0, 0, 1, 0)
}

#[cfg(feature = "kernel")]
const fn driver_runtime_command_endpoint_send_rights() -> sel4_sys::seL4_CapRights {
    // MCS Call requires GrantReply so the receiver's dedicated Reply object can
    // capture the caller association. Grant stays clear: no capability may be
    // transferred over the driver command lane.
    sel4_sys::seL4_CapRights::new(1, 0, 0, 1)
}

#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
fn driver_runtime_command_endpoint_badge(task_key: usize) -> Result<seL4_Word, HalError> {
    let task_key = u32::try_from(task_key)
        .map_err(|_| HalError::Unsupported("driver-runtime-mcs-task-key"))?;
    seL4_Word::try_from(pi4_driver_abi::driver_runtime_command_badge(task_key))
        .map_err(|_| HalError::Unsupported("driver-runtime-mcs-command-badge"))
}

#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
fn mint_driver_runtime_command_endpoint(
    env: &mut KernelEnv<'_>,
    command_endpoint_origin: seL4_CPtr,
    task_key: usize,
) -> Result<seL4_CPtr, HalError> {
    let command_badge = driver_runtime_command_endpoint_badge(task_key)?;
    let command_endpoint = env.allocate_slot();
    let err = sel4::cnode_mint_depth(
        env.init_cnode_cap(),
        command_endpoint,
        sel4::word_bits() as u8,
        env.init_cnode_cap(),
        command_endpoint_origin,
        sel4::word_bits() as u8,
        driver_runtime_command_endpoint_send_rights(),
        command_badge,
    );
    if err != seL4_NoError {
        return Err(HalError::Sel4(err));
    }
    Ok(command_endpoint)
}

#[cfg(feature = "kernel")]
const fn driver_runtime_fault_send_rights() -> sel4_sys::seL4_CapRights {
    // The kernel delivers standard/timeout faults as Call-like IPC. Write plus
    // GrantReply is sufficient on seL4 MCS and avoids ordinary Grant authority.
    sel4_sys::seL4_CapRights::new(1, 0, 0, 1)
}

#[cfg(feature = "kernel")]
const fn driver_runtime_completion_notification_send_rights() -> sel4_sys::seL4_CapRights {
    sel4_sys::seL4_CapRights::new(0, 0, 0, 1)
}

#[cfg(feature = "kernel")]
const fn driver_runtime_completion_notification_receive_rights() -> sel4_sys::seL4_CapRights {
    sel4_sys::seL4_CapRights::new(0, 0, 1, 0)
}

#[cfg(all(feature = "kernel", not(sel4_config_kernel_mcs)))]
fn allocate_driver_task_mcs_objects(
    _env: &mut KernelEnv<'_>,
    _contract: DriverTaskContract,
    _task_key: usize,
    _child_cnode: seL4_CPtr,
    _child_depth: u8,
    command_endpoint_origin: seL4_CPtr,
    _fault_endpoint_origin: seL4_CPtr,
) -> Result<DriverTaskMcsObjects, HalError> {
    Ok(DriverTaskMcsObjects::classic(command_endpoint_origin))
}

#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
fn allocate_driver_task_mcs_objects(
    env: &mut KernelEnv<'_>,
    contract: DriverTaskContract,
    task_key: usize,
    child_cnode: seL4_CPtr,
    child_depth: u8,
    command_endpoint_origin: seL4_CPtr,
    _fault_endpoint_origin: seL4_CPtr,
) -> Result<DriverTaskMcsObjects, HalError> {
    let spec = driver_task::pi4_driver_task_runtime_image_spec_for_contract(contract)
        .ok_or(HalError::Unsupported("driver-runtime-mcs-image-spec"))?;
    let temporal = driver_task::driver_task_temporal_config(spec.hot_path)
        .ok_or(HalError::Unsupported("driver-runtime-mcs-temporal-config"))?;
    let fault_origin = critical_tcb::target_fault_endpoint_origin().ok_or(
        HalError::Unsupported("driver-runtime-mcs-critical-fault-endpoint"),
    )?;
    let (generated_standard_badge, generated_timeout_badge) =
        critical_tcb::temporal_fault_badges(temporal.id).ok_or(HalError::Unsupported(
            "driver-runtime-mcs-generated-fault-badges",
        ))?;
    let root_cnode = env.init_cnode_cap();
    let root_depth = sel4::word_bits() as u8;

    let command_endpoint =
        mint_driver_runtime_command_endpoint(env, command_endpoint_origin, task_key)?;

    let command_reply = env.alloc_reply().map_err(HalError::Sel4)?;
    let err = sel4::cnode_copy_depth(
        child_cnode,
        pi4_driver_abi::DRIVER_RUNTIME_COMMAND_REPLY_SLOT as seL4_CPtr,
        child_depth,
        root_cnode,
        command_reply,
        root_depth,
        sel4_sys::seL4_CapRights_All,
    );
    if err != seL4_NoError {
        return Err(HalError::Sel4(err));
    }

    let completion_notification_origin = env.alloc_notification().map_err(HalError::Sel4)?;
    let completion_notification = env.allocate_slot();
    let err = sel4::cnode_mint_depth(
        root_cnode,
        completion_notification,
        root_depth,
        root_cnode,
        completion_notification_origin,
        root_depth,
        driver_runtime_completion_notification_receive_rights(),
        0,
    );
    if err != seL4_NoError {
        return Err(HalError::Sel4(err));
    }
    let completion_task_key = u32::try_from(task_key)
        .map_err(|_| HalError::Unsupported("driver-runtime-mcs-task-key"))?;
    let completion_badge = seL4_Word::try_from(pi4_driver_abi::driver_runtime_completion_badge(
        completion_task_key,
    ))
    .map_err(|_| HalError::Unsupported("driver-runtime-mcs-completion-badge"))?;
    let err = sel4::cnode_mint_depth(
        child_cnode,
        pi4_driver_abi::DRIVER_RUNTIME_COMPLETION_NOTIFICATION_SLOT as seL4_CPtr,
        child_depth,
        root_cnode,
        completion_notification_origin,
        root_depth,
        driver_runtime_completion_notification_send_rights(),
        completion_badge,
    );
    if err != seL4_NoError {
        return Err(HalError::Sel4(err));
    }

    let standard_fault_endpoint = env.allocate_slot();
    let standard_fault_badge = seL4_Word::try_from(generated_standard_badge)
        .map_err(|_| HalError::Unsupported("driver-runtime-mcs-standard-fault-badge"))?;
    let err = sel4::cnode_mint_depth(
        root_cnode,
        standard_fault_endpoint,
        root_depth,
        root_cnode,
        fault_origin,
        root_depth,
        driver_runtime_fault_send_rights(),
        standard_fault_badge,
    );
    if err != seL4_NoError {
        return Err(HalError::Sel4(err));
    }

    let timeout_fault_endpoint = env.allocate_slot();
    let timeout_badge = seL4_Word::try_from(generated_timeout_badge)
        .map_err(|_| HalError::Unsupported("driver-runtime-mcs-timeout-badge"))?;
    let err = sel4::cnode_mint_depth(
        root_cnode,
        timeout_fault_endpoint,
        root_depth,
        root_cnode,
        fault_origin,
        root_depth,
        driver_runtime_fault_send_rights(),
        timeout_badge,
    );
    if err != seL4_NoError {
        return Err(HalError::Sel4(err));
    }

    let sched_context = env
        .alloc_sched_context(temporal.scheduling_context_bits)
        .map_err(HalError::Sel4)?;
    let sched_control = env
        .sched_control_for_core(temporal.sched_control_core)
        .map_err(HalError::Sel4)?;
    let extra_refills = temporal
        .max_refills
        .checked_sub(2)
        .ok_or(HalError::Unsupported("driver-runtime-mcs-refills"))?;
    sel4::configure_sched_context(
        sched_control,
        sched_context,
        u64::from(temporal.budget_us),
        u64::from(temporal.period_us),
        seL4_Word::from(extra_refills),
        timeout_badge,
        0,
    )
    .map_err(HalError::Sel4)?;

    Ok(DriverTaskMcsObjects {
        command_endpoint,
        command_reply,
        completion_notification_origin,
        completion_notification,
        sched_context,
        standard_fault_endpoint,
        timeout_fault_endpoint,
    })
}

#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
fn register_driver_task_fault_source(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
) -> Result<(), HalError> {
    let spec = driver_task::pi4_driver_task_runtime_image_spec_for_contract(contract)
        .ok_or(HalError::Unsupported("driver-runtime-mcs-image-spec"))?;
    let temporal = driver_task::driver_task_temporal_config(spec.hot_path)
        .ok_or(HalError::Unsupported("driver-runtime-mcs-temporal-config"))?;
    let slot = driver_task::driver_runtime_registry_slot(contract)
        .ok_or(HalError::Unsupported("driver-runtime-mcs-registry-slot"))?;
    let cap_generation = driver_task::driver_task_mcs_cap_generation(contract)
        .ok_or(HalError::Unsupported("driver-runtime-mcs-cap-generation"))?;
    critical_tcb::register_target_fault_source(
        temporal.id,
        tcb,
        crate::critical_tcb::GenerationIdentity {
            slot,
            lease_epoch: 1,
            supervisor_generation: 1,
            cap_generation,
        },
    )
    .map_err(|_| HalError::Unsupported("driver-runtime-mcs-fault-register"))
}

#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
fn install_driver_task_supervisor_authority(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
    command_endpoint_origin: seL4_CPtr,
    mcs: DriverTaskMcsObjects,
) -> Result<(), HalError> {
    let configured = crate::generated::worker_resource_admission_config()
        .handoff
        .driver_fault_records;
    if configured == 0 {
        return Ok(());
    }
    let runtime_slot = driver_task::driver_runtime_registry_slot(contract)
        .filter(|slot| *slot < configured)
        .ok_or(HalError::Unsupported(
            "driver-runtime-supervisor-authority-slot",
        ))?;
    let local = critical_tcb::install_driver_supervisor_runtime_caps(
        runtime_slot,
        critical_tcb::DriverSupervisorRuntimeRootCaps {
            tcb,
            command_endpoint_origin,
            command_reply: mcs.command_reply,
            completion_notification_origin: mcs.completion_notification_origin,
            sched_context: mcs.sched_context,
            standard_fault_endpoint: mcs.standard_fault_endpoint,
            timeout_fault_endpoint: mcs.timeout_fault_endpoint,
        },
    )
    .map_err(|_| HalError::Unsupported("driver-runtime-supervisor-authority-install"))?;
    if !driver_task::publish_driver_task_supervisor_authority(contract, local) {
        return Err(HalError::Unsupported(
            "driver-runtime-supervisor-authority-publish",
        ));
    }
    let mut line = heapless::String::<320>::new();
    let _ = fmt::write(
        &mut line,
        format_args!(
            "DRIVER_TASK_SUPERVISOR_AUTHORITY schema=v1 contract={} runtime_slot={} root_tcb=0x{:04x} local_tcb=0x{:02x} local_command_origin=0x{:02x} local_reply=0x{:02x} local_completion_origin=0x{:02x} local_sc=0x{:02x} local_standard_fault=0x{:02x} local_timeout_fault=0x{:02x} root_origins=moved state=ready",
            contract.name,
            runtime_slot,
            tcb,
            local.tcb,
            local.command_endpoint_origin,
            local.command_reply,
            local.completion_notification_origin,
            local.sched_context,
            local.standard_fault_endpoint,
            local.timeout_fault_endpoint,
        ),
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
    Ok(())
}

#[cfg(all(feature = "kernel", sel4_config_kernel_mcs))]
fn register_dormant_driver_task_fault_source(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
) -> Result<(), HalError> {
    const IDENTITY_ONLY_CAP_GENERATION: u32 = 1;

    let spec = driver_task::pi4_driver_task_runtime_image_spec_for_contract(contract)
        .ok_or(HalError::Unsupported("driver-runtime-mcs-image-spec"))?;
    let temporal = driver_task::driver_task_temporal_config(spec.hot_path)
        .ok_or(HalError::Unsupported("driver-runtime-mcs-temporal-config"))?;
    let slot = driver_task::driver_runtime_registry_slot(contract)
        .ok_or(HalError::Unsupported("driver-runtime-mcs-registry-slot"))?;
    critical_tcb::register_target_fault_source(
        temporal.id,
        tcb,
        crate::critical_tcb::GenerationIdentity {
            slot,
            lease_epoch: 1,
            supervisor_generation: 1,
            cap_generation: IDENTITY_ONLY_CAP_GENERATION,
        },
    )
    .map_err(|_| HalError::Unsupported("driver-runtime-mcs-dormant-fault-register"))
}

#[cfg(feature = "kernel")]
fn runtime_elf_page_mapping(fill: RuntimeElfPageFill) -> Result<RuntimeElfPageMapping, HalError> {
    if fill.writable && fill.executable {
        return Err(HalError::Unsupported("driver-runtime-elf-wx-page"));
    }
    Ok(RuntimeElfPageMapping {
        rights: if fill.writable {
            sel4_sys::seL4_CapRights_ReadWrite
        } else {
            sel4_sys::seL4_CapRights::new(0, 0, 1, 0)
        },
        attributes: if fill.executable {
            sel4_sys::seL4_ARM_Page_Default
        } else {
            runtime_cacheable_xn_attributes()
        },
    })
}

#[cfg(feature = "kernel")]
const MAX_RUNTIME_ELF_LOAD_SEGMENTS: usize = 8;

#[cfg(feature = "kernel")]
fn read_le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let raw: [u8; 2] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u16::from_le_bytes(raw))
}

#[cfg(feature = "kernel")]
fn read_le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let raw: [u8; 4] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(raw))
}

#[cfg(feature = "kernel")]
fn read_le_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let raw: [u8; 8] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u64::from_le_bytes(raw))
}

#[cfg(feature = "kernel")]
fn plan_runtime_elf_load(
    image: &[u8],
    declared_code_pages: u16,
) -> Result<RuntimeElfLoadPlan, HalError> {
    const ELF_HEADER_LEN: usize = 64;
    const PROGRAM_HEADER_LEN: usize = 56;
    const PT_LOAD: u32 = 1;
    const PF_X: u32 = 1;
    const PF_W: u32 = 2;
    const PF_R: u32 = 4;
    const PF_MASK: u32 = PF_X | PF_W | PF_R;
    const EM_AARCH64: u16 = 183;
    const ET_EXEC: u16 = 2;

    if image.len() < ELF_HEADER_LEN || image.get(0..4) != Some(b"\x7fELF") {
        return Err(HalError::Unsupported("driver-runtime-elf-magic"));
    }
    if image.get(4) != Some(&2) || image.get(5) != Some(&1) {
        return Err(HalError::Unsupported("driver-runtime-elf-class"));
    }
    if read_le_u16(image, 16) != Some(ET_EXEC) || read_le_u16(image, 18) != Some(EM_AARCH64) {
        return Err(HalError::Unsupported("driver-runtime-elf-target"));
    }

    let entry = usize::try_from(
        read_le_u64(image, 24).ok_or(HalError::Unsupported("driver-runtime-elf-entry"))?,
    )
    .map_err(|_| HalError::Unsupported("driver-runtime-elf-entry"))?;
    let phoff = usize::try_from(
        read_le_u64(image, 32).ok_or(HalError::Unsupported("driver-runtime-elf-phoff"))?,
    )
    .map_err(|_| HalError::Unsupported("driver-runtime-elf-phoff"))?;
    let phentsize = usize::from(
        read_le_u16(image, 54).ok_or(HalError::Unsupported("driver-runtime-elf-phentsize"))?,
    );
    let phnum = usize::from(
        read_le_u16(image, 56).ok_or(HalError::Unsupported("driver-runtime-elf-phnum"))?,
    );
    if phentsize < PROGRAM_HEADER_LEN || phnum == 0 {
        return Err(HalError::Unsupported("driver-runtime-elf-phdr"));
    }

    let page_bytes = 1usize << sel4::PAGE_BITS;
    let mut segments = [RuntimeElfSegment::empty(); MAX_RUNTIME_ELF_LOAD_SEGMENTS];
    let mut segment_count = 0usize;
    let mut min_vaddr = usize::MAX;
    let mut max_vaddr = 0usize;
    let mut entry_in_exec = false;
    for index in 0..phnum {
        let ph = phoff
            .checked_add(
                index
                    .checked_mul(phentsize)
                    .ok_or(HalError::Unsupported("driver-runtime-elf-phdr"))?,
            )
            .ok_or(HalError::Unsupported("driver-runtime-elf-phdr"))?;
        let ph_end = ph
            .checked_add(PROGRAM_HEADER_LEN)
            .ok_or(HalError::Unsupported("driver-runtime-elf-phdr"))?;
        if ph_end > image.len() {
            return Err(HalError::Unsupported("driver-runtime-elf-phdr"));
        }
        let p_type =
            read_le_u32(image, ph).ok_or(HalError::Unsupported("driver-runtime-elf-phdr"))?;
        let p_flags =
            read_le_u32(image, ph + 4).ok_or(HalError::Unsupported("driver-runtime-elf-phdr"))?;
        if p_type != PT_LOAD {
            continue;
        }
        if p_flags & !PF_MASK != 0 || p_flags & PF_R == 0 {
            return Err(HalError::Unsupported("driver-runtime-elf-flags"));
        }
        if p_flags & (PF_W | PF_X) == (PF_W | PF_X) {
            return Err(HalError::Unsupported("driver-runtime-elf-wx-segment"));
        }
        let p_offset = usize::try_from(
            read_le_u64(image, ph + 8).ok_or(HalError::Unsupported("driver-runtime-elf-offset"))?,
        )
        .map_err(|_| HalError::Unsupported("driver-runtime-elf-offset"))?;
        let p_vaddr = usize::try_from(
            read_le_u64(image, ph + 16).ok_or(HalError::Unsupported("driver-runtime-elf-vaddr"))?,
        )
        .map_err(|_| HalError::Unsupported("driver-runtime-elf-vaddr"))?;
        let p_filesz = usize::try_from(
            read_le_u64(image, ph + 32)
                .ok_or(HalError::Unsupported("driver-runtime-elf-filesz"))?,
        )
        .map_err(|_| HalError::Unsupported("driver-runtime-elf-filesz"))?;
        let p_memsz = usize::try_from(
            read_le_u64(image, ph + 40).ok_or(HalError::Unsupported("driver-runtime-elf-memsz"))?,
        )
        .map_err(|_| HalError::Unsupported("driver-runtime-elf-memsz"))?;
        if p_memsz == 0 {
            continue;
        }
        let file_end = p_offset
            .checked_add(p_filesz)
            .ok_or(HalError::Unsupported("driver-runtime-elf-filesz"))?;
        if file_end > image.len()
            || p_filesz > p_memsz
            || segment_count >= MAX_RUNTIME_ELF_LOAD_SEGMENTS
        {
            return Err(HalError::Unsupported("driver-runtime-elf-segment"));
        }
        let segment_end = p_vaddr
            .checked_add(p_memsz)
            .ok_or(HalError::Unsupported("driver-runtime-elf-memsz"))?;
        let page_base = p_vaddr & !(page_bytes - 1);
        let page_end = segment_end
            .checked_add(page_bytes - 1)
            .ok_or(HalError::Unsupported("driver-runtime-elf-memsz"))?
            & !(page_bytes - 1);
        min_vaddr = core::cmp::min(min_vaddr, page_base);
        max_vaddr = core::cmp::max(max_vaddr, page_end);
        if p_flags & PF_X != 0 && entry >= p_vaddr && entry < segment_end {
            entry_in_exec = true;
        }
        segments[segment_count] = RuntimeElfSegment {
            offset: p_offset,
            vaddr: p_vaddr,
            filesz: p_filesz,
            memsz: p_memsz,
            flags: p_flags,
        };
        segment_count += 1;
    }

    if segment_count == 0 || !entry_in_exec || min_vaddr == usize::MAX || max_vaddr <= min_vaddr {
        return Err(HalError::Unsupported("driver-runtime-elf-exec-segment"));
    }
    for executable in segments.iter().take(segment_count).copied() {
        if executable.flags & PF_X == 0 {
            continue;
        }
        let executable_start = executable.vaddr & !(page_bytes - 1);
        let executable_end = executable
            .vaddr
            .checked_add(executable.memsz)
            .and_then(|end| end.checked_add(page_bytes - 1))
            .map(|end| end & !(page_bytes - 1))
            .ok_or(HalError::Unsupported("driver-runtime-elf-memsz"))?;
        for writable in segments.iter().take(segment_count).copied() {
            if writable.flags & PF_W == 0 {
                continue;
            }
            let writable_start = writable.vaddr & !(page_bytes - 1);
            let writable_end = writable
                .vaddr
                .checked_add(writable.memsz)
                .and_then(|end| end.checked_add(page_bytes - 1))
                .map(|end| end & !(page_bytes - 1))
                .ok_or(HalError::Unsupported("driver-runtime-elf-memsz"))?;
            if executable_start < writable_end && writable_start < executable_end {
                return Err(HalError::Unsupported("driver-runtime-elf-wx-page"));
            }
        }
    }
    let span = max_vaddr
        .checked_sub(min_vaddr)
        .ok_or(HalError::Unsupported("driver-runtime-elf-span"))?;
    let page_count = span
        .checked_div(page_bytes)
        .ok_or(HalError::Unsupported("driver-runtime-elf-span"))?;
    if page_count == 0 || page_count > usize::from(declared_code_pages) {
        return Err(HalError::Unsupported("driver-runtime-elf-code-pages"));
    }
    Ok(RuntimeElfLoadPlan {
        entry,
        base_vaddr: min_vaddr,
        page_count,
        segment_count,
        segments,
    })
}

#[cfg(feature = "kernel")]
fn fill_runtime_elf_page(
    image: &[u8],
    plan: RuntimeElfLoadPlan,
    page_index: usize,
    page: &mut [u8],
) -> Result<RuntimeElfPageFill, HalError> {
    const PF_X: u32 = 1;
    const PF_W: u32 = 2;

    let page_bytes = 1usize << sel4::PAGE_BITS;
    let page_vaddr = plan
        .base_vaddr
        .checked_add(page_index.saturating_mul(page_bytes))
        .ok_or(HalError::Unsupported("driver-runtime-elf-page"))?;
    let page_end = page_vaddr
        .checked_add(page_bytes)
        .ok_or(HalError::Unsupported("driver-runtime-elf-page"))?;
    page.fill(0);
    let mut fill = RuntimeElfPageFill::default();
    let mut index = 0usize;
    while index < plan.segment_count {
        let segment = plan.segments[index];
        let segment_mem_end = segment
            .vaddr
            .checked_add(segment.memsz)
            .ok_or(HalError::Unsupported("driver-runtime-elf-memsz"))?;
        if segment.vaddr < page_end && segment_mem_end > page_vaddr {
            fill.writable |= segment.flags & PF_W != 0;
            fill.executable |= segment.flags & PF_X != 0;
            let copy_start = core::cmp::max(page_vaddr, segment.vaddr);
            let segment_file_end = segment
                .vaddr
                .checked_add(segment.filesz)
                .ok_or(HalError::Unsupported("driver-runtime-elf-filesz"))?;
            let copy_end = core::cmp::min(page_end, segment_file_end);
            if copy_start < copy_end {
                let dest_start = copy_start
                    .checked_sub(page_vaddr)
                    .ok_or(HalError::Unsupported("driver-runtime-elf-copy"))?;
                let src_start = segment
                    .offset
                    .checked_add(
                        copy_start
                            .checked_sub(segment.vaddr)
                            .ok_or(HalError::Unsupported("driver-runtime-elf-copy"))?,
                    )
                    .ok_or(HalError::Unsupported("driver-runtime-elf-copy"))?;
                let copy_len = copy_end
                    .checked_sub(copy_start)
                    .ok_or(HalError::Unsupported("driver-runtime-elf-copy"))?;
                let src_end = src_start
                    .checked_add(copy_len)
                    .ok_or(HalError::Unsupported("driver-runtime-elf-copy"))?;
                let dest_end = dest_start
                    .checked_add(copy_len)
                    .ok_or(HalError::Unsupported("driver-runtime-elf-copy"))?;
                let Some(src) = image.get(src_start..src_end) else {
                    return Err(HalError::Unsupported("driver-runtime-elf-copy"));
                };
                let Some(dst) = page.get_mut(dest_start..dest_end) else {
                    return Err(HalError::Unsupported("driver-runtime-elf-copy"));
                };
                dst.copy_from_slice(src);
            }
        }
        index += 1;
    }
    if fill.writable && fill.executable {
        return Err(HalError::Unsupported("driver-runtime-elf-wx-page"));
    }
    Ok(fill)
}

#[cfg(feature = "kernel")]
fn finalize_driver_task_bootstrap_report(
    report: &mut DriverTaskBootstrapReport,
    expected_count: usize,
) {
    let all_configured = report.configured_count == expected_count && report.failed_count == 0;
    let all_live = report.live_tcb_count == expected_count;
    report.capset_proof = all_configured;
    report.fault_proof = all_configured;
    report.revoke_proof = all_configured;
    report.sched_proof = all_configured && all_live;
    report.affinity_proof = all_configured
        && report.affinity_configured_count == expected_count
        && report.affinity_configured_count == report.affinity_applied_count;
    report.vspace_proof = all_configured && report.isolated_vspace_count == expected_count;
    report.pointer_free_ipc_proof =
        report.vspace_proof && report.pointer_free_ipc_count == expected_count;
    let required_hot_path_mask = driver_task::current_pi4_acceptance_hot_path_mask();
    let required_hot_path_count = driver_task::current_pi4_acceptance_hot_path_count();
    let owner_hot_paths_complete =
        report.owner_state_hot_path_mask & required_hot_path_mask == required_hot_path_mask;
    let runtime_images_acceptance_ready = report.runtime_image_acceptance_count
        == required_hot_path_count
        && report.runtime_image_transport_mapped_hot_path_mask & required_hot_path_mask
            == required_hot_path_mask;
    report.owner_state_proof = report.vspace_proof
        && report.pointer_free_ipc_proof
        && owner_hot_paths_complete
        && runtime_images_acceptance_ready;
}

#[cfg(feature = "kernel")]
fn driver_affinity_target(contract: DriverTaskContract) -> Option<DriverAffinityTarget> {
    match contract.name {
        "serial" => Some(DriverAffinityTarget::Serial),
        "usb-local-seat" => Some(DriverAffinityTarget::UsbLocalSeat),
        "hdmi-text" => Some(DriverAffinityTarget::HdmiText),
        "bcmgenet-v5" => Some(DriverAffinityTarget::BcmGenetV5),
        "cyw43455" => Some(DriverAffinityTarget::Cyw43455),
        "rtl8139" => Some(DriverAffinityTarget::Rtl8139),
        "virtio-net" => Some(DriverAffinityTarget::VirtioNet),
        "sdio-host" => Some(DriverAffinityTarget::SdioHost),
        "pcie-root" => Some(DriverAffinityTarget::PcieRoot),
        _ => None,
    }
}

#[cfg(feature = "kernel")]
fn apply_driver_tcb_affinity_for_boot(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
) -> Result<Option<u8>, HalError> {
    #[cfg(sel4_config_kernel_mcs)]
    {
        let spec = driver_task::pi4_driver_task_runtime_image_spec_for_contract(contract)
            .ok_or(HalError::Unsupported("driver-runtime-mcs-image-spec"))?;
        let temporal = driver_task::driver_task_temporal_config(spec.hot_path)
            .ok_or(HalError::Unsupported("driver-runtime-mcs-temporal-config"))?;
        if temporal.sched_control_core != temporal.core {
            return Err(HalError::Unsupported(
                "driver-runtime-mcs-sched-control-core-mismatch",
            ));
        }
        let mut line = heapless::String::<192>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "DRIVER_TASK_AFFINITY_MCS contract={} tcb=0x{:04x} core={} source=sched-control-sc-bind direct-set-affinity=no status=configured",
                contract.name, tcb, temporal.core,
            ),
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
        return Ok(Some(temporal.core));
    }
    #[cfg(not(sel4_config_kernel_mcs))]
    {
        let affinity_target =
            driver_affinity_target(contract).ok_or(HalError::Unsupported("driver-affinity"))?;
        let affinity_policy = affinity::policy();
        if driver_task::physical_pi_driver_task_only_owner_state_active() {
            let selected_core = affinity::select_driver_core(&affinity_policy, affinity_target);
            if let Some(core) = selected_core {
                let mut line = heapless::String::<192>::new();
                let _ = fmt::write(
                &mut line,
                format_args!(
                    "DRIVER_TASK_AFFINITY_DEFERRED contract={} target={} selected_core={} reason=pi4-child-tcb-affinity-boot-stall-guard",
                    contract.name,
                    affinity_target.label(),
                    core,
                ),
            );
                crate::bootstrap::log::force_uart_line(line.as_str());
            }
            return Ok(None);
        }
        affinity::apply_driver_tcb_affinity(tcb, affinity_target, &affinity_policy)
            .map_err(HalError::Affinity)
    }
}

#[cfg(feature = "kernel")]
fn apply_driver_tcb_affinity_after_bootstrap(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
    current: Option<u8>,
) -> Result<Option<u8>, HalError> {
    if current.is_some() || !driver_task::physical_pi_driver_task_only_owner_state_active() {
        return Ok(current);
    }
    let affinity_target =
        driver_affinity_target(contract).ok_or(HalError::Unsupported("driver-affinity"))?;
    let affinity_policy = affinity::policy();
    let applied = affinity::apply_driver_tcb_affinity(tcb, affinity_target, &affinity_policy)
        .map_err(HalError::Affinity)?;
    if let Some(core) = applied {
        let mut line = heapless::String::<192>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "DRIVER_TASK_AFFINITY_APPLIED contract={} target={} selected_core={} reason=pi4-post-bootstrap-proof",
                contract.name,
                affinity_target.label(),
                core,
            ),
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
    }
    Ok(applied)
}

#[cfg(feature = "kernel")]
fn generated_driver_tcb_notification_binding_source(
    contract: DriverTaskContract,
) -> Result<Option<&'static str>, HalError> {
    if contract == SERIAL_DRIVER_TASK_CONTRACT {
        let _ = generated_serial_runtime_irq()?;
        return Ok(Some("generated-serial-irq-topology"));
    }
    if contract == GENET_DRIVER_TASK_CONTRACT {
        let _ = generated_genet_runtime_irq()?;
        return Ok(Some("generated-genet-irq-topology"));
    }
    if contract == PCIE_ROOT_DRIVER_TASK_CONTRACT {
        let _ = generated_pcie_timer_runtime_irq()?;
        return Ok(Some("generated-pcie-timer-irq-topology"));
    }

    let topology = generated_cyw43_sdio_topology()?;
    let generated_peer = (contract == CYW43_WIFI_DRIVER_TASK_CONTRACT
        && topology.link.client_hot_path == driver_task::DriverTaskHotPath::Cyw43Wifi.as_str())
        || (contract == SDIO_HOST_DRIVER_TASK_CONTRACT
            && topology.link.owner_hot_path == driver_task::DriverTaskHotPath::SdioHost.as_str());
    Ok(generated_peer.then_some("generated-cyw43-sdio-topology"))
}

#[cfg(feature = "kernel")]
fn bind_driver_tcb_notification_for_boot(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
    notification: seL4_CPtr,
) -> Result<bool, HalError> {
    if driver_task::physical_pi_driver_task_only_owner_state_active() {
        if let Some(source) = generated_driver_tcb_notification_binding_source(contract)? {
            sel4::bind_tcb_notification(tcb, notification).map_err(HalError::Sel4)?;
            let mut line = heapless::String::<192>::new();
            let _ = fmt::write(
                &mut line,
                format_args!(
                    "DRIVER_TASK_NOTIFICATION_BOUND contract={} tcb=0x{:04x} notification=0x{:04x} source={}",
                    contract.name, tcb, notification, source,
                ),
            );
            crate::bootstrap::log::force_uart_line(line.as_str());
            return Ok(true);
        }
        let mut line = heapless::String::<192>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "DRIVER_TASK_NOTIFICATION_BIND_DEFERRED contract={} tcb=0x{:04x} notification=0x{:04x} reason=pi4-early-tcb-notification-bind-boot-stall-guard",
                contract.name,
                tcb,
                notification,
            ),
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
        return Ok(false);
    }
    sel4::bind_tcb_notification(tcb, notification).map_err(HalError::Sel4)?;
    Ok(true)
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DriverTaskTcbBootState {
    Active,
    Dormant,
}

/// Whether the selected generated policy installs a driver TCB timeout endpoint.
///
/// Natural postponement retains the separately reserved timeout capability,
/// badge, registry identity, and supervisor authority, but leaves the TCB
/// handler slot empty so seL4 postpones current-refill exhaustion. Every other
/// policy retains the existing timeout-fault delivery path.
#[cfg(any(all(feature = "kernel", sel4_config_kernel_mcs), test))]
const fn driver_task_requires_timeout_endpoint(policy: crate::generated::TimeoutPolicy) -> bool {
    !matches!(policy, crate::generated::TimeoutPolicy::NaturalPostpone)
}

#[cfg(feature = "kernel")]
fn configure_driver_tcb_priority_for_boot(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
    mcs: DriverTaskMcsObjects,
    boot_state: DriverTaskTcbBootState,
) -> Result<(u8, u8), HalError> {
    #[cfg(sel4_config_kernel_mcs)]
    {
        let spec = driver_task::pi4_driver_task_runtime_image_spec_for_contract(contract)
            .ok_or(HalError::Unsupported("driver-runtime-mcs-image-spec"))?;
        let temporal = driver_task::driver_task_temporal_config(spec.hot_path)
            .ok_or(HalError::Unsupported("driver-runtime-mcs-temporal-config"))?;
        if mcs.sched_context == 0
            || mcs.standard_fault_endpoint == 0
            || mcs.timeout_fault_endpoint == 0
        {
            return Err(HalError::Unsupported("driver-runtime-mcs-objects"));
        }
        sel4::set_tcb_sched_params_mcs(
            tcb,
            sel4_sys::seL4_CapInitThreadTCB,
            temporal.mcp,
            temporal.priority,
            mcs.sched_context,
            mcs.standard_fault_endpoint,
        )
        .map_err(HalError::Sel4)?;
        let install_timeout_endpoint =
            driver_task_requires_timeout_endpoint(temporal.timeout_policy);
        if install_timeout_endpoint {
            sel4::set_tcb_timeout_endpoint(tcb, mcs.timeout_fault_endpoint)
                .map_err(HalError::Sel4)?;
        }
        let mut line = heapless::String::<240>::new();
        match boot_state {
            DriverTaskTcbBootState::Active => {
                let _ = fmt::write(
                    &mut line,
                    format_args!(
                        "DRIVER_TASK_MCS_ACTIVE contract={} tcb=0x{:04x} sc=0x{:04x} core={} budget_us={} period_us={} priority={} mcp={} timeout_policy={:?} timeout_endpoint={}",
                        contract.name,
                        tcb,
                        mcs.sched_context,
                        temporal.core,
                        temporal.budget_us,
                        temporal.period_us,
                        temporal.priority,
                        temporal.mcp,
                        temporal.timeout_policy,
                        if install_timeout_endpoint {
                            "installed"
                        } else {
                            "omitted"
                        },
                    ),
                );
            }
            DriverTaskTcbBootState::Dormant => {
                let _ = fmt::write(
                    &mut line,
                    format_args!(
                        "DRIVER_TASK_MCS_DORMANT contract={} tcb=0x{:04x} sc=0x{:04x} core={} budget_us={} period_us={} priority={} mcp={} timeout_policy={:?} timeout_endpoint={} state=suspended",
                        contract.name,
                        tcb,
                        mcs.sched_context,
                        temporal.core,
                        temporal.budget_us,
                        temporal.period_us,
                        temporal.priority,
                        temporal.mcp,
                        temporal.timeout_policy,
                        if install_timeout_endpoint {
                            "installed"
                        } else {
                            "omitted"
                        },
                    ),
                );
            }
        }
        crate::bootstrap::log::force_uart_line(line.as_str());
        return Ok((temporal.priority, temporal.priority));
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    let _ = (mcs, boot_state);
    #[cfg(not(sel4_config_kernel_mcs))]
    {
        let steady_priority = contract.sel4_priority();
        let bootstrap_priority = driver_task::driver_task_bootstrap_priority(contract);
        let bootstrap_mcp = core::cmp::max(steady_priority, bootstrap_priority);
        sel4::set_tcb_sched_params(
            tcb,
            sel4_sys::seL4_CapInitThreadTCB,
            bootstrap_mcp,
            bootstrap_priority,
        )
        .map_err(HalError::Sel4)?;
        sel4::set_tcb_priority(tcb, sel4_sys::seL4_CapInitThreadTCB, bootstrap_priority)
            .map_err(HalError::Sel4)?;

        let mut line = heapless::String::<192>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "DRIVER_TASK_START_PRIORITY contract={} tcb=0x{:04x} bootstrap={} steady={}",
                contract.name, tcb, bootstrap_priority, steady_priority,
            ),
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
        Ok((bootstrap_priority, steady_priority))
    }
}

#[cfg(feature = "kernel")]
fn restore_driver_tcb_steady_priority(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
    bootstrap_priority: u8,
    steady_priority: u8,
) -> Result<(), HalError> {
    #[cfg(sel4_config_kernel_mcs)]
    {
        let _ = (tcb, bootstrap_priority, steady_priority);
        driver_task::publish_driver_task_steady_priority_active(contract);
        return Ok(());
    }
    #[cfg(not(sel4_config_kernel_mcs))]
    {
        if bootstrap_priority == steady_priority {
            driver_task::publish_driver_task_steady_priority_active(contract);
            return Ok(());
        }
        sel4::set_tcb_sched_params(
            tcb,
            sel4_sys::seL4_CapInitThreadTCB,
            steady_priority,
            steady_priority,
        )
        .map_err(HalError::Sel4)?;
        sel4::set_tcb_priority(tcb, sel4_sys::seL4_CapInitThreadTCB, steady_priority)
            .map_err(HalError::Sel4)?;

        let mut line = heapless::String::<192>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "DRIVER_TASK_STEADY_PRIORITY contract={} tcb=0x{:04x} priority={}",
                contract.name, tcb, steady_priority,
            ),
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
        driver_task::publish_driver_task_steady_priority_active(contract);
        Ok(())
    }
}

#[cfg(feature = "kernel")]
fn retain_deferred_cyw43_pair_bootstrap_priority(
    contract: DriverTaskContract,
    runtime_init_deferred: bool,
) -> bool {
    runtime_init_deferred
        && (contract == driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT
            || contract == driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT)
}

#[cfg(feature = "kernel")]
fn emit_driver_tcb_resume_return(contract: DriverTaskContract, tcb: seL4_CPtr, mode: &str) {
    let mut line = heapless::String::<192>::new();
    let _ = fmt::write(
        &mut line,
        format_args!(
            "DRIVER_TASK_RESUME_RETURN contract={} tcb=0x{:04x} mode={}",
            contract.name, tcb, mode,
        ),
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_driver_task_bootstrap_deferred(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
    runtime_descriptor_staged: bool,
) {
    let mut line = heapless::String::<224>::new();
    let _ = fmt::write(
        &mut line,
        format_args!(
            "DRIVER_TASK_BOOTSTRAP_DEFERRED contract={} tcb=0x{:04x} runtime_descriptor={} reason=root-shell-before-first-service-proof",
            contract.name,
            tcb,
            if runtime_descriptor_staged { "yes" } else { "no" },
        ),
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
}

/// Raw-pointer Wi-Fi debug adapter used by the root console without borrowing
/// the leaked kernel HAL for the entire runtime.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelWifiDebugHandle {
    hal_ptr: usize,
}

#[cfg(feature = "kernel")]
impl<'a> KernelHal<'a> {
    /// Construct a new HAL instance wrapping the supplied [`KernelEnv`].
    #[must_use]
    pub fn new(env: KernelEnv<'a>) -> Self {
        Self {
            env,
            root_control_wake_notification_origin: sel4_sys::seL4_CapNull,
            driver_tasks: heapless::Vec::new(),
            dormant_driver_tcbs: heapless::Vec::new(),
            driver_task_report: DriverTaskBootstrapReport::default(),
        }
    }

    /// Retain root's sole receive/origin capability for the root-control fan-in.
    ///
    /// Physical runtimes receive only fixed-slot, badged, Write-only children
    /// minted from this cap. Replacing the origin after any child construction
    /// would split one durable wake domain across unrelated notification
    /// objects, so installation is deliberately once-only.
    pub(crate) fn install_root_control_wake_notification_origin(
        &mut self,
        cap: seL4_CPtr,
    ) -> Result<(), HalError> {
        if cap == sel4_sys::seL4_CapNull {
            return Err(HalError::Unsupported(
                "root-control-wake-notification-origin-null",
            ));
        }
        if self.root_control_wake_notification_origin != sel4_sys::seL4_CapNull {
            return Err(HalError::Unsupported(
                "root-control-wake-notification-origin-already-installed",
            ));
        }
        self.root_control_wake_notification_origin = cap;
        Ok(())
    }

    /// Return root's retained receive/origin cap for the root-control fan-in.
    #[must_use]
    pub(crate) fn root_control_wake_notification_origin(&self) -> Option<seL4_CPtr> {
        (self.root_control_wake_notification_origin != sel4_sys::seL4_CapNull)
            .then_some(self.root_control_wake_notification_origin)
    }

    /// Consumes bootstrap CSpace slots allocated before the HAL is initialised.
    pub fn consume_bootstrap_slots(&mut self, slots: usize) {
        self.env.consume_bootstrap_slots(slots);
    }

    /// Admits selected child-only MMIO pages before higher root MMIO mappings
    /// advance the same seL4 device-untyped cursor.
    ///
    /// The retained capability is HAL-owned and unmapped in root. Runtime
    /// bootstrap later maps it once into the isolated child VSpace and consumes
    /// the admission record so root cannot discover or alias it.
    pub fn admit_selected_pi4_runtime_mmio(&mut self) -> Result<usize, HalError> {
        let pages = selected_pi4_early_child_mmio_pages(
            driver_task::pi4_pre_root_net_bootstrap_selection(),
        );
        for &paddr in pages {
            self.env
                .admit_device_page_for_child(paddr)
                .map_err(HalError::Sel4)?;
        }
        Ok(pages.len())
    }

    /// Returns the underlying bootinfo pointer.
    pub fn bootinfo(&self) -> &'a sel4_sys::seL4_BootInfo {
        self.env.bootinfo()
    }

    /// Access to the underlying [`KernelEnv`] for transitional callers.
    pub fn as_env_mut(&mut self) -> &mut KernelEnv<'a> {
        &mut self.env
    }

    fn preflight_isolated_driver_task_cspace(
        &self,
        contracts: &[DriverTaskContract],
    ) -> Result<DriverTaskCspaceBudget, DriverTaskCspacePreflightError> {
        let mut required_slots = DRIVER_TASK_CSPACE_POST_BOOT_RESERVE;
        for contract in contracts {
            let spec = driver_task::pi4_driver_task_runtime_image_spec_for_contract(*contract)
                .ok_or(DriverTaskCspacePreflightError::MissingRuntimeImage(
                    contract.name,
                ))?;
            let image = driver_task::driver_runtime_image_bytes(spec.hot_path).ok_or(
                DriverTaskCspacePreflightError::MissingRuntimeImage(contract.name),
            )?;
            let plan = plan_runtime_elf_load(
                image,
                spec.region_pages(driver_task::DriverTaskRuntimeRegionKind::Code),
            )
            .map_err(|_| DriverTaskCspacePreflightError::InvalidRuntimeImage(contract.name))?;
            let contract_slots = isolated_runtime_cspace_upper_bound(
                spec,
                plan.page_count,
                *contract == HDMI_TEXT_DRIVER_TASK_CONTRACT,
            )
            .ok_or(DriverTaskCspacePreflightError::ArithmeticOverflow)?;
            required_slots = required_slots
                .checked_add(contract_slots)
                .ok_or(DriverTaskCspacePreflightError::ArithmeticOverflow)?;
        }
        let dormant_contract_count = PHYSICAL_PI_DRIVER_TASK_FAULT_CONTRACTS
            .iter()
            .filter(|contract| !contracts.contains(contract))
            .count();
        required_slots = required_slots
            .checked_add(
                dormant_contract_count
                    .checked_mul(DORMANT_DRIVER_FAULT_IDENTITY_ROOT_SLOTS)
                    .ok_or(DriverTaskCspacePreflightError::ArithmeticOverflow)?,
            )
            .ok_or(DriverTaskCspacePreflightError::ArithmeticOverflow)?;

        let available_slots = self.env.snapshot().cspace_remaining;
        if available_slots < required_slots {
            return Err(DriverTaskCspacePreflightError::Insufficient {
                required_slots,
                available_slots,
            });
        }
        Ok(DriverTaskCspaceBudget {
            required_slots,
            available_slots,
            reserve_slots: DRIVER_TASK_CSPACE_POST_BOOT_RESERVE,
            contract_count: contracts.len(),
            dormant_contract_count,
        })
    }

    /// Creates the seL4 driver-task substrate for Pi 4 hardware roles.
    ///
    /// This creates live, separately scheduled TCBs with restricted child CSpaces,
    /// command endpoints, notifications, IPC buffers, stacks, and fault endpoints.
    /// It does not by itself claim hot-path migration; active drivers must still
    /// record dedicated service turns before closure can pass.
    pub fn bootstrap_driver_task_substrate(
        &mut self,
        fault_endpoint: seL4_CPtr,
    ) -> DriverTaskBootstrapReport {
        let mut report = DriverTaskBootstrapReport {
            broad_caps_leaked: 0,
            ..DriverTaskBootstrapReport::default()
        };
        driver_task::clear_wifi_driver_task_bootstrap_failure();

        let use_isolated_vspace =
            driver_task::physical_pi_driver_task_bootstrap_requires_isolated_vspace();

        let bootstrap_contracts = if driver_task::physical_pi_driver_task_only_owner_state_active()
        {
            physical_pi_driver_task_bootstrap_contracts()
        } else {
            DRIVER_TASK_BOOTSTRAP_CONTRACTS
        };

        if use_isolated_vspace {
            match self.preflight_isolated_driver_task_cspace(bootstrap_contracts) {
                Ok(budget) => {
                    let mut line = heapless::String::<224>::new();
                    let _ = fmt::write(
                        &mut line,
                        format_args!(
                            "DRIVER_TASK_CSPACE_PREFLIGHT status=ready contracts={} dormant_fault_identities={} required={} available={} reserve={} framebuffer_pages_max={}",
                            budget.contract_count,
                            budget.dormant_contract_count,
                            budget.required_slots,
                            budget.available_slots,
                            budget.reserve_slots,
                            DRIVER_TASK_CSPACE_MAX_FRAMEBUFFER_PAGES,
                        ),
                    );
                    crate::bootstrap::log::force_uart_line(line.as_str());
                }
                Err(err) => {
                    report.failed_count = bootstrap_contracts.len();
                    let mut line = heapless::String::<256>::new();
                    let _ = fmt::write(
                        &mut line,
                        format_args!(
                            "DRIVER_TASK_CSPACE_PREFLIGHT status=failed contracts={} err={} action=reject-before-first-child",
                            bootstrap_contracts.len(),
                            err,
                        ),
                    );
                    crate::bootstrap::log::force_uart_line(line.as_str());
                    finalize_driver_task_bootstrap_report(&mut report, bootstrap_contracts.len());
                    self.driver_task_report = report;
                    driver_task::publish_driver_task_bootstrap_report(report);
                    return report;
                }
            }
        }

        for contract in bootstrap_contracts {
            let created = if use_isolated_vspace {
                self.create_isolated_driver_task(*contract, fault_endpoint)
            } else {
                self.create_driver_task(*contract, fault_endpoint)
            };
            match created {
                Ok(handle) => {
                    let _ = driver_task::register_pi4_bus_ring_service(*contract);
                    if let Err(rejected_handle) = self.driver_tasks.push(handle) {
                        let _ = release_runtime_irq_bindings(rejected_handle.runtime_irqs);
                        report.failed_count = report.failed_count.saturating_add(1);
                        if let Some(task_key) = driver_task::driver_task_contract_key(*contract) {
                            driver_task::publish_wifi_driver_task_bootstrap_failure(
                                DriverTaskBootstrapFailure {
                                    task_key,
                                    reason: DriverTaskBootstrapFailureReason::Other,
                                },
                            );
                        }
                        let mut line = heapless::String::<192>::new();
                        let _ = fmt::write(
                            &mut line,
                            format_args!(
                                "DRIVER_TASK_BOOT contract={} role={} status=failed err=driver-task-handle-capacity",
                                contract.name,
                                contract.kind.proof_role(),
                            ),
                        );
                        crate::bootstrap::log::force_uart_line(line.as_str());
                        continue;
                    }
                    add_driver_task_handle_to_report(&mut report, &handle);
                    let runtime_owner_state_registered =
                        handle.runtime_image_spec.is_some_and(|spec| {
                            driver_task::driver_task_runtime_owner_state_registered(spec.hot_path)
                        });
                    let mut line = heapless::String::<1024>::new();
                    let _ = fmt::write(
                        &mut line,
                        format_args!(
                            "DRIVER_TASK_BOOT contract={} role={} tcb=0x{:04x} cnode=0x{:04x} endpoint=0x{:04x} notification=0x{:04x} root_notification=0x{:04x} root_wake_notification=0x{:04x} started={} affinity_core={} isolation_cspace=restricted vspace={} vspace_cap=0x{:04x} code_vaddr=0x{:08x} ring_vaddr=0x{:08x} ipc_abi={} pointer_free_ipc={} runtime_image={} runtime_declared=0x{:02x} runtime_mapped=0x{:02x} runtime_acceptance={} owner_state={} owner_state_reason={}",
                            handle.contract.name,
                            handle.contract.kind.proof_role(),
                            handle.tcb,
                            handle.cnode,
                            handle.command_endpoint,
                            handle.notification,
                            handle.root_notification,
                            handle.root_wake_notification,
                            if handle.started { "yes" } else { "no" },
                            match handle.affinity_core {
                                Some(core) => core as i32,
                                None => -1,
                            },
                            if handle.vspace_isolated { "isolated" } else { "shared-root" },
                            handle.vspace.unwrap_or(0),
                            handle.code_vaddr,
                            handle.ring_vaddr,
                            driver_task::CURRENT_DRIVER_TASK_IPC_ABI.as_str(),
                            if handle.pointer_free_ipc { "yes" } else { "no" },
                            if handle.runtime_image_spec.is_some() {
                                if handle.vspace_isolated {
                                    "transport-mapped"
                                } else {
                                    "declared-only"
                                }
                            } else {
                                "none"
                            },
                            handle.runtime_image_declared_region_mask,
                            handle.runtime_image_mapped_region_mask,
                            if handle.runtime_image_acceptance_eligible {
                                "yes"
                            } else {
                                "no"
                            },
                            if runtime_owner_state_registered {
                                "driver-owned"
                            } else if handle.vspace_isolated {
                                "not-proven"
                            } else {
                                "root-owned"
                            },
                            handle.runtime_image_non_acceptance_reason,
                        ),
                    );
                    crate::bootstrap::log::force_uart_line(line.as_str());
                }
                Err(err) => {
                    if let Some(task_key) = driver_task::driver_task_contract_key(*contract) {
                        driver_task::publish_wifi_driver_task_bootstrap_failure(
                            DriverTaskBootstrapFailure {
                                task_key,
                                reason: classify_driver_task_bootstrap_failure(err),
                            },
                        );
                    }
                    driver_task::clear_driver_task_transport(*contract);
                    report.failed_count = report.failed_count.saturating_add(1);
                    let mut line = heapless::String::<192>::new();
                    let _ = fmt::write(
                        &mut line,
                        format_args!(
                            "DRIVER_TASK_BOOT contract={} role={} status=failed err={}",
                            contract.name,
                            contract.kind.proof_role(),
                            err,
                        ),
                    );
                    crate::bootstrap::log::force_uart_line(line.as_str());
                }
            }
        }

        if driver_task::physical_pi_driver_task_only_owner_state_active() {
            for contract in PHYSICAL_PI_DRIVER_TASK_FAULT_CONTRACTS
                .iter()
                .copied()
                .filter(|contract| !bootstrap_contracts.contains(contract))
            {
                match self.construct_dormant_driver_fault_identity(contract) {
                    Ok(tcb) => {
                        if self.dormant_driver_tcbs.push((contract, tcb)).is_err() {
                            report.failed_count = report.failed_count.saturating_add(1);
                            crate::bootstrap::log::force_uart_line(
                                "DRIVER_TASK_DORMANT status=failed err=dormant-handle-capacity",
                            );
                        }
                    }
                    Err(error) => {
                        report.failed_count = report.failed_count.saturating_add(1);
                        let mut line = heapless::String::<224>::new();
                        let _ = fmt::write(
                            &mut line,
                            format_args!(
                                "DRIVER_TASK_DORMANT contract={} role={} status=failed err={}",
                                contract.name,
                                contract.kind.proof_role(),
                                error,
                            ),
                        );
                        crate::bootstrap::log::force_uart_line(line.as_str());
                    }
                }
            }
        }

        finalize_driver_task_bootstrap_report(&mut report, bootstrap_contracts.len());
        self.driver_task_report = report;
        driver_task::publish_driver_task_bootstrap_report(report);
        report
    }

    /// Construct one non-selected Pi driver as a suspended MCS fault identity.
    ///
    /// The alternate driver receives no MMIO, DMA, IRQ, shared ring, published
    /// transport, entry registers, or bus-service registration. Its TCB, CSpace,
    /// VSpace, SC, Reply/notification objects, and generated fault caps exist so
    /// the exact target registry can seal without admitting a second physical
    /// dataplane owner.
    #[cfg(sel4_config_kernel_mcs)]
    fn construct_dormant_driver_fault_identity(
        &mut self,
        contract: DriverTaskContract,
    ) -> Result<seL4_CPtr, HalError> {
        contract.validate().map_err(HalError::DriverTaskContract)?;
        let mut begin_line = heapless::String::<160>::new();
        let _ = fmt::write(
            &mut begin_line,
            format_args!(
                "DRIVER_TASK_DORMANT contract={} role={} phase=allocate-begin state=suspended hardware_authority=none",
                contract.name,
                contract.kind.proof_role(),
            ),
        );
        crate::bootstrap::log::force_uart_line(begin_line.as_str());
        let task_key = driver_task::driver_task_contract_key(contract)
            .ok_or(HalError::Unsupported("driver-task-key"))?;
        let child_depth = driver_task::DRIVER_TASK_CHILD_CNODE_RADIX_BITS;
        let child_cnode = self.env.alloc_cnode(child_depth).map_err(HalError::Sel4)?;
        let tcb = self.env.alloc_tcb().map_err(HalError::Sel4)?;
        let command_endpoint_origin = self.env.alloc_endpoint().map_err(HalError::Sel4)?;
        let vspace = self.env.alloc_vspace_root().map_err(HalError::Sel4)?;
        self.env
            .assign_vspace_asid_from_init_pool(vspace)
            .map_err(HalError::Sel4)?;
        let mut objects_line = heapless::String::<224>::new();
        let _ = fmt::write(
            &mut objects_line,
            format_args!(
                "DRIVER_TASK_DORMANT contract={} phase=objects-ready tcb=0x{:04x} cnode=0x{:04x} endpoint_origin=0x{:04x} vspace=0x{:04x}",
                contract.name, tcb, child_cnode, command_endpoint_origin, vspace,
            ),
        );
        crate::bootstrap::log::force_uart_line(objects_line.as_str());
        let mcs = allocate_driver_task_mcs_objects(
            &mut self.env,
            contract,
            task_key,
            child_cnode,
            child_depth,
            command_endpoint_origin,
            sel4_sys::seL4_CapNull,
        )?;
        let guard_bits = sel4::word_bits().saturating_sub(child_depth as seL4_Word);
        sel4::set_tcb_space(
            tcb,
            mcs.standard_fault_endpoint,
            child_cnode,
            sel4::cap_data_guard(0, guard_bits),
            vspace,
            0,
        )
        .map_err(HalError::Sel4)?;
        let _ = configure_driver_tcb_priority_for_boot(
            contract,
            tcb,
            mcs,
            DriverTaskTcbBootState::Dormant,
        )?;
        let affinity_core = apply_driver_tcb_affinity_for_boot(contract, tcb)?;
        register_dormant_driver_task_fault_source(contract, tcb)?;
        sel4::suspend_tcb(tcb).map_err(HalError::Sel4)?;

        let mut line = heapless::String::<320>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "DRIVER_TASK_DORMANT contract={} role={} tcb=0x{:04x} cnode=0x{:04x} vspace=0x{:04x} sc=0x{:04x} standard_fault=0x{:04x} timeout_fault=0x{:04x} core={} state=suspended registers=unset transport=unpublished hardware_authority=none registry=registered",
                contract.name,
                contract.kind.proof_role(),
                tcb,
                child_cnode,
                vspace,
                mcs.sched_context,
                mcs.standard_fault_endpoint,
                mcs.timeout_fault_endpoint,
                affinity_core.map_or(-1, i32::from),
            ),
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
        Ok(tcb)
    }

    #[cfg(not(sel4_config_kernel_mcs))]
    fn construct_dormant_driver_fault_identity(
        &mut self,
        _contract: DriverTaskContract,
    ) -> Result<seL4_CPtr, HalError> {
        Err(HalError::Unsupported(
            "driver-task-dormant-identity-requires-mcs",
        ))
    }

    /// Publishes an inactive driver-task substrate when a profile must preserve
    /// scarce boot resources for a compatibility path.
    ///
    /// Contract declarations remain valid and later proof still fails closed:
    /// no boot may claim dedicated driver-task isolation from this report.
    pub fn skip_driver_task_substrate(
        &mut self,
        reason: &'static str,
    ) -> DriverTaskBootstrapReport {
        let report = DriverTaskBootstrapReport {
            broad_caps_leaked: 0,
            ..DriverTaskBootstrapReport::default()
        };
        let mut line = heapless::String::<160>::new();
        let _ = fmt::write(
            &mut line,
            format_args!("DRIVER_TASK_BOOT status=skipped reason={reason}"),
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
        self.driver_task_report = report;
        driver_task::publish_driver_task_bootstrap_report(report);
        report
    }

    /// Creates a QEMU-only post-network driver-task smoke probe.
    ///
    /// QEMU virtio compatibility builds intentionally skip the full pre-network
    /// substrate so TCP console bring-up keeps enough boot resources. After
    /// virtio net is already online, this creates the full declared driver-task
    /// contract set so the QEMU run can prove TCB/cap/affinity mechanics
    /// without claiming Pi 4 hardware closure or isolated driver VSpaces.
    pub fn bootstrap_qemu_post_net_driver_task_smoke(
        &mut self,
        fault_endpoint: seL4_CPtr,
    ) -> DriverTaskBootstrapReport {
        if !self.driver_tasks.is_empty() {
            let mut line = heapless::String::<192>::new();
            let _ = fmt::write(
                &mut line,
                format_args!(
                    "DRIVER_TASK_BOOT_SMOKE phase=post-net-qemu contracts={} status=failed err=driver-task-substrate-already-active action=root-task-compatibility",
                    DRIVER_TASK_BOOTSTRAP_CONTRACTS.len(),
                ),
            );
            crate::bootstrap::log::force_uart_line(line.as_str());
            return self.driver_task_report;
        }

        let mut report = DriverTaskBootstrapReport {
            broad_caps_leaked: 0,
            ..DriverTaskBootstrapReport::default()
        };

        for contract in DRIVER_TASK_BOOTSTRAP_CONTRACTS {
            match self.create_isolated_driver_task(*contract, fault_endpoint) {
                Ok(handle) => {
                    if let Err(rejected_handle) = self.driver_tasks.push(handle) {
                        let _ = release_runtime_irq_bindings(rejected_handle.runtime_irqs);
                        report.failed_count = report.failed_count.saturating_add(1);
                        let mut line = heapless::String::<192>::new();
                        let _ = fmt::write(
                            &mut line,
                            format_args!(
                                "DRIVER_TASK_BOOT_SMOKE phase=post-net-qemu contract={} role={} status=failed err=driver-task-handle-capacity action=root-task-compatibility",
                                contract.name,
                                contract.kind.proof_role(),
                            ),
                        );
                        crate::bootstrap::log::force_uart_line(line.as_str());
                        continue;
                    }

                    add_driver_task_handle_to_report(&mut report, &handle);

                    let mut line = heapless::String::<1024>::new();
                    let _ = fmt::write(
                        &mut line,
                        format_args!(
                            "DRIVER_TASK_BOOT_SMOKE phase=post-net-qemu contract={} role={} status=created tcb=0x{:04x} cnode=0x{:04x} endpoint=0x{:04x} notification=0x{:04x} root_notification=0x{:04x} root_wake_notification=0x{:04x} started={} affinity_core={} isolation_cspace=restricted vspace={} vspace_cap=0x{:04x} code_vaddr=0x{:08x} ring_vaddr=0x{:08x} ipc_abi={} pointer_free_ipc={} proof={} runtime_image={} runtime_declared=0x{:02x} runtime_mapped=0x{:02x} runtime_acceptance={} owner_state=not-proven owner_state_reason={}",
                            handle.contract.name,
                            handle.contract.kind.proof_role(),
                            handle.tcb,
                            handle.cnode,
                            handle.command_endpoint,
                            handle.notification,
                            handle.root_notification,
                            handle.root_wake_notification,
                            if handle.started { "yes" } else { "no" },
                            match handle.affinity_core {
                                Some(core) => core as i32,
                                None => -1,
                            },
                            if handle.vspace_isolated { "isolated" } else { "shared-root" },
                            handle.vspace.unwrap_or(0),
                            handle.code_vaddr,
                            handle.ring_vaddr,
                            driver_task::DriverTaskIpcAbi::SharedRingCommand.as_str(),
                            if handle.pointer_free_ipc { "yes" } else { "no" },
                            if handle.pointer_free_ipc { "ring" } else { "partial" },
                            if handle.runtime_image_spec.is_some() {
                                "transport-mapped"
                            } else {
                                "none"
                            },
                            handle.runtime_image_declared_region_mask,
                            handle.runtime_image_mapped_region_mask,
                            if handle.runtime_image_acceptance_eligible {
                                "yes"
                            } else {
                                "no"
                            },
                            handle.runtime_image_non_acceptance_reason,
                        ),
                    );
                    crate::bootstrap::log::force_uart_line(line.as_str());
                }
                Err(err) => {
                    driver_task::clear_driver_task_transport(*contract);
                    report.failed_count = report.failed_count.saturating_add(1);
                    let mut line = heapless::String::<192>::new();
                    let _ = fmt::write(
                        &mut line,
                        format_args!(
                            "DRIVER_TASK_BOOT_SMOKE phase=post-net-qemu contract={} role={} status=failed err={} action=root-task-compatibility",
                            contract.name,
                            contract.kind.proof_role(),
                            err,
                        ),
                    );
                    crate::bootstrap::log::force_uart_line(line.as_str());
                }
            }
        }

        finalize_driver_task_bootstrap_report(&mut report, DRIVER_TASK_BOOTSTRAP_CONTRACTS.len());
        self.driver_task_report = report;
        driver_task::publish_driver_task_bootstrap_report(report);
        report
    }

    /// Returns the latest driver-task runtime proof snapshot.
    #[must_use]
    pub fn driver_task_runtime_proof(&self) -> DriverTaskRuntimeProof {
        driver_task::driver_task_runtime_proof()
    }

    /// Install the two send-only capabilities for the exact Pi GENET direct link.
    ///
    /// The existing GENET notification remains the sole child-bound IRQ/work
    /// receive object; the console child gets only a badged signal cap. The
    /// GENET child receives the reciprocal badged signal cap to the console
    /// child's existing wake notification. No root packet authority is added.
    #[cfg(feature = "net-backend-genet-direct")]
    pub(crate) fn install_direct_genet_peer_notifications(
        &mut self,
        console_child_cnode: seL4_CPtr,
        console_root_to_child_notification: seL4_CPtr,
        enabled: bool,
    ) -> Result<ReciprocalLinkCapGuard, HalError> {
        if !enabled {
            return Ok(ReciprocalLinkCapGuard::empty());
        }
        let owner = self
            .driver_tasks
            .iter()
            .find(|handle| handle.contract == GENET_DRIVER_TASK_CONTRACT)
            .copied()
            .ok_or(HalError::Unsupported(
                "console-network-direct-genet-owner-missing",
            ))?;
        if !owner.started
            || !owner.vspace_isolated
            || owner.notification == sel4_sys::seL4_CapNull
            || owner.cnode == sel4_sys::seL4_CapNull
            || console_child_cnode == sel4_sys::seL4_CapNull
            || console_root_to_child_notification == sel4_sys::seL4_CapNull
            || owner.reciprocal_link_caps.iter().any(Option::is_some)
        {
            return Err(HalError::Unsupported(
                "console-network-direct-genet-owner-state",
            ));
        }
        let root_cnode = self.env.init_cnode_cap();
        let root_depth = sel4::word_bits() as u8;
        let child_depth = driver_task::DRIVER_TASK_CHILD_CNODE_RADIX_BITS;
        let send_only = sel4_sys::seL4_CapRights::new(0, 0, 0, 1);
        let mut installed = ReciprocalLinkCapGuard::empty();

        let console_peer_slot =
            seL4_CPtr::from(console_network_abi::DIRECT_GENET_PEER_WAKE_NOTIFICATION_SLOT);
        let console_to_genet = sel4::cnode_mint_depth(
            console_child_cnode,
            console_peer_slot,
            child_depth,
            root_cnode,
            owner.notification,
            root_depth,
            send_only,
            seL4_Word::from(pi4_driver_abi::DRIVER_RUNTIME_DIRECT_GENET_NOTIFICATION_BADGE),
        );
        if console_to_genet != seL4_NoError {
            return Err(HalError::Sel4(console_to_genet));
        }
        installed.push(InstalledChildCap {
            cnode: console_child_cnode,
            slot: console_peer_slot,
            depth: child_depth,
        })?;

        let genet_peer_slot =
            seL4_CPtr::from(pi4_driver_abi::DRIVER_RUNTIME_DIRECT_GENET_PEER_NOTIFICATION_SLOT);
        let genet_to_console = sel4::cnode_mint_depth(
            owner.cnode,
            genet_peer_slot,
            child_depth,
            root_cnode,
            console_root_to_child_notification,
            root_depth,
            send_only,
            seL4_Word::from(console_network_abi::WAKE_DIRECT_GENET_LINK),
        );
        if genet_to_console != seL4_NoError {
            return Err(HalError::Sel4(genet_to_console));
        }
        installed.push(InstalledChildCap {
            cnode: owner.cnode,
            slot: genet_peer_slot,
            depth: child_depth,
        })?;

        Ok(installed)
    }

    /// Commit reciprocal peer-cap metadata only after the complete console
    /// generation has crossed every later fallible construction boundary.
    #[cfg(feature = "net-backend-genet-direct")]
    pub(crate) fn commit_direct_genet_peer_notifications(
        &mut self,
        installed: ReciprocalLinkCapGuard,
    ) -> Result<(), HalError> {
        let owner = self
            .driver_tasks
            .iter_mut()
            .find(|handle| handle.contract == GENET_DRIVER_TASK_CONTRACT)
            .ok_or(HalError::Unsupported(
                "console-network-direct-genet-owner-missing",
            ))?;
        if owner.reciprocal_link_caps.iter().any(Option::is_some) {
            return Err(HalError::Unsupported(
                "console-network-direct-genet-owner-state",
            ));
        }
        owner.reciprocal_link_caps = installed.commit();
        Ok(())
    }

    /// Fence the coupled direct-GENET generation before console teardown.
    ///
    /// Suspending the sole MMIO/DMA/IRQ owner precedes deletion of either
    /// cross-child signal cap. Only after both caps are gone is the root-side
    /// handoff token permanently fenced. The GENET child is not resumed and
    /// root packet mediation is never reinstated as a recovery fallback.
    #[cfg(feature = "net-backend-genet-direct")]
    pub(crate) fn fence_direct_genet_peer(&mut self) -> Result<(), HalError> {
        let owner = self
            .driver_tasks
            .iter_mut()
            .find(|handle| handle.contract == GENET_DRIVER_TASK_CONTRACT)
            .ok_or(HalError::Unsupported(
                "console-network-direct-genet-owner-missing",
            ))?;
        if !owner.started || owner.tcb == sel4_sys::seL4_CapNull {
            return Err(HalError::Unsupported(
                "console-network-direct-genet-owner-state",
            ));
        }
        sel4::suspend_tcb(owner.tcb).map_err(HalError::Sel4)?;
        for installed in &mut owner.reciprocal_link_caps {
            let Some(cap) = *installed else {
                continue;
            };
            let error = sel4::cnode_delete(cap.cnode, cap.slot, cap.depth);
            if error != sel4_sys::seL4_NoError {
                return Err(HalError::Sel4(error));
            }
            *installed = None;
        }
        if owner.reciprocal_link_caps.iter().any(Option::is_some) {
            return Err(HalError::Unsupported(
                "console-network-direct-genet-peer-cap-retained",
            ));
        }
        crate::drivers::driver_task_net::fence_genet_direct_link();
        Ok(())
    }

    /// Non-direct profiles install no cross-child GENET capabilities.
    #[cfg(not(feature = "net-backend-genet-direct"))]
    pub(crate) fn install_direct_genet_peer_notifications(
        &mut self,
        _console_child_cnode: seL4_CPtr,
        _console_root_to_child_notification: seL4_CPtr,
        _enabled: bool,
    ) -> Result<ReciprocalLinkCapGuard, HalError> {
        Ok(ReciprocalLinkCapGuard::empty())
    }

    #[cfg(not(feature = "net-backend-genet-direct"))]
    pub(crate) fn commit_direct_genet_peer_notifications(
        &mut self,
        _installed: ReciprocalLinkCapGuard,
    ) -> Result<(), HalError> {
        Ok(())
    }

    /// Non-direct profiles have no paired GENET generation to fence.
    #[cfg(not(feature = "net-backend-genet-direct"))]
    pub(crate) fn fence_direct_genet_peer(&mut self) -> Result<(), HalError> {
        Err(HalError::Unsupported(
            "console-network-direct-genet-disabled",
        ))
    }

    fn create_driver_task(
        &mut self,
        contract: DriverTaskContract,
        fault_endpoint: seL4_CPtr,
    ) -> Result<KernelDriverTaskHandle, HalError> {
        contract.validate().map_err(HalError::DriverTaskContract)?;

        let role_bit = driver_task::driver_task_role_bit(contract.kind);
        if role_bit == 0 {
            return Err(HalError::Unsupported("driver-task-role"));
        }
        let task_key = driver_task::driver_task_contract_key(contract)
            .ok_or(HalError::Unsupported("driver-task-key"))?;
        let runtime_image_spec =
            driver_task::pi4_driver_task_runtime_image_spec_for_contract(contract);

        let root_cnode = self.env.init_cnode_cap();
        let root_depth = sel4::word_bits() as u8;
        let child_depth = driver_task::DRIVER_TASK_CHILD_CNODE_RADIX_BITS;
        let child_cnode = self.env.alloc_cnode(child_depth).map_err(HalError::Sel4)?;
        let tcb = self.env.alloc_tcb().map_err(HalError::Sel4)?;
        let command_endpoint_origin = self.env.alloc_endpoint().map_err(HalError::Sel4)?;
        let notification = self.env.alloc_notification().map_err(HalError::Sel4)?;
        let ipc_frame = self
            .env
            .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
            .map_err(HalError::Sel4)?;
        let mut ring_frame = self
            .env
            .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Uncached)
            .map_err(HalError::Sel4)?;
        let stack_frame = self
            .env
            .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
            .map_err(HalError::Sel4)?;
        ring_frame.as_mut_slice().fill(0);

        let mcs = allocate_driver_task_mcs_objects(
            &mut self.env,
            contract,
            task_key,
            child_cnode,
            child_depth,
            command_endpoint_origin,
            fault_endpoint,
        )?;
        let command_endpoint = mcs.command_endpoint;

        #[cfg(not(sel4_config_kernel_mcs))]
        {
            let badge = 0xD000 | (role_bit as seL4_Word);
            let fault_err = sel4::cnode_mint_depth(
                child_cnode,
                driver_task::DRIVER_TASK_CHILD_FAULT_SLOT,
                child_depth,
                root_cnode,
                fault_endpoint,
                root_depth,
                sel4_sys::seL4_CapRights_All,
                badge,
            );
            if fault_err != seL4_NoError {
                return Err(HalError::Sel4(fault_err));
            }
        }

        let endpoint_err = sel4::cnode_mint_depth(
            child_cnode,
            driver_task::DRIVER_TASK_CHILD_COMMAND_SLOT,
            child_depth,
            root_cnode,
            command_endpoint_origin,
            root_depth,
            driver_runtime_command_endpoint_receive_rights(),
            0,
        );
        if endpoint_err != seL4_NoError {
            return Err(HalError::Sel4(endpoint_err));
        }
        driver_task::publish_driver_task_command_endpoint(contract, command_endpoint as usize);
        driver_task::publish_driver_task_ring(contract, ring_frame.ptr().as_ptr() as usize);

        let notification_err = sel4::cnode_mint_depth(
            child_cnode,
            driver_task::DRIVER_TASK_CHILD_NOTIFICATION_SLOT,
            child_depth,
            root_cnode,
            notification,
            root_depth,
            driver_runtime_local_notification_receive_rights(),
            0,
        );
        if notification_err != seL4_NoError {
            return Err(HalError::Sel4(notification_err));
        }
        let guard_bits = sel4::word_bits().saturating_sub(child_depth as seL4_Word);
        let cspace_root_data = sel4::cap_data_guard(0, guard_bits);
        #[cfg(sel4_config_kernel_mcs)]
        let tcb_fault_endpoint = mcs.standard_fault_endpoint;
        #[cfg(not(sel4_config_kernel_mcs))]
        let tcb_fault_endpoint = driver_task::DRIVER_TASK_CHILD_FAULT_SLOT;
        sel4::set_tcb_space(
            tcb,
            tcb_fault_endpoint,
            child_cnode,
            cspace_root_data,
            sel4_sys::seL4_CapInitThreadVSpace,
            0,
        )
        .map_err(HalError::Sel4)?;

        let ipc_vaddr = ipc_frame.ptr().as_ptr() as usize;
        self.env
            .bind_child_ipc_buffer(tcb, ipc_frame.cap(), ipc_vaddr)
            .map_err(HalError::Sel4)?;

        #[cfg(not(sel4_config_kernel_mcs))]
        let affinity_core = apply_driver_tcb_affinity_for_boot(contract, tcb)?;

        let (bootstrap_priority, steady_priority) = configure_driver_tcb_priority_for_boot(
            contract,
            tcb,
            mcs,
            DriverTaskTcbBootState::Active,
        )?;
        #[cfg(sel4_config_kernel_mcs)]
        let affinity_core = apply_driver_tcb_affinity_for_boot(contract, tcb)?;
        driver_task::publish_driver_task_scheduler(contract, tcb as usize, steady_priority);
        #[cfg(sel4_config_kernel_mcs)]
        if !driver_task::publish_driver_task_mcs_kernel_objects(
            contract,
            command_endpoint_origin as usize,
            mcs.command_reply as usize,
            mcs.completion_notification_origin as usize,
            mcs.sched_context as usize,
            mcs.standard_fault_endpoint as usize,
            mcs.timeout_fault_endpoint as usize,
        ) {
            return Err(HalError::Unsupported("driver-runtime-mcs-recovery-publish"));
        }
        #[cfg(sel4_config_kernel_mcs)]
        register_driver_task_fault_source(contract, tcb)?;
        #[cfg(sel4_config_kernel_mcs)]
        install_driver_task_supervisor_authority(contract, tcb, command_endpoint_origin, mcs)?;

        let _notification_bound =
            bind_driver_tcb_notification_for_boot(contract, tcb, notification)?;

        let stack_top = (stack_frame.ptr().as_ptr() as usize + (1usize << sel4::PAGE_BITS)) & !0xf;
        sel4::write_tcb_registers(
            tcb,
            driver_task::driver_task_entry as *const () as usize,
            stack_top,
            task_key as seL4_Word,
            true,
        )
        .map_err(HalError::Sel4)?;
        emit_driver_tcb_resume_return(contract, tcb, "write-registers");
        let started = driver_task::wait_for_driver_task_start(task_key, 256);
        restore_driver_tcb_steady_priority(contract, tcb, bootstrap_priority, steady_priority)?;
        let affinity_core =
            apply_driver_tcb_affinity_after_bootstrap(contract, tcb, affinity_core)?;

        let root_notification = if driver_runtime_needs_root_notification(contract) {
            let slot = self.env.allocate_slot();
            let err = sel4::cnode_mint_depth(
                root_cnode,
                slot,
                root_depth,
                root_cnode,
                notification,
                root_depth,
                driver_runtime_root_notification_send_rights(),
                seL4_Word::from(DRIVER_RUNTIME_RESERVED_ROOT_BADGE),
            );
            if err != seL4_NoError {
                let _ = sel4::cnode_delete(root_cnode, slot, root_depth);
                return Err(HalError::Sel4(err));
            }
            slot
        } else {
            0
        };
        driver_task::publish_driver_task_root_notification(contract, root_notification as usize);
        Ok(KernelDriverTaskHandle {
            contract,
            role_bit,
            tcb,
            cnode: child_cnode,
            command_endpoint_origin,
            command_endpoint,
            command_reply: mcs.command_reply,
            completion_notification_origin: mcs.completion_notification_origin,
            completion_notification: mcs.completion_notification,
            sched_context: mcs.sched_context,
            standard_fault_endpoint: mcs.standard_fault_endpoint,
            timeout_fault_endpoint: mcs.timeout_fault_endpoint,
            notification,
            root_notification,
            root_wake_notification: 0,
            fault_slot: driver_task::DRIVER_TASK_CHILD_FAULT_SLOT,
            ipc_frame: ipc_frame.cap(),
            stack_frame: stack_frame.cap(),
            ring_frame: Some(ring_frame.cap()),
            vspace: None,
            code_frame: None,
            runtime_irqs: [None; DRIVER_RUNTIME_IRQ_BINDING_CAPACITY],
            reciprocal_link_caps: [None; 2],
            runtime_image_spec,
            runtime_image_declared_region_mask: runtime_image_declared_region_mask(
                runtime_image_spec,
            ),
            runtime_image_mapped_region_mask: 0,
            runtime_image_acceptance_eligible: runtime_image_acceptance_eligible(
                runtime_image_spec,
            ),
            runtime_image_non_acceptance_reason: runtime_image_non_acceptance_reason(
                runtime_image_spec,
            ),
            code_vaddr: 0,
            ipc_vaddr,
            ring_vaddr: ring_frame.ptr().as_ptr() as usize,
            stack_top,
            affinity_core,
            vspace_isolated: false,
            pointer_free_ipc: driver_task::CURRENT_DRIVER_TASK_IPC_ABI.is_pointer_free(),
            started,
        })
    }

    fn map_isolated_runtime_declared_regions(
        &mut self,
        spec: Option<driver_task::DriverTaskRuntimeImageSpec>,
        vspace: seL4_CPtr,
        tracker: &mut VSpaceTableTracker,
        mut init_descriptor: Option<&mut RuntimeInitDescriptorBuilder>,
    ) -> Result<u16, HalError> {
        let Some(spec) = spec else {
            return Ok(0);
        };
        if !driver_task::physical_pi_driver_task_only_owner_state_active() {
            return Ok(driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK);
        }

        let mut mapped_mask = driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK;
        for region in spec.regions.iter().flatten().copied() {
            match region.kind {
                driver_task::DriverTaskRuntimeRegionKind::Code
                | driver_task::DriverTaskRuntimeRegionKind::Stack
                | driver_task::DriverTaskRuntimeRegionKind::Ipc
                | driver_task::DriverTaskRuntimeRegionKind::Ring => {}
                driver_task::DriverTaskRuntimeRegionKind::Mmio => {
                    if self.map_isolated_runtime_mmio_region(
                        spec.hot_path,
                        region,
                        vspace,
                        tracker,
                        init_descriptor.as_deref_mut(),
                    )? {
                        mapped_mask |= region.kind.mask_bit();
                    }
                }
                driver_task::DriverTaskRuntimeRegionKind::Dma => {
                    if self.map_isolated_runtime_ram_region(
                        spec.hot_path,
                        region,
                        vspace,
                        tracker,
                        true,
                        init_descriptor.as_deref_mut(),
                    )? {
                        mapped_mask |= region.kind.mask_bit();
                    }
                }
                driver_task::DriverTaskRuntimeRegionKind::SharedBuffer => {
                    if self.map_isolated_runtime_ram_region(
                        spec.hot_path,
                        region,
                        vspace,
                        tracker,
                        false,
                        init_descriptor.as_deref_mut(),
                    )? {
                        mapped_mask |= region.kind.mask_bit();
                    }
                }
            }
        }
        if spec.hot_path == driver_task::DriverTaskHotPath::HdmiText {
            let _ = self.map_isolated_runtime_hdmi_framebuffer(
                vspace,
                tracker,
                init_descriptor.as_deref_mut(),
            )?;
        }
        Ok(mapped_mask)
    }

    fn map_runtime_elf_image(
        &mut self,
        image: &[u8],
        plan: RuntimeElfLoadPlan,
        vspace: seL4_CPtr,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<RuntimeElfLoad, HalError> {
        let page_bytes = 1usize << sel4::PAGE_BITS;
        for page_index in 0..plan.page_count {
            let mut frame = self
                .env
                .alloc_dma_frame_attr(runtime_cacheable_xn_attributes())
                .map_err(HalError::Sel4)?;
            let fill = fill_runtime_elf_page(image, plan, page_index, frame.as_mut_slice())?;
            crate::hal::cache::cache_clean(
                sel4_sys::seL4_CapInitThreadVSpace,
                frame.ptr().as_ptr() as usize,
                page_bytes,
            )
            .map_err(|err| HalError::Sel4(err.code()))?;
            let vaddr = plan
                .base_vaddr
                .checked_add(page_index.saturating_mul(page_bytes))
                .ok_or(HalError::Unsupported("driver-runtime-elf-map-vaddr"))?;
            let mapping = runtime_elf_page_mapping(fill)?;
            self.env
                .unmap_page_cap(frame.cap())
                .map_err(HalError::Sel4)?;
            self.env
                .map_page_cap_into_vspace(
                    frame.cap(),
                    vspace,
                    vaddr,
                    mapping.rights,
                    mapping.attributes,
                    tracker,
                )
                .map_err(HalError::Sel4)?;
            if fill.executable {
                crate::hal::cache::cache_unify_instruction(vspace, vaddr, page_bytes)
                    .map_err(|err| HalError::Sel4(err.code()))?;
            }
        }
        Ok(RuntimeElfLoad {
            entry: plan.entry,
            code_vaddr: plan.base_vaddr,
            root_write_aliases_unmapped: true,
        })
    }

    fn map_isolated_runtime_hdmi_framebuffer(
        &mut self,
        vspace: seL4_CPtr,
        tracker: &mut VSpaceTableTracker,
        mut init_descriptor: Option<&mut RuntimeInitDescriptorBuilder>,
    ) -> Result<bool, HalError> {
        let Some(mut framebuffer) = driver_task::hdmi_runtime_framebuffer_hint() else {
            driver_task::emit_driver_task_resource_init_status(
                HDMI_TEXT_DRIVER_TASK_CONTRACT,
                driver_task::DriverTaskHotPath::HdmiText,
                "hdmi-framebuffer-map",
                "no-hint",
                None,
            );
            return Ok(false);
        };
        let paddr = usize::try_from(framebuffer.paddr)
            .map_err(|_| HalError::Unsupported("driver-runtime-hdmi-fb-paddr"))?;
        let width = framebuffer.width as usize;
        let height = framebuffer.height as usize;
        let pitch = framebuffer.pitch as usize;
        let Some(framebuffer_len) = pitch.checked_mul(height) else {
            return Err(HalError::Unsupported("driver-runtime-hdmi-fb-size"));
        };
        let page_bytes = 1usize << sel4::PAGE_BITS;
        let page_base = paddr & !(page_bytes - 1);
        let page_offset = paddr & (page_bytes - 1);
        let Some(map_len) = page_offset.checked_add(framebuffer_len) else {
            return Err(HalError::Unsupported("driver-runtime-hdmi-fb-map-len"));
        };
        let page_count = map_len.saturating_add(page_bytes - 1) / page_bytes;
        if page_count == 0 || width == 0 || page_count > DRIVER_TASK_CSPACE_MAX_FRAMEBUFFER_PAGES {
            return Err(HalError::Unsupported("driver-runtime-hdmi-fb-pages"));
        }
        let rights = sel4_sys::seL4_CapRights_ReadWrite;
        for page in 0..page_count {
            let paddr = page_base
                .checked_add(page.saturating_mul(page_bytes))
                .ok_or(HalError::Unsupported("driver-runtime-hdmi-fb-page"))?;
            let vaddr = (DRIVER_RUNTIME_FRAMEBUFFER_VADDR as usize)
                .checked_add(page.saturating_mul(page_bytes))
                .ok_or(HalError::Unsupported("driver-runtime-hdmi-fb-vaddr"))?;
            self.env
                .map_exclusive_device_page_into_vspace(
                    paddr,
                    vspace,
                    vaddr,
                    rights,
                    runtime_uncached_xn_attributes(),
                    tracker,
                )
                .map_err(HalError::Sel4)?;
        }
        framebuffer.vaddr = DRIVER_RUNTIME_FRAMEBUFFER_VADDR
            .checked_add(page_offset as u64)
            .ok_or(HalError::Unsupported("driver-runtime-hdmi-fb-vaddr"))?;
        if let Some(builder) = init_descriptor.as_deref_mut() {
            builder.set_framebuffer_region(framebuffer, page_base, map_len, page_count)?;
        }
        let mut line = heapless::String::<512>::new();
        let _ = fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "DRIVER_TASK_RESOURCE_INIT contract=hdmi-text hot_path=hdmi-text stage=hdmi-framebuffer-map status=ready acceptance=no owner=linked-runtime root_action=publish-descriptor blocker=none next_action=continue-next-driver-gate paddr=0x{paddr:016x} vaddr=0x{vaddr:016x} width={width} height={height} pitch={pitch} pages={page_count} map_len={map_len}",
                paddr = paddr,
                vaddr = framebuffer.vaddr,
                width = framebuffer.width,
                height = framebuffer.height,
                pitch = framebuffer.pitch,
                page_count = page_count,
                map_len = map_len,
            ),
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
        Ok(true)
    }

    fn map_isolated_runtime_mmio_region(
        &mut self,
        hot_path: driver_task::DriverTaskHotPath,
        region: driver_task::DriverTaskRuntimeRegion,
        vspace: seL4_CPtr,
        tracker: &mut VSpaceTableTracker,
        mut init_descriptor: Option<&mut RuntimeInitDescriptorBuilder>,
    ) -> Result<bool, HalError> {
        let pages = region.pages as usize;
        if pages == 0 {
            return Ok(false);
        }
        let page_bytes = 1usize << sel4::PAGE_BITS;
        let rights = sel4_sys::seL4_CapRights_ReadWrite;
        if hot_path == driver_task::DriverTaskHotPath::SdioHost && pages != 3 {
            return Err(HalError::Unsupported(
                "driver-runtime-sdio-mmio-page-budget",
            ));
        }
        if hot_path == driver_task::DriverTaskHotPath::SdioHost {
            for ((&sdhci_base, &pwrseq_base), &dma_base) in PI4_DRIVER_RUNTIME_SDIO_MMIO_BASES
                .iter()
                .zip(PI4_DRIVER_RUNTIME_WIFI_PWRSEQ_MMIO_BASES.iter())
                .zip(PI4_DRIVER_RUNTIME_BCM2835_DMA_MMIO_BASES.iter())
            {
                if !self
                    .env
                    .device_page_admitted_for_child_without_root_mapping(dma_base)
                {
                    return Err(HalError::Unsupported(
                        "driver-runtime-sdio-dma-mmio-pre-admission-missing",
                    ));
                }
                if !runtime_candidate_covers_pages(&self.env, sdhci_base, 1)
                    || !runtime_candidate_covers_pages(&self.env, pwrseq_base, 1)
                    || !runtime_candidate_covers_pages(&self.env, dma_base, 1)
                {
                    continue;
                }
                // The pinned BCM2711 device tree admits channels 0, 2, and
                // 4-10 through mask 0x07f5. Channels 0/2/3 have special-use
                // restrictions, while the binding identifies 1/3/6/7 as
                // firmware-used. Channel 4 is therefore the lowest admitted
                // ordinary channel for this sole Cohesix DMA owner.
                // Its 0x100-byte register bank begins at page offset 0x400.
                let dma_channel_bit = 1u16
                    .checked_shl(PI4_DRIVER_RUNTIME_BCM2835_DMA_CHANNEL as u32)
                    .ok_or(HalError::Unsupported("driver-runtime-sdio-dma-channel"))?;
                let dma_channel_offset = PI4_DRIVER_RUNTIME_BCM2835_DMA_CHANNEL
                    .checked_mul(PI4_DRIVER_RUNTIME_BCM2835_DMA_CHANNEL_STRIDE)
                    .ok_or(HalError::Unsupported("driver-runtime-sdio-dma-channel"))?;
                if PI4_DRIVER_RUNTIME_BCM2835_DMA_AVAILABLE_CHANNEL_MASK & dma_channel_bit == 0
                    || dma_channel_offset
                        .checked_add(PI4_DRIVER_RUNTIME_BCM2835_DMA_CHANNEL_STRIDE)
                        .is_none_or(|end| end > page_bytes)
                {
                    return Err(HalError::Unsupported("driver-runtime-sdio-dma-channel"));
                }
                let first_page_index = init_descriptor
                    .as_deref()
                    .map(|builder| builder.descriptor.mmio_page_count)
                    .unwrap_or(0);
                for (page, paddr) in [sdhci_base, pwrseq_base, dma_base].into_iter().enumerate() {
                    let vaddr = runtime_region_page_vaddr(region, page)
                        .ok_or(HalError::Unsupported("driver-runtime-mmio-vaddr"))?;
                    if page == 2 {
                        self.env
                            .map_admitted_device_page_exclusively_into_vspace(
                                paddr,
                                vspace,
                                vaddr,
                                rights,
                                runtime_uncached_xn_attributes(),
                                tracker,
                            )
                            .map_err(HalError::Sel4)?;
                    } else {
                        self.env
                            .map_device_page_into_vspace(
                                paddr,
                                vspace,
                                vaddr,
                                rights,
                                runtime_uncached_xn_attributes(),
                                tracker,
                            )
                            .map_err(HalError::Sel4)?;
                    }
                    if let Some(builder) = init_descriptor.as_deref_mut() {
                        builder.add_mmio_page(paddr)?;
                    }
                }
                if let Some(builder) = init_descriptor.as_deref_mut() {
                    builder.add_tagged_mmio_resource_range(
                        DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
                        region.vaddr,
                        sdhci_base,
                        1,
                        first_page_index,
                    )?;
                    builder.add_tagged_mmio_resource_range(
                        DRIVER_RUNTIME_RESOURCE_TAG_WIFI_PWRSEQ,
                        region
                            .vaddr
                            .checked_add(page_bytes)
                            .ok_or(HalError::Unsupported("driver-runtime-mmio-vaddr"))?,
                        pwrseq_base,
                        1,
                        first_page_index.saturating_add(1),
                    )?;
                    builder.add_tagged_mmio_resource_range(
                        DRIVER_RUNTIME_RESOURCE_TAG_BCM2835_DMA,
                        region
                            .vaddr
                            .checked_add(page_bytes.saturating_mul(2))
                            .ok_or(HalError::Unsupported("driver-runtime-mmio-vaddr"))?,
                        dma_base,
                        1,
                        first_page_index.saturating_add(2),
                    )?;
                }
                return Ok(true);
            }
            return Err(HalError::Unsupported(
                "driver-runtime-sdio-dma-mmio-not-covered",
            ));
        }
        if hot_path == driver_task::DriverTaskHotPath::PcieRoot {
            if pages != PI4_DRIVER_RUNTIME_PCIE_TOTAL_MMIO_PAGES {
                return Err(HalError::Unsupported(
                    "driver-runtime-pcie-timer-mmio-page-budget",
                ));
            }
            if !runtime_candidate_covers_pages(
                &self.env,
                PI4_DRIVER_RUNTIME_SYSTEM_TIMER_PADDR,
                PI4_DRIVER_RUNTIME_PCIE_TIMER_MMIO_PAGES,
            ) {
                return Err(HalError::Unsupported(
                    "driver-runtime-pcie-timer-mmio-not-covered",
                ));
            }
            for &host_base in PI4_DRIVER_RUNTIME_PCIE_MMIO_BASES {
                if !runtime_candidate_covers_pages(
                    &self.env,
                    host_base,
                    PI4_DRIVER_RUNTIME_PCIE_HOST_MMIO_PAGES,
                ) {
                    continue;
                }
                let first_page_index = init_descriptor
                    .as_deref()
                    .map(|builder| builder.descriptor.mmio_page_count)
                    .unwrap_or(0);
                for page in 0..PI4_DRIVER_RUNTIME_PCIE_HOST_MMIO_PAGES {
                    let paddr = host_base
                        .checked_add(page.saturating_mul(page_bytes))
                        .ok_or(HalError::Unsupported("driver-runtime-pcie-mmio-paddr"))?;
                    let vaddr = runtime_region_page_vaddr(region, page)
                        .ok_or(HalError::Unsupported("driver-runtime-pcie-mmio-vaddr"))?;
                    self.env
                        .map_device_page_into_vspace(
                            paddr,
                            vspace,
                            vaddr,
                            rights,
                            runtime_uncached_xn_attributes(),
                            tracker,
                        )
                        .map_err(HalError::Sel4)?;
                    if let Some(builder) = init_descriptor.as_deref_mut() {
                        builder.add_mmio_page(paddr)?;
                    }
                }
                let timer_page = PI4_DRIVER_RUNTIME_PCIE_HOST_MMIO_PAGES;
                let timer_vaddr = runtime_region_page_vaddr(region, timer_page)
                    .ok_or(HalError::Unsupported("driver-runtime-pcie-timer-vaddr"))?;
                self.env
                    .map_device_page_into_vspace(
                        PI4_DRIVER_RUNTIME_SYSTEM_TIMER_PADDR,
                        vspace,
                        timer_vaddr,
                        rights,
                        runtime_uncached_xn_attributes(),
                        tracker,
                    )
                    .map_err(HalError::Sel4)?;
                if let Some(builder) = init_descriptor.as_deref_mut() {
                    builder.add_mmio_page(PI4_DRIVER_RUNTIME_SYSTEM_TIMER_PADDR)?;
                    builder.add_tagged_mmio_resource_range(
                        DRIVER_RUNTIME_RESOURCE_TAG_PCIE_HOST,
                        region.vaddr,
                        host_base,
                        PI4_DRIVER_RUNTIME_PCIE_HOST_MMIO_PAGES,
                        first_page_index,
                    )?;
                    builder.add_tagged_mmio_resource_range(
                        DRIVER_RUNTIME_RESOURCE_TAG_PI4_SYSTEM_TIMER,
                        timer_vaddr,
                        PI4_DRIVER_RUNTIME_SYSTEM_TIMER_PADDR,
                        PI4_DRIVER_RUNTIME_PCIE_TIMER_MMIO_PAGES,
                        first_page_index
                            .saturating_add(PI4_DRIVER_RUNTIME_PCIE_HOST_MMIO_PAGES as u16),
                    )?;
                }
                return Ok(true);
            }
            return Err(HalError::Unsupported(
                "driver-runtime-pcie-host-mmio-not-covered",
            ));
        }
        for &base in runtime_mmio_candidate_bases(hot_path) {
            if !runtime_candidate_covers_pages(&self.env, base, pages) {
                continue;
            }
            let first_page_index = init_descriptor
                .as_deref()
                .map(|builder| builder.descriptor.mmio_page_count)
                .unwrap_or(0);
            for page in 0..pages {
                let paddr = base
                    .checked_add(page.saturating_mul(page_bytes))
                    .ok_or(HalError::Unsupported("driver-runtime-mmio-paddr"))?;
                let vaddr = runtime_region_page_vaddr(region, page)
                    .ok_or(HalError::Unsupported("driver-runtime-mmio-vaddr"))?;
                self.env
                    .map_device_page_into_vspace(
                        paddr,
                        vspace,
                        vaddr,
                        rights,
                        runtime_uncached_xn_attributes(),
                        tracker,
                    )
                    .map_err(HalError::Sel4)?;
                if let Some(builder) = init_descriptor.as_deref_mut() {
                    builder.add_mmio_page(paddr)?;
                }
            }
            if let Some(builder) = init_descriptor.as_deref_mut() {
                builder.add_mmio_resource_range(
                    hot_path,
                    region.vaddr,
                    base,
                    pages,
                    first_page_index,
                )?;
            }
            return Ok(true);
        }
        Err(HalError::Unsupported("driver-runtime-mmio-not-covered"))
    }

    fn runtime_ram_region_attr(
        hot_path: driver_task::DriverTaskHotPath,
        dma_owned: bool,
    ) -> sel4_sys::seL4_ARM_VMAttributes {
        // Serial and GENET shared pages are CPU-to-CPU SPSC memory whose
        // cursors use AArch64 atomic acquire/release operations. The selected
        // seL4 AArch64 kernel maps Page_Uncached as Device-nGnRnE, where Rust
        // atomics/exclusive accesses are not an admissible synchronization
        // primitive. Give exactly these CPU-only pages coherent Normal-memory
        // aliases in every participant. GENET MMIO/DMA pages and every other
        // shared driver payload retain the uncached device boundary.
        if !dma_owned
            && matches!(
                hot_path,
                driver_task::DriverTaskHotPath::SerialConsole
                    | driver_task::DriverTaskHotPath::GenetNic
            )
        {
            runtime_cacheable_xn_attributes()
        } else {
            runtime_uncached_xn_attributes()
        }
    }

    fn map_isolated_runtime_ram_region(
        &mut self,
        hot_path: driver_task::DriverTaskHotPath,
        region: driver_task::DriverTaskRuntimeRegion,
        vspace: seL4_CPtr,
        tracker: &mut VSpaceTableTracker,
        dma_owned: bool,
        mut init_descriptor: Option<&mut RuntimeInitDescriptorBuilder>,
    ) -> Result<bool, HalError> {
        let pages = region.pages as usize;
        if pages == 0 {
            return Ok(false);
        }
        let sdio_dma_owned = dma_owned && hot_path == driver_task::DriverTaskHotPath::SdioHost;
        if sdio_dma_owned && pages != PI4_SDIO_DMA_PRIVATE_PAGE_COUNT {
            return Err(HalError::Unsupported("driver-runtime-sdio-dma-page-budget"));
        }
        let rights = sel4_sys::seL4_CapRights_ReadWrite;
        // DMA and device-facing payloads cross boundaries without runtime-side
        // EL0 cache maintenance. The CPU-only serial SPSC and direct GENET
        // regions use coherent Normal memory so their atomic cursor protocols
        // are valid; neither exception grants device DMA authority.
        let attr = Self::runtime_ram_region_attr(hot_path, dma_owned);
        let page_bytes = 1usize << sel4::PAGE_BITS;
        let first_page_index = init_descriptor
            .as_deref()
            .map(|builder| {
                if dma_owned {
                    builder.descriptor.dma_page_count
                } else {
                    builder.descriptor.shared_page_count
                }
            })
            .unwrap_or(0);
        let mut first_paddr = 0usize;
        let mut paddr_contiguous = true;
        let mut sdio_dma_arena_first_paddr = 0usize;
        let mut sdio_dma_arena_paddr_contiguous = true;
        if dma_owned {
            let mut frames: AllocVec<UnmappedRamFrame> = AllocVec::new();
            frames
                .try_reserve_exact(pages)
                .map_err(|_| HalError::Unsupported("driver-runtime-dma-plan-oom"))?;
            for page in 0..pages {
                let _ = runtime_region_page_vaddr(region, page)
                    .ok_or(HalError::Unsupported("driver-runtime-buffer-vaddr"))?;
                let frame = if sdio_dma_owned {
                    self.env
                        .alloc_unmapped_ram_frame_low_attr(attr)
                        .map_err(HalError::Sel4)?
                } else {
                    self.env
                        .alloc_unmapped_ram_frame_attr(attr)
                        .map_err(HalError::Sel4)?
                };
                let paddr = frame.paddr();
                if sdio_dma_owned && (paddr > 0x3fff_ffff || paddr & 0xf != 0) {
                    return Err(HalError::Unsupported("driver-runtime-sdio-dma-address"));
                }
                if page == 0 {
                    first_paddr = paddr;
                } else if paddr_contiguous
                    && !runtime_region_paddr_is_contiguous(first_paddr, page, page_bytes, paddr)
                {
                    paddr_contiguous = false;
                }
                if sdio_dma_owned {
                    if page == 1 {
                        sdio_dma_arena_first_paddr = paddr;
                    } else if page > 1
                        && sdio_dma_arena_paddr_contiguous
                        && !runtime_region_paddr_is_contiguous(
                            sdio_dma_arena_first_paddr,
                            page - 1,
                            page_bytes,
                            paddr,
                        )
                    {
                        sdio_dma_arena_paddr_contiguous = false;
                    }
                }
                frames.push(frame);
            }

            let mut mapped_frames = 0usize;
            for (page, frame) in frames.iter().enumerate() {
                let vaddr = runtime_region_page_vaddr(region, page)
                    .ok_or(HalError::Unsupported("driver-runtime-buffer-vaddr"))?;
                if let Err(err) = self.env.map_page_cap_into_vspace(
                    frame.cap(),
                    vspace,
                    vaddr,
                    rights,
                    attr,
                    tracker,
                ) {
                    for mapped in frames.iter().take(mapped_frames) {
                        let _ = self.env.unmap_page_cap(mapped.cap());
                    }
                    return Err(HalError::Sel4(err));
                }
                mapped_frames = mapped_frames.saturating_add(1);
            }

            if let Some(builder) = init_descriptor.as_deref_mut() {
                for frame in &frames {
                    builder.add_dma_page(frame.paddr())?;
                }
            }
        } else {
            for page in 0..pages {
                let vaddr = runtime_region_page_vaddr(region, page)
                    .ok_or(HalError::Unsupported("driver-runtime-buffer-vaddr"))?;
                let frame = self
                    .env
                    .alloc_dma_frame_attr(attr)
                    .map_err(HalError::Sel4)?;
                let paddr = frame.paddr();
                if page == 0 {
                    first_paddr = paddr;
                }
                self.env
                    .map_page_copy_into_vspace(frame.cap(), vspace, vaddr, rights, attr, tracker)
                    .map_err(HalError::Sel4)?;
                driver_task::publish_driver_task_shared_frame(
                    hot_path.contract(),
                    page,
                    frame.cap() as usize,
                    frame.ptr().as_ptr() as usize,
                );
                if let Some(builder) = init_descriptor.as_deref_mut() {
                    builder.add_shared_page(paddr)?;
                }
            }
        }
        if let Some(builder) = init_descriptor.as_deref_mut() {
            if sdio_dma_owned {
                builder.add_tagged_dma_resource_range(
                    DRIVER_RUNTIME_RESOURCE_TAG_WIFI_PWRSEQ_REQUEST,
                    region.vaddr,
                    first_paddr,
                    1,
                    first_page_index,
                    true,
                )?;
                builder.add_tagged_dma_resource_range(
                    DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
                    region
                        .vaddr
                        .checked_add(page_bytes)
                        .ok_or(HalError::Unsupported("driver-runtime-buffer-vaddr"))?,
                    sdio_dma_arena_first_paddr,
                    pages - 1,
                    first_page_index
                        .checked_add(1)
                        .ok_or(HalError::Unsupported("driver-runtime-init-dma-range-index"))?,
                    sdio_dma_arena_paddr_contiguous,
                )?;
            } else {
                builder.add_buffer_resource_range(
                    hot_path,
                    if dma_owned {
                        DRIVER_RUNTIME_RESOURCE_KIND_DMA
                    } else {
                        DRIVER_RUNTIME_RESOURCE_KIND_SHARED
                    },
                    region.vaddr,
                    first_paddr,
                    pages,
                    first_page_index,
                    paddr_contiguous,
                )?;
            }
        }
        Ok(true)
    }

    fn install_cyw43_sdio_bus_link(
        &mut self,
        contract: DriverTaskContract,
        child_cnode: seL4_CPtr,
        child_depth: u8,
        client_notification: seL4_CPtr,
        vspace: seL4_CPtr,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<ReciprocalLinkCapGuard, HalError> {
        if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT {
            return Ok(ReciprocalLinkCapGuard::empty());
        }
        let topology = generated_cyw43_sdio_topology()?;
        let owner = self
            .driver_tasks
            .iter()
            .find(|handle| handle.contract == SDIO_HOST_DRIVER_TASK_CONTRACT)
            .copied()
            .ok_or(HalError::Unsupported(
                "driver-runtime-sdio-owner-handle-missing",
            ))?;
        let (_sdio_endpoint, sdio_ring_frame, sdio_shared_frames) =
            driver_task::driver_task_bus_owner_transport_caps_with_shared(
                SDIO_HOST_DRIVER_TASK_CONTRACT,
                driver_task::DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY,
            )
            .ok_or(HalError::Unsupported(
                "driver-runtime-sdio-bus-link-missing",
            ))?;
        let root_cnode = self.env.init_cnode_cap();
        let root_depth = sel4::word_bits() as u8;
        let send_only = sel4_sys::seL4_CapRights::new(0, 0, 0, 1);
        let mut installed = ReciprocalLinkCapGuard::empty();
        let client_to_owner_slot = seL4_CPtr::from(topology.link.client_to_owner_slot);
        let client_to_owner_badge =
            cyw43_sdio_peer_notification_badge(u32::from(topology.link.client_to_owner_slot))
                .ok_or(HalError::Unsupported(
                    "driver-runtime-sdio-owner-notification-badge",
                ))?;
        let client_to_owner_err = sel4::cnode_mint_depth(
            child_cnode,
            client_to_owner_slot,
            child_depth,
            root_cnode,
            owner.notification,
            root_depth,
            send_only,
            seL4_Word::from(client_to_owner_badge),
        );
        if client_to_owner_err != seL4_NoError {
            return Err(HalError::Sel4(client_to_owner_err));
        }
        installed.push(InstalledChildCap {
            cnode: child_cnode,
            slot: client_to_owner_slot,
            depth: child_depth,
        })?;
        let owner_to_client_slot = seL4_CPtr::from(topology.link.owner_to_client_slot);
        let owner_to_client_badge =
            cyw43_sdio_peer_notification_badge(u32::from(topology.link.owner_to_client_slot))
                .ok_or(HalError::Unsupported(
                    "driver-runtime-cyw43-completion-notification-badge",
                ))?;
        let owner_to_client_err = sel4::cnode_mint_depth(
            owner.cnode,
            owner_to_client_slot,
            child_depth,
            root_cnode,
            client_notification,
            root_depth,
            send_only,
            seL4_Word::from(owner_to_client_badge),
        );
        if owner_to_client_err != seL4_NoError {
            return Err(HalError::Sel4(owner_to_client_err));
        }
        installed.push(InstalledChildCap {
            cnode: owner.cnode,
            slot: owner_to_client_slot,
            depth: child_depth,
        })?;
        self.env
            .map_page_copy_into_vspace(
                sdio_ring_frame,
                vspace,
                driver_task::DRIVER_TASK_SDIO_BUS_RING_VADDR,
                sel4_sys::seL4_CapRights_ReadWrite,
                runtime_uncached_xn_attributes(),
                tracker,
            )
            .map_err(HalError::Sel4)?;
        let page_bytes = 1usize << sel4::PAGE_BITS;
        for (page, frame) in sdio_shared_frames.iter().enumerate() {
            let vaddr = driver_task::DRIVER_TASK_SDIO_BUS_RING_VADDR
                .checked_add(driver_task::DRIVER_TASK_RING_PAGE_BYTES)
                .and_then(|base| base.checked_add(page.saturating_mul(page_bytes)))
                .ok_or(HalError::Unsupported("driver-runtime-sdio-bus-link-vaddr"))?;
            self.env
                .map_page_copy_into_vspace(
                    *frame,
                    vspace,
                    vaddr,
                    sel4_sys::seL4_CapRights_ReadWrite,
                    runtime_uncached_xn_attributes(),
                    tracker,
                )
                .map_err(HalError::Sel4)?;
        }
        let line = format_cyw43_sdio_bus_link_diagnostic(
            contract,
            topology,
            client_to_owner_badge,
            owner_to_client_badge,
        )?;
        crate::bootstrap::log::force_uart_line(line.as_str());
        Ok(installed)
    }

    fn install_usb_pcie_bus_link(
        &mut self,
        contract: DriverTaskContract,
        child_cnode: seL4_CPtr,
        child_depth: u8,
        vspace: seL4_CPtr,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<(), HalError> {
        if contract != USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT {
            return Ok(());
        }
        let (pcie_endpoint, pcie_ring_frame) =
            driver_task::driver_task_bus_owner_transport_caps(PCIE_ROOT_DRIVER_TASK_CONTRACT)
                .ok_or(HalError::Unsupported(
                    "driver-runtime-pcie-bus-link-missing",
                ))?;
        let root_cnode = self.env.init_cnode_cap();
        let root_depth = sel4::word_bits() as u8;
        let pcie_command_badge = pi4_driver_abi::driver_runtime_command_badge(
            driver_task::DRIVER_TASK_KEY_PCIE_ROOT as u32,
        );
        let mut cap_line = heapless::String::<256>::new();
        fmt::write(
            &mut cap_line,
            format_args!(
                "DRIVER_TASK_BUS_LINK_CAP channel=usb-pcie operation=copy-preserve-badge source_cptr=0x{:04x} expected_badge=0x{:016x} dest_slot=0x{:04x} rights=write+grantreply status=begin",
                pcie_endpoint,
                pcie_command_badge,
                driver_task::DRIVER_TASK_CHILD_PCIE_BUS_ENDPOINT_SLOT,
            ),
        )
        .map_err(|_| HalError::Unsupported("driver-runtime-usb-pcie-cap-log-overflow"))?;
        crate::bootstrap::log::force_uart_line(cap_line.as_str());
        // The MCS owner publishes an already-badged command cap. Copy preserves
        // that identity while applying the same least-authority rights filter;
        // minting again would ask seL4 to mutate a nonzero endpoint badge.
        let endpoint_err = sel4::cnode_copy_depth(
            child_cnode,
            driver_task::DRIVER_TASK_CHILD_PCIE_BUS_ENDPOINT_SLOT,
            child_depth,
            root_cnode,
            pcie_endpoint,
            root_depth,
            driver_runtime_command_endpoint_send_rights(),
        );
        if endpoint_err != seL4_NoError {
            return Err(HalError::Sel4(endpoint_err));
        }
        crate::bootstrap::log::force_uart_line(
            "DRIVER_TASK_BUS_LINK_CAP channel=usb-pcie operation=copy-preserve-badge dest_slot=0x0009 rights=write+grantreply status=ready",
        );
        self.env
            .map_page_copy_into_vspace(
                pcie_ring_frame,
                vspace,
                driver_task::DRIVER_TASK_PCIE_BUS_RING_VADDR,
                sel4_sys::seL4_CapRights_ReadWrite,
                runtime_uncached_xn_attributes(),
                tracker,
            )
            .map_err(HalError::Sel4)?;
        crate::bootstrap::log::force_uart_line(
            "DRIVER_TASK_BUS_LINK contract=usb-keyboard owner=pcie-root channel=usb-pcie endpoint_slot=0x0009 ring_vaddr=0x70e01000",
        );
        Ok(())
    }

    fn create_isolated_driver_task(
        &mut self,
        contract: DriverTaskContract,
        fault_endpoint: seL4_CPtr,
    ) -> Result<KernelDriverTaskHandle, HalError> {
        contract.validate().map_err(HalError::DriverTaskContract)?;

        let role_bit = driver_task::driver_task_role_bit(contract.kind);
        if role_bit == 0 {
            return Err(HalError::Unsupported("driver-task-role"));
        }
        let task_key = driver_task::driver_task_contract_key(contract)
            .ok_or(HalError::Unsupported("driver-task-key"))?;
        let runtime_image_spec =
            driver_task::pi4_driver_task_runtime_image_spec_for_contract(contract);
        let root_wake_route = runtime_image_spec
            .map(driver_runtime_root_wake_route)
            .transpose()?
            .flatten();

        let page_bytes = 1usize << sel4::PAGE_BITS;
        let linked_runtime_image = runtime_image_spec.and_then(|spec| {
            driver_task::physical_pi_driver_task_only_owner_state_active()
                .then(|| driver_task::driver_runtime_image_bytes(spec.hot_path))
                .flatten()
        });
        let root_control_wake_required = linked_runtime_image.is_some();
        if driver_task::physical_pi_driver_task_only_owner_state_active()
            && runtime_image_spec.is_some()
            && linked_runtime_image.is_none()
        {
            return Err(HalError::Unsupported("driver-runtime-image-missing"));
        }
        if linked_runtime_image.is_none() && !driver_task::isolated_trampoline_supported() {
            return Err(HalError::Unsupported("driver-task-isolated-trampoline"));
        }
        let trampoline_range = driver_task::isolated_trampoline_range();
        if linked_runtime_image.is_none()
            && (trampoline_range.start == 0
                || trampoline_range.end <= trampoline_range.start
                || trampoline_range.start & (page_bytes - 1) != 0
                || trampoline_range.end - trampoline_range.start > page_bytes)
        {
            return Err(HalError::Unsupported("driver-task-trampoline-layout"));
        }
        let linked_runtime_plan =
            if let (Some(image), Some(spec)) = (linked_runtime_image, runtime_image_spec) {
                Some(plan_runtime_elf_load(
                    image,
                    spec.region_pages(driver_task::DriverTaskRuntimeRegionKind::Code),
                )?)
            } else {
                None
            };

        let root_cnode = self.env.init_cnode_cap();
        let root_depth = sel4::word_bits() as u8;
        let child_depth = driver_task::DRIVER_TASK_CHILD_CNODE_RADIX_BITS;
        let child_cnode = self.env.alloc_cnode(child_depth).map_err(HalError::Sel4)?;
        let tcb = self.env.alloc_tcb().map_err(HalError::Sel4)?;
        let command_endpoint_origin = self.env.alloc_endpoint().map_err(HalError::Sel4)?;
        // Recovery keeps one private root cap that is never published as a
        // steady producer. Under MCS it must carry the exact same task-key
        // command badge and send-only rights as the ordinary command cap:
        // restarted runtimes accept their first sequence-last ring turn
        // without IPC, but every retained continuation validates that badge.
        // The normal SDIO handoff still deletes its ordinary root send cap;
        // this cap is admitted only while both linked peers are suspended by
        // the HAL pair-restart supervisor.
        let recovery_endpoint = if contract == CYW43_WIFI_DRIVER_TASK_CONTRACT
            || contract == SDIO_HOST_DRIVER_TASK_CONTRACT
        {
            #[cfg(sel4_config_kernel_mcs)]
            let endpoint = mint_driver_runtime_command_endpoint(
                &mut self.env,
                command_endpoint_origin,
                task_key,
            )?;
            #[cfg(not(sel4_config_kernel_mcs))]
            let endpoint = self
                .env
                .copy_cap_to_new_slot(command_endpoint_origin, sel4_sys::seL4_CapRights_All)
                .map_err(HalError::Sel4)?;
            Some(endpoint)
        } else {
            None
        };
        let notification = self.env.alloc_notification().map_err(HalError::Sel4)?;
        let root_wake_notification = if root_wake_route.is_some() {
            self.env.alloc_notification().map_err(HalError::Sel4)?
        } else {
            0
        };
        let vspace = self.env.alloc_vspace_root().map_err(HalError::Sel4)?;
        self.env
            .assign_vspace_asid_from_init_pool(vspace)
            .map_err(HalError::Sel4)?;

        let mut ring_frame = self
            .env
            .alloc_dma_frame_attr(runtime_uncached_xn_attributes())
            .map_err(HalError::Sel4)?;
        let mut ipc_frame = self
            .env
            .alloc_dma_frame_attr(runtime_cacheable_xn_attributes())
            .map_err(HalError::Sel4)?;
        let stack_pages = runtime_image_stack_pages(runtime_image_spec);
        let stack_top = runtime_image_stack_top(runtime_image_spec)?;
        let mut stack_frames: AllocVec<RamFrame> = AllocVec::new();
        stack_frames
            .try_reserve_exact(stack_pages)
            .map_err(|_| HalError::Unsupported("driver-runtime-stack-plan-oom"))?;
        for _ in 0..stack_pages {
            stack_frames.push(
                self.env
                    .alloc_dma_frame_attr(runtime_cacheable_xn_attributes())
                    .map_err(HalError::Sel4)?,
            );
        }

        ring_frame.as_mut_slice().fill(0);
        ipc_frame.as_mut_slice().fill(0);
        for stack_frame in &mut stack_frames {
            stack_frame.as_mut_slice().fill(0);
        }

        let mcs = allocate_driver_task_mcs_objects(
            &mut self.env,
            contract,
            task_key,
            child_cnode,
            child_depth,
            command_endpoint_origin,
            fault_endpoint,
        )?;
        let command_endpoint = mcs.command_endpoint;

        #[cfg(not(sel4_config_kernel_mcs))]
        {
            let badge = 0xD000 | (role_bit as seL4_Word);
            let fault_err = sel4::cnode_mint_depth(
                child_cnode,
                driver_task::DRIVER_TASK_CHILD_FAULT_SLOT,
                child_depth,
                root_cnode,
                fault_endpoint,
                root_depth,
                sel4_sys::seL4_CapRights_All,
                badge,
            );
            if fault_err != seL4_NoError {
                return Err(HalError::Sel4(fault_err));
            }
        }

        let endpoint_err = sel4::cnode_mint_depth(
            child_cnode,
            driver_task::DRIVER_TASK_CHILD_COMMAND_SLOT,
            child_depth,
            root_cnode,
            command_endpoint_origin,
            root_depth,
            driver_runtime_command_endpoint_receive_rights(),
            0,
        );
        if endpoint_err != seL4_NoError {
            return Err(HalError::Sel4(endpoint_err));
        }
        driver_task::publish_driver_task_command_endpoint(contract, command_endpoint as usize);
        driver_task::publish_driver_task_ring(contract, ring_frame.ptr().as_ptr() as usize);
        driver_task::publish_driver_task_ring_frame_cap(contract, ring_frame.cap() as usize);

        let notification_err = sel4::cnode_mint_depth(
            child_cnode,
            driver_task::DRIVER_TASK_CHILD_NOTIFICATION_SLOT,
            child_depth,
            root_cnode,
            notification,
            root_depth,
            driver_runtime_local_notification_receive_rights(),
            0,
        );
        if notification_err != seL4_NoError {
            return Err(HalError::Sel4(notification_err));
        }
        if let Some((slot, badge)) = root_wake_route {
            let root_wake_err = sel4::cnode_mint_depth(
                child_cnode,
                seL4_CPtr::from(slot),
                child_depth,
                root_cnode,
                root_wake_notification,
                root_depth,
                driver_runtime_child_root_wake_send_rights(),
                seL4_Word::from(badge),
            );
            if root_wake_err != seL4_NoError {
                return Err(HalError::Sel4(root_wake_err));
            }
        }
        if root_control_wake_required {
            let root_control_wake_notification = self
                .root_control_wake_notification_origin()
                .ok_or(HalError::Unsupported(
                    "driver-runtime-root-control-wake-origin-missing",
                ))?;
            let root_control_wake_err = sel4::cnode_mint_depth(
                child_cnode,
                seL4_CPtr::from(pi4_driver_abi::DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT),
                child_depth,
                root_cnode,
                root_control_wake_notification,
                root_depth,
                driver_runtime_child_root_wake_send_rights(),
                seL4_Word::from(
                    pi4_driver_abi::DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_BADGE,
                ),
            );
            if root_control_wake_err != seL4_NoError {
                return Err(HalError::Sel4(root_control_wake_err));
            }
        }
        let mut tracker = VSpaceTableTracker::new();
        let code_rights = sel4_sys::seL4_CapRights::new(0, 0, 1, 0);
        let data_rights = sel4_sys::seL4_CapRights_ReadWrite;
        let mut mapped_code_frame = None;
        let runtime_load = if let (Some(image), Some(plan)) =
            (linked_runtime_image, linked_runtime_plan)
        {
            self.map_runtime_elf_image(image, plan, vspace, &mut tracker)?
        } else {
            let mut code_frame = self
                .env
                .alloc_dma_frame_attr(runtime_cacheable_xn_attributes())
                .map_err(HalError::Sel4)?;
            code_frame.as_mut_slice().fill(0);
            // SAFETY: The linker script page-aligns `.driver_task_text`, the
            // range check above bounds it to one mapped user-image page, and
            // this copy reads only that page into a HAL-owned frame.
            let source = unsafe {
                core::slice::from_raw_parts(trampoline_range.start as *const u8, page_bytes)
            };
            code_frame.as_mut_slice().copy_from_slice(source);
            crate::hal::cache::cache_clean(
                sel4_sys::seL4_CapInitThreadVSpace,
                code_frame.ptr().as_ptr() as usize,
                page_bytes,
            )
            .map_err(|err| HalError::Sel4(err.code()))?;
            self.env
                .map_page_copy_into_vspace(
                    code_frame.cap(),
                    vspace,
                    trampoline_range.start,
                    code_rights,
                    sel4_sys::seL4_ARM_Page_Default,
                    &mut tracker,
                )
                .map_err(HalError::Sel4)?;
            crate::hal::cache::cache_unify_instruction(vspace, trampoline_range.start, page_bytes)
                .map_err(|err| HalError::Sel4(err.code()))?;
            self.env
                .unmap_page_cap(code_frame.cap())
                .map_err(HalError::Sel4)?;
            mapped_code_frame = Some(code_frame.cap());
            RuntimeElfLoad {
                entry: driver_task::isolated_trampoline_entry(),
                code_vaddr: trampoline_range.start,
                root_write_aliases_unmapped: true,
            }
        };
        self.env
            .map_page_copy_into_vspace(
                ring_frame.cap(),
                vspace,
                driver_task::DRIVER_TASK_RING_VADDR,
                data_rights,
                runtime_uncached_xn_attributes(),
                &mut tracker,
            )
            .map_err(HalError::Sel4)?;
        let child_ipc_frame = self
            .env
            .map_page_copy_into_vspace(
                ipc_frame.cap(),
                vspace,
                driver_task::DRIVER_TASK_IPC_VADDR,
                data_rights,
                runtime_cacheable_xn_attributes(),
                &mut tracker,
            )
            .map_err(HalError::Sel4)?;
        emit_driver_task_ipc_bind_caps(
            contract,
            ipc_frame.cap(),
            child_ipc_frame,
            driver_task::DRIVER_TASK_IPC_VADDR,
        );
        for (page, stack_frame) in stack_frames.iter().enumerate() {
            let vaddr = driver_task::DRIVER_TASK_STACK_BOTTOM_VADDR
                .checked_add(page.saturating_mul(1usize << sel4::PAGE_BITS))
                .ok_or(HalError::Unsupported("driver-runtime-stack-vaddr"))?;
            crate::hal::cache::cache_clean(
                sel4_sys::seL4_CapInitThreadVSpace,
                stack_frame.ptr().as_ptr() as usize,
                page_bytes,
            )
            .map_err(|err| HalError::Sel4(err.code()))?;
            self.env
                .unmap_page_cap(stack_frame.cap())
                .map_err(HalError::Sel4)?;
            self.env
                .map_page_cap_into_vspace(
                    stack_frame.cap(),
                    vspace,
                    vaddr,
                    data_rights,
                    runtime_cacheable_xn_attributes(),
                    &mut tracker,
                )
                .map_err(HalError::Sel4)?;
        }
        self.install_usb_pcie_bus_link(contract, child_cnode, child_depth, vspace, &mut tracker)?;
        let reciprocal_link_caps = self.install_cyw43_sdio_bus_link(
            contract,
            child_cnode,
            child_depth,
            notification,
            vspace,
            &mut tracker,
        )?;

        let mut runtime_init_descriptor = runtime_image_spec
            .map(|spec| {
                RuntimeInitDescriptorBuilder::new(
                    spec,
                    role_bit,
                    task_key,
                    driver_task::pi4_driver_task_runtime_artifact_hash(spec.hot_path),
                )
            })
            .transpose()?;
        let runtime_image_mapped_region_mask = self.map_isolated_runtime_declared_regions(
            runtime_image_spec,
            vspace,
            &mut tracker,
            runtime_init_descriptor.as_mut(),
        )?;
        let restart_sdhci_root_ptr = if contract == SDIO_HOST_DRIVER_TASK_CONTRACT {
            let frame = self
                .env
                .map_device(PI4_DRIVER_RUNTIME_SDIO_MMIO_BASES[0])
                .map_err(HalError::Sel4)?;
            frame.ptr().as_ptr() as usize
        } else {
            0
        };
        let runtime_irq = match runtime_init_descriptor.as_mut() {
            Some(builder) => self.install_generated_runtime_irq(
                contract,
                child_cnode,
                child_depth,
                notification,
                builder,
            )?,
            None => RuntimeIrqInstallGuard::empty(),
        };
        let runtime_init_descriptor = match runtime_init_descriptor {
            Some(builder) if driver_task::physical_pi_driver_task_only_owner_state_active() => {
                Some(builder.finish()?)
            }
            _ => None,
        };
        if let Some(descriptor) = runtime_init_descriptor {
            if (contract == CYW43_WIFI_DRIVER_TASK_CONTRACT
                || contract == SDIO_HOST_DRIVER_TASK_CONTRACT)
                && !driver_task::retain_driver_runtime_restart_descriptor(contract, descriptor)
            {
                return Err(HalError::Unsupported(
                    "driver-runtime-restart-descriptor-retain",
                ));
            }
        }

        let guard_bits = sel4::word_bits().saturating_sub(child_depth as seL4_Word);
        let cspace_root_data = sel4::cap_data_guard(0, guard_bits);
        #[cfg(sel4_config_kernel_mcs)]
        let tcb_fault_endpoint = mcs.standard_fault_endpoint;
        #[cfg(not(sel4_config_kernel_mcs))]
        let tcb_fault_endpoint = driver_task::DRIVER_TASK_CHILD_FAULT_SLOT;
        sel4::set_tcb_space(
            tcb,
            tcb_fault_endpoint,
            child_cnode,
            cspace_root_data,
            vspace,
            0,
        )
        .map_err(HalError::Sel4)?;

        self.env
            .bind_child_ipc_buffer(
                tcb,
                remote_tcb_ipc_buffer_frame_cap(ipc_frame.cap(), child_ipc_frame),
                driver_task::DRIVER_TASK_IPC_VADDR,
            )
            .map_err(HalError::Sel4)?;

        #[cfg(not(sel4_config_kernel_mcs))]
        let affinity_core = apply_driver_tcb_affinity_for_boot(contract, tcb)?;

        let (bootstrap_priority, steady_priority) = configure_driver_tcb_priority_for_boot(
            contract,
            tcb,
            mcs,
            DriverTaskTcbBootState::Active,
        )?;
        #[cfg(sel4_config_kernel_mcs)]
        let affinity_core = apply_driver_tcb_affinity_for_boot(contract, tcb)?;
        driver_task::publish_driver_task_scheduler(contract, tcb as usize, steady_priority);
        #[cfg(sel4_config_kernel_mcs)]
        if !driver_task::publish_driver_task_mcs_kernel_objects(
            contract,
            command_endpoint_origin as usize,
            mcs.command_reply as usize,
            mcs.completion_notification_origin as usize,
            mcs.sched_context as usize,
            mcs.standard_fault_endpoint as usize,
            mcs.timeout_fault_endpoint as usize,
        ) {
            return Err(HalError::Unsupported("driver-runtime-mcs-recovery-publish"));
        }
        #[cfg(sel4_config_kernel_mcs)]
        register_driver_task_fault_source(contract, tcb)?;
        #[cfg(sel4_config_kernel_mcs)]
        install_driver_task_supervisor_authority(contract, tcb, command_endpoint_origin, mcs)?;

        let _notification_bound =
            bind_driver_tcb_notification_for_boot(contract, tcb, notification)?;
        if let Some(recovery_endpoint) = recovery_endpoint {
            let irq_handlers = runtime_irq.root_handler_slots();
            if !driver_task::publish_cyw43_sdio_restart_context(
                contract,
                runtime_load.entry,
                stack_top,
                task_key,
                recovery_endpoint as usize,
                command_endpoint as usize,
                notification as usize,
                irq_handlers,
                restart_sdhci_root_ptr,
            ) {
                return Err(HalError::Unsupported("driver-runtime-restart-context"));
            }
        }
        validate_runtime_load_for_resume(runtime_load)?;
        sel4::write_tcb_registers(
            tcb,
            runtime_load.entry,
            stack_top,
            task_key as seL4_Word,
            true,
        )
        .map_err(HalError::Sel4)?;
        emit_driver_tcb_resume_return(contract, tcb, "write-registers");

        let runtime_recv_ready = if linked_runtime_image.is_some() {
            driver_task::wait_for_driver_task_runtime_recv_ready(contract, task_key, 4096)
        } else {
            true
        };
        let runtime_init_deferred = runtime_init_descriptor.is_some()
            && driver_task::pre_root_runtime_init_deferred_for_shell(contract);
        let runtime_init_ok = if !runtime_recv_ready {
            if let Some(spec) = runtime_image_spec {
                driver_task::emit_driver_task_resource_init_status(
                    contract,
                    spec.hot_path,
                    "runtime-entry",
                    "no-recv-ready",
                    None,
                );
            }
            if runtime_init_deferred {
                let runtime_descriptor_recorded =
                    runtime_init_descriptor.as_ref().is_some_and(|descriptor| {
                        driver_task::record_deferred_runtime_init_descriptor(contract, *descriptor)
                    });
                emit_driver_task_bootstrap_deferred(contract, tcb, runtime_descriptor_recorded);
            }
            false
        } else if runtime_init_deferred {
            let runtime_descriptor_recorded =
                runtime_init_descriptor.as_ref().is_some_and(|descriptor| {
                    driver_task::record_deferred_runtime_init_descriptor(contract, *descriptor)
                });
            emit_driver_task_bootstrap_deferred(contract, tcb, runtime_descriptor_recorded);
            false
        } else if let Some(descriptor) = runtime_init_descriptor.as_ref() {
            let spec =
                runtime_image_spec.ok_or(HalError::Unsupported("driver-runtime-init-spec"))?;
            let Some(frame) = driver_task::describe_driver_runtime_init_descriptor(descriptor)
            else {
                driver_task::emit_driver_task_resource_init_status(
                    contract,
                    spec.hot_path,
                    "runtime-descriptor-bootstrap",
                    "stage-failed",
                    None,
                );
                return Err(HalError::Unsupported("driver-runtime-init-stage"));
            };
            let Some(descriptor_bytes) =
                driver_task::driver_runtime_init_descriptor_bytes(descriptor)
            else {
                driver_task::emit_driver_task_resource_init_status(
                    contract,
                    spec.hot_path,
                    "runtime-descriptor-bootstrap",
                    "stage-failed",
                    None,
                );
                return Err(HalError::Unsupported("driver-runtime-init-stage"));
            };
            let staging_segments = [driver_task::DriverTaskStagingSegment::runtime_init(
                descriptor_bytes,
            )];
            let command = driver_task::runtime_init_command(
                spec.hot_path,
                driver_task::DriverTaskBudgetGrant::from_contract(contract),
                frame,
            );
            let completion = driver_task::run_driver_task_ring_command_bootstrap_staged(
                contract,
                command,
                &staging_segments,
            );
            let runtime_init_ok = matches!(
                completion,
                Some(done)
                if done.code == driver_task::DriverTaskCompletionCode::Progress.as_u16()
                    && done.result == spec.hot_path.as_u32()
            );
            let status = if runtime_init_ok {
                "ready"
            } else if completion.is_some() {
                "unexpected-completion"
            } else {
                "no-reply"
            };
            if runtime_init_ok {
                let _ = driver_task::record_driver_runtime_descriptor_seal(
                    contract,
                    spec.hot_path,
                    descriptor,
                );
            }
            driver_task::emit_driver_task_resource_init_status(
                contract,
                spec.hot_path,
                "runtime-descriptor-bootstrap",
                status,
                completion,
            );
            runtime_init_ok
        } else {
            !driver_task::physical_pi_driver_task_only_owner_state_active()
        };
        if runtime_init_ok {
            if let Some(spec) = runtime_image_spec {
                if spec.hot_path == driver_task::DriverTaskHotPath::SerialConsole {
                    let _ = crate::serial::init_serial_driver_task_runtime();
                }
                let _ = bootstrap_linked_runtime_engine_for_early_console(contract, spec)?;
            }
        }

        let pointer_free_ipc = if driver_task::physical_pi_driver_task_only_owner_state_active() {
            if retain_deferred_cyw43_pair_bootstrap_priority(contract, runtime_init_deferred) {
                let mut line = heapless::String::<192>::new();
                let _ = fmt::write(
                    &mut line,
                    format_args!(
                        "DRIVER_TASK_BOOTSTRAP_PRIORITY_RETAINED contract={} tcb=0x{:04x} priority={} reason=deferred-cyw43-sdio-owner-first",
                        contract.name, tcb, bootstrap_priority,
                    ),
                );
                crate::bootstrap::log::force_uart_line(line.as_str());
            } else {
                restore_driver_tcb_steady_priority(
                    contract,
                    tcb,
                    bootstrap_priority,
                    steady_priority,
                )?;
            }
            if !runtime_init_ok && !runtime_init_deferred {
                emit_driver_task_bootstrap_deferred(
                    contract,
                    tcb,
                    runtime_init_descriptor.is_some(),
                );
            }
            runtime_recv_ready
                && runtime_image_transport_pointer_free_ipc_ready(
                    runtime_image_mapped_region_mask,
                    driver_task::CURRENT_DRIVER_TASK_IPC_ABI,
                )
        } else {
            let completion = if runtime_init_ok {
                let command = driver_task::DriverTaskCommandRecord::service(
                    0,
                    driver_task::DriverTaskBudgetGrant::from_contract(contract),
                );
                driver_task::run_driver_task_ring_command_bootstrap(contract, command)
            } else {
                None
            };
            let pointer_free_ipc = matches!(
                completion,
                Some(done)
                    if done.code == driver_task::DriverTaskCompletionCode::Progress.as_u16()
                        && done.result == task_key as u32
            ) && runtime_init_ok;
            if pointer_free_ipc {
                restore_driver_tcb_steady_priority(
                    contract,
                    tcb,
                    bootstrap_priority,
                    steady_priority,
                )?;
            } else {
                let _ = restore_driver_tcb_steady_priority(
                    contract,
                    tcb,
                    bootstrap_priority,
                    steady_priority,
                );
                let _ = sel4::suspend_tcb(tcb);
                let mut line = heapless::String::<192>::new();
                let _ = fmt::write(
                    &mut line,
                    format_args!(
                        "DRIVER_TASK_BOOTSTRAP_SUSPENDED contract={} tcb=0x{:04x} reason=pointer-free-ipc-not-proved",
                        contract.name, tcb,
                    ),
                );
                crate::bootstrap::log::force_uart_line(line.as_str());
            }
            pointer_free_ipc
        };
        let affinity_core =
            apply_driver_tcb_affinity_after_bootstrap(contract, tcb, affinity_core)?;
        self.env
            .unmap_page_cap(ipc_frame.cap())
            .map_err(HalError::Sel4)?;
        let stack_frame_cap = stack_frames
            .first()
            .map(RamFrame::cap)
            .ok_or(HalError::Unsupported("driver-runtime-stack-empty"))?;

        let root_notification = if driver_runtime_needs_root_notification(contract) {
            let slot = self.env.allocate_slot();
            let err = sel4::cnode_mint_depth(
                root_cnode,
                slot,
                root_depth,
                root_cnode,
                notification,
                root_depth,
                driver_runtime_root_notification_send_rights(),
                seL4_Word::from(DRIVER_RUNTIME_RESERVED_ROOT_BADGE),
            );
            if err != seL4_NoError {
                let _ = sel4::cnode_delete(root_cnode, slot, root_depth);
                return Err(HalError::Sel4(err));
            }
            slot
        } else {
            0
        };
        driver_task::publish_driver_task_root_notification(contract, root_notification as usize);
        if root_wake_notification != 0
            && !driver_task::publish_driver_task_root_wake_notification(
                contract,
                root_wake_notification as usize,
                root_wake_route.map_or(0, |(_, badge)| badge),
            )
        {
            return Err(HalError::Unsupported("driver-runtime-root-wake-publish"));
        }
        let runtime_irqs = runtime_irq.commit();
        let reciprocal_link_caps = reciprocal_link_caps.commit();
        Ok(KernelDriverTaskHandle {
            contract,
            role_bit,
            tcb,
            cnode: child_cnode,
            command_endpoint_origin,
            command_endpoint,
            command_reply: mcs.command_reply,
            completion_notification_origin: mcs.completion_notification_origin,
            completion_notification: mcs.completion_notification,
            sched_context: mcs.sched_context,
            standard_fault_endpoint: mcs.standard_fault_endpoint,
            timeout_fault_endpoint: mcs.timeout_fault_endpoint,
            notification,
            root_notification,
            root_wake_notification,
            fault_slot: driver_task::DRIVER_TASK_CHILD_FAULT_SLOT,
            ipc_frame: ipc_frame.cap(),
            stack_frame: stack_frame_cap,
            ring_frame: Some(ring_frame.cap()),
            vspace: Some(vspace),
            code_frame: mapped_code_frame,
            runtime_irqs,
            reciprocal_link_caps,
            runtime_image_spec,
            runtime_image_declared_region_mask: runtime_image_declared_region_mask(
                runtime_image_spec,
            ),
            runtime_image_mapped_region_mask,
            runtime_image_acceptance_eligible: runtime_image_acceptance_eligible(
                runtime_image_spec,
            ),
            runtime_image_non_acceptance_reason: runtime_image_non_acceptance_reason(
                runtime_image_spec,
            ),
            code_vaddr: runtime_load.code_vaddr,
            ipc_vaddr: driver_task::DRIVER_TASK_IPC_VADDR,
            ring_vaddr: driver_task::DRIVER_TASK_RING_VADDR,
            stack_top,
            affinity_core,
            vspace_isolated: true,
            pointer_free_ipc,
            started: pointer_free_ipc || (runtime_recv_ready && runtime_init_deferred),
        })
    }

    fn install_generated_runtime_irq(
        &mut self,
        contract: DriverTaskContract,
        child_cnode: seL4_CPtr,
        child_depth: u8,
        notification: seL4_CPtr,
        builder: &mut RuntimeInitDescriptorBuilder,
    ) -> Result<RuntimeIrqInstallGuard, HalError> {
        let irqs = if contract == SERIAL_DRIVER_TASK_CONTRACT {
            [Some(generated_serial_runtime_irq()?), None]
        } else if contract == GENET_DRIVER_TASK_CONTRACT {
            let policy = crate::generated::driver_runtime_image_policy();
            if policy
                .irqs
                .iter()
                .any(|irq| irq.hot_path == driver_task::DriverTaskHotPath::GenetNic.as_str())
            {
                [Some(generated_genet_runtime_irq()?), None]
            } else {
                [None, None]
            }
        } else if contract == SDIO_HOST_DRIVER_TASK_CONTRACT {
            generated_cyw43_sdio_topology()?.irqs.map(Some)
        } else if contract == PCIE_ROOT_DRIVER_TASK_CONTRACT {
            [Some(generated_pcie_timer_runtime_irq()?), None]
        } else {
            return Ok(RuntimeIrqInstallGuard::empty());
        };
        let mut guard = RuntimeIrqInstallGuard::empty();
        for irq in irqs.into_iter().flatten() {
            let kernel = match self.bind_irq_to_notification_with_badge(
                Irq(irq.irq),
                generated_irq_trigger(irq.trigger),
                seL4_Word::from(irq.badge),
                notification,
                false,
            ) {
                Ok(binding) => binding,
                Err(err) => {
                    let mut line = heapless::String::<224>::new();
                    let _ = fmt::write(
                        &mut line,
                        format_args!(
                            "DRIVER_TASK_IRQ_TOPOLOGY contract={} irq={} badge={} status=failed proof_effect=acceptance-red err={}",
                            contract.name, irq.irq, irq.badge, err,
                        ),
                    );
                    crate::bootstrap::log::force_uart_line(line.as_str());
                    return Err(err);
                }
            };
            let child_handler_slot = seL4_CPtr::from(irq.handler_slot);
            let root_cnode = self.env.init_cnode_cap();
            let root_depth = sel4::word_bits() as u8;
            let copy_err = sel4::cnode_mint_depth(
                child_cnode,
                child_handler_slot,
                child_depth,
                root_cnode,
                kernel.handler_slot,
                root_depth,
                sel4_sys::seL4_CapRights_All,
                0,
            );
            if copy_err != seL4_NoError {
                let _ = self.release_irq_notification(kernel);
                let mut line = heapless::String::<224>::new();
                let _ = fmt::write(
                    &mut line,
                    format_args!(
                        "DRIVER_TASK_IRQ_TOPOLOGY contract={} irq={} badge={} status=failed proof_effect=acceptance-red err=handler-cap-mint-{}",
                        contract.name, irq.irq, irq.badge, copy_err,
                    ),
                );
                crate::bootstrap::log::force_uart_line(line.as_str());
                return Err(HalError::Sel4(copy_err));
            }
            if let Err(err) = builder.add_irq(irq) {
                let runtime = RuntimeIrqBinding {
                    kernel,
                    child_cnode,
                    child_handler_slot,
                    child_depth,
                };
                let _ = release_runtime_irq_binding(runtime);
                return Err(err);
            }
            let runtime = RuntimeIrqBinding {
                kernel,
                child_cnode,
                child_handler_slot,
                child_depth,
            };
            guard.push(runtime)?;
            let mut line = heapless::String::<224>::new();
            let proof_effect = if contract == SERIAL_DRIVER_TASK_CONTRACT {
                "irq-rx-ready"
            } else if contract == GENET_DRIVER_TASK_CONTRACT {
                "bounded-napi-ready"
            } else if contract == PCIE_ROOT_DRIVER_TASK_CONTRACT {
                "root-deadline-wake-ready"
            } else if irq.irq == BCM2711_SDIO_DMA_IRQ {
                "dma-completion-ready"
            } else {
                "notification-dpc-ready"
            };
            let _ = fmt::write(
                &mut line,
                format_args!(
                    "DRIVER_TASK_IRQ_TOPOLOGY contract={} irq={} badge={} handler_slot={} notification_slot={} trigger=level status=bound proof_effect={}",
                    contract.name,
                    irq.irq,
                    irq.badge,
                    irq.handler_slot,
                    irq.notification_slot,
                    proof_effect,
                ),
            );
            crate::bootstrap::log::force_uart_line(line.as_str());
        }
        Ok(guard)
    }

    /// Creates an IRQHandler and badged notification cap for a device IRQ.
    ///
    /// The returned binding is intentionally inert until the driver clears or
    /// masks its device-side interrupt source and calls [`Self::ack_irq`].
    pub fn bind_irq_notification(
        &mut self,
        irq: Irq,
        trigger: IrqTrigger,
    ) -> Result<KernelIrqBinding, HalError> {
        let notification_slot = self.env.alloc_notification().map_err(HalError::Sel4)?;
        match self.bind_irq_to_notification_with_badge(
            irq,
            trigger,
            irq_notification_badge(irq),
            notification_slot,
            true,
        ) {
            Ok(binding) => Ok(binding),
            Err(err) => {
                let _ = sel4::cnode_delete(
                    self.env.init_cnode_cap(),
                    notification_slot,
                    sel4::word_bits() as u8,
                );
                Err(err)
            }
        }
    }

    fn bind_irq_to_notification_with_badge(
        &mut self,
        irq: Irq,
        trigger: IrqTrigger,
        badge: seL4_Word,
        notification_slot: seL4_CPtr,
        owns_notification: bool,
    ) -> Result<KernelIrqBinding, HalError> {
        if badge == 0 {
            return Err(HalError::Unsupported("irq-notification-badge"));
        }
        let depth = sel4::word_bits() as u8;
        let root_cnode = self.env.init_cnode_cap();
        let handler_slot = self.env.allocate_slot();

        #[cfg(all(target_arch = "aarch64", target_os = "none"))]
        let get_err = sel4::irq_control_get_trigger_handler(
            irq.0 as seL4_Word,
            trigger.arm_trigger_word(),
            root_cnode,
            handler_slot,
            depth,
        );

        #[cfg(not(all(target_arch = "aarch64", target_os = "none")))]
        let get_err = {
            let _ = trigger;
            sel4::irq_control_get_level_handler(irq.0 as seL4_Word, root_cnode, handler_slot, depth)
        };

        if get_err != seL4_NoError {
            let _ = sel4::cnode_delete(root_cnode, handler_slot, depth);
            return Err(HalError::Sel4(get_err));
        }

        let badged_notification_slot = self.env.allocate_slot();
        let mint_err = sel4::cnode_mint_depth(
            root_cnode,
            badged_notification_slot,
            depth,
            root_cnode,
            notification_slot,
            depth,
            sel4_sys::seL4_CapRights_All,
            badge,
        );
        if mint_err != seL4_NoError {
            let _ = sel4::cnode_delete(root_cnode, badged_notification_slot, depth);
            let _ = sel4::cnode_delete(root_cnode, handler_slot, depth);
            return Err(HalError::Sel4(mint_err));
        }

        let bind_err = sel4::irq_handler_set_notification(handler_slot, badged_notification_slot);
        if bind_err != seL4_NoError {
            let _ = sel4::irq_handler_clear(handler_slot);
            let _ = sel4::cnode_delete(root_cnode, badged_notification_slot, depth);
            let _ = sel4::cnode_delete(root_cnode, handler_slot, depth);
            return Err(HalError::Sel4(bind_err));
        }

        Ok(KernelIrqBinding {
            irq,
            trigger,
            handler_slot,
            notification_slot,
            badged_notification_slot,
            badge,
            owns_notification,
        })
    }

    /// Acknowledges an IRQHandler after the driver has cleared the source.
    pub fn ack_irq(&mut self, binding: &KernelIrqBinding) -> Result<(), HalError> {
        binding.ack_from_hal()
    }

    /// Polls one IRQ notification and services it with a device clear callback.
    ///
    /// This is the required seL4 ordering for level-triggered device IRQs:
    /// observe the notification, clear the device-side interrupt source, then
    /// acknowledge the IRQHandler so seL4 may deliver the next interrupt.
    pub fn poll_and_service_irq<F>(
        &mut self,
        binding: &KernelIrqBinding,
        clear_device_source: F,
    ) -> Result<IrqServiceOutcome, HalError>
    where
        F: FnOnce(&mut Self) -> Result<(), HalError>,
    {
        let mut badge = 0;
        let _ = sel4::poll(binding.notification_slot, &mut badge);
        if badge == 0 {
            return Ok(IrqServiceOutcome::Idle);
        }

        clear_device_source(self)?;
        binding.ack_from_hal()?;
        Ok(IrqServiceOutcome::Serviced { badge })
    }

    /// Waits for one IRQ notification and services it with a device clear callback.
    pub fn wait_and_service_irq<F>(
        &mut self,
        binding: &KernelIrqBinding,
        clear_device_source: F,
    ) -> Result<seL4_Word, HalError>
    where
        F: FnOnce(&mut Self) -> Result<(), HalError>,
    {
        let mut badge = 0;
        let _ = sel4::wait(binding.notification_slot, &mut badge);
        clear_device_source(self)?;
        binding.ack_from_hal()?;
        Ok(badge)
    }

    /// Clears an IRQ binding and deletes the caps owned by the HAL binding.
    pub fn release_irq_notification(&mut self, binding: KernelIrqBinding) -> Result<(), HalError> {
        let depth = sel4::word_bits() as u8;
        let root_cnode = self.env.init_cnode_cap();
        let mut first_error = seL4_NoError;

        let mut errors = [seL4_NoError; 4];
        errors[0] = sel4::irq_handler_clear(binding.handler_slot);
        errors[1] = sel4::cnode_delete(root_cnode, binding.badged_notification_slot, depth);
        if binding.owns_notification {
            errors[2] = sel4::cnode_delete(root_cnode, binding.notification_slot, depth);
        }
        errors[3] = sel4::cnode_delete(root_cnode, binding.handler_slot, depth);
        for err in errors {
            if err != seL4_NoError && first_error == seL4_NoError {
                first_error = err;
            }
        }

        if first_error == seL4_NoError {
            Ok(())
        } else {
            Err(HalError::Sel4(first_error))
        }
    }
}

#[cfg(feature = "kernel")]
impl KernelWifiDebugHandle {
    #[must_use]
    pub const fn from_ptr(hal_ptr: usize) -> Option<Self> {
        if hal_ptr == 0 {
            None
        } else {
            Some(Self { hal_ptr })
        }
    }

    fn linked_runtime_required(&self) -> HalError {
        if self.hal_ptr == 0 {
            HalError::Unsupported("wifi-debug-handle")
        } else {
            HalError::Unsupported("pi4-wifi-driver-task-runtime-required")
        }
    }
}

#[cfg(feature = "kernel")]
impl WifiDebugOps for KernelWifiDebugHandle {
    fn dump_state(&mut self, _stage: &'static str) -> Result<WifiDebugSnapshot, HalError> {
        Err(self.linked_runtime_required())
    }

    fn firmware_contract_trace(&mut self) -> Option<WifiFirmwareContractTrace> {
        None
    }

    fn sdhci_contract_trace(&mut self) -> Option<WifiSdhciContractTrace> {
        None
    }

    fn control_plane_trace(&mut self) -> Option<WifiControlPlaneTrace> {
        None
    }
}

#[cfg(feature = "kernel")]
impl<'a> DeviceHal for KernelHal<'a> {
    type Error = HalError;

    fn map_device(&mut self, paddr: usize) -> Result<DeviceFrame, Self::Error> {
        self.env.map_device(paddr).map_err(HalError::from)
    }

    fn alloc_dma_frame(&mut self) -> Result<RamFrame, Self::Error> {
        self.env.alloc_dma_frame().map_err(HalError::from)
    }

    fn alloc_dma_frame_low(&mut self) -> Result<RamFrame, Self::Error> {
        self.env.alloc_dma_frame_low().map_err(HalError::from)
    }

    fn alloc_dma_frame_attr(
        &mut self,
        attr: seL4_ARM_VMAttributes,
    ) -> Result<RamFrame, Self::Error> {
        self.env.alloc_dma_frame_attr(attr).map_err(HalError::from)
    }

    fn alloc_dma_frame_low_attr(
        &mut self,
        attr: seL4_ARM_VMAttributes,
    ) -> Result<RamFrame, Self::Error> {
        self.env
            .alloc_dma_frame_low_attr(attr)
            .map_err(HalError::from)
    }

    fn reserve_dma_guard_page(&mut self) -> Result<usize, Self::Error> {
        Ok(self.env.reserve_dma_guard_page())
    }

    fn device_coverage(&self, paddr: usize, size_bits: usize) -> Option<DeviceCoverage> {
        self.env.device_coverage(paddr, size_bits)
    }

    fn snapshot(&self) -> KernelEnvSnapshot {
        self.env.snapshot()
    }

    fn bind_irq_notification(
        &mut self,
        irq: Irq,
        trigger: IrqTrigger,
    ) -> Result<KernelIrqBinding, Self::Error> {
        KernelHal::bind_irq_notification(self, irq, trigger)
    }

    fn ack_irq_notification(&mut self, binding: &KernelIrqBinding) -> Result<(), Self::Error> {
        KernelHal::ack_irq(self, binding)
    }
}

#[cfg(feature = "kernel")]
impl<'a> PciHal for KernelHal<'a> {
    fn pci_topology(&self) -> Option<&PciTopology> {
        None
    }
}

#[cfg(feature = "kernel")]
impl<'a> Cyw43Hal for KernelHal<'a> {
    fn wifi_firmware_bundle(&self) -> Result<WifiFirmwareBundle<'static>, Self::Error> {
        Ok(WifiFirmwareBundle::new(
            pi4_wifi::PI4_WIFI_FIRMWARE,
            pi4_wifi::PI4_WIFI_NVRAM,
            Some(pi4_wifi::PI4_WIFI_CLM_BLOB),
            pi4_wifi::PI4_WIFI_BOARD_TYPE,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{SdioBusWidth, SdioFunction, WifiFirmwareBundle};

    #[cfg(feature = "kernel")]
    use super::{
        cyw43_sdio_peer_notification_badge, driver_runtime_command_endpoint_receive_rights,
        driver_runtime_local_notification_receive_rights, format_cyw43_sdio_bus_link_diagnostic,
        irq_notification_badge, Irq, IrqTrigger, CYW43_SDIO_BUS_LINK_DIAGNOSTIC_CAPACITY,
        DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE,
        DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_SLOT,
        DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE,
        DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_SLOT, DRIVER_RUNTIME_SDIO_IRQ_BADGE,
    };
    #[cfg(feature = "kernel")]
    use pi4_driver_abi::DRIVER_RUNTIME_RESERVED_ROOT_BADGE;

    #[test]
    fn driver_timeout_endpoint_installation_follows_generated_policy() {
        use crate::generated::TimeoutPolicy;

        assert!(!super::driver_task_requires_timeout_endpoint(
            TimeoutPolicy::NaturalPostpone
        ));
        for policy in [
            TimeoutPolicy::Terminal,
            TimeoutPolicy::ReplenishOnce,
            TimeoutPolicy::ReturnError,
            TimeoutPolicy::FailStop,
        ] {
            assert!(super::driver_task_requires_timeout_endpoint(policy));
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn deferred_cyw43_pair_retains_bootstrap_priority_until_descriptor_proof() {
        assert!(super::retain_deferred_cyw43_pair_bootstrap_priority(
            super::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
            true,
        ));
        assert!(super::retain_deferred_cyw43_pair_bootstrap_priority(
            super::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            true,
        ));
        assert!(!super::retain_deferred_cyw43_pair_bootstrap_priority(
            super::driver_task::SERIAL_DRIVER_TASK_CONTRACT,
            true,
        ));
        assert!(!super::retain_deferred_cyw43_pair_bootstrap_priority(
            super::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
            false,
        ));
    }

    #[cfg(feature = "kernel")]
    fn fake_driver_task_handle(
        contract: super::DriverTaskContract,
        started: bool,
        affinity_core: Option<u8>,
    ) -> super::KernelDriverTaskHandle {
        let runtime_image_spec =
            super::driver_task::pi4_driver_task_runtime_image_spec_for_contract(contract);
        super::KernelDriverTaskHandle {
            contract,
            role_bit: super::driver_task::driver_task_role_bit(contract.kind),
            tcb: 0x100,
            cnode: 0x101,
            command_endpoint_origin: 0x10a,
            command_endpoint: 0x102,
            command_reply: 0x10b,
            completion_notification_origin: 0x10c,
            completion_notification: 0x10d,
            sched_context: 0x10e,
            standard_fault_endpoint: 0x10f,
            timeout_fault_endpoint: 0x110,
            notification: 0x103,
            root_notification: 0x106,
            root_wake_notification: 0x107,
            fault_slot: super::driver_task::DRIVER_TASK_CHILD_FAULT_SLOT,
            ipc_frame: 0x104,
            stack_frame: 0x105,
            ring_frame: None,
            vspace: None,
            code_frame: None,
            runtime_irqs: [None; super::DRIVER_RUNTIME_IRQ_BINDING_CAPACITY],
            reciprocal_link_caps: [None; 2],
            runtime_image_spec,
            runtime_image_declared_region_mask: super::runtime_image_declared_region_mask(
                runtime_image_spec,
            ),
            runtime_image_mapped_region_mask: 0,
            runtime_image_acceptance_eligible: super::runtime_image_acceptance_eligible(
                runtime_image_spec,
            ),
            runtime_image_non_acceptance_reason: super::runtime_image_non_acceptance_reason(
                runtime_image_spec,
            ),
            code_vaddr: 0,
            ipc_vaddr: 0x4000_0000,
            ring_vaddr: 0,
            stack_top: 0x4000_1000,
            affinity_core,
            vspace_isolated: false,
            pointer_free_ipc: false,
            started,
        }
    }

    #[cfg(feature = "kernel")]
    fn fake_isolated_driver_task_handle(
        contract: super::DriverTaskContract,
        started: bool,
        affinity_core: Option<u8>,
    ) -> super::KernelDriverTaskHandle {
        let mut handle = fake_driver_task_handle(contract, started, affinity_core);
        handle.vspace = Some(0x200);
        handle.ring_frame = Some(0x201);
        handle.code_frame = Some(0x202);
        handle.code_vaddr = 0x8000_0000;
        handle.ring_vaddr = super::driver_task::DRIVER_TASK_RING_VADDR;
        handle.ipc_vaddr = super::driver_task::DRIVER_TASK_IPC_VADDR;
        handle.stack_top = super::driver_task::DRIVER_TASK_STACK_TOP_VADDR;
        handle.vspace_isolated = true;
        handle.pointer_free_ipc = started;
        handle.runtime_image_mapped_region_mask =
            super::driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK;
        handle
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn pi4_runtime_mmio_candidates_require_usb_high_bar() {
        let bases =
            super::runtime_mmio_candidate_bases(super::driver_task::DriverTaskHotPath::UsbKeyboard);
        assert_eq!(bases, &[0x0000_0006_0000_0000]);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn pi4_runtime_mmio_candidates_keep_cyw43_behind_cpu_physical_sdio_runtime() {
        let cyw43 =
            super::runtime_mmio_candidate_bases(super::driver_task::DriverTaskHotPath::Cyw43Wifi);
        let sdio =
            super::runtime_mmio_candidate_bases(super::driver_task::DriverTaskHotPath::SdioHost);
        assert!(cyw43.is_empty());
        assert_eq!(sdio, &[0xFE30_0000]);
        assert_eq!(
            super::PI4_DRIVER_RUNTIME_WIFI_PWRSEQ_MMIO_BASES,
            &[0xFE00_B000]
        );
        assert_eq!(
            super::PI4_DRIVER_RUNTIME_BCM2835_DMA_MMIO_BASES,
            &[0xFE00_7000]
        );
        assert_eq!(super::PI4_DRIVER_RUNTIME_BCM2835_DMA_CHANNEL, 4);
        assert_ne!(
            super::PI4_DRIVER_RUNTIME_BCM2835_DMA_AVAILABLE_CHANNEL_MASK
                & (1 << super::PI4_DRIVER_RUNTIME_BCM2835_DMA_CHANNEL),
            0
        );
        assert_eq!(
            super::PI4_DRIVER_RUNTIME_BCM2835_DMA_MMIO_BASES[0]
                + super::PI4_DRIVER_RUNTIME_BCM2835_DMA_CHANNEL
                    * super::PI4_DRIVER_RUNTIME_BCM2835_DMA_CHANNEL_STRIDE,
            0xFE00_7400
        );
        assert!(!sdio.contains(&0x7E30_0000));
        assert!(!super::PI4_DRIVER_RUNTIME_WIFI_PWRSEQ_MMIO_BASES.contains(&0x7E00_B000));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn pi4_wifi_early_child_admission_precedes_root_mailbox_mapping() {
        use super::driver_task::Pi4PreRootNetBootstrapSelection;

        assert_eq!(
            super::selected_pi4_early_child_mmio_pages(Pi4PreRootNetBootstrapSelection::Wifi),
            super::PI4_DRIVER_RUNTIME_BCM2835_DMA_MMIO_BASES,
        );
        assert!(
            super::selected_pi4_early_child_mmio_pages(Pi4PreRootNetBootstrapSelection::Wired)
                .is_empty()
        );
        assert!(super::selected_pi4_early_child_mmio_pages(
            Pi4PreRootNetBootstrapSelection::Disabled
        )
        .is_empty());
        assert!(
            super::PI4_DRIVER_RUNTIME_BCM2835_DMA_MMIO_BASES[0]
                < super::PI4_DRIVER_RUNTIME_WIFI_PWRSEQ_MMIO_BASES[0]
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_region_paddr_contiguity_checks_exact_page_stride() {
        assert!(super::runtime_region_paddr_is_contiguous(
            0x4000_0000,
            3,
            0x1000,
            0x4000_3000
        ));
        assert!(!super::runtime_region_paddr_is_contiguous(
            0x4000_0000,
            3,
            0x1000,
            0x4000_4000
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_ram_region_attr_uses_normal_memory_only_for_cpu_spsc_links() {
        assert_eq!(
            super::KernelHal::runtime_ram_region_attr(
                super::driver_task::DriverTaskHotPath::SerialConsole,
                false,
            ),
            super::runtime_cacheable_xn_attributes()
        );
        assert_eq!(
            super::KernelHal::runtime_ram_region_attr(
                super::driver_task::DriverTaskHotPath::SerialConsole,
                true,
            ),
            super::runtime_uncached_xn_attributes()
        );
        assert_eq!(
            super::KernelHal::runtime_ram_region_attr(
                super::driver_task::DriverTaskHotPath::SdioHost,
                false,
            ),
            super::runtime_uncached_xn_attributes()
        );
        assert_eq!(
            super::KernelHal::runtime_ram_region_attr(
                super::driver_task::DriverTaskHotPath::GenetNic,
                false,
            ),
            super::runtime_cacheable_xn_attributes()
        );
        assert_eq!(
            super::KernelHal::runtime_ram_region_attr(
                super::driver_task::DriverTaskHotPath::GenetNic,
                true,
            ),
            super::runtime_uncached_xn_attributes()
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_xn_attributes_preserve_cache_policy() {
        let xn = crate::sel4::vm_attributes_raw(sel4_sys::seL4_ARM_ExecuteNever);
        let default = crate::sel4::vm_attributes_raw(sel4_sys::seL4_ARM_Page_Default);
        let cacheable_xn = crate::sel4::vm_attributes_raw(super::runtime_cacheable_xn_attributes());
        let uncached_xn = crate::sel4::vm_attributes_raw(super::runtime_uncached_xn_attributes());

        assert_eq!(cacheable_xn, default | xn);
        assert_eq!(uncached_xn, xn);
        assert_eq!(cacheable_xn & default, default);
        assert_eq!(uncached_xn & default, 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn mapped_register_pages_rejects_noncontiguous_physical_frames() {
        let mut pages = heapless::Vec::<super::DeviceFrame, 2>::new();
        assert!(pages
            .push(super::DeviceFrame::for_test(
                core::ptr::NonNull::dangling(),
                0xFE30_0000,
            ))
            .is_ok());
        assert!(pages
            .push(super::DeviceFrame::for_test(
                core::ptr::NonNull::dangling(),
                0xFE30_3000,
            ))
            .is_ok());

        let err = match super::MappedRegisterPages::new(0xFE30_0000, pages) {
            Ok(_) => panic!("noncontiguous page should be rejected"),
            Err(err) => err,
        };

        assert!(matches!(
            err,
            super::HalError::Unsupported("register-pages-noncontiguous")
        ));
    }

    #[cfg(feature = "kernel")]
    fn runtime_init_test_task_key(hot_path: super::driver_task::DriverTaskHotPath) -> usize {
        match hot_path {
            super::driver_task::DriverTaskHotPath::SerialConsole => {
                super::driver_task::DRIVER_TASK_KEY_SERIAL
            }
            super::driver_task::DriverTaskHotPath::UsbKeyboard => {
                super::driver_task::DRIVER_TASK_KEY_USB_LOCAL_SEAT
            }
            super::driver_task::DriverTaskHotPath::HdmiText => {
                super::driver_task::DRIVER_TASK_KEY_HDMI_TEXT
            }
            super::driver_task::DriverTaskHotPath::GenetNic => {
                super::driver_task::DRIVER_TASK_KEY_BCMGENET_V5
            }
            super::driver_task::DriverTaskHotPath::Cyw43Wifi => {
                super::driver_task::DRIVER_TASK_KEY_CYW43455
            }
            super::driver_task::DriverTaskHotPath::SdioHost => {
                super::driver_task::DRIVER_TASK_KEY_SDIO_HOST
            }
            super::driver_task::DriverTaskHotPath::PcieRoot => {
                super::driver_task::DRIVER_TASK_KEY_PCIE_ROOT
            }
        }
    }

    #[cfg(feature = "kernel")]
    fn runtime_init_test_artifact_hash(hot_path: super::driver_task::DriverTaskHotPath) -> u32 {
        0x5254_0000 | hot_path.as_u32()
    }

    #[cfg(feature = "kernel")]
    fn runtime_init_test_builder(
        spec: super::driver_task::DriverTaskRuntimeImageSpec,
        role_bit: usize,
    ) -> Result<super::RuntimeInitDescriptorBuilder, super::HalError> {
        let hot_path = spec.hot_path;
        let identity = u64::from(hot_path.as_u32());
        let core = (hot_path.as_u32() % 4) as u8;
        super::RuntimeInitDescriptorBuilder::new_with_scheduler(
            spec,
            role_bit,
            runtime_init_test_task_key(hot_path),
            runtime_init_test_artifact_hash(hot_path),
            super::RuntimeInitSchedulerConfig {
                scheduling_context_slot: 32 + hot_path.as_u32(),
                scheduling_context_bits: 8,
                sched_control_core: core,
                max_refills: 2,
                affinity_core: core,
                budget_us: 500,
                period_us: 10_000,
                standard_fault_badge: 0x26e2_1000 + identity,
                timeout_fault_badge: 0x26ed_1000 + identity,
            },
        )
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_init_descriptor_builder_records_primitive_page_metadata() {
        let hot_path = super::driver_task::DriverTaskHotPath::SerialConsole;
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            hot_path, 1, 1, 1, 1, 1, true, false,
        );
        let mut builder =
            runtime_init_test_builder(spec, super::driver_task::DRIVER_TASK_ROLE_SERIAL_BIT)
                .unwrap();
        builder.add_mmio_page(0xFD58_0000).unwrap();
        builder.add_dma_page(0x4000_0000).unwrap();
        builder.add_shared_page(0x5000_0000).unwrap();

        let descriptor = builder.finish().unwrap();
        assert_eq!(descriptor.hot_path, hot_path.as_u32());
        assert_eq!(
            descriptor.role_bit as usize,
            super::driver_task::DRIVER_TASK_ROLE_SERIAL_BIT
        );
        assert_eq!(descriptor.mmio_page_count, 1);
        assert_eq!(descriptor.dma_page_count, 1);
        assert_eq!(descriptor.shared_page_count, 1);
        assert_eq!(descriptor.mmio_pages[0].paddr, 0xFD58_0000);
        assert_eq!(descriptor.dma_pages[0].paddr, 0x4000_0000);
        assert_eq!(descriptor.shared_pages[0].paddr, 0x5000_0000);
        assert!(descriptor.valid());
        assert!(
            descriptor.sealed_identity_valid_for_task(runtime_init_test_task_key(hot_path) as u32)
        );
    }

    #[cfg(all(feature = "kernel", feature = "release-pi4"))]
    #[test]
    fn pcie_timer_descriptor_keeps_host_aperture_discontiguous_and_exact() {
        let hot_path = super::driver_task::DriverTaskHotPath::PcieRoot;
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            hot_path,
            1,
            1,
            super::PI4_DRIVER_RUNTIME_PCIE_TOTAL_MMIO_PAGES as u16,
            0,
            16,
            true,
            true,
        );
        let mut builder =
            runtime_init_test_builder(spec, super::driver_task::DRIVER_TASK_ROLE_PCIE_BIT)
                .expect("PCIe descriptor builder");
        let host_base = super::PI4_DRIVER_RUNTIME_PCIE_MMIO_BASES[0];
        for page in 0..super::PI4_DRIVER_RUNTIME_PCIE_HOST_MMIO_PAGES {
            builder
                .add_mmio_page(
                    host_base + page * pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_PAGE_BYTES as usize,
                )
                .expect("PCIe host page");
        }
        builder
            .add_mmio_page(super::PI4_DRIVER_RUNTIME_SYSTEM_TIMER_PADDR)
            .expect("system timer page");
        builder
            .add_tagged_mmio_resource_range(
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_PCIE_HOST,
                super::driver_task::DRIVER_TASK_DEVICE_MMIO_VADDR,
                host_base,
                super::PI4_DRIVER_RUNTIME_PCIE_HOST_MMIO_PAGES,
                0,
            )
            .expect("PCIe host range");
        builder
            .add_tagged_mmio_resource_range(
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_PI4_SYSTEM_TIMER,
                super::driver_task::DRIVER_TASK_DEVICE_MMIO_VADDR
                    + super::PI4_DRIVER_RUNTIME_PCIE_HOST_MMIO_PAGES
                        * pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_PAGE_BYTES as usize,
                super::PI4_DRIVER_RUNTIME_SYSTEM_TIMER_PADDR,
                super::PI4_DRIVER_RUNTIME_PCIE_TIMER_MMIO_PAGES,
                super::PI4_DRIVER_RUNTIME_PCIE_HOST_MMIO_PAGES as u16,
            )
            .expect("system timer range");
        for page in 0..16usize {
            builder
                .add_shared_page(0x4000_0000 + page * 0x1000)
                .expect("PCIe shared page");
        }
        builder
            .add_buffer_resource_range(
                hot_path,
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
                super::driver_task::DRIVER_TASK_SHARED_BUFFER_VADDR,
                0x4000_0000,
                16,
                0,
                true,
            )
            .expect("PCIe shared range");
        let timer_irq = crate::generated::DriverRuntimeIrqSpec {
            hot_path: "pcie-root",
            irq: 99,
            badge: 2048,
            handler_slot: 4,
            notification_slot: 3,
            trigger: crate::generated::DriverRuntimeIrqTrigger::Level,
        };
        assert!(super::generated_pcie_timer_runtime_irq_valid(timer_irq));
        builder.add_irq(timer_irq).expect("timer IRQ descriptor");

        let descriptor = builder.finish().expect("exact PCIe timer descriptor");
        assert_eq!(descriptor.mmio_page_count, 11);
        assert_eq!(descriptor.irq_count, 1);
        assert_eq!(descriptor.irqs[0].irq, 99);
        assert_eq!(descriptor.irqs[0].badge, 2048);
        assert_eq!(descriptor.irqs[0].handler_slot, 4);
        assert_eq!(descriptor.irqs[0].notification_slot, 3);
        assert_eq!(descriptor.mmio_pages[10].paddr, 0xFE00_3000);
        assert_eq!(descriptor.resource_range_count, 3);
        assert_eq!(descriptor.resource_ranges[0].page_count, 10);
        assert_eq!(descriptor.resource_ranges[0].tag, 8);
        assert_eq!(descriptor.resource_ranges[1].page_count, 1);
        assert_eq!(descriptor.resource_ranges[1].first_page_index, 10);
        assert_eq!(descriptor.resource_ranges[1].tag, 15);
        assert_eq!(descriptor.resource_ranges[1].paddr, 0xFE00_3000);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_init_descriptor_builder_records_hdmi_framebuffer_metadata() {
        let hot_path = super::driver_task::DriverTaskHotPath::HdmiText;
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            hot_path, 1, 1, 1, 1, 1, true, false,
        );
        let mut builder =
            runtime_init_test_builder(spec, super::driver_task::DRIVER_TASK_ROLE_DISPLAY_BIT)
                .unwrap();
        builder.add_mmio_page(0xFE00_B000).unwrap();
        builder.add_dma_page(0x4000_0000).unwrap();
        builder.add_shared_page(0x5000_0000).unwrap();
        builder.set_framebuffer(pi4_driver_abi::DriverRuntimeFramebufferDescriptor {
            vaddr: pi4_driver_abi::DRIVER_RUNTIME_FRAMEBUFFER_VADDR,
            paddr: 0x3000_0000,
            width: 640,
            height: 480,
            pitch: 640 * 4,
            format: pi4_driver_abi::DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888,
        });

        let descriptor = builder.finish().unwrap();
        assert!(descriptor.hdmi_ready());
        assert_eq!(
            descriptor.framebuffer.vaddr,
            pi4_driver_abi::DRIVER_RUNTIME_FRAMEBUFFER_VADDR
        );
        assert!(
            descriptor.sealed_identity_valid_for_task(runtime_init_test_task_key(hot_path) as u32)
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_init_descriptor_builder_records_semantic_ranges_and_bus_links() {
        let hot_path = super::driver_task::DriverTaskHotPath::UsbKeyboard;
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            hot_path, 64, 16, 512, 128, 32, true, false,
        );
        let mut builder =
            runtime_init_test_builder(spec, super::driver_task::DRIVER_TASK_ROLE_USB_BIT).unwrap();
        for index in 0..pi4_driver_abi::DRIVER_RUNTIME_INIT_MAX_MMIO_PAGES {
            builder
                .add_mmio_page(0x0000_0006_0000_0000usize + index * 0x1000)
                .unwrap();
        }
        builder
            .add_mmio_resource_range(
                super::driver_task::DriverTaskHotPath::UsbKeyboard,
                super::driver_task::DRIVER_TASK_DEVICE_MMIO_VADDR,
                0x0000_0006_0000_0000usize,
                512,
                0,
            )
            .unwrap();
        for index in 0..128 {
            builder.add_dma_page(0x4000_0000 + index * 0x1000).unwrap();
        }
        builder
            .add_buffer_resource_range(
                super::driver_task::DriverTaskHotPath::UsbKeyboard,
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_DMA,
                super::driver_task::DRIVER_TASK_DMA_BUFFER_VADDR,
                0x4000_0000,
                128,
                0,
                true,
            )
            .unwrap();
        for index in 0..32 {
            builder
                .add_shared_page(0x5000_0000 + index * 0x1000)
                .unwrap();
        }
        builder
            .add_buffer_resource_range(
                super::driver_task::DriverTaskHotPath::UsbKeyboard,
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
                super::driver_task::DRIVER_TASK_SHARED_BUFFER_VADDR,
                0x5000_0000,
                32,
                0,
                true,
            )
            .unwrap();

        let descriptor = builder.finish().unwrap();
        assert_eq!(
            descriptor.resource_pages_by_kind(pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_MMIO),
            512
        );
        assert!(descriptor.has_resource_range(
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_USB_XHCI
        ));
        assert!(
            descriptor.has_bus_link_to(super::driver_task::DriverTaskHotPath::PcieRoot.as_u32())
        );
        assert!(descriptor.has_sealed_pointer_free_bus_link(
            runtime_init_test_task_key(hot_path) as u32,
            super::driver_task::DriverTaskHotPath::PcieRoot.as_u32(),
            pi4_driver_abi::DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE
        ));
        assert_eq!(descriptor.bus_alias_or, super::PI4_VL805_DMA_BUS_ALIAS_OR);
        assert_eq!(descriptor.bus_alias_and, super::PI4_VL805_DMA_BUS_ALIAS_AND);
        assert!(descriptor.has_resource_range_at_with_flags(
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_DMA,
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
            super::driver_task::DRIVER_TASK_DMA_BUFFER_VADDR as u64,
            128,
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
        ));
        assert_eq!(
            descriptor.dma_page_count,
            pi4_driver_abi::DRIVER_RUNTIME_INIT_MAX_DMA_PAGES as u16
        );
        assert_eq!(
            descriptor.mmio_page_count,
            pi4_driver_abi::DRIVER_RUNTIME_INIT_MAX_MMIO_PAGES as u16
        );
        assert!(descriptor.valid_for_resources(
            super::driver_task::DriverTaskHotPath::UsbKeyboard.as_u32(),
            super::driver_task::DRIVER_TASK_ROLE_USB_BIT as u32,
            512,
            128,
            32,
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_init_descriptor_builder_records_cyw43_sdio_shared_rx_window() {
        let hot_path = super::driver_task::DriverTaskHotPath::Cyw43Wifi;
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            hot_path, 64, 16, 0, 0, 64, false, true,
        )
        .with_root_wake_notification(
            pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT as u8,
            pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE,
        );
        let mut builder =
            runtime_init_test_builder(spec, super::driver_task::DRIVER_TASK_ROLE_NET_BIT).unwrap();
        for index in 0..64 {
            builder
                .add_shared_page(0x5000_0000usize + index * 0x1000)
                .unwrap();
        }
        builder
            .add_buffer_resource_range(
                super::driver_task::DriverTaskHotPath::Cyw43Wifi,
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
                super::driver_task::DRIVER_TASK_SHARED_BUFFER_VADDR,
                0x5000_0000,
                64,
                0,
                true,
            )
            .unwrap();

        let descriptor = builder.finish().unwrap();
        assert_eq!(descriptor.bus_link_count, 1);
        assert_eq!(
            descriptor.root_wake_notification_slot,
            pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT
        );
        assert_eq!(
            descriptor.root_wake_notification_badge,
            pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE
        );
        assert_eq!(
            descriptor.root_control_wake_notification_slot,
            pi4_driver_abi::DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT
        );
        let link = descriptor.bus_links[0];
        assert_eq!(
            link.owner_hot_path,
            super::driver_task::DriverTaskHotPath::SdioHost.as_u32()
        );
        assert_eq!(
            link.channel_id,
            pi4_driver_abi::DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO
        );
        assert_eq!(
            link.shared_offset,
            pi4_driver_abi::DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32
        );
        assert_eq!(
            link.shared_len,
            pi4_driver_abi::DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as u32
        );
        assert!(descriptor.has_sealed_pointer_free_bus_link(
            runtime_init_test_task_key(hot_path) as u32,
            super::driver_task::DriverTaskHotPath::SdioHost.as_u32(),
            pi4_driver_abi::DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO
        ));
        assert_eq!(
            descriptor.shared_page_count,
            pi4_driver_abi::DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES as u16
        );
        assert_eq!(
            descriptor.resource_pages_by_kind(pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_SHARED),
            64
        );
        assert!(descriptor.valid_for_resources(
            super::driver_task::DriverTaskHotPath::Cyw43Wifi.as_u32(),
            super::driver_task::DRIVER_TASK_ROLE_NET_BIT as u32,
            0,
            0,
            64,
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_init_descriptor_builder_records_sdio_dma_authority() {
        let hot_path = super::driver_task::DriverTaskHotPath::SdioHost;
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            hot_path,
            256,
            16,
            3,
            super::PI4_SDIO_DMA_PRIVATE_PAGE_COUNT as u16,
            32,
            false,
            true,
        );
        let mut builder =
            runtime_init_test_builder(spec, super::driver_task::DRIVER_TASK_ROLE_SDIO_BIT).unwrap();
        let page_bytes = pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_PAGE_BYTES as usize;
        for paddr in [0xFE30_0000, 0xFE00_B000, 0xFE00_7000] {
            builder.add_mmio_page(paddr).unwrap();
        }
        builder
            .add_tagged_mmio_resource_range(
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST,
                super::driver_task::DRIVER_TASK_DEVICE_MMIO_VADDR,
                0xFE30_0000,
                1,
                0,
            )
            .unwrap();
        builder
            .add_tagged_mmio_resource_range(
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_WIFI_PWRSEQ,
                super::driver_task::DRIVER_TASK_DEVICE_MMIO_VADDR + page_bytes,
                0xFE00_B000,
                1,
                1,
            )
            .unwrap();
        builder
            .add_tagged_mmio_resource_range(
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_BCM2835_DMA,
                super::driver_task::DRIVER_TASK_DEVICE_MMIO_VADDR + 2 * page_bytes,
                0xFE00_7000,
                1,
                2,
            )
            .unwrap();

        let mut dma_paddrs = [0usize; super::PI4_SDIO_DMA_PRIVATE_PAGE_COUNT];
        for (index, paddr) in dma_paddrs.iter_mut().enumerate() {
            *paddr = 0x0010_0000usize + index * 0x0010_0000;
        }
        for paddr in dma_paddrs {
            builder.add_dma_page(paddr).unwrap();
        }
        builder
            .add_tagged_dma_resource_range(
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_WIFI_PWRSEQ_REQUEST,
                super::driver_task::DRIVER_TASK_DMA_BUFFER_VADDR,
                dma_paddrs[0],
                1,
                0,
                true,
            )
            .unwrap();
        builder
            .add_tagged_dma_resource_range(
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
                super::driver_task::DRIVER_TASK_DMA_BUFFER_VADDR + page_bytes,
                dma_paddrs[1],
                super::PI4_SDIO_DMA_PRIVATE_PAGE_COUNT - 1,
                1,
                false,
            )
            .unwrap();

        for index in 0..32 {
            builder
                .add_shared_page(0x1000_0000 + index * page_bytes)
                .unwrap();
        }
        builder
            .add_buffer_resource_range(
                hot_path,
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
                super::driver_task::DRIVER_TASK_SHARED_BUFFER_VADDR,
                0x1000_0000,
                32,
                0,
                true,
            )
            .unwrap();

        let descriptor = builder.finish().unwrap();
        assert_eq!(descriptor.bus_alias_or, super::PI4_SDIO_DMA_BUS_ALIAS_OR);
        assert_eq!(descriptor.bus_alias_and, super::PI4_SDIO_DMA_BUS_ALIAS_AND);
        assert!(descriptor.has_resource_range_at(
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_BCM2835_DMA,
            (super::driver_task::DRIVER_TASK_DEVICE_MMIO_VADDR + 2 * page_bytes) as u64,
            1,
        ));
        assert!(descriptor.has_resource_range_at(
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_DMA,
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_WIFI_PWRSEQ_REQUEST,
            super::driver_task::DRIVER_TASK_DMA_BUFFER_VADDR as u64,
            1,
        ));
        assert!(descriptor.has_resource_range_at(
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_DMA,
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
            (super::driver_task::DRIVER_TASK_DMA_BUFFER_VADDR + page_bytes) as u64,
            (super::PI4_SDIO_DMA_PRIVATE_PAGE_COUNT - 1) as u16,
        ));
        let dma_arena = descriptor.resource_ranges[..usize::from(descriptor.resource_range_count)]
            .iter()
            .find(|range| range.tag == pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA)
            .expect("SDIO DMA arena range");
        assert_eq!(dma_arena.first_page_index, 1);
        assert_eq!(
            dma_arena.flags & pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS,
            0
        );
        assert!(descriptor.valid_for_resources(
            hot_path.as_u32(),
            super::driver_task::DRIVER_TASK_ROLE_SDIO_BIT as u32,
            3,
            super::PI4_SDIO_DMA_PRIVATE_PAGE_COUNT as u16,
            32,
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_init_descriptor_builder_keeps_large_buffer_budgets_semantic() {
        let hot_path = super::driver_task::DriverTaskHotPath::GenetNic;
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            hot_path, 64, 16, 6, 512, 32, true, false,
        );
        let mut builder =
            runtime_init_test_builder(spec, super::driver_task::DRIVER_TASK_ROLE_NET_BIT).unwrap();
        for index in 0..6 {
            builder
                .add_mmio_page(0xfd58_0000usize + index * 0x1000)
                .unwrap();
        }
        builder
            .add_mmio_resource_range(
                super::driver_task::DriverTaskHotPath::GenetNic,
                super::driver_task::DRIVER_TASK_DEVICE_MMIO_VADDR,
                0xfd58_0000usize,
                6,
                0,
            )
            .unwrap();
        for index in 0..512 {
            builder
                .add_dma_page(0x4000_0000usize + index * 0x1000)
                .unwrap();
        }
        builder
            .add_buffer_resource_range(
                super::driver_task::DriverTaskHotPath::GenetNic,
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_DMA,
                super::driver_task::DRIVER_TASK_DMA_BUFFER_VADDR,
                0x4000_0000,
                512,
                0,
                true,
            )
            .unwrap();
        for index in 0..32 {
            builder
                .add_shared_page(0x5000_0000usize + index * 0x1000)
                .unwrap();
        }
        builder
            .add_buffer_resource_range(
                super::driver_task::DriverTaskHotPath::GenetNic,
                pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
                super::driver_task::DRIVER_TASK_SHARED_BUFFER_VADDR,
                0x5000_0000,
                32,
                0,
                true,
            )
            .unwrap();

        let descriptor = builder.finish().unwrap();
        assert_eq!(
            descriptor.dma_page_count,
            pi4_driver_abi::DRIVER_RUNTIME_INIT_MAX_DMA_PAGES as u16
        );
        assert_eq!(
            descriptor.shared_page_count,
            pi4_driver_abi::DRIVER_RUNTIME_INIT_MAX_SHARED_PAGES as u16
        );
        assert_eq!(
            descriptor.resource_pages_by_kind(pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_DMA),
            512
        );
        assert_eq!(
            descriptor.resource_pages_by_kind(pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_SHARED),
            32
        );
        assert_ne!(
            descriptor.flags & pi4_driver_abi::DRIVER_RUNTIME_INIT_FLAG_DIRECT_GENET,
            0
        );
        assert!(descriptor.direct_genet.valid());
        assert!(
            descriptor.shared_pages[..usize::from(descriptor.shared_page_count)]
                .iter()
                .all(|page| page.paddr == 0)
        );
        let direct = descriptor.resource_ranges[..usize::from(descriptor.resource_range_count)]
            .iter()
            .find(|range| {
                range.tag == pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_TAG_GENET_DIRECT_LINK
            })
            .expect("direct GENET CPU-only range");
        assert_eq!(direct.paddr, 0);
        assert_eq!(
            direct.flags,
            pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED
                | pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_FLAG_CPU_ONLY
        );
        assert!(descriptor.valid_for_resources(
            super::driver_task::DriverTaskHotPath::GenetNic.as_u32(),
            super::driver_task::DRIVER_TASK_ROLE_NET_BIT as u32,
            6,
            512,
            32,
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_init_descriptor_builder_rejects_non_exact_direct_genet_range() {
        let hot_path = super::driver_task::DriverTaskHotPath::GenetNic;
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            hot_path, 64, 16, 6, 512, 32, true, false,
        );
        let mut builder =
            runtime_init_test_builder(spec, super::driver_task::DRIVER_TASK_ROLE_NET_BIT).unwrap();

        assert_eq!(
            builder
                .add_buffer_resource_range(
                    hot_path,
                    pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
                    super::driver_task::DRIVER_TASK_SHARED_BUFFER_VADDR,
                    0x5000_0000,
                    31,
                    0,
                    true,
                )
                .unwrap_err(),
            super::HalError::Unsupported("driver-runtime-init-direct-genet-range")
        );
        assert_eq!(
            builder
                .add_buffer_resource_range(
                    hot_path,
                    pi4_driver_abi::DRIVER_RUNTIME_RESOURCE_KIND_SHARED,
                    super::driver_task::DRIVER_TASK_SHARED_BUFFER_VADDR + 0x1000,
                    0x5000_0000,
                    32,
                    0,
                    true,
                )
                .unwrap_err(),
            super::HalError::Unsupported("driver-runtime-init-direct-genet-range")
        );
    }

    #[cfg(feature = "kernel")]
    fn put_runtime_elf_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    #[cfg(feature = "kernel")]
    fn put_runtime_elf_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[cfg(feature = "kernel")]
    fn put_runtime_elf_u64(bytes: &mut [u8], offset: usize, value: u64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    #[cfg(feature = "kernel")]
    fn runtime_test_elf(
        entry: u64,
        segments: &[(u32, u64, u64, u64, u64)],
        image_len: usize,
    ) -> Vec<u8> {
        let mut image = vec![0u8; image_len];
        image[0..4].copy_from_slice(b"\x7fELF");
        image[4] = 2;
        image[5] = 1;
        put_runtime_elf_u16(&mut image, 16, 2);
        put_runtime_elf_u16(&mut image, 18, 183);
        put_runtime_elf_u64(&mut image, 24, entry);
        put_runtime_elf_u64(&mut image, 32, 64);
        put_runtime_elf_u16(&mut image, 52, 64);
        put_runtime_elf_u16(&mut image, 54, 56);
        put_runtime_elf_u16(&mut image, 56, segments.len() as u16);
        for (index, &(flags, offset, vaddr, filesz, memsz)) in segments.iter().enumerate() {
            let base = 64 + index * 56;
            put_runtime_elf_u32(&mut image, base, 1);
            put_runtime_elf_u32(&mut image, base + 4, flags);
            put_runtime_elf_u64(&mut image, base + 8, offset);
            put_runtime_elf_u64(&mut image, base + 16, vaddr);
            put_runtime_elf_u64(&mut image, base + 24, vaddr);
            put_runtime_elf_u64(&mut image, base + 32, filesz);
            put_runtime_elf_u64(&mut image, base + 40, memsz);
            put_runtime_elf_u64(&mut image, base + 48, 0x10000);
        }
        image
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_elf_loader_plans_multiple_load_segments() {
        let mut image = runtime_test_elf(
            0x210010,
            &[
                (4, 0x1000, 0x200000, 0x10, 0x10),
                (5, 0x2000, 0x210000, 0x1200, 0x1200),
                (6, 0x4000, 0x226000, 0x20, 0x80),
            ],
            0x5000,
        );
        image[0x2000..0x3200].fill(0xaa);
        image[0x4000..0x4020].fill(0xbb);

        assert!(matches!(
            super::plan_runtime_elf_load(&image, 1),
            Err(super::HalError::Unsupported(
                "driver-runtime-elf-code-pages"
            ))
        ));
        let plan = super::plan_runtime_elf_load(&image, 64).unwrap();
        assert_eq!(plan.base_vaddr, 0x200000);
        assert_eq!(plan.page_count, 39);
        assert_eq!(plan.segment_count, 3);

        let mut page = [0u8; 4096];
        let rx_page = (0x210000 - plan.base_vaddr) / 4096;
        let fill = super::fill_runtime_elf_page(&image, plan, rx_page, &mut page).unwrap();
        assert!(!fill.writable);
        assert!(fill.executable);
        let mapping = super::runtime_elf_page_mapping(fill).unwrap();
        assert_eq!(mapping.rights.raw(), 0b10);
        assert_eq!(
            crate::sel4::vm_attributes_raw(mapping.attributes)
                & crate::sel4::vm_attributes_raw(sel4_sys::seL4_ARM_ExecuteNever),
            0
        );
        assert_eq!(page[0], 0xaa);
        assert_eq!(page[0x0fff], 0xaa);

        let data_page = (0x226000 - plan.base_vaddr) / 4096;
        let fill = super::fill_runtime_elf_page(&image, plan, data_page, &mut page).unwrap();
        assert!(fill.writable);
        assert!(!fill.executable);
        let mapping = super::runtime_elf_page_mapping(fill).unwrap();
        assert_eq!(mapping.rights.raw(), 0b11);
        assert_ne!(
            crate::sel4::vm_attributes_raw(mapping.attributes)
                & crate::sel4::vm_attributes_raw(sel4_sys::seL4_ARM_ExecuteNever),
            0
        );
        assert_eq!(page[0], 0xbb);
        assert_eq!(page[0x1f], 0xbb);
        assert_eq!(page[0x20], 0);

        let ro_page = 0;
        let fill = super::fill_runtime_elf_page(&image, plan, ro_page, &mut page).unwrap();
        assert!(!fill.writable);
        assert!(!fill.executable);
        let mapping = super::runtime_elf_page_mapping(fill).unwrap();
        assert_eq!(mapping.rights.raw(), 0b10);
        assert_ne!(
            crate::sel4::vm_attributes_raw(mapping.attributes)
                & crate::sel4::vm_attributes_raw(sel4_sys::seL4_ARM_ExecuteNever),
            0
        );

        let hole_page = 1;
        let fill = super::fill_runtime_elf_page(&image, plan, hole_page, &mut page).unwrap();
        assert!(!fill.writable);
        assert!(!fill.executable);
        let mapping = super::runtime_elf_page_mapping(fill).unwrap();
        assert_eq!(mapping.rights.raw(), 0b10);
        assert_ne!(
            crate::sel4::vm_attributes_raw(mapping.attributes)
                & crate::sel4::vm_attributes_raw(sel4_sys::seL4_ARM_ExecuteNever),
            0
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_elf_loader_rejects_writable_executable_segment() {
        let image = runtime_test_elf(0x210010, &[(7, 0x1000, 0x210000, 0x100, 0x100)], 0x2000);
        assert!(matches!(
            super::plan_runtime_elf_load(&image, 4),
            Err(super::HalError::Unsupported(
                "driver-runtime-elf-wx-segment"
            ))
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_elf_loader_rejects_effective_wx_page() {
        let image = runtime_test_elf(
            0x210010,
            &[
                (5, 0x1000, 0x210000, 0x800, 0x800),
                (6, 0x1800, 0x210800, 0x100, 0x100),
            ],
            0x2000,
        );
        assert!(matches!(
            super::plan_runtime_elf_load(&image, 4),
            Err(super::HalError::Unsupported("driver-runtime-elf-wx-page"))
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_elf_loader_rejects_unsupported_or_non_readable_flags() {
        for flags in [1, 12] {
            let image =
                runtime_test_elf(0x210010, &[(flags, 0x1000, 0x210000, 0x100, 0x100)], 0x2000);
            assert!(matches!(
                super::plan_runtime_elf_load(&image, 4),
                Err(super::HalError::Unsupported("driver-runtime-elf-flags"))
            ));
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_elf_page_mapping_defensively_rejects_wx() {
        assert!(matches!(
            super::runtime_elf_page_mapping(super::RuntimeElfPageFill {
                writable: true,
                executable: true,
            }),
            Err(super::HalError::Unsupported("driver-runtime-elf-wx-page"))
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_resume_requires_executable_root_write_aliases_unmapped() {
        let load = super::RuntimeElfLoad {
            entry: 0x210000,
            code_vaddr: 0x210000,
            root_write_aliases_unmapped: false,
        };
        assert!(matches!(
            super::validate_runtime_load_for_resume(load),
            Err(super::HalError::Unsupported(
                "driver-runtime-executable-root-alias"
            ))
        ));
        assert!(
            super::validate_runtime_load_for_resume(super::RuntimeElfLoad {
                root_write_aliases_unmapped: true,
                ..load
            })
            .is_ok()
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn irq_notification_badges_are_nonzero_and_irq_derived() {
        assert_eq!(irq_notification_badge(Irq(0)), 1);
        assert_eq!(irq_notification_badge(Irq(143)), 144);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_runtime_service_badges_exclude_the_reserved_root_bit() {
        assert_ne!(DRIVER_RUNTIME_RESERVED_ROOT_BADGE, 0);
        assert_eq!(
            driver_runtime_local_notification_receive_rights().raw(),
            0b0010,
            "the child local cap may receive but cannot self-signal"
        );
        assert_eq!(
            driver_runtime_command_endpoint_receive_rights().raw(),
            0b0010,
            "the child command cap may receive but cannot become a second producer"
        );
        assert_eq!(
            super::driver_runtime_command_endpoint_send_rights().raw(),
            0b1001,
            "root command authority is Write + GrantReply without Grant or Read"
        );
        #[cfg(sel4_config_kernel_mcs)]
        for task_key in [
            super::driver_task::DRIVER_TASK_KEY_CYW43455,
            super::driver_task::DRIVER_TASK_KEY_SDIO_HOST,
        ] {
            assert_eq!(
                super::driver_runtime_command_endpoint_badge(task_key).unwrap(),
                pi4_driver_abi::driver_runtime_command_badge(task_key as u32),
                "normal and recovery command caps must share the exact task-key badge"
            );
        }
        assert_eq!(
            super::driver_runtime_fault_send_rights().raw(),
            0b1001,
            "driver fault authority is Write + GrantReply without Grant or Read"
        );
        assert_eq!(
            super::driver_runtime_completion_notification_send_rights().raw(),
            0b0001,
            "the child completion cap may signal but cannot consume root wakes"
        );
        assert_eq!(
            super::driver_runtime_completion_notification_receive_rights().raw(),
            0b0010,
            "the root completion cap may receive but cannot self-signal"
        );
        assert_eq!(
            super::driver_runtime_root_notification_send_rights().raw(),
            0b0001,
            "root may signal but cannot receive from the child-bound notification"
        );
        assert_eq!(
            super::driver_runtime_child_root_wake_send_rights().raw(),
            0b0001,
            "physical runtimes may signal but cannot receive from root-owned wake objects"
        );
        assert_eq!(
            pi4_driver_abi::DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT,
            12,
        );
        assert_ne!(
            pi4_driver_abi::DRIVER_RUNTIME_ROOT_CONTROL_WAKE_NOTIFICATION_SLOT,
            pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT,
        );
        assert_eq!(
            cyw43_sdio_peer_notification_badge(DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_SLOT),
            Some(DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE)
        );
        assert_eq!(
            cyw43_sdio_peer_notification_badge(DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_SLOT),
            Some(DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE)
        );
        assert_ne!(DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE, 0);
        assert_ne!(DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE, 0);
        assert_ne!(
            DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE,
            DRIVER_RUNTIME_SDIO_IRQ_BADGE
        );
        assert_ne!(
            DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE,
            DRIVER_RUNTIME_SDIO_IRQ_BADGE
        );
        assert_ne!(
            DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE,
            DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE
        );
        assert_eq!(
            DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE & DRIVER_RUNTIME_SDIO_IRQ_BADGE,
            0,
            "the client-to-owner badge must remain bitwise disjoint from the SDIO IRQ"
        );
        assert_eq!(
            DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE
                & DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE,
            0,
            "the reciprocal peer badges must remain bitwise disjoint"
        );
        assert_eq!(
            DRIVER_RUNTIME_RESERVED_ROOT_BADGE
                & (DRIVER_RUNTIME_BUS_LINK_CYW43_NOTIFICATION_BADGE
                    | DRIVER_RUNTIME_BUS_LINK_SDIO_NOTIFICATION_BADGE
                    | DRIVER_RUNTIME_SDIO_IRQ_BADGE),
            0,
            "completion/DPC and IRQ badges must exclude the reserved root bit"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn root_continuation_notification_authority_is_profile_and_owner_scoped() {
        assert!(super::driver_runtime_needs_root_notification(
            super::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
        ));
        assert_eq!(
            super::driver_runtime_needs_root_notification(
                super::driver_task::SERIAL_DRIVER_TASK_CONTRACT,
            ),
            super::driver_task::physical_pi_driver_task_only_owner_state_active(),
        );
        for contract in [
            super::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
            super::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            super::driver_task::GENET_DRIVER_TASK_CONTRACT,
        ] {
            assert!(!super::driver_runtime_needs_root_notification(contract));
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn only_cyw43_receives_exact_root_rx_wake_authority() {
        let cyw43 = super::driver_task::DriverTaskRuntimeImageSpec::new(
            super::driver_task::DriverTaskHotPath::Cyw43Wifi,
            1,
            1,
            0,
            0,
            1,
            false,
            true,
        )
        .with_root_wake_notification(
            pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT as u8,
            pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE,
        );
        assert_eq!(
            super::driver_runtime_root_wake_route(cyw43).unwrap(),
            Some((
                pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT as u8,
                pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE,
            ))
        );

        let missing = super::driver_task::DriverTaskRuntimeImageSpec::new(
            super::driver_task::DriverTaskHotPath::Cyw43Wifi,
            1,
            1,
            0,
            0,
            1,
            false,
            true,
        );
        assert!(super::driver_runtime_root_wake_route(missing).is_err());

        let genet = super::driver_task::DriverTaskRuntimeImageSpec::new(
            super::driver_task::DriverTaskHotPath::GenetNic,
            1,
            1,
            1,
            1,
            1,
            false,
            true,
        )
        .with_root_wake_notification(
            pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_SLOT as u8,
            pi4_driver_abi::DRIVER_RUNTIME_CYW43_ROOT_WAKE_NOTIFICATION_BADGE,
        );
        assert!(super::driver_runtime_root_wake_route(genet).is_err());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_sdio_bus_link_diagnostic_is_complete_and_bounded() {
        let topology = super::generated_cyw43_sdio_topology()
            .expect("generated Pi 4 CYW43/SDIO topology must remain valid");
        let client_to_owner_badge =
            cyw43_sdio_peer_notification_badge(u32::from(topology.link.client_to_owner_slot))
                .expect("the client continuation notification must retain a generated badge");
        let owner_to_client_badge =
            cyw43_sdio_peer_notification_badge(u32::from(topology.link.owner_to_client_slot))
                .expect("the owner completion notification must retain a generated badge");
        let line = format_cyw43_sdio_bus_link_diagnostic(
            super::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            topology,
            client_to_owner_badge,
            owner_to_client_badge,
        )
        .expect("the complete bounded diagnostic must fit");

        assert!(line.len() < CYW43_SDIO_BUS_LINK_DIAGNOSTIC_CAPACITY);
        assert!(line.contains("client_doorbell=notification"));
        assert!(line.contains("client_badge=256"));
        assert!(line.contains("owner_doorbell=notification"));
        assert!(line.contains("rights=send-only"));
        let (_, epoch) = line
            .rsplit_once("link_epoch=")
            .expect("the final diagnostic field must be link_epoch");
        assert_eq!(epoch.parse::<u32>(), Ok(topology.link.link_epoch));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn generated_serial_and_sdio_irqs_are_exact_distinct_owner_lanes() {
        let serial = super::generated_serial_runtime_irq()
            .expect("generated Pi 4 serial IRQ topology must remain valid");
        let topology = super::generated_cyw43_sdio_topology()
            .expect("generated Pi 4 SDIO IRQ topology must remain valid");
        let [sdio, dma] = topology.irqs;

        assert_eq!(serial.irq, pi4_driver_abi::DRIVER_RUNTIME_SERIAL_IRQ);
        assert_eq!(
            serial.badge,
            pi4_driver_abi::DRIVER_RUNTIME_SERIAL_IRQ_BADGE
        );
        assert_eq!(serial.handler_slot, sdio.handler_slot);
        assert_eq!(serial.notification_slot, sdio.notification_slot);
        assert_eq!(
            serial.trigger,
            crate::generated::DriverRuntimeIrqTrigger::Level
        );
        assert_ne!(serial.irq, sdio.irq);
        assert_ne!(serial.badge, sdio.badge);
        assert_eq!(dma.irq, pi4_driver_abi::DRIVER_RUNTIME_SDIO_DMA_IRQ);
        assert_eq!(dma.badge, pi4_driver_abi::DRIVER_RUNTIME_SDIO_DMA_IRQ_BADGE);
        assert_eq!(
            u32::from(dma.handler_slot),
            pi4_driver_abi::DRIVER_TASK_CHILD_SDIO_DMA_IRQ_HANDLER_SLOT
        );
        assert_eq!(dma.notification_slot, sdio.notification_slot);
        assert_eq!(
            dma.trigger,
            crate::generated::DriverRuntimeIrqTrigger::Level
        );
        assert_ne!(dma.handler_slot, sdio.handler_slot);
        assert_eq!(dma.badge & sdio.badge, 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn generated_serial_irq_requires_a_bound_child_notification() {
        assert_eq!(
            super::generated_driver_tcb_notification_binding_source(
                super::driver_task::SERIAL_DRIVER_TASK_CONTRACT,
            )
            .expect("generated serial IRQ topology"),
            Some("generated-serial-irq-topology"),
        );
        assert_eq!(
            super::generated_driver_tcb_notification_binding_source(
                super::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
            )
            .expect("generated SDIO IRQ topology"),
            Some("generated-cyw43-sdio-topology"),
        );
        assert_eq!(
            super::generated_driver_tcb_notification_binding_source(
                super::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            )
            .expect("generated CYW43 notification topology"),
            Some("generated-cyw43-sdio-topology"),
        );
        assert_eq!(
            super::generated_driver_tcb_notification_binding_source(
                super::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            )
            .expect("generated non-IRQ runtime topology"),
            None,
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn irq_trigger_words_match_arm_sel4_contract() {
        assert_eq!(IrqTrigger::Level.arm_trigger_word(), 0);
        assert_eq!(IrqTrigger::Edge.arm_trigger_word(), 1);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn driver_tcb_affinity_uses_manifest_policy_for_pi_contracts() {
        assert_eq!(
            super::apply_driver_tcb_affinity_for_boot(
                super::driver_task::SERIAL_DRIVER_TASK_CONTRACT,
                0x1000,
            ),
            Ok(Some(1))
        );
        assert_eq!(
            super::apply_driver_tcb_affinity_for_boot(
                super::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
                0x1001,
            ),
            Ok(Some(1))
        );
        assert_eq!(
            super::apply_driver_tcb_affinity_for_boot(
                super::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
                0x1002,
            ),
            Ok(Some(2))
        );
        assert_eq!(
            super::apply_driver_tcb_affinity_for_boot(
                super::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
                0x1003,
            ),
            Ok(Some(2))
        );
        assert_eq!(
            super::apply_driver_tcb_affinity_for_boot(
                super::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
                0x1004,
            ),
            Ok(Some(3))
        );
        assert_eq!(
            super::apply_driver_tcb_affinity_for_boot(
                super::driver_task::GENET_DRIVER_TASK_CONTRACT,
                0x1005,
            ),
            Ok(Some(3))
        );
        assert_eq!(
            super::apply_driver_tcb_affinity_for_boot(
                super::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
                0x1006,
            ),
            Ok(Some(3))
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn driver_task_bootstrap_report_covers_full_contract_set() {
        let mut report = super::DriverTaskBootstrapReport::default();
        for contract in super::DRIVER_TASK_BOOTSTRAP_CONTRACTS {
            let handle = fake_driver_task_handle(*contract, true, Some(2));
            super::add_driver_task_handle_to_report(&mut report, &handle);
        }
        super::finalize_driver_task_bootstrap_report(
            &mut report,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len(),
        );

        assert_eq!(
            report.configured_count,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len()
        );
        assert_eq!(report.failed_count, 0);
        assert_eq!(
            report.live_tcb_count,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len()
        );
        assert_eq!(
            report.live_tcb_role_mask & super::driver_task::REQUIRED_DRIVER_TASK_ROLE_MASK,
            super::driver_task::REQUIRED_DRIVER_TASK_ROLE_MASK
        );
        assert!(report.capset_proof);
        assert!(report.fault_proof);
        assert!(report.revoke_proof);
        assert!(report.sched_proof);
        assert_eq!(
            report.affinity_configured_count,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len()
        );
        assert_eq!(
            report.affinity_applied_count,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len()
        );
        assert!(report.affinity_proof);
        assert!(!report.vspace_proof);
        assert!(!report.pointer_free_ipc_proof);
        assert!(!report.owner_state_proof);
        assert_eq!(report.runtime_image_declared_count, 7);
        assert_eq!(report.runtime_image_transport_mapped_count, 0);
        assert_eq!(
            report.runtime_image_acceptance_count,
            super::driver_task::REQUIRED_PI4_ACCEPTANCE_HOT_PATHS
        );
        assert_eq!(
            report.runtime_image_declared_hot_path_mask,
            super::driver_task::REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK
        );
        assert_eq!(report.runtime_image_transport_mapped_hot_path_mask, 0);
        assert_eq!(report.broad_caps_leaked, 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn driver_task_bootstrap_report_proves_isolated_ring_only_for_all_contracts() {
        let mut report = super::DriverTaskBootstrapReport::default();
        for contract in super::DRIVER_TASK_BOOTSTRAP_CONTRACTS {
            let handle = fake_isolated_driver_task_handle(*contract, true, Some(3));
            super::add_driver_task_handle_to_report(&mut report, &handle);
        }
        super::finalize_driver_task_bootstrap_report(
            &mut report,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len(),
        );

        assert_eq!(
            report.isolated_vspace_count,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len()
        );
        assert_eq!(
            report.pointer_free_ipc_count,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len()
        );
        assert_eq!(report.runtime_image_declared_count, 7);
        assert_eq!(report.runtime_image_transport_mapped_count, 7);
        assert_eq!(
            report.runtime_image_acceptance_count,
            super::driver_task::REQUIRED_PI4_ACCEPTANCE_HOT_PATHS
        );
        assert_eq!(
            report.runtime_image_transport_mapped_hot_path_mask
                & super::driver_task::REQUIRED_PI4_ACCEPTANCE_HOT_PATH_MASK,
            super::driver_task::REQUIRED_PI4_ACCEPTANCE_HOT_PATH_MASK
        );
        assert!(report.vspace_proof);
        assert!(report.pointer_free_ipc_proof);
        assert!(!report.owner_state_proof);

        report.owner_state_hot_path_mask =
            super::driver_task::REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK;
        super::finalize_driver_task_bootstrap_report(
            &mut report,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len(),
        );
        assert!(report.owner_state_proof);

        report.pointer_free_ipc_count -= 1;
        report.owner_state_proof = true;
        super::finalize_driver_task_bootstrap_report(
            &mut report,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len(),
        );
        assert!(report.vspace_proof);
        assert!(!report.pointer_free_ipc_proof);
        assert!(!report.owner_state_proof);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn bootstrap_report_preserves_early_runtime_owner_state() {
        super::driver_task::publish_driver_task_bootstrap_report(
            super::DriverTaskBootstrapReport::default(),
        );
        assert!(
            super::driver_task::register_driver_task_runtime_owner_state(
                super::driver_task::DriverTaskHotPath::HdmiText,
            )
        );

        let mut report = super::DriverTaskBootstrapReport::default();
        let handle = fake_isolated_driver_task_handle(
            super::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
            true,
            Some(2),
        );
        super::add_driver_task_handle_to_report(&mut report, &handle);

        assert!(
            report.owner_state_hot_path_mask
                & super::driver_task::DriverTaskHotPath::HdmiText.owner_state_bit()
                != 0
        );
        assert!(
            report.owner_state_role_mask & super::driver_task::DRIVER_TASK_ROLE_DISPLAY_BIT != 0
        );
        super::driver_task::publish_driver_task_bootstrap_report(
            super::DriverTaskBootstrapReport::default(),
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn isolated_runtime_image_handles_do_not_credit_owner_state_by_declaration() {
        let pi_contracts = [
            super::driver_task::SERIAL_DRIVER_TASK_CONTRACT,
            super::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            super::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
            super::driver_task::GENET_DRIVER_TASK_CONTRACT,
            super::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            super::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
            super::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
        ];

        for contract in pi_contracts {
            let handle = fake_isolated_driver_task_handle(contract, true, Some(3));
            assert!(handle.runtime_image_spec.is_some(), "{}", contract.name);
            assert_ne!(
                handle.runtime_image_declared_region_mask, 0,
                "{}",
                contract.name
            );
            assert_eq!(
                handle.runtime_image_mapped_region_mask
                    & super::driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK,
                super::driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK,
                "{}",
                contract.name
            );
            assert!(
                handle.runtime_image_acceptance_eligible,
                "{}",
                contract.name
            );
            assert_eq!(
                handle.runtime_image_non_acceptance_reason, "acceptance-ready",
                "{}",
                contract.name
            );
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn isolated_runtime_transport_mapping_proves_pointer_free_ipc_shape() {
        assert!(super::runtime_image_transport_pointer_free_ipc_ready(
            super::driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK,
            super::driver_task::DriverTaskIpcAbi::SharedRingCommand,
        ));
        assert!(!super::runtime_image_transport_pointer_free_ipc_ready(
            super::driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK & !0x1,
            super::driver_task::DriverTaskIpcAbi::SharedRingCommand,
        ));
        assert!(!super::runtime_image_transport_pointer_free_ipc_ready(
            super::driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK,
            super::driver_task::DriverTaskIpcAbi::CallbackPointer,
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn isolated_runtime_ipc_buffer_uses_child_mapped_frame_cap() {
        assert_eq!(super::remote_tcb_ipc_buffer_frame_cap(0x104, 0x204), 0x204);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn physical_pi_bootstrap_contracts_match_generated_runtime_hot_paths() {
        let wifi = super::PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_WIFI_SELECTED;
        assert_eq!(wifi.len(), 6);
        for contract in [
            super::driver_task::SERIAL_DRIVER_TASK_CONTRACT,
            super::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
            super::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            super::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
            super::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
            super::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
        ] {
            assert!(wifi.contains(&contract), "{}", contract.name);
            assert!(
                super::driver_task::pi4_driver_task_runtime_image_spec_for_contract(contract)
                    .is_some(),
                "{}",
                contract.name
            );
        }
        assert!(!wifi.contains(&super::driver_task::GENET_DRIVER_TASK_CONTRACT));
        assert!(!wifi.contains(&super::driver_task::RTL8139_DRIVER_TASK_CONTRACT));
        assert!(!wifi.contains(&super::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT));

        let wired = super::PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_WIRED_SELECTED;
        assert_eq!(wired.len(), 5);
        for contract in [
            super::driver_task::SERIAL_DRIVER_TASK_CONTRACT,
            super::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
            super::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            super::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
            super::driver_task::GENET_DRIVER_TASK_CONTRACT,
        ] {
            assert!(wired.contains(&contract), "{}", contract.name);
            assert!(
                super::driver_task::pi4_driver_task_runtime_image_spec_for_contract(contract)
                    .is_some(),
                "{}",
                contract.name
            );
        }
        assert!(!wired.contains(&super::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT));
        assert!(!wired.contains(&super::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT));
        assert!(!wired.contains(&super::driver_task::RTL8139_DRIVER_TASK_CONTRACT));
        assert!(!wired.contains(&super::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT));

        let base = super::PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_BASE;
        assert_eq!(base.len(), 4);
        assert!(!base.contains(&super::driver_task::GENET_DRIVER_TASK_CONTRACT));
        assert!(!base.contains(&super::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT));
        assert!(!base.contains(&super::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT));

        let fault_topology = super::PHYSICAL_PI_DRIVER_TASK_FAULT_CONTRACTS;
        assert_eq!(fault_topology.len(), 7);
        assert!(fault_topology.iter().all(|contract| !matches!(
            *contract,
            super::driver_task::RTL8139_DRIVER_TASK_CONTRACT
                | super::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT
        )));
        let wifi_dormant: heapless::Vec<_, 3> = fault_topology
            .iter()
            .copied()
            .filter(|contract| !wifi.contains(contract))
            .collect();
        assert_eq!(
            wifi_dormant.as_slice(),
            &[super::driver_task::GENET_DRIVER_TASK_CONTRACT]
        );
        let wired_dormant: heapless::Vec<_, 3> = fault_topology
            .iter()
            .copied()
            .filter(|contract| !wired.contains(contract))
            .collect();
        assert_eq!(
            wired_dormant.as_slice(),
            &[
                super::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
                super::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            ]
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn pi_linked_runtime_only_cspace_bound_rejects_13_bits() {
        const CURRENT_LINKED_RUNTIME_PAGES: usize = 257;
        const DECLARED_LINKED_RUNTIME_PAGE_BOUND: usize = 320;
        const OBSERVED_EMPTY_START: usize = 0x0a6a;

        fn required_slots(contracts: &[super::DriverTaskContract], code_pages: usize) -> usize {
            let active = contracts
                .iter()
                .try_fold(
                    super::DRIVER_TASK_CSPACE_POST_BOOT_RESERVE,
                    |total, contract| {
                        let spec =
                            super::driver_task::pi4_driver_task_runtime_image_spec_for_contract(
                                *contract,
                            )?;
                        let slots = super::isolated_runtime_cspace_upper_bound(
                            spec,
                            code_pages,
                            *contract == super::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
                        )?;
                        total.checked_add(slots)
                    },
                )
                .expect("generated Pi contracts have a bounded CSpace estimate");
            let dormant_count = super::PHYSICAL_PI_DRIVER_TASK_FAULT_CONTRACTS
                .iter()
                .filter(|contract| !contracts.contains(contract))
                .count();
            active + dormant_count * super::DORMANT_DRIVER_FAULT_IDENTITY_ROOT_SLOTS
        }

        let capacity_13 = (1usize << 13) - OBSERVED_EMPTY_START;
        let capacity_14 = (1usize << 14) - OBSERVED_EMPTY_START;
        for contracts in [
            super::PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_WIFI_SELECTED,
            super::PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_WIRED_SELECTED,
        ] {
            for code_pages in [
                CURRENT_LINKED_RUNTIME_PAGES,
                DECLARED_LINKED_RUNTIME_PAGE_BOUND,
            ] {
                let required = required_slots(contracts, code_pages);
                assert!(
                    required > capacity_13,
                    "13-bit Pi CSpace must reject the linked-runtime-only upper bound"
                );
                assert!(
                    required <= capacity_14,
                    "the linked-runtime-only bound must fit a 14-bit aperture even though the full 256-Worker Pi profile selects 16 bits"
                );
            }
        }
        assert_eq!(super::DORMANT_DRIVER_FAULT_IDENTITY_ROOT_SLOTS, 11);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn framebuffer_budget_is_worst_case_and_alias_conservative() {
        let spec = super::driver_task::pi4_driver_task_runtime_image_spec_for_contract(
            super::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
        )
        .expect("HDMI runtime spec");
        let without_framebuffer =
            super::isolated_runtime_cspace_upper_bound(spec, 320, false).expect("bounded plan");
        let with_framebuffer =
            super::isolated_runtime_cspace_upper_bound(spec, 320, true).expect("bounded plan");

        assert_eq!(
            with_framebuffer - without_framebuffer,
            super::DRIVER_TASK_CSPACE_MAX_FRAMEBUFFER_PAGES
                * super::DRIVER_TASK_CSPACE_CAPS_PER_ALIASABLE_PAGE
        );
        assert_eq!(super::DRIVER_TASK_CSPACE_CAPS_PER_ALIASABLE_PAGE, 2);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn qemu_virtual_contracts_may_lack_pi_runtime_image_specs() {
        for contract in [
            super::driver_task::RTL8139_DRIVER_TASK_CONTRACT,
            super::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT,
        ] {
            let handle = fake_isolated_driver_task_handle(contract, true, Some(1));
            assert!(handle.runtime_image_spec.is_none(), "{}", contract.name);
            assert_eq!(
                handle.runtime_image_declared_region_mask, 0,
                "{}",
                contract.name
            );
            assert_eq!(
                handle.runtime_image_mapped_region_mask,
                super::driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK,
                "{}",
                contract.name
            );
            assert!(
                !handle.runtime_image_acceptance_eligible,
                "{}",
                contract.name
            );
            assert_eq!(
                handle.runtime_image_non_acceptance_reason, "qemu-compatibility-or-non-pi-contract",
                "{}",
                contract.name
            );
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn driver_task_bootstrap_order_creates_bus_owners_before_clients() {
        let sdio_index = super::DRIVER_TASK_BOOTSTRAP_CONTRACTS
            .iter()
            .position(|contract| *contract == super::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT)
            .expect("sdio-host contract is bootstrapped");
        let cyw_index = super::DRIVER_TASK_BOOTSTRAP_CONTRACTS
            .iter()
            .position(|contract| *contract == super::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT)
            .expect("cyw43 contract is bootstrapped");
        assert!(
            sdio_index < cyw_index,
            "sdio-host must publish transport caps before cyw43 is created"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn physical_pi_driver_bootstrap_presents_hdmi_before_bus_enumeration() {
        for contracts in [
            super::PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_WIFI_SELECTED,
            super::PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_WIRED_SELECTED,
            super::PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_BASE,
        ] {
            assert_eq!(
                contracts[0],
                super::driver_task::SERIAL_DRIVER_TASK_CONTRACT
            );
            assert_eq!(
                contracts[1],
                super::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT
            );
            let pcie_index = contracts
                .iter()
                .position(|contract| {
                    *contract == super::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT
                })
                .expect("physical Pi bootstrap includes PCIe");
            let usb_index = contracts
                .iter()
                .position(|contract| {
                    *contract == super::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT
                })
                .expect("physical Pi bootstrap includes USB");
            assert!(
                pcie_index < usb_index,
                "PCIe must still publish the xHCI bus link before USB construction"
            );
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn driver_task_bootstrap_report_fails_when_task_does_not_start() {
        let mut report = super::DriverTaskBootstrapReport::default();
        for (index, contract) in super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.iter().enumerate() {
            let handle = fake_driver_task_handle(*contract, index != 0, Some(2));
            super::add_driver_task_handle_to_report(&mut report, &handle);
        }
        super::finalize_driver_task_bootstrap_report(
            &mut report,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len(),
        );

        assert_eq!(
            report.configured_count,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len()
        );
        assert_eq!(
            report.live_tcb_count,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len() - 1
        );
        assert!(report.capset_proof);
        assert!(report.fault_proof);
        assert!(report.revoke_proof);
        assert!(!report.sched_proof);
        assert!(report.affinity_proof);
        assert!(!report.vspace_proof);
        assert!(!report.pointer_free_ipc_proof);
        assert!(!report.owner_state_proof);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn driver_task_bootstrap_report_fails_when_contract_is_missing() {
        let mut report = super::DriverTaskBootstrapReport {
            failed_count: 1,
            ..super::DriverTaskBootstrapReport::default()
        };
        for contract in super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.iter().skip(1) {
            let handle = fake_driver_task_handle(*contract, true, Some(2));
            super::add_driver_task_handle_to_report(&mut report, &handle);
        }
        super::finalize_driver_task_bootstrap_report(
            &mut report,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len(),
        );

        assert_eq!(
            report.configured_count,
            super::DRIVER_TASK_BOOTSTRAP_CONTRACTS.len() - 1
        );
        assert_eq!(report.failed_count, 1);
        assert!(!report.capset_proof);
        assert!(!report.fault_proof);
        assert!(!report.revoke_proof);
        assert!(!report.sched_proof);
        assert!(!report.affinity_proof);
        assert!(!report.vspace_proof);
        assert!(!report.pointer_free_ipc_proof);
        assert!(!report.owner_state_proof);
    }

    #[test]
    fn wifi_firmware_bundle_validation_rejects_missing_assets() {
        let missing_firmware = WifiFirmwareBundle::new(&[], b"nvram", Some(b"clm"), "cyw43455");
        assert_eq!(missing_firmware.validate(), Err("missing-firmware"));

        let missing_nvram = WifiFirmwareBundle::new(b"fw", &[], Some(b"clm"), "cyw43455");
        assert_eq!(missing_nvram.validate(), Err("missing-nvram"));

        let missing_board = WifiFirmwareBundle::new(b"fw", b"nvram", Some(b"clm"), " ");
        assert_eq!(missing_board.validate(), Err("missing-board-type"));
    }

    #[test]
    fn wifi_firmware_bundle_validation_accepts_minimal_bundle() {
        let bundle = WifiFirmwareBundle::new(b"fw", b"nvram", None, "cyw43455");
        assert_eq!(bundle.validate(), Ok(()));
    }

    #[test]
    fn sdio_helpers_report_expected_values() {
        assert_eq!(SdioBusWidth::OneBit.bits(), 1);
        assert_eq!(SdioBusWidth::FourBit.bits(), 4);
        assert_eq!(SdioFunction::Function0.number(), 0);
        assert_eq!(SdioFunction::Function1.number(), 1);
        assert_eq!(SdioFunction::Function2.number(), 2);
    }

    #[test]
    fn startup_link_rescue_budget_is_bounded() {
        assert_eq!(super::control_plane_startup_link_rescue_limit(), 2);
        assert!(!super::control_plane_startup_link_rescue_budget_exhausted(
            1
        ));
        assert!(super::control_plane_startup_link_rescue_budget_exhausted(2));
    }
}
