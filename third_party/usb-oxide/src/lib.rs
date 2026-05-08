// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
//! Bare-metal lightweight xHCI/USB stack for OS development.
//!
//! This crate provides functionality for interacting with USB devices via xHCI
//! (eXtensible Dma Controller Interface) in environments without the standard
//! library, such as kernels, bootloaders, or embedded systems.
//!
//! # Features
//!
//! - xHCI controller initialization and management
//! - USB device enumeration and configuration
//! - HID (Human Interface Device) support for keyboards and mice
//! - Mass Storage Class (MSC) with SCSI commands
//! - Comprehensive USB descriptor and class definitions
//!
//! # Example
//!
//! ```ignore
//! // Initialize xHCI controller
//! let ctrl = XhciCtrl::new(mmio_phys, host)?;
//! let ctrl = Arc::new(ctrl);
//!
//! // Enumerate connected devices
//! for port in 0..ctrl.max_ports() {
//!     if ctrl.port_connected(port) {
//!         let device = UsbDevice::new(ctrl.clone(), port)?;
//!         // ... configure and use device
//!     }
//! }
//! ```
#![no_std]
#![deny(missing_docs)]

extern crate alloc;
#[cfg(test)]
extern crate std;

mod desc;
mod dev;
mod err;
mod hid;
mod msc;
mod ram;
mod reg;
mod ring;
mod xhci;

// Re-export main types
pub use crate::{
    dev::UsbDevice,
    err::{Result, UsbError},
    ram::{Dma, DmaShareError},
    ring::{PhysMem, Trb},
    xhci::{
        XhciControllerParams, XhciCtrl, XhciDiagHook, XhciFirmwareHandoff, XhciPortReadHook,
        XhciPortWriteHook, XhciPostedWriteFlushHook, XhciRuntimeSeedSnapshot, set_xhci_diag_hook,
        set_xhci_port_access_hooks, set_xhci_posted_write_flush_hook,
    },
};

// Re-export descriptor types and constants
pub use crate::desc::{
    // Descriptor structures
    BosDesc,
    ConfigDesc,
    DeviceDesc,
    DeviceQualifierDesc,
    EndpointDesc,
    HidDesc,
    HubDesc,
    InterfaceAssocDesc,
    InterfaceDesc,
    SetupPacket,
    SsDevCapDesc,
    SsEpCompDesc,
    SsHubDesc,
    Usb20ExtCapDesc,
    // Constant modules
    capability,
    cdc_subclass,
    class,
    desc_type,
    ep_sync,
    ep_type,
    ep_usage,
    feature,
    hid_protocol,
    hid_subclass,
    hub_feature,
    hub_protocol,
    hub_subclass,
    lang_id,
    msc_protocol,
    msc_subclass,
    req_dir,
    req_recipient,
    req_type,
    request,
};

// Re-export HID types and constants
pub use crate::hid::{
    // Structures
    HidDevice,
    HidType,
    KeyboardReport,
    MouseReport,
    // Functions
    find_hid_interfaces,
    // Constant modules
    led,
    modifier,
    report_type,
    scancode,
    scancode_to_ascii,
    usage_desktop,
    usage_page,
};

// Re-export MSC types and constants
pub use crate::msc::{
    // Structures
    Cbw,
    Csw,
    InquiryData,
    MscDevice,
    ReadCapacity10Data,
    RequestSenseData,
    // Functions
    find_msc_interfaces,
    // Constant modules
    scsi_op,
    sense_key,
};

// Re-export ring types and constants
pub use crate::ring::{completion, trb_flags, trb_type};

// Re-export device context types
pub use crate::dev::{DeviceContext, EndpointContext, InputContext, SlotContext, TtContext};

// Re-export register definitions (useful for advanced users)
/// xHCI register offsets and constants.
pub mod regs {
    pub use crate::reg::*;
}
