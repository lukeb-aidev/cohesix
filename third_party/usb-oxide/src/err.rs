// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
//! USB error types.

use core::result::Result as CoreResult;

/// USB driver error types.
#[derive(Debug, Clone, Copy)]
pub enum UsbError {
    /// Operation timed out
    Timeout,
    /// Port reset timed out
    PortResetTimeout,
    /// Port did not reach enabled/ready state after reset
    PortEnableTimeout,
    /// Enable Slot command timed out
    EnableSlotTimeout,
    /// Address Device command timed out
    AddressDeviceTimeout,
    /// Out of memory
    OoRam,
    /// Failed to map MMIO region
    MapFail,
    /// Invalid slot ID
    InvSlot,
    /// Invalid port number
    InvPort,
    /// Invalid endpoint
    InvEndpoint,
    /// Command failed with completion code
    CmdFail(u8),
    /// Transfer failed with completion code
    XferFail(u8),
    /// Device not found
    DeviceNotFound,
    /// Operation not supported
    NotSupported,
    /// Invalid descriptor
    InvalidDescriptor,
    /// Endpoint stalled
    Stall,
}

/// Result type for USB operations.
pub type Result<T> = CoreResult<T, UsbError>;
