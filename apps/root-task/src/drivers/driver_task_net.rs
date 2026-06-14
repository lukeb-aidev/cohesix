// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide Pi 4 NIC clients that route hardware ownership to driver tasks.
// Author: Lukas Bower

//! Pi 4 NIC clients for the driver-task runtime boundary.
//!
//! These devices intentionally do not map GENET, CYW43, or SDIO registers into
//! root. They give the root TCP console a smoltcp-compatible client endpoint
//! while physical hardware ownership sits behind driver-task shared-ring service
//! turns. GENET and CYW43 runtime construction is itself requested by a bounded
//! init command, so steady-state root code remains a ring client.

#![allow(unsafe_code)]

use core::fmt;
use core::sync::atomic::{AtomicU32, Ordering};

use smoltcp::phy::{self, Device, DeviceCapabilities};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;
use spin::Mutex;

use crate::drivers::cyw43_host_eapol::{
    self, HostEapolAction, HostEapolState, ETHER_ADDR_LEN, ETH_HEADER_LEN, ETH_P_EAPOL,
    WPA2_PSK_CCMP_RSN_IE, WSEC_KEY_PAYLOAD_LEN,
};
#[cfg(feature = "kernel")]
use crate::hal::driver_task::DriverTaskRingProgressSnapshot;
use crate::hal::driver_task::{
    DriverFrameDescriptor, DriverTaskBudgetGrant, DriverTaskCommandRecord,
    DriverTaskCompletionCode, DriverTaskCompletionRecord, DriverTaskContract, DriverTaskFaultCode,
    DriverTaskHotPath, DriverTaskStagingSegment, CYW43_WIFI_DRIVER_TASK_CONTRACT,
    GENET_DRIVER_TASK_CONTRACT, MAX_DRIVER_TASK_FRAME_BYTES, SDIO_HOST_DRIVER_TASK_CONTRACT,
};
use crate::hal::{HalError, Hardware};
use crate::net::{
    ConsoleNetConfig, NetDevice, NetDeviceCounters, NetDriverError, NetInterfacePolicy, NetStage,
    WifiCredentials, MAX_FRAME_LEN,
};
use pi4_driver_abi::{
    DriverRuntimeCyw43CommandDescriptor, DRIVER_RUNTIME_CYW43_COMMAND_AUX,
    DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER, DRIVER_RUNTIME_CYW43_FLAG_FORCE_BYTE_MODE,
    DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
    DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL, DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
    DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT, DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_MASK,
    DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME,
    DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL, DRIVER_RUNTIME_CYW43_OP_ETH_TX,
    DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK, DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP,
    DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK, DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL,
    DRIVER_RUNTIME_CYW43_OP_RELEASE, DRIVER_RUNTIME_CYW43_OP_RX_POLL,
    DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT, DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_F2_READ_FAILED,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_FAILED,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_INVALID_SDPCM,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_REMAINDER_FAILED,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_REMAINDER_TOO_LARGE,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_INVALID_RFRAME_LEN,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_NOT_READY, DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_NO_RFRAME,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_RFRAME_READ_FAILED,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_RX_REQUEST_TOO_LARGE,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_SDPCM_DECODE_MISS,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_CARD_INTERRUPT,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FRAME_INDICATED,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FUNCTION2_READY,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_HOST_INTERRUPT,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_MASK,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_SHIFT, DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_MAGIC,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_PROBE_LEN_MASK,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BACKPLANE_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BUS_LINK_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_CARD_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F1_BLOCK_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F1_ENABLED,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F2_BLOCK_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_HOST_READY, DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_START, DRIVER_RUNTIME_NET_INIT_AUX,
    DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED,
    DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_ADOPT_CLOCK_FAILED,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_ADOPT_INHIBIT_FAILED,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_ADOPT_POWER_MISSING,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_CLOCK_FAILED, DRIVER_RUNTIME_SDIO_INIT_DETAIL_INHIBIT_FAILED,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_RESET_ALL_FAILED,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_RESET_CMD_DATA_FAILED,
    DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES,
};

const GENET_DRIVER_TASK_MAC: EthernetAddress =
    EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x31]);
const CYW43_DRIVER_TASK_MAC: EthernetAddress =
    EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x32]);
const DRIVER_TASK_NET_STATUS: &str = "driver-task-ring-client";
const CYW43_RAM_BASE_4345: u32 = 0x0019_8000;
const CYW43_RAM_SIZE_4345_PI4: u32 = 0x000c_8000;
// Keep root-to-runtime firmware chunks aligned to the linked runtime's declared
// SDIO owner window so retained-stage recovery can replay an exact failed
// backplane window without restarting the whole firmware stream.
const CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES: usize =
    DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as usize;
const CYW43_RUNTIME_STREAM_PROGRESS_INTERVAL: usize = 32 * 1024;
const CYW43_RUNTIME_STREAM_COMMAND_RETRIES: usize = 2;
const CYW43_TRANSPORT_ADMISSION_REJECT_RETRIES: usize = 4;
const CYW43_RUNTIME_TRANSPORT_NO_REPLY_RESUMES: usize = 8;
const CYW43_RUNTIME_CONTROL_EXCHANGE_NO_REPLY_RESUMES: usize = 63;
const CYW43_RUNTIME_TRANSPORT_PHASE_ATTEMPTS: usize = 128;
const CYW43_RUNTIME_FIRMWARE_OWNER_RECOVERY_ATTEMPTS: usize = 192;
const CYW43_RUNTIME_FIRMWARE_OWNER_SAME_OFFSET_LIMIT: usize = 24;
const CYW43_BACKPLANE_ADDRESS_MASK: u32 = 0x7fff;
const CYW43_BACKPLANE_WINDOW_MASK: u32 = 0xffff_8000;
const CYW43_BACKPLANE_32BIT_FLAG: u32 = 0x8000;
const CYW43_CONTROL_PLANE_POLL_ATTEMPTS: usize = 256;
const CYW43_CONTROL_EXCHANGE_TIMEOUT_RESULT_MAGIC: u32 = 0x4300_0000;
const CYW43_HOST_EAPOL_PRE_ASSOC_POLLS: usize = 8_192;
const CYW43_HOST_EAPOL_POST_ASSOC_POLLS: usize = 16_384;
const CYW43_HOST_EAPOL_JOIN_POLLS: usize =
    CYW43_HOST_EAPOL_PRE_ASSOC_POLLS + CYW43_HOST_EAPOL_POST_ASSOC_POLLS;
const CYW43_HOST_EAPOL_START_FIRST_POLL: usize = 8_192;
const CYW43_HOST_EAPOL_START_INTERVAL_POLLS: usize = 8192;
const CYW43_HOST_EAPOL_START_MAX: u32 = 12;
const CYW43_HOST_EAPOL_TX_ATTEMPTS: usize = 8;
const CYW43_HOST_EAPOL_RX_REFRESH_AFTER_POST_ASSOC_POLLS: u32 = 1_024;
const CYW43_HOST_EAPOL_RX_RESCUE_AFTER_POST_ASSOC_POLLS: u32 = 4_096;
const CYW43_HOST_EAPOL_RX_RESCUE_AFTER_STARTS: u32 = 2;
const CYW43_HOST_EAPOL_ASSOC_PROBE_AFTER_POLLS: u32 = CYW43_HOST_EAPOL_PRE_ASSOC_POLLS as u32;
const CYW43_BCDC_HEADER_BYTES: usize = 16;
const CYW43_BDC_HEADER_BYTES: usize = 4;
const CYW43_BDC_VERSION: u8 = 2;
const CYW43_BDC_VERSION_SHIFT: u8 = 4;
const CYW43_REVINFO_RESPONSE_BYTES: usize = 68;
const CYW43_BCDC_FLAG_GET: u16 = 0x0000;
const CYW43_BCDC_FLAG_SET: u16 = 0x0002;
const CYW43_WLC_UP: u32 = 2;
const CYW43_WLC_SET_PROMISC: u32 = 10;
const CYW43_WLC_SET_INFRA: u32 = 20;
const CYW43_WLC_SET_AUTH: u32 = 22;
const CYW43_WLC_GET_BSSID: u32 = 23;
const CYW43_WLC_SET_SSID: u32 = 26;
const CYW43_WLC_GET_REVINFO: u32 = 98;
const CYW43_WLC_SET_WSEC: u32 = 134;
const CYW43_WLC_SET_WPA_AUTH: u32 = 165;
const CYW43_WLC_GET_VAR: u32 = 262;
const CYW43_WLC_SET_VAR: u32 = 263;
const CYW43_CONTROL_EXCHANGE_FAULT_DETAIL: u16 = 0x530b;
const CYW43_BCME_UNSUPPORTED_STATUS: u32 = 0xffff_ffe9;
const CYW43_BCME_BADARG_STATUS: u32 = 0xffff_fffe;
const CYW43_WSEC_NONE: u32 = 0;
const CYW43_WSEC_AES: u32 = 4;
const CYW43_WPA_AUTH_DISABLED: u32 = 0;
const CYW43_WPA2_AUTH_PSK_OR_UNSPECIFIED: u32 = 0x00c0;
const CYW43_WPA2_AUTH_PSK: u32 = 0x0080;
const CYW43_ETH_P_LINK_CTL: u16 = 0x886c;
const CYW43_PAE_GROUP_ADDR: [u8; 6] = [0x01, 0x80, 0xc2, 0x00, 0x00, 0x03];
const CYW43_EVENT_SET_SSID: u8 = 0;
const CYW43_EVENT_AUTH: u8 = 3;
const CYW43_EVENT_DEAUTH: u8 = 5;
const CYW43_EVENT_ASSOC: u8 = 7;
const CYW43_EVENT_ASSOC_IND: u8 = 8;
const CYW43_EVENT_REASSOC: u8 = 9;
const CYW43_EVENT_REASSOC_IND: u8 = 10;
const CYW43_EVENT_DISASSOC: u8 = 11;
const CYW43_EVENT_DISASSOC_IND: u8 = 12;
const CYW43_EVENT_LINK: u8 = 16;
const CYW43_EVENT_ROAM: u8 = 19;
const CYW43_EVENT_MIC_ERROR: u8 = 33;
const CYW43_EVENT_PSK_SUP: u8 = 46;
const CYW43_EVENT_FLAG_LINK: u16 = 0x0001;
const CYW43_EVENT_STATUS_SUCCESS: u32 = 0;
const CYW43_EVENT_MASK_LEN: usize = 27;
const CYW43_EVENTMSGS_EXT_VER: u8 = 1;
const CYW43_EVENTMSGS_EXT_SET_MASK: u8 = 3;
const CYW43_EVENTMSGS_EXT_MAX_GET_SIZE: u8 = 0;
const CYW43_EVENTMSGS_EXT_HEADER_LEN: usize = 4;
const CYW43_EVENTMSGS_EXT_PAYLOAD_LEN: usize =
    CYW43_EVENTMSGS_EXT_HEADER_LEN + CYW43_EVENT_MASK_LEN;
const CYW43_LINUX_EXT_JOIN_SSID_OFFSET: usize = 0;
const CYW43_LINUX_EXT_JOIN_SCAN_OFFSET: usize = CYW43_LINUX_EXT_JOIN_SSID_OFFSET + 36;
const CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET: usize = CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 20;
const CYW43_LINUX_BSSCFG_JOIN_PAYLOAD_LEN: usize = CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET + 12;
const CYW43_LINUX_EVENTMSGS_EXT_MASK: [u8; CYW43_EVENT_MASK_LEN] = [
    0x61, 0x15, 0x0b, 0x00, 0x02, 0x42, 0xc0, 0x11, 0x60, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x78, 0x00, 0x00, 0x00,
];
const CYW43_JOIN_COMPLETION_EVENTS: [u8; 13] = [
    CYW43_EVENT_SET_SSID,
    CYW43_EVENT_AUTH,
    CYW43_EVENT_ASSOC,
    CYW43_EVENT_REASSOC,
    CYW43_EVENT_LINK,
    CYW43_EVENT_DEAUTH,
    CYW43_EVENT_DISASSOC,
    CYW43_EVENT_DISASSOC_IND,
    CYW43_EVENT_ASSOC_IND,
    CYW43_EVENT_REASSOC_IND,
    CYW43_EVENT_ROAM,
    CYW43_EVENT_MIC_ERROR,
    CYW43_EVENT_PSK_SUP,
];
const CYW43_BCMILCP_SUBTYPE_VENDOR_LONG: u16 = 32769;
const CYW43_BCMILCP_BCM_SUBTYPE_EVENT: u16 = 1;
const CYW43_BROADCOM_OUI: [u8; 3] = [0x00, 0x10, 0x18];
const CYW43_BRCMF_EVENT_MIN_PACKET_LEN: usize = 72;
const CYW43_BRCMF_EVENT_FLAGS_OFFSET: usize = 26;
const CYW43_BRCMF_EVENT_TYPE_OFFSET: usize = 31;
const CYW43_BRCMF_EVENT_STATUS_OFFSET: usize = 32;
const CYW43_BRCMF_EVENT_REASON_OFFSET: usize = 36;
const CYW43_BRCMF_EVENT_AUTH_OFFSET: usize = 40;
const CYW43_BRCMF_EVENT_ADDR_OFFSET: usize = 48;
const SDIO_CMD53_BYTE_MODE_MAX: u16 = 512;
const CYW43_RUNTIME_FIRMWARE_TAIL_PAD_ALIGNMENT: usize = SDIO_CMD53_BYTE_MODE_MAX as usize;
const CYW43_RUNTIME_FIRMWARE_TAIL_PAD_MAX_BYTES: usize = 4096;
const SDHCI_INT_ERROR: u32 = 1 << 15;
const SDHCI_INT_TIMEOUT: u32 = 1 << 16;
const SDHCI_INT_CRC: u32 = 1 << 17;
const SDHCI_INT_END_BIT: u32 = 1 << 18;
const SDHCI_INT_INDEX: u32 = 1 << 19;
const SDHCI_INT_DATA_TIMEOUT: u32 = 1 << 20;
const SDHCI_INT_DATA_CRC: u32 = 1 << 21;
const SDHCI_INT_DATA_END_BIT: u32 = 1 << 22;
const SDIO_FAULT_TELEMETRY_MAGIC: u32 = 0x5344_494f;
const SDIO_FAULT_TELEMETRY_VERSION: u32 = 1;
const SDIO_FAULT_TELEMETRY_BYTES: usize = 56;
const SDIO_FAULT_TELEMETRY_ARG_OFFSET: usize = 8;
const SDIO_FAULT_TELEMETRY_CMD_FLAGS_OFFSET: usize = 12;
const SDIO_FAULT_TELEMETRY_LEN_BLOCK_OFFSET: usize = 16;
const SDIO_FAULT_TELEMETRY_COUNT_MODE_OFFSET: usize = 20;
const SDIO_FAULT_TELEMETRY_PRESENT_OFFSET: usize = 24;
const SDIO_FAULT_TELEMETRY_INT_STATUS_OFFSET: usize = 28;
const SDIO_FAULT_TELEMETRY_RESPONSE0_OFFSET: usize = 32;
const SDIO_FAULT_TELEMETRY_HOST_CLOCK_OFFSET: usize = 36;
const SDIO_FAULT_TELEMETRY_FAILURE_OFFSET: usize = 40;
const SDIO_FAULT_TELEMETRY_BLOCK_REG_OFFSET: usize = 44;
const SDIO_FAULT_TELEMETRY_PAYLOAD_EDGE_OFFSET: usize = 48;
const SDIO_FAULT_TELEMETRY_PAYLOAD_SUM_OFFSET: usize = 52;
static GENET_TX_SUBMITTED: AtomicU32 = AtomicU32::new(0);
static GENET_TX_DROPPED: AtomicU32 = AtomicU32::new(0);
static GENET_RX_FRAMES: AtomicU32 = AtomicU32::new(0);
static CYW43_TX_SUBMITTED: AtomicU32 = AtomicU32::new(0);
static CYW43_TX_DROPPED: AtomicU32 = AtomicU32::new(0);
static CYW43_RX_FRAMES: AtomicU32 = AtomicU32::new(0);
static GENET_LINKED_RUNTIME_READY: AtomicU32 = AtomicU32::new(0);
static CYW43_LINKED_RUNTIME_READY: AtomicU32 = AtomicU32::new(0);
static CYW43_CONTROL_PLANE_READY: AtomicU32 = AtomicU32::new(0);
static CYW43_ASSOCIATED: AtomicU32 = AtomicU32::new(0);
static CYW43_LINK_UP: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_RX: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_START: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_ACTIVE: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_REQUIRED: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_SECURE: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_TX_RETRIES: AtomicU32 = AtomicU32::new(0);
static CYW43_BCDC_IOCTL_ID: AtomicU32 = AtomicU32::new(0);
static CYW43_RUNTIME_MAC: Mutex<EthernetAddress> = Mutex::new(CYW43_DRIVER_TASK_MAC);
#[cfg(feature = "kernel")]
static CYW43_HOST_EAPOL_SESSION: Mutex<Option<Cyw43HostEapolSession>> = Mutex::new(None);
#[cfg(feature = "kernel")]
static CYW43_HOST_EAPOL_PENDING_EVENT: Mutex<Option<Cyw43PendingHostEapolEvent>> = Mutex::new(None);
#[cfg(feature = "kernel")]
static CYW43_ACTIVE_PROMPT_POLL_REQUEST: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static CYW43_ACTIVE_PROMPT_POLL_OP: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static CYW43_ACTIVE_PROMPT_POLL_FLAGS: AtomicU32 = AtomicU32::new(0);
static SDIO_LINKED_RUNTIME_READY: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static CYW43_LAST_RUNTIME_COMMAND_FAULT: Mutex<Option<Cyw43RuntimeCommandFaultStatus>> =
    Mutex::new(None);
#[cfg(feature = "kernel")]
static CYW43_LAST_SDIO_OWNER_FAULT: Mutex<Option<Cyw43SdioOwnerFaultStatus>> = Mutex::new(None);
#[cfg(feature = "kernel")]
static SDIO_LAST_RUNTIME_REPLAY_STATUS: Mutex<Option<SdioRuntimeReplayStatus>> = Mutex::new(None);

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Cyw43RuntimeCommandFaultStatus {
    pub stage: &'static str,
    pub op: u16,
    pub flags: u16,
    pub target_addr: u32,
    pub payload_offset: u16,
    pub payload_len: u16,
    pub total_len: u32,
    pub control_cmd: u32,
    pub control_id: u16,
    pub control_header_mode: &'static str,
    pub control_response_len: u16,
    pub detail: u16,
    pub reason: &'static str,
    pub result: u32,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cyw43ControlHeaderMode {
    Plain,
    Extended,
}

#[cfg(feature = "kernel")]
impl Cyw43ControlHeaderMode {
    const fn runtime_flags(self) -> u16 {
        match self {
            Self::Plain => 0,
            Self::Extended => DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Extended => "extended",
        }
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_descriptor_control_cmd(descriptor: DriverRuntimeCyw43CommandDescriptor) -> u32 {
    if descriptor.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE {
        descriptor.arg0
    } else {
        0
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_descriptor_control_id(descriptor: DriverRuntimeCyw43CommandDescriptor) -> u16 {
    if descriptor.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE {
        descriptor.arg1 as u16
    } else {
        0
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_descriptor_control_header_mode(
    descriptor: DriverRuntimeCyw43CommandDescriptor,
) -> &'static str {
    if descriptor.op != DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE {
        "not-control"
    } else if descriptor.flags & DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER != 0 {
        "extended"
    } else {
        "plain"
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SdioRuntimeReplayStatus {
    pub stage: &'static str,
    pub status: &'static str,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Cyw43SdioOwnerFaultStatus {
    pub stage: &'static str,
    pub op: u16,
    pub cmd: u16,
    pub arg: u32,
    pub function: u8,
    pub addr: u32,
    pub target_addr: u32,
    pub effective_target: u32,
    pub chunk_offset: u32,
    pub payload_offset: u32,
    pub increment: bool,
    pub write: bool,
    pub block_mode: bool,
    pub len: u16,
    pub block_size: u16,
    pub block_count: u16,
    pub transfer_mode: u16,
    pub host_control: u8,
    pub power_control: u8,
    pub clock_control: u16,
    pub present_state: u32,
    pub int_status: u32,
    pub response0: u32,
    pub block_size_count_reg: u32,
    pub detail: u16,
    pub reason: &'static str,
    pub transfer_stage: &'static str,
    pub transfer_status: u32,
    pub transfer_reason: &'static str,
    pub r5: u32,
    pub owner_window: &'static str,
    pub retry: &'static str,
    pub payload_first: u8,
    pub payload_last: u8,
    pub payload_xor: u8,
    pub payload_sum: u32,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43PayloadDigest {
    first: u8,
    last: u8,
    xor: u8,
    sum: u32,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Cyw43EventFrame {
    src_mac: [u8; 6],
    addr: [u8; 6],
    flags: u16,
    event_type: u8,
    status: u32,
    reason: u32,
    auth_type: u32,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Cyw43PendingHostEapolEvent {
    event: Cyw43EventFrame,
    flags: u16,
    len: u16,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Cyw43RxSourceResult {
    probe_len: u16,
    interrupt_enable: u8,
    frame_indicated: bool,
    host_interrupt: bool,
    card_interrupt: bool,
    function2_ready: bool,
}

#[cfg(feature = "kernel")]
fn cyw43_rx_source_result(result: u32) -> Option<Cyw43RxSourceResult> {
    if result & DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_MAGIC == 0 {
        return None;
    }
    Some(Cyw43RxSourceResult {
        probe_len: (result & DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_PROBE_LEN_MASK) as u16,
        interrupt_enable: ((result & DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_MASK)
            >> DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_SHIFT) as u8,
        frame_indicated: result & DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FRAME_INDICATED != 0,
        host_interrupt: result & DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_HOST_INTERRUPT != 0,
        card_interrupt: result & DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_CARD_INTERRUPT != 0,
        function2_ready: result & DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FUNCTION2_READY != 0,
    })
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Cyw43HostEapolProgress {
    polls: u32,
    data_rx: u32,
    eapol_rx: u32,
    non_eapol_rx: u32,
    event_rx: u32,
    control_rx: u32,
    empty_polls: u32,
    associated: bool,
    link_up: bool,
    association_event: Option<&'static str>,
    association_poll: u32,
    post_assoc_polls: u32,
    rx_firstread_attempts: u32,
    rx_firstread_empty: u32,
    rx_firstread_invalid: u32,
    rx_firstread_failed: u32,
    rx_firstread_remainder_failed: u32,
    rx_firstread_decode_miss: u32,
    control_rx_firstread_attempts: u32,
    control_rx_firstread_empty: u32,
    control_rx_firstread_failed: u32,
    last_rx_idle_detail: u16,
    last_rx_idle_result: u32,
    last_control_rx_idle_detail: u16,
    last_control_rx_idle_result: u32,
    last_rx_source: Option<Cyw43RxSourceResult>,
    last_control_rx_source: Option<Cyw43RxSourceResult>,
    last_flags: u16,
    last_len: u16,
    last_ethertype: u16,
    last_ethertype_valid: bool,
}

#[cfg(feature = "kernel")]
impl Cyw43HostEapolProgress {
    fn record_data_frame(&mut self, flags: u16, len: usize, ethertype: Option<u16>) {
        self.data_rx = self.data_rx.saturating_add(1);
        self.last_flags = flags;
        self.last_len = len.min(u16::MAX as usize) as u16;
        if let Some(ethertype) = ethertype {
            self.last_ethertype = ethertype;
            self.last_ethertype_valid = true;
            if ethertype == ETH_P_EAPOL {
                self.eapol_rx = self.eapol_rx.saturating_add(1);
            } else {
                self.non_eapol_rx = self.non_eapol_rx.saturating_add(1);
            }
        } else {
            self.last_ethertype = 0;
            self.last_ethertype_valid = false;
            self.non_eapol_rx = self.non_eapol_rx.saturating_add(1);
        }
    }

    fn record_control_frame(&mut self, flags: u16, len: usize) {
        self.control_rx = self.control_rx.saturating_add(1);
        self.last_flags = flags;
        self.last_len = len.min(u16::MAX as usize) as u16;
    }

    fn record_event_frame(&mut self, flags: u16, len: usize, event: Cyw43EventFrame, poll: usize) {
        self.event_rx = self.event_rx.saturating_add(1);
        self.last_flags = flags;
        self.last_len = len.min(u16::MAX as usize) as u16;
        if let Some(associated) = cyw43_event_association_state_update(event) {
            self.associated = associated;
        }
        if let Some(link_up) = cyw43_event_link_state_update(event) {
            self.link_up = link_up;
            if link_up {
                self.associated = true;
            }
        }
        if self.association_event.is_none() {
            if let Some(label) = cyw43_host_eapol_post_assoc_event_label(event, self.associated) {
                self.association_event = Some(label);
                self.association_poll = (poll as u32).saturating_add(1);
            }
        }
    }

    fn record_empty_poll(&mut self) {
        self.empty_polls = self.empty_polls.saturating_add(1);
    }

    fn record_rx_idle_completion(&mut self, completion: DriverTaskCompletionRecord) {
        self.last_rx_idle_detail = completion.detail;
        self.last_rx_idle_result = completion.result;
        self.last_rx_source =
            if completion.detail == DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY {
                cyw43_rx_source_result(completion.result)
            } else {
                None
            };
        match completion.detail {
            DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY => {
                self.rx_firstread_attempts = self.rx_firstread_attempts.saturating_add(1);
                self.rx_firstread_empty = self.rx_firstread_empty.saturating_add(1);
            }
            DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_INVALID_SDPCM => {
                self.rx_firstread_attempts = self.rx_firstread_attempts.saturating_add(1);
                self.rx_firstread_invalid = self.rx_firstread_invalid.saturating_add(1);
            }
            DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_FAILED => {
                self.rx_firstread_attempts = self.rx_firstread_attempts.saturating_add(1);
                self.rx_firstread_failed = self.rx_firstread_failed.saturating_add(1);
            }
            DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_REMAINDER_FAILED
            | DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_REMAINDER_TOO_LARGE => {
                self.rx_firstread_attempts = self.rx_firstread_attempts.saturating_add(1);
                self.rx_firstread_remainder_failed =
                    self.rx_firstread_remainder_failed.saturating_add(1);
            }
            DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_SDPCM_DECODE_MISS => {
                self.rx_firstread_decode_miss = self.rx_firstread_decode_miss.saturating_add(1);
            }
            DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_F2_READ_FAILED => {
                self.rx_firstread_failed = self.rx_firstread_failed.saturating_add(1);
            }
            _ => {}
        }
    }

    fn record_control_rx_idle_completion(&mut self, completion: DriverTaskCompletionRecord) {
        self.last_control_rx_idle_detail = completion.detail;
        self.last_control_rx_idle_result = completion.result;
        self.last_control_rx_source =
            if completion.detail == DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY {
                cyw43_rx_source_result(completion.result)
            } else {
                None
            };
        match completion.detail {
            DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY => {
                self.control_rx_firstread_attempts =
                    self.control_rx_firstread_attempts.saturating_add(1);
                self.control_rx_firstread_empty = self.control_rx_firstread_empty.saturating_add(1);
            }
            DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_FAILED
            | DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_INVALID_SDPCM
            | DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_REMAINDER_FAILED
            | DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_REMAINDER_TOO_LARGE
            | DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_SDPCM_DECODE_MISS
            | DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_F2_READ_FAILED => {
                self.control_rx_firstread_attempts =
                    self.control_rx_firstread_attempts.saturating_add(1);
                self.control_rx_firstread_failed =
                    self.control_rx_firstread_failed.saturating_add(1);
            }
            _ => {}
        }
    }
}

#[cfg(feature = "kernel")]
fn cyw43_store_pending_host_eapol_event(
    contract: DriverTaskContract,
    stage: &'static str,
    flags: u16,
    len: usize,
    event: Cyw43EventFrame,
) {
    let associated_update = cyw43_event_association_state_update(event);
    let link_update = cyw43_event_link_state_update(event);
    let mut associated = CYW43_ASSOCIATED.load(Ordering::Acquire) != 0;
    let mut link_up = CYW43_LINK_UP.load(Ordering::Acquire) != 0;
    if let Some(next_associated) = associated_update {
        associated = next_associated;
    }
    if let Some(next_link_up) = link_update {
        link_up = next_link_up;
        if next_link_up {
            associated = true;
        }
    }
    CYW43_ASSOCIATED.store(if associated { 1 } else { 0 }, Ordering::Release);
    CYW43_LINK_UP.store(if link_up { 1 } else { 0 }, Ordering::Release);

    let event_label = cyw43_host_eapol_post_assoc_event_label(event, associated);
    let relevant = associated_update.is_some() || link_update.is_some() || event_label.is_some();
    if relevant {
        *CYW43_HOST_EAPOL_PENDING_EVENT.lock() = Some(Cyw43PendingHostEapolEvent {
            event,
            flags,
            len: len.min(u16::MAX as usize) as u16,
        });
    }
    emit_cyw43_host_eapol_event_capture(
        contract,
        stage,
        flags,
        len,
        event,
        event_label.unwrap_or("none"),
        relevant,
    );
}

#[cfg(feature = "kernel")]
fn cyw43_apply_pending_host_eapol_event(
    contract: DriverTaskContract,
    session: &mut Cyw43HostEapolSession,
    poll: usize,
) {
    let pending = {
        let mut guard = CYW43_HOST_EAPOL_PENDING_EVENT.lock();
        guard.take()
    };
    if let Some(pending) = pending {
        session.progress.record_event_frame(
            pending.flags,
            usize::from(pending.len),
            pending.event,
            poll,
        );
        emit_cyw43_host_eapol_status(contract, "event-rx", &session.progress);
    }
}

#[cfg(feature = "kernel")]
fn cyw43_capture_event_frame_from_token(
    contract: DriverTaskContract,
    stage: &'static str,
    flags: u16,
    token: &DriverTaskNetRxToken,
) -> bool {
    if cyw43_frame_channel(flags) != DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT {
        return false;
    }
    let frame = &token.buffer[..token.len];
    let Some(event) = cyw43_parse_control_or_event_frame(frame) else {
        return false;
    };
    cyw43_store_pending_host_eapol_event(contract, stage, flags, frame.len(), event);
    true
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy)]
struct Cyw43HostEapolSession {
    eapol: HostEapolState,
    progress: Cyw43HostEapolProgress,
    refreshed_after_assoc: bool,
    rescued_after_assoc: bool,
    probed_assoc_bssid: bool,
}

#[cfg(feature = "kernel")]
impl Cyw43HostEapolSession {
    fn new(credentials: WifiCredentials) -> Result<Self, DriverTaskNetError> {
        let ssid_len = usize::from(credentials.ssid_len);
        let psk_len = usize::from(credentials.psk_len);
        let ssid = &credentials.ssid[..ssid_len];
        let psk = &credentials.psk[..psk_len];
        let eapol = HostEapolState::new(ssid, psk).map_err(DriverTaskNetError::RuntimeInit)?;
        Ok(Self {
            eapol,
            progress: Cyw43HostEapolProgress::default(),
            refreshed_after_assoc: false,
            rescued_after_assoc: false,
            probed_assoc_bssid: false,
        })
    }
}

#[cfg(feature = "kernel")]
pub(crate) fn latest_cyw43_runtime_command_fault_status() -> Option<Cyw43RuntimeCommandFaultStatus>
{
    *CYW43_LAST_RUNTIME_COMMAND_FAULT.lock()
}

#[cfg(feature = "kernel")]
pub(crate) fn latest_cyw43_sdio_owner_fault_status() -> Option<Cyw43SdioOwnerFaultStatus> {
    *CYW43_LAST_SDIO_OWNER_FAULT.lock()
}

#[cfg(feature = "kernel")]
pub(crate) fn latest_sdio_runtime_replay_status() -> Option<SdioRuntimeReplayStatus> {
    *SDIO_LAST_RUNTIME_REPLAY_STATUS.lock()
}

#[cfg(feature = "kernel")]
fn clear_cyw43_runtime_command_fault_status() {
    *CYW43_LAST_RUNTIME_COMMAND_FAULT.lock() = None;
    *CYW43_LAST_SDIO_OWNER_FAULT.lock() = None;
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy)]
enum Cyw43CommandSubmitError {
    Runtime(DriverTaskNetError),
    Completion(DriverTaskCompletionRecord),
}

#[cfg(feature = "kernel")]
impl Cyw43CommandSubmitError {
    const fn into_net_error(self) -> DriverTaskNetError {
        match self {
            Self::Runtime(err) => err,
            Self::Completion(_) => DriverTaskNetError::RuntimeInit("cyw43-command"),
        }
    }

    fn recoverable_completion(self) -> Option<DriverTaskCompletionRecord> {
        match self {
            Self::Completion(completion)
                if completion.code == DriverTaskCompletionCode::Fault.as_u16()
                    && cyw43_fault_detail_allows_sdio_owner_recovery(completion.detail) =>
            {
                Some(completion)
            }
            _ => None,
        }
    }

    fn same_command_retry_completion(self) -> Option<DriverTaskCompletionRecord> {
        match self {
            Self::Completion(completion)
                if completion.code == DriverTaskCompletionCode::Fault.as_u16()
                    && cyw43_fault_detail_allows_same_command_retry(completion.detail) =>
            {
                Some(completion)
            }
            _ => None,
        }
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy)]
enum Cyw43FirmwareInitError {
    Runtime(DriverTaskNetError),
    Command(Cyw43CommandSubmitError),
}

#[cfg(feature = "kernel")]
impl Cyw43FirmwareInitError {
    fn into_net_error(self) -> DriverTaskNetError {
        match self {
            Self::Runtime(err) => err,
            Self::Command(err) => err.into_net_error(),
        }
    }

    fn recoverable_completion(self) -> Option<DriverTaskCompletionRecord> {
        match self {
            Self::Command(err) => err.recoverable_completion(),
            _ => None,
        }
    }

    fn same_command_retry_completion(self) -> Option<DriverTaskCompletionRecord> {
        match self {
            Self::Command(err) => err.same_command_retry_completion(),
            _ => None,
        }
    }
}

#[cfg(feature = "kernel")]
fn run_driver_task_net_service(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        crate::hal::driver_task::run_driver_task_ring_service_nonblocking(contract, command)
    } else {
        crate::hal::driver_task::run_driver_task_ring_service(contract, command)
    }
}

#[cfg(feature = "kernel")]
fn run_driver_task_net_service_staged(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
    staging_segments: &[DriverTaskStagingSegment<'_>],
) -> Option<DriverTaskCompletionRecord> {
    if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        crate::hal::driver_task::run_driver_task_ring_service_nonblocking_staged(
            contract,
            command,
            staging_segments,
        )
    } else {
        crate::hal::driver_task::run_driver_task_ring_service_staged(
            contract,
            command,
            staging_segments,
        )
    }
}

#[cfg(feature = "kernel")]
fn run_driver_task_net_service_prompt_slice_staged(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
    staging_segments: &[DriverTaskStagingSegment<'_>],
) -> Option<DriverTaskCompletionRecord> {
    if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        crate::hal::driver_task::run_driver_task_ring_service_prompt_slice_staged(
            contract,
            command,
            staging_segments,
        )
    } else {
        crate::hal::driver_task::run_driver_task_ring_service_staged(
            contract,
            command,
            staging_segments,
        )
    }
}

#[cfg(not(feature = "kernel"))]
fn run_driver_task_net_service(
    _contract: DriverTaskContract,
    _command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    None
}

#[cfg(feature = "kernel")]
fn driver_task_resource_completion_status(
    completion: Option<DriverTaskCompletionRecord>,
    ready: bool,
) -> &'static str {
    if ready {
        "ready"
    } else {
        match completion {
            Some(completion) if completion.code == DriverTaskCompletionCode::Fault.as_u16() => {
                "fault"
            }
            Some(_) => "unexpected-completion",
            None => "no-reply",
        }
    }
}

#[cfg(feature = "kernel")]
const fn sdio_engine_init_detail_status(detail: u16) -> Option<&'static str> {
    match detail {
        detail if detail == DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH as u16 => {
            Some("resource-hot-path-mismatch")
        }
        detail
            if detail == DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED as u16 =>
        {
            Some("resource-check-failed")
        }
        DRIVER_RUNTIME_SDIO_INIT_DETAIL_ADOPT_POWER_MISSING => Some("adopt-power-missing"),
        DRIVER_RUNTIME_SDIO_INIT_DETAIL_ADOPT_CLOCK_FAILED => Some("adopt-clock-failed"),
        DRIVER_RUNTIME_SDIO_INIT_DETAIL_ADOPT_INHIBIT_FAILED => Some("adopt-inhibit-failed"),
        DRIVER_RUNTIME_SDIO_INIT_DETAIL_RESET_ALL_FAILED => Some("reset-all-failed"),
        DRIVER_RUNTIME_SDIO_INIT_DETAIL_RESET_CMD_DATA_FAILED => Some("reset-cmd-data-failed"),
        DRIVER_RUNTIME_SDIO_INIT_DETAIL_CLOCK_FAILED => Some("clock-failed"),
        DRIVER_RUNTIME_SDIO_INIT_DETAIL_INHIBIT_FAILED => Some("inhibit-failed"),
        _ => None,
    }
}

#[cfg(feature = "kernel")]
fn sdio_engine_init_completion_status(
    completion: Option<DriverTaskCompletionRecord>,
    ready: bool,
) -> &'static str {
    if ready {
        return "ready";
    }
    match completion {
        Some(completion) if completion.code == DriverTaskCompletionCode::Fault.as_u16() => {
            sdio_engine_init_detail_status(completion.detail).unwrap_or("fault")
        }
        _ => driver_task_resource_completion_status(completion, ready),
    }
}

#[cfg(feature = "kernel")]
fn emit_net_driver_task_replay_status(
    config: ConsoleNetConfig,
    hot_path: DriverTaskHotPath,
    stage: &'static str,
    status: &'static str,
) {
    use core::fmt::Write;

    let selected = match (config.policy.interface, hot_path) {
        (NetInterfacePolicy::Wired, DriverTaskHotPath::GenetNic)
        | (NetInterfacePolicy::Wifi, DriverTaskHotPath::Cyw43Wifi)
        | (NetInterfacePolicy::Auto, DriverTaskHotPath::GenetNic)
        | (NetInterfacePolicy::Auto, DriverTaskHotPath::Cyw43Wifi) => "yes",
        _ => "no",
    };
    let mut line = heapless::String::<320>::new();
    let _ = write!(
        line,
        "NET_DRIVER_TASK_REPLAY_STATUS role={} selected={} policy={} attempted=yes stage={} blocker={} owner=linked-runtime root_action=descriptor-replay proof_effect={} next_action={}",
        hot_path.as_str(),
        selected,
        config.policy.interface.as_str(),
        stage,
        status,
        driver_task_replay_proof_effect(status),
        driver_task_replay_next_action(stage, status),
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_sdio_driver_task_replay_status(stage: &'static str, status: &'static str) {
    use core::fmt::Write;

    *SDIO_LAST_RUNTIME_REPLAY_STATUS.lock() = Some(SdioRuntimeReplayStatus { stage, status });
    let mut line = heapless::String::<320>::new();
    let _ = write!(
        line,
        "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host selected=wifi-owner-link attempted=yes stage={} blocker={} owner=linked-runtime root_action=descriptor-replay proof_effect={} next_action={}",
        stage,
        status,
        driver_task_replay_proof_effect(status),
        driver_task_replay_next_action(stage, status),
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn driver_task_replay_proof_effect(status: &str) -> &'static str {
    match status {
        "ready" | "preserved-ready" => "replay-ready",
        "begin" | "pending" | "retry" => "replay-in-progress",
        _ => "acceptance-red",
    }
}

#[cfg(feature = "kernel")]
fn driver_task_replay_next_action(stage: &str, status: &str) -> &'static str {
    match status {
        "ready" | "preserved-ready" => "continue-next-driver-gate",
        "begin" | "pending" | "retry" => "poll-linked-runtime-replay",
        "no-reply" => "inspect-linked-runtime-progress",
        "unsupported" => "use-linked-runtime-supported-command",
        "failed" | "fault" | "clock-failed" => "inspect-linked-runtime-fault",
        _ if stage.contains("descriptor") => "inspect-runtime-descriptor",
        _ => "inspect-linked-runtime-replay",
    }
}

/// Error surfaced by driver-task NIC clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverTaskNetError {
    /// The isolated driver runtime has not completed its hardware service turn.
    RuntimePending(&'static str),
    /// The driver-task runtime could not initialise the real hardware backend.
    RuntimeInit(&'static str),
}

impl fmt::Display for DriverTaskNetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimePending(role) => {
                write!(f, "{role} driver-task runtime is pending hardware service")
            }
            Self::RuntimeInit(role) => write!(f, "{role} driver-task runtime init failed"),
        }
    }
}

impl NetDriverError for DriverTaskNetError {
    fn is_absent(&self) -> bool {
        false
    }
}

/// Construct the GENET hardware runtime behind the driver-task ring client.
pub fn init_genet_runtime<H>(
    hal: &mut H,
    config: &ConsoleNetConfig,
    stage: NetStage,
) -> Result<(), DriverTaskNetError>
where
    H: Hardware<Error = HalError>,
{
    init_runtime_via_driver_task::<H>(
        hal,
        *config,
        stage,
        GENET_DRIVER_TASK_CONTRACT,
        DriverTaskHotPath::GenetNic,
    )
}

/// Construct the CYW43 hardware runtime behind the driver-task ring client.
pub fn init_cyw43_runtime<H>(
    hal: &mut H,
    config: &ConsoleNetConfig,
    stage: NetStage,
) -> Result<(), DriverTaskNetError>
where
    H: Hardware<Error = HalError>,
{
    init_runtime_via_driver_task::<H>(
        hal,
        *config,
        stage,
        CYW43_WIFI_DRIVER_TASK_CONTRACT,
        DriverTaskHotPath::Cyw43Wifi,
    )
}

fn init_runtime_via_driver_task<H>(
    hal: &mut H,
    config: ConsoleNetConfig,
    stage: NetStage,
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
) -> Result<(), DriverTaskNetError>
where
    H: Hardware<Error = HalError>,
{
    if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
            contract,
            hot_path.as_u32() as usize,
            runtime_ring_service,
        );
        emit_net_driver_task_replay_status(config, hot_path, "descriptor-replay", "begin");
        if !crate::hal::driver_task::ensure_deferred_runtime_init_descriptor(contract, hot_path) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                hot_path,
                "runtime-descriptor-replay",
                "pending",
                None,
            );
            emit_net_driver_task_replay_status(config, hot_path, "descriptor-replay", "pending");
            return Err(DriverTaskNetError::RuntimePending(hot_path.as_str()));
        }
        emit_net_driver_task_replay_status(config, hot_path, "descriptor-replay", "ready");
        if hot_path == DriverTaskHotPath::Cyw43Wifi {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                SDIO_HOST_DRIVER_TASK_CONTRACT,
                DriverTaskHotPath::SdioHost,
                "cyw43-sdio-prereq",
                "begin",
                None,
            );
            if let Err(err) = init_sdio_host_linked_runtime(hal) {
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    SDIO_HOST_DRIVER_TASK_CONTRACT,
                    DriverTaskHotPath::SdioHost,
                    "cyw43-sdio-prereq",
                    "failed",
                    None,
                );
                emit_net_driver_task_replay_status(config, hot_path, "cyw43-sdio-prereq", "failed");
                return Err(err);
            }
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                SDIO_HOST_DRIVER_TASK_CONTRACT,
                DriverTaskHotPath::SdioHost,
                "cyw43-sdio-prereq",
                "ready",
                None,
            );
            emit_net_driver_task_replay_status(config, hot_path, "cyw43-sdio-prereq", "ready");
        }
        let mut command = DriverTaskCommandRecord::pi4_hot_path(
            0,
            hot_path,
            DriverTaskBudgetGrant::from_contract(contract),
            DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        command.aux0 = DRIVER_RUNTIME_NET_INIT_AUX;
        emit_net_driver_task_replay_status(config, hot_path, "engine-init", "begin");
        let completion = run_driver_task_net_service(contract, command);
        let initialized = completion.is_some_and(|completion| {
            completion.code == DriverTaskCompletionCode::Progress.as_u16() && completion.result == 1
        });
        let status = driver_task_resource_completion_status(completion, initialized);
        emit_net_driver_task_replay_status(config, hot_path, "engine-init", status);
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            hot_path,
            "net-engine-init",
            status,
            completion,
        );
        if initialized {
            match hot_path {
                DriverTaskHotPath::GenetNic => {
                    if !crate::hal::driver_task::register_driver_task_runtime_owner_state(hot_path)
                    {
                        crate::hal::driver_task::emit_driver_task_resource_init_status(
                            contract,
                            hot_path,
                            "genet-owner-state",
                            "descriptor-rejected",
                            None,
                        );
                        emit_net_driver_task_replay_status(
                            config,
                            hot_path,
                            "owner-state",
                            "descriptor-rejected",
                        );
                        return Err(DriverTaskNetError::RuntimeInit("genet-owner-state"));
                    }
                    GENET_LINKED_RUNTIME_READY.store(1, Ordering::Release);
                    emit_net_driver_task_replay_status(config, hot_path, "owner-state", "ready");
                }
                DriverTaskHotPath::Cyw43Wifi => {
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        contract,
                        hot_path,
                        "cyw43-firmware",
                        "begin",
                        None,
                    );
                    emit_net_driver_task_replay_status(config, hot_path, "cyw43-firmware", "begin");
                    if let Err(err) = complete_cyw43_linked_runtime_firmware(hal, contract) {
                        crate::hal::driver_task::emit_driver_task_resource_init_status(
                            contract,
                            hot_path,
                            "cyw43-firmware",
                            "failed",
                            None,
                        );
                        emit_net_driver_task_replay_status(
                            config,
                            hot_path,
                            "cyw43-firmware",
                            "failed",
                        );
                        return Err(err);
                    }
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        contract,
                        hot_path,
                        "cyw43-firmware",
                        "ready",
                        None,
                    );
                    emit_net_driver_task_replay_status(config, hot_path, "cyw43-firmware", "ready");
                    if !crate::hal::driver_task::register_driver_task_runtime_owner_state(hot_path)
                    {
                        crate::hal::driver_task::emit_driver_task_resource_init_status(
                            contract,
                            hot_path,
                            "cyw43-owner-state",
                            "descriptor-rejected",
                            None,
                        );
                        emit_net_driver_task_replay_status(
                            config,
                            hot_path,
                            "owner-state",
                            "descriptor-rejected",
                        );
                        return Err(DriverTaskNetError::RuntimeInit("cyw43-owner-state"));
                    }
                    CYW43_LINKED_RUNTIME_READY.store(1, Ordering::Release);
                    emit_net_driver_task_replay_status(config, hot_path, "owner-state", "ready");
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        contract,
                        hot_path,
                        "cyw43-control-plane",
                        "begin",
                        None,
                    );
                    emit_net_driver_task_replay_status(
                        config,
                        hot_path,
                        "cyw43-control-plane",
                        "begin",
                    );
                    if let Err(err) = complete_cyw43_linked_runtime_control_plane(hal, config) {
                        crate::hal::driver_task::emit_driver_task_resource_init_status(
                            contract,
                            hot_path,
                            "cyw43-control-plane",
                            "failed",
                            None,
                        );
                        emit_net_driver_task_replay_status(
                            config,
                            hot_path,
                            "cyw43-control-plane",
                            "failed",
                        );
                        return Err(err);
                    }
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        contract,
                        hot_path,
                        "cyw43-control-plane",
                        "ready",
                        None,
                    );
                    emit_net_driver_task_replay_status(
                        config,
                        hot_path,
                        "cyw43-control-plane",
                        "ready",
                    );
                }
                _ => {}
            }
            return Ok(());
        }
        return Err(DriverTaskNetError::RuntimeInit(hot_path.as_str()));
    }

    let _ = (hal, config, stage);
    Err(DriverTaskNetError::RuntimeInit(hot_path.as_str()))
}

#[cfg(feature = "kernel")]
fn complete_cyw43_linked_runtime_firmware<H>(
    hal: &mut H,
    contract: DriverTaskContract,
) -> Result<(), DriverTaskNetError>
where
    H: Hardware<Error = HalError>,
{
    let bundle = hal
        .wifi_firmware_bundle()
        .map_err(|_| DriverTaskNetError::RuntimeInit("cyw43-firmware-bundle"))?;
    bundle
        .validate()
        .map_err(|_| DriverTaskNetError::RuntimeInit("cyw43-firmware-bundle"))?;
    let reset_vector = firmware_reset_vector(bundle.firmware)
        .ok_or(DriverTaskNetError::RuntimeInit("cyw43-rstvec"))?;
    let nvram_len = crate::hal::pi4_wifi::normalize_nvram(bundle.nvram).len();
    clear_cyw43_runtime_command_fault_status();
    let mut recovery_attempts = 0usize;
    let mut last_resume_offset = None;
    let mut same_resume_offset_attempts = 0usize;
    let mut next_resume: Option<(usize, bool)> = None;
    loop {
        let result = if let Some((resume_offset, force_resume_byte_mode)) = next_resume.take() {
            complete_cyw43_linked_runtime_firmware_from_offset(
                hal,
                contract,
                bundle,
                reset_vector,
                resume_offset,
                force_resume_byte_mode,
            )
        } else {
            complete_cyw43_linked_runtime_firmware_once(hal, contract, bundle, reset_vector)
        };
        match result {
            Ok(()) => {
                clear_cyw43_runtime_command_fault_status();
                return Ok(());
            }
            Err(err) => {
                if recovery_attempts >= CYW43_RUNTIME_FIRMWARE_OWNER_RECOVERY_ATTEMPTS {
                    return Err(err.into_net_error());
                }
                let Some(completion) = err.recoverable_completion() else {
                    return Err(err.into_net_error());
                };
                let resume_fault = latest_cyw43_runtime_command_fault_status();
                let resume_offset = resume_fault.and_then(|fault| {
                    cyw43_firmware_resume_offset(fault, bundle.firmware.len()).or_else(|| {
                        cyw43_nvram_tail_resume_offset(fault, bundle.firmware.len(), nvram_len)
                    })
                });
                if resume_offset.is_none() && recovery_attempts != 0 {
                    return Err(err.into_net_error());
                }
                let force_resume_byte_mode =
                    resume_fault.is_some_and(cyw43_firmware_resume_forces_byte_mode);
                if let Some(resume_offset) = resume_offset {
                    if last_resume_offset == Some(resume_offset) {
                        same_resume_offset_attempts = same_resume_offset_attempts.saturating_add(1);
                    } else {
                        last_resume_offset = Some(resume_offset);
                        same_resume_offset_attempts = 1;
                    }
                    if same_resume_offset_attempts > CYW43_RUNTIME_FIRMWARE_OWNER_SAME_OFFSET_LIMIT
                    {
                        return Err(err.into_net_error());
                    }
                }
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    contract,
                    DriverTaskHotPath::Cyw43Wifi,
                    "cyw43-firmware-recover",
                    "sdio-owner-replay",
                    Some(completion),
                );
                replay_sdio_host_linked_runtime_preserving_hal(hal, "cyw43-firmware-recover")?;
                recovery_attempts = recovery_attempts.saturating_add(1);
                if let Some(resume_offset) = resume_offset {
                    emit_cyw43_runtime_firmware_recovery(
                        contract,
                        recovery_attempts,
                        resume_offset,
                        force_resume_byte_mode,
                        same_resume_offset_attempts,
                    );
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        contract,
                        DriverTaskHotPath::Cyw43Wifi,
                        "cyw43-firmware-recover",
                        "resume-retained-stage",
                        Some(completion),
                    );
                    next_resume = Some((resume_offset, force_resume_byte_mode));
                }
            }
        }
    }
}

#[cfg(feature = "kernel")]
fn complete_cyw43_linked_runtime_control_plane<H>(
    hal: &mut H,
    config: ConsoleNetConfig,
) -> Result<(), DriverTaskNetError>
where
    H: Hardware<Error = HalError>,
{
    let _ = hal;
    let contract = CYW43_WIFI_DRIVER_TASK_CONTRACT;
    reset_cyw43_control_plane_state();
    let Some(credentials) = config.wifi_credentials else {
        return Err(DriverTaskNetError::RuntimeInit("wifi-credentials-missing"));
    };
    if !credentials.has_ssid() {
        return Err(DriverTaskNetError::RuntimeInit("wifi-ssid-missing"));
    }
    let mac = cyw43_prepare_runtime_control_plane(contract)?;
    *CYW43_RUNTIME_MAC.lock() = mac;
    cyw43_enable_join_event_messages(contract, "cyw43-control-event-mask")?;
    cyw43_submit_bcdc_empty(contract, CYW43_WLC_UP, "cyw43-control-up")?;
    cyw43_submit_bcdc_u32(contract, CYW43_WLC_SET_INFRA, 1, "cyw43-control-infra")?;
    let _ = cyw43_poll_control_plane_frames("cyw43-control-post-up-event-drain");
    cyw43_enable_join_event_messages(contract, "cyw43-control-event-mask-post-up")?;
    if credentials.has_psk() {
        cyw43_submit_bcdc_iovar_bytes(
            contract,
            "wpaie",
            &WPA2_PSK_CCMP_RSN_IE,
            "cyw43-control-wpaie",
        )?;
        cyw43_submit_bcdc_u32(
            contract,
            CYW43_WLC_SET_WPA_AUTH,
            CYW43_WPA2_AUTH_PSK_OR_UNSPECIFIED,
            "cyw43-control-wpa-auth-initial",
        )?;
        cyw43_submit_bcdc_u32(contract, CYW43_WLC_SET_AUTH, 0, "cyw43-control-auth")?;
        cyw43_submit_bcdc_u32(
            contract,
            CYW43_WLC_SET_WSEC,
            CYW43_WSEC_AES,
            "cyw43-control-security-wpa2-psk",
        )?;
        cyw43_submit_bcdc_u32(
            contract,
            CYW43_WLC_SET_WPA_AUTH,
            CYW43_WPA2_AUTH_PSK,
            "cyw43-control-wpa-auth-final",
        )?;
        cyw43_configure_host_eapol_rx(contract)?;
        cyw43_submit_join_request(contract, credentials)?;
        arm_cyw43_host_eapol_pending(contract, credentials, mac)?;
        return Ok(());
    }
    cyw43_submit_bcdc_u32(contract, CYW43_WLC_SET_AUTH, 0, "cyw43-control-auth")?;
    cyw43_submit_bcdc_u32(
        contract,
        CYW43_WLC_SET_WSEC,
        CYW43_WSEC_NONE,
        "cyw43-control-security-open",
    )?;
    cyw43_submit_bcdc_u32(
        contract,
        CYW43_WLC_SET_WPA_AUTH,
        CYW43_WPA_AUTH_DISABLED,
        "cyw43-control-wpa-auth",
    )?;
    cyw43_submit_join_request(contract, credentials)?;
    let _observed = cyw43_poll_control_plane_frames("cyw43-control-poll");
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        "cyw43-join-event",
        "required",
        None,
    );
    Err(DriverTaskNetError::RuntimeInit("join-event-required"))
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_empty(
    contract: DriverTaskContract,
    cmd: u32,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_frame(&mut frame, cmd, CYW43_BCDC_FLAG_SET, id, &[])?;
    cyw43_submit_control_exchange_checked(contract, &frame[..len], cmd, id, stage)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_u32(
    contract: DriverTaskContract,
    cmd: u32,
    value: u32,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    let mut payload = [0u8; 4];
    payload.copy_from_slice(&value.to_le_bytes());
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_frame(&mut frame, cmd, CYW43_BCDC_FLAG_SET, id, &payload)?;
    cyw43_submit_control_exchange_checked(contract, &frame[..len], cmd, id, stage)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_ssid(
    contract: DriverTaskContract,
    credentials: crate::net::WifiCredentials,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    let ssid_len = usize::from(credentials.ssid_len);
    let mut ssid_payload = [0u8; 36];
    ssid_payload[..4].copy_from_slice(&(ssid_len as u32).to_le_bytes());
    ssid_payload[4..4 + ssid_len].copy_from_slice(&credentials.ssid[..ssid_len]);
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_frame(
        &mut frame,
        CYW43_WLC_SET_SSID,
        CYW43_BCDC_FLAG_SET,
        id,
        &ssid_payload,
    )?;
    cyw43_submit_control_exchange_checked(contract, &frame[..len], CYW43_WLC_SET_SSID, id, stage)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_join_request(
    contract: DriverTaskContract,
    credentials: crate::net::WifiCredentials,
) -> Result<(), DriverTaskNetError> {
    match cyw43_submit_linux_bsscfg_join(contract, credentials) {
        Ok(()) => {
            emit_cyw43_join_request_trace(
                contract,
                "primary-bsscfg:join",
                "ready",
                usize::from(credentials.ssid_len),
                0,
            );
            return Ok(());
        }
        Err(Cyw43CommandSubmitError::Completion(completion))
            if cyw43_join_iovar_completion_allows_set_ssid(completion) =>
        {
            emit_cyw43_join_request_trace(
                contract,
                "primary-bsscfg:join",
                "fallback-set-ssid",
                usize::from(credentials.ssid_len),
                completion.result,
            );
        }
        Err(err) => {
            let result = match &err {
                Cyw43CommandSubmitError::Completion(completion) => completion.result,
                Cyw43CommandSubmitError::Runtime(_) => 0,
            };
            emit_cyw43_join_request_trace(
                contract,
                "primary-bsscfg:join",
                "fail-no-fallback",
                usize::from(credentials.ssid_len),
                result,
            );
            return Err(err.into_net_error());
        }
    }

    cyw43_submit_bcdc_ssid(contract, credentials, "cyw43-control-ssid")?;
    emit_cyw43_join_request_trace(
        contract,
        "set-ssid",
        "ready",
        usize::from(credentials.ssid_len),
        0,
    );
    Ok(())
}

#[cfg(feature = "kernel")]
fn cyw43_submit_linux_bsscfg_join(
    contract: DriverTaskContract,
    credentials: crate::net::WifiCredentials,
) -> Result<(), Cyw43CommandSubmitError> {
    let mut join_payload = [0u8; CYW43_LINUX_BSSCFG_JOIN_PAYLOAD_LEN];
    cyw43_write_linux_bsscfg_join_payload(&mut join_payload, credentials)
        .map_err(|err| Cyw43CommandSubmitError::Runtime(DriverTaskNetError::RuntimeInit(err)))?;
    cyw43_submit_bcdc_iovar_bytes_unmapped_with_header_mode(
        contract,
        "join",
        &join_payload,
        "cyw43-join-bsscfg",
        Cyw43ControlHeaderMode::Extended,
    )
    .map(|_| ())
}

fn cyw43_write_linux_bsscfg_join_payload(
    payload: &mut [u8],
    credentials: crate::net::WifiCredentials,
) -> Result<(), &'static str> {
    if payload.len() != CYW43_LINUX_BSSCFG_JOIN_PAYLOAD_LEN {
        return Err("cyw43-join-payload-len");
    }
    let ssid_len = usize::from(credentials.ssid_len);
    if ssid_len > credentials.ssid.len() {
        return Err("wifi-ssid-too-long");
    }

    payload.fill(0);
    payload[CYW43_LINUX_EXT_JOIN_SSID_OFFSET..CYW43_LINUX_EXT_JOIN_SSID_OFFSET + 4]
        .copy_from_slice(&(ssid_len as u32).to_le_bytes());
    payload[CYW43_LINUX_EXT_JOIN_SSID_OFFSET + 4..CYW43_LINUX_EXT_JOIN_SSID_OFFSET + 4 + ssid_len]
        .copy_from_slice(&credentials.ssid[..ssid_len]);
    payload[CYW43_LINUX_EXT_JOIN_SCAN_OFFSET] = 0xff;
    payload[CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 4..CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 8]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    payload[CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 8..CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 12]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    payload[CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 12..CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 16]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    payload[CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 16..CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 20]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    payload[CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET..CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET + 6]
        .copy_from_slice(&[0xff; 6]);
    payload[CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET + 8..CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET + 12]
        .copy_from_slice(&0u32.to_le_bytes());
    Ok(())
}

#[cfg(feature = "kernel")]
const fn cyw43_join_iovar_completion_allows_set_ssid(
    completion: DriverTaskCompletionRecord,
) -> bool {
    completion.code == DriverTaskCompletionCode::Fault.as_u16()
        && completion.detail == CYW43_CONTROL_EXCHANGE_FAULT_DETAIL
        && (completion.result == CYW43_BCME_UNSUPPORTED_STATUS
            || completion.result == CYW43_BCME_BADARG_STATUS)
}

#[cfg(feature = "kernel")]
fn emit_cyw43_join_request_trace(
    contract: DriverTaskContract,
    path: &'static str,
    action: &'static str,
    ssid_len: usize,
    result: u32,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_JOIN_REQUEST contract={} path={} action={} ssid_len={} result=0x{:08x}",
        contract.name, path, action, ssid_len, result
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_bytes(
    contract: DriverTaskContract,
    name: &str,
    data: &[u8],
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    cyw43_submit_bcdc_iovar_bytes_with_header_mode(
        contract,
        name,
        data,
        stage,
        Cyw43ControlHeaderMode::Extended,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_bytes_with_header_mode(
    contract: DriverTaskContract,
    name: &str,
    data: &[u8],
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
) -> Result<(), DriverTaskNetError> {
    let name_len = name.len();
    let payload_len = name_len
        .checked_add(1)
        .and_then(|len| len.checked_add(data.len()))
        .ok_or(DriverTaskNetError::RuntimeInit("cyw43-iovar-len"))?;
    if payload_len > MAX_DRIVER_TASK_FRAME_BYTES.saturating_sub(CYW43_BCDC_HEADER_BYTES) {
        return Err(DriverTaskNetError::RuntimeInit("cyw43-iovar-len"));
    }
    let mut payload = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    payload[..name_len].copy_from_slice(name.as_bytes());
    payload[name_len] = 0;
    payload[name_len + 1..payload_len].copy_from_slice(data);
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_frame(
        &mut frame,
        CYW43_WLC_SET_VAR,
        CYW43_BCDC_FLAG_SET,
        id,
        &payload[..payload_len],
    )?;
    cyw43_submit_control_exchange_checked_with_header_mode(
        contract,
        &frame[..len],
        CYW43_WLC_SET_VAR,
        id,
        stage,
        header_mode,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_u32(
    contract: DriverTaskContract,
    name: &str,
    value: u32,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    cyw43_submit_bcdc_iovar_bytes(contract, name, &value.to_le_bytes(), stage)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_u32_with_header_mode(
    contract: DriverTaskContract,
    name: &str,
    value: u32,
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
) -> Result<(), DriverTaskNetError> {
    cyw43_submit_bcdc_iovar_bytes_with_header_mode(
        contract,
        name,
        &value.to_le_bytes(),
        stage,
        header_mode,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_bytes_unmapped_with_header_mode(
    contract: DriverTaskContract,
    name: &str,
    data: &[u8],
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
    let name_len = name.len();
    let payload_len = name_len
        .checked_add(1)
        .and_then(|len| len.checked_add(data.len()))
        .ok_or(Cyw43CommandSubmitError::Runtime(
            DriverTaskNetError::RuntimeInit("cyw43-iovar-len"),
        ))?;
    if payload_len > MAX_DRIVER_TASK_FRAME_BYTES.saturating_sub(CYW43_BCDC_HEADER_BYTES) {
        return Err(Cyw43CommandSubmitError::Runtime(
            DriverTaskNetError::RuntimeInit("cyw43-iovar-len"),
        ));
    }
    let mut payload = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    payload[..name_len].copy_from_slice(name.as_bytes());
    payload[name_len] = 0;
    payload[name_len + 1..payload_len].copy_from_slice(data);
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_frame(
        &mut frame,
        CYW43_WLC_SET_VAR,
        CYW43_BCDC_FLAG_SET,
        id,
        &payload[..payload_len],
    )
    .map_err(Cyw43CommandSubmitError::Runtime)?;
    cyw43_submit_control_exchange_unmapped_with_header_mode(
        contract,
        &frame[..len],
        CYW43_WLC_SET_VAR,
        id,
        stage,
        header_mode,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_enable_join_event_messages(
    contract: DriverTaskContract,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    let mask = cyw43_linux_join_event_mask()?;
    if cyw43_try_set_event_msgs_ext_mask(contract, stage, &mask)? {
        emit_cyw43_event_mask_trace(contract, stage, "event_msgs_ext", "ready");
        return Ok(());
    }

    emit_cyw43_event_mask_trace(contract, stage, "event_msgs_ext", "unsupported");
    let mut current = [0u8; CYW43_EVENT_MASK_LEN];
    let response_len = cyw43_get_bcdc_iovar(contract, "event_msgs", &mut current, stage)?;
    for (slot, required) in current.iter_mut().zip(mask.iter()) {
        *slot |= *required;
    }
    let set_len = response_len.max(CYW43_EVENT_MASK_LEN).min(current.len());
    cyw43_submit_bcdc_iovar_bytes(contract, "event_msgs", &current[..set_len], stage)?;
    emit_cyw43_event_mask_trace(contract, stage, "event_msgs", "ready");
    Ok(())
}

#[cfg(feature = "kernel")]
fn cyw43_try_set_event_msgs_ext_mask(
    contract: DriverTaskContract,
    stage: &'static str,
    mask: &[u8; CYW43_EVENT_MASK_LEN],
) -> Result<bool, DriverTaskNetError> {
    let mut payload = [0u8; CYW43_EVENTMSGS_EXT_PAYLOAD_LEN];
    cyw43_write_event_msgs_ext_payload(&mut payload, mask)?;
    match cyw43_submit_bcdc_iovar_bytes_unmapped_with_header_mode(
        contract,
        "event_msgs_ext",
        &payload,
        stage,
        Cyw43ControlHeaderMode::Extended,
    ) {
        Ok(completion) if completion.code == DriverTaskCompletionCode::FrameReady.as_u16() => {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "ready",
                Some(completion),
            );
            Ok(true)
        }
        Ok(completion) => {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "fail",
                Some(completion),
            );
            Err(DriverTaskNetError::RuntimeInit(stage))
        }
        Err(Cyw43CommandSubmitError::Completion(completion))
            if cyw43_control_exchange_completion_is_unsupported(completion) =>
        {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "unsupported",
                Some(completion),
            );
            Ok(false)
        }
        Err(err) => Err(err.into_net_error()),
    }
}

#[cfg(feature = "kernel")]
fn emit_cyw43_event_mask_trace(
    contract: DriverTaskContract,
    stage: &'static str,
    path: &'static str,
    action: &'static str,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_EVENT_MASK contract={} stage={} path={} action={} len={}",
        contract.name, stage, path, action, CYW43_EVENT_MASK_LEN
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

fn cyw43_linux_join_event_mask() -> Result<[u8; CYW43_EVENT_MASK_LEN], DriverTaskNetError> {
    let mut mask = CYW43_LINUX_EVENTMSGS_EXT_MASK;
    for event in CYW43_JOIN_COMPLETION_EVENTS {
        cyw43_set_event_mask_bit(&mut mask, event)?;
    }
    Ok(mask)
}

fn cyw43_write_event_msgs_ext_payload(
    payload: &mut [u8],
    mask: &[u8; CYW43_EVENT_MASK_LEN],
) -> Result<(), DriverTaskNetError> {
    if payload.len() != CYW43_EVENTMSGS_EXT_PAYLOAD_LEN {
        return Err(DriverTaskNetError::RuntimeInit(
            "event-msgs-ext-payload-len",
        ));
    }
    payload.fill(0);
    payload[0] = CYW43_EVENTMSGS_EXT_VER;
    payload[1] = CYW43_EVENTMSGS_EXT_SET_MASK;
    payload[2] = CYW43_EVENT_MASK_LEN as u8;
    payload[3] = CYW43_EVENTMSGS_EXT_MAX_GET_SIZE;
    payload[CYW43_EVENTMSGS_EXT_HEADER_LEN..].copy_from_slice(mask);
    Ok(())
}

fn cyw43_set_event_mask_bit(mask: &mut [u8], event: u8) -> Result<(), DriverTaskNetError> {
    let index = usize::from(event / 8);
    let bit = event % 8;
    let Some(slot) = mask.get_mut(index) else {
        return Err(DriverTaskNetError::RuntimeInit("wifi-event-mask-too-short"));
    };
    *slot |= 1 << bit;
    Ok(())
}

#[cfg(feature = "kernel")]
fn cyw43_get_bcdc_iovar(
    contract: DriverTaskContract,
    name: &str,
    response: &mut [u8],
    stage: &'static str,
) -> Result<usize, DriverTaskNetError> {
    let name_len = name.len();
    let payload_len = name_len
        .checked_add(1)
        .and_then(|len| len.checked_add(response.len()))
        .ok_or(DriverTaskNetError::RuntimeInit("cyw43-iovar-len"))?;
    if payload_len > MAX_DRIVER_TASK_FRAME_BYTES.saturating_sub(CYW43_BCDC_HEADER_BYTES) {
        return Err(DriverTaskNetError::RuntimeInit("cyw43-iovar-len"));
    }
    let mut payload = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    payload[..name_len].copy_from_slice(name.as_bytes());
    payload[name_len] = 0;
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_frame(
        &mut frame,
        CYW43_WLC_GET_VAR,
        CYW43_BCDC_FLAG_GET,
        id,
        &payload[..payload_len],
    )?;
    let completion = cyw43_submit_control_exchange_completion(
        contract,
        &frame[..len],
        CYW43_WLC_GET_VAR,
        id,
        stage,
    )?;
    let Some(bytes) =
        crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)
    else {
        return Err(DriverTaskNetError::RuntimeInit("cyw43-iovar-response"));
    };
    let copy_len = bytes.len().min(response.len());
    response[..copy_len].copy_from_slice(&bytes[..copy_len]);
    Ok(copy_len)
}

#[cfg(feature = "kernel")]
fn cyw43_query_runtime_mac(
    contract: DriverTaskContract,
) -> Result<EthernetAddress, DriverTaskNetError> {
    let mut mac = [0u8; 6];
    let len = cyw43_get_bcdc_iovar(contract, "cur_etheraddr", &mut mac, "cyw43-control-mac")?;
    if len != mac.len() || mac.iter().all(|byte| *byte == 0) {
        return Err(DriverTaskNetError::RuntimeInit("cyw43-control-mac"));
    }
    Ok(EthernetAddress(mac))
}

#[cfg(feature = "kernel")]
fn cyw43_prepare_runtime_control_plane(
    contract: DriverTaskContract,
) -> Result<EthernetAddress, DriverTaskNetError> {
    cyw43_submit_bcdc_iovar_u32_with_header_mode(
        contract,
        "bus:txglomalign",
        8,
        "cyw43-control-txglomalign",
        Cyw43ControlHeaderMode::Plain,
    )?;
    cyw43_get_bcdc_iovar_optional_unsupported_with_header_mode(
        contract,
        "ulp_sdioctrl",
        "cyw43-control-ulp-sdioctrl",
        Cyw43ControlHeaderMode::Plain,
    )?;
    cyw43_submit_bcdc_iovar_u32_with_header_mode(
        contract,
        "bus:rxglom",
        1,
        "cyw43-control-rxglom",
        Cyw43ControlHeaderMode::Plain,
    )?;
    let mac = cyw43_query_runtime_mac(contract)?;
    cyw43_get_bcdc_revinfo(contract)?;
    cyw43_submit_bcdc_iovar_u32(contract, "mpc", 0, "cyw43-control-mpc")?;
    Ok(mac)
}

#[cfg(feature = "kernel")]
fn cyw43_get_bcdc_iovar_optional_unsupported(
    contract: DriverTaskContract,
    name: &str,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    cyw43_get_bcdc_iovar_optional_unsupported_with_header_mode(
        contract,
        name,
        stage,
        Cyw43ControlHeaderMode::Extended,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_get_bcdc_iovar_optional_unsupported_with_header_mode(
    contract: DriverTaskContract,
    name: &str,
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
) -> Result<(), DriverTaskNetError> {
    let name_len = name.len();
    let payload_len = name_len
        .checked_add(1)
        .ok_or(DriverTaskNetError::RuntimeInit("cyw43-iovar-len"))?;
    if payload_len > MAX_DRIVER_TASK_FRAME_BYTES.saturating_sub(CYW43_BCDC_HEADER_BYTES) {
        return Err(DriverTaskNetError::RuntimeInit("cyw43-iovar-len"));
    }
    let mut payload = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    payload[..name_len].copy_from_slice(name.as_bytes());
    payload[name_len] = 0;
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_frame(
        &mut frame,
        CYW43_WLC_GET_VAR,
        CYW43_BCDC_FLAG_GET,
        id,
        &payload[..payload_len],
    )?;
    match cyw43_submit_control_exchange_unmapped_with_header_mode(
        contract,
        &frame[..len],
        CYW43_WLC_GET_VAR,
        id,
        stage,
        header_mode,
    ) {
        Ok(completion) if completion.code == DriverTaskCompletionCode::FrameReady.as_u16() => {
            Ok(())
        }
        Ok(completion) => {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "fail",
                Some(completion),
            );
            Err(DriverTaskNetError::RuntimeInit(stage))
        }
        Err(Cyw43CommandSubmitError::Completion(completion))
            if cyw43_control_exchange_completion_is_unsupported(completion) =>
        {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "unsupported",
                Some(completion),
            );
            Ok(())
        }
        Err(err) => Err(err.into_net_error()),
    }
}

#[cfg(feature = "kernel")]
fn cyw43_get_bcdc_revinfo(contract: DriverTaskContract) -> Result<(), DriverTaskNetError> {
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_revinfo_frame(&mut frame, id)?;
    cyw43_submit_control_exchange_checked(
        contract,
        &frame[..len],
        CYW43_WLC_GET_REVINFO,
        id,
        "cyw43-control-revinfo",
    )
}

#[cfg(feature = "kernel")]
fn cyw43_get_bcdc_bssid(
    contract: DriverTaskContract,
) -> Result<EthernetAddress, DriverTaskNetError> {
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let response = [0u8; ETHER_ADDR_LEN];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_frame(
        &mut frame,
        CYW43_WLC_GET_BSSID,
        CYW43_BCDC_FLAG_GET,
        id,
        &response,
    )?;
    let completion = cyw43_submit_control_exchange_completion(
        contract,
        &frame[..len],
        CYW43_WLC_GET_BSSID,
        id,
        "cyw43-host-eapol-bssid-probe",
    )?;
    let Some(bytes) =
        crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)
    else {
        return Err(DriverTaskNetError::RuntimeInit(
            "cyw43-host-eapol-bssid-probe",
        ));
    };
    if bytes.len() < ETHER_ADDR_LEN {
        return Err(DriverTaskNetError::RuntimeInit(
            "cyw43-host-eapol-bssid-short",
        ));
    }
    let mut bssid = [0u8; ETHER_ADDR_LEN];
    bssid.copy_from_slice(&bytes[..ETHER_ADDR_LEN]);
    Ok(EthernetAddress(bssid))
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy)]
struct Cyw43ControlIovarInfo<'a> {
    name: &'a str,
    data_len: usize,
    value_u32: Option<u32>,
}

#[cfg(feature = "kernel")]
fn cyw43_read_le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let b0 = *bytes.get(offset)?;
    let b1 = *bytes.get(offset + 1)?;
    Some(u16::from_le_bytes([b0, b1]))
}

#[cfg(feature = "kernel")]
fn cyw43_read_le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let b0 = *bytes.get(offset)?;
    let b1 = *bytes.get(offset + 1)?;
    let b2 = *bytes.get(offset + 2)?;
    let b3 = *bytes.get(offset + 3)?;
    Some(u32::from_le_bytes([b0, b1, b2, b3]))
}

#[cfg(feature = "kernel")]
fn cyw43_control_iovar_info(payload: &[u8], cmd: u32) -> Option<Cyw43ControlIovarInfo<'_>> {
    if cmd != CYW43_WLC_GET_VAR && cmd != CYW43_WLC_SET_VAR {
        return None;
    }
    let body = payload.get(CYW43_BCDC_HEADER_BYTES..)?;
    let name_len = body.iter().position(|byte| *byte == 0)?;
    if name_len == 0 {
        return None;
    }
    let name = core::str::from_utf8(body.get(..name_len)?).ok()?;
    let data = body.get(name_len + 1..)?;
    let value_u32 = if data.len() == 4 {
        cyw43_read_le_u32(data, 0)
    } else {
        None
    };
    Some(Cyw43ControlIovarInfo {
        name,
        data_len: data.len(),
        value_u32,
    })
}

#[cfg(feature = "kernel")]
fn cyw43_control_request_expected_response_len(
    cmd: u32,
    info: Option<Cyw43ControlIovarInfo<'_>>,
) -> usize {
    if cmd == CYW43_WLC_GET_REVINFO {
        CYW43_REVINFO_RESPONSE_BYTES
    } else if cmd == CYW43_WLC_GET_BSSID {
        ETHER_ADDR_LEN
    } else if cmd == CYW43_WLC_GET_VAR {
        info.map_or(0, |info| info.data_len)
    } else {
        0
    }
}

#[cfg(feature = "kernel")]
fn cyw43_control_request_digest_len(
    payload_len: usize,
    info: Option<Cyw43ControlIovarInfo<'_>>,
) -> (usize, &'static str) {
    let Some(info) = info else {
        return (payload_len.min(CYW43_BCDC_HEADER_BYTES), "header");
    };
    if info.data_len <= 4 {
        return (payload_len, "full");
    }
    let redacted_len = CYW43_BCDC_HEADER_BYTES
        .saturating_add(info.name.len())
        .saturating_add(1)
        .min(payload_len);
    (redacted_len, "header-iovar")
}

#[cfg(feature = "kernel")]
fn emit_cyw43_control_request_trace(
    contract: DriverTaskContract,
    stage: &'static str,
    cmd: u32,
    id: u16,
    header_mode: Cyw43ControlHeaderMode,
    payload: &[u8],
) {
    use core::fmt::Write;

    let bcdc_flags = cyw43_read_le_u16(payload, 8).unwrap_or(0xffff);
    let info = cyw43_control_iovar_info(payload, cmd);
    let response_len = cyw43_control_request_expected_response_len(cmd, info);
    let (digest_len, digest_scope) = cyw43_control_request_digest_len(payload.len(), info);
    let digest = cyw43_payload_digest(&payload[..digest_len]);
    let iovar = info.map_or("none", |info| info.name);
    let value = info.and_then(|info| info.value_u32);
    let mut line = heapless::String::<768>::new();
    match value {
        Some(value) => {
            let _ = write!(
                line,
                "CYW43_DRIVER_TASK_CONTROL_REQUEST contract={} stage={} cmd={} cmd_hex=0x{:08x} id={} runtime_flags=0x{:04x} bcdc_flags=0x{:04x} payload_len={} response_len={} iovar={} value=0x{:08x} header_mode={} digest_scope={} digest_len={} first=0x{:02x} last=0x{:02x} xor=0x{:02x} sum=0x{:08x}",
                contract.name,
                stage,
                cmd,
                cmd,
                id,
                header_mode.runtime_flags(),
                bcdc_flags,
                payload.len(),
                response_len,
                iovar,
                value,
                header_mode.as_str(),
                digest_scope,
                digest_len,
                digest.first,
                digest.last,
                digest.xor,
                digest.sum,
            );
        }
        None => {
            let _ = write!(
                line,
                "CYW43_DRIVER_TASK_CONTROL_REQUEST contract={} stage={} cmd={} cmd_hex=0x{:08x} id={} runtime_flags=0x{:04x} bcdc_flags=0x{:04x} payload_len={} response_len={} iovar={} value=none header_mode={} digest_scope={} digest_len={} first=0x{:02x} last=0x{:02x} xor=0x{:02x} sum=0x{:08x}",
                contract.name,
                stage,
                cmd,
                cmd,
                id,
                header_mode.runtime_flags(),
                bcdc_flags,
                payload.len(),
                response_len,
                iovar,
                header_mode.as_str(),
                digest_scope,
                digest_len,
                digest.first,
                digest.last,
                digest.xor,
                digest.sum,
            );
        }
    }
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn cyw43_write_bcdc_revinfo_frame(
    frame: &mut [u8; MAX_DRIVER_TASK_FRAME_BYTES],
    id: u16,
) -> Result<usize, DriverTaskNetError> {
    let response = [0u8; CYW43_REVINFO_RESPONSE_BYTES];
    cyw43_write_bcdc_frame(
        frame,
        CYW43_WLC_GET_REVINFO,
        CYW43_BCDC_FLAG_GET,
        id,
        &response,
    )
}

#[cfg(feature = "kernel")]
const fn cyw43_control_exchange_completion_is_unsupported(
    completion: DriverTaskCompletionRecord,
) -> bool {
    completion.code == DriverTaskCompletionCode::Fault.as_u16()
        && completion.detail == CYW43_CONTROL_EXCHANGE_FAULT_DETAIL
        && completion.result == CYW43_BCME_UNSUPPORTED_STATUS
}

#[cfg(feature = "kernel")]
fn cyw43_configure_host_eapol_rx(contract: DriverTaskContract) -> Result<(), DriverTaskNetError> {
    cyw43_configure_host_eapol_rx_mode(
        contract,
        0,
        0,
        "cyw43-host-eapol-mcast",
        "cyw43-host-eapol-allmulti",
        "cyw43-host-eapol-promisc",
    )
}

#[cfg(feature = "kernel")]
fn cyw43_refresh_host_eapol_rx_after_assoc(
    contract: DriverTaskContract,
) -> Result<(), DriverTaskNetError> {
    cyw43_configure_host_eapol_rx_mode(
        contract,
        0,
        0,
        "cyw43-host-eapol-refresh-mcast",
        "cyw43-host-eapol-refresh-allmulti",
        "cyw43-host-eapol-refresh-promisc",
    )
}

#[cfg(feature = "kernel")]
fn cyw43_rescue_host_eapol_rx_after_assoc(
    contract: DriverTaskContract,
) -> Result<(), DriverTaskNetError> {
    cyw43_configure_host_eapol_rx_mode(
        contract,
        1,
        1,
        "cyw43-host-eapol-rescue-mcast",
        "cyw43-host-eapol-rescue-allmulti",
        "cyw43-host-eapol-rescue-promisc",
    )
}

#[cfg(feature = "kernel")]
fn cyw43_configure_host_eapol_rx_mode(
    contract: DriverTaskContract,
    allmulti: u32,
    promisc: u32,
    mcast_stage: &'static str,
    allmulti_stage: &'static str,
    promisc_stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    let mut mcast = [0u8; 10];
    mcast[..4].copy_from_slice(&1u32.to_le_bytes());
    mcast[4..10].copy_from_slice(&CYW43_PAE_GROUP_ADDR);
    cyw43_submit_bcdc_iovar_bytes(contract, "mcast_list", &mcast, mcast_stage)?;
    cyw43_submit_bcdc_iovar_u32(contract, "allmulti", allmulti, allmulti_stage)?;
    cyw43_submit_bcdc_u32(contract, CYW43_WLC_SET_PROMISC, promisc, promisc_stage)
}

#[cfg(feature = "kernel")]
fn cyw43_install_wsec_key(
    contract: DriverTaskContract,
    index: u32,
    key: &[u8],
    ea: &[u8; 6],
    rsc: Option<&[u8]>,
    primary: bool,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    let mut payload = [0u8; WSEC_KEY_PAYLOAD_LEN];
    let len = cyw43_host_eapol::write_wsec_key_payload(&mut payload, index, key, ea, rsc, primary)
        .map_err(DriverTaskNetError::RuntimeInit)?;
    cyw43_submit_bcdc_iovar_bytes(contract, "wsec_key", &payload[..len], stage)
}

#[cfg(feature = "kernel")]
fn arm_cyw43_host_eapol_pending(
    contract: DriverTaskContract,
    credentials: WifiCredentials,
    _station_mac: EthernetAddress,
) -> Result<(), DriverTaskNetError> {
    let mut session = Cyw43HostEapolSession::new(credentials)?;
    cyw43_apply_pending_host_eapol_event(contract, &mut session, 0);
    let progress = session.progress;
    *CYW43_HOST_EAPOL_SESSION.lock() = Some(session);
    CYW43_CONTROL_PLANE_READY.store(1, Ordering::Release);
    CYW43_ASSOCIATED.store(if progress.associated { 1 } else { 0 }, Ordering::Release);
    CYW43_LINK_UP.store(if progress.link_up { 1 } else { 0 }, Ordering::Release);
    CYW43_HOST_EAPOL_ACTIVE.store(1, Ordering::Release);
    CYW43_HOST_EAPOL_REQUIRED.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        "cyw43-host-eapol",
        "pending",
        None,
    );
    emit_cyw43_host_eapol_status(contract, "pending", &progress);
    Ok(())
}

#[cfg(feature = "kernel")]
pub(crate) fn service_cyw43_host_eapol_slice(
    credentials: WifiCredentials,
    poll_limit: usize,
) -> bool {
    if poll_limit == 0
        || CYW43_HOST_EAPOL_ACTIVE.load(Ordering::Acquire) == 0
        || CYW43_HOST_EAPOL_REQUIRED.load(Ordering::Acquire) != 0
        || CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) != 0
        || !runtime_ready(DriverTaskHotPath::Cyw43Wifi)
    {
        return false;
    }

    let contract = CYW43_WIFI_DRIVER_TASK_CONTRACT;
    let station_mac = *CYW43_RUNTIME_MAC.lock();
    let mut guard = CYW43_HOST_EAPOL_SESSION.lock();
    if guard.is_none() {
        let Ok(session) = Cyw43HostEapolSession::new(credentials) else {
            CYW43_HOST_EAPOL_ACTIVE.store(0, Ordering::Release);
            CYW43_HOST_EAPOL_REQUIRED.store(1, Ordering::Release);
            return false;
        };
        *guard = Some(session);
    }
    let Some(session) = guard.as_mut() else {
        return false;
    };
    let poll = session.progress.polls as usize;
    cyw43_apply_pending_host_eapol_event(contract, session, poll);

    let mut activity = false;
    let mut clear_session = false;
    for _ in 0..poll_limit {
        if session.progress.polls >= CYW43_HOST_EAPOL_JOIN_POLLS as u32 {
            mark_cyw43_host_eapol_required(contract, &session.progress);
            clear_session = true;
            activity = true;
            break;
        }
        match poll_cyw43_host_eapol_once(contract, station_mac, session) {
            Ok(Cyw43HostEapolStep::Pending { activity: polled }) => {
                activity |= polled;
            }
            Ok(Cyw43HostEapolStep::Secure) => {
                mark_cyw43_host_eapol_secure(contract, &session.progress);
                clear_session = true;
                activity = true;
                break;
            }
            Err(_) => {
                mark_cyw43_host_eapol_required(contract, &session.progress);
                clear_session = true;
                activity = true;
                break;
            }
        }
    }
    if clear_session {
        *guard = None;
    }
    activity
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cyw43HostEapolStep {
    Pending { activity: bool },
    Secure,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cyw43HostEapolPollKind {
    Control,
    Data,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43HostEapolActivePoll {
    kind: Cyw43HostEapolPollKind,
    flags: u16,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Cyw43HostEapolPollResult {
    completed: bool,
    observed_frame: bool,
    activity: bool,
    secure: bool,
}

#[cfg(feature = "kernel")]
fn poll_cyw43_host_eapol_once(
    contract: DriverTaskContract,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
) -> Result<Cyw43HostEapolStep, DriverTaskNetError> {
    let poll = session.progress.polls as usize;
    let mut tx_frame = [0u8; MAX_FRAME_LEN];
    let rx_poll_flags = if cyw43_host_eapol_rx_firstread_due(
        poll,
        CYW43_HOST_EAPOL_START.load(Ordering::Acquire),
    ) {
        DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD
    } else {
        0
    };

    if let Some(active_poll) = cyw43_active_prompt_poll(contract) {
        let mut result = poll_cyw43_host_eapol_kind(
            contract,
            station_mac,
            session,
            poll,
            active_poll.flags,
            active_poll.kind,
            &mut tx_frame,
        )?;
        record_cyw43_host_eapol_poll_completion(session, result);
        if result.secure {
            return Ok(Cyw43HostEapolStep::Secure);
        }
        result.activity |=
            cyw43_service_host_eapol_maintenance_if_idle(contract, station_mac, session);
        return Ok(Cyw43HostEapolStep::Pending {
            activity: result.activity,
        });
    }

    let mut result = poll_cyw43_host_eapol_kind(
        contract,
        station_mac,
        session,
        poll,
        rx_poll_flags,
        Cyw43HostEapolPollKind::Control,
        &mut tx_frame,
    )?;
    if result.secure {
        return Ok(Cyw43HostEapolStep::Secure);
    }
    if cyw43_active_prompt_poll(contract).is_none() {
        let data_result = poll_cyw43_host_eapol_kind(
            contract,
            station_mac,
            session,
            poll,
            rx_poll_flags,
            Cyw43HostEapolPollKind::Data,
            &mut tx_frame,
        )?;
        if data_result.secure {
            return Ok(Cyw43HostEapolStep::Secure);
        }
        result.observed_frame |= data_result.observed_frame;
        result.activity |= data_result.activity;
        result.completed |= data_result.completed;
    }

    result.activity |= cyw43_service_host_eapol_maintenance_if_idle(contract, station_mac, session);
    record_cyw43_host_eapol_poll_completion(session, result);
    Ok(Cyw43HostEapolStep::Pending {
        activity: result.activity,
    })
}

#[cfg(feature = "kernel")]
fn record_cyw43_host_eapol_poll_completion(
    session: &mut Cyw43HostEapolSession,
    result: Cyw43HostEapolPollResult,
) {
    if !result.completed {
        return;
    }
    session.progress.polls = session.progress.polls.saturating_add(1);
    if !result.observed_frame {
        session.progress.record_empty_poll();
    }
}

#[cfg(feature = "kernel")]
fn poll_cyw43_host_eapol_kind(
    contract: DriverTaskContract,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
    poll: usize,
    rx_poll_flags: u16,
    kind: Cyw43HostEapolPollKind,
    tx_frame: &mut [u8; MAX_FRAME_LEN],
) -> Result<Cyw43HostEapolPollResult, DriverTaskNetError> {
    let completion = match kind {
        Cyw43HostEapolPollKind::Control => poll_cyw43_driver_task_control_completion(rx_poll_flags),
        Cyw43HostEapolPollKind::Data => poll_cyw43_driver_task_data_completion(rx_poll_flags),
    };
    let Some(completion) = completion else {
        return Ok(Cyw43HostEapolPollResult::default());
    };
    match kind {
        Cyw43HostEapolPollKind::Control => Ok(process_cyw43_host_eapol_control_completion(
            contract,
            session,
            poll,
            rx_poll_flags,
            completion,
        )),
        Cyw43HostEapolPollKind::Data => process_cyw43_host_eapol_data_completion(
            contract,
            station_mac,
            session,
            poll,
            rx_poll_flags,
            completion,
            tx_frame,
        ),
    }
}

#[cfg(feature = "kernel")]
fn process_cyw43_host_eapol_control_completion(
    contract: DriverTaskContract,
    session: &mut Cyw43HostEapolSession,
    poll: usize,
    rx_poll_flags: u16,
    completion: DriverTaskCompletionRecord,
) -> Cyw43HostEapolPollResult {
    let mut result = Cyw43HostEapolPollResult::default();
    result.completed = true;
    if let Some((flags, token)) = cyw43_driver_task_frame_from_completion(contract, completion) {
        result.observed_frame = true;
        result.activity = true;
        let frame = &token.buffer[..token.len];
        if cyw43_frame_channel(flags) == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT {
            if let Some(event) = cyw43_parse_control_or_event_frame(frame) {
                session
                    .progress
                    .record_event_frame(flags, frame.len(), event, poll);
                if session.progress.event_rx == 1 || session.progress.association_event.is_some() {
                    emit_cyw43_host_eapol_status(contract, "event-rx", &session.progress);
                }
            } else {
                session.progress.record_control_frame(flags, frame.len());
            }
        } else {
            session.progress.record_control_frame(flags, frame.len());
        }
    } else if completion.code == DriverTaskCompletionCode::Idle.as_u16()
        && rx_poll_flags & DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD != 0
    {
        session
            .progress
            .record_control_rx_idle_completion(completion);
    }
    result
}

#[cfg(feature = "kernel")]
fn process_cyw43_host_eapol_data_completion(
    contract: DriverTaskContract,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
    poll: usize,
    rx_poll_flags: u16,
    completion: DriverTaskCompletionRecord,
    tx_frame: &mut [u8; MAX_FRAME_LEN],
) -> Result<Cyw43HostEapolPollResult, DriverTaskNetError> {
    let mut result = Cyw43HostEapolPollResult::default();
    result.completed = true;
    if let Some((flags, token)) = cyw43_driver_task_frame_from_completion(contract, completion) {
        result.observed_frame = true;
        result.activity = true;
        let frame = &token.buffer[..token.len];
        let data_event =
            if cyw43_frame_channel(flags) == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA {
                cyw43_parse_data_event_frame(frame)
            } else {
                None
            };
        if let Some(event) = data_event {
            session
                .progress
                .record_event_frame(flags, frame.len(), event, poll);
            emit_cyw43_host_eapol_status(contract, "event-rx", &session.progress);
        } else {
            let ethertype = cyw43_ethertype(frame);
            session
                .progress
                .record_data_frame(flags, frame.len(), ethertype);
            if session.progress.data_rx == 1 || session.progress.eapol_rx == 1 {
                emit_cyw43_host_eapol_status(contract, "rx-observed", &session.progress);
            }
            if ethertype == Some(ETH_P_EAPOL) {
                let action = session
                    .eapol
                    .handle_packet(station_mac.0, frame, tx_frame)
                    .map_err(DriverTaskNetError::RuntimeInit)?;
                CYW43_HOST_EAPOL_RX.store(session.eapol.rx_packets(), Ordering::Release);
                emit_cyw43_host_eapol_status(contract, "eapol-rx", &session.progress);
                match action {
                    HostEapolAction::None => {}
                    HostEapolAction::SendM2 { len } => {
                        if !submit_cyw43_host_eapol_payload_bounded(
                            contract,
                            &tx_frame[..len],
                            "cyw43-host-eapol-m2",
                        ) {
                            return Err(DriverTaskNetError::RuntimeInit("host-eapol-m2-tx"));
                        }
                        result.activity = true;
                    }
                    HostEapolAction::SendM4InstallKeys { len, keys } => {
                        if !submit_cyw43_host_eapol_payload_bounded(
                            contract,
                            &tx_frame[..len],
                            "cyw43-host-eapol-m4",
                        ) {
                            return Err(DriverTaskNetError::RuntimeInit("host-eapol-m4-tx"));
                        }
                        let pairwise_rsc = [0u8; 6];
                        cyw43_install_wsec_key(
                            contract,
                            0,
                            &keys.pairwise_tk,
                            &keys.ap_mac,
                            Some(&pairwise_rsc),
                            false,
                            "cyw43-host-eapol-ptk",
                        )?;
                        let group_ea = [0u8; 6];
                        cyw43_install_wsec_key(
                            contract,
                            u32::from(keys.gtk.index),
                            &keys.gtk.key[..keys.gtk.key_len],
                            &group_ea,
                            Some(&keys.rsc),
                            true,
                            "cyw43-host-eapol-gtk",
                        )?;
                        cyw43_submit_bcdc_u32(
                            contract,
                            CYW43_WLC_SET_WSEC,
                            CYW43_WSEC_AES,
                            "cyw43-host-eapol-reassert-wsec",
                        )?;
                        result.secure = true;
                    }
                }
            }
        }
    } else if completion.code == DriverTaskCompletionCode::Idle.as_u16()
        && rx_poll_flags & DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD != 0
    {
        session.progress.record_rx_idle_completion(completion);
    }
    Ok(result)
}

#[cfg(feature = "kernel")]
fn cyw43_service_host_eapol_maintenance_if_idle(
    contract: DriverTaskContract,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
) -> bool {
    let mut activity = false;
    if cyw43_active_prompt_poll(contract).is_none() {
        activity |= cyw43_service_host_eapol_assoc_probe(contract, station_mac, session);
    }
    if cyw43_active_prompt_poll(contract).is_none() {
        activity |= cyw43_service_host_eapol_post_assoc(contract, station_mac, session);
    }
    activity
}

#[cfg(feature = "kernel")]
fn cyw43_service_host_eapol_assoc_probe(
    contract: DriverTaskContract,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
) -> bool {
    if !cyw43_host_eapol_assoc_probe_due(&session.progress, session.probed_assoc_bssid) {
        return false;
    }
    session.probed_assoc_bssid = true;
    let poll = session.progress.polls as usize;
    match cyw43_get_bcdc_bssid(contract) {
        Ok(bssid) => {
            let accepted = cyw43_apply_host_eapol_bssid_probe(session, station_mac, bssid, poll);
            emit_cyw43_host_eapol_assoc_probe(
                contract,
                poll,
                if accepted { "associated" } else { "ignored" },
                bssid,
                if accepted {
                    "valid-bssid"
                } else {
                    "not-ap-candidate"
                },
            );
            if accepted {
                CYW43_ASSOCIATED.store(1, Ordering::Release);
                emit_cyw43_host_eapol_status(contract, "assoc-probe", &session.progress);
            }
            accepted
        }
        Err(_) => {
            emit_cyw43_host_eapol_assoc_probe(
                contract,
                poll,
                "failed",
                EthernetAddress([0; ETHER_ADDR_LEN]),
                "control-error",
            );
            false
        }
    }
}

#[cfg(feature = "kernel")]
fn cyw43_apply_host_eapol_bssid_probe(
    session: &mut Cyw43HostEapolSession,
    station_mac: EthernetAddress,
    bssid: EthernetAddress,
    poll: usize,
) -> bool {
    if !cyw43_host_eapol_bssid_candidate(bssid, station_mac) {
        return false;
    }
    session.progress.associated = true;
    if session.progress.association_event.is_none() {
        session.progress.association_event = Some("bssid-probe");
        session.progress.association_poll = (poll as u32).saturating_add(1);
    }
    true
}

#[cfg(feature = "kernel")]
fn cyw43_service_host_eapol_post_assoc(
    contract: DriverTaskContract,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
) -> bool {
    if !session.progress.associated {
        return false;
    }
    let mut activity = false;
    session.progress.post_assoc_polls = session.progress.post_assoc_polls.saturating_add(1);
    if cyw43_host_eapol_post_assoc_refresh_due(&session.progress, session.refreshed_after_assoc) {
        let _ = cyw43_refresh_host_eapol_rx_after_assoc(contract);
        session.refreshed_after_assoc = true;
        activity = true;
        emit_cyw43_host_eapol_status(contract, "rx-admission-refresh", &session.progress);
    }
    if cyw43_host_eapol_post_assoc_rescue_due(&session.progress, session.rescued_after_assoc) {
        let _ = cyw43_rescue_host_eapol_rx_after_assoc(contract);
        session.rescued_after_assoc = true;
        activity = true;
        emit_cyw43_host_eapol_status(contract, "rx-admission-rescue", &session.progress);
    }
    let start_sent = CYW43_HOST_EAPOL_START.load(Ordering::Acquire);
    if cyw43_host_eapol_start_due(session.progress.post_assoc_polls as usize, start_sent) {
        cyw43_try_send_host_eapol_start(
            contract,
            station_mac,
            session.progress.post_assoc_polls as usize,
        );
        activity = CYW43_HOST_EAPOL_START.load(Ordering::Acquire) != start_sent;
    }
    activity
}

#[cfg(feature = "kernel")]
fn mark_cyw43_host_eapol_secure(contract: DriverTaskContract, progress: &Cyw43HostEapolProgress) {
    CYW43_CONTROL_PLANE_READY.store(1, Ordering::Release);
    CYW43_ASSOCIATED.store(1, Ordering::Release);
    CYW43_LINK_UP.store(1, Ordering::Release);
    CYW43_HOST_EAPOL_ACTIVE.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_REQUIRED.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        "cyw43-host-eapol",
        "secure",
        None,
    );
    emit_cyw43_host_eapol_status(contract, "secure", progress);
}

#[cfg(feature = "kernel")]
fn mark_cyw43_host_eapol_required(contract: DriverTaskContract, progress: &Cyw43HostEapolProgress) {
    CYW43_HOST_EAPOL_ACTIVE.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_REQUIRED.store(1, Ordering::Release);
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        "cyw43-host-eapol",
        "required",
        None,
    );
    emit_cyw43_host_eapol_status(contract, "required", progress);
}

#[cfg(feature = "kernel")]
fn cyw43_try_send_host_eapol_start(
    contract: DriverTaskContract,
    station_mac: EthernetAddress,
    poll: usize,
) {
    let mut frame = [0u8; 18];
    let len = match cyw43_host_eapol::write_eapol_start_frame(
        &mut frame,
        &CYW43_PAE_GROUP_ADDR,
        &station_mac.0,
    ) {
        Ok(len) => len,
        Err(_) => {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                "cyw43-host-eapol-start",
                "frame-failed",
                None,
            );
            return;
        }
    };
    if submit_cyw43_host_eapol_payload_bounded(contract, &frame[..len], "cyw43-host-eapol-start") {
        CYW43_HOST_EAPOL_START.fetch_add(1, Ordering::AcqRel);
        emit_cyw43_host_eapol_tx_shape(contract, "cyw43-host-eapol-start", poll, &frame[..len]);
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            "cyw43-host-eapol-start",
            "sent",
            None,
        );
    } else {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            "cyw43-host-eapol-start",
            "tx-failed",
            None,
        );
    }
}

#[must_use]
const fn cyw43_host_eapol_start_due(poll: usize, sent: u32) -> bool {
    sent < CYW43_HOST_EAPOL_START_MAX
        && poll >= CYW43_HOST_EAPOL_START_FIRST_POLL
        && cyw43_host_eapol_start_poll_due(poll)
}

#[must_use]
const fn cyw43_host_eapol_start_poll_due(poll: usize) -> bool {
    poll % CYW43_HOST_EAPOL_START_INTERVAL_POLLS == 0
}

#[cfg(feature = "kernel")]
const fn cyw43_host_eapol_rx_firstread_due(_poll: usize, _starts_sent: u32) -> bool {
    true
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_assoc_probe_due(progress: &Cyw43HostEapolProgress, probed: bool) -> bool {
    !probed
        && !progress.associated
        && progress.event_rx == 0
        && progress.data_rx == 0
        && progress.polls >= CYW43_HOST_EAPOL_ASSOC_PROBE_AFTER_POLLS
}

#[cfg(feature = "kernel")]
const fn cyw43_host_eapol_post_assoc_refresh_due(
    progress: &Cyw43HostEapolProgress,
    refreshed: bool,
) -> bool {
    progress.associated
        && !refreshed
        && progress.eapol_rx == 0
        && progress.post_assoc_polls >= CYW43_HOST_EAPOL_RX_REFRESH_AFTER_POST_ASSOC_POLLS
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_post_assoc_rescue_due(
    progress: &Cyw43HostEapolProgress,
    rescued: bool,
) -> bool {
    progress.associated
        && !rescued
        && progress.eapol_rx == 0
        && (progress.post_assoc_polls >= CYW43_HOST_EAPOL_RX_RESCUE_AFTER_POST_ASSOC_POLLS
            || CYW43_HOST_EAPOL_START.load(Ordering::Acquire)
                >= CYW43_HOST_EAPOL_RX_RESCUE_AFTER_STARTS)
}

#[cfg(feature = "kernel")]
fn cyw43_ethertype(frame: &[u8]) -> Option<u16> {
    if frame.len() < ETH_HEADER_LEN {
        return None;
    }
    Some(u16::from_be_bytes([frame[12], frame[13]]))
}

#[cfg(feature = "kernel")]
const fn cyw43_frame_channel(flags: u16) -> u16 {
    flags & DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_MASK
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_bssid_candidate(bssid: EthernetAddress, station_mac: EthernetAddress) -> bool {
    let mac = bssid.0;
    let station = station_mac.0;
    !(mac[0] == 0 && mac[1] == 0 && mac[2] == 0 && mac[3] == 0 && mac[4] == 0 && mac[5] == 0)
        && mac != station
        && mac != CYW43_PAE_GROUP_ADDR
        && mac[0] & 0x01 == 0
        && mac[0] & 0x02 == 0
}

#[cfg(feature = "kernel")]
const fn cyw43_event_has_link_flag(event: Cyw43EventFrame) -> bool {
    event.flags & CYW43_EVENT_FLAG_LINK != 0
}

#[cfg(feature = "kernel")]
const fn cyw43_host_eapol_post_assoc_event_label(
    event: Cyw43EventFrame,
    associated_after_event: bool,
) -> Option<&'static str> {
    if !associated_after_event {
        return None;
    }
    match event.event_type {
        CYW43_EVENT_ASSOC | CYW43_EVENT_REASSOC if event.status == CYW43_EVENT_STATUS_SUCCESS => {
            Some("assoc")
        }
        CYW43_EVENT_LINK
            if event.status == CYW43_EVENT_STATUS_SUCCESS && cyw43_event_has_link_flag(event) =>
        {
            Some("link-up")
        }
        _ => None,
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_event_link_state_update(event: Cyw43EventFrame) -> Option<bool> {
    match event.event_type {
        CYW43_EVENT_SET_SSID if event.status != CYW43_EVENT_STATUS_SUCCESS => Some(false),
        CYW43_EVENT_LINK
            if event.status == CYW43_EVENT_STATUS_SUCCESS && cyw43_event_has_link_flag(event) =>
        {
            Some(true)
        }
        CYW43_EVENT_LINK => Some(false),
        _ => None,
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_event_association_state_update(event: Cyw43EventFrame) -> Option<bool> {
    match event.event_type {
        CYW43_EVENT_SET_SSID if event.status != CYW43_EVENT_STATUS_SUCCESS => Some(false),
        CYW43_EVENT_ASSOC | CYW43_EVENT_REASSOC if event.status == CYW43_EVENT_STATUS_SUCCESS => {
            Some(true)
        }
        CYW43_EVENT_LINK
            if event.status == CYW43_EVENT_STATUS_SUCCESS && cyw43_event_has_link_flag(event) =>
        {
            Some(true)
        }
        CYW43_EVENT_LINK => Some(false),
        _ => None,
    }
}

#[cfg(feature = "kernel")]
fn cyw43_parse_control_or_event_frame(frame: &[u8]) -> Option<Cyw43EventFrame> {
    let packet = cyw43_bdc_payload(frame)?;
    cyw43_parse_broadcom_event(packet)
}

#[cfg(feature = "kernel")]
fn cyw43_parse_data_event_frame(frame: &[u8]) -> Option<Cyw43EventFrame> {
    cyw43_parse_broadcom_event(frame)
}

#[cfg(feature = "kernel")]
fn cyw43_bdc_payload(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() < CYW43_BDC_HEADER_BYTES {
        return None;
    }
    if payload[0] >> CYW43_BDC_VERSION_SHIFT != CYW43_BDC_VERSION {
        return None;
    }
    let data_offset_words = usize::from(payload[3]);
    let start = CYW43_BDC_HEADER_BYTES.checked_add(data_offset_words.checked_mul(4)?)?;
    payload.get(start..)
}

#[cfg(feature = "kernel")]
fn cyw43_parse_broadcom_event(packet: &[u8]) -> Option<Cyw43EventFrame> {
    if packet.len() < CYW43_BRCMF_EVENT_MIN_PACKET_LEN {
        return None;
    }
    if cyw43_get_u16_be(packet, 12) != Some(CYW43_ETH_P_LINK_CTL)
        || cyw43_get_u16_be(packet, 14) != Some(CYW43_BCMILCP_SUBTYPE_VENDOR_LONG)
        || packet.get(19..22) != Some(&CYW43_BROADCOM_OUI)
        || cyw43_get_u16_be(packet, 22) != Some(CYW43_BCMILCP_BCM_SUBTYPE_EVENT)
    {
        return None;
    }

    let mut src_mac = [0u8; 6];
    src_mac.copy_from_slice(packet.get(6..12)?);
    let mut addr = [0u8; 6];
    addr.copy_from_slice(
        packet.get(CYW43_BRCMF_EVENT_ADDR_OFFSET..CYW43_BRCMF_EVENT_ADDR_OFFSET + 6)?,
    );

    Some(Cyw43EventFrame {
        src_mac,
        addr,
        flags: cyw43_get_u16_be(packet, CYW43_BRCMF_EVENT_FLAGS_OFFSET)?,
        event_type: *packet.get(CYW43_BRCMF_EVENT_TYPE_OFFSET)?,
        status: cyw43_get_u32_be(packet, CYW43_BRCMF_EVENT_STATUS_OFFSET)?,
        reason: cyw43_get_u32_be(packet, CYW43_BRCMF_EVENT_REASON_OFFSET)?,
        auth_type: cyw43_get_u32_be(packet, CYW43_BRCMF_EVENT_AUTH_OFFSET)?,
    })
}

#[cfg(feature = "kernel")]
fn cyw43_get_u16_be(buf: &[u8], offset: usize) -> Option<u16> {
    let bytes = buf.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

#[cfg(feature = "kernel")]
fn cyw43_get_u32_be(buf: &[u8], offset: usize) -> Option<u32> {
    let bytes = buf.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_status(
    contract: DriverTaskContract,
    status: &'static str,
    progress: &Cyw43HostEapolProgress,
) {
    use core::fmt::Write;

    let reason = if status == "required" {
        "host-eapol-required"
    } else {
        "none"
    };
    let next_action = cyw43_host_eapol_next_action(status, progress);
    let assoc_event = progress.association_event.unwrap_or("none");
    let rx_source = progress.last_rx_source.unwrap_or_default();
    let control_rx_source = progress.last_control_rx_source.unwrap_or_default();
    let mut line = heapless::String::<1536>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract={} status={} reason={} polls={} starts={} tx_retries={} data_rx={} eapol_rx={} non_eapol_rx={} event_rx={} control_rx={} empty_polls={} associated={} link_up={} assoc_event={} assoc_poll={} post_assoc_polls={} rx_firstread_attempts={} rx_firstread_empty={} rx_firstread_invalid={} rx_firstread_failed={} rx_firstread_remainder_failed={} rx_firstread_decode_miss={} control_rx_firstread_attempts={} control_rx_firstread_empty={} control_rx_firstread_failed={} last_rx_idle_detail=0x{:04x} last_rx_idle_result=0x{:08x} last_control_rx_idle_detail=0x{:04x} last_control_rx_idle_result=0x{:08x} rxsrc_probe_len={} rxsrc_ien=0x{:02x} rxsrc_frame_ind={} rxsrc_host_int={} rxsrc_card_int={} rxsrc_f2_ready={} control_rxsrc_probe_len={} control_rxsrc_ien=0x{:02x} control_rxsrc_frame_ind={} control_rxsrc_host_int={} control_rxsrc_card_int={} control_rxsrc_f2_ready={} last_flags=0x{:04x} last_len={} last_ethertype=0x{:04x} last_ethertype_valid={} next_action={}",
        contract.name,
        status,
        reason,
        progress.polls,
        CYW43_HOST_EAPOL_START.load(Ordering::Acquire),
        CYW43_HOST_EAPOL_TX_RETRIES.load(Ordering::Acquire),
        progress.data_rx,
        progress.eapol_rx,
        progress.non_eapol_rx,
        progress.event_rx,
        progress.control_rx,
        progress.empty_polls,
        if progress.associated { "yes" } else { "no" },
        if progress.link_up { "yes" } else { "no" },
        assoc_event,
        progress.association_poll,
        progress.post_assoc_polls,
        progress.rx_firstread_attempts,
        progress.rx_firstread_empty,
        progress.rx_firstread_invalid,
        progress.rx_firstread_failed,
        progress.rx_firstread_remainder_failed,
        progress.rx_firstread_decode_miss,
        progress.control_rx_firstread_attempts,
        progress.control_rx_firstread_empty,
        progress.control_rx_firstread_failed,
        progress.last_rx_idle_detail,
        progress.last_rx_idle_result,
        progress.last_control_rx_idle_detail,
        progress.last_control_rx_idle_result,
        rx_source.probe_len,
        rx_source.interrupt_enable,
        yes_no(rx_source.frame_indicated),
        yes_no(rx_source.host_interrupt),
        yes_no(rx_source.card_interrupt),
        yes_no(rx_source.function2_ready),
        control_rx_source.probe_len,
        control_rx_source.interrupt_enable,
        yes_no(control_rx_source.frame_indicated),
        yes_no(control_rx_source.host_interrupt),
        yes_no(control_rx_source.card_interrupt),
        yes_no(control_rx_source.function2_ready),
        progress.last_flags,
        progress.last_len,
        progress.last_ethertype,
        yes_no(progress.last_ethertype_valid),
        next_action,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_event_capture(
    contract: DriverTaskContract,
    stage: &'static str,
    flags: u16,
    len: usize,
    event: Cyw43EventFrame,
    label: &'static str,
    retained: bool,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<384>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_EVENT_RX contract={} stage={} flags=0x{:04x} len={} event_type={} status=0x{:08x} reason=0x{:08x} auth=0x{:08x} label={} retained={}",
        contract.name,
        stage,
        flags,
        len,
        event.event_type,
        event.status,
        event.reason,
        event.auth_type,
        label,
        yes_no(retained),
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_assoc_probe(
    contract: DriverTaskContract,
    poll: usize,
    status: &'static str,
    bssid: EthernetAddress,
    reason: &'static str,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<256>::new();
    let mac = bssid.0;
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_ASSOC_PROBE contract={} poll={} status={} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} reason={}",
        contract.name,
        poll,
        status,
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5],
        reason,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_tx_shape(
    contract: DriverTaskContract,
    stage: &'static str,
    poll: usize,
    frame: &[u8],
) {
    use core::fmt::Write;

    if frame.len() < 14 {
        return;
    }
    let ethertype = cyw43_ethertype(frame).unwrap_or(0);
    let mut line = heapless::String::<256>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_TX contract={} stage={} poll={} len={} dst={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} src={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} ethertype=0x{:04x} bdc_priority=6",
        contract.name,
        stage,
        poll,
        frame.len(),
        frame[0],
        frame[1],
        frame[2],
        frame[3],
        frame[4],
        frame[5],
        frame[6],
        frame[7],
        frame[8],
        frame[9],
        frame[10],
        frame[11],
        ethertype,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_next_action(
    status: &'static str,
    progress: &Cyw43HostEapolProgress,
) -> &'static str {
    if status == "secure" {
        return "release-dhcp-data";
    }
    if progress.eapol_rx != 0 {
        "inspect-host-eapol-handshake-state"
    } else if progress.event_rx != 0 && !progress.associated {
        "inspect-cyw43-join-event-state"
    } else if progress.data_rx != 0 {
        "inspect-eapol-filter-or-ap-m1"
    } else if progress.rx_firstread_invalid != 0 {
        "inspect-cyw43-data-rx-firstread-prefix"
    } else if progress.rx_firstread_failed != 0 || progress.rx_firstread_remainder_failed != 0 {
        "inspect-cyw43-data-rx-cmd53-firstread"
    } else if progress.control_rx_firstread_empty != 0 && !progress.associated {
        "inspect-cyw43-assoc-event-rx-or-ienx"
    } else if progress.rx_firstread_empty != 0 && progress.associated {
        "inspect-ap-m1-or-cyw43-rx-latch"
    } else if progress.rx_firstread_empty != 0 {
        "inspect-association-event-or-cyw43-rx-latch"
    } else {
        "inspect-cyw43-data-rx-path"
    }
}

#[cfg(feature = "kernel")]
fn cyw43_next_bcdc_ioctl_id() -> u16 {
    let next = CYW43_BCDC_IOCTL_ID
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1) as u16;
    if next == 0 {
        1
    } else {
        next
    }
}

#[cfg(feature = "kernel")]
fn cyw43_write_bcdc_frame(
    frame: &mut [u8; MAX_DRIVER_TASK_FRAME_BYTES],
    cmd: u32,
    flags: u16,
    id: u16,
    payload: &[u8],
) -> Result<usize, DriverTaskNetError> {
    let len = CYW43_BCDC_HEADER_BYTES
        .checked_add(payload.len())
        .ok_or(DriverTaskNetError::RuntimeInit("cyw43-control-frame-len"))?;
    if len > frame.len() {
        return Err(DriverTaskNetError::RuntimeInit("cyw43-control-frame-len"));
    }
    frame[0..4].copy_from_slice(&cmd.to_le_bytes());
    frame[4..8].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    frame[8..10].copy_from_slice(&flags.to_le_bytes());
    frame[10..12].copy_from_slice(&id.to_le_bytes());
    frame[12..16].copy_from_slice(&0u32.to_le_bytes());
    frame[CYW43_BCDC_HEADER_BYTES..len].copy_from_slice(payload);
    Ok(len)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_control_exchange_checked(
    contract: DriverTaskContract,
    payload: &[u8],
    cmd: u32,
    id: u16,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    cyw43_submit_control_exchange_checked_with_header_mode(
        contract,
        payload,
        cmd,
        id,
        stage,
        Cyw43ControlHeaderMode::Extended,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_control_exchange_checked_with_header_mode(
    contract: DriverTaskContract,
    payload: &[u8],
    cmd: u32,
    id: u16,
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
) -> Result<(), DriverTaskNetError> {
    let completion = cyw43_submit_control_exchange_completion_with_header_mode(
        contract,
        payload,
        cmd,
        id,
        stage,
        header_mode,
    )?;
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        stage,
        "ready",
        Some(completion),
    );
    Ok(())
}

#[cfg(feature = "kernel")]
fn cyw43_submit_control_exchange_completion(
    contract: DriverTaskContract,
    payload: &[u8],
    cmd: u32,
    id: u16,
    stage: &'static str,
) -> Result<DriverTaskCompletionRecord, DriverTaskNetError> {
    cyw43_submit_control_exchange_completion_with_header_mode(
        contract,
        payload,
        cmd,
        id,
        stage,
        Cyw43ControlHeaderMode::Extended,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_control_exchange_completion_with_header_mode(
    contract: DriverTaskContract,
    payload: &[u8],
    cmd: u32,
    id: u16,
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
) -> Result<DriverTaskCompletionRecord, DriverTaskNetError> {
    let completion = cyw43_submit_control_exchange_unmapped_with_header_mode(
        contract,
        payload,
        cmd,
        id,
        stage,
        header_mode,
    )
    .map_err(|err| err.into_net_error())?;
    if completion.code == DriverTaskCompletionCode::FrameReady.as_u16() {
        Ok(completion)
    } else {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            stage,
            "fail",
            Some(completion),
        );
        Err(DriverTaskNetError::RuntimeInit(stage))
    }
}

#[cfg(feature = "kernel")]
fn cyw43_submit_control_exchange_unmapped(
    contract: DriverTaskContract,
    payload: &[u8],
    cmd: u32,
    id: u16,
    stage: &'static str,
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
    cyw43_submit_control_exchange_unmapped_with_header_mode(
        contract,
        payload,
        cmd,
        id,
        stage,
        Cyw43ControlHeaderMode::Extended,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_control_exchange_unmapped_with_header_mode(
    contract: DriverTaskContract,
    payload: &[u8],
    cmd: u32,
    id: u16,
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        stage,
        "begin",
        None,
    );
    emit_cyw43_control_request_trace(contract, stage, cmd, id, header_mode, payload);
    let control_iovar = cyw43_control_iovar_info(payload, cmd).map_or("none", |info| info.name);
    let expected_response_len =
        cyw43_control_request_expected_response_len(cmd, cyw43_control_iovar_info(payload, cmd))
            as u16;
    let tx_descriptor = cyw43_control_frame_descriptor(payload.len(), header_mode);
    let Some(tx_completion) =
        run_cyw43_runtime_descriptor_command(contract, tx_descriptor, payload)
    else {
        record_cyw43_control_split_failure(
            contract,
            stage,
            tx_descriptor,
            "cyw43-control-tx-no-reply",
            None,
            cmd,
            id,
            header_mode,
            expected_response_len,
            control_iovar,
            0,
            0,
        );
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            stage,
            "tx-no-reply",
            None,
        );
        return Err(Cyw43CommandSubmitError::Runtime(
            DriverTaskNetError::RuntimeInit("cyw43-command-completion"),
        ));
    };
    emit_cyw43_control_split_completion(
        contract,
        stage,
        "tx-complete",
        0,
        0,
        tx_completion,
        cmd,
        id,
        header_mode,
        expected_response_len,
        control_iovar,
        0,
        0,
    );
    if !driver_task_tx_completion_submitted(tx_completion) {
        record_cyw43_control_split_failure(
            contract,
            stage,
            tx_descriptor,
            "cyw43-control-tx-not-submitted",
            Some(tx_completion),
            cmd,
            id,
            header_mode,
            expected_response_len,
            control_iovar,
            0,
            0,
        );
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            stage,
            "tx-submit-fail",
            Some(tx_completion),
        );
        return Err(Cyw43CommandSubmitError::Completion(tx_completion));
    }
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        stage,
        "tx-ready",
        Some(tx_completion),
    );
    cyw43_poll_control_exchange_reply(
        contract,
        stage,
        cmd,
        id,
        payload.len(),
        header_mode,
        expected_response_len,
        control_iovar,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_control_exchange_descriptor(
    payload_len: usize,
    cmd: u32,
    id: u16,
    header_mode: Cyw43ControlHeaderMode,
) -> DriverRuntimeCyw43CommandDescriptor {
    DriverRuntimeCyw43CommandDescriptor {
        op: DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE,
        flags: header_mode.runtime_flags(),
        payload_len: payload_len as u16,
        total_len: payload_len as u32,
        arg0: cmd,
        arg1: u32::from(id),
        ..DriverRuntimeCyw43CommandDescriptor::empty()
    }
}

#[cfg(feature = "kernel")]
fn cyw43_control_frame_descriptor(
    payload_len: usize,
    header_mode: Cyw43ControlHeaderMode,
) -> DriverRuntimeCyw43CommandDescriptor {
    DriverRuntimeCyw43CommandDescriptor {
        op: DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME,
        flags: header_mode.runtime_flags(),
        payload_len: payload_len as u16,
        total_len: payload_len as u32,
        ..DriverRuntimeCyw43CommandDescriptor::empty()
    }
}

#[cfg(feature = "kernel")]
fn cyw43_poll_control_exchange_reply(
    contract: DriverTaskContract,
    stage: &'static str,
    cmd: u32,
    id: u16,
    payload_len: usize,
    header_mode: Cyw43ControlHeaderMode,
    expected_response_len: u16,
    control_iovar: &str,
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
    let exchange_descriptor = cyw43_control_exchange_descriptor(payload_len, cmd, id, header_mode);
    let mut nonmatching_frames = 0u32;
    let mut malformed_frames = 0u32;
    let mut last_completion = None;
    for poll in 1..=CYW43_CONTROL_PLANE_POLL_ATTEMPTS {
        let flags = cyw43_control_split_poll_flags(poll);
        let Some(completion) = poll_cyw43_driver_task_control_completion(flags) else {
            continue;
        };
        last_completion = Some(completion);
        if cyw43_control_split_poll_completion_should_trace(poll, flags, completion) {
            emit_cyw43_control_split_completion(
                contract,
                stage,
                "poll-complete",
                poll,
                flags,
                completion,
                cmd,
                id,
                header_mode,
                expected_response_len,
                control_iovar,
                nonmatching_frames,
                malformed_frames,
            );
        }
        if completion.code == DriverTaskCompletionCode::Idle.as_u16() {
            continue;
        }
        if completion.code == DriverTaskCompletionCode::Fault.as_u16() {
            emit_cyw43_runtime_command_fault(
                contract,
                stage,
                exchange_descriptor,
                completion,
                None,
            );
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "poll-fault",
                Some(completion),
            );
            return Err(Cyw43CommandSubmitError::Completion(completion));
        }
        if completion.code != DriverTaskCompletionCode::FrameReady.as_u16() {
            record_cyw43_control_split_failure(
                contract,
                stage,
                exchange_descriptor,
                "cyw43-control-poll-unexpected-completion",
                Some(completion),
                cmd,
                id,
                header_mode,
                expected_response_len,
                control_iovar,
                nonmatching_frames,
                malformed_frames,
            );
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "poll-unexpected",
                Some(completion),
            );
            return Err(Cyw43CommandSubmitError::Completion(completion));
        }
        let Some((frame_flags, token)) =
            cyw43_driver_task_frame_from_completion(contract, completion)
        else {
            record_cyw43_control_split_failure(
                contract,
                stage,
                exchange_descriptor,
                "cyw43-control-frame-unavailable",
                Some(completion),
                cmd,
                id,
                header_mode,
                expected_response_len,
                control_iovar,
                nonmatching_frames,
                malformed_frames,
            );
            continue;
        };
        let frame_channel = cyw43_frame_channel(frame_flags);
        if frame_channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT {
            if cyw43_capture_event_frame_from_token(contract, stage, frame_flags, &token) {
                nonmatching_frames = nonmatching_frames.saturating_add(1);
                continue;
            }
            malformed_frames = malformed_frames.saturating_add(1);
            emit_cyw43_control_split_reply_trace(
                contract,
                stage,
                "malformed-event",
                poll,
                flags,
                completion.sequence,
                None,
                nonmatching_frames,
                malformed_frames,
                cmd,
                id,
                header_mode,
                expected_response_len,
                control_iovar,
            );
            continue;
        }
        if frame_channel != DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL {
            malformed_frames = malformed_frames.saturating_add(1);
            emit_cyw43_control_split_reply_trace(
                contract,
                stage,
                "unexpected-channel",
                poll,
                flags,
                completion.sequence,
                None,
                nonmatching_frames,
                malformed_frames,
                cmd,
                id,
                header_mode,
                expected_response_len,
                control_iovar,
            );
            continue;
        }
        let Some(reply) = cyw43_control_reply_from_token(&token) else {
            malformed_frames = malformed_frames.saturating_add(1);
            emit_cyw43_control_split_reply_trace(
                contract,
                stage,
                "malformed-reply",
                poll,
                flags,
                completion.sequence,
                None,
                nonmatching_frames,
                malformed_frames,
                cmd,
                id,
                header_mode,
                expected_response_len,
                control_iovar,
            );
            continue;
        };
        let reply_matches = reply.cmd == cmd && reply.id == id;
        if !reply_matches {
            nonmatching_frames = nonmatching_frames.saturating_add(1);
        }
        emit_cyw43_control_split_reply_trace(
            contract,
            stage,
            if reply_matches {
                "matched-reply"
            } else {
                "nonmatching-reply"
            },
            poll,
            flags,
            completion.sequence,
            Some(reply),
            nonmatching_frames,
            malformed_frames,
            cmd,
            id,
            header_mode,
            expected_response_len,
            control_iovar,
        );
        if !reply_matches {
            continue;
        }
        let response_len = match usize::try_from(reply.response_len) {
            Ok(len) => len,
            Err(_) => {
                let fault = cyw43_control_fault_completion(
                    completion.sequence,
                    CYW43_CONTROL_EXCHANGE_FAULT_DETAIL,
                    reply.response_len,
                );
                record_cyw43_control_split_failure(
                    contract,
                    stage,
                    exchange_descriptor,
                    "cyw43-control-reply-len-overflow",
                    Some(fault),
                    cmd,
                    id,
                    header_mode,
                    expected_response_len,
                    control_iovar,
                    nonmatching_frames,
                    malformed_frames,
                );
                emit_cyw43_runtime_command_fault(contract, stage, exchange_descriptor, fault, None);
                return Err(Cyw43CommandSubmitError::Completion(fault));
            }
        };
        if response_len > reply.payload_available {
            let fault = cyw43_control_fault_completion(
                completion.sequence,
                CYW43_CONTROL_EXCHANGE_FAULT_DETAIL,
                reply.response_len,
            );
            record_cyw43_control_split_failure(
                contract,
                stage,
                exchange_descriptor,
                "cyw43-control-reply-len-invalid",
                Some(fault),
                cmd,
                id,
                header_mode,
                expected_response_len,
                control_iovar,
                nonmatching_frames,
                malformed_frames,
            );
            emit_cyw43_runtime_command_fault(contract, stage, exchange_descriptor, fault, None);
            return Err(Cyw43CommandSubmitError::Completion(fault));
        }
        if reply.status != 0 {
            let fault = cyw43_control_fault_completion(
                completion.sequence,
                CYW43_CONTROL_EXCHANGE_FAULT_DETAIL,
                reply.status,
            );
            record_cyw43_control_split_failure(
                contract,
                stage,
                exchange_descriptor,
                "cyw43-control-reply-status",
                Some(fault),
                cmd,
                id,
                header_mode,
                expected_response_len,
                control_iovar,
                nonmatching_frames,
                malformed_frames,
            );
            emit_cyw43_runtime_command_fault(contract, stage, exchange_descriptor, fault, None);
            return Err(Cyw43CommandSubmitError::Completion(fault));
        }
        let Some(response_completion) = cyw43_control_response_completion(completion, response_len)
        else {
            let fault = cyw43_control_fault_completion(
                completion.sequence,
                CYW43_CONTROL_EXCHANGE_FAULT_DETAIL,
                reply.response_len,
            );
            record_cyw43_control_split_failure(
                contract,
                stage,
                exchange_descriptor,
                "cyw43-control-reply-frame-range",
                Some(fault),
                cmd,
                id,
                header_mode,
                expected_response_len,
                control_iovar,
                nonmatching_frames,
                malformed_frames,
            );
            emit_cyw43_runtime_command_fault(contract, stage, exchange_descriptor, fault, None);
            return Err(Cyw43CommandSubmitError::Completion(fault));
        };
        emit_cyw43_control_split_completion(
            contract,
            stage,
            "response-ready",
            poll,
            flags,
            response_completion,
            cmd,
            id,
            header_mode,
            expected_response_len,
            control_iovar,
            nonmatching_frames,
            malformed_frames,
        );
        return Ok(response_completion);
    }
    let timeout_event = if nonmatching_frames != 0 || malformed_frames != 0 {
        "cyw43-control-reply-nonmatching"
    } else {
        "cyw43-control-split-no-reply"
    };
    record_cyw43_control_split_failure(
        contract,
        stage,
        exchange_descriptor,
        timeout_event,
        last_completion,
        cmd,
        id,
        header_mode,
        expected_response_len,
        control_iovar,
        nonmatching_frames,
        malformed_frames,
    );
    record_cyw43_runtime_command_no_reply(contract, stage, exchange_descriptor, 0);
    let fault = cyw43_control_fault_completion(
        last_completion.map_or(0, |completion| completion.sequence),
        CYW43_CONTROL_EXCHANGE_FAULT_DETAIL,
        cyw43_control_split_timeout_result(last_completion, nonmatching_frames, malformed_frames),
    );
    emit_cyw43_runtime_command_fault(contract, stage, exchange_descriptor, fault, None);
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        stage,
        "poll-timeout",
        Some(fault),
    );
    Err(Cyw43CommandSubmitError::Completion(fault))
}

#[cfg(feature = "kernel")]
const fn cyw43_control_exchange_timeout_result(reason: u32, value: u32) -> u32 {
    let bounded_value = if value > 0xffff { 0xffff } else { value };
    CYW43_CONTROL_EXCHANGE_TIMEOUT_RESULT_MAGIC | ((reason & 0xff) << 16) | bounded_value
}

#[cfg(feature = "kernel")]
const fn cyw43_control_split_timeout_result(
    completion: Option<DriverTaskCompletionRecord>,
    nonmatching_frames: u32,
    malformed_frames: u32,
) -> u32 {
    let mismatch_frames = nonmatching_frames.saturating_add(malformed_frames);
    if mismatch_frames != 0 {
        return cyw43_control_exchange_timeout_result(8, mismatch_frames);
    }
    let completion = match completion {
        Some(completion) => completion,
        None => return cyw43_control_exchange_timeout_result(3, 0),
    };
    let reason = match completion.detail {
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_NOT_READY => 1,
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_RFRAME_READ_FAILED => 2,
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_NO_RFRAME => 3,
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_INVALID_RFRAME_LEN => 4,
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_RX_REQUEST_TOO_LARGE => 5,
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_F2_READ_FAILED => 6,
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_SDPCM_DECODE_MISS => 7,
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_FAILED => 9,
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY => 10,
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_INVALID_SDPCM => 11,
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_REMAINDER_FAILED => 12,
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_REMAINDER_TOO_LARGE => 13,
        _ => 3,
    };
    cyw43_control_exchange_timeout_result(reason, completion.result)
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43ControlReply {
    cmd: u32,
    id: u16,
    status: u32,
    response_len: u32,
    payload_available: usize,
}

#[cfg(feature = "kernel")]
fn cyw43_control_reply_from_token(token: &DriverTaskNetRxToken) -> Option<Cyw43ControlReply> {
    if token.len < CYW43_BCDC_HEADER_BYTES {
        return None;
    }
    let bytes = &token.buffer[..token.len];
    Some(Cyw43ControlReply {
        cmd: le_u32_at(bytes, 0)?,
        response_len: le_u32_at(bytes, 4)?,
        id: le_u16_at(bytes, 10)?,
        status: le_u32_at(bytes, 12)?,
        payload_available: token.len.saturating_sub(CYW43_BCDC_HEADER_BYTES),
    })
}

#[cfg(feature = "kernel")]
fn cyw43_control_response_completion(
    completion: DriverTaskCompletionRecord,
    response_len: usize,
) -> Option<DriverTaskCompletionRecord> {
    let frame_len = u16::try_from(response_len).ok()?;
    let offset = completion
        .frame
        .offset
        .checked_add(CYW43_BCDC_HEADER_BYTES as u32)?;
    Some(DriverTaskCompletionRecord {
        sequence: completion.sequence,
        code: DriverTaskCompletionCode::FrameReady.as_u16(),
        detail: 0,
        result: u32::from(frame_len),
        frame: DriverFrameDescriptor {
            offset,
            len: frame_len,
            flags: completion.frame.flags,
        },
    })
}

#[cfg(feature = "kernel")]
const fn cyw43_control_fault_completion(
    sequence: u32,
    detail: u16,
    result: u32,
) -> DriverTaskCompletionRecord {
    DriverTaskCompletionRecord {
        sequence,
        code: DriverTaskCompletionCode::Fault.as_u16(),
        detail,
        result,
        frame: DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: 0,
        },
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_control_split_poll_flags(poll: usize) -> u16 {
    if matches!(poll, 1 | 4 | 16 | 64 | 256) {
        DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD
    } else {
        0
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_control_split_poll_completion_should_trace(
    poll: usize,
    flags: u16,
    completion: DriverTaskCompletionRecord,
) -> bool {
    flags & DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD != 0
        || completion.code != DriverTaskCompletionCode::Idle.as_u16()
        || poll == CYW43_CONTROL_PLANE_POLL_ATTEMPTS
}

#[cfg(feature = "kernel")]
fn record_cyw43_control_split_failure(
    contract: DriverTaskContract,
    stage: &'static str,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    reason: &'static str,
    completion: Option<DriverTaskCompletionRecord>,
    expected_cmd: u32,
    expected_id: u16,
    header_mode: Cyw43ControlHeaderMode,
    expected_response_len: u16,
    control_iovar: &str,
    nonmatching_frames: u32,
    malformed_frames: u32,
) {
    let (detail, result) =
        completion.map_or((0, 0), |completion| (completion.detail, completion.result));
    *CYW43_LAST_RUNTIME_COMMAND_FAULT.lock() = Some(Cyw43RuntimeCommandFaultStatus {
        stage,
        op: descriptor.op,
        flags: descriptor.flags,
        target_addr: descriptor.target_addr,
        payload_offset: descriptor.payload_offset,
        payload_len: descriptor.payload_len,
        total_len: descriptor.total_len,
        control_cmd: cyw43_descriptor_control_cmd(descriptor),
        control_id: cyw43_descriptor_control_id(descriptor),
        control_header_mode: cyw43_descriptor_control_header_mode(descriptor),
        control_response_len: cyw43_control_request_expected_response_len(
            cyw43_descriptor_control_cmd(descriptor),
            None,
        ) as u16,
        detail,
        reason,
        result,
    });
    *CYW43_LAST_SDIO_OWNER_FAULT.lock() = None;
    if let Some(completion) = completion {
        emit_cyw43_control_split_completion(
            contract,
            stage,
            reason,
            0,
            descriptor.flags,
            completion,
            expected_cmd,
            expected_id,
            header_mode,
            expected_response_len,
            control_iovar,
            nonmatching_frames,
            malformed_frames,
        );
    }
}

#[cfg(feature = "kernel")]
fn emit_cyw43_control_split_completion(
    contract: DriverTaskContract,
    stage: &'static str,
    event: &'static str,
    poll: usize,
    flags: u16,
    completion: DriverTaskCompletionRecord,
    expected_cmd: u32,
    expected_id: u16,
    header_mode: Cyw43ControlHeaderMode,
    expected_response_len: u16,
    control_iovar: &str,
    nonmatching_frames: u32,
    malformed_frames: u32,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<768>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_CONTROL_SPLIT contract={} stage={} event={} poll={} flags=0x{:04x} sequence={} code={} detail=0x{:04x} result=0x{:08x} frame_off={} frame_len={} frame_flags=0x{:04x} expected_cmd={} expected_cmd_hex=0x{:08x} expected_id={} header_mode={} expected_response_len={} iovar={} nonmatching_frames={} malformed_frames={}",
        contract.name,
        stage,
        event,
        poll,
        flags,
        completion.sequence,
        completion.code,
        completion.detail,
        completion.result,
        completion.frame.offset,
        completion.frame.len,
        completion.frame.flags,
        expected_cmd,
        expected_cmd,
        expected_id,
        header_mode.as_str(),
        expected_response_len,
        control_iovar,
        nonmatching_frames,
        malformed_frames,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_control_split_reply_trace(
    contract: DriverTaskContract,
    stage: &'static str,
    event: &'static str,
    poll: usize,
    flags: u16,
    completion_sequence: u32,
    reply: Option<Cyw43ControlReply>,
    nonmatching_frames: u32,
    malformed_frames: u32,
    expected_cmd: u32,
    expected_id: u16,
    header_mode: Cyw43ControlHeaderMode,
    expected_response_len: u16,
    control_iovar: &str,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<768>::new();
    if let Some(reply) = reply {
        let _ = write!(
            line,
            "CYW43_DRIVER_TASK_CONTROL_REPLY contract={} stage={} event={} poll={} flags=0x{:04x} completion_sequence={} cmd={} cmd_hex=0x{:08x} id={} status=0x{:08x} response_len={} payload_available={} expected_cmd={} expected_cmd_hex=0x{:08x} expected_id={} header_mode={} expected_response_len={} iovar={} reply_match={} nonmatching_frames={} malformed_frames={}",
            contract.name,
            stage,
            event,
            poll,
            flags,
            completion_sequence,
            reply.cmd,
            reply.cmd,
            reply.id,
            reply.status,
            reply.response_len,
            reply.payload_available,
            expected_cmd,
            expected_cmd,
            expected_id,
            header_mode.as_str(),
            expected_response_len,
            control_iovar,
            if reply.cmd == expected_cmd && reply.id == expected_id {
                "yes"
            } else {
                "no"
            },
            nonmatching_frames,
            malformed_frames,
        );
    } else {
        let _ = write!(
            line,
            "CYW43_DRIVER_TASK_CONTROL_REPLY contract={} stage={} event={} poll={} flags=0x{:04x} completion_sequence={} cmd=none cmd_hex=none id=none status=none response_len=0 payload_available=0 expected_cmd={} expected_cmd_hex=0x{:08x} expected_id={} header_mode={} expected_response_len={} iovar={} reply_match=no nonmatching_frames={} malformed_frames={}",
            contract.name,
            stage,
            event,
            poll,
            flags,
            completion_sequence,
            expected_cmd,
            expected_cmd,
            expected_id,
            header_mode.as_str(),
            expected_response_len,
            control_iovar,
            nonmatching_frames,
            malformed_frames,
        );
    }
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn cyw43_poll_control_plane_frames(stage: &'static str) -> u32 {
    let mut observed = 0u32;
    for _ in 0..CYW43_CONTROL_PLANE_POLL_ATTEMPTS {
        let Some((flags, token)) = poll_cyw43_driver_task_control_frame() else {
            continue;
        };
        observed = observed.saturating_add(1);
        let _ = cyw43_capture_event_frame_from_token(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            stage,
            flags,
            &token,
        );
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::Cyw43Wifi,
            stage,
            "frame",
            None,
        );
        if token.len != 0 {
            break;
        }
    }
    observed
}

#[cfg(feature = "kernel")]
fn reset_cyw43_control_plane_state() {
    CYW43_CONTROL_PLANE_READY.store(0, Ordering::Release);
    CYW43_ASSOCIATED.store(0, Ordering::Release);
    CYW43_LINK_UP.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_RX.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_START.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_ACTIVE.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_REQUIRED.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_TX_RETRIES.store(0, Ordering::Release);
    *CYW43_HOST_EAPOL_SESSION.lock() = None;
    *CYW43_HOST_EAPOL_PENDING_EVENT.lock() = None;
    clear_cyw43_active_prompt_poll();
    CYW43_BCDC_IOCTL_ID.store(0, Ordering::Release);
    *CYW43_RUNTIME_MAC.lock() = CYW43_DRIVER_TASK_MAC;
}

#[cfg(feature = "kernel")]
fn complete_cyw43_linked_runtime_firmware_once<H>(
    hal: &mut H,
    contract: DriverTaskContract,
    bundle: crate::hal::WifiFirmwareBundle<'_>,
    reset_vector: u32,
) -> Result<(), Cyw43FirmwareInitError>
where
    H: Hardware<Error = HalError>,
{
    init_sdio_host_linked_runtime(hal).map_err(Cyw43FirmwareInitError::Runtime)?;
    complete_cyw43_linked_runtime_transport(contract).map_err(Cyw43FirmwareInitError::Command)?;
    submit_cyw43_runtime_command_checked(
        contract,
        DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        },
        &[],
    )
    .map_err(Cyw43FirmwareInitError::Command)?;
    stream_cyw43_runtime_payload(
        hal,
        contract,
        DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
        CYW43_RAM_BASE_4345,
        bundle.firmware,
        bundle.firmware.len(),
    )?;
    complete_cyw43_linked_runtime_firmware_tail(hal, contract, bundle, reset_vector)
}

#[cfg(feature = "kernel")]
fn complete_cyw43_linked_runtime_transport(
    contract: DriverTaskContract,
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
    let descriptor = DriverRuntimeCyw43CommandDescriptor {
        op: DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT,
        ..DriverRuntimeCyw43CommandDescriptor::empty()
    };
    for _ in 0..CYW43_RUNTIME_TRANSPORT_PHASE_ATTEMPTS {
        let completion = submit_cyw43_runtime_command_checked(contract, descriptor, &[])?;
        if completion.detail == DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_READY {
            return Ok(completion);
        }
    }
    Err(Cyw43CommandSubmitError::Runtime(
        DriverTaskNetError::RuntimeInit("cyw43-transport-phase-budget"),
    ))
}

#[cfg(feature = "kernel")]
fn complete_cyw43_linked_runtime_firmware_from_offset<H>(
    hal: &mut H,
    contract: DriverTaskContract,
    bundle: crate::hal::WifiFirmwareBundle<'_>,
    reset_vector: u32,
    resume_offset: usize,
    force_first_chunk_byte_mode: bool,
) -> Result<(), Cyw43FirmwareInitError>
where
    H: Hardware<Error = HalError>,
{
    stream_cyw43_runtime_payload_from_offset(
        hal,
        contract,
        DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
        CYW43_RAM_BASE_4345,
        bundle.firmware,
        bundle.firmware.len(),
        resume_offset,
        force_first_chunk_byte_mode,
    )?;
    complete_cyw43_linked_runtime_firmware_tail(hal, contract, bundle, reset_vector)
}

#[cfg(feature = "kernel")]
fn complete_cyw43_linked_runtime_firmware_tail<H>(
    hal: &mut H,
    contract: DriverTaskContract,
    bundle: crate::hal::WifiFirmwareBundle<'_>,
    reset_vector: u32,
) -> Result<(), Cyw43FirmwareInitError>
where
    H: Hardware<Error = HalError>,
{
    let nvram = crate::hal::pi4_wifi::normalize_nvram(bundle.nvram);
    let nvram_offset = CYW43_RAM_BASE_4345
        .checked_add(CYW43_RAM_SIZE_4345_PI4)
        .and_then(|value| value.checked_sub(4))
        .and_then(|value| value.checked_sub(u32::try_from(nvram.len()).ok()?))
        .ok_or(Cyw43FirmwareInitError::Runtime(
            DriverTaskNetError::RuntimeInit("cyw43-nvram-range"),
        ))?;
    stream_cyw43_runtime_payload(
        hal,
        contract,
        DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK,
        nvram_offset,
        nvram.as_slice(),
        nvram.len(),
    )?;

    let nvram_words = u32::try_from(nvram.len() / 4).map_err(|_| {
        Cyw43FirmwareInitError::Runtime(DriverTaskNetError::RuntimeInit("cyw43-nvram-len"))
    })?;
    let nvram_magic = (!nvram_words << 16) | nvram_words;
    let nvram_tail = CYW43_RAM_BASE_4345
        .checked_add(CYW43_RAM_SIZE_4345_PI4)
        .and_then(|value| value.checked_sub(4))
        .ok_or(Cyw43FirmwareInitError::Runtime(
            DriverTaskNetError::RuntimeInit("cyw43-nvram-tail"),
        ))?;
    submit_cyw43_runtime_command_checked(
        contract,
        DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL,
            target_addr: nvram_tail,
            arg0: nvram_magic,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        },
        &[],
    )
    .map_err(Cyw43FirmwareInitError::Command)?;
    submit_cyw43_runtime_command_checked(
        contract,
        DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_RELEASE,
            arg0: reset_vector,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        },
        &[],
    )
    .map_err(Cyw43FirmwareInitError::Command)?;
    Ok(())
}

#[cfg(feature = "kernel")]
fn cyw43_firmware_resume_offset(
    fault: Cyw43RuntimeCommandFaultStatus,
    firmware_len: usize,
) -> Option<usize> {
    if fault.op != DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK
        || fault.payload_len == 0
        || usize::try_from(fault.total_len).ok()? != firmware_len
        || fault.target_addr < CYW43_RAM_BASE_4345
    {
        return None;
    }
    let offset = usize::try_from(fault.target_addr - CYW43_RAM_BASE_4345).ok()?;
    let payload_len = usize::from(fault.payload_len);
    let end = offset.checked_add(payload_len)?;
    let padded_tail_end = firmware_len
        .checked_add(CYW43_RUNTIME_FIRMWARE_TAIL_PAD_ALIGNMENT - 1)?
        / CYW43_RUNTIME_FIRMWARE_TAIL_PAD_ALIGNMENT
        * CYW43_RUNTIME_FIRMWARE_TAIL_PAD_ALIGNMENT;
    if end <= firmware_len
        || (offset < firmware_len
            && end <= padded_tail_end
            && payload_len <= CYW43_RUNTIME_FIRMWARE_TAIL_PAD_MAX_BYTES)
    {
        Some(offset)
    } else {
        None
    }
}

#[cfg(feature = "kernel")]
fn cyw43_nvram_tail_resume_offset(
    fault: Cyw43RuntimeCommandFaultStatus,
    firmware_len: usize,
    nvram_len: usize,
) -> Option<usize> {
    if fault.op != DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK
        || fault.payload_len == 0
        || usize::try_from(fault.total_len).ok()? != nvram_len
        || usize::from(fault.payload_len) != nvram_len
    {
        return None;
    }
    let nvram_base = CYW43_RAM_BASE_4345
        .checked_add(CYW43_RAM_SIZE_4345_PI4)?
        .checked_sub(4)?
        .checked_sub(u32::try_from(nvram_len).ok()?)?;
    if fault.target_addr == nvram_base {
        Some(firmware_len)
    } else {
        None
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_firmware_resume_forces_byte_mode(fault: Cyw43RuntimeCommandFaultStatus) -> bool {
    crate::cyw43_recovery::firmware_resume_forces_byte_mode(fault.op, fault.detail)
}

#[cfg(feature = "kernel")]
fn init_sdio_host_linked_runtime<H>(_hal: &mut H) -> Result<(), DriverTaskNetError>
where
    H: Hardware<Error = HalError>,
{
    if SDIO_LINKED_RUNTIME_READY.load(Ordering::Acquire) != 0 {
        return Ok(());
    }
    let contract = SDIO_HOST_DRIVER_TASK_CONTRACT;
    let _ = crate::hal::driver_task::register_pi4_bus_ring_service(contract);
    emit_sdio_driver_task_replay_status("descriptor-replay", "begin");
    if !crate::hal::driver_task::ensure_deferred_runtime_init_descriptor(
        contract,
        DriverTaskHotPath::SdioHost,
    ) {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::SdioHost,
            "runtime-descriptor-replay",
            "pending",
            None,
        );
        emit_sdio_driver_task_replay_status("descriptor-replay", "pending");
        return Err(DriverTaskNetError::RuntimePending("sdio-host"));
    }
    emit_sdio_driver_task_replay_status("descriptor-replay", "ready");
    let command = crate::hal::driver_task::runtime_engine_init_command(
        DriverTaskHotPath::SdioHost,
        DriverTaskBudgetGrant::from_contract(contract),
    );
    emit_sdio_driver_task_replay_status("engine-init", "begin");
    let completion = run_driver_task_net_service(contract, command);
    let initialized = completion.is_some_and(|completion| {
        completion.code == DriverTaskCompletionCode::Progress.as_u16() && completion.result == 1
    });
    let status = sdio_engine_init_completion_status(completion, initialized);
    emit_sdio_driver_task_replay_status("engine-init", status);
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::SdioHost,
        "sdio-engine-init",
        status,
        completion,
    );
    if !initialized {
        return Err(DriverTaskNetError::RuntimeInit("sdio-host-linked-runtime"));
    }
    if crate::hal::driver_task::register_driver_task_runtime_owner_state(
        DriverTaskHotPath::SdioHost,
    ) {
        SDIO_LINKED_RUNTIME_READY.store(1, Ordering::Release);
        emit_sdio_driver_task_replay_status("owner-state", "ready");
        Ok(())
    } else {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::SdioHost,
            "sdio-owner-state",
            "descriptor-rejected",
            None,
        );
        emit_sdio_driver_task_replay_status("owner-state", "descriptor-rejected");
        Err(DriverTaskNetError::RuntimeInit("sdio-host-linked-runtime"))
    }
}

#[cfg(feature = "kernel")]
fn replay_sdio_host_linked_runtime_preserving_hal<H>(
    hal: &mut H,
    stage: &'static str,
) -> Result<(), DriverTaskNetError>
where
    H: Hardware<Error = HalError>,
{
    emit_cyw43_sdio_replay_resource_init_status(stage, "begin", None);
    if sdio_owner_recovery_can_preserve_ready_state() {
        emit_sdio_driver_task_replay_status("owner-state", "preserved-ready");
        emit_cyw43_sdio_replay_resource_init_status(stage, "ready", None);
        return Ok(());
    }
    SDIO_LINKED_RUNTIME_READY.store(0, Ordering::Release);
    match init_sdio_host_linked_runtime(hal) {
        Ok(()) => {
            emit_cyw43_sdio_replay_resource_init_status(stage, "ready", None);
            Ok(())
        }
        Err(err) => {
            emit_cyw43_sdio_replay_resource_init_status(stage, "failed", None);
            Err(err)
        }
    }
}

#[cfg(feature = "kernel")]
fn emit_cyw43_sdio_replay_resource_init_status(
    stage: &'static str,
    status: &'static str,
    completion: Option<DriverTaskCompletionRecord>,
) {
    if cyw43_sdio_replay_resource_status_is_redundant(stage, status) {
        return;
    }
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        CYW43_WIFI_DRIVER_TASK_CONTRACT,
        DriverTaskHotPath::Cyw43Wifi,
        stage,
        status,
        completion,
    );
}

#[cfg(feature = "kernel")]
fn cyw43_sdio_replay_resource_status_is_redundant(stage: &str, status: &str) -> bool {
    stage == "cyw43-firmware-recover" && matches!(status, "begin" | "ready")
}

#[cfg(feature = "kernel")]
fn sdio_owner_recovery_can_preserve_ready_state() -> bool {
    SDIO_LINKED_RUNTIME_READY.load(Ordering::Acquire) != 0
}

#[cfg(not(feature = "kernel"))]
fn complete_cyw43_linked_runtime_firmware<H>(
    _hal: &mut H,
    _contract: DriverTaskContract,
) -> Result<(), DriverTaskNetError>
where
    H: Hardware<Error = HalError>,
{
    Err(DriverTaskNetError::RuntimeInit("cyw43-kernel-runtime"))
}

#[cfg(feature = "kernel")]
fn cyw43_runtime_stream_payload_limit(_desc_size: usize) -> Result<usize, Cyw43FirmwareInitError> {
    Ok((DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as usize)
        .min(CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES))
}

#[cfg(feature = "kernel")]
const fn cyw43_runtime_stream_progress_due(
    offset: usize,
    chunk_len: usize,
    total_len: usize,
) -> bool {
    let uploaded = offset + chunk_len;
    offset == 0
        || uploaded == total_len
        || offset / CYW43_RUNTIME_STREAM_PROGRESS_INTERVAL
            != uploaded / CYW43_RUNTIME_STREAM_PROGRESS_INTERVAL
}

#[cfg(feature = "kernel")]
fn emit_cyw43_runtime_stream_progress(
    contract: DriverTaskContract,
    stage: &'static str,
    uploaded: usize,
    total_len: usize,
    target_addr: u32,
    chunk_len: usize,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_STREAM_PROGRESS contract={} stage={} uploaded={} total_len={} target=0x{:08x} chunk_len={}",
        contract.name,
        stage,
        uploaded,
        total_len,
        target_addr,
        chunk_len,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_runtime_stream_tail_padding(
    contract: DriverTaskContract,
    stage: &'static str,
    offset: usize,
    total_len: usize,
    target_addr: u32,
    chunk_len: usize,
    padded_len: usize,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_STREAM_TAIL_PAD contract={} stage={} offset={} total_len={} target=0x{:08x} chunk_len={} padded_len={}",
        contract.name, stage, offset, total_len, target_addr, chunk_len, padded_len,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_runtime_firmware_recovery(
    contract: DriverTaskContract,
    attempt: usize,
    resume_offset: usize,
    force_byte_mode: bool,
    same_offset_attempts: usize,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_FIRMWARE_RECOVERY contract={} attempt={} resume_offset={} force_byte={} same_offset_attempts={}",
        contract.name, attempt, resume_offset, force_byte_mode, same_offset_attempts,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
const fn cyw43_runtime_firmware_tail_padded_len(
    op: u16,
    offset: usize,
    payload_len: usize,
    chunk_len: usize,
    max_payload: usize,
) -> usize {
    if op != DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK
        || offset > payload_len
        || chunk_len == 0
        || chunk_len != payload_len - offset
        || chunk_len % CYW43_RUNTIME_FIRMWARE_TAIL_PAD_ALIGNMENT == 0
    {
        return chunk_len;
    }
    let padded = ((chunk_len + CYW43_RUNTIME_FIRMWARE_TAIL_PAD_ALIGNMENT - 1)
        / CYW43_RUNTIME_FIRMWARE_TAIL_PAD_ALIGNMENT)
        * CYW43_RUNTIME_FIRMWARE_TAIL_PAD_ALIGNMENT;
    if padded <= max_payload && padded <= CYW43_RUNTIME_FIRMWARE_TAIL_PAD_MAX_BYTES {
        padded
    } else {
        chunk_len
    }
}

#[cfg(feature = "kernel")]
fn submit_cyw43_runtime_stream_command(
    contract: DriverTaskContract,
    op: u16,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    payload: &[u8],
) -> Result<(), Cyw43FirmwareInitError> {
    let mut attempts = 0usize;
    loop {
        match submit_cyw43_runtime_command_checked(contract, descriptor, payload) {
            Ok(_) => return Ok(()),
            Err(err) => {
                if let Some(completion) = err.same_command_retry_completion() {
                    attempts = attempts.saturating_add(1);
                    let status = if attempts < CYW43_RUNTIME_STREAM_COMMAND_RETRIES {
                        "stream-fault-retry-runtime-ladder"
                    } else {
                        "stream-fault-retry-exhausted"
                    };
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        contract,
                        DriverTaskHotPath::Cyw43Wifi,
                        cyw43_runtime_command_stage(op),
                        status,
                        Some(completion),
                    );
                    if attempts < CYW43_RUNTIME_STREAM_COMMAND_RETRIES {
                        continue;
                    }
                } else if let Some(completion) = err.recoverable_completion() {
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        contract,
                        DriverTaskHotPath::Cyw43Wifi,
                        cyw43_runtime_command_stage(op),
                        "stream-fault-owner-recovery-required",
                        Some(completion),
                    );
                }
                return Err(Cyw43FirmwareInitError::Command(err));
            }
        }
    }
}

#[cfg(feature = "kernel")]
fn stream_cyw43_runtime_payload<H>(
    hal: &mut H,
    contract: DriverTaskContract,
    op: u16,
    base_addr: u32,
    payload: &[u8],
    total_len: usize,
) -> Result<(), Cyw43FirmwareInitError>
where
    H: Hardware<Error = HalError>,
{
    stream_cyw43_runtime_payload_from_offset(
        hal, contract, op, base_addr, payload, total_len, 0, false,
    )
}

#[cfg(feature = "kernel")]
fn stream_cyw43_runtime_payload_from_offset<H>(
    _hal: &mut H,
    contract: DriverTaskContract,
    op: u16,
    base_addr: u32,
    payload: &[u8],
    total_len: usize,
    start_offset: usize,
    force_first_chunk_byte_mode: bool,
) -> Result<(), Cyw43FirmwareInitError>
where
    H: Hardware<Error = HalError>,
{
    let desc_size = core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>();
    let max_payload = cyw43_runtime_stream_payload_limit(desc_size)?;
    if start_offset > payload.len() {
        return Err(Cyw43FirmwareInitError::Runtime(
            DriverTaskNetError::RuntimeInit("cyw43-stream-resume-offset"),
        ));
    }
    let mut offset = start_offset;
    while offset < payload.len() {
        let chunk_len = (payload.len() - offset).min(max_payload);
        let target_addr = base_addr
            .checked_add(u32::try_from(offset).map_err(|_| {
                Cyw43FirmwareInitError::Runtime(DriverTaskNetError::RuntimeInit("cyw43-offset"))
            })?)
            .ok_or(Cyw43FirmwareInitError::Runtime(
                DriverTaskNetError::RuntimeInit("cyw43-target-range"),
            ))?;
        let mut descriptor = DriverRuntimeCyw43CommandDescriptor {
            op,
            target_addr,
            payload_len: chunk_len as u16,
            total_len: total_len as u32,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        };
        if op == DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK {
            descriptor.arg0 = chunk_len as u32;
        }
        if force_first_chunk_byte_mode && offset == start_offset {
            descriptor.flags |= DRIVER_RUNTIME_CYW43_FLAG_FORCE_BYTE_MODE;
        }
        let padded_chunk_len = cyw43_runtime_firmware_tail_padded_len(
            op,
            offset,
            payload.len(),
            chunk_len,
            max_payload,
        );
        if padded_chunk_len > chunk_len {
            let mut padded_payload = [0u8; CYW43_RUNTIME_FIRMWARE_TAIL_PAD_MAX_BYTES];
            padded_payload[..chunk_len].copy_from_slice(&payload[offset..offset + chunk_len]);
            descriptor.payload_len = padded_chunk_len as u16;
            emit_cyw43_runtime_stream_tail_padding(
                contract,
                cyw43_runtime_command_stage(op),
                offset,
                payload.len(),
                target_addr,
                chunk_len,
                padded_chunk_len,
            );
            submit_cyw43_runtime_stream_command(
                contract,
                op,
                descriptor,
                &padded_payload[..padded_chunk_len],
            )?;
        } else {
            submit_cyw43_runtime_stream_command(
                contract,
                op,
                descriptor,
                &payload[offset..offset + chunk_len],
            )?;
        }
        let uploaded = offset + chunk_len;
        if cyw43_runtime_stream_progress_due(offset, chunk_len, payload.len()) {
            emit_cyw43_runtime_stream_progress(
                contract,
                cyw43_runtime_command_stage(op),
                uploaded,
                payload.len(),
                target_addr,
                chunk_len,
            );
        }
        offset += chunk_len;
    }
    Ok(())
}

#[cfg(feature = "kernel")]
const fn cyw43_runtime_command_stage(op: u16) -> &'static str {
    match op {
        DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT => "cyw43-transport-init",
        DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP => "cyw43-firmware-prep",
        DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK => "cyw43-firmware-chunk",
        DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK => "cyw43-nvram-chunk",
        DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL => "cyw43-nvram-tail",
        DRIVER_RUNTIME_CYW43_OP_RELEASE => "cyw43-firmware-release",
        DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE => "cyw43-control-exchange",
        _ => "cyw43-command",
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_command_stage_always_logs_success(op: u16) -> bool {
    !matches!(
        op,
        DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK | DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK
    )
}

#[cfg(feature = "kernel")]
const fn cyw43_runtime_command_progress_status(op: u16, detail: u16) -> &'static str {
    if op == DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT
        && detail != DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_READY
    {
        "progress"
    } else {
        "ready"
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_transport_detail_is_progress(detail: u16) -> bool {
    matches!(
        detail,
        DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BUS_LINK_READY
            | DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_CARD_READY
            | DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F1_BLOCK_READY
            | DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F2_BLOCK_READY
            | DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F1_ENABLED
            | DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_HOST_READY
            | DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BACKPLANE_READY
            | DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_READY
    )
}

#[cfg(feature = "kernel")]
fn cyw43_runtime_command_completion_is_progress(
    op: u16,
    completion: DriverTaskCompletionRecord,
) -> bool {
    completion.code == DriverTaskCompletionCode::Progress.as_u16()
        && (completion.result != 0
            || (op == DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT
                && cyw43_transport_detail_is_progress(completion.detail)))
}

#[cfg(feature = "kernel")]
const fn cyw43_runtime_command_uses_shared_payload(op: u16) -> bool {
    matches!(
        op,
        DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK | DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK
    )
}

#[cfg(feature = "kernel")]
fn submit_cyw43_runtime_command_checked(
    contract: DriverTaskContract,
    mut descriptor: DriverRuntimeCyw43CommandDescriptor,
    payload: &[u8],
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
    let stage = cyw43_runtime_command_stage(descriptor.op);
    let desc_size = core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>();
    let ring_payload_limit = MAX_DRIVER_TASK_FRAME_BYTES.checked_sub(desc_size).ok_or(
        Cyw43CommandSubmitError::Runtime(DriverTaskNetError::RuntimeInit("cyw43-command-budget")),
    )?;
    let use_shared_payload =
        !payload.is_empty() && cyw43_runtime_command_uses_shared_payload(descriptor.op);
    if !use_shared_payload && payload.len() > ring_payload_limit {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            stage,
            "budget-exceeded",
            None,
        );
        return Err(Cyw43CommandSubmitError::Runtime(
            DriverTaskNetError::RuntimeInit("cyw43-command-budget"),
        ));
    }
    if payload.is_empty() {
        descriptor.payload_offset = 0;
    } else if use_shared_payload {
        let Some(staged_payload) =
            crate::hal::driver_task::describe_driver_task_shared_payload(payload, 0)
        else {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "stage-shared-failed",
                None,
            );
            return Err(Cyw43CommandSubmitError::Runtime(
                DriverTaskNetError::RuntimeInit("cyw43-stage-shared-payload"),
            ));
        };
        descriptor.payload_offset = staged_payload.offset;
        descriptor.payload_len = staged_payload.len;
    } else {
        let payload_offset = crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET
            .checked_add(desc_size)
            .ok_or(Cyw43CommandSubmitError::Runtime(
                DriverTaskNetError::RuntimeInit("cyw43-payload-offset"),
            ))?;
        descriptor.payload_offset = u16::try_from(payload_offset).map_err(|_| {
            Cyw43CommandSubmitError::Runtime(DriverTaskNetError::RuntimeInit(
                "cyw43-payload-offset",
            ))
        })?;
    }
    let mut scratch = [0u8; core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>()];
    encode_cyw43_descriptor(&mut scratch, descriptor);
    if !use_shared_payload && !payload.is_empty() {
        let mut ring_payload = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
        ring_payload[..desc_size].copy_from_slice(&scratch);
        ring_payload[desc_size..desc_size + payload.len()].copy_from_slice(payload);
        let staged_payload = &ring_payload[..desc_size + payload.len()];
        let Some(staged) =
            crate::hal::driver_task::describe_driver_task_ring_frame(staged_payload, 0)
        else {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "stage-failed",
                None,
            );
            return Err(Cyw43CommandSubmitError::Runtime(
                DriverTaskNetError::RuntimeInit("cyw43-stage-command"),
            ));
        };
        let staging_segments = [DriverTaskStagingSegment::ring_frame(staged_payload, 0)];
        return submit_staged_cyw43_runtime_descriptor(
            contract,
            descriptor,
            stage,
            desc_size,
            staged,
            &staging_segments,
            None,
        );
    }
    let Some(staged) = crate::hal::driver_task::describe_driver_task_ring_payload_at(
        crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET,
        &scratch,
        0,
    ) else {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            stage,
            "stage-failed",
            None,
        );
        return Err(Cyw43CommandSubmitError::Runtime(
            DriverTaskNetError::RuntimeInit("cyw43-stage-command"),
        ));
    };
    if use_shared_payload {
        let staging_segments = [
            DriverTaskStagingSegment::shared(payload, 0),
            DriverTaskStagingSegment::ring_payload_at(
                crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET,
                &scratch,
                0,
            ),
        ];
        submit_staged_cyw43_runtime_descriptor(
            contract,
            descriptor,
            stage,
            desc_size,
            staged,
            &staging_segments,
            Some(payload),
        )
    } else {
        let staging_segments = [DriverTaskStagingSegment::ring_payload_at(
            crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET,
            &scratch,
            0,
        )];
        submit_staged_cyw43_runtime_descriptor(
            contract,
            descriptor,
            stage,
            desc_size,
            staged,
            &staging_segments,
            None,
        )
    }
}

#[cfg(feature = "kernel")]
fn cyw43_payload_digest(payload: &[u8]) -> Cyw43PayloadDigest {
    let mut first = 0u8;
    let mut last = 0u8;
    let mut xor = 0u8;
    let mut sum = 0u32;
    for (index, byte) in payload.iter().copied().enumerate() {
        if index == 0 {
            first = byte;
        }
        last = byte;
        xor ^= byte;
        sum = sum.wrapping_add(u32::from(byte));
    }
    Cyw43PayloadDigest {
        first,
        last,
        xor,
        sum,
    }
}

fn submit_staged_cyw43_runtime_descriptor(
    contract: DriverTaskContract,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    stage: &'static str,
    desc_size: usize,
    staged: DriverFrameDescriptor,
    staging_segments: &[DriverTaskStagingSegment<'_>],
    producer_payload: Option<&[u8]>,
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
    let mut command = DriverTaskCommandRecord::pi4_hot_path(
        0,
        DriverTaskHotPath::Cyw43Wifi,
        DriverTaskBudgetGrant::from_contract(contract),
        DriverFrameDescriptor {
            offset: staged.offset,
            len: desc_size as u16,
            flags: staged.flags,
        },
    );
    command.aux0 = DRIVER_RUNTIME_CYW43_COMMAND_AUX;
    let no_reply_resume_limit = cyw43_runtime_no_reply_resume_limit(descriptor.op);
    let mut no_reply_resumes = 0usize;
    let mut admission_reject_retries = 0usize;
    loop {
        let completion = loop {
            if let Some(completion) =
                run_driver_task_net_service_staged(contract, command, staging_segments)
            {
                break completion;
            }
            if no_reply_resumes < no_reply_resume_limit {
                no_reply_resumes = no_reply_resumes.saturating_add(1);
                continue;
            }
            record_cyw43_runtime_command_no_reply(contract, stage, descriptor, no_reply_resumes);
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "no-reply",
                None,
            );
            return Err(Cyw43CommandSubmitError::Runtime(
                DriverTaskNetError::RuntimeInit("cyw43-command-completion"),
            ));
        };
        if cyw43_transport_admission_reject_without_current_progress(
            descriptor,
            completion,
            crate::hal::driver_task::latest_driver_task_ring_progress(contract),
        ) && admission_reject_retries < CYW43_TRANSPORT_ADMISSION_REJECT_RETRIES
        {
            admission_reject_retries = admission_reject_retries.saturating_add(1);
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "transport-admission-retry",
                Some(completion),
            );
            continue;
        }
        if cyw43_runtime_command_completion_is_progress(descriptor.op, completion) {
            if cyw43_command_stage_always_logs_success(descriptor.op) {
                let status =
                    cyw43_runtime_command_progress_status(descriptor.op, completion.detail);
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    contract,
                    DriverTaskHotPath::Cyw43Wifi,
                    stage,
                    status,
                    Some(completion),
                );
            }
            return Ok(completion);
        } else if descriptor.op == DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
            && completion.code == DriverTaskCompletionCode::FrameReady.as_u16()
        {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "ready",
                Some(completion),
            );
            return Ok(completion);
        } else {
            let status = if completion.code == DriverTaskCompletionCode::Fault.as_u16() {
                "fault"
            } else {
                "unexpected-completion"
            };
            emit_cyw43_runtime_command_fault(
                contract,
                stage,
                descriptor,
                completion,
                producer_payload,
            );
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                status,
                Some(completion),
            );
            return Err(Cyw43CommandSubmitError::Completion(completion));
        }
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_runtime_no_reply_resume_limit(op: u16) -> usize {
    match op {
        DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT => CYW43_RUNTIME_TRANSPORT_NO_REPLY_RESUMES,
        DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE => CYW43_RUNTIME_CONTROL_EXCHANGE_NO_REPLY_RESUMES,
        _ => 0,
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_runtime_descriptor_uses_prompt_slice(op: u16) -> bool {
    matches!(
        op,
        DRIVER_RUNTIME_CYW43_OP_RX_POLL | DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL
    )
}

#[cfg(feature = "kernel")]
const fn cyw43_host_eapol_poll_kind_for_op(op: u16) -> Option<Cyw43HostEapolPollKind> {
    match op {
        DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL => Some(Cyw43HostEapolPollKind::Control),
        DRIVER_RUNTIME_CYW43_OP_RX_POLL => Some(Cyw43HostEapolPollKind::Data),
        _ => None,
    }
}

#[cfg(feature = "kernel")]
fn cyw43_active_prompt_poll(contract: DriverTaskContract) -> Option<Cyw43HostEapolActivePoll> {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT {
        return None;
    }
    let active_request = crate::hal::driver_task::active_driver_task_ring_request(contract)? as u32;
    if CYW43_ACTIVE_PROMPT_POLL_REQUEST.load(Ordering::Acquire) == active_request {
        let op = CYW43_ACTIVE_PROMPT_POLL_OP.load(Ordering::Acquire) as u16;
        if let Some(kind) = cyw43_host_eapol_poll_kind_for_op(op) {
            return Some(Cyw43HostEapolActivePoll {
                kind,
                flags: CYW43_ACTIVE_PROMPT_POLL_FLAGS.load(Ordering::Acquire) as u16,
            });
        }
    }
    cyw43_active_prompt_poll_from_ring(contract, active_request)
}

#[cfg(feature = "kernel")]
fn cyw43_active_prompt_poll_from_ring(
    contract: DriverTaskContract,
    active_request: u32,
) -> Option<Cyw43HostEapolActivePoll> {
    let progress = crate::hal::driver_task::latest_driver_task_ring_progress(contract)?;
    let frame = DriverFrameDescriptor {
        offset: crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET as u32,
        len: core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>() as u16,
        flags: 0,
    };
    let bytes = crate::hal::driver_task::driver_task_ring_frame_bytes(contract, frame)?;
    let descriptor = decode_cyw43_descriptor(bytes)?;
    let active_poll =
        cyw43_active_prompt_poll_for_descriptor(active_request, Some(progress), descriptor)?;
    store_cyw43_active_prompt_poll(active_request, descriptor);
    Some(active_poll)
}

#[cfg(feature = "kernel")]
fn cyw43_active_prompt_poll_for_descriptor(
    active_request: u32,
    progress: Option<crate::hal::driver_task::DriverTaskRingProgressSnapshot>,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
) -> Option<Cyw43HostEapolActivePoll> {
    let progress = progress?;
    if active_request == 0
        || !progress.marker_valid
        || progress.sequence != active_request
        || progress.aux0 != DRIVER_RUNTIME_CYW43_COMMAND_AUX
    {
        return None;
    }
    cyw43_host_eapol_poll_kind_for_op(descriptor.op).map(|kind| Cyw43HostEapolActivePoll {
        kind,
        flags: descriptor.flags,
    })
}

#[cfg(feature = "kernel")]
fn store_cyw43_active_prompt_poll(request: u32, descriptor: DriverRuntimeCyw43CommandDescriptor) {
    CYW43_ACTIVE_PROMPT_POLL_OP.store(u32::from(descriptor.op), Ordering::Release);
    CYW43_ACTIVE_PROMPT_POLL_FLAGS.store(u32::from(descriptor.flags), Ordering::Release);
    CYW43_ACTIVE_PROMPT_POLL_REQUEST.store(request, Ordering::Release);
}

#[cfg(feature = "kernel")]
fn record_cyw43_active_prompt_poll(
    contract: DriverTaskContract,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    active_before: Option<u32>,
    completion: Option<DriverTaskCompletionRecord>,
) {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT
        || !cyw43_runtime_descriptor_uses_prompt_slice(descriptor.op)
    {
        return;
    }
    if let Some(completion) = completion {
        if CYW43_ACTIVE_PROMPT_POLL_REQUEST.load(Ordering::Acquire) == completion.sequence {
            clear_cyw43_active_prompt_poll();
        }
        return;
    }
    let active_after = crate::hal::driver_task::active_driver_task_ring_request(contract)
        .and_then(|request| u32::try_from(request).ok());
    let Some(active_after) = active_after else {
        clear_cyw43_active_prompt_poll();
        return;
    };
    let tracked_request = CYW43_ACTIVE_PROMPT_POLL_REQUEST.load(Ordering::Acquire);
    if active_before != Some(active_after)
        || (tracked_request == active_after
            && CYW43_ACTIVE_PROMPT_POLL_OP.load(Ordering::Acquire) as u16 == descriptor.op
            && CYW43_ACTIVE_PROMPT_POLL_FLAGS.load(Ordering::Acquire) as u16 == descriptor.flags)
    {
        store_cyw43_active_prompt_poll(active_after, descriptor);
    } else if cyw43_active_prompt_poll_from_ring(contract, active_after).is_none() {
        clear_cyw43_active_prompt_poll();
    }
}

#[cfg(feature = "kernel")]
fn clear_cyw43_active_prompt_poll() {
    CYW43_ACTIVE_PROMPT_POLL_REQUEST.store(0, Ordering::Release);
    CYW43_ACTIVE_PROMPT_POLL_OP.store(0, Ordering::Release);
    CYW43_ACTIVE_PROMPT_POLL_FLAGS.store(0, Ordering::Release);
}

#[cfg(feature = "kernel")]
fn record_cyw43_runtime_command_no_reply(
    contract: DriverTaskContract,
    stage: &'static str,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    resumes: usize,
) {
    use core::fmt::Write;

    *CYW43_LAST_RUNTIME_COMMAND_FAULT.lock() = Some(Cyw43RuntimeCommandFaultStatus {
        stage,
        op: descriptor.op,
        flags: descriptor.flags,
        target_addr: descriptor.target_addr,
        payload_offset: descriptor.payload_offset,
        payload_len: descriptor.payload_len,
        total_len: descriptor.total_len,
        control_cmd: cyw43_descriptor_control_cmd(descriptor),
        control_id: cyw43_descriptor_control_id(descriptor),
        control_header_mode: cyw43_descriptor_control_header_mode(descriptor),
        control_response_len: cyw43_control_request_expected_response_len(
            cyw43_descriptor_control_cmd(descriptor),
            None,
        ) as u16,
        detail: 0,
        reason: "cyw43-runtime-command-no-reply",
        result: 0,
    });
    *CYW43_LAST_SDIO_OWNER_FAULT.lock() = None;
    let request = crate::hal::driver_task::current_driver_task_ring_request(contract);
    let progress = crate::hal::driver_task::latest_driver_task_ring_progress(contract);
    let control_cmd = cyw43_descriptor_control_cmd(descriptor);
    let control_id = cyw43_descriptor_control_id(descriptor);
    let control_header_mode = cyw43_descriptor_control_header_mode(descriptor);
    let control_response_len =
        cyw43_control_request_expected_response_len(control_cmd, None) as u16;
    let mut line = heapless::String::<768>::new();
    match (request, progress) {
        (Some(request), Some(progress)) => {
            let _ = write!(
                line,
                "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract={} stage={} op={} flags=0x{:04x} target=0x{:08x} payload_off={} payload_len={} total_len={} control_cmd={} control_cmd_hex=0x{:08x} control_id={} control_header_mode={} control_response_len={} reason=cyw43-runtime-command-no-reply request={} resumes={} progress_marker_valid={} progress_sequence={} progress_phase={} progress_phase_name={} progress_aux0=0x{:08x}",
                contract.name,
                stage,
                descriptor.op,
                descriptor.flags,
                descriptor.target_addr,
                descriptor.payload_offset,
                descriptor.payload_len,
                descriptor.total_len,
                control_cmd,
                control_cmd,
                control_id,
                control_header_mode,
                control_response_len,
                request,
                resumes,
                if progress.marker_valid { "yes" } else { "no" },
                progress.sequence,
                progress.phase,
                progress.phase_name,
                progress.aux0,
            );
        }
        (Some(request), None) => {
            let _ = write!(
                line,
                "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract={} stage={} op={} flags=0x{:04x} target=0x{:08x} payload_off={} payload_len={} total_len={} control_cmd={} control_cmd_hex=0x{:08x} control_id={} control_header_mode={} control_response_len={} reason=cyw43-runtime-command-no-reply request={} resumes={} progress_marker_valid=no progress_sequence=0 progress_phase=0 progress_phase_name=none progress_aux0=0x00000000",
                contract.name,
                stage,
                descriptor.op,
                descriptor.flags,
                descriptor.target_addr,
                descriptor.payload_offset,
                descriptor.payload_len,
                descriptor.total_len,
                control_cmd,
                control_cmd,
                control_id,
                control_header_mode,
                control_response_len,
                request,
                resumes,
            );
        }
        (None, Some(progress)) => {
            let _ = write!(
                line,
                "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract={} stage={} op={} flags=0x{:04x} target=0x{:08x} payload_off={} payload_len={} total_len={} control_cmd={} control_cmd_hex=0x{:08x} control_id={} control_header_mode={} control_response_len={} reason=cyw43-runtime-command-no-reply request=none resumes={} progress_marker_valid={} progress_sequence={} progress_phase={} progress_phase_name={} progress_aux0=0x{:08x}",
                contract.name,
                stage,
                descriptor.op,
                descriptor.flags,
                descriptor.target_addr,
                descriptor.payload_offset,
                descriptor.payload_len,
                descriptor.total_len,
                control_cmd,
                control_cmd,
                control_id,
                control_header_mode,
                control_response_len,
                resumes,
                if progress.marker_valid { "yes" } else { "no" },
                progress.sequence,
                progress.phase,
                progress.phase_name,
                progress.aux0,
            );
        }
        (None, None) => {
            let _ = write!(
                line,
                "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract={} stage={} op={} flags=0x{:04x} target=0x{:08x} payload_off={} payload_len={} total_len={} control_cmd={} control_cmd_hex=0x{:08x} control_id={} control_header_mode={} control_response_len={} reason=cyw43-runtime-command-no-reply request=none resumes={} progress_marker_valid=no progress_sequence=0 progress_phase=0 progress_phase_name=none progress_aux0=0x00000000",
                contract.name,
                stage,
                descriptor.op,
                descriptor.flags,
                descriptor.target_addr,
                descriptor.payload_offset,
                descriptor.payload_len,
                descriptor.total_len,
                control_cmd,
                control_cmd,
                control_id,
                control_header_mode,
                control_response_len,
                resumes,
            );
        }
    }
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(not(feature = "kernel"))]
fn record_cyw43_runtime_command_no_reply(
    _contract: DriverTaskContract,
    _stage: &'static str,
    _descriptor: DriverRuntimeCyw43CommandDescriptor,
    _resumes: usize,
) {
}

#[cfg(feature = "kernel")]
fn emit_cyw43_runtime_command_fault(
    contract: DriverTaskContract,
    stage: &'static str,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    completion: DriverTaskCompletionRecord,
    producer_payload: Option<&[u8]>,
) {
    use core::fmt::Write;

    if completion.code != DriverTaskCompletionCode::Fault.as_u16() {
        return;
    }
    let reason = cyw43_runtime_fault_reason_for_descriptor(descriptor, completion);
    *CYW43_LAST_RUNTIME_COMMAND_FAULT.lock() = Some(Cyw43RuntimeCommandFaultStatus {
        stage,
        op: descriptor.op,
        flags: descriptor.flags,
        target_addr: descriptor.target_addr,
        payload_offset: descriptor.payload_offset,
        payload_len: descriptor.payload_len,
        total_len: descriptor.total_len,
        control_cmd: cyw43_descriptor_control_cmd(descriptor),
        control_id: cyw43_descriptor_control_id(descriptor),
        control_header_mode: cyw43_descriptor_control_header_mode(descriptor),
        control_response_len: cyw43_control_request_expected_response_len(
            cyw43_descriptor_control_cmd(descriptor),
            None,
        ) as u16,
        detail: completion.detail,
        reason,
        result: completion.result,
    });
    *CYW43_LAST_SDIO_OWNER_FAULT.lock() = None;
    let control_cmd = cyw43_descriptor_control_cmd(descriptor);
    let control_id = cyw43_descriptor_control_id(descriptor);
    let control_header_mode = cyw43_descriptor_control_header_mode(descriptor);
    let control_response_len =
        cyw43_control_request_expected_response_len(control_cmd, None) as u16;
    let mut line = heapless::String::<512>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_COMMAND_FAULT contract={} stage={} op={} flags=0x{:04x} target=0x{:08x} payload_off={} payload_len={} total_len={} control_cmd={} control_cmd_hex=0x{:08x} control_id={} control_header_mode={} control_response_len={} detail={} reason={} result={}",
        contract.name,
        stage,
        descriptor.op,
        descriptor.flags,
        descriptor.target_addr,
        descriptor.payload_offset,
        descriptor.payload_len,
        descriptor.total_len,
        control_cmd,
        control_cmd,
        control_id,
        control_header_mode,
        control_response_len,
        completion.detail,
        reason,
        completion.result,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
    emit_cyw43_sdio_owner_fault_snapshot(contract, stage, descriptor, completion, producer_payload);
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SdioFaultTelemetry {
    arg: u32,
    cmd: u16,
    flags: u16,
    len: u16,
    block_size: u16,
    block_count: u16,
    transfer_mode: u16,
    present_state: u32,
    int_status: u32,
    response0: u32,
    host_control: u8,
    power_control: u8,
    clock_control: u16,
    failure_result: u32,
    block_size_count_reg: u32,
    payload_first: u8,
    payload_last: u8,
    payload_xor: u8,
    payload_sum: u32,
}

#[cfg(feature = "kernel")]
impl SdioFaultTelemetry {
    fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < SDIO_FAULT_TELEMETRY_BYTES {
            return None;
        }
        let magic = le_u32_at(bytes, 0)?;
        let version = le_u32_at(bytes, 4)?;
        if magic != SDIO_FAULT_TELEMETRY_MAGIC || version != SDIO_FAULT_TELEMETRY_VERSION {
            return None;
        }
        let cmd_flags = le_u32_at(bytes, SDIO_FAULT_TELEMETRY_CMD_FLAGS_OFFSET)?;
        let len_block = le_u32_at(bytes, SDIO_FAULT_TELEMETRY_LEN_BLOCK_OFFSET)?;
        let count_mode = le_u32_at(bytes, SDIO_FAULT_TELEMETRY_COUNT_MODE_OFFSET)?;
        let host_clock = le_u32_at(bytes, SDIO_FAULT_TELEMETRY_HOST_CLOCK_OFFSET)?;
        let payload_edge = le_u32_at(bytes, SDIO_FAULT_TELEMETRY_PAYLOAD_EDGE_OFFSET)?;
        Some(Self {
            arg: le_u32_at(bytes, SDIO_FAULT_TELEMETRY_ARG_OFFSET)?,
            cmd: (cmd_flags & 0xffff) as u16,
            flags: (cmd_flags >> 16) as u16,
            len: (len_block & 0xffff) as u16,
            block_size: (len_block >> 16) as u16,
            block_count: (count_mode & 0xffff) as u16,
            transfer_mode: (count_mode >> 16) as u16,
            present_state: le_u32_at(bytes, SDIO_FAULT_TELEMETRY_PRESENT_OFFSET)?,
            int_status: le_u32_at(bytes, SDIO_FAULT_TELEMETRY_INT_STATUS_OFFSET)?,
            response0: le_u32_at(bytes, SDIO_FAULT_TELEMETRY_RESPONSE0_OFFSET)?,
            host_control: (host_clock & 0xff) as u8,
            power_control: ((host_clock >> 8) & 0xff) as u8,
            clock_control: (host_clock >> 16) as u16,
            failure_result: le_u32_at(bytes, SDIO_FAULT_TELEMETRY_FAILURE_OFFSET)?,
            block_size_count_reg: le_u32_at(bytes, SDIO_FAULT_TELEMETRY_BLOCK_REG_OFFSET)?,
            payload_first: (payload_edge & 0xff) as u8,
            payload_last: ((payload_edge >> 8) & 0xff) as u8,
            payload_xor: ((payload_edge >> 16) & 0xff) as u8,
            payload_sum: le_u32_at(bytes, SDIO_FAULT_TELEMETRY_PAYLOAD_SUM_OFFSET)?,
        })
    }

    const fn cmd53_write(self) -> bool {
        self.arg & (1 << 31) != 0
    }

    const fn cmd53_function(self) -> u8 {
        ((self.arg >> 28) & 0x7) as u8
    }

    const fn cmd53_block_mode(self) -> bool {
        self.arg & (1 << 27) != 0
    }

    const fn cmd53_increment(self) -> bool {
        self.arg & (1 << 26) != 0
    }

    const fn cmd53_addr(self) -> u32 {
        (self.arg >> 9) & 0x1ffff
    }
}

#[cfg(feature = "kernel")]
fn emit_cyw43_sdio_owner_fault_snapshot(
    contract: DriverTaskContract,
    stage: &'static str,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    completion: DriverTaskCompletionRecord,
    producer_payload: Option<&[u8]>,
) {
    use core::fmt::Write;

    let Some(bytes) =
        crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)
    else {
        return;
    };
    let Some(snapshot) = SdioFaultTelemetry::decode(bytes) else {
        return;
    };
    let result = if snapshot.failure_result != 0 {
        snapshot.failure_result
    } else {
        completion.result
    };
    let effective_target = cyw43_owner_effective_backplane_target(descriptor, snapshot);
    let owner_suboffset = effective_target
        .and_then(|target| target.checked_sub(descriptor.target_addr))
        .map(|offset| offset as usize);
    let owner_payload_offset = owner_suboffset
        .and_then(|offset| u32::from(descriptor.payload_offset).checked_add(offset as u32));
    let owner_window = cyw43_owner_window_label(descriptor);
    let retry = cyw43_owner_retry_label(snapshot, descriptor, completion.detail);
    *CYW43_LAST_SDIO_OWNER_FAULT.lock() = Some(Cyw43SdioOwnerFaultStatus {
        stage,
        op: descriptor.op,
        cmd: snapshot.cmd,
        arg: snapshot.arg,
        function: snapshot.cmd53_function(),
        addr: snapshot.cmd53_addr(),
        target_addr: descriptor.target_addr,
        effective_target: effective_target.unwrap_or(0),
        chunk_offset: owner_suboffset
            .and_then(|offset| u32::try_from(offset).ok())
            .unwrap_or(u32::MAX),
        payload_offset: owner_payload_offset.unwrap_or(u32::MAX),
        increment: snapshot.cmd53_increment(),
        write: snapshot.cmd53_write(),
        block_mode: snapshot.cmd53_block_mode(),
        len: snapshot.len,
        block_size: snapshot.block_size,
        block_count: snapshot.block_count,
        transfer_mode: snapshot.transfer_mode,
        host_control: snapshot.host_control,
        power_control: snapshot.power_control,
        clock_control: snapshot.clock_control,
        present_state: snapshot.present_state,
        int_status: snapshot.int_status,
        response0: snapshot.response0,
        block_size_count_reg: snapshot.block_size_count_reg,
        detail: completion.detail,
        reason: cyw43_runtime_fault_reason(completion.detail),
        transfer_stage: sdio_transfer_failure_stage_label(result),
        transfer_status: sdio_transfer_failure_status(result),
        transfer_reason: sdio_transfer_failure_reason_label(result),
        r5: sdio_transfer_failure_r5(result),
        owner_window,
        retry,
        payload_first: snapshot.payload_first,
        payload_last: snapshot.payload_last,
        payload_xor: snapshot.payload_xor,
        payload_sum: snapshot.payload_sum,
    });
    let mut line = heapless::String::<896>::new();
    let _ = write!(
        line,
        "CYW43_SDIO_OWNER_FAULT contract={} stage={} op={} cmd={} arg=0x{:08x} fn={} win=0x{:05x} target=0x{:08x} effective=0x{:08x} chunk_off={} payload_off={} inc={} write={} mode={} len={} blksz={} blkcnt={} tm=0x{:04x} host=0x{:02x} power=0x{:02x} clock=0x{:04x} present=0x{:08x} int=0x{:08x} resp0=0x{:08x} blkreg=0x{:08x} detail=0x{:04x} reason={} xfer_stage={} xfer_status=0x{:06x} xfer_reason={} r5=0x{:04x} owner_window={} retry={}",
        contract.name,
        stage,
        descriptor.op,
        snapshot.cmd,
        snapshot.arg,
        snapshot.cmd53_function(),
        snapshot.cmd53_addr(),
        descriptor.target_addr,
        effective_target.unwrap_or(0),
        owner_suboffset.unwrap_or(usize::MAX),
        owner_payload_offset.unwrap_or(u32::MAX),
        yes_no(snapshot.cmd53_increment()),
        yes_no(snapshot.cmd53_write()),
        sdio_fault_transfer_mode_label(snapshot),
        snapshot.len,
        snapshot.block_size,
        snapshot.block_count,
        snapshot.transfer_mode,
        snapshot.host_control,
        snapshot.power_control,
        snapshot.clock_control,
        snapshot.present_state,
        snapshot.int_status,
        snapshot.response0,
        snapshot.block_size_count_reg,
        completion.detail,
        cyw43_runtime_fault_reason(completion.detail),
        sdio_transfer_failure_stage_label(result),
        sdio_transfer_failure_status(result),
        sdio_transfer_failure_reason_label(result),
        sdio_transfer_failure_r5(result),
        owner_window,
        retry,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
    let mut payload_line = heapless::String::<256>::new();
    let _ = write!(
        payload_line,
        "CYW43_SDIO_OWNER_PAYLOAD contract={} stage={} target=0x{:08x} effective=0x{:08x} chunk_off={} payload_off={} len={} first=0x{:02x} last=0x{:02x} xor=0x{:02x} sum=0x{:08x}",
        contract.name,
        stage,
        descriptor.target_addr,
        effective_target.unwrap_or(0),
        owner_suboffset.unwrap_or(usize::MAX),
        owner_payload_offset.unwrap_or(u32::MAX),
        snapshot.len,
        snapshot.payload_first,
        snapshot.payload_last,
        snapshot.payload_xor,
        snapshot.payload_sum,
    );
    crate::bootstrap::log::force_uart_line_raw(payload_line.as_str());
    emit_cyw43_sdio_payload_compare(
        contract,
        stage,
        descriptor,
        snapshot,
        producer_payload,
        owner_suboffset,
    );
}

#[cfg(feature = "kernel")]
fn cyw43_owner_effective_backplane_target(
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    snapshot: SdioFaultTelemetry,
) -> Option<u32> {
    if snapshot.cmd != 53
        || snapshot.cmd53_function() != 1
        || snapshot.cmd53_addr() & CYW43_BACKPLANE_32BIT_FLAG == 0
    {
        return None;
    }
    Some(
        (descriptor.target_addr & CYW43_BACKPLANE_WINDOW_MASK)
            | (snapshot.cmd53_addr() & CYW43_BACKPLANE_ADDRESS_MASK),
    )
}

#[cfg(feature = "kernel")]
fn emit_cyw43_sdio_payload_compare(
    contract: DriverTaskContract,
    stage: &'static str,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    snapshot: SdioFaultTelemetry,
    producer_payload: Option<&[u8]>,
    owner_suboffset: Option<usize>,
) {
    use core::fmt::Write;

    let Some(payload) = producer_payload else {
        return;
    };
    let Some(offset) = owner_suboffset else {
        return;
    };
    let len = usize::from(snapshot.len);
    let Some(end) = offset.checked_add(len) else {
        return;
    };
    if end > payload.len() {
        return;
    }
    let digest = cyw43_payload_digest(&payload[offset..end]);
    let matched = digest.first == snapshot.payload_first
        && digest.last == snapshot.payload_last
        && digest.xor == snapshot.payload_xor
        && digest.sum == snapshot.payload_sum;
    let mut line = heapless::String::<384>::new();
    let _ = write!(
        line,
        "CYW43_SDIO_PAYLOAD_CMP contract={} stage={} op={} target=0x{:08x} off={} len={} status={} pf=0x{:02x} pl=0x{:02x} px=0x{:02x} ps=0x{:08x} of=0x{:02x} ol=0x{:02x} ox=0x{:02x} os=0x{:08x}",
        contract.name,
        stage,
        descriptor.op,
        descriptor.target_addr,
        offset,
        len,
        if matched { "match" } else { "mismatch" },
        digest.first,
        digest.last,
        digest.xor,
        digest.sum,
        snapshot.payload_first,
        snapshot.payload_last,
        snapshot.payload_xor,
        snapshot.payload_sum,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
const fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

#[cfg(feature = "kernel")]
const fn sdio_fault_transfer_mode_label(snapshot: SdioFaultTelemetry) -> &'static str {
    if snapshot.cmd != 53 {
        "non-cmd53"
    } else if snapshot.cmd53_block_mode() {
        "block"
    } else if snapshot.len == SDIO_CMD53_BYTE_MODE_MAX {
        "byte512"
    } else {
        "byte"
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_owner_window_label(descriptor: DriverRuntimeCyw43CommandDescriptor) -> &'static str {
    match descriptor.op {
        DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK | DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK => {
            "sdio-shared-8192"
        }
        DRIVER_RUNTIME_CYW43_OP_ETH_TX
        | DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
        | DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
        | DRIVER_RUNTIME_CYW43_OP_RX_POLL
        | DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL => "function2-fifo",
        _ => "unknown",
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_owner_retry_label(
    snapshot: SdioFaultTelemetry,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    detail: u16,
) -> &'static str {
    if descriptor.flags & DRIVER_RUNTIME_CYW43_FLAG_FORCE_BYTE_MODE != 0 {
        if snapshot.host_control & 0x06 == 0 {
            "forced-byte-mode-conservative"
        } else {
            "forced-byte-mode-promoted"
        }
    } else if !snapshot.cmd53_block_mode() {
        if snapshot.host_control & 0x06 == 0
            && detail == 0x5329
            && snapshot.len < SDIO_CMD53_BYTE_MODE_MAX
        {
            "byte-narrow-conservative-exhausted"
        } else if snapshot.host_control & 0x06 == 0 && detail == 0x5329 {
            "byte-conservative-exhausted"
        } else if snapshot.host_control & 0x06 == 0 && snapshot.len < SDIO_CMD53_BYTE_MODE_MAX {
            "byte-narrow-conservative"
        } else if snapshot.host_control & 0x06 == 0 {
            "byte-conservative"
        } else if detail == 0x5329 && snapshot.len < SDIO_CMD53_BYTE_MODE_MAX {
            "byte-narrow-fallback-exhausted"
        } else if detail == 0x5329 {
            "byte-fallback-exhausted"
        } else if snapshot.len < SDIO_CMD53_BYTE_MODE_MAX {
            "byte-narrow-fallback"
        } else {
            "byte-fallback"
        }
    } else if detail == 0x5329 {
        "block-retry-exhausted"
    } else if snapshot.host_control & 0x04 == 0 || snapshot.clock_control != 0x5007 {
        "block-clock-retry"
    } else {
        "primary"
    }
}

#[cfg(feature = "kernel")]
const fn sdio_transfer_failure_stage_label(result: u32) -> &'static str {
    match (result >> 24) & 0xff {
        1 => "inhibit",
        2 => "command",
        3 => "data-wait",
        4 => "data-end",
        5 => "response",
        _ => "unknown",
    }
}

#[cfg(feature = "kernel")]
const fn sdio_transfer_failure_status(result: u32) -> u32 {
    result & 0x00ff_ffff
}

#[cfg(feature = "kernel")]
const fn sdio_transfer_finish_error_label(status: u32) -> &'static str {
    if status & SDHCI_INT_TIMEOUT != 0 {
        "sdhci-transfer-finish-timeout"
    } else if status & SDHCI_INT_CRC != 0 {
        "sdhci-transfer-finish-crc"
    } else if status & SDHCI_INT_END_BIT != 0 {
        "sdhci-transfer-finish-end-bit"
    } else if status & SDHCI_INT_INDEX != 0 {
        "sdhci-transfer-finish-index"
    } else if status & SDHCI_INT_DATA_TIMEOUT != 0 {
        "sdhci-transfer-finish-data-timeout"
    } else if status & SDHCI_INT_DATA_CRC != 0 {
        "sdhci-transfer-finish-data-crc"
    } else if status & SDHCI_INT_DATA_END_BIT != 0 {
        "sdhci-transfer-finish-data-end-bit"
    } else if status & SDHCI_INT_ERROR != 0 {
        "sdhci-transfer-finish-error"
    } else {
        "sdhci-transfer-finish"
    }
}

#[cfg(feature = "kernel")]
const fn sdio_transfer_failure_reason_label(result: u32) -> &'static str {
    let status = sdio_transfer_failure_status(result);
    match (result >> 24) & 0xff {
        3 => {
            if status & SDHCI_INT_DATA_CRC != 0 {
                "sdhci-transfer-data-crc"
            } else if status & SDHCI_INT_DATA_TIMEOUT != 0 {
                "sdhci-transfer-data-timeout"
            } else if status & SDHCI_INT_ERROR != 0 {
                "sdhci-transfer-data-error"
            } else {
                "sdhci-transfer-data"
            }
        }
        4 => sdio_transfer_finish_error_label(status),
        5 => "sdio-r5-response",
        2 => "sdhci-command",
        1 => "sdhci-inhibit",
        _ => "unknown",
    }
}

#[cfg(feature = "kernel")]
const fn sdio_transfer_failure_r5(result: u32) -> u32 {
    if ((result >> 24) & 0xff) == 5 {
        result & 0xffff
    } else {
        0
    }
}

#[cfg(feature = "kernel")]
fn le_u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let slice = bytes.get(offset..end)?;
    Some(
        u32::from(slice[0])
            | (u32::from(slice[1]) << 8)
            | (u32::from(slice[2]) << 16)
            | (u32::from(slice[3]) << 24),
    )
}

#[cfg(feature = "kernel")]
fn le_u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let slice = bytes.get(offset..end)?;
    Some(u16::from(slice[0]) | (u16::from(slice[1]) << 8))
}

#[cfg(feature = "kernel")]
const fn cyw43_fault_detail_allows_sdio_owner_recovery(detail: u16) -> bool {
    crate::cyw43_recovery::fault_detail_allows_sdio_owner_recovery(detail)
}

#[cfg(feature = "kernel")]
const fn cyw43_fault_detail_allows_same_command_retry(detail: u16) -> bool {
    crate::cyw43_recovery::fault_detail_allows_same_command_retry(detail)
}

#[cfg(feature = "kernel")]
pub(crate) const fn cyw43_runtime_fault_reason(detail: u16) -> &'static str {
    match detail {
        0x0001 => "rejected-command",
        0x5101 => "sdio-command-unavailable",
        0x5102 => "sdio-descriptor-unavailable",
        0x5103 => "sdio-descriptor-transfer-failed",
        0x5104 => "sdio-host-config-failed",
        0x5301 => "cyw43-transport-init",
        0x5302 => "cyw43-firmware-chunk",
        0x5303 => "cyw43-nvram-chunk",
        0x5304 => "cyw43-nvram-tail",
        0x5305 => "cyw43-release",
        0x5306 => "cyw43-control-frame",
        0x5307 => "cyw43-eth-tx",
        0x5308 => "cyw43-firmware-prep",
        0x5309 => "cyw43-descriptor-unavailable",
        0x530a => "cyw43-descriptor-invalid",
        CYW43_CONTROL_EXCHANGE_FAULT_DETAIL => "cyw43-control-exchange",
        0x5310 => "cyw43-transport-bus-link-missing",
        0x5311 => "cyw43-transport-direct-sdio-init",
        0x5312 => "cyw43-transport-card-init",
        0x5313 => "cyw43-transport-f1-block-size",
        0x5314 => "cyw43-transport-f2-block-size",
        0x5315 => "cyw43-transport-f1-enable",
        0x5316 => "cyw43-transport-card-bus-width",
        0x5317 => "cyw43-transport-host-bus-width",
        0x5319 => "cyw43-transport-high-speed",
        0x531a => "cyw43-backplane-alp",
        0x531b => "cyw43-backplane-wake",
        0x531c => "cyw43-backplane-kso",
        0x531d => "cyw43-backplane-watermark",
        0x531e => "cyw43-backplane-device-control",
        0x531f => "cyw43-backplane-armcr4-reset",
        0x5320 => "cyw43-firmware-range",
        0x5321 => "cyw43-backplane-window",
        0x5322 => "cyw43-post-release-cardcap",
        0x5323 => "cyw43-backplane-chipcommon-read",
        0x5324 => "cyw43-transport-card-cmd0",
        0x5325 => "cyw43-transport-card-cmd5-ocr",
        0x5326 => "cyw43-transport-card-cmd5-ready",
        0x5327 => "cyw43-transport-card-cmd3-rca",
        0x5328 => "cyw43-transport-card-cmd7-select",
        0x5329 => "cyw43-firmware-retry-exhausted",
        0x532a => "cyw43-post-release-ht-clock",
        0x532b => "cyw43-post-release-function2-ready",
        0x532c => "cyw43-post-release-corecontrol",
        0x532d => "cyw43-post-release-mailbox-ready",
        0x532e => "cyw43-post-release-protocol-version",
        0x53ff => "cyw43-command",
        DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_START => "cyw43-transport-phase-start",
        DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BUS_LINK_READY => {
            "cyw43-transport-phase-bus-link-ready"
        }
        DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_CARD_READY => "cyw43-transport-phase-card-ready",
        DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F1_BLOCK_READY => {
            "cyw43-transport-phase-f1-block-ready"
        }
        DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F2_BLOCK_READY => {
            "cyw43-transport-phase-f2-block-ready"
        }
        DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F1_ENABLED => "cyw43-transport-phase-f1-enabled",
        DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_HOST_READY => "cyw43-transport-phase-host-ready",
        DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BACKPLANE_READY => {
            "cyw43-transport-phase-backplane-ready"
        }
        DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_READY => "cyw43-transport-phase-ready",
        _ => "unknown",
    }
}

#[cfg(feature = "kernel")]
fn cyw43_runtime_fault_reason_for_descriptor(
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    completion: DriverTaskCompletionRecord,
) -> &'static str {
    if descriptor.op == DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT
        && completion.detail == DriverTaskFaultCode::RejectedCommand.as_u16()
    {
        "cyw43-transport-command-admission"
    } else {
        cyw43_runtime_fault_reason(completion.detail)
    }
}

#[cfg(feature = "kernel")]
fn cyw43_transport_admission_reject_without_current_progress(
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    completion: DriverTaskCompletionRecord,
    progress: Option<DriverTaskRingProgressSnapshot>,
) -> bool {
    if descriptor.op != DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT
        || completion.code != DriverTaskCompletionCode::Fault.as_u16()
        || completion.detail != DriverTaskFaultCode::RejectedCommand.as_u16()
    {
        return false;
    }
    match progress {
        Some(progress) => {
            !(progress.marker_valid
                && progress.sequence == completion.sequence
                && progress.aux0 == DRIVER_RUNTIME_CYW43_COMMAND_AUX)
        }
        None => true,
    }
}

#[cfg(feature = "kernel")]
fn encode_cyw43_descriptor(out: &mut [u8], descriptor: DriverRuntimeCyw43CommandDescriptor) {
    put_le_u16(out, 0, descriptor.op);
    put_le_u16(out, 2, descriptor.flags);
    put_le_u32(out, 4, descriptor.target_addr);
    put_le_u16(out, 8, descriptor.payload_offset);
    put_le_u16(out, 10, descriptor.payload_len);
    put_le_u32(out, 12, descriptor.total_len);
    put_le_u32(out, 16, descriptor.arg0);
    put_le_u32(out, 20, descriptor.arg1);
    put_le_u32(out, 24, descriptor.reserved);
}

#[cfg(feature = "kernel")]
fn decode_cyw43_descriptor(bytes: &[u8]) -> Option<DriverRuntimeCyw43CommandDescriptor> {
    if bytes.len() < core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>() {
        return None;
    }
    Some(DriverRuntimeCyw43CommandDescriptor {
        op: le_u16_at(bytes, 0)?,
        flags: le_u16_at(bytes, 2)?,
        target_addr: le_u32_at(bytes, 4)?,
        payload_offset: le_u16_at(bytes, 8)?,
        payload_len: le_u16_at(bytes, 10)?,
        total_len: le_u32_at(bytes, 12)?,
        arg0: le_u32_at(bytes, 16)?,
        arg1: le_u32_at(bytes, 20)?,
        reserved: le_u32_at(bytes, 24)?,
    })
}

#[cfg(test)]
fn encode_sdio_descriptor(
    out: &mut [u8],
    descriptor: pi4_driver_abi::DriverRuntimeSdioCommandDescriptor,
) {
    put_le_u16(out, 0, descriptor.op);
    out[2] = descriptor.function;
    out[3] = descriptor.response_kind;
    put_le_u32(out, 4, descriptor.addr);
    put_le_u16(out, 8, descriptor.data_offset);
    put_le_u16(out, 10, descriptor.len);
    put_le_u16(out, 12, descriptor.block_size);
    put_le_u16(out, 14, descriptor.block_count);
    put_le_u16(out, 16, descriptor.flags);
    put_le_u16(out, 18, descriptor.reserved);
    put_le_u32(out, 20, descriptor.timeout_us);
}

#[cfg(feature = "kernel")]
fn put_le_u16(out: &mut [u8], offset: usize, value: u16) {
    out[offset] = (value & 0xff) as u8;
    out[offset + 1] = (value >> 8) as u8;
}

#[cfg(feature = "kernel")]
fn put_le_u32(out: &mut [u8], offset: usize, value: u32) {
    out[offset] = (value & 0xff) as u8;
    out[offset + 1] = ((value >> 8) & 0xff) as u8;
    out[offset + 2] = ((value >> 16) & 0xff) as u8;
    out[offset + 3] = ((value >> 24) & 0xff) as u8;
}

#[cfg(feature = "kernel")]
fn firmware_reset_vector(firmware: &[u8]) -> Option<u32> {
    let bytes = firmware.get(0..4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Service one pointer-free NIC runtime command from the driver-task ring.
pub fn service_runtime_command(
    hot_path: DriverTaskHotPath,
    command: DriverTaskCommandRecord,
) -> DriverTaskCompletionRecord {
    if command.aux0 == DRIVER_RUNTIME_NET_INIT_AUX {
        return service_runtime_init_command(hot_path, command);
    }
    match hot_path {
        DriverTaskHotPath::GenetNic => service_genet(command),
        DriverTaskHotPath::Cyw43Wifi => service_cyw43(command),
        _ => DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::RejectedCommand,
        ),
    }
}

/// Shared-ring service entry for GENET/CYW43 runtime owners.
#[cfg(feature = "kernel")]
pub unsafe fn runtime_ring_service(
    context: usize,
    command: DriverTaskCommandRecord,
) -> DriverTaskCompletionRecord {
    let hot_path = if context == DriverTaskHotPath::GenetNic.as_u32() as usize {
        DriverTaskHotPath::GenetNic
    } else if context == DriverTaskHotPath::Cyw43Wifi.as_u32() as usize {
        DriverTaskHotPath::Cyw43Wifi
    } else {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::RejectedCommand,
        );
    };
    if command.opcode != hot_path.opcode().as_u16()
        || command.arg0 != hot_path.as_u32()
        || command.arg1 != hot_path.role_bit() as u32
    {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::RejectedCommand,
        );
    }
    service_runtime_command(hot_path, command)
}

fn service_runtime_init_command(
    hot_path: DriverTaskHotPath,
    command: DriverTaskCommandRecord,
) -> DriverTaskCompletionRecord {
    let _ = hot_path;
    if command.frame.len != 0 {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::RejectedCommand,
        );
    }
    DriverTaskCompletionRecord::fault(command.sequence, DriverTaskFaultCode::DeviceUnavailable)
}

fn service_genet(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    DriverTaskCompletionRecord::fault(command.sequence, DriverTaskFaultCode::DeviceUnavailable)
}

fn service_cyw43(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    DriverTaskCompletionRecord::fault(command.sequence, DriverTaskFaultCode::DeviceUnavailable)
}

fn runtime_ready(hot_path: DriverTaskHotPath) -> bool {
    match hot_path {
        DriverTaskHotPath::GenetNic => GENET_LINKED_RUNTIME_READY.load(Ordering::Acquire) != 0,
        DriverTaskHotPath::Cyw43Wifi => CYW43_LINKED_RUNTIME_READY.load(Ordering::Acquire) != 0,
        _ => false,
    }
}

fn cyw43_data_plane_ready() -> bool {
    CYW43_ASSOCIATED.load(Ordering::Acquire) != 0
        && CYW43_LINK_UP.load(Ordering::Acquire) != 0
        && CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) != 0
}

fn cyw43_driver_task_bringup_status_label() -> Option<&'static str> {
    if !runtime_ready(DriverTaskHotPath::Cyw43Wifi) {
        return Some(DRIVER_TASK_NET_STATUS);
    }
    if cyw43_data_plane_ready() {
        return None;
    }
    if CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) != 0 {
        return Some("wifi-link-down");
    }
    if CYW43_HOST_EAPOL_REQUIRED.load(Ordering::Acquire) != 0 {
        return Some("wifi-host-eapol-required");
    }
    if CYW43_HOST_EAPOL_ACTIVE.load(Ordering::Acquire) != 0 {
        return Some("wifi-host-eapol-pending");
    }
    if CYW43_LINK_UP.load(Ordering::Acquire) != 0 || CYW43_ASSOCIATED.load(Ordering::Acquire) != 0 {
        return Some("wifi-host-eapol-pending");
    }
    if CYW43_CONTROL_PLANE_READY.load(Ordering::Acquire) != 0 {
        return Some("wifi-associating");
    }
    Some("wifi-associating")
}

fn runtime_mac(hot_path: DriverTaskHotPath) -> Option<EthernetAddress> {
    match hot_path {
        DriverTaskHotPath::GenetNic => Some(GENET_DRIVER_TASK_MAC),
        DriverTaskHotPath::Cyw43Wifi => Some(*CYW43_RUNTIME_MAC.lock()),
        _ => None,
    }
}

/// RX token backed by a frame copied out of the driver-task shared ring.
pub struct DriverTaskNetRxToken {
    len: usize,
    buffer: [u8; MAX_FRAME_LEN],
}

impl phy::RxToken for DriverTaskNetRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.buffer[..self.len])
    }
}

/// TX token that stages one frame into the driver-task shared ring.
pub struct DriverTaskNetTxToken {
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
    tx_submitted: &'static AtomicU32,
    tx_dropped: &'static AtomicU32,
}

impl phy::TxToken for DriverTaskNetTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut scratch = [0u8; MAX_FRAME_LEN];
        let frame_len = len.min(MAX_FRAME_LEN);
        let result = f(&mut scratch[..frame_len]);
        if submit_driver_task_frame(self.contract, self.hot_path, &scratch[..frame_len]) {
            self.tx_submitted.fetch_add(1, Ordering::AcqRel);
        } else {
            self.tx_dropped.fetch_add(1, Ordering::AcqRel);
        }
        result
    }
}

#[cfg(feature = "kernel")]
fn submit_driver_task_frame(
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
    frame: &[u8],
) -> bool {
    if hot_path == DriverTaskHotPath::Cyw43Wifi {
        return submit_cyw43_driver_task_eth_frame(contract, frame);
    }
    let Some(descriptor) = crate::hal::driver_task::describe_driver_task_ring_frame(frame, 0)
    else {
        return false;
    };
    let command = DriverTaskCommandRecord::pi4_hot_path(
        0,
        hot_path,
        DriverTaskBudgetGrant::from_contract(contract),
        descriptor,
    );
    let staging_segments = [DriverTaskStagingSegment::ring_frame(frame, 0)];
    run_driver_task_net_service_staged(contract, command, &staging_segments)
        .is_some_and(driver_task_tx_completion_submitted)
}

#[cfg(feature = "kernel")]
fn driver_task_tx_completion_submitted(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    completion.code == DriverTaskCompletionCode::Progress.as_u16() && completion.result != 0
}

#[cfg(feature = "kernel")]
fn submit_cyw43_driver_task_eth_frame(contract: DriverTaskContract, frame: &[u8]) -> bool {
    let completion = run_cyw43_runtime_descriptor_command(
        contract,
        DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_ETH_TX,
            payload_len: frame.len() as u16,
            total_len: frame.len() as u32,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        },
        frame,
    );
    completion.is_some_and(driver_task_tx_completion_submitted)
}

#[cfg(feature = "kernel")]
fn submit_cyw43_host_eapol_payload_bounded(
    contract: DriverTaskContract,
    frame: &[u8],
    stage: &'static str,
) -> bool {
    for attempt in 0..CYW43_HOST_EAPOL_TX_ATTEMPTS {
        if submit_cyw43_driver_task_eth_frame(contract, frame) {
            CYW43_TX_SUBMITTED.fetch_add(1, Ordering::AcqRel);
            if attempt != 0 {
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    contract,
                    DriverTaskHotPath::Cyw43Wifi,
                    stage,
                    "tx-retried",
                    None,
                );
            }
            return true;
        }
        CYW43_HOST_EAPOL_TX_RETRIES.fetch_add(1, Ordering::AcqRel);
        let _ = poll_cyw43_driver_task_control_frame();
        core::hint::spin_loop();
    }
    CYW43_TX_DROPPED.fetch_add(1, Ordering::AcqRel);
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        stage,
        "tx-retry-exhausted",
        None,
    );
    false
}

#[cfg(feature = "kernel")]
pub(crate) fn submit_cyw43_driver_task_eth_payload(frame: &[u8]) -> bool {
    let submitted = submit_cyw43_driver_task_eth_frame(CYW43_WIFI_DRIVER_TASK_CONTRACT, frame);
    if submitted {
        CYW43_TX_SUBMITTED.fetch_add(1, Ordering::AcqRel);
    } else {
        CYW43_TX_DROPPED.fetch_add(1, Ordering::AcqRel);
    }
    submitted
}

#[cfg(feature = "kernel")]
pub(crate) fn submit_cyw43_driver_task_control_payload(
    payload: &[u8],
    control_ext_header: bool,
) -> bool {
    let completion = run_cyw43_runtime_descriptor_command(
        CYW43_WIFI_DRIVER_TASK_CONTRACT,
        DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME,
            flags: if control_ext_header {
                DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
            } else {
                0
            },
            payload_len: payload.len() as u16,
            total_len: payload.len() as u32,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        },
        payload,
    );
    completion.is_some_and(driver_task_tx_completion_submitted)
}

#[cfg(feature = "kernel")]
fn run_cyw43_runtime_descriptor_command(
    contract: DriverTaskContract,
    mut descriptor: DriverRuntimeCyw43CommandDescriptor,
    payload: &[u8],
) -> Option<DriverTaskCompletionRecord> {
    let desc_size = core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>();
    if desc_size > MAX_DRIVER_TASK_FRAME_BYTES || payload.len() > MAX_DRIVER_TASK_FRAME_BYTES {
        return None;
    }
    let mut payload_descriptor = None;
    if !payload.is_empty() {
        let payload_offset = crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET + 512;
        let staged_payload = crate::hal::driver_task::describe_driver_task_ring_payload_at(
            payload_offset,
            payload,
            0,
        )?;
        descriptor.payload_offset = u16::try_from(staged_payload.offset).ok()?;
        descriptor.payload_len = staged_payload.len;
        if descriptor.total_len == 0 {
            descriptor.total_len = u32::from(staged_payload.len);
        }
        payload_descriptor = Some(staged_payload);
    } else {
        descriptor.payload_offset = 0;
        descriptor.payload_len = 0;
        descriptor.total_len = 0;
    }
    let mut scratch = [0u8; core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>()];
    encode_cyw43_descriptor(&mut scratch, descriptor);
    let staged_descriptor = crate::hal::driver_task::describe_driver_task_ring_payload_at(
        crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET,
        &scratch,
        0,
    )?;
    let mut command = DriverTaskCommandRecord::pi4_hot_path(
        0,
        DriverTaskHotPath::Cyw43Wifi,
        DriverTaskBudgetGrant::from_contract(contract),
        staged_descriptor,
    );
    command.aux0 = DRIVER_RUNTIME_CYW43_COMMAND_AUX;
    let use_prompt_slice = cyw43_runtime_descriptor_uses_prompt_slice(descriptor.op);
    let active_before = crate::hal::driver_task::active_driver_task_ring_request(contract)
        .and_then(|request| u32::try_from(request).ok());
    let completion = if payload_descriptor.is_some() {
        let staging_segments = [
            DriverTaskStagingSegment::ring_payload_at(
                crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET + 512,
                payload,
                0,
            ),
            DriverTaskStagingSegment::ring_payload_at(
                crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET,
                &scratch,
                0,
            ),
        ];
        if use_prompt_slice {
            run_driver_task_net_service_prompt_slice_staged(contract, command, &staging_segments)
        } else {
            run_driver_task_net_service_staged(contract, command, &staging_segments)
        }
    } else {
        let staging_segments = [DriverTaskStagingSegment::ring_payload_at(
            crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET,
            &scratch,
            0,
        )];
        if use_prompt_slice {
            run_driver_task_net_service_prompt_slice_staged(contract, command, &staging_segments)
        } else {
            run_driver_task_net_service_staged(contract, command, &staging_segments)
        }
    };
    record_cyw43_active_prompt_poll(contract, descriptor, active_before, completion);
    completion
}

#[cfg(not(feature = "kernel"))]
fn submit_driver_task_frame(
    _contract: DriverTaskContract,
    _hot_path: DriverTaskHotPath,
    _frame: &[u8],
) -> bool {
    false
}

#[cfg(feature = "kernel")]
fn receive_driver_task_frame(
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
) -> Option<DriverTaskNetRxToken> {
    if hot_path == DriverTaskHotPath::Cyw43Wifi {
        return receive_cyw43_driver_task_frame(contract);
    }
    let command = DriverTaskCommandRecord::pi4_hot_path(
        0,
        hot_path,
        DriverTaskBudgetGrant::from_contract(contract),
        DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: 0,
        },
    );
    let completion = run_driver_task_net_service(contract, command)?;
    if completion.code != DriverTaskCompletionCode::FrameReady.as_u16() {
        return None;
    }
    let bytes = crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)?;
    let len = bytes.len().min(MAX_FRAME_LEN);
    let mut buffer = [0u8; MAX_FRAME_LEN];
    buffer[..len].copy_from_slice(&bytes[..len]);
    Some(DriverTaskNetRxToken { len, buffer })
}

#[cfg(feature = "kernel")]
fn receive_cyw43_driver_task_frame(contract: DriverTaskContract) -> Option<DriverTaskNetRxToken> {
    let completion = run_cyw43_runtime_descriptor_command(
        contract,
        DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_RX_POLL,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        },
        &[],
    )?;
    if completion.code != DriverTaskCompletionCode::FrameReady.as_u16() {
        return None;
    }
    let bytes = crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)?;
    let len = bytes.len().min(MAX_FRAME_LEN);
    let mut buffer = [0u8; MAX_FRAME_LEN];
    buffer[..len].copy_from_slice(&bytes[..len]);
    Some(DriverTaskNetRxToken { len, buffer })
}

#[cfg(feature = "kernel")]
pub(crate) fn poll_cyw43_driver_task_control_frame() -> Option<(u16, DriverTaskNetRxToken)> {
    poll_cyw43_driver_task_control_frame_with_flags(0)
}

#[cfg(feature = "kernel")]
fn poll_cyw43_driver_task_control_frame_with_flags(
    flags: u16,
) -> Option<(u16, DriverTaskNetRxToken)> {
    let contract = CYW43_WIFI_DRIVER_TASK_CONTRACT;
    let completion = poll_cyw43_driver_task_control_completion(flags)?;
    cyw43_driver_task_frame_from_completion(contract, completion)
}

#[cfg(feature = "kernel")]
fn poll_cyw43_driver_task_control_completion(flags: u16) -> Option<DriverTaskCompletionRecord> {
    let contract = CYW43_WIFI_DRIVER_TASK_CONTRACT;
    let completion = run_cyw43_runtime_descriptor_command(
        contract,
        DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL,
            flags,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        },
        &[],
    )?;
    Some(completion)
}

#[cfg(feature = "kernel")]
pub(crate) fn poll_cyw43_driver_task_data_frame() -> Option<(u16, DriverTaskNetRxToken)> {
    let completion = poll_cyw43_driver_task_data_completion(0)?;
    cyw43_driver_task_frame_from_completion(CYW43_WIFI_DRIVER_TASK_CONTRACT, completion)
}

#[cfg(feature = "kernel")]
fn poll_cyw43_driver_task_data_completion(flags: u16) -> Option<DriverTaskCompletionRecord> {
    let contract = CYW43_WIFI_DRIVER_TASK_CONTRACT;
    run_cyw43_runtime_descriptor_command(
        contract,
        DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_RX_POLL,
            flags,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        },
        &[],
    )
}

#[cfg(feature = "kernel")]
fn cyw43_driver_task_frame_from_completion(
    contract: DriverTaskContract,
    completion: DriverTaskCompletionRecord,
) -> Option<(u16, DriverTaskNetRxToken)> {
    if completion.code != DriverTaskCompletionCode::FrameReady.as_u16() {
        return None;
    }
    let bytes = crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)?;
    let len = bytes.len().min(MAX_FRAME_LEN);
    let mut buffer = [0u8; MAX_FRAME_LEN];
    buffer[..len].copy_from_slice(&bytes[..len]);
    Some((completion.frame.flags, DriverTaskNetRxToken { len, buffer }))
}

#[cfg(feature = "kernel")]
pub(crate) fn poll_cyw43_driver_task_any_frame() -> Option<(u16, DriverTaskNetRxToken)> {
    poll_cyw43_driver_task_control_frame().or_else(poll_cyw43_driver_task_data_frame)
}

#[cfg(not(feature = "kernel"))]
fn receive_driver_task_frame(
    _contract: DriverTaskContract,
    _hot_path: DriverTaskHotPath,
) -> Option<DriverTaskNetRxToken> {
    None
}

macro_rules! driver_task_nic {
    (
        $name:ident,
        $label:literal,
        $iface:literal,
        $mac:ident,
        $contract:ident,
        $hot_path:ident,
        $tx_submitted:ident,
        $tx_dropped:ident,
        $rx_frames:ident
    ) => {
        /// Smoltcp device shell for a Pi 4 NIC whose hardware state lives in a
        /// driver-task runtime instead of root.
        #[derive(Debug, Clone, Copy, Default)]
        pub struct $name {
            tx_drops: u32,
        }

        impl Device for $name {
            type RxToken<'a>
                = DriverTaskNetRxToken
            where
                Self: 'a;
            type TxToken<'a>
                = DriverTaskNetTxToken
            where
                Self: 'a;

            fn receive(
                &mut self,
                _timestamp: Instant,
            ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
                if matches!(DriverTaskHotPath::$hot_path, DriverTaskHotPath::Cyw43Wifi)
                    && !cyw43_data_plane_ready()
                {
                    return None;
                }
                let rx = receive_driver_task_frame($contract, DriverTaskHotPath::$hot_path)?;
                $rx_frames.fetch_add(1, Ordering::AcqRel);
                Some((
                    rx,
                    DriverTaskNetTxToken {
                        contract: $contract,
                        hot_path: DriverTaskHotPath::$hot_path,
                        tx_submitted: &$tx_submitted,
                        tx_dropped: &$tx_dropped,
                    },
                ))
            }

            fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
                if matches!(DriverTaskHotPath::$hot_path, DriverTaskHotPath::Cyw43Wifi)
                    && !cyw43_data_plane_ready()
                {
                    return None;
                }
                Some(DriverTaskNetTxToken {
                    contract: $contract,
                    hot_path: DriverTaskHotPath::$hot_path,
                    tx_submitted: &$tx_submitted,
                    tx_dropped: &$tx_dropped,
                })
            }

            fn capabilities(&self) -> DeviceCapabilities {
                let mut caps = DeviceCapabilities::default();
                caps.max_transmission_unit = MAX_FRAME_LEN;
                caps.medium = smoltcp::phy::Medium::Ethernet;
                caps
            }
        }

        impl NetDevice for $name {
            type Error = DriverTaskNetError;

            fn create<H>(_hal: &mut H) -> Result<Self, Self::Error>
            where
                H: Hardware<Error = HalError>,
                Self: Sized,
            {
                Ok(Self::default())
            }

            fn create_with_stage<H>(
                _hal: &mut H,
                _config: &ConsoleNetConfig,
                _stage: NetStage,
            ) -> Result<Self, Self::Error>
            where
                H: Hardware<Error = HalError>,
                Self: Sized,
            {
                Ok(Self::default())
            }

            fn mac(&self) -> EthernetAddress {
                runtime_mac(DriverTaskHotPath::$hot_path).unwrap_or($mac)
            }

            fn tx_drop_count(&self) -> u32 {
                self.tx_drops
                    .saturating_add($tx_dropped.load(Ordering::Acquire))
            }

            fn name() -> &'static str
            where
                Self: Sized,
            {
                $label
            }

            fn driver_task_contract() -> DriverTaskContract
            where
                Self: Sized,
            {
                $contract
            }

            fn interface_label(&self) -> &'static str {
                $iface
            }

            fn bringup_status_label(&self) -> Option<&'static str> {
                if matches!(DriverTaskHotPath::$hot_path, DriverTaskHotPath::Cyw43Wifi) {
                    return cyw43_driver_task_bringup_status_label();
                }
                if runtime_ready(DriverTaskHotPath::$hot_path) {
                    None
                } else {
                    Some(DRIVER_TASK_NET_STATUS)
                }
            }

            fn driver_task_runtime_client() -> bool
            where
                Self: Sized,
            {
                true
            }

            fn debug_snapshot(&mut self) {}

            fn counters(&self) -> NetDeviceCounters {
                NetDeviceCounters {
                    rx_packets: $rx_frames.load(Ordering::Acquire) as u64,
                    tx_packets: $tx_submitted.load(Ordering::Acquire) as u64,
                    tx_submit: $tx_submitted.load(Ordering::Acquire) as u64,
                    tx_complete: $tx_submitted.load(Ordering::Acquire) as u64,
                    wifi_assoc: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) && CYW43_ASSOCIATED.load(Ordering::Acquire) != 0
                    {
                        1
                    } else {
                        0
                    },
                    wifi_link_up: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) && CYW43_LINK_UP.load(Ordering::Acquire) != 0
                    {
                        1
                    } else {
                        0
                    },
                    wifi_host_eapol_rx: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        CYW43_HOST_EAPOL_RX.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    wifi_host_eapol_start: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        CYW43_HOST_EAPOL_START.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    wifi_host_eapol_secure: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) && CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire)
                        != 0
                    {
                        1
                    } else {
                        0
                    },
                    ..NetDeviceCounters::default()
                }
            }
        }
    };
}

driver_task_nic!(
    GenetDriverTaskDevice,
    "bcmgenet-v5-driver-task",
    "wired",
    GENET_DRIVER_TASK_MAC,
    GENET_DRIVER_TASK_CONTRACT,
    GenetNic,
    GENET_TX_SUBMITTED,
    GENET_TX_DROPPED,
    GENET_RX_FRAMES
);
driver_task_nic!(
    Cyw43DriverTaskDevice,
    "cyw43455-driver-task",
    "wifi",
    CYW43_DRIVER_TASK_MAC,
    CYW43_WIFI_DRIVER_TASK_CONTRACT,
    Cyw43Wifi,
    CYW43_TX_SUBMITTED,
    CYW43_TX_DROPPED,
    CYW43_RX_FRAMES
);

#[cfg(test)]
mod tests {
    use super::*;
    use smoltcp::phy::{Device, TxToken};
    use smoltcp::time::Instant;

    static CYW43_STATUS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset_cyw43_status_flags() {
        CYW43_LINKED_RUNTIME_READY.store(0, Ordering::Release);
        CYW43_CONTROL_PLANE_READY.store(0, Ordering::Release);
        CYW43_ASSOCIATED.store(0, Ordering::Release);
        CYW43_LINK_UP.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_RX.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_START.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_ACTIVE.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_REQUIRED.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TX_RETRIES.store(0, Ordering::Release);
        *CYW43_HOST_EAPOL_SESSION.lock() = None;
        *CYW43_HOST_EAPOL_PENDING_EVENT.lock() = None;
        clear_cyw43_active_prompt_poll();
    }

    #[test]
    fn cyw43_control_preflight_matches_linux_ordered_primitives() {
        assert_eq!(CYW43_WLC_GET_REVINFO, 98);
        assert_eq!(CYW43_CONTROL_EXCHANGE_FAULT_DETAIL, 0x530b);
        assert_eq!(CYW43_BCME_UNSUPPORTED_STATUS, 0xffff_ffe9);
        let unsupported = DriverTaskCompletionRecord {
            sequence: 0,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: CYW43_CONTROL_EXCHANGE_FAULT_DETAIL,
            result: CYW43_BCME_UNSUPPORTED_STATUS,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        let other_fault = DriverTaskCompletionRecord {
            result: 0xffff_ffff,
            ..unsupported
        };
        assert!(cyw43_control_exchange_completion_is_unsupported(
            unsupported
        ));
        assert!(!cyw43_control_exchange_completion_is_unsupported(
            other_fault
        ));
    }

    #[test]
    fn cyw43_join_event_mask_matches_old_good_event_msgs_ext_shape() {
        let mask = cyw43_linux_join_event_mask().expect("join event mask must fit");
        let mut payload = [0u8; CYW43_EVENTMSGS_EXT_PAYLOAD_LEN];
        cyw43_write_event_msgs_ext_payload(&mut payload, &mask)
            .expect("event_msgs_ext payload must fit");

        assert_eq!(CYW43_EVENT_MASK_LEN, 27);
        assert_eq!(CYW43_EVENTMSGS_EXT_PAYLOAD_LEN, 31);
        assert_eq!(payload[0], CYW43_EVENTMSGS_EXT_VER);
        assert_eq!(payload[1], CYW43_EVENTMSGS_EXT_SET_MASK);
        assert_eq!(payload[2], CYW43_EVENT_MASK_LEN as u8);
        assert_eq!(payload[3], CYW43_EVENTMSGS_EXT_MAX_GET_SIZE);
        for event in CYW43_JOIN_COMPLETION_EVENTS {
            let index = usize::from(event / 8);
            let bit = event % 8;
            assert_ne!(
                mask[index] & (1 << bit),
                0,
                "join event {event} must be enabled"
            );
        }
    }

    #[test]
    fn cyw43_bsscfg_join_payload_matches_old_good_linux_shape() {
        let mut credentials = crate::net::WifiCredentials::empty();
        credentials.ssid_len = 7;
        credentials.ssid[..7].copy_from_slice(b"cohesix");
        let mut payload = [0xa5u8; CYW43_LINUX_BSSCFG_JOIN_PAYLOAD_LEN];

        cyw43_write_linux_bsscfg_join_payload(&mut payload, credentials)
            .expect("bsscfg join payload must encode");

        assert_eq!(CYW43_LINUX_BSSCFG_JOIN_PAYLOAD_LEN, 68);
        assert_eq!(
            &payload[CYW43_LINUX_EXT_JOIN_SSID_OFFSET..CYW43_LINUX_EXT_JOIN_SSID_OFFSET + 4],
            &7u32.to_le_bytes()
        );
        assert_eq!(
            &payload[CYW43_LINUX_EXT_JOIN_SSID_OFFSET + 4..CYW43_LINUX_EXT_JOIN_SSID_OFFSET + 11],
            b"cohesix"
        );
        assert_eq!(payload[CYW43_LINUX_EXT_JOIN_SCAN_OFFSET], 0xff);
        assert_eq!(
            &payload[CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 4..CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 8],
            &u32::MAX.to_le_bytes()
        );
        assert_eq!(
            &payload[CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 8..CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 12],
            &u32::MAX.to_le_bytes()
        );
        assert_eq!(
            &payload[CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 12..CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 16],
            &u32::MAX.to_le_bytes()
        );
        assert_eq!(
            &payload[CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 16..CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 20],
            &u32::MAX.to_le_bytes()
        );
        assert_eq!(
            &payload[CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET..CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET + 6],
            &[0xff; 6]
        );
        assert_eq!(
            &payload[CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET + 8..CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET + 12],
            &0u32.to_le_bytes()
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_join_iovar_fallback_is_limited_to_known_firmware_statuses() {
        let unsupported = DriverTaskCompletionRecord {
            sequence: 0,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: CYW43_CONTROL_EXCHANGE_FAULT_DETAIL,
            result: CYW43_BCME_UNSUPPORTED_STATUS,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        let badarg = DriverTaskCompletionRecord {
            result: CYW43_BCME_BADARG_STATUS,
            ..unsupported
        };
        let transport_fault = DriverTaskCompletionRecord {
            detail: DriverTaskFaultCode::RejectedCommand.as_u16(),
            ..unsupported
        };
        let other_status = DriverTaskCompletionRecord {
            result: 0xffff_ffff,
            ..unsupported
        };

        assert!(cyw43_join_iovar_completion_allows_set_ssid(unsupported));
        assert!(cyw43_join_iovar_completion_allows_set_ssid(badarg));
        assert!(!cyw43_join_iovar_completion_allows_set_ssid(
            transport_fault
        ));
        assert!(!cyw43_join_iovar_completion_allows_set_ssid(other_status));
    }

    #[test]
    fn cyw43_control_exchange_descriptor_uses_plain_startup_header_mode() {
        let descriptor = cyw43_control_exchange_descriptor(
            36,
            CYW43_WLC_SET_VAR,
            1,
            Cyw43ControlHeaderMode::Plain,
        );

        assert_eq!(descriptor.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE);
        assert_eq!(descriptor.flags, 0);
        assert_eq!(descriptor.payload_len, 36);
        assert_eq!(descriptor.total_len, 36);
        assert_eq!(descriptor.arg0, CYW43_WLC_SET_VAR);
        assert_eq!(descriptor.arg1, 1);
    }

    #[test]
    fn cyw43_control_exchange_descriptor_keeps_extended_default_mode() {
        let descriptor = cyw43_control_exchange_descriptor(
            16,
            CYW43_WLC_GET_REVINFO,
            4,
            Cyw43ControlHeaderMode::Extended,
        );

        assert_eq!(descriptor.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE);
        assert_eq!(
            descriptor.flags,
            DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
        );
        assert_eq!(descriptor.payload_len, 16);
        assert_eq!(descriptor.total_len, 16);
        assert_eq!(descriptor.arg0, CYW43_WLC_GET_REVINFO);
        assert_eq!(descriptor.arg1, 4);
    }

    #[test]
    fn cyw43_control_frame_descriptor_uses_split_tx_op() {
        let plain = cyw43_control_frame_descriptor(36, Cyw43ControlHeaderMode::Plain);
        let extended = cyw43_control_frame_descriptor(16, Cyw43ControlHeaderMode::Extended);

        assert_eq!(plain.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(plain.flags, 0);
        assert_eq!(plain.payload_len, 36);
        assert_eq!(plain.total_len, 36);
        assert_eq!(extended.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(extended.flags, DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER);
    }

    #[test]
    fn cyw43_control_reply_parser_decodes_cdc_header() {
        let mut buffer = [0u8; MAX_FRAME_LEN];
        buffer[0..4].copy_from_slice(&CYW43_WLC_SET_VAR.to_le_bytes());
        buffer[4..8].copy_from_slice(&4u32.to_le_bytes());
        buffer[8..10].copy_from_slice(&CYW43_BCDC_FLAG_SET.to_le_bytes());
        buffer[10..12].copy_from_slice(&7u16.to_le_bytes());
        buffer[12..16].copy_from_slice(&0u32.to_le_bytes());
        buffer[16..20].copy_from_slice(&0x1122_3344u32.to_le_bytes());
        let token = DriverTaskNetRxToken { len: 20, buffer };

        let reply = cyw43_control_reply_from_token(&token).expect("CDC header must decode");

        assert_eq!(reply.cmd, CYW43_WLC_SET_VAR);
        assert_eq!(reply.id, 7);
        assert_eq!(reply.status, 0);
        assert_eq!(reply.response_len, 4);
        assert_eq!(reply.payload_available, 4);
    }

    #[test]
    fn cyw43_control_response_completion_strips_cdc_header() {
        let completion = DriverTaskCompletionRecord {
            sequence: 42,
            code: DriverTaskCompletionCode::FrameReady.as_u16(),
            detail: 0,
            result: 20,
            frame: DriverFrameDescriptor {
                offset: 1024,
                len: 20,
                flags: 0x0002,
            },
        };

        let response =
            cyw43_control_response_completion(completion, 4).expect("response frame must fit");

        assert_eq!(response.sequence, 42);
        assert_eq!(response.code, DriverTaskCompletionCode::FrameReady.as_u16());
        assert_eq!(response.result, 4);
        assert_eq!(response.frame.offset, 1024 + CYW43_BCDC_HEADER_BYTES as u32);
        assert_eq!(response.frame.len, 4);
        assert_eq!(response.frame.flags, 0x0002);
    }

    #[test]
    fn cyw43_control_split_timeout_preserves_idle_detail() {
        let completion = DriverTaskCompletionRecord {
            sequence: 42,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY,
            result: 0xd700_0000,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };

        assert_eq!(
            cyw43_control_split_timeout_result(Some(completion), 0, 0),
            0x430a_ffff
        );
        assert_eq!(cyw43_control_split_timeout_result(None, 0, 0), 0x4303_0000);
        assert_eq!(
            cyw43_control_split_timeout_result(Some(completion), 1, 0),
            0x4308_0001
        );
    }

    #[test]
    fn cyw43_revinfo_frame_reserves_known_good_response_bytes() {
        let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
        let len = cyw43_write_bcdc_revinfo_frame(&mut frame, 7)
            .expect("revinfo request frame must fit the bounded control buffer");

        assert_eq!(CYW43_REVINFO_RESPONSE_BYTES, 68);
        assert_eq!(len, CYW43_BCDC_HEADER_BYTES + CYW43_REVINFO_RESPONSE_BYTES);
        assert_eq!(
            u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]),
            CYW43_WLC_GET_REVINFO
        );
        assert_eq!(
            u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]),
            CYW43_REVINFO_RESPONSE_BYTES as u32
        );
        assert_eq!(
            u16::from_le_bytes([frame[8], frame[9]]),
            CYW43_BCDC_FLAG_GET
        );
        assert_eq!(u16::from_le_bytes([frame[10], frame[11]]), 7);
        assert!(frame[CYW43_BCDC_HEADER_BYTES..len]
            .iter()
            .all(|byte| *byte == 0));
    }

    #[test]
    fn driver_task_nic_tx_token_stages_or_counts_drop_without_mmio() {
        let before = GENET_TX_DROPPED.load(Ordering::Acquire);
        let mut dev = GenetDriverTaskDevice::default();
        let token = dev
            .transmit(Instant::from_millis(0))
            .expect("driver-task NIC must expose a TX ring token");

        token.consume(16, |buf| {
            buf.copy_from_slice(&[0xa5; 16]);
        });

        assert!(GENET_TX_DROPPED.load(Ordering::Acquire) >= before.saturating_add(1));
        assert!(dev.tx_drop_count() >= 1);
    }

    #[test]
    fn driver_task_nic_receive_is_ring_driven() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status-label tests must serialize");
        reset_cyw43_status_flags();
        let mut dev = Cyw43DriverTaskDevice::default();
        assert!(dev.receive(Instant::from_millis(0)).is_none());
        assert_eq!(dev.bringup_status_label(), Some("driver-task-ring-client"));
        reset_cyw43_status_flags();
    }

    #[test]
    fn cyw43_driver_task_firmware_ready_is_not_dhcp_ready() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status-label tests must serialize");
        reset_cyw43_status_flags();
        CYW43_LINKED_RUNTIME_READY.store(1, Ordering::Release);
        CYW43_CONTROL_PLANE_READY.store(0, Ordering::Release);
        CYW43_ASSOCIATED.store(0, Ordering::Release);
        CYW43_LINK_UP.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);

        let mut dev = Cyw43DriverTaskDevice::default();
        assert_eq!(dev.bringup_status_label(), Some("wifi-associating"));
        assert!(
            dev.transmit(Instant::from_millis(0)).is_none(),
            "DHCP/data TX must stay blocked until host-EAPOL is complete"
        );

        CYW43_HOST_EAPOL_ACTIVE.store(1, Ordering::Release);
        assert_eq!(dev.bringup_status_label(), Some("wifi-host-eapol-pending"));
        assert!(
            dev.transmit(Instant::from_millis(0)).is_none(),
            "active host-EAPOL must not release DHCP/data TX"
        );
        CYW43_HOST_EAPOL_ACTIVE.store(0, Ordering::Release);

        CYW43_HOST_EAPOL_REQUIRED.store(1, Ordering::Release);
        assert_eq!(dev.bringup_status_label(), Some("wifi-host-eapol-required"));
        assert!(
            dev.transmit(Instant::from_millis(0)).is_none(),
            "terminal host-EAPOL failure must keep DHCP/data TX blocked"
        );
        CYW43_HOST_EAPOL_REQUIRED.store(0, Ordering::Release);

        CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);
        assert_eq!(dev.bringup_status_label(), Some("wifi-link-down"));
        assert!(
            dev.transmit(Instant::from_millis(0)).is_none(),
            "host-EAPOL alone is not secure carrier readiness"
        );
        CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);

        CYW43_ASSOCIATED.store(1, Ordering::Release);
        CYW43_LINK_UP.store(1, Ordering::Release);
        assert_eq!(dev.bringup_status_label(), Some("wifi-host-eapol-pending"));
        assert!(
            dev.transmit(Instant::from_millis(0)).is_none(),
            "association alone is not DHCP/data readiness"
        );

        CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);
        assert_eq!(dev.bringup_status_label(), None);
        assert!(
            dev.transmit(Instant::from_millis(0)).is_some(),
            "descriptor data TX should become available after host-EAPOL"
        );

        CYW43_LINKED_RUNTIME_READY.store(0, Ordering::Release);
        CYW43_CONTROL_PLANE_READY.store(0, Ordering::Release);
        CYW43_ASSOCIATED.store(0, Ordering::Release);
        CYW43_LINK_UP.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);
    }

    #[test]
    fn host_eapol_start_cadence_is_bounded() {
        assert_eq!(CYW43_HOST_EAPOL_PRE_ASSOC_POLLS, 8_192);
        assert_eq!(CYW43_HOST_EAPOL_POST_ASSOC_POLLS, 16_384);
        assert_eq!(CYW43_HOST_EAPOL_JOIN_POLLS, 24_576);
        assert!(!cyw43_host_eapol_start_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL - 1,
            0
        ));
        assert!(cyw43_host_eapol_start_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL,
            0
        ));
        assert!(!cyw43_host_eapol_start_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL + 1,
            1
        ));
        assert!(!cyw43_host_eapol_start_due(64, 1));
        assert!(!cyw43_host_eapol_start_due(4096, 1));
        assert!(cyw43_host_eapol_start_due(16384, 1));
        assert!(!cyw43_host_eapol_start_due(
            16384,
            CYW43_HOST_EAPOL_START_MAX
        ));
        assert!(!cyw43_host_eapol_start_due(2, 1));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_pending_transport_does_not_spend_poll_window() {
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");

        record_cyw43_host_eapol_poll_completion(&mut session, Cyw43HostEapolPollResult::default());
        assert_eq!(session.progress.polls, 0);
        assert_eq!(session.progress.empty_polls, 0);

        record_cyw43_host_eapol_poll_completion(
            &mut session,
            Cyw43HostEapolPollResult {
                completed: true,
                observed_frame: false,
                activity: false,
                secure: false,
            },
        );
        assert_eq!(session.progress.polls, 1);
        assert_eq!(session.progress.empty_polls, 1);

        record_cyw43_host_eapol_poll_completion(
            &mut session,
            Cyw43HostEapolPollResult {
                completed: true,
                observed_frame: true,
                activity: true,
                secure: false,
            },
        );
        assert_eq!(session.progress.polls, 2);
        assert_eq!(session.progress.empty_polls, 1);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_bssid_probe_promotes_only_valid_ap_candidate() {
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let station = EthernetAddress([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10]);
        let ap = EthernetAddress([0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5]);
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");

        assert!(!cyw43_host_eapol_assoc_probe_due(
            &session.progress,
            session.probed_assoc_bssid
        ));
        session.progress.polls = CYW43_HOST_EAPOL_ASSOC_PROBE_AFTER_POLLS;
        assert!(cyw43_host_eapol_assoc_probe_due(
            &session.progress,
            session.probed_assoc_bssid
        ));

        assert!(!cyw43_host_eapol_bssid_candidate(
            EthernetAddress([0; ETHER_ADDR_LEN]),
            station
        ));
        assert!(!cyw43_host_eapol_bssid_candidate(station, station));
        assert!(!cyw43_host_eapol_bssid_candidate(
            EthernetAddress(CYW43_PAE_GROUP_ADDR),
            station
        ));
        assert!(!cyw43_host_eapol_bssid_candidate(
            EthernetAddress([0x02, 0x72, 0xea, 0x4c, 0xc7, 0xa5]),
            station
        ));
        assert!(cyw43_host_eapol_bssid_candidate(ap, station));

        assert!(cyw43_apply_host_eapol_bssid_probe(
            &mut session,
            station,
            ap,
            CYW43_HOST_EAPOL_ASSOC_PROBE_AFTER_POLLS as usize
        ));
        assert!(session.progress.associated);
        assert!(!session.progress.link_up);
        assert_eq!(session.progress.event_rx, 0);
        assert_eq!(session.progress.eapol_rx, 0);
        assert_eq!(session.progress.association_event, Some("bssid-probe"));
        assert_eq!(
            session.progress.association_poll,
            CYW43_HOST_EAPOL_ASSOC_PROBE_AFTER_POLLS + 1
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_poll_kind_serializes_prompt_poll_ops() {
        assert_eq!(
            cyw43_host_eapol_poll_kind_for_op(DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL),
            Some(Cyw43HostEapolPollKind::Control)
        );
        assert_eq!(
            cyw43_host_eapol_poll_kind_for_op(DRIVER_RUNTIME_CYW43_OP_RX_POLL),
            Some(Cyw43HostEapolPollKind::Data)
        );
        assert_eq!(
            cyw43_host_eapol_poll_kind_for_op(DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME),
            None
        );
        assert_eq!(
            cyw43_host_eapol_poll_kind_for_op(DRIVER_RUNTIME_CYW43_OP_ETH_TX),
            None
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_active_prompt_poll_recovers_descriptor_flags() {
        let progress = crate::hal::driver_task::DriverTaskRingProgressSnapshot {
            marker_valid: true,
            sequence: 478,
            phase: 142,
            phase_name: "cyw43-sdio-owner-wait-begin",
            aux0: DRIVER_RUNTIME_CYW43_COMMAND_AUX,
        };
        let descriptor = DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL,
            flags: DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        };
        let mut bytes = [0u8; core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>()];
        encode_cyw43_descriptor(&mut bytes, descriptor);

        assert_eq!(decode_cyw43_descriptor(&bytes), Some(descriptor));
        assert_eq!(
            cyw43_active_prompt_poll_for_descriptor(478, Some(progress), descriptor),
            Some(Cyw43HostEapolActivePoll {
                kind: Cyw43HostEapolPollKind::Control,
                flags: DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
            })
        );
        assert_eq!(
            cyw43_active_prompt_poll_for_descriptor(477, Some(progress), descriptor),
            None
        );
        assert_eq!(
            cyw43_active_prompt_poll_for_descriptor(
                478,
                Some(crate::hal::driver_task::DriverTaskRingProgressSnapshot {
                    aux0: 0,
                    ..progress
                }),
                descriptor
            ),
            None
        );
        assert_eq!(
            cyw43_active_prompt_poll_for_descriptor(
                478,
                Some(progress),
                DriverRuntimeCyw43CommandDescriptor {
                    op: DRIVER_RUNTIME_CYW43_OP_ETH_TX,
                    ..DriverRuntimeCyw43CommandDescriptor::empty()
                }
            ),
            None
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_progress_tracks_data_and_eapol_edges() {
        let mut progress = Cyw43HostEapolProgress::default();
        progress.record_data_frame(0x1200, 48, Some(0x0800));
        assert_eq!(progress.data_rx, 1);
        assert_eq!(progress.eapol_rx, 0);
        assert_eq!(progress.non_eapol_rx, 1);
        assert_eq!(progress.last_flags, 0x1200);
        assert_eq!(progress.last_len, 48);
        assert_eq!(progress.last_ethertype, 0x0800);
        assert!(progress.last_ethertype_valid);
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-eapol-filter-or-ap-m1"
        );

        progress.record_data_frame(0x1300, 117, Some(ETH_P_EAPOL));
        assert_eq!(progress.data_rx, 2);
        assert_eq!(progress.eapol_rx, 1);
        assert_eq!(progress.non_eapol_rx, 1);
        assert_eq!(progress.last_ethertype, ETH_P_EAPOL);
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-host-eapol-handshake-state"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_required_without_rx_targets_data_path() {
        let mut progress = Cyw43HostEapolProgress::default();
        progress.polls = CYW43_HOST_EAPOL_JOIN_POLLS as u32;
        progress.record_empty_poll();

        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-cyw43-data-rx-path"
        );
        assert_eq!(
            cyw43_host_eapol_next_action("secure", &progress),
            "release-dhcp-data"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_event_frame_marks_association_window() {
        let sta = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        let mut packet = [0u8; CYW43_BRCMF_EVENT_MIN_PACKET_LEN];
        packet[..6].copy_from_slice(&sta);
        packet[6..12].copy_from_slice(&ap);
        packet[12..14].copy_from_slice(&CYW43_ETH_P_LINK_CTL.to_be_bytes());
        packet[14..16].copy_from_slice(&CYW43_BCMILCP_SUBTYPE_VENDOR_LONG.to_be_bytes());
        packet[19..22].copy_from_slice(&CYW43_BROADCOM_OUI);
        packet[22..24].copy_from_slice(&CYW43_BCMILCP_BCM_SUBTYPE_EVENT.to_be_bytes());
        packet[CYW43_BRCMF_EVENT_FLAGS_OFFSET..CYW43_BRCMF_EVENT_FLAGS_OFFSET + 2]
            .copy_from_slice(&CYW43_EVENT_FLAG_LINK.to_be_bytes());
        packet[CYW43_BRCMF_EVENT_TYPE_OFFSET] = CYW43_EVENT_LINK;
        packet[CYW43_BRCMF_EVENT_STATUS_OFFSET..CYW43_BRCMF_EVENT_STATUS_OFFSET + 4]
            .copy_from_slice(&CYW43_EVENT_STATUS_SUCCESS.to_be_bytes());
        packet[CYW43_BRCMF_EVENT_ADDR_OFFSET..CYW43_BRCMF_EVENT_ADDR_OFFSET + 6]
            .copy_from_slice(&ap);

        let mut event_frame = [0u8; CYW43_BDC_HEADER_BYTES + CYW43_BRCMF_EVENT_MIN_PACKET_LEN];
        event_frame[0] = CYW43_BDC_VERSION << CYW43_BDC_VERSION_SHIFT;
        event_frame[CYW43_BDC_HEADER_BYTES..].copy_from_slice(&packet);
        let event = cyw43_parse_control_or_event_frame(&event_frame).expect("link event");
        assert_eq!(event.src_mac, ap);
        assert_eq!(event.addr, ap);
        assert_eq!(
            cyw43_host_eapol_post_assoc_event_label(event, true),
            Some("link-up")
        );

        let mut progress = Cyw43HostEapolProgress::default();
        progress.record_event_frame(
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT,
            event_frame.len(),
            event,
            42,
        );
        assert!(progress.associated);
        assert!(progress.link_up);
        assert_eq!(progress.event_rx, 1);
        assert_eq!(progress.association_event, Some("link-up"));
        assert_eq!(progress.association_poll, 43);
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-cyw43-data-rx-path"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_event_token_survives_control_reply_polling() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status-label tests must serialize");
        reset_cyw43_status_flags();

        let sta = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        let mut packet = [0u8; CYW43_BRCMF_EVENT_MIN_PACKET_LEN];
        packet[..6].copy_from_slice(&sta);
        packet[6..12].copy_from_slice(&ap);
        packet[12..14].copy_from_slice(&CYW43_ETH_P_LINK_CTL.to_be_bytes());
        packet[14..16].copy_from_slice(&CYW43_BCMILCP_SUBTYPE_VENDOR_LONG.to_be_bytes());
        packet[19..22].copy_from_slice(&CYW43_BROADCOM_OUI);
        packet[22..24].copy_from_slice(&CYW43_BCMILCP_BCM_SUBTYPE_EVENT.to_be_bytes());
        packet[CYW43_BRCMF_EVENT_FLAGS_OFFSET..CYW43_BRCMF_EVENT_FLAGS_OFFSET + 2]
            .copy_from_slice(&CYW43_EVENT_FLAG_LINK.to_be_bytes());
        packet[CYW43_BRCMF_EVENT_TYPE_OFFSET] = CYW43_EVENT_LINK;
        packet[CYW43_BRCMF_EVENT_STATUS_OFFSET..CYW43_BRCMF_EVENT_STATUS_OFFSET + 4]
            .copy_from_slice(&CYW43_EVENT_STATUS_SUCCESS.to_be_bytes());
        packet[CYW43_BRCMF_EVENT_ADDR_OFFSET..CYW43_BRCMF_EVENT_ADDR_OFFSET + 6]
            .copy_from_slice(&ap);

        let mut event_frame = [0u8; CYW43_BDC_HEADER_BYTES + CYW43_BRCMF_EVENT_MIN_PACKET_LEN];
        event_frame[0] = CYW43_BDC_VERSION << CYW43_BDC_VERSION_SHIFT;
        event_frame[CYW43_BDC_HEADER_BYTES..].copy_from_slice(&packet);
        let mut token = DriverTaskNetRxToken {
            len: event_frame.len(),
            buffer: [0; MAX_FRAME_LEN],
        };
        token.buffer[..event_frame.len()].copy_from_slice(&event_frame);

        assert!(cyw43_capture_event_frame_from_token(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            "test-control-poll",
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT,
            &token,
        ));
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 1);

        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        cyw43_apply_pending_host_eapol_event(CYW43_WIFI_DRIVER_TASK_CONTRACT, &mut session, 7);

        assert_eq!(session.progress.event_rx, 1);
        assert!(session.progress.associated);
        assert!(session.progress.link_up);
        assert_eq!(session.progress.association_event, Some("link-up"));
        assert_eq!(session.progress.association_poll, 8);
        assert!(CYW43_HOST_EAPOL_PENDING_EVENT.lock().is_none());
        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_firstread_details_select_next_action() {
        let mut progress = Cyw43HostEapolProgress::default();
        progress.record_rx_idle_completion(DriverTaskCompletionRecord {
            sequence: 1,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY,
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        });
        assert_eq!(progress.rx_firstread_attempts, 1);
        assert_eq!(progress.rx_firstread_empty, 1);
        assert_eq!(progress.last_rx_source, None);
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-association-event-or-cyw43-rx-latch"
        );
        progress.associated = true;
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-ap-m1-or-cyw43-rx-latch"
        );

        progress.record_rx_idle_completion(DriverTaskCompletionRecord {
            sequence: 2,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_INVALID_SDPCM,
            result: 0x3412,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        });
        assert_eq!(progress.rx_firstread_invalid, 1);
        assert_eq!(progress.last_rx_idle_result, 0x3412);
        assert_eq!(progress.last_rx_source, None);
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-cyw43-data-rx-firstread-prefix"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_firstread_decodes_rx_source_result() {
        let mut progress = Cyw43HostEapolProgress::default();
        let result = DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_MAGIC
            | 512
            | (0x07 << DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_SHIFT)
            | DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FRAME_INDICATED
            | DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_HOST_INTERRUPT
            | DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_CARD_INTERRUPT
            | DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FUNCTION2_READY;

        progress.record_rx_idle_completion(DriverTaskCompletionRecord {
            sequence: 1,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY,
            result,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        });
        assert_eq!(
            progress.last_rx_source,
            Some(Cyw43RxSourceResult {
                probe_len: 512,
                interrupt_enable: 0x07,
                frame_indicated: true,
                host_interrupt: true,
                card_interrupt: true,
                function2_ready: true,
            })
        );

        progress.record_control_rx_idle_completion(DriverTaskCompletionRecord {
            sequence: 2,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY,
            result: DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_MAGIC | 64,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        });
        assert_eq!(
            progress.last_control_rx_source,
            Some(Cyw43RxSourceResult {
                probe_len: 64,
                interrupt_enable: 0,
                frame_indicated: false,
                host_interrupt: false,
                card_interrupt: false,
                function2_ready: false,
            })
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_firstread_cadence_starts_after_eapol_start() {
        assert!(cyw43_host_eapol_rx_firstread_due(0, 0));
        assert!(cyw43_host_eapol_rx_firstread_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL,
            0
        ));
        assert!(cyw43_host_eapol_rx_firstread_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL + 1024,
            1
        ));
        assert!(cyw43_host_eapol_rx_firstread_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL + 1025,
            1
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_fault_reason_labels_split_transport_details() {
        assert_eq!(
            cyw43_runtime_fault_reason(0x5323),
            "cyw43-backplane-chipcommon-read"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x5102),
            "sdio-descriptor-unavailable"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x5103),
            "sdio-descriptor-transfer-failed"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x5104),
            "sdio-host-config-failed"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x5312),
            "cyw43-transport-card-init"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x5325),
            "cyw43-transport-card-cmd5-ocr"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x5328),
            "cyw43-transport-card-cmd7-select"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x5329),
            "cyw43-firmware-retry-exhausted"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x532a),
            "cyw43-post-release-ht-clock"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x532b),
            "cyw43-post-release-function2-ready"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x532c),
            "cyw43-post-release-corecontrol"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x532d),
            "cyw43-post-release-mailbox-ready"
        );
        assert_eq!(
            cyw43_runtime_fault_reason(0x532e),
            "cyw43-post-release-protocol-version"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_transport_recovery_is_limited_to_owner_backplane_faults() {
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x5323));
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x5321));
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x531a));
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x5101));
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x5102));
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x5103));
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x5104));
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x5329));
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x532a));
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x532b));
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x532c));
        assert!(cyw43_fault_detail_allows_sdio_owner_recovery(0x532d));
        assert!(!cyw43_fault_detail_allows_sdio_owner_recovery(0x532e));
        assert!(!cyw43_fault_detail_allows_sdio_owner_recovery(0x5302));
        assert!(!cyw43_fault_detail_allows_sdio_owner_recovery(0x5306));
        assert!(!cyw43_fault_detail_allows_sdio_owner_recovery(0x53ff));
        assert!(cyw43_fault_detail_allows_same_command_retry(0x5103));
        assert!(!cyw43_fault_detail_allows_same_command_retry(0x5102));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_firmware_recovery_suppresses_redundant_replay_brackets() {
        assert!(cyw43_sdio_replay_resource_status_is_redundant(
            "cyw43-firmware-recover",
            "begin",
        ));
        assert!(cyw43_sdio_replay_resource_status_is_redundant(
            "cyw43-firmware-recover",
            "ready",
        ));
        assert!(!cyw43_sdio_replay_resource_status_is_redundant(
            "cyw43-firmware-recover",
            "failed",
        ));
        assert!(!cyw43_sdio_replay_resource_status_is_redundant(
            "cyw43-firmware-chunk",
            "fault",
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_no_reply_resumes_cover_long_control_exchange_turns() {
        assert_eq!(
            cyw43_runtime_no_reply_resume_limit(DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT),
            CYW43_RUNTIME_TRANSPORT_NO_REPLY_RESUMES
        );
        assert_eq!(
            cyw43_runtime_no_reply_resume_limit(DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE),
            CYW43_RUNTIME_CONTROL_EXCHANGE_NO_REPLY_RESUMES
        );
        assert!(
            CYW43_RUNTIME_CONTROL_EXCHANGE_NO_REPLY_RESUMES
                > CYW43_RUNTIME_TRANSPORT_NO_REPLY_RESUMES
        );
        assert_eq!(
            cyw43_runtime_no_reply_resume_limit(DRIVER_RUNTIME_CYW43_OP_RX_POLL),
            0
        );
        assert!(cyw43_runtime_descriptor_uses_prompt_slice(
            DRIVER_RUNTIME_CYW43_OP_RX_POLL
        ));
        assert!(cyw43_runtime_descriptor_uses_prompt_slice(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL
        ));
        assert!(!cyw43_runtime_descriptor_uses_prompt_slice(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn sdio_engine_init_status_preserves_exact_fault_detail() {
        assert_eq!(
            sdio_engine_init_detail_status(DRIVER_RUNTIME_SDIO_INIT_DETAIL_ADOPT_POWER_MISSING),
            Some("adopt-power-missing")
        );
        assert_eq!(
            sdio_engine_init_detail_status(
                DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH as u16,
            ),
            Some("resource-hot-path-mismatch")
        );
        assert_eq!(
            sdio_engine_init_completion_status(
                Some(DriverTaskCompletionRecord {
                    sequence: 2,
                    code: DriverTaskCompletionCode::Fault.as_u16(),
                    detail: DRIVER_RUNTIME_SDIO_INIT_DETAIL_CLOCK_FAILED,
                    result: 0,
                    frame: DriverFrameDescriptor {
                        offset: 0,
                        len: 0,
                        flags: 0,
                    },
                }),
                false,
            ),
            "clock-failed"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_transport_status_only_reports_ready_at_ready_detail() {
        assert_eq!(
            cyw43_runtime_command_progress_status(
                DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT,
                DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BUS_LINK_READY,
            ),
            "progress"
        );
        assert_eq!(
            cyw43_runtime_command_progress_status(
                DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT,
                DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_READY,
            ),
            "ready"
        );
        assert_eq!(
            cyw43_runtime_command_progress_status(DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP, 0),
            "ready"
        );
        assert!(cyw43_runtime_command_completion_is_progress(
            DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT,
            DriverTaskCompletionRecord {
                sequence: 1,
                code: DriverTaskCompletionCode::Progress.as_u16(),
                detail: DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BUS_LINK_READY,
                result: 0,
                frame: DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            }
        ));
        assert!(!cyw43_runtime_command_completion_is_progress(
            DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP,
            DriverTaskCompletionRecord {
                sequence: 1,
                code: DriverTaskCompletionCode::Progress.as_u16(),
                detail: 0,
                result: 0,
                frame: DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            }
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_transport_admission_reject_requires_current_progress() {
        let descriptor = DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        };
        let completion = DriverTaskCompletionRecord {
            sequence: 3,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: DriverTaskFaultCode::RejectedCommand.as_u16(),
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };

        assert!(cyw43_transport_admission_reject_without_current_progress(
            descriptor, completion, None
        ));
        assert!(cyw43_transport_admission_reject_without_current_progress(
            descriptor,
            completion,
            Some(DriverTaskRingProgressSnapshot {
                marker_valid: true,
                sequence: 0,
                phase: 202,
                phase_name: "runtime-poll-ready",
                aux0: 4,
            }),
        ));
        assert!(!cyw43_transport_admission_reject_without_current_progress(
            descriptor,
            completion,
            Some(DriverTaskRingProgressSnapshot {
                marker_valid: true,
                sequence: 3,
                phase: 202,
                phase_name: "command-observed",
                aux0: DRIVER_RUNTIME_CYW43_COMMAND_AUX,
            }),
        ));
        assert_eq!(
            cyw43_runtime_fault_reason_for_descriptor(descriptor, completion),
            "cyw43-transport-command-admission"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn sdio_host_config_descriptor_encoder_matches_runtime_abi() {
        const TEST_SDIO_STARTUP_CLOCK_HZ: u32 = 400_000;
        let descriptor = pi4_driver_abi::DriverRuntimeSdioCommandDescriptor {
            op: pi4_driver_abi::DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG,
            addr: TEST_SDIO_STARTUP_CLOCK_HZ,
            flags: pi4_driver_abi::DriverRuntimeSdioCommandDescriptor::FLAG_HOST_BUS_WIDTH_4BIT,
            timeout_us: 100_000,
            ..pi4_driver_abi::DriverRuntimeSdioCommandDescriptor::empty()
        };
        let mut bytes =
            [0u8; core::mem::size_of::<pi4_driver_abi::DriverRuntimeSdioCommandDescriptor>()];

        encode_sdio_descriptor(&mut bytes, descriptor);

        assert_eq!(
            &bytes[0..2],
            &pi4_driver_abi::DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG.to_le_bytes()
        );
        assert_eq!(bytes[2], 0);
        assert_eq!(bytes[3], pi4_driver_abi::DRIVER_RUNTIME_SDIO_RESP_NONE);
        assert_eq!(&bytes[4..8], &TEST_SDIO_STARTUP_CLOCK_HZ.to_le_bytes());
        assert_eq!(
            &bytes[16..18],
            &pi4_driver_abi::DriverRuntimeSdioCommandDescriptor::FLAG_HOST_BUS_WIDTH_4BIT
                .to_le_bytes()
        );
        assert_eq!(&bytes[20..24], &100_000u32.to_le_bytes());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_firmware_recovery_preserves_completion_detail() {
        let completion = DriverTaskCompletionRecord {
            sequence: 7,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: 0x5102,
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        let err = Cyw43FirmwareInitError::Command(Cyw43CommandSubmitError::Completion(completion));

        assert_eq!(err.recoverable_completion(), Some(completion));
        assert_eq!(err.same_command_retry_completion(), None);
        let transfer_failed = DriverTaskCompletionRecord {
            sequence: 9,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: 0x5103,
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        let retry =
            Cyw43FirmwareInitError::Command(Cyw43CommandSubmitError::Completion(transfer_failed));
        assert_eq!(retry.recoverable_completion(), Some(transfer_failed));
        assert_eq!(retry.same_command_retry_completion(), Some(transfer_failed));
        let rejected = DriverTaskCompletionRecord {
            sequence: 8,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: 0x5302,
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        assert_eq!(
            Cyw43FirmwareInitError::Command(Cyw43CommandSubmitError::Completion(rejected))
                .recoverable_completion(),
            None
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn sdio_fault_telemetry_decode_matches_runtime_wire_layout() {
        let mut bytes = [0u8; SDIO_FAULT_TELEMETRY_BYTES];
        put_le_u32(&mut bytes, 0, SDIO_FAULT_TELEMETRY_MAGIC);
        put_le_u32(&mut bytes, 4, SDIO_FAULT_TELEMETRY_VERSION);
        put_le_u32(&mut bytes, SDIO_FAULT_TELEMETRY_ARG_OFFSET, 0x9401_0000);
        put_le_u32(
            &mut bytes,
            SDIO_FAULT_TELEMETRY_CMD_FLAGS_OFFSET,
            53 | (0x0021 << 16),
        );
        put_le_u32(
            &mut bytes,
            SDIO_FAULT_TELEMETRY_LEN_BLOCK_OFFSET,
            512 | (512 << 16),
        );
        put_le_u32(
            &mut bytes,
            SDIO_FAULT_TELEMETRY_COUNT_MODE_OFFSET,
            1 | (0x0002 << 16),
        );
        put_le_u32(&mut bytes, SDIO_FAULT_TELEMETRY_PRESENT_OFFSET, 0x0003_0000);
        put_le_u32(
            &mut bytes,
            SDIO_FAULT_TELEMETRY_INT_STATUS_OFFSET,
            0x0020_8040,
        );
        put_le_u32(
            &mut bytes,
            SDIO_FAULT_TELEMETRY_RESPONSE0_OFFSET,
            0x0000_0100,
        );
        put_le_u32(
            &mut bytes,
            SDIO_FAULT_TELEMETRY_HOST_CLOCK_OFFSET,
            0x1234_0e06,
        );
        put_le_u32(&mut bytes, SDIO_FAULT_TELEMETRY_FAILURE_OFFSET, 0x0500_0100);
        put_le_u32(
            &mut bytes,
            SDIO_FAULT_TELEMETRY_BLOCK_REG_OFFSET,
            0x0001_0200,
        );
        put_le_u32(
            &mut bytes,
            SDIO_FAULT_TELEMETRY_PAYLOAD_EDGE_OFFSET,
            0x0033_2211,
        );
        put_le_u32(
            &mut bytes,
            SDIO_FAULT_TELEMETRY_PAYLOAD_SUM_OFFSET,
            0x0000_4444,
        );

        let snapshot = SdioFaultTelemetry::decode(&bytes).expect("valid telemetry");

        assert_eq!(snapshot.cmd, 53);
        assert_eq!(snapshot.flags, 0x0021);
        assert!(snapshot.cmd53_write());
        assert_eq!(snapshot.cmd53_function(), 1);
        assert!(snapshot.cmd53_increment());
        assert!(!snapshot.cmd53_block_mode());
        assert_eq!(snapshot.len, 512);
        assert_eq!(snapshot.block_size, 512);
        assert_eq!(snapshot.block_count, 1);
        assert_eq!(snapshot.transfer_mode, 0x0002);
        assert_eq!(snapshot.host_control, 0x06);
        assert_eq!(snapshot.power_control, 0x0e);
        assert_eq!(snapshot.clock_control, 0x1234);
        assert_eq!(snapshot.failure_result, 0x0500_0100);
        assert_eq!(snapshot.payload_first, 0x11);
        assert_eq!(snapshot.payload_last, 0x22);
        assert_eq!(snapshot.payload_xor, 0x33);
        assert_eq!(snapshot.payload_sum, 0x4444);
        assert_eq!(
            sdio_transfer_failure_stage_label(snapshot.failure_result),
            "response"
        );
        assert_eq!(
            sdio_transfer_failure_reason_label(snapshot.failure_result),
            "sdio-r5-response"
        );
        assert_eq!(sdio_transfer_failure_r5(snapshot.failure_result), 0x0100);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn sdio_transfer_failure_reason_decodes_data_crc_finish() {
        let result = 0x0400_0000 | SDHCI_INT_ERROR | SDHCI_INT_DATA_CRC | 0x40;

        assert_eq!(sdio_transfer_failure_stage_label(result), "data-end");
        assert_eq!(
            sdio_transfer_failure_reason_label(result),
            "sdhci-transfer-finish-data-crc"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_owner_retry_label_reports_actual_sdio_lane() {
        let descriptor = DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        };
        let forced_descriptor = DriverRuntimeCyw43CommandDescriptor {
            flags: DRIVER_RUNTIME_CYW43_FLAG_FORCE_BYTE_MODE,
            ..descriptor
        };
        let primary = SdioFaultTelemetry {
            arg: (1 << 31) | (1 << 28) | (1 << 27) | (1 << 26),
            cmd: 53,
            flags: 0,
            len: 2048,
            block_size: 64,
            block_count: 32,
            transfer_mode: 0x0022,
            present_state: 0,
            int_status: 0,
            response0: 0,
            host_control: 0x06,
            power_control: 0x0f,
            clock_control: 0x5007,
            failure_result: 0x0500_0800,
            block_size_count_reg: 0,
            payload_first: 0,
            payload_last: 0,
            payload_xor: 0,
            payload_sum: 0,
        };
        let byte = SdioFaultTelemetry {
            arg: (1 << 31) | (1 << 28) | (1 << 26) | 64,
            cmd: 53,
            flags: 0,
            len: 64,
            block_size: 64,
            block_count: 1,
            transfer_mode: 0x0002,
            present_state: 0,
            int_status: 0,
            response0: 0,
            host_control: 0x00,
            power_control: 0x0f,
            clock_control: 0x0007,
            failure_result: 0x0500_0800,
            block_size_count_reg: 0,
            payload_first: 0,
            payload_last: 0,
            payload_xor: 0,
            payload_sum: 0,
        };
        let byte512 = SdioFaultTelemetry {
            len: SDIO_CMD53_BYTE_MODE_MAX,
            block_size: SDIO_CMD53_BYTE_MODE_MAX,
            ..byte
        };

        assert_eq!(
            cyw43_owner_retry_label(primary, descriptor, 0x5103),
            "primary"
        );
        assert_eq!(
            cyw43_owner_retry_label(
                SdioFaultTelemetry {
                    host_control: 0x00,
                    clock_control: 0x0007,
                    ..primary
                },
                descriptor,
                0x5103
            ),
            "block-clock-retry"
        );
        assert_eq!(
            cyw43_owner_retry_label(byte, descriptor, 0x5103),
            "byte-narrow-conservative"
        );
        assert_eq!(
            cyw43_owner_retry_label(byte, descriptor, 0x5329),
            "byte-narrow-conservative-exhausted"
        );
        assert_eq!(
            cyw43_owner_retry_label(byte512, descriptor, 0x5103),
            "byte-conservative"
        );
        assert_eq!(
            cyw43_owner_retry_label(byte512, descriptor, 0x5329),
            "byte-conservative-exhausted"
        );
        assert_eq!(
            cyw43_owner_retry_label(byte512, forced_descriptor, 0x5103),
            "forced-byte-mode-conservative"
        );
        assert_eq!(
            cyw43_owner_retry_label(
                SdioFaultTelemetry {
                    host_control: 0x06,
                    ..byte512
                },
                forced_descriptor,
                0x5103
            ),
            "forced-byte-mode-promoted"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_owner_fault_effective_target_tracks_backplane_suboffset() {
        let descriptor = DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            target_addr: CYW43_RAM_BASE_4345,
            payload_offset: 8192,
            payload_len: 8192,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        };
        let snapshot = SdioFaultTelemetry {
            arg: (1 << 31) | (1 << 28) | (1 << 26) | ((CYW43_BACKPLANE_32BIT_FLAG | 0x1600) << 9),
            cmd: 53,
            flags: 0,
            len: 256,
            block_size: 256,
            block_count: 1,
            transfer_mode: 0x0002,
            present_state: 0,
            int_status: 0,
            response0: 0,
            host_control: 0x06,
            power_control: 0x0f,
            clock_control: 0x5007,
            failure_result: 0x0400_8040,
            block_size_count_reg: 0,
            payload_first: 0,
            payload_last: 0,
            payload_xor: 0,
            payload_sum: 0,
        };

        assert_eq!(
            cyw43_owner_effective_backplane_target(descriptor, snapshot),
            Some(CYW43_RAM_BASE_4345 + 0x1600)
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_payload_digest_matches_owner_fault_fields() {
        let payload = [0x10, 0x21, 0x32, 0x43];
        let digest = cyw43_payload_digest(&payload);

        assert_eq!(digest.first, 0x10);
        assert_eq!(digest.last, 0x43);
        assert_eq!(digest.xor, 0x40);
        assert_eq!(digest.sum, 0x0000_00a6);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_control_request_iovar_info_extracts_txglomalign_value() {
        let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
        let mut payload = [0u8; 20];
        payload[..15].copy_from_slice(b"bus:txglomalign");
        payload[16..20].copy_from_slice(&8u32.to_le_bytes());
        let len = cyw43_write_bcdc_frame(
            &mut frame,
            CYW43_WLC_SET_VAR,
            CYW43_BCDC_FLAG_SET,
            1,
            &payload,
        )
        .expect("txglomalign BCDC frame should fit");
        let info = cyw43_control_iovar_info(&frame[..len], CYW43_WLC_SET_VAR)
            .expect("txglomalign iovar should decode");
        let (digest_len, digest_scope) = cyw43_control_request_digest_len(len, Some(info));

        assert_eq!(len, 36);
        assert_eq!(info.name, "bus:txglomalign");
        assert_eq!(info.data_len, 4);
        assert_eq!(info.value_u32, Some(8));
        assert_eq!(
            cyw43_read_le_u16(&frame[..len], 8),
            Some(CYW43_BCDC_FLAG_SET)
        );
        assert_eq!(digest_len, len);
        assert_eq!(digest_scope, "full");
        assert_eq!(Cyw43ControlHeaderMode::Plain.runtime_flags(), 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_control_request_digest_redacts_large_iovar_body() {
        let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
        let mut payload = [0u8; 24];
        payload[..8].copy_from_slice(b"wsec_key");
        payload[9..].fill(0xa5);
        let len = cyw43_write_bcdc_frame(
            &mut frame,
            CYW43_WLC_SET_VAR,
            CYW43_BCDC_FLAG_SET,
            9,
            &payload,
        )
        .expect("wsec_key BCDC frame should fit");
        let info = cyw43_control_iovar_info(&frame[..len], CYW43_WLC_SET_VAR)
            .expect("wsec_key iovar should decode");
        let (digest_len, digest_scope) = cyw43_control_request_digest_len(len, Some(info));

        assert_eq!(info.name, "wsec_key");
        assert_eq!(info.data_len, 15);
        assert_eq!(info.value_u32, None);
        assert_eq!(digest_len, CYW43_BCDC_HEADER_BYTES + "wsec_key".len() + 1);
        assert_eq!(digest_scope, "header-iovar");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn sdio_owner_recovery_preserves_ready_state_before_full_replay() {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK
            .lock()
            .expect("sdio owner recovery tests must serialize ready flag");

        SDIO_LINKED_RUNTIME_READY.store(0, Ordering::Release);
        assert!(!sdio_owner_recovery_can_preserve_ready_state());
        SDIO_LINKED_RUNTIME_READY.store(1, Ordering::Release);
        assert!(sdio_owner_recovery_can_preserve_ready_state());
        SDIO_LINKED_RUNTIME_READY.store(0, Ordering::Release);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_firmware_streaming_uses_bounded_boot_chunks() {
        let desc_size = core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>();
        assert_eq!(
            cyw43_runtime_stream_payload_limit(desc_size).ok(),
            Some(CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES)
        );
        assert_eq!(
            CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES,
            DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as usize
        );
        assert!(CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES > SDIO_CMD53_BYTE_MODE_MAX as usize);
        assert_eq!(CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES % 64, 0);
        assert_eq!(CYW43_RUNTIME_FIRMWARE_OWNER_RECOVERY_ATTEMPTS, 192);
        assert_eq!(CYW43_RUNTIME_FIRMWARE_OWNER_SAME_OFFSET_LIMIT, 24);
        assert_eq!(CYW43_RUNTIME_FIRMWARE_TAIL_PAD_ALIGNMENT, 512);
        assert_eq!(CYW43_RUNTIME_FIRMWARE_TAIL_PAD_MAX_BYTES, 4096);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_firmware_tail_padding_preserves_block_mode_shape() {
        let firmware_len = 609_309usize;
        let tail_offset = firmware_len - 3101;

        assert_eq!(
            cyw43_runtime_firmware_tail_padded_len(
                DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
                tail_offset,
                firmware_len,
                3101,
                CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES
            ),
            3584
        );
        assert_eq!(
            cyw43_runtime_firmware_tail_padded_len(
                DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
                0,
                firmware_len,
                CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES,
                CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES
            ),
            CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES
        );
        assert_eq!(
            cyw43_runtime_firmware_tail_padded_len(
                DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK,
                tail_offset,
                firmware_len,
                3101,
                CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES
            ),
            3101
        );
        assert_eq!(
            cyw43_runtime_firmware_tail_padded_len(
                DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
                firmware_len - 5000,
                firmware_len,
                5000,
                CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES
            ),
            5000
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_firmware_resume_offset_accepts_only_matching_firmware_faults() {
        let fault = Cyw43RuntimeCommandFaultStatus {
            stage: "cyw43-firmware-chunk",
            op: DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            flags: DRIVER_RUNTIME_CYW43_FLAG_FORCE_BYTE_MODE,
            target_addr: CYW43_RAM_BASE_4345 + 0x1c00,
            payload_offset: 4096,
            payload_len: 1024,
            total_len: 609_309,
            control_cmd: 0,
            control_id: 0,
            control_header_mode: "not-control",
            control_response_len: 0,
            detail: 0x5103,
            reason: "sdio-descriptor-transfer-failed",
            result: 0x0500_0100,
        };

        assert_eq!(cyw43_firmware_resume_offset(fault, 609_309), Some(0x1c00));
        assert_eq!(
            cyw43_firmware_resume_offset(
                Cyw43RuntimeCommandFaultStatus {
                    target_addr: CYW43_RAM_BASE_4345 + 606_208,
                    payload_len: 3584,
                    ..fault
                },
                609_309
            ),
            Some(606_208)
        );

        assert_eq!(
            cyw43_firmware_resume_offset(
                Cyw43RuntimeCommandFaultStatus {
                    op: DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK,
                    ..fault
                },
                609_309
            ),
            None
        );
        assert_eq!(
            cyw43_firmware_resume_offset(
                Cyw43RuntimeCommandFaultStatus {
                    target_addr: CYW43_RAM_BASE_4345 - 1,
                    ..fault
                },
                609_309
            ),
            None
        );
        assert_eq!(cyw43_firmware_resume_offset(fault, 609_308), None);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_nvram_resume_reenters_tail_after_exact_nvram_fault() {
        let firmware_len = 609_309usize;
        let nvram_len = 1_744usize;
        let nvram_base = CYW43_RAM_BASE_4345 + CYW43_RAM_SIZE_4345_PI4 - 4 - nvram_len as u32;
        let fault = Cyw43RuntimeCommandFaultStatus {
            stage: "cyw43-nvram-chunk",
            op: DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK,
            flags: 0,
            target_addr: nvram_base,
            payload_offset: 4096,
            payload_len: nvram_len as u16,
            total_len: nvram_len as u32,
            control_cmd: 0,
            control_id: 0,
            control_header_mode: "not-control",
            control_response_len: 0,
            detail: 0x5329,
            reason: "cyw43-firmware-retry-exhausted",
            result: 0x0500_0800,
        };

        assert_eq!(
            cyw43_nvram_tail_resume_offset(fault, firmware_len, nvram_len),
            Some(firmware_len)
        );
        assert_eq!(
            cyw43_nvram_tail_resume_offset(
                Cyw43RuntimeCommandFaultStatus {
                    target_addr: nvram_base + 4,
                    ..fault
                },
                firmware_len,
                nvram_len
            ),
            None
        );
        assert_eq!(
            cyw43_nvram_tail_resume_offset(
                Cyw43RuntimeCommandFaultStatus {
                    op: DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
                    ..fault
                },
                firmware_len,
                nvram_len
            ),
            None
        );
        assert_eq!(
            cyw43_nvram_tail_resume_offset(
                Cyw43RuntimeCommandFaultStatus {
                    payload_len: (nvram_len - 4) as u16,
                    ..fault
                },
                firmware_len,
                nvram_len
            ),
            None
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_firmware_resume_retries_retry_exhaustion_on_primary_lane() {
        let fault = Cyw43RuntimeCommandFaultStatus {
            stage: "cyw43-firmware-chunk",
            op: DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            flags: 0,
            target_addr: CYW43_RAM_BASE_4345,
            payload_offset: 4096,
            payload_len: 1024,
            total_len: 609_309,
            control_cmd: 0,
            control_id: 0,
            control_header_mode: "not-control",
            control_response_len: 0,
            detail: 0x5329,
            reason: "cyw43-firmware-retry-exhausted",
            result: 0x0420_8040,
        };
        assert!(!cyw43_firmware_resume_forces_byte_mode(fault));
        assert!(cyw43_firmware_resume_forces_byte_mode(
            Cyw43RuntimeCommandFaultStatus {
                detail: 0x5103,
                reason: "sdio-owner-response-fault",
                ..fault
            }
        ));
        assert!(!cyw43_firmware_resume_forces_byte_mode(
            Cyw43RuntimeCommandFaultStatus {
                op: DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK,
                ..fault
            }
        ));
        assert!(!cyw43_firmware_resume_forces_byte_mode(
            Cyw43RuntimeCommandFaultStatus {
                detail: 0x5302,
                ..fault
            }
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_firmware_stream_progress_is_coarse_and_bounded() {
        assert!(cyw43_runtime_stream_progress_due(0, 1024, 609_309));
        assert!(!cyw43_runtime_stream_progress_due(1024, 1024, 609_309));
        assert!(cyw43_runtime_stream_progress_due(
            CYW43_RUNTIME_STREAM_PROGRESS_INTERVAL - 1024,
            1024,
            609_309
        ));
        assert!(cyw43_runtime_stream_progress_due(608_256, 1053, 609_309));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn tx_completion_requires_nonzero_progress() {
        use crate::hal::driver_task::{
            DriverTaskCompletionCode, DriverTaskCompletionRecord, DriverTaskFaultCode,
        };

        assert!(driver_task_tx_completion_submitted(
            DriverTaskCompletionRecord::progress(7, 1)
        ));
        assert!(!driver_task_tx_completion_submitted(
            DriverTaskCompletionRecord::progress(8, 0)
        ));
        assert!(!driver_task_tx_completion_submitted(
            DriverTaskCompletionRecord::idle(9)
        ));
        assert!(!driver_task_tx_completion_submitted(
            DriverTaskCompletionRecord::fault(10, DriverTaskFaultCode::RejectedCommand)
        ));
        assert_eq!(
            DriverTaskCompletionCode::Progress.as_u16(),
            DriverTaskCompletionRecord::progress(11, 1).code
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_ring_service_admits_frame_bearing_tx_commands() {
        use crate::hal::driver_task::{
            DriverTaskCompletionCode, DriverTaskFaultCode, DriverTaskHotPath,
        };

        let contract = GENET_DRIVER_TASK_CONTRACT;
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        crate::hal::driver_task::publish_driver_task_ring(
            contract,
            ring_page.as_mut_ptr() as usize,
        );
        let frame = crate::hal::driver_task::stage_driver_task_ring_frame(contract, &[0xa5; 32], 0)
            .expect("test ring has room for one TX frame");
        let command = DriverTaskCommandRecord::pi4_hot_path(
            21,
            DriverTaskHotPath::GenetNic,
            DriverTaskBudgetGrant::from_contract(contract),
            frame,
        );

        let completion =
            unsafe { runtime_ring_service(DriverTaskHotPath::GenetNic.as_u32() as usize, command) };
        crate::hal::driver_task::publish_driver_task_ring(contract, 0);

        assert_eq!(completion.sequence, 21);
        assert_eq!(completion.code, DriverTaskCompletionCode::Fault.as_u16());
        assert_eq!(
            completion.detail,
            DriverTaskFaultCode::DeviceUnavailable.as_u16()
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_init_command_requires_a_driver_task_init_lease() {
        use crate::hal::driver_task::{
            DriverTaskCompletionCode, DriverTaskFaultCode, DriverTaskHotPath,
        };

        let mut command = DriverTaskCommandRecord::pi4_hot_path(
            22,
            DriverTaskHotPath::Cyw43Wifi,
            DriverTaskBudgetGrant::from_contract(CYW43_WIFI_DRIVER_TASK_CONTRACT),
            DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        command.aux0 = DRIVER_RUNTIME_NET_INIT_AUX;

        let completion = unsafe {
            runtime_ring_service(DriverTaskHotPath::Cyw43Wifi.as_u32() as usize, command)
        };

        assert_eq!(completion.sequence, 22);
        assert_eq!(completion.code, DriverTaskCompletionCode::Fault.as_u16());
        assert_eq!(
            completion.detail,
            DriverTaskFaultCode::DeviceUnavailable.as_u16()
        );
    }
}
