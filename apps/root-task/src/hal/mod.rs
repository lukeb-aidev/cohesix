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

#[cfg(any(feature = "kernel", feature = "cache-maintenance"))]
pub mod cache;

pub mod driver_task;

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
    DriverTaskBootstrapReport, DriverTaskContract, DriverTaskContractError, DriverTaskRuntimeProof,
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
    DriverRuntimeBusLinkDescriptor, DriverRuntimeInitDescriptor, DriverRuntimePageDescriptor,
    DriverRuntimeResourceRangeDescriptor, DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
    DRIVER_RUNTIME_BUS_LINK_CHANNEL_USB_PCIE, DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT,
    DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE, DRIVER_RUNTIME_FRAMEBUFFER_VADDR,
    DRIVER_RUNTIME_INIT_FLAG_BUS_ADDRESSING, DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS,
    DRIVER_RUNTIME_INIT_FLAG_DMA_PADDRS, DRIVER_RUNTIME_INIT_FLAG_FRAMEBUFFER,
    DRIVER_RUNTIME_INIT_FLAG_MMIO_MAPPED, DRIVER_RUNTIME_INIT_FLAG_POINTER_FREE,
    DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY, DRIVER_RUNTIME_INIT_FLAG_ROOT_CONTEXT_FORBIDDEN,
    DRIVER_RUNTIME_INIT_FLAG_SHARED_PADDRS, DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
    DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS, DRIVER_RUNTIME_RESOURCE_FLAG_ROOT_SHARED,
    DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS, DRIVER_RUNTIME_RESOURCE_KIND_DMA,
    DRIVER_RUNTIME_RESOURCE_KIND_FRAMEBUFFER, DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
    DRIVER_RUNTIME_RESOURCE_KIND_SHARED, DRIVER_RUNTIME_RESOURCE_PAGE_BYTES,
    DRIVER_RUNTIME_RESOURCE_TAG_CYW43_CONTROL, DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA,
    DRIVER_RUNTIME_RESOURCE_TAG_GENET_REGS, DRIVER_RUNTIME_RESOURCE_TAG_HDMI_FRAMEBUFFER,
    DRIVER_RUNTIME_RESOURCE_TAG_HDMI_REGS, DRIVER_RUNTIME_RESOURCE_TAG_PCIE_HOST,
    DRIVER_RUNTIME_RESOURCE_TAG_SDIO_HOST, DRIVER_RUNTIME_RESOURCE_TAG_SERIAL_MINI_UART,
    DRIVER_RUNTIME_RESOURCE_TAG_SHARED_CONTROL, DRIVER_RUNTIME_RESOURCE_TAG_USB_XHCI,
    DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE,
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
    fn probe_ht_clock(&mut self) -> Result<bool, HalError>;
    fn load_firmware(&mut self) -> Result<WifiDebugSnapshot, HalError>;
    fn retry_transport_and_firmware(&mut self) -> Result<WifiDebugSnapshot, HalError>;
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
    driver_tasks: heapless::Vec<KernelDriverTaskHandle, MAX_KERNEL_DRIVER_TASKS>,
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
    PCIE_ROOT_DRIVER_TASK_CONTRACT,
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    HDMI_TEXT_DRIVER_TASK_CONTRACT,
    SDIO_HOST_DRIVER_TASK_CONTRACT,
    CYW43_WIFI_DRIVER_TASK_CONTRACT,
];

#[cfg(feature = "kernel")]
const PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_WIRED_SELECTED: &[DriverTaskContract] = &[
    SERIAL_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT,
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    HDMI_TEXT_DRIVER_TASK_CONTRACT,
    GENET_DRIVER_TASK_CONTRACT,
];

#[cfg(feature = "kernel")]
const PHYSICAL_PI_DRIVER_TASK_BOOTSTRAP_CONTRACTS_BASE: &[DriverTaskContract] = &[
    SERIAL_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT,
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    HDMI_TEXT_DRIVER_TASK_CONTRACT,
];

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
#[derive(Clone, Copy, Debug)]
struct KernelDriverTaskHandle {
    contract: DriverTaskContract,
    role_bit: usize,
    tcb: seL4_CPtr,
    cnode: seL4_CPtr,
    command_endpoint: seL4_CPtr,
    notification: seL4_CPtr,
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
const PI4_DRIVER_RUNTIME_SDIO_MMIO_BASES: &[usize] = &[0xFE30_0000, 0x7E30_0000];
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_NO_MMIO_BASES: &[usize] = &[];
#[cfg(feature = "kernel")]
const PI4_DRIVER_RUNTIME_PCIE_MMIO_BASES: &[usize] = &[0xFD50_0000, 0xFE50_0000, 0x7D50_0000];

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
        if env.device_coverage(paddr, sel4::PAGE_BITS).is_none() {
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
const PI4_VL805_DMA_BUS_ALIAS_OR: u64 = 0x0000_0004_0000_0000;
#[cfg(feature = "kernel")]
const PI4_VL805_DMA_BUS_ALIAS_AND: u64 = 0x0000_0000_ffff_ffff;

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeInitDescriptorBuilder {
    descriptor: DriverRuntimeInitDescriptor,
    expected_mmio_pages: u16,
    expected_dma_pages: u16,
    expected_shared_pages: u16,
}

#[cfg(feature = "kernel")]
impl RuntimeInitDescriptorBuilder {
    fn new(spec: driver_task::DriverTaskRuntimeImageSpec, role_bit: usize) -> Self {
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
        descriptor.hot_path = spec.hot_path.as_u32();
        descriptor.role_bit = role_bit as u32;
        descriptor.flags = DRIVER_RUNTIME_INIT_FLAG_POINTER_FREE
            | DRIVER_RUNTIME_INIT_FLAG_BUS_ADDRESSING
            | DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY
            | DRIVER_RUNTIME_INIT_FLAG_ROOT_CONTEXT_FORBIDDEN;
        descriptor.mmio_vaddr_base = driver_task::DRIVER_TASK_DEVICE_MMIO_VADDR as u64;
        descriptor.dma_vaddr_base = driver_task::DRIVER_TASK_DMA_BUFFER_VADDR as u64;
        descriptor.shared_vaddr_base = driver_task::DRIVER_TASK_SHARED_BUFFER_VADDR as u64;
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
                descriptor.bus_link_count = 1;
                descriptor.bus_links[0] = DriverRuntimeBusLinkDescriptor::new(
                    driver_task::DriverTaskHotPath::SdioHost.as_u32(),
                    DRIVER_RUNTIME_BUS_LINK_CHANNEL_CYW43_SDIO,
                    DRIVER_RUNTIME_SHARED_PAYLOAD_OFFSET_BASE as u32,
                    driver_task::DRIVER_TASK_SDIO_BUS_SHARED_DATA_BYTES as u32,
                    DRIVER_RUNTIME_BUS_LINK_FLAG_CLIENT | DRIVER_RUNTIME_BUS_LINK_FLAG_POINTER_FREE,
                );
                descriptor.flags |= DRIVER_RUNTIME_INIT_FLAG_BUS_LINKS;
            }
            _ => {}
        }
        Self {
            descriptor,
            expected_mmio_pages: spec.region_pages(driver_task::DriverTaskRuntimeRegionKind::Mmio),
            expected_dma_pages: spec.region_pages(driver_task::DriverTaskRuntimeRegionKind::Dma),
            expected_shared_pages: spec
                .region_pages(driver_task::DriverTaskRuntimeRegionKind::SharedBuffer),
        }
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
            *slot = DriverRuntimePageDescriptor::new(paddr);
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
        let page_count = u16::try_from(pages)
            .map_err(|_| HalError::Unsupported("driver-runtime-init-mmio-range-pages"))?;
        self.add_resource_range(DriverRuntimeResourceRangeDescriptor::new(
            DRIVER_RUNTIME_RESOURCE_KIND_MMIO,
            DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS
                | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE,
            runtime_mmio_resource_tag(hot_path),
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
        let page_count = u16::try_from(pages)
            .map_err(|_| HalError::Unsupported("driver-runtime-init-buffer-range-pages"))?;
        let (tag, flags) = match kind {
            DRIVER_RUNTIME_RESOURCE_KIND_DMA => {
                let mut flags = DRIVER_RUNTIME_RESOURCE_FLAG_VADDR_CONTIGUOUS
                    | DRIVER_RUNTIME_RESOURCE_FLAG_DEVICE_VISIBLE;
                if paddr_contiguous {
                    flags |= DRIVER_RUNTIME_RESOURCE_FLAG_PADDR_CONTIGUOUS;
                }
                (DRIVER_RUNTIME_RESOURCE_TAG_DMA_ARENA, flags)
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
        self.add_resource_range(DriverRuntimeResourceRangeDescriptor::new(
            kind,
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
        if self.descriptor.valid_for_resources(
            self.descriptor.hot_path,
            self.descriptor.role_bit,
            self.expected_mmio_pages,
            self.expected_dma_pages,
            self.expected_shared_pages,
        ) {
            Ok(self.descriptor)
        } else {
            Err(HalError::Unsupported("driver-runtime-init-invalid"))
        }
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

    let mut engine_ready = true;
    driver_task::emit_driver_task_resource_init_status(
        contract,
        driver_task::DriverTaskHotPath::HdmiText,
        "hdmi-engine-init",
        "ready",
        None,
    );
    let mut banner = false;
    let banner_payload = b"\x0cStarting HDMI\n";
    let frame = driver_task::describe_driver_task_ring_frame(banner_payload, 0).ok_or(
        HalError::Unsupported("driver-runtime-hdmi-early-banner-stage"),
    )?;
    let draw_command = driver_task::DriverTaskCommandRecord::pi4_hot_path(
        0,
        spec.hot_path,
        driver_task::DriverTaskBudgetGrant::from_contract(contract),
        frame,
    );
    let banner_segments = [driver_task::DriverTaskStagingSegment::ring_frame(
        banner_payload,
        0,
    )];
    let draw_completion = driver_task::run_driver_task_ring_command_nonblocking_staged(
        contract,
        draw_command,
        &banner_segments,
    );
    let draw_ready = matches!(
        draw_completion,
        Some(done)
            if done.code == driver_task::DriverTaskCompletionCode::Progress.as_u16()
                && done.result != 0
    );
    let draw_status = if draw_ready {
        "ready"
    } else if draw_completion.is_some() {
        "unexpected-completion"
    } else {
        "no-reply"
    };
    driver_task::emit_driver_task_resource_init_status(
        contract,
        driver_task::DriverTaskHotPath::HdmiText,
        "hdmi-first-draw",
        draw_status,
        draw_completion,
    );
    if draw_ready {
        banner = true;
    } else {
        engine_ready = false;
    }
    if !engine_ready {
        crate::bootstrap::log::force_uart_line(
            "DRIVER_TASK_HDMI_EARLY_READY contract=hdmi-text engine_init=no owner_state=no banner=no action=serial-continues",
        );
        return Ok(false);
    }

    let owner_state = driver_task::register_driver_task_runtime_owner_state(
        driver_task::DriverTaskHotPath::HdmiText,
    );
    if owner_state && !banner {
        let frame = driver_task::describe_driver_task_ring_frame(banner_payload, 0).ok_or(
            HalError::Unsupported("driver-runtime-hdmi-early-banner-stage"),
        )?;
        let command = driver_task::DriverTaskCommandRecord::pi4_hot_path(
            0,
            spec.hot_path,
            driver_task::DriverTaskBudgetGrant::from_contract(contract),
            frame,
        );
        let banner_segments = [driver_task::DriverTaskStagingSegment::ring_frame(
            banner_payload,
            0,
        )];
        banner = matches!(
            driver_task::run_driver_task_ring_command_nonblocking_staged(
                contract,
                command,
                &banner_segments,
            ),
            Some(done)
                if done.code == driver_task::DriverTaskCompletionCode::Progress.as_u16()
                    && done.result != 0
        );
    }
    let mut line = heapless::String::<160>::new();
    let _ = fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "DRIVER_TASK_HDMI_EARLY_READY contract=hdmi-text engine_init=yes owner_state={} banner={} action=boot-progress-mirror",
            if owner_state { "yes" } else { "no" },
            if banner { "yes" } else { "no" },
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
            .checked_add(index.saturating_mul(phentsize))
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
        let page_end = segment_end.saturating_add(page_bytes - 1) & !(page_bytes - 1);
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
fn bind_driver_tcb_notification_for_boot(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
    notification: seL4_CPtr,
) -> Result<bool, HalError> {
    if driver_task::physical_pi_driver_task_only_owner_state_active() {
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
fn configure_driver_tcb_priority_for_boot(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
) -> Result<(u8, u8), HalError> {
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

#[cfg(feature = "kernel")]
fn restore_driver_tcb_steady_priority(
    contract: DriverTaskContract,
    tcb: seL4_CPtr,
    bootstrap_priority: u8,
    steady_priority: u8,
) -> Result<(), HalError> {
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
            driver_tasks: heapless::Vec::new(),
            driver_task_report: DriverTaskBootstrapReport::default(),
        }
    }

    /// Consumes bootstrap CSpace slots allocated before the HAL is initialised.
    pub fn consume_bootstrap_slots(&mut self, slots: usize) {
        self.env.consume_bootstrap_slots(slots);
    }

    /// Returns the underlying bootinfo pointer.
    pub fn bootinfo(&self) -> &'a sel4_sys::seL4_BootInfo {
        self.env.bootinfo()
    }

    /// Access to the underlying [`KernelEnv`] for transitional callers.
    pub fn as_env_mut(&mut self) -> &mut KernelEnv<'a> {
        &mut self.env
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

        let use_isolated_vspace =
            driver_task::physical_pi_driver_task_bootstrap_requires_isolated_vspace();

        let bootstrap_contracts = if driver_task::physical_pi_driver_task_only_owner_state_active()
        {
            physical_pi_driver_task_bootstrap_contracts()
        } else {
            DRIVER_TASK_BOOTSTRAP_CONTRACTS
        };

        for contract in bootstrap_contracts {
            let created = if use_isolated_vspace {
                self.create_isolated_driver_task(*contract, fault_endpoint)
            } else {
                self.create_driver_task(*contract, fault_endpoint)
            };
            match created {
                Ok(handle) => {
                    let _ = driver_task::register_pi4_bus_ring_service(*contract);
                    if self.driver_tasks.push(handle).is_err() {
                        report.failed_count = report.failed_count.saturating_add(1);
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
                            "DRIVER_TASK_BOOT contract={} role={} tcb=0x{:04x} cnode=0x{:04x} endpoint=0x{:04x} notification=0x{:04x} started={} affinity_core={} isolation_cspace=restricted vspace={} vspace_cap=0x{:04x} code_vaddr=0x{:08x} ring_vaddr=0x{:08x} ipc_abi={} pointer_free_ipc={} runtime_image={} runtime_declared=0x{:02x} runtime_mapped=0x{:02x} runtime_acceptance={} owner_state={} owner_state_reason={}",
                            handle.contract.name,
                            handle.contract.kind.proof_role(),
                            handle.tcb,
                            handle.cnode,
                            handle.command_endpoint,
                            handle.notification,
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

        finalize_driver_task_bootstrap_report(&mut report, bootstrap_contracts.len());
        self.driver_task_report = report;
        driver_task::publish_driver_task_bootstrap_report(report);
        report
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
                    if self.driver_tasks.push(handle).is_err() {
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
                            "DRIVER_TASK_BOOT_SMOKE phase=post-net-qemu contract={} role={} status=created tcb=0x{:04x} cnode=0x{:04x} endpoint=0x{:04x} notification=0x{:04x} started={} affinity_core={} isolation_cspace=restricted vspace={} vspace_cap=0x{:04x} code_vaddr=0x{:08x} ring_vaddr=0x{:08x} ipc_abi={} pointer_free_ipc={} proof={} runtime_image={} runtime_declared=0x{:02x} runtime_mapped=0x{:02x} runtime_acceptance={} owner_state=not-proven owner_state_reason={}",
                            handle.contract.name,
                            handle.contract.kind.proof_role(),
                            handle.tcb,
                            handle.cnode,
                            handle.command_endpoint,
                            handle.notification,
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
        let command_endpoint = self.env.alloc_endpoint().map_err(HalError::Sel4)?;
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

        let endpoint_err = sel4::cnode_mint_depth(
            child_cnode,
            driver_task::DRIVER_TASK_CHILD_COMMAND_SLOT,
            child_depth,
            root_cnode,
            command_endpoint,
            root_depth,
            sel4_sys::seL4_CapRights_All,
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
            sel4_sys::seL4_CapRights_All,
            0,
        );
        if notification_err != seL4_NoError {
            return Err(HalError::Sel4(notification_err));
        }

        let guard_bits = sel4::word_bits().saturating_sub(child_depth as seL4_Word);
        let cspace_root_data = sel4::cap_data_guard(0, guard_bits);
        sel4::set_tcb_space(
            tcb,
            driver_task::DRIVER_TASK_CHILD_FAULT_SLOT,
            child_cnode,
            cspace_root_data,
            sel4_sys::seL4_CapInitThreadVSpace,
            0,
        )
        .map_err(HalError::Sel4)?;

        let ipc_vaddr = ipc_frame.ptr().as_ptr() as usize;
        self.env
            .bind_remote_ipc_buffer(tcb, ipc_frame.cap(), ipc_vaddr)
            .map_err(HalError::Sel4)?;

        let (bootstrap_priority, steady_priority) =
            configure_driver_tcb_priority_for_boot(contract, tcb)?;
        driver_task::publish_driver_task_scheduler(contract, tcb as usize, steady_priority);

        let affinity_core = apply_driver_tcb_affinity_for_boot(contract, tcb)?;

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

        Ok(KernelDriverTaskHandle {
            contract,
            role_bit,
            tcb,
            cnode: child_cnode,
            command_endpoint,
            notification,
            fault_slot: driver_task::DRIVER_TASK_CHILD_FAULT_SLOT,
            ipc_frame: ipc_frame.cap(),
            stack_frame: stack_frame.cap(),
            ring_frame: Some(ring_frame.cap()),
            vspace: None,
            code_frame: None,
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
        let code_rights = sel4_sys::seL4_CapRights::new(0, 0, 1, 0);
        let data_rights = sel4_sys::seL4_CapRights_ReadWrite;
        let page_bytes = 1usize << sel4::PAGE_BITS;
        for page_index in 0..plan.page_count {
            let mut frame = self
                .env
                .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
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
            self.env
                .map_page_copy_into_vspace(
                    frame.cap(),
                    vspace,
                    vaddr,
                    if fill.writable {
                        data_rights
                    } else {
                        code_rights
                    },
                    sel4_sys::seL4_ARM_Page_Default,
                    tracker,
                )
                .map_err(HalError::Sel4)?;
            if fill.executable {
                crate::hal::cache::cache_unify_instruction(vspace, vaddr, page_bytes)
                    .map_err(|err| HalError::Sel4(err.code()))?;
            }
            self.env
                .unmap_page_cap(frame.cap())
                .map_err(HalError::Sel4)?;
        }
        Ok(RuntimeElfLoad {
            entry: plan.entry,
            code_vaddr: plan.base_vaddr,
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
        if page_count == 0 || width == 0 || page_count > 2048 {
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
            let frame = self.env.map_device(paddr).map_err(HalError::Sel4)?;
            self.env
                .map_page_copy_into_vspace(
                    frame.cap(),
                    vspace,
                    vaddr,
                    rights,
                    sel4_sys::seL4_ARM_Page_Uncached,
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
                        sel4_sys::seL4_ARM_Page_Uncached,
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

    fn runtime_ram_region_attr(_dma_owned: bool) -> sel4_sys::seL4_ARM_VMAttributes {
        sel4_sys::seL4_ARM_Page_Uncached
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
        let rights = sel4_sys::seL4_CapRights_ReadWrite;
        // Runtime DMA buffers and root-shared payload/control pages cross TCBs
        // without runtime-side EL0 cache maintenance.
        let attr = Self::runtime_ram_region_attr(dma_owned);
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
        if dma_owned {
            let mut frames: AllocVec<UnmappedRamFrame> = AllocVec::new();
            frames
                .try_reserve_exact(pages)
                .map_err(|_| HalError::Unsupported("driver-runtime-dma-plan-oom"))?;
            for page in 0..pages {
                let _ = runtime_region_page_vaddr(region, page)
                    .ok_or(HalError::Unsupported("driver-runtime-buffer-vaddr"))?;
                let frame = self
                    .env
                    .alloc_unmapped_ram_frame_attr(attr)
                    .map_err(HalError::Sel4)?;
                let paddr = frame.paddr();
                if page == 0 {
                    first_paddr = paddr;
                } else if paddr_contiguous
                    && !runtime_region_paddr_is_contiguous(first_paddr, page, page_bytes, paddr)
                {
                    paddr_contiguous = false;
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
        Ok(true)
    }

    fn install_cyw43_sdio_bus_link(
        &mut self,
        contract: DriverTaskContract,
        child_cnode: seL4_CPtr,
        child_depth: u8,
        vspace: seL4_CPtr,
        tracker: &mut VSpaceTableTracker,
    ) -> Result<(), HalError> {
        if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT {
            return Ok(());
        }
        let (sdio_endpoint, sdio_ring_frame, sdio_shared_frames) =
            driver_task::driver_task_bus_owner_transport_caps_with_shared(
                SDIO_HOST_DRIVER_TASK_CONTRACT,
                driver_task::DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY,
            )
            .ok_or(HalError::Unsupported(
                "driver-runtime-sdio-bus-link-missing",
            ))?;
        let root_cnode = self.env.init_cnode_cap();
        let root_depth = sel4::word_bits() as u8;
        let endpoint_err = sel4::cnode_mint_depth(
            child_cnode,
            driver_task::DRIVER_TASK_CHILD_SDIO_BUS_ENDPOINT_SLOT,
            child_depth,
            root_cnode,
            sdio_endpoint,
            root_depth,
            sel4_sys::seL4_CapRights_All,
            0,
        );
        if endpoint_err != seL4_NoError {
            return Err(HalError::Sel4(endpoint_err));
        }
        self.env
            .map_page_copy_into_vspace(
                sdio_ring_frame,
                vspace,
                driver_task::DRIVER_TASK_SDIO_BUS_RING_VADDR,
                sel4_sys::seL4_CapRights_ReadWrite,
                sel4_sys::seL4_ARM_Page_Uncached,
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
                    sel4_sys::seL4_ARM_Page_Uncached,
                    tracker,
                )
                .map_err(HalError::Sel4)?;
        }
        crate::bootstrap::log::force_uart_line(
            "DRIVER_TASK_BUS_LINK contract=cyw43455 owner=sdio-host channel=cyw43-sdio endpoint_slot=0x0008 ring_vaddr=0x70e00000 data_vaddr=0x70e01000 shared_len=8192",
        );
        Ok(())
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
        let endpoint_err = sel4::cnode_mint_depth(
            child_cnode,
            driver_task::DRIVER_TASK_CHILD_PCIE_BUS_ENDPOINT_SLOT,
            child_depth,
            root_cnode,
            pcie_endpoint,
            root_depth,
            sel4_sys::seL4_CapRights_All,
            0,
        );
        if endpoint_err != seL4_NoError {
            return Err(HalError::Sel4(endpoint_err));
        }
        self.env
            .map_page_copy_into_vspace(
                pcie_ring_frame,
                vspace,
                driver_task::DRIVER_TASK_PCIE_BUS_RING_VADDR,
                sel4_sys::seL4_CapRights_ReadWrite,
                sel4_sys::seL4_ARM_Page_Uncached,
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

        let page_bytes = 1usize << sel4::PAGE_BITS;
        let linked_runtime_image = runtime_image_spec.and_then(|spec| {
            driver_task::physical_pi_driver_task_only_owner_state_active()
                .then(|| driver_task::driver_runtime_image_bytes(spec.hot_path))
                .flatten()
        });
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
        let command_endpoint = self.env.alloc_endpoint().map_err(HalError::Sel4)?;
        let notification = self.env.alloc_notification().map_err(HalError::Sel4)?;
        let vspace = self.env.alloc_vspace_root().map_err(HalError::Sel4)?;
        self.env
            .assign_vspace_asid_from_init_pool(vspace)
            .map_err(HalError::Sel4)?;

        let mut ring_frame = self
            .env
            .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Uncached)
            .map_err(HalError::Sel4)?;
        let mut ipc_frame = self
            .env
            .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
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
                    .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
                    .map_err(HalError::Sel4)?,
            );
        }

        ring_frame.as_mut_slice().fill(0);
        ipc_frame.as_mut_slice().fill(0);
        for stack_frame in &mut stack_frames {
            stack_frame.as_mut_slice().fill(0);
        }

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

        let endpoint_err = sel4::cnode_mint_depth(
            child_cnode,
            driver_task::DRIVER_TASK_CHILD_COMMAND_SLOT,
            child_depth,
            root_cnode,
            command_endpoint,
            root_depth,
            sel4_sys::seL4_CapRights_All,
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
            sel4_sys::seL4_CapRights_All,
            0,
        );
        if notification_err != seL4_NoError {
            return Err(HalError::Sel4(notification_err));
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
                .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
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
            mapped_code_frame = Some(code_frame.cap());
            RuntimeElfLoad {
                entry: driver_task::isolated_trampoline_entry(),
                code_vaddr: trampoline_range.start,
            }
        };
        self.env
            .map_page_copy_into_vspace(
                ring_frame.cap(),
                vspace,
                driver_task::DRIVER_TASK_RING_VADDR,
                data_rights,
                sel4_sys::seL4_ARM_Page_Uncached,
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
                sel4_sys::seL4_ARM_Page_Default,
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
            self.env
                .map_page_copy_into_vspace(
                    stack_frame.cap(),
                    vspace,
                    vaddr,
                    data_rights,
                    sel4_sys::seL4_ARM_Page_Default,
                    &mut tracker,
                )
                .map_err(HalError::Sel4)?;
        }
        self.install_usb_pcie_bus_link(contract, child_cnode, child_depth, vspace, &mut tracker)?;
        self.install_cyw43_sdio_bus_link(contract, child_cnode, child_depth, vspace, &mut tracker)?;

        let mut runtime_init_descriptor =
            runtime_image_spec.map(|spec| RuntimeInitDescriptorBuilder::new(spec, role_bit));
        let runtime_image_mapped_region_mask = self.map_isolated_runtime_declared_regions(
            runtime_image_spec,
            vspace,
            &mut tracker,
            runtime_init_descriptor.as_mut(),
        )?;
        let runtime_init_descriptor = match runtime_init_descriptor {
            Some(builder) if driver_task::physical_pi_driver_task_only_owner_state_active() => {
                Some(builder.finish()?)
            }
            _ => None,
        };

        let guard_bits = sel4::word_bits().saturating_sub(child_depth as seL4_Word);
        let cspace_root_data = sel4::cap_data_guard(0, guard_bits);
        sel4::set_tcb_space(
            tcb,
            driver_task::DRIVER_TASK_CHILD_FAULT_SLOT,
            child_cnode,
            cspace_root_data,
            vspace,
            0,
        )
        .map_err(HalError::Sel4)?;

        self.env
            .bind_remote_ipc_buffer(
                tcb,
                remote_tcb_ipc_buffer_frame_cap(ipc_frame.cap(), child_ipc_frame),
                driver_task::DRIVER_TASK_IPC_VADDR,
            )
            .map_err(HalError::Sel4)?;

        let (bootstrap_priority, steady_priority) =
            configure_driver_tcb_priority_for_boot(contract, tcb)?;
        driver_task::publish_driver_task_scheduler(contract, tcb as usize, steady_priority);

        let affinity_core = apply_driver_tcb_affinity_for_boot(contract, tcb)?;

        let _notification_bound =
            bind_driver_tcb_notification_for_boot(contract, tcb, notification)?;
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
            let staging_segments = [driver_task::DriverTaskStagingSegment::ring_frame(
                descriptor_bytes,
                0,
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
            restore_driver_tcb_steady_priority(contract, tcb, bootstrap_priority, steady_priority)?;
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
        if let Some(code_frame) = mapped_code_frame {
            self.env
                .unmap_page_cap(code_frame)
                .map_err(HalError::Sel4)?;
        }
        let affinity_core =
            apply_driver_tcb_affinity_after_bootstrap(contract, tcb, affinity_core)?;
        self.env
            .unmap_page_cap(ipc_frame.cap())
            .map_err(HalError::Sel4)?;
        for stack_frame in &stack_frames {
            self.env
                .unmap_page_cap(stack_frame.cap())
                .map_err(HalError::Sel4)?;
        }
        let stack_frame_cap = stack_frames
            .first()
            .map(RamFrame::cap)
            .ok_or(HalError::Unsupported("driver-runtime-stack-empty"))?;

        Ok(KernelDriverTaskHandle {
            contract,
            role_bit,
            tcb,
            cnode: child_cnode,
            command_endpoint,
            notification,
            fault_slot: driver_task::DRIVER_TASK_CHILD_FAULT_SLOT,
            ipc_frame: ipc_frame.cap(),
            stack_frame: stack_frame_cap,
            ring_frame: Some(ring_frame.cap()),
            vspace: Some(vspace),
            code_frame: mapped_code_frame,
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

    /// Creates an IRQHandler and badged notification cap for a device IRQ.
    ///
    /// The returned binding is intentionally inert until the driver clears or
    /// masks its device-side interrupt source and calls [`Self::ack_irq`].
    pub fn bind_irq_notification(
        &mut self,
        irq: Irq,
        trigger: IrqTrigger,
    ) -> Result<KernelIrqBinding, HalError> {
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

        let notification_slot = self.env.alloc_notification().map_err(|err| {
            let _ = sel4::cnode_delete(root_cnode, handler_slot, depth);
            HalError::Sel4(err)
        })?;

        let badged_notification_slot = self.env.allocate_slot();
        let badge = irq_notification_badge(irq);
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
            let _ = sel4::cnode_delete(root_cnode, notification_slot, depth);
            let _ = sel4::cnode_delete(root_cnode, handler_slot, depth);
            return Err(HalError::Sel4(mint_err));
        }

        let bind_err = sel4::irq_handler_set_notification(handler_slot, badged_notification_slot);
        if bind_err != seL4_NoError {
            let _ = sel4::irq_handler_clear(handler_slot);
            let _ = sel4::cnode_delete(root_cnode, badged_notification_slot, depth);
            let _ = sel4::cnode_delete(root_cnode, notification_slot, depth);
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

        for err in [
            sel4::irq_handler_clear(binding.handler_slot),
            sel4::cnode_delete(root_cnode, binding.badged_notification_slot, depth),
            sel4::cnode_delete(root_cnode, binding.notification_slot, depth),
            sel4::cnode_delete(root_cnode, binding.handler_slot, depth),
        ] {
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

    fn probe_ht_clock(&mut self) -> Result<bool, HalError> {
        Err(self.linked_runtime_required())
    }

    fn load_firmware(&mut self) -> Result<WifiDebugSnapshot, HalError> {
        Err(self.linked_runtime_required())
    }

    fn retry_transport_and_firmware(&mut self) -> Result<WifiDebugSnapshot, HalError> {
        Err(self.linked_runtime_required())
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
    use super::{irq_notification_badge, Irq, IrqTrigger};

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
            command_endpoint: 0x102,
            notification: 0x103,
            fault_slot: super::driver_task::DRIVER_TASK_CHILD_FAULT_SLOT,
            ipc_frame: 0x104,
            stack_frame: 0x105,
            ring_frame: None,
            vspace: None,
            code_frame: None,
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
    fn pi4_runtime_mmio_candidates_keep_cyw43_behind_sdio_runtime() {
        let cyw43 =
            super::runtime_mmio_candidate_bases(super::driver_task::DriverTaskHotPath::Cyw43Wifi);
        let sdio =
            super::runtime_mmio_candidate_bases(super::driver_task::DriverTaskHotPath::SdioHost);
        assert!(cyw43.is_empty());
        assert_eq!(sdio, super::PI4_DRIVER_RUNTIME_SDIO_MMIO_BASES);
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
    fn runtime_ram_region_attr_keeps_shared_payload_pages_uncached() {
        assert_eq!(
            super::KernelHal::runtime_ram_region_attr(true),
            sel4_sys::seL4_ARM_Page_Uncached
        );
        assert_eq!(
            super::KernelHal::runtime_ram_region_attr(false),
            sel4_sys::seL4_ARM_Page_Uncached
        );
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
    #[test]
    fn runtime_init_descriptor_builder_records_primitive_page_metadata() {
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            super::driver_task::DriverTaskHotPath::GenetNic,
            1,
            1,
            1,
            1,
            1,
            true,
            false,
        );
        let mut builder = super::RuntimeInitDescriptorBuilder::new(
            spec,
            super::driver_task::DRIVER_TASK_ROLE_NET_BIT,
        );
        builder.add_mmio_page(0xFD58_0000).unwrap();
        builder.add_dma_page(0x4000_0000).unwrap();
        builder.add_shared_page(0x5000_0000).unwrap();

        let descriptor = builder.finish().unwrap();
        assert_eq!(
            descriptor.hot_path,
            super::driver_task::DriverTaskHotPath::GenetNic.as_u32()
        );
        assert_eq!(
            descriptor.role_bit as usize,
            super::driver_task::DRIVER_TASK_ROLE_NET_BIT
        );
        assert_eq!(descriptor.mmio_page_count, 1);
        assert_eq!(descriptor.dma_page_count, 1);
        assert_eq!(descriptor.shared_page_count, 1);
        assert_eq!(descriptor.mmio_pages[0].paddr, 0xFD58_0000);
        assert_eq!(descriptor.dma_pages[0].paddr, 0x4000_0000);
        assert_eq!(descriptor.shared_pages[0].paddr, 0x5000_0000);
        assert!(descriptor.valid());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_init_descriptor_builder_records_hdmi_framebuffer_metadata() {
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            super::driver_task::DriverTaskHotPath::HdmiText,
            1,
            1,
            1,
            1,
            1,
            true,
            false,
        );
        let mut builder = super::RuntimeInitDescriptorBuilder::new(
            spec,
            super::driver_task::DRIVER_TASK_ROLE_DISPLAY_BIT,
        );
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
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_init_descriptor_builder_records_semantic_ranges_and_bus_links() {
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            super::driver_task::DriverTaskHotPath::UsbKeyboard,
            64,
            16,
            512,
            128,
            32,
            true,
            false,
        );
        let mut builder = super::RuntimeInitDescriptorBuilder::new(
            spec,
            super::driver_task::DRIVER_TASK_ROLE_USB_BIT,
        );
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
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            super::driver_task::DriverTaskHotPath::Cyw43Wifi,
            64,
            16,
            0,
            0,
            64,
            false,
            true,
        );
        let mut builder = super::RuntimeInitDescriptorBuilder::new(
            spec,
            super::driver_task::DRIVER_TASK_ROLE_NET_BIT,
        );
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
        assert!(descriptor.has_pointer_free_bus_link(
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
    fn runtime_init_descriptor_builder_keeps_large_buffer_budgets_semantic() {
        let spec = super::driver_task::DriverTaskRuntimeImageSpec::new(
            super::driver_task::DriverTaskHotPath::GenetNic,
            64,
            16,
            6,
            512,
            32,
            true,
            false,
        );
        let mut builder = super::RuntimeInitDescriptorBuilder::new(
            spec,
            super::driver_task::DRIVER_TASK_ROLE_NET_BIT,
        );
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
    fn runtime_elf_loader_plans_multiple_load_segments() {
        fn put16(bytes: &mut [u8], offset: usize, value: u16) {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn put32(bytes: &mut [u8], offset: usize, value: u32) {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        fn put64(bytes: &mut [u8], offset: usize, value: u64) {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        fn phdr(
            bytes: &mut [u8],
            index: usize,
            flags: u32,
            offset: u64,
            vaddr: u64,
            filesz: u64,
            memsz: u64,
        ) {
            let base = 64 + index * 56;
            put32(bytes, base, 1);
            put32(bytes, base + 4, flags);
            put64(bytes, base + 8, offset);
            put64(bytes, base + 16, vaddr);
            put64(bytes, base + 24, vaddr);
            put64(bytes, base + 32, filesz);
            put64(bytes, base + 40, memsz);
            put64(bytes, base + 48, 0x10000);
        }

        let mut image = vec![0u8; 0x5000];
        image[0..4].copy_from_slice(b"\x7fELF");
        image[4] = 2;
        image[5] = 1;
        put16(&mut image, 16, 2);
        put16(&mut image, 18, 183);
        put64(&mut image, 24, 0x210010);
        put64(&mut image, 32, 64);
        put16(&mut image, 52, 64);
        put16(&mut image, 54, 56);
        put16(&mut image, 56, 3);
        phdr(&mut image, 0, 4, 0x1000, 0x200000, 0x10, 0x10);
        phdr(&mut image, 1, 5, 0x2000, 0x210000, 0x1200, 0x1200);
        phdr(&mut image, 2, 6, 0x4000, 0x226000, 0x20, 0x80);
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
        assert_eq!(page[0], 0xaa);
        assert_eq!(page[0x0fff], 0xaa);

        let data_page = (0x226000 - plan.base_vaddr) / 4096;
        let fill = super::fill_runtime_elf_page(&image, plan, data_page, &mut page).unwrap();
        assert!(fill.writable);
        assert!(!fill.executable);
        assert_eq!(page[0], 0xbb);
        assert_eq!(page[0x1f], 0xbb);
        assert_eq!(page[0x20], 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn irq_notification_badges_are_nonzero_and_irq_derived() {
        assert_eq!(irq_notification_badge(Irq(0)), 1);
        assert_eq!(irq_notification_badge(Irq(143)), 144);
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
