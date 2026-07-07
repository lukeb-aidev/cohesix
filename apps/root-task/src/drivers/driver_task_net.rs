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
use smoltcp::wire::{EthernetAddress, Ipv4Address};
use spin::Mutex;

use crate::drivers::cyw43_host_eapol::{
    self, HostEapolAction, HostEapolFrameProof, HostEapolState, ETHER_ADDR_LEN, ETH_HEADER_LEN,
    ETH_P_EAPOL, WPA2_PSK_CCMP_RSN_IE, WSEC_KEY_PAYLOAD_LEN,
};
#[cfg(feature = "kernel")]
use crate::hal::driver_task::DriverTaskRingProgressSnapshot;
use crate::hal::driver_task::{
    DriverFrameDescriptor, DriverTaskBudgetGrant, DriverTaskCommandRecord,
    DriverTaskCompletionCode, DriverTaskCompletionRecord, DriverTaskContract, DriverTaskFaultCode,
    DriverTaskHotPath, DriverTaskStagingSegment, CYW43_WIFI_DRIVER_TASK_CONTRACT,
    DRIVER_TASK_RING_FLAG_QUIET_HOT_PATH, GENET_DRIVER_TASK_CONTRACT, MAX_DRIVER_TASK_FRAME_BYTES,
    SDIO_HOST_DRIVER_TASK_CONTRACT,
};
use crate::hal::{HalError, Hardware};
use crate::net::{
    ConsoleNetConfig, NetDevice, NetDeviceCounters, NetDriverError, NetInterfacePolicy, NetStage,
    WifiCredentials, MAX_FRAME_LEN, NET_DIAG,
};
use pi4_driver_abi::{
    driver_runtime_genet_result_is_packed, driver_runtime_genet_result_rx_byte_budget_hit,
    driver_runtime_genet_result_rx_drain_budget_hit,
    driver_runtime_genet_result_rx_max_drained_per_turn,
    driver_runtime_genet_result_rx_overflow_seen, driver_runtime_genet_result_rx_queue_count,
    driver_runtime_genet_result_rx_queue_high_water, driver_runtime_genet_result_tx_free,
    driver_runtime_genet_result_tx_in_flight, DriverRuntimeCyw43CommandDescriptor,
    DriverRuntimeSdioCommandDescriptor, DRIVER_RUNTIME_CYW43_COMMAND_AUX,
    DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER, DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN,
    DRIVER_RUNTIME_CYW43_FLAG_FORCE_BYTE_MODE, DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
    DRIVER_RUNTIME_CYW43_FLAG_RX_STEADY_TAIL_DRAIN,
    DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL, DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
    DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT, DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_MASK,
    DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_MASK, DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT,
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
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_SOURCE_ASSERTED_EMPTY,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_INVALID_RFRAME_LEN,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_NOT_READY, DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_NO_RFRAME,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_RFRAME_READ_FAILED,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_RX_REQUEST_TOO_LARGE,
    DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_SDPCM_DECODE_MISS,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_CACHED,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_CARD_INTERRUPT,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FRAME_INDICATED,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FUNCTION2_READY,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_HOST_INTERRUPT,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_MASK,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_SHIFT, DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_MAGIC,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_PASSIVE,
    DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_PROBE_LEN_MASK,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BACKPLANE_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_BUS_LINK_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_CARD_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F1_BLOCK_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F1_ENABLED,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_F2_BLOCK_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_HOST_READY, DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_READY,
    DRIVER_RUNTIME_CYW43_TRANSPORT_DETAIL_START, DRIVER_RUNTIME_ENGINE_INIT_AUX,
    DRIVER_RUNTIME_NET_INIT_AUX, DRIVER_RUNTIME_RING_PROGRESS_ENGINE_INIT_RESOURCE_CHECK_FAILED,
    DRIVER_RUNTIME_RING_PROGRESS_RESOURCE_HOT_PATH_MISMATCH,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_ADOPT_CLOCK_FAILED,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_ADOPT_INHIBIT_FAILED,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_ADOPT_POWER_MISSING,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_CLOCK_FAILED, DRIVER_RUNTIME_SDIO_INIT_DETAIL_INHIBIT_FAILED,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_RESET_ALL_FAILED,
    DRIVER_RUNTIME_SDIO_INIT_DETAIL_RESET_CMD_DATA_FAILED, DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG,
    DRIVER_RUNTIME_SDIO_RESP_NONE, DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES,
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
const CYW43_SDIO_HOST_REPRIME_CLOCK_HZ: u32 = 50_000_000;
const CYW43_SDIO_HOST_REPRIME_TIMEOUT_US: u32 = 100_000;
const CYW43_RUNTIME_TRANSPORT_NO_REPLY_RESUMES: usize = 8;
const CYW43_RUNTIME_NESTED_SDIO_NO_REPLY_RESUMES: usize = 32_768;
const CYW43_RUNTIME_CONTROL_EXCHANGE_NO_REPLY_RESUMES: usize =
    CYW43_RUNTIME_NESTED_SDIO_NO_REPLY_RESUMES;
const CYW43_RUNTIME_CONTROL_FRAME_NO_REPLY_RESUMES: usize =
    CYW43_RUNTIME_NESTED_SDIO_NO_REPLY_RESUMES;
const CYW43_RUNTIME_FIRMWARE_RELEASE_NO_REPLY_RESUMES: usize =
    CYW43_RUNTIME_NESTED_SDIO_NO_REPLY_RESUMES;
const CYW43_RUNTIME_CONTROL_POLL_NO_REPLY_RESUMES: usize = 63;
const CYW43_RUNTIME_DATA_POLL_NO_REPLY_RESUMES: usize = 63;
const CYW43_RUNTIME_DATA_TX_NO_REPLY_RESUMES: usize = 63;
const CYW43_DESCRIPTOR_UNAVAILABLE_DETAIL: u16 = 0x5309;
const CYW43_SDIO_DESCRIPTOR_TRANSFER_FAILED_DETAIL: u16 = 0x5103;
const CYW43_CONTROL_FRAME_DETAIL: u16 = 0x5306;
const CYW43_SDIO_POST_RELEASE_HT_CLOCK_DETAIL: u16 = 0x532a;
const CYW43_SDIO_FUNCTION2_NOT_READY_DETAIL: u16 = 0x532b;
const CYW43_RUNTIME_DESCRIPTOR_UNAVAILABLE_RETRIES: usize = 2;
// One same-descriptor replay keeps the first SDIO owner fault visible while
// allowing the split control path to survive a single Function 2 CMD53 owner
// boundary fault before the control plane fails closed.
const CYW43_CONTROL_TX_SUBMIT_RETRIES: usize = 1;
const CYW43_RUNTIME_TRANSPORT_PHASE_ATTEMPTS: usize = 128;
const CYW43_RUNTIME_FIRMWARE_OWNER_RECOVERY_ATTEMPTS: usize = 192;
const CYW43_RUNTIME_FIRMWARE_OWNER_SAME_OFFSET_LIMIT: usize = 24;
const CYW43_BACKPLANE_ADDRESS_MASK: u32 = 0x7fff;
const CYW43_BACKPLANE_WINDOW_MASK: u32 = 0xffff_8000;
const CYW43_BACKPLANE_32BIT_FLAG: u32 = 0x8000;
const CYW43_CONTROL_PLANE_POLL_ATTEMPTS: usize = 256;
const CYW43_HOST_EAPOL_WSEC_KEY_POLL_ATTEMPTS: usize = 8_000;
const CYW43_HOST_EAPOL_POST_SECURE_WSEC_KEY_POLL_ATTEMPTS: usize = 512;
const CYW43_CONTROL_PLANE_REPLY_TIMEOUT_MS: u64 = 1_000;
const CYW43_HOST_EAPOL_WSEC_KEY_REPLY_TIMEOUT_MS: u64 = 2_500;
const CYW43_HOST_EAPOL_POST_SECURE_WSEC_KEY_REPLY_TIMEOUT_MS: u64 = 250;
const CYW43_CONTROL_EXCHANGE_TIMEOUT_RESULT_MAGIC: u32 = 0x4300_0000;
const CYW43_HOST_EAPOL_PRE_ASSOC_POLLS: usize = 8_192;
const CYW43_HOST_EAPOL_POST_ASSOC_POLLS: usize = 16_384;
const CYW43_HOST_EAPOL_JOIN_POLLS: usize =
    CYW43_HOST_EAPOL_PRE_ASSOC_POLLS + CYW43_HOST_EAPOL_POST_ASSOC_POLLS;
const CYW43_HOST_EAPOL_JOIN_SUBMIT_POLLS: usize = CYW43_HOST_EAPOL_PRE_ASSOC_POLLS + 1;
const CYW43_HOST_EAPOL_START_FIRST_POLL: usize = 8_192;
const CYW43_HOST_EAPOL_START_INTERVAL_POLLS: usize = 8192;
const CYW43_HOST_EAPOL_PRE_ASSOC_TIMEOUT_MS: u64 = 8_192;
const CYW43_HOST_EAPOL_POST_ASSOC_TIMEOUT_MS: u64 = 16_384;
const CYW43_HOST_EAPOL_JOIN_TIMEOUT_MS: u64 = 24_576;
const CYW43_HOST_EAPOL_START_FIRST_MS: u64 = 8_192;
const CYW43_HOST_EAPOL_START_INTERVAL_MS: u64 = 8_192;
const CYW43_HOST_EAPOL_START_MAX: u32 = 12;
const CYW43_DATA_TX_ATTEMPTS: usize = 8;
const CYW43_DATA_TX_RETRY_RECOVERY_POLLS: usize = 4;
const CYW43_DATA_TX_CREDIT_PROOF_POLLS: usize = 16;
const CYW43_DATA_TX_POST_SUBMIT_RECOVERY_POLLS: usize = 8;
const CYW43_DATA_TX_ADMISSION_RECOVERY_POLLS: usize = 16;
const CYW43_DATA_TX_MIN_FUNCTION2_BYTES: usize = 128;
const CYW43_DATA_RX_STEADY_POLL_FLAGS: u16 = DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD
    | DRIVER_RUNTIME_CYW43_FLAG_RX_STEADY_TAIL_DRAIN;
const CYW43_HOST_EAPOL_TX_ATTEMPTS: usize = 8;
const CYW43_HOST_EAPOL_TX_DRAIN_POLLS: usize = 2_000;
const CYW43_HOST_EAPOL_TX_DRAIN_TIMEOUT_MS: u64 = 2_000;
const CYW43_HOST_EAPOL_RX_ADMISSION_POLL_ATTEMPTS: usize = 4_096;
const CYW43_HOST_EAPOL_RX_ADMISSION_REPLY_TIMEOUT_MS: u64 = 4_096;
const CYW43_HOST_EAPOL_RX_REFRESH_AFTER_POST_ASSOC_POLLS: u32 = 1_024;
const CYW43_HOST_EAPOL_RX_RESCUE_AFTER_POST_ASSOC_POLLS: u32 = 4_096;
const CYW43_HOST_EAPOL_RX_REFRESH_AFTER_POST_ASSOC_MS: u64 = 1_024;
const CYW43_HOST_EAPOL_RX_RESCUE_AFTER_POST_ASSOC_MS: u64 = 4_096;
const CYW43_HOST_EAPOL_RX_RESCUE_AFTER_STARTS: u32 = 2;
const CYW43_HOST_EAPOL_BSSID_REFRESH_TX_RETRIES: usize = 1;
const CYW43_HOST_EAPOL_BSSID_REFRESH_PRE_TX_DRAIN: bool = false;
const CYW43_HOST_EAPOL_PROMISC_PRE_TX_DRAIN: bool = true;
const CYW43_HOST_EAPOL_WSEC_REASSERT_PRE_TX_DRAIN: bool = true;
const CYW43_HOST_EAPOL_ASSOC_PROBE_POLLS: [u32; 4] = [1_024, 4_096, 8_192, 16_384];
const CYW43_HOST_EAPOL_ASSOC_RESCUE_POLL: u32 = CYW43_HOST_EAPOL_PRE_ASSOC_POLLS as u32;
const CYW43_HOST_EAPOL_ASSOC_PROBE_MS: [u64; 4] = [1_024, 4_096, 8_192, 16_384];
const CYW43_HOST_EAPOL_ASSOC_RESCUE_MS: u64 = CYW43_HOST_EAPOL_PRE_ASSOC_TIMEOUT_MS;
const CYW43_HOST_EAPOL_PRE_ASSOC_FIRSTREAD_MS: [u64; 8] = [0, 1, 4, 16, 64, 256, 1_024, 4_096];
const CYW43_HOST_EAPOL_POST_START_FIRSTREAD_MS: [u64; 6] = [1, 4, 16, 64, 256, 1_024];
const CYW43_SDIO_EXPECTED_IENX: u8 = 0x07;
const CYW43_BCDC_HEADER_BYTES: usize = 16;
const CYW43_BDC_HEADER_BYTES: usize = 4;
const CYW43_BDC_VERSION: u8 = 2;
const CYW43_BDC_VERSION_SHIFT: u8 = 4;
const CYW43_SDPCM_HEADER_BYTES: usize = 12;
const CYW43_SDPCM_HWEXT_BYTES: usize = 8;
const CYW43_SDPCM_DATA_TX_HEADER_BYTES: usize = CYW43_SDPCM_HEADER_BYTES + CYW43_SDPCM_HWEXT_BYTES;
const CYW43_SDPCM_DATA_TX_PADDING_BYTES: usize = 6;
const CYW43_SDPCM_DATA_TX_BDC_OFFSET: usize =
    CYW43_SDPCM_DATA_TX_HEADER_BYTES + CYW43_SDPCM_DATA_TX_PADDING_BYTES;
const CYW43_SDPCM_DATA_TX_OVERHEAD_BYTES: usize =
    CYW43_SDPCM_DATA_TX_BDC_OFFSET + CYW43_BDC_HEADER_BYTES;
const CYW43_FUNCTION2_BLOCK_BYTES: usize = 512;
const CYW43_HOST_EAPOL_BDC_PRIORITY: u8 = 6;
const CYW43_REVINFO_RESPONSE_BYTES: usize = 68;
const CYW43_CLM_IOVAR_NAME: &str = "clmload";
const CYW43_CLM_IOVAR_NAME_WITH_NUL_BYTES: usize = 8;
const CYW43_CLM_IOVAR_HEADER_BYTES: usize = 12;
const CYW43_CLM_CHUNK_BYTES: usize = 1400;
const CYW43_CLM_VERSION_RESPONSE_BYTES: usize = 256;
const CYW43_CLM_DOWNLOAD_FLAG_BEGIN: u16 = 0x0002;
const CYW43_CLM_DOWNLOAD_FLAG_END: u16 = 0x0004;
const CYW43_CLM_DOWNLOAD_FLAG_HANDLER_VER: u16 = 0x1000;
const CYW43_CLM_DOWNLOAD_TYPE: u16 = 2;
const CYW43_BCDC_FLAG_GET: u16 = 0x0000;
const CYW43_BCDC_FLAG_SET: u16 = 0x0002;
const CYW43_WLC_UP: u32 = 2;
const CYW43_WLC_SET_PROMISC: u32 = 10;
const CYW43_WLC_SET_INFRA: u32 = 20;
const CYW43_WLC_GET_BSSID: u32 = 23;
const CYW43_WLC_SET_SSID: u32 = 26;
const CYW43_WLC_SET_PM: u32 = 86;
const CYW43_WLC_GET_REVINFO: u32 = 98;
const CYW43_WLC_SET_SCAN_CHANNEL_TIME: u32 = 185;
const CYW43_WLC_SET_SCAN_UNASSOC_TIME: u32 = 187;
const CYW43_WLC_GET_VAR: u32 = 262;
const CYW43_WLC_SET_VAR: u32 = 263;
const CYW43_CONTROL_EXCHANGE_FAULT_DETAIL: u16 = 0x530b;
const CYW43_BCME_UNSUPPORTED_STATUS: u32 = 0xffff_ffe9;
const CYW43_BCME_NOTASSOCIATED_STATUS: u32 = 0xffff_ffef;
const CYW43_BCME_BADARG_STATUS: u32 = 0xffff_fffe;
const CYW43_WSEC_NONE: u32 = 0;
const CYW43_WSEC_AES: u32 = 4;
const CYW43_MFP_NONE: u32 = 0;
const CYW43_WME_BSS_DISABLE_RSN_DEFAULT: u32 = 1;
const CYW43_PM_OFF: u32 = 0;
const CYW43_LINUX_PREJOIN_MPC_VALUE: Option<u32> = None;
const CYW43_LINUX_SCAN_CHANNEL_TIME_MS: u32 = 40;
const CYW43_LINUX_SCAN_UNASSOC_TIME_MS: u32 = 40;
const CYW43_BSSCFG_PRIMARY_INDEX: u32 = 0;
const CYW43_SUP_WPA2_EAPVER_ANY: u32 = u32::MAX;
const CYW43_SUP_WPA_TIMEOUT_MS: u32 = 2500;
const CYW43_CONNECT_STATION_POLICY_DISABLED: u32 = 0;
const CYW43_JOIN_SECURITY_ORDER_LABEL: &str =
    "connect-policy-wpaie-wpa_auth-initial-auth-wsec-rsn-cap-policy-wpa_auth-final";
const CYW43_LINUX_CONNECT_STATION_POLICY_IOVARS: [(&str, u32); 4] = [
    ("mpc", CYW43_CONNECT_STATION_POLICY_DISABLED),
    ("arp_ol", CYW43_CONNECT_STATION_POLICY_DISABLED),
    ("arpoe", CYW43_CONNECT_STATION_POLICY_DISABLED),
    ("ndoe", CYW43_CONNECT_STATION_POLICY_DISABLED),
];
const CYW43_LINUX_JOIN_PREF_DEFAULT: [u8; 8] = [0x04, 0x02, 0x08, 0x01, 0x01, 0x02, 0x00, 0x00];
const CYW43_WPA_AUTH_DISABLED: u32 = 0;
const CYW43_WPA2_AUTH_PSK_OR_UNSPECIFIED: u32 = 0x00c0;
const CYW43_WPA2_AUTH_PSK: u32 = 0x0080;
const CYW43_ETH_P_LINK_CTL: u16 = 0x886c;
const CYW43_ETH_P_IPV4: u16 = 0x0800;
const CYW43_ETH_P_ARP: u16 = 0x0806;
const CYW43_ETH_P_IPV6: u16 = 0x86dd;
const CYW43_IP_PROTO_TCP: u8 = 6;
const CYW43_IP_PROTO_UDP: u8 = 17;
const CYW43_DHCP_SERVER_PORT: u16 = 67;
const CYW43_DHCP_CLIENT_PORT: u16 = 68;
const CYW43_DHCP_MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
const CYW43_DHCP_FIXED_BYTES: usize = 236;
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
const CYW43_EVENT_IF: u8 = 54;
const CYW43_EVENT_FLAG_LINK: u16 = 0x0001;
const CYW43_EVENT_STATUS_SUCCESS: u32 = 0;
const CYW43_EVENT_STATUS_TIMEOUT: u32 = 2;
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
const SDHCI_HOST_CONTROL_4BIT: u8 = 0x02;
const SDHCI_HOST_CONTROL_HIGH_SPEED: u8 = 0x04;
const SDHCI_CLOCK_INT_EN: u16 = 1 << 0;
const SDHCI_CLOCK_INT_STABLE: u16 = 1 << 1;
const SDHCI_CLOCK_CARD_EN: u16 = 1 << 2;
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
static GENET_TX_HW_COMPLETED: AtomicU32 = AtomicU32::new(0);
static GENET_TX_HW_FREE: AtomicU32 = AtomicU32::new(0);
static GENET_TX_HW_IN_FLIGHT: AtomicU32 = AtomicU32::new(0);
static GENET_RX_HW_FRAMES: AtomicU32 = AtomicU32::new(0);
static GENET_RX_LAST_ETHERTYPE: AtomicU32 = AtomicU32::new(0);
static GENET_RX_LAST_LEN: AtomicU32 = AtomicU32::new(0);
static GENET_RX_RUNTIME_QUEUE_COUNT: AtomicU32 = AtomicU32::new(0);
static GENET_RX_RUNTIME_QUEUE_HIGH_WATER: AtomicU32 = AtomicU32::new(0);
static GENET_RX_RUNTIME_QUEUE_OVERFLOW_SEEN: AtomicU32 = AtomicU32::new(0);
static GENET_RX_RUNTIME_DRAIN_BUDGET_HIT: AtomicU32 = AtomicU32::new(0);
static GENET_RX_RUNTIME_BYTE_BUDGET_HIT: AtomicU32 = AtomicU32::new(0);
static GENET_RX_RUNTIME_MAX_DRAINED_PER_TURN: AtomicU32 = AtomicU32::new(0);
static GENET_PENDING_RX_HIGH_WATER: AtomicU32 = AtomicU32::new(0);
static GENET_PENDING_RX_DROPS: AtomicU32 = AtomicU32::new(0);
static GENET_ARP_RX: AtomicU32 = AtomicU32::new(0);
static GENET_ARP_TX: AtomicU32 = AtomicU32::new(0);
static CYW43_PENDING_RX_HIGH_WATER: AtomicU32 = AtomicU32::new(0);
static CYW43_PENDING_RX_DROPS: AtomicU32 = AtomicU32::new(0);
static CYW43_TX_SUBMITTED: AtomicU32 = AtomicU32::new(0);
static CYW43_TX_DROPPED: AtomicU32 = AtomicU32::new(0);
static CYW43_RX_FRAMES: AtomicU32 = AtomicU32::new(0);
static CYW43_TX_CREDIT_COMPLETED: AtomicU32 = AtomicU32::new(0);
static CYW43_TX_CREDIT_UNPROVEN: AtomicU32 = AtomicU32::new(0);
const CYW43_TX_UNPROVEN_NONE: u32 = 0;
const CYW43_TX_UNPROVEN_KNOWN: u32 = 1;
const CYW43_TX_UNPROVEN_UNKNOWN: u32 = 2;
static CYW43_TX_UNPROVEN_ACTIVE: AtomicU32 = AtomicU32::new(0);
static CYW43_TX_UNPROVEN_SEQ: AtomicU32 = AtomicU32::new(0);
static CYW43_TX_UNPROVEN_COUNT: AtomicU32 = AtomicU32::new(0);
static CYW43_RX_RUNTIME_QUEUE_COUNT: AtomicU32 = AtomicU32::new(0);
static CYW43_RX_RUNTIME_QUEUE_HIGH_WATER: AtomicU32 = AtomicU32::new(0);
static CYW43_RX_RUNTIME_QUEUE_OVERFLOW_SEEN: AtomicU32 = AtomicU32::new(0);
static CYW43_RX_RUNTIME_DRAIN_BUDGET_HIT: AtomicU32 = AtomicU32::new(0);
static CYW43_RX_RUNTIME_MAX_DRAINED_PER_TURN: AtomicU32 = AtomicU32::new(0);
static CYW43_ARP_RX: AtomicU32 = AtomicU32::new(0);
static CYW43_ARP_TX: AtomicU32 = AtomicU32::new(0);
static GENET_LINKED_RUNTIME_READY: AtomicU32 = AtomicU32::new(0);
static CYW43_LINKED_RUNTIME_READY: AtomicU32 = AtomicU32::new(0);
static CYW43_CONTROL_PLANE_READY: AtomicU32 = AtomicU32::new(0);
static CYW43_ASSOCIATED: AtomicU32 = AtomicU32::new(0);
static CYW43_LINK_UP: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_RX: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_START: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_M1: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_M2: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_M3: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_M4: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_PTK: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_GTK: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_ACTIVE: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_REQUIRED: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_SECURE: AtomicU32 = AtomicU32::new(0);
static CYW43_POST_SECURE_DATA_RX_ADMITTED: AtomicU32 = AtomicU32::new(0);
static CYW43_DATA_TX_RETRIES: AtomicU32 = AtomicU32::new(0);
static CYW43_HOST_EAPOL_TX_RETRIES: AtomicU32 = AtomicU32::new(0);
static CYW43_PRIMARY_BSSCFG_JOIN_READY: AtomicU32 = AtomicU32::new(0);
static CYW43_ASSIGNED_IPV4_BE: AtomicU32 = AtomicU32::new(0);
static CYW43_DATA_TRACE_DHCP_COUNT: AtomicU32 = AtomicU32::new(0);
static CYW43_DATA_TRACE_EAPOL_CONSUME_COUNT: AtomicU32 = AtomicU32::new(0);
static CYW43_DATA_TRACE_FAULT_COUNT: AtomicU32 = AtomicU32::new(0);
static CYW43_DATA_TRACE_DROP_COUNT: AtomicU32 = AtomicU32::new(0);
static CYW43_DATA_TRACE_PENDING_COUNT: AtomicU32 = AtomicU32::new(0);
static CYW43_DATA_TRACE_TX_RETRY_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_DATA_TX_TEST_STUB: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_DATA_TX_TEST_FAILS_BEFORE_SUCCESS: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_DATA_TX_TEST_IDLE_BEFORE_SUCCESS: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_DATA_TX_TEST_FAULTS_BEFORE_SUCCESS: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_DATA_TX_TEST_SUCCESS_WITHOUT_CREDIT: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_DATA_TX_TEST_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_IO_STUB: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_TX_SUBMITTED: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_PTK_INSTALLED: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_GTK_INSTALLED: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_WSEC_REASSERTED: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_TX_DRAINED: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_TX_DRAIN_TIMEOUTS: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_DRAIN_BEFORE_PTK: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_DRAIN_BEFORE_SECURE: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_WSEC_PRE_TX_DRAIN: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_RX_RESTORED: AtomicU32 = AtomicU32::new(0);
#[cfg(test)]
static CYW43_HOST_EAPOL_TEST_BSSID_VALID: AtomicU32 = AtomicU32::new(0);
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
const fn cyw43_control_runtime_flags(
    header_mode: Cyw43ControlHeaderMode,
    pre_tx_drain: bool,
) -> u16 {
    let mut flags = header_mode.runtime_flags();
    if pre_tx_drain {
        flags |= DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN;
    }
    flags
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
    pub cmd53_count: u16,
    pub desc_block_count: u16,
    pub host_block_count: u16,
    pub transfer_mode: u16,
    pub host_control: u8,
    pub host_mode: &'static str,
    pub power_control: u8,
    pub clock_control: u16,
    pub clock_state: &'static str,
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
    passive: bool,
    cached: bool,
}

#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_MAGIC: u32 = 0x4352_5854;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_VERSION_V1: u16 = 1;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_VERSION_V2: u16 = 2;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_VERSION_V3: u16 = 3;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_VERSION_V4: u16 = 4;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_VERSION_V5: u16 = 5;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_VERSION: u16 = 6;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_BYTES: usize = 40;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_V2_BYTES: usize = 60;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_V3_BYTES: usize = 108;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_V4_BYTES: usize = 120;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_V5_BYTES: usize = 136;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_V6_BYTES: usize = 156;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_MASK: u16 = 0x000f;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_BLOCK: u16 = 1;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_CLEAR_STALE: u16 = 2;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_READ_ASSERTED_ZERO: u16 = 3;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_READ_RFRAME_READY: u16 = 4;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_READ_SOURCE_ASSERTED: u16 = 5;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_SOURCE_FLAG_PRE_FRESH: u16 = 1 << 0;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_SOURCE_FLAG_PRE_ASSERTED: u16 = 1 << 1;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_SOURCE_FLAG_POST_FRESH: u16 = 1 << 2;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_SOURCE_FLAG_POST_ASSERTED: u16 = 1 << 3;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_SOURCE_FLAG_PRE_FAILED: u16 = 1 << 4;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_SOURCE_FLAG_POST_FAILED: u16 = 1 << 5;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_SOURCE_FLAG_EVER_ASSERTED: u16 = 1 << 6;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_FIFO_FLAG_SET_OK: u16 = 1 << 0;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_FIFO_FLAG_READBACK_OK: u16 = 1 << 1;
#[cfg(feature = "kernel")]
const CYW43_RX_IDLE_TRACE_FIFO_FLAG_READBACK_MATCH: u16 = 1 << 2;

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Cyw43RxIdleTrace {
    valid: bool,
    flags: u16,
    detail: u16,
    probe_len: u16,
    source_result: u32,
    prefix_signature: u32,
    prefix_digest: u32,
    rframe_len: u16,
    firstread_reads: u16,
    block_reads: u16,
    rframe_reads: u16,
    request_len: u16,
    block_size: u16,
    block_count: u16,
    retransmit_sample: u16,
    queue_depth: u16,
    queue_high_water: u16,
    cmd53_arg: u32,
    transfer_result: u32,
    payload_before_digest: u32,
    payload_after_digest: u32,
    pre_source_result: u32,
    post_source_result: u32,
    source_flags: u16,
    first_nonzero_offset: u16,
    first_nonzero_byte: u16,
    fifo_window_flags: u16,
    pre_intstatus: u32,
    post_intstatus: u32,
    pre_sdhci_status: u32,
    post_sdhci_status: u32,
    fifo_window_requested: u32,
    fifo_window_programmed: u32,
    fifo_window_readback: u32,
    source_empty_polls: u32,
    rx_drain_budget_hits: u32,
    rx_queue_overflows: u32,
    rx_max_drained_per_turn: u16,
    rx_irq_preserve_count: u32,
    rx_irq_preserve_reason: u16,
    rx_irq_preserve_int_status: u32,
    rx_irq_preserve_ack_bits: u32,
    sequence: u32,
    start_ticks_lo: u32,
    pre_sample_delta_ticks: u32,
    transfer_delta_ticks: u32,
    post_sample_delta_ticks: u32,
}

#[cfg(feature = "kernel")]
fn cyw43_rx_idle_trace(bytes: &[u8]) -> Option<Cyw43RxIdleTrace> {
    if bytes.len() < CYW43_RX_IDLE_TRACE_BYTES {
        return None;
    }
    if cyw43_read_le_u32(bytes, 0)? != CYW43_RX_IDLE_TRACE_MAGIC {
        return None;
    }
    let version = cyw43_read_le_u16(bytes, 4)?;
    if version != CYW43_RX_IDLE_TRACE_VERSION_V1
        && version != CYW43_RX_IDLE_TRACE_VERSION_V2
        && version != CYW43_RX_IDLE_TRACE_VERSION_V3
        && version != CYW43_RX_IDLE_TRACE_VERSION_V4
        && version != CYW43_RX_IDLE_TRACE_VERSION_V5
        && version != CYW43_RX_IDLE_TRACE_VERSION
    {
        return None;
    }
    if version == CYW43_RX_IDLE_TRACE_VERSION_V2 && bytes.len() < CYW43_RX_IDLE_TRACE_V2_BYTES {
        return None;
    }
    if version == CYW43_RX_IDLE_TRACE_VERSION_V3 && bytes.len() < CYW43_RX_IDLE_TRACE_V3_BYTES {
        return None;
    }
    if version == CYW43_RX_IDLE_TRACE_VERSION_V4 && bytes.len() < CYW43_RX_IDLE_TRACE_V4_BYTES {
        return None;
    }
    if version == CYW43_RX_IDLE_TRACE_VERSION_V5 && bytes.len() < CYW43_RX_IDLE_TRACE_V5_BYTES {
        return None;
    }
    if version == CYW43_RX_IDLE_TRACE_VERSION && bytes.len() < CYW43_RX_IDLE_TRACE_V6_BYTES {
        return None;
    }
    Some(Cyw43RxIdleTrace {
        valid: true,
        flags: cyw43_read_le_u16(bytes, 6)?,
        detail: cyw43_read_le_u16(bytes, 8)?,
        probe_len: cyw43_read_le_u16(bytes, 10)?,
        source_result: cyw43_read_le_u32(bytes, 12)?,
        prefix_signature: cyw43_read_le_u32(bytes, 16)?,
        prefix_digest: cyw43_read_le_u32(bytes, 20)?,
        rframe_len: cyw43_read_le_u16(bytes, 24)?,
        firstread_reads: cyw43_read_le_u16(bytes, 26)?,
        block_reads: cyw43_read_le_u16(bytes, 28)?,
        rframe_reads: cyw43_read_le_u16(bytes, 30)?,
        request_len: cyw43_read_le_u16(bytes, 32)?,
        block_size: cyw43_read_le_u16(bytes, 34)?,
        block_count: cyw43_read_le_u16(bytes, 36)?,
        retransmit_sample: cyw43_read_le_u16(bytes, 38)?,
        queue_depth: cyw43_read_le_u16(bytes, 40).unwrap_or(0),
        queue_high_water: cyw43_read_le_u16(bytes, 42).unwrap_or(0),
        cmd53_arg: cyw43_read_le_u32(bytes, 44).unwrap_or(0),
        transfer_result: cyw43_read_le_u32(bytes, 48).unwrap_or(0),
        payload_before_digest: cyw43_read_le_u32(bytes, 52).unwrap_or(0),
        payload_after_digest: cyw43_read_le_u32(bytes, 56).unwrap_or(0),
        pre_source_result: cyw43_read_le_u32(bytes, 60).unwrap_or(0),
        post_source_result: cyw43_read_le_u32(bytes, 64).unwrap_or(0),
        source_flags: cyw43_read_le_u16(bytes, 68).unwrap_or(0),
        first_nonzero_offset: cyw43_read_le_u16(bytes, 70).unwrap_or(u16::MAX),
        first_nonzero_byte: cyw43_read_le_u16(bytes, 72).unwrap_or(0),
        fifo_window_flags: cyw43_read_le_u16(bytes, 74).unwrap_or(0),
        pre_intstatus: cyw43_read_le_u32(bytes, 76).unwrap_or(0),
        post_intstatus: cyw43_read_le_u32(bytes, 80).unwrap_or(0),
        pre_sdhci_status: cyw43_read_le_u32(bytes, 84).unwrap_or(0),
        post_sdhci_status: cyw43_read_le_u32(bytes, 88).unwrap_or(0),
        fifo_window_requested: cyw43_read_le_u32(bytes, 92).unwrap_or(0),
        fifo_window_programmed: cyw43_read_le_u32(bytes, 96).unwrap_or(0),
        fifo_window_readback: cyw43_read_le_u32(bytes, 100).unwrap_or(0),
        source_empty_polls: cyw43_read_le_u32(bytes, 104).unwrap_or(0),
        rx_drain_budget_hits: cyw43_read_le_u32(bytes, 108).unwrap_or(0),
        rx_queue_overflows: cyw43_read_le_u32(bytes, 112).unwrap_or(0),
        rx_max_drained_per_turn: cyw43_read_le_u16(bytes, 116).unwrap_or(0),
        rx_irq_preserve_count: cyw43_read_le_u32(bytes, 120).unwrap_or(0),
        rx_irq_preserve_reason: cyw43_read_le_u16(bytes, 124).unwrap_or(0),
        rx_irq_preserve_int_status: cyw43_read_le_u32(bytes, 128).unwrap_or(0),
        rx_irq_preserve_ack_bits: cyw43_read_le_u32(bytes, 132).unwrap_or(0),
        sequence: cyw43_read_le_u32(bytes, 136).unwrap_or(0),
        start_ticks_lo: cyw43_read_le_u32(bytes, 140).unwrap_or(0),
        pre_sample_delta_ticks: cyw43_read_le_u32(bytes, 144).unwrap_or(0),
        transfer_delta_ticks: cyw43_read_le_u32(bytes, 148).unwrap_or(0),
        post_sample_delta_ticks: cyw43_read_le_u32(bytes, 152).unwrap_or(0),
    })
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_retransmit_action(sample: u16) -> u16 {
    sample & CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_MASK
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_retransmit_action_name(sample: u16) -> &'static str {
    match cyw43_rx_trace_retransmit_action(sample) {
        CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_BLOCK => "block",
        CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_CLEAR_STALE => "clear-stale",
        CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_READ_ASSERTED_ZERO => "read-asserted-zero",
        CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_READ_RFRAME_READY => "read-rframe-ready",
        CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_READ_SOURCE_ASSERTED => "read-source-asserted",
        _ => "none",
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_cmd53_write(arg: u32) -> bool {
    arg & (1 << 31) != 0
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_cmd53_function(arg: u32) -> u8 {
    ((arg >> 28) & 0x7) as u8
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_cmd53_block_mode(arg: u32) -> bool {
    arg & (1 << 27) != 0
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_cmd53_increment(arg: u32) -> bool {
    arg & (1 << 26) != 0
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_cmd53_addr(arg: u32) -> u32 {
    (arg >> 9) & 0x1ffff
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_cmd53_count(arg: u32) -> u16 {
    if arg == 0 {
        return 0;
    }
    let raw_count = (arg & 0x01ff) as u16;
    if !cyw43_rx_trace_cmd53_block_mode(arg) && raw_count == 0 {
        512
    } else {
        raw_count
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_cmd53_mode(arg: u32) -> &'static str {
    if arg == 0 {
        "none"
    } else if cyw43_rx_trace_cmd53_block_mode(arg) {
        "block"
    } else if cyw43_rx_trace_cmd53_count(arg) == 512 {
        "byte512"
    } else {
        "byte"
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_pre_source_asserted(trace: Cyw43RxIdleTrace) -> bool {
    trace.source_flags & CYW43_RX_IDLE_TRACE_SOURCE_FLAG_PRE_ASSERTED != 0
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_post_source_asserted(trace: Cyw43RxIdleTrace) -> bool {
    trace.source_flags & CYW43_RX_IDLE_TRACE_SOURCE_FLAG_POST_ASSERTED != 0
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_pre_source_fresh(trace: Cyw43RxIdleTrace) -> bool {
    trace.source_flags & CYW43_RX_IDLE_TRACE_SOURCE_FLAG_PRE_FRESH != 0
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_post_source_fresh(trace: Cyw43RxIdleTrace) -> bool {
    trace.source_flags & CYW43_RX_IDLE_TRACE_SOURCE_FLAG_POST_FRESH != 0
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_pre_source_failed(trace: Cyw43RxIdleTrace) -> bool {
    trace.source_flags & CYW43_RX_IDLE_TRACE_SOURCE_FLAG_PRE_FAILED != 0
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_post_source_failed(trace: Cyw43RxIdleTrace) -> bool {
    trace.source_flags & CYW43_RX_IDLE_TRACE_SOURCE_FLAG_POST_FAILED != 0
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_source_asserted_ever(trace: Cyw43RxIdleTrace) -> bool {
    trace.source_flags & CYW43_RX_IDLE_TRACE_SOURCE_FLAG_EVER_ASSERTED != 0
}

#[cfg(feature = "kernel")]
fn cyw43_rx_trace_source_asserts_pending_frame(trace: Cyw43RxIdleTrace) -> bool {
    cyw43_rx_trace_pre_source_asserted(trace)
        || cyw43_rx_trace_post_source_asserted(trace)
        || cyw43_rx_trace_source_asserted_ever(trace)
        || cyw43_rx_source_result(trace.pre_source_result)
            .is_some_and(cyw43_rx_source_asserts_pending_frame)
        || cyw43_rx_source_result(trace.post_source_result)
            .is_some_and(cyw43_rx_source_asserts_pending_frame)
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_fifo_window_ok(trace: Cyw43RxIdleTrace) -> bool {
    trace.fifo_window_flags
        & (CYW43_RX_IDLE_TRACE_FIFO_FLAG_SET_OK
            | CYW43_RX_IDLE_TRACE_FIFO_FLAG_READBACK_OK
            | CYW43_RX_IDLE_TRACE_FIFO_FLAG_READBACK_MATCH)
        == (CYW43_RX_IDLE_TRACE_FIFO_FLAG_SET_OK
            | CYW43_RX_IDLE_TRACE_FIFO_FLAG_READBACK_OK
            | CYW43_RX_IDLE_TRACE_FIFO_FLAG_READBACK_MATCH)
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_trace_first_nonzero_desc(trace: Cyw43RxIdleTrace) -> &'static str {
    if trace.first_nonzero_offset == u16::MAX {
        "none"
    } else {
        "present"
    }
}

#[cfg(feature = "kernel")]
fn cyw43_completion_rx_idle_trace(
    contract: DriverTaskContract,
    completion: DriverTaskCompletionRecord,
) -> Option<Cyw43RxIdleTrace> {
    let bytes = crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)?;
    cyw43_rx_idle_trace(bytes)
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
        passive: result & DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_PASSIVE != 0,
        cached: result & DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_CACHED != 0,
    })
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_source_mode(source: Option<Cyw43RxSourceResult>) -> &'static str {
    match source {
        Some(source) if source.passive => "passive-sdio-bus-link",
        Some(source) if source.cached => "owner-card-sampled-cached",
        Some(_) => "owner-card-sampled",
        None => "unreported",
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_source_is_passive(source: Option<Cyw43RxSourceResult>) -> bool {
    match source {
        Some(source) => source.passive,
        None => false,
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_source_owner_state_missing(source: Option<Cyw43RxSourceResult>) -> bool {
    match source {
        Some(source) => {
            !source.passive
                && (!source.function2_ready || source.interrupt_enable != CYW43_SDIO_EXPECTED_IENX)
        }
        None => false,
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_rx_source_asserts_pending_frame(source: Cyw43RxSourceResult) -> bool {
    !source.passive
        && !source.cached
        && source.function2_ready
        && (source.frame_indicated || source.host_interrupt || source.card_interrupt)
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_source_asserted(progress: &Cyw43HostEapolProgress) -> bool {
    if let Some(source) = progress.last_rx_source {
        if cyw43_rx_source_asserts_pending_frame(source) {
            return true;
        }
    }
    if let Some(source) = progress.last_control_rx_source {
        if cyw43_rx_source_asserts_pending_frame(source) {
            return true;
        }
    }
    cyw43_rx_source_result(progress.last_rx_trace.source_result)
        .is_some_and(cyw43_rx_source_asserts_pending_frame)
        || cyw43_rx_source_result(progress.last_control_rx_trace.source_result)
            .is_some_and(cyw43_rx_source_asserts_pending_frame)
        || cyw43_rx_trace_source_asserts_pending_frame(progress.last_rx_trace)
        || cyw43_rx_trace_source_asserts_pending_frame(progress.last_control_rx_trace)
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
    last_rx_probe_poll: u32,
    last_rx_probe_flags: u16,
    last_control_rx_probe_poll: u32,
    last_control_rx_probe_flags: u16,
    last_rx_source: Option<Cyw43RxSourceResult>,
    last_control_rx_source: Option<Cyw43RxSourceResult>,
    last_rx_trace: Cyw43RxIdleTrace,
    last_control_rx_trace: Cyw43RxIdleTrace,
    assoc_probe_not_associated: bool,
    assoc_probe_status: Option<&'static str>,
    assoc_probe_result: u32,
    assoc_join_rescue_attempted: bool,
    assoc_set_ssid_rescue_attempted: bool,
    auth_timeout_seen: bool,
    set_ssid_failure_seen: bool,
    eapol_error: Option<&'static str>,
    last_assoc_event_type: u8,
    last_assoc_event_status: u32,
    last_assoc_event_reason: u32,
    last_assoc_event_auth: u32,
    last_flags: u16,
    last_len: u16,
    last_ethertype: u16,
    last_ethertype_valid: bool,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43HostEapolTraceKey {
    valid: bool,
    flags: u16,
    detail: u16,
    probe_len: u16,
    source_result: u32,
    prefix_signature: u32,
    prefix_digest: u32,
    rframe_len: u16,
    firstread_reads: u16,
    block_reads: u16,
    rframe_reads: u16,
    request_len: u16,
    block_size: u16,
    block_count: u16,
    retransmit_sample: u16,
    queue_depth: u16,
    queue_high_water: u16,
    cmd53_arg: u32,
    transfer_result: u32,
    payload_before_digest: u32,
    payload_after_digest: u32,
    pre_source_result: u32,
    post_source_result: u32,
    source_flags: u16,
    first_nonzero_offset: u16,
    first_nonzero_byte: u16,
    fifo_window_flags: u16,
    pre_intstatus: u32,
    post_intstatus: u32,
    pre_sdhci_status: u32,
    post_sdhci_status: u32,
    fifo_window_requested: u32,
    fifo_window_programmed: u32,
    fifo_window_readback: u32,
    source_empty_polls: u32,
    rx_drain_budget_hits: u32,
    rx_queue_overflows: u32,
    rx_max_drained_per_turn: u16,
}

#[cfg(feature = "kernel")]
impl From<Cyw43RxIdleTrace> for Cyw43HostEapolTraceKey {
    fn from(trace: Cyw43RxIdleTrace) -> Self {
        Self {
            valid: trace.valid,
            flags: trace.flags,
            detail: trace.detail,
            probe_len: trace.probe_len,
            source_result: trace.source_result,
            prefix_signature: trace.prefix_signature,
            prefix_digest: trace.prefix_digest,
            rframe_len: trace.rframe_len,
            firstread_reads: trace.firstread_reads,
            block_reads: trace.block_reads,
            rframe_reads: trace.rframe_reads,
            request_len: trace.request_len,
            block_size: trace.block_size,
            block_count: trace.block_count,
            retransmit_sample: trace.retransmit_sample,
            queue_depth: trace.queue_depth,
            queue_high_water: trace.queue_high_water,
            cmd53_arg: trace.cmd53_arg,
            transfer_result: trace.transfer_result,
            payload_before_digest: trace.payload_before_digest,
            payload_after_digest: trace.payload_after_digest,
            pre_source_result: trace.pre_source_result,
            post_source_result: trace.post_source_result,
            source_flags: trace.source_flags,
            first_nonzero_offset: trace.first_nonzero_offset,
            first_nonzero_byte: trace.first_nonzero_byte,
            fifo_window_flags: trace.fifo_window_flags,
            pre_intstatus: trace.pre_intstatus,
            post_intstatus: trace.post_intstatus,
            pre_sdhci_status: trace.pre_sdhci_status,
            post_sdhci_status: trace.post_sdhci_status,
            fifo_window_requested: trace.fifo_window_requested,
            fifo_window_programmed: trace.fifo_window_programmed,
            fifo_window_readback: trace.fifo_window_readback,
            source_empty_polls: trace.source_empty_polls,
            rx_drain_budget_hits: trace.rx_drain_budget_hits,
            rx_queue_overflows: trace.rx_queue_overflows,
            rx_max_drained_per_turn: trace.rx_max_drained_per_turn,
        }
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43HostEapolStatusKey {
    status: &'static str,
    reason: &'static str,
    next_action: &'static str,
    starts: u32,
    tx_retries: u32,
    data_rx: u32,
    eapol_rx: u32,
    non_eapol_rx: u32,
    event_rx: u32,
    control_rx: u32,
    associated: bool,
    link_up: bool,
    assoc_event: &'static str,
    assoc_probe: &'static str,
    assoc_probe_result: u32,
    assoc_join_rescue: bool,
    assoc_set_ssid_rescue: bool,
    firstread_class: &'static str,
    rx_idle_detail: u16,
    rx_idle_result: u32,
    control_rx_idle_detail: u16,
    control_rx_idle_result: u32,
    rx_source: Option<Cyw43RxSourceResult>,
    control_rx_source: Option<Cyw43RxSourceResult>,
    rx_trace: Cyw43HostEapolTraceKey,
    control_rx_trace: Cyw43HostEapolTraceKey,
    eapol_error: Option<&'static str>,
    last_assoc_event_type: u8,
    last_assoc_event_status: u32,
    last_assoc_event_reason: u32,
    last_assoc_event_auth: u32,
    last_flags: u16,
    last_len: u16,
    last_ethertype: u16,
    last_ethertype_valid: bool,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Cyw43HostEapolStatusThrottle {
    last_key: Option<Cyw43HostEapolStatusKey>,
    suppressed: u32,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43HostEapolStatusLogDecision {
    emit_full: bool,
    suppressed_before: u32,
}

#[cfg(feature = "kernel")]
const CYW43_HOST_EAPOL_STATUS_SUPPRESS_MILESTONE: u32 = 64;

#[cfg(feature = "kernel")]
static CYW43_HOST_EAPOL_STATUS_THROTTLE: Mutex<Cyw43HostEapolStatusThrottle> =
    Mutex::new(Cyw43HostEapolStatusThrottle {
        last_key: None,
        suppressed: 0,
    });

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_status_suppressible(status: &'static str) -> bool {
    status == "pending"
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_status_key(
    status: &'static str,
    reason: &'static str,
    next_action: &'static str,
    starts: u32,
    tx_retries: u32,
    progress: &Cyw43HostEapolProgress,
) -> Cyw43HostEapolStatusKey {
    Cyw43HostEapolStatusKey {
        status,
        reason,
        next_action,
        starts,
        tx_retries,
        data_rx: progress.data_rx,
        eapol_rx: progress.eapol_rx,
        non_eapol_rx: progress.non_eapol_rx,
        event_rx: progress.event_rx,
        control_rx: progress.control_rx,
        associated: progress.associated,
        link_up: progress.link_up,
        assoc_event: progress.association_event.unwrap_or("none"),
        assoc_probe: progress.assoc_probe_status.unwrap_or("none"),
        assoc_probe_result: progress.assoc_probe_result,
        assoc_join_rescue: progress.assoc_join_rescue_attempted,
        assoc_set_ssid_rescue: progress.assoc_set_ssid_rescue_attempted,
        firstread_class: cyw43_host_eapol_firstread_class(progress),
        rx_idle_detail: progress.last_rx_idle_detail,
        rx_idle_result: progress.last_rx_idle_result,
        control_rx_idle_detail: progress.last_control_rx_idle_detail,
        control_rx_idle_result: progress.last_control_rx_idle_result,
        rx_source: progress.last_rx_source,
        control_rx_source: progress.last_control_rx_source,
        rx_trace: progress.last_rx_trace.into(),
        control_rx_trace: progress.last_control_rx_trace.into(),
        eapol_error: progress.eapol_error,
        last_assoc_event_type: progress.last_assoc_event_type,
        last_assoc_event_status: progress.last_assoc_event_status,
        last_assoc_event_reason: progress.last_assoc_event_reason,
        last_assoc_event_auth: progress.last_assoc_event_auth,
        last_flags: progress.last_flags,
        last_len: progress.last_len,
        last_ethertype: progress.last_ethertype,
        last_ethertype_valid: progress.last_ethertype_valid,
    }
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_status_log_decision(
    throttle: &mut Cyw43HostEapolStatusThrottle,
    key: Cyw43HostEapolStatusKey,
) -> Cyw43HostEapolStatusLogDecision {
    if cyw43_host_eapol_status_suppressible(key.status) && throttle.last_key == Some(key) {
        let next_suppressed = throttle.suppressed.saturating_add(1);
        if next_suppressed < CYW43_HOST_EAPOL_STATUS_SUPPRESS_MILESTONE {
            throttle.suppressed = next_suppressed;
            return Cyw43HostEapolStatusLogDecision {
                emit_full: false,
                suppressed_before: 0,
            };
        }
    }

    let suppressed_before = throttle.suppressed;
    throttle.last_key = Some(key);
    throttle.suppressed = 0;
    Cyw43HostEapolStatusLogDecision {
        emit_full: true,
        suppressed_before,
    }
}

#[cfg(feature = "kernel")]
fn clear_cyw43_host_eapol_status_throttle() {
    *CYW43_HOST_EAPOL_STATUS_THROTTLE.lock() = Cyw43HostEapolStatusThrottle::default();
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
        if cyw43_host_eapol_join_event_type(event.event_type) {
            self.last_assoc_event_type = event.event_type;
            self.last_assoc_event_status = event.status;
            self.last_assoc_event_reason = event.reason;
            self.last_assoc_event_auth = event.auth_type;
            if event.event_type == CYW43_EVENT_AUTH && event.status == CYW43_EVENT_STATUS_TIMEOUT {
                self.auth_timeout_seen = true;
            }
            if event.event_type == CYW43_EVENT_SET_SSID
                && event.status != CYW43_EVENT_STATUS_SUCCESS
            {
                self.set_ssid_failure_seen = true;
            }
        }
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
        CYW43_ASSOCIATED.store(if self.associated { 1 } else { 0 }, Ordering::Release);
        CYW43_LINK_UP.store(if self.link_up { 1 } else { 0 }, Ordering::Release);
    }

    fn record_empty_poll(&mut self) {
        self.empty_polls = self.empty_polls.saturating_add(1);
    }

    fn record_assoc_probe(&mut self, status: &'static str, result: u32) {
        self.assoc_probe_status = Some(status);
        self.assoc_probe_result = result;
        if status == "not-associated" {
            self.assoc_probe_not_associated = true;
        }
    }

    fn record_assoc_join_rescue_attempt(&mut self) {
        self.assoc_join_rescue_attempted = true;
    }

    fn record_assoc_set_ssid_rescue_attempt(&mut self) {
        self.assoc_set_ssid_rescue_attempted = true;
    }

    fn restart_assoc_window_after_rescue(&mut self) {
        self.assoc_probe_not_associated = false;
        self.assoc_probe_status = None;
        self.assoc_probe_result = 0;
        self.set_ssid_failure_seen = false;
        self.last_assoc_event_type = 0;
        self.last_assoc_event_status = 0;
        self.last_assoc_event_reason = 0;
        self.last_assoc_event_auth = 0;
    }

    fn record_eapol_error(&mut self, error: &'static str) {
        self.eapol_error = Some(error);
    }

    fn record_eapol_association_proof(&mut self, label: &'static str, poll: usize) {
        self.associated = true;
        if self.association_event.is_none() {
            self.association_event = Some(label);
            self.association_poll = poll.min(u32::MAX as usize) as u32;
        }
        CYW43_ASSOCIATED.store(1, Ordering::Release);
    }

    fn record_rx_idle_completion(&mut self, completion: DriverTaskCompletionRecord) {
        self.record_rx_idle_completion_with_trace(completion, None, 0, 0);
    }

    fn record_rx_idle_completion_with_trace(
        &mut self,
        completion: DriverTaskCompletionRecord,
        trace: Option<Cyw43RxIdleTrace>,
        poll: usize,
        flags: u16,
    ) {
        self.last_rx_idle_detail = completion.detail;
        self.last_rx_idle_result = completion.result;
        self.last_rx_trace = trace.unwrap_or_default();
        self.last_rx_probe_poll = poll.min(u32::MAX as usize) as u32;
        self.last_rx_probe_flags = flags;
        self.last_rx_source = if completion.detail
            == DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY
            || completion.detail
                == DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_SOURCE_ASSERTED_EMPTY
        {
            cyw43_rx_source_result(completion.result)
        } else {
            None
        };
        match completion.detail {
            DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY
            | DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_SOURCE_ASSERTED_EMPTY => {
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
        self.record_control_rx_idle_completion_with_trace(completion, None, 0, 0);
    }

    fn record_control_rx_idle_completion_with_trace(
        &mut self,
        completion: DriverTaskCompletionRecord,
        trace: Option<Cyw43RxIdleTrace>,
        poll: usize,
        flags: u16,
    ) {
        self.last_control_rx_idle_detail = completion.detail;
        self.last_control_rx_idle_result = completion.result;
        self.last_control_rx_trace = trace.unwrap_or_default();
        self.last_control_rx_probe_poll = poll.min(u32::MAX as usize) as u32;
        self.last_control_rx_probe_flags = flags;
        self.last_control_rx_source = if completion.detail
            == DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY
            || completion.detail
                == DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_SOURCE_ASSERTED_EMPTY
        {
            cyw43_rx_source_result(completion.result)
        } else {
            None
        };
        match completion.detail {
            DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY
            | DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_SOURCE_ASSERTED_EMPTY => {
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

    let event_label = cyw43_host_eapol_association_event_label(event);
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
    started_ms: Option<u64>,
    associated_ms: Option<u64>,
    last_pre_assoc_activity_ms: Option<u64>,
    last_pre_assoc_activity_poll: Option<u32>,
    last_eapol_activity_ms: Option<u64>,
    last_eapol_activity_poll: Option<u32>,
    last_eapol_start_ms: Option<u64>,
    last_timer_firstread_base_ms: Option<u64>,
    last_timer_firstread_slot: Option<u16>,
    refreshed_after_assoc: bool,
    rescued_after_assoc: bool,
    post_rescue_assoc_window_due: bool,
    bssid_refreshed_after_assoc: bool,
    bssid_probed_before_required: bool,
    assoc_probe_attempts: u8,
}

#[cfg(feature = "kernel")]
impl Cyw43HostEapolSession {
    fn new(credentials: WifiCredentials) -> Result<Self, DriverTaskNetError> {
        let ssid_len = usize::from(credentials.ssid_len);
        let psk_len = usize::from(credentials.psk_len);
        let ssid = &credentials.ssid[..ssid_len];
        let psk = &credentials.psk[..psk_len];
        let eapol = HostEapolState::new_with_rsn_ie(ssid, psk, &WPA2_PSK_CCMP_RSN_IE)
            .map_err(DriverTaskNetError::RuntimeInit)?;
        Ok(Self {
            eapol,
            progress: Cyw43HostEapolProgress::default(),
            started_ms: None,
            associated_ms: None,
            last_pre_assoc_activity_ms: None,
            last_pre_assoc_activity_poll: None,
            last_eapol_activity_ms: None,
            last_eapol_activity_poll: None,
            last_eapol_start_ms: None,
            last_timer_firstread_base_ms: None,
            last_timer_firstread_slot: None,
            refreshed_after_assoc: false,
            rescued_after_assoc: false,
            post_rescue_assoc_window_due: false,
            bssid_refreshed_after_assoc: false,
            bssid_probed_before_required: false,
            assoc_probe_attempts: 0,
        })
    }

    fn record_time(&mut self, now_ms: u64) {
        if self.started_ms.is_none() {
            self.started_ms = Some(now_ms);
        }
        if self.progress.associated && self.associated_ms.is_none() {
            self.associated_ms = Some(now_ms);
        }
    }

    fn elapsed_ms(&self, now_ms: u64) -> u64 {
        self.started_ms
            .map(|started| now_ms.saturating_sub(started))
            .unwrap_or(0)
    }

    fn record_pre_assoc_activity(&mut self, now_ms: u64, poll: u32) {
        if !self.progress.associated {
            self.last_pre_assoc_activity_ms = Some(now_ms);
            self.last_pre_assoc_activity_poll = Some(poll);
        }
    }

    fn restart_pre_assoc_window_after_rescue(&mut self, now_ms: u64) {
        if self.progress.associated {
            return;
        }
        self.progress.restart_assoc_window_after_rescue();
        self.last_pre_assoc_activity_ms = Some(now_ms);
        self.last_pre_assoc_activity_poll = Some(0);
        self.progress.polls = 0;
        self.progress.empty_polls = 0;
        self.assoc_probe_attempts = 0;
        self.bssid_probed_before_required = false;
        self.post_rescue_assoc_window_due = true;
    }

    fn take_post_rescue_assoc_window_due(&mut self) -> bool {
        let due = self.post_rescue_assoc_window_due && !self.progress.associated;
        self.post_rescue_assoc_window_due = false;
        due
    }

    fn pre_assoc_idle_ms(&self, now_ms: u64) -> u64 {
        self.last_pre_assoc_activity_ms
            .or(self.started_ms)
            .map(|last_activity| now_ms.saturating_sub(last_activity))
            .unwrap_or(0)
    }

    fn pre_assoc_idle_polls(&self) -> u32 {
        self.last_pre_assoc_activity_poll
            .map(|last_activity| self.progress.polls.saturating_sub(last_activity))
            .unwrap_or(self.progress.polls)
    }

    fn pre_assoc_timebase_ready(&self) -> bool {
        self.last_pre_assoc_activity_ms
            .or(self.started_ms)
            .is_some()
    }

    fn post_assoc_elapsed_ms(&self, now_ms: u64) -> u64 {
        self.associated_ms
            .map(|associated| now_ms.saturating_sub(associated))
            .unwrap_or(0)
    }

    fn record_eapol_activity(&mut self, now_ms: u64, poll: u32) {
        self.last_eapol_activity_ms = Some(now_ms);
        self.last_eapol_activity_poll = Some(poll);
    }

    fn post_assoc_eapol_idle_ms(&self, now_ms: u64) -> u64 {
        self.last_eapol_activity_ms
            .or(self.associated_ms)
            .map(|last_activity| now_ms.saturating_sub(last_activity))
            .unwrap_or(0)
    }

    fn post_assoc_eapol_idle_polls(&self) -> u32 {
        self.last_eapol_activity_poll
            .map(|last_activity| self.progress.polls.saturating_sub(last_activity))
            .unwrap_or(self.progress.post_assoc_polls)
    }

    fn post_assoc_timebase_ready(&self) -> bool {
        self.last_eapol_activity_ms.or(self.associated_ms).is_some()
    }

    fn claim_timer_firstread_slot(&mut self, starts_sent: u32, now_ms: u64) -> bool {
        let associated_after_start = self.progress.associated && starts_sent != 0;
        let base_ms = if associated_after_start {
            match self.last_eapol_start_ms {
                Some(base_ms) => base_ms,
                None => return false,
            }
        } else {
            match self.started_ms {
                Some(base_ms) => base_ms,
                None => return false,
            }
        };
        let elapsed_ms = now_ms.saturating_sub(base_ms);
        let Some(slot) =
            cyw43_host_eapol_rx_firstread_timer_slot(elapsed_ms, associated_after_start)
        else {
            return false;
        };
        if self.last_timer_firstread_base_ms == Some(base_ms)
            && self.last_timer_firstread_slot == Some(slot)
        {
            return false;
        }
        self.last_timer_firstread_base_ms = Some(base_ms);
        self.last_timer_firstread_slot = Some(slot);
        true
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

#[cfg(all(test, feature = "kernel"))]
pub(crate) fn test_clear_cyw43_runtime_replay_status() {
    clear_cyw43_runtime_command_fault_status();
    *SDIO_LAST_RUNTIME_REPLAY_STATUS.lock() = None;
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
enum Cyw43BssidRefreshError {
    Runtime(DriverTaskNetError),
    Completion(DriverTaskCompletionRecord),
}

#[cfg(feature = "kernel")]
impl Cyw43BssidRefreshError {
    const fn result(self) -> u32 {
        match self {
            Self::Runtime(_) => 0,
            Self::Completion(completion) => completion.result,
        }
    }

    const fn is_not_associated(self) -> bool {
        match self {
            Self::Completion(completion) => {
                completion.code == DriverTaskCompletionCode::Fault.as_u16()
                    && completion.detail == CYW43_CONTROL_EXCHANGE_FAULT_DETAIL
                    && completion.result == CYW43_BCME_NOTASSOCIATED_STATUS
            }
            Self::Runtime(_) => false,
        }
    }
}

#[cfg(feature = "kernel")]
fn cyw43_bssid_refresh_tx_retry_completion(
    err: Cyw43CommandSubmitError,
    retries_spent: usize,
) -> Option<DriverTaskCompletionRecord> {
    if retries_spent >= CYW43_HOST_EAPOL_BSSID_REFRESH_TX_RETRIES {
        return None;
    }
    err.same_command_retry_completion()
}

#[cfg(feature = "kernel")]
fn cyw43_control_tx_submit_retry_completion(
    stage: &'static str,
    completion: DriverTaskCompletionRecord,
    retries_spent: usize,
) -> Option<DriverTaskCompletionRecord> {
    if retries_spent >= CYW43_CONTROL_TX_SUBMIT_RETRIES {
        return None;
    }
    if completion.code != DriverTaskCompletionCode::Fault.as_u16() {
        return None;
    }
    let status = sdio_transfer_failure_status(completion.result);
    let failure_stage = (completion.result >> 24) & 0xff;
    let retryable_transfer_fault = (matches!(failure_stage, 3 | 4)
        && status & (SDHCI_INT_ERROR | SDHCI_INT_DATA_TIMEOUT | SDHCI_INT_DATA_CRC) != 0)
        || (failure_stage == 5 && sdio_transfer_failure_r5(completion.result) != 0);
    if completion.detail == CYW43_CONTROL_FRAME_DETAIL {
        return retryable_transfer_fault.then_some(completion);
    }
    if !cyw43_control_tx_detail_allows_submit_retry(stage, completion.detail) {
        return None;
    }
    if completion.detail == CYW43_SDIO_POST_RELEASE_HT_CLOCK_DETAIL {
        return Some(completion);
    }
    if completion.detail == CYW43_SDIO_FUNCTION2_NOT_READY_DETAIL {
        return Some(completion);
    }
    if retryable_transfer_fault {
        Some(completion)
    } else {
        None
    }
}

#[cfg(feature = "kernel")]
fn cyw43_control_tx_detail_allows_submit_retry(stage: &'static str, detail: u16) -> bool {
    cyw43_fault_detail_allows_same_command_retry(detail)
        || (detail == CYW43_SDIO_POST_RELEASE_HT_CLOCK_DETAIL
            && matches!(stage, "cyw43-control-txglomalign"))
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
fn run_driver_task_net_engine_init_service(
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
const CYW43_ENGINE_INIT_REPLAY_ATTEMPTS: usize = 3;

#[cfg(feature = "kernel")]
fn cyw43_engine_init_completion_observed_by_runtime(
    completion: DriverTaskCompletionRecord,
    progress: Option<DriverTaskRingProgressSnapshot>,
) -> bool {
    matches!(
        progress,
        Some(progress)
            if progress.marker_valid
                && progress.sequence == completion.sequence
                && progress.aux0 == DRIVER_RUNTIME_ENGINE_INIT_AUX
    )
}

#[cfg(feature = "kernel")]
fn cyw43_engine_init_completion_replay_reason(
    completion: Option<DriverTaskCompletionRecord>,
) -> Option<&'static str> {
    match completion {
        None => Some("no-reply"),
        Some(completion) => match (completion.code, completion.detail) {
            (code, detail)
                if code == DriverTaskCompletionCode::Fault.as_u16()
                    && detail == DriverTaskFaultCode::DeviceUnavailable.as_u16() =>
            {
                Some("device-unavailable")
            }
            (code, detail)
                if code == DriverTaskCompletionCode::Fault.as_u16()
                    && detail == DriverTaskFaultCode::RejectedCommand.as_u16()
                    && !cyw43_engine_init_completion_observed_by_runtime(
                        completion,
                        crate::hal::driver_task::latest_driver_task_ring_progress(
                            CYW43_WIFI_DRIVER_TASK_CONTRACT,
                        ),
                    ) =>
            {
                Some("stale-admission")
            }
            _ => None,
        },
    }
}

#[cfg(feature = "kernel")]
fn cyw43_engine_init_completion_status(
    completion: Option<DriverTaskCompletionRecord>,
    initialized: bool,
    replay_reason: Option<&'static str>,
    final_attempt: bool,
) -> &'static str {
    if initialized {
        return "ready";
    }
    match replay_reason {
        Some("stale-admission") if !final_attempt => "stale-admission-retry",
        Some("stale-admission") => "stale-admission-exhausted",
        _ => driver_task_resource_completion_status(completion, initialized),
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_engine_init_completion_allows_replay(
    replay_reason: Option<&'static str>,
    final_attempt: bool,
) -> bool {
    match replay_reason {
        Some(_) => !final_attempt,
        None => false,
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
        let command = driver_task_net_engine_init_command(contract, hot_path);
        let max_engine_init_attempts = if hot_path == DriverTaskHotPath::Cyw43Wifi {
            CYW43_ENGINE_INIT_REPLAY_ATTEMPTS
        } else {
            1
        };
        let mut initialized = false;
        for attempt in 0..max_engine_init_attempts {
            let replay_stage = if attempt == 0 {
                "engine-init"
            } else {
                "engine-init-replay"
            };
            let resource_stage = if attempt == 0 {
                "net-engine-init"
            } else {
                "net-engine-init-replay"
            };
            emit_net_driver_task_replay_status(config, hot_path, replay_stage, "begin");
            let completion = run_driver_task_net_engine_init_service(contract, command);
            initialized = completion.is_some_and(|completion| {
                completion.code == DriverTaskCompletionCode::Progress.as_u16()
                    && completion.result == 1
            });
            let final_attempt = attempt.saturating_add(1) >= max_engine_init_attempts;
            let replay_reason = if hot_path == DriverTaskHotPath::Cyw43Wifi {
                cyw43_engine_init_completion_replay_reason(completion)
            } else {
                None
            };
            let status = if hot_path == DriverTaskHotPath::Cyw43Wifi {
                cyw43_engine_init_completion_status(
                    completion,
                    initialized,
                    replay_reason,
                    final_attempt,
                )
            } else {
                driver_task_resource_completion_status(completion, initialized)
            };
            emit_net_driver_task_replay_status(config, hot_path, replay_stage, status);
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                hot_path,
                resource_stage,
                status,
                completion,
            );
            if initialized
                || hot_path != DriverTaskHotPath::Cyw43Wifi
                || !cyw43_engine_init_completion_allows_replay(replay_reason, final_attempt)
            {
                break;
            }
        }
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
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        contract,
                        hot_path,
                        "genet-owner-state",
                        "ready",
                        None,
                    );
                    emit_net_driver_task_replay_status(config, hot_path, "owner-state", "ready");
                    crate::hal::driver_task::emit_owner_state_transition_boot_contract_proof(
                        hot_path,
                    );
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
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        contract,
                        hot_path,
                        "cyw43-function2",
                        "ready",
                        None,
                    );
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
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        contract,
                        hot_path,
                        "cyw43-owner-state",
                        "ready",
                        None,
                    );
                    emit_net_driver_task_replay_status(config, hot_path, "owner-state", "ready");
                    crate::hal::driver_task::emit_owner_state_transition_boot_contract_proof(
                        hot_path,
                    );
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
fn driver_task_net_engine_init_command(
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
) -> DriverTaskCommandRecord {
    let budget = DriverTaskBudgetGrant::from_contract(contract);
    if hot_path == DriverTaskHotPath::Cyw43Wifi {
        crate::hal::driver_task::runtime_engine_init_command(hot_path, budget)
    } else {
        let mut command = DriverTaskCommandRecord::pi4_hot_path(
            0,
            hot_path,
            budget,
            DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        command.aux0 = DRIVER_RUNTIME_NET_INIT_AUX;
        command
    }
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
    let contract = CYW43_WIFI_DRIVER_TASK_CONTRACT;
    let firmware_bundle = hal
        .wifi_firmware_bundle()
        .map_err(|_| DriverTaskNetError::RuntimeInit("cyw43-firmware-bundle"))?;
    firmware_bundle
        .validate()
        .map_err(|_| DriverTaskNetError::RuntimeInit("cyw43-firmware-bundle"))?;
    reset_cyw43_control_plane_state();
    let Some(credentials) = config.wifi_credentials else {
        return Err(DriverTaskNetError::RuntimeInit("wifi-credentials-missing"));
    };
    if !credentials.has_ssid() {
        return Err(DriverTaskNetError::RuntimeInit("wifi-ssid-missing"));
    }
    let mac = cyw43_prepare_runtime_control_plane(contract, firmware_bundle.clm_blob)?;
    *CYW43_RUNTIME_MAC.lock() = mac;
    cyw43_enable_join_event_messages(contract, "cyw43-control-event-mask")?;
    cyw43_submit_bcdc_empty(contract, CYW43_WLC_UP, "cyw43-control-up")?;
    cyw43_submit_bcdc_u32(contract, CYW43_WLC_SET_INFRA, 1, "cyw43-control-infra")?;
    cyw43_submit_bcdc_u32(
        contract,
        CYW43_WLC_SET_PM,
        CYW43_PM_OFF,
        "cyw43-control-pm-off",
    )?;
    let _ = cyw43_poll_control_plane_frames("cyw43-control-post-up-event-drain");
    cyw43_enable_join_event_messages(contract, "cyw43-control-event-mask-post-up")?;
    cyw43_apply_linux_connect_station_policy(contract)?;
    if credentials.has_psk() {
        cyw43_submit_bcdc_iovar_bytes(
            contract,
            "wpaie",
            &WPA2_PSK_CCMP_RSN_IE,
            "cyw43-control-wpaie",
        )?;
        cyw43_submit_bcdc_iovar_u32(
            contract,
            "wpa_auth",
            CYW43_WPA2_AUTH_PSK_OR_UNSPECIFIED,
            "cyw43-control-wpa-auth-initial",
        )?;
        cyw43_submit_bcdc_iovar_u32(contract, "auth", 0, "cyw43-control-auth")?;
        cyw43_submit_bcdc_iovar_u32(
            contract,
            "wsec",
            CYW43_WSEC_AES,
            "cyw43-control-security-wpa2-psk",
        )?;
        cyw43_apply_linux_wpa2_rsn_capability_policy(contract)?;
        cyw43_submit_bcdc_iovar_u32(
            contract,
            "wpa_auth",
            CYW43_WPA2_AUTH_PSK,
            "cyw43-control-wpa-auth-final",
        )?;
        let firmware_supplicant = cyw43_probe_wpa2_firmware_supplicant(contract)?;
        let host_eapol_session = prepare_cyw43_host_eapol_session(contract, credentials)?;
        cyw43_configure_host_eapol_rx(contract)?;
        emit_cyw43_join_policy_trace(
            contract,
            credentials,
            "wpa2-psk",
            "host-eapol-required",
            "primary-bsscfg:join",
            firmware_supplicant.label(),
            "host-eapol-deferred",
        );
        cyw43_submit_join_request(contract, credentials)?;
        arm_cyw43_host_eapol_pending(contract, host_eapol_session, mac)?;
        service_cyw43_host_eapol_join_submit_window(
            contract,
            credentials,
            CYW43_HOST_EAPOL_JOIN_SUBMIT_POLLS,
            crate::hal::timebase().now_ms(),
        );
        return Ok(());
    }
    cyw43_submit_bcdc_iovar_u32(contract, "auth", 0, "cyw43-control-auth")?;
    cyw43_submit_bcdc_iovar_u32(
        contract,
        "wsec",
        CYW43_WSEC_NONE,
        "cyw43-control-security-open",
    )?;
    cyw43_submit_bcdc_iovar_u32(
        contract,
        "wpa_auth",
        CYW43_WPA_AUTH_DISABLED,
        "cyw43-control-wpa-auth",
    )?;
    emit_cyw43_join_policy_trace(
        contract,
        credentials,
        "open",
        "set-ssid-or-event",
        "primary-bsscfg:join",
        "not-needed",
        "not-needed",
    );
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
fn cyw43_apply_linux_wpa2_rsn_capability_policy(
    contract: DriverTaskContract,
) -> Result<(), DriverTaskNetError> {
    cyw43_submit_bcdc_iovar_u32_optional_unsupported_or_submit_fault(
        contract,
        "mfp",
        CYW43_MFP_NONE,
        "cyw43-control-rsn-mfp",
    )?;
    cyw43_submit_bcdc_iovar_u32(
        contract,
        "wme_bss_disable",
        CYW43_WME_BSS_DISABLE_RSN_DEFAULT,
        "cyw43-control-rsn-wme-bss-disable",
    )
}

#[cfg(feature = "kernel")]
fn cyw43_apply_linux_connect_station_policy(
    contract: DriverTaskContract,
) -> Result<(), DriverTaskNetError> {
    let (mpc_name, mpc_value) = CYW43_LINUX_CONNECT_STATION_POLICY_IOVARS[0];
    let (arp_ol_name, arp_ol_value) = CYW43_LINUX_CONNECT_STATION_POLICY_IOVARS[1];
    let (arpoe_name, arpoe_value) = CYW43_LINUX_CONNECT_STATION_POLICY_IOVARS[2];
    let (ndoe_name, ndoe_value) = CYW43_LINUX_CONNECT_STATION_POLICY_IOVARS[3];
    cyw43_submit_bcdc_iovar_u32(contract, mpc_name, mpc_value, "cyw43-control-connect-mpc")?;
    cyw43_submit_bcdc_iovar_u32_optional_unsupported_or_submit_fault(
        contract,
        arp_ol_name,
        arp_ol_value,
        "cyw43-control-connect-arp-ol",
    )?;
    cyw43_submit_bcdc_iovar_u32_optional_unsupported_or_submit_fault(
        contract,
        arpoe_name,
        arpoe_value,
        "cyw43-control-connect-arpoe",
    )?;
    cyw43_submit_bcdc_iovar_u32_optional_unsupported_or_submit_fault(
        contract,
        ndoe_name,
        ndoe_value,
        "cyw43-control-connect-ndoe",
    )
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cyw43FirmwareSupplicantProbe {
    Disabled,
    Unsupported,
}

#[cfg(feature = "kernel")]
impl Cyw43FirmwareSupplicantProbe {
    const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "sup_wpa-disabled-host-eapol",
            Self::Unsupported => "sup_wpa-unsupported-host-eapol",
        }
    }
}

#[cfg(feature = "kernel")]
fn cyw43_probe_wpa2_firmware_supplicant(
    contract: DriverTaskContract,
) -> Result<Cyw43FirmwareSupplicantProbe, DriverTaskNetError> {
    emit_cyw43_firmware_supplicant_trace(
        contract,
        "primary-plain",
        "disable",
        "set-disabled",
        "host-eapol-authoritative",
        0,
    );
    let primary_data = 0u32.to_le_bytes();
    if cyw43_probe_wpa2_firmware_supplicant_iovar(
        contract,
        "sup_wpa",
        &primary_data,
        "cyw43-join-security-sup-wpa-disable",
    )? {
        emit_cyw43_firmware_supplicant_trace(
            contract,
            "primary-plain",
            "disabled",
            "continue-host-eapol-required",
            "host-eapol-authoritative",
            0,
        );
        return Ok(Cyw43FirmwareSupplicantProbe::Disabled);
    }
    emit_cyw43_firmware_supplicant_trace(
        contract,
        "primary-plain",
        "unsupported",
        "try-bsscfg-wrapper",
        "host-eapol-authoritative",
        CYW43_BCME_UNSUPPORTED_STATUS,
    );

    let mut wrapper_data = [0u8; 8];
    wrapper_data[..4].copy_from_slice(&CYW43_BSSCFG_PRIMARY_INDEX.to_le_bytes());
    wrapper_data[4..8].copy_from_slice(&0u32.to_le_bytes());
    if cyw43_probe_wpa2_firmware_supplicant_iovar(
        contract,
        "bsscfg:sup_wpa",
        &wrapper_data,
        "cyw43-join-security-bsscfg-sup-wpa-disable",
    )? {
        emit_cyw43_firmware_supplicant_trace(
            contract,
            "bsscfg-wrapper",
            "disabled",
            "continue-host-eapol-required",
            "host-eapol-authoritative",
            0,
        );
        return Ok(Cyw43FirmwareSupplicantProbe::Disabled);
    }
    emit_cyw43_firmware_supplicant_trace(
        contract,
        "bsscfg-wrapper",
        "unsupported",
        "continue-host-eapol-required",
        "host-eapol-authoritative",
        CYW43_BCME_UNSUPPORTED_STATUS,
    );
    Ok(Cyw43FirmwareSupplicantProbe::Unsupported)
}

#[cfg(feature = "kernel")]
fn cyw43_probe_wpa2_firmware_supplicant_iovar(
    contract: DriverTaskContract,
    name: &str,
    data: &[u8],
    stage: &'static str,
) -> Result<bool, DriverTaskNetError> {
    match cyw43_submit_bcdc_iovar_bytes_unmapped_with_header_mode(
        contract,
        name,
        data,
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
            clear_cyw43_runtime_command_fault_status();
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
fn emit_cyw43_firmware_supplicant_trace(
    contract: DriverTaskContract,
    path: &'static str,
    status: &'static str,
    action: &'static str,
    reason: &'static str,
    result: u32,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<256>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_FIRMWARE_SUPPLICANT contract={} path={} status={} action={} reason={} eapver=0x{:08x} timeout_ms={} result=0x{:08x}",
        contract.name,
        path,
        status,
        action,
        reason,
        CYW43_SUP_WPA2_EAPVER_ANY,
        CYW43_SUP_WPA_TIMEOUT_MS,
        result,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
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
    cyw43_submit_bcdc_u32_with_options(contract, cmd, value, stage, false)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_u32_with_options(
    contract: DriverTaskContract,
    cmd: u32,
    value: u32,
    stage: &'static str,
    pre_tx_drain: bool,
) -> Result<(), DriverTaskNetError> {
    #[cfg(test)]
    if CYW43_HOST_EAPOL_TEST_IO_STUB.load(Ordering::Acquire) != 0 {
        let _ = (contract, cmd, value, pre_tx_drain);
        if stage == "cyw43-host-eapol-reassert-wsec"
            || stage == "cyw43-host-eapol-post-secure-reassert-wsec"
        {
            CYW43_HOST_EAPOL_TEST_WSEC_REASSERTED.fetch_add(1, Ordering::AcqRel);
        }
        return Ok(());
    }
    let mut payload = [0u8; 4];
    payload.copy_from_slice(&value.to_le_bytes());
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_frame(&mut frame, cmd, CYW43_BCDC_FLAG_SET, id, &payload)?;
    cyw43_submit_control_exchange_checked_with_options(
        contract,
        &frame[..len],
        cmd,
        id,
        stage,
        Cyw43ControlHeaderMode::Extended,
        pre_tx_drain,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_u32_optional_filter(
    contract: DriverTaskContract,
    cmd: u32,
    value: u32,
    stage: &'static str,
    pre_tx_drain: bool,
) -> Result<bool, DriverTaskNetError> {
    #[cfg(test)]
    if CYW43_HOST_EAPOL_TEST_IO_STUB.load(Ordering::Acquire) != 0 {
        let _ = (contract, cmd, value, stage, pre_tx_drain);
        return Ok(false);
    }
    let mut payload = [0u8; 4];
    payload.copy_from_slice(&value.to_le_bytes());
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_frame(&mut frame, cmd, CYW43_BCDC_FLAG_SET, id, &payload)?;
    cyw43_submit_bcdc_optional_filter_exchange(
        contract,
        &frame[..len],
        cmd,
        id,
        stage,
        Cyw43ControlHeaderMode::Extended,
        pre_tx_drain,
    )
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
    emit_cyw43_set_ssid_payload_trace(contract, credentials, &ssid_payload);
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
            CYW43_PRIMARY_BSSCFG_JOIN_READY.store(1, Ordering::Release);
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
            CYW43_PRIMARY_BSSCFG_JOIN_READY.store(0, Ordering::Release);
            emit_cyw43_join_request_trace(
                contract,
                "primary-bsscfg:join",
                "fallback-set-ssid",
                usize::from(credentials.ssid_len),
                completion.result,
            );
        }
        Err(err) => {
            CYW43_PRIMARY_BSSCFG_JOIN_READY.store(0, Ordering::Release);
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
    cyw43_submit_linux_bsscfg_join_with_stage(contract, credentials, "cyw43-join-bsscfg")
}

#[cfg(feature = "kernel")]
fn cyw43_submit_linux_bsscfg_join_with_stage(
    contract: DriverTaskContract,
    credentials: crate::net::WifiCredentials,
    stage: &'static str,
) -> Result<(), Cyw43CommandSubmitError> {
    let mut join_payload = [0u8; CYW43_LINUX_BSSCFG_JOIN_PAYLOAD_LEN];
    cyw43_write_linux_bsscfg_join_payload(&mut join_payload, credentials)
        .map_err(|err| Cyw43CommandSubmitError::Runtime(DriverTaskNetError::RuntimeInit(err)))?;
    emit_cyw43_join_payload_trace(contract, credentials, &join_payload);
    cyw43_submit_bcdc_iovar_bytes_unmapped_with_header_mode(
        contract,
        "join",
        &join_payload,
        stage,
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
    emit_cyw43_wifi_gate7_subgate(
        contract,
        "join-request",
        "7a",
        "join-submit",
        action,
        path,
        0,
        false,
        false,
        0,
        0,
        0,
        result,
    );
}

#[cfg(feature = "kernel")]
fn emit_cyw43_join_policy_trace(
    contract: DriverTaskContract,
    credentials: WifiCredentials,
    security: &'static str,
    completion_rule: &'static str,
    join_path: &'static str,
    firmware_supplicant: &'static str,
    pmk: &'static str,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<512>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_JOIN_POLICY contract={} security={} ssid_len={} psk_kind={} completion_rule={} join_path={} firmware_supplicant={} pmk={} wpaie={} host_eapol_rx={} mpc=0 offloads=disabled connect_policy=pre-security security_iovars=set-var order={}",
        contract.name,
        security,
        credentials.ssid_len,
        cyw43_credentials_psk_kind(credentials),
        completion_rule,
        join_path,
        firmware_supplicant,
        pmk,
        yes_no(credentials.has_psk()),
        yes_no(credentials.has_psk()),
        CYW43_JOIN_SECURITY_ORDER_LABEL,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_set_ssid_payload_trace(
    contract: DriverTaskContract,
    credentials: WifiCredentials,
    payload: &[u8],
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_SET_SSID_PAYLOAD contract={} payload_len={} ssid_len={} digest=0x{:08x}",
        contract.name,
        payload.len(),
        credentials.ssid_len,
        cyw43_redacted_payload_digest(payload),
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_join_payload_trace(
    contract: DriverTaskContract,
    credentials: WifiCredentials,
    payload: &[u8],
) {
    use core::fmt::Write;

    let assoc_bssid = payload
        .get(CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET..CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET + 6)
        .unwrap_or(&[0, 0, 0, 0, 0, 0]);
    let mut line = heapless::String::<512>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_JOIN_PAYLOAD contract={} path=primary-bsscfg:join header=extended payload_len={} ssid_len={} scan_type=0x{:02x} scan_nprobes=0x{:08x} scan_active_time=0x{:08x} scan_passive_time=0x{:08x} scan_home_time=0x{:08x} assoc_bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} chanspec_num={} digest=0x{:08x}",
        contract.name,
        payload.len(),
        credentials.ssid_len,
        payload
            .get(CYW43_LINUX_EXT_JOIN_SCAN_OFFSET)
            .copied()
            .unwrap_or(0),
        cyw43_join_payload_u32(payload, CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 4),
        cyw43_join_payload_u32(payload, CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 8),
        cyw43_join_payload_u32(payload, CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 12),
        cyw43_join_payload_u32(payload, CYW43_LINUX_EXT_JOIN_SCAN_OFFSET + 16),
        assoc_bssid[0],
        assoc_bssid[1],
        assoc_bssid[2],
        assoc_bssid[3],
        assoc_bssid[4],
        assoc_bssid[5],
        cyw43_join_payload_u32(payload, CYW43_LINUX_EXT_JOIN_ASSOC_OFFSET + 8),
        cyw43_redacted_payload_digest(payload),
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
const fn cyw43_credentials_psk_kind(credentials: WifiCredentials) -> &'static str {
    if credentials.psk_len == 0 {
        "none"
    } else if credentials.psk_len == 64 {
        "hex-pmk"
    } else {
        "passphrase"
    }
}

#[cfg(feature = "kernel")]
fn cyw43_join_payload_u32(payload: &[u8], offset: usize) -> u32 {
    let Some(bytes) = payload.get(offset..offset + 4) else {
        return 0;
    };
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(feature = "kernel")]
fn cyw43_redacted_payload_digest(payload: &[u8]) -> u32 {
    payload.iter().fold(0x811c_9dc5, |digest, byte| {
        digest.wrapping_mul(16_777_619) ^ u32::from(*byte)
    })
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
    cyw43_submit_bcdc_iovar_bytes_with_options(contract, name, data, stage, header_mode, false)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_bytes_with_options(
    contract: DriverTaskContract,
    name: &str,
    data: &[u8],
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
    pre_tx_drain: bool,
) -> Result<(), DriverTaskNetError> {
    #[cfg(test)]
    if CYW43_HOST_EAPOL_TEST_IO_STUB.load(Ordering::Acquire) != 0 {
        let _ = (contract, data, header_mode, pre_tx_drain);
        if name == "wsec"
            && (stage == "cyw43-host-eapol-reassert-wsec"
                || stage == "cyw43-host-eapol-post-secure-reassert-wsec")
        {
            CYW43_HOST_EAPOL_TEST_WSEC_REASSERTED.fetch_add(1, Ordering::AcqRel);
        }
        return Ok(());
    }
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
    cyw43_submit_control_exchange_checked_with_options(
        contract,
        &frame[..len],
        CYW43_WLC_SET_VAR,
        id,
        stage,
        header_mode,
        pre_tx_drain,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_u32(
    contract: DriverTaskContract,
    name: &str,
    value: u32,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    #[cfg(test)]
    if CYW43_HOST_EAPOL_TEST_IO_STUB.load(Ordering::Acquire) != 0 {
        let _ = (contract, name, value);
        if stage == "cyw43-host-eapol-reassert-wsec"
            || stage == "cyw43-host-eapol-post-secure-reassert-wsec"
        {
            CYW43_HOST_EAPOL_TEST_WSEC_REASSERTED.fetch_add(1, Ordering::AcqRel);
        }
        return Ok(());
    }
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
    cyw43_submit_bcdc_iovar_u32_with_options(contract, name, value, stage, header_mode, false)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_u32_with_options(
    contract: DriverTaskContract,
    name: &str,
    value: u32,
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
    pre_tx_drain: bool,
) -> Result<(), DriverTaskNetError> {
    cyw43_submit_bcdc_iovar_bytes_with_options(
        contract,
        name,
        &value.to_le_bytes(),
        stage,
        header_mode,
        pre_tx_drain,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_u32_optional_filter(
    contract: DriverTaskContract,
    name: &str,
    value: u32,
    stage: &'static str,
) -> Result<bool, DriverTaskNetError> {
    cyw43_submit_bcdc_iovar_bytes_optional_filter(contract, name, &value.to_le_bytes(), stage)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_u32_optional_unsupported(
    contract: DriverTaskContract,
    name: &str,
    value: u32,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    cyw43_submit_bcdc_iovar_u32_optional_with_policy(contract, name, value, stage, false)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_u32_optional_unsupported_or_submit_fault(
    contract: DriverTaskContract,
    name: &str,
    value: u32,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    cyw43_submit_bcdc_iovar_u32_optional_with_policy(contract, name, value, stage, true)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_u32_optional_with_policy(
    contract: DriverTaskContract,
    name: &str,
    value: u32,
    stage: &'static str,
    allow_optional_submit_fault: bool,
) -> Result<(), DriverTaskNetError> {
    match cyw43_submit_bcdc_iovar_bytes_unmapped_with_header_mode(
        contract,
        name,
        &value.to_le_bytes(),
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
        Err(Cyw43CommandSubmitError::Completion(completion))
            if allow_optional_submit_fault
                && cyw43_optional_iovar_submit_fault_is_fail_soft(name, stage, completion) =>
        {
            clear_cyw43_runtime_command_fault_status();
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "optional-submit-fault",
                Some(completion),
            );
            Ok(())
        }
        Err(err) => Err(err.into_net_error()),
    }
}

#[cfg(feature = "kernel")]
fn cyw43_optional_iovar_submit_fault_is_fail_soft(
    name: &str,
    stage: &'static str,
    completion: DriverTaskCompletionRecord,
) -> bool {
    matches!(
        (name, stage),
        ("mfp", "cyw43-control-rsn-mfp")
            | ("arp_ol", "cyw43-control-connect-arp-ol")
            | ("arpoe", "cyw43-control-connect-arpoe")
            | ("ndoe", "cyw43-control-connect-ndoe")
    ) && completion.code == DriverTaskCompletionCode::Fault.as_u16()
        && cyw43_fault_detail_allows_same_command_retry(completion.detail)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_iovar_bytes_optional_filter(
    contract: DriverTaskContract,
    name: &str,
    data: &[u8],
    stage: &'static str,
) -> Result<bool, DriverTaskNetError> {
    #[cfg(test)]
    if CYW43_HOST_EAPOL_TEST_IO_STUB.load(Ordering::Acquire) != 0 {
        let _ = (contract, name, data, stage);
        return Ok(false);
    }
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
    cyw43_submit_bcdc_optional_filter_exchange(
        contract,
        &frame[..len],
        CYW43_WLC_SET_VAR,
        id,
        stage,
        Cyw43ControlHeaderMode::Extended,
        false,
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

#[inline]
const fn cyw43_clm_iovar_data_len(chunk_len: usize) -> usize {
    CYW43_CLM_IOVAR_HEADER_BYTES + chunk_len
}

#[inline]
const fn cyw43_clm_setvar_payload_len(chunk_len: usize) -> usize {
    CYW43_CLM_IOVAR_NAME_WITH_NUL_BYTES + cyw43_clm_iovar_data_len(chunk_len)
}

fn cyw43_clm_download_flags(offset: usize, chunk_len: usize, total_len: usize) -> u16 {
    let mut flags = CYW43_CLM_DOWNLOAD_FLAG_HANDLER_VER;
    if offset == 0 {
        flags |= CYW43_CLM_DOWNLOAD_FLAG_BEGIN;
    }
    if offset.saturating_add(chunk_len) >= total_len {
        flags |= CYW43_CLM_DOWNLOAD_FLAG_END;
    }
    flags
}

fn cyw43_write_clm_download_payload(
    payload: &mut [u8],
    clm: &[u8],
    offset: usize,
    chunk_len: usize,
) -> Result<usize, DriverTaskNetError> {
    if chunk_len == 0 || chunk_len > CYW43_CLM_CHUNK_BYTES {
        return Err(DriverTaskNetError::RuntimeInit("cyw43-clm-chunk-len"));
    }
    let end = offset
        .checked_add(chunk_len)
        .ok_or(DriverTaskNetError::RuntimeInit("cyw43-clm-chunk-len"))?;
    if end > clm.len() {
        return Err(DriverTaskNetError::RuntimeInit("cyw43-clm-chunk-len"));
    }
    let payload_len = cyw43_clm_iovar_data_len(chunk_len);
    if payload.len() < payload_len {
        return Err(DriverTaskNetError::RuntimeInit("cyw43-clm-payload-len"));
    }

    payload[..payload_len].fill(0);
    let flags = cyw43_clm_download_flags(offset, chunk_len, clm.len());
    payload[0..2].copy_from_slice(&flags.to_le_bytes());
    payload[2..4].copy_from_slice(&CYW43_CLM_DOWNLOAD_TYPE.to_le_bytes());
    payload[4..8].copy_from_slice(
        &u32::try_from(chunk_len)
            .map_err(|_| DriverTaskNetError::RuntimeInit("cyw43-clm-chunk-len"))?
            .to_le_bytes(),
    );
    payload[8..12].copy_from_slice(&0u32.to_le_bytes());
    payload[CYW43_CLM_IOVAR_HEADER_BYTES..payload_len].copy_from_slice(&clm[offset..end]);
    Ok(payload_len)
}

#[cfg(feature = "kernel")]
fn cyw43_load_runtime_clm(
    contract: DriverTaskContract,
    clm_blob: Option<&[u8]>,
) -> Result<(), DriverTaskNetError> {
    let Some(clm) = clm_blob.filter(|blob| !blob.is_empty()) else {
        emit_cyw43_clm_trace(contract, "cyw43-control-clmload", "skip", 0, 0, 0, 0);
        return Ok(());
    };

    let mut offset = 0usize;
    let mut chunk_index = 0usize;
    let mut payload = [0u8; CYW43_CLM_IOVAR_HEADER_BYTES + CYW43_CLM_CHUNK_BYTES];
    while offset < clm.len() {
        let chunk_len = core::cmp::min(clm.len() - offset, CYW43_CLM_CHUNK_BYTES);
        let flags = cyw43_clm_download_flags(offset, chunk_len, clm.len());
        let payload_len = cyw43_write_clm_download_payload(&mut payload, clm, offset, chunk_len)?;
        emit_cyw43_clm_trace(
            contract,
            "cyw43-control-clmload",
            "chunk",
            chunk_index,
            offset,
            chunk_len,
            flags,
        );
        cyw43_submit_bcdc_iovar_bytes(
            contract,
            CYW43_CLM_IOVAR_NAME,
            &payload[..payload_len],
            "cyw43-control-clmload",
        )?;
        offset += chunk_len;
        chunk_index += 1;
    }

    emit_cyw43_clm_trace(
        contract,
        "cyw43-control-clmload",
        "ready",
        chunk_index,
        clm.len(),
        clm.len(),
        0,
    );
    cyw43_read_runtime_text_iovar(contract, "ver", "cyw43-control-firmware-version")?;
    cyw43_read_runtime_text_iovar(contract, "clmver", "cyw43-control-clm-version")?;
    Ok(())
}

#[cfg(feature = "kernel")]
fn cyw43_read_runtime_text_iovar(
    contract: DriverTaskContract,
    name: &str,
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    let mut response = [0u8; CYW43_CLM_VERSION_RESPONSE_BYTES];
    let response_len = cyw43_get_bcdc_iovar(contract, name, &mut response, stage)?;
    if response_len == 0 {
        return Err(DriverTaskNetError::RuntimeInit(stage));
    }
    let printable_len = response[..response_len]
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(response_len);
    if printable_len == 0 {
        return Err(DriverTaskNetError::RuntimeInit(stage));
    }
    emit_cyw43_text_iovar_trace(contract, stage, name, printable_len);
    Ok(())
}

#[cfg(feature = "kernel")]
fn emit_cyw43_clm_trace(
    contract: DriverTaskContract,
    stage: &'static str,
    action: &'static str,
    index: usize,
    offset: usize,
    len: usize,
    flags: u16,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_CLM contract={} stage={} action={} index={} offset={} len={} flags=0x{:04x}",
        contract.name, stage, action, index, offset, len, flags
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_text_iovar_trace(
    contract: DriverTaskContract,
    stage: &'static str,
    name: &str,
    printable_len: usize,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_TEXT_IOVAR contract={} stage={} name={} printable_len={}",
        contract.name, stage, name, printable_len
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn cyw43_prepare_runtime_control_plane(
    contract: DriverTaskContract,
    clm_blob: Option<&[u8]>,
) -> Result<EthernetAddress, DriverTaskNetError> {
    cyw43_submit_bcdc_iovar_u32_with_options(
        contract,
        "bus:txglomalign",
        8,
        "cyw43-control-txglomalign",
        Cyw43ControlHeaderMode::Plain,
        false,
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
    cyw43_load_runtime_clm(contract, clm_blob)?;
    cyw43_submit_bcdc_iovar_u32(contract, "mpc", 0, "cyw43-control-mpc")?;
    cyw43_apply_linux_prejoin_association_policy(contract)?;
    Ok(mac)
}

#[cfg(feature = "kernel")]
fn cyw43_apply_linux_prejoin_association_policy(
    contract: DriverTaskContract,
) -> Result<(), DriverTaskNetError> {
    if let Some(mpc_value) = CYW43_LINUX_PREJOIN_MPC_VALUE {
        cyw43_submit_bcdc_iovar_u32_with_options(
            contract,
            "mpc",
            mpc_value,
            "cyw43-control-prejoin-mpc",
            Cyw43ControlHeaderMode::Extended,
            true,
        )?;
    }
    cyw43_submit_bcdc_iovar_bytes(
        contract,
        "join_pref",
        &CYW43_LINUX_JOIN_PREF_DEFAULT,
        "cyw43-control-prejoin-join-pref",
    )?;
    cyw43_enable_join_event_messages(contract, "cyw43-control-prejoin-event-mask")?;
    cyw43_submit_bcdc_u32(
        contract,
        CYW43_WLC_SET_SCAN_CHANNEL_TIME,
        CYW43_LINUX_SCAN_CHANNEL_TIME_MS,
        "cyw43-control-prejoin-scan-channel-time",
    )?;
    cyw43_submit_bcdc_u32(
        contract,
        CYW43_WLC_SET_SCAN_UNASSOC_TIME,
        CYW43_LINUX_SCAN_UNASSOC_TIME_MS,
        "cyw43-control-prejoin-scan-unassoc-time",
    )?;
    cyw43_submit_bcdc_iovar_u32_optional_unsupported(
        contract,
        "txbf",
        1,
        "cyw43-control-prejoin-txbf",
    )
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
    stage: &'static str,
) -> Result<EthernetAddress, Cyw43BssidRefreshError> {
    #[cfg(test)]
    if CYW43_HOST_EAPOL_TEST_BSSID_VALID.load(Ordering::Acquire) != 0 {
        let _ = (contract, stage);
        return Ok(EthernetAddress([0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5]));
    }
    let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
    let response = [0u8; ETHER_ADDR_LEN];
    let id = cyw43_next_bcdc_ioctl_id();
    let len = cyw43_write_bcdc_frame(
        &mut frame,
        CYW43_WLC_GET_BSSID,
        CYW43_BCDC_FLAG_GET,
        id,
        &response,
    )
    .map_err(Cyw43BssidRefreshError::Runtime)?;
    let mut tx_retries_spent = 0usize;
    let completion = loop {
        match cyw43_submit_control_exchange_unmapped_with_options(
            contract,
            &frame[..len],
            CYW43_WLC_GET_BSSID,
            id,
            stage,
            Cyw43ControlHeaderMode::Extended,
            CYW43_HOST_EAPOL_BSSID_REFRESH_PRE_TX_DRAIN,
        ) {
            Ok(completion) => break completion,
            Err(err) => {
                if let Some(completion) =
                    cyw43_bssid_refresh_tx_retry_completion(err, tx_retries_spent)
                {
                    tx_retries_spent = tx_retries_spent.saturating_add(1);
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        contract,
                        DriverTaskHotPath::Cyw43Wifi,
                        stage,
                        "tx-fault-retry",
                        Some(completion),
                    );
                    continue;
                }
                return Err(match err {
                    Cyw43CommandSubmitError::Runtime(err) => Cyw43BssidRefreshError::Runtime(err),
                    Cyw43CommandSubmitError::Completion(completion) => {
                        Cyw43BssidRefreshError::Completion(completion)
                    }
                });
            }
        }
    };
    let Some(bytes) =
        crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)
    else {
        return Err(Cyw43BssidRefreshError::Runtime(
            DriverTaskNetError::RuntimeInit("cyw43-host-eapol-bssid-refresh"),
        ));
    };
    if bytes.len() < ETHER_ADDR_LEN {
        return Err(Cyw43BssidRefreshError::Runtime(
            DriverTaskNetError::RuntimeInit("cyw43-host-eapol-bssid-short"),
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
    pre_tx_drain: bool,
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
    let runtime_flags = cyw43_control_runtime_flags(header_mode, pre_tx_drain);
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
                runtime_flags,
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
                runtime_flags,
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
const fn cyw43_control_exchange_completion_is_optional_filter_reject(
    completion: DriverTaskCompletionRecord,
) -> bool {
    completion.code == DriverTaskCompletionCode::Fault.as_u16()
        && completion.detail == CYW43_CONTROL_EXCHANGE_FAULT_DETAIL
        && (completion.result == CYW43_BCME_UNSUPPORTED_STATUS
            || completion.result == CYW43_BCME_BADARG_STATUS)
}

#[cfg(feature = "kernel")]
fn cyw43_submit_bcdc_optional_filter_exchange(
    contract: DriverTaskContract,
    payload: &[u8],
    cmd: u32,
    id: u16,
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
    pre_tx_drain: bool,
) -> Result<bool, DriverTaskNetError> {
    match cyw43_submit_control_exchange_unmapped_with_options(
        contract,
        payload,
        cmd,
        id,
        stage,
        header_mode,
        pre_tx_drain,
    ) {
        Ok(completion) if completion.code == DriverTaskCompletionCode::FrameReady.as_u16() => {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "ready",
                Some(completion),
            );
            Ok(false)
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
            if cyw43_control_exchange_completion_is_optional_filter_reject(completion) =>
        {
            clear_cyw43_runtime_command_fault_status();
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "optional-filter-reject",
                Some(completion),
            );
            Ok(true)
        }
        Err(err) => Err(err.into_net_error()),
    }
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
const CYW43_POST_SECURE_DATA_ALLMULTI: u32 = 1;

#[cfg(feature = "kernel")]
const CYW43_POST_SECURE_DATA_PROMISC: u32 = 1;

#[cfg(feature = "kernel")]
fn restore_cyw43_host_eapol_rx_after_secure(contract: DriverTaskContract) -> bool {
    #[cfg(test)]
    if CYW43_HOST_EAPOL_TEST_IO_STUB.load(Ordering::Acquire) != 0 {
        CYW43_HOST_EAPOL_TEST_RX_RESTORED.fetch_add(1, Ordering::AcqRel);
        CYW43_POST_SECURE_DATA_RX_ADMITTED.store(1, Ordering::Release);
        emit_cyw43_host_eapol_rx_admission_restore(contract, "ready");
        return true;
    }
    match cyw43_restore_post_secure_data_filters(contract) {
        Ok(repair_pending) => {
            CYW43_POST_SECURE_DATA_RX_ADMITTED.store(1, Ordering::Release);
            let status = if repair_pending {
                "repair-pending"
            } else {
                "ready"
            };
            emit_cyw43_host_eapol_rx_admission_restore(contract, status);
            true
        }
        Err(_) => {
            CYW43_POST_SECURE_DATA_RX_ADMITTED.store(0, Ordering::Release);
            emit_cyw43_host_eapol_rx_admission_restore(contract, "error");
            false
        }
    }
}

#[cfg(feature = "kernel")]
fn cyw43_restore_post_secure_data_filters(
    contract: DriverTaskContract,
) -> Result<bool, DriverTaskNetError> {
    let mut mcast = [0u8; 10];
    mcast[..4].copy_from_slice(&1u32.to_le_bytes());
    mcast[4..10].copy_from_slice(&CYW43_PAE_GROUP_ADDR);
    cyw43_submit_bcdc_iovar_bytes(
        contract,
        "mcast_list",
        &mcast,
        "cyw43-host-eapol-restore-mcast",
    )?;
    let allmulti_repair_pending = cyw43_submit_bcdc_iovar_u32_optional_filter(
        contract,
        "allmulti",
        CYW43_POST_SECURE_DATA_ALLMULTI,
        "cyw43-host-eapol-restore-allmulti",
    )?;
    let promisc_repair_pending = cyw43_submit_bcdc_u32_optional_filter(
        contract,
        CYW43_WLC_SET_PROMISC,
        CYW43_POST_SECURE_DATA_PROMISC,
        "cyw43-host-eapol-restore-promisc",
        CYW43_HOST_EAPOL_PROMISC_PRE_TX_DRAIN,
    )?;
    Ok(allmulti_repair_pending || promisc_repair_pending)
}

#[cfg(feature = "kernel")]
fn cyw43_post_secure_data_rx_admitted() -> bool {
    CYW43_POST_SECURE_DATA_RX_ADMITTED.load(Ordering::Acquire) != 0
}

#[cfg(feature = "kernel")]
pub(crate) fn reassert_cyw43_post_secure_data_rx(contract: DriverTaskContract) -> bool {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT
        || CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) == 0
    {
        return false;
    }
    if cyw43_post_secure_data_rx_admitted() {
        return true;
    }
    restore_cyw43_host_eapol_rx_after_secure(contract)
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
    let allmulti_repair_pending = cyw43_submit_bcdc_iovar_u32_optional_filter(
        contract,
        "allmulti",
        allmulti,
        allmulti_stage,
    )?;
    let promisc_repair_pending = cyw43_submit_bcdc_u32_optional_filter(
        contract,
        CYW43_WLC_SET_PROMISC,
        promisc,
        promisc_stage,
        CYW43_HOST_EAPOL_PROMISC_PRE_TX_DRAIN,
    )?;
    if allmulti_repair_pending || promisc_repair_pending {
        emit_cyw43_host_eapol_rx_admission_restore(contract, "repair-pending");
    }
    Ok(())
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
    cyw43_install_wsec_key_with_pre_tx_drain(contract, index, key, ea, rsc, primary, stage, false)
}

#[cfg(feature = "kernel")]
fn cyw43_install_wsec_key_with_pre_tx_drain(
    contract: DriverTaskContract,
    index: u32,
    key: &[u8],
    ea: &[u8; 6],
    rsc: Option<&[u8]>,
    primary: bool,
    stage: &'static str,
    pre_tx_drain: bool,
) -> Result<(), DriverTaskNetError> {
    let mut payload = [0u8; WSEC_KEY_PAYLOAD_LEN];
    let len = cyw43_host_eapol::write_wsec_key_payload(&mut payload, index, key, ea, rsc, primary)
        .map_err(DriverTaskNetError::RuntimeInit)?;
    #[cfg(test)]
    if CYW43_HOST_EAPOL_TEST_IO_STUB.load(Ordering::Acquire) != 0 {
        let _ = (contract, len);
        if pre_tx_drain {
            CYW43_HOST_EAPOL_TEST_WSEC_PRE_TX_DRAIN.fetch_add(1, Ordering::AcqRel);
        }
        if stage == "cyw43-host-eapol-ptk" || stage == "cyw43-host-eapol-post-secure-ptk" {
            CYW43_HOST_EAPOL_TEST_PTK_INSTALLED.fetch_add(1, Ordering::AcqRel);
        } else if stage == "cyw43-host-eapol-gtk" || stage == "cyw43-host-eapol-post-secure-gtk" {
            CYW43_HOST_EAPOL_TEST_GTK_INSTALLED.fetch_add(1, Ordering::AcqRel);
        }
        return Ok(());
    }
    cyw43_submit_bcdc_iovar_bytes_with_options(
        contract,
        "wsec_key",
        &payload[..len],
        stage,
        Cyw43ControlHeaderMode::Extended,
        pre_tx_drain,
    )
}

#[cfg(feature = "kernel")]
fn prepare_cyw43_host_eapol_session(
    contract: DriverTaskContract,
    credentials: WifiCredentials,
) -> Result<Cyw43HostEapolSession, DriverTaskNetError> {
    let session = Cyw43HostEapolSession::new(credentials)?;
    emit_cyw43_host_eapol_pmk_ready(contract, credentials);
    Ok(session)
}

#[cfg(feature = "kernel")]
fn arm_cyw43_host_eapol_pending(
    contract: DriverTaskContract,
    mut session: Cyw43HostEapolSession,
    _station_mac: EthernetAddress,
) -> Result<(), DriverTaskNetError> {
    cyw43_apply_pending_host_eapol_event(contract, &mut session, 0);
    let progress = session.progress;
    *CYW43_HOST_EAPOL_SESSION.lock() = Some(session);
    CYW43_CONTROL_PLANE_READY.store(1, Ordering::Release);
    CYW43_ASSOCIATED.store(if progress.associated { 1 } else { 0 }, Ordering::Release);
    CYW43_LINK_UP.store(if progress.link_up { 1 } else { 0 }, Ordering::Release);
    CYW43_HOST_EAPOL_ACTIVE.store(1, Ordering::Release);
    CYW43_HOST_EAPOL_REQUIRED.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);
    CYW43_POST_SECURE_DATA_RX_ADMITTED.store(0, Ordering::Release);
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
fn emit_cyw43_host_eapol_pmk_ready(contract: DriverTaskContract, credentials: WifiCredentials) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_PMK contract={} status=ready kind={} ssid_len={} psk_len={} action=derive-host-ptk-on-m1",
        contract.name,
        cyw43_credentials_psk_kind(credentials),
        credentials.ssid_len,
        credentials.psk_len,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn service_cyw43_host_eapol_join_submit_window(
    contract: DriverTaskContract,
    credentials: WifiCredentials,
    poll_limit: usize,
    now_ms: u64,
) {
    let before = cyw43_host_eapol_progress_snapshot();
    emit_cyw43_host_eapol_join_submit_window(contract, "begin", poll_limit, false, before.as_ref());
    let outcome = service_cyw43_host_eapol_slice_with_outcome(credentials, poll_limit, now_ms);
    emit_cyw43_host_eapol_join_submit_window(
        contract,
        "end",
        poll_limit,
        outcome.activity,
        outcome.progress.as_ref(),
    );
    if outcome.post_rescue_assoc_window_due {
        let post_rescue_now_ms = crate::hal::timebase().now_ms();
        emit_cyw43_host_eapol_join_submit_window(
            contract,
            "post-rescue-begin",
            poll_limit,
            false,
            outcome.progress.as_ref(),
        );
        let post_rescue = service_cyw43_host_eapol_slice_with_outcome(
            credentials,
            poll_limit,
            post_rescue_now_ms,
        );
        emit_cyw43_host_eapol_join_submit_window(
            contract,
            "post-rescue-end",
            poll_limit,
            post_rescue.activity,
            post_rescue.progress.as_ref(),
        );
    }
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_progress_snapshot() -> Option<Cyw43HostEapolProgress> {
    CYW43_HOST_EAPOL_SESSION
        .lock()
        .as_ref()
        .map(|session| session.progress)
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Cyw43HostEapolSliceOutcome {
    activity: bool,
    post_rescue_assoc_window_due: bool,
    progress: Option<Cyw43HostEapolProgress>,
}

#[cfg(feature = "kernel")]
pub(crate) fn service_cyw43_host_eapol_slice(
    credentials: WifiCredentials,
    poll_limit: usize,
    now_ms: u64,
) -> bool {
    service_cyw43_host_eapol_slice_with_outcome(credentials, poll_limit, now_ms).activity
}

#[cfg(feature = "kernel")]
fn service_cyw43_host_eapol_slice_with_outcome(
    credentials: WifiCredentials,
    poll_limit: usize,
    now_ms: u64,
) -> Cyw43HostEapolSliceOutcome {
    if poll_limit == 0
        || CYW43_HOST_EAPOL_ACTIVE.load(Ordering::Acquire) == 0
        || CYW43_HOST_EAPOL_REQUIRED.load(Ordering::Acquire) != 0
        || CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) != 0
        || !runtime_ready(DriverTaskHotPath::Cyw43Wifi)
    {
        return Cyw43HostEapolSliceOutcome::default();
    }

    let contract = CYW43_WIFI_DRIVER_TASK_CONTRACT;
    let station_mac = *CYW43_RUNTIME_MAC.lock();
    let mut guard = CYW43_HOST_EAPOL_SESSION.lock();
    if guard.is_none() {
        let Ok(session) = Cyw43HostEapolSession::new(credentials) else {
            CYW43_HOST_EAPOL_ACTIVE.store(0, Ordering::Release);
            CYW43_HOST_EAPOL_REQUIRED.store(1, Ordering::Release);
            return Cyw43HostEapolSliceOutcome::default();
        };
        *guard = Some(session);
    }
    let Some(session) = guard.as_mut() else {
        return Cyw43HostEapolSliceOutcome::default();
    };
    session.record_time(now_ms);
    let poll = session.progress.polls as usize;
    cyw43_apply_pending_host_eapol_event(contract, session, poll);
    session.record_time(now_ms);

    let mut activity = false;
    let mut clear_session = false;
    let mut current_now_ms = now_ms;
    for iteration in 0..poll_limit {
        if iteration != 0 {
            current_now_ms = crate::hal::timebase().now_ms();
        }
        session.record_time(current_now_ms);
        if cyw43_host_eapol_join_timeout_expired(session, current_now_ms) {
            let was_associated = session.progress.associated;
            cyw43_probe_host_eapol_bssid_before_required(contract, station_mac, session);
            session.record_time(current_now_ms);
            if !was_associated && session.progress.associated {
                activity = true;
                continue;
            }
            mark_cyw43_host_eapol_required(contract, &session.progress);
            clear_session = true;
            activity = true;
            break;
        }
        match poll_cyw43_host_eapol_once(
            contract,
            credentials,
            station_mac,
            session,
            current_now_ms,
        ) {
            Ok(Cyw43HostEapolStep::Pending { activity: polled }) => {
                activity |= polled;
            }
            Ok(Cyw43HostEapolStep::Secure) => {
                mark_cyw43_host_eapol_secure(contract, &session.progress);
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
    let post_rescue_assoc_window_due = session.take_post_rescue_assoc_window_due();
    let progress = Some(session.progress);
    if clear_session {
        *guard = None;
    }
    Cyw43HostEapolSliceOutcome {
        activity,
        post_rescue_assoc_window_due,
        progress,
    }
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
fn merge_cyw43_host_eapol_poll_result(
    result: &mut Cyw43HostEapolPollResult,
    next: Cyw43HostEapolPollResult,
) {
    result.observed_frame |= next.observed_frame;
    result.activity |= next.activity;
    result.completed |= next.completed;
    result.secure |= next.secure;
}

#[cfg(feature = "kernel")]
const fn cyw43_host_eapol_followup_firstread_due(
    scheduled_flags: u16,
    active_flags: u16,
    active_result: Cyw43HostEapolPollResult,
) -> bool {
    scheduled_flags & DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD != 0
        && active_flags & DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD == 0
        && active_result.completed
        && !active_result.observed_frame
        && !active_result.secure
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_active_followup_plan(
    active_kind: Cyw43HostEapolPollKind,
    firstread_due: bool,
) -> (
    Option<Cyw43HostEapolPollKind>,
    Option<Cyw43HostEapolPollKind>,
) {
    if firstread_due {
        return (
            Some(Cyw43HostEapolPollKind::Control),
            Some(Cyw43HostEapolPollKind::Data),
        );
    }
    match active_kind {
        Cyw43HostEapolPollKind::Control => (Some(Cyw43HostEapolPollKind::Data), None),
        Cyw43HostEapolPollKind::Data => (Some(Cyw43HostEapolPollKind::Control), None),
    }
}

#[cfg(feature = "kernel")]
fn poll_cyw43_host_eapol_once(
    contract: DriverTaskContract,
    credentials: WifiCredentials,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
    now_ms: u64,
) -> Result<Cyw43HostEapolStep, DriverTaskNetError> {
    let poll = session.progress.polls as usize;
    let mut tx_frame = [0u8; MAX_FRAME_LEN];
    let eapol_starts = CYW43_HOST_EAPOL_START.load(Ordering::Acquire);
    let rx_poll_flags =
        if cyw43_host_eapol_rx_firstread_due_from_session(poll, eapol_starts, session, now_ms) {
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
        if result.secure {
            record_cyw43_host_eapol_poll_completion(session, result);
            return Ok(Cyw43HostEapolStep::Secure);
        }
        if result.completed && !result.secure && cyw43_active_prompt_poll(contract).is_none() {
            let firstread_due =
                cyw43_host_eapol_followup_firstread_due(rx_poll_flags, active_poll.flags, result);
            let (first_followup, second_followup) =
                cyw43_host_eapol_active_followup_plan(active_poll.kind, firstread_due);
            for followup_kind in [first_followup, second_followup] {
                let Some(followup_kind) = followup_kind else {
                    continue;
                };
                if cyw43_active_prompt_poll(contract).is_some() {
                    break;
                }
                let followup = poll_cyw43_host_eapol_kind(
                    contract,
                    station_mac,
                    session,
                    poll,
                    rx_poll_flags,
                    followup_kind,
                    &mut tx_frame,
                )?;
                merge_cyw43_host_eapol_poll_result(&mut result, followup);
                if result.secure {
                    record_cyw43_host_eapol_poll_completion(session, result);
                    return Ok(Cyw43HostEapolStep::Secure);
                }
            }
        }
        record_cyw43_host_eapol_poll_completion(session, result);
        if result.secure {
            return Ok(Cyw43HostEapolStep::Secure);
        }
        result.activity |= cyw43_service_host_eapol_maintenance_if_idle(
            contract,
            credentials,
            station_mac,
            session,
            now_ms,
        );
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
        merge_cyw43_host_eapol_poll_result(&mut result, data_result);
    }

    result.activity |= cyw43_service_host_eapol_maintenance_if_idle(
        contract,
        credentials,
        station_mac,
        session,
        now_ms,
    );
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
        let frame_len = token.len;
        let frame_channel = cyw43_frame_channel(flags);
        if frame_channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT {
            let frame = &token.buffer[..token.len];
            if let Some(event) = cyw43_parse_control_or_event_frame(frame) {
                record_cyw43_host_eapol_event(
                    contract,
                    session,
                    poll,
                    flags,
                    frame.len(),
                    event,
                    "host-eapol-control",
                );
            } else {
                session.progress.record_control_frame(flags, frame_len);
            }
        } else {
            if frame_channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL {
                let _ = store_cyw43_pending_control_reply(flags, token, completion.sequence);
            }
            session.progress.record_control_frame(flags, frame_len);
        }
    } else if completion.code == DriverTaskCompletionCode::Idle.as_u16()
        && rx_poll_flags & DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD != 0
    {
        let trace = cyw43_completion_rx_idle_trace(contract, completion);
        session
            .progress
            .record_control_rx_idle_completion_with_trace(completion, trace, poll, rx_poll_flags);
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
        let frame_channel = cyw43_frame_channel(flags);
        if frame_channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL {
            let frame_len = token.len;
            let _ = store_cyw43_pending_control_reply(flags, token, completion.sequence);
            session.progress.record_control_frame(flags, frame_len);
            return Ok(result);
        }
        let frame = &token.buffer[..token.len];
        let data_event = match frame_channel {
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT => {
                cyw43_parse_control_or_event_frame(frame)
            }
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA => cyw43_parse_data_event_frame(frame),
            _ => None,
        };
        if let Some(event) = data_event {
            record_cyw43_host_eapol_event(
                contract,
                session,
                poll,
                flags,
                frame.len(),
                event,
                "host-eapol-data",
            );
        } else {
            let ethertype = cyw43_ethertype(frame);
            session
                .progress
                .record_data_frame(flags, frame.len(), ethertype);
            if session.progress.data_rx == 1 || session.progress.eapol_rx == 1 {
                emit_cyw43_host_eapol_status(contract, "rx-observed", &session.progress);
            }
            if ethertype == Some(ETH_P_EAPOL) {
                session.record_eapol_activity(crate::hal::timebase().now_ms(), poll as u32);
                if let Some(proof) = cyw43_host_eapol::inspect_host_eapol_frame(frame) {
                    emit_cyw43_host_eapol_proof(contract, "rx", poll, frame.len(), proof);
                }
                let action = match session.eapol.handle_packet(station_mac.0, frame, tx_frame) {
                    Ok(action) => action,
                    Err(err) => {
                        session.progress.record_eapol_error(err);
                        emit_cyw43_host_eapol_error(
                            contract,
                            "handle-packet",
                            err,
                            poll,
                            frame.len(),
                        );
                        return Err(DriverTaskNetError::RuntimeInit(err));
                    }
                };
                CYW43_HOST_EAPOL_RX.store(session.eapol.rx_packets(), Ordering::Release);
                emit_cyw43_host_eapol_status(contract, "eapol-rx", &session.progress);
                match action {
                    HostEapolAction::None => {}
                    HostEapolAction::Inspect { .. } => {}
                    HostEapolAction::SendM2 { len } => {
                        CYW43_HOST_EAPOL_M1.fetch_add(1, Ordering::AcqRel);
                        session
                            .progress
                            .record_eapol_association_proof("eapol-m1", poll);
                        emit_cyw43_host_eapol_message(contract, "m1", "recv-m1", poll, frame.len());
                        let Some(m2_completion) =
                            submit_cyw43_host_eapol_payload_bounded_completion(
                                contract,
                                &tx_frame[..len],
                                "cyw43-host-eapol-m2",
                            )
                        else {
                            session.progress.record_eapol_error("host-eapol-m2-tx");
                            return Err(DriverTaskNetError::RuntimeInit("host-eapol-m2-tx"));
                        };
                        CYW43_HOST_EAPOL_M2.fetch_add(1, Ordering::AcqRel);
                        emit_cyw43_host_eapol_tx_shape(
                            contract,
                            "cyw43-host-eapol-m2",
                            poll,
                            &tx_frame[..len],
                            Some(m2_completion),
                        );
                        emit_cyw43_host_eapol_message(contract, "m2", "send-m2", poll, len);
                        match wait_cyw43_host_eapol_tx_drain(
                            contract,
                            session,
                            "m2-before-m3",
                            poll,
                            m2_completion,
                        ) {
                            Ok(_drained) => {
                                // M3 can arrive after AP M1 retransmits; only the M2 submit/proof is fatal.
                            }
                            Err(err) => {
                                session.progress.record_eapol_error("host-eapol-m2-drain");
                                return Err(err);
                            }
                        }
                        session.record_eapol_activity(crate::hal::timebase().now_ms(), poll as u32);
                        session
                            .progress
                            .record_eapol_association_proof("eapol-m2", poll);
                        result.activity = true;
                    }
                    HostEapolAction::SendM4 { len } => {
                        CYW43_HOST_EAPOL_M3.fetch_add(1, Ordering::AcqRel);
                        emit_cyw43_host_eapol_message(
                            contract,
                            "m3",
                            "recv-m3-retransmit",
                            poll,
                            frame.len(),
                        );
                        let Some(m4_completion) =
                            submit_cyw43_host_eapol_payload_bounded_completion(
                                contract,
                                &tx_frame[..len],
                                "cyw43-host-eapol-m4-retransmit",
                            )
                        else {
                            session.progress.record_eapol_error("host-eapol-m4-tx");
                            return Err(DriverTaskNetError::RuntimeInit("host-eapol-m4-tx"));
                        };
                        CYW43_HOST_EAPOL_M4.fetch_add(1, Ordering::AcqRel);
                        emit_cyw43_host_eapol_tx_shape(
                            contract,
                            "cyw43-host-eapol-m4-retransmit",
                            poll,
                            &tx_frame[..len],
                            Some(m4_completion),
                        );
                        emit_cyw43_host_eapol_message(
                            contract,
                            "m4",
                            "send-m4-retransmit",
                            poll,
                            len,
                        );
                        result.activity = true;
                    }
                    HostEapolAction::SendM4InstallKeys { len, keys } => {
                        CYW43_HOST_EAPOL_M3.fetch_add(1, Ordering::AcqRel);
                        session
                            .progress
                            .record_eapol_association_proof("eapol-m3", poll);
                        emit_cyw43_host_eapol_message(contract, "m3", "recv-m3", poll, frame.len());
                        let Some(m4_completion) =
                            submit_cyw43_host_eapol_payload_bounded_completion(
                                contract,
                                &tx_frame[..len],
                                "cyw43-host-eapol-m4",
                            )
                        else {
                            session.progress.record_eapol_error("host-eapol-m4-tx");
                            return Err(DriverTaskNetError::RuntimeInit("host-eapol-m4-tx"));
                        };
                        CYW43_HOST_EAPOL_M4.fetch_add(1, Ordering::AcqRel);
                        emit_cyw43_host_eapol_message(contract, "m4", "send-m4", poll, len);
                        let m4_drain_ready = wait_cyw43_host_eapol_tx_drain(
                            contract,
                            session,
                            "m4-before-wsec",
                            poll,
                            m4_completion,
                        )?;
                        if let Err(err) = cyw43_install_wsec_key_with_pre_tx_drain(
                            contract,
                            0,
                            &keys.pairwise_tk,
                            &keys.ap_mac,
                            None,
                            false,
                            "cyw43-host-eapol-ptk",
                            m4_drain_ready,
                        ) {
                            session
                                .progress
                                .record_eapol_error("host-eapol-ptk-install");
                            emit_cyw43_host_eapol_key(
                                contract,
                                "ptk",
                                "cyw43-host-eapol-ptk",
                                "failed",
                            );
                            return Err(err);
                        }
                        CYW43_HOST_EAPOL_PTK.fetch_add(1, Ordering::AcqRel);
                        emit_cyw43_host_eapol_key(contract, "ptk", "cyw43-host-eapol-ptk", "ready");
                        if let Some(gtk) = keys.gtk {
                            let group_ea = [0u8; 6];
                            if let Err(err) = cyw43_install_wsec_key(
                                contract,
                                u32::from(gtk.index),
                                &gtk.key[..gtk.key_len],
                                &group_ea,
                                Some(&keys.rsc),
                                true,
                                "cyw43-host-eapol-gtk",
                            ) {
                                session
                                    .progress
                                    .record_eapol_error("host-eapol-gtk-install");
                                emit_cyw43_host_eapol_key(
                                    contract,
                                    "gtk",
                                    "cyw43-host-eapol-gtk",
                                    "failed",
                                );
                                return Err(err);
                            }
                            CYW43_HOST_EAPOL_GTK.fetch_add(1, Ordering::AcqRel);
                            emit_cyw43_host_eapol_key(
                                contract,
                                "gtk",
                                "cyw43-host-eapol-gtk",
                                "ready",
                            );
                            if let Err(err) = cyw43_submit_bcdc_iovar_u32_with_options(
                                contract,
                                "wsec",
                                CYW43_WSEC_AES,
                                "cyw43-host-eapol-reassert-wsec",
                                Cyw43ControlHeaderMode::Extended,
                                CYW43_HOST_EAPOL_WSEC_REASSERT_PRE_TX_DRAIN,
                            ) {
                                session
                                    .progress
                                    .record_eapol_error("host-eapol-wsec-reassert");
                                emit_cyw43_host_eapol_key(
                                    contract,
                                    "wsec",
                                    "cyw43-host-eapol-reassert-wsec",
                                    "failed",
                                );
                                return Err(err);
                            }
                            emit_cyw43_host_eapol_key(
                                contract,
                                "wsec",
                                "cyw43-host-eapol-reassert-wsec",
                                "ready",
                            );
                            restore_cyw43_host_eapol_rx_after_secure(contract);
                            session.progress.associated = true;
                            session.progress.link_up = true;
                            CYW43_ASSOCIATED.store(1, Ordering::Release);
                            CYW43_LINK_UP.store(1, Ordering::Release);
                            result.secure = true;
                        } else {
                            emit_cyw43_host_eapol_key(
                                contract,
                                "gtk",
                                "cyw43-host-eapol-gtk",
                                "deferred",
                            );
                        }
                    }
                    HostEapolAction::SendGroupM2InstallGtk { len, keys } => {
                        emit_cyw43_host_eapol_message(
                            contract,
                            "group-key",
                            "recv-group-key",
                            poll,
                            frame.len(),
                        );
                        let group_ea = [0u8; 6];
                        if let Err(err) = cyw43_install_wsec_key(
                            contract,
                            u32::from(keys.gtk.index),
                            &keys.gtk.key[..keys.gtk.key_len],
                            &group_ea,
                            Some(&keys.rsc),
                            true,
                            "cyw43-host-eapol-gtk",
                        ) {
                            session
                                .progress
                                .record_eapol_error("host-eapol-gtk-install");
                            emit_cyw43_host_eapol_key(
                                contract,
                                "gtk",
                                "cyw43-host-eapol-gtk",
                                "failed",
                            );
                            return Err(err);
                        }
                        CYW43_HOST_EAPOL_GTK.fetch_add(1, Ordering::AcqRel);
                        emit_cyw43_host_eapol_key(contract, "gtk", "cyw43-host-eapol-gtk", "ready");
                        if let Err(err) = cyw43_submit_bcdc_iovar_u32_with_options(
                            contract,
                            "wsec",
                            CYW43_WSEC_AES,
                            "cyw43-host-eapol-reassert-wsec",
                            Cyw43ControlHeaderMode::Extended,
                            CYW43_HOST_EAPOL_WSEC_REASSERT_PRE_TX_DRAIN,
                        ) {
                            session
                                .progress
                                .record_eapol_error("host-eapol-wsec-reassert");
                            emit_cyw43_host_eapol_key(
                                contract,
                                "wsec",
                                "cyw43-host-eapol-reassert-wsec",
                                "failed",
                            );
                            return Err(err);
                        }
                        emit_cyw43_host_eapol_key(
                            contract,
                            "wsec",
                            "cyw43-host-eapol-reassert-wsec",
                            "ready",
                        );
                        let Some(group_m2_completion) =
                            submit_cyw43_host_eapol_payload_bounded_completion(
                                contract,
                                &tx_frame[..len],
                                "cyw43-host-eapol-group-m2",
                            )
                        else {
                            session
                                .progress
                                .record_eapol_error("host-eapol-group-m2-tx");
                            return Err(DriverTaskNetError::RuntimeInit("host-eapol-group-m2-tx"));
                        };
                        emit_cyw43_host_eapol_message(
                            contract,
                            "group-m2",
                            "send-group-m2",
                            poll,
                            len,
                        );
                        if !wait_cyw43_host_eapol_tx_drain(
                            contract,
                            session,
                            "group-m2-before-secure",
                            poll,
                            group_m2_completion,
                        )? {
                            session
                                .progress
                                .record_eapol_error("host-eapol-group-m2-drain");
                            return Err(DriverTaskNetError::RuntimeInit(
                                "host-eapol-tx-drain-timeout",
                            ));
                        }
                        restore_cyw43_host_eapol_rx_after_secure(contract);
                        session.progress.associated = true;
                        session.progress.link_up = true;
                        CYW43_ASSOCIATED.store(1, Ordering::Release);
                        CYW43_LINK_UP.store(1, Ordering::Release);
                        result.secure = true;
                    }
                }
            }
        }
    } else if completion.code == DriverTaskCompletionCode::Idle.as_u16()
        && rx_poll_flags & DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD != 0
    {
        let trace = cyw43_completion_rx_idle_trace(contract, completion);
        session.progress.record_rx_idle_completion_with_trace(
            completion,
            trace,
            poll,
            rx_poll_flags,
        );
    }
    Ok(result)
}

#[cfg(feature = "kernel")]
fn record_cyw43_host_eapol_event(
    contract: DriverTaskContract,
    session: &mut Cyw43HostEapolSession,
    poll: usize,
    flags: u16,
    len: usize,
    event: Cyw43EventFrame,
    stage: &'static str,
) {
    session.progress.record_event_frame(flags, len, event, poll);
    if cyw43_host_eapol_join_event_type(event.event_type) {
        session.record_pre_assoc_activity(
            crate::hal::timebase().now_ms(),
            poll.min(u32::MAX as usize) as u32,
        );
    }
    let label = cyw43_host_eapol_event_trace_label(event);
    let retained = cyw43_event_association_state_update(event).is_some()
        || cyw43_event_link_state_update(event).is_some();
    emit_cyw43_host_eapol_event_capture(contract, stage, flags, len, event, label, retained);
    emit_cyw43_host_eapol_status(contract, "event-rx", &session.progress);
}

#[cfg(feature = "kernel")]
fn cyw43_service_host_eapol_maintenance_if_idle(
    contract: DriverTaskContract,
    credentials: WifiCredentials,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
    now_ms: u64,
) -> bool {
    if cyw43_active_prompt_poll(contract).is_some() {
        return false;
    }
    session.record_time(now_ms);
    if cyw43_service_host_eapol_pre_assoc(contract, credentials, station_mac, session, now_ms) {
        return true;
    }
    cyw43_service_host_eapol_post_assoc(contract, station_mac, session, now_ms)
}

#[cfg(feature = "kernel")]
fn cyw43_service_host_eapol_pre_assoc(
    contract: DriverTaskContract,
    credentials: WifiCredentials,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
    now_ms: u64,
) -> bool {
    let pre_assoc_idle_ms = session.pre_assoc_idle_ms(now_ms);
    let poll = session.progress.polls as usize;
    if cyw43_host_eapol_set_ssid_rescue_due(session) {
        session.assoc_probe_attempts = session.assoc_probe_attempts.saturating_add(1);
        let attempt = session.assoc_probe_attempts;
        cyw43_try_host_eapol_assoc_rescue(contract, credentials, session, poll, attempt);
        return true;
    }
    if session.progress.associated
        || !cyw43_host_eapol_assoc_probe_due_any(
            session.progress.polls,
            pre_assoc_idle_ms,
            session.assoc_probe_attempts,
        )
    {
        return false;
    }

    session.assoc_probe_attempts = session.assoc_probe_attempts.saturating_add(1);
    let attempt = session.assoc_probe_attempts;
    let probe = cyw43_probe_host_eapol_assoc_state(
        contract,
        station_mac,
        session,
        poll,
        attempt,
        "cyw43-host-eapol-assoc-probe",
    );
    if matches!(probe, Cyw43AssocProbeResult::NotAssociated)
        && !session.progress.assoc_join_rescue_attempted
        && CYW43_PRIMARY_BSSCFG_JOIN_READY.load(Ordering::Acquire) != 0
        && cyw43_host_eapol_assoc_rescue_due_any(session.progress.polls, pre_assoc_idle_ms)
    {
        cyw43_try_host_eapol_assoc_rescue(contract, credentials, session, poll, attempt);
    }
    true
}

#[cfg(feature = "kernel")]
const fn cyw43_host_eapol_set_ssid_rescue_due(session: &Cyw43HostEapolSession) -> bool {
    !session.progress.associated
        && session.progress.set_ssid_failure_seen
        && !session.progress.assoc_join_rescue_attempted
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_assoc_probe_due(poll: u32, attempts: u8) -> bool {
    let index = usize::from(attempts);
    CYW43_HOST_EAPOL_ASSOC_PROBE_POLLS
        .get(index)
        .is_some_and(|threshold| poll >= *threshold)
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_assoc_probe_due_ms(elapsed_ms: u64, attempts: u8) -> bool {
    let index = usize::from(attempts);
    CYW43_HOST_EAPOL_ASSOC_PROBE_MS
        .get(index)
        .is_some_and(|threshold_ms| elapsed_ms >= *threshold_ms)
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_assoc_probe_due_any(poll: u32, elapsed_ms: u64, attempts: u8) -> bool {
    cyw43_host_eapol_assoc_probe_due(poll, attempts)
        || cyw43_host_eapol_assoc_probe_due_ms(elapsed_ms, attempts)
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_assoc_rescue_due_any(poll: u32, elapsed_ms: u64) -> bool {
    poll >= CYW43_HOST_EAPOL_ASSOC_RESCUE_POLL || elapsed_ms >= CYW43_HOST_EAPOL_ASSOC_RESCUE_MS
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_join_timeout_expired(session: &Cyw43HostEapolSession, now_ms: u64) -> bool {
    if session.progress.associated {
        let (post_assoc_idle_ms, post_assoc_idle_polls) = if session.progress.eapol_rx == 0 {
            (
                session.post_assoc_elapsed_ms(now_ms),
                session.progress.post_assoc_polls,
            )
        } else {
            (
                session.post_assoc_eapol_idle_ms(now_ms),
                session.post_assoc_eapol_idle_polls(),
            )
        };
        return if session.post_assoc_timebase_ready() {
            post_assoc_idle_ms >= CYW43_HOST_EAPOL_POST_ASSOC_TIMEOUT_MS
        } else {
            post_assoc_idle_polls >= CYW43_HOST_EAPOL_POST_ASSOC_POLLS as u32
        };
    }
    if session.pre_assoc_timebase_ready() {
        session.pre_assoc_idle_ms(now_ms) >= CYW43_HOST_EAPOL_JOIN_TIMEOUT_MS
    } else {
        session.pre_assoc_idle_polls() >= CYW43_HOST_EAPOL_JOIN_POLLS as u32
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cyw43AssocProbeResult {
    BssidObserved,
    NotAssociated,
    Ignored,
    ControlError,
}

#[cfg(feature = "kernel")]
fn cyw43_probe_host_eapol_bssid_before_required(
    contract: DriverTaskContract,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
) -> bool {
    if session.bssid_probed_before_required || session.progress.associated {
        return false;
    }
    if cyw43_active_prompt_poll(contract).is_some() {
        return false;
    }
    session.bssid_probed_before_required = true;
    let poll = session.progress.polls as usize;
    session.assoc_probe_attempts = session.assoc_probe_attempts.saturating_add(1);
    let attempt = session.assoc_probe_attempts;
    let _ = cyw43_probe_host_eapol_assoc_state(
        contract,
        station_mac,
        session,
        poll,
        attempt,
        "cyw43-host-eapol-bssid-probe",
    );
    true
}

#[cfg(feature = "kernel")]
fn cyw43_probe_host_eapol_assoc_state(
    contract: DriverTaskContract,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
    poll: usize,
    attempt: u8,
    stage: &'static str,
) -> Cyw43AssocProbeResult {
    match cyw43_get_bcdc_bssid(contract, stage) {
        Ok(bssid) => {
            let accepted = cyw43_host_eapol_bssid_candidate(bssid, station_mac);
            session.progress.record_assoc_probe(
                if accepted {
                    "valid-bssid"
                } else {
                    "ignored-bssid"
                },
                0,
            );
            emit_cyw43_host_eapol_assoc_probe(
                contract,
                poll,
                attempt,
                if accepted { "observed" } else { "ignored" },
                bssid,
                if accepted {
                    "valid-bssid"
                } else {
                    "not-ap-candidate"
                },
                0,
            );
            emit_cyw43_host_eapol_bssid_refresh(
                contract,
                poll,
                if accepted { "observed" } else { "ignored" },
                bssid,
                if accepted {
                    "pre-required-valid-bssid"
                } else {
                    "pre-required-not-ap-candidate"
                },
                0,
            );
            if accepted {
                Cyw43AssocProbeResult::BssidObserved
            } else {
                Cyw43AssocProbeResult::Ignored
            }
        }
        Err(err) => {
            let status = if err.is_not_associated() {
                "not-associated"
            } else {
                "control-error"
            };
            session.progress.record_assoc_probe(status, err.result());
            emit_cyw43_host_eapol_bssid_refresh(
                contract,
                poll,
                "failed",
                EthernetAddress([0; ETHER_ADDR_LEN]),
                if err.is_not_associated() {
                    "pre-required-firmware-not-associated"
                } else {
                    "pre-required-control-error"
                },
                err.result(),
            );
            emit_cyw43_host_eapol_assoc_probe(
                contract,
                poll,
                attempt,
                "failed",
                EthernetAddress([0; ETHER_ADDR_LEN]),
                if err.is_not_associated() {
                    if session.progress.polls >= CYW43_HOST_EAPOL_ASSOC_RESCUE_POLL {
                        "firmware-not-associated-limit"
                    } else {
                        "firmware-not-associated-probe"
                    }
                } else {
                    "control-error"
                },
                err.result(),
            );
            if err.is_not_associated() {
                Cyw43AssocProbeResult::NotAssociated
            } else {
                Cyw43AssocProbeResult::ControlError
            }
        }
    }
}

#[cfg(feature = "kernel")]
fn cyw43_try_host_eapol_assoc_rescue(
    contract: DriverTaskContract,
    credentials: WifiCredentials,
    session: &mut Cyw43HostEapolSession,
    poll: usize,
    attempt: u8,
) {
    session.progress.record_assoc_join_rescue_attempt();
    match cyw43_submit_linux_bsscfg_join_with_stage(
        contract,
        credentials,
        "cyw43-host-eapol-join-rescue",
    ) {
        Ok(()) => {
            session.restart_pre_assoc_window_after_rescue(crate::hal::timebase().now_ms());
            emit_cyw43_host_eapol_assoc_rescue(
                contract,
                poll,
                attempt,
                "ready",
                "firmware-not-associated-limit",
                "bsscfg-join",
                0,
            );
        }
        Err(Cyw43CommandSubmitError::Completion(completion))
            if cyw43_join_iovar_completion_allows_set_ssid(completion) =>
        {
            session.progress.record_assoc_set_ssid_rescue_attempt();
            match cyw43_submit_bcdc_ssid(contract, credentials, "cyw43-host-eapol-set-ssid-rescue")
            {
                Ok(()) => {
                    session.restart_pre_assoc_window_after_rescue(crate::hal::timebase().now_ms());
                    emit_cyw43_host_eapol_assoc_rescue(
                        contract,
                        poll,
                        attempt,
                        "ready",
                        "bsscfg-join-unsupported-fallback",
                        "set-ssid-fallback",
                        0,
                    );
                }
                Err(_) => emit_cyw43_host_eapol_assoc_rescue(
                    contract,
                    poll,
                    attempt,
                    "failed",
                    "set-ssid-control-error",
                    "set-ssid-fallback",
                    0,
                ),
            }
        }
        Err(Cyw43CommandSubmitError::Completion(completion)) => emit_cyw43_host_eapol_assoc_rescue(
            contract,
            poll,
            attempt,
            "failed",
            "bsscfg-join-control-error",
            "bsscfg-join",
            completion.result,
        ),
        Err(Cyw43CommandSubmitError::Runtime(_)) => emit_cyw43_host_eapol_assoc_rescue(
            contract,
            poll,
            attempt,
            "failed",
            "bsscfg-join-runtime-error",
            "bsscfg-join",
            0,
        ),
    }
}

#[cfg(feature = "kernel")]
fn cyw43_service_host_eapol_post_assoc(
    contract: DriverTaskContract,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
    now_ms: u64,
) -> bool {
    if !session.progress.associated {
        return false;
    }
    session.record_time(now_ms);
    let post_assoc_elapsed_ms = session.post_assoc_elapsed_ms(now_ms);
    let mut activity = false;
    session.progress.post_assoc_polls = session.progress.post_assoc_polls.saturating_add(1);
    if cyw43_host_eapol_post_assoc_refresh_due_ms(
        &session.progress,
        post_assoc_elapsed_ms,
        session.refreshed_after_assoc,
    ) {
        let _ = cyw43_refresh_host_eapol_rx_after_assoc(contract);
        session.refreshed_after_assoc = true;
        activity = true;
        emit_cyw43_host_eapol_status(contract, "rx-admission-refresh", &session.progress);
    }
    if cyw43_host_eapol_post_assoc_rescue_due_ms(
        &session.progress,
        post_assoc_elapsed_ms,
        session.rescued_after_assoc,
    ) {
        let _ = cyw43_rescue_host_eapol_rx_after_assoc(contract);
        session.rescued_after_assoc = true;
        activity = true;
        emit_cyw43_host_eapol_status(contract, "rx-admission-rescue", &session.progress);
    }
    let start_sent = CYW43_HOST_EAPOL_START.load(Ordering::Acquire);
    if cyw43_host_eapol_start_due_ms(
        post_assoc_elapsed_ms,
        start_sent,
        session.last_eapol_start_ms,
        now_ms,
    ) {
        if cyw43_refresh_host_eapol_bssid_after_assoc(contract, station_mac, session) {
            activity = true;
        }
        cyw43_try_send_host_eapol_start(
            contract,
            station_mac,
            session.progress.post_assoc_polls as usize,
        );
        if CYW43_HOST_EAPOL_START.load(Ordering::Acquire) != start_sent {
            session.last_eapol_start_ms = Some(now_ms);
            activity = true;
        }
    }
    activity
}

#[cfg(feature = "kernel")]
fn cyw43_refresh_host_eapol_bssid_after_assoc(
    contract: DriverTaskContract,
    station_mac: EthernetAddress,
    session: &mut Cyw43HostEapolSession,
) -> bool {
    if session.bssid_refreshed_after_assoc {
        return false;
    }
    session.bssid_refreshed_after_assoc = true;
    let poll = session.progress.polls as usize;
    match cyw43_get_bcdc_bssid(contract, "cyw43-host-eapol-bssid-refresh") {
        Ok(bssid) => {
            let accepted = cyw43_host_eapol_bssid_candidate(bssid, station_mac);
            emit_cyw43_host_eapol_bssid_refresh(
                contract,
                poll,
                if accepted { "ready" } else { "ignored" },
                bssid,
                if accepted {
                    "valid-bssid"
                } else {
                    "not-ap-candidate"
                },
                0,
            );
        }
        Err(err) => {
            let reason = if err.is_not_associated() {
                "firmware-not-associated-yet"
            } else {
                "control-error"
            };
            emit_cyw43_host_eapol_bssid_refresh(
                contract,
                poll,
                "failed",
                EthernetAddress([0; ETHER_ADDR_LEN]),
                reason,
                err.result(),
            );
        }
    }
    true
}

#[cfg(feature = "kernel")]
fn mark_cyw43_host_eapol_secure(contract: DriverTaskContract, progress: &Cyw43HostEapolProgress) {
    if !progress.associated || !progress.link_up {
        mark_cyw43_host_eapol_required(contract, progress);
        return;
    }
    CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);
    CYW43_HOST_EAPOL_ACTIVE.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_REQUIRED.store(0, Ordering::Release);
    let data_rx_admitted =
        cyw43_post_secure_data_rx_admitted() || restore_cyw43_host_eapol_rx_after_secure(contract);
    CYW43_CONTROL_PLANE_READY.store(if data_rx_admitted { 1 } else { 0 }, Ordering::Release);
    let status = if data_rx_admitted {
        "secure"
    } else {
        "secure-rx-admission-blocked"
    };
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        "cyw43-host-eapol",
        status,
        None,
    );
    emit_cyw43_host_eapol_status(contract, status, progress);
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
    if let Some(start_completion) = submit_cyw43_host_eapol_payload_bounded_completion(
        contract,
        &frame[..len],
        "cyw43-host-eapol-start",
    ) {
        CYW43_HOST_EAPOL_START.fetch_add(1, Ordering::AcqRel);
        emit_cyw43_host_eapol_tx_shape(
            contract,
            "cyw43-host-eapol-start",
            poll,
            &frame[..len],
            Some(start_completion),
        );
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
#[cfg(test)]
const fn cyw43_host_eapol_start_due(poll: usize, sent: u32) -> bool {
    sent < CYW43_HOST_EAPOL_START_MAX
        && poll >= CYW43_HOST_EAPOL_START_FIRST_POLL
        && cyw43_host_eapol_start_poll_due(poll)
}

#[must_use]
#[cfg(test)]
const fn cyw43_host_eapol_start_poll_due(poll: usize) -> bool {
    poll % CYW43_HOST_EAPOL_START_INTERVAL_POLLS == 0
}

#[must_use]
const fn cyw43_host_eapol_start_due_ms(
    post_assoc_elapsed_ms: u64,
    sent: u32,
    last_start_ms: Option<u64>,
    now_ms: u64,
) -> bool {
    sent < CYW43_HOST_EAPOL_START_MAX
        && post_assoc_elapsed_ms >= CYW43_HOST_EAPOL_START_FIRST_MS
        && match last_start_ms {
            Some(last) => now_ms.saturating_sub(last) >= CYW43_HOST_EAPOL_START_INTERVAL_MS,
            None => true,
        }
}

#[cfg(feature = "kernel")]
const fn cyw43_host_eapol_rx_firstread_due(
    poll: usize,
    starts_sent: u32,
    associated: bool,
) -> bool {
    if !associated {
        return cyw43_host_eapol_pre_assoc_rx_firstread_due(poll);
    }
    if starts_sent == 0 {
        return cyw43_host_eapol_pre_assoc_rx_firstread_due(poll);
    }
    if poll <= CYW43_HOST_EAPOL_START_FIRST_POLL {
        return false;
    }
    let after_first_start = poll - CYW43_HOST_EAPOL_START_FIRST_POLL;
    matches!(after_first_start, 1 | 4 | 16 | 64 | 256 | 1024)
        || (poll > CYW43_HOST_EAPOL_START_INTERVAL_POLLS
            && (poll - 1) % CYW43_HOST_EAPOL_START_INTERVAL_POLLS == 0)
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_rx_firstread_due_from_progress(
    poll: usize,
    starts_sent: u32,
    progress: &Cyw43HostEapolProgress,
) -> bool {
    cyw43_host_eapol_rx_firstread_due(poll, starts_sent, progress.associated)
        || cyw43_host_eapol_source_asserted(progress)
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_rx_firstread_due_from_session(
    poll: usize,
    starts_sent: u32,
    session: &mut Cyw43HostEapolSession,
    now_ms: u64,
) -> bool {
    let timer_due = session.claim_timer_firstread_slot(starts_sent, now_ms);
    cyw43_host_eapol_rx_firstread_due_from_progress(poll, starts_sent, &session.progress)
        || timer_due
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_rx_firstread_timer_slot(elapsed_ms: u64, post_start: bool) -> Option<u16> {
    let thresholds = if post_start {
        &CYW43_HOST_EAPOL_POST_START_FIRSTREAD_MS[..]
    } else {
        &CYW43_HOST_EAPOL_PRE_ASSOC_FIRSTREAD_MS[..]
    };
    let mut slot = None;
    for (index, threshold_ms) in thresholds.iter().enumerate() {
        if elapsed_ms >= *threshold_ms {
            slot = Some(index as u16);
        }
    }
    if elapsed_ms >= CYW43_HOST_EAPOL_START_INTERVAL_MS {
        let interval_slot =
            (elapsed_ms / CYW43_HOST_EAPOL_START_INTERVAL_MS).min(u16::MAX as u64 - 0x0100);
        return Some(0x0100u16.saturating_add(interval_slot as u16));
    }
    slot
}

#[cfg(feature = "kernel")]
const fn cyw43_host_eapol_pre_assoc_rx_firstread_due(poll: usize) -> bool {
    matches!(poll, 0 | 1 | 4 | 16 | 64 | 256 | 1024 | 4096)
        || (poll != 0 && poll % CYW43_HOST_EAPOL_START_INTERVAL_POLLS == 0)
}

#[cfg(test)]
const fn cyw43_host_eapol_post_assoc_refresh_due(
    progress: &Cyw43HostEapolProgress,
    refreshed: bool,
) -> bool {
    progress.associated
        && !refreshed
        && progress.eapol_rx == 0
        && progress.post_assoc_polls >= CYW43_HOST_EAPOL_RX_REFRESH_AFTER_POST_ASSOC_POLLS
}

const fn cyw43_host_eapol_post_assoc_refresh_due_ms(
    progress: &Cyw43HostEapolProgress,
    post_assoc_elapsed_ms: u64,
    refreshed: bool,
) -> bool {
    progress.associated
        && !refreshed
        && progress.eapol_rx == 0
        && post_assoc_elapsed_ms >= CYW43_HOST_EAPOL_RX_REFRESH_AFTER_POST_ASSOC_MS
}

#[cfg(feature = "kernel")]
#[cfg(test)]
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
fn cyw43_host_eapol_post_assoc_rescue_due_ms(
    progress: &Cyw43HostEapolProgress,
    post_assoc_elapsed_ms: u64,
    rescued: bool,
) -> bool {
    progress.associated
        && !rescued
        && progress.eapol_rx == 0
        && (post_assoc_elapsed_ms >= CYW43_HOST_EAPOL_RX_RESCUE_AFTER_POST_ASSOC_MS
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
const fn cyw43_frame_channel_label(flags: u16) -> &'static str {
    match cyw43_frame_channel(flags) {
        DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA => "data",
        DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL => "control",
        DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT => "event",
        _ => "unknown",
    }
}

#[cfg(feature = "kernel")]
fn cyw43_trace_channel_label(
    completion_code: u16,
    completion_flags: u16,
    frame_flags: u16,
) -> &'static str {
    if completion_code == DriverTaskCompletionCode::FrameReady.as_u16() || frame_flags != 0 {
        cyw43_frame_channel_label(completion_flags)
    } else {
        "none"
    }
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
const fn cyw43_host_eapol_join_event_type(event_type: u8) -> bool {
    matches!(
        event_type,
        CYW43_EVENT_SET_SSID
            | CYW43_EVENT_AUTH
            | CYW43_EVENT_DEAUTH
            | CYW43_EVENT_ASSOC
            | CYW43_EVENT_ASSOC_IND
            | CYW43_EVENT_REASSOC
            | CYW43_EVENT_REASSOC_IND
            | CYW43_EVENT_DISASSOC
            | CYW43_EVENT_DISASSOC_IND
            | CYW43_EVENT_LINK
            | CYW43_EVENT_ROAM
            | CYW43_EVENT_MIC_ERROR
            | CYW43_EVENT_PSK_SUP
    )
}

#[cfg(feature = "kernel")]
const fn cyw43_host_eapol_association_event_label(event: Cyw43EventFrame) -> Option<&'static str> {
    match event.event_type {
        CYW43_EVENT_SET_SSID if event.status == CYW43_EVENT_STATUS_SUCCESS => Some("set-ssid"),
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
const fn cyw43_host_eapol_event_trace_label(event: Cyw43EventFrame) -> &'static str {
    match cyw43_host_eapol_association_event_label(event) {
        Some(label) => label,
        None => match event.event_type {
            CYW43_EVENT_SET_SSID => "set-ssid-failed",
            CYW43_EVENT_AUTH if event.status == CYW43_EVENT_STATUS_TIMEOUT => "auth-timeout",
            CYW43_EVENT_AUTH => "auth",
            CYW43_EVENT_DEAUTH => "deauth",
            CYW43_EVENT_DISASSOC | CYW43_EVENT_DISASSOC_IND => "disassoc",
            CYW43_EVENT_ASSOC_IND | CYW43_EVENT_REASSOC_IND => "assoc-ind",
            CYW43_EVENT_ROAM => "roam",
            CYW43_EVENT_MIC_ERROR => "mic-error",
            CYW43_EVENT_PSK_SUP => "psk-sup",
            CYW43_EVENT_IF => "if",
            _ => "none",
        },
    }
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
        CYW43_EVENT_LINK | CYW43_EVENT_DEAUTH | CYW43_EVENT_DISASSOC => Some(false),
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
        CYW43_EVENT_LINK | CYW43_EVENT_DEAUTH | CYW43_EVENT_DISASSOC | CYW43_EVENT_DISASSOC_IND => {
            Some(false)
        }
        _ => None,
    }
}

#[cfg(feature = "kernel")]
fn cyw43_parse_control_or_event_frame(frame: &[u8]) -> Option<Cyw43EventFrame> {
    cyw43_bdc_payload(frame)
        .and_then(cyw43_parse_broadcom_event)
        .or_else(|| cyw43_parse_broadcom_event(frame))
}

#[cfg(feature = "kernel")]
fn cyw43_parse_data_event_frame(frame: &[u8]) -> Option<Cyw43EventFrame> {
    cyw43_parse_broadcom_event(frame)
        .or_else(|| cyw43_bdc_payload(frame).and_then(cyw43_parse_broadcom_event))
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43DataPathInfo {
    dst: [u8; ETHER_ADDR_LEN],
    src: [u8; ETHER_ADDR_LEN],
    ethertype: u16,
    ip_proto: u8,
    udp_src: u16,
    udp_dst: u16,
    dhcp: &'static str,
    arp: &'static str,
    arp_spa: [u8; 4],
    arp_tpa: [u8; 4],
}

#[cfg(feature = "kernel")]
fn cyw43_data_path_info(frame: &[u8]) -> Option<Cyw43DataPathInfo> {
    let ethertype = cyw43_ethertype(frame)?;
    let (dst, src) = cyw43_ethernet_addrs(frame)?;
    match ethertype {
        CYW43_ETH_P_ARP => Some(cyw43_arp_data_path_info(frame, dst, src)),
        CYW43_ETH_P_IPV4 => cyw43_ipv4_dhcp_data_path_info(frame, dst, src),
        _ => None,
    }
}

#[cfg(feature = "kernel")]
fn cyw43_trace_frame_info(frame: &[u8]) -> Cyw43DataPathInfo {
    cyw43_data_path_info(frame).unwrap_or_else(|| {
        let ethertype = cyw43_ethertype(frame).unwrap_or(0);
        let (dst, src) =
            cyw43_ethernet_addrs(frame).unwrap_or(([0; ETHER_ADDR_LEN], [0; ETHER_ADDR_LEN]));
        let ip_proto = if ethertype == CYW43_ETH_P_IPV4 {
            frame.get(ETH_HEADER_LEN + 9).copied().unwrap_or(0)
        } else {
            0
        };
        Cyw43DataPathInfo {
            dst,
            src,
            ethertype,
            ip_proto,
            udp_src: 0,
            udp_dst: 0,
            dhcp: "none",
            arp: "none",
            arp_spa: [0; 4],
            arp_tpa: [0; 4],
        }
    })
}

#[cfg(feature = "kernel")]
fn cyw43_ethernet_addrs(frame: &[u8]) -> Option<([u8; ETHER_ADDR_LEN], [u8; ETHER_ADDR_LEN])> {
    let mut dst = [0u8; ETHER_ADDR_LEN];
    let mut src = [0u8; ETHER_ADDR_LEN];
    dst.copy_from_slice(frame.get(..ETHER_ADDR_LEN)?);
    src.copy_from_slice(frame.get(ETHER_ADDR_LEN..ETHER_ADDR_LEN * 2)?);
    Some((dst, src))
}

#[cfg(feature = "kernel")]
fn cyw43_arp_data_path_info(
    frame: &[u8],
    dst: [u8; ETHER_ADDR_LEN],
    src: [u8; ETHER_ADDR_LEN],
) -> Cyw43DataPathInfo {
    let (_sha, spa, _tha, tpa) = cyw43_arp_addrs(frame).unwrap_or_default();
    Cyw43DataPathInfo {
        dst,
        src,
        ethertype: CYW43_ETH_P_ARP,
        ip_proto: 0,
        udp_src: 0,
        udp_dst: 0,
        dhcp: "none",
        arp: cyw43_arp_operation_label(frame),
        arp_spa: spa,
        arp_tpa: tpa,
    }
}

#[cfg(feature = "kernel")]
fn cyw43_arp_operation_label(frame: &[u8]) -> &'static str {
    let Some(op) = cyw43_get_u16_be(frame, ETH_HEADER_LEN + 6) else {
        return "malformed";
    };
    match op {
        1 => "request",
        2 => "reply",
        _ => "other",
    }
}

#[cfg(feature = "kernel")]
fn cyw43_arp_addrs(frame: &[u8]) -> Option<([u8; 6], [u8; 4], [u8; 6], [u8; 4])> {
    if cyw43_get_u16_be(frame, ETH_HEADER_LEN)? != 1
        || cyw43_get_u16_be(frame, ETH_HEADER_LEN + 2)? != CYW43_ETH_P_IPV4
        || frame.get(ETH_HEADER_LEN + 4).copied()? != ETHER_ADDR_LEN as u8
        || frame.get(ETH_HEADER_LEN + 5).copied()? != 4
    {
        return None;
    }
    let mut sender_hw = [0u8; 6];
    sender_hw.copy_from_slice(frame.get(ETH_HEADER_LEN + 8..ETH_HEADER_LEN + 14)?);
    let mut sender_ip = [0u8; 4];
    sender_ip.copy_from_slice(frame.get(ETH_HEADER_LEN + 14..ETH_HEADER_LEN + 18)?);
    let mut target_hw = [0u8; 6];
    target_hw.copy_from_slice(frame.get(ETH_HEADER_LEN + 18..ETH_HEADER_LEN + 24)?);
    let mut target_ip = [0u8; 4];
    target_ip.copy_from_slice(frame.get(ETH_HEADER_LEN + 24..ETH_HEADER_LEN + 28)?);
    Some((sender_hw, sender_ip, target_hw, target_ip))
}

#[cfg(feature = "kernel")]
fn cyw43_assigned_ipv4() -> Option<[u8; 4]> {
    let raw = CYW43_ASSIGNED_IPV4_BE.load(Ordering::Acquire);
    if raw == 0 {
        None
    } else {
        Some(raw.to_be_bytes())
    }
}

#[cfg(feature = "kernel")]
fn cyw43_post_dhcp_zero_sender_arp(frame: &[u8]) -> bool {
    let Some(assigned_ip) = cyw43_assigned_ipv4() else {
        return false;
    };
    let Some(info) = cyw43_data_path_info(frame) else {
        return false;
    };
    info.ethertype == CYW43_ETH_P_ARP
        && info.arp == "request"
        && info.arp_spa == [0; 4]
        && info.arp_tpa != [0; 4]
        && info.arp_tpa != assigned_ip
}

#[cfg(feature = "kernel")]
fn cyw43_mac_is_unicast(mac: [u8; 6]) -> bool {
    mac != [0; 6] && mac != [0xff; 6] && mac[0] & 0x01 == 0
}

#[cfg(feature = "kernel")]
fn cyw43_arp_frame(
    eth_dst: [u8; ETHER_ADDR_LEN],
    eth_src: [u8; ETHER_ADDR_LEN],
    op: u16,
    sender_hw: [u8; ETHER_ADDR_LEN],
    sender_ip: [u8; 4],
    target_hw: [u8; ETHER_ADDR_LEN],
    target_ip: [u8; 4],
) -> [u8; 42] {
    let mut frame = [0u8; 42];
    frame[..ETHER_ADDR_LEN].copy_from_slice(&eth_dst);
    frame[ETHER_ADDR_LEN..ETHER_ADDR_LEN * 2].copy_from_slice(&eth_src);
    frame[12..14].copy_from_slice(&CYW43_ETH_P_ARP.to_be_bytes());
    frame[ETH_HEADER_LEN..ETH_HEADER_LEN + 2].copy_from_slice(&1u16.to_be_bytes());
    frame[ETH_HEADER_LEN + 2..ETH_HEADER_LEN + 4].copy_from_slice(&CYW43_ETH_P_IPV4.to_be_bytes());
    frame[ETH_HEADER_LEN + 4] = ETHER_ADDR_LEN as u8;
    frame[ETH_HEADER_LEN + 5] = 4;
    frame[ETH_HEADER_LEN + 6..ETH_HEADER_LEN + 8].copy_from_slice(&op.to_be_bytes());
    frame[ETH_HEADER_LEN + 8..ETH_HEADER_LEN + 14].copy_from_slice(&sender_hw);
    frame[ETH_HEADER_LEN + 14..ETH_HEADER_LEN + 18].copy_from_slice(&sender_ip);
    frame[ETH_HEADER_LEN + 18..ETH_HEADER_LEN + 24].copy_from_slice(&target_hw);
    frame[ETH_HEADER_LEN + 24..ETH_HEADER_LEN + 28].copy_from_slice(&target_ip);
    frame
}

#[cfg(feature = "kernel")]
fn submit_cyw43_gratuitous_arp_announcement(
    contract: DriverTaskContract,
    assigned_ip: [u8; 4],
) -> bool {
    submit_driver_task_gratuitous_arp_announcement(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        assigned_ip,
    )
}

#[cfg(feature = "kernel")]
fn submit_driver_task_gratuitous_arp_announcement(
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
    assigned_ip: [u8; 4],
) -> bool {
    let expected_contract = match hot_path {
        DriverTaskHotPath::GenetNic => GENET_DRIVER_TASK_CONTRACT,
        DriverTaskHotPath::Cyw43Wifi => CYW43_WIFI_DRIVER_TASK_CONTRACT,
        _ => return false,
    };
    if contract != expected_contract || assigned_ip == [0; 4] {
        return false;
    }
    let fallback_mac = match hot_path {
        DriverTaskHotPath::GenetNic => GENET_DRIVER_TASK_MAC,
        DriverTaskHotPath::Cyw43Wifi => CYW43_DRIVER_TASK_MAC,
        _ => return false,
    };
    let station_mac = runtime_mac(hot_path).unwrap_or(fallback_mac).0;
    let request = cyw43_arp_frame(
        [0xff; ETHER_ADDR_LEN],
        station_mac,
        1,
        station_mac,
        assigned_ip,
        [0; ETHER_ADDR_LEN],
        assigned_ip,
    );
    let reply = cyw43_arp_frame(
        [0xff; ETHER_ADDR_LEN],
        station_mac,
        2,
        station_mac,
        assigned_ip,
        [0xff; ETHER_ADDR_LEN],
        assigned_ip,
    );
    let request_submitted = submit_driver_task_frame(contract, hot_path, &request);
    if request_submitted {
        record_driver_task_arp_tx(hot_path, &request);
    }
    let reply_submitted = submit_driver_task_frame(contract, hot_path, &reply);
    if reply_submitted {
        record_driver_task_arp_tx(hot_path, &reply);
    }
    request_submitted | reply_submitted
}

#[cfg(feature = "kernel")]
fn submit_cyw43_arp_assist_if_needed(contract: DriverTaskContract, frame: &[u8]) -> bool {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT
        || cyw43_ethertype(frame) != Some(CYW43_ETH_P_ARP)
        || cyw43_arp_operation_label(frame) != "request"
        || frame.get(..ETHER_ADDR_LEN) != Some(&[0xff; ETHER_ADDR_LEN])
    {
        return false;
    }
    let Some(assigned_ip) = cyw43_assigned_ipv4() else {
        return false;
    };
    let Some((sender_hw, sender_ip, _target_hw, target_ip)) = cyw43_arp_addrs(frame) else {
        return false;
    };
    if target_ip != assigned_ip || !cyw43_mac_is_unicast(sender_hw) || sender_ip == [0; 4] {
        return false;
    }
    let station_mac = runtime_mac(DriverTaskHotPath::Cyw43Wifi)
        .unwrap_or(CYW43_DRIVER_TASK_MAC)
        .0;
    let unicast_reply = cyw43_arp_frame(
        sender_hw,
        station_mac,
        2,
        station_mac,
        assigned_ip,
        sender_hw,
        sender_ip,
    );
    let broadcast_reply = cyw43_arp_frame(
        [0xff; ETHER_ADDR_LEN],
        station_mac,
        2,
        station_mac,
        assigned_ip,
        sender_hw,
        sender_ip,
    );
    submit_cyw43_driver_task_eth_payload(&unicast_reply)
        | submit_cyw43_driver_task_eth_payload(&broadcast_reply)
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cyw43DataPathTraceClass {
    Suppress,
    Fault,
    Drop,
    TxRetry,
    Dhcp,
    EapolConsume,
    PendingTransition,
    MacMismatch,
}

#[cfg(feature = "kernel")]
fn cyw43_data_path_info_is_routine_ip(info: Cyw43DataPathInfo) -> bool {
    info.ethertype == CYW43_ETH_P_IPV4
        && info.dhcp == "none"
        && (info.ip_proto == CYW43_IP_PROTO_TCP || info.ip_proto == CYW43_IP_PROTO_UDP)
}

#[cfg(feature = "kernel")]
fn cyw43_data_path_trace_class(
    event: &'static str,
    action: &'static str,
    info: Cyw43DataPathInfo,
    completion_code: u16,
    assigned_ipv4: Option<[u8; 4]>,
    runtime_station_mac: EthernetAddress,
    pending_before: bool,
    pending_after: bool,
) -> Cyw43DataPathTraceClass {
    if matches!(event, "rx-preserve-drop" | "rx-channel-drop" | "tx-drop")
        || action == "invalid-arp-spa"
    {
        return Cyw43DataPathTraceClass::Drop;
    }
    if matches!(action, "retry" | "credit-unproven") || action.starts_with("no-completion") {
        return Cyw43DataPathTraceClass::TxRetry;
    }
    if completion_code == DriverTaskCompletionCode::Fault.as_u16()
        || completion_code == DriverTaskCompletionCode::BudgetExhausted.as_u16()
    {
        return Cyw43DataPathTraceClass::Fault;
    }
    let wifi_bound = assigned_ipv4.is_some();
    if info.dhcp != "none" {
        if wifi_bound && info.src != runtime_station_mac.0 && info.dst != runtime_station_mac.0 {
            return Cyw43DataPathTraceClass::Suppress;
        }
        return Cyw43DataPathTraceClass::Dhcp;
    }
    if info.arp == "request"
        && matches!(assigned_ipv4, Some(ip) if info.arp_tpa != [0; 4] && info.arp_tpa == ip)
    {
        return Cyw43DataPathTraceClass::Suppress;
    }
    if wifi_bound
        && info.ethertype == CYW43_ETH_P_ARP
        && info.src != runtime_station_mac.0
        && !matches!(assigned_ipv4, Some(ip) if info.arp_tpa != [0; 4] && info.arp_tpa == ip)
    {
        return Cyw43DataPathTraceClass::Suppress;
    }
    if event == "tx-result"
        && matches!(action, "submitted" | "credit-proven")
        && info.arp == "reply"
    {
        return Cyw43DataPathTraceClass::PendingTransition;
    }
    if event == "rx-consume" && info.ethertype == ETH_P_EAPOL {
        return Cyw43DataPathTraceClass::EapolConsume;
    }
    if (matches!(action, "pending" | "resume") || (pending_before != pending_after))
        && !cyw43_data_path_info_is_routine_ip(info)
    {
        return Cyw43DataPathTraceClass::PendingTransition;
    }
    if event == "tx-result"
        && matches!(action, "submitted" | "credit-proven")
        && info.src != runtime_station_mac.0
    {
        return Cyw43DataPathTraceClass::MacMismatch;
    }
    Cyw43DataPathTraceClass::Suppress
}

#[cfg(feature = "kernel")]
fn cyw43_data_path_trace_repeat_milestone(count: u32) -> bool {
    count <= 4 || count.is_power_of_two()
}

#[cfg(all(feature = "kernel", test))]
const fn cyw43_data_path_trace_class_uses_milestone_gate(
    trace_class: Cyw43DataPathTraceClass,
) -> bool {
    matches!(
        trace_class,
        Cyw43DataPathTraceClass::Dhcp
            | Cyw43DataPathTraceClass::Drop
            | Cyw43DataPathTraceClass::EapolConsume
            | Cyw43DataPathTraceClass::Fault
            | Cyw43DataPathTraceClass::PendingTransition
            | Cyw43DataPathTraceClass::TxRetry
    )
}

#[cfg(feature = "kernel")]
fn cyw43_data_path_trace_repeat_allowed(trace_class: Cyw43DataPathTraceClass) -> bool {
    match trace_class {
        Cyw43DataPathTraceClass::Suppress => false,
        Cyw43DataPathTraceClass::Dhcp => {
            let count = CYW43_DATA_TRACE_DHCP_COUNT
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1);
            cyw43_data_path_trace_repeat_milestone(count)
        }
        Cyw43DataPathTraceClass::Drop => {
            let count = CYW43_DATA_TRACE_DROP_COUNT
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1);
            cyw43_data_path_trace_repeat_milestone(count)
        }
        Cyw43DataPathTraceClass::EapolConsume => {
            let count = CYW43_DATA_TRACE_EAPOL_CONSUME_COUNT
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1);
            cyw43_data_path_trace_repeat_milestone(count)
        }
        Cyw43DataPathTraceClass::Fault => {
            let count = CYW43_DATA_TRACE_FAULT_COUNT
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1);
            cyw43_data_path_trace_repeat_milestone(count)
        }
        Cyw43DataPathTraceClass::PendingTransition => {
            let count = CYW43_DATA_TRACE_PENDING_COUNT
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1);
            cyw43_data_path_trace_repeat_milestone(count)
        }
        Cyw43DataPathTraceClass::TxRetry => {
            let count = CYW43_DATA_TRACE_TX_RETRY_COUNT
                .fetch_add(1, Ordering::AcqRel)
                .saturating_add(1);
            cyw43_data_path_trace_repeat_milestone(count)
        }
        Cyw43DataPathTraceClass::MacMismatch => true,
    }
}

#[cfg(feature = "kernel")]
fn cyw43_ipv4_dhcp_data_path_info(
    frame: &[u8],
    dst: [u8; ETHER_ADDR_LEN],
    src: [u8; ETHER_ADDR_LEN],
) -> Option<Cyw43DataPathInfo> {
    let version_ihl = *frame.get(ETH_HEADER_LEN)?;
    if version_ihl >> 4 != 4 {
        return None;
    }
    let ip_header_len = usize::from(version_ihl & 0x0f) * 4;
    if ip_header_len < 20 {
        return None;
    }
    let ip_proto = *frame.get(ETH_HEADER_LEN + 9)?;
    if ip_proto != CYW43_IP_PROTO_UDP {
        return None;
    }
    let udp_offset = ETH_HEADER_LEN.checked_add(ip_header_len)?;
    let udp_src = cyw43_get_u16_be(frame, udp_offset)?;
    let udp_dst = cyw43_get_u16_be(frame, udp_offset + 2)?;
    if !cyw43_udp_ports_are_dhcp(udp_src, udp_dst) {
        return None;
    }
    let dhcp_offset = udp_offset.checked_add(8)?;
    let dhcp_payload = frame.get(dhcp_offset..).unwrap_or(&[]);
    Some(Cyw43DataPathInfo {
        dst,
        src,
        ethertype: CYW43_ETH_P_IPV4,
        ip_proto,
        udp_src,
        udp_dst,
        dhcp: cyw43_dhcp_message_label(dhcp_payload),
        arp: "none",
        arp_spa: [0; 4],
        arp_tpa: [0; 4],
    })
}

#[cfg(feature = "kernel")]
const fn cyw43_udp_ports_are_dhcp(src: u16, dst: u16) -> bool {
    (src == CYW43_DHCP_CLIENT_PORT && dst == CYW43_DHCP_SERVER_PORT)
        || (src == CYW43_DHCP_SERVER_PORT && dst == CYW43_DHCP_CLIENT_PORT)
}

#[cfg(feature = "kernel")]
fn cyw43_dhcp_message_label(payload: &[u8]) -> &'static str {
    let options_offset = CYW43_DHCP_FIXED_BYTES + CYW43_DHCP_MAGIC_COOKIE.len();
    if payload.len() < options_offset {
        return "malformed";
    }
    if payload.get(CYW43_DHCP_FIXED_BYTES..options_offset)
        != Some(CYW43_DHCP_MAGIC_COOKIE.as_slice())
    {
        return "malformed";
    }

    let mut cursor = options_offset;
    while cursor < payload.len() {
        let option = payload[cursor];
        cursor += 1;
        match option {
            0 => continue,
            255 => return "unknown",
            53 => {
                let Some(len) = payload.get(cursor).copied().map(usize::from) else {
                    return "malformed";
                };
                cursor += 1;
                if len == 0 {
                    return "malformed";
                }
                return match payload.get(cursor).copied() {
                    Some(1) => "discover",
                    Some(2) => "offer",
                    Some(3) => "request",
                    Some(5) => "ack",
                    Some(6) => "nak",
                    Some(_) => "other",
                    None => "malformed",
                };
            }
            _ => {
                let Some(len) = payload.get(cursor).copied().map(usize::from) else {
                    return "malformed";
                };
                cursor += 1;
                let Some(next) = cursor.checked_add(len) else {
                    return "malformed";
                };
                if next > payload.len() {
                    return "malformed";
                }
                cursor = next;
            }
        }
    }
    "unknown"
}

#[cfg(feature = "kernel")]
fn cyw43_active_descriptor_label(contract: DriverTaskContract) -> &'static str {
    cyw43_descriptor_label_for_op(
        cyw43_active_runtime_descriptor(contract).map(|(_, descriptor)| descriptor.op),
    )
}

#[cfg(feature = "kernel")]
const fn cyw43_descriptor_label_for_op(op: Option<u16>) -> &'static str {
    match op {
        Some(DRIVER_RUNTIME_CYW43_OP_ETH_TX) => "eth-tx",
        Some(DRIVER_RUNTIME_CYW43_OP_RX_POLL) => "rx-poll",
        Some(DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL) => "control-poll",
        Some(DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME) => "control-frame",
        Some(DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE) => "control-exchange",
        Some(DRIVER_RUNTIME_CYW43_OP_RELEASE) => "release",
        Some(DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT) => "transport-init",
        Some(DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP) => "firmware-prep",
        Some(DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK) => "firmware-chunk",
        Some(DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK) => "nvram-chunk",
        Some(DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL) => "nvram-tail",
        Some(_) => "other",
        None => "none",
    }
}

#[cfg(feature = "kernel")]
fn cyw43_tx_no_completion_action(contract: DriverTaskContract) -> &'static str {
    match cyw43_active_descriptor_label(contract) {
        "eth-tx" => "no-completion-active-tx",
        "rx-poll" => "no-completion-active-rx",
        "control-poll" => "no-completion-active-control",
        "control-frame" => "no-completion-active-control-frame",
        "control-exchange" => "no-completion-active-control-exchange",
        "none" => "no-completion",
        _ => "no-completion-active-other",
    }
}

#[cfg(feature = "kernel")]
fn cyw43_active_descriptor_op(contract: DriverTaskContract) -> u16 {
    cyw43_active_runtime_descriptor(contract)
        .map(|(_, descriptor)| descriptor.op)
        .unwrap_or_default()
}

#[cfg(feature = "kernel")]
fn emit_cyw43_data_path_trace(
    contract: DriverTaskContract,
    event: &'static str,
    action: &'static str,
    attempt: usize,
    frame: &[u8],
    completion: Option<DriverTaskCompletionRecord>,
    frame_flags: u16,
    pending_before: bool,
    pending_after: bool,
) {
    use core::fmt::Write;

    let info = cyw43_trace_frame_info(frame);
    let tx_total_len = cyw43_data_tx_total_len(frame.len()).unwrap_or(0);
    let tx_request_len = if tx_total_len == 0 {
        0
    } else {
        cyw43_data_tx_request_len_for_frame(frame, tx_total_len).0
    };
    let data_block_mode = if tx_total_len == 0 {
        false
    } else {
        cyw43_data_tx_request_len_for_frame(frame, tx_total_len).1
    };
    let (block_size, block_count) =
        cyw43_function2_data_tx_cmd53_shape(tx_request_len, data_block_mode);
    let cmd53_mode = if tx_request_len == 0 {
        "none"
    } else if block_count == 0 {
        "byte"
    } else {
        "block"
    };
    let completion_code = completion.map_or(0, |completion| completion.code);
    let completion_detail = completion.map_or(0, |completion| completion.detail);
    let completion_result = completion.map_or(0, |completion| completion.result);
    let completion_flags = completion.map_or(frame_flags, |completion| completion.frame.flags);
    let completion_len = completion.map_or(0, |completion| completion.frame.len);
    let runtime_station_mac =
        runtime_mac(DriverTaskHotPath::Cyw43Wifi).unwrap_or(CYW43_DRIVER_TASK_MAC);
    let active_descriptor = cyw43_active_descriptor_label(contract);
    let assigned_ipv4 = cyw43_assigned_ipv4();
    let trace_class = cyw43_data_path_trace_class(
        event,
        action,
        info,
        completion_code,
        assigned_ipv4,
        runtime_station_mac,
        pending_before,
        pending_after,
    );
    if !cyw43_data_path_trace_repeat_allowed(trace_class) {
        return;
    }
    let channel = cyw43_trace_channel_label(completion_code, completion_flags, frame_flags);
    let src_runtime_match = yes_no(runtime_station_mac.0 == info.src);
    let assigned_ipv4 = assigned_ipv4.unwrap_or([0; 4]);
    let arp_tpa_assigned = yes_no(info.arp_tpa != [0; 4] && info.arp_tpa == assigned_ipv4);
    let bdc_priority = cyw43_bdc_priority_for_ethertype(info.ethertype);
    let mut line = heapless::String::<1280>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_DATA_PATH contract={} event={} action={} attempt={} len={} channel={} active_descriptor={} active_op=0x{:04x} ethertype=0x{:04x} ip_proto={} udp_src={} udp_dst={} dhcp={} arp={} arp_spa={}.{}.{}.{} arp_tpa={}.{}.{}.{} arp_tpa_assigned={} dst={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} src={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} runtime_mac={} src_runtime_match={} bdc_priority={} tx_total_len={} tx_request_len={} cmd53_mode={} block_size={} block_count={} completion_code={} completion_detail=0x{:04x} completion_result=0x{:08x} completion_flags=0x{:04x} completion_len={} pending_before={} pending_after={}",
        contract.name,
        event,
        action,
        attempt,
        frame.len(),
        channel,
        active_descriptor,
        cyw43_active_descriptor_op(contract),
        info.ethertype,
        info.ip_proto,
        info.udp_src,
        info.udp_dst,
        info.dhcp,
        info.arp,
        info.arp_spa[0],
        info.arp_spa[1],
        info.arp_spa[2],
        info.arp_spa[3],
        info.arp_tpa[0],
        info.arp_tpa[1],
        info.arp_tpa[2],
        info.arp_tpa[3],
        arp_tpa_assigned,
        info.dst[0],
        info.dst[1],
        info.dst[2],
        info.dst[3],
        info.dst[4],
        info.dst[5],
        info.src[0],
        info.src[1],
        info.src[2],
        info.src[3],
        info.src[4],
        info.src[5],
        runtime_station_mac,
        src_runtime_match,
        bdc_priority,
        tx_total_len,
        tx_request_len,
        cmd53_mode,
        block_size,
        block_count,
        completion_code,
        completion_detail,
        completion_result,
        completion_flags,
        completion_len,
        yes_no(pending_before),
        yes_no(pending_after),
    );
    crate::bootstrap::log::force_uart_line_raw_without_prompt_refresh(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_status(
    contract: DriverTaskContract,
    status: &'static str,
    progress: &Cyw43HostEapolProgress,
) {
    use core::fmt::Write;

    let reason = if status == "required" {
        cyw43_host_eapol_required_reason(progress)
    } else {
        "none"
    };
    let next_action = cyw43_host_eapol_next_action(status, progress);
    let starts = CYW43_HOST_EAPOL_START.load(Ordering::Acquire);
    let tx_retries = CYW43_HOST_EAPOL_TX_RETRIES.load(Ordering::Acquire);
    let status_key =
        cyw43_host_eapol_status_key(status, reason, next_action, starts, tx_retries, progress);
    let log_decision = {
        let mut throttle = CYW43_HOST_EAPOL_STATUS_THROTTLE.lock();
        cyw43_host_eapol_status_log_decision(&mut throttle, status_key)
    };
    if !log_decision.emit_full {
        return;
    }
    let assoc_event = progress.association_event.unwrap_or("none");
    let rx_source_mode = cyw43_rx_source_mode(progress.last_rx_source);
    let control_rx_source_mode = cyw43_rx_source_mode(progress.last_control_rx_source);
    let rx_source = progress.last_rx_source.unwrap_or_default();
    let control_rx_source = progress.last_control_rx_source.unwrap_or_default();
    let rx_trace = progress.last_rx_trace;
    let control_rx_trace = progress.last_control_rx_trace;
    let firstread_class = cyw43_host_eapol_firstread_class(progress);
    let assoc_probe = progress.assoc_probe_status.unwrap_or("none");
    let mut line = heapless::String::<4096>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_STATUS contract={} status={} reason={} polls={} starts={} tx_retries={} suppressed_status={} data_rx={} eapol_rx={} non_eapol_rx={} event_rx={} control_rx={} empty_polls={} associated={} link_up={} assoc_event={} assoc_poll={} post_assoc_polls={} assoc_probe={} assoc_probe_result=0x{:08x} assoc_join_rescue={} assoc_set_ssid_rescue={} firstread_class={} rx_probe_poll={} rx_probe_flags=0x{:04x} control_rx_probe_poll={} control_rx_probe_flags=0x{:04x} rx_firstread_attempts={} rx_firstread_empty={} rx_firstread_invalid={} rx_firstread_failed={} rx_firstread_remainder_failed={} rx_firstread_decode_miss={} control_rx_firstread_attempts={} control_rx_firstread_empty={} control_rx_firstread_failed={} last_rx_idle_detail=0x{:04x} last_rx_idle_result=0x{:08x} last_control_rx_idle_detail=0x{:04x} last_control_rx_idle_result=0x{:08x} rxsrc_mode={} rxsrc_probe_len={} rxsrc_ien=0x{:02x} rxsrc_frame_ind={} rxsrc_host_int={} rxsrc_card_int={} rxsrc_f2_ready={} control_rxsrc_mode={} control_rxsrc_probe_len={} control_rxsrc_ien=0x{:02x} control_rxsrc_frame_ind={} control_rxsrc_host_int={} control_rxsrc_card_int={} control_rxsrc_f2_ready={} rxtrace_valid={} rxtrace_flags=0x{:04x} rxtrace_detail=0x{:04x} rxtrace_probe_len={} rxtrace_source=0x{:08x} rxtrace_prefix=0x{:08x} rxtrace_digest=0x{:08x} rxtrace_rframe=0x{:04x} rxtrace_firstread_reads={} rxtrace_block_reads={} rxtrace_rframe_reads={} rxtrace_request_len={} rxtrace_block_size={} rxtrace_block_count={} rxtrace_retx_sample=0x{:04x} rxtrace_retx_action={} rxtrace_queue_depth={} rxtrace_queue_high_water={} rxtrace_cmd53_arg=0x{:08x} rxtrace_cmd53_fn={} rxtrace_cmd53_addr=0x{:05x} rxtrace_cmd53_write={} rxtrace_cmd53_mode={} rxtrace_cmd53_inc={} rxtrace_cmd53_count={} rxtrace_transfer_result=0x{:08x} rxtrace_payload_before=0x{:08x} rxtrace_payload_after=0x{:08x} control_rxtrace_valid={} control_rxtrace_flags=0x{:04x} control_rxtrace_detail=0x{:04x} control_rxtrace_probe_len={} control_rxtrace_source=0x{:08x} control_rxtrace_prefix=0x{:08x} control_rxtrace_digest=0x{:08x} control_rxtrace_rframe=0x{:04x} control_rxtrace_firstread_reads={} control_rxtrace_block_reads={} control_rxtrace_rframe_reads={} control_rxtrace_request_len={} control_rxtrace_block_size={} control_rxtrace_block_count={} control_rxtrace_retx_sample=0x{:04x} control_rxtrace_retx_action={} control_rxtrace_queue_depth={} control_rxtrace_queue_high_water={} control_rxtrace_cmd53_arg=0x{:08x} control_rxtrace_cmd53_fn={} control_rxtrace_cmd53_addr=0x{:05x} control_rxtrace_cmd53_write={} control_rxtrace_cmd53_mode={} control_rxtrace_cmd53_inc={} control_rxtrace_cmd53_count={} control_rxtrace_transfer_result=0x{:08x} control_rxtrace_payload_before=0x{:08x} control_rxtrace_payload_after=0x{:08x} last_flags=0x{:04x} last_len={} last_ethertype=0x{:04x} last_ethertype_valid={} next_action={}",
        contract.name,
        status,
        reason,
        progress.polls,
        starts,
        tx_retries,
        log_decision.suppressed_before,
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
        assoc_probe,
        progress.assoc_probe_result,
        yes_no(progress.assoc_join_rescue_attempted),
        yes_no(progress.assoc_set_ssid_rescue_attempted),
        firstread_class,
        progress.last_rx_probe_poll,
        progress.last_rx_probe_flags,
        progress.last_control_rx_probe_poll,
        progress.last_control_rx_probe_flags,
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
        rx_source_mode,
        rx_source.probe_len,
        rx_source.interrupt_enable,
        yes_no(rx_source.frame_indicated),
        yes_no(rx_source.host_interrupt),
        yes_no(rx_source.card_interrupt),
        yes_no(rx_source.function2_ready),
        control_rx_source_mode,
        control_rx_source.probe_len,
        control_rx_source.interrupt_enable,
        yes_no(control_rx_source.frame_indicated),
        yes_no(control_rx_source.host_interrupt),
        yes_no(control_rx_source.card_interrupt),
        yes_no(control_rx_source.function2_ready),
        yes_no(rx_trace.valid),
        rx_trace.flags,
        rx_trace.detail,
        rx_trace.probe_len,
        rx_trace.source_result,
        rx_trace.prefix_signature,
        rx_trace.prefix_digest,
        rx_trace.rframe_len,
        rx_trace.firstread_reads,
        rx_trace.block_reads,
        rx_trace.rframe_reads,
        rx_trace.request_len,
        rx_trace.block_size,
        rx_trace.block_count,
        rx_trace.retransmit_sample,
        cyw43_rx_trace_retransmit_action_name(rx_trace.retransmit_sample),
        rx_trace.queue_depth,
        rx_trace.queue_high_water,
        rx_trace.cmd53_arg,
        cyw43_rx_trace_cmd53_function(rx_trace.cmd53_arg),
        cyw43_rx_trace_cmd53_addr(rx_trace.cmd53_arg),
        yes_no(cyw43_rx_trace_cmd53_write(rx_trace.cmd53_arg)),
        cyw43_rx_trace_cmd53_mode(rx_trace.cmd53_arg),
        yes_no(cyw43_rx_trace_cmd53_increment(rx_trace.cmd53_arg)),
        cyw43_rx_trace_cmd53_count(rx_trace.cmd53_arg),
        rx_trace.transfer_result,
        rx_trace.payload_before_digest,
        rx_trace.payload_after_digest,
        yes_no(control_rx_trace.valid),
        control_rx_trace.flags,
        control_rx_trace.detail,
        control_rx_trace.probe_len,
        control_rx_trace.source_result,
        control_rx_trace.prefix_signature,
        control_rx_trace.prefix_digest,
        control_rx_trace.rframe_len,
        control_rx_trace.firstread_reads,
        control_rx_trace.block_reads,
        control_rx_trace.rframe_reads,
        control_rx_trace.request_len,
        control_rx_trace.block_size,
        control_rx_trace.block_count,
        control_rx_trace.retransmit_sample,
        cyw43_rx_trace_retransmit_action_name(control_rx_trace.retransmit_sample),
        control_rx_trace.queue_depth,
        control_rx_trace.queue_high_water,
        control_rx_trace.cmd53_arg,
        cyw43_rx_trace_cmd53_function(control_rx_trace.cmd53_arg),
        cyw43_rx_trace_cmd53_addr(control_rx_trace.cmd53_arg),
        yes_no(cyw43_rx_trace_cmd53_write(control_rx_trace.cmd53_arg)),
        cyw43_rx_trace_cmd53_mode(control_rx_trace.cmd53_arg),
        yes_no(cyw43_rx_trace_cmd53_increment(control_rx_trace.cmd53_arg)),
        cyw43_rx_trace_cmd53_count(control_rx_trace.cmd53_arg),
        control_rx_trace.transfer_result,
        control_rx_trace.payload_before_digest,
        control_rx_trace.payload_after_digest,
        progress.last_flags,
        progress.last_len,
        progress.last_ethertype,
        yes_no(progress.last_ethertype_valid),
        next_action,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
    emit_cyw43_wifi_gate7_host_eapol_subgate(contract, "host-eapol-status", status, progress);
    emit_cyw43_host_eapol_rxtrace_detail(contract, "data", rx_trace);
    emit_cyw43_host_eapol_rxtrace_detail(contract, "control", control_rx_trace);
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_join_submit_window(
    contract: DriverTaskContract,
    event: &'static str,
    poll_limit: usize,
    activity: bool,
    progress: Option<&Cyw43HostEapolProgress>,
) {
    use core::fmt::Write;

    let fallback = Cyw43HostEapolProgress::default();
    let progress = progress.unwrap_or(&fallback);
    let status = cyw43_host_eapol_atomic_status();
    let (subgate, name, focus) = cyw43_wifi_gate7_host_eapol_subgate(status, progress);
    let focus = if event == "end"
        && !activity
        && progress.polls == 0
        && progress.event_rx == 0
        && progress.data_rx == 0
    {
        "no-runtime-completions"
    } else {
        focus
    };
    let mut line = heapless::String::<320>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_JOIN_SUBMIT_WINDOW contract={} event={} limit={} activity={} subgate={} name={} focus={} status={} polls={} associated={} link_up={} event_rx={} eapol_rx={} data_rx={}",
        contract.name,
        event,
        poll_limit,
        yes_no(activity),
        subgate,
        name,
        focus,
        status,
        progress.polls,
        yes_no(progress.associated),
        yes_no(progress.link_up),
        progress.event_rx,
        progress.eapol_rx,
        progress.data_rx,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_wifi_gate7_host_eapol_subgate(
    contract: DriverTaskContract,
    source: &'static str,
    status: &'static str,
    progress: &Cyw43HostEapolProgress,
) {
    let (subgate, name, focus) = cyw43_wifi_gate7_host_eapol_subgate(status, progress);
    emit_cyw43_wifi_gate7_subgate(
        contract,
        source,
        subgate,
        name,
        status,
        focus,
        progress.polls,
        progress.associated,
        progress.link_up,
        progress.event_rx,
        progress.eapol_rx,
        progress.data_rx,
        0,
    );
}

#[cfg(feature = "kernel")]
fn emit_cyw43_wifi_gate7_subgate(
    contract: DriverTaskContract,
    source: &'static str,
    subgate: &'static str,
    name: &'static str,
    status: &'static str,
    reason: &'static str,
    polls: u32,
    associated: bool,
    link_up: bool,
    event_rx: u32,
    eapol_rx: u32,
    data_rx: u32,
    result: u32,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<384>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_WIFI_GATE7 contract={} source={} subgate={} name={} status={} reason={} polls={} associated={} link_up={} event_rx={} eapol_rx={} data_rx={} result=0x{:08x}",
        contract.name,
        source,
        subgate,
        name,
        status,
        reason,
        polls,
        yes_no(associated),
        yes_no(link_up),
        event_rx,
        eapol_rx,
        data_rx,
        result,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn cyw43_wifi_gate7_host_eapol_subgate(
    status: &'static str,
    progress: &Cyw43HostEapolProgress,
) -> (&'static str, &'static str, &'static str) {
    if status == "secure" || CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) != 0 {
        return ("7e", "secure-release", "passed");
    }
    if !progress.associated {
        if status == "required" {
            return (
                "7b",
                "association",
                cyw43_host_eapol_required_reason(progress),
            );
        }
        if progress.polls == 0 && progress.event_rx == 0 && progress.data_rx == 0 {
            return ("7a", "join-submit", "join-accepted");
        }
        return (
            "7b",
            "association",
            cyw43_host_eapol_required_reason(progress),
        );
    }
    if progress.eapol_rx == 0 {
        return ("7c", "eapol-rx", "waiting-m1");
    }
    if CYW43_HOST_EAPOL_M4.load(Ordering::Acquire) == 0
        || CYW43_HOST_EAPOL_PTK.load(Ordering::Acquire) == 0
        || CYW43_HOST_EAPOL_GTK.load(Ordering::Acquire) == 0
    {
        return ("7d", "eapol-handshake", "waiting-keys");
    }
    ("7e", "secure-release", "waiting-carrier-release")
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_atomic_status() -> &'static str {
    if CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) != 0 {
        "secure"
    } else if CYW43_HOST_EAPOL_REQUIRED.load(Ordering::Acquire) != 0 {
        "required"
    } else if CYW43_HOST_EAPOL_ACTIVE.load(Ordering::Acquire) != 0 {
        "pending"
    } else {
        "inactive"
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_host_eapol_required_reason(progress: &Cyw43HostEapolProgress) -> &'static str {
    if let Some(error) = progress.eapol_error {
        error
    } else if progress.auth_timeout_seen && !progress.associated {
        "cyw43-association-auth-timeout"
    } else if progress.set_ssid_failure_seen && !progress.associated {
        "cyw43-association-set-ssid-failed"
    } else if progress.assoc_probe_not_associated && !progress.associated {
        "cyw43-association-not-associated"
    } else if !progress.associated && progress.event_rx == 0 && progress.data_rx == 0 {
        "cyw43-association-event-missing"
    } else {
        "host-eapol-required"
    }
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_firstread_class(progress: &Cyw43HostEapolProgress) -> &'static str {
    let has_empty = progress.rx_firstread_empty != 0 || progress.control_rx_firstread_empty != 0;
    if !has_empty {
        return "none";
    }
    if cyw43_host_eapol_source_asserted(progress) {
        return "source-asserted-empty";
    }
    if progress.associated {
        "postassoc-empty"
    } else {
        "preassoc-cadence-empty"
    }
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_rxtrace_detail(
    contract: DriverTaskContract,
    lane: &'static str,
    trace: Cyw43RxIdleTrace,
) {
    if !trace.valid {
        return;
    }
    use core::fmt::Write;

    let mut line = heapless::String::<2048>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_RXTRACE contract={} lane={} flags=0x{:04x} detail=0x{:04x} probe_len={} source=0x{:08x} prefix=0x{:08x} digest=0x{:08x} rframe=0x{:04x} firstread_reads={} block_reads={} rframe_reads={} request_len={} block_size={} block_count={} retx_sample=0x{:04x} retx_action={} queue_depth={} queue_high_water={} cmd53_arg=0x{:08x} cmd53_fn={} cmd53_addr=0x{:05x} cmd53_write={} cmd53_mode={} cmd53_inc={} cmd53_count={} transfer_result=0x{:08x} payload_before=0x{:08x} payload_after=0x{:08x} source_flags=0x{:04x} pre_source=0x{:08x} post_source=0x{:08x} pre_fresh={} pre_asserted={} pre_failed={} post_fresh={} post_asserted={} post_failed={} source_asserted_ever={} pre_int=0x{:08x} post_int=0x{:08x} pre_sdhci=0x{:08x} post_sdhci=0x{:08x} first_nonzero={} first_nonzero_off={} first_nonzero_byte=0x{:02x} fifo_window_req=0x{:08x} fifo_window_programmed=0x{:08x} fifo_window_readback=0x{:08x} fifo_window_flags=0x{:04x} fifo_window_ok={} source_empty_polls={} irq_preserve_count={} irq_preserve_reason={} irq_preserve_int=0x{:08x} irq_preserve_ack=0x{:08x} trace_seq={} start_ticks_lo=0x{:08x} pre_sample_delta_ticks={} transfer_delta_ticks={} post_sample_delta_ticks={}",
        contract.name,
        lane,
        trace.flags,
        trace.detail,
        trace.probe_len,
        trace.source_result,
        trace.prefix_signature,
        trace.prefix_digest,
        trace.rframe_len,
        trace.firstread_reads,
        trace.block_reads,
        trace.rframe_reads,
        trace.request_len,
        trace.block_size,
        trace.block_count,
        trace.retransmit_sample,
        cyw43_rx_trace_retransmit_action_name(trace.retransmit_sample),
        trace.queue_depth,
        trace.queue_high_water,
        trace.cmd53_arg,
        cyw43_rx_trace_cmd53_function(trace.cmd53_arg),
        cyw43_rx_trace_cmd53_addr(trace.cmd53_arg),
        yes_no(cyw43_rx_trace_cmd53_write(trace.cmd53_arg)),
        cyw43_rx_trace_cmd53_mode(trace.cmd53_arg),
        yes_no(cyw43_rx_trace_cmd53_increment(trace.cmd53_arg)),
        cyw43_rx_trace_cmd53_count(trace.cmd53_arg),
        trace.transfer_result,
        trace.payload_before_digest,
        trace.payload_after_digest,
        trace.source_flags,
        trace.pre_source_result,
        trace.post_source_result,
        yes_no(cyw43_rx_trace_pre_source_fresh(trace)),
        yes_no(cyw43_rx_trace_pre_source_asserted(trace)),
        yes_no(cyw43_rx_trace_pre_source_failed(trace)),
        yes_no(cyw43_rx_trace_post_source_fresh(trace)),
        yes_no(cyw43_rx_trace_post_source_asserted(trace)),
        yes_no(cyw43_rx_trace_post_source_failed(trace)),
        yes_no(cyw43_rx_trace_source_asserted_ever(trace)),
        trace.pre_intstatus,
        trace.post_intstatus,
        trace.pre_sdhci_status,
        trace.post_sdhci_status,
        cyw43_rx_trace_first_nonzero_desc(trace),
        trace.first_nonzero_offset,
        trace.first_nonzero_byte,
        trace.fifo_window_requested,
        trace.fifo_window_programmed,
        trace.fifo_window_readback,
        trace.fifo_window_flags,
        yes_no(cyw43_rx_trace_fifo_window_ok(trace)),
        trace.source_empty_polls,
        trace.rx_irq_preserve_count,
        trace.rx_irq_preserve_reason,
        trace.rx_irq_preserve_int_status,
        trace.rx_irq_preserve_ack_bits,
        trace.sequence,
        trace.start_ticks_lo,
        trace.pre_sample_delta_ticks,
        trace.transfer_delta_ticks,
        trace.post_sample_delta_ticks,
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
fn emit_cyw43_host_eapol_bssid_refresh(
    contract: DriverTaskContract,
    poll: usize,
    status: &'static str,
    bssid: EthernetAddress,
    reason: &'static str,
    result: u32,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<320>::new();
    let mac = bssid.0;
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_BSSID_REFRESH contract={} poll={} status={} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} reason={} result=0x{:08x}",
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
        result,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_assoc_probe(
    contract: DriverTaskContract,
    poll: usize,
    attempt: u8,
    status: &'static str,
    bssid: EthernetAddress,
    reason: &'static str,
    result: u32,
) {
    use core::fmt::Write;

    let mac = bssid.0;
    let mut line = heapless::String::<320>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_ASSOC_PROBE contract={} poll={} attempt={} status={} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} reason={} result=0x{:08x}",
        contract.name,
        poll,
        attempt,
        status,
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5],
        reason,
        result,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_assoc_rescue(
    contract: DriverTaskContract,
    poll: usize,
    attempt: u8,
    status: &'static str,
    reason: &'static str,
    action: &'static str,
    result: u32,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<256>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_ASSOC_RESCUE contract={} poll={} attempt={} status={} reason={} action={} result=0x{:08x}",
        contract.name, poll, attempt, status, reason, action, result
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_tx_shape(
    contract: DriverTaskContract,
    stage: &'static str,
    poll: usize,
    frame: &[u8],
    completion: Option<DriverTaskCompletionRecord>,
) {
    use core::fmt::Write;

    if frame.len() < 14 {
        return;
    }
    let ethertype = cyw43_ethertype(frame).unwrap_or(0);
    let tx_result = completion.map_or(0, |completion| completion.result);
    let tx_detail = completion.map_or(0, |completion| completion.detail);
    let tx_code = completion.map_or(0, |completion| completion.code);
    let total_len = usize::try_from(tx_result)
        .ok()
        .filter(|result| *result != 0)
        .unwrap_or_else(|| cyw43_data_tx_total_len(frame.len()).unwrap_or(0));
    let derived_request_len = if total_len == 0 {
        (0, false)
    } else {
        cyw43_data_tx_request_len_for_frame(frame, total_len)
    };
    let request_len = if tx_detail != 0 {
        usize::from(tx_detail)
    } else {
        derived_request_len.0
    };
    let request_source = if tx_detail != 0 {
        "completion"
    } else {
        "derived"
    };
    let data_block_mode = tx_detail == 0 && derived_request_len.1;
    let (block_size, block_count) =
        cyw43_function2_data_tx_cmd53_shape(request_len, data_block_mode);
    let cmd53_mode = if block_count == 0 { "byte" } else { "block" };
    let bdc_priority = cyw43_bdc_priority_for_ethertype(ethertype);
    let mut line = heapless::String::<512>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_TX contract={} stage={} poll={} len={} total_len={} request_len={} request_source={} derived_request_len={} cmd53_mode={} block_size={} block_count={} tx_code={} tx_detail=0x{:04x} tx_result=0x{:08x} dst={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} src={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} ethertype=0x{:04x} bdc_priority={}",
        contract.name,
        stage,
        poll,
        frame.len(),
        total_len,
        request_len,
        request_source,
        derived_request_len.0,
        cmd53_mode,
        block_size,
        block_count,
        tx_code,
        tx_detail,
        tx_result,
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
        bdc_priority,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn cyw43_data_tx_total_len(payload_len: usize) -> Option<usize> {
    payload_len.checked_add(CYW43_SDPCM_DATA_TX_OVERHEAD_BYTES)
}

#[cfg(feature = "kernel")]
const fn cyw43_data_tx_request_len(unpadded_len: usize) -> usize {
    let aligned = align4(unpadded_len);
    if aligned <= SDIO_CMD53_BYTE_MODE_MAX as usize {
        aligned
    } else {
        let remainder = aligned % CYW43_FUNCTION2_BLOCK_BYTES;
        if remainder == 0 {
            aligned
        } else {
            aligned + (CYW43_FUNCTION2_BLOCK_BYTES - remainder)
        }
    }
}

#[cfg(feature = "kernel")]
fn cyw43_data_tx_request_len_for_frame(frame: &[u8], unpadded_len: usize) -> (usize, bool) {
    let _ = frame;
    let request_len = cyw43_data_tx_request_len(unpadded_len);
    if request_len < CYW43_DATA_TX_MIN_FUNCTION2_BYTES {
        (CYW43_DATA_TX_MIN_FUNCTION2_BYTES, false)
    } else {
        (request_len, false)
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_function2_cmd53_shape(request_len: usize) -> (u16, u16) {
    if request_len > SDIO_CMD53_BYTE_MODE_MAX as usize
        && request_len % CYW43_FUNCTION2_BLOCK_BYTES == 0
    {
        (
            CYW43_FUNCTION2_BLOCK_BYTES as u16,
            (request_len / CYW43_FUNCTION2_BLOCK_BYTES) as u16,
        )
    } else {
        (request_len as u16, 0)
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_function2_data_tx_cmd53_shape(
    request_len: usize,
    prefer_block_mode: bool,
) -> (u16, u16) {
    if prefer_block_mode
        && request_len >= CYW43_FUNCTION2_BLOCK_BYTES
        && request_len % CYW43_FUNCTION2_BLOCK_BYTES == 0
    {
        (
            CYW43_FUNCTION2_BLOCK_BYTES as u16,
            (request_len / CYW43_FUNCTION2_BLOCK_BYTES) as u16,
        )
    } else {
        cyw43_function2_cmd53_shape(request_len)
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_bdc_priority_for_ethertype(ethertype: u16) -> u8 {
    if ethertype == ETH_P_EAPOL {
        CYW43_HOST_EAPOL_BDC_PRIORITY
    } else {
        0
    }
}

#[cfg(feature = "kernel")]
const fn align4(value: usize) -> usize {
    (value + 3) & !3
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_message(
    contract: DriverTaskContract,
    message: &'static str,
    action: &'static str,
    poll: usize,
    len: usize,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_MESSAGE contract={} msg={} action={} poll={} len={}",
        contract.name, message, action, poll, len,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_proof(
    contract: DriverTaskContract,
    action: &'static str,
    poll: usize,
    len: usize,
    proof: HostEapolFrameProof,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<384>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_PROOF contract={} action={} poll={} len={} msg={} next_action={} key_info=0x{:04x} key_ver={} key_len={} kde_len={} pairwise={} ack={} mic={} install={} secure={} encrypted={} nonce={} replay={}",
        contract.name,
        action,
        poll,
        len,
        proof.message,
        proof.next_action,
        proof.key_info,
        proof.key_version,
        proof.key_len,
        proof.key_data_len,
        yes_no(proof.pairwise),
        yes_no(proof.ack),
        yes_no(proof.mic),
        yes_no(proof.install),
        yes_no(proof.secure),
        yes_no(proof.encrypted_key_data),
        yes_no(proof.nonce_present),
        yes_no(proof.replay_counter_nonzero),
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_key(
    contract: DriverTaskContract,
    kind: &'static str,
    stage: &'static str,
    status: &'static str,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_KEY contract={} kind={} stage={} status={}",
        contract.name, kind, stage, status,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_tx_drain(
    contract: DriverTaskContract,
    stage: &'static str,
    result: &'static str,
    tx_result: u32,
    polls: usize,
    observed_control: u32,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_DRAIN contract={} stage={} result={} tx_result=0x{:08x} polls={} observed_control={}",
        contract.name, stage, result, tx_result, polls, observed_control,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_rx_admission_restore(contract: DriverTaskContract, status: &'static str) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_RX_ADMISSION contract={} action=restore-after-secure status={} allmulti={} promisc={} data=allowed-after-keys",
        contract.name,
        status,
        CYW43_POST_SECURE_DATA_ALLMULTI,
        CYW43_POST_SECURE_DATA_PROMISC,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_cyw43_host_eapol_error(
    contract: DriverTaskContract,
    stage: &'static str,
    error: &'static str,
    poll: usize,
    len: usize,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_HOST_EAPOL_ERROR contract={} stage={} error={} poll={} len={}",
        contract.name, stage, error, poll, len,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43HostEapolTxProof {
    submitted_seq: u8,
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cyw43UnprovenTxWindow {
    Known {
        proof: Cyw43HostEapolTxProof,
        submitted_count: u32,
    },
    Unknown,
}

#[cfg(feature = "kernel")]
const fn cyw43_tx_completion_proof(
    completion: DriverTaskCompletionRecord,
) -> Option<Cyw43HostEapolTxProof> {
    if completion.code != DriverTaskCompletionCode::Progress.as_u16()
        || completion.result == 0
        || completion.frame.flags == 0
    {
        return None;
    }
    Some(Cyw43HostEapolTxProof {
        submitted_seq: (completion.frame.flags & 0x00ff) as u8,
    })
}

#[cfg(feature = "kernel")]
const fn cyw43_sdpcm_credit_from_flags(flags: u16) -> Option<u8> {
    let channel = flags & DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_MASK;
    if channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL
        || channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT
        || channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA
        || flags & DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_MASK != 0
    {
        return Some(
            ((flags & DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_MASK)
                >> DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT) as u8,
        );
    }
    None
}

#[cfg(feature = "kernel")]
const fn cyw43_sdpcm_credit_from_completion_flags(flags: u16) -> u8 {
    ((flags & DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_MASK)
        >> DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT) as u8
}

#[cfg(feature = "kernel")]
const fn cyw43_completion_sdpcm_credit(completion: DriverTaskCompletionRecord) -> Option<u8> {
    let code = completion.code;
    if code == DriverTaskCompletionCode::FrameReady.as_u16()
        || code == DriverTaskCompletionCode::Idle.as_u16()
        || code == DriverTaskCompletionCode::Progress.as_u16()
    {
        if code != DriverTaskCompletionCode::FrameReady.as_u16() && completion.frame.len == 0 {
            return None;
        }
        return Some(cyw43_sdpcm_credit_from_completion_flags(
            completion.frame.flags,
        ));
    }
    None
}

#[cfg(feature = "kernel")]
const fn cyw43_sdpcm_credit_observation_covers_submitted_seq(
    seq_max: u8,
    submitted_seq: u8,
) -> bool {
    let next_seq = submitted_seq.wrapping_add(1);
    next_seq == seq_max || (seq_max.wrapping_sub(next_seq) & 0x80) == 0
}

#[cfg(feature = "kernel")]
const fn cyw43_frame_flags_credit_covers_tx(flags: u16, proof: Cyw43HostEapolTxProof) -> bool {
    match cyw43_sdpcm_credit_from_flags(flags) {
        Some(seq_max) => {
            cyw43_sdpcm_credit_observation_covers_submitted_seq(seq_max, proof.submitted_seq)
        }
        None => false,
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_completion_credit_covers_tx(
    completion: DriverTaskCompletionRecord,
    proof: Cyw43HostEapolTxProof,
) -> bool {
    match cyw43_completion_sdpcm_credit(completion) {
        Some(seq_max) => {
            cyw43_sdpcm_credit_observation_covers_submitted_seq(seq_max, proof.submitted_seq)
        }
        None => false,
    }
}

#[cfg(feature = "kernel")]
fn cyw43_data_tx_credit_proven(
    contract: DriverTaskContract,
    tx_completion: DriverTaskCompletionRecord,
) -> bool {
    let Some(tx_proof) = cyw43_tx_completion_proof(tx_completion) else {
        return false;
    };
    if cyw43_completion_credit_covers_tx(tx_completion, tx_proof) {
        record_cyw43_completion_credit_accounting(tx_completion);
        return true;
    }
    for _ in 0..CYW43_DATA_TX_CREDIT_PROOF_POLLS {
        let Some(completion) =
            poll_cyw43_driver_task_data_completion(CYW43_DATA_RX_STEADY_POLL_FLAGS)
        else {
            core::hint::spin_loop();
            continue;
        };
        let credit_covers_tx = cyw43_completion_credit_covers_tx(completion, tx_proof);
        let _ = preserve_driver_task_pre_poll_completion(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            completion,
        );
        if credit_covers_tx {
            record_cyw43_completion_credit_accounting(completion);
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

#[cfg(feature = "kernel")]
fn clear_cyw43_unproven_tx_window() {
    CYW43_TX_UNPROVEN_ACTIVE.store(CYW43_TX_UNPROVEN_NONE, Ordering::Release);
    CYW43_TX_UNPROVEN_SEQ.store(0, Ordering::Release);
    CYW43_TX_UNPROVEN_COUNT.store(0, Ordering::Release);
}

#[cfg(feature = "kernel")]
fn record_cyw43_unproven_tx_window(completion: Option<DriverTaskCompletionRecord>) {
    CYW43_TX_CREDIT_UNPROVEN.fetch_add(1, Ordering::AcqRel);
    if let Some(proof) = completion.and_then(cyw43_tx_completion_proof) {
        CYW43_TX_UNPROVEN_SEQ.store(u32::from(proof.submitted_seq), Ordering::Release);
        CYW43_TX_UNPROVEN_COUNT.store(
            CYW43_TX_SUBMITTED.load(Ordering::Acquire),
            Ordering::Release,
        );
        CYW43_TX_UNPROVEN_ACTIVE.store(CYW43_TX_UNPROVEN_KNOWN, Ordering::Release);
    } else {
        CYW43_TX_UNPROVEN_SEQ.store(0, Ordering::Release);
        CYW43_TX_UNPROVEN_COUNT.store(0, Ordering::Release);
        CYW43_TX_UNPROVEN_ACTIVE.store(CYW43_TX_UNPROVEN_UNKNOWN, Ordering::Release);
    }
}

#[cfg(feature = "kernel")]
fn cyw43_unproven_tx_window() -> Option<Cyw43UnprovenTxWindow> {
    match CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire) {
        CYW43_TX_UNPROVEN_KNOWN => Some(Cyw43UnprovenTxWindow::Known {
            proof: Cyw43HostEapolTxProof {
                submitted_seq: CYW43_TX_UNPROVEN_SEQ.load(Ordering::Acquire) as u8,
            },
            submitted_count: CYW43_TX_UNPROVEN_COUNT.load(Ordering::Acquire),
        }),
        CYW43_TX_UNPROVEN_UNKNOWN => Some(Cyw43UnprovenTxWindow::Unknown),
        _ => None,
    }
}

#[cfg(feature = "kernel")]
fn cyw43_frame_flags_credit_covers_window(flags: u16, window: Cyw43UnprovenTxWindow) -> bool {
    match window {
        Cyw43UnprovenTxWindow::Known { proof, .. } => {
            cyw43_frame_flags_credit_covers_tx(flags, proof)
        }
        Cyw43UnprovenTxWindow::Unknown => cyw43_sdpcm_credit_from_flags(flags).is_some(),
    }
}

#[cfg(feature = "kernel")]
fn cyw43_completion_credit_covers_window(
    completion: DriverTaskCompletionRecord,
    window: Cyw43UnprovenTxWindow,
) -> bool {
    match window {
        Cyw43UnprovenTxWindow::Known { proof, .. } => {
            cyw43_completion_credit_covers_tx(completion, proof)
        }
        Cyw43UnprovenTxWindow::Unknown => cyw43_completion_sdpcm_credit(completion).is_some(),
    }
}

#[cfg(feature = "kernel")]
fn cyw43_pending_rx_credit_covers_window(window: Cyw43UnprovenTxWindow) -> bool {
    CYW43_PENDING_RX_QUEUE
        .lock()
        .iter()
        .any(|pending| cyw43_frame_flags_credit_covers_window(pending.flags, window))
}

#[cfg(feature = "kernel")]
fn mark_cyw43_tx_completed_through(completed_through: u32) {
    let submitted = CYW43_TX_SUBMITTED.load(Ordering::Acquire);
    update_atomic_u32_max(&CYW43_TX_CREDIT_COMPLETED, completed_through.min(submitted));
}

#[cfg(feature = "kernel")]
fn mark_all_submitted_cyw43_tx_completed() {
    mark_cyw43_tx_completed_through(CYW43_TX_SUBMITTED.load(Ordering::Acquire));
}

#[cfg(feature = "kernel")]
fn mark_one_cyw43_tx_completed() {
    let submitted = CYW43_TX_SUBMITTED.load(Ordering::Acquire);
    let mut observed = CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire);
    while observed < submitted {
        let next = observed.saturating_add(1).min(submitted);
        match CYW43_TX_CREDIT_COMPLETED.compare_exchange(
            observed,
            next,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => break,
            Err(current) => observed = current,
        }
    }
}

#[cfg(feature = "kernel")]
fn cyw43_complete_unproven_tx_window(window: Cyw43UnprovenTxWindow) {
    match window {
        Cyw43UnprovenTxWindow::Known {
            submitted_count, ..
        } => mark_cyw43_tx_completed_through(submitted_count),
        Cyw43UnprovenTxWindow::Unknown => mark_one_cyw43_tx_completed(),
    }
    clear_cyw43_unproven_tx_window();
}

#[cfg(feature = "kernel")]
fn record_cyw43_completion_credit_accounting(completion: DriverTaskCompletionRecord) {
    if cyw43_completion_sdpcm_credit(completion).is_none() {
        return;
    }
    if let Some(window) = cyw43_unproven_tx_window() {
        if cyw43_completion_credit_covers_window(completion, window) {
            cyw43_complete_unproven_tx_window(window);
            return;
        }
    }
    mark_all_submitted_cyw43_tx_completed();
}

#[cfg(feature = "kernel")]
fn complete_cyw43_unproven_tx_window_from_rx_flags(flags: u16) -> bool {
    let Some(window) = cyw43_unproven_tx_window() else {
        return false;
    };
    if cyw43_frame_flags_credit_covers_window(flags, window) {
        cyw43_complete_unproven_tx_window(window);
        return true;
    }
    false
}

#[cfg(feature = "kernel")]
fn cyw43_tx_unproven_window_ready(contract: DriverTaskContract) -> bool {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT {
        return true;
    }
    let Some(window) = cyw43_unproven_tx_window() else {
        return true;
    };
    if cyw43_pending_rx_credit_covers_window(window) {
        cyw43_complete_unproven_tx_window(window);
        return true;
    }
    for _ in 0..CYW43_DATA_TX_ADMISSION_RECOVERY_POLLS {
        let _ = resume_cyw43_active_prompt_poll_for_tx_retry(contract);
        if cyw43_pending_rx_credit_covers_window(window) {
            cyw43_complete_unproven_tx_window(window);
            return true;
        }
        let Some(completion) =
            poll_cyw43_driver_task_data_completion(CYW43_DATA_RX_STEADY_POLL_FLAGS)
        else {
            core::hint::spin_loop();
            continue;
        };
        let credit_covers_window = cyw43_completion_credit_covers_window(completion, window);
        let _ = preserve_driver_task_pre_poll_completion(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            completion,
        );
        if credit_covers_window || cyw43_pending_rx_credit_covers_window(window) {
            cyw43_complete_unproven_tx_window(window);
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

#[cfg(feature = "kernel")]
fn cyw43_fresh_tx_admission_ready(contract: DriverTaskContract) -> bool {
    contract != CYW43_WIFI_DRIVER_TASK_CONTRACT
        || !cyw43_active_descriptor_blocks_fresh_net_poll(contract)
}

fn cyw43_data_tx_admission_ready(contract: DriverTaskContract) -> bool {
    if !cyw43_data_plane_ready() {
        return false;
    }
    #[cfg(feature = "kernel")]
    {
        cyw43_fresh_tx_admission_ready(contract) && cyw43_tx_unproven_window_ready(contract)
    }
    #[cfg(not(feature = "kernel"))]
    {
        let _ = contract;
        true
    }
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_tx_drain_window(stage: &'static str) -> (u64, usize) {
    let _ = stage;
    (
        CYW43_HOST_EAPOL_TX_DRAIN_TIMEOUT_MS,
        CYW43_HOST_EAPOL_TX_DRAIN_POLLS,
    )
}

#[cfg(feature = "kernel")]
fn wait_cyw43_host_eapol_tx_drain(
    contract: DriverTaskContract,
    session: &mut Cyw43HostEapolSession,
    stage: &'static str,
    poll: usize,
    tx_completion: DriverTaskCompletionRecord,
) -> Result<bool, DriverTaskNetError> {
    #[cfg(test)]
    if CYW43_HOST_EAPOL_TEST_IO_STUB.load(Ordering::Acquire) != 0 {
        CYW43_HOST_EAPOL_TEST_TX_DRAINED.fetch_add(1, Ordering::AcqRel);
        if (stage == "m4-before-wsec" || stage == "post-secure-m4-before-wsec")
            && CYW43_HOST_EAPOL_TEST_PTK_INSTALLED.load(Ordering::Acquire) == 0
        {
            CYW43_HOST_EAPOL_TEST_DRAIN_BEFORE_PTK.store(1, Ordering::Release);
        }
        if stage == "group-m2-before-secure" && CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) == 0
        {
            CYW43_HOST_EAPOL_TEST_DRAIN_BEFORE_SECURE.store(1, Ordering::Release);
        }
        let timeout_bit = match stage {
            "m2-before-m3" => 1,
            "m4-before-wsec" | "post-secure-m4-before-wsec" => 2,
            "group-m2-before-secure" => 4,
            _ => 0,
        };
        if CYW43_HOST_EAPOL_TEST_TX_DRAIN_TIMEOUTS.load(Ordering::Acquire) & timeout_bit != 0 {
            emit_cyw43_host_eapol_tx_drain(
                contract,
                stage,
                "test-timeout",
                tx_completion.result,
                0,
                0,
            );
            return Ok(false);
        }
        emit_cyw43_host_eapol_tx_drain(contract, stage, "test-stub", tx_completion.result, 0, 0);
        return Ok(true);
    }

    let Some(tx_proof) = cyw43_tx_completion_proof(tx_completion) else {
        emit_cyw43_host_eapol_tx_drain(
            contract,
            stage,
            "missing-tx-proof",
            tx_completion.result,
            0,
            0,
        );
        return Err(DriverTaskNetError::RuntimeInit("host-eapol-tx-drain-proof"));
    };
    let mut observed_control = 0u32;
    let mut polls = 0usize;
    let mut last_credit_completion = None;
    let (timeout_ms, poll_limit) = cyw43_host_eapol_tx_drain_window(stage);
    let mut deadline = cyw43_poll_deadline_from_millis_or_polls(timeout_ms, poll_limit);
    while cyw43_poll_deadline_open(&mut deadline) {
        polls = polls.saturating_add(1);
        let flags = cyw43_control_split_poll_flags(polls);
        let Some(completion) = poll_cyw43_driver_task_control_completion(flags) else {
            core::hint::spin_loop();
            continue;
        };
        if cyw43_completion_sdpcm_credit(completion).is_some() {
            last_credit_completion = Some(completion);
            if cyw43_completion_credit_covers_tx(completion, tx_proof) {
                record_cyw43_completion_credit_accounting(completion);
                emit_cyw43_host_eapol_tx_drain(
                    contract,
                    stage,
                    "credit-observed",
                    tx_completion.result,
                    polls,
                    observed_control,
                );
                return Ok(true);
            }
        }
        if completion.code == DriverTaskCompletionCode::FrameReady.as_u16() {
            let control_result = process_cyw43_host_eapol_control_completion(
                contract, session, poll, flags, completion,
            );
            if control_result.observed_frame {
                observed_control = observed_control.saturating_add(1);
            }
        }
        core::hint::spin_loop();
    }
    if let Some(completion) = last_credit_completion {
        if cyw43_completion_credit_covers_tx(completion, tx_proof) {
            record_cyw43_completion_credit_accounting(completion);
            emit_cyw43_host_eapol_tx_drain(
                contract,
                stage,
                "submitted-credit-window",
                tx_completion.result,
                polls,
                observed_control,
            );
            return Ok(true);
        }
    }
    emit_cyw43_host_eapol_tx_drain(
        contract,
        stage,
        "timeout",
        tx_completion.result,
        polls,
        observed_control,
    );
    Ok(false)
}

#[cfg(feature = "kernel")]
fn cyw43_host_eapol_next_action(
    status: &'static str,
    progress: &Cyw43HostEapolProgress,
) -> &'static str {
    if status == "secure" {
        return "release-dhcp-data";
    }
    if progress.auth_timeout_seen && !progress.associated {
        return if progress.assoc_join_rescue_attempted {
            "inspect-cyw43-auth-timeout-after-bsscfg-join-rescue"
        } else {
            "inspect-cyw43-auth-timeout-or-join-policy"
        };
    }
    if progress.set_ssid_failure_seen && !progress.associated {
        return "inspect-cyw43-set-ssid-failure-or-join-policy";
    }
    if progress.eapol_error.is_some() {
        return "inspect-host-eapol-error";
    }
    if progress.assoc_join_rescue_attempted && !progress.associated {
        return "inspect-cyw43-association-event-after-bsscfg-join-rescue";
    }
    if progress.assoc_set_ssid_rescue_attempted && !progress.associated {
        return "inspect-cyw43-association-event-after-set-ssid-rescue";
    }
    if progress.eapol_rx != 0 {
        "inspect-host-eapol-handshake-state"
    } else if progress.event_rx != 0 && !progress.associated {
        "inspect-cyw43-join-event-state"
    } else if progress.data_rx != 0 {
        "inspect-eapol-filter-or-ap-m1"
    } else if status == "required"
        && !progress.associated
        && progress.event_rx == 0
        && progress.rx_firstread_empty == 0
        && progress.rx_firstread_invalid == 0
        && progress.rx_firstread_failed == 0
        && progress.rx_firstread_remainder_failed == 0
        && progress.rx_firstread_decode_miss == 0
        && progress.control_rx_firstread_empty == 0
        && progress.control_rx_firstread_failed == 0
    {
        "inspect-cyw43-association-event-or-join-policy"
    } else if progress.rx_firstread_invalid != 0 {
        "inspect-cyw43-data-rx-firstread-prefix"
    } else if progress.rx_firstread_decode_miss != 0 {
        "inspect-sdpcm-readahead-channel-or-fws-tlv"
    } else if progress.rx_firstread_failed != 0 || progress.rx_firstread_remainder_failed != 0 {
        "inspect-cyw43-data-rx-cmd53-firstread"
    } else if progress.assoc_probe_not_associated && !progress.associated {
        if cyw43_rx_source_owner_state_missing(progress.last_control_rx_source)
            || cyw43_rx_source_owner_state_missing(progress.last_rx_source)
        {
            "inspect-sdio-owner-function2-rx-source"
        } else if cyw43_rx_source_is_passive(progress.last_control_rx_source)
            || cyw43_rx_source_is_passive(progress.last_rx_source)
        {
            "inspect-cyw43-assoc-event-rx-or-sdio-owner-ienx-snapshot"
        } else if progress.assoc_join_rescue_attempted {
            "inspect-cyw43-association-event-after-bsscfg-join-rescue"
        } else if progress.assoc_set_ssid_rescue_attempted {
            "inspect-cyw43-association-event-after-set-ssid-rescue"
        } else {
            "inspect-cyw43-association-event-or-join-policy"
        }
    } else if progress.control_rx_firstread_empty != 0 && !progress.associated {
        if cyw43_rx_source_owner_state_missing(progress.last_control_rx_source) {
            "inspect-sdio-owner-function2-rx-source"
        } else if cyw43_rx_source_is_passive(progress.last_control_rx_source) {
            "inspect-cyw43-assoc-event-rx-or-sdio-owner-ienx-snapshot"
        } else if progress.assoc_join_rescue_attempted {
            "inspect-cyw43-association-event-after-bsscfg-join-rescue"
        } else if progress.assoc_set_ssid_rescue_attempted {
            "inspect-cyw43-association-event-after-set-ssid-rescue"
        } else {
            "inspect-cyw43-assoc-event-rx-or-ienx"
        }
    } else if progress.rx_firstread_empty != 0 && progress.associated {
        "inspect-ap-m1-or-cyw43-rx-latch"
    } else if progress.rx_firstread_empty != 0 {
        if cyw43_rx_source_owner_state_missing(progress.last_rx_source) {
            "inspect-sdio-owner-function2-rx-source"
        } else if cyw43_rx_source_is_passive(progress.last_rx_source) {
            "inspect-association-event-or-sdio-owner-rx-source"
        } else if progress.assoc_join_rescue_attempted {
            "inspect-cyw43-association-event-after-bsscfg-join-rescue"
        } else if progress.assoc_set_ssid_rescue_attempted {
            "inspect-cyw43-association-event-after-set-ssid-rescue"
        } else {
            "inspect-association-event-or-cyw43-rx-latch"
        }
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
    cyw43_submit_control_exchange_checked_with_options(
        contract,
        payload,
        cmd,
        id,
        stage,
        header_mode,
        false,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_control_exchange_checked_with_options(
    contract: DriverTaskContract,
    payload: &[u8],
    cmd: u32,
    id: u16,
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
    pre_tx_drain: bool,
) -> Result<(), DriverTaskNetError> {
    let completion = cyw43_submit_control_exchange_unmapped_with_options(
        contract,
        payload,
        cmd,
        id,
        stage,
        header_mode,
        pre_tx_drain,
    )
    .map_err(|err| err.into_net_error())?;
    if completion.code != DriverTaskCompletionCode::FrameReady.as_u16() {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            stage,
            "fail",
            Some(completion),
        );
        return Err(DriverTaskNetError::RuntimeInit(stage));
    }
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
    cyw43_submit_control_exchange_unmapped_with_options(
        contract,
        payload,
        cmd,
        id,
        stage,
        header_mode,
        false,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_control_exchange_unmapped_with_options(
    contract: DriverTaskContract,
    payload: &[u8],
    cmd: u32,
    id: u16,
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
    pre_tx_drain: bool,
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::Cyw43Wifi,
        stage,
        "begin",
        None,
    );
    emit_cyw43_control_request_trace(contract, stage, cmd, id, header_mode, pre_tx_drain, payload);
    let control_iovar = cyw43_control_iovar_info(payload, cmd).map_or("none", |info| info.name);
    let expected_response_len =
        cyw43_control_request_expected_response_len(cmd, cyw43_control_iovar_info(payload, cmd))
            as u16;
    if cyw43_control_uses_runtime_exchange(stage, control_iovar) {
        return cyw43_submit_runtime_control_exchange(
            contract,
            payload,
            cmd,
            id,
            stage,
            header_mode,
            pre_tx_drain,
            expected_response_len,
            control_iovar,
        );
    }
    let tx_descriptor = cyw43_control_frame_descriptor(payload.len(), header_mode, pre_tx_drain);
    let mut tx_retries_spent = 0usize;
    let tx_completion = loop {
        let Some(completion) =
            run_cyw43_runtime_descriptor_command(contract, tx_descriptor, payload)
        else {
            let reason = if tx_retries_spent == 0 {
                "cyw43-control-tx-no-reply"
            } else {
                "cyw43-control-tx-retry-no-reply"
            };
            record_cyw43_control_split_failure(
                contract,
                stage,
                tx_descriptor,
                reason,
                None,
                cmd,
                id,
                header_mode,
                expected_response_len,
                control_iovar,
                0,
                0,
            );
            record_cyw43_runtime_command_no_reply_with_control_meta(
                contract,
                stage,
                tx_descriptor,
                tx_retries_spent,
                cmd,
                id,
                header_mode.as_str(),
                expected_response_len,
                control_iovar,
            );
            let status = if tx_retries_spent == 0 {
                "tx-no-reply"
            } else {
                "tx-retry-no-reply"
            };
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                status,
                None,
            );
            return Err(Cyw43CommandSubmitError::Runtime(
                DriverTaskNetError::RuntimeInit("cyw43-command-completion"),
            ));
        };
        let event = if tx_retries_spent == 0 {
            "tx-complete"
        } else {
            "tx-retry-complete"
        };
        emit_cyw43_control_split_completion(
            contract,
            stage,
            event,
            tx_retries_spent,
            tx_descriptor.flags,
            completion,
            cmd,
            id,
            header_mode,
            expected_response_len,
            control_iovar,
            0,
            0,
        );
        if driver_task_tx_completion_submitted(completion) {
            break completion;
        }
        if let Some(retry_completion) =
            cyw43_control_tx_submit_retry_completion(stage, completion, tx_retries_spent)
        {
            tx_retries_spent = tx_retries_spent.saturating_add(1);
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "tx-fault-sdio-owner-recover-begin",
                Some(retry_completion),
            );
            if recover_sdio_host_config_for_cyw43_tx_retry("cyw43-control-tx-sdio-reprime").is_err()
            {
                record_cyw43_control_split_failure(
                    contract,
                    stage,
                    tx_descriptor,
                    "cyw43-control-tx-sdio-owner-recover-failed",
                    Some(retry_completion),
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
                    "tx-fault-sdio-owner-recover-failed",
                    Some(retry_completion),
                );
                return Err(Cyw43CommandSubmitError::Completion(retry_completion));
            }
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "tx-fault-sdio-owner-recover-ready",
                Some(retry_completion),
            );
            resume_cyw43_active_prompt_poll_for_tx_retry(contract);
            core::hint::spin_loop();
            continue;
        }
        record_cyw43_control_split_failure(
            contract,
            stage,
            tx_descriptor,
            "cyw43-control-tx-not-submitted",
            Some(completion),
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
            Some(completion),
        );
        return Err(Cyw43CommandSubmitError::Completion(completion));
    };
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
        pre_tx_drain,
        expected_response_len,
        control_iovar,
    )
}

#[cfg(feature = "kernel")]
fn cyw43_submit_runtime_control_exchange(
    contract: DriverTaskContract,
    payload: &[u8],
    cmd: u32,
    id: u16,
    stage: &'static str,
    header_mode: Cyw43ControlHeaderMode,
    pre_tx_drain: bool,
    expected_response_len: u16,
    control_iovar: &str,
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
    let descriptor =
        cyw43_control_exchange_descriptor(payload.len(), cmd, id, header_mode, pre_tx_drain);
    let mut tx_retries_spent = 0usize;
    loop {
        let Some(completion) = run_cyw43_runtime_descriptor_command(contract, descriptor, payload)
        else {
            record_cyw43_runtime_command_no_reply(contract, stage, descriptor, tx_retries_spent);
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                if tx_retries_spent == 0 {
                    "no-reply"
                } else {
                    "retry-no-reply"
                },
                None,
            );
            return Err(Cyw43CommandSubmitError::Runtime(
                DriverTaskNetError::RuntimeInit("cyw43-command-completion"),
            ));
        };
        emit_cyw43_control_split_completion(
            contract,
            stage,
            if tx_retries_spent == 0 {
                "runtime-exchange-complete"
            } else {
                "runtime-exchange-retry-complete"
            },
            tx_retries_spent,
            descriptor.flags,
            completion,
            cmd,
            id,
            header_mode,
            expected_response_len,
            control_iovar,
            0,
            0,
        );
        if completion.code == DriverTaskCompletionCode::FrameReady.as_u16() {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "ready",
                Some(completion),
            );
            return Ok(completion);
        }
        if let Some(retry_completion) =
            cyw43_control_tx_submit_retry_completion(stage, completion, tx_retries_spent)
        {
            tx_retries_spent = tx_retries_spent.saturating_add(1);
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "runtime-exchange-sdio-owner-recover-begin",
                Some(retry_completion),
            );
            if recover_sdio_host_config_for_cyw43_tx_retry("cyw43-runtime-control-sdio-reprime")
                .is_err()
            {
                record_cyw43_control_split_failure(
                    contract,
                    stage,
                    descriptor,
                    "cyw43-runtime-control-sdio-owner-recover-failed",
                    Some(retry_completion),
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
                    "runtime-exchange-sdio-owner-recover-failed",
                    Some(retry_completion),
                );
                return Err(Cyw43CommandSubmitError::Completion(retry_completion));
            }
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "runtime-exchange-sdio-owner-recover-ready",
                Some(retry_completion),
            );
            resume_cyw43_active_prompt_poll_for_tx_retry(contract);
            core::hint::spin_loop();
            continue;
        }
        if completion.code == DriverTaskCompletionCode::Fault.as_u16() {
            emit_cyw43_runtime_command_fault(contract, stage, descriptor, completion, None);
        }
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            stage,
            "fail",
            Some(completion),
        );
        return Err(Cyw43CommandSubmitError::Completion(completion));
    }
}

#[cfg(feature = "kernel")]
fn cyw43_control_uses_runtime_exchange(stage: &'static str, control_iovar: &str) -> bool {
    let _ = (stage, control_iovar);
    false
}

#[cfg(feature = "kernel")]
fn cyw43_control_stage_is_host_eapol_promisc(stage: &'static str) -> bool {
    matches!(
        stage,
        "cyw43-host-eapol-promisc"
            | "cyw43-host-eapol-refresh-promisc"
            | "cyw43-host-eapol-rescue-promisc"
            | "cyw43-host-eapol-restore-promisc"
    )
}

#[cfg(feature = "kernel")]
fn cyw43_control_stage_is_optional_host_eapol_filter(stage: &'static str) -> bool {
    matches!(
        stage,
        "cyw43-host-eapol-allmulti"
            | "cyw43-host-eapol-promisc"
            | "cyw43-host-eapol-refresh-allmulti"
            | "cyw43-host-eapol-refresh-promisc"
            | "cyw43-host-eapol-rescue-allmulti"
            | "cyw43-host-eapol-rescue-promisc"
            | "cyw43-host-eapol-restore-allmulti"
            | "cyw43-host-eapol-restore-promisc"
    )
}

#[cfg(feature = "kernel")]
const fn cyw43_control_reply_is_optional_filter_reject(reply: Cyw43ControlReply) -> bool {
    reply.cmd == 0
        && reply.id == 0
        && (reply.status == CYW43_BCME_UNSUPPORTED_STATUS
            || reply.status == CYW43_BCME_BADARG_STATUS)
}

#[cfg(feature = "kernel")]
const fn cyw43_control_reply_is_commandless_reject(reply: Cyw43ControlReply) -> bool {
    reply.cmd == 0 && reply.id == 0 && reply.status != 0
}

#[cfg(feature = "kernel")]
fn cyw43_control_reply_is_host_eapol_wsec_key_commandless_reject(
    stage: &'static str,
    control_iovar: &str,
    reply: Cyw43ControlReply,
) -> bool {
    cyw43_control_reply_is_commandless_reject(reply)
        && (cyw43_control_uses_host_eapol_wsec_key_reply_window(stage, control_iovar)
            || cyw43_control_uses_post_secure_host_eapol_wsec_key_reply_window(
                stage,
                control_iovar,
            ))
}

#[cfg(feature = "kernel")]
fn cyw43_control_stage_is_host_eapol_rx_admission(stage: &'static str) -> bool {
    matches!(
        stage,
        "cyw43-host-eapol-mcast"
            | "cyw43-host-eapol-allmulti"
            | "cyw43-host-eapol-promisc"
            | "cyw43-host-eapol-refresh-mcast"
            | "cyw43-host-eapol-refresh-allmulti"
            | "cyw43-host-eapol-refresh-promisc"
            | "cyw43-host-eapol-rescue-mcast"
            | "cyw43-host-eapol-rescue-allmulti"
            | "cyw43-host-eapol-rescue-promisc"
            | "cyw43-host-eapol-restore-mcast"
            | "cyw43-host-eapol-restore-allmulti"
            | "cyw43-host-eapol-restore-promisc"
    )
}

#[cfg(feature = "kernel")]
fn cyw43_control_exchange_descriptor(
    payload_len: usize,
    cmd: u32,
    id: u16,
    header_mode: Cyw43ControlHeaderMode,
    pre_tx_drain: bool,
) -> DriverRuntimeCyw43CommandDescriptor {
    DriverRuntimeCyw43CommandDescriptor {
        op: DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE,
        flags: cyw43_control_runtime_flags(header_mode, pre_tx_drain),
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
    pre_tx_drain: bool,
) -> DriverRuntimeCyw43CommandDescriptor {
    DriverRuntimeCyw43CommandDescriptor {
        op: DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME,
        flags: cyw43_control_runtime_flags(header_mode, pre_tx_drain),
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
    pre_tx_drain: bool,
    expected_response_len: u16,
    control_iovar: &str,
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
    let exchange_descriptor =
        cyw43_control_exchange_descriptor(payload_len, cmd, id, header_mode, pre_tx_drain);
    let mut nonmatching_frames = 0u32;
    let mut malformed_frames = 0u32;
    let mut last_completion = None;
    let poll_attempts = cyw43_control_exchange_poll_attempts(stage, control_iovar);
    let timeout_ms = cyw43_control_exchange_timeout_ms(stage, control_iovar);
    let mut deadline = cyw43_poll_deadline_from_millis_or_polls(timeout_ms, poll_attempts);
    emit_cyw43_control_deadline_trace(
        contract,
        stage,
        "begin",
        timeout_ms,
        poll_attempts,
        0,
        &deadline,
        control_iovar,
    );
    if let Some((completion, reply)) =
        take_cyw43_pending_control_reply_completion(contract, cmd, id)
    {
        emit_cyw43_control_split_reply_trace(
            contract,
            stage,
            "cached-matched-reply",
            0,
            0,
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
        let response_completion = cyw43_control_response_completion_from_reply(
            contract,
            stage,
            exchange_descriptor,
            completion,
            reply,
            cmd,
            id,
            header_mode,
            expected_response_len,
            control_iovar,
            nonmatching_frames,
            malformed_frames,
        )?;
        emit_cyw43_control_split_completion(
            contract,
            stage,
            "cached-response-ready",
            0,
            0,
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
    let mut poll = 1usize;
    while cyw43_poll_deadline_open(&mut deadline) {
        let current_poll = poll;
        poll = poll.saturating_add(1);
        let poll = current_poll;
        let flags = cyw43_control_split_poll_flags(poll);
        let Some(completion) = poll_cyw43_driver_task_control_completion(flags) else {
            continue;
        };
        last_completion = Some(completion);
        if cyw43_control_split_poll_completion_should_trace(poll, poll_attempts, flags, completion)
        {
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
            if cyw43_control_stage_is_optional_host_eapol_filter(stage)
                && cyw43_control_reply_is_optional_filter_reject(reply)
            {
                let fault = cyw43_control_fault_completion(
                    completion.sequence,
                    CYW43_CONTROL_EXCHANGE_FAULT_DETAIL,
                    reply.status,
                );
                emit_cyw43_control_split_completion(
                    contract,
                    stage,
                    "optional-filter-reject-nonmatching",
                    poll,
                    flags,
                    fault,
                    cmd,
                    id,
                    header_mode,
                    expected_response_len,
                    control_iovar,
                    nonmatching_frames,
                    malformed_frames,
                );
                return Err(Cyw43CommandSubmitError::Completion(fault));
            }
            if cyw43_control_reply_is_host_eapol_wsec_key_commandless_reject(
                stage,
                control_iovar,
                reply,
            ) {
                let fault = cyw43_control_fault_completion(
                    completion.sequence,
                    CYW43_CONTROL_EXCHANGE_FAULT_DETAIL,
                    reply.status,
                );
                emit_cyw43_control_split_completion(
                    contract,
                    stage,
                    "wsec-key-commandless-reject",
                    poll,
                    flags,
                    fault,
                    cmd,
                    id,
                    header_mode,
                    expected_response_len,
                    control_iovar,
                    nonmatching_frames,
                    malformed_frames,
                );
                return Err(Cyw43CommandSubmitError::Completion(fault));
            }
            if !cyw43_control_reply_is_commandless_reject(reply) {
                let _ = store_cyw43_pending_control_reply(frame_flags, token, completion.sequence);
            }
            continue;
        }
        let response_completion = cyw43_control_response_completion_from_reply(
            contract,
            stage,
            exchange_descriptor,
            completion,
            reply,
            cmd,
            id,
            header_mode,
            expected_response_len,
            control_iovar,
            nonmatching_frames,
            malformed_frames,
        )?;
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
    emit_cyw43_control_deadline_trace(
        contract,
        stage,
        "timeout",
        timeout_ms,
        poll_attempts,
        poll.saturating_sub(1),
        &deadline,
        control_iovar,
    );
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
fn cyw43_control_response_completion_from_reply(
    contract: DriverTaskContract,
    stage: &'static str,
    exchange_descriptor: DriverRuntimeCyw43CommandDescriptor,
    completion: DriverTaskCompletionRecord,
    reply: Cyw43ControlReply,
    cmd: u32,
    id: u16,
    header_mode: Cyw43ControlHeaderMode,
    expected_response_len: u16,
    control_iovar: &str,
    nonmatching_frames: u32,
    malformed_frames: u32,
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
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
    Ok(response_completion)
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
        DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_SOURCE_ASSERTED_EMPTY => 14,
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
    if le_u32_at(bytes, 0)? == SDIO_FAULT_TELEMETRY_MAGIC {
        return None;
    }
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
    if matches!(poll, 1 | 4 | 16 | 64 | 256 | 1024 | 4096) {
        DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD
    } else {
        0
    }
}

#[cfg(feature = "kernel")]
fn cyw43_control_exchange_poll_attempts(stage: &'static str, control_iovar: &str) -> usize {
    if cyw43_control_uses_post_secure_host_eapol_wsec_key_reply_window(stage, control_iovar) {
        CYW43_HOST_EAPOL_POST_SECURE_WSEC_KEY_POLL_ATTEMPTS
    } else if cyw43_control_uses_host_eapol_wsec_key_reply_window(stage, control_iovar) {
        CYW43_HOST_EAPOL_WSEC_KEY_POLL_ATTEMPTS
    } else if cyw43_control_uses_host_eapol_rx_admission_reply_window(stage, control_iovar) {
        CYW43_HOST_EAPOL_RX_ADMISSION_POLL_ATTEMPTS
    } else {
        CYW43_CONTROL_PLANE_POLL_ATTEMPTS
    }
}

#[cfg(feature = "kernel")]
fn cyw43_control_exchange_timeout_ms(stage: &'static str, control_iovar: &str) -> u64 {
    if cyw43_control_uses_post_secure_host_eapol_wsec_key_reply_window(stage, control_iovar) {
        CYW43_HOST_EAPOL_POST_SECURE_WSEC_KEY_REPLY_TIMEOUT_MS
    } else if cyw43_control_uses_host_eapol_wsec_key_reply_window(stage, control_iovar) {
        CYW43_HOST_EAPOL_WSEC_KEY_REPLY_TIMEOUT_MS
    } else if cyw43_control_uses_host_eapol_rx_admission_reply_window(stage, control_iovar) {
        CYW43_HOST_EAPOL_RX_ADMISSION_REPLY_TIMEOUT_MS
    } else {
        CYW43_CONTROL_PLANE_REPLY_TIMEOUT_MS
    }
}

#[cfg(feature = "kernel")]
fn cyw43_control_uses_host_eapol_wsec_key_reply_window(
    stage: &'static str,
    control_iovar: &str,
) -> bool {
    control_iovar == "wsec_key"
        && stage.starts_with("cyw43-host-eapol-")
        && !stage.starts_with("cyw43-host-eapol-post-secure-")
}

#[cfg(feature = "kernel")]
fn cyw43_control_uses_post_secure_host_eapol_wsec_key_reply_window(
    stage: &'static str,
    control_iovar: &str,
) -> bool {
    control_iovar == "wsec_key"
        && matches!(
            stage,
            "cyw43-host-eapol-post-secure-ptk" | "cyw43-host-eapol-post-secure-gtk"
        )
}

#[cfg(feature = "kernel")]
fn cyw43_control_uses_host_eapol_rx_admission_reply_window(
    stage: &'static str,
    control_iovar: &str,
) -> bool {
    cyw43_control_stage_is_host_eapol_rx_admission(stage)
        && matches!(control_iovar, "mcast_list" | "allmulti" | "none")
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cyw43PollDeadline {
    Counter { start: u64, cycles: u64 },
    Polls { remaining: usize },
}

#[cfg(feature = "kernel")]
const fn cyw43_millis_to_cycles_at_hz(ms: u64, freq_hz: u64) -> u64 {
    if ms == 0 || freq_hz == 0 {
        return 0;
    }
    let cycles = (ms as u128)
        .saturating_mul(freq_hz as u128)
        .saturating_div(1_000u128);
    if cycles == 0 {
        1
    } else if cycles > u64::MAX as u128 {
        u64::MAX
    } else {
        cycles as u64
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "timers-arch-counter",
    target_arch = "aarch64",
    target_os = "none"
))]
fn cyw43_counter_ticks() -> Option<u64> {
    let ticks = crate::arch::aarch64::timer::timer_counter_ticks();
    (ticks != 0).then_some(ticks)
}

#[cfg(not(all(
    feature = "kernel",
    feature = "timers-arch-counter",
    target_arch = "aarch64",
    target_os = "none"
)))]
fn cyw43_counter_ticks() -> Option<u64> {
    None
}

#[cfg(all(
    feature = "kernel",
    feature = "timers-arch-counter",
    target_arch = "aarch64",
    target_os = "none"
))]
fn cyw43_counter_freq_hz() -> Option<u64> {
    let freq_hz = crate::arch::aarch64::timer::timer_freq_hz();
    (freq_hz != 0).then_some(freq_hz)
}

#[cfg(not(all(
    feature = "kernel",
    feature = "timers-arch-counter",
    target_arch = "aarch64",
    target_os = "none"
)))]
fn cyw43_counter_freq_hz() -> Option<u64> {
    None
}

#[cfg(feature = "kernel")]
fn cyw43_poll_deadline_from_millis_or_polls(ms: u64, fallback_polls: usize) -> Cyw43PollDeadline {
    match (cyw43_counter_ticks(), cyw43_counter_freq_hz()) {
        (Some(start), Some(freq_hz)) => {
            let cycles = cyw43_millis_to_cycles_at_hz(ms, freq_hz);
            if cycles == 0 {
                Cyw43PollDeadline::Polls {
                    remaining: fallback_polls,
                }
            } else {
                Cyw43PollDeadline::Counter { start, cycles }
            }
        }
        _ => Cyw43PollDeadline::Polls {
            remaining: fallback_polls,
        },
    }
}

#[cfg(feature = "kernel")]
fn cyw43_poll_deadline_open(deadline: &mut Cyw43PollDeadline) -> bool {
    match deadline {
        Cyw43PollDeadline::Counter { start, cycles } => cyw43_counter_ticks()
            .map(|current| current.wrapping_sub(*start) < *cycles)
            .unwrap_or(false),
        Cyw43PollDeadline::Polls { remaining } => {
            if *remaining == 0 {
                false
            } else {
                *remaining = (*remaining).saturating_sub(1);
                true
            }
        }
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_poll_deadline_trace_fields(
    deadline: &Cyw43PollDeadline,
) -> (&'static str, u64, usize) {
    match *deadline {
        Cyw43PollDeadline::Counter { cycles, .. } => ("counter", cycles, 0),
        Cyw43PollDeadline::Polls { remaining } => ("polls", 0, remaining),
    }
}

#[cfg(feature = "kernel")]
fn emit_cyw43_control_deadline_trace(
    contract: DriverTaskContract,
    stage: &'static str,
    event: &'static str,
    timeout_ms: u64,
    poll_limit: usize,
    poll: usize,
    deadline: &Cyw43PollDeadline,
    control_iovar: &str,
) {
    use core::fmt::Write;

    let (backend, cycles, remaining_polls) = cyw43_poll_deadline_trace_fields(deadline);
    let mut line = heapless::String::<384>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_CONTROL_DEADLINE contract={} stage={} event={} backend={} timeout_ms={} poll_limit={} poll={} cycles={} remaining_polls={} iovar={}",
        contract.name,
        stage,
        event,
        backend,
        timeout_ms,
        poll_limit,
        poll,
        cycles,
        remaining_polls,
        control_iovar,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
const fn cyw43_control_split_poll_completion_should_trace(
    poll: usize,
    poll_attempts: usize,
    flags: u16,
    completion: DriverTaskCompletionRecord,
) -> bool {
    flags & DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD != 0
        || completion.code != DriverTaskCompletionCode::Idle.as_u16()
        || poll == poll_attempts
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
        control_cmd: expected_cmd,
        control_id: expected_id,
        control_header_mode: header_mode.as_str(),
        control_response_len: expected_response_len,
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
        emit_cyw43_sdio_owner_fault_snapshot(contract, stage, descriptor, completion, None);
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
        let token_len = token.len;
        let frame_channel = cyw43_frame_channel(flags);
        if frame_channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT {
            let _ = cyw43_capture_event_frame_from_token(
                CYW43_WIFI_DRIVER_TASK_CONTRACT,
                stage,
                flags,
                &token,
            );
        } else if frame_channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL {
            let _ = store_cyw43_pending_control_reply(flags, token, 0);
        }
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::Cyw43Wifi,
            stage,
            "frame",
            None,
        );
        if token_len != 0 {
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
    CYW43_HOST_EAPOL_M1.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_M2.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_M3.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_M4.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_PTK.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_GTK.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_ACTIVE.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_REQUIRED.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);
    CYW43_POST_SECURE_DATA_RX_ADMITTED.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_TX_RETRIES.store(0, Ordering::Release);
    CYW43_PRIMARY_BSSCFG_JOIN_READY.store(0, Ordering::Release);
    *CYW43_HOST_EAPOL_SESSION.lock() = None;
    *CYW43_HOST_EAPOL_PENDING_EVENT.lock() = None;
    clear_cyw43_active_prompt_poll();
    clear_cyw43_pending_control_replies();
    clear_cyw43_host_eapol_status_throttle();
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
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::SdioHost,
            "sdio-owner-state",
            "ready",
            None,
        );
        emit_sdio_driver_task_replay_status("owner-state", "ready");
        crate::hal::driver_task::emit_owner_state_transition_boot_contract_proof(
            DriverTaskHotPath::SdioHost,
        );
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
const fn cyw43_sdio_host_reprime_descriptor() -> DriverRuntimeSdioCommandDescriptor {
    DriverRuntimeSdioCommandDescriptor {
        op: DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG,
        response_kind: DRIVER_RUNTIME_SDIO_RESP_NONE,
        addr: CYW43_SDIO_HOST_REPRIME_CLOCK_HZ,
        flags: DriverRuntimeSdioCommandDescriptor::FLAG_HOST_BUS_WIDTH_4BIT
            | DriverRuntimeSdioCommandDescriptor::FLAG_HOST_HIGH_SPEED,
        timeout_us: CYW43_SDIO_HOST_REPRIME_TIMEOUT_US,
        ..DriverRuntimeSdioCommandDescriptor::empty()
    }
}

#[cfg(feature = "kernel")]
fn recover_sdio_host_config_for_cyw43_tx_retry(
    stage: &'static str,
) -> Result<(), DriverTaskNetError> {
    let contract = SDIO_HOST_DRIVER_TASK_CONTRACT;
    let mut scratch = [0u8; core::mem::size_of::<DriverRuntimeSdioCommandDescriptor>()];
    encode_sdio_descriptor(&mut scratch, cyw43_sdio_host_reprime_descriptor());
    let Some(frame) = crate::hal::driver_task::describe_driver_task_ring_frame(&scratch, 0) else {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::SdioHost,
            stage,
            "stage-failed",
            None,
        );
        return Err(DriverTaskNetError::RuntimeInit("sdio-host-config-stage"));
    };
    let command = DriverTaskCommandRecord::pi4_hot_path(
        0,
        DriverTaskHotPath::SdioHost,
        DriverTaskBudgetGrant::from_contract(contract),
        frame,
    );
    let staging_segments = [DriverTaskStagingSegment::ring_frame(&scratch, 0)];
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::SdioHost,
        stage,
        "begin",
        None,
    );
    let completion = run_driver_task_net_service_staged(contract, command, &staging_segments);
    let ready = completion.is_some_and(|completion| {
        completion.code == DriverTaskCompletionCode::Progress.as_u16() && completion.result != 0
    });
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::SdioHost,
        stage,
        if ready { "ready" } else { "failed" },
        completion,
    );
    if ready {
        Ok(())
    } else {
        Err(DriverTaskNetError::RuntimeInit("sdio-host-config-reprime"))
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
        DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME => "cyw43-control-frame",
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
const fn cyw43_runtime_command_completion_is_quiet_expected(
    op: u16,
    completion: DriverTaskCompletionRecord,
) -> bool {
    cyw43_runtime_descriptor_quiet_hot_path(op)
        && completion.code == DriverTaskCompletionCode::Idle.as_u16()
}

#[cfg(feature = "kernel")]
const fn cyw43_runtime_command_uses_shared_payload(op: u16) -> bool {
    matches!(
        op,
        DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK
            | DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK
            | DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
            | DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
            | DRIVER_RUNTIME_CYW43_OP_ETH_TX
    )
}

#[cfg(feature = "kernel")]
fn submit_cyw43_runtime_command_checked(
    contract: DriverTaskContract,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    payload: &[u8],
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
    let stage = cyw43_runtime_command_stage(descriptor.op);
    submit_cyw43_runtime_command_checked_with_stage(contract, descriptor, payload, stage)
}

#[cfg(feature = "kernel")]
fn submit_cyw43_runtime_command_checked_with_stage(
    contract: DriverTaskContract,
    mut descriptor: DriverRuntimeCyw43CommandDescriptor,
    payload: &[u8],
    stage: &'static str,
) -> Result<DriverTaskCompletionRecord, Cyw43CommandSubmitError> {
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
        } else if cyw43_runtime_command_completion_is_quiet_expected(descriptor.op, completion) {
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
        DRIVER_RUNTIME_CYW43_OP_RELEASE => CYW43_RUNTIME_FIRMWARE_RELEASE_NO_REPLY_RESUMES,
        DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE => CYW43_RUNTIME_CONTROL_EXCHANGE_NO_REPLY_RESUMES,
        DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME => CYW43_RUNTIME_CONTROL_FRAME_NO_REPLY_RESUMES,
        DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL => CYW43_RUNTIME_CONTROL_POLL_NO_REPLY_RESUMES,
        DRIVER_RUNTIME_CYW43_OP_RX_POLL => CYW43_RUNTIME_DATA_POLL_NO_REPLY_RESUMES,
        DRIVER_RUNTIME_CYW43_OP_ETH_TX => CYW43_RUNTIME_DATA_TX_NO_REPLY_RESUMES,
        _ => 0,
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_runtime_descriptor_uses_prompt_slice(op: u16) -> bool {
    matches!(
        op,
        DRIVER_RUNTIME_CYW43_OP_ETH_TX
            | DRIVER_RUNTIME_CYW43_OP_RX_POLL
            | DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL
            | DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
            | DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
    )
}

#[cfg(feature = "kernel")]
const fn cyw43_runtime_descriptor_quiet_hot_path(op: u16) -> bool {
    matches!(
        op,
        DRIVER_RUNTIME_CYW43_OP_ETH_TX
            | DRIVER_RUNTIME_CYW43_OP_RX_POLL
            | DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL
            | DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
    )
}

#[cfg(feature = "kernel")]
const fn cyw43_runtime_descriptor_blocks_net_pre_poll(op: u16) -> bool {
    cyw43_runtime_descriptor_uses_prompt_slice(op)
}

#[cfg(feature = "kernel")]
pub(crate) fn driver_task_runtime_pre_poll_allowed(contract: DriverTaskContract) -> bool {
    !cyw43_active_descriptor_blocks_fresh_net_poll(contract)
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
    let descriptor = cyw43_active_runtime_descriptor_for_request(contract, active_request)?;
    let progress = crate::hal::driver_task::latest_driver_task_ring_progress(contract)?;
    let active_poll =
        cyw43_active_prompt_poll_for_descriptor(active_request, Some(progress), descriptor)?;
    store_cyw43_active_prompt_poll(active_request, descriptor);
    Some(active_poll)
}

#[cfg(feature = "kernel")]
fn cyw43_active_runtime_descriptor(
    contract: DriverTaskContract,
) -> Option<(u32, DriverRuntimeCyw43CommandDescriptor)> {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT {
        return None;
    }
    let active_request = crate::hal::driver_task::active_driver_task_ring_request(contract)
        .and_then(|request| u32::try_from(request).ok())?;
    if active_request == 0 {
        return None;
    }
    cyw43_active_runtime_descriptor_for_request(contract, active_request)
        .map(|descriptor| (active_request, descriptor))
}

#[cfg(feature = "kernel")]
fn cyw43_active_runtime_descriptor_for_request(
    contract: DriverTaskContract,
    active_request: u32,
) -> Option<DriverRuntimeCyw43CommandDescriptor> {
    let command = crate::hal::driver_task::active_driver_task_ring_command(contract)?;
    if active_request == 0
        || command.sequence != active_request
        || command.aux0 != DRIVER_RUNTIME_CYW43_COMMAND_AUX
        || usize::from(command.frame.len)
            != core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>()
    {
        return None;
    }
    let bytes = crate::hal::driver_task::driver_task_ring_frame_bytes(contract, command.frame)?;
    decode_cyw43_descriptor(bytes)
}

#[cfg(feature = "kernel")]
fn cyw43_active_descriptor_blocks_fresh_net_poll(contract: DriverTaskContract) -> bool {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT {
        return false;
    }
    let Some(active_request) = crate::hal::driver_task::active_driver_task_ring_request(contract)
        .and_then(|request| u32::try_from(request).ok())
    else {
        return false;
    };
    if active_request == 0 {
        return false;
    }
    match cyw43_active_runtime_descriptor_for_request(contract, active_request) {
        Some(descriptor) if cyw43_descriptor_op_known(descriptor.op) => {
            cyw43_runtime_descriptor_blocks_net_pre_poll(descriptor.op)
        }
        Some(_) => true,
        None => true,
    }
}

#[cfg(feature = "kernel")]
const fn cyw43_descriptor_op_known(op: u16) -> bool {
    matches!(
        op,
        DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT
            | DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK
            | DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK
            | DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL
            | DRIVER_RUNTIME_CYW43_OP_RELEASE
            | DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
            | DRIVER_RUNTIME_CYW43_OP_ETH_TX
            | DRIVER_RUNTIME_CYW43_OP_RX_POLL
            | DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP
            | DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL
            | DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
    )
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
fn cyw43_prompt_slice_active_descriptor_resume_ready(
    active_request: u32,
    progress: Option<DriverTaskRingProgressSnapshot>,
    expected: DriverRuntimeCyw43CommandDescriptor,
    active: DriverRuntimeCyw43CommandDescriptor,
) -> bool {
    let Some(progress) = progress else {
        return false;
    };
    active_request != 0
        && progress.marker_valid
        && progress.sequence == active_request
        && progress.aux0 == DRIVER_RUNTIME_CYW43_COMMAND_AUX
        && cyw43_runtime_descriptor_uses_prompt_slice(expected.op)
        && expected.op == active.op
        && expected.flags == active.flags
        && expected.target_addr == active.target_addr
        && expected.payload_len == active.payload_len
        && expected.total_len == active.total_len
        && expected.arg0 == active.arg0
        && expected.arg1 == active.arg1
}

#[cfg(feature = "kernel")]
fn cyw43_active_prompt_descriptor_resume_ready(
    contract: DriverTaskContract,
    active_request: u32,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
) -> bool {
    let Some(active_descriptor) =
        cyw43_active_runtime_descriptor_for_request(contract, active_request)
    else {
        return false;
    };
    cyw43_prompt_slice_active_descriptor_resume_ready(
        active_request,
        crate::hal::driver_task::latest_driver_task_ring_progress(contract),
        descriptor,
        active_descriptor,
    )
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
        || cyw43_active_prompt_descriptor_resume_ready(contract, active_after, descriptor)
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
    let control_cmd = cyw43_descriptor_control_cmd(descriptor);
    record_cyw43_runtime_command_no_reply_with_control_meta(
        contract,
        stage,
        descriptor,
        resumes,
        control_cmd,
        cyw43_descriptor_control_id(descriptor),
        cyw43_descriptor_control_header_mode(descriptor),
        cyw43_control_request_expected_response_len(control_cmd, None) as u16,
        "none",
    );
}

#[cfg(feature = "kernel")]
fn record_cyw43_runtime_command_no_reply_with_control_meta(
    contract: DriverTaskContract,
    stage: &'static str,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
    resumes: usize,
    control_cmd: u32,
    control_id: u16,
    control_header_mode: &'static str,
    control_response_len: u16,
    control_iovar: &str,
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
        control_cmd,
        control_id,
        control_header_mode,
        control_response_len,
        detail: 0,
        reason: "cyw43-runtime-command-no-reply",
        result: 0,
    });
    *CYW43_LAST_SDIO_OWNER_FAULT.lock() = None;
    let request = crate::hal::driver_task::current_driver_task_ring_request(contract);
    let progress = crate::hal::driver_task::latest_driver_task_ring_progress(contract);
    let mut line = heapless::String::<768>::new();
    match (request, progress) {
        (Some(request), Some(progress)) => {
            let _ = write!(
                line,
                "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract={} stage={} op={} flags=0x{:04x} target=0x{:08x} payload_off={} payload_len={} total_len={} control_cmd={} control_cmd_hex=0x{:08x} control_id={} control_header_mode={} control_response_len={} iovar={} reason=cyw43-runtime-command-no-reply request={} resumes={} progress_marker_valid={} progress_sequence={} progress_phase={} progress_phase_name={} progress_aux0=0x{:08x}",
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
                control_iovar,
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
                "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract={} stage={} op={} flags=0x{:04x} target=0x{:08x} payload_off={} payload_len={} total_len={} control_cmd={} control_cmd_hex=0x{:08x} control_id={} control_header_mode={} control_response_len={} iovar={} reason=cyw43-runtime-command-no-reply request={} resumes={} progress_marker_valid=no progress_sequence=0 progress_phase=0 progress_phase_name=none progress_aux0=0x00000000",
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
                control_iovar,
                request,
                resumes,
            );
        }
        (None, Some(progress)) => {
            let _ = write!(
                line,
                "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract={} stage={} op={} flags=0x{:04x} target=0x{:08x} payload_off={} payload_len={} total_len={} control_cmd={} control_cmd_hex=0x{:08x} control_id={} control_header_mode={} control_response_len={} iovar={} reason=cyw43-runtime-command-no-reply request=none resumes={} progress_marker_valid={} progress_sequence={} progress_phase={} progress_phase_name={} progress_aux0=0x{:08x}",
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
                control_iovar,
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
                "CYW43_DRIVER_TASK_COMMAND_NO_REPLY contract={} stage={} op={} flags=0x{:04x} target=0x{:08x} payload_off={} payload_len={} total_len={} control_cmd={} control_cmd_hex=0x{:08x} control_id={} control_header_mode={} control_response_len={} iovar={} reason=cyw43-runtime-command-no-reply request=none resumes={} progress_marker_valid=no progress_sequence=0 progress_phase=0 progress_phase_name=none progress_aux0=0x00000000",
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
                control_iovar,
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
fn cyw43_optional_control_stage_allows_quiet_fault(stage: &'static str) -> bool {
    matches!(
        stage,
        "cyw43-control-ulp-sdioctrl"
            | "cyw43-control-prejoin-txbf"
            | "cyw43-control-rsn-mfp"
            | "cyw43-control-connect-arp-ol"
            | "cyw43-control-connect-arpoe"
            | "cyw43-control-connect-ndoe"
    )
}

#[cfg(feature = "kernel")]
fn cyw43_runtime_command_fault_uart_trace_enabled(
    stage: &'static str,
    completion: DriverTaskCompletionRecord,
) -> bool {
    if !cyw43_optional_control_stage_allows_quiet_fault(stage) {
        return true;
    }
    !(cyw43_control_exchange_completion_is_unsupported(completion)
        || cyw43_fault_detail_allows_same_command_retry(completion.detail))
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
    if !cyw43_runtime_command_fault_uart_trace_enabled(stage, completion) {
        return;
    }
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

    const fn cmd53_count(self) -> u16 {
        if self.cmd != 53 {
            return 0;
        }
        let count = (self.arg & 0x1ff) as u16;
        if count == 0 {
            SDIO_CMD53_BYTE_MODE_MAX
        } else {
            count
        }
    }

    const fn cmd53_descriptor_block_count(self) -> u16 {
        if self.cmd == 53 && self.cmd53_block_mode() {
            self.cmd53_count()
        } else {
            0
        }
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
        cmd53_count: snapshot.cmd53_count(),
        desc_block_count: snapshot.cmd53_descriptor_block_count(),
        host_block_count: snapshot.block_count,
        transfer_mode: snapshot.transfer_mode,
        host_control: snapshot.host_control,
        host_mode: sdio_host_control_mode_label(snapshot.host_control),
        power_control: snapshot.power_control,
        clock_control: snapshot.clock_control,
        clock_state: sdio_clock_control_state_label(snapshot.clock_control),
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
    let mut line = heapless::String::<1024>::new();
    let _ = write!(
        line,
        "CYW43_SDIO_OWNER_FAULT contract={} stage={} op={} cmd={} arg=0x{:08x} fn={} win=0x{:05x} target=0x{:08x} effective=0x{:08x} chunk_off={} payload_off={} inc={} write={} mode={} len={} blksz={} blkcnt={} cmd53_count={} desc_blkcnt={} host_blkcnt={} tm=0x{:04x} host=0x{:02x} host_mode={} power=0x{:02x} clock=0x{:04x} clock_state={} present=0x{:08x} int=0x{:08x} resp0=0x{:08x} blkreg=0x{:08x} detail=0x{:04x} reason={} xfer_stage={} xfer_status=0x{:06x} xfer_reason={} r5=0x{:04x} owner_window={} retry={}",
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
        snapshot.cmd53_count(),
        snapshot.cmd53_descriptor_block_count(),
        snapshot.block_count,
        snapshot.transfer_mode,
        snapshot.host_control,
        sdio_host_control_mode_label(snapshot.host_control),
        snapshot.power_control,
        snapshot.clock_control,
        sdio_clock_control_state_label(snapshot.clock_control),
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
const fn sdio_host_control_mode_label(host_control: u8) -> &'static str {
    let wide = host_control & SDHCI_HOST_CONTROL_4BIT != 0;
    let high_speed = host_control & SDHCI_HOST_CONTROL_HIGH_SPEED != 0;
    if wide && high_speed {
        "4bit+high-speed"
    } else if wide {
        "4bit"
    } else if high_speed {
        "1bit+high-speed"
    } else {
        "1bit"
    }
}

#[cfg(feature = "kernel")]
const fn sdio_clock_control_state_label(clock_control: u16) -> &'static str {
    let internal = clock_control & SDHCI_CLOCK_INT_EN != 0;
    let stable = clock_control & SDHCI_CLOCK_INT_STABLE != 0;
    let card = clock_control & SDHCI_CLOCK_CARD_EN != 0;
    if internal && stable && card {
        "internal+stable+card"
    } else if internal && stable {
        "internal+stable"
    } else if internal && card {
        "internal+unstable+card"
    } else if internal {
        "internal"
    } else if card {
        "card-no-internal"
    } else {
        "off"
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

#[cfg(any(test, feature = "kernel"))]
fn encode_sdio_descriptor(out: &mut [u8], descriptor: DriverRuntimeSdioCommandDescriptor) {
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
    if matches!(
        command.aux0,
        DRIVER_RUNTIME_ENGINE_INIT_AUX | DRIVER_RUNTIME_NET_INIT_AUX
    ) {
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

fn cyw43_secure_carrier_ready() -> bool {
    CYW43_ASSOCIATED.load(Ordering::Acquire) != 0
        && CYW43_LINK_UP.load(Ordering::Acquire) != 0
        && CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) != 0
}

fn cyw43_data_plane_ready() -> bool {
    cyw43_secure_carrier_ready() && cyw43_post_secure_data_rx_admitted()
}

fn cyw43_driver_task_bringup_status_label() -> Option<&'static str> {
    if !runtime_ready(DriverTaskHotPath::Cyw43Wifi) {
        return Some(DRIVER_TASK_NET_STATUS);
    }
    if cyw43_data_plane_ready() {
        return None;
    }
    if cyw43_secure_carrier_ready() {
        return Some("wifi-data-rx-admission-blocked");
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

fn cyw43_wifi_credential_warning_from_progress(
    progress: &Cyw43HostEapolProgress,
) -> Option<&'static str> {
    if let Some(error) = progress.eapol_error {
        return match error {
            "host-eapol-m3-mic" | "host-eapol-group-mic" => Some(error),
            _ => None,
        };
    }
    if progress.associated {
        return None;
    }
    if progress.auth_timeout_seen {
        return Some("cyw43-association-auth-timeout");
    }
    if progress.set_ssid_failure_seen {
        return Some("cyw43-association-set-ssid-failed");
    }
    if progress.assoc_probe_not_associated {
        return Some("cyw43-association-not-associated");
    }
    if progress.polls >= CYW43_HOST_EAPOL_JOIN_POLLS as u32
        && progress.event_rx == 0
        && progress.data_rx == 0
    {
        return Some("cyw43-association-event-missing");
    }
    None
}

/// Return only Wi-Fi credential or SSID evidence suitable for operator warnings.
#[cfg(feature = "kernel")]
pub(crate) fn latest_cyw43_wifi_credential_warning() -> Option<&'static str> {
    if cyw43_data_plane_ready() {
        return None;
    }
    let guard = CYW43_HOST_EAPOL_SESSION.lock();
    let session = guard.as_ref()?;
    cyw43_wifi_credential_warning_from_progress(&session.progress)
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

#[cfg(feature = "kernel")]
struct DriverTaskNetPendingRxToken {
    flags: u16,
    token: DriverTaskNetRxToken,
}

#[cfg(feature = "kernel")]
const CYW43_PENDING_RX_QUEUE_CAP: usize = 32;
#[cfg(feature = "kernel")]
static CYW43_PENDING_RX_QUEUE: Mutex<
    heapless::Deque<DriverTaskNetPendingRxToken, CYW43_PENDING_RX_QUEUE_CAP>,
> = Mutex::new(heapless::Deque::new());

#[cfg(feature = "kernel")]
struct DriverTaskNetPendingControlReply {
    flags: u16,
    sequence: u32,
    cmd: u32,
    id: u16,
    token: DriverTaskNetRxToken,
}

#[cfg(feature = "kernel")]
const CYW43_PENDING_CONTROL_REPLY_QUEUE_CAP: usize = 4;
#[cfg(feature = "kernel")]
static CYW43_PENDING_CONTROL_REPLY_QUEUE: Mutex<
    heapless::Deque<DriverTaskNetPendingControlReply, CYW43_PENDING_CONTROL_REPLY_QUEUE_CAP>,
> = Mutex::new(heapless::Deque::new());
#[cfg(feature = "kernel")]
// Match the linked Genet runtime's one-turn drain budget so pre-poll bursts do
// not silently lose half of a full runtime drain.
const GENET_PENDING_RX_QUEUE_CAP: usize = 16;
#[cfg(feature = "kernel")]
static GENET_PENDING_RX_QUEUE: Mutex<
    heapless::Deque<DriverTaskNetRxToken, GENET_PENDING_RX_QUEUE_CAP>,
> = Mutex::new(heapless::Deque::new());

fn driver_task_ethertype(frame: &[u8]) -> Option<u16> {
    let ethertype = frame.get(12..14)?;
    Some(u16::from_be_bytes([ethertype[0], ethertype[1]]))
}

fn driver_task_arp_rx_counter(hot_path: DriverTaskHotPath) -> Option<&'static AtomicU32> {
    match hot_path {
        DriverTaskHotPath::GenetNic => Some(&GENET_ARP_RX),
        DriverTaskHotPath::Cyw43Wifi => Some(&CYW43_ARP_RX),
        _ => None,
    }
}

fn driver_task_arp_tx_counter(hot_path: DriverTaskHotPath) -> Option<&'static AtomicU32> {
    match hot_path {
        DriverTaskHotPath::GenetNic => Some(&GENET_ARP_TX),
        DriverTaskHotPath::Cyw43Wifi => Some(&CYW43_ARP_TX),
        _ => None,
    }
}

fn record_driver_task_arp_rx(hot_path: DriverTaskHotPath, frame: &[u8]) {
    if driver_task_ethertype(frame) == Some(CYW43_ETH_P_ARP) {
        if let Some(counter) = driver_task_arp_rx_counter(hot_path) {
            counter.fetch_add(1, Ordering::AcqRel);
        }
    }
}

fn record_driver_task_arp_tx(hot_path: DriverTaskHotPath, frame: &[u8]) {
    if driver_task_ethertype(frame) == Some(CYW43_ETH_P_ARP) {
        if let Some(counter) = driver_task_arp_tx_counter(hot_path) {
            counter.fetch_add(1, Ordering::AcqRel);
        }
    }
}

fn driver_task_arp_counts(hot_path: DriverTaskHotPath) -> (u64, u64) {
    (
        driver_task_arp_rx_counter(hot_path)
            .map(|counter| counter.load(Ordering::Acquire) as u64)
            .unwrap_or_default(),
        driver_task_arp_tx_counter(hot_path)
            .map(|counter| counter.load(Ordering::Acquire) as u64)
            .unwrap_or_default(),
    )
}

impl phy::RxToken for DriverTaskNetRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        NET_DIAG.record_smoltcp_rx();
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
        if frame_len != 0 {
            NET_DIAG.record_smoltcp_tx();
        }
        let result = f(&mut scratch[..frame_len]);
        if submit_driver_task_frame(self.contract, self.hot_path, &scratch[..frame_len]) {
            if self.hot_path != DriverTaskHotPath::Cyw43Wifi {
                self.tx_submitted.fetch_add(1, Ordering::AcqRel);
            }
            record_driver_task_arp_tx(self.hot_path, &scratch[..frame_len]);
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
    let completion = run_driver_task_net_service_staged(contract, command, &staging_segments);
    if hot_path == DriverTaskHotPath::GenetNic {
        if let Some(completion) = completion {
            record_genet_runtime_completion(completion);
        }
    }
    completion.is_some_and(driver_task_tx_completion_submitted)
}

#[cfg(feature = "kernel")]
fn driver_task_tx_completion_submitted(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    completion.code == DriverTaskCompletionCode::Progress.as_u16() && completion.result != 0
}

#[cfg(feature = "kernel")]
fn cyw43_tx_retry_completion_progressed(completion: &DriverTaskCompletionRecord) -> bool {
    completion.code == DriverTaskCompletionCode::FrameReady.as_u16()
        || (completion.code == DriverTaskCompletionCode::Progress.as_u16()
            && completion.result != 0)
        || completion.frame.flags != 0
}

#[cfg(feature = "kernel")]
fn cyw43_tx_retry_should_yield(
    completion: Option<DriverTaskCompletionRecord>,
    retry_poll_progress: bool,
) -> bool {
    if retry_poll_progress {
        return false;
    }
    match completion {
        Some(completion) => {
            if completion.code == DriverTaskCompletionCode::Fault.as_u16()
                && cyw43_fault_detail_allows_same_command_retry(completion.detail)
            {
                return false;
            }
            completion.code == DriverTaskCompletionCode::Idle.as_u16()
                || completion.code == DriverTaskCompletionCode::BudgetExhausted.as_u16()
                || completion.code == DriverTaskCompletionCode::Fault.as_u16()
        }
        None => true,
    }
}

#[cfg(feature = "kernel")]
fn cyw43_data_tx_retry_recovery_poll_budget(
    completion: Option<DriverTaskCompletionRecord>,
) -> usize {
    if completion.is_some_and(|completion| {
        completion.code == DriverTaskCompletionCode::Fault.as_u16()
            && cyw43_fault_detail_allows_same_command_retry(completion.detail)
    }) {
        CYW43_DATA_TX_RETRY_RECOVERY_POLLS
    } else {
        1
    }
}

#[cfg(feature = "kernel")]
fn resume_cyw43_data_tx_retry_recovery(
    contract: DriverTaskContract,
    completion: Option<DriverTaskCompletionRecord>,
) -> bool {
    let mut progressed = false;
    let poll_budget = cyw43_data_tx_retry_recovery_poll_budget(completion);
    for _ in 0..poll_budget {
        if resume_cyw43_active_prompt_poll_for_tx_retry(contract) {
            progressed = true;
        }
        if cyw43_pending_rx_token_occupied() || poll_budget == 1 {
            break;
        }
        core::hint::spin_loop();
    }
    progressed
}

#[cfg(feature = "kernel")]
fn cyw43_data_tx_post_submit_credit_recovery(
    contract: DriverTaskContract,
    completion: DriverTaskCompletionRecord,
) -> bool {
    for _ in 0..CYW43_DATA_TX_POST_SUBMIT_RECOVERY_POLLS {
        let _ = resume_cyw43_active_prompt_poll_for_tx_retry(contract);
        if cyw43_data_tx_credit_proven(contract, completion) {
            return true;
        }
        if cyw43_pending_rx_token_occupied() {
            break;
        }
        core::hint::spin_loop();
    }
    false
}

#[cfg(feature = "kernel")]
fn update_atomic_u32_max(counter: &AtomicU32, value: u32) {
    let mut observed = counter.load(Ordering::Acquire);
    while value > observed {
        match counter.compare_exchange(observed, value, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(next) => observed = next,
        }
    }
}

#[cfg(feature = "kernel")]
fn record_genet_runtime_completion(completion: DriverTaskCompletionRecord) {
    if completion.code == DriverTaskCompletionCode::Progress.as_u16() {
        record_genet_tx_window(
            completion.detail,
            completion.frame.len,
            completion.frame.flags,
        );
    } else if completion.code == DriverTaskCompletionCode::Idle.as_u16() {
        record_genet_rx_runtime_backlog(completion.result);
        record_genet_tx_window(
            completion.detail,
            completion.frame.len,
            completion.frame.flags,
        );
    } else if completion.code == DriverTaskCompletionCode::FrameReady.as_u16() {
        record_genet_rx_runtime_backlog(completion.result);
        if let Some((tx_free, tx_in_flight)) = genet_frame_ready_tx_window(completion) {
            record_genet_tx_window(completion.detail, tx_free, tx_in_flight);
        }
        GENET_RX_HW_FRAMES.fetch_add(1, Ordering::AcqRel);
        GENET_RX_LAST_ETHERTYPE.store(u32::from(completion.frame.flags), Ordering::Release);
        GENET_RX_LAST_LEN.store(u32::from(completion.frame.len), Ordering::Release);
    }
}

#[cfg(feature = "kernel")]
fn record_cyw43_runtime_completion(
    contract: DriverTaskContract,
    completion: DriverTaskCompletionRecord,
) {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT {
        return;
    }
    record_cyw43_completion_credit_accounting(completion);
    let Some(trace) = cyw43_completion_rx_idle_trace(contract, completion) else {
        return;
    };
    CYW43_RX_RUNTIME_QUEUE_COUNT.store(u32::from(trace.queue_depth), Ordering::Release);
    update_atomic_u32_max(
        &CYW43_RX_RUNTIME_QUEUE_HIGH_WATER,
        u32::from(trace.queue_high_water),
    );
    update_atomic_u32_max(
        &CYW43_RX_RUNTIME_MAX_DRAINED_PER_TURN,
        u32::from(trace.rx_max_drained_per_turn),
    );
    if trace.rx_drain_budget_hits != 0 {
        CYW43_RX_RUNTIME_DRAIN_BUDGET_HIT.store(1, Ordering::Release);
    }
    if trace.rx_queue_overflows != 0 {
        CYW43_RX_RUNTIME_QUEUE_OVERFLOW_SEEN.store(1, Ordering::Release);
    }
}

#[cfg(feature = "kernel")]
fn record_genet_tx_window(completed: u16, tx_free: u16, tx_in_flight: u16) {
    if completed != 0 {
        GENET_TX_HW_COMPLETED.fetch_add(u32::from(completed), Ordering::AcqRel);
    }
    GENET_TX_HW_FREE.store(u32::from(tx_free), Ordering::Release);
    GENET_TX_HW_IN_FLIGHT.store(u32::from(tx_in_flight), Ordering::Release);
}

#[cfg(feature = "kernel")]
fn record_genet_rx_runtime_backlog(result: u32) {
    if !driver_runtime_genet_result_is_packed(result) {
        return;
    }
    GENET_RX_RUNTIME_QUEUE_COUNT.store(
        u32::from(driver_runtime_genet_result_rx_queue_count(result)),
        Ordering::Release,
    );
    GENET_RX_RUNTIME_QUEUE_HIGH_WATER.store(
        u32::from(driver_runtime_genet_result_rx_queue_high_water(result)),
        Ordering::Release,
    );
    GENET_RX_RUNTIME_MAX_DRAINED_PER_TURN.store(
        u32::from(driver_runtime_genet_result_rx_max_drained_per_turn(result)),
        Ordering::Release,
    );
    if driver_runtime_genet_result_rx_drain_budget_hit(result) {
        GENET_RX_RUNTIME_DRAIN_BUDGET_HIT.store(1, Ordering::Release);
    }
    if driver_runtime_genet_result_rx_byte_budget_hit(result) {
        GENET_RX_RUNTIME_BYTE_BUDGET_HIT.store(1, Ordering::Release);
    }
    if driver_runtime_genet_result_rx_overflow_seen(result) {
        GENET_RX_RUNTIME_QUEUE_OVERFLOW_SEEN.store(1, Ordering::Release);
    }
}

#[cfg(feature = "kernel")]
fn genet_frame_ready_tx_window(completion: DriverTaskCompletionRecord) -> Option<(u16, u16)> {
    if driver_runtime_genet_result_is_packed(completion.result) {
        return Some((
            driver_runtime_genet_result_tx_free(completion.result),
            driver_runtime_genet_result_tx_in_flight(completion.result),
        ));
    }
    let tx_free = (completion.result >> 16) as u16;
    let tx_in_flight = completion.result as u16;
    let ring_window = tx_free.saturating_add(tx_in_flight);
    if ring_window <= 64 {
        Some((tx_free, tx_in_flight))
    } else {
        None
    }
}

#[cfg(feature = "kernel")]
pub(crate) fn preserve_driver_task_pre_poll_completion(
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
    completion: DriverTaskCompletionRecord,
) -> bool {
    if hot_path == DriverTaskHotPath::GenetNic {
        record_genet_runtime_completion(completion);
    }
    if hot_path == DriverTaskHotPath::Cyw43Wifi {
        record_cyw43_completion_credit_accounting(completion);
    }
    if completion.code == DriverTaskCompletionCode::Progress.as_u16() {
        return completion.result != 0;
    }
    if completion.code != DriverTaskCompletionCode::FrameReady.as_u16() {
        return false;
    }
    let preserved = match hot_path {
        DriverTaskHotPath::GenetNic => driver_task_rx_token_from_completion(contract, completion)
            .is_some_and(store_genet_pending_rx_token),
        DriverTaskHotPath::Cyw43Wifi => {
            if let Some((flags, token)) =
                cyw43_driver_task_data_frame_with_flags_from_completion(contract, completion)
            {
                if consume_cyw43_post_secure_eapol_frame(contract, flags, &token, "pre-poll") {
                    return true;
                }
                let pending_before = cyw43_pending_rx_token_occupied();
                let stored = cyw43_pending_rx_token_store_possible(flags, &token);
                let pending_after = pending_before || stored;
                let event = if stored {
                    "rx-preserve"
                } else {
                    "rx-preserve-drop"
                };
                emit_cyw43_data_path_trace(
                    contract,
                    event,
                    "pre-poll",
                    0,
                    &token.buffer[..token.len],
                    Some(completion),
                    flags,
                    pending_before,
                    pending_after,
                );
                if stored {
                    store_cyw43_pending_rx_token(flags, token)
                } else {
                    record_cyw43_pending_rx_drop();
                    false
                }
            } else {
                false
            }
        }
        _ => false,
    };
    preserved
}

#[cfg(feature = "kernel")]
fn submit_cyw43_driver_task_eth_frame(contract: DriverTaskContract, frame: &[u8]) -> bool {
    if cyw43_post_dhcp_zero_sender_arp(frame) {
        let pending_rx = cyw43_pending_rx_token_occupied();
        emit_cyw43_data_path_trace(
            contract,
            "tx-drop",
            "invalid-arp-spa",
            0,
            frame,
            None,
            0,
            pending_rx,
            pending_rx,
        );
        return false;
    }
    if !cyw43_fresh_tx_admission_ready(contract) {
        let pending_rx = cyw43_pending_rx_token_occupied();
        emit_cyw43_data_path_trace(
            contract,
            "tx-drop",
            cyw43_tx_no_completion_action(contract),
            0,
            frame,
            None,
            0,
            pending_rx,
            pending_rx,
        );
        return false;
    }
    if !cyw43_tx_unproven_window_ready(contract) {
        let pending_rx = cyw43_pending_rx_token_occupied();
        emit_cyw43_data_path_trace(
            contract,
            "tx-drop",
            "credit-window-busy",
            0,
            frame,
            None,
            0,
            pending_rx,
            pending_rx,
        );
        return false;
    }
    for attempt in 0..CYW43_DATA_TX_ATTEMPTS {
        let completion = submit_cyw43_driver_task_eth_frame_completion(contract, frame);
        let submitted = completion.is_some_and(driver_task_tx_completion_submitted);
        if submitted {
            CYW43_TX_SUBMITTED.fetch_add(1, Ordering::AcqRel);
        }
        let credit_proven = completion.is_some_and(|completion| {
            submitted && cyw43_data_tx_credit_proven(contract, completion)
        });
        let pending_rx = cyw43_pending_rx_token_occupied();
        let action = if credit_proven {
            "credit-proven"
        } else if submitted {
            "credit-unproven"
        } else if completion.is_some() {
            "retry"
        } else {
            cyw43_tx_no_completion_action(contract)
        };
        emit_cyw43_data_path_trace(
            contract,
            "tx-result",
            action,
            attempt + 1,
            frame,
            completion,
            0,
            pending_rx,
            pending_rx,
        );
        if credit_proven {
            clear_cyw43_unproven_tx_window();
            return true;
        }
        if submitted {
            if let Some(completion) = completion {
                if cyw43_data_tx_post_submit_credit_recovery(contract, completion) {
                    clear_cyw43_unproven_tx_window();
                } else {
                    record_cyw43_unproven_tx_window(Some(completion));
                }
            } else {
                record_cyw43_unproven_tx_window(None);
            }
            return true;
        }
        if attempt + 1 != CYW43_DATA_TX_ATTEMPTS {
            CYW43_DATA_TX_RETRIES.fetch_add(1, Ordering::AcqRel);
            let retry_poll_progress = resume_cyw43_data_tx_retry_recovery(contract, completion);
            if cyw43_tx_retry_should_yield(completion, retry_poll_progress) {
                break;
            }
            core::hint::spin_loop();
        }
    }
    false
}

#[cfg(feature = "kernel")]
fn submit_cyw43_driver_task_eth_frame_completion(
    contract: DriverTaskContract,
    frame: &[u8],
) -> Option<DriverTaskCompletionRecord> {
    #[cfg(test)]
    if CYW43_DATA_TX_TEST_STUB.load(Ordering::Acquire) != 0 {
        let _ = contract;
        let attempt = CYW43_DATA_TX_TEST_ATTEMPTS.fetch_add(1, Ordering::AcqRel);
        if attempt < CYW43_DATA_TX_TEST_IDLE_BEFORE_SUCCESS.load(Ordering::Acquire) {
            return Some(DriverTaskCompletionRecord::idle(0));
        }
        if attempt < CYW43_DATA_TX_TEST_FAULTS_BEFORE_SUCCESS.load(Ordering::Acquire) {
            return Some(DriverTaskCompletionRecord {
                sequence: attempt.saturating_add(1),
                code: DriverTaskCompletionCode::Fault.as_u16(),
                detail: CYW43_SDIO_DESCRIPTOR_TRANSFER_FAILED_DETAIL,
                result: 0x0500_0800,
                frame: DriverFrameDescriptor {
                    offset: 0,
                    len: 56,
                    flags: 0,
                },
            });
        }
        if attempt < CYW43_DATA_TX_TEST_FAILS_BEFORE_SUCCESS.load(Ordering::Acquire) {
            return Some(DriverTaskCompletionRecord::progress(0, 0));
        }
        let mut completion = DriverTaskCompletionRecord::progress(0, frame.len() as u32);
        let submitted_seq = (attempt.saturating_add(1) & 0xff) as u16;
        let credit = if CYW43_DATA_TX_TEST_SUCCESS_WITHOUT_CREDIT.load(Ordering::Acquire) == 0 {
            (attempt.saturating_add(2) & 0xff) as u16
        } else {
            submitted_seq
        };
        completion.frame = DriverFrameDescriptor {
            offset: 0,
            len: 1,
            flags: submitted_seq | (credit << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT),
        };
        return Some(completion);
    }
    run_cyw43_runtime_descriptor_command(
        contract,
        DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_ETH_TX,
            payload_len: frame.len() as u16,
            total_len: frame.len() as u32,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        },
        frame,
    )
}

#[cfg(feature = "kernel")]
fn submit_cyw43_host_eapol_payload_bounded(
    contract: DriverTaskContract,
    frame: &[u8],
    stage: &'static str,
) -> bool {
    submit_cyw43_host_eapol_payload_bounded_completion(contract, frame, stage).is_some()
}

#[cfg(feature = "kernel")]
fn submit_cyw43_host_eapol_payload_bounded_completion(
    contract: DriverTaskContract,
    frame: &[u8],
    stage: &'static str,
) -> Option<DriverTaskCompletionRecord> {
    #[cfg(test)]
    if CYW43_HOST_EAPOL_TEST_IO_STUB.load(Ordering::Acquire) != 0 {
        let _ = (contract, stage);
        if !frame.is_empty() {
            CYW43_HOST_EAPOL_TEST_TX_SUBMITTED.fetch_add(1, Ordering::AcqRel);
            CYW43_TX_SUBMITTED.fetch_add(1, Ordering::AcqRel);
            return Some(DriverTaskCompletionRecord::progress(0, frame.len() as u32));
        }
        CYW43_TX_DROPPED.fetch_add(1, Ordering::AcqRel);
        return None;
    }
    for attempt in 0..CYW43_HOST_EAPOL_TX_ATTEMPTS {
        let completion = submit_cyw43_driver_task_eth_frame_completion(contract, frame);
        if completion.is_some_and(driver_task_tx_completion_submitted) {
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
            return completion;
        }
        CYW43_HOST_EAPOL_TX_RETRIES.fetch_add(1, Ordering::AcqRel);
        let _ = resume_cyw43_data_tx_retry_recovery(contract, completion);
        if let Some((flags, token)) = poll_cyw43_driver_task_control_frame() {
            if cyw43_frame_channel(flags) == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL {
                let _ = store_cyw43_pending_control_reply(flags, token, 0);
            }
        }
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
    None
}

#[cfg(feature = "kernel")]
pub(crate) fn submit_cyw43_driver_task_eth_payload(frame: &[u8]) -> bool {
    let submitted = submit_cyw43_driver_task_eth_frame(CYW43_WIFI_DRIVER_TASK_CONTRACT, frame);
    if submitted {
        record_driver_task_arp_tx(DriverTaskHotPath::Cyw43Wifi, frame);
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
    let use_shared_payload =
        !payload.is_empty() && cyw43_runtime_command_uses_shared_payload(descriptor.op);
    let mut payload_staged = false;
    if !payload.is_empty() {
        let staged_payload = if use_shared_payload {
            let shared = crate::hal::driver_task::describe_driver_task_shared_payload(payload, 0)?;
            DriverFrameDescriptor {
                offset: u32::from(shared.offset),
                len: shared.len,
                flags: shared.flags,
            }
        } else {
            let payload_offset = crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET + 512;
            crate::hal::driver_task::describe_driver_task_ring_payload_at(
                payload_offset,
                payload,
                0,
            )?
        };
        descriptor.payload_offset = u16::try_from(staged_payload.offset).ok()?;
        descriptor.payload_len = staged_payload.len;
        if descriptor.total_len == 0 {
            descriptor.total_len = u32::from(staged_payload.len);
        }
        payload_staged = true;
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
    if cyw43_runtime_descriptor_quiet_hot_path(descriptor.op) {
        command.flags |= DRIVER_TASK_RING_FLAG_QUIET_HOT_PATH;
    }
    let use_prompt_slice = cyw43_runtime_descriptor_uses_prompt_slice(descriptor.op);
    let no_reply_resume_limit = if use_prompt_slice {
        cyw43_runtime_no_reply_resume_limit(descriptor.op)
    } else {
        0
    };
    let mut no_reply_resumes = 0usize;
    let mut descriptor_unavailable_retries = 0usize;
    loop {
        let active_before = crate::hal::driver_task::active_driver_task_ring_request(contract)
            .and_then(|request| u32::try_from(request).ok());
        let completion = if payload_staged && use_shared_payload {
            let staging_segments = [
                DriverTaskStagingSegment::shared(payload, 0),
                DriverTaskStagingSegment::ring_payload_at(
                    crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET,
                    &scratch,
                    0,
                ),
            ];
            if use_prompt_slice {
                run_driver_task_net_service_prompt_slice_staged(
                    contract,
                    command,
                    &staging_segments,
                )
            } else {
                run_driver_task_net_service_staged(contract, command, &staging_segments)
            }
        } else if payload_staged {
            let payload_offset = crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET + 512;
            let staging_segments = [
                DriverTaskStagingSegment::ring_payload_at(payload_offset, payload, 0),
                DriverTaskStagingSegment::ring_payload_at(
                    crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET,
                    &scratch,
                    0,
                ),
            ];
            if use_prompt_slice {
                run_driver_task_net_service_prompt_slice_staged(
                    contract,
                    command,
                    &staging_segments,
                )
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
                run_driver_task_net_service_prompt_slice_staged(
                    contract,
                    command,
                    &staging_segments,
                )
            } else {
                run_driver_task_net_service_staged(contract, command, &staging_segments)
            }
        };
        record_cyw43_active_prompt_poll(contract, descriptor, active_before, completion);
        if let Some(completion) = completion {
            record_cyw43_runtime_completion(contract, completion);
        }
        if completion.as_ref().is_some_and(|completion| {
            cyw43_descriptor_unavailable_retry_allowed(
                descriptor.op,
                completion,
                descriptor_unavailable_retries,
            )
        }) {
            descriptor_unavailable_retries = descriptor_unavailable_retries.saturating_add(1);
            core::hint::spin_loop();
            continue;
        }
        if completion.is_some()
            || no_reply_resumes >= no_reply_resume_limit
            || !cyw43_prompt_poll_no_reply_resume_ready(contract, descriptor)
        {
            return completion;
        }
        no_reply_resumes = no_reply_resumes.saturating_add(1);
    }
}

#[cfg(feature = "kernel")]
fn cyw43_descriptor_unavailable_retry_allowed(
    op: u16,
    completion: &DriverTaskCompletionRecord,
    retries_spent: usize,
) -> bool {
    retries_spent < CYW43_RUNTIME_DESCRIPTOR_UNAVAILABLE_RETRIES
        && cyw43_runtime_descriptor_quiet_hot_path(op)
        && completion.code == DriverTaskCompletionCode::Fault.as_u16()
        && completion.detail == CYW43_DESCRIPTOR_UNAVAILABLE_DETAIL
}

#[cfg(feature = "kernel")]
fn cyw43_prompt_poll_no_reply_resume_ready(
    contract: DriverTaskContract,
    descriptor: DriverRuntimeCyw43CommandDescriptor,
) -> bool {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT
        || !cyw43_runtime_descriptor_uses_prompt_slice(descriptor.op)
    {
        return false;
    }
    let Some(active_request) = crate::hal::driver_task::active_driver_task_ring_request(contract)
        .and_then(|request| u32::try_from(request).ok())
    else {
        return false;
    };
    if active_request == 0 {
        return false;
    }
    if CYW43_ACTIVE_PROMPT_POLL_REQUEST.load(Ordering::Acquire) == active_request
        && CYW43_ACTIVE_PROMPT_POLL_OP.load(Ordering::Acquire) as u16 == descriptor.op
        && CYW43_ACTIVE_PROMPT_POLL_FLAGS.load(Ordering::Acquire) as u16 == descriptor.flags
    {
        return true;
    }
    if cyw43_active_prompt_descriptor_resume_ready(contract, active_request, descriptor) {
        store_cyw43_active_prompt_poll(active_request, descriptor);
        return true;
    }
    let Some(active_poll) = cyw43_active_prompt_poll_from_ring(contract, active_request) else {
        return false;
    };
    cyw43_host_eapol_poll_kind_for_op(descriptor.op) == Some(active_poll.kind)
        && descriptor.flags == active_poll.flags
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
    if hot_path == DriverTaskHotPath::GenetNic {
        if let Some(token) = take_genet_pending_rx_token() {
            return Some(token);
        }
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
    if hot_path == DriverTaskHotPath::GenetNic {
        record_genet_runtime_completion(completion);
    }
    if completion.code != DriverTaskCompletionCode::FrameReady.as_u16() {
        return None;
    }
    driver_task_rx_token_from_completion(contract, completion)
}

#[cfg(feature = "kernel")]
fn receive_cyw43_driver_task_frame(contract: DriverTaskContract) -> Option<DriverTaskNetRxToken> {
    for _ in 0..CYW43_PENDING_RX_QUEUE_CAP {
        if let Some((flags, token)) = take_cyw43_pending_rx_token() {
            let _ = complete_cyw43_unproven_tx_window_from_rx_flags(flags);
            if consume_cyw43_post_secure_eapol_frame(contract, flags, &token, "pending") {
                continue;
            }
            emit_cyw43_data_path_trace(
                contract,
                "rx-deliver",
                "pending",
                0,
                &token.buffer[..token.len],
                None,
                flags,
                true,
                cyw43_pending_rx_token_occupied(),
            );
            submit_cyw43_arp_assist_if_needed(contract, &token.buffer[..token.len]);
            return Some(token);
        }
        if let Some((flags, token)) = resume_cyw43_active_prompt_poll_for_data_path(contract) {
            if consume_cyw43_post_secure_eapol_frame(contract, flags, &token, "resume") {
                continue;
            }
            emit_cyw43_data_path_trace(
                contract,
                "rx-deliver",
                "resume",
                0,
                &token.buffer[..token.len],
                None,
                flags,
                false,
                false,
            );
            submit_cyw43_arp_assist_if_needed(contract, &token.buffer[..token.len]);
            return Some(token);
        }
        if cyw43_active_descriptor_blocks_fresh_net_poll(contract) {
            return None;
        }
        let completion = run_cyw43_runtime_descriptor_command(
            contract,
            DriverRuntimeCyw43CommandDescriptor {
                op: DRIVER_RUNTIME_CYW43_OP_RX_POLL,
                flags: CYW43_DATA_RX_STEADY_POLL_FLAGS,
                ..DriverRuntimeCyw43CommandDescriptor::empty()
            },
            &[],
        )?;
        let (flags, token) =
            cyw43_driver_task_data_frame_with_flags_from_completion(contract, completion)?;
        if consume_cyw43_post_secure_eapol_frame(contract, flags, &token, "poll") {
            continue;
        }
        emit_cyw43_data_path_trace(
            contract,
            "rx-deliver",
            "poll",
            0,
            &token.buffer[..token.len],
            Some(completion),
            flags,
            false,
            false,
        );
        submit_cyw43_arp_assist_if_needed(contract, &token.buffer[..token.len]);
        return Some(token);
    }
    None
}

#[cfg(feature = "kernel")]
fn consume_cyw43_post_secure_eapol_frame(
    contract: DriverTaskContract,
    flags: u16,
    token: &DriverTaskNetRxToken,
    action: &'static str,
) -> bool {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT
        || CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) == 0
        || cyw43_frame_channel(flags) != DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA
    {
        return false;
    }
    let frame = &token.buffer[..token.len];
    if cyw43_ethertype(frame) != Some(ETH_P_EAPOL) {
        return false;
    }

    emit_cyw43_data_path_trace(
        contract,
        "rx-consume",
        action,
        0,
        frame,
        None,
        flags,
        false,
        false,
    );

    let station_mac = *CYW43_RUNTIME_MAC.lock();
    let mut tx_frame = [0u8; MAX_FRAME_LEN];
    let mut guard = CYW43_HOST_EAPOL_SESSION.lock();
    let Some(session) = guard.as_mut() else {
        return true;
    };
    let poll = session.progress.polls as usize;
    session
        .progress
        .record_data_frame(flags, frame.len(), Some(ETH_P_EAPOL));
    if let Some(proof) = cyw43_host_eapol::inspect_host_eapol_frame(frame) {
        emit_cyw43_host_eapol_proof(contract, "post-secure-rx", poll, frame.len(), proof);
    }

    let eapol_action = match session
        .eapol
        .handle_packet(station_mac.0, frame, &mut tx_frame)
    {
        Ok(action) => action,
        Err(err) => {
            session.progress.record_eapol_error(err);
            emit_cyw43_host_eapol_error(
                contract,
                "post-secure-handle-packet",
                err,
                poll,
                frame.len(),
            );
            emit_cyw43_host_eapol_status(contract, "post-secure-error", &session.progress);
            return true;
        }
    };
    CYW43_HOST_EAPOL_RX.store(session.eapol.rx_packets(), Ordering::Release);

    match eapol_action {
        HostEapolAction::None | HostEapolAction::Inspect { .. } => {}
        HostEapolAction::SendM2 { len } => {
            CYW43_HOST_EAPOL_M1.fetch_add(1, Ordering::AcqRel);
            emit_cyw43_host_eapol_message(contract, "m1", "post-secure-recv-m1", poll, frame.len());
            if submit_cyw43_host_eapol_payload_bounded(
                contract,
                &tx_frame[..len],
                "cyw43-host-eapol-post-secure-m2",
            ) {
                CYW43_HOST_EAPOL_M2.fetch_add(1, Ordering::AcqRel);
                emit_cyw43_host_eapol_message(contract, "m2", "post-secure-send-m2", poll, len);
            } else {
                session
                    .progress
                    .record_eapol_error("host-eapol-post-secure-m2-tx");
            }
        }
        HostEapolAction::SendM4 { len } => {
            CYW43_HOST_EAPOL_M3.fetch_add(1, Ordering::AcqRel);
            emit_cyw43_host_eapol_message(
                contract,
                "m3",
                "post-secure-recv-m3-retransmit",
                poll,
                frame.len(),
            );
            if submit_cyw43_host_eapol_payload_bounded_completion(
                contract,
                &tx_frame[..len],
                "cyw43-host-eapol-post-secure-m4-retransmit",
            )
            .is_some()
            {
                CYW43_HOST_EAPOL_M4.fetch_add(1, Ordering::AcqRel);
                emit_cyw43_host_eapol_message(
                    contract,
                    "m4",
                    "post-secure-send-m4-retransmit",
                    poll,
                    len,
                );
            } else {
                session
                    .progress
                    .record_eapol_error("host-eapol-post-secure-m4-tx");
            }
        }
        HostEapolAction::SendM4InstallKeys { len, keys } => {
            CYW43_HOST_EAPOL_M3.fetch_add(1, Ordering::AcqRel);
            emit_cyw43_host_eapol_message(contract, "m3", "post-secure-recv-m3", poll, frame.len());
            let Some(m4_completion) = submit_cyw43_host_eapol_payload_bounded_completion(
                contract,
                &tx_frame[..len],
                "cyw43-host-eapol-post-secure-m4",
            ) else {
                session
                    .progress
                    .record_eapol_error("host-eapol-post-secure-m4-tx");
                return true;
            };
            CYW43_HOST_EAPOL_M4.fetch_add(1, Ordering::AcqRel);
            emit_cyw43_host_eapol_message(contract, "m4", "post-secure-send-m4", poll, len);
            let m4_drain_ready = match wait_cyw43_host_eapol_tx_drain(
                contract,
                session,
                "post-secure-m4-before-wsec",
                poll,
                m4_completion,
            ) {
                Ok(drained) => drained,
                Err(_) => {
                    session
                        .progress
                        .record_eapol_error("host-eapol-post-secure-m4-drain");
                    emit_cyw43_host_eapol_error(
                        contract,
                        "post-secure-m4-drain",
                        "host-eapol-post-secure-m4-drain",
                        poll,
                        frame.len(),
                    );
                    return true;
                }
            };
            if cyw43_install_wsec_key_with_pre_tx_drain(
                contract,
                0,
                &keys.pairwise_tk,
                &keys.ap_mac,
                None,
                false,
                "cyw43-host-eapol-post-secure-ptk",
                m4_drain_ready,
            )
            .is_err()
            {
                session
                    .progress
                    .record_eapol_error("host-eapol-post-secure-ptk-install");
                emit_cyw43_host_eapol_error(
                    contract,
                    "post-secure-ptk-install",
                    "host-eapol-post-secure-ptk-install",
                    poll,
                    frame.len(),
                );
                return true;
            }
            CYW43_HOST_EAPOL_PTK.fetch_add(1, Ordering::AcqRel);
            if let Some(gtk) = keys.gtk {
                let group_ea = [0u8; 6];
                if cyw43_install_wsec_key(
                    contract,
                    u32::from(gtk.index),
                    &gtk.key[..gtk.key_len],
                    &group_ea,
                    Some(&keys.rsc),
                    true,
                    "cyw43-host-eapol-post-secure-gtk",
                )
                .is_err()
                {
                    session
                        .progress
                        .record_eapol_error("host-eapol-post-secure-gtk-install");
                    emit_cyw43_host_eapol_error(
                        contract,
                        "post-secure-gtk-install",
                        "host-eapol-post-secure-gtk-install",
                        poll,
                        frame.len(),
                    );
                    return true;
                }
                CYW43_HOST_EAPOL_GTK.fetch_add(1, Ordering::AcqRel);
            }
            if cyw43_submit_bcdc_iovar_u32_with_options(
                contract,
                "wsec",
                CYW43_WSEC_AES,
                "cyw43-host-eapol-post-secure-reassert-wsec",
                Cyw43ControlHeaderMode::Extended,
                CYW43_HOST_EAPOL_WSEC_REASSERT_PRE_TX_DRAIN,
            )
            .is_err()
            {
                session
                    .progress
                    .record_eapol_error("host-eapol-post-secure-wsec-reassert");
                emit_cyw43_host_eapol_error(
                    contract,
                    "post-secure-wsec-reassert",
                    "host-eapol-post-secure-wsec-reassert",
                    poll,
                    frame.len(),
                );
            }
        }
        HostEapolAction::SendGroupM2InstallGtk { len, keys } => {
            let group_ea = [0u8; 6];
            if cyw43_install_wsec_key(
                contract,
                u32::from(keys.gtk.index),
                &keys.gtk.key[..keys.gtk.key_len],
                &group_ea,
                Some(&keys.rsc),
                true,
                "cyw43-host-eapol-post-secure-gtk",
            )
            .is_err()
            {
                session
                    .progress
                    .record_eapol_error("host-eapol-post-secure-gtk-install");
                emit_cyw43_host_eapol_error(
                    contract,
                    "post-secure-gtk-install",
                    "host-eapol-post-secure-gtk-install",
                    poll,
                    frame.len(),
                );
                return true;
            }
            CYW43_HOST_EAPOL_GTK.fetch_add(1, Ordering::AcqRel);
            emit_cyw43_host_eapol_message(
                contract,
                "group-key",
                "post-secure-recv-group-key",
                poll,
                frame.len(),
            );
            if submit_cyw43_host_eapol_payload_bounded(
                contract,
                &tx_frame[..len],
                "cyw43-host-eapol-post-secure-group-m2",
            ) {
                emit_cyw43_host_eapol_message(
                    contract,
                    "group-m2",
                    "post-secure-send-group-m2",
                    poll,
                    len,
                );
            } else {
                session
                    .progress
                    .record_eapol_error("host-eapol-post-secure-group-m2-tx");
            }
        }
    }
    emit_cyw43_host_eapol_status(contract, "post-secure-eapol-rx", &session.progress);
    true
}

#[cfg(feature = "kernel")]
fn take_cyw43_pending_rx_token() -> Option<(u16, DriverTaskNetRxToken)> {
    CYW43_PENDING_RX_QUEUE
        .lock()
        .pop_front()
        .map(|pending| (pending.flags, pending.token))
}

#[cfg(feature = "kernel")]
fn store_cyw43_pending_rx_token(flags: u16, token: DriverTaskNetRxToken) -> bool {
    let mut queue = CYW43_PENDING_RX_QUEUE.lock();
    let pending = match queue.push_back(DriverTaskNetPendingRxToken { flags, token }) {
        Ok(()) => {
            update_atomic_u32_max(&CYW43_PENDING_RX_HIGH_WATER, queue.len() as u32);
            return true;
        }
        Err(pending) => pending,
    };
    if !cyw43_evict_one_pending_rx_for(&mut queue, flags, &pending.token) {
        record_cyw43_pending_rx_drop();
        return false;
    }

    record_cyw43_pending_rx_drop();
    if queue.push_back(pending).is_ok() {
        update_atomic_u32_max(&CYW43_PENDING_RX_HIGH_WATER, queue.len() as u32);
        true
    } else {
        record_cyw43_pending_rx_drop();
        false
    }
}

#[cfg(feature = "kernel")]
fn cyw43_pending_rx_token_store_possible(flags: u16, token: &DriverTaskNetRxToken) -> bool {
    let queue = CYW43_PENDING_RX_QUEUE.lock();
    queue.len() < CYW43_PENDING_RX_QUEUE_CAP
        || cyw43_pending_rx_queue_contains_evictable_for(&queue, flags, token)
}

#[cfg(feature = "kernel")]
fn cyw43_pending_rx_priority(flags: u16, token: &DriverTaskNetRxToken) -> u8 {
    if cyw43_frame_channel(flags) != DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA {
        return 7;
    }
    let frame = &token.buffer[..token.len];
    let ethertype = cyw43_ethertype(frame);
    if CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) != 0 && ethertype == Some(ETH_P_EAPOL) {
        return 0;
    }
    let runtime_mac = runtime_mac(DriverTaskHotPath::Cyw43Wifi)
        .unwrap_or(CYW43_DRIVER_TASK_MAC)
        .0;
    let (dst, src) =
        cyw43_ethernet_addrs(frame).unwrap_or(([0; ETHER_ADDR_LEN], [0; ETHER_ADDR_LEN]));
    if let Some(info) = cyw43_data_path_info(frame) {
        if info.dhcp != "none" {
            return 6;
        }
        if info.ethertype == CYW43_ETH_P_ARP {
            return if info.arp_tpa == [0; 4]
                || matches!(cyw43_assigned_ipv4(), Some(ip) if info.arp_tpa == ip)
                || dst == runtime_mac
                || src == runtime_mac
            {
                5
            } else {
                2
            };
        }
    }
    if ethertype == Some(CYW43_ETH_P_IPV4) {
        let ip_proto = frame.get(ETH_HEADER_LEN + 9).copied().unwrap_or(0);
        if matches!(ip_proto, CYW43_IP_PROTO_TCP | CYW43_IP_PROTO_UDP)
            && (dst == runtime_mac || src == runtime_mac)
        {
            return 5;
        }
        return 3;
    }
    if matches!(ethertype, Some(CYW43_ETH_P_IPV6)) && (dst[0] & 0x01) != 0 {
        return 1;
    }
    if (dst[0] & 0x01) != 0 {
        return 1;
    }
    3
}

#[cfg(feature = "kernel")]
fn cyw43_pending_rx_queue_contains_evictable_for(
    queue: &heapless::Deque<DriverTaskNetPendingRxToken, CYW43_PENDING_RX_QUEUE_CAP>,
    flags: u16,
    token: &DriverTaskNetRxToken,
) -> bool {
    let incoming_priority = cyw43_pending_rx_priority(flags, token);
    let (front, back) = queue.as_slices();
    front
        .iter()
        .chain(back.iter())
        .any(|pending| cyw43_pending_rx_priority(pending.flags, &pending.token) < incoming_priority)
}

#[cfg(feature = "kernel")]
fn cyw43_evict_one_pending_rx_for(
    queue: &mut heapless::Deque<DriverTaskNetPendingRxToken, CYW43_PENDING_RX_QUEUE_CAP>,
    flags: u16,
    token: &DriverTaskNetRxToken,
) -> bool {
    let original_len = queue.len();
    let incoming_priority = cyw43_pending_rx_priority(flags, token);
    let target_priority = {
        let (front, back) = queue.as_slices();
        front
            .iter()
            .chain(back.iter())
            .filter_map(|pending| {
                let priority = cyw43_pending_rx_priority(pending.flags, &pending.token);
                (priority < incoming_priority).then_some(priority)
            })
            .min()
    };
    let Some(target_priority) = target_priority else {
        return false;
    };
    let mut retained =
        heapless::Deque::<DriverTaskNetPendingRxToken, CYW43_PENDING_RX_QUEUE_CAP>::new();
    let mut evicted = false;

    for _ in 0..original_len {
        let Some(pending) = queue.pop_front() else {
            break;
        };
        if !evicted && cyw43_pending_rx_priority(pending.flags, &pending.token) == target_priority {
            evicted = true;
            continue;
        }
        let _ = retained.push_back(pending);
    }
    while let Some(pending) = retained.pop_front() {
        let _ = queue.push_back(pending);
    }
    evicted
}

#[cfg(feature = "kernel")]
fn store_cyw43_pending_control_reply(
    flags: u16,
    token: DriverTaskNetRxToken,
    sequence: u32,
) -> bool {
    if cyw43_frame_channel(flags) != DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL {
        return false;
    }
    let Some(reply) = cyw43_control_reply_from_token(&token) else {
        return false;
    };
    let pending = DriverTaskNetPendingControlReply {
        flags,
        sequence,
        cmd: reply.cmd,
        id: reply.id,
        token,
    };
    let mut queue = CYW43_PENDING_CONTROL_REPLY_QUEUE.lock();
    if queue.len() == CYW43_PENDING_CONTROL_REPLY_QUEUE_CAP {
        let _ = queue.pop_front();
    }
    queue.push_back(pending).is_ok()
}

#[cfg(feature = "kernel")]
fn take_cyw43_pending_control_reply(cmd: u32, id: u16) -> Option<DriverTaskNetPendingControlReply> {
    let mut queue = CYW43_PENDING_CONTROL_REPLY_QUEUE.lock();
    let len = queue.len();
    for _ in 0..len {
        let pending = queue.pop_front()?;
        if pending.cmd == cmd && pending.id == id {
            return Some(pending);
        }
        let _ = queue.push_back(pending);
    }
    None
}

#[cfg(feature = "kernel")]
fn take_cyw43_pending_control_reply_completion(
    contract: DriverTaskContract,
    cmd: u32,
    id: u16,
) -> Option<(DriverTaskCompletionRecord, Cyw43ControlReply)> {
    let pending = take_cyw43_pending_control_reply(cmd, id)?;
    let reply = cyw43_control_reply_from_token(&pending.token)?;
    let payload = &pending.token.buffer[..pending.token.len];
    let frame =
        crate::hal::driver_task::stage_driver_task_ring_frame(contract, payload, pending.flags)?;
    Some((
        DriverTaskCompletionRecord {
            sequence: pending.sequence,
            code: DriverTaskCompletionCode::FrameReady.as_u16(),
            detail: 0,
            result: pending.token.len as u32,
            frame,
        },
        reply,
    ))
}

#[cfg(feature = "kernel")]
fn clear_cyw43_pending_control_replies() {
    CYW43_PENDING_CONTROL_REPLY_QUEUE.lock().clear();
}

#[cfg(feature = "kernel")]
fn cyw43_pending_rx_queue_len() -> u64 {
    CYW43_PENDING_RX_QUEUE.lock().len() as u64
}

#[cfg(feature = "kernel")]
fn record_cyw43_pending_rx_drop() {
    CYW43_PENDING_RX_DROPS.fetch_add(1, Ordering::AcqRel);
}

#[cfg(feature = "kernel")]
fn cyw43_rx_token_ethertype(flags: u16, token: &DriverTaskNetRxToken) -> Option<u16> {
    if cyw43_frame_channel(flags) != DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA {
        return None;
    }
    cyw43_ethertype(&token.buffer[..token.len])
}

#[cfg(feature = "kernel")]
fn take_genet_pending_rx_token() -> Option<DriverTaskNetRxToken> {
    GENET_PENDING_RX_QUEUE.lock().pop_front()
}

#[cfg(feature = "kernel")]
fn store_genet_pending_rx_token(token: DriverTaskNetRxToken) -> bool {
    let mut queue = GENET_PENDING_RX_QUEUE.lock();
    if queue.push_back(token).is_ok() {
        update_atomic_u32_max(&GENET_PENDING_RX_HIGH_WATER, queue.len() as u32);
        true
    } else {
        GENET_PENDING_RX_DROPS.fetch_add(1, Ordering::AcqRel);
        false
    }
}

#[cfg(feature = "kernel")]
fn genet_pending_rx_queue_len() -> u64 {
    GENET_PENDING_RX_QUEUE.lock().len() as u64
}

#[cfg(feature = "kernel")]
fn cyw43_pending_rx_token_occupied() -> bool {
    !CYW43_PENDING_RX_QUEUE.lock().is_empty()
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
    let completion = poll_cyw43_driver_task_data_completion(CYW43_DATA_RX_STEADY_POLL_FLAGS)?;
    cyw43_driver_task_frame_from_completion(CYW43_WIFI_DRIVER_TASK_CONTRACT, completion)
}

#[cfg(feature = "kernel")]
pub(crate) fn poll_cyw43_driver_task_steady_data_completion(
    contract: DriverTaskContract,
) -> Option<DriverTaskCompletionRecord> {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT {
        return None;
    }
    poll_cyw43_driver_task_data_completion(CYW43_DATA_RX_STEADY_POLL_FLAGS)
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
    let token = driver_task_rx_token_from_completion(contract, completion)?;
    Some((completion.frame.flags, token))
}

#[cfg(feature = "kernel")]
fn driver_task_rx_token_from_completion(
    contract: DriverTaskContract,
    completion: DriverTaskCompletionRecord,
) -> Option<DriverTaskNetRxToken> {
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
fn cyw43_driver_task_data_frame_from_completion(
    contract: DriverTaskContract,
    completion: DriverTaskCompletionRecord,
) -> Option<DriverTaskNetRxToken> {
    cyw43_driver_task_data_frame_with_flags_from_completion(contract, completion)
        .map(|(_flags, token)| token)
}

#[cfg(feature = "kernel")]
fn cyw43_driver_task_data_frame_with_flags_from_completion(
    contract: DriverTaskContract,
    completion: DriverTaskCompletionRecord,
) -> Option<(u16, DriverTaskNetRxToken)> {
    let (flags, token) = cyw43_driver_task_frame_from_completion(contract, completion)?;
    let _ = complete_cyw43_unproven_tx_window_from_rx_flags(flags);
    let frame_channel = cyw43_frame_channel(flags);
    if frame_channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA {
        Some((flags, token))
    } else {
        emit_cyw43_data_path_trace(
            contract,
            "rx-channel-drop",
            "non-data",
            0,
            &token.buffer[..token.len],
            Some(completion),
            flags,
            cyw43_pending_rx_token_occupied(),
            cyw43_pending_rx_token_occupied(),
        );
        if frame_channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL {
            let _ = store_cyw43_pending_control_reply(flags, token, completion.sequence);
        }
        None
    }
}

#[cfg(feature = "kernel")]
fn resume_cyw43_active_prompt_poll_for_data_path(
    contract: DriverTaskContract,
) -> Option<(u16, DriverTaskNetRxToken)> {
    let active_poll = cyw43_active_prompt_poll(contract)?;
    let completion = match active_poll.kind {
        Cyw43HostEapolPollKind::Control => {
            poll_cyw43_driver_task_control_completion(active_poll.flags)
        }
        Cyw43HostEapolPollKind::Data => poll_cyw43_driver_task_data_completion(active_poll.flags),
    }?;
    cyw43_driver_task_data_frame_with_flags_from_completion(contract, completion)
}

#[cfg(feature = "kernel")]
fn resume_cyw43_active_prompt_poll_for_tx_retry(contract: DriverTaskContract) -> bool {
    if contract != CYW43_WIFI_DRIVER_TASK_CONTRACT || cyw43_pending_rx_token_occupied() {
        return false;
    }
    let Some(active_poll) = cyw43_active_prompt_poll(contract) else {
        if cyw43_active_descriptor_blocks_fresh_net_poll(contract) {
            return false;
        }
        if let Some((flags, token)) = poll_cyw43_driver_task_any_frame() {
            let frame_channel = cyw43_frame_channel(flags);
            if frame_channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA {
                if consume_cyw43_post_secure_eapol_frame(
                    contract,
                    flags,
                    &token,
                    "tx-retry-control-fallback",
                ) {
                    return true;
                }
                emit_cyw43_data_path_trace(
                    contract,
                    "rx-preserve",
                    "tx-retry-control-fallback",
                    0,
                    &token.buffer[..token.len],
                    None,
                    flags,
                    false,
                    true,
                );
                return store_cyw43_pending_rx_token(flags, token);
            } else if frame_channel == DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL {
                let _ = store_cyw43_pending_control_reply(flags, token, 0);
                return true;
            }
        }
        return false;
    };
    let completion = match active_poll.kind {
        Cyw43HostEapolPollKind::Control => {
            poll_cyw43_driver_task_control_completion(active_poll.flags)
        }
        Cyw43HostEapolPollKind::Data => poll_cyw43_driver_task_data_completion(active_poll.flags),
    };
    let Some(completion) = completion else {
        return false;
    };
    let progress = cyw43_tx_retry_completion_progressed(&completion);
    if let Some((flags, token)) =
        cyw43_driver_task_data_frame_with_flags_from_completion(contract, completion)
    {
        if consume_cyw43_post_secure_eapol_frame(contract, flags, &token, "tx-retry") {
            return true;
        }
        emit_cyw43_data_path_trace(
            contract,
            "rx-preserve",
            "tx-retry",
            0,
            &token.buffer[..token.len],
            Some(completion),
            flags,
            false,
            true,
        );
        return store_cyw43_pending_rx_token(flags, token);
    }
    progress
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
                let rx = receive_driver_task_frame($contract, DriverTaskHotPath::$hot_path)?;
                record_driver_task_arp_rx(DriverTaskHotPath::$hot_path, &rx.buffer[..rx.len]);
                $rx_frames.fetch_add(1, Ordering::AcqRel);
                NET_DIAG.record_rx_frame_to_stack();
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
                    && !cyw43_data_tx_admission_ready($contract)
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

            fn set_assigned_ipv4(&mut self, ip: Ipv4Address) {
                #[cfg(feature = "kernel")]
                {
                    let _ = submit_driver_task_gratuitous_arp_announcement(
                        $contract,
                        DriverTaskHotPath::$hot_path,
                        ip.octets(),
                    );
                }
                if matches!(DriverTaskHotPath::$hot_path, DriverTaskHotPath::Cyw43Wifi) {
                    CYW43_ASSIGNED_IPV4_BE
                        .store(u32::from_be_bytes(ip.octets()), Ordering::Release);
                }
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
                let tx_submit = $tx_submitted.load(Ordering::Acquire) as u64;
                let (arp_rx, arp_tx) = driver_task_arp_counts(DriverTaskHotPath::$hot_path);
                let (rx_used_advances, tx_used_advances, tx_complete, tx_free, tx_in_flight) =
                    if matches!(DriverTaskHotPath::$hot_path, DriverTaskHotPath::GenetNic) {
                        let tx_complete = GENET_TX_HW_COMPLETED.load(Ordering::Acquire) as u64;
                        (
                            GENET_RX_HW_FRAMES.load(Ordering::Acquire) as u64,
                            tx_complete,
                            tx_complete,
                            GENET_TX_HW_FREE.load(Ordering::Acquire) as u64,
                            GENET_TX_HW_IN_FLIGHT.load(Ordering::Acquire) as u64,
                        )
                    } else if matches!(DriverTaskHotPath::$hot_path, DriverTaskHotPath::Cyw43Wifi) {
                        let tx_complete = (CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire)
                            as u64)
                            .min(tx_submit);
                        let tx_window_busy = CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire) != 0;
                        let tx_in_flight = tx_submit
                            .saturating_sub(tx_complete)
                            .max(u64::from(tx_window_busy));
                        (
                            0,
                            tx_complete,
                            tx_complete,
                            if tx_in_flight == 0 && !tx_window_busy {
                                1
                            } else {
                                0
                            },
                            tx_in_flight,
                        )
                    } else {
                        (0, 0, tx_submit, 0, 0)
                    };
                NetDeviceCounters {
                    rx_packets: $rx_frames.load(Ordering::Acquire) as u64,
                    tx_packets: tx_submit,
                    rx_used_advances,
                    tx_used_advances,
                    tx_submit,
                    tx_complete,
                    tx_free,
                    tx_in_flight,
                    arp_rx,
                    arp_tx,
                    driver_rx_last_len: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::GenetNic
                    ) {
                        GENET_RX_LAST_LEN.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    driver_rx_last_ethertype: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::GenetNic
                    ) {
                        GENET_RX_LAST_ETHERTYPE.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    genet_rx_runtime_queue_count: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::GenetNic
                    ) {
                        GENET_RX_RUNTIME_QUEUE_COUNT.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    genet_rx_runtime_queue_high_water: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::GenetNic
                    ) {
                        GENET_RX_RUNTIME_QUEUE_HIGH_WATER.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    genet_rx_runtime_queue_overflow_seen: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::GenetNic
                    ) {
                        GENET_RX_RUNTIME_QUEUE_OVERFLOW_SEEN.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    genet_rx_runtime_drain_budget_hit: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::GenetNic
                    ) {
                        GENET_RX_RUNTIME_DRAIN_BUDGET_HIT.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    genet_rx_runtime_byte_budget_hit: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::GenetNic
                    ) {
                        GENET_RX_RUNTIME_BYTE_BUDGET_HIT.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    genet_rx_runtime_max_drained_per_turn: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::GenetNic
                    ) {
                        GENET_RX_RUNTIME_MAX_DRAINED_PER_TURN.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    genet_rx_pending_queue_count: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::GenetNic
                    ) {
                        genet_pending_rx_queue_len()
                    } else {
                        0
                    },
                    genet_rx_pending_queue_high_water: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::GenetNic
                    ) {
                        GENET_PENDING_RX_HIGH_WATER.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    genet_rx_pending_drops: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::GenetNic
                    ) {
                        GENET_PENDING_RX_DROPS.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    wifi_rx_pending_queue_count: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        cyw43_pending_rx_queue_len()
                    } else {
                        0
                    },
                    wifi_rx_pending_queue_high_water: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        CYW43_PENDING_RX_HIGH_WATER.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    wifi_rx_pending_drops: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        CYW43_PENDING_RX_DROPS.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    wifi_rx_runtime_queue_count: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        CYW43_RX_RUNTIME_QUEUE_COUNT.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    wifi_rx_runtime_queue_high_water: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        CYW43_RX_RUNTIME_QUEUE_HIGH_WATER.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    wifi_rx_runtime_queue_overflow_seen: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        CYW43_RX_RUNTIME_QUEUE_OVERFLOW_SEEN.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    wifi_rx_runtime_drain_budget_hit: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        CYW43_RX_RUNTIME_DRAIN_BUDGET_HIT.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    wifi_rx_runtime_max_drained_per_turn: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        CYW43_RX_RUNTIME_MAX_DRAINED_PER_TURN.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    wifi_data_trace_faults: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        CYW43_DATA_TRACE_FAULT_COUNT.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
                    wifi_data_trace_tx_retries: if matches!(
                        DriverTaskHotPath::$hot_path,
                        DriverTaskHotPath::Cyw43Wifi
                    ) {
                        CYW43_DATA_TRACE_TX_RETRY_COUNT.load(Ordering::Acquire) as u64
                    } else {
                        0
                    },
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
    use smoltcp::phy::{Device, RxToken, TxToken};
    use smoltcp::time::Instant;

    static CYW43_STATUS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn reset_cyw43_status_flags() {
        CYW43_LINKED_RUNTIME_READY.store(0, Ordering::Release);
        CYW43_CONTROL_PLANE_READY.store(0, Ordering::Release);
        CYW43_ASSOCIATED.store(0, Ordering::Release);
        CYW43_LINK_UP.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_RX.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_START.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_M1.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_M2.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_M3.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_M4.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_PTK.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_GTK.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_ACTIVE.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_REQUIRED.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);
        CYW43_POST_SECURE_DATA_RX_ADMITTED.store(0, Ordering::Release);
        CYW43_DATA_TX_RETRIES.store(0, Ordering::Release);
        CYW43_DATA_TRACE_DHCP_COUNT.store(0, Ordering::Release);
        CYW43_DATA_TRACE_DROP_COUNT.store(0, Ordering::Release);
        CYW43_DATA_TRACE_EAPOL_CONSUME_COUNT.store(0, Ordering::Release);
        CYW43_DATA_TRACE_FAULT_COUNT.store(0, Ordering::Release);
        CYW43_DATA_TRACE_PENDING_COUNT.store(0, Ordering::Release);
        CYW43_DATA_TRACE_TX_RETRY_COUNT.store(0, Ordering::Release);
        CYW43_ARP_RX.store(0, Ordering::Release);
        CYW43_ARP_TX.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TX_RETRIES.store(0, Ordering::Release);
        CYW43_ASSIGNED_IPV4_BE.store(0, Ordering::Release);
        CYW43_TX_SUBMITTED.store(0, Ordering::Release);
        CYW43_TX_DROPPED.store(0, Ordering::Release);
        CYW43_TX_CREDIT_COMPLETED.store(0, Ordering::Release);
        CYW43_TX_CREDIT_UNPROVEN.store(0, Ordering::Release);
        CYW43_TX_UNPROVEN_ACTIVE.store(0, Ordering::Release);
        CYW43_TX_UNPROVEN_SEQ.store(0, Ordering::Release);
        CYW43_TX_UNPROVEN_COUNT.store(0, Ordering::Release);
        CYW43_RX_FRAMES.store(0, Ordering::Release);
        CYW43_PENDING_RX_HIGH_WATER.store(0, Ordering::Release);
        CYW43_PENDING_RX_DROPS.store(0, Ordering::Release);
        CYW43_DATA_TX_TEST_STUB.store(0, Ordering::Release);
        CYW43_DATA_TX_TEST_FAILS_BEFORE_SUCCESS.store(0, Ordering::Release);
        CYW43_DATA_TX_TEST_IDLE_BEFORE_SUCCESS.store(0, Ordering::Release);
        CYW43_DATA_TX_TEST_FAULTS_BEFORE_SUCCESS.store(0, Ordering::Release);
        CYW43_DATA_TX_TEST_SUCCESS_WITHOUT_CREDIT.store(0, Ordering::Release);
        CYW43_DATA_TX_TEST_ATTEMPTS.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_IO_STUB.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_TX_SUBMITTED.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_PTK_INSTALLED.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_GTK_INSTALLED.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_WSEC_REASSERTED.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_TX_DRAINED.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_TX_DRAIN_TIMEOUTS.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_DRAIN_BEFORE_PTK.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_DRAIN_BEFORE_SECURE.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_WSEC_PRE_TX_DRAIN.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_RX_RESTORED.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_BSSID_VALID.store(0, Ordering::Release);
        while take_cyw43_pending_rx_token().is_some() {}
        clear_cyw43_pending_control_replies();
        clear_cyw43_host_eapol_status_throttle();
        *CYW43_HOST_EAPOL_SESSION.lock() = None;
        *CYW43_HOST_EAPOL_PENDING_EVENT.lock() = None;
        clear_cyw43_active_prompt_poll();
    }

    fn mark_cyw43_data_plane_ready_for_test() {
        CYW43_LINKED_RUNTIME_READY.store(1, Ordering::Release);
        CYW43_CONTROL_PLANE_READY.store(1, Ordering::Release);
        CYW43_ASSOCIATED.store(1, Ordering::Release);
        CYW43_LINK_UP.store(1, Ordering::Release);
        CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);
        CYW43_POST_SECURE_DATA_RX_ADMITTED.store(1, Ordering::Release);
    }

    fn test_cyw43_event_packet(
        event_type: u8,
        status: u32,
        flags: u16,
    ) -> [u8; CYW43_BRCMF_EVENT_MIN_PACKET_LEN] {
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
            .copy_from_slice(&flags.to_be_bytes());
        packet[CYW43_BRCMF_EVENT_TYPE_OFFSET] = event_type;
        packet[CYW43_BRCMF_EVENT_STATUS_OFFSET..CYW43_BRCMF_EVENT_STATUS_OFFSET + 4]
            .copy_from_slice(&status.to_be_bytes());
        packet[CYW43_BRCMF_EVENT_ADDR_OFFSET..CYW43_BRCMF_EVENT_ADDR_OFFSET + 6]
            .copy_from_slice(&ap);
        packet
    }

    fn test_cyw43_bdc_event_frame(
        packet: &[u8; CYW43_BRCMF_EVENT_MIN_PACKET_LEN],
    ) -> [u8; CYW43_BDC_HEADER_BYTES + CYW43_BRCMF_EVENT_MIN_PACKET_LEN] {
        let mut frame = [0u8; CYW43_BDC_HEADER_BYTES + CYW43_BRCMF_EVENT_MIN_PACKET_LEN];
        frame[0] = CYW43_BDC_VERSION << CYW43_BDC_VERSION_SHIFT;
        frame[CYW43_BDC_HEADER_BYTES..].copy_from_slice(packet);
        frame
    }

    fn test_cyw43_arp_frame(op: u16) -> [u8; 42] {
        let mut frame = [0u8; 42];
        frame[..6].copy_from_slice(&[0xff; 6]);
        frame[6..12].copy_from_slice(&CYW43_DRIVER_TASK_MAC.0);
        frame[12..14].copy_from_slice(&CYW43_ETH_P_ARP.to_be_bytes());
        frame[ETH_HEADER_LEN..ETH_HEADER_LEN + 2].copy_from_slice(&1u16.to_be_bytes());
        frame[ETH_HEADER_LEN + 2..ETH_HEADER_LEN + 4]
            .copy_from_slice(&CYW43_ETH_P_IPV4.to_be_bytes());
        frame[ETH_HEADER_LEN + 4] = 6;
        frame[ETH_HEADER_LEN + 5] = 4;
        frame[ETH_HEADER_LEN + 6..ETH_HEADER_LEN + 8].copy_from_slice(&op.to_be_bytes());
        frame
    }

    fn test_cyw43_arp_request(
        sender_hw: [u8; 6],
        sender_ip: [u8; 4],
        target_ip: [u8; 4],
    ) -> [u8; 42] {
        let mut frame = test_cyw43_arp_frame(1);
        frame[..6].copy_from_slice(&[0xff; 6]);
        frame[6..12].copy_from_slice(&sender_hw);
        frame[ETH_HEADER_LEN + 8..ETH_HEADER_LEN + 14].copy_from_slice(&sender_hw);
        frame[ETH_HEADER_LEN + 14..ETH_HEADER_LEN + 18].copy_from_slice(&sender_ip);
        frame[ETH_HEADER_LEN + 18..ETH_HEADER_LEN + 24].copy_from_slice(&[0; 6]);
        frame[ETH_HEADER_LEN + 24..ETH_HEADER_LEN + 28].copy_from_slice(&target_ip);
        frame
    }

    fn test_cyw43_dhcp_frame(message_type: u8, src_port: u16, dst_port: u16) -> [u8; 286] {
        const IPV4_HEADER_LEN: usize = 20;
        const UDP_HEADER_LEN: usize = 8;
        const DHCP_PAYLOAD_LEN: usize = 244;
        let mut frame = [0u8; ETH_HEADER_LEN + IPV4_HEADER_LEN + UDP_HEADER_LEN + DHCP_PAYLOAD_LEN];
        frame[..6].copy_from_slice(&[0xff; 6]);
        frame[6..12].copy_from_slice(&CYW43_DRIVER_TASK_MAC.0);
        frame[12..14].copy_from_slice(&CYW43_ETH_P_IPV4.to_be_bytes());
        let ip = ETH_HEADER_LEN;
        frame[ip] = 0x45;
        frame[ip + 2..ip + 4].copy_from_slice(
            &((IPV4_HEADER_LEN + UDP_HEADER_LEN + DHCP_PAYLOAD_LEN) as u16).to_be_bytes(),
        );
        frame[ip + 9] = CYW43_IP_PROTO_UDP;
        frame[ip + 12..ip + 16].copy_from_slice(&[0, 0, 0, 0]);
        frame[ip + 16..ip + 20].copy_from_slice(&[255, 255, 255, 255]);
        let udp = ip + IPV4_HEADER_LEN;
        frame[udp..udp + 2].copy_from_slice(&src_port.to_be_bytes());
        frame[udp + 2..udp + 4].copy_from_slice(&dst_port.to_be_bytes());
        frame[udp + 4..udp + 6]
            .copy_from_slice(&((UDP_HEADER_LEN + DHCP_PAYLOAD_LEN) as u16).to_be_bytes());
        let dhcp = udp + UDP_HEADER_LEN;
        frame[dhcp] = 1;
        frame[dhcp + CYW43_DHCP_FIXED_BYTES..dhcp + CYW43_DHCP_FIXED_BYTES + 4]
            .copy_from_slice(&CYW43_DHCP_MAGIC_COOKIE);
        frame[dhcp + CYW43_DHCP_FIXED_BYTES + 4] = 53;
        frame[dhcp + CYW43_DHCP_FIXED_BYTES + 5] = 1;
        frame[dhcp + CYW43_DHCP_FIXED_BYTES + 6] = message_type;
        frame[dhcp + CYW43_DHCP_FIXED_BYTES + 7] = 255;
        frame
    }

    fn test_cyw43_tcp_frame() -> [u8; 94] {
        const IPV4_HEADER_LEN: usize = 20;
        const TCP_HEADER_LEN: usize = 20;
        const TCP_PAYLOAD_LEN: usize = 40;
        let mut frame = [0u8; ETH_HEADER_LEN + IPV4_HEADER_LEN + TCP_HEADER_LEN + TCP_PAYLOAD_LEN];
        frame[..6].copy_from_slice(&[0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5]);
        frame[6..12].copy_from_slice(&CYW43_DRIVER_TASK_MAC.0);
        frame[12..14].copy_from_slice(&CYW43_ETH_P_IPV4.to_be_bytes());
        let ip = ETH_HEADER_LEN;
        frame[ip] = 0x45;
        frame[ip + 2..ip + 4].copy_from_slice(
            &((IPV4_HEADER_LEN + TCP_HEADER_LEN + TCP_PAYLOAD_LEN) as u16).to_be_bytes(),
        );
        frame[ip + 9] = CYW43_IP_PROTO_TCP;
        let tcp = ip + IPV4_HEADER_LEN;
        frame[tcp..tcp + 2].copy_from_slice(&31337u16.to_be_bytes());
        frame[tcp + 2..tcp + 4].copy_from_slice(&49152u16.to_be_bytes());
        frame
    }

    fn test_cyw43_ipv6_multicast_frame() -> [u8; 86] {
        let mut frame = [0u8; 86];
        frame[..6].copy_from_slice(&[0x33, 0x33, 0x00, 0x00, 0x00, 0xfb]);
        frame[6..12].copy_from_slice(&[0x3a, 0xca, 0x84, 0x66, 0x80, 0x2a]);
        frame[12..14].copy_from_slice(&CYW43_ETH_P_IPV6.to_be_bytes());
        frame[ETH_HEADER_LEN] = 0x60;
        frame
    }

    fn test_cyw43_eapol_frame() -> [u8; 64] {
        let mut frame = [0u8; 64];
        frame[..6].copy_from_slice(&CYW43_DRIVER_TASK_MAC.0);
        frame[6..12].copy_from_slice(&[0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5]);
        frame[12..14].copy_from_slice(&ETH_P_EAPOL.to_be_bytes());
        frame
    }

    fn test_rx_token(payload: &[u8]) -> DriverTaskNetRxToken {
        let mut token = DriverTaskNetRxToken {
            len: payload.len(),
            buffer: [0; MAX_FRAME_LEN],
        };
        token.buffer[..payload.len()].copy_from_slice(payload);
        token
    }

    fn test_control_reply_token(
        cmd: u32,
        id: u16,
        status: u32,
        body: &[u8],
    ) -> DriverTaskNetRxToken {
        let mut payload = [0u8; MAX_FRAME_LEN];
        payload[0..4].copy_from_slice(&cmd.to_le_bytes());
        payload[4..8].copy_from_slice(&(body.len() as u32).to_le_bytes());
        payload[10..12].copy_from_slice(&id.to_le_bytes());
        payload[12..16].copy_from_slice(&status.to_le_bytes());
        payload[CYW43_BCDC_HEADER_BYTES..CYW43_BCDC_HEADER_BYTES + body.len()]
            .copy_from_slice(body);
        DriverTaskNetRxToken {
            len: CYW43_BCDC_HEADER_BYTES + body.len(),
            buffer: payload,
        }
    }

    struct TestCyw43RingGuard;

    impl Drop for TestCyw43RingGuard {
        fn drop(&mut self) {
            crate::hal::driver_task::clear_driver_task_transport(CYW43_WIFI_DRIVER_TASK_CONTRACT);
        }
    }

    fn test_publish_cyw43_ring(
        ring_page: &mut [u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES],
    ) -> TestCyw43RingGuard {
        crate::hal::driver_task::clear_driver_task_transport(CYW43_WIFI_DRIVER_TASK_CONTRACT);
        crate::hal::driver_task::publish_driver_task_ring(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            ring_page.as_mut_ptr() as usize,
        );
        TestCyw43RingGuard
    }

    struct TestCyw43HostEapolIoGuard;

    impl Drop for TestCyw43HostEapolIoGuard {
        fn drop(&mut self) {
            CYW43_HOST_EAPOL_TEST_IO_STUB.store(0, Ordering::Release);
        }
    }

    fn test_enable_cyw43_host_eapol_io_stub() -> TestCyw43HostEapolIoGuard {
        CYW43_HOST_EAPOL_TEST_IO_STUB.store(1, Ordering::Release);
        TestCyw43HostEapolIoGuard
    }

    fn test_stage_cyw43_completion(
        payload: &[u8],
        flags: u16,
        sequence: u32,
    ) -> DriverTaskCompletionRecord {
        let frame = crate::hal::driver_task::stage_driver_task_ring_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            payload,
            flags,
        )
        .expect("test ring has room for linked-runtime frame");
        DriverTaskCompletionRecord {
            sequence,
            code: DriverTaskCompletionCode::FrameReady.as_u16(),
            detail: 0,
            result: 0,
            frame,
        }
    }

    fn assert_cyw43_u32_iovar_frame(name: &str, value: u32) {
        let mut payload = [0u8; 32];
        let name_len = name.len();
        payload[..name_len].copy_from_slice(name.as_bytes());
        payload[name_len] = 0;
        payload[name_len + 1..name_len + 5].copy_from_slice(&value.to_le_bytes());

        let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
        let len = cyw43_write_bcdc_frame(
            &mut frame,
            CYW43_WLC_SET_VAR,
            CYW43_BCDC_FLAG_SET,
            1,
            &payload[..name_len + 5],
        )
        .expect("u32 iovar BCDC frame should fit");
        let info = cyw43_control_iovar_info(&frame[..len], CYW43_WLC_SET_VAR)
            .expect("u32 iovar should decode");

        assert_eq!(cyw43_read_le_u32(&frame[..len], 0), Some(CYW43_WLC_SET_VAR));
        assert_eq!(
            cyw43_read_le_u16(&frame[..len], 8),
            Some(CYW43_BCDC_FLAG_SET)
        );
        assert_eq!(info.name, name);
        assert_eq!(info.data_len, 4);
        assert_eq!(info.value_u32, Some(value));
    }

    #[test]
    fn cyw43_control_preflight_matches_linux_ordered_primitives() {
        assert_eq!(CYW43_WLC_GET_REVINFO, 98);
        assert_eq!(CYW43_WLC_SET_PM, 86);
        assert_eq!(CYW43_CONTROL_EXCHANGE_FAULT_DETAIL, 0x530b);
        assert_eq!(CYW43_BCME_UNSUPPORTED_STATUS, 0xffff_ffe9);
        assert_eq!(CYW43_BCME_BADARG_STATUS, 0xffff_fffe);
        assert_eq!(CYW43_BCME_NOTASSOCIATED_STATUS, 0xffff_ffef);
        assert_eq!(CYW43_MFP_NONE, 0);
        assert_eq!(CYW43_WME_BSS_DISABLE_RSN_DEFAULT, 1);
        assert_eq!(CYW43_PM_OFF, 0);
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
        let badarg = DriverTaskCompletionRecord {
            result: CYW43_BCME_BADARG_STATUS,
            ..unsupported
        };
        assert!(cyw43_control_exchange_completion_is_unsupported(
            unsupported
        ));
        assert!(!cyw43_control_exchange_completion_is_unsupported(
            other_fault
        ));
        assert!(cyw43_control_exchange_completion_is_optional_filter_reject(
            unsupported
        ));
        assert!(cyw43_control_exchange_completion_is_optional_filter_reject(
            badarg
        ));
        assert!(!cyw43_control_exchange_completion_is_optional_filter_reject(other_fault));
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
        );
        assert_eq!(CYW43_ENGINE_INIT_REPLAY_ATTEMPTS, 3);
        assert_eq!(
            cyw43_engine_init_completion_replay_reason(None),
            Some("no-reply")
        );
        assert!(cyw43_engine_init_completion_allows_replay(
            Some("no-reply"),
            false
        ));
        assert_eq!(
            cyw43_engine_init_completion_replay_reason(Some(DriverTaskCompletionRecord::fault(
                2,
                DriverTaskFaultCode::DeviceUnavailable
            ))),
            Some("device-unavailable")
        );
        assert!(cyw43_engine_init_completion_allows_replay(
            Some("device-unavailable"),
            false
        ));
        assert_eq!(
            cyw43_engine_init_completion_replay_reason(Some(DriverTaskCompletionRecord::fault(
                3,
                DriverTaskFaultCode::RejectedCommand
            ))),
            Some("stale-admission")
        );
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
        );
        let not_associated = DriverTaskCompletionRecord {
            result: CYW43_BCME_NOTASSOCIATED_STATUS,
            ..unsupported
        };
        let bssid_err = Cyw43BssidRefreshError::Completion(not_associated);
        assert!(bssid_err.is_not_associated());
        assert_eq!(bssid_err.result(), CYW43_BCME_NOTASSOCIATED_STATUS);
    }

    #[test]
    fn cyw43_rsn_policy_iovars_match_old_good_values() {
        for (name, value) in [
            ("mfp", CYW43_MFP_NONE),
            ("wme_bss_disable", CYW43_WME_BSS_DISABLE_RSN_DEFAULT),
        ] {
            let mut payload = [0u8; 32];
            let name_len = name.len();
            payload[..name_len].copy_from_slice(name.as_bytes());
            payload[name_len] = 0;
            payload[name_len + 1..name_len + 5].copy_from_slice(&value.to_le_bytes());

            let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
            let len = cyw43_write_bcdc_frame(
                &mut frame,
                CYW43_WLC_SET_VAR,
                CYW43_BCDC_FLAG_SET,
                1,
                &payload[..name_len + 5],
            )
            .expect("RSN policy iovar frame should fit");
            let info = cyw43_control_iovar_info(&frame[..len], CYW43_WLC_SET_VAR)
                .expect("RSN policy iovar should decode");

            assert_eq!(info.name, name);
            assert_eq!(info.data_len, 4);
            assert_eq!(info.value_u32, Some(value));
        }
    }

    #[test]
    fn cyw43_connect_station_policy_iovars_match_linux_values() {
        for (name, value) in CYW43_LINUX_CONNECT_STATION_POLICY_IOVARS {
            assert_cyw43_u32_iovar_frame(name, value);
        }
    }

    #[test]
    fn cyw43_low_latency_power_policy_matches_linux_command_shape() {
        let mut payload = [0u8; 4];
        payload.copy_from_slice(&CYW43_PM_OFF.to_le_bytes());

        let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
        let len = cyw43_write_bcdc_frame(
            &mut frame,
            CYW43_WLC_SET_PM,
            CYW43_BCDC_FLAG_SET,
            1,
            &payload,
        )
        .expect("PM_OFF BCDC frame should fit");

        assert_eq!(cyw43_read_le_u32(&frame[..len], 0), Some(CYW43_WLC_SET_PM));
        assert_eq!(
            cyw43_read_le_u16(&frame[..len], 8),
            Some(CYW43_BCDC_FLAG_SET)
        );
        assert_eq!(
            cyw43_read_le_u32(&frame[..len], CYW43_BCDC_HEADER_BYTES),
            Some(0)
        );
    }

    #[test]
    fn cyw43_clm_chunk_shape_matches_linux_firmware_pi4b() {
        assert_eq!(CYW43_CLM_CHUNK_BYTES, 1400);
        assert_eq!(cyw43_clm_iovar_data_len(CYW43_CLM_CHUNK_BYTES), 1412);
        assert_eq!(cyw43_clm_setvar_payload_len(CYW43_CLM_CHUNK_BYTES), 1420);
        assert_eq!(cyw43_clm_iovar_data_len(2676 - CYW43_CLM_CHUNK_BYTES), 1288);
        assert_eq!(
            cyw43_clm_setvar_payload_len(2676 - CYW43_CLM_CHUNK_BYTES),
            1296
        );
    }

    #[test]
    fn cyw43_clm_download_payload_matches_linux_pi4b_header() {
        let mut clm = [0u8; 2676];
        for (index, byte) in clm.iter_mut().enumerate() {
            *byte = (index & 0xff) as u8;
        }

        let mut payload = [0u8; CYW43_CLM_IOVAR_HEADER_BYTES + CYW43_CLM_CHUNK_BYTES];
        let first_len =
            cyw43_write_clm_download_payload(&mut payload, &clm, 0, CYW43_CLM_CHUNK_BYTES)
                .expect("first CLM chunk should fit");
        assert_eq!(first_len, 1412);
        assert_eq!(
            &payload[0..2],
            &(CYW43_CLM_DOWNLOAD_FLAG_HANDLER_VER | CYW43_CLM_DOWNLOAD_FLAG_BEGIN).to_le_bytes()
        );
        assert_eq!(&payload[2..4], &CYW43_CLM_DOWNLOAD_TYPE.to_le_bytes());
        assert_eq!(
            &payload[4..8],
            &(CYW43_CLM_CHUNK_BYTES as u32).to_le_bytes()
        );
        assert_eq!(&payload[8..12], &0u32.to_le_bytes());
        assert_eq!(
            &payload[CYW43_CLM_IOVAR_HEADER_BYTES..CYW43_CLM_IOVAR_HEADER_BYTES + 4],
            &clm[..4]
        );

        let final_offset = CYW43_CLM_CHUNK_BYTES;
        let final_len_bytes = clm.len() - final_offset;
        let final_len =
            cyw43_write_clm_download_payload(&mut payload, &clm, final_offset, final_len_bytes)
                .expect("final CLM chunk should fit");
        assert_eq!(final_len, 1288);
        assert_eq!(
            &payload[0..2],
            &(CYW43_CLM_DOWNLOAD_FLAG_HANDLER_VER | CYW43_CLM_DOWNLOAD_FLAG_END).to_le_bytes()
        );
        assert_eq!(&payload[4..8], &(final_len_bytes as u32).to_le_bytes());
        assert_eq!(
            &payload[CYW43_CLM_IOVAR_HEADER_BYTES..CYW43_CLM_IOVAR_HEADER_BYTES + 4],
            &clm[final_offset..final_offset + 4]
        );
    }

    #[test]
    fn cyw43_clmload_iovar_frame_matches_bounded_control_shape() {
        let clm = [0x5au8; CYW43_CLM_CHUNK_BYTES];
        let mut clm_payload = [0u8; CYW43_CLM_IOVAR_HEADER_BYTES + CYW43_CLM_CHUNK_BYTES];
        let clm_payload_len =
            cyw43_write_clm_download_payload(&mut clm_payload, &clm, 0, clm.len())
                .expect("CLM payload should fit");
        let name_len = CYW43_CLM_IOVAR_NAME.len();
        let payload_len = name_len + 1 + clm_payload_len;
        let mut payload = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
        payload[..name_len].copy_from_slice(CYW43_CLM_IOVAR_NAME.as_bytes());
        payload[name_len] = 0;
        payload[name_len + 1..payload_len].copy_from_slice(&clm_payload[..clm_payload_len]);

        let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
        let len = cyw43_write_bcdc_frame(
            &mut frame,
            CYW43_WLC_SET_VAR,
            CYW43_BCDC_FLAG_SET,
            1,
            &payload[..payload_len],
        )
        .expect("CLM BCDC frame should fit");
        let info = cyw43_control_iovar_info(&frame[..len], CYW43_WLC_SET_VAR)
            .expect("CLM iovar should decode");
        let data_start = CYW43_BCDC_HEADER_BYTES + name_len + 1;

        assert_eq!(info.name, CYW43_CLM_IOVAR_NAME);
        assert_eq!(info.data_len, clm_payload_len);
        assert_eq!(info.value_u32, None);
        assert_eq!(
            &frame[data_start..data_start + CYW43_CLM_IOVAR_HEADER_BYTES],
            &clm_payload[..CYW43_CLM_IOVAR_HEADER_BYTES]
        );
    }

    #[test]
    fn cyw43_join_security_iovars_match_old_good_shape() {
        assert_eq!(
            CYW43_JOIN_SECURITY_ORDER_LABEL,
            "connect-policy-wpaie-wpa_auth-initial-auth-wsec-rsn-cap-policy-wpa_auth-final"
        );
        for (name, value) in [
            ("wpa_auth", CYW43_WPA2_AUTH_PSK_OR_UNSPECIFIED),
            ("auth", 0),
            ("wsec", CYW43_WSEC_AES),
            ("wpa_auth", CYW43_WPA2_AUTH_PSK),
            ("wsec", CYW43_WSEC_NONE),
            ("wpa_auth", CYW43_WPA_AUTH_DISABLED),
        ] {
            assert_cyw43_u32_iovar_frame(name, value);
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_firmware_supplicant_probe_payloads_match_old_good_shape() {
        assert_eq!(
            Cyw43FirmwareSupplicantProbe::Disabled.label(),
            "sup_wpa-disabled-host-eapol"
        );
        assert_eq!(
            Cyw43FirmwareSupplicantProbe::Unsupported.label(),
            "sup_wpa-unsupported-host-eapol"
        );
        assert_eq!(CYW43_BSSCFG_PRIMARY_INDEX, 0);
        assert_eq!(CYW43_SUP_WPA2_EAPVER_ANY, u32::MAX);
        assert_eq!(CYW43_SUP_WPA_TIMEOUT_MS, 2500);

        let mut primary_data = [0u8; 8];
        primary_data[..4].copy_from_slice(&0u32.to_le_bytes());
        let mut wrapper_data = [0u8; 8];
        wrapper_data[..4].copy_from_slice(&CYW43_BSSCFG_PRIMARY_INDEX.to_le_bytes());
        wrapper_data[4..8].copy_from_slice(&0u32.to_le_bytes());

        for (name, data_len, data) in [
            ("sup_wpa", 4usize, primary_data),
            ("bsscfg:sup_wpa", 8usize, wrapper_data),
        ] {
            let name_len = name.len();
            let payload_len = name_len + 1 + data_len;
            let mut payload = [0u8; 32];
            payload[..name_len].copy_from_slice(name.as_bytes());
            payload[name_len] = 0;
            payload[name_len + 1..payload_len].copy_from_slice(&data[..data_len]);

            let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
            let len = cyw43_write_bcdc_frame(
                &mut frame,
                CYW43_WLC_SET_VAR,
                CYW43_BCDC_FLAG_SET,
                1,
                &payload[..payload_len],
            )
            .expect("supplicant probe frame should fit");
            let info = cyw43_control_iovar_info(&frame[..len], CYW43_WLC_SET_VAR)
                .expect("supplicant iovar should decode");
            let data_start = CYW43_BCDC_HEADER_BYTES + name_len + 1;

            assert_eq!(info.name, name);
            assert_eq!(info.data_len, data_len);
            assert_eq!(&frame[data_start..data_start + data_len], &data[..data_len]);
        }
    }

    #[test]
    fn cyw43_prejoin_association_policy_matches_linux_values() {
        let mut event_mask = [0u8; CYW43_EVENT_MASK_LEN];
        cyw43_set_event_mask_bit(&mut event_mask, CYW43_EVENT_IF)
            .expect("IF event bit should fit in event_msgs mask");

        assert_eq!(CYW43_EVENT_IF, 54);
        assert_eq!(usize::from(CYW43_EVENT_IF / 8).saturating_add(1), 7);
        assert_ne!(event_mask[usize::from(CYW43_EVENT_IF / 8)] & 0x40, 0);

        let mut join_pref_payload = [0u8; 32];
        let name = "join_pref";
        let name_len = name.len();
        join_pref_payload[..name_len].copy_from_slice(name.as_bytes());
        join_pref_payload[name_len] = 0;
        join_pref_payload[name_len + 1..name_len + 1 + CYW43_LINUX_JOIN_PREF_DEFAULT.len()]
            .copy_from_slice(&CYW43_LINUX_JOIN_PREF_DEFAULT);

        let mut frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
        let payload_len = name_len + 1 + CYW43_LINUX_JOIN_PREF_DEFAULT.len();
        let len = cyw43_write_bcdc_frame(
            &mut frame,
            CYW43_WLC_SET_VAR,
            CYW43_BCDC_FLAG_SET,
            1,
            &join_pref_payload[..payload_len],
        )
        .expect("join_pref BCDC frame should fit");
        let info = cyw43_control_iovar_info(&frame[..len], CYW43_WLC_SET_VAR)
            .expect("join_pref iovar should decode");
        let value_start = CYW43_BCDC_HEADER_BYTES + name_len + 1;

        assert_eq!(CYW43_LINUX_PREJOIN_MPC_VALUE, None);
        assert_eq!(
            CYW43_LINUX_CONNECT_STATION_POLICY_IOVARS[0],
            ("mpc", CYW43_CONNECT_STATION_POLICY_DISABLED)
        );
        assert_eq!(info.name, "join_pref");
        assert_eq!(info.data_len, CYW43_LINUX_JOIN_PREF_DEFAULT.len());
        assert_eq!(info.value_u32, None);
        assert_eq!(
            &frame[value_start..value_start + CYW43_LINUX_JOIN_PREF_DEFAULT.len()],
            &CYW43_LINUX_JOIN_PREF_DEFAULT
        );

        for (cmd, value) in [
            (
                CYW43_WLC_SET_SCAN_CHANNEL_TIME,
                CYW43_LINUX_SCAN_CHANNEL_TIME_MS,
            ),
            (
                CYW43_WLC_SET_SCAN_UNASSOC_TIME,
                CYW43_LINUX_SCAN_UNASSOC_TIME_MS,
            ),
        ] {
            let mut scan_frame = [0u8; MAX_DRIVER_TASK_FRAME_BYTES];
            let len = cyw43_write_bcdc_frame(
                &mut scan_frame,
                cmd,
                CYW43_BCDC_FLAG_SET,
                2,
                &value.to_le_bytes(),
            )
            .expect("scan timing BCDC frame should fit");

            assert_eq!(
                cyw43_read_le_u32(&scan_frame[..len], CYW43_BCDC_HEADER_BYTES),
                Some(value)
            );
        }
    }

    #[test]
    fn cyw43_set_ssid_fallback_payload_is_bounded_and_redacted() {
        let credentials = crate::net::WifiCredentials::new("DachshundHub", "passphrase")
            .expect("valid wifi credentials");
        let mut payload = [0u8; 36];
        let ssid_len = usize::from(credentials.ssid_len);

        payload[..4].copy_from_slice(&(ssid_len as u32).to_le_bytes());
        payload[4..4 + ssid_len].copy_from_slice(&credentials.ssid[..ssid_len]);

        assert_eq!(payload.len(), 36);
        assert_eq!(cyw43_read_le_u32(&payload, 0), Some(12));
        assert_eq!(&payload[4..16], b"DachshundHub");
        assert_ne!(cyw43_redacted_payload_digest(&payload), 0);
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

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_optional_iovar_submit_fault_is_fail_soft_only_for_optional_controls() {
        let submit_fault = DriverTaskCompletionRecord {
            sequence: 7,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: 0x5103,
            result: 0x0420_8000,
            frame: DriverFrameDescriptor {
                offset: 768,
                len: 56,
                flags: 0,
            },
        };
        let descriptor_unavailable = DriverTaskCompletionRecord {
            detail: 0x5102,
            ..submit_fault
        };
        let reply_unsupported = DriverTaskCompletionRecord {
            detail: CYW43_CONTROL_EXCHANGE_FAULT_DETAIL,
            result: CYW43_BCME_UNSUPPORTED_STATUS,
            ..submit_fault
        };

        for (name, stage) in [
            ("mfp", "cyw43-control-rsn-mfp"),
            ("arp_ol", "cyw43-control-connect-arp-ol"),
            ("arpoe", "cyw43-control-connect-arpoe"),
            ("ndoe", "cyw43-control-connect-ndoe"),
        ] {
            assert!(cyw43_optional_iovar_submit_fault_is_fail_soft(
                name,
                stage,
                submit_fault
            ));
        }

        assert!(!cyw43_optional_iovar_submit_fault_is_fail_soft(
            "wme_bss_disable",
            "cyw43-control-rsn-wme-bss-disable",
            submit_fault
        ));
        assert!(!cyw43_optional_iovar_submit_fault_is_fail_soft(
            "mpc",
            "cyw43-control-connect-mpc",
            submit_fault
        ));
        assert!(!cyw43_optional_iovar_submit_fault_is_fail_soft(
            "ndoe",
            "cyw43-control-join-ssid",
            submit_fault
        ));
        assert!(!cyw43_optional_iovar_submit_fault_is_fail_soft(
            "mfp",
            "cyw43-control-rsn-mfp",
            descriptor_unavailable
        ));
        assert!(!cyw43_optional_iovar_submit_fault_is_fail_soft(
            "mfp",
            "cyw43-control-rsn-mfp",
            reply_unsupported
        ));
        assert!(cyw43_control_exchange_completion_is_unsupported(
            reply_unsupported
        ));
        assert!(!cyw43_runtime_command_fault_uart_trace_enabled(
            "cyw43-control-rsn-mfp",
            submit_fault
        ));
        assert!(!cyw43_runtime_command_fault_uart_trace_enabled(
            "cyw43-control-ulp-sdioctrl",
            reply_unsupported
        ));
        assert!(cyw43_runtime_command_fault_uart_trace_enabled(
            "cyw43-control-connect-mpc",
            submit_fault
        ));
        assert!(cyw43_runtime_command_fault_uart_trace_enabled(
            "cyw43-control-rsn-mfp",
            descriptor_unavailable
        ));
    }

    #[test]
    fn cyw43_control_exchange_descriptor_uses_plain_startup_header_mode() {
        let descriptor = cyw43_control_exchange_descriptor(
            36,
            CYW43_WLC_SET_VAR,
            1,
            Cyw43ControlHeaderMode::Plain,
            false,
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
            false,
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
    fn cyw43_prejoin_policy_does_not_reenable_mpc_before_association() {
        assert_eq!(CYW43_LINUX_PREJOIN_MPC_VALUE, None);
        assert_eq!(
            CYW43_LINUX_CONNECT_STATION_POLICY_IOVARS[0],
            ("mpc", CYW43_CONNECT_STATION_POLICY_DISABLED)
        );
        assert!(!cyw43_control_uses_runtime_exchange(
            "cyw43-control-prejoin-mpc",
            "mpc"
        ));
    }

    #[test]
    fn host_eapol_wsec_key_uses_split_pre_tx_drain_frame_path() {
        let descriptor =
            cyw43_control_frame_descriptor(189, Cyw43ControlHeaderMode::Extended, true);

        assert_eq!(descriptor.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(
            descriptor.flags,
            DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
                | DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN
        );
        assert_eq!(descriptor.payload_len, 189);
        assert_eq!(descriptor.total_len, 189);
        assert!(!cyw43_control_uses_runtime_exchange(
            "cyw43-host-eapol-ptk",
            "wsec_key"
        ));
        assert!(!cyw43_control_uses_runtime_exchange(
            "cyw43-host-eapol-gtk",
            "wsec_key"
        ));
        assert!(!cyw43_control_uses_runtime_exchange(
            "cyw43-control-security-wpa2-psk",
            "wsec"
        ));
        assert_eq!(
            cyw43_control_exchange_poll_attempts("cyw43-host-eapol-ptk", "wsec_key"),
            CYW43_HOST_EAPOL_WSEC_KEY_POLL_ATTEMPTS
        );
    }

    #[test]
    fn host_eapol_promisc_uses_split_control_frame_with_pre_tx_drain() {
        let descriptor = cyw43_control_frame_descriptor(
            20,
            Cyw43ControlHeaderMode::Extended,
            CYW43_HOST_EAPOL_PROMISC_PRE_TX_DRAIN,
        );

        assert_eq!(descriptor.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(
            descriptor.flags,
            DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
                | DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN
        );
        assert_eq!(descriptor.payload_len, 20);
        assert_eq!(descriptor.total_len, 20);
        assert_eq!(
            cyw43_control_request_expected_response_len(CYW43_WLC_SET_PROMISC, None),
            0
        );
        for stage in [
            "cyw43-host-eapol-promisc",
            "cyw43-host-eapol-refresh-promisc",
            "cyw43-host-eapol-rescue-promisc",
            "cyw43-host-eapol-restore-promisc",
        ] {
            assert!(!cyw43_control_uses_runtime_exchange(stage, "none"));
            assert!(cyw43_control_stage_is_host_eapol_promisc(stage));
        }
        assert!(!cyw43_control_uses_runtime_exchange(
            "cyw43-control-infra",
            "none"
        ));
    }

    #[test]
    fn host_eapol_optional_filter_reject_classifies_zero_id_badarg_reply() {
        let badarg = Cyw43ControlReply {
            cmd: 0,
            id: 0,
            status: CYW43_BCME_BADARG_STATUS,
            response_len: 0,
            payload_available: 0,
        };
        let unsupported = Cyw43ControlReply {
            status: CYW43_BCME_UNSUPPORTED_STATUS,
            ..badarg
        };
        let matched_command_badarg = Cyw43ControlReply {
            cmd: CYW43_WLC_SET_PROMISC,
            id: 39,
            ..badarg
        };

        assert!(cyw43_control_stage_is_optional_host_eapol_filter(
            "cyw43-host-eapol-allmulti"
        ));
        assert!(cyw43_control_stage_is_optional_host_eapol_filter(
            "cyw43-host-eapol-promisc"
        ));
        assert!(cyw43_control_stage_is_optional_host_eapol_filter(
            "cyw43-host-eapol-restore-promisc"
        ));
        assert!(!cyw43_control_stage_is_optional_host_eapol_filter(
            "cyw43-host-eapol-mcast"
        ));
        assert!(!cyw43_control_stage_is_optional_host_eapol_filter(
            "cyw43-control-infra"
        ));

        assert!(cyw43_control_reply_is_optional_filter_reject(badarg));
        assert!(cyw43_control_reply_is_optional_filter_reject(unsupported));
        assert!(!cyw43_control_reply_is_optional_filter_reject(
            matched_command_badarg
        ));
        assert!(cyw43_control_reply_is_commandless_reject(badarg));
        assert!(cyw43_control_reply_is_commandless_reject(unsupported));
        assert!(!cyw43_control_reply_is_commandless_reject(
            matched_command_badarg
        ));
        assert!(
            cyw43_control_reply_is_host_eapol_wsec_key_commandless_reject(
                "cyw43-host-eapol-ptk",
                "wsec_key",
                badarg
            )
        );
        assert!(
            cyw43_control_reply_is_host_eapol_wsec_key_commandless_reject(
                "cyw43-host-eapol-post-secure-ptk",
                "wsec_key",
                unsupported
            )
        );
        assert!(
            !cyw43_control_reply_is_host_eapol_wsec_key_commandless_reject(
                "cyw43-host-eapol-allmulti",
                "allmulti",
                badarg
            )
        );
        assert!(
            !cyw43_control_reply_is_host_eapol_wsec_key_commandless_reject(
                "cyw43-host-eapol-ptk",
                "wsec_key",
                matched_command_badarg
            )
        );
    }

    #[test]
    fn host_eapol_rx_admission_controls_use_split_extended_reply_window() {
        for (stage, iovar) in [
            ("cyw43-host-eapol-mcast", "mcast_list"),
            ("cyw43-host-eapol-allmulti", "allmulti"),
            ("cyw43-host-eapol-promisc", "none"),
            ("cyw43-host-eapol-refresh-mcast", "mcast_list"),
            ("cyw43-host-eapol-refresh-allmulti", "allmulti"),
            ("cyw43-host-eapol-refresh-promisc", "none"),
            ("cyw43-host-eapol-rescue-mcast", "mcast_list"),
            ("cyw43-host-eapol-rescue-allmulti", "allmulti"),
            ("cyw43-host-eapol-rescue-promisc", "none"),
            ("cyw43-host-eapol-restore-mcast", "mcast_list"),
            ("cyw43-host-eapol-restore-allmulti", "allmulti"),
            ("cyw43-host-eapol-restore-promisc", "none"),
        ] {
            assert!(!cyw43_control_uses_runtime_exchange(stage, iovar));
            assert!(cyw43_control_stage_is_host_eapol_rx_admission(stage));
            assert!(cyw43_control_uses_host_eapol_rx_admission_reply_window(
                stage, iovar
            ));
            assert_eq!(
                cyw43_control_exchange_poll_attempts(stage, iovar),
                CYW43_HOST_EAPOL_RX_ADMISSION_POLL_ATTEMPTS
            );
            assert_eq!(
                cyw43_control_exchange_timeout_ms(stage, iovar),
                CYW43_HOST_EAPOL_RX_ADMISSION_REPLY_TIMEOUT_MS
            );
        }

        assert_eq!(
            cyw43_control_request_expected_response_len(CYW43_WLC_SET_VAR, None),
            0
        );
        assert_eq!(
            cyw43_control_request_expected_response_len(CYW43_WLC_SET_PROMISC, None),
            0
        );
        assert!(!cyw43_control_uses_host_eapol_rx_admission_reply_window(
            "cyw43-control-auth",
            "auth"
        ));
    }

    #[test]
    fn split_control_poll_keeps_oldgood_late_hintless_firstread_cadence() {
        for poll in [1, 4, 16, 64, 256, 1024, 4096] {
            assert_eq!(
                cyw43_control_split_poll_flags(poll),
                DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD
            );
        }
        for poll in [2, 8, 128, 512, 2048, 4095] {
            assert_eq!(cyw43_control_split_poll_flags(poll), 0);
        }
        assert!(CYW43_HOST_EAPOL_RX_ADMISSION_POLL_ATTEMPTS >= 4096);
    }

    #[test]
    fn post_secure_eapol_keeps_broadcast_data_admitted_for_dhcp() {
        assert_eq!(CYW43_POST_SECURE_DATA_ALLMULTI, 1);
        assert_eq!(CYW43_POST_SECURE_DATA_PROMISC, 1);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn post_secure_data_rx_reassert_waits_for_secure_eapol() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 replay tests must serialize");
        reset_cyw43_status_flags();
        let _io_guard = test_enable_cyw43_host_eapol_io_stub();

        assert!(!reassert_cyw43_post_secure_data_rx(
            CYW43_WIFI_DRIVER_TASK_CONTRACT
        ));
        assert_eq!(CYW43_HOST_EAPOL_TEST_RX_RESTORED.load(Ordering::Acquire), 0);

        CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);

        assert!(reassert_cyw43_post_secure_data_rx(
            CYW43_WIFI_DRIVER_TASK_CONTRACT
        ));
        assert_eq!(CYW43_HOST_EAPOL_TEST_RX_RESTORED.load(Ordering::Acquire), 1);
        assert_eq!(
            CYW43_POST_SECURE_DATA_RX_ADMITTED.load(Ordering::Acquire),
            1
        );

        reset_cyw43_status_flags();
    }

    #[test]
    fn cyw43_control_frame_descriptor_uses_split_tx_op() {
        let plain = cyw43_control_frame_descriptor(36, Cyw43ControlHeaderMode::Plain, false);
        let extended = cyw43_control_frame_descriptor(16, Cyw43ControlHeaderMode::Extended, false);
        let drained_plain = cyw43_control_frame_descriptor(36, Cyw43ControlHeaderMode::Plain, true);
        let drained_extended =
            cyw43_control_frame_descriptor(16, Cyw43ControlHeaderMode::Extended, true);

        assert_eq!(plain.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(plain.flags, 0);
        assert_eq!(plain.payload_len, 36);
        assert_eq!(plain.total_len, 36);
        assert_eq!(extended.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(extended.flags, DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER);
        assert_eq!(drained_plain.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(
            drained_plain.flags,
            DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN
        );
        assert_eq!(drained_plain.payload_len, 36);
        assert_eq!(drained_plain.total_len, 36);
        assert_eq!(drained_extended.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(
            drained_extended.flags,
            DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
                | DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN
        );
    }

    #[test]
    fn cyw43_glom_control_uses_split_frame_without_rx_pre_drain() {
        let txglom_descriptor =
            cyw43_control_frame_descriptor(36, Cyw43ControlHeaderMode::Plain, false);

        assert_eq!(txglom_descriptor.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(txglom_descriptor.flags, 0);
        assert_eq!(txglom_descriptor.payload_len, 36);
        assert_eq!(txglom_descriptor.total_len, 36);
        assert_eq!(
            cyw43_control_runtime_flags(Cyw43ControlHeaderMode::Plain, false),
            txglom_descriptor.flags
        );
        let rxglom_descriptor =
            cyw43_control_frame_descriptor(36, Cyw43ControlHeaderMode::Plain, false);
        assert_eq!(rxglom_descriptor.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(rxglom_descriptor.flags, 0);
        assert!(!cyw43_control_uses_runtime_exchange(
            "cyw43-control-txglomalign",
            "bus:txglomalign"
        ));
        assert!(!cyw43_control_uses_runtime_exchange(
            "cyw43-control-rxglom",
            "bus:rxglom"
        ));
        assert!(!cyw43_control_uses_runtime_exchange(
            "cyw43-control-ulp-sdioctrl",
            "ulp_sdioctrl"
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_split_control_fault_status_preserves_expected_metadata() {
        test_clear_cyw43_runtime_replay_status();

        let descriptor = cyw43_control_frame_descriptor(36, Cyw43ControlHeaderMode::Plain, false);
        record_cyw43_control_split_failure(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            "cyw43-control-txglomalign",
            descriptor,
            "cyw43-control-tx-retry-no-reply",
            None,
            CYW43_WLC_SET_VAR,
            1,
            Cyw43ControlHeaderMode::Plain,
            0,
            "bus:txglomalign",
            0,
            0,
        );

        let fault = latest_cyw43_runtime_command_fault_status().unwrap();
        assert_eq!(fault.stage, "cyw43-control-txglomalign");
        assert_eq!(fault.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(fault.flags, 0);
        assert_eq!(fault.control_cmd, CYW43_WLC_SET_VAR);
        assert_eq!(fault.control_id, 1);
        assert_eq!(fault.control_header_mode, "plain");
        assert_eq!(fault.control_response_len, 0);
        assert_eq!(fault.reason, "cyw43-control-tx-retry-no-reply");
    }

    #[test]
    fn cyw43_bssid_refresh_keeps_split_tx_undrained() {
        let descriptor = cyw43_control_frame_descriptor(
            16,
            Cyw43ControlHeaderMode::Extended,
            CYW43_HOST_EAPOL_BSSID_REFRESH_PRE_TX_DRAIN,
        );

        assert_eq!(descriptor.op, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME);
        assert_eq!(
            descriptor.flags,
            DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER
        );
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
    fn cyw43_control_reply_parser_rejects_sdio_telemetry_prefix() {
        let mut buffer = [0u8; MAX_FRAME_LEN];
        buffer[0..4].copy_from_slice(&SDIO_FAULT_TELEMETRY_MAGIC.to_le_bytes());
        buffer[4..8].copy_from_slice(&1u32.to_le_bytes());
        buffer[10..12].copy_from_slice(&40320u16.to_le_bytes());
        buffer[12..16].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        let token = DriverTaskNetRxToken { len: 17, buffer };

        assert!(cyw43_control_reply_from_token(&token).is_none());
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
    fn cyw43_pending_control_reply_matches_exact_cmd_id() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 pending-control tests must serialize");
        clear_cyw43_pending_control_replies();
        let body = [0x11, 0x22, 0x33, 0x44];
        let token = test_control_reply_token(CYW43_WLC_SET_VAR, 54, 0, &body);

        assert!(store_cyw43_pending_control_reply(
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL,
            token,
            99
        ));
        assert!(take_cyw43_pending_control_reply(CYW43_WLC_SET_VAR, 55).is_none());
        let pending = take_cyw43_pending_control_reply(CYW43_WLC_SET_VAR, 54)
            .expect("exact command id must retrieve pending control reply");
        let reply =
            cyw43_control_reply_from_token(&pending.token).expect("cached reply remains parseable");

        assert_eq!(pending.sequence, 99);
        assert_eq!(reply.cmd, CYW43_WLC_SET_VAR);
        assert_eq!(reply.id, 54);
        assert_eq!(reply.response_len, body.len() as u32);
        assert!(take_cyw43_pending_control_reply(CYW43_WLC_SET_VAR, 54).is_none());
    }

    #[test]
    fn cyw43_pending_control_reply_restages_copied_body_after_ring_overwrite() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 pending-control tests must serialize");
        clear_cyw43_pending_control_replies();
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        let _ring = test_publish_cyw43_ring(&mut ring_page);
        let body = [0xde, 0xad, 0xbe, 0xef, 0x55, 0xaa];
        let token = test_control_reply_token(CYW43_WLC_SET_VAR, 7, 0, &body);

        assert!(store_cyw43_pending_control_reply(
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL,
            token,
            123
        ));
        let overwrite = [0xa5u8; 64];
        crate::hal::driver_task::stage_driver_task_ring_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &overwrite,
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
        )
        .expect("test ring overwrite should stage");
        let (completion, reply) = take_cyw43_pending_control_reply_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            CYW43_WLC_SET_VAR,
            7,
        )
        .expect("cached control reply should restage");
        let response = cyw43_control_response_completion(completion, body.len())
            .expect("restaged response body should have a valid descriptor");
        let offset = response.frame.offset as usize;
        let end = offset + response.frame.len as usize;

        assert_eq!(reply.id, 7);
        assert_eq!(completion.sequence, 123);
        assert_eq!(&ring_page[offset..end], &body);
        assert_eq!(
            response.frame.flags,
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL
        );
    }

    #[test]
    fn host_eapol_control_completion_preserves_cdc_reply() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 pending-control tests must serialize");
        clear_cyw43_pending_control_replies();
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        let _ring = test_publish_cyw43_ring(&mut ring_page);
        let body = [0x4a, 0x4b, 0x4c, 0x4d];
        let token = test_control_reply_token(CYW43_WLC_SET_VAR, 88, 0, &body);
        let completion = test_stage_cyw43_completion(
            &token.buffer[..token.len],
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL,
            321,
        );

        let result = process_cyw43_host_eapol_control_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &mut session,
            42,
            DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
            completion,
        );
        let pending = take_cyw43_pending_control_reply(CYW43_WLC_SET_VAR, 88)
            .expect("host-EAPOL control drain must preserve CDC replies");

        assert!(result.completed);
        assert!(result.observed_frame);
        assert_eq!(pending.sequence, 321);
        assert_eq!(pending.id, 88);
    }

    #[test]
    fn cyw43_matched_control_reply_validation_reuses_response_completion_shape() {
        let descriptor = cyw43_control_exchange_descriptor(
            189,
            CYW43_WLC_SET_VAR,
            55,
            Cyw43ControlHeaderMode::Extended,
            true,
        );
        let completion = DriverTaskCompletionRecord {
            sequence: 72,
            code: DriverTaskCompletionCode::FrameReady.as_u16(),
            detail: 0,
            result: 189,
            frame: DriverFrameDescriptor {
                offset: 256,
                len: 189,
                flags: DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL,
            },
        };
        let reply = Cyw43ControlReply {
            cmd: CYW43_WLC_SET_VAR,
            id: 55,
            status: 0,
            response_len: 173,
            payload_available: 173,
        };

        let response = match cyw43_control_response_completion_from_reply(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            "cyw43-host-eapol-post-secure-ptk",
            descriptor,
            completion,
            reply,
            CYW43_WLC_SET_VAR,
            55,
            Cyw43ControlHeaderMode::Extended,
            0,
            "wsec_key",
            1,
            0,
        ) {
            Ok(response) => response,
            Err(_) => panic!("matched zero-status reply should complete"),
        };

        assert_eq!(response.sequence, 72);
        assert_eq!(response.code, DriverTaskCompletionCode::FrameReady.as_u16());
        assert_eq!(response.result, 173);
        assert_eq!(response.frame.offset, 256 + CYW43_BCDC_HEADER_BYTES as u32);
        assert_eq!(response.frame.len, 173);
        assert_eq!(
            response.frame.flags,
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL
        );
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
        assert_eq!(
            dev.bringup_status_label(),
            Some("wifi-data-rx-admission-blocked")
        );
        assert!(
            dev.transmit(Instant::from_millis(0)).is_none(),
            "secure carrier still needs explicit post-secure data RX admission"
        );

        CYW43_POST_SECURE_DATA_RX_ADMITTED.store(1, Ordering::Release);
        assert!(
            dev.transmit(Instant::from_millis(0)).is_some(),
            "descriptor data TX should become available after post-secure data RX admission"
        );

        CYW43_LINKED_RUNTIME_READY.store(0, Ordering::Release);
        CYW43_CONTROL_PLANE_READY.store(0, Ordering::Release);
        CYW43_ASSOCIATED.store(0, Ordering::Release);
        CYW43_LINK_UP.store(0, Ordering::Release);
        CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);
        CYW43_POST_SECURE_DATA_RX_ADMITTED.store(0, Ordering::Release);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_secure_does_not_manufacture_assoc_or_link() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status-label tests must serialize");
        reset_cyw43_status_flags();

        let mut progress = Cyw43HostEapolProgress {
            eapol_rx: 2,
            ..Cyw43HostEapolProgress::default()
        };
        mark_cyw43_host_eapol_secure(CYW43_WIFI_DRIVER_TASK_CONTRACT, &progress);

        assert_eq!(CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_HOST_EAPOL_REQUIRED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 0);

        reset_cyw43_status_flags();
        progress.associated = true;
        progress.link_up = true;
        CYW43_LINKED_RUNTIME_READY.store(1, Ordering::Release);
        CYW43_ASSOCIATED.store(1, Ordering::Release);
        CYW43_LINK_UP.store(1, Ordering::Release);
        CYW43_HOST_EAPOL_TEST_IO_STUB.store(1, Ordering::Release);
        mark_cyw43_host_eapol_secure(CYW43_WIFI_DRIVER_TASK_CONTRACT, &progress);

        assert_eq!(CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_REQUIRED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_CONTROL_PLANE_READY.load(Ordering::Acquire), 1);
        assert_eq!(
            CYW43_POST_SECURE_DATA_RX_ADMITTED.load(Ordering::Acquire),
            1
        );
        assert_eq!(cyw43_driver_task_bringup_status_label(), None);
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 1);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn post_secure_rx_admission_transport_failure_blocks_data_plane() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status-label tests must serialize");
        reset_cyw43_status_flags();

        let progress = Cyw43HostEapolProgress {
            associated: true,
            link_up: true,
            eapol_rx: 4,
            ..Cyw43HostEapolProgress::default()
        };
        CYW43_LINKED_RUNTIME_READY.store(1, Ordering::Release);
        CYW43_ASSOCIATED.store(1, Ordering::Release);
        CYW43_LINK_UP.store(1, Ordering::Release);
        mark_cyw43_host_eapol_secure(CYW43_WIFI_DRIVER_TASK_CONTRACT, &progress);

        assert_eq!(CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_CONTROL_PLANE_READY.load(Ordering::Acquire), 0);
        assert_eq!(
            CYW43_POST_SECURE_DATA_RX_ADMITTED.load(Ordering::Acquire),
            0
        );
        assert_eq!(
            cyw43_driver_task_bringup_status_label(),
            Some("wifi-data-rx-admission-blocked")
        );

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_tx_drain_credit_must_cover_submitted_sequence() {
        let no_proof_completion = DriverTaskCompletionRecord::progress(6, 143);
        assert_eq!(cyw43_tx_completion_proof(no_proof_completion), None);

        let mut tx_completion = DriverTaskCompletionRecord::progress(7, 143);
        tx_completion.detail = 144;
        tx_completion.frame = DriverFrameDescriptor {
            offset: 0,
            len: 3,
            flags: 7 | (8u16 << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT),
        };
        let proof = cyw43_tx_completion_proof(tx_completion).expect("TX proof carries sequence");
        assert_eq!(proof.submitted_seq, 7);

        let mut stale_idle = DriverTaskCompletionRecord::idle(8);
        stale_idle.frame = DriverFrameDescriptor {
            offset: 0,
            len: 4,
            flags: 8 | (7u16 << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT),
        };
        assert!(!cyw43_completion_credit_covers_tx(stale_idle, proof));

        let mut fresh_idle = DriverTaskCompletionRecord::idle(9);
        fresh_idle.frame = DriverFrameDescriptor {
            offset: 0,
            len: 5,
            flags: 8 | (8u16 << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT),
        };
        assert!(cyw43_completion_credit_covers_tx(fresh_idle, proof));

        let frame_ready = DriverTaskCompletionRecord::frame_ready(
            10,
            DriverFrameDescriptor {
                offset: 0,
                len: 64,
                flags: DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL
                    | (9u16 << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT),
            },
        );
        assert!(cyw43_completion_credit_covers_tx(frame_ready, proof));

        let mut wrapped_tx_completion = DriverTaskCompletionRecord::progress(11, 144);
        wrapped_tx_completion.detail = 144;
        wrapped_tx_completion.frame = DriverFrameDescriptor {
            offset: 0,
            len: 3,
            flags: 255,
        };
        let wrapped_proof = cyw43_tx_completion_proof(wrapped_tx_completion)
            .expect("wrapped TX proof carries sequence 255");
        assert_eq!(wrapped_proof.submitted_seq, 255);
        assert!(
            cyw43_completion_credit_covers_tx(wrapped_tx_completion, wrapped_proof),
            "SDPCM credit 0 covers submitted seq 255 after u8 wrap"
        );

        let stale_wrap_proof = Cyw43HostEapolTxProof { submitted_seq: 100 };
        assert!(!cyw43_completion_credit_covers_tx(
            wrapped_tx_completion,
            stale_wrap_proof
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_m2_tx_drain_timeout_is_advisory() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 replay tests must serialize");
        reset_cyw43_status_flags();
        let _io_guard = test_enable_cyw43_host_eapol_io_stub();
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        let _ring_guard = test_publish_cyw43_ring(&mut ring_page);

        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        *CYW43_RUNTIME_MAC.lock() = EthernetAddress(station);
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        let mut tx_frame = [0u8; MAX_FRAME_LEN];

        CYW43_HOST_EAPOL_TEST_TX_DRAIN_TIMEOUTS.store(1, Ordering::Release);
        let mut m1 = [0u8; MAX_FRAME_LEN];
        let m1_len =
            cyw43_host_eapol::write_test_m1_frame(&mut m1, &station, &ap).expect("test m1 frame");
        let m1_result = process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            8193,
            0,
            test_stage_cyw43_completion(
                &m1[..m1_len],
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
                1,
            ),
            &mut tx_frame,
        )
        .expect("M2 drain timeout after a submitted M2 must stay advisory");

        assert!(m1_result.observed_frame);
        assert!(!m1_result.secure);
        assert_eq!(session.progress.eapol_error, None);
        assert_eq!(CYW43_HOST_EAPOL_M2.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_TEST_TX_DRAINED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_REQUIRED.load(Ordering::Acquire), 0);

        CYW43_HOST_EAPOL_TEST_TX_DRAIN_TIMEOUTS.store(0, Ordering::Release);
        let mut m3 = [0u8; MAX_FRAME_LEN];
        let m3_len = cyw43_host_eapol::write_test_m3_frame(&mut m3, &station, &session.eapol)
            .expect("test m3 frame");
        let m3_result = process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            8197,
            0,
            test_stage_cyw43_completion(
                &m3[..m3_len],
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
                2,
            ),
            &mut tx_frame,
        )
        .expect("M3 must remain accepted after advisory M2 drain timeout");

        assert!(m3_result.observed_frame);
        assert!(m3_result.secure);
        assert_eq!(session.progress.eapol_error, None);
        assert_eq!(CYW43_HOST_EAPOL_M4.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_PTK.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_GTK.load(Ordering::Acquire), 1);
        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_m4_tx_drain_timeout_is_advisory_before_key_install() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 replay tests must serialize");
        reset_cyw43_status_flags();
        let _io_guard = test_enable_cyw43_host_eapol_io_stub();
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        let _ring_guard = test_publish_cyw43_ring(&mut ring_page);

        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        *CYW43_RUNTIME_MAC.lock() = EthernetAddress(station);
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        let mut tx_frame = [0u8; MAX_FRAME_LEN];

        let mut m1 = [0u8; MAX_FRAME_LEN];
        let m1_len =
            cyw43_host_eapol::write_test_m1_frame(&mut m1, &station, &ap).expect("test m1 frame");
        process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            8193,
            0,
            test_stage_cyw43_completion(
                &m1[..m1_len],
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
                1,
            ),
            &mut tx_frame,
        )
        .expect("M1 replay should submit M2");

        CYW43_HOST_EAPOL_TEST_TX_DRAIN_TIMEOUTS.store(2, Ordering::Release);
        let mut m3 = [0u8; MAX_FRAME_LEN];
        let m3_len = cyw43_host_eapol::write_test_m3_frame(&mut m3, &station, &session.eapol)
            .expect("test m3 frame");
        let m3_result = process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            8197,
            0,
            test_stage_cyw43_completion(
                &m3[..m3_len],
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
                2,
            ),
            &mut tx_frame,
        )
        .expect("M4 drain timeout after submitted M4 must not block key install");

        assert!(m3_result.observed_frame);
        assert!(m3_result.secure);
        assert_eq!(session.progress.eapol_error, None);
        assert_eq!(CYW43_HOST_EAPOL_M4.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_PTK.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_GTK.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_REQUIRED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_HOST_EAPOL_TEST_TX_DRAINED.load(Ordering::Acquire), 2);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_DRAIN_BEFORE_PTK.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_PRE_TX_DRAIN.load(Ordering::Acquire),
            0
        );
        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_missing_m4_tx_proof_remains_fatal() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 replay tests must serialize");
        reset_cyw43_status_flags();

        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");

        let err = wait_cyw43_host_eapol_tx_drain(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &mut session,
            "m4-before-wsec",
            8197,
            DriverTaskCompletionRecord::progress(0, 143),
        )
        .expect_err("missing submitted-sequence proof must stay fatal");

        assert_eq!(
            err,
            DriverTaskNetError::RuntimeInit("host-eapol-tx-drain-proof")
        );
        assert_eq!(CYW43_HOST_EAPOL_PTK.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_HOST_EAPOL_GTK.load(Ordering::Acquire), 0);
        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_session_is_prepared_before_join_arm() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status-label tests must serialize");
        reset_cyw43_status_flags();

        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let session =
            prepare_cyw43_host_eapol_session(CYW43_WIFI_DRIVER_TASK_CONTRACT, credentials)
                .expect("host eapol session prepares before join");

        assert_eq!(session.progress.polls, 0);
        assert_eq!(session.progress.eapol_rx, 0);
        assert_eq!(CYW43_HOST_EAPOL_ACTIVE.load(Ordering::Acquire), 0);
        assert!(CYW43_HOST_EAPOL_SESSION.lock().is_none());

        arm_cyw43_host_eapol_pending(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            session,
            EthernetAddress([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10]),
        )
        .expect("prepared host eapol session arms after join");

        assert_eq!(CYW43_HOST_EAPOL_ACTIVE.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_REQUIRED.load(Ordering::Acquire), 0);
        assert!(CYW43_HOST_EAPOL_SESSION.lock().is_some());

        reset_cyw43_status_flags();
    }

    #[test]
    fn host_eapol_start_cadence_is_bounded() {
        assert_eq!(CYW43_HOST_EAPOL_PRE_ASSOC_POLLS, 8_192);
        assert_eq!(CYW43_HOST_EAPOL_POST_ASSOC_POLLS, 16_384);
        assert_eq!(CYW43_HOST_EAPOL_JOIN_POLLS, 24_576);
        assert_eq!(CYW43_HOST_EAPOL_PRE_ASSOC_TIMEOUT_MS, 8_192);
        assert_eq!(CYW43_HOST_EAPOL_POST_ASSOC_TIMEOUT_MS, 16_384);
        assert_eq!(CYW43_HOST_EAPOL_JOIN_TIMEOUT_MS, 24_576);
        assert_eq!(CYW43_HOST_EAPOL_START_FIRST_MS, 8_192);
        assert_eq!(CYW43_HOST_EAPOL_START_INTERVAL_MS, 8_192);
        assert_eq!(
            CYW43_HOST_EAPOL_JOIN_SUBMIT_POLLS,
            CYW43_HOST_EAPOL_PRE_ASSOC_POLLS + 1
        );
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

        assert!(cyw43_host_eapol_start_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL,
            0
        ));
        assert!(
            cyw43_host_eapol_start_due(CYW43_HOST_EAPOL_START_FIRST_POLL, 0),
            "observed M1/M2 traffic must not suppress bounded EAPOL-Start retries before M3"
        );
        assert!(!cyw43_host_eapol_start_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL,
            CYW43_HOST_EAPOL_START_MAX
        ));
        assert!(!cyw43_host_eapol_start_due_ms(
            CYW43_HOST_EAPOL_START_FIRST_MS - 1,
            0,
            None,
            CYW43_HOST_EAPOL_START_FIRST_MS - 1
        ));
        assert!(cyw43_host_eapol_start_due_ms(
            CYW43_HOST_EAPOL_START_FIRST_MS,
            0,
            None,
            CYW43_HOST_EAPOL_START_FIRST_MS
        ));
        assert!(!cyw43_host_eapol_start_due_ms(
            CYW43_HOST_EAPOL_START_FIRST_MS + 1,
            1,
            Some(CYW43_HOST_EAPOL_START_FIRST_MS),
            CYW43_HOST_EAPOL_START_FIRST_MS + 1
        ));
        assert!(cyw43_host_eapol_start_due_ms(
            CYW43_HOST_EAPOL_START_FIRST_MS + CYW43_HOST_EAPOL_START_INTERVAL_MS,
            1,
            Some(CYW43_HOST_EAPOL_START_FIRST_MS),
            CYW43_HOST_EAPOL_START_FIRST_MS + CYW43_HOST_EAPOL_START_INTERVAL_MS
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_timeout_window_restarts_after_association() {
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        session.started_ms = Some(0);

        session.progress.polls = CYW43_HOST_EAPOL_JOIN_POLLS as u32 - 1;
        assert!(!cyw43_host_eapol_join_timeout_expired(&session, 0));
        session.progress.polls = CYW43_HOST_EAPOL_JOIN_POLLS as u32;
        assert!(
            !cyw43_host_eapol_join_timeout_expired(&session, 0),
            "poll progress must not stand in for elapsed association time"
        );
        session.progress.polls = 0;
        assert!(cyw43_host_eapol_join_timeout_expired(
            &session,
            CYW43_HOST_EAPOL_JOIN_TIMEOUT_MS
        ));

        let associated_at = CYW43_HOST_EAPOL_JOIN_TIMEOUT_MS + 1;
        session.progress.associated = true;
        session.record_time(associated_at);

        assert!(!cyw43_host_eapol_join_timeout_expired(
            &session,
            associated_at
        ));
        assert!(!cyw43_host_eapol_join_timeout_expired(
            &session,
            associated_at + CYW43_HOST_EAPOL_POST_ASSOC_TIMEOUT_MS - 1
        ));
        session.progress.post_assoc_polls = CYW43_HOST_EAPOL_POST_ASSOC_POLLS as u32 - 1;
        assert!(!cyw43_host_eapol_join_timeout_expired(
            &session,
            associated_at
        ));
        session.progress.post_assoc_polls = CYW43_HOST_EAPOL_POST_ASSOC_POLLS as u32;
        assert!(
            !cyw43_host_eapol_join_timeout_expired(&session, associated_at),
            "poll progress must not stand in for elapsed post-association time"
        );
        session.progress.post_assoc_polls = 0;
        assert!(cyw43_host_eapol_join_timeout_expired(
            &session,
            associated_at + CYW43_HOST_EAPOL_POST_ASSOC_TIMEOUT_MS
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_activity_extends_post_assoc_timeout() {
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        let associated_at = 10_000;
        session.progress.associated = true;
        session.record_time(associated_at);
        session.progress.post_assoc_polls = CYW43_HOST_EAPOL_POST_ASSOC_POLLS as u32;

        let old_deadline = associated_at + CYW43_HOST_EAPOL_POST_ASSOC_TIMEOUT_MS;
        assert!(
            cyw43_host_eapol_join_timeout_expired(&session, old_deadline),
            "without EAPOL activity the association-time deadline still applies"
        );

        let m1_replay_ms = old_deadline.saturating_sub(1);
        let m1_replay_poll = 12_288;
        session.progress.eapol_rx = 2;
        session.progress.polls = m1_replay_poll + 1;
        session.record_eapol_activity(m1_replay_ms, m1_replay_poll);
        assert!(
            !cyw43_host_eapol_join_timeout_expired(&session, old_deadline),
            "a fresh M1/M2 exchange must keep the handshake pending for late M3"
        );
        assert!(cyw43_host_eapol_join_timeout_expired(
            &session,
            m1_replay_ms + CYW43_HOST_EAPOL_POST_ASSOC_TIMEOUT_MS
        ));
        session.progress.polls = m1_replay_poll + CYW43_HOST_EAPOL_POST_ASSOC_POLLS as u32;
        assert!(
            !cyw43_host_eapol_join_timeout_expired(&session, m1_replay_ms + 1),
            "post-association poll progress must not expire a fresh EAPOL activity window"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_pre_assoc_activity_extends_join_timeout() {
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        let started_at = 10_000;
        session.record_time(started_at);
        session.progress.polls = CYW43_HOST_EAPOL_JOIN_POLLS as u32;

        let old_deadline = started_at + CYW43_HOST_EAPOL_JOIN_TIMEOUT_MS;
        assert!(
            cyw43_host_eapol_join_timeout_expired(&session, old_deadline),
            "without association activity the original join deadline still applies"
        );

        let rescue_ms = old_deadline.saturating_sub(1);
        let rescue_poll = CYW43_HOST_EAPOL_PRE_ASSOC_POLLS as u32;
        session.progress.polls = rescue_poll + 1;
        session.record_pre_assoc_activity(rescue_ms, rescue_poll);
        assert!(
            !cyw43_host_eapol_join_timeout_expired(&session, old_deadline),
            "a fresh auth event or join rescue must keep association pending"
        );
        assert!(cyw43_host_eapol_join_timeout_expired(
            &session,
            rescue_ms + CYW43_HOST_EAPOL_JOIN_TIMEOUT_MS
        ));
        session.progress.polls = rescue_poll + CYW43_HOST_EAPOL_JOIN_POLLS as u32;
        assert!(
            !cyw43_host_eapol_join_timeout_expired(&session, rescue_ms + 1),
            "pre-association poll progress must not expire a fresh rescue window"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_assoc_rescue_restarts_pre_assoc_window() {
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        let started_at = 20_000;
        session.record_time(started_at);
        session.progress.polls = CYW43_HOST_EAPOL_ASSOC_RESCUE_POLL;
        session.progress.empty_polls = CYW43_HOST_EAPOL_ASSOC_RESCUE_POLL;
        session.assoc_probe_attempts = 3;
        session.bssid_probed_before_required = true;
        session
            .progress
            .record_assoc_probe("not-associated", CYW43_BCME_NOTASSOCIATED_STATUS);
        session.progress.record_assoc_join_rescue_attempt();
        let auth_timeout = Cyw43EventFrame {
            src_mac: [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10],
            event_type: CYW43_EVENT_AUTH,
            status: CYW43_EVENT_STATUS_TIMEOUT,
            reason: 2,
            auth_type: 0,
            addr: [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5],
            flags: 0,
        };
        session.progress.record_event_frame(
            0x4f01,
            78,
            auth_timeout,
            session.progress.polls as usize,
        );

        let rescue_at = started_at + CYW43_HOST_EAPOL_ASSOC_RESCUE_MS;
        session.restart_pre_assoc_window_after_rescue(rescue_at);

        assert_eq!(session.progress.polls, 0);
        assert_eq!(session.progress.empty_polls, 0);
        assert_eq!(session.assoc_probe_attempts, 0);
        assert!(!session.bssid_probed_before_required);
        assert!(session.progress.assoc_join_rescue_attempted);
        assert!(session.progress.auth_timeout_seen);
        assert!(
            session.take_post_rescue_assoc_window_due(),
            "a ready rescue join must request one fresh bounded association window"
        );
        assert!(
            !session.take_post_rescue_assoc_window_due(),
            "the follow-up window request is one-shot"
        );
        assert!(
            !cyw43_host_eapol_join_timeout_expired(&session, rescue_at + 1),
            "the fresh post-rescue window must not expire immediately"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_gate7_subgate_tracks_join_progress() {
        let _guard = CYW43_STATUS_TEST_LOCK.lock().expect("status test lock");
        reset_cyw43_status_flags();

        let mut progress = Cyw43HostEapolProgress::default();
        assert_eq!(
            cyw43_wifi_gate7_host_eapol_subgate("pending", &progress),
            ("7a", "join-submit", "join-accepted")
        );
        assert_eq!(
            cyw43_wifi_gate7_host_eapol_subgate("required", &progress),
            ("7b", "association", "cyw43-association-event-missing")
        );

        progress.polls = 1;
        assert_eq!(
            cyw43_wifi_gate7_host_eapol_subgate("pending", &progress),
            ("7b", "association", "cyw43-association-event-missing")
        );

        progress.associated = true;
        progress.link_up = false;
        assert_eq!(
            cyw43_wifi_gate7_host_eapol_subgate("pending", &progress),
            ("7c", "eapol-rx", "waiting-m1")
        );

        progress.eapol_rx = 1;
        assert_eq!(
            cyw43_wifi_gate7_host_eapol_subgate("pending", &progress),
            ("7d", "eapol-handshake", "waiting-keys")
        );

        progress.link_up = true;
        CYW43_HOST_EAPOL_M4.store(1, Ordering::Release);
        CYW43_HOST_EAPOL_PTK.store(1, Ordering::Release);
        CYW43_HOST_EAPOL_GTK.store(1, Ordering::Release);
        assert_eq!(
            cyw43_wifi_gate7_host_eapol_subgate("pending", &progress),
            ("7e", "secure-release", "waiting-carrier-release")
        );

        CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);
        assert_eq!(
            cyw43_wifi_gate7_host_eapol_subgate("secure", &progress),
            ("7e", "secure-release", "passed")
        );

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_status_throttle_suppresses_poll_only_pending_repeats() {
        let _guard = CYW43_STATUS_TEST_LOCK.lock().expect("status test lock");
        reset_cyw43_status_flags();

        let progress = Cyw43HostEapolProgress::default();
        let next_action = cyw43_host_eapol_next_action("pending", &progress);
        let key = cyw43_host_eapol_status_key("pending", "none", next_action, 0, 0, &progress);
        let mut throttle = Cyw43HostEapolStatusThrottle::default();

        let first = cyw43_host_eapol_status_log_decision(&mut throttle, key);
        let second = cyw43_host_eapol_status_log_decision(&mut throttle, key);

        assert!(first.emit_full);
        assert_eq!(first.suppressed_before, 0);
        assert!(!second.emit_full);
        assert_eq!(throttle.suppressed, 1);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_status_throttle_emits_real_progress_and_terminal_status() {
        let _guard = CYW43_STATUS_TEST_LOCK.lock().expect("status test lock");
        reset_cyw43_status_flags();

        let mut throttle = Cyw43HostEapolStatusThrottle::default();
        let mut progress = Cyw43HostEapolProgress::default();
        let pending_action = cyw43_host_eapol_next_action("pending", &progress);
        let pending_key =
            cyw43_host_eapol_status_key("pending", "none", pending_action, 0, 0, &progress);
        assert!(cyw43_host_eapol_status_log_decision(&mut throttle, pending_key).emit_full);
        assert!(!cyw43_host_eapol_status_log_decision(&mut throttle, pending_key).emit_full);

        progress.polls = 4096;
        progress.empty_polls = 4096;
        let poll_only_key =
            cyw43_host_eapol_status_key("pending", "none", pending_action, 0, 0, &progress);
        assert_eq!(pending_key, poll_only_key);
        assert!(!cyw43_host_eapol_status_log_decision(&mut throttle, poll_only_key).emit_full);

        progress.last_rx_trace.valid = true;
        progress.last_rx_trace.pre_intstatus = 0x20;
        progress.last_rx_trace.source_empty_polls = 1;
        let trace_key =
            cyw43_host_eapol_status_key("pending", "none", pending_action, 0, 0, &progress);
        let trace_decision = cyw43_host_eapol_status_log_decision(&mut throttle, trace_key);
        assert!(trace_decision.emit_full);
        assert_eq!(trace_decision.suppressed_before, 2);
        assert!(!cyw43_host_eapol_status_log_decision(&mut throttle, trace_key).emit_full);

        progress.record_data_frame(
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
            42,
            Some(ETH_P_EAPOL),
        );
        let progress_action = cyw43_host_eapol_next_action("pending", &progress);
        let progress_key =
            cyw43_host_eapol_status_key("pending", "none", progress_action, 0, 0, &progress);
        let progress_decision = cyw43_host_eapol_status_log_decision(&mut throttle, progress_key);
        assert!(progress_decision.emit_full);
        assert_eq!(progress_decision.suppressed_before, 1);

        let required_reason = cyw43_host_eapol_required_reason(&progress);
        let required_action = cyw43_host_eapol_next_action("required", &progress);
        let required_key = cyw43_host_eapol_status_key(
            "required",
            required_reason,
            required_action,
            0,
            0,
            &progress,
        );
        assert!(cyw43_host_eapol_status_log_decision(&mut throttle, required_key).emit_full);
        assert!(cyw43_host_eapol_status_log_decision(&mut throttle, required_key).emit_full);
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
    fn host_eapol_bssid_refresh_does_not_prove_association() {
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let station = EthernetAddress([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10]);
        let ap = EthernetAddress([0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5]);
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");

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

        session.progress.post_assoc_polls = CYW43_HOST_EAPOL_START_FIRST_POLL as u32;
        session.bssid_refreshed_after_assoc = true;
        assert!(!session.progress.associated);
        assert!(!session.progress.link_up);
        assert_eq!(session.progress.event_rx, 0);
        assert_eq!(session.progress.eapol_rx, 0);
        assert_eq!(session.progress.association_event, None);
        assert_eq!(session.progress.association_poll, 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_bssid_probe_records_metadata_not_carrier() {
        let _guard = CYW43_STATUS_TEST_LOCK.lock().expect("status test lock");
        reset_cyw43_status_flags();
        CYW43_HOST_EAPOL_TEST_BSSID_VALID.store(1, Ordering::Release);

        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let station = EthernetAddress([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10]);
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");

        assert_eq!(
            cyw43_probe_host_eapol_assoc_state(
                CYW43_WIFI_DRIVER_TASK_CONTRACT,
                station,
                &mut session,
                7,
                1,
                "test-bssid-probe",
            ),
            Cyw43AssocProbeResult::BssidObserved
        );
        assert_eq!(session.progress.assoc_probe_status, Some("valid-bssid"));
        assert!(!session.progress.associated);
        assert!(!session.progress.link_up);
        assert_eq!(session.progress.association_event, None);
        assert_eq!(session.progress.association_poll, 0);
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 0);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_join_window_allows_one_bsscfg_join_rescue_after_probe_limit() {
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");

        assert_eq!(
            cyw43_host_eapol_next_action("required", &session.progress),
            "inspect-cyw43-association-event-or-join-policy"
        );
        assert!(cyw43_host_eapol_assoc_probe_due(1_024, 0));
        assert!(!cyw43_host_eapol_assoc_probe_due(1_023, 0));
        assert!(cyw43_host_eapol_assoc_probe_due(4_096, 1));
        assert!(!cyw43_host_eapol_assoc_probe_due(4_095, 1));
        assert!(cyw43_host_eapol_assoc_probe_due_ms(1_024, 0));
        assert!(!cyw43_host_eapol_assoc_probe_due_ms(1_023, 0));
        assert!(cyw43_host_eapol_assoc_probe_due_ms(4_096, 1));
        assert!(!cyw43_host_eapol_assoc_probe_due_ms(4_095, 1));
        assert!(cyw43_host_eapol_assoc_probe_due_any(1_024, 0, 0));
        assert!(cyw43_host_eapol_assoc_probe_due_any(0, 1_024, 0));
        assert!(cyw43_host_eapol_assoc_probe_due_any(4_096, 1_023, 1));
        assert!(!cyw43_host_eapol_assoc_probe_due_any(1_023, 1_023, 0));
        assert!(cyw43_host_eapol_assoc_rescue_due_any(
            CYW43_HOST_EAPOL_ASSOC_RESCUE_POLL,
            0
        ));
        assert!(cyw43_host_eapol_assoc_rescue_due_any(
            0,
            CYW43_HOST_EAPOL_ASSOC_RESCUE_MS
        ));
        assert!(!cyw43_host_eapol_assoc_rescue_due_any(
            CYW43_HOST_EAPOL_ASSOC_RESCUE_POLL - 1,
            CYW43_HOST_EAPOL_ASSOC_RESCUE_MS - 1
        ));
        assert!(!session.progress.assoc_join_rescue_attempted);
        assert!(!session.progress.assoc_set_ssid_rescue_attempted);
        session
            .progress
            .record_assoc_probe("not-associated", CYW43_BCME_NOTASSOCIATED_STATUS);
        session.progress.record_assoc_join_rescue_attempt();
        assert!(session.progress.assoc_probe_not_associated);
        assert!(session.progress.assoc_join_rescue_attempted);
        assert!(!session.progress.assoc_set_ssid_rescue_attempted);
        assert_eq!(
            cyw43_host_eapol_next_action("required", &session.progress),
            "inspect-cyw43-association-event-after-bsscfg-join-rescue"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_post_assoc_recovery_keeps_poll_and_timer_bounds() {
        let mut progress = Cyw43HostEapolProgress {
            associated: true,
            ..Default::default()
        };

        progress.post_assoc_polls = CYW43_HOST_EAPOL_RX_REFRESH_AFTER_POST_ASSOC_POLLS - 1;
        assert!(!cyw43_host_eapol_post_assoc_refresh_due(&progress, false));
        progress.post_assoc_polls = CYW43_HOST_EAPOL_RX_REFRESH_AFTER_POST_ASSOC_POLLS;
        assert!(cyw43_host_eapol_post_assoc_refresh_due(&progress, false));
        assert!(cyw43_host_eapol_post_assoc_refresh_due_ms(
            &progress,
            CYW43_HOST_EAPOL_RX_REFRESH_AFTER_POST_ASSOC_MS,
            false
        ));

        progress.post_assoc_polls = CYW43_HOST_EAPOL_RX_RESCUE_AFTER_POST_ASSOC_POLLS - 1;
        assert!(!cyw43_host_eapol_post_assoc_rescue_due(&progress, false));
        progress.post_assoc_polls = CYW43_HOST_EAPOL_RX_RESCUE_AFTER_POST_ASSOC_POLLS;
        assert!(cyw43_host_eapol_post_assoc_rescue_due(&progress, false));
        assert!(cyw43_host_eapol_post_assoc_rescue_due_ms(
            &progress,
            CYW43_HOST_EAPOL_RX_RESCUE_AFTER_POST_ASSOC_MS,
            false
        ));

        progress.eapol_rx = 1;
        assert!(!cyw43_host_eapol_post_assoc_refresh_due(&progress, false));
        assert!(!cyw43_host_eapol_post_assoc_rescue_due(&progress, false));
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
    fn host_eapol_active_prompt_poll_services_complementary_lane() {
        assert_eq!(
            cyw43_host_eapol_active_followup_plan(Cyw43HostEapolPollKind::Control, false),
            (Some(Cyw43HostEapolPollKind::Data), None)
        );
        assert_eq!(
            cyw43_host_eapol_active_followup_plan(Cyw43HostEapolPollKind::Data, false),
            (Some(Cyw43HostEapolPollKind::Control), None)
        );
        assert_eq!(
            cyw43_host_eapol_active_followup_plan(Cyw43HostEapolPollKind::Control, true),
            (
                Some(Cyw43HostEapolPollKind::Control),
                Some(Cyw43HostEapolPollKind::Data)
            )
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_steady_data_rx_uses_hintless_firstread_rescue() {
        assert_eq!(
            CYW43_DATA_RX_STEADY_POLL_FLAGS,
            DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD
                | DRIVER_RUNTIME_CYW43_FLAG_RX_STEADY_TAIL_DRAIN
        );
        assert_eq!(
            cyw43_control_split_poll_flags(1),
            DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD
        );
        assert_eq!(
            cyw43_control_split_poll_flags(1) & DRIVER_RUNTIME_CYW43_FLAG_RX_STEADY_TAIL_DRAIN,
            0
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
    fn cyw43_control_exchange_prompt_resume_uses_active_descriptor_progress() {
        let progress = crate::hal::driver_task::DriverTaskRingProgressSnapshot {
            marker_valid: true,
            sequence: 182,
            phase: 142,
            phase_name: "cyw43-sdio-owner-wait-begin",
            aux0: DRIVER_RUNTIME_CYW43_COMMAND_AUX,
        };
        let descriptor = DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE,
            flags: DRIVER_RUNTIME_CYW43_FLAG_CONTROL_PRE_TX_DRAIN,
            payload_len: 36,
            total_len: 36,
            arg0: 0x107,
            arg1: 1,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        };
        let active_descriptor = DriverRuntimeCyw43CommandDescriptor {
            payload_offset: 512,
            ..descriptor
        };

        assert_eq!(
            cyw43_active_prompt_poll_for_descriptor(182, Some(progress), descriptor),
            None
        );
        assert!(cyw43_prompt_slice_active_descriptor_resume_ready(
            182,
            Some(progress),
            descriptor,
            active_descriptor,
        ));
        assert!(!cyw43_prompt_slice_active_descriptor_resume_ready(
            181,
            Some(progress),
            descriptor,
            active_descriptor,
        ));
        assert!(!cyw43_prompt_slice_active_descriptor_resume_ready(
            182,
            Some(crate::hal::driver_task::DriverTaskRingProgressSnapshot {
                aux0: 0,
                ..progress
            }),
            descriptor,
            active_descriptor,
        ));
        assert!(!cyw43_prompt_slice_active_descriptor_resume_ready(
            182,
            Some(progress),
            descriptor,
            DriverRuntimeCyw43CommandDescriptor {
                flags: 0,
                ..active_descriptor
            },
        ));
        assert!(!cyw43_prompt_slice_active_descriptor_resume_ready(
            182,
            Some(progress),
            descriptor,
            DriverRuntimeCyw43CommandDescriptor {
                payload_len: 32,
                ..active_descriptor
            },
        ));
        assert!(!cyw43_prompt_slice_active_descriptor_resume_ready(
            182,
            Some(progress),
            DriverRuntimeCyw43CommandDescriptor {
                op: DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT,
                ..descriptor
            },
            DriverRuntimeCyw43CommandDescriptor {
                op: DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT,
                ..descriptor
            },
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_device_receive_delivers_only_data_channel_frames() {
        let _guard = CYW43_STATUS_TEST_LOCK.lock().expect("status test lock");
        reset_cyw43_status_flags();
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        let _ring = test_publish_cyw43_ring(&mut ring_page);
        let payload = [0xa5u8; 32];

        let data_completion =
            test_stage_cyw43_completion(&payload, DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA, 1);
        let token = cyw43_driver_task_data_frame_from_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            data_completion,
        )
        .expect("data-channel frame is delivered to smoltcp");
        assert_eq!(token.len, payload.len());
        assert_eq!(&token.buffer[..token.len], &payload);

        let control_completion = test_stage_cyw43_completion(
            &payload,
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_CONTROL,
            2,
        );
        assert!(cyw43_driver_task_data_frame_from_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            control_completion
        )
        .is_none());

        let event_completion =
            test_stage_cyw43_completion(&payload, DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT, 3);
        assert!(cyw43_driver_task_data_frame_from_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            event_completion
        )
        .is_none());

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_path_info_identifies_gate9_arp_and_dhcp() {
        let arp_request = test_cyw43_arp_frame(1);
        let arp_info = cyw43_data_path_info(&arp_request).expect("ARP is tracked");
        assert_eq!(arp_info.dst, [0xff; ETHER_ADDR_LEN]);
        assert_eq!(arp_info.src, CYW43_DRIVER_TASK_MAC.0);
        assert_eq!(arp_info.ethertype, CYW43_ETH_P_ARP);
        assert_eq!(arp_info.arp, "request");
        assert_eq!(arp_info.dhcp, "none");

        let dhcp_discover =
            test_cyw43_dhcp_frame(1, CYW43_DHCP_CLIENT_PORT, CYW43_DHCP_SERVER_PORT);
        let discover_info = cyw43_data_path_info(&dhcp_discover).expect("DHCP discover is tracked");
        assert_eq!(discover_info.dst, [0xff; ETHER_ADDR_LEN]);
        assert_eq!(discover_info.src, CYW43_DRIVER_TASK_MAC.0);
        assert_eq!(discover_info.ethertype, CYW43_ETH_P_IPV4);
        assert_eq!(discover_info.ip_proto, CYW43_IP_PROTO_UDP);
        assert_eq!(discover_info.udp_src, CYW43_DHCP_CLIENT_PORT);
        assert_eq!(discover_info.udp_dst, CYW43_DHCP_SERVER_PORT);
        assert_eq!(discover_info.dhcp, "discover");

        let dhcp_offer = test_cyw43_dhcp_frame(2, CYW43_DHCP_SERVER_PORT, CYW43_DHCP_CLIENT_PORT);
        let offer_info = cyw43_data_path_info(&dhcp_offer).expect("DHCP offer is tracked");
        assert_eq!(offer_info.dst, [0xff; ETHER_ADDR_LEN]);
        assert_eq!(offer_info.src, CYW43_DRIVER_TASK_MAC.0);
        assert_eq!(offer_info.udp_src, CYW43_DHCP_SERVER_PORT);
        assert_eq!(offer_info.udp_dst, CYW43_DHCP_CLIENT_PORT);
        assert_eq!(offer_info.dhcp, "offer");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_path_info_identifies_arp_protocol_sender_and_target() {
        let request = test_cyw43_arp_request(
            [0x62, 0x72, 0x58, 0xed, 0x47, 0x5b],
            [192, 168, 86, 102],
            [192, 168, 86, 154],
        );

        let info = cyw43_data_path_info(&request).expect("ARP request is tracked");

        assert_eq!(info.arp, "request");
        assert_eq!(info.arp_spa, [192, 168, 86, 102]);
        assert_eq!(info.arp_tpa, [192, 168, 86, 154]);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_tx_shape_pads_short_frames_to_data_tx_floor() {
        let dhcp_discover =
            test_cyw43_dhcp_frame(1, CYW43_DHCP_CLIENT_PORT, CYW43_DHCP_SERVER_PORT);
        let dhcp_total_len = CYW43_SDPCM_DATA_TX_OVERHEAD_BYTES + dhcp_discover.len();
        let (dhcp_request_len, dhcp_block_mode) =
            cyw43_data_tx_request_len_for_frame(&dhcp_discover, dhcp_total_len);
        assert_eq!(dhcp_request_len, cyw43_data_tx_request_len(dhcp_total_len));
        assert!(!dhcp_block_mode);
        assert_eq!(
            cyw43_function2_data_tx_cmd53_shape(dhcp_request_len, dhcp_block_mode),
            (dhcp_request_len as u16, 0)
        );

        let eapol = test_cyw43_eapol_frame();
        let eapol_total_len = CYW43_SDPCM_DATA_TX_OVERHEAD_BYTES + eapol.len();
        let (eapol_request_len, eapol_block_mode) =
            cyw43_data_tx_request_len_for_frame(&eapol, eapol_total_len);
        assert_eq!(eapol_request_len, CYW43_DATA_TX_MIN_FUNCTION2_BYTES);
        assert!(!eapol_block_mode);
        assert_eq!(
            cyw43_function2_data_tx_cmd53_shape(eapol_request_len, eapol_block_mode),
            (CYW43_DATA_TX_MIN_FUNCTION2_BYTES as u16, 0)
        );

        let arp_reply = test_cyw43_arp_frame(2);
        let arp_total_len = CYW43_SDPCM_DATA_TX_OVERHEAD_BYTES + arp_reply.len();
        let (arp_request_len, arp_block_mode) =
            cyw43_data_tx_request_len_for_frame(&arp_reply, arp_total_len);
        assert_eq!(arp_total_len, 72);
        assert_eq!(CYW43_DATA_TX_MIN_FUNCTION2_BYTES, 128);
        assert_eq!(arp_request_len, CYW43_DATA_TX_MIN_FUNCTION2_BYTES);
        assert!(!arp_block_mode);
        assert_eq!(
            cyw43_function2_data_tx_cmd53_shape(arp_request_len, arp_block_mode),
            (CYW43_DATA_TX_MIN_FUNCTION2_BYTES as u16, 0)
        );

        let udp_frame = test_cyw43_dhcp_frame(3, 49152, 31337);
        let udp_total_len = CYW43_SDPCM_DATA_TX_OVERHEAD_BYTES + udp_frame.len();
        let (udp_request_len, udp_block_mode) =
            cyw43_data_tx_request_len_for_frame(&udp_frame, udp_total_len);
        assert_eq!(udp_request_len, cyw43_data_tx_request_len(udp_total_len));
        assert!(!udp_block_mode);
        assert_eq!(
            cyw43_function2_data_tx_cmd53_shape(udp_request_len, udp_block_mode),
            (udp_request_len as u16, 0)
        );

        let tcp_frame = test_cyw43_tcp_frame();
        let tcp_total_len = CYW43_SDPCM_DATA_TX_OVERHEAD_BYTES + tcp_frame.len();
        let (tcp_request_len, tcp_block_mode) =
            cyw43_data_tx_request_len_for_frame(&tcp_frame, tcp_total_len);
        assert_eq!(tcp_total_len, 124);
        assert_eq!(cyw43_data_tx_request_len(tcp_total_len), 124);
        assert_eq!(tcp_request_len, CYW43_DATA_TX_MIN_FUNCTION2_BYTES);
        assert!(!tcp_block_mode);
        assert_eq!(
            cyw43_function2_data_tx_cmd53_shape(tcp_request_len, tcp_block_mode),
            (CYW43_DATA_TX_MIN_FUNCTION2_BYTES as u16, 0)
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_path_trace_policy_keeps_only_high_impact_frames() {
        let discover = test_cyw43_dhcp_frame(1, CYW43_DHCP_CLIENT_PORT, CYW43_DHCP_SERVER_PORT);
        let discover_info = cyw43_trace_frame_info(&discover);
        assert_eq!(
            cyw43_data_path_trace_class(
                "tx-result",
                "submitted",
                discover_info,
                DriverTaskCompletionCode::Progress.as_u16(),
                None,
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::Dhcp
        );
        assert_eq!(
            cyw43_data_path_trace_class(
                "tx-result",
                "submitted",
                discover_info,
                DriverTaskCompletionCode::Progress.as_u16(),
                Some([192, 168, 86, 154]),
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::Dhcp
        );

        let offer = test_cyw43_dhcp_frame(2, CYW43_DHCP_SERVER_PORT, CYW43_DHCP_CLIENT_PORT);
        let offer_info = cyw43_trace_frame_info(&offer);
        assert_eq!(
            cyw43_data_path_trace_class(
                "rx-deliver",
                "poll",
                offer_info,
                DriverTaskCompletionCode::FrameReady.as_u16(),
                None,
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::Dhcp
        );
        let mut foreign_offer = offer;
        foreign_offer[6..12].copy_from_slice(&[0x62, 0x72, 0x58, 0xed, 0x47, 0x5b]);
        let foreign_offer_info = cyw43_trace_frame_info(&foreign_offer);
        assert_eq!(
            cyw43_data_path_trace_class(
                "rx-deliver",
                "poll",
                foreign_offer_info,
                DriverTaskCompletionCode::FrameReady.as_u16(),
                Some([192, 168, 86, 154]),
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::Suppress
        );

        let assigned_arp = test_cyw43_arp_request(
            [0x62, 0x72, 0x58, 0xed, 0x47, 0x5b],
            [192, 168, 86, 102],
            [192, 168, 86, 154],
        );
        let assigned_arp_info = cyw43_trace_frame_info(&assigned_arp);
        assert_eq!(
            cyw43_data_path_trace_class(
                "rx-deliver",
                "poll",
                assigned_arp_info,
                DriverTaskCompletionCode::FrameReady.as_u16(),
                Some([192, 168, 86, 154]),
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::Suppress
        );
        assert_eq!(
            cyw43_data_path_trace_class(
                "rx-deliver",
                "poll",
                assigned_arp_info,
                DriverTaskCompletionCode::FrameReady.as_u16(),
                Some([192, 168, 86, 200]),
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::Suppress
        );
        let link_local_arp = test_cyw43_arp_request(
            [0x62, 0x72, 0x58, 0xed, 0x47, 0x5b],
            [192, 168, 86, 102],
            [169, 254, 169, 254],
        );
        let link_local_arp_info = cyw43_trace_frame_info(&link_local_arp);
        assert_eq!(
            cyw43_data_path_trace_class(
                "rx-preserve",
                "pre-poll",
                link_local_arp_info,
                DriverTaskCompletionCode::FrameReady.as_u16(),
                Some([192, 168, 86, 154]),
                CYW43_DRIVER_TASK_MAC,
                false,
                true,
            ),
            Cyw43DataPathTraceClass::Suppress
        );

        let arp_reply_info = cyw43_trace_frame_info(&test_cyw43_arp_frame(2));
        assert_eq!(
            cyw43_data_path_trace_class(
                "tx-result",
                "submitted",
                arp_reply_info,
                DriverTaskCompletionCode::Progress.as_u16(),
                Some([192, 168, 86, 154]),
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::PendingTransition
        );
        assert_eq!(
            cyw43_data_path_trace_class(
                "tx-result",
                "retry",
                discover_info,
                DriverTaskCompletionCode::Fault.as_u16(),
                Some([192, 168, 86, 154]),
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::TxRetry
        );
        assert_eq!(
            cyw43_data_path_trace_class(
                "tx-result",
                "no-completion-active-tx",
                discover_info,
                0,
                Some([192, 168, 86, 154]),
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::TxRetry
        );
        assert_eq!(
            cyw43_data_path_trace_class(
                "rx-channel-drop",
                "control",
                discover_info,
                DriverTaskCompletionCode::FrameReady.as_u16(),
                Some([192, 168, 86, 154]),
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::Drop
        );
        assert_eq!(
            cyw43_data_path_trace_class(
                "tx-drop",
                "invalid-arp-spa",
                assigned_arp_info,
                DriverTaskCompletionCode::Idle.as_u16(),
                Some([192, 168, 86, 154]),
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::Drop
        );

        let tcp_info = cyw43_trace_frame_info(&test_cyw43_tcp_frame());
        assert_eq!(
            cyw43_data_path_trace_class(
                "rx-deliver",
                "pending",
                tcp_info,
                DriverTaskCompletionCode::Idle.as_u16(),
                Some([192, 168, 86, 154]),
                CYW43_DRIVER_TASK_MAC,
                true,
                false,
            ),
            Cyw43DataPathTraceClass::Suppress
        );
        assert_eq!(
            cyw43_data_path_trace_class(
                "rx-deliver",
                "resume",
                tcp_info,
                DriverTaskCompletionCode::Idle.as_u16(),
                Some([192, 168, 86, 154]),
                CYW43_DRIVER_TASK_MAC,
                false,
                false,
            ),
            Cyw43DataPathTraceClass::Suppress
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_path_trace_repeat_gate_keeps_milestones() {
        assert!(cyw43_data_path_trace_repeat_milestone(1));
        assert!(cyw43_data_path_trace_repeat_milestone(4));
        assert!(cyw43_data_path_trace_repeat_milestone(8));
        assert!(!cyw43_data_path_trace_repeat_milestone(9));
        assert!(cyw43_data_path_trace_class_uses_milestone_gate(
            Cyw43DataPathTraceClass::Dhcp
        ));
        assert!(cyw43_data_path_trace_class_uses_milestone_gate(
            Cyw43DataPathTraceClass::EapolConsume
        ));
        assert!(cyw43_data_path_trace_class_uses_milestone_gate(
            Cyw43DataPathTraceClass::TxRetry
        ));
        assert!(cyw43_data_path_trace_class_uses_milestone_gate(
            Cyw43DataPathTraceClass::Fault
        ));
        assert!(cyw43_data_path_trace_class_uses_milestone_gate(
            Cyw43DataPathTraceClass::Drop
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_broadcast_arp_assist_replies_for_assigned_ipv4() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_DATA_TX_TEST_STUB.store(1, Ordering::Release);
        CYW43_ASSIGNED_IPV4_BE.store(u32::from_be_bytes([192, 168, 86, 154]), Ordering::Release);
        *CYW43_RUNTIME_MAC.lock() = EthernetAddress([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10]);
        let request = test_cyw43_arp_request(
            [0x62, 0x72, 0x58, 0xed, 0x47, 0x5b],
            [192, 168, 86, 102],
            [192, 168, 86, 154],
        );

        assert!(submit_cyw43_arp_assist_if_needed(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &request
        ));
        assert_eq!(CYW43_DATA_TX_TEST_ATTEMPTS.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_TX_SUBMITTED.load(Ordering::Acquire), 2);
        assert_eq!(driver_task_arp_counts(DriverTaskHotPath::Cyw43Wifi), (0, 2));

        let unrelated = test_cyw43_arp_request(
            [0x62, 0x72, 0x58, 0xed, 0x47, 0x5b],
            [192, 168, 86, 102],
            [192, 168, 86, 200],
        );
        assert!(!submit_cyw43_arp_assist_if_needed(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &unrelated
        ));
        assert_eq!(CYW43_DATA_TX_TEST_ATTEMPTS.load(Ordering::Acquire), 2);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_assigned_ipv4_emits_gratuitous_arp_announcement() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_DATA_TX_TEST_STUB.store(1, Ordering::Release);
        *CYW43_RUNTIME_MAC.lock() = EthernetAddress([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10]);
        let mut dev = Cyw43DriverTaskDevice::default();

        dev.set_assigned_ipv4(Ipv4Address::new(192, 168, 86, 154));

        assert_eq!(
            CYW43_ASSIGNED_IPV4_BE.load(Ordering::Acquire),
            u32::from_be_bytes([192, 168, 86, 154])
        );
        assert_eq!(CYW43_DATA_TX_TEST_ATTEMPTS.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_TX_SUBMITTED.load(Ordering::Acquire), 2);
        assert_eq!(driver_task_arp_counts(DriverTaskHotPath::Cyw43Wifi), (0, 2));

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_rejects_post_dhcp_zero_sender_arp_before_runtime_submit() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_DATA_TX_TEST_STUB.store(1, Ordering::Release);
        CYW43_ASSIGNED_IPV4_BE.store(u32::from_be_bytes([192, 168, 86, 154]), Ordering::Release);
        let gateway_arp =
            test_cyw43_arp_request(CYW43_DRIVER_TASK_MAC.0, [0, 0, 0, 0], [192, 168, 86, 1]);

        assert!(cyw43_post_dhcp_zero_sender_arp(&gateway_arp));
        assert!(!submit_cyw43_driver_task_eth_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &gateway_arp
        ));
        assert_eq!(CYW43_DATA_TX_TEST_ATTEMPTS.load(Ordering::Acquire), 0);

        let valid_gateway_arp = test_cyw43_arp_request(
            CYW43_DRIVER_TASK_MAC.0,
            [192, 168, 86, 154],
            [192, 168, 86, 1],
        );
        assert!(!cyw43_post_dhcp_zero_sender_arp(&valid_gateway_arp));
        assert!(submit_cyw43_driver_task_eth_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &valid_gateway_arp
        ));
        assert_eq!(CYW43_DATA_TX_TEST_ATTEMPTS.load(Ordering::Acquire), 1);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_path_info_ignores_non_dhcp_ipv4() {
        let mut frame = test_cyw43_dhcp_frame(1, CYW43_DHCP_CLIENT_PORT, CYW43_DHCP_SERVER_PORT);
        let udp = ETH_HEADER_LEN + 20;
        frame[udp..udp + 2].copy_from_slice(&12345u16.to_be_bytes());
        frame[udp + 2..udp + 4].copy_from_slice(&54321u16.to_be_bytes());

        assert!(cyw43_data_path_info(&frame).is_none());
        let trace_info = cyw43_trace_frame_info(&frame);
        assert_eq!(trace_info.ethertype, CYW43_ETH_P_IPV4);
        assert_eq!(trace_info.ip_proto, CYW43_IP_PROTO_UDP);
        assert_eq!(trace_info.dhcp, "none");
        assert_eq!(trace_info.arp, "none");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_active_prompt_completion_preserves_firstread_proof() {
        let idle_completion = Cyw43HostEapolPollResult {
            completed: true,
            observed_frame: false,
            activity: false,
            secure: false,
        };
        assert!(cyw43_host_eapol_followup_firstread_due(
            DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
            0,
            idle_completion
        ));
        assert!(!cyw43_host_eapol_followup_firstread_due(
            DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
            DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
            idle_completion
        ));
        assert!(!cyw43_host_eapol_followup_firstread_due(
            0,
            0,
            idle_completion
        ));
        assert!(!cyw43_host_eapol_followup_firstread_due(
            DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
            0,
            Cyw43HostEapolPollResult {
                completed: true,
                observed_frame: true,
                activity: true,
                secure: false,
            }
        ));
        assert!(!cyw43_host_eapol_followup_firstread_due(
            DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
            0,
            Cyw43HostEapolPollResult {
                completed: true,
                observed_frame: false,
                activity: true,
                secure: true,
            }
        ));
        assert!(!cyw43_host_eapol_followup_firstread_due(
            DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
            0,
            Cyw43HostEapolPollResult::default()
        ));

        let mut merged = idle_completion;
        merge_cyw43_host_eapol_poll_result(
            &mut merged,
            Cyw43HostEapolPollResult {
                completed: true,
                observed_frame: true,
                activity: true,
                secure: false,
            },
        );
        assert_eq!(
            merged,
            Cyw43HostEapolPollResult {
                completed: true,
                observed_frame: true,
                activity: true,
                secure: false,
            }
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
    fn host_eapol_required_without_rx_targets_association_event_path() {
        let mut progress = Cyw43HostEapolProgress::default();
        progress.polls = CYW43_HOST_EAPOL_JOIN_POLLS as u32;
        progress.record_empty_poll();

        assert_eq!(
            cyw43_host_eapol_required_reason(&progress),
            "cyw43-association-event-missing"
        );
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-cyw43-association-event-or-join-policy"
        );
        assert_eq!(
            cyw43_host_eapol_next_action("secure", &progress),
            "release-dhcp-data"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_required_without_association_names_event_gap() {
        let mut progress = Cyw43HostEapolProgress::default();
        progress.polls = CYW43_HOST_EAPOL_JOIN_POLLS as u32;

        assert_eq!(
            cyw43_host_eapol_required_reason(&progress),
            "cyw43-association-event-missing"
        );
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-cyw43-association-event-or-join-policy"
        );

        progress.record_assoc_probe("not-associated", CYW43_BCME_NOTASSOCIATED_STATUS);
        assert_eq!(
            cyw43_host_eapol_required_reason(&progress),
            "cyw43-association-not-associated"
        );
        assert_eq!(progress.assoc_probe_status, Some("not-associated"));
        assert_eq!(progress.assoc_probe_result, CYW43_BCME_NOTASSOCIATED_STATUS);

        progress.associated = true;
        assert_eq!(
            cyw43_host_eapol_required_reason(&progress),
            "host-eapol-required"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_auth_timeout_names_association_gate() {
        let mut progress = Cyw43HostEapolProgress::default();
        let auth_timeout = Cyw43EventFrame {
            flags: 0x4f01,
            event_type: CYW43_EVENT_AUTH,
            status: CYW43_EVENT_STATUS_TIMEOUT,
            reason: 2,
            auth_type: 0,
            ..Default::default()
        };

        progress.record_event_frame(0x4f01, 78, auth_timeout, 1_024);

        assert_eq!(progress.event_rx, 1);
        assert!(progress.auth_timeout_seen);
        assert_eq!(progress.last_assoc_event_type, CYW43_EVENT_AUTH);
        assert_eq!(progress.last_assoc_event_status, CYW43_EVENT_STATUS_TIMEOUT);
        assert_eq!(progress.last_assoc_event_reason, 2);
        assert_eq!(
            cyw43_host_eapol_event_trace_label(auth_timeout),
            "auth-timeout"
        );
        assert_eq!(
            cyw43_host_eapol_required_reason(&progress),
            "cyw43-association-auth-timeout"
        );
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-cyw43-auth-timeout-or-join-policy"
        );

        progress.record_assoc_join_rescue_attempt();
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-cyw43-auth-timeout-after-bsscfg-join-rescue"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_error_overrides_generic_required_reason() {
        let mut progress = Cyw43HostEapolProgress::default();
        progress.associated = true;
        progress.eapol_rx = 4;
        progress.record_eapol_error("host-eapol-m3-mic");

        assert_eq!(
            cyw43_host_eapol_required_reason(&progress),
            "host-eapol-m3-mic"
        );
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-host-eapol-error"
        );
        assert_eq!(
            cyw43_wifi_gate7_host_eapol_subgate("required", &progress),
            ("7d", "eapol-handshake", "waiting-keys")
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_mic_failures_are_credential_warnings() {
        let mut progress = Cyw43HostEapolProgress::default();
        progress.associated = true;
        progress.eapol_rx = 4;
        progress.record_eapol_error("host-eapol-m3-mic");

        assert_eq!(
            cyw43_wifi_credential_warning_from_progress(&progress),
            Some("host-eapol-m3-mic")
        );

        progress.record_eapol_error("host-eapol-group-mic");
        assert_eq!(
            cyw43_wifi_credential_warning_from_progress(&progress),
            Some("host-eapol-group-mic")
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_post_secure_maintenance_errors_are_not_credential_warnings() {
        let mut progress = Cyw43HostEapolProgress::default();
        progress.associated = true;
        progress.link_up = true;
        progress.eapol_rx = 4;
        progress.record_eapol_error("host-eapol-post-secure-ptk-install");

        assert_eq!(cyw43_wifi_credential_warning_from_progress(&progress), None);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_association_failures_are_ssid_warnings() {
        let mut progress = Cyw43HostEapolProgress::default();
        progress.auth_timeout_seen = true;
        assert_eq!(
            cyw43_wifi_credential_warning_from_progress(&progress),
            Some("cyw43-association-auth-timeout")
        );

        progress.auth_timeout_seen = false;
        progress.set_ssid_failure_seen = true;
        assert_eq!(
            cyw43_wifi_credential_warning_from_progress(&progress),
            Some("cyw43-association-set-ssid-failed")
        );

        progress.set_ssid_failure_seen = false;
        progress.assoc_probe_not_associated = true;
        assert_eq!(
            cyw43_wifi_credential_warning_from_progress(&progress),
            Some("cyw43-association-not-associated")
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
    fn host_eapol_event_parser_accepts_linked_boundary_event_shapes() {
        let packet = test_cyw43_event_packet(
            CYW43_EVENT_LINK,
            CYW43_EVENT_STATUS_SUCCESS,
            CYW43_EVENT_FLAG_LINK,
        );
        let bdc_frame = test_cyw43_bdc_event_frame(&packet);

        assert_eq!(
            cyw43_parse_control_or_event_frame(&bdc_frame).map(|event| event.event_type),
            Some(CYW43_EVENT_LINK)
        );
        assert_eq!(
            cyw43_parse_control_or_event_frame(&packet).map(|event| event.event_type),
            Some(CYW43_EVENT_LINK)
        );
        assert_eq!(
            cyw43_parse_data_event_frame(&packet).map(|event| event.event_type),
            Some(CYW43_EVENT_LINK)
        );
        assert_eq!(
            cyw43_parse_data_event_frame(&bdc_frame).map(|event| event.event_type),
            Some(CYW43_EVENT_LINK)
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_set_ssid_event_is_not_carrier_proof() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status-label tests must serialize");
        reset_cyw43_status_flags();

        let packet = test_cyw43_event_packet(CYW43_EVENT_SET_SSID, CYW43_EVENT_STATUS_SUCCESS, 0);
        let event_frame = test_cyw43_bdc_event_frame(&packet);
        let mut token = DriverTaskNetRxToken {
            len: event_frame.len(),
            buffer: [0; MAX_FRAME_LEN],
        };
        token.buffer[..event_frame.len()].copy_from_slice(&event_frame);

        assert!(cyw43_capture_event_frame_from_token(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            "test-set-ssid-event",
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT,
            &token,
        ));
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 0);

        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        cyw43_apply_pending_host_eapol_event(CYW43_WIFI_DRIVER_TASK_CONTRACT, &mut session, 3);

        assert_eq!(session.progress.event_rx, 1);
        assert!(!session.progress.associated);
        assert!(!session.progress.link_up);
        assert_eq!(session.progress.association_event, None);
        assert_eq!(
            cyw43_host_eapol_required_reason(&session.progress),
            "host-eapol-required"
        );
        assert_eq!(
            cyw43_host_eapol_next_action("required", &session.progress),
            "inspect-cyw43-join-event-state"
        );
        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_disconnect_events_clear_carrier_state() {
        let mut progress = Cyw43HostEapolProgress {
            associated: true,
            link_up: true,
            ..Cyw43HostEapolProgress::default()
        };
        let disassoc = test_cyw43_event_packet(CYW43_EVENT_DISASSOC, CYW43_EVENT_STATUS_SUCCESS, 0);
        let event = cyw43_parse_broadcom_event(&disassoc).expect("disassoc event");

        progress.record_event_frame(
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT,
            disassoc.len(),
            event,
            11,
        );

        assert!(!progress.associated);
        assert!(!progress.link_up);
        assert_eq!(progress.event_rx, 1);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_data_completion_accepts_event_channel_assoc_event() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status-label tests must serialize");
        reset_cyw43_status_flags();
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        let _ring_guard = test_publish_cyw43_ring(&mut ring_page);

        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        let mut tx_frame = [0u8; MAX_FRAME_LEN];

        let assoc_packet =
            test_cyw43_event_packet(CYW43_EVENT_ASSOC, CYW43_EVENT_STATUS_SUCCESS, 0);
        let assoc_frame = test_cyw43_bdc_event_frame(&assoc_packet);
        let result = process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            19,
            0,
            test_stage_cyw43_completion(
                &assoc_frame,
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT,
                1,
            ),
            &mut tx_frame,
        )
        .expect("event-channel data completion should decode as host-eapol event");

        assert!(result.observed_frame);
        assert!(!result.secure);
        assert_eq!(session.progress.event_rx, 1);
        assert_eq!(session.progress.data_rx, 0);
        assert_eq!(session.progress.non_eapol_rx, 0);
        assert!(session.progress.associated);
        assert!(!session.progress.link_up);
        assert_eq!(session.progress.association_event, Some("assoc"));
        assert_eq!(session.progress.association_poll, 20);
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 0);
        reset_cyw43_status_flags();
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
    fn linked_runtime_wifi_replay_harness_reaches_secure_eapol() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 replay tests must serialize");
        reset_cyw43_status_flags();
        let _io_guard = test_enable_cyw43_host_eapol_io_stub();
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        let _ring_guard = test_publish_cyw43_ring(&mut ring_page);

        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        *CYW43_RUNTIME_MAC.lock() = EthernetAddress(station);
        CYW43_LINKED_RUNTIME_READY.store(1, Ordering::Release);
        CYW43_CONTROL_PLANE_READY.store(1, Ordering::Release);
        CYW43_HOST_EAPOL_ACTIVE.store(1, Ordering::Release);
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        let mut tx_frame = [0u8; MAX_FRAME_LEN];

        assert_eq!(
            cyw43_driver_task_bringup_status_label(),
            Some("wifi-host-eapol-pending")
        );
        assert!(!cyw43_data_plane_ready());

        let set_ssid_packet =
            test_cyw43_event_packet(CYW43_EVENT_SET_SSID, CYW43_EVENT_STATUS_SUCCESS, 0);
        let set_ssid_frame = test_cyw43_bdc_event_frame(&set_ssid_packet);
        let set_ssid_result = process_cyw43_host_eapol_control_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &mut session,
            0,
            0,
            test_stage_cyw43_completion(
                &set_ssid_frame,
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT,
                1,
            ),
        );

        assert!(set_ssid_result.observed_frame);
        assert_eq!(session.progress.event_rx, 1);
        assert!(!session.progress.associated);
        assert!(!session.progress.link_up);
        assert_eq!(session.progress.association_event, None);
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 0);
        assert!(!cyw43_data_plane_ready());

        let link_packet = test_cyw43_event_packet(
            CYW43_EVENT_LINK,
            CYW43_EVENT_STATUS_SUCCESS,
            CYW43_EVENT_FLAG_LINK,
        );
        let link_frame = test_cyw43_bdc_event_frame(&link_packet);
        let link_result = process_cyw43_host_eapol_control_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &mut session,
            1,
            0,
            test_stage_cyw43_completion(
                &link_frame,
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT,
                2,
            ),
        );

        assert!(link_result.observed_frame);
        assert_eq!(session.progress.event_rx, 2);
        assert!(session.progress.associated);
        assert!(session.progress.link_up);
        assert_eq!(session.progress.association_event, Some("link-up"));
        assert_eq!(session.progress.association_poll, 2);
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 1);
        assert!(!cyw43_data_plane_ready());

        let mut m1 = [0u8; MAX_FRAME_LEN];
        let m1_len =
            cyw43_host_eapol::write_test_m1_frame(&mut m1, &station, &ap).expect("test m1 frame");
        let m1_result = process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            2,
            0,
            test_stage_cyw43_completion(
                &m1[..m1_len],
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
                3,
            ),
            &mut tx_frame,
        )
        .expect("m1 replay should produce m2");

        assert!(m1_result.observed_frame);
        assert!(!m1_result.secure);
        assert_eq!(session.progress.eapol_rx, 1);
        assert_eq!(CYW43_HOST_EAPOL_RX.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_M1.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_M2.load(Ordering::Acquire), 1);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_TX_SUBMITTED.load(Ordering::Acquire),
            1
        );
        assert_eq!(CYW43_HOST_EAPOL_TEST_TX_DRAINED.load(Ordering::Acquire), 1);

        let mut m3 = [0u8; MAX_FRAME_LEN];
        let m3_len = cyw43_host_eapol::write_test_m3_frame(&mut m3, &station, &session.eapol)
            .expect("test m3 frame");
        let m3_result = process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            3,
            0,
            test_stage_cyw43_completion(
                &m3[..m3_len],
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
                4,
            ),
            &mut tx_frame,
        )
        .expect("m3 replay should produce m4 and keys");

        assert!(m3_result.observed_frame);
        assert!(m3_result.secure);
        assert_eq!(session.progress.eapol_rx, 2);
        assert_eq!(CYW43_HOST_EAPOL_RX.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_HOST_EAPOL_M3.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_M4.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_PTK.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_GTK.load(Ordering::Acquire), 1);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_TX_SUBMITTED.load(Ordering::Acquire),
            2
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_PTK_INSTALLED.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_GTK_INSTALLED.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_REASSERTED.load(Ordering::Acquire),
            1
        );
        assert_eq!(CYW43_HOST_EAPOL_TEST_TX_DRAINED.load(Ordering::Acquire), 2);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_DRAIN_BEFORE_PTK.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_PRE_TX_DRAIN.load(Ordering::Acquire),
            1
        );
        assert_eq!(CYW43_HOST_EAPOL_TEST_RX_RESTORED.load(Ordering::Acquire), 1);
        assert!(!cyw43_data_plane_ready());

        mark_cyw43_host_eapol_secure(CYW43_WIFI_DRIVER_TASK_CONTRACT, &session.progress);

        assert_eq!(CYW43_HOST_EAPOL_ACTIVE.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_REQUIRED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_CONTROL_PLANE_READY.load(Ordering::Acquire), 1);
        assert!(cyw43_data_plane_ready());
        assert_eq!(cyw43_driver_task_bringup_status_label(), None);

        *CYW43_HOST_EAPOL_SESSION.lock() = Some(session);
        let retransmit_token = test_rx_token(&m3[..m3_len]);
        assert!(consume_cyw43_post_secure_eapol_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
            &retransmit_token,
            "test-retransmit",
        ));
        assert_eq!(CYW43_HOST_EAPOL_M3.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_HOST_EAPOL_M4.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_HOST_EAPOL_PTK.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_GTK.load(Ordering::Acquire), 1);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_TX_SUBMITTED.load(Ordering::Acquire),
            3
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_PTK_INSTALLED.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_GTK_INSTALLED.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_REASSERTED.load(Ordering::Acquire),
            1
        );
        assert_eq!(CYW43_HOST_EAPOL_TEST_TX_DRAINED.load(Ordering::Acquire), 2);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_PRE_TX_DRAIN.load(Ordering::Acquire),
            1
        );

        let mut later_m1 = m1;
        cyw43_host_eapol::set_test_m1_replay_counter_last(&mut later_m1[..m1_len], 3)
            .expect("post-secure rekey m1 replay update");
        cyw43_host_eapol::set_test_m1_anonce_last(&mut later_m1[..m1_len], 0x7e)
            .expect("post-secure rekey m1 anonce update");
        let later_m1_token = test_rx_token(&later_m1[..m1_len]);
        assert!(consume_cyw43_post_secure_eapol_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
            &later_m1_token,
            "test-rekey-m1",
        ));
        assert_eq!(CYW43_HOST_EAPOL_M1.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_HOST_EAPOL_M2.load(Ordering::Acquire), 2);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_TX_SUBMITTED.load(Ordering::Acquire),
            4
        );

        let stale_m3_token = test_rx_token(&m3[..m3_len]);
        assert!(consume_cyw43_post_secure_eapol_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
            &stale_m3_token,
            "test-stale-m3-after-rekey-m1",
        ));
        assert_eq!(CYW43_HOST_EAPOL_M3.load(Ordering::Acquire), 3);
        assert_eq!(CYW43_HOST_EAPOL_M4.load(Ordering::Acquire), 3);
        assert_eq!(CYW43_HOST_EAPOL_PTK.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_GTK.load(Ordering::Acquire), 1);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_TX_SUBMITTED.load(Ordering::Acquire),
            5
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_PTK_INSTALLED.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_GTK_INSTALLED.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_REASSERTED.load(Ordering::Acquire),
            1
        );
        assert_eq!(CYW43_HOST_EAPOL_TEST_TX_DRAINED.load(Ordering::Acquire), 2);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_PRE_TX_DRAIN.load(Ordering::Acquire),
            1
        );

        let mut later_m3 = [0u8; MAX_FRAME_LEN];
        let later_m3_len = {
            let guard = CYW43_HOST_EAPOL_SESSION.lock();
            let stored_session = guard
                .as_ref()
                .expect("post-secure rekey session remains armed");
            cyw43_host_eapol::write_test_m3_frame(&mut later_m3, &station, &stored_session.eapol)
                .expect("post-secure rekey m3")
        };
        let later_m3_token = test_rx_token(&later_m3[..later_m3_len]);
        assert!(consume_cyw43_post_secure_eapol_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
            &later_m3_token,
            "test-rekey-m3",
        ));
        assert_eq!(CYW43_HOST_EAPOL_M3.load(Ordering::Acquire), 4);
        assert_eq!(CYW43_HOST_EAPOL_M4.load(Ordering::Acquire), 4);
        assert_eq!(CYW43_HOST_EAPOL_PTK.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_HOST_EAPOL_GTK.load(Ordering::Acquire), 2);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_TX_SUBMITTED.load(Ordering::Acquire),
            6
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_PTK_INSTALLED.load(Ordering::Acquire),
            2
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_GTK_INSTALLED.load(Ordering::Acquire),
            2
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_REASSERTED.load(Ordering::Acquire),
            2
        );
        assert_eq!(CYW43_HOST_EAPOL_TEST_TX_DRAINED.load(Ordering::Acquire), 3);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_PRE_TX_DRAIN.load(Ordering::Acquire),
            2
        );
        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_m1_opens_post_assoc_maintenance_window() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 replay tests must serialize");
        reset_cyw43_status_flags();
        let _io_guard = test_enable_cyw43_host_eapol_io_stub();
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        let _ring_guard = test_publish_cyw43_ring(&mut ring_page);

        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        *CYW43_RUNTIME_MAC.lock() = EthernetAddress(station);
        CYW43_LINKED_RUNTIME_READY.store(1, Ordering::Release);
        CYW43_CONTROL_PLANE_READY.store(1, Ordering::Release);
        CYW43_HOST_EAPOL_ACTIVE.store(1, Ordering::Release);
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        let mut tx_frame = [0u8; MAX_FRAME_LEN];
        let mut m1 = [0u8; MAX_FRAME_LEN];
        let m1_len =
            cyw43_host_eapol::write_test_m1_frame(&mut m1, &station, &ap).expect("test m1 frame");

        let m1_result = process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            CYW43_HOST_EAPOL_PRE_ASSOC_POLLS,
            0,
            test_stage_cyw43_completion(
                &m1[..m1_len],
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
                4,
            ),
            &mut tx_frame,
        )
        .expect("m1 replay should produce m2");

        assert!(m1_result.observed_frame);
        assert_eq!(session.progress.eapol_rx, 1);
        assert!(session.progress.associated);
        assert!(!session.progress.link_up);
        assert_eq!(session.progress.association_event, Some("eapol-m1"));
        assert_eq!(
            session.progress.association_poll,
            CYW43_HOST_EAPOL_PRE_ASSOC_POLLS as u32
        );
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 0);

        session.progress.post_assoc_polls = CYW43_HOST_EAPOL_START_FIRST_POLL as u32 - 1;
        session.associated_ms = Some(0);
        assert!(cyw43_service_host_eapol_post_assoc(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            CYW43_HOST_EAPOL_START_FIRST_MS
        ));
        assert_eq!(CYW43_HOST_EAPOL_START.load(Ordering::Acquire), 1);
        assert_eq!(
            session.progress.post_assoc_polls,
            CYW43_HOST_EAPOL_START_FIRST_POLL as u32
        );
        assert_eq!(CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire), 0);
        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_runtime_wifi_replay_harness_accepts_oldgood_assoc_sequence() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 replay tests must serialize");
        reset_cyw43_status_flags();
        let _io_guard = test_enable_cyw43_host_eapol_io_stub();
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        let _ring_guard = test_publish_cyw43_ring(&mut ring_page);

        let station = [0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10];
        let ap = [0xf0, 0x72, 0xea, 0x4c, 0xc7, 0xa5];
        *CYW43_RUNTIME_MAC.lock() = EthernetAddress(station);
        CYW43_LINKED_RUNTIME_READY.store(1, Ordering::Release);
        CYW43_CONTROL_PLANE_READY.store(1, Ordering::Release);
        CYW43_HOST_EAPOL_ACTIVE.store(1, Ordering::Release);
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        let mut tx_frame = [0u8; MAX_FRAME_LEN];

        assert_eq!(
            cyw43_driver_task_bringup_status_label(),
            Some("wifi-host-eapol-pending")
        );
        assert!(!cyw43_data_plane_ready());

        let auth_packet = test_cyw43_event_packet(CYW43_EVENT_AUTH, CYW43_EVENT_STATUS_SUCCESS, 0);
        let auth_frame = test_cyw43_bdc_event_frame(&auth_packet);
        let auth_result = process_cyw43_host_eapol_control_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &mut session,
            703,
            0,
            test_stage_cyw43_completion(
                &auth_frame,
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT,
                1,
            ),
        );
        assert!(auth_result.observed_frame);
        assert_eq!(session.progress.event_rx, 1);
        assert!(!session.progress.associated);
        assert!(!session.progress.link_up);
        assert_eq!(session.progress.association_event, None);
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 0);

        let assoc_packet =
            test_cyw43_event_packet(CYW43_EVENT_ASSOC, CYW43_EVENT_STATUS_SUCCESS, 0);
        let assoc_frame = test_cyw43_bdc_event_frame(&assoc_packet);
        let assoc_result = process_cyw43_host_eapol_control_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &mut session,
            704,
            0,
            test_stage_cyw43_completion(
                &assoc_frame,
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT,
                2,
            ),
        );
        assert!(assoc_result.observed_frame);
        assert_eq!(session.progress.event_rx, 2);
        assert!(session.progress.associated);
        assert!(!session.progress.link_up);
        assert_eq!(session.progress.association_event, Some("assoc"));
        assert_eq!(session.progress.association_poll, 705);
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 0);
        assert!(!cyw43_data_plane_ready());

        let link_packet = test_cyw43_event_packet(
            CYW43_EVENT_LINK,
            CYW43_EVENT_STATUS_SUCCESS,
            CYW43_EVENT_FLAG_LINK,
        );
        let link_frame = test_cyw43_bdc_event_frame(&link_packet);
        let link_result = process_cyw43_host_eapol_control_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            &mut session,
            705,
            0,
            test_stage_cyw43_completion(
                &link_frame,
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_EVENT,
                3,
            ),
        );
        assert!(link_result.observed_frame);
        assert_eq!(session.progress.event_rx, 3);
        assert!(session.progress.associated);
        assert!(session.progress.link_up);
        assert_eq!(session.progress.association_event, Some("assoc"));
        assert_eq!(session.progress.association_poll, 705);
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 1);
        assert!(!cyw43_data_plane_ready());

        let mut m1 = [0u8; MAX_FRAME_LEN];
        let m1_len =
            cyw43_host_eapol::write_test_m1_frame(&mut m1, &station, &ap).expect("test m1 frame");
        let m1_result = process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            709,
            0,
            test_stage_cyw43_completion(
                &m1[..m1_len],
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
                4,
            ),
            &mut tx_frame,
        )
        .expect("m1 replay should produce m2");

        assert!(m1_result.observed_frame);
        assert!(!m1_result.secure);
        assert_eq!(session.progress.eapol_rx, 1);
        assert_eq!(CYW43_HOST_EAPOL_RX.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_M1.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_M2.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_TEST_TX_DRAINED.load(Ordering::Acquire), 1);

        let set_ssid_packet =
            test_cyw43_event_packet(CYW43_EVENT_SET_SSID, CYW43_EVENT_STATUS_SUCCESS, 0);
        let set_ssid_frame = test_cyw43_bdc_event_frame(&set_ssid_packet);
        let set_ssid_result = process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            710,
            0,
            test_stage_cyw43_completion(
                &set_ssid_frame,
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
                5,
            ),
            &mut tx_frame,
        )
        .expect("set-ssid event replay should stay non-secure");
        assert!(set_ssid_result.observed_frame);
        assert!(!set_ssid_result.secure);
        assert_eq!(session.progress.event_rx, 4);
        assert_eq!(session.progress.association_event, Some("assoc"));
        assert!(session.progress.link_up);
        assert_eq!(CYW43_ASSOCIATED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_LINK_UP.load(Ordering::Acquire), 1);
        assert!(!cyw43_data_plane_ready());

        let mut m3 = [0u8; MAX_FRAME_LEN];
        let m3_len =
            cyw43_host_eapol::write_test_m3_frame_without_gtk(&mut m3, &station, &session.eapol)
                .expect("test ptk-only m3 frame");
        let m3_result = process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            714,
            0,
            test_stage_cyw43_completion(
                &m3[..m3_len],
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
                6,
            ),
            &mut tx_frame,
        )
        .expect("m3 replay should produce m4 and ptk");

        assert!(m3_result.observed_frame);
        assert!(!m3_result.secure);
        assert_eq!(session.progress.eapol_rx, 2);
        assert_eq!(CYW43_HOST_EAPOL_RX.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_HOST_EAPOL_M3.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_M4.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_PTK.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_GTK.load(Ordering::Acquire), 0);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_TX_SUBMITTED.load(Ordering::Acquire),
            2
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_PTK_INSTALLED.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_GTK_INSTALLED.load(Ordering::Acquire),
            0
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_REASSERTED.load(Ordering::Acquire),
            0
        );
        assert_eq!(CYW43_HOST_EAPOL_TEST_TX_DRAINED.load(Ordering::Acquire), 2);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_DRAIN_BEFORE_PTK.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_PRE_TX_DRAIN.load(Ordering::Acquire),
            1
        );
        assert_eq!(CYW43_HOST_EAPOL_TEST_RX_RESTORED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_HOST_EAPOL_START.load(Ordering::Acquire), 0);
        assert!(!cyw43_data_plane_ready());

        let mut group = [0u8; MAX_FRAME_LEN];
        let group_len =
            cyw43_host_eapol::write_test_group_key_frame(&mut group, &station, &session.eapol)
                .expect("test group-key frame");
        let group_result = process_cyw43_host_eapol_data_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            EthernetAddress(station),
            &mut session,
            715,
            0,
            test_stage_cyw43_completion(
                &group[..group_len],
                DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
                7,
            ),
            &mut tx_frame,
        )
        .expect("group-key replay should install gtk and respond");

        assert!(group_result.observed_frame);
        assert!(group_result.secure);
        assert_eq!(session.progress.eapol_rx, 3);
        assert_eq!(CYW43_HOST_EAPOL_RX.load(Ordering::Acquire), 3);
        assert_eq!(CYW43_HOST_EAPOL_M3.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_M4.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_PTK.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_GTK.load(Ordering::Acquire), 1);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_TX_SUBMITTED.load(Ordering::Acquire),
            3
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_PTK_INSTALLED.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_GTK_INSTALLED.load(Ordering::Acquire),
            1
        );
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_WSEC_REASSERTED.load(Ordering::Acquire),
            1
        );
        assert_eq!(CYW43_HOST_EAPOL_TEST_TX_DRAINED.load(Ordering::Acquire), 3);
        assert_eq!(
            CYW43_HOST_EAPOL_TEST_DRAIN_BEFORE_SECURE.load(Ordering::Acquire),
            1
        );
        assert_eq!(CYW43_HOST_EAPOL_TEST_RX_RESTORED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_START.load(Ordering::Acquire), 0);
        assert!(!cyw43_data_plane_ready());

        mark_cyw43_host_eapol_secure(CYW43_WIFI_DRIVER_TASK_CONTRACT, &session.progress);

        assert_eq!(CYW43_HOST_EAPOL_ACTIVE.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_HOST_EAPOL_REQUIRED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_CONTROL_PLANE_READY.load(Ordering::Acquire), 1);
        assert!(cyw43_data_plane_ready());
        assert_eq!(cyw43_driver_task_bringup_status_label(), None);
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

        let mut progress = Cyw43HostEapolProgress::default();
        progress.record_rx_idle_completion(DriverTaskCompletionRecord {
            sequence: 3,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_SDPCM_DECODE_MISS,
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        });
        assert_eq!(progress.rx_firstread_decode_miss, 1);
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-sdpcm-readahead-channel-or-fws-tlv"
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
                passive: false,
                cached: false,
            })
        );

        progress.record_control_rx_idle_completion(DriverTaskCompletionRecord {
            sequence: 2,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY,
            result: DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_MAGIC
                | 64
                | DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_PASSIVE,
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
                passive: true,
                cached: false,
            })
        );
        assert_eq!(
            cyw43_rx_source_mode(progress.last_control_rx_source),
            "passive-sdio-bus-link"
        );
        assert_eq!(
            cyw43_host_eapol_next_action("required", &progress),
            "inspect-cyw43-assoc-event-rx-or-sdio-owner-ienx-snapshot"
        );
        assert!(cyw43_host_eapol_source_asserted(&progress));

        let mut cached_progress = Cyw43HostEapolProgress::default();
        cached_progress.record_control_rx_idle_completion(DriverTaskCompletionRecord {
            sequence: 3,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY,
            result: DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_MAGIC
                | 64
                | (0x02 << DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_SHIFT)
                | DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_CACHED,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        });
        assert_eq!(
            cached_progress.last_control_rx_source,
            Some(Cyw43RxSourceResult {
                probe_len: 64,
                interrupt_enable: 0x02,
                frame_indicated: false,
                host_interrupt: false,
                card_interrupt: false,
                function2_ready: false,
                passive: false,
                cached: true,
            })
        );
        assert_eq!(
            cyw43_rx_source_mode(cached_progress.last_control_rx_source),
            "owner-card-sampled-cached"
        );
        assert!(!cyw43_host_eapol_source_asserted(&cached_progress));
        assert_eq!(
            cyw43_host_eapol_firstread_class(&cached_progress),
            "preassoc-cadence-empty"
        );
        assert_eq!(
            cyw43_host_eapol_next_action("required", &cached_progress),
            "inspect-sdio-owner-function2-rx-source"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_firstread_decodes_rx_idle_trace() {
        let mut frame = [0u8; CYW43_RX_IDLE_TRACE_V6_BYTES];
        frame[0..4].copy_from_slice(&CYW43_RX_IDLE_TRACE_MAGIC.to_le_bytes());
        frame[4..6].copy_from_slice(&CYW43_RX_IDLE_TRACE_VERSION.to_le_bytes());
        frame[6..8].copy_from_slice(&0x0001u16.to_le_bytes());
        frame[8..10].copy_from_slice(
            &DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_SOURCE_ASSERTED_EMPTY.to_le_bytes(),
        );
        frame[10..12].copy_from_slice(&512u16.to_le_bytes());
        frame[12..16].copy_from_slice(&0x8b07_0200u32.to_le_bytes());
        frame[16..20].copy_from_slice(&0x4433_2211u32.to_le_bytes());
        frame[20..24].copy_from_slice(&0x0c0b_0a09u32.to_le_bytes());
        frame[24..26].copy_from_slice(&0u16.to_le_bytes());
        frame[26..28].copy_from_slice(&2u16.to_le_bytes());
        frame[28..30].copy_from_slice(&1u16.to_le_bytes());
        frame[30..32].copy_from_slice(&6u16.to_le_bytes());
        frame[32..34].copy_from_slice(&512u16.to_le_bytes());
        frame[34..36].copy_from_slice(&512u16.to_le_bytes());
        frame[36..38].copy_from_slice(&1u16.to_le_bytes());
        frame[38..40].copy_from_slice(&0x0223u16.to_le_bytes());
        frame[40..42].copy_from_slice(&3u16.to_le_bytes());
        frame[42..44].copy_from_slice(&7u16.to_le_bytes());
        frame[44..48].copy_from_slice(&0x2100_0040u32.to_le_bytes());
        frame[48..52].copy_from_slice(&0x0000_0040u32.to_le_bytes());
        frame[52..56].copy_from_slice(&0x0000_0000u32.to_le_bytes());
        frame[56..60].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        frame[60..64].copy_from_slice(&0xa807_0040u32.to_le_bytes());
        frame[64..68].copy_from_slice(&0xab07_0040u32.to_le_bytes());
        frame[68..70].copy_from_slice(
            &(CYW43_RX_IDLE_TRACE_SOURCE_FLAG_PRE_FRESH
                | CYW43_RX_IDLE_TRACE_SOURCE_FLAG_POST_FRESH
                | CYW43_RX_IDLE_TRACE_SOURCE_FLAG_POST_ASSERTED)
                .to_le_bytes(),
        );
        frame[70..72].copy_from_slice(&u16::MAX.to_le_bytes());
        frame[72..74].copy_from_slice(&0u16.to_le_bytes());
        frame[74..76].copy_from_slice(
            &(CYW43_RX_IDLE_TRACE_FIFO_FLAG_SET_OK
                | CYW43_RX_IDLE_TRACE_FIFO_FLAG_READBACK_OK
                | CYW43_RX_IDLE_TRACE_FIFO_FLAG_READBACK_MATCH)
                .to_le_bytes(),
        );
        frame[76..80].copy_from_slice(&0x0000_0000u32.to_le_bytes());
        frame[80..84].copy_from_slice(&0x0000_0040u32.to_le_bytes());
        frame[84..88].copy_from_slice(&0x0000_0000u32.to_le_bytes());
        frame[88..92].copy_from_slice(&0x0000_0100u32.to_le_bytes());
        frame[92..96].copy_from_slice(&0x1800_0000u32.to_le_bytes());
        frame[96..100].copy_from_slice(&0x1800_0000u32.to_le_bytes());
        frame[100..104].copy_from_slice(&0x1800_0000u32.to_le_bytes());
        frame[104..108].copy_from_slice(&4096u32.to_le_bytes());
        frame[108..112].copy_from_slice(&2u32.to_le_bytes());
        frame[112..116].copy_from_slice(&1u32.to_le_bytes());
        frame[116..118].copy_from_slice(&4u16.to_le_bytes());
        frame[120..124].copy_from_slice(&9u32.to_le_bytes());
        frame[124..126].copy_from_slice(&2u16.to_le_bytes());
        frame[128..132].copy_from_slice(&0x2000_0040u32.to_le_bytes());
        frame[132..136].copy_from_slice(&0x2000_0000u32.to_le_bytes());
        frame[136..140].copy_from_slice(&23u32.to_le_bytes());
        frame[140..144].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        frame[144..148].copy_from_slice(&12u32.to_le_bytes());
        frame[148..152].copy_from_slice(&345u32.to_le_bytes());
        frame[152..156].copy_from_slice(&456u32.to_le_bytes());

        let trace = cyw43_rx_idle_trace(&frame).expect("valid rx idle trace");

        assert!(trace.valid);
        assert_eq!(trace.flags, 0x0001);
        assert_eq!(
            trace.detail,
            DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_SOURCE_ASSERTED_EMPTY
        );
        assert_eq!(trace.probe_len, 512);
        assert_eq!(trace.source_result, 0x8b07_0200);
        assert_eq!(trace.prefix_signature, 0x4433_2211);
        assert_eq!(trace.prefix_digest, 0x0c0b_0a09);
        assert_eq!(trace.firstread_reads, 2);
        assert_eq!(trace.block_reads, 1);
        assert_eq!(trace.rframe_reads, 6);
        assert_eq!(trace.request_len, 512);
        assert_eq!(trace.block_size, 512);
        assert_eq!(trace.block_count, 1);
        assert_eq!(trace.retransmit_sample, 0x0223);
        assert_eq!(trace.queue_depth, 3);
        assert_eq!(trace.queue_high_water, 7);
        assert_eq!(trace.cmd53_arg, 0x2100_0040);
        assert_eq!(cyw43_rx_trace_cmd53_function(trace.cmd53_arg), 2);
        assert_eq!(cyw43_rx_trace_cmd53_addr(trace.cmd53_arg), 0x8000);
        assert!(!cyw43_rx_trace_cmd53_write(trace.cmd53_arg));
        assert_eq!(cyw43_rx_trace_cmd53_mode(trace.cmd53_arg), "byte");
        assert!(!cyw43_rx_trace_cmd53_increment(trace.cmd53_arg));
        assert_eq!(cyw43_rx_trace_cmd53_count(trace.cmd53_arg), 64);
        assert_eq!(cyw43_rx_trace_cmd53_count(0x2100_0000), 512);
        assert_eq!(cyw43_rx_trace_cmd53_mode(0x2900_0001), "block");
        assert_eq!(trace.transfer_result, 0x0000_0040);
        assert_eq!(trace.payload_before_digest, 0);
        assert_eq!(trace.payload_after_digest, 0x0102_0304);
        assert_eq!(trace.pre_source_result, 0xa807_0040);
        assert_eq!(trace.post_source_result, 0xab07_0040);
        assert!(cyw43_rx_trace_pre_source_fresh(trace));
        assert!(!cyw43_rx_trace_pre_source_asserted(trace));
        assert!(cyw43_rx_trace_post_source_fresh(trace));
        assert!(cyw43_rx_trace_post_source_asserted(trace));
        assert!(!cyw43_rx_trace_pre_source_failed(trace));
        assert!(!cyw43_rx_trace_post_source_failed(trace));
        assert_eq!(trace.first_nonzero_offset, u16::MAX);
        assert_eq!(cyw43_rx_trace_first_nonzero_desc(trace), "none");
        assert_eq!(trace.first_nonzero_byte, 0);
        assert_eq!(trace.pre_intstatus, 0);
        assert_eq!(trace.post_intstatus, 0x40);
        assert_eq!(trace.pre_sdhci_status, 0);
        assert_eq!(trace.post_sdhci_status, 0x100);
        assert_eq!(trace.fifo_window_requested, 0x1800_0000);
        assert_eq!(trace.fifo_window_programmed, 0x1800_0000);
        assert_eq!(trace.fifo_window_readback, 0x1800_0000);
        assert_eq!(trace.source_empty_polls, 4096);
        assert_eq!(trace.rx_drain_budget_hits, 2);
        assert_eq!(trace.rx_queue_overflows, 1);
        assert_eq!(trace.rx_max_drained_per_turn, 4);
        assert_eq!(trace.rx_irq_preserve_count, 9);
        assert_eq!(trace.rx_irq_preserve_reason, 2);
        assert_eq!(trace.rx_irq_preserve_int_status, 0x2000_0040);
        assert_eq!(trace.rx_irq_preserve_ack_bits, 0x2000_0000);
        assert_eq!(trace.sequence, 23);
        assert_eq!(trace.start_ticks_lo, 0x1234_5678);
        assert_eq!(trace.pre_sample_delta_ticks, 12);
        assert_eq!(trace.transfer_delta_ticks, 345);
        assert_eq!(trace.post_sample_delta_ticks, 456);
        assert!(cyw43_rx_trace_fifo_window_ok(trace));
        assert_eq!(
            cyw43_rx_trace_retransmit_action_name(trace.retransmit_sample),
            "read-asserted-zero"
        );
        assert_eq!(
            cyw43_rx_trace_retransmit_action_name(
                CYW43_RX_IDLE_TRACE_RETRANSMIT_ACTION_READ_SOURCE_ASSERTED
            ),
            "read-source-asserted"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_firstread_class_splits_quiet_and_asserted_sources() {
        let mut progress = Cyw43HostEapolProgress::default();
        assert_eq!(cyw43_host_eapol_firstread_class(&progress), "none");

        progress.record_rx_idle_completion(DriverTaskCompletionRecord {
            sequence: 1,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: DRIVER_RUNTIME_CYW43_RX_IDLE_DETAIL_FIRSTREAD_EMPTY,
            result: DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_MAGIC
                | 64
                | ((CYW43_SDIO_EXPECTED_IENX as u32)
                    << DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_SHIFT)
                | DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FUNCTION2_READY,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        });
        assert_eq!(
            cyw43_host_eapol_firstread_class(&progress),
            "preassoc-cadence-empty"
        );

        progress.last_rx_trace.source_flags = CYW43_RX_IDLE_TRACE_SOURCE_FLAG_PRE_ASSERTED;
        assert_eq!(
            cyw43_host_eapol_firstread_class(&progress),
            "source-asserted-empty"
        );
        assert!(cyw43_host_eapol_source_asserted(&progress));

        progress.last_rx_trace.source_flags = CYW43_RX_IDLE_TRACE_SOURCE_FLAG_EVER_ASSERTED;
        assert_eq!(
            cyw43_host_eapol_firstread_class(&progress),
            "source-asserted-empty"
        );
        assert!(cyw43_host_eapol_source_asserted(&progress));

        progress.last_rx_trace.source_flags = 0;
        progress.last_rx_trace.pre_source_result = DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_MAGIC
            | 64
            | ((CYW43_SDIO_EXPECTED_IENX as u32)
                << DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_IEN_SHIFT)
            | DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FUNCTION2_READY
            | DRIVER_RUNTIME_CYW43_RX_SOURCE_RESULT_FRAME_INDICATED;
        assert!(cyw43_host_eapol_source_asserted(&progress));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_firstread_cadence_services_pre_assoc_and_post_start() {
        assert!(cyw43_host_eapol_rx_firstread_due(0, 0, false));
        assert!(cyw43_host_eapol_rx_firstread_due(1, 0, false));
        assert!(!cyw43_host_eapol_rx_firstread_due(2, 0, false));
        assert!(cyw43_host_eapol_rx_firstread_due(4096, 0, false));
        assert!(cyw43_host_eapol_rx_firstread_due(
            CYW43_HOST_EAPOL_START_INTERVAL_POLLS,
            0,
            false
        ));
        assert!(!cyw43_host_eapol_rx_firstread_due(4097, 0, false));
        assert!(cyw43_host_eapol_rx_firstread_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL,
            0,
            true
        ));
        assert!(!cyw43_host_eapol_rx_firstread_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL + 1,
            0,
            true
        ));
        assert!(!cyw43_host_eapol_rx_firstread_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL,
            1,
            true
        ));
        assert!(cyw43_host_eapol_rx_firstread_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL + 1,
            1,
            true
        ));
        assert!(cyw43_host_eapol_rx_firstread_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL + 1024,
            1,
            true
        ));
        assert!(!cyw43_host_eapol_rx_firstread_due(
            CYW43_HOST_EAPOL_START_FIRST_POLL + 1025,
            1,
            true
        ));
        let mut progress = Cyw43HostEapolProgress::default();
        progress.polls = 4097;
        assert!(!cyw43_host_eapol_rx_firstread_due_from_progress(
            progress.polls as usize,
            0,
            &progress
        ));
        progress.last_rx_source = Some(Cyw43RxSourceResult {
            probe_len: 64,
            interrupt_enable: CYW43_SDIO_EXPECTED_IENX,
            frame_indicated: true,
            host_interrupt: true,
            card_interrupt: false,
            function2_ready: true,
            passive: false,
            cached: false,
        });
        assert!(cyw43_host_eapol_rx_firstread_due_from_progress(
            progress.polls as usize,
            0,
            &progress
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn host_eapol_firstread_timer_slots_are_bounded() {
        let credentials = crate::net::WifiCredentials::new("cohesix", "passphrase")
            .expect("valid wifi credentials");
        let mut session =
            Cyw43HostEapolSession::new(credentials).expect("host eapol session starts");
        session.record_time(100);

        assert!(cyw43_host_eapol_rx_firstread_due_from_session(
            2,
            0,
            &mut session,
            100
        ));
        assert!(!cyw43_host_eapol_rx_firstread_due_from_session(
            2,
            0,
            &mut session,
            100
        ));
        assert!(cyw43_host_eapol_rx_firstread_due_from_session(
            2,
            0,
            &mut session,
            101
        ));
        assert!(!cyw43_host_eapol_rx_firstread_due_from_session(
            2,
            0,
            &mut session,
            101
        ));
        assert!(cyw43_host_eapol_rx_firstread_due_from_session(
            2,
            0,
            &mut session,
            104
        ));

        session.progress.associated = true;
        session.associated_ms = Some(200);
        session.last_eapol_start_ms = Some(200);
        assert!(!cyw43_host_eapol_rx_firstread_due_from_session(
            CYW43_HOST_EAPOL_START_FIRST_POLL,
            1,
            &mut session,
            200
        ));
        assert!(cyw43_host_eapol_rx_firstread_due_from_session(
            CYW43_HOST_EAPOL_START_FIRST_POLL,
            1,
            &mut session,
            201
        ));
        assert!(!cyw43_host_eapol_rx_firstread_due_from_session(
            CYW43_HOST_EAPOL_START_FIRST_POLL,
            1,
            &mut session,
            201
        ));
        assert!(cyw43_host_eapol_rx_firstread_due_from_session(
            CYW43_HOST_EAPOL_START_FIRST_POLL,
            1,
            &mut session,
            204
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
        assert!(cyw43_fault_detail_allows_same_command_retry(0x532b));
        assert!(!cyw43_fault_detail_allows_same_command_retry(0x532a));
        assert!(!cyw43_fault_detail_allows_same_command_retry(0x5102));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn bssid_refresh_tx_retry_is_limited_to_descriptor_transfer_fault() {
        let transfer_failed = DriverTaskCompletionRecord {
            sequence: 9,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: 0x5103,
            result: 0x0500_0800,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        let descriptor_unavailable = DriverTaskCompletionRecord {
            detail: 0x5102,
            ..transfer_failed
        };
        let firmware_not_associated = DriverTaskCompletionRecord {
            detail: CYW43_CONTROL_EXCHANGE_FAULT_DETAIL,
            result: CYW43_BCME_NOTASSOCIATED_STATUS,
            ..transfer_failed
        };

        assert_eq!(CYW43_HOST_EAPOL_BSSID_REFRESH_TX_RETRIES, 1);
        assert_eq!(
            cyw43_bssid_refresh_tx_retry_completion(
                Cyw43CommandSubmitError::Completion(transfer_failed),
                0,
            ),
            Some(transfer_failed)
        );
        assert_eq!(
            cyw43_bssid_refresh_tx_retry_completion(
                Cyw43CommandSubmitError::Completion(transfer_failed),
                1,
            ),
            None
        );
        assert_eq!(
            cyw43_bssid_refresh_tx_retry_completion(
                Cyw43CommandSubmitError::Completion(descriptor_unavailable),
                0,
            ),
            None
        );
        assert_eq!(
            cyw43_bssid_refresh_tx_retry_completion(
                Cyw43CommandSubmitError::Completion(firmware_not_associated),
                0,
            ),
            None
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn generic_control_tx_submit_retry_allows_one_sdio_owner_replay() {
        let transfer_failed = DriverTaskCompletionRecord {
            sequence: 20,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: 0x5103,
            result: 0x0420_8000,
            frame: DriverFrameDescriptor {
                offset: 768,
                len: 56,
                flags: 0,
            },
        };
        let descriptor_unavailable = DriverTaskCompletionRecord {
            detail: 0x5102,
            ..transfer_failed
        };
        let function2_not_ready = DriverTaskCompletionRecord {
            detail: 0x532b,
            ..transfer_failed
        };
        let post_release_ht_fault = DriverTaskCompletionRecord {
            detail: 0x532a,
            result: 0x0500_0800,
            ..transfer_failed
        };
        let response_r5_fault = DriverTaskCompletionRecord {
            result: 0x0500_0800,
            ..transfer_failed
        };
        let wrapped_control_frame_transfer = DriverTaskCompletionRecord {
            detail: CYW43_CONTROL_FRAME_DETAIL,
            ..transfer_failed
        };
        let wrapped_control_frame_command_error = DriverTaskCompletionRecord {
            detail: CYW43_CONTROL_FRAME_DETAIL,
            result: 0x0200_8000,
            ..transfer_failed
        };
        let command_error_transfer = DriverTaskCompletionRecord {
            result: 0x0200_8000,
            ..transfer_failed
        };
        let submitted = DriverTaskCompletionRecord {
            code: DriverTaskCompletionCode::Progress.as_u16(),
            ..transfer_failed
        };

        assert_eq!(CYW43_CONTROL_TX_SUBMIT_RETRIES, 1);
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-firmware-version",
                transfer_failed,
                0
            ),
            Some(transfer_failed)
        );
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-firmware-version",
                transfer_failed,
                1
            ),
            None
        );
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-firmware-version",
                descriptor_unavailable,
                0
            ),
            None
        );
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-firmware-version",
                function2_not_ready,
                0
            ),
            Some(function2_not_ready)
        );
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-txglomalign",
                post_release_ht_fault,
                0
            ),
            Some(post_release_ht_fault)
        );
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-firmware-version",
                post_release_ht_fault,
                0
            ),
            None
        );
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-firmware-version",
                response_r5_fault,
                0
            ),
            Some(response_r5_fault)
        );
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-txglomalign",
                wrapped_control_frame_transfer,
                0
            ),
            Some(wrapped_control_frame_transfer)
        );
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-txglomalign",
                wrapped_control_frame_transfer,
                1
            ),
            None
        );
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-txglomalign",
                wrapped_control_frame_command_error,
                0
            ),
            None
        );
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-firmware-version",
                command_error_transfer,
                0
            ),
            None
        );
        assert_eq!(
            cyw43_control_tx_submit_retry_completion(
                "cyw43-control-firmware-version",
                submitted,
                0
            ),
            None
        );
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
    fn cyw43_no_reply_resumes_cover_long_control_and_release_turns() {
        assert_eq!(
            cyw43_runtime_no_reply_resume_limit(DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT),
            CYW43_RUNTIME_TRANSPORT_NO_REPLY_RESUMES
        );
        assert_eq!(
            cyw43_runtime_no_reply_resume_limit(DRIVER_RUNTIME_CYW43_OP_RELEASE),
            CYW43_RUNTIME_FIRMWARE_RELEASE_NO_REPLY_RESUMES
        );
        assert_eq!(
            cyw43_runtime_no_reply_resume_limit(DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE),
            CYW43_RUNTIME_CONTROL_EXCHANGE_NO_REPLY_RESUMES
        );
        assert_eq!(
            cyw43_runtime_no_reply_resume_limit(DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME),
            CYW43_RUNTIME_CONTROL_FRAME_NO_REPLY_RESUMES
        );
        assert_eq!(
            cyw43_runtime_no_reply_resume_limit(DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL),
            CYW43_RUNTIME_CONTROL_POLL_NO_REPLY_RESUMES
        );
        assert_eq!(
            cyw43_runtime_no_reply_resume_limit(DRIVER_RUNTIME_CYW43_OP_RX_POLL),
            CYW43_RUNTIME_DATA_POLL_NO_REPLY_RESUMES
        );
        assert_eq!(
            cyw43_runtime_no_reply_resume_limit(DRIVER_RUNTIME_CYW43_OP_ETH_TX),
            CYW43_RUNTIME_DATA_TX_NO_REPLY_RESUMES
        );
        assert!(
            CYW43_RUNTIME_CONTROL_EXCHANGE_NO_REPLY_RESUMES
                > CYW43_RUNTIME_TRANSPORT_NO_REPLY_RESUMES
        );
        assert!(
            CYW43_RUNTIME_NESTED_SDIO_NO_REPLY_RESUMES
                >= crate::hal::driver_task::DRIVER_TASK_CYW43_SDIO_OWNER_REPLY_TIMEOUT_KEEP_ACTIVE_LIMIT
        );
        assert!(
            CYW43_RUNTIME_FIRMWARE_RELEASE_NO_REPLY_RESUMES
                >= CYW43_RUNTIME_CONTROL_EXCHANGE_NO_REPLY_RESUMES
        );
        assert!(cyw43_runtime_command_uses_shared_payload(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
        ));
        assert!(cyw43_runtime_command_uses_shared_payload(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
        ));
        assert!(cyw43_runtime_command_uses_shared_payload(
            DRIVER_RUNTIME_CYW43_OP_ETH_TX
        ));
        assert!(!cyw43_runtime_command_uses_shared_payload(
            DRIVER_RUNTIME_CYW43_OP_RX_POLL
        ));
        assert!(
            CYW43_RUNTIME_CONTROL_EXCHANGE_NO_REPLY_RESUMES
                > CYW43_RUNTIME_CONTROL_POLL_NO_REPLY_RESUMES
        );
        assert!(cyw43_runtime_descriptor_uses_prompt_slice(
            DRIVER_RUNTIME_CYW43_OP_RX_POLL
        ));
        assert!(cyw43_runtime_descriptor_quiet_hot_path(
            DRIVER_RUNTIME_CYW43_OP_RX_POLL
        ));
        assert!(cyw43_runtime_descriptor_blocks_net_pre_poll(
            DRIVER_RUNTIME_CYW43_OP_RX_POLL
        ));
        assert!(cyw43_runtime_descriptor_uses_prompt_slice(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL
        ));
        assert!(cyw43_runtime_descriptor_quiet_hot_path(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL
        ));
        assert!(cyw43_runtime_descriptor_blocks_net_pre_poll(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL
        ));
        assert!(cyw43_runtime_descriptor_uses_prompt_slice(
            DRIVER_RUNTIME_CYW43_OP_ETH_TX
        ));
        assert!(cyw43_runtime_descriptor_quiet_hot_path(
            DRIVER_RUNTIME_CYW43_OP_ETH_TX
        ));
        assert!(cyw43_runtime_descriptor_blocks_net_pre_poll(
            DRIVER_RUNTIME_CYW43_OP_ETH_TX
        ));
        assert!(cyw43_runtime_descriptor_uses_prompt_slice(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
        ));
        assert!(cyw43_runtime_descriptor_quiet_hot_path(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
        ));
        assert!(cyw43_runtime_descriptor_blocks_net_pre_poll(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME
        ));
        assert!(cyw43_runtime_descriptor_uses_prompt_slice(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
        ));
        assert!(!cyw43_runtime_descriptor_quiet_hot_path(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
        ));
        assert!(cyw43_runtime_descriptor_blocks_net_pre_poll(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE
        ));
        assert!(cyw43_runtime_command_completion_is_quiet_expected(
            DRIVER_RUNTIME_CYW43_OP_RX_POLL,
            DriverTaskCompletionRecord::idle(5)
        ));
        assert!(cyw43_runtime_command_completion_is_quiet_expected(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL,
            DriverTaskCompletionRecord::idle(6)
        ));
        assert!(cyw43_runtime_command_completion_is_quiet_expected(
            DRIVER_RUNTIME_CYW43_OP_ETH_TX,
            DriverTaskCompletionRecord::idle(7)
        ));
        assert!(!cyw43_runtime_command_completion_is_quiet_expected(
            DRIVER_RUNTIME_CYW43_OP_RX_POLL,
            DriverTaskCompletionRecord {
                sequence: 8,
                code: DriverTaskCompletionCode::Fault.as_u16(),
                detail: 0x5321,
                result: 0,
                frame: DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            }
        ));
        assert_eq!(
            cyw43_descriptor_label_for_op(Some(DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME)),
            "control-frame"
        );
        assert_eq!(
            cyw43_descriptor_label_for_op(Some(DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE)),
            "control-exchange"
        );
        assert_eq!(cyw43_descriptor_label_for_op(Some(0xffff)), "other");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_engine_init_uses_canonical_runtime_engine_aux() {
        let cyw43 = driver_task_net_engine_init_command(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::Cyw43Wifi,
        );
        assert_eq!(cyw43.arg0, DriverTaskHotPath::Cyw43Wifi.as_u32());
        assert_eq!(cyw43.arg1, DriverTaskHotPath::Cyw43Wifi.role_bit() as u32);
        assert_eq!(cyw43.aux0, pi4_driver_abi::DRIVER_RUNTIME_ENGINE_INIT_AUX);
        assert_eq!(
            cyw43.flags & crate::hal::driver_task::DRIVER_TASK_RING_FLAG_ONE_WAY,
            0
        );
        assert_eq!(cyw43.frame.len, 0);
        assert!(cyw43.owner_state_credit_eligible());

        let genet = driver_task_net_engine_init_command(
            GENET_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::GenetNic,
        );
        assert_eq!(genet.arg0, DriverTaskHotPath::GenetNic.as_u32());
        assert_eq!(genet.arg1, DriverTaskHotPath::GenetNic.role_bit() as u32);
        assert_eq!(genet.aux0, DRIVER_RUNTIME_NET_INIT_AUX);
        assert_eq!(
            genet.flags & crate::hal::driver_task::DRIVER_TASK_RING_FLAG_ONE_WAY,
            0
        );
        assert_eq!(genet.frame.len, 0);
        assert!(genet.owner_state_credit_eligible());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_engine_init_replays_markerless_rejected_completion() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
        );
        crate::hal::driver_task::test_record_driver_task_ring_progress_snapshot(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            0,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_RUNTIME_POLL_READY,
            DRIVER_RUNTIME_CYW43_COMMAND_AUX,
        );
        let completion = DriverTaskCompletionRecord {
            sequence: 2,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: DriverTaskFaultCode::RejectedCommand.as_u16(),
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };

        let replay_reason = cyw43_engine_init_completion_replay_reason(Some(completion));

        assert_eq!(replay_reason, Some("stale-admission"));
        assert_eq!(
            cyw43_engine_init_completion_status(Some(completion), false, replay_reason, false),
            "stale-admission-retry"
        );
        assert!(cyw43_engine_init_completion_allows_replay(
            replay_reason,
            false
        ));
        assert_eq!(
            cyw43_engine_init_completion_status(Some(completion), false, replay_reason, true),
            "stale-admission-exhausted"
        );
        assert!(!cyw43_engine_init_completion_allows_replay(
            replay_reason,
            true
        ));
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_engine_init_reject_with_current_progress_is_terminal() {
        let _guard = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
        );
        crate::hal::driver_task::test_record_driver_task_ring_progress_snapshot(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            2,
            pi4_driver_abi::DRIVER_RUNTIME_RING_PROGRESS_COMMAND_VALIDATED,
            DRIVER_RUNTIME_ENGINE_INIT_AUX,
        );
        let completion = DriverTaskCompletionRecord {
            sequence: 2,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: DriverTaskFaultCode::RejectedCommand.as_u16(),
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };

        let replay_reason = cyw43_engine_init_completion_replay_reason(Some(completion));

        assert_eq!(replay_reason, None);
        assert_eq!(
            cyw43_engine_init_completion_status(Some(completion), false, replay_reason, false),
            "fault"
        );
        assert!(!cyw43_engine_init_completion_allows_replay(
            replay_reason,
            false
        ));
        crate::hal::driver_task::test_clear_driver_task_ring_progress_snapshot(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_descriptor_unavailable_retry_is_narrow() {
        let descriptor_unavailable = DriverTaskCompletionRecord {
            sequence: 11,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: CYW43_DESCRIPTOR_UNAVAILABLE_DETAIL,
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        let descriptor_invalid = DriverTaskCompletionRecord {
            detail: 0x530a,
            ..descriptor_unavailable
        };
        let rejected = DriverTaskCompletionRecord {
            detail: DriverTaskFaultCode::RejectedCommand.as_u16(),
            ..descriptor_unavailable
        };
        let progress = DriverTaskCompletionRecord {
            code: DriverTaskCompletionCode::Progress.as_u16(),
            detail: 0,
            ..descriptor_unavailable
        };

        for op in [
            DRIVER_RUNTIME_CYW43_OP_RX_POLL,
            DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL,
            DRIVER_RUNTIME_CYW43_OP_ETH_TX,
            DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME,
        ] {
            assert!(cyw43_descriptor_unavailable_retry_allowed(
                op,
                &descriptor_unavailable,
                0
            ));
            assert!(cyw43_descriptor_unavailable_retry_allowed(
                op,
                &descriptor_unavailable,
                1
            ));
            assert!(!cyw43_descriptor_unavailable_retry_allowed(
                op,
                &descriptor_unavailable,
                CYW43_RUNTIME_DESCRIPTOR_UNAVAILABLE_RETRIES
            ));
            assert!(!cyw43_descriptor_unavailable_retry_allowed(
                op,
                &descriptor_invalid,
                0
            ));
            assert!(!cyw43_descriptor_unavailable_retry_allowed(
                op, &rejected, 0
            ));
            assert!(!cyw43_descriptor_unavailable_retry_allowed(
                op, &progress, 0
            ));
        }

        assert!(!cyw43_descriptor_unavailable_retry_allowed(
            DRIVER_RUNTIME_CYW43_OP_CONTROL_EXCHANGE,
            &descriptor_unavailable,
            0
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
        let descriptor = cyw43_sdio_host_reprime_descriptor();
        let mut bytes = [0u8; core::mem::size_of::<DriverRuntimeSdioCommandDescriptor>()];

        encode_sdio_descriptor(&mut bytes, descriptor);

        assert_eq!(
            &bytes[0..2],
            &DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG.to_le_bytes()
        );
        assert_eq!(bytes[2], 0);
        assert_eq!(bytes[3], DRIVER_RUNTIME_SDIO_RESP_NONE);
        assert_eq!(
            &bytes[4..8],
            &CYW43_SDIO_HOST_REPRIME_CLOCK_HZ.to_le_bytes()
        );
        assert_eq!(
            &bytes[16..18],
            &(DriverRuntimeSdioCommandDescriptor::FLAG_HOST_BUS_WIDTH_4BIT
                | DriverRuntimeSdioCommandDescriptor::FLAG_HOST_HIGH_SPEED)
                .to_le_bytes(),
        );
        assert_eq!(
            &bytes[20..24],
            &CYW43_SDIO_HOST_REPRIME_TIMEOUT_US.to_le_bytes()
        );
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
    fn sdio_owner_fault_decodes_function2_byte_count_separately_from_host_block_count() {
        let txglomalign = SdioFaultTelemetry {
            arg: 0xa100_0030,
            cmd: 53,
            flags: 0,
            len: 48,
            block_size: 48,
            block_count: 1,
            transfer_mode: 0x0002,
            present_state: 0,
            int_status: 0,
            response0: 0,
            host_control: 0x06,
            power_control: 0x0f,
            clock_control: 0x0307,
            failure_result: 0x0420_8000,
            block_size_count_reg: 0x0001_0030,
            payload_first: 0,
            payload_last: 0,
            payload_xor: 0,
            payload_sum: 0,
        };

        assert_eq!(txglomalign.cmd53_function(), 2);
        assert_eq!(txglomalign.cmd53_addr(), CYW43_BACKPLANE_32BIT_FLAG);
        assert!(txglomalign.cmd53_write());
        assert!(!txglomalign.cmd53_increment());
        assert!(!txglomalign.cmd53_block_mode());
        assert_eq!(txglomalign.cmd53_count(), 48);
        assert_eq!(txglomalign.cmd53_descriptor_block_count(), 0);
        assert_eq!(txglomalign.block_count, 1);
        assert_eq!(sdio_fault_transfer_mode_label(txglomalign), "byte");
        assert_eq!(
            sdio_host_control_mode_label(txglomalign.host_control),
            "4bit+high-speed"
        );
        assert_eq!(
            sdio_clock_control_state_label(txglomalign.clock_control),
            "internal+stable+card"
        );

        let block_mode = SdioFaultTelemetry {
            arg: (1 << 31) | (2 << 28) | (1 << 27) | (CYW43_BACKPLANE_32BIT_FLAG << 9) | 3,
            len: 1536,
            block_size: 512,
            block_count: 3,
            block_size_count_reg: 0x0003_0200,
            ..txglomalign
        };
        assert!(block_mode.cmd53_block_mode());
        assert_eq!(block_mode.cmd53_count(), 3);
        assert_eq!(block_mode.cmd53_descriptor_block_count(), 3);
        assert_eq!(block_mode.block_count, 3);
        assert_eq!(sdio_fault_transfer_mode_label(block_mode), "block");
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
    fn host_eapol_wsec_key_uses_oldgood_control_reply_window() {
        assert_eq!(CYW43_HOST_EAPOL_WSEC_KEY_POLL_ATTEMPTS, 8_000);
        assert_eq!(CYW43_HOST_EAPOL_POST_SECURE_WSEC_KEY_POLL_ATTEMPTS, 512);
        assert_eq!(CYW43_CONTROL_PLANE_REPLY_TIMEOUT_MS, 1_000);
        assert_eq!(CYW43_HOST_EAPOL_WSEC_KEY_REPLY_TIMEOUT_MS, 2_500);
        assert_eq!(CYW43_HOST_EAPOL_POST_SECURE_WSEC_KEY_REPLY_TIMEOUT_MS, 250);
        assert_eq!(CYW43_HOST_EAPOL_TX_DRAIN_POLLS, 2_000);
        assert_eq!(CYW43_HOST_EAPOL_TX_DRAIN_TIMEOUT_MS, 2_000);
        assert_eq!(
            cyw43_host_eapol_tx_drain_window("m4-before-wsec"),
            (
                CYW43_HOST_EAPOL_TX_DRAIN_TIMEOUT_MS,
                CYW43_HOST_EAPOL_TX_DRAIN_POLLS
            )
        );
        assert_eq!(
            cyw43_host_eapol_tx_drain_window("post-secure-m4-before-wsec"),
            (
                CYW43_HOST_EAPOL_TX_DRAIN_TIMEOUT_MS,
                CYW43_HOST_EAPOL_TX_DRAIN_POLLS
            )
        );
        assert_eq!(
            cyw43_host_eapol_tx_drain_window("m2-before-m3"),
            (
                CYW43_HOST_EAPOL_TX_DRAIN_TIMEOUT_MS,
                CYW43_HOST_EAPOL_TX_DRAIN_POLLS
            )
        );
        assert_eq!(
            cyw43_control_exchange_poll_attempts("cyw43-host-eapol-ptk", "wsec_key"),
            CYW43_HOST_EAPOL_WSEC_KEY_POLL_ATTEMPTS
        );
        assert_eq!(
            cyw43_control_exchange_timeout_ms("cyw43-host-eapol-ptk", "wsec_key"),
            CYW43_HOST_EAPOL_WSEC_KEY_REPLY_TIMEOUT_MS
        );
        assert_eq!(
            cyw43_control_exchange_poll_attempts("cyw43-host-eapol-gtk", "wsec_key"),
            CYW43_HOST_EAPOL_WSEC_KEY_POLL_ATTEMPTS
        );
        assert_eq!(
            cyw43_control_exchange_timeout_ms("cyw43-host-eapol-gtk", "wsec_key"),
            CYW43_HOST_EAPOL_WSEC_KEY_REPLY_TIMEOUT_MS
        );
        assert_eq!(
            cyw43_control_exchange_poll_attempts("cyw43-host-eapol-post-secure-ptk", "wsec_key"),
            CYW43_HOST_EAPOL_POST_SECURE_WSEC_KEY_POLL_ATTEMPTS
        );
        assert_eq!(
            cyw43_control_exchange_timeout_ms("cyw43-host-eapol-post-secure-ptk", "wsec_key"),
            CYW43_HOST_EAPOL_POST_SECURE_WSEC_KEY_REPLY_TIMEOUT_MS
        );
        assert_eq!(
            cyw43_control_exchange_poll_attempts("cyw43-host-eapol-post-secure-gtk", "wsec_key"),
            CYW43_HOST_EAPOL_POST_SECURE_WSEC_KEY_POLL_ATTEMPTS
        );
        assert_eq!(
            cyw43_control_exchange_timeout_ms("cyw43-host-eapol-post-secure-gtk", "wsec_key"),
            CYW43_HOST_EAPOL_POST_SECURE_WSEC_KEY_REPLY_TIMEOUT_MS
        );
        assert!(!cyw43_control_uses_host_eapol_wsec_key_reply_window(
            "cyw43-host-eapol-post-secure-ptk",
            "wsec_key"
        ));
        assert!(
            cyw43_control_uses_post_secure_host_eapol_wsec_key_reply_window(
                "cyw43-host-eapol-post-secure-ptk",
                "wsec_key"
            )
        );
        assert!(!cyw43_control_uses_runtime_exchange(
            "cyw43-host-eapol-post-secure-ptk",
            "wsec_key"
        ));
        assert_eq!(
            cyw43_control_exchange_poll_attempts("cyw43-control-security-wpa2-psk", "wsec"),
            CYW43_CONTROL_PLANE_POLL_ATTEMPTS
        );
        assert_eq!(
            cyw43_control_exchange_timeout_ms("cyw43-control-security-wpa2-psk", "wsec"),
            CYW43_CONTROL_PLANE_REPLY_TIMEOUT_MS
        );
        assert_eq!(
            cyw43_control_exchange_poll_attempts("cyw43-control-wpa-auth-final", "wpa_auth"),
            CYW43_CONTROL_PLANE_POLL_ATTEMPTS
        );
        assert_eq!(
            cyw43_control_exchange_timeout_ms("cyw43-control-wpa-auth-final", "wpa_auth"),
            CYW43_CONTROL_PLANE_REPLY_TIMEOUT_MS
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_poll_deadlines_use_pi_counter_cycles_when_available() {
        assert_eq!(
            cyw43_millis_to_cycles_at_hz(CYW43_HOST_EAPOL_TX_DRAIN_TIMEOUT_MS, 54_000_000),
            108_000_000
        );
        assert_eq!(
            cyw43_millis_to_cycles_at_hz(CYW43_CONTROL_PLANE_REPLY_TIMEOUT_MS, 54_000_000),
            54_000_000
        );
        let mut fallback = Cyw43PollDeadline::Polls { remaining: 2 };
        assert!(cyw43_poll_deadline_open(&mut fallback));
        assert!(cyw43_poll_deadline_open(&mut fallback));
        assert!(!cyw43_poll_deadline_open(&mut fallback));

        let counter = Cyw43PollDeadline::Counter {
            start: 123,
            cycles: 108_000_000,
        };
        assert_eq!(
            cyw43_poll_deadline_trace_fields(&counter),
            ("counter", 108_000_000, 0)
        );
        let polls = Cyw43PollDeadline::Polls { remaining: 17 };
        assert_eq!(cyw43_poll_deadline_trace_fields(&polls), ("polls", 0, 17));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn control_split_terminal_trace_uses_selected_poll_limit() {
        let idle = DriverTaskCompletionRecord {
            sequence: 0,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: 0,
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };

        assert!(cyw43_control_split_poll_completion_should_trace(
            CYW43_HOST_EAPOL_WSEC_KEY_POLL_ATTEMPTS,
            CYW43_HOST_EAPOL_WSEC_KEY_POLL_ATTEMPTS,
            0,
            idle
        ));
        assert!(!cyw43_control_split_poll_completion_should_trace(
            CYW43_CONTROL_PLANE_POLL_ATTEMPTS,
            CYW43_HOST_EAPOL_WSEC_KEY_POLL_ATTEMPTS,
            0,
            idle
        ));
        assert!(cyw43_control_split_poll_completion_should_trace(
            16,
            CYW43_HOST_EAPOL_WSEC_KEY_POLL_ATTEMPTS,
            DRIVER_RUNTIME_CYW43_FLAG_RX_HINTLESS_FIRSTREAD,
            idle
        ));
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
                609_310
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

    #[test]
    fn driver_task_arp_counters_split_genet_and_wifi_edges() {
        GENET_ARP_RX.store(0, Ordering::Release);
        GENET_ARP_TX.store(0, Ordering::Release);
        CYW43_ARP_RX.store(0, Ordering::Release);
        CYW43_ARP_TX.store(0, Ordering::Release);

        let mut arp = [0u8; 60];
        arp[12] = 0x08;
        arp[13] = 0x06;
        let mut ipv4 = [0u8; 60];
        ipv4[12] = 0x08;
        ipv4[13] = 0x00;

        record_driver_task_arp_rx(DriverTaskHotPath::GenetNic, &arp);
        record_driver_task_arp_tx(DriverTaskHotPath::GenetNic, &arp);
        record_driver_task_arp_rx(DriverTaskHotPath::Cyw43Wifi, &arp);
        record_driver_task_arp_tx(DriverTaskHotPath::Cyw43Wifi, &ipv4);

        assert_eq!(driver_task_arp_counts(DriverTaskHotPath::GenetNic), (1, 1));
        assert_eq!(driver_task_arp_counts(DriverTaskHotPath::Cyw43Wifi), (1, 0));

        GENET_ARP_RX.store(0, Ordering::Release);
        GENET_ARP_TX.store(0, Ordering::Release);
        CYW43_ARP_RX.store(0, Ordering::Release);
        CYW43_ARP_TX.store(0, Ordering::Release);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn genet_runtime_completion_updates_hardware_netstats() {
        use crate::hal::driver_task::{DriverTaskCompletionCode, DriverTaskCompletionRecord};

        GENET_TX_HW_COMPLETED.store(0, Ordering::Release);
        GENET_TX_HW_FREE.store(0, Ordering::Release);
        GENET_TX_HW_IN_FLIGHT.store(0, Ordering::Release);
        GENET_RX_HW_FRAMES.store(0, Ordering::Release);
        GENET_RX_LAST_ETHERTYPE.store(0, Ordering::Release);
        GENET_RX_LAST_LEN.store(0, Ordering::Release);
        GENET_RX_RUNTIME_QUEUE_COUNT.store(0, Ordering::Release);
        GENET_RX_RUNTIME_QUEUE_HIGH_WATER.store(0, Ordering::Release);
        GENET_RX_RUNTIME_QUEUE_OVERFLOW_SEEN.store(0, Ordering::Release);
        GENET_RX_RUNTIME_DRAIN_BUDGET_HIT.store(0, Ordering::Release);
        GENET_RX_RUNTIME_BYTE_BUDGET_HIT.store(0, Ordering::Release);
        GENET_RX_RUNTIME_MAX_DRAINED_PER_TURN.store(0, Ordering::Release);

        record_genet_runtime_completion(DriverTaskCompletionRecord {
            sequence: 1,
            code: DriverTaskCompletionCode::Progress.as_u16(),
            detail: 2,
            result: 300,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 30,
                flags: 2,
            },
        });
        record_genet_runtime_completion(DriverTaskCompletionRecord {
            sequence: 2,
            code: DriverTaskCompletionCode::FrameReady.as_u16(),
            detail: 0,
            result: pi4_driver_abi::driver_runtime_genet_completion_result(
                pi4_driver_abi::DriverRuntimeGenetCompletionResultParts {
                    tx_free: 30,
                    tx_in_flight: 2,
                    rx_queue_count: 4,
                    rx_queue_high_water: 8,
                    rx_max_drained_per_turn: 9,
                    rx_drain_budget_hit: true,
                    rx_byte_budget_hit: false,
                    rx_overflow_seen: true,
                },
            ),
            frame: DriverFrameDescriptor {
                offset: 1024,
                len: 342,
                flags: 0x0800,
            },
        });
        record_genet_runtime_completion(DriverTaskCompletionRecord {
            sequence: 3,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: 1,
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 31,
                flags: 1,
            },
        });
        record_genet_runtime_completion(DriverTaskCompletionRecord {
            sequence: 4,
            code: DriverTaskCompletionCode::FrameReady.as_u16(),
            detail: 1,
            result: (32u32 << 16),
            frame: DriverFrameDescriptor {
                offset: 1024,
                len: 60,
                flags: 0x0806,
            },
        });

        let counters = GenetDriverTaskDevice::default().counters();
        assert_eq!(counters.tx_complete, 4);
        assert_eq!(counters.tx_used_advances, 4);
        assert_eq!(counters.tx_free, 32);
        assert_eq!(counters.tx_in_flight, 0);
        assert_eq!(counters.rx_used_advances, 2);
        assert_eq!(counters.driver_rx_last_len, 60);
        assert_eq!(counters.driver_rx_last_ethertype, 0x0806);
        assert_eq!(counters.genet_rx_runtime_queue_count, 4);
        assert_eq!(counters.genet_rx_runtime_queue_high_water, 8);
        assert_eq!(counters.genet_rx_runtime_queue_overflow_seen, 1);
        assert_eq!(counters.genet_rx_runtime_drain_budget_hit, 1);
        assert_eq!(counters.genet_rx_runtime_byte_budget_hit, 0);
        assert_eq!(counters.genet_rx_runtime_max_drained_per_turn, 9);

        GENET_TX_HW_COMPLETED.store(0, Ordering::Release);
        GENET_TX_HW_FREE.store(0, Ordering::Release);
        GENET_TX_HW_IN_FLIGHT.store(0, Ordering::Release);
        GENET_RX_HW_FRAMES.store(0, Ordering::Release);
        GENET_RX_LAST_ETHERTYPE.store(0, Ordering::Release);
        GENET_RX_LAST_LEN.store(0, Ordering::Release);
        GENET_RX_RUNTIME_QUEUE_COUNT.store(0, Ordering::Release);
        GENET_RX_RUNTIME_QUEUE_HIGH_WATER.store(0, Ordering::Release);
        GENET_RX_RUNTIME_QUEUE_OVERFLOW_SEEN.store(0, Ordering::Release);
        GENET_RX_RUNTIME_DRAIN_BUDGET_HIT.store(0, Ordering::Release);
        GENET_RX_RUNTIME_BYTE_BUDGET_HIT.store(0, Ordering::Release);
        GENET_RX_RUNTIME_MAX_DRAINED_PER_TURN.store(0, Ordering::Release);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_runtime_idle_trace_updates_hardware_netstats() {
        use crate::hal::driver_task::{DriverTaskCompletionCode, DriverTaskCompletionRecord};

        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        let contract = CYW43_WIFI_DRIVER_TASK_CONTRACT;
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        crate::hal::driver_task::publish_driver_task_ring(
            contract,
            ring_page.as_mut_ptr() as usize,
        );
        CYW43_RX_RUNTIME_QUEUE_COUNT.store(0, Ordering::Release);
        CYW43_RX_RUNTIME_QUEUE_HIGH_WATER.store(0, Ordering::Release);
        CYW43_RX_RUNTIME_QUEUE_OVERFLOW_SEEN.store(0, Ordering::Release);
        CYW43_RX_RUNTIME_DRAIN_BUDGET_HIT.store(0, Ordering::Release);
        CYW43_RX_RUNTIME_MAX_DRAINED_PER_TURN.store(0, Ordering::Release);

        let mut trace = [0u8; CYW43_RX_IDLE_TRACE_V6_BYTES];
        trace[0..4].copy_from_slice(&CYW43_RX_IDLE_TRACE_MAGIC.to_le_bytes());
        trace[4..6].copy_from_slice(&CYW43_RX_IDLE_TRACE_VERSION.to_le_bytes());
        trace[40..42].copy_from_slice(&3u16.to_le_bytes());
        trace[42..44].copy_from_slice(&5u16.to_le_bytes());
        trace[108..112].copy_from_slice(&1u32.to_le_bytes());
        trace[112..116].copy_from_slice(&1u32.to_le_bytes());
        trace[116..118].copy_from_slice(&4u16.to_le_bytes());
        let frame = crate::hal::driver_task::stage_driver_task_ring_frame(contract, &trace, 0)
            .expect("test ring has room for one idle trace");
        record_cyw43_runtime_completion(
            contract,
            DriverTaskCompletionRecord {
                sequence: 1,
                code: DriverTaskCompletionCode::Idle.as_u16(),
                detail: 0,
                result: 0,
                frame,
            },
        );

        let counters = Cyw43DriverTaskDevice::default().counters();
        assert_eq!(counters.wifi_rx_runtime_queue_count, 3);
        assert_eq!(counters.wifi_rx_runtime_queue_high_water, 5);
        assert_eq!(counters.wifi_rx_runtime_queue_overflow_seen, 1);
        assert_eq!(counters.wifi_rx_runtime_drain_budget_hit, 1);
        assert_eq!(counters.wifi_rx_runtime_max_drained_per_turn, 4);

        CYW43_TX_SUBMITTED.store(1, Ordering::Release);
        let mut tx_completion = DriverTaskCompletionRecord::progress(7, 64);
        tx_completion.frame = DriverFrameDescriptor {
            offset: 0,
            len: 1,
            flags: 1,
        };
        record_cyw43_unproven_tx_window(Some(tx_completion));
        let credit_completion = DriverTaskCompletionRecord {
            sequence: 2,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: 0,
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 1,
                flags: 2u16 << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT,
            },
        };
        record_cyw43_runtime_completion(contract, credit_completion);
        assert_eq!(
            CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire),
            CYW43_TX_UNPROVEN_NONE
        );
        assert_eq!(CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire), 1);

        crate::hal::driver_task::publish_driver_task_ring(contract, 0);
        CYW43_RX_RUNTIME_QUEUE_COUNT.store(0, Ordering::Release);
        CYW43_RX_RUNTIME_QUEUE_HIGH_WATER.store(0, Ordering::Release);
        CYW43_RX_RUNTIME_QUEUE_OVERFLOW_SEEN.store(0, Ordering::Release);
        CYW43_RX_RUNTIME_DRAIN_BUDGET_HIT.store(0, Ordering::Release);
        CYW43_RX_RUNTIME_MAX_DRAINED_PER_TURN.store(0, Ordering::Release);
        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_trace_counters_are_exposed_without_genet_churn() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        let contract = CYW43_WIFI_DRIVER_TASK_CONTRACT;
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        crate::hal::driver_task::publish_driver_task_ring(
            contract,
            ring_page.as_mut_ptr() as usize,
        );
        let device = Cyw43DriverTaskDevice::default();
        CYW43_DATA_TRACE_FAULT_COUNT.store(7, Ordering::Release);
        CYW43_DATA_TRACE_TX_RETRY_COUNT.store(11, Ordering::Release);

        let counters = device.counters();
        assert_eq!(counters.wifi_data_trace_faults, 7);
        assert_eq!(counters.wifi_data_trace_tx_retries, 11);

        let genet_counters = GenetDriverTaskDevice::default().counters();
        assert_eq!(genet_counters.wifi_data_trace_faults, 0);
        assert_eq!(genet_counters.wifi_data_trace_tx_retries, 0);

        crate::hal::driver_task::publish_driver_task_ring(contract, 0);
        CYW43_DATA_TRACE_FAULT_COUNT.store(0, Ordering::Release);
        CYW43_DATA_TRACE_TX_RETRY_COUNT.store(0, Ordering::Release);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn genet_prepoll_frame_ready_is_preserved_for_receive() {
        use crate::hal::driver_task::{DriverTaskCompletionCode, DriverTaskHotPath};

        let contract = GENET_DRIVER_TASK_CONTRACT;
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        crate::hal::driver_task::publish_driver_task_ring(
            contract,
            ring_page.as_mut_ptr() as usize,
        );
        while take_genet_pending_rx_token().is_some() {}
        GENET_RX_HW_FRAMES.store(0, Ordering::Release);
        GENET_RX_LAST_ETHERTYPE.store(0, Ordering::Release);
        GENET_RX_LAST_LEN.store(0, Ordering::Release);
        GENET_PENDING_RX_HIGH_WATER.store(0, Ordering::Release);
        GENET_PENDING_RX_DROPS.store(0, Ordering::Release);

        let payload = b"dhcp-offer";
        let frame =
            crate::hal::driver_task::stage_driver_task_ring_frame(contract, payload, 0x0800)
                .expect("test ring has room for one RX frame");
        let completion = DriverTaskCompletionRecord {
            sequence: 71,
            code: DriverTaskCompletionCode::FrameReady.as_u16(),
            detail: 0,
            result: payload.len() as u32,
            frame,
        };

        assert!(preserve_driver_task_pre_poll_completion(
            contract,
            DriverTaskHotPath::GenetNic,
            completion
        ));
        let payload_b = b"tcp-auth";
        let frame_b =
            crate::hal::driver_task::stage_driver_task_ring_frame(contract, payload_b, 0x0800)
                .expect("test ring has room for a second RX frame");
        let completion_b = DriverTaskCompletionRecord {
            sequence: 72,
            code: DriverTaskCompletionCode::FrameReady.as_u16(),
            detail: 0,
            result: payload_b.len() as u32,
            frame: frame_b,
        };

        assert!(preserve_driver_task_pre_poll_completion(
            contract,
            DriverTaskHotPath::GenetNic,
            completion_b
        ));
        assert_eq!(GENET_RX_HW_FRAMES.load(Ordering::Acquire), 2);
        assert_eq!(
            GENET_RX_LAST_LEN.load(Ordering::Acquire),
            payload_b.len() as u32
        );
        assert_eq!(GENET_RX_LAST_ETHERTYPE.load(Ordering::Acquire), 0x0800);
        assert_eq!(GENET_PENDING_RX_HIGH_WATER.load(Ordering::Acquire), 2);
        assert_eq!(GENET_PENDING_RX_DROPS.load(Ordering::Acquire), 0);

        let token = receive_driver_task_frame(contract, DriverTaskHotPath::GenetNic)
            .expect("pre-poll Genet RX token is delivered before a fresh poll");
        token.consume(|bytes| assert_eq!(bytes, payload));
        let token_b = receive_driver_task_frame(contract, DriverTaskHotPath::GenetNic)
            .expect("second pre-poll Genet RX token preserves burst order");
        token_b.consume(|bytes| assert_eq!(bytes, payload_b));
        assert!(take_genet_pending_rx_token().is_none());

        crate::hal::driver_task::publish_driver_task_ring(contract, 0);
        GENET_RX_HW_FRAMES.store(0, Ordering::Release);
        GENET_RX_LAST_ETHERTYPE.store(0, Ordering::Release);
        GENET_RX_LAST_LEN.store(0, Ordering::Release);
        GENET_PENDING_RX_HIGH_WATER.store(0, Ordering::Release);
        GENET_PENDING_RX_DROPS.store(0, Ordering::Release);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn genet_pending_rx_queue_matches_runtime_drain_budget_and_counts_drops() {
        while take_genet_pending_rx_token().is_some() {}
        GENET_PENDING_RX_HIGH_WATER.store(0, Ordering::Release);
        GENET_PENDING_RX_DROPS.store(0, Ordering::Release);

        assert_eq!(GENET_PENDING_RX_QUEUE_CAP, 16);
        for slot in 0..GENET_PENDING_RX_QUEUE_CAP {
            let mut buffer = [0u8; MAX_FRAME_LEN];
            buffer[0] = slot as u8;
            assert!(store_genet_pending_rx_token(DriverTaskNetRxToken {
                len: 1,
                buffer,
            }));
        }
        let overflow = DriverTaskNetRxToken {
            len: 1,
            buffer: [0xff; MAX_FRAME_LEN],
        };

        assert!(!store_genet_pending_rx_token(overflow));
        assert_eq!(
            genet_pending_rx_queue_len(),
            GENET_PENDING_RX_QUEUE_CAP as u64
        );
        assert_eq!(
            GENET_PENDING_RX_HIGH_WATER.load(Ordering::Acquire),
            GENET_PENDING_RX_QUEUE_CAP as u32
        );
        assert_eq!(GENET_PENDING_RX_DROPS.load(Ordering::Acquire), 1);

        for slot in 0..GENET_PENDING_RX_QUEUE_CAP {
            let token = take_genet_pending_rx_token().expect("queued token remains available");
            token.consume(|bytes| assert_eq!(bytes, &[slot as u8]));
        }
        assert!(take_genet_pending_rx_token().is_none());

        GENET_PENDING_RX_HIGH_WATER.store(0, Ordering::Release);
        GENET_PENDING_RX_DROPS.store(0, Ordering::Release);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_prepoll_data_frame_ready_is_preserved_for_device_receive() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_ASSOCIATED.store(1, Ordering::Release);
        CYW43_LINK_UP.store(1, Ordering::Release);
        CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);
        CYW43_POST_SECURE_DATA_RX_ADMITTED.store(1, Ordering::Release);

        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        let _ring = test_publish_cyw43_ring(&mut ring_page);
        let payload = test_cyw43_dhcp_frame(2, CYW43_DHCP_SERVER_PORT, CYW43_DHCP_CLIENT_PORT);
        let flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA;
        let completion = test_stage_cyw43_completion(&payload, flags, 72);

        assert!(preserve_driver_task_pre_poll_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::Cyw43Wifi,
            completion
        ));
        assert!(cyw43_pending_rx_token_occupied());

        let (pending_flags, pending_token) =
            take_cyw43_pending_rx_token().expect("pre-poll CYW43 RX token is pending");
        assert_eq!(pending_flags, flags);
        assert_eq!(pending_token.len, payload.len());
        assert_eq!(&pending_token.buffer[..pending_token.len], &payload);
        assert!(store_cyw43_pending_rx_token(pending_flags, pending_token));

        let mut dev = Cyw43DriverTaskDevice::default();
        let (rx, _) = dev
            .receive(Instant::from_millis(0))
            .expect("pre-poll CYW43 RX token is delivered before a fresh poll");
        rx.consume(|bytes| assert_eq!(bytes, &payload));
        assert!(take_cyw43_pending_rx_token().is_none());

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_post_secure_pending_eapol_yields_to_dhcp_offer() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_ASSOCIATED.store(1, Ordering::Release);
        CYW43_LINK_UP.store(1, Ordering::Release);
        CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);
        CYW43_POST_SECURE_DATA_RX_ADMITTED.store(1, Ordering::Release);

        let flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA;
        let eapol = test_cyw43_eapol_frame();
        let dhcp = test_cyw43_dhcp_frame(2, CYW43_DHCP_SERVER_PORT, CYW43_DHCP_CLIENT_PORT);

        assert!(store_cyw43_pending_rx_token(flags, test_rx_token(&eapol)));
        assert!(cyw43_pending_rx_token_occupied());
        assert!(cyw43_pending_rx_token_store_possible(
            flags,
            &test_rx_token(&dhcp)
        ));
        assert!(store_cyw43_pending_rx_token(flags, test_rx_token(&dhcp)));
        assert_eq!(cyw43_pending_rx_queue_len(), 2);
        assert_eq!(CYW43_PENDING_RX_HIGH_WATER.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_PENDING_RX_DROPS.load(Ordering::Acquire), 0);

        let mut dev = Cyw43DriverTaskDevice::default();
        let (rx, _) = dev
            .receive(Instant::from_millis(0))
            .expect("post-secure EAPOL is consumed and DHCP is delivered in the same receive turn");
        rx.consume(|bytes| assert_eq!(bytes, &dhcp));
        assert!(take_cyw43_pending_rx_token().is_none());

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_prepoll_post_secure_eapol_is_consumed_not_queued() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_ASSOCIATED.store(1, Ordering::Release);
        CYW43_LINK_UP.store(1, Ordering::Release);
        CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);

        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        let _ring = test_publish_cyw43_ring(&mut ring_page);
        let flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA;
        let eapol = test_cyw43_eapol_frame();
        let completion = test_stage_cyw43_completion(&eapol, flags, 73);

        assert!(preserve_driver_task_pre_poll_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::Cyw43Wifi,
            completion
        ));
        assert!(!cyw43_pending_rx_token_occupied());
        assert_eq!(
            CYW43_DATA_TRACE_EAPOL_CONSUME_COUNT.load(Ordering::Acquire),
            1
        );

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_full_post_secure_eapol_queue_admits_dhcp_offer() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_ASSOCIATED.store(1, Ordering::Release);
        CYW43_LINK_UP.store(1, Ordering::Release);
        CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);
        CYW43_POST_SECURE_DATA_RX_ADMITTED.store(1, Ordering::Release);

        let flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA;
        let eapol = test_cyw43_eapol_frame();
        let dhcp = test_cyw43_dhcp_frame(2, CYW43_DHCP_SERVER_PORT, CYW43_DHCP_CLIENT_PORT);
        for _ in 0..CYW43_PENDING_RX_QUEUE_CAP {
            assert!(store_cyw43_pending_rx_token(flags, test_rx_token(&eapol)));
        }

        assert!(cyw43_pending_rx_token_store_possible(
            flags,
            &test_rx_token(&dhcp)
        ));
        assert!(store_cyw43_pending_rx_token(flags, test_rx_token(&dhcp)));
        assert_eq!(
            cyw43_pending_rx_queue_len(),
            CYW43_PENDING_RX_QUEUE_CAP as u64
        );
        assert_eq!(
            CYW43_PENDING_RX_DROPS.load(Ordering::Acquire),
            1,
            "one post-secure EAPOL retransmit is evicted for DHCP"
        );

        let mut dev = Cyw43DriverTaskDevice::default();
        let (rx, _) = dev
            .receive(Instant::from_millis(0))
            .expect("DHCP survives a full queue of post-secure EAPOL retransmits");
        rx.consume(|bytes| assert_eq!(bytes, &dhcp));
        assert!(take_cyw43_pending_rx_token().is_none());

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_pending_rx_queue_records_high_water_and_drops() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        assert_eq!(CYW43_PENDING_RX_QUEUE_CAP, 32);

        let flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA;
        let dhcp = test_cyw43_dhcp_frame(2, CYW43_DHCP_SERVER_PORT, CYW43_DHCP_CLIENT_PORT);
        for _ in 0..CYW43_PENDING_RX_QUEUE_CAP {
            assert!(store_cyw43_pending_rx_token(flags, test_rx_token(&dhcp)));
        }
        assert_eq!(
            cyw43_pending_rx_queue_len(),
            CYW43_PENDING_RX_QUEUE_CAP as u64
        );
        assert_eq!(
            CYW43_PENDING_RX_HIGH_WATER.load(Ordering::Acquire),
            CYW43_PENDING_RX_QUEUE_CAP as u32
        );
        assert!(!store_cyw43_pending_rx_token(flags, test_rx_token(&dhcp)));
        assert_eq!(CYW43_PENDING_RX_DROPS.load(Ordering::Acquire), 1);

        while take_cyw43_pending_rx_token().is_some() {}
        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_full_multicast_queue_yields_to_runtime_tcp() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        mark_cyw43_data_plane_ready_for_test();

        let flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA;
        let multicast = test_cyw43_ipv6_multicast_frame();
        let tcp = test_cyw43_tcp_frame();
        for _ in 0..CYW43_PENDING_RX_QUEUE_CAP {
            assert!(store_cyw43_pending_rx_token(
                flags,
                test_rx_token(&multicast)
            ));
        }

        assert!(cyw43_pending_rx_token_store_possible(
            flags,
            &test_rx_token(&tcp)
        ));
        assert!(store_cyw43_pending_rx_token(flags, test_rx_token(&tcp)));
        assert_eq!(
            cyw43_pending_rx_queue_len(),
            CYW43_PENDING_RX_QUEUE_CAP as u64
        );
        assert_eq!(
            CYW43_PENDING_RX_DROPS.load(Ordering::Acquire),
            1,
            "one multicast/noise frame is evicted for runtime TCP"
        );

        let mut dev = Cyw43DriverTaskDevice::default();
        for _ in 0..CYW43_PENDING_RX_QUEUE_CAP {
            let (rx, _) = dev
                .receive(Instant::from_millis(0))
                .expect("pending multicast queue eventually yields runtime TCP");
            if rx.len == tcp.len() {
                rx.consume(|bytes| assert_eq!(bytes, &tcp));
                reset_cyw43_status_flags();
                return;
            }
        }
        panic!("runtime TCP was not preserved through multicast queue pressure");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_full_dhcp_queue_does_not_evict_dhcp_for_equal_priority_dhcp() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        mark_cyw43_data_plane_ready_for_test();

        let flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA;
        let dhcp = test_cyw43_dhcp_frame(2, CYW43_DHCP_SERVER_PORT, CYW43_DHCP_CLIENT_PORT);
        for _ in 0..CYW43_PENDING_RX_QUEUE_CAP {
            assert!(store_cyw43_pending_rx_token(flags, test_rx_token(&dhcp)));
        }

        assert!(!cyw43_pending_rx_token_store_possible(
            flags,
            &test_rx_token(&dhcp)
        ));
        assert!(!store_cyw43_pending_rx_token(flags, test_rx_token(&dhcp)));
        assert_eq!(CYW43_PENDING_RX_DROPS.load(Ordering::Acquire), 1);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_pre_secure_pending_queue_preserves_eapol_then_dhcp_order() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();

        let flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA;
        let eapol = test_cyw43_eapol_frame();
        let dhcp = test_cyw43_dhcp_frame(2, CYW43_DHCP_SERVER_PORT, CYW43_DHCP_CLIENT_PORT);

        assert!(store_cyw43_pending_rx_token(flags, test_rx_token(&eapol)));
        assert!(cyw43_pending_rx_token_store_possible(
            flags,
            &test_rx_token(&dhcp)
        ));
        assert!(store_cyw43_pending_rx_token(flags, test_rx_token(&dhcp)));

        let (pending_flags, pending_token) =
            take_cyw43_pending_rx_token().expect("pre-secure pending EAPOL remains first");
        assert_eq!(pending_flags, flags);
        assert_eq!(
            cyw43_rx_token_ethertype(pending_flags, &pending_token),
            Some(ETH_P_EAPOL)
        );
        pending_token.consume(|bytes| assert_eq!(bytes, &eapol));

        let (pending_flags, pending_token) =
            take_cyw43_pending_rx_token().expect("pre-secure DHCP remains queued after EAPOL");
        assert_eq!(pending_flags, flags);
        assert_eq!(
            cyw43_rx_token_ethertype(pending_flags, &pending_token),
            Some(CYW43_ETH_P_IPV4)
        );
        pending_token.consume(|bytes| assert_eq!(bytes, &dhcp));

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_tx_retries_transient_no_credit_completion() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_DATA_TX_TEST_STUB.store(1, Ordering::Release);
        CYW43_DATA_TX_TEST_FAILS_BEFORE_SUCCESS.store(3, Ordering::Release);

        assert!(submit_cyw43_driver_task_eth_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            b"dhcp-discover"
        ));
        assert_eq!(CYW43_DATA_TX_TEST_ATTEMPTS.load(Ordering::Acquire), 4);
        assert_eq!(CYW43_DATA_TX_RETRIES.load(Ordering::Acquire), 3);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_tx_accepts_submit_before_deferred_credit_proof() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_DATA_TX_TEST_STUB.store(1, Ordering::Release);
        CYW43_DATA_TX_TEST_SUCCESS_WITHOUT_CREDIT.store(1, Ordering::Release);

        assert!(submit_cyw43_driver_task_eth_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            b"arp-request"
        ));
        assert_eq!(CYW43_DATA_TX_TEST_ATTEMPTS.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_TX_SUBMITTED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_TX_CREDIT_UNPROVEN.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_TX_UNPROVEN_SEQ.load(Ordering::Acquire), 1);

        mark_cyw43_data_plane_ready_for_test();
        let mut dev = Cyw43DriverTaskDevice::default();
        assert!(
            dev.transmit(Instant::from_millis(0)).is_none(),
            "the next WiFi data TX token must wait until deferred credit is proven"
        );
        assert!(store_cyw43_pending_rx_token(
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
            test_rx_token(&test_cyw43_tcp_frame()),
        ));
        let Some((rx, _tx)) = dev.receive(Instant::from_millis(0)) else {
            panic!("WiFi RX must drain while a TX window is waiting for credit");
        };
        assert_eq!(rx.len, test_cyw43_tcp_frame().len());
        assert_eq!(
            CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire),
            CYW43_TX_UNPROVEN_KNOWN
        );

        let counters = dev.counters();
        assert_eq!(counters.tx_submit, 1);
        assert_eq!(counters.tx_complete, 0);
        assert_eq!(counters.tx_free, 0);
        assert_eq!(counters.tx_in_flight, 1);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_rx_credit_clears_deferred_tx_before_response_token() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_DATA_TX_TEST_STUB.store(1, Ordering::Release);
        CYW43_DATA_TX_TEST_SUCCESS_WITHOUT_CREDIT.store(1, Ordering::Release);

        assert!(submit_cyw43_driver_task_eth_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            b"tcp-syn-ack"
        ));
        assert_eq!(CYW43_TX_SUBMITTED.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire), 0);
        assert_eq!(
            CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire),
            CYW43_TX_UNPROVEN_KNOWN
        );

        let credit_flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA
            | (2u16 << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT);
        assert!(store_cyw43_pending_rx_token(
            credit_flags,
            test_rx_token(&test_cyw43_tcp_frame())
        ));
        mark_cyw43_data_plane_ready_for_test();

        let mut dev = Cyw43DriverTaskDevice::default();
        let (rx, tx) = dev
            .receive(Instant::from_millis(0))
            .expect("credit-bearing RX must be delivered");
        rx.consume(|bytes| assert_eq!(bytes, test_cyw43_tcp_frame()));
        assert_eq!(
            CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire),
            CYW43_TX_UNPROVEN_NONE,
            "RX-carried SDPCM credit must reopen CYW43 TX before smoltcp uses the response token"
        );
        assert_eq!(CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire), 1);

        CYW43_DATA_TX_TEST_SUCCESS_WITHOUT_CREDIT.store(0, Ordering::Release);
        tx.consume(64, |buf| {
            buf.fill(0x42);
        });
        assert_eq!(CYW43_TX_SUBMITTED.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_TX_DROPPED.load(Ordering::Acquire), 0);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_tx_deferred_credit_reopens_admission_from_pending_rx() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_DATA_TX_TEST_STUB.store(1, Ordering::Release);
        CYW43_DATA_TX_TEST_SUCCESS_WITHOUT_CREDIT.store(1, Ordering::Release);

        assert!(submit_cyw43_driver_task_eth_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            b"tcp-syn-ack"
        ));
        assert_eq!(CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire), 1);

        let credit_flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA
            | (2u16 << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT);
        assert!(store_cyw43_pending_rx_token(
            credit_flags,
            test_rx_token(&test_cyw43_tcp_frame())
        ));
        mark_cyw43_data_plane_ready_for_test();

        let mut dev = Cyw43DriverTaskDevice::default();
        assert!(
            dev.transmit(Instant::from_millis(0)).is_some(),
            "pending RX credit that covers the submitted sequence must reopen WiFi TX admission"
        );
        assert_eq!(CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire), 1);
        let counters = dev.counters();
        assert_eq!(counters.tx_submit, 1);
        assert_eq!(counters.tx_complete, 1);
        assert_eq!(counters.tx_free, 1);
        assert_eq!(counters.tx_in_flight, 0);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_pre_poll_progress_credit_reopens_deferred_tx_window() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_TX_SUBMITTED.store(1, Ordering::Release);

        let mut tx_completion = DriverTaskCompletionRecord::progress(7, 64);
        tx_completion.frame = DriverFrameDescriptor {
            offset: 0,
            len: 1,
            flags: 1,
        };
        record_cyw43_unproven_tx_window(Some(tx_completion));
        assert_eq!(
            CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire),
            CYW43_TX_UNPROVEN_KNOWN
        );
        assert_eq!(CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire), 0);

        let mut credit_completion = DriverTaskCompletionRecord::progress(8, 1);
        credit_completion.frame = DriverFrameDescriptor {
            offset: 0,
            len: 1,
            flags: 2u16 << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT,
        };
        assert!(preserve_driver_task_pre_poll_completion(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::Cyw43Wifi,
            credit_completion
        ));
        assert_eq!(
            CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire),
            CYW43_TX_UNPROVEN_NONE,
            "credit-bearing non-frame pre-poll completions must reopen CYW43 TX admission"
        );
        assert_eq!(CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire), 1);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_rx_credit_closes_cumulative_submitted_window() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_TX_SUBMITTED.store(6, Ordering::Release);
        CYW43_TX_CREDIT_COMPLETED.store(2, Ordering::Release);
        let mut tx_completion = DriverTaskCompletionRecord::progress(7, 64);
        tx_completion.frame = DriverFrameDescriptor {
            offset: 0,
            len: 1,
            flags: 5 | (6u16 << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT),
        };
        record_cyw43_unproven_tx_window(Some(tx_completion));
        assert_eq!(
            CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire),
            CYW43_TX_UNPROVEN_KNOWN
        );

        let credit_flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA
            | (6u16 << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT);
        assert!(store_cyw43_pending_rx_token(
            credit_flags,
            test_rx_token(&test_cyw43_tcp_frame())
        ));
        assert!(
            cyw43_tx_unproven_window_ready(CYW43_WIFI_DRIVER_TASK_CONTRACT),
            "RX-carried SDPCM credit covering seq 5 must reopen CYW43 TX admission"
        );

        assert_eq!(CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire), 0);
        assert_eq!(
            CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire),
            6,
            "known SDPCM credit is cumulative and must close all covered submissions"
        );
        let counters = Cyw43DriverTaskDevice::default().counters();
        assert_eq!(counters.tx_submit, 6);
        assert_eq!(counters.tx_complete, 6);
        assert_eq!(counters.tx_in_flight, 0);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_rx_credit_zero_after_wrap_reopens_submitted_window() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_TX_SUBMITTED.store(256, Ordering::Release);
        CYW43_TX_CREDIT_COMPLETED.store(255, Ordering::Release);

        let mut tx_completion = DriverTaskCompletionRecord::progress(7, 64);
        tx_completion.frame = DriverFrameDescriptor {
            offset: 0,
            len: 1,
            flags: 255,
        };
        record_cyw43_unproven_tx_window(Some(tx_completion));
        assert_eq!(
            CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire),
            CYW43_TX_UNPROVEN_KNOWN
        );

        assert!(store_cyw43_pending_rx_token(
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
            test_rx_token(&test_cyw43_tcp_frame())
        ));
        assert!(
            cyw43_tx_unproven_window_ready(CYW43_WIFI_DRIVER_TASK_CONTRACT),
            "RX-carried SDPCM credit 0 must cover submitted seq 255 after u8 wrap"
        );

        assert_eq!(CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire), 0);
        assert_eq!(
            CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire),
            256,
            "wrapped SDPCM credit must close the cumulative submitted window"
        );

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_tx_missing_credit_proof_blocks_until_credit_observed() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_TX_SUBMITTED.store(1, Ordering::Release);
        record_cyw43_unproven_tx_window(Some(DriverTaskCompletionRecord::progress(7, 64)));
        assert_eq!(
            CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire),
            CYW43_TX_UNPROVEN_UNKNOWN
        );
        mark_cyw43_data_plane_ready_for_test();

        let mut dev = Cyw43DriverTaskDevice::default();
        assert!(
            dev.transmit(Instant::from_millis(0)).is_none(),
            "missing CYW43 TX proof metadata must close WiFi TX admission"
        );

        let credit_flags = DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA
            | (3u16 << DRIVER_RUNTIME_CYW43_FRAME_FLAG_CREDIT_SHIFT);
        assert!(store_cyw43_pending_rx_token(
            credit_flags,
            test_rx_token(&test_cyw43_tcp_frame())
        ));
        assert!(
            dev.transmit(Instant::from_millis(1)).is_some(),
            "an observed SDPCM credit may reopen an unknown unproven window"
        );
        assert_eq!(CYW43_TX_UNPROVEN_ACTIVE.load(Ordering::Acquire), 0);
        assert_eq!(CYW43_TX_CREDIT_COMPLETED.load(Ordering::Acquire), 1);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_tx_retry_yields_on_idle_without_progress() {
        assert!(cyw43_tx_retry_should_yield(
            Some(DriverTaskCompletionRecord::idle(7)),
            false
        ));
        assert!(cyw43_tx_retry_should_yield(None, false));
        assert!(cyw43_tx_retry_should_yield(
            Some(DriverTaskCompletionRecord::fault(
                7,
                DriverTaskFaultCode::DeviceUnavailable
            )),
            false
        ));
        assert!(!cyw43_tx_retry_should_yield(
            Some(DriverTaskCompletionRecord {
                sequence: 7,
                code: DriverTaskCompletionCode::Fault.as_u16(),
                detail: CYW43_SDIO_DESCRIPTOR_TRANSFER_FAILED_DETAIL,
                result: 0x0500_0800,
                frame: DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            }),
            false
        ));
        assert!(!cyw43_tx_retry_should_yield(
            Some(DriverTaskCompletionRecord::idle(7)),
            true
        ));
        assert!(!cyw43_tx_retry_should_yield(
            Some(DriverTaskCompletionRecord::progress(7, 0)),
            false
        ));
        assert!(cyw43_tx_retry_completion_progressed(
            &DriverTaskCompletionRecord::progress(7, 1)
        ));
        assert_eq!(
            cyw43_data_tx_retry_recovery_poll_budget(Some(DriverTaskCompletionRecord {
                sequence: 7,
                code: DriverTaskCompletionCode::Fault.as_u16(),
                detail: CYW43_SDIO_DESCRIPTOR_TRANSFER_FAILED_DETAIL,
                result: 0x0500_0800,
                frame: DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            })),
            CYW43_DATA_TX_RETRY_RECOVERY_POLLS
        );
        assert_eq!(
            cyw43_data_tx_retry_recovery_poll_budget(Some(DriverTaskCompletionRecord::idle(7))),
            1
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_tx_idle_completion_yields_without_hammering_retries() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_DATA_TX_TEST_STUB.store(1, Ordering::Release);
        CYW43_DATA_TX_TEST_IDLE_BEFORE_SUCCESS.store(1, Ordering::Release);

        assert!(!submit_cyw43_driver_task_eth_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            b"arp-request"
        ));
        assert_eq!(CYW43_DATA_TX_TEST_ATTEMPTS.load(Ordering::Acquire), 1);
        assert_eq!(CYW43_DATA_TX_RETRIES.load(Ordering::Acquire), 1);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_tx_retries_retryable_fault_completion() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_DATA_TX_TEST_STUB.store(1, Ordering::Release);
        CYW43_DATA_TX_TEST_FAULTS_BEFORE_SUCCESS.store(1, Ordering::Release);

        assert!(submit_cyw43_driver_task_eth_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            b"dhcp-discover"
        ));
        assert_eq!(CYW43_DATA_TX_TEST_ATTEMPTS.load(Ordering::Acquire), 2);
        assert_eq!(CYW43_DATA_TX_RETRIES.load(Ordering::Acquire), 1);

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_tx_retryable_fault_window_remains_bounded() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_DATA_TX_TEST_STUB.store(1, Ordering::Release);
        CYW43_DATA_TX_TEST_FAULTS_BEFORE_SUCCESS
            .store(CYW43_DATA_TX_ATTEMPTS as u32, Ordering::Release);

        assert!(!submit_cyw43_driver_task_eth_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            b"dhcp-request"
        ));
        assert_eq!(
            CYW43_DATA_TX_TEST_ATTEMPTS.load(Ordering::Acquire),
            CYW43_DATA_TX_ATTEMPTS as u32
        );
        assert_eq!(
            CYW43_DATA_TX_RETRIES.load(Ordering::Acquire),
            CYW43_DATA_TX_ATTEMPTS.saturating_sub(1) as u32
        );

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_tx_retry_pending_rx_token_is_delivered_to_receive() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_HOST_EAPOL_SECURE.store(1, Ordering::Release);
        CYW43_LINK_UP.store(1, Ordering::Release);
        let mut buffer = [0u8; MAX_FRAME_LEN];
        buffer[..4].copy_from_slice(b"dhcP");
        assert!(store_cyw43_pending_rx_token(
            DRIVER_RUNTIME_CYW43_FRAME_FLAG_CHANNEL_DATA,
            DriverTaskNetRxToken { len: 4, buffer },
        ));

        let token = receive_cyw43_driver_task_frame(CYW43_WIFI_DRIVER_TASK_CONTRACT)
            .expect("pending RX token is delivered before a fresh poll");
        token.consume(|bytes| assert_eq!(bytes, b"dhcP"));
        assert!(take_cyw43_pending_rx_token().is_none());

        reset_cyw43_status_flags();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_data_tx_retry_window_remains_bounded() {
        let _lock = CYW43_STATUS_TEST_LOCK
            .lock()
            .expect("cyw43 status test lock");
        reset_cyw43_status_flags();
        CYW43_DATA_TX_TEST_STUB.store(1, Ordering::Release);
        CYW43_DATA_TX_TEST_FAILS_BEFORE_SUCCESS
            .store(CYW43_DATA_TX_ATTEMPTS as u32, Ordering::Release);

        assert!(!submit_cyw43_driver_task_eth_frame(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            b"dhcp-discover"
        ));
        assert_eq!(
            CYW43_DATA_TX_TEST_ATTEMPTS.load(Ordering::Acquire),
            CYW43_DATA_TX_ATTEMPTS as u32
        );
        assert_eq!(
            CYW43_DATA_TX_RETRIES.load(Ordering::Acquire),
            CYW43_DATA_TX_ATTEMPTS.saturating_sub(1) as u32
        );

        reset_cyw43_status_flags();
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
    fn runtime_net_init_command_requires_a_driver_task_init_lease() {
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

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_engine_init_command_uses_init_path_in_root_fallback() {
        use crate::hal::driver_task::{
            DriverTaskCompletionCode, DriverTaskFaultCode, DriverTaskHotPath,
        };

        let mut command = crate::hal::driver_task::runtime_engine_init_command(
            DriverTaskHotPath::Cyw43Wifi,
            DriverTaskBudgetGrant::from_contract(CYW43_WIFI_DRIVER_TASK_CONTRACT),
        );
        command.sequence = 23;

        let completion = unsafe {
            runtime_ring_service(DriverTaskHotPath::Cyw43Wifi.as_u32() as usize, command)
        };

        assert_eq!(completion.sequence, 23);
        assert_eq!(completion.code, DriverTaskCompletionCode::Fault.as_u16());
        assert_eq!(
            completion.detail,
            DriverTaskFaultCode::DeviceUnavailable.as_u16()
        );
    }
}
