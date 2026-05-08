// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
//! Dma trait for DMA and MMIO operations.

/// Error returned when a DMA range could not be prepared for device access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmaShareError;

/// Allocates physically contiguous memory and manages MMIO mappings.
///
/// Used for DMA operations requiring contiguous physical memory
/// and for mapping xHCI controller registers.
pub trait Dma: Send + Sync {
    /// Allocates a `size` byte region of physically contiguous memory
    /// with the specified alignment.
    ///
    /// Returns the virtual address of the allocated region, or `None` on failure.
    ///
    /// # Arguments
    ///
    /// * `size` - Size in bytes to allocate
    /// * `align` - Required alignment (must be a power of 2)
    ///
    /// # Safety
    ///
    /// - Returns uninitialized memory
    /// - Memory must be physically contiguous
    /// - Memory must be correctly mapped to virtual address space
    /// - Returned address must be aligned to `align` bytes
    unsafe fn alloc(&self, size: usize, align: usize) -> Option<usize>;

    /// Deallocates a previously allocated region of memory.
    ///
    /// # Safety
    ///
    /// - The address must have been returned by `alloc`
    /// - The memory must not have been freed already
    /// - `size` and `align` must match the original allocation
    unsafe fn free(&self, addr: usize, size: usize, align: usize);

    /// Maps an MMIO region into virtual address space.
    ///
    /// Returns the virtual address, or `None` on failure.
    ///
    /// # Safety
    ///
    /// - The physical address must be a valid MMIO region
    /// - The mapping must have appropriate memory attributes (uncached, device memory)
    unsafe fn map_mmio(&self, phys: usize, size: usize) -> Option<usize>;

    /// Unmaps a previously mapped MMIO region.
    ///
    /// # Safety
    ///
    /// - The address must have been returned by `map_mmio`
    unsafe fn unmap_mmio(&self, virt: usize, size: usize);

    /// Translates a virtual address to a physical address.
    fn virt_to_phys(&self, va: usize) -> usize;

    /// Translates a virtual address to a device-visible physical or bus address.
    ///
    /// Implementations should return `None` when the range cannot be represented
    /// on the device bus. The legacy infallible [`Dma::virt_to_phys`] method is
    /// retained for callers that cannot yet surface a typed error.
    fn try_virt_to_phys(&self, va: usize) -> Option<usize> {
        Some(self.virt_to_phys(va))
    }

    /// Prepares a DMA-backed memory range for device access after CPU writes.
    ///
    /// Implementations may clean caches, emit share diagnostics, or perform
    /// other platform-specific DMA visibility transitions before the bus
    /// address is handed to the controller.
    fn share_for_device(
        &self,
        _vaddr: usize,
        _len: usize,
        _label: &'static str,
    ) -> core::result::Result<(), DmaShareError> {
        Ok(())
    }

    /// Prepares a DMA-backed memory range for CPU reads after device writes.
    ///
    /// Implementations may invalidate caches or perform other platform-specific
    /// DMA visibility transitions before software inspects device-written
    /// memory.
    fn sync_for_cpu(
        &self,
        _vaddr: usize,
        _len: usize,
        _label: &'static str,
    ) -> core::result::Result<(), DmaShareError> {
        Ok(())
    }

    /// Returns the system page size in bytes.
    fn page_size(&self) -> usize {
        4096
    }
}
