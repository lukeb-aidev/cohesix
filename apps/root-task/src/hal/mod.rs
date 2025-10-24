// Author: Lukas Bower

//! Lightweight hardware abstraction used by the root task to decouple
//! low-level seL4 primitives from driver code.
//!
//! The abstraction intentionally exposes only the operations that the
//! current driver set depends on. This keeps the surface area small while
//! providing a structured location for future peripherals.

use core::fmt;

use crate::sel4::{DeviceCoverage, DeviceFrame, KernelEnv, KernelEnvSnapshot, RamFrame};
#[cfg(feature = "kernel")]
use sel4_sys::seL4_Error;

/// Errors surfaced by hardware accessors.
#[cfg(feature = "kernel")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HalError {
    /// seL4 system call failure while manipulating capabilities or mappings.
    Sel4(seL4_Error),
}

#[cfg(feature = "kernel")]
impl fmt::Display for HalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sel4(err) => write!(f, "seL4 error {err:?}"),
        }
    }
}

#[cfg(feature = "kernel")]
impl From<seL4_Error> for HalError {
    fn from(value: seL4_Error) -> Self {
        Self::Sel4(value)
    }
}

/// Trait implemented by hardware providers used inside the VM.
#[cfg(feature = "kernel")]
pub trait Hardware {
    /// Error type emitted by the hardware provider.
    type Error;

    /// Maps the physical device page at `paddr` into the device window.
    fn map_device(&mut self, paddr: usize) -> Result<DeviceFrame, Self::Error>;

    /// Allocates a DMA-capable frame and maps it into the DMA window.
    fn alloc_dma_frame(&mut self) -> Result<RamFrame, Self::Error>;

    /// Returns device coverage information for diagnostics.
    fn device_coverage(&self, paddr: usize, size_bits: usize) -> Option<DeviceCoverage>;

    /// Snapshot of allocator usage for debugging.
    fn snapshot(&self) -> KernelEnvSnapshot;
}

/// seL4-backed hardware provider that owns the [`KernelEnv`].
#[cfg(feature = "kernel")]
pub struct KernelHal<'a> {
    env: KernelEnv<'a>,
}

#[cfg(feature = "kernel")]
impl<'a> KernelHal<'a> {
    /// Construct a new HAL instance wrapping the supplied [`KernelEnv`].
    #[must_use]
    pub fn new(env: KernelEnv<'a>) -> Self {
        Self { env }
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
}

#[cfg(feature = "kernel")]
impl<'a> Hardware for KernelHal<'a> {
    type Error = HalError;

    fn map_device(&mut self, paddr: usize) -> Result<DeviceFrame, Self::Error> {
        self.env.map_device(paddr).map_err(HalError::from)
    }

    fn alloc_dma_frame(&mut self) -> Result<RamFrame, Self::Error> {
        self.env.alloc_dma_frame().map_err(HalError::from)
    }

    fn device_coverage(&self, paddr: usize, size_bits: usize) -> Option<DeviceCoverage> {
        self.env.device_coverage(paddr, size_bits)
    }

    fn snapshot(&self) -> KernelEnvSnapshot {
        self.env.snapshot()
    }
}
