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

use core::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "kernel")]
use core::{fmt, ptr::NonNull};

#[cfg(any(feature = "kernel", feature = "cache-maintenance"))]
pub mod cache;

#[cfg(any(feature = "kernel", feature = "cache-maintenance"))]
pub mod dma;

#[cfg(feature = "kernel")]
pub mod pci;
#[cfg(feature = "kernel")]
pub mod pi4_wifi;

#[cfg(feature = "kernel")]
use crate::drivers::cyw43;
#[cfg(feature = "kernel")]
use crate::sel4::{DeviceCoverage, DeviceFrame, KernelEnv, KernelEnvSnapshot, RamFrame};
#[cfg(feature = "kernel")]
use pci::{PciAddress, PciTopology};
#[cfg(feature = "kernel")]
use sel4_sys::seL4_ARM_VMAttributes;
#[cfg(feature = "kernel")]
use sel4_sys::seL4_Error;

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

/// Root-console Wi-Fi debug hooks backed by the kernel HAL.
#[cfg(feature = "kernel")]
pub trait WifiDebugOps {
    fn dump_state(&mut self, stage: &'static str) -> Result<WifiDebugSnapshot, HalError>;
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

/// Abstraction over IRQ controller behaviour.
#[cfg(feature = "kernel")]
pub trait IrqCtl {
    /// Returns the next pending IRQ when available.
    fn poll(&self) -> Option<Irq>;

    /// Acknowledges a previously observed IRQ.
    fn ack(&self, irq: Irq);
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
    pi4_wifi: Option<pi4_wifi::Pi4WifiState>,
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
            pi4_wifi: None,
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

    fn pi4_wifi_state(&mut self) -> Result<&mut pi4_wifi::Pi4WifiState, HalError> {
        if self.pi4_wifi.is_none() {
            self.pi4_wifi = Some(pi4_wifi::Pi4WifiState::new(self)?);
        }
        self.pi4_wifi
            .as_mut()
            .ok_or(HalError::Unsupported("pi4-wifi-state"))
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
