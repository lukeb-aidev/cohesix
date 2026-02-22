// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
//! USB descriptor types and structures.
//!
//! This module provides all standard USB descriptor types, class codes,
//! and related constants as defined in the USB 2.0 and USB 3.x specifications.

/// USB descriptor type constants.
pub mod desc_type {
    /// Device descriptor (18 bytes)
    pub const DEVICE: u8 = 1;
    /// Configuration descriptor (9 bytes + additional descriptors)
    pub const CONFIGURATION: u8 = 2;
    /// String descriptor (variable length UTF-16LE)
    pub const STRING: u8 = 3;
    /// Interface descriptor (9 bytes)
    pub const INTERFACE: u8 = 4;
    /// Endpoint descriptor (7 bytes)
    pub const ENDPOINT: u8 = 5;
    /// Device Qualifier descriptor (USB 2.0, 10 bytes)
    pub const DEVICE_QUALIFIER: u8 = 6;
    /// Other Speed Configuration descriptor (USB 2.0)
    pub const OTHER_SPEED_CONFIG: u8 = 7;
    /// Interface Power descriptor
    pub const INTERFACE_POWER: u8 = 8;
    /// On-The-Go descriptor
    pub const OTG: u8 = 9;
    /// Debug descriptor
    pub const DEBUG: u8 = 10;
    /// Interface Association Descriptor (IAD)
    pub const INTERFACE_ASSOCIATION: u8 = 11;
    /// Binary Object Store descriptor (USB 3.0)
    pub const BOS: u8 = 15;
    /// Device Capability descriptor (USB 3.0)
    pub const DEVICE_CAPABILITY: u8 = 16;
    /// SuperSpeed USB Endpoint Companion descriptor
    pub const SS_EP_COMPANION: u8 = 48;
    /// SuperSpeedPlus Isochronous Endpoint Companion descriptor
    pub const SSP_ISO_EP_COMPANION: u8 = 49;

    // Class-specific descriptors
    /// HID descriptor
    pub const HID: u8 = 0x21;
    /// HID report descriptor
    pub const HID_REPORT: u8 = 0x22;
    /// HID physical descriptor
    pub const HID_PHYSICAL: u8 = 0x23;

    /// Hub descriptor (USB 2.0)
    pub const HUB: u8 = 0x29;
    /// SuperSpeed Hub descriptor (USB 3.0)
    pub const SS_HUB: u8 = 0x2A;
}

/// USB device class codes.
pub mod class {
    /// Class specified at interface level
    pub const INTERFACE_SPECIFIC: u8 = 0x00;
    /// Audio class
    pub const AUDIO: u8 = 0x01;
    /// Communications and CDC control
    pub const CDC: u8 = 0x02;
    /// Human Interface Device
    pub const HID: u8 = 0x03;
    /// Physical Interface Device
    pub const PHYSICAL: u8 = 0x05;
    /// Still Image class
    pub const IMAGE: u8 = 0x06;
    /// Printer class
    pub const PRINTER: u8 = 0x07;
    /// Mass storage class
    pub const MASS_STORAGE: u8 = 0x08;
    /// Hub class
    pub const HUB: u8 = 0x09;
    /// CDC-Data class
    pub const CDC_DATA: u8 = 0x0A;
    /// Smart Card class
    pub const SMART_CARD: u8 = 0x0B;
    /// Content Security class
    pub const CONTENT_SECURITY: u8 = 0x0D;
    /// Video class
    pub const VIDEO: u8 = 0x0E;
    /// Personal Healthcare class
    pub const PERSONAL_HEALTHCARE: u8 = 0x0F;
    /// Audio/Video Devices class
    pub const AUDIO_VIDEO: u8 = 0x10;
    /// Billboard Device class
    pub const BILLBOARD: u8 = 0x11;
    /// USB Type-C Bridge class
    pub const TYPE_C_BRIDGE: u8 = 0x12;
    /// Diagnostic Device class
    pub const DIAGNOSTIC: u8 = 0xDC;
    /// Wireless Controller class
    pub const WIRELESS: u8 = 0xE0;
    /// Miscellaneous class
    pub const MISC: u8 = 0xEF;
    /// Application Specific class
    pub const APPLICATION_SPECIFIC: u8 = 0xFE;
    /// Vendor specific class
    pub const VENDOR_SPECIFIC: u8 = 0xFF;
}

/// Mass Storage subclass codes.
pub mod msc_subclass {
    /// SCSI command set not reported
    pub const SCSI_NOT_REPORTED: u8 = 0x00;
    /// RBC (Reduced Block Commands)
    pub const RBC: u8 = 0x01;
    /// MMC-5 (ATAPI)
    pub const MMC5: u8 = 0x02;
    /// Obsolete (QIC-157)
    pub const QIC157: u8 = 0x03;
    /// UFI (USB Floppy Interface)
    pub const UFI: u8 = 0x04;
    /// Obsolete (SFF-8070i)
    pub const SFF8070I: u8 = 0x05;
    /// SCSI transparent command set
    pub const SCSI_TRANSPARENT: u8 = 0x06;
    /// LSD FS (Lockable Storage Devices Feature Spec)
    pub const LSD_FS: u8 = 0x07;
    /// IEEE 1667
    pub const IEEE1667: u8 = 0x08;
    /// Vendor specific
    pub const VENDOR_SPECIFIC: u8 = 0xFF;
}

/// Mass Storage protocol codes.
pub mod msc_protocol {
    /// Control/Bulk/Interrupt (CBI) with command completion interrupt
    pub const CBI_INTERRUPT: u8 = 0x00;
    /// Control/Bulk/Interrupt (CBI) without command completion interrupt
    pub const CBI_NO_INTERRUPT: u8 = 0x01;
    /// Bulk-Only Transport (BOT)
    pub const BBB: u8 = 0x50;
    /// USB Attached SCSI (UAS)
    pub const UAS: u8 = 0x62;
    /// Vendor specific
    pub const VENDOR_SPECIFIC: u8 = 0xFF;
}

/// HID subclass codes.
pub mod hid_subclass {
    /// No subclass
    pub const NONE: u8 = 0;
    /// Boot interface subclass
    pub const BOOT: u8 = 1;
}

/// HID protocol codes.
pub mod hid_protocol {
    /// No protocol
    pub const NONE: u8 = 0;
    /// Keyboard
    pub const KEYBOARD: u8 = 1;
    /// Mouse
    pub const MOUSE: u8 = 2;
}

/// Hub subclass codes.
pub mod hub_subclass {
    /// Full/Low speed hub
    pub const FULL_SPEED: u8 = 0;
    /// Hi-speed hub with single TT
    pub const HI_SPEED_SINGLE_TT: u8 = 1;
    /// Hi-speed hub with multiple TTs
    pub const HI_SPEED_MULTI_TT: u8 = 2;
}

/// Hub protocol codes.
pub mod hub_protocol {
    /// Full speed hub
    pub const FULL_SPEED: u8 = 0;
    /// Hi-speed hub with single TT
    pub const HI_SPEED_SINGLE_TT: u8 = 1;
    /// Hi-speed hub with multiple TTs
    pub const HI_SPEED_MULTI_TT: u8 = 2;
    /// SuperSpeed hub
    pub const SUPER_SPEED: u8 = 3;
}

/// CDC subclass codes.
pub mod cdc_subclass {
    /// Direct Line Control Model
    pub const DLCM: u8 = 0x01;
    /// Abstract Control Model (most common)
    pub const ACM: u8 = 0x02;
    /// Telephone Control Model
    pub const TCM: u8 = 0x03;
    /// Multi-Channel Control Model
    pub const MCCM: u8 = 0x04;
    /// CAPI Control Model
    pub const CAPI: u8 = 0x05;
    /// Ethernet Networking Control Model
    pub const ENCM: u8 = 0x06;
    /// ATM Networking Control Model
    pub const ANCM: u8 = 0x07;
    /// Wireless Handset Control Model
    pub const WHCM: u8 = 0x08;
    /// Device Management
    pub const DM: u8 = 0x09;
    /// Mobile Direct Line Model
    pub const MDLM: u8 = 0x0A;
    /// OBEX
    pub const OBEX: u8 = 0x0B;
    /// Ethernet Emulation Model
    pub const EEM: u8 = 0x0C;
    /// Network Control Model
    pub const NCM: u8 = 0x0D;
}

/// Endpoint transfer type codes.
pub mod ep_type {
    /// Control transfer
    pub const CONTROL: u8 = 0;
    /// Isochronous transfer
    pub const ISOCHRONOUS: u8 = 1;
    /// Bulk transfer
    pub const BULK: u8 = 2;
    /// Interrupt transfer
    pub const INTERRUPT: u8 = 3;
}

/// Endpoint synchronization types (for isochronous endpoints).
pub mod ep_sync {
    /// No synchronization
    pub const NONE: u8 = 0;
    /// Asynchronous
    pub const ASYNC: u8 = 1;
    /// Adaptive
    pub const ADAPTIVE: u8 = 2;
    /// Synchronous
    pub const SYNC: u8 = 3;
}

/// Endpoint usage types (for isochronous endpoints).
pub mod ep_usage {
    /// Data endpoint
    pub const DATA: u8 = 0;
    /// Feedback endpoint
    pub const FEEDBACK: u8 = 1;
    /// Implicit feedback data endpoint
    pub const IMPLICIT_FEEDBACK: u8 = 2;
}

/// Standard USB request codes.
pub mod request {
    /// Get device/interface/endpoint status
    pub const GET_STATUS: u8 = 0;
    /// Clear a feature
    pub const CLEAR_FEATURE: u8 = 1;
    /// Set a feature
    pub const SET_FEATURE: u8 = 3;
    /// Set device address
    pub const SET_ADDRESS: u8 = 5;
    /// Get descriptor
    pub const GET_DESCRIPTOR: u8 = 6;
    /// Set descriptor
    pub const SET_DESCRIPTOR: u8 = 7;
    /// Get configuration value
    pub const GET_CONFIGURATION: u8 = 8;
    /// Set configuration value
    pub const SET_CONFIGURATION: u8 = 9;
    /// Get interface alternate setting
    pub const GET_INTERFACE: u8 = 10;
    /// Set interface alternate setting
    pub const SET_INTERFACE: u8 = 11;
    /// Sync frame (isochronous)
    pub const SYNCH_FRAME: u8 = 12;
    /// Set System Exit Latency (USB 3.0)
    pub const SET_SEL: u8 = 48;
    /// Set isochronous delay (USB 3.0)
    pub const SET_ISOCH_DELAY: u8 = 49;
}

/// USB feature selectors.
pub mod feature {
    /// Endpoint halt (stall)
    pub const ENDPOINT_HALT: u16 = 0;
    /// Device remote wakeup
    pub const DEVICE_REMOTE_WAKEUP: u16 = 1;
    /// Test mode (USB 2.0)
    pub const TEST_MODE: u16 = 2;
    /// B HNP enable (OTG)
    pub const B_HNP_ENABLE: u16 = 3;
    /// A HNP support (OTG)
    pub const A_HNP_SUPPORT: u16 = 4;
    /// A alt HNP support (OTG)
    pub const A_ALT_HNP_SUPPORT: u16 = 5;
    /// USB 3.0 U1 enable
    pub const U1_ENABLE: u16 = 48;
    /// USB 3.0 U2 enable
    pub const U2_ENABLE: u16 = 49;
    /// USB 3.0 LTM enable
    pub const LTM_ENABLE: u16 = 50;
    /// USB 3.1 B3 NTF host release
    pub const B3_NTF_HOST_REL: u16 = 51;
    /// USB 3.1 B3 RSP enable
    pub const B3_RSP_ENABLE: u16 = 52;
    /// USB 3.2 LDM enable
    pub const LDM_ENABLE: u16 = 53;
}

/// Request type direction bit.
pub mod req_dir {
    /// Host to device
    pub const OUT: u8 = 0x00;
    /// Device to host
    pub const IN: u8 = 0x80;
}

/// Request type type bits.
pub mod req_type {
    /// Standard request
    pub const STANDARD: u8 = 0x00;
    /// Class-specific request
    pub const CLASS: u8 = 0x20;
    /// Vendor-specific request
    pub const VENDOR: u8 = 0x40;
}

/// Request type recipient bits.
pub mod req_recipient {
    /// Device recipient
    pub const DEVICE: u8 = 0x00;
    /// Interface recipient
    pub const INTERFACE: u8 = 0x01;
    /// Endpoint recipient
    pub const ENDPOINT: u8 = 0x02;
    /// Other recipient
    pub const OTHER: u8 = 0x03;
}

/// Device capability types (for BOS descriptor).
pub mod capability {
    /// Wireless USB
    pub const WIRELESS_USB: u8 = 0x01;
    /// USB 2.0 Extension
    pub const USB_2_0_EXTENSION: u8 = 0x02;
    /// SuperSpeed USB
    pub const SUPERSPEED_USB: u8 = 0x03;
    /// Container ID
    pub const CONTAINER_ID: u8 = 0x04;
    /// Platform
    pub const PLATFORM: u8 = 0x05;
    /// Power Delivery Capability
    pub const POWER_DELIVERY: u8 = 0x06;
    /// Battery Info Capability
    pub const BATTERY_INFO: u8 = 0x07;
    /// PD Consumer Port Capability
    pub const PD_CONSUMER_PORT: u8 = 0x08;
    /// PD Provider Port Capability
    pub const PD_PROVIDER_PORT: u8 = 0x09;
    /// SuperSpeed Plus
    pub const SUPERSPEED_PLUS: u8 = 0x0A;
    /// Precision Time Measurement
    pub const PRECISION_TIME_MEASUREMENT: u8 = 0x0B;
    /// Wireless USB Ext
    pub const WIRELESS_USB_EXT: u8 = 0x0C;
    /// Billboard
    pub const BILLBOARD: u8 = 0x0D;
    /// Authentication
    pub const AUTHENTICATION: u8 = 0x0E;
    /// Billboard Ex
    pub const BILLBOARD_EX: u8 = 0x0F;
    /// Configuration Summary
    pub const CONFIGURATION_SUMMARY: u8 = 0x10;
}

/// USB device descriptor (18 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DeviceDesc {
    /// Descriptor length (18)
    pub length: u8,
    /// Descriptor type (1 for device)
    pub desc_type: u8,
    /// USB specification version (BCD)
    pub bcd_usb: u16,
    /// Device class code
    pub device_class: u8,
    /// Device subclass code
    pub device_subclass: u8,
    /// Device protocol code
    pub device_protocol: u8,
    /// Maximum packet size for endpoint 0 (8, 16, 32, or 64)
    pub max_packet_size0: u8,
    /// Vendor ID
    pub vendor_id: u16,
    /// Product ID
    pub product_id: u16,
    /// Device release number (BCD)
    pub bcd_device: u16,
    /// Manufacturer string index
    pub manufacturer: u8,
    /// Product string index
    pub product: u8,
    /// Serial number string index
    pub serial_number: u8,
    /// Number of configurations
    pub num_configurations: u8,
}

impl DeviceDesc {
    /// Returns the USB version as a tuple (major, minor).
    pub fn usb_version(&self) -> (u8, u8) {
        ((self.bcd_usb >> 8) as u8, (self.bcd_usb & 0xFF) as u8)
    }

    /// Returns the device version as a tuple (major, minor).
    pub fn device_version(&self) -> (u8, u8) {
        ((self.bcd_device >> 8) as u8, (self.bcd_device & 0xFF) as u8)
    }
}

/// USB configuration descriptor (9 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ConfigDesc {
    /// Descriptor length (9)
    pub length: u8,
    /// Descriptor type (2 for configuration)
    pub desc_type: u8,
    /// Total length of configuration data (includes all descriptors)
    pub total_length: u16,
    /// Number of interfaces
    pub num_interfaces: u8,
    /// Configuration value for SetConfiguration
    pub config_value: u8,
    /// Configuration string index
    pub configuration: u8,
    /// Configuration attributes (D7: reserved, D6: self-powered, D5: remote wakeup)
    pub attributes: u8,
    /// Maximum power consumption (2mA units for USB 2.0, 8mA units for USB 3.0)
    pub max_power: u8,
}

impl ConfigDesc {
    /// Returns true if the device is self-powered in this configuration.
    pub fn self_powered(&self) -> bool {
        (self.attributes & 0x40) != 0
    }

    /// Returns true if remote wakeup is supported in this configuration.
    pub fn remote_wakeup(&self) -> bool {
        (self.attributes & 0x20) != 0
    }

    /// Returns the maximum power in milliamps (USB 2.0 calculation).
    pub fn max_power_ma(&self) -> u16 {
        self.max_power as u16 * 2
    }
}

/// USB interface descriptor (9 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct InterfaceDesc {
    /// Descriptor length (9)
    pub length: u8,
    /// Descriptor type (4 for interface)
    pub desc_type: u8,
    /// Interface number
    pub interface_number: u8,
    /// Alternate setting number
    pub alternate_setting: u8,
    /// Number of endpoints (excluding endpoint 0)
    pub num_endpoints: u8,
    /// Interface class code
    pub interface_class: u8,
    /// Interface subclass code
    pub interface_subclass: u8,
    /// Interface protocol code
    pub interface_protocol: u8,
    /// Interface string index
    pub interface: u8,
}

/// USB endpoint descriptor (7 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EndpointDesc {
    /// Descriptor length (7)
    pub length: u8,
    /// Descriptor type (5 for endpoint)
    pub desc_type: u8,
    /// Endpoint address (D7: direction, D3-D0: endpoint number)
    pub endpoint_address: u8,
    /// Endpoint attributes (transfer type, sync type, usage type)
    pub attributes: u8,
    /// Maximum packet size (D10-D0: size, D12-D11: additional transactions for HS)
    pub max_packet_size: u16,
    /// Polling interval (frame count for FS/LS, 2^(n-1) microframes for HS)
    pub interval: u8,
}

impl EndpointDesc {
    /// Returns the endpoint number (0-15).
    pub fn number(&self) -> u8 {
        self.endpoint_address & 0x0F
    }

    /// Returns true if this is an IN endpoint.
    pub fn is_in(&self) -> bool {
        (self.endpoint_address & 0x80) != 0
    }

    /// Returns true if this is an OUT endpoint.
    pub fn is_out(&self) -> bool {
        (self.endpoint_address & 0x80) == 0
    }

    /// Returns the transfer type.
    pub fn transfer_type(&self) -> u8 {
        self.attributes & 0x03
    }

    /// Returns the synchronization type (for isochronous endpoints).
    pub fn sync_type(&self) -> u8 {
        (self.attributes >> 2) & 0x03
    }

    /// Returns the usage type (for isochronous endpoints).
    pub fn usage_type(&self) -> u8 {
        (self.attributes >> 4) & 0x03
    }

    /// Returns the actual maximum packet size (without additional transaction bits).
    pub fn packet_size(&self) -> u16 {
        self.max_packet_size & 0x07FF
    }

    /// Returns the number of additional transactions per microframe (HS only, 0-2).
    pub fn additional_transactions(&self) -> u8 {
        ((self.max_packet_size >> 11) & 0x03) as u8
    }
}

/// USB Device Qualifier descriptor (10 bytes).
///
/// Reports device capabilities when operating at other speed (USB 2.0).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DeviceQualifierDesc {
    /// Descriptor length (10)
    pub length: u8,
    /// Descriptor type (6)
    pub desc_type: u8,
    /// USB specification version (BCD)
    pub bcd_usb: u16,
    /// Device class code
    pub device_class: u8,
    /// Device subclass code
    pub device_subclass: u8,
    /// Device protocol code
    pub device_protocol: u8,
    /// Maximum packet size for endpoint 0
    pub max_packet_size0: u8,
    /// Number of configurations
    pub num_configurations: u8,
    /// Reserved
    pub reserved: u8,
}

/// Interface Association Descriptor (8 bytes).
///
/// Groups multiple interfaces that belong to a single function.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct InterfaceAssocDesc {
    /// Descriptor length (8)
    pub length: u8,
    /// Descriptor type (11)
    pub desc_type: u8,
    /// First interface number
    pub first_interface: u8,
    /// Number of contiguous interfaces
    pub interface_count: u8,
    /// Function class code
    pub function_class: u8,
    /// Function subclass code
    pub function_subclass: u8,
    /// Function protocol code
    pub function_protocol: u8,
    /// Function string index
    pub function: u8,
}

/// Binary Object Store (BOS) descriptor header (5 bytes).
///
/// Container for device capability descriptors (USB 3.0+).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BosDesc {
    /// Descriptor length (5)
    pub length: u8,
    /// Descriptor type (15)
    pub desc_type: u8,
    /// Total length of BOS descriptor and all capability descriptors
    pub total_length: u16,
    /// Number of device capability descriptors
    pub num_device_caps: u8,
}

/// USB 2.0 Extension Capability descriptor (7 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Usb20ExtCapDesc {
    /// Descriptor length (7)
    pub length: u8,
    /// Descriptor type (16)
    pub desc_type: u8,
    /// Capability type (2)
    pub dev_capability_type: u8,
    /// Bitmap of supported features (D1: LPM supported)
    pub bm_attributes: u32,
}

impl Usb20ExtCapDesc {
    /// Returns true if Link Power Management (LPM) is supported.
    pub fn lpm_supported(&self) -> bool {
        (self.bm_attributes & 0x02) != 0
    }
}

/// SuperSpeed USB Device Capability descriptor (10 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SsDevCapDesc {
    /// Descriptor length (10)
    pub length: u8,
    /// Descriptor type (16)
    pub desc_type: u8,
    /// Capability type (3)
    pub dev_capability_type: u8,
    /// Bitmap of supported features
    pub bm_attributes: u8,
    /// Bitmap of supported speeds
    pub speeds_supported: u16,
    /// U1 device exit latency
    pub u1_dev_exit_lat: u8,
    /// U2 device exit latency
    pub u2_dev_exit_lat: u16,
}

impl SsDevCapDesc {
    /// Returns true if Low-power operation is supported.
    pub fn ltm_capable(&self) -> bool {
        (self.bm_attributes & 0x02) != 0
    }
}

/// SuperSpeed Endpoint Companion descriptor (6 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SsEpCompDesc {
    /// Descriptor length (6)
    pub length: u8,
    /// Descriptor type (48)
    pub desc_type: u8,
    /// Maximum number of packets within a service interval
    pub max_burst: u8,
    /// Attributes (for bulk: max streams, for isoch: mult)
    pub bm_attributes: u8,
    /// Total number of bytes per service interval
    pub bytes_per_interval: u16,
}

impl SsEpCompDesc {
    /// Returns the maximum number of streams for bulk endpoints.
    pub fn max_streams(&self) -> u8 {
        self.bm_attributes & 0x1F
    }

    /// Returns the Mult value for isochronous endpoints.
    pub fn mult(&self) -> u8 {
        self.bm_attributes & 0x03
    }
}

/// HID descriptor.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct HidDesc {
    /// Descriptor length
    pub length: u8,
    /// Descriptor type (0x21 for HID)
    pub desc_type: u8,
    /// HID specification version (BCD)
    pub bcd_hid: u16,
    /// Country code
    pub country_code: u8,
    /// Number of HID class descriptors
    pub num_descriptors: u8,
    /// Report descriptor type
    pub report_desc_type: u8,
    /// Report descriptor length
    pub report_desc_length: u16,
}

/// USB Hub descriptor (variable length, at least 7 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct HubDesc {
    /// Descriptor length
    pub length: u8,
    /// Descriptor type (0x29 for hub, 0x2A for SS hub)
    pub desc_type: u8,
    /// Number of downstream ports
    pub num_ports: u8,
    /// Hub characteristics
    pub hub_characteristics: u16,
    /// Power on to power good time (2ms units)
    pub pwr_on_2_pwr_good: u8,
    /// Hub controller current (mA)
    pub hub_contr_current: u8,
    // Variable length fields follow: DeviceRemovable, PortPwrCtrlMask
}

impl HubDesc {
    /// Returns true if this is a compound device.
    pub fn is_compound(&self) -> bool {
        (self.hub_characteristics & 0x04) != 0
    }

    /// Returns the power switching mode (0=ganged, 1=individual, 2-3=reserved).
    pub fn power_switching_mode(&self) -> u8 {
        (self.hub_characteristics & 0x03) as u8
    }

    /// Returns the overcurrent protection mode.
    pub fn overcurrent_protection_mode(&self) -> u8 {
        ((self.hub_characteristics >> 3) & 0x03) as u8
    }

    /// Returns the TT think time (0=8, 1=16, 2=24, 3=32 FS bit times).
    pub fn tt_think_time(&self) -> u8 {
        ((self.hub_characteristics >> 5) & 0x03) as u8
    }
}

/// SuperSpeed Hub descriptor (12 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SsHubDesc {
    /// Descriptor length (12)
    pub length: u8,
    /// Descriptor type (0x2A)
    pub desc_type: u8,
    /// Number of downstream ports
    pub num_ports: u8,
    /// Hub characteristics
    pub hub_characteristics: u16,
    /// Power on to power good time (2ms units)
    pub pwr_on_2_pwr_good: u8,
    /// Hub controller current (mA)
    pub hub_contr_current: u8,
    /// Hub header decode latency
    pub hub_hdr_dec_lat: u8,
    /// Hub delay
    pub hub_delay: u16,
    /// Device removable bitmap
    pub device_removable: u16,
}

/// USB setup packet for control transfers (8 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SetupPacket {
    /// Request type (D7: direction, D6-5: type, D4-0: recipient)
    pub request_type: u8,
    /// Request code
    pub request: u8,
    /// Value parameter
    pub value: u16,
    /// Index parameter
    pub index: u16,
    /// Data length
    pub length: u16,
}

impl SetupPacket {
    /// Creates a new setup packet.
    pub const fn new(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> Self {
        Self {
            request_type,
            request,
            value,
            index,
            length,
        }
    }

    /// Creates a GET_STATUS request for device.
    pub fn get_device_status() -> Self {
        Self::new(0x80, request::GET_STATUS, 0, 0, 2)
    }

    /// Creates a GET_STATUS request for interface.
    pub fn get_interface_status(interface: u8) -> Self {
        Self::new(0x81, request::GET_STATUS, 0, interface as u16, 2)
    }

    /// Creates a GET_STATUS request for endpoint.
    pub fn get_endpoint_status(endpoint: u8) -> Self {
        Self::new(0x82, request::GET_STATUS, 0, endpoint as u16, 2)
    }

    /// Creates a CLEAR_FEATURE request for device.
    pub fn clear_device_feature(feature: u16) -> Self {
        Self::new(0x00, request::CLEAR_FEATURE, feature, 0, 0)
    }

    /// Creates a CLEAR_FEATURE request for interface.
    pub fn clear_interface_feature(feature: u16, interface: u8) -> Self {
        Self::new(0x01, request::CLEAR_FEATURE, feature, interface as u16, 0)
    }

    /// Creates a CLEAR_FEATURE request for endpoint (typically to clear HALT).
    pub fn clear_endpoint_feature(feature: u16, endpoint: u8) -> Self {
        Self::new(0x02, request::CLEAR_FEATURE, feature, endpoint as u16, 0)
    }

    /// Creates a SET_FEATURE request for device.
    pub fn set_device_feature(feature: u16) -> Self {
        Self::new(0x00, request::SET_FEATURE, feature, 0, 0)
    }

    /// Creates a SET_FEATURE request for endpoint.
    pub fn set_endpoint_feature(feature: u16, endpoint: u8) -> Self {
        Self::new(0x02, request::SET_FEATURE, feature, endpoint as u16, 0)
    }

    /// Creates a GET_DESCRIPTOR request.
    pub fn get_descriptor(desc_type: u8, index: u8, length: u16) -> Self {
        Self::new(
            0x80,
            request::GET_DESCRIPTOR,
            ((desc_type as u16) << 8) | (index as u16),
            0,
            length,
        )
    }

    /// Creates a GET_DESCRIPTOR request for a specific language (strings).
    pub fn get_string_descriptor(index: u8, lang_id: u16, length: u16) -> Self {
        Self::new(
            0x80,
            request::GET_DESCRIPTOR,
            ((desc_type::STRING as u16) << 8) | (index as u16),
            lang_id,
            length,
        )
    }

    /// Creates a GET_CONFIGURATION request.
    pub fn get_configuration() -> Self {
        Self::new(0x80, request::GET_CONFIGURATION, 0, 0, 1)
    }

    /// Creates a SET_CONFIGURATION request.
    pub fn set_configuration(config: u8) -> Self {
        Self::new(0x00, request::SET_CONFIGURATION, config as u16, 0, 0)
    }

    /// Creates a GET_INTERFACE request.
    pub fn get_interface(interface: u8) -> Self {
        Self::new(0x81, request::GET_INTERFACE, 0, interface as u16, 1)
    }

    /// Creates a SET_INTERFACE request.
    pub fn set_interface(interface: u8, alt_setting: u8) -> Self {
        Self::new(
            0x01,
            request::SET_INTERFACE,
            alt_setting as u16,
            interface as u16,
            0,
        )
    }

    /// Creates a SYNCH_FRAME request.
    pub fn synch_frame(endpoint: u8) -> Self {
        Self::new(0x82, request::SYNCH_FRAME, 0, endpoint as u16, 2)
    }

    // HID class requests

    /// Creates a GET_REPORT request (HID class).
    pub fn hid_get_report(interface: u8, report_type: u8, report_id: u8, length: u16) -> Self {
        Self::new(
            0xA1,
            0x01,
            ((report_type as u16) << 8) | (report_id as u16),
            interface as u16,
            length,
        )
    }

    /// Creates a GET_IDLE request (HID class).
    pub fn hid_get_idle(interface: u8, report_id: u8) -> Self {
        Self::new(0xA1, 0x02, report_id as u16, interface as u16, 1)
    }

    /// Creates a GET_PROTOCOL request (HID class).
    pub fn hid_get_protocol(interface: u8) -> Self {
        Self::new(0xA1, 0x03, 0, interface as u16, 1)
    }

    /// Creates a SET_REPORT request (HID class).
    pub fn hid_set_report(interface: u8, report_type: u8, report_id: u8, length: u16) -> Self {
        Self::new(
            0x21,
            0x09,
            ((report_type as u16) << 8) | (report_id as u16),
            interface as u16,
            length,
        )
    }

    /// Creates a SET_IDLE request (HID class).
    pub fn set_idle(interface: u8, duration: u8, report_id: u8) -> Self {
        Self::new(
            0x21,
            0x0A,
            ((duration as u16) << 8) | (report_id as u16),
            interface as u16,
            0,
        )
    }

    /// Creates a SET_PROTOCOL request (HID class).
    pub fn set_protocol(interface: u8, protocol: u8) -> Self {
        Self::new(0x21, 0x0B, protocol as u16, interface as u16, 0)
    }

    // Hub class requests

    /// Creates a GET_HUB_STATUS request.
    pub fn hub_get_status() -> Self {
        Self::new(0xA0, request::GET_STATUS, 0, 0, 4)
    }

    /// Creates a GET_PORT_STATUS request.
    pub fn hub_get_port_status(port: u8) -> Self {
        Self::new(0xA3, request::GET_STATUS, 0, port as u16, 4)
    }

    /// Creates a SET_PORT_FEATURE request.
    pub fn hub_set_port_feature(feature: u16, port: u8) -> Self {
        Self::new(0x23, request::SET_FEATURE, feature, port as u16, 0)
    }

    /// Creates a CLEAR_PORT_FEATURE request.
    pub fn hub_clear_port_feature(feature: u16, port: u8) -> Self {
        Self::new(0x23, request::CLEAR_FEATURE, feature, port as u16, 0)
    }

    /// Creates a GET_HUB_DESCRIPTOR request.
    pub fn hub_get_descriptor(length: u16) -> Self {
        Self::new(
            0xA0,
            request::GET_DESCRIPTOR,
            (desc_type::HUB as u16) << 8,
            0,
            length,
        )
    }

    // Mass Storage class requests

    /// Creates a GET_MAX_LUN request (Mass Storage class).
    pub fn msc_get_max_lun(interface: u8) -> Self {
        Self::new(0xA1, 0xFE, 0, interface as u16, 1)
    }

    /// Creates a BULK_ONLY_RESET request (Mass Storage class).
    pub fn msc_reset(interface: u8) -> Self {
        Self::new(0x21, 0xFF, 0, interface as u16, 0)
    }

    // Deprecated aliases for backward compatibility

    /// Creates a GET_REPORT request (HID class).
    #[deprecated(note = "Use hid_get_report instead")]
    pub fn get_report(interface: u8, report_type: u8, report_id: u8, length: u16) -> Self {
        Self::hid_get_report(interface, report_type, report_id, length)
    }
}

/// Hub port feature selectors.
pub mod hub_feature {
    /// Port connection
    pub const PORT_CONNECTION: u16 = 0;
    /// Port enable
    pub const PORT_ENABLE: u16 = 1;
    /// Port suspend
    pub const PORT_SUSPEND: u16 = 2;
    /// Port over-current
    pub const PORT_OVER_CURRENT: u16 = 3;
    /// Port reset
    pub const PORT_RESET: u16 = 4;
    /// Port power
    pub const PORT_POWER: u16 = 8;
    /// Port low speed
    pub const PORT_LOW_SPEED: u16 = 9;
    /// C_PORT_CONNECTION (connect change)
    pub const C_PORT_CONNECTION: u16 = 16;
    /// C_PORT_ENABLE (enable change)
    pub const C_PORT_ENABLE: u16 = 17;
    /// C_PORT_SUSPEND (suspend change)
    pub const C_PORT_SUSPEND: u16 = 18;
    /// C_PORT_OVER_CURRENT (over-current change)
    pub const C_PORT_OVER_CURRENT: u16 = 19;
    /// C_PORT_RESET (reset change)
    pub const C_PORT_RESET: u16 = 20;
    /// Port test
    pub const PORT_TEST: u16 = 21;
    /// Port indicator
    pub const PORT_INDICATOR: u16 = 22;

    // USB 3.0 additions
    /// Port link state
    pub const PORT_LINK_STATE: u16 = 5;
    /// Port U1 timeout
    pub const PORT_U1_TIMEOUT: u16 = 23;
    /// Port U2 timeout
    pub const PORT_U2_TIMEOUT: u16 = 24;
    /// C_PORT_LINK_STATE (link state change)
    pub const C_PORT_LINK_STATE: u16 = 25;
    /// C_PORT_CONFIG_ERROR (config error change)
    pub const C_PORT_CONFIG_ERROR: u16 = 26;
    /// Port remote wake mask
    pub const PORT_REMOTE_WAKE_MASK: u16 = 27;
    /// BH_PORT_RESET
    pub const BH_PORT_RESET: u16 = 28;
    /// C_BH_PORT_RESET (BH reset change)
    pub const C_BH_PORT_RESET: u16 = 29;
    /// Force link PM accept
    pub const FORCE_LINKPM_ACCEPT: u16 = 30;
}

/// Language IDs for string descriptors.
pub mod lang_id {
    /// English (United States)
    pub const EN_US: u16 = 0x0409;
    /// English (United Kingdom)
    pub const EN_GB: u16 = 0x0809;
    /// German
    pub const DE: u16 = 0x0407;
    /// French
    pub const FR: u16 = 0x040C;
    /// Spanish
    pub const ES: u16 = 0x0C0A;
    /// Italian
    pub const IT: u16 = 0x0410;
    /// Japanese
    pub const JA: u16 = 0x0411;
    /// Korean
    pub const KO: u16 = 0x0412;
    /// Chinese (Simplified)
    pub const ZH_CN: u16 = 0x0804;
    /// Chinese (Traditional)
    pub const ZH_TW: u16 = 0x0404;
}
