// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
//! xHCI register offsets and bit definitions.
//!
//! This module contains constants for interacting with xHCI controller
//! registers as defined in the xHCI specification.

// ============================================================================
// Capability Registers (offset from MMIO base)
// ============================================================================

/// Capability Registers Length and HC Interface Version Number
pub const CAPLENGTH: usize = 0x00;
/// Structural Parameters 1
pub const HCSPARAMS1: usize = 0x04;
/// Structural Parameters 2
pub const HCSPARAMS2: usize = 0x08;
/// Structural Parameters 3
pub const HCSPARAMS3: usize = 0x0C;
/// Capability Parameters 1
pub const HCCPARAMS1: usize = 0x10;
/// Doorbell Offset
pub const DBOFF: usize = 0x14;
/// Runtime Register Space Offset
pub const RTSOFF: usize = 0x18;
/// Capability Parameters 2
pub const HCCPARAMS2: usize = 0x1C;

// ============================================================================
// Operational Registers (offset from operational base)
// ============================================================================

/// USB Command Register
pub const USBCMD: usize = 0x00;
/// USB Status Register
pub const USBSTS: usize = 0x04;
/// Page Size Register
pub const PAGESIZE: usize = 0x08;
/// Device Notification Control Register
pub const DNCTRL: usize = 0x14;
/// Command Ring Control Register
pub const CRCR: usize = 0x18;
/// Device Context Base Address Array Pointer
pub const DCBAAP: usize = 0x30;
/// Configure Register
pub const CONFIG: usize = 0x38;

/// Reserved CRCR bits that must be preserved across updates.
pub const CMD_RING_RSVD_BITS: u64 = 0x3f;

// ============================================================================
// USBCMD Register Bits
// ============================================================================

/// Run/Stop - 1 = Run, 0 = Stop
pub const USBCMD_RUN: u32 = 1 << 0;
/// Host Controller Reset
pub const USBCMD_HCRST: u32 = 1 << 1;
/// Interrupter Enable
pub const USBCMD_INTE: u32 = 1 << 2;
/// Host System Error Enable
pub const USBCMD_HSEE: u32 = 1 << 3;
/// Light Host Controller Reset
pub const USBCMD_LHCRST: u32 = 1 << 7;
/// Controller Save State
pub const USBCMD_CSS: u32 = 1 << 8;
/// Controller Restore State
pub const USBCMD_CRS: u32 = 1 << 9;
/// Enable Wrap Event
pub const USBCMD_EWE: u32 = 1 << 10;
/// Enable U3 MFINDEX Stop
pub const USBCMD_EU3S: u32 = 1 << 11;
/// CEM Enable
pub const USBCMD_CME: u32 = 1 << 13;
/// Extended TBC Enable
pub const USBCMD_ETE: u32 = 1 << 14;
/// Extended TBC TRB Status Enable
pub const USBCMD_TSC_EN: u32 = 1 << 15;
/// VTIO Enable
pub const USBCMD_VTIOE: u32 = 1 << 16;

// ============================================================================
// USBSTS Register Bits
// ============================================================================

/// Host Controller Halted
pub const USBSTS_HCH: u32 = 1 << 0;
/// Host System Error
pub const USBSTS_HSE: u32 = 1 << 2;
/// Event Interrupt
pub const USBSTS_EINT: u32 = 1 << 3;
/// Port Change Detect
pub const USBSTS_PCD: u32 = 1 << 4;
/// Save State Status
pub const USBSTS_SSS: u32 = 1 << 8;
/// Restore State Status
pub const USBSTS_RSS: u32 = 1 << 9;
/// Save/Restore Error
pub const USBSTS_SRE: u32 = 1 << 10;
/// Controller Not Ready
pub const USBSTS_CNR: u32 = 1 << 11;
/// Host Controller Error
pub const USBSTS_HCE: u32 = 1 << 12;

// ============================================================================
// Port Register Set (offset from port register set base)
// ============================================================================

/// Port Status and Control
pub const PORTSC: usize = 0x00;
/// Port Power Management Status and Control
pub const PORTPMSC: usize = 0x04;
/// Port Link Info
pub const PORTLI: usize = 0x08;
/// Port Hardware LPM Control (USB 3.0)
pub const PORTHLPMC: usize = 0x0C;

// ============================================================================
// PORTSC Register Bits
// ============================================================================

/// Current Connect Status
pub const PORTSC_CCS: u32 = 1 << 0;
/// Port Enabled/Disabled
pub const PORTSC_PED: u32 = 1 << 1;
/// Overcurrent Active
pub const PORTSC_OCA: u32 = 1 << 3;
/// Port Reset
pub const PORTSC_PR: u32 = 1 << 4;
/// Port Link State (bits 8:5)
pub const PORTSC_PLS_MASK: u32 = 0xF << 5;
/// Port Power
pub const PORTSC_PP: u32 = 1 << 9;
/// Port Speed (bits 13:10)
pub const PORTSC_SPEED_MASK: u32 = 0xF << 10;
/// Port Indicator Control (bits 15:14)
pub const PORTSC_PIC_MASK: u32 = 0x3 << 14;
/// Port Link State Write Strobe
pub const PORTSC_LWS: u32 = 1 << 16;
/// Connect Status Change
pub const PORTSC_CSC: u32 = 1 << 17;
/// Port Enabled/Disabled Change
pub const PORTSC_PEC: u32 = 1 << 18;
/// Warm Port Reset Change
pub const PORTSC_WRC: u32 = 1 << 19;
/// Over-current Change
pub const PORTSC_OCC: u32 = 1 << 20;
/// Port Reset Change
pub const PORTSC_PRC: u32 = 1 << 21;
/// Port Link State Change
pub const PORTSC_PLC: u32 = 1 << 22;
/// Port Config Error Change
pub const PORTSC_CEC: u32 = 1 << 23;
/// Cold Attach Status
pub const PORTSC_CAS: u32 = 1 << 24;
/// Wake on Connect Enable
pub const PORTSC_WCE: u32 = 1 << 25;
/// Wake on Disconnect Enable
pub const PORTSC_WDE: u32 = 1 << 26;
/// Wake on Over-current Enable
pub const PORTSC_WOE: u32 = 1 << 27;
/// Device Removable
pub const PORTSC_DR: u32 = 1 << 30;
/// Warm Port Reset
pub const PORTSC_WPR: u32 = 1 << 31;

// ============================================================================
// Port Link States
// ============================================================================

/// U0 State (USB 3.0)
pub const PLS_U0: u32 = 0;
/// U1 State (USB 3.0)
pub const PLS_U1: u32 = 1;
/// U2 State (USB 3.0)
pub const PLS_U2: u32 = 2;
/// U3 State (Suspended)
pub const PLS_U3: u32 = 3;
/// Disabled
pub const PLS_DISABLED: u32 = 4;
/// RxDetect
pub const PLS_RXDETECT: u32 = 5;
/// Inactive
pub const PLS_INACTIVE: u32 = 6;
/// Polling
pub const PLS_POLLING: u32 = 7;
/// Recovery
pub const PLS_RECOVERY: u32 = 8;
/// Hot Reset
pub const PLS_HOT_RESET: u32 = 9;
/// Compliance Mode
pub const PLS_COMPLIANCE: u32 = 10;
/// Test Mode
pub const PLS_TEST: u32 = 11;
/// Resume
pub const PLS_RESUME: u32 = 15;

// ============================================================================
// Port Speed Values (from PORTSC bits 13:10)
// ============================================================================

/// Full Speed (12 Mbps)
pub const SPEED_FULL: u8 = 1;
/// Low Speed (1.5 Mbps)
pub const SPEED_LOW: u8 = 2;
/// High Speed (480 Mbps)
pub const SPEED_HIGH: u8 = 3;
/// SuperSpeed (5 Gbps)
pub const SPEED_SUPER: u8 = 4;
/// SuperSpeed Plus (10 Gbps)
pub const SPEED_SUPER_PLUS: u8 = 5;

// ============================================================================
// Interrupter Registers (offset from runtime register base + 0x20 * n)
// ============================================================================

/// Interrupter Management Register
pub const IMAN: usize = 0x00;
/// Interrupter Moderation Register
pub const IMOD: usize = 0x04;
/// Event Ring Segment Table Size Register
pub const ERSTSZ: usize = 0x08;
/// Event Ring Segment Table Base Address Register
pub const ERSTBA: usize = 0x10;
/// Event Ring Dequeue Pointer Register
pub const ERDP: usize = 0x18;

/// Preserve bits 16:31 when updating ERSTSZ.
pub const ERST_SIZE_MASK: u32 = 0xffff_0000;
/// Event Handler Busy (write 1 to clear when advancing ERDP).
pub const ERST_EHB: u64 = 1 << 3;
/// Low ERST pointer bits reserved by the controller.
pub const ERST_PTR_MASK: u64 = 0xf;

// ============================================================================
// IMAN Register Bits
// ============================================================================

/// Interrupt Pending
pub const IMAN_IP: u32 = 1 << 0;
/// Interrupt Enable
pub const IMAN_IE: u32 = 1 << 1;

// ============================================================================
// Extended Capability IDs
// ============================================================================

/// USB Legacy Support
pub const ECAP_USB_LEGACY: u8 = 1;
/// Supported Protocol
pub const ECAP_SUPPORTED_PROTOCOL: u8 = 2;
/// Extended Power Management
pub const ECAP_EXT_POWER_MGMT: u8 = 3;
/// I/O Virtualization
pub const ECAP_IO_VIRT: u8 = 4;
/// Message Interrupt
pub const ECAP_MSG_INTERRUPT: u8 = 5;
/// Local Memory
pub const ECAP_LOCAL_MEM: u8 = 6;
/// USB Debug Capability
pub const ECAP_USB_DEBUG: u8 = 10;
/// Extended Message Interrupt
pub const ECAP_EXT_MSG_INT: u8 = 17;

// ============================================================================
// Helper Functions
// ============================================================================

/// Returns the base offset for a port's register set.
pub const fn port_reg_base(cap_length: u8, port: u8) -> usize {
    cap_length as usize + 0x400 + (port as usize * 0x10)
}

/// Returns the offset for a doorbell register.
pub const fn doorbell(db_offset: u32, slot: u8) -> usize {
    db_offset as usize + (slot as usize * 4)
}

/// Returns the base offset for an interrupter's register set.
pub const fn interrupter_base(rts_offset: u32, interrupter: u8) -> usize {
    rts_offset as usize + 0x20 + (interrupter as usize * 0x20)
}

/// Extracts the port speed from a PORTSC register value.
pub const fn portsc_speed(portsc: u32) -> u8 {
    ((portsc >> 10) & 0xF) as u8
}

/// Extracts the port link state from a PORTSC register value.
pub const fn portsc_pls(portsc: u32) -> u8 {
    ((portsc >> 5) & 0xF) as u8
}

/// Creates a PORTSC value with the specified Port Link State.
pub const fn portsc_set_pls(pls: u32) -> u32 {
    (pls & 0xF) << 5
}
