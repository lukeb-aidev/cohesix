// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
//! Dma trait for DMA and MMIO operations.

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

    /// Returns the system page size in bytes.
    fn page_size(&self) -> usize {
        4096
    }
}
