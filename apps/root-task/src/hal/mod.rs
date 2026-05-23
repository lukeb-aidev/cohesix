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

#[cfg(feature = "kernel")]
pub mod bcmgenet;

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
use crate::drivers::cyw43;
#[cfg(feature = "kernel")]
use crate::hal::driver_task::{
    DriverTaskBootstrapReport, DriverTaskContract, DriverTaskContractError, DriverTaskRuntimeProof,
    CYW43_WIFI_DRIVER_TASK_CONTRACT, GENET_DRIVER_TASK_CONTRACT, HDMI_TEXT_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT, RTL8139_DRIVER_TASK_CONTRACT, SDIO_HOST_DRIVER_TASK_CONTRACT,
    SERIAL_DRIVER_TASK_CONTRACT, USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    VIRTIO_NET_DRIVER_TASK_CONTRACT,
};
#[cfg(feature = "kernel")]
use crate::sel4::{
    self, DeviceCoverage, DeviceFrame, KernelEnv, KernelEnvSnapshot, RamFrame, VSpaceTableTracker,
};
#[cfg(feature = "kernel")]
use pci::{PciAddress, PciTopology};
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
        Ok(Self { base_paddr, pages })
    }

    /// Constructs a single-page register mapping.
    pub fn single(frame: DeviceFrame) -> Result<Self, HalError> {
        let base_paddr = frame.paddr();
        let mut pages = heapless::Vec::new();
        pages
            .push(frame)
            .map_err(|_| HalError::Unsupported("register-page-capacity"))?;
        Ok(Self { base_paddr, pages })
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

    /// Drives the Wi-Fi power control line.
    fn wifi_set_power(&mut self, _state: WifiPowerState) -> Result<(), Self::Error>
    where
        Self::Error: From<HalError>,
    {
        Err(HalError::Unsupported("wifi-power").into())
    }

    /// Drives the Wi-Fi reset control line.
    fn wifi_set_reset(&mut self, _state: WifiResetState) -> Result<(), Self::Error>
    where
        Self::Error: From<HalError>,
    {
        Err(HalError::Unsupported("wifi-reset").into())
    }

    /// Returns whether the Wi-Fi out-of-band interrupt line is pending.
    fn wifi_oob_irq_pending(&self) -> Result<bool, Self::Error>
    where
        Self::Error: From<HalError>,
    {
        Err(HalError::Unsupported("wifi-oob-irq").into())
    }

    /// Acknowledges a Wi-Fi out-of-band interrupt indication.
    fn wifi_ack_oob_irq(&mut self) -> Result<(), Self::Error>
    where
        Self::Error: From<HalError>,
    {
        Err(HalError::Unsupported("wifi-oob-irq-ack").into())
    }

    /// Resets the SDIO host/controller before Wi-Fi attach.
    fn sdio_reset_host(&mut self) -> Result<(), Self::Error>
    where
        Self::Error: From<HalError>,
    {
        Err(HalError::Unsupported("sdio-reset-host").into())
    }

    /// Applies the requested SDIO bus width and returns the effective width in bits.
    fn sdio_set_bus_width(&mut self, _width: SdioBusWidth) -> Result<(), Self::Error>
    where
        Self::Error: From<HalError>,
    {
        Err(HalError::Unsupported("sdio-set-bus-width").into())
    }

    /// Applies the requested SDIO clock and returns the effective clock rate in hertz.
    fn sdio_set_clock_hz(&mut self, _target_hz: u32) -> Result<u32, Self::Error>
    where
        Self::Error: From<HalError>,
    {
        Err(HalError::Unsupported("sdio-set-clock").into())
    }

    /// Executes a CMD52-style SDIO direct read.
    fn sdio_io_direct_read(
        &mut self,
        _function: SdioFunction,
        _addr: u32,
    ) -> Result<u8, Self::Error>
    where
        Self::Error: From<HalError>,
    {
        Err(HalError::Unsupported("sdio-io-direct-read").into())
    }

    /// Executes a CMD52-style SDIO direct write.
    fn sdio_io_direct_write(
        &mut self,
        _function: SdioFunction,
        _addr: u32,
        _value: u8,
    ) -> Result<(), Self::Error>
    where
        Self::Error: From<HalError>,
    {
        Err(HalError::Unsupported("sdio-io-direct-write").into())
    }

    /// Executes a CMD53-style SDIO extended transfer in-place.
    fn sdio_io_extended(
        &mut self,
        _function: SdioFunction,
        _addr: u32,
        _increment_addr: bool,
        _write: bool,
        _buffer: &mut [u8],
    ) -> Result<(), Self::Error>
    where
        Self::Error: From<HalError>,
    {
        Err(HalError::Unsupported("sdio-io-extended").into())
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
    #[cfg(not(all(
        target_arch = "aarch64",
        target_os = "none",
        not(feature = "net-backend-virtio")
    )))]
    pi4_wifi: Option<pi4_wifi::Pi4WifiState>,
    driver_tasks: heapless::Vec<KernelDriverTaskHandle, MAX_KERNEL_DRIVER_TASKS>,
    driver_task_report: DriverTaskBootstrapReport,
}

#[cfg(feature = "kernel")]
const MAX_KERNEL_DRIVER_TASKS: usize = 9;

#[cfg(feature = "kernel")]
const DRIVER_TASK_BOOTSTRAP_CONTRACTS: &[DriverTaskContract] = &[
    SERIAL_DRIVER_TASK_CONTRACT,
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    HDMI_TEXT_DRIVER_TASK_CONTRACT,
    GENET_DRIVER_TASK_CONTRACT,
    CYW43_WIFI_DRIVER_TASK_CONTRACT,
    RTL8139_DRIVER_TASK_CONTRACT,
    VIRTIO_NET_DRIVER_TASK_CONTRACT,
    SDIO_HOST_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT,
];

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
    spec.and_then(driver_task::DriverTaskRuntimeImageSpec::non_acceptance_reason)
        .unwrap_or("qemu-compatibility-or-non-pi-contract")
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
    report.owner_state_proof = report.vspace_proof
        && report.pointer_free_ipc_proof
        && report.owner_state_hot_path_mask & driver_task::REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK
            == driver_task::REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK;
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
            #[cfg(not(all(
                target_arch = "aarch64",
                target_os = "none",
                not(feature = "net-backend-virtio")
            )))]
            pi4_wifi: None,
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

        for contract in DRIVER_TASK_BOOTSTRAP_CONTRACTS {
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
                            if handle.runtime_image_acceptance_eligible {
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

        finalize_driver_task_bootstrap_report(&mut report, DRIVER_TASK_BOOTSTRAP_CONTRACTS.len());
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
                            "DRIVER_TASK_BOOT_SMOKE phase=post-net-qemu contract={} role={} status=created tcb=0x{:04x} cnode=0x{:04x} endpoint=0x{:04x} notification=0x{:04x} started={} affinity_core={} isolation_cspace=restricted vspace={} vspace_cap=0x{:04x} code_vaddr=0x{:08x} ring_vaddr=0x{:08x} ipc_abi={} pointer_free_ipc={} proof={} runtime_image={} runtime_declared=0x{:02x} runtime_mapped=0x{:02x} runtime_acceptance={} owner_state=root-owned owner_state_reason={}",
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
            .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
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

        let priority = contract.sel4_priority();
        sel4::set_tcb_sched_params(tcb, sel4_sys::seL4_CapInitThreadTCB, priority, priority)
            .map_err(HalError::Sel4)?;
        sel4::set_tcb_priority(tcb, sel4_sys::seL4_CapInitThreadTCB, priority)
            .map_err(HalError::Sel4)?;

        let affinity_target =
            driver_affinity_target(contract).ok_or(HalError::Unsupported("driver-affinity"))?;
        let affinity_policy = affinity::policy();
        let affinity_core =
            affinity::apply_driver_tcb_affinity(tcb, affinity_target, &affinity_policy)
                .map_err(HalError::Affinity)?;

        sel4::bind_tcb_notification(tcb, notification).map_err(HalError::Sel4)?;

        let stack_top = (stack_frame.ptr().as_ptr() as usize + (1usize << sel4::PAGE_BITS)) & !0xf;
        sel4::write_tcb_registers(
            tcb,
            driver_task::driver_task_entry as *const () as usize,
            stack_top,
            task_key as seL4_Word,
            false,
        )
        .map_err(HalError::Sel4)?;
        sel4::resume_tcb(tcb).map_err(HalError::Sel4)?;
        let started = driver_task::wait_for_driver_task_start(task_key, 256);

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
            pointer_free_ipc: false,
            started,
        })
    }

    fn create_isolated_driver_task(
        &mut self,
        contract: DriverTaskContract,
        fault_endpoint: seL4_CPtr,
    ) -> Result<KernelDriverTaskHandle, HalError> {
        contract.validate().map_err(HalError::DriverTaskContract)?;
        if !driver_task::isolated_trampoline_supported() {
            return Err(HalError::Unsupported("driver-task-isolated-trampoline"));
        }

        let role_bit = driver_task::driver_task_role_bit(contract.kind);
        if role_bit == 0 {
            return Err(HalError::Unsupported("driver-task-role"));
        }
        let task_key = driver_task::driver_task_contract_key(contract)
            .ok_or(HalError::Unsupported("driver-task-key"))?;
        let runtime_image_spec =
            driver_task::pi4_driver_task_runtime_image_spec_for_contract(contract);

        let trampoline_range = driver_task::isolated_trampoline_range();
        let page_bytes = 1usize << sel4::PAGE_BITS;
        if trampoline_range.start == 0
            || trampoline_range.end <= trampoline_range.start
            || trampoline_range.start & (page_bytes - 1) != 0
            || trampoline_range.end - trampoline_range.start > page_bytes
        {
            return Err(HalError::Unsupported("driver-task-trampoline-layout"));
        }

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

        let mut code_frame = self
            .env
            .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
            .map_err(HalError::Sel4)?;
        let mut ring_frame = self
            .env
            .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
            .map_err(HalError::Sel4)?;
        let mut ipc_frame = self
            .env
            .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
            .map_err(HalError::Sel4)?;
        let mut stack_frame = self
            .env
            .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Default)
            .map_err(HalError::Sel4)?;

        code_frame.as_mut_slice().fill(0);
        // SAFETY: The linker script page-aligns `.driver_task_text`, the range
        // check above bounds it to one mapped user-image page, and this copy
        // reads only that page into a HAL-owned frame.
        let source =
            unsafe { core::slice::from_raw_parts(trampoline_range.start as *const u8, page_bytes) };
        code_frame.as_mut_slice().copy_from_slice(source);
        ring_frame.as_mut_slice().fill(0);
        ipc_frame.as_mut_slice().fill(0);
        stack_frame.as_mut_slice().fill(0);

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

        let mut tracker = VSpaceTableTracker::new();
        let code_rights = sel4_sys::seL4_CapRights::new(0, 0, 1, 0);
        let data_rights = sel4_sys::seL4_CapRights_ReadWrite;
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
        self.env
            .map_page_copy_into_vspace(
                ring_frame.cap(),
                vspace,
                driver_task::DRIVER_TASK_RING_VADDR,
                data_rights,
                sel4_sys::seL4_ARM_Page_Default,
                &mut tracker,
            )
            .map_err(HalError::Sel4)?;
        self.env
            .map_page_copy_into_vspace(
                ipc_frame.cap(),
                vspace,
                driver_task::DRIVER_TASK_IPC_VADDR,
                data_rights,
                sel4_sys::seL4_ARM_Page_Default,
                &mut tracker,
            )
            .map_err(HalError::Sel4)?;
        self.env
            .map_page_copy_into_vspace(
                stack_frame.cap(),
                vspace,
                driver_task::DRIVER_TASK_STACK_BOTTOM_VADDR,
                data_rights,
                sel4_sys::seL4_ARM_Page_Default,
                &mut tracker,
            )
            .map_err(HalError::Sel4)?;

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
            .bind_remote_ipc_buffer(tcb, ipc_frame.cap(), driver_task::DRIVER_TASK_IPC_VADDR)
            .map_err(HalError::Sel4)?;

        let priority = contract.sel4_priority();
        sel4::set_tcb_sched_params(tcb, sel4_sys::seL4_CapInitThreadTCB, priority, priority)
            .map_err(HalError::Sel4)?;
        sel4::set_tcb_priority(tcb, sel4_sys::seL4_CapInitThreadTCB, priority)
            .map_err(HalError::Sel4)?;

        let affinity_target =
            driver_affinity_target(contract).ok_or(HalError::Unsupported("driver-affinity"))?;
        let affinity_policy = affinity::policy();
        let affinity_core =
            affinity::apply_driver_tcb_affinity(tcb, affinity_target, &affinity_policy)
                .map_err(HalError::Affinity)?;

        sel4::bind_tcb_notification(tcb, notification).map_err(HalError::Sel4)?;
        sel4::write_tcb_registers(
            tcb,
            driver_task::isolated_trampoline_entry(),
            driver_task::DRIVER_TASK_STACK_TOP_VADDR,
            task_key as seL4_Word,
            false,
        )
        .map_err(HalError::Sel4)?;
        sel4::resume_tcb(tcb).map_err(HalError::Sel4)?;

        let command = driver_task::DriverTaskCommandRecord::service(
            0,
            driver_task::DriverTaskBudgetGrant::from_contract(contract),
        );
        let completion = driver_task::run_driver_task_ring_command(contract, command);
        let pointer_free_ipc = matches!(
            completion,
            Some(done)
                if done.code == driver_task::DriverTaskCompletionCode::Progress.as_u16()
                    && done.result == task_key as u32
        );

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
            vspace: Some(vspace),
            code_frame: Some(code_frame.cap()),
            runtime_image_spec,
            runtime_image_declared_region_mask: runtime_image_declared_region_mask(
                runtime_image_spec,
            ),
            runtime_image_mapped_region_mask:
                driver_task::DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK,
            runtime_image_acceptance_eligible: runtime_image_acceptance_eligible(
                runtime_image_spec,
            ),
            runtime_image_non_acceptance_reason: runtime_image_non_acceptance_reason(
                runtime_image_spec,
            ),
            code_vaddr: trampoline_range.start,
            ipc_vaddr: driver_task::DRIVER_TASK_IPC_VADDR,
            ring_vaddr: driver_task::DRIVER_TASK_RING_VADDR,
            stack_top: driver_task::DRIVER_TASK_STACK_TOP_VADDR,
            affinity_core,
            vspace_isolated: true,
            pointer_free_ipc,
            started: pointer_free_ipc,
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

    fn pi4_wifi_state(&mut self) -> Result<&mut pi4_wifi::Pi4WifiState, HalError> {
        #[cfg(all(
            target_arch = "aarch64",
            target_os = "none",
            not(feature = "net-backend-virtio")
        ))]
        {
            return Err(HalError::Unsupported(
                "pi4-wifi-driver-task-runtime-required",
            ));
        }
        #[cfg(not(all(
            target_arch = "aarch64",
            target_os = "none",
            not(feature = "net-backend-virtio")
        )))]
        {
            if self.pi4_wifi.is_none() {
                self.pi4_wifi = Some(pi4_wifi::Pi4WifiState::new(self)?);
            }
            self.pi4_wifi
                .as_mut()
                .ok_or(HalError::Unsupported("pi4-wifi-state"))
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

    #[allow(unsafe_code)]
    fn hal_mut(&mut self) -> Result<&'static mut KernelHal<'static>, HalError> {
        if self.hal_ptr == 0 {
            return Err(HalError::Unsupported("wifi-debug-handle"));
        }

        // SAFETY: `hal_ptr` is derived from the leaked bootstrap `KernelHal`
        // and remains valid for the process lifetime. The handle only
        // materializes a temporary mutable reference while servicing a single
        // root-console Wi-Fi debug command.
        Ok(unsafe { &mut *(self.hal_ptr as *mut KernelHal<'static>) })
    }
}

#[cfg(feature = "kernel")]
impl WifiDebugOps for KernelWifiDebugHandle {
    fn dump_state(&mut self, stage: &'static str) -> Result<WifiDebugSnapshot, HalError> {
        self.hal_mut()?.pi4_wifi_state()?.debug_dump_state(stage)
    }

    fn firmware_contract_trace(&mut self) -> Option<WifiFirmwareContractTrace> {
        self.hal_mut()
            .ok()
            .and_then(|hal| hal.pi4_wifi_state().ok())
            .map(|state| state.debug_firmware_contract_trace())
    }

    fn sdhci_contract_trace(&mut self) -> Option<WifiSdhciContractTrace> {
        self.hal_mut()
            .ok()
            .and_then(|hal| hal.pi4_wifi_state().ok())
            .map(|state| state.debug_sdhci_contract_trace())
    }

    fn control_plane_trace(&mut self) -> Option<WifiControlPlaneTrace> {
        self.hal_mut()
            .ok()
            .and_then(|hal| hal.pi4_wifi_state().ok())
            .map(|state| state.debug_control_plane_trace())
    }

    fn probe_ht_clock(&mut self) -> Result<bool, HalError> {
        self.hal_mut()?.pi4_wifi_state()?.debug_probe_ht_clock()
    }

    fn load_firmware(&mut self) -> Result<WifiDebugSnapshot, HalError> {
        let state = self.hal_mut()?.pi4_wifi_state()?;
        cyw43::debug_load_firmware_from_transport(state)?;
        state.debug_dump_state("console-load-fw")
    }

    fn retry_transport_and_firmware(&mut self) -> Result<WifiDebugSnapshot, HalError> {
        let state = self.hal_mut()?.pi4_wifi_state()?;
        cyw43::debug_retry_transport_and_firmware(state)?;
        state.debug_dump_state("console-retry")
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
        #[cfg(all(
            target_arch = "aarch64",
            target_os = "none",
            not(feature = "net-backend-virtio")
        ))]
        {
            Ok(WifiFirmwareBundle::new(
                pi4_wifi::PI4_WIFI_FIRMWARE,
                pi4_wifi::PI4_WIFI_NVRAM,
                Some(pi4_wifi::PI4_WIFI_CLM_BLOB),
                pi4_wifi::PI4_WIFI_BOARD_TYPE,
            ))
        }
        #[cfg(not(all(
            target_arch = "aarch64",
            target_os = "none",
            not(feature = "net-backend-virtio")
        )))]
        {
            Ok(self
                .pi4_wifi
                .as_ref()
                .map(pi4_wifi::Pi4WifiState::firmware_bundle)
                .unwrap_or_else(|| {
                    WifiFirmwareBundle::new(
                        pi4_wifi::PI4_WIFI_FIRMWARE,
                        pi4_wifi::PI4_WIFI_NVRAM,
                        Some(pi4_wifi::PI4_WIFI_CLM_BLOB),
                        pi4_wifi::PI4_WIFI_BOARD_TYPE,
                    )
                }))
        }
    }

    fn wifi_set_power(&mut self, state: WifiPowerState) -> Result<(), Self::Error> {
        self.pi4_wifi_state()?.set_power(state)
    }

    fn wifi_set_reset(&mut self, state: WifiResetState) -> Result<(), Self::Error> {
        self.pi4_wifi_state()?.set_reset(state)
    }

    fn sdio_reset_host(&mut self) -> Result<(), Self::Error> {
        self.pi4_wifi_state()?.reset_host()
    }

    fn sdio_set_bus_width(&mut self, width: SdioBusWidth) -> Result<(), Self::Error> {
        self.pi4_wifi_state()?.set_bus_width(width)
    }

    fn sdio_set_clock_hz(&mut self, target_hz: u32) -> Result<u32, Self::Error> {
        self.pi4_wifi_state()?.set_clock_hz(target_hz)
    }

    fn sdio_io_direct_read(
        &mut self,
        function: SdioFunction,
        addr: u32,
    ) -> Result<u8, Self::Error> {
        self.pi4_wifi_state()?.io_direct_read(function, addr)
    }

    fn sdio_io_direct_write(
        &mut self,
        function: SdioFunction,
        addr: u32,
        value: u8,
    ) -> Result<(), Self::Error> {
        self.pi4_wifi_state()?
            .io_direct_write(function, addr, value)
    }

    fn sdio_io_extended(
        &mut self,
        function: SdioFunction,
        addr: u32,
        increment_addr: bool,
        write: bool,
        buffer: &mut [u8],
    ) -> Result<(), Self::Error> {
        self.pi4_wifi_state()?
            .io_extended(function, addr, increment_addr, write, buffer)
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
        assert_eq!(report.runtime_image_acceptance_count, 0);
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
        assert_eq!(report.runtime_image_acceptance_count, 0);
        assert_eq!(
            report.runtime_image_transport_mapped_hot_path_mask,
            super::driver_task::REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK
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
                !handle.runtime_image_acceptance_eligible,
                "{}",
                contract.name
            );
            assert_eq!(
                handle.runtime_image_non_acceptance_reason, "root-context-required",
                "{}",
                contract.name
            );
        }
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
