// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
//! USB Mass Storage Class (MSC) support.
//!
//! Provides structures and functions for interacting with USB mass storage
//! devices using the Bulk-Only Transport (BOT) protocol.

use crate::{
    Dma, Result, UsbError,
    desc::{EndpointDesc, InterfaceDesc, SetupPacket, class, ep_type, msc_protocol},
    dev::UsbDevice,
    ring::PhysMem,
};

use alloc::sync::Arc;
use core::hint::spin_loop;

/// Command Block Wrapper (CBW) - 31 bytes.
///
/// Used to send SCSI commands over USB Bulk-Only Transport.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct Cbw {
    /// Signature (must be 0x43425355 "USBC")
    pub signature: u32,
    /// Tag to associate CBW with CSW
    pub tag: u32,
    /// Number of bytes to transfer
    pub data_transfer_length: u32,
    /// Flags (bit 7: direction, 0=OUT, 1=IN)
    pub flags: u8,
    /// LUN (bits 3:0)
    pub lun: u8,
    /// Command block length (1-16)
    pub cb_length: u8,
    /// Command block (SCSI CDB)
    pub cb: [u8; 16],
}

impl Cbw {
    /// CBW signature constant.
    pub const SIGNATURE: u32 = 0x43425355;

    /// Creates a new CBW.
    pub fn new(tag: u32, length: u32, direction_in: bool, lun: u8, cdb: &[u8]) -> Self {
        let mut cb = [0u8; 16];
        let len = cdb.len().min(16);
        cb[..len].copy_from_slice(&cdb[..len]);

        Self {
            signature: Self::SIGNATURE,
            tag,
            data_transfer_length: length,
            flags: if direction_in { 0x80 } else { 0x00 },
            lun: lun & 0x0F,
            cb_length: len as u8,
            cb,
        }
    }
}

impl Default for Cbw {
    fn default() -> Self {
        Self {
            signature: Self::SIGNATURE,
            tag: 0,
            data_transfer_length: 0,
            flags: 0,
            lun: 0,
            cb_length: 0,
            cb: [0; 16],
        }
    }
}

/// Command Status Wrapper (CSW) - 13 bytes.
///
/// Status returned after a SCSI command completes.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Csw {
    /// Signature (must be 0x53425355 "USBS")
    pub signature: u32,
    /// Tag (should match CBW tag)
    pub tag: u32,
    /// Data residue (difference between expected and actual)
    pub data_residue: u32,
    /// Status (0=passed, 1=failed, 2=phase error)
    pub status: u8,
}

impl Csw {
    /// CSW signature constant.
    pub const SIGNATURE: u32 = 0x53425355;

    /// Command passed.
    pub const STATUS_PASSED: u8 = 0;
    /// Command failed.
    pub const STATUS_FAILED: u8 = 1;
    /// Phase error.
    pub const STATUS_PHASE_ERROR: u8 = 2;

    /// Returns true if the command completed successfully.
    pub fn is_ok(&self) -> bool {
        self.signature == Self::SIGNATURE && self.status == Self::STATUS_PASSED
    }
}

/// SCSI operation codes.
pub mod scsi_op {
    /// Test Unit Ready
    pub const TEST_UNIT_READY: u8 = 0x00;
    /// Request Sense
    pub const REQUEST_SENSE: u8 = 0x03;
    /// Format Unit
    pub const FORMAT_UNIT: u8 = 0x04;
    /// Inquiry
    pub const INQUIRY: u8 = 0x12;
    /// Mode Select (6)
    pub const MODE_SELECT_6: u8 = 0x15;
    /// Mode Sense (6)
    pub const MODE_SENSE_6: u8 = 0x1A;
    /// Start/Stop Unit
    pub const START_STOP_UNIT: u8 = 0x1B;
    /// Prevent/Allow Medium Removal
    pub const PREVENT_ALLOW_MEDIUM_REMOVAL: u8 = 0x1E;
    /// Read Format Capacities
    pub const READ_FORMAT_CAPACITIES: u8 = 0x23;
    /// Read Capacity (10)
    pub const READ_CAPACITY_10: u8 = 0x25;
    /// Read (10)
    pub const READ_10: u8 = 0x28;
    /// Write (10)
    pub const WRITE_10: u8 = 0x2A;
    /// Seek (10)
    pub const SEEK_10: u8 = 0x2B;
    /// Write and Verify (10)
    pub const WRITE_AND_VERIFY_10: u8 = 0x2E;
    /// Verify (10)
    pub const VERIFY_10: u8 = 0x2F;
    /// Synchronize Cache (10)
    pub const SYNCHRONIZE_CACHE_10: u8 = 0x35;
    /// Read TOC/PMA/ATIP
    pub const READ_TOC: u8 = 0x43;
    /// Mode Select (10)
    pub const MODE_SELECT_10: u8 = 0x55;
    /// Mode Sense (10)
    pub const MODE_SENSE_10: u8 = 0x5A;
    /// Read (12)
    pub const READ_12: u8 = 0xA8;
    /// Write (12)
    pub const WRITE_12: u8 = 0xAA;
    /// Read Capacity (16)
    pub const READ_CAPACITY_16: u8 = 0x9E;
    /// Read (16)
    pub const READ_16: u8 = 0x88;
    /// Write (16)
    pub const WRITE_16: u8 = 0x8A;
}

/// SCSI Inquiry data (standard response, 36 bytes minimum).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct InquiryData {
    /// Peripheral qualifier and device type
    pub peripheral: u8,
    /// RMB (removable media bit) in bit 7
    pub rmb: u8,
    /// Version
    pub version: u8,
    /// Response data format
    pub response_format: u8,
    /// Additional length
    pub additional_length: u8,
    /// Flags
    pub flags: [u8; 3],
    /// Vendor identification (8 bytes)
    pub vendor: [u8; 8],
    /// Product identification (16 bytes)
    pub product: [u8; 16],
    /// Product revision (4 bytes)
    pub revision: [u8; 4],
}

impl InquiryData {
    /// Returns the peripheral device type (0x00 = direct access block device).
    pub fn device_type(&self) -> u8 {
        self.peripheral & 0x1F
    }

    /// Returns true if the medium is removable.
    pub fn is_removable(&self) -> bool {
        (self.rmb & 0x80) != 0
    }
}

/// Read Capacity (10) response data.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ReadCapacity10Data {
    /// Last logical block address (big-endian)
    pub last_lba: u32,
    /// Block size in bytes (big-endian)
    pub block_size: u32,
}

impl ReadCapacity10Data {
    /// Returns the last LBA (converted from big-endian).
    pub fn last_lba(&self) -> u32 {
        u32::from_be(self.last_lba)
    }

    /// Returns the block size (converted from big-endian).
    pub fn block_size(&self) -> u32 {
        u32::from_be(self.block_size)
    }

    /// Returns the total capacity in bytes.
    pub fn capacity_bytes(&self) -> u64 {
        (self.last_lba() as u64 + 1) * self.block_size() as u64
    }
}

/// Request Sense data (fixed format).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RequestSenseData {
    /// Response code (0x70 or 0x71)
    pub response_code: u8,
    /// Obsolete
    pub obsolete: u8,
    /// Sense key, flags
    pub sense_key: u8,
    /// Information
    pub information: [u8; 4],
    /// Additional sense length
    pub additional_sense_length: u8,
    /// Command-specific information
    pub command_specific: [u8; 4],
    /// Additional sense code
    pub asc: u8,
    /// Additional sense code qualifier
    pub ascq: u8,
    /// Field replaceable unit code
    pub fruc: u8,
    /// Sense key specific
    pub sense_key_specific: [u8; 3],
}

impl RequestSenseData {
    /// Returns the sense key.
    pub fn sense_key(&self) -> u8 {
        self.sense_key & 0x0F
    }
}

/// SCSI sense keys.
pub mod sense_key {
    /// No sense
    pub const NO_SENSE: u8 = 0x00;
    /// Recovered error
    pub const RECOVERED_ERROR: u8 = 0x01;
    /// Not ready
    pub const NOT_READY: u8 = 0x02;
    /// Medium error
    pub const MEDIUM_ERROR: u8 = 0x03;
    /// Hardware error
    pub const HARDWARE_ERROR: u8 = 0x04;
    /// Illegal request
    pub const ILLEGAL_REQUEST: u8 = 0x05;
    /// Unit attention
    pub const UNIT_ATTENTION: u8 = 0x06;
    /// Data protect
    pub const DATA_PROTECT: u8 = 0x07;
    /// Blank check
    pub const BLANK_CHECK: u8 = 0x08;
    /// Vendor specific
    pub const VENDOR_SPECIFIC: u8 = 0x09;
    /// Copy aborted
    pub const COPY_ABORTED: u8 = 0x0A;
    /// Aborted command
    pub const ABORTED_COMMAND: u8 = 0x0B;
    /// Volume overflow
    pub const VOLUME_OVERFLOW: u8 = 0x0D;
    /// Miscompare
    pub const MISCOMPARE: u8 = 0x0E;
}

/// USB Mass Storage device.
pub struct MscDevice<H: Dma> {
    device: Arc<UsbDevice<H>>,
    interface: u8,
    ep_in: u8,
    ep_out: u8,
    ep_in_max_packet: u16,
    ep_out_max_packet: u16,
    max_lun: u8,
    tag: u32,
}

impl<H: Dma> MscDevice<H> {
    /// Creates a new MSC device from interface and endpoint descriptors.
    pub fn from_interface(
        device: Arc<UsbDevice<H>>,
        iface: &InterfaceDesc,
        ep_in: &EndpointDesc,
        ep_out: &EndpointDesc,
    ) -> Result<Self> {
        if iface.interface_class != class::MASS_STORAGE {
            return Err(UsbError::NotSupported);
        }

        // Configure endpoints
        device.configure_endpoint(ep_in)?;
        device.configure_endpoint(ep_out)?;

        let mut msc = Self {
            device,
            interface: iface.interface_number,
            ep_in: ep_in.number(),
            ep_out: ep_out.number(),
            ep_in_max_packet: ep_in.max_packet_size,
            ep_out_max_packet: ep_out.max_packet_size,
            max_lun: 0,
            tag: 1,
        };

        // Get max LUN
        msc.max_lun = msc.get_max_lun().unwrap_or(0);

        Ok(msc)
    }

    /// Returns the maximum LUN number.
    pub fn max_lun(&self) -> u8 {
        self.max_lun
    }

    /// Gets the maximum LUN from the device.
    fn get_max_lun(&self) -> Result<u8> {
        let mut buf = [0u8; 1];
        let setup = SetupPacket::msc_get_max_lun(self.interface);
        match self.device.control_transfer(&setup, Some(&mut buf)) {
            Ok(_) => Ok(buf[0]),
            Err(UsbError::Stall) => Ok(0), // Single LUN device
            Err(e) => Err(e),
        }
    }

    /// Performs a Bulk-Only Mass Storage Reset.
    pub fn reset(&self) -> Result<()> {
        let setup = SetupPacket::msc_reset(self.interface);
        self.device.control_transfer(&setup, None)?;
        Ok(())
    }

    /// Executes a SCSI command.
    pub fn scsi_command(
        &mut self,
        lun: u8,
        cdb: &[u8],
        data: Option<&mut [u8]>,
        direction_in: bool,
    ) -> Result<usize> {
        let host = self.device.ctrl().host();
        let data_len = data.as_ref().map(|d| d.len()).unwrap_or(0);

        // Allocate buffers (64-byte alignment for DMA)
        let cbw_buf = PhysMem::alloc(host, core::mem::size_of::<Cbw>(), 64)?;
        let csw_buf = PhysMem::alloc(host, core::mem::size_of::<Csw>(), 64)?;
        let data_buf = if data_len > 0 {
            Some(PhysMem::alloc(host, data_len, 64)?)
        } else {
            None
        };

        // Build and send CBW
        let cbw = Cbw::new(self.tag, data_len as u32, direction_in, lun, cdb);
        self.tag = self.tag.wrapping_add(1);

        unsafe {
            core::ptr::copy_nonoverlapping(&cbw as *const Cbw as *const u8, cbw_buf.as_ptr(), 31);
        }

        self.device
            .queue_transfer(self.ep_out, false, &cbw_buf, 31)?;
        self.wait_transfer()?;

        // Data phase (if any)
        let transferred = if let (Some(buf), Some(ref mut d)) = (&data_buf, data) {
            if direction_in {
                // IN: device to host
                self.device
                    .queue_transfer(self.ep_in, true, buf, data_len)?;
                let len = self.wait_transfer()?;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        buf.as_ptr::<u8>(),
                        d.as_mut_ptr(),
                        len.min(d.len()),
                    );
                }
                len
            } else {
                // OUT: host to device
                unsafe {
                    core::ptr::copy_nonoverlapping(d.as_ptr(), buf.as_ptr(), d.len());
                }
                self.device
                    .queue_transfer(self.ep_out, false, buf, data_len)?;
                self.wait_transfer()?
            }
        } else {
            0
        };

        // Receive CSW
        self.device.queue_transfer(self.ep_in, true, &csw_buf, 13)?;
        self.wait_transfer()?;

        let csw = unsafe { *(csw_buf.as_ptr::<Csw>()) };

        // Free buffers
        cbw_buf.free(host);
        csw_buf.free(host);
        if let Some(buf) = data_buf {
            buf.free(host);
        }

        // Check CSW
        if !csw.is_ok() {
            return Err(UsbError::XferFail(csw.status));
        }

        Ok(transferred)
    }

    fn wait_transfer(&self) -> Result<usize> {
        loop {
            if let Some(evt) = self.device.ctrl().poll_event()
                && evt.slot_id() == self.device.slot_id()
            {
                let code = evt.completion_code();
                if code == 1 || code == 13 {
                    // SUCCESS or SHORT_PACKET
                    return Ok(evt.transfer_length() as usize);
                } else {
                    return Err(UsbError::XferFail(code));
                }
            }
            spin_loop();
        }
    }

    /// Sends TEST UNIT READY command.
    pub fn test_unit_ready(&mut self, lun: u8) -> Result<bool> {
        let cdb = [scsi_op::TEST_UNIT_READY, 0, 0, 0, 0, 0];
        match self.scsi_command(lun, &cdb, None, false) {
            Ok(_) => Ok(true),
            Err(UsbError::XferFail(1)) => Ok(false), // Command failed
            Err(e) => Err(e),
        }
    }

    /// Sends INQUIRY command.
    pub fn inquiry(&mut self, lun: u8) -> Result<InquiryData> {
        let cdb = [scsi_op::INQUIRY, 0, 0, 0, 36, 0];
        let mut data = [0u8; 36];
        self.scsi_command(lun, &cdb, Some(&mut data), true)?;
        Ok(unsafe { *(data.as_ptr() as *const InquiryData) })
    }

    /// Sends READ CAPACITY (10) command.
    pub fn read_capacity(&mut self, lun: u8) -> Result<ReadCapacity10Data> {
        let cdb = [scsi_op::READ_CAPACITY_10, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let mut data = [0u8; 8];
        self.scsi_command(lun, &cdb, Some(&mut data), true)?;
        Ok(unsafe { *(data.as_ptr() as *const ReadCapacity10Data) })
    }

    /// Sends REQUEST SENSE command.
    pub fn request_sense(&mut self, lun: u8) -> Result<RequestSenseData> {
        let cdb = [scsi_op::REQUEST_SENSE, 0, 0, 0, 18, 0];
        let mut data = [0u8; 18];
        self.scsi_command(lun, &cdb, Some(&mut data), true)?;
        Ok(unsafe { *(data.as_ptr() as *const RequestSenseData) })
    }

    /// Reads blocks from the device (READ 10).
    pub fn read_blocks(&mut self, lun: u8, lba: u32, count: u16, buf: &mut [u8]) -> Result<usize> {
        let cdb = [
            scsi_op::READ_10,
            0,
            (lba >> 24) as u8,
            (lba >> 16) as u8,
            (lba >> 8) as u8,
            lba as u8,
            0,
            (count >> 8) as u8,
            count as u8,
            0,
        ];
        self.scsi_command(lun, &cdb, Some(buf), true)
    }

    /// Writes blocks to the device (WRITE 10).
    pub fn write_blocks(&mut self, lun: u8, lba: u32, count: u16, buf: &mut [u8]) -> Result<usize> {
        let cdb = [
            scsi_op::WRITE_10,
            0,
            (lba >> 24) as u8,
            (lba >> 16) as u8,
            (lba >> 8) as u8,
            lba as u8,
            0,
            (count >> 8) as u8,
            count as u8,
            0,
        ];
        self.scsi_command(lun, &cdb, Some(buf), false)
    }

    /// Synchronizes the cache (SYNCHRONIZE CACHE 10).
    pub fn sync_cache(&mut self, lun: u8) -> Result<()> {
        let cdb = [scsi_op::SYNCHRONIZE_CACHE_10, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        self.scsi_command(lun, &cdb, None, false)?;
        Ok(())
    }

    /// Returns a reference to the underlying USB device.
    pub fn device(&self) -> &Arc<UsbDevice<H>> {
        &self.device
    }

    /// Returns the interface number.
    pub fn interface(&self) -> u8 {
        self.interface
    }
}

/// Parses configuration descriptor to find MSC interfaces.
pub fn find_msc_interfaces(
    config_data: &[u8],
) -> alloc::vec::Vec<(InterfaceDesc, EndpointDesc, EndpointDesc)> {
    use crate::desc::desc_type;

    let mut result = alloc::vec::Vec::new();
    let mut offset = 0;
    let mut current_iface: Option<InterfaceDesc> = None;
    let mut ep_in: Option<EndpointDesc> = None;
    let mut ep_out: Option<EndpointDesc> = None;

    while offset + 2 <= config_data.len() {
        let len = config_data[offset] as usize;
        let dtype = config_data[offset + 1];

        if len == 0 || offset + len > config_data.len() {
            break;
        }

        match dtype {
            desc_type::INTERFACE if len >= 9 => {
                // Save previous interface if complete
                if let (Some(iface), Some(ein), Some(eout)) = (current_iface, ep_in, ep_out) {
                    result.push((iface, ein, eout));
                }

                let iface = unsafe { *(config_data.as_ptr().add(offset) as *const InterfaceDesc) };
                if iface.interface_class == class::MASS_STORAGE
                    && iface.interface_protocol == msc_protocol::BBB
                {
                    current_iface = Some(iface);
                    ep_in = None;
                    ep_out = None;
                } else {
                    current_iface = None;
                }
            }
            desc_type::ENDPOINT if len >= 7 => {
                if current_iface.is_some() {
                    let ep = unsafe { *(config_data.as_ptr().add(offset) as *const EndpointDesc) };
                    if ep.transfer_type() == ep_type::BULK {
                        if ep.is_in() {
                            ep_in = Some(ep);
                        } else {
                            ep_out = Some(ep);
                        }
                    }
                }
            }
            _ => {}
        }

        offset += len;
    }

    // Save last interface if complete
    if let (Some(iface), Some(ein), Some(eout)) = (current_iface, ep_in, ep_out) {
        result.push((iface, ein, eout));
    }

    result
}
