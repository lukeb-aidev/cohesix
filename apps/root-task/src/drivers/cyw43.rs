// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide a bounded CYW43455 Wi-Fi driver for Pi 4 console networking over HAL-owned SDIO transport.
// Author: Lukas Bower

//! HAL-bound CYW43455 bring-up and Ethernet datapath for Raspberry Pi 4.

use core::fmt;
use core::hint::spin_loop;
use core::mem::size_of;
use core::ops::Range;

use heapless::Vec as HeaplessVec;
use log::{debug, info, trace, warn};
use smoltcp::phy::{self, Device, DeviceCapabilities};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;

use crate::hal::pi4_wifi::Pi4WifiState;
use crate::hal::{
    Cyw43Hal, HalError, Hardware, SdioBusWidth, SdioFunction, WifiFirmwareBundle, WifiPowerState,
    WifiResetState,
};
#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
use crate::local_seat_pi4::{wifi_progress_begin, wifi_progress_finish, wifi_progress_tick};
use crate::net::{
    wifi_boot_join_should_defer, ConsoleNetConfig, NetDevice, NetDeviceCounters, NetDriverError,
    WifiCredentials,
};
use crate::net_consts::MAX_FRAME_LEN;
use crate::rust_alloc::boxed::Box;

const SDIO_STARTUP_CLOCK_HZ: u32 = 400_000;
// Match Linux brcmfmac on Pi 4: request 50 MHz high-speed SDIO and let the
// HAL/SDHCI divider report the effective board clock.
const SDIO_DATA_CLOCK_HZ: u32 = 50_000_000;
const SDIO_CCCR_IOEX: u32 = 0x02;
const DEFAULT_WIFI_MAC: [u8; 6] = [0x02, 0x43, 0x4f, 0x48, 0x58, 0x55];

const SDPCM_HEADER_LEN: usize = 12;
const SDPCM_HWHDR_LEN: usize = 4;
const SDPCM_CONTROL_TX_HWEXT_LEN: usize = 8;
const SDPCM_CONTROL_TX_HEADER_LEN: usize = SDPCM_HEADER_LEN;
const SDPCM_CONTROL_TX_EXT_HEADER_LEN: usize = SDPCM_HEADER_LEN + SDPCM_CONTROL_TX_HWEXT_LEN;
const SDPCM_CONTROL_TX_BLOCK_SIZE: usize = 512;
const SDPCM_CONTROL_TX_LAST_FRAME: u32 = 1 << 24;
const CDC_HEADER_LEN: usize = 16;
const BDC_HEADER_LEN: usize = 4;
const DATA_PADDING_LEN: usize = 2;

const FRAME_BUF_LEN: usize = 2112;
const CONTROL_RESPONSE_BUF_LEN: usize = 2048;
const CLM_CHUNK_SIZE: usize = 1400;
const CLM_IOVAR_NAME_LEN: usize = 8;
const CLM_IOVAR_HEADER_LEN: usize = 12;
const IOCTL_WAIT_LOOPS: usize = 8_000;
// The bounded startup-link lane now preserves the exact blocker end-to-end, so
// long ioctl waits only stretch a known failure. Keep one short startup-link
// window, then collapse rescue, repeat-no-progress, and the final bounded
// probe aggressively.
const IOCTL_WAIT_LOOPS_STARTUP_LINK_STABILIZED: usize = 32_000;
const IOCTL_WAIT_LOOPS_STARTUP_LINK_RESCUE: usize = 8_000;
const IOCTL_WAIT_LOOPS_STARTUP_LINK_RESCUE_REPEAT: usize = 2_000;
const IOCTL_WAIT_LOOPS_STARTUP_LINK_FINAL_BOUNDED: usize = 1_000;
const IOCTL_NO_PROGRESS_AFTER_NONMATCHING_LIMIT: usize = 128;
const JOIN_WAIT_LOOPS: usize = 64_000;
const DEFERRED_JOIN_FRAME_BUDGET: usize = 8;
const DEFERRED_JOIN_POLL_LIMIT: u16 = 1_200;
const HOST_EAPOL_JOIN_SUBMIT_PROOF_POLLS: usize = 24;
const CREDIT_WAIT_LOOPS: usize = 2_000;
const RX_PUMP_LIMIT: usize = 8;
const LINUX_STARTUP_STATUS_DRAIN_BUDGET: usize = 2;
const POST_UP_EVENT_DRAIN_BUDGET: usize = 8;

const CHANNEL_CONTROL: u8 = 0;
const CHANNEL_EVENT: u8 = 1;
const CHANNEL_DATA: u8 = 2;
const CHANNEL_GLOM: u8 = 3;

const DOWNLOAD_FLAG_BEGIN: u16 = 0x0002;
const DOWNLOAD_FLAG_END: u16 = 0x0004;
const DOWNLOAD_FLAG_HANDLER_VER: u16 = 0x1000;
const DOWNLOAD_TYPE_CLM: u16 = 2;

const ETH_HEADER_LEN: usize = 14;
const ETH_P_LINK_CTL: u16 = 0x886c;
const ETH_P_EAPOL: u16 = 0x888e;
const EAPOL_HEADER_LEN: usize = 4;
const EAPOL_PACKET_TYPE_KEY: u8 = 3;
const EAPOL_KEY_MIN_BODY_LEN: usize = 95;
const EAPOL_KEY_INFO_KEY_VERSION_MASK: u16 = 0x0007;
const EAPOL_KEY_INFO_KEY_TYPE: u16 = 0x0008;
const EAPOL_KEY_INFO_INSTALL: u16 = 0x0040;
const EAPOL_KEY_INFO_ACK: u16 = 0x0080;
const EAPOL_KEY_INFO_MIC: u16 = 0x0100;
const EAPOL_KEY_INFO_SECURE: u16 = 0x0200;
const EAPOL_KEY_INFO_ENCRYPTED_KEY_DATA: u16 = 0x1000;
const BCMILCP_SUBTYPE_VENDOR_LONG: u16 = 32769;
const BCMILCP_BCM_SUBTYPE_EVENT: u16 = 1;
const BROADCOM_OUI: [u8; 3] = [0x00, 0x10, 0x18];
const EVENT_FLAG_LINK: u16 = 0x0001;

const EVENT_SET_SSID: u8 = 0;
const EVENT_AUTH: u8 = 3;
const EVENT_DEAUTH: u8 = 5;
const EVENT_ASSOC: u8 = 7;
const EVENT_ASSOC_IND: u8 = 8;
const EVENT_REASSOC: u8 = 9;
const EVENT_REASSOC_IND: u8 = 10;
const EVENT_DISASSOC: u8 = 11;
const EVENT_DISASSOC_IND: u8 = 12;
const EVENT_LINK: u8 = 16;
const EVENT_ROAM: u8 = 19;
const EVENT_MIC_ERROR: u8 = 33;
const EVENT_PSK_SUP: u8 = 46;
const EVENT_IF: u8 = 54;
const STATUS_SUCCESS: u32 = 0;
const STATUS_FAIL: u32 = 1;
const STATUS_NO_NETWORKS: u32 = 3;
const STATUS_ABORT: u32 = 4;
const STATUS_UNSOLICITED: u32 = 6;
const EVENT_MASK_LEN: usize = 27;
const EVENTMSGS_EXT_VER: u8 = 1;
const EVENTMSGS_EXT_SET_MASK: u8 = 3;
const EVENTMSGS_EXT_MAX_GET_SIZE: u8 = 0;
const EVENTMSGS_EXT_HEADER_LEN: usize = 4;
const LINUX_EVENTMSGS_EXT_MASK: [u8; EVENT_MASK_LEN] = [
    0x61, 0x15, 0x0b, 0x00, 0x02, 0x42, 0xc0, 0x11, 0x60, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x78, 0x00, 0x00, 0x00,
];
const JOIN_COMPLETION_EVENTS: [u8; 13] = [
    EVENT_SET_SSID,
    EVENT_AUTH,
    EVENT_ASSOC,
    EVENT_REASSOC,
    EVENT_LINK,
    EVENT_DEAUTH,
    EVENT_DISASSOC,
    EVENT_DISASSOC_IND,
    EVENT_ASSOC_IND,
    EVENT_REASSOC_IND,
    EVENT_ROAM,
    EVENT_MIC_ERROR,
    EVENT_PSK_SUP,
];
const DEFAULT_SCAN_CHANNEL_TIME_MS: u32 = 40;
const DEFAULT_SCAN_UNASSOC_TIME_MS: u32 = 40;
const BCME_UNSUPPORTED: u32 = 0xffff_ffe9;
const BCME_BADARG: u32 = 0xffff_fffe;
const BSSCFG_PRIMARY_INDEX: u32 = 0;
const WSEC_PMK_LEN: usize = 32;
const WSEC_PMK_KEY_CAPACITY: usize = 128;
const WSEC_PMK_PAYLOAD_LEN: usize = 4 + WSEC_PMK_KEY_CAPACITY;
const WSEC_LEGACY_HEX_PMK_LEN: usize = WSEC_PMK_LEN * 2;
const WSEC_FLAG_PASSPHRASE: u16 = 1;
const WPA2_PSK_MIN_PASSPHRASE_LEN: usize = 8;
const WPA2_PSK_MAX_PASSPHRASE_LEN: usize = 63;
const WPA2_PSK_PBKDF2_ROUNDS: u16 = 4096;
const WPA2_PSK_BLOCK_COUNT: u32 = 2;
const SHA1_BLOCK_LEN: usize = 64;
const SHA1_DIGEST_LEN: usize = 20;
const LINUX_JOIN_PREF_DEFAULT: [u8; 8] = [0x04, 0x02, 0x08, 0x01, 0x01, 0x02, 0x00, 0x00];

const BDC_VERSION: u8 = 2;
const BDC_VERSION_SHIFT: u8 = 4;

const WSEC_AES: u32 = 0x04;
const AUTH_OPEN: u32 = 0x00;
const MFP_CAPABLE: u32 = 1;
const WPA_AUTH_DISABLED: u32 = 0x0000;
const WPA_AUTH_WPA2_UNSPECIFIED: u32 = 0x0040;
const WPA_AUTH_WPA2_PSK: u32 = 0x0080;
const WPA_AUTH_WPA2_PSK_OR_UNSPECIFIED: u32 = WPA_AUTH_WPA2_UNSPECIFIED | WPA_AUTH_WPA2_PSK;
const WPA2_PSK_CCMP_RSN_IE: [u8; 22] = [
    0x30, 0x14, 0x01, 0x00, 0x00, 0x0f, 0xac, 0x04, 0x01, 0x00, 0x00, 0x0f, 0xac, 0x04, 0x01, 0x00,
    0x00, 0x0f, 0xac, 0x02, 0x00, 0x00,
];
const LINUX_REVINFO_LEN: usize = 68;
const WIFI_SSID_MAX_LEN: usize = 32;
const LINUX_EXT_JOIN_SSID_OFFSET: usize = 0;
const LINUX_EXT_JOIN_SCAN_OFFSET: usize = LINUX_EXT_JOIN_SSID_OFFSET + 36;
const LINUX_EXT_JOIN_ASSOC_OFFSET: usize = LINUX_EXT_JOIN_SCAN_OFFSET + 20;
const LINUX_EXT_JOIN_PARAMS_LEN: usize = LINUX_EXT_JOIN_ASSOC_OFFSET + 12;
const LINUX_BSSCFG_JOIN_PAYLOAD_LEN: usize = LINUX_EXT_JOIN_PARAMS_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirmwareLayout {
    pub firmware_len: usize,
    pub nvram_len: usize,
    pub clm_len: usize,
    pub board_type: &'static str,
}

impl FirmwareLayout {
    #[must_use]
    pub const fn from_bundle(bundle: WifiFirmwareBundle<'static>) -> Self {
        Self {
            firmware_len: bundle.firmware.len(),
            nvram_len: bundle.nvram.len(),
            clm_len: match bundle.clm_blob {
                Some(clm) => clm.len(),
                None => 0,
            },
            board_type: bundle.board_type,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeReport {
    pub effective_clock_hz: u32,
    pub ioex: u8,
    pub bus_width: SdioBusWidth,
    pub firmware: FirmwareLayout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IoctlType {
    Get = 0,
    Set = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
enum Ioctl {
    Up = 2,
    GetRevInfo = 98,
    SetInfra = 20,
    SetAuth = 22,
    SetSsid = 26,
    SetAntdiv = 64,
    SetPm = 86,
    SetGmode = 110,
    SetWsec = 134,
    SetBand = 142,
    SetWpaAuth = 165,
    SetScanChannelTime = 185,
    SetScanUnassocTime = 187,
    GetVar = 262,
    SetVar = 263,
    SetWsecPmk = 268,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Cyw43Event {
    flags: u16,
    event_type: u8,
    status: u32,
    reason: u32,
    auth_type: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RxFrameResult {
    None,
    Control {
        cmd: u32,
        id: u16,
        status: u32,
        response_len: usize,
    },
    Event(Cyw43Event),
    Data(HeaplessVec<u8, MAX_FRAME_LEN>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RxSdpcmHeader {
    packet_len: usize,
    payload_start: usize,
    channel: u8,
    credit: u8,
}

#[derive(Debug)]
pub enum DriverError {
    NoDevice,
    Hal(HalError),
    InvalidFirmware(&'static str),
    Config(&'static str),
    Protocol(&'static str),
    IoctlFailed { cmd: u32, status: u32 },
    JoinFailed { status: u32, auth_status: u32 },
    FrameTooLarge,
    ResponseTooLarge,
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("cyw43 device not present"),
            Self::Hal(err) => write!(f, "{err}"),
            Self::InvalidFirmware(reason) => write!(f, "cyw43 firmware invalid: {reason}"),
            Self::Config(reason) => write!(f, "cyw43 config invalid: {reason}"),
            Self::Protocol(reason) => write!(f, "cyw43 protocol error: {reason}"),
            Self::IoctlFailed { cmd, status } => {
                write!(f, "cyw43 ioctl 0x{cmd:08x} failed status=0x{status:08x}")
            }
            Self::JoinFailed {
                status,
                auth_status,
            } => {
                write!(
                    f,
                    "cyw43 join failed status=0x{status:08x} auth=0x{auth_status:08x}"
                )
            }
            Self::FrameTooLarge => f.write_str("cyw43 frame too large"),
            Self::ResponseTooLarge => f.write_str("cyw43 response too large"),
        }
    }
}

impl From<HalError> for DriverError {
    fn from(value: HalError) -> Self {
        Self::Hal(value)
    }
}

fn is_transport_retryable(err: &HalError) -> bool {
    matches!(
        err,
        HalError::Unsupported("sdhci-command-error")
            | HalError::Unsupported("sdio-ocr-timeout")
            | HalError::Unsupported("sdio-card-not-ready")
            | HalError::Unsupported("sdhci-int-timeout")
    )
}

#[inline]
fn linux_optional_iovar_allows_unsupported(name: &str, cmd: u32, status: u32) -> bool {
    name == "ulp_sdioctrl" && cmd == Ioctl::GetVar as u32 && status == BCME_UNSUPPORTED
}

#[inline]
const fn linux_first_control_plane_iovar_order() -> [&'static str; 3] {
    ["bus:txglomalign", "ulp_sdioctrl", "bus:rxglom"]
}

#[inline]
const fn linux_attach_control_plane_probe_order() -> [&'static str; 5] {
    [
        "bus:txglomalign",
        "ulp_sdioctrl",
        "bus:rxglom",
        "cur_etheraddr",
        "revinfo",
    ]
}

#[inline]
const fn linux_station_path_keeps_txglom_configured_before_preinit() -> bool {
    true
}

#[inline]
const fn linux_station_path_keeps_rxglom_configured_before_preinit() -> bool {
    true
}

#[inline]
fn optional_control_plane_iovar_allows_failure(name: &str, err: &DriverError) -> bool {
    matches!(
        err,
        DriverError::IoctlFailed {
            status: BCME_UNSUPPORTED,
            ..
        } if matches!(
            name,
            "ampdu_ba_wsize"
                | "ampdu_mpdu"
                | "bsscfg:sup_wpa2_eapver"
                | "bsscfg:sup_wpa_tmo"
                | "bus:txglom"
                | "mfp"
                | "sup_wpa"
                | "sup_wpa2_eapver"
                | "sup_wpa_tmo"
        )
    )
}

#[inline]
const fn firmware_supplicant_wrapper_fallback_allowed() -> bool {
    true
}

#[inline]
const fn linux_station_path_enables_apsta() -> bool {
    false
}

#[inline]
const fn linux_station_path_sets_country() -> bool {
    false
}

#[inline]
const fn linux_station_path_sets_antdiv_before_join() -> bool {
    false
}

#[inline]
const fn linux_station_path_sets_ampdu_limits_before_join() -> bool {
    false
}

#[inline]
const fn linux_station_path_sets_legacy_gmode() -> bool {
    false
}

#[inline]
const fn linux_station_path_sets_legacy_band() -> bool {
    false
}

#[inline]
const fn linux_station_path_sets_power_mode_before_join() -> bool {
    false
}

#[inline]
const fn ioctl_failed_status(err: &DriverError) -> Option<u32> {
    match err {
        DriverError::IoctlFailed { status, .. } => Some(*status),
        _ => None,
    }
}

#[inline]
fn optional_txbf_allows_failure(err: &DriverError) -> bool {
    ioctl_failed_status(err) == Some(BCME_UNSUPPORTED)
}

fn write_wsec_pmk_payload(
    payload: &mut [u8],
    ssid: &[u8],
    psk: &[u8],
) -> Result<WsecPmkKind, DriverError> {
    if payload.len() < WSEC_PMK_PAYLOAD_LEN {
        return Err(DriverError::FrameTooLarge);
    }
    let mut pmk = [0u8; WSEC_PMK_LEN];
    let kind = fill_wpa2_psk_pmk(ssid, psk, &mut pmk)?;
    payload[..WSEC_PMK_PAYLOAD_LEN].fill(0);
    put_u16_le(payload, 0, WSEC_PMK_LEN as u16);
    put_u16_le(payload, 2, 0);
    payload[4..4 + WSEC_PMK_LEN].copy_from_slice(&pmk);
    Ok(kind)
}

fn write_wsec_legacy_hex_pmk_payload(
    payload: &mut [u8],
    ssid: &[u8],
    psk: &[u8],
) -> Result<(), DriverError> {
    if payload.len() < WSEC_PMK_PAYLOAD_LEN {
        return Err(DriverError::FrameTooLarge);
    }
    let mut pmk = [0u8; WSEC_PMK_LEN];
    fill_wpa2_psk_pmk(ssid, psk, &mut pmk)?;
    payload[..WSEC_PMK_PAYLOAD_LEN].fill(0);
    put_u16_le(payload, 0, WSEC_LEGACY_HEX_PMK_LEN as u16);
    put_u16_le(payload, 2, WSEC_FLAG_PASSPHRASE);
    write_lower_hex(&pmk, &mut payload[4..4 + WSEC_LEGACY_HEX_PMK_LEN]);
    Ok(())
}

fn legacy_set_ssid_payload_len(ssid: &str) -> Result<usize, DriverError> {
    if ssid.len() > WIFI_SSID_MAX_LEN {
        return Err(DriverError::Config("wifi-ssid-too-long"));
    }
    Ok(36)
}

fn write_legacy_set_ssid_payload(payload: &mut [u8], ssid: &str) -> Result<(), DriverError> {
    let payload_len = legacy_set_ssid_payload_len(ssid)?;
    if payload.len() < payload_len {
        return Err(DriverError::FrameTooLarge);
    }
    payload[..payload_len].fill(0);
    put_u32_le(
        payload,
        0,
        u32::try_from(ssid.len()).map_err(|_| DriverError::Config("wifi-ssid-too-long"))?,
    );
    payload[4..4 + ssid.len()].copy_from_slice(ssid.as_bytes());
    Ok(())
}

fn write_linux_bsscfg_join_payload(payload: &mut [u8], ssid: &str) -> Result<(), DriverError> {
    if ssid.len() > WIFI_SSID_MAX_LEN {
        return Err(DriverError::Config("wifi-ssid-too-long"));
    }
    if payload.len() < LINUX_BSSCFG_JOIN_PAYLOAD_LEN {
        return Err(DriverError::FrameTooLarge);
    }

    payload[..LINUX_BSSCFG_JOIN_PAYLOAD_LEN].fill(0);
    put_u32_le(
        payload,
        LINUX_EXT_JOIN_SSID_OFFSET,
        u32::try_from(ssid.len()).map_err(|_| DriverError::Config("wifi-ssid-too-long"))?,
    );
    payload[LINUX_EXT_JOIN_SSID_OFFSET + 4..LINUX_EXT_JOIN_SSID_OFFSET + 4 + ssid.len()]
        .copy_from_slice(ssid.as_bytes());

    payload[LINUX_EXT_JOIN_SCAN_OFFSET] = 0xff;
    put_u32_le(payload, LINUX_EXT_JOIN_SCAN_OFFSET + 4, u32::MAX);
    put_u32_le(payload, LINUX_EXT_JOIN_SCAN_OFFSET + 8, u32::MAX);
    put_u32_le(payload, LINUX_EXT_JOIN_SCAN_OFFSET + 12, u32::MAX);
    put_u32_le(payload, LINUX_EXT_JOIN_SCAN_OFFSET + 16, u32::MAX);

    payload[LINUX_EXT_JOIN_ASSOC_OFFSET..LINUX_EXT_JOIN_ASSOC_OFFSET + 6].fill(0xff);
    put_u32_le(payload, LINUX_EXT_JOIN_ASSOC_OFFSET + 8, 0);
    Ok(())
}

const fn linux_wpa2_join_sets_mfp_without_rsn_ie() -> bool {
    false
}

fn join_security_iovar_name(name: &str) -> bool {
    matches!(
        name,
        "auth"
            | "wpaie"
            | "wsec"
            | "wpa_auth"
            | "sup_wpa"
            | "sup_wpa2_eapver"
            | "sup_wpa_tmo"
            | "bsscfg:sup_wpa"
            | "bsscfg:sup_wpa2_eapver"
            | "bsscfg:sup_wpa_tmo"
    )
}

fn join_security_iovar_failure_exact_error(name: &str, err: &DriverError) -> Option<&'static str> {
    match (name, err) {
        (
            "wpaie",
            DriverError::Protocol("ioctl-timeout")
            | DriverError::Protocol("ioctl-no-progress-after-frame"),
        ) => Some("cyw43-join-security-wpaie-loop"),
        (
            "wsec",
            DriverError::Protocol("ioctl-timeout")
            | DriverError::Protocol("ioctl-no-progress-after-frame"),
        ) => Some("cyw43-join-security-wsec-first-loop"),
        (
            "sup_wpa",
            DriverError::Protocol("ioctl-timeout")
            | DriverError::Protocol("ioctl-no-progress-after-frame"),
        ) => Some("cyw43-join-security-sup-wpa-loop"),
        (
            "bsscfg:sup_wpa",
            DriverError::Protocol("ioctl-timeout")
            | DriverError::Protocol("ioctl-no-progress-after-frame"),
        ) => Some("cyw43-join-security-bsscfg-sup-wpa-loop"),
        _ => None,
    }
}

fn join_security_wpa_auth_stage(value: u32) -> &'static str {
    match value {
        WPA_AUTH_WPA2_PSK_OR_UNSPECIFIED => "initial",
        WPA_AUTH_WPA2_PSK => "final",
        _ => "unknown",
    }
}

fn join_security_wpa_auth_failure_exact_error(
    value: u32,
    err: &DriverError,
) -> Option<&'static str> {
    match (value, err) {
        (
            WPA_AUTH_WPA2_PSK_OR_UNSPECIFIED,
            DriverError::Protocol("ioctl-timeout")
            | DriverError::Protocol("ioctl-no-progress-after-frame"),
        ) => Some("cyw43-join-security-wpa-auth-initial-loop"),
        (
            WPA_AUTH_WPA2_PSK,
            DriverError::Protocol("ioctl-timeout")
            | DriverError::Protocol("ioctl-no-progress-after-frame"),
        ) => Some("cyw43-join-security-wpa-auth-final-loop"),
        _ => None,
    }
}

fn join_iovar_fallback_allows_set_ssid(err: &DriverError) -> bool {
    matches!(
        err,
        DriverError::IoctlFailed { cmd, status }
            if *cmd == Ioctl::SetVar as u32
                && matches!(*status, BCME_UNSUPPORTED | BCME_BADARG)
    )
}

fn fill_wpa2_psk_pmk(
    ssid: &[u8],
    psk: &[u8],
    output: &mut [u8; WSEC_PMK_LEN],
) -> Result<WsecPmkKind, DriverError> {
    if psk.is_empty() {
        return Err(DriverError::Config("wifi-psk-empty"));
    }
    if decode_hex_pmk(psk, output) {
        return Ok(WsecPmkKind::HexPmk);
    }
    if psk.len() < WPA2_PSK_MIN_PASSPHRASE_LEN {
        return Err(DriverError::Config("wifi-psk-too-short"));
    }
    if psk.len() > WPA2_PSK_MAX_PASSPHRASE_LEN {
        return Err(DriverError::Config("wifi-psk-invalid"));
    }
    derive_wpa2_psk_pmk(psk, ssid, output);
    Ok(WsecPmkKind::Pbkdf2Passphrase)
}

fn wsec_pmk_legacy_hex_fallback_allowed(psk: &[u8]) -> bool {
    psk_is_hex_pmk(psk)
        || (WPA2_PSK_MIN_PASSPHRASE_LEN..=WPA2_PSK_MAX_PASSPHRASE_LEN).contains(&psk.len())
}

const fn ioctl_no_progress_after_nonmatching_frames(
    nonmatching_frames: usize,
    no_progress_polls: usize,
) -> bool {
    nonmatching_frames != 0 && no_progress_polls >= IOCTL_NO_PROGRESS_AFTER_NONMATCHING_LIMIT
}

fn psk_is_hex_pmk(input: &[u8]) -> bool {
    if input.len() != WSEC_PMK_LEN * 2 {
        return false;
    }
    input.iter().copied().all(|byte| hex_nibble(byte).is_some())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct JoinCompletionState {
    auth_status: u32,
    carrier_confirmed: bool,
    link_down_seen: bool,
    set_ssid_completed: bool,
    psk_completed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JoinCompletionRule {
    SetSsid,
    FirmwareSupplicantPskSup,
    HostEapolRequired,
}

impl JoinCompletionRule {
    const fn label(self) -> &'static str {
        match self {
            Self::SetSsid => "set-ssid",
            Self::FirmwareSupplicantPskSup => "firmware-supplicant-psk-sup",
            Self::HostEapolRequired => "host-eapol-required",
        }
    }

    const fn firmware_supplicant_required(self) -> bool {
        matches!(self, Self::FirmwareSupplicantPskSup)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FirmwareSupplicantPath {
    PrimaryPlain,
    BsscfgWrapper,
    Unsupported,
}

impl FirmwareSupplicantPath {
    const fn label(self) -> &'static str {
        match self {
            Self::PrimaryPlain => "primary-plain",
            Self::BsscfgWrapper => "bsscfg-wrapper",
            Self::Unsupported => "unsupported",
        }
    }

    const fn order_label(self) -> &'static str {
        match self {
            Self::PrimaryPlain => "sup_wpa",
            Self::BsscfgWrapper => "sup_wpa-or-bsscfg_sup_wpa",
            Self::Unsupported => "sup_wpa-or-bsscfg_sup_wpa-host-eapol",
        }
    }

    const fn completion_rule(self) -> JoinCompletionRule {
        match self {
            Self::PrimaryPlain | Self::BsscfgWrapper => {
                JoinCompletionRule::FirmwareSupplicantPskSup
            }
            Self::Unsupported => JoinCompletionRule::HostEapolRequired,
        }
    }
}

#[inline]
const fn event_has_link_flag(event: Cyw43Event) -> bool {
    event.flags & EVENT_FLAG_LINK != 0
}

#[inline]
const fn join_completion_link_up(
    completion_rule: JoinCompletionRule,
    state: JoinCompletionState,
) -> bool {
    let security_complete = match completion_rule {
        JoinCompletionRule::SetSsid => true,
        JoinCompletionRule::FirmwareSupplicantPskSup | JoinCompletionRule::HostEapolRequired => {
            state.psk_completed
        }
    };
    state.set_ssid_completed
        && security_complete
        && (!state.link_down_seen || state.carrier_confirmed)
}

#[inline]
const fn join_completion_timeout_reason(completion_rule: JoinCompletionRule) -> &'static str {
    match completion_rule {
        JoinCompletionRule::HostEapolRequired => "host-eapol-required",
        JoinCompletionRule::SetSsid | JoinCompletionRule::FirmwareSupplicantPskSup => {
            "join-timeout"
        }
    }
}

fn join_event_result(
    event: Cyw43Event,
    secure: bool,
    completion_rule: JoinCompletionRule,
    state: &mut JoinCompletionState,
) -> Option<Result<(), DriverError>> {
    if secure && matches!(completion_rule, JoinCompletionRule::SetSsid) {
        return Some(Err(DriverError::Protocol("host-eapol-required")));
    }

    if event.event_type == EVENT_AUTH && event.status != STATUS_SUCCESS {
        state.auth_status = event.status;
        if secure && event.status == STATUS_FAIL {
            return Some(Err(DriverError::JoinFailed {
                status: event.status,
                auth_status: state.auth_status,
            }));
        }
    }

    match event.event_type {
        EVENT_SET_SSID if event.status == STATUS_SUCCESS => {
            state.set_ssid_completed = true;
            if join_completion_link_up(completion_rule, *state) {
                Some(Ok(()))
            } else {
                None
            }
        }
        EVENT_SET_SSID if event.status == STATUS_NO_NETWORKS => {
            Some(Err(DriverError::JoinFailed {
                status: event.status,
                auth_status: state.auth_status,
            }))
        }
        EVENT_SET_SSID => Some(Err(DriverError::JoinFailed {
            status: event.status,
            auth_status: state.auth_status,
        })),
        EVENT_LINK if event.status == STATUS_SUCCESS && event_has_link_flag(event) => {
            state.carrier_confirmed = true;
            state.link_down_seen = false;
            if join_completion_link_up(completion_rule, *state) {
                Some(Ok(()))
            } else {
                None
            }
        }
        EVENT_LINK if event.status == STATUS_SUCCESS => {
            state.carrier_confirmed = false;
            state.link_down_seen = true;
            None
        }
        EVENT_LINK => {
            state.carrier_confirmed = false;
            state.link_down_seen = true;
            state.set_ssid_completed = false;
            state.psk_completed = false;
            Some(Err(DriverError::JoinFailed {
                status: event.status,
                auth_status: state.auth_status,
            }))
        }
        EVENT_PSK_SUP if secure && event.status == STATUS_UNSOLICITED => {
            state.psk_completed = true;
            if join_completion_link_up(completion_rule, *state) {
                Some(Ok(()))
            } else {
                None
            }
        }
        EVENT_PSK_SUP if secure => Some(Err(DriverError::JoinFailed {
            status: event.status,
            auth_status: state.auth_status,
        })),
        _ => None,
    }
}

fn event_link_state_update(event: Cyw43Event, deferred_join_pending: bool) -> Option<bool> {
    match event.event_type {
        EVENT_SET_SSID if event.status != STATUS_SUCCESS => Some(false),
        EVENT_LINK
            if event.status == STATUS_SUCCESS
                && event_has_link_flag(event)
                && !deferred_join_pending =>
        {
            Some(true)
        }
        EVENT_LINK if event.status == STATUS_SUCCESS => Some(false),
        EVENT_LINK => Some(false),
        EVENT_DEAUTH | EVENT_DISASSOC => Some(false),
        _ => None,
    }
}

fn decode_hex_pmk(input: &[u8], output: &mut [u8; WSEC_PMK_LEN]) -> bool {
    if input.len() != WSEC_PMK_LEN * 2 {
        return false;
    }
    let mut index = 0;
    while index < WSEC_PMK_LEN {
        let Some(hi) = hex_nibble(input[index * 2]) else {
            return false;
        };
        let Some(lo) = hex_nibble(input[index * 2 + 1]) else {
            return false;
        };
        output[index] = (hi << 4) | lo;
        index += 1;
    }
    true
}

fn write_lower_hex(input: &[u8], output: &mut [u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut index = 0usize;
    while index < input.len() && index.saturating_mul(2).saturating_add(1) < output.len() {
        let value = input[index];
        output[index * 2] = HEX[usize::from(value >> 4)];
        output[index * 2 + 1] = HEX[usize::from(value & 0x0f)];
        index += 1;
    }
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn derive_wpa2_psk_pmk(passphrase: &[u8], ssid: &[u8], output: &mut [u8; WSEC_PMK_LEN]) {
    // WPA2-PSK firmware offload follows Linux: store the 32-byte PBKDF2 PMK, not the raw passphrase.
    for block_index in 1..=WPA2_PSK_BLOCK_COUNT {
        let mut block_suffix = [0u8; 4];
        block_suffix.copy_from_slice(&block_index.to_be_bytes());
        let mut u = hmac_sha1(passphrase, ssid, &block_suffix);
        let mut t = u;
        for _ in 1..WPA2_PSK_PBKDF2_ROUNDS {
            u = hmac_sha1(passphrase, &u, &[]);
            let mut index = 0;
            while index < SHA1_DIGEST_LEN {
                t[index] ^= u[index];
                index += 1;
            }
        }
        let output_offset = (block_index as usize - 1) * SHA1_DIGEST_LEN;
        let remaining = WSEC_PMK_LEN - output_offset;
        let copy_len = core::cmp::min(SHA1_DIGEST_LEN, remaining);
        output[output_offset..output_offset + copy_len].copy_from_slice(&t[..copy_len]);
    }
}

fn hmac_sha1(key: &[u8], first: &[u8], second: &[u8]) -> [u8; SHA1_DIGEST_LEN] {
    let mut key_block = [0u8; SHA1_BLOCK_LEN];
    if key.len() > SHA1_BLOCK_LEN {
        let digest = sha1_digest(key, &[], &[]);
        key_block[..SHA1_DIGEST_LEN].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0x36u8; SHA1_BLOCK_LEN];
    let mut outer_pad = [0x5cu8; SHA1_BLOCK_LEN];
    let mut index = 0;
    while index < SHA1_BLOCK_LEN {
        inner_pad[index] ^= key_block[index];
        outer_pad[index] ^= key_block[index];
        index += 1;
    }
    let inner = sha1_digest(&inner_pad, first, second);
    sha1_digest(&outer_pad, &inner, &[])
}

fn sha1_digest(first: &[u8], second: &[u8], third: &[u8]) -> [u8; SHA1_DIGEST_LEN] {
    let mut state = Sha1State::new();
    state.update(first);
    state.update(second);
    state.update(third);
    state.finalize()
}

#[derive(Clone)]
struct Sha1State {
    state: [u32; 5],
    buffer: [u8; SHA1_BLOCK_LEN],
    buffer_len: usize,
    total_len: u64,
}

impl Sha1State {
    const fn new() -> Self {
        Self {
            state: [
                0x6745_2301,
                0xefcd_ab89,
                0x98ba_dcfe,
                0x1032_5476,
                0xc3d2_e1f0,
            ],
            buffer: [0; SHA1_BLOCK_LEN],
            buffer_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, mut input: &[u8]) {
        self.total_len = self.total_len.wrapping_add(input.len() as u64);
        if self.buffer_len != 0 {
            let fill = core::cmp::min(SHA1_BLOCK_LEN - self.buffer_len, input.len());
            self.buffer[self.buffer_len..self.buffer_len + fill].copy_from_slice(&input[..fill]);
            self.buffer_len += fill;
            input = &input[fill..];
            if self.buffer_len == SHA1_BLOCK_LEN {
                let block = self.buffer;
                self.process_block(&block);
                self.buffer_len = 0;
            }
        }
        while input.len() >= SHA1_BLOCK_LEN {
            let mut block = [0u8; SHA1_BLOCK_LEN];
            block.copy_from_slice(&input[..SHA1_BLOCK_LEN]);
            self.process_block(&block);
            input = &input[SHA1_BLOCK_LEN..];
        }
        if !input.is_empty() {
            self.buffer[..input.len()].copy_from_slice(input);
            self.buffer_len = input.len();
        }
    }

    fn finalize(mut self) -> [u8; SHA1_DIGEST_LEN] {
        let bit_len = self.total_len.wrapping_mul(8);
        self.update(&[0x80]);
        let zeros = [0u8; SHA1_BLOCK_LEN];
        while self.buffer_len != 56 {
            let fill = if self.buffer_len < 56 {
                56 - self.buffer_len
            } else {
                SHA1_BLOCK_LEN - self.buffer_len
            };
            self.update(&zeros[..fill]);
        }
        self.update(&bit_len.to_be_bytes());
        let mut digest = [0u8; SHA1_DIGEST_LEN];
        for (index, word) in self.state.iter().copied().enumerate() {
            digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        digest
    }

    fn process_block(&mut self, block: &[u8; SHA1_BLOCK_LEN]) {
        let mut schedule = [0u32; 80];
        for (index, chunk) in block.chunks_exact(4).enumerate().take(16) {
            schedule[index] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for index in 16..80 {
            schedule[index] = (schedule[index - 3]
                ^ schedule[index - 8]
                ^ schedule[index - 14]
                ^ schedule[index - 16])
                .rotate_left(1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];

        for (index, word) in schedule.iter().copied().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
    }
}

#[inline]
fn first_control_plane_retry_after_promoted_timeout(
    experimental_no_ht_transport: bool,
    control_plane_probe_pending: bool,
    err: &DriverError,
) -> bool {
    experimental_no_ht_transport
        && control_plane_probe_pending
        && matches!(
            err,
            DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-promoted-rearm-timeout"
            ))
        )
}

#[inline]
fn first_control_plane_retry_after_startup_link_reply_failure(
    experimental_no_ht_transport: bool,
    control_plane_probe_pending: bool,
    err: &DriverError,
) -> bool {
    experimental_no_ht_transport
        && control_plane_probe_pending
        && matches!(
            err,
            DriverError::Hal(HalError::Unsupported(reason))
                if startup_link_reply_rescue_reason(reason)
        )
}

#[inline]
fn startup_link_reply_failure_reason(reason: &str) -> bool {
    reason.starts_with("cyw43-function2-enable-latched-not-ready")
        || reason.starts_with("cyw43-function2-reply-")
        || reason == "cyw43-control-plane-no-reply-linux-f2-armed"
        || reason == "cyw43-control-plane-pure-f2-startup-link-no-reply"
        || reason == "cyw43-control-plane-linux-interrupts-deferred"
        || reason == "cyw43-control-plane-sideband-unreadable"
        || reason.starts_with("cyw43-control-plane-sideband-")
        || reason == "cyw43-control-plane-startup-link-reply-timeout"
        || reason == "cyw43-control-plane-passive-startup-link-timeout"
}

#[inline]
fn startup_link_reply_rescue_reason(reason: &str) -> bool {
    reason.starts_with("cyw43-function2-enable-latched-not-ready")
        || reason == "cyw43-control-plane-no-reply-linux-f2-armed"
        || reason == "cyw43-control-plane-pure-f2-startup-link-no-reply"
        || reason == "cyw43-control-plane-linux-interrupts-deferred"
        || reason == "cyw43-control-plane-sideband-unreadable"
        || reason.starts_with("cyw43-control-plane-sideband-")
        || reason == "cyw43-control-plane-startup-link-reply-timeout"
        || reason == "cyw43-control-plane-passive-startup-link-timeout"
}

#[inline]
fn startup_link_ioctl_timeout_preserved_exact_error(
    startup_link_stabilized: bool,
    _control_plane_probe_pending: bool,
    preserved_exact_error: Option<&'static str>,
) -> Option<&'static str> {
    if startup_link_stabilized {
        preserved_exact_error.filter(|reason| startup_link_reply_failure_reason(reason))
    } else {
        None
    }
}

#[inline]
fn control_plane_retry_after_promoted_timeout_target_clock_hz(
    experimental_no_ht_transport: bool,
    control_plane_probe_pending: bool,
    current_clock_hz: u32,
    err: &DriverError,
) -> Option<u32> {
    if first_control_plane_retry_after_promoted_timeout(
        experimental_no_ht_transport,
        control_plane_probe_pending,
        err,
    ) {
        Some(if current_clock_hz > SDIO_STARTUP_CLOCK_HZ {
            SDIO_STARTUP_CLOCK_HZ
        } else {
            current_clock_hz
        })
    } else {
        None
    }
}

#[inline]
fn control_plane_retry_after_startup_link_reply_failure_target_clock_hz(
    experimental_no_ht_transport: bool,
    control_plane_probe_pending: bool,
    current_clock_hz: u32,
    err: &DriverError,
) -> Option<u32> {
    if first_control_plane_retry_after_startup_link_reply_failure(
        experimental_no_ht_transport,
        control_plane_probe_pending,
        err,
    ) {
        Some(if current_clock_hz > SDIO_STARTUP_CLOCK_HZ {
            SDIO_STARTUP_CLOCK_HZ
        } else {
            current_clock_hz
        })
    } else {
        None
    }
}

#[inline]
fn control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
    err: &DriverError,
) -> bool {
    match err {
        DriverError::Protocol("ioctl-timeout") => true,
        DriverError::Hal(HalError::Unsupported(reason)) => {
            let reason = *reason;
            startup_link_reply_rescue_reason(reason)
                || reason == "sdio-function2-ready-timeout"
                || reason == "cyw43-control-plane-startup-link-reply-timeout"
        }
        _ => false,
    }
}

#[inline]
fn control_plane_retry_after_reply_wait_resend_target_clock_hz(
    experimental_no_ht_transport: bool,
    effective_clock_hz: u32,
    initial_reason: &DriverError,
) -> u32 {
    if !experimental_no_ht_transport {
        return effective_clock_hz;
    }

    let _ = initial_reason;
    if effective_clock_hz > SDIO_STARTUP_CLOCK_HZ {
        SDIO_STARTUP_CLOCK_HZ
    } else {
        effective_clock_hz
    }
}

#[inline]
fn control_plane_bootstrap_needs_full_replay_retry(err: &DriverError) -> bool {
    matches!(
        err,
        DriverError::Hal(HalError::Unsupported(reason))
            if crate::net::cyw43_control_plane_bootstrap_replay_reason(reason)
    )
}

#[inline]
const fn control_plane_retry_after_promoted_timeout_resend_uses_startup_link(
    experimental_no_ht_transport: bool,
    allow_speculative_retry_credit: bool,
    effective_clock_hz: u32,
) -> bool {
    experimental_no_ht_transport
        && allow_speculative_retry_credit
        && effective_clock_hz <= SDIO_STARTUP_CLOCK_HZ
}

#[inline]
const fn control_plane_retry_after_reply_wait_uses_promoted_link(
    experimental_no_ht_transport: bool,
    effective_clock_hz: u32,
) -> bool {
    experimental_no_ht_transport && effective_clock_hz > SDIO_STARTUP_CLOCK_HZ
}

#[inline]
fn rearm_control_plane_after_reply_wait(
    state: &mut Pi4WifiState,
    effective_clock_hz: u32,
    stage: &'static str,
) -> Result<(), DriverError> {
    if control_plane_retry_after_reply_wait_uses_promoted_link(
        state.cyw43_experimental_no_ht_transport(),
        effective_clock_hz,
    ) {
        state.rearm_cyw43_control_plane_promoted_link(true)?;
        info!(
            "[cyw43] control-plane reply wait rearm policy=promoted-link stage={stage} clock={}Hz chunk_limit={} mode=bounded-no-ht",
            effective_clock_hz,
            state.cyw43_control_plane_chunk_limit(),
        );
    } else {
        state.rearm_cyw43_control_plane_slow_link()?;
        state.resume_cyw43_control_plane_reply_probe_on_startup_link(stage);
        info!(
            "[cyw43] control-plane reply wait rearm policy=startup-link stage={stage} clock={}Hz chunk_limit={} mode=bounded-no-ht",
            effective_clock_hz,
            state.cyw43_control_plane_chunk_limit(),
        );
    }
    Ok(())
}

#[inline]
const fn speculative_credit_window_after_promoted_timeout_retry(
    allow_speculative_retry_credit: bool,
    sdpcm_seq: u8,
    sdpcm_seq_max: u8,
) -> Option<u8> {
    if allow_speculative_retry_credit && !has_sdpcm_credit(sdpcm_seq, sdpcm_seq_max) {
        Some(sdpcm_seq.wrapping_add(1))
    } else {
        None
    }
}

fn recover_startup_transport(
    state: &mut Pi4WifiState,
    init_transport_label: &'static str,
) -> Result<(), HalError> {
    if startup_transport_recovery_should_reset_experimental_state(
        state.cyw43_experimental_no_ht_transport(),
        state.cyw43_control_plane_probe_pending(),
        state.cyw43_control_plane_startup_link_stabilized(),
    ) {
        info!(
            "[cyw43] step: recover_transport(reset-experimental-state no_ht={} probe_pending={} startup_link_stable={})",
            state.cyw43_experimental_no_ht_transport(),
            state.cyw43_control_plane_probe_pending(),
            state.cyw43_control_plane_startup_link_stabilized(),
        );
        state.finish_cyw43_experimental_transport_probe();
    }
    info!("[cyw43] step: recover_transport(assert-reset)");
    state.set_reset(WifiResetState::Asserted)?;
    info!("[cyw43] step: recover_transport(power-off)");
    state.set_power(WifiPowerState::Off)?;
    info!("[cyw43] step: recover_transport(power-on)");
    state.set_power(WifiPowerState::On)?;
    wifi_progress_tick();
    info!("[cyw43] step: recover_transport(assert-reset)");
    state.set_reset(WifiResetState::Asserted)?;
    info!("[cyw43] step: recover_transport(reset_host)");
    state.reset_host()?;
    wifi_progress_tick();
    info!("[cyw43] step: recover_transport(set_clock)");
    state.set_clock_hz(SDIO_STARTUP_CLOCK_HZ)?;
    info!("[cyw43] step: recover_transport(set_bus_width)");
    state.set_bus_width(SdioBusWidth::OneBit)?;
    info!("[cyw43] step: recover_transport(deassert-reset)");
    state.set_reset(WifiResetState::Deasserted)?;
    info!("[cyw43] step: {init_transport_label}");
    state.init_cyw43_transport()?;
    Ok(())
}

#[inline]
const fn startup_transport_recovery_should_reset_experimental_state(
    experimental_no_ht_transport: bool,
    control_plane_probe_pending: bool,
    startup_link_stabilized: bool,
) -> bool {
    experimental_no_ht_transport || control_plane_probe_pending || startup_link_stabilized
}

fn prepare_initial_control_plane_transport(
    state: &mut Pi4WifiState,
) -> Result<(u32, SdioBusWidth), DriverError> {
    info!("[cyw43] step: set_clock(control-plane-bootstrap)");
    let recommended_data_clock_hz = state.recommended_data_clock_hz();
    let experimental_no_ht_transport = state.cyw43_experimental_no_ht_transport();
    let data_clock_target_hz = initial_control_plane_data_clock_target_hz(
        recommended_data_clock_hz,
        experimental_no_ht_transport,
    );
    let data_clock_hz = state.set_clock_hz(data_clock_target_hz)?;
    let transport_mode = if experimental_no_ht_transport {
        "bounded-no-ht"
    } else {
        "strict"
    };
    let first_probe_policy =
        initial_control_plane_bootstrap_policy_label(experimental_no_ht_transport, data_clock_hz);
    info!(
        "[cyw43] control transport ready clock={}Hz target={}Hz recommended={}Hz bus_width=4 mode={transport_mode} first_probe_policy={first_probe_policy} write_chunk_limit={} reply_chunk_limit={}",
        data_clock_hz,
        data_clock_target_hz,
        recommended_data_clock_hz,
        state.cyw43_control_plane_write_chunk_limit(),
        state.cyw43_control_plane_reply_chunk_limit(),
    );
    Ok((data_clock_hz, SdioBusWidth::FourBit))
}

fn log_cyw43_init_failure(
    stage: &'static str,
    state: &mut Pi4WifiState,
    err: &DriverError,
    include_control_plane_snapshot: bool,
) -> Option<&'static str> {
    warn!("[cyw43] init failure stage={stage} err={err}");
    if let Some(reason) = promoted_cyw43_init_failure_exact_error(err) {
        state.promote_cached_control_plane_exact_error(reason);
    }
    if include_control_plane_snapshot {
        state.log_cyw43_control_plane_snapshot(stage);
    }
    let snapshot_exact_error = match state.debug_dump_state(stage) {
        Ok(snapshot) => {
            warn!("[cyw43] init snapshot stage={stage} snapshot={snapshot:?}");
            Some(snapshot.control_plane_exact_error)
        }
        Err(snapshot_err) => {
            warn!("[cyw43] init snapshot unavailable stage={stage} err={snapshot_err}");
            None
        }
    };
    snapshot_exact_error
}

fn promoted_cyw43_init_failure_exact_error(err: &DriverError) -> Option<&'static str> {
    match err {
        DriverError::Hal(HalError::Unsupported(reason)) => Some(*reason),
        DriverError::IoctlFailed { cmd, status }
            if *cmd == Ioctl::SetWsecPmk as u32 && *status == BCME_BADARG =>
        {
            Some("wsec-pmk-bad-argument")
        }
        _ => None,
    }
}

fn preserve_cyw43_init_failure_exact_error(
    retry_err: DriverError,
    snapshot_exact_error: Option<&'static str>,
) -> DriverError {
    match retry_err {
        DriverError::Hal(HalError::Unsupported(reason))
            if matches!(
                reason,
                "cyw43-control-plane-no-reply-linux-f2-armed"
                    | "cyw43-control-plane-pure-f2-startup-link-no-reply"
                    | "cyw43-control-plane-startup-link-rescue-budget-exhausted"
            ) || reason.starts_with("cyw43-function2-reply-") =>
        {
            if reason.starts_with("cyw43-function2-reply-") {
                return DriverError::Hal(HalError::Unsupported(reason));
            }
            if let Some(snapshot_exact_error) = snapshot_exact_error {
                if !snapshot_exact_error.is_empty() && snapshot_exact_error != reason {
                    return DriverError::Hal(HalError::Unsupported(snapshot_exact_error));
                }
            }
            DriverError::Hal(HalError::Unsupported(reason))
        }
        DriverError::Hal(HalError::Unsupported(reason))
            if (reason.starts_with("cyw43-control-plane-sideband-")
                || reason.starts_with("cyw43-function2-enable-latched-not-ready")
                || reason.starts_with("cyw43-function2-enable-latched-not-ready-sideband-"))
                && snapshot_exact_error.is_some_and(|snapshot_exact_error| {
                    !snapshot_exact_error.is_empty()
                        && preserved_cyw43_init_failure_snapshot_is_stronger(snapshot_exact_error)
                }) =>
        {
            DriverError::Hal(HalError::Unsupported(
                snapshot_exact_error.unwrap_or(reason),
            ))
        }
        other => other,
    }
}

#[inline]
fn preserved_cyw43_init_failure_snapshot_is_stronger(snapshot_exact_error: &str) -> bool {
    snapshot_exact_error.starts_with("cyw43-function2-reply-")
        || snapshot_exact_error.starts_with("cyw43-function2-enable-latched-not-ready")
        || snapshot_exact_error.starts_with("cyw43-ht-clock-timeout")
        || snapshot_exact_error.starts_with("sdio-cmd52")
        || snapshot_exact_error.starts_with("sdio-cmd53")
}

pub(crate) fn debug_load_firmware_from_transport(
    state: &mut Pi4WifiState,
) -> Result<u32, HalError> {
    info!("[cyw43] debug: set_bus_width(4bit)");
    state.set_bus_width(SdioBusWidth::FourBit)?;
    info!("[cyw43] debug: prepare_firmware_upload_transport");
    state.prepare_cyw43_firmware_upload_transport()?;
    info!("[cyw43] debug: load_firmware(linux-high-speed)");
    state.load_cyw43_firmware()?;
    info!("[cyw43] debug: set_clock(data)");
    let data_clock_target_hz = SDIO_DATA_CLOCK_HZ.min(state.recommended_data_clock_hz());
    state.set_clock_hz(data_clock_target_hz)
}

pub(crate) fn debug_retry_transport_and_firmware(
    state: &mut Pi4WifiState,
) -> Result<u32, HalError> {
    info!("[cyw43] debug: recover_transport");
    recover_startup_transport(state, "debug-retry(init_transport)")?;
    debug_load_firmware_from_transport(state)
}

#[cfg(not(all(feature = "kernel", target_arch = "aarch64", target_os = "none")))]
#[inline]
fn wifi_progress_begin() {}

#[cfg(not(all(feature = "kernel", target_arch = "aarch64", target_os = "none")))]
#[inline]
fn wifi_progress_tick() {}

#[cfg(not(all(feature = "kernel", target_arch = "aarch64", target_os = "none")))]
#[inline]
fn wifi_progress_finish() {}

struct WifiBootProgressGuard;

impl WifiBootProgressGuard {
    #[inline]
    fn begin() -> Self {
        wifi_progress_begin();
        Self
    }

    #[inline]
    fn tick(&self) {
        wifi_progress_tick();
    }
}

impl Drop for WifiBootProgressGuard {
    fn drop(&mut self) {
        wifi_progress_finish();
    }
}

impl NetDriverError for DriverError {
    fn is_absent(&self) -> bool {
        matches!(self, Self::NoDevice)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeferredJoinState {
    Disabled,
    Pending {
        completion: JoinCompletionState,
        polls: u16,
        secure: bool,
        completion_rule: JoinCompletionRule,
    },
    Failed {
        reason: &'static str,
    },
}

#[inline]
fn deferred_join_is_pending(state: DeferredJoinState) -> bool {
    matches!(state, DeferredJoinState::Pending { .. })
}

#[inline]
fn deferred_join_requires_host_eapol(state: DeferredJoinState) -> bool {
    matches!(
        state,
        DeferredJoinState::Pending {
            completion_rule: JoinCompletionRule::HostEapolRequired,
            ..
        } | DeferredJoinState::Failed {
            reason: "host-eapol-required"
        }
    )
}

#[inline]
fn deferred_join_allows_rx_polling(state: DeferredJoinState) -> bool {
    !matches!(state, DeferredJoinState::Failed { .. })
}

#[inline]
fn deferred_join_bringup_status_label(
    state: DeferredJoinState,
    link_up: bool,
) -> Option<&'static str> {
    match state {
        DeferredJoinState::Disabled if link_up => None,
        DeferredJoinState::Disabled => Some("wifi-link-down"),
        DeferredJoinState::Pending {
            completion_rule: JoinCompletionRule::HostEapolRequired,
            ..
        } => Some("wifi-host-eapol-required"),
        DeferredJoinState::Pending { .. } => Some("wifi-associating"),
        DeferredJoinState::Failed {
            reason: "host-eapol-required",
        } => Some("wifi-host-eapol-required"),
        DeferredJoinState::Failed { .. } => Some("wifi-association-failed"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WsecPmkKind {
    Pbkdf2Passphrase,
    HexPmk,
    LegacyHexPmk,
    HostEapolDeferred,
}

impl WsecPmkKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Pbkdf2Passphrase => "pbkdf2-passphrase",
            Self::HexPmk => "hex-pmk",
            Self::LegacyHexPmk => "legacy-hex-pmk",
            Self::HostEapolDeferred => "host-eapol-deferred",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HostEapolFrameProof {
    eapol_version: u8,
    packet_type: u8,
    body_len: u16,
    body_len_valid: bool,
    descriptor_type: u8,
    key_info: u16,
    key_version: u16,
    key_data_len: u16,
    replay_counter_nonzero: bool,
    message: &'static str,
}

fn host_eapol_frame_proof(packet: &[u8]) -> HostEapolFrameProof {
    let Some(eapol) = packet.get(ETH_HEADER_LEN..) else {
        return host_eapol_malformed_proof("short-ethernet");
    };
    let eapol_version = eapol.first().copied().unwrap_or(0);
    let packet_type = eapol.get(1).copied().unwrap_or(0);
    let body_len = get_u16_be(eapol, 2).unwrap_or(0);
    if eapol.len() < EAPOL_HEADER_LEN {
        return HostEapolFrameProof {
            eapol_version,
            packet_type,
            body_len,
            body_len_valid: false,
            ..host_eapol_malformed_proof("short-eapol")
        };
    }

    let body = &eapol[EAPOL_HEADER_LEN..];
    let body_len_available = usize::from(body_len) <= body.len();
    if packet_type != EAPOL_PACKET_TYPE_KEY {
        return HostEapolFrameProof {
            eapol_version,
            packet_type,
            body_len,
            body_len_valid: body_len_available,
            message: "non-key",
            ..host_eapol_malformed_proof("non-key")
        };
    }
    let key_body_len_valid = body_len_available && usize::from(body_len) >= EAPOL_KEY_MIN_BODY_LEN;
    if !key_body_len_valid || body.len() < EAPOL_KEY_MIN_BODY_LEN {
        return HostEapolFrameProof {
            eapol_version,
            packet_type,
            body_len,
            body_len_valid: key_body_len_valid,
            message: "short-key",
            ..host_eapol_malformed_proof("short-key")
        };
    }

    let descriptor_type = body[0];
    let key_info = get_u16_be(body, 1).unwrap_or(0);
    let key_data_len = get_u16_be(body, 93).unwrap_or(0);
    let replay_counter_nonzero = body[5..13].iter().any(|byte| *byte != 0);
    HostEapolFrameProof {
        eapol_version,
        packet_type,
        body_len,
        body_len_valid: key_body_len_valid,
        descriptor_type,
        key_info,
        key_version: key_info & EAPOL_KEY_INFO_KEY_VERSION_MASK,
        key_data_len,
        replay_counter_nonzero,
        message: classify_eapol_key_message(key_info),
    }
}

const fn host_eapol_malformed_proof(message: &'static str) -> HostEapolFrameProof {
    HostEapolFrameProof {
        eapol_version: 0,
        packet_type: 0,
        body_len: 0,
        body_len_valid: false,
        descriptor_type: 0,
        key_info: 0,
        key_version: 0,
        key_data_len: 0,
        replay_counter_nonzero: false,
        message,
    }
}

const fn classify_eapol_key_message(key_info: u16) -> &'static str {
    let pairwise = key_info & EAPOL_KEY_INFO_KEY_TYPE != 0;
    let ack = key_info & EAPOL_KEY_INFO_ACK != 0;
    let mic = key_info & EAPOL_KEY_INFO_MIC != 0;
    let install = key_info & EAPOL_KEY_INFO_INSTALL != 0;
    let secure = key_info & EAPOL_KEY_INFO_SECURE != 0;
    let encrypted = key_info & EAPOL_KEY_INFO_ENCRYPTED_KEY_DATA != 0;
    if pairwise && ack && !mic {
        "m1"
    } else if pairwise && !ack && mic && !secure {
        "m2"
    } else if pairwise && ack && mic && install && encrypted {
        "m3"
    } else if pairwise && !ack && mic && secure {
        "m4"
    } else if !pairwise && mic {
        "group-key"
    } else {
        "unknown"
    }
}

pub struct Cyw43NetDevice {
    state: Box<Pi4WifiState>,
    probe: ProbeReport,
    mac: EthernetAddress,
    tx_drops: u32,
    rx_packets: u64,
    tx_packets: u64,
    sdpcm_seq: u8,
    sdpcm_seq_max: u8,
    ioctl_id: u16,
    control_tx_ext_header: bool,
    link_up: bool,
    deferred_join_state: DeferredJoinState,
    host_eapol_rx_packets: u32,
    rx_frame: Box<[u8; FRAME_BUF_LEN]>,
    tx_frame: Box<[u8; FRAME_BUF_LEN]>,
    control_response: Box<[u8; CONTROL_RESPONSE_BUF_LEN]>,
}

pub struct RxToken {
    frame: HeaplessVec<u8, MAX_FRAME_LEN>,
}

pub struct TxToken<'a> {
    device: &'a mut Cyw43NetDevice,
}

impl Cyw43NetDevice {
    pub fn new<H>(hal: &mut H, config: &ConsoleNetConfig) -> Result<Self, DriverError>
    where
        H: Cyw43Hal<Error = HalError>,
    {
        let credentials = config
            .wifi_credentials
            .ok_or(DriverError::Config("wifi-credentials-missing"))?;
        let defer_join_completion = wifi_boot_join_should_defer(config.policy.interface);
        let retry_credentials = WifiCredentials {
            ssid_len: credentials.ssid_len,
            ssid: credentials.ssid,
            psk_len: credentials.psk_len,
            psk: credentials.psk,
        };

        info!(
            "[cyw43] init: begin ssid_len={} psk_len={}",
            credentials.ssid_len, credentials.psk_len,
        );
        let progress = WifiBootProgressGuard::begin();
        let mut state = Pi4WifiState::new(hal)?;
        progress.tick();
        let firmware = state.firmware_bundle();
        firmware.validate().map_err(DriverError::InvalidFirmware)?;

        info!("[cyw43] step: set_power(on)");
        state.set_power(WifiPowerState::On)?;
        progress.tick();
        info!("[cyw43] step: set_reset(asserted)");
        state.set_reset(WifiResetState::Asserted)?;
        info!("[cyw43] step: reset_host");
        state.reset_host()?;
        progress.tick();
        info!("[cyw43] step: set_clock(startup)");
        let effective_clock_hz = state.set_clock_hz(SDIO_STARTUP_CLOCK_HZ)?;
        info!("[cyw43] step: set_bus_width(1bit)");
        state.set_bus_width(SdioBusWidth::OneBit)?;
        info!("[cyw43] step: set_reset(deasserted)");
        state.set_reset(WifiResetState::Deasserted)?;
        progress.tick();
        info!(
            "[cyw43] init: power/reset/clock ready startup_clock={}Hz",
            effective_clock_hz
        );

        info!(
            "[cyw43] attach: fw={} nvram={} clm={} board={} clock={}Hz",
            firmware.firmware.len(),
            firmware.nvram.len(),
            firmware.clm_blob.map_or(0, <[u8]>::len),
            firmware.board_type,
            effective_clock_hz,
        );

        info!("[cyw43] step: init_transport");
        if let Err(err) = state.init_cyw43_transport() {
            if !is_transport_retryable(&err) {
                let driver_err = DriverError::from(err);
                log_cyw43_init_failure("cyw43-init-transport-fail", &mut state, &driver_err, false);
                return Err(driver_err);
            }
            warn!("[cyw43] init_transport retryable failure: {err}");
            if let Err(retry_err) = recover_startup_transport(&mut state, "init_transport(retry)") {
                let driver_err = DriverError::from(retry_err);
                log_cyw43_init_failure(
                    "cyw43-init-transport-retry-fail",
                    &mut state,
                    &driver_err,
                    false,
                );
                return Err(driver_err);
            }
        }
        progress.tick();
        info!("[cyw43] step: set_bus_width(4bit)");
        if let Err(err) = state.set_bus_width(SdioBusWidth::FourBit) {
            let driver_err = DriverError::from(err);
            log_cyw43_init_failure(
                "cyw43-set-bus-width-4bit-fail",
                &mut state,
                &driver_err,
                false,
            );
            return Err(driver_err);
        }
        progress.tick();
        info!("[cyw43] step: prepare_firmware_upload_transport");
        if let Err(err) = state.prepare_cyw43_firmware_upload_transport() {
            let driver_err = DriverError::from(err);
            log_cyw43_init_failure(
                "cyw43-prepare-firmware-upload-transport-fail",
                &mut state,
                &driver_err,
                false,
            );
            return Err(driver_err);
        }
        progress.tick();
        info!("[cyw43] step: load_firmware(linux-high-speed)");
        // Match the Linux Pi 4 lane before firmware attach: Function 1 is
        // selected, the bus is already 4-bit, and the SDIO data clock is raised
        // before the first CYW43 core-control writes.
        if let Err(err) = state.load_cyw43_firmware() {
            let driver_err = DriverError::from(err);
            log_cyw43_init_failure("cyw43-load-firmware-fail", &mut state, &driver_err, false);
            return Err(driver_err);
        }
        progress.tick();
        let (data_clock_hz, bus_width) = match prepare_initial_control_plane_transport(&mut state) {
            Ok(result) => result,
            Err(err) => {
                log_cyw43_init_failure(
                    "cyw43-control-transport-prepare-fail",
                    &mut state,
                    &err,
                    false,
                );
                return Err(err);
            }
        };
        progress.tick();
        info!("[cyw43] step: read_ioex");
        let ioex = match state.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IOEX) {
            Ok(value) => value,
            Err(err) => {
                let driver_err = DriverError::from(err);
                log_cyw43_init_failure("cyw43-read-ioex-fail", &mut state, &driver_err, false);
                return Err(driver_err);
            }
        };
        progress.tick();

        let mut device = Self {
            state: Box::new(state),
            probe: ProbeReport {
                effective_clock_hz: data_clock_hz,
                ioex,
                bus_width,
                firmware: FirmwareLayout::from_bundle(firmware),
            },
            mac: EthernetAddress(DEFAULT_WIFI_MAC),
            tx_drops: 0,
            rx_packets: 0,
            tx_packets: 0,
            sdpcm_seq: 0,
            sdpcm_seq_max: 1,
            ioctl_id: 0,
            control_tx_ext_header: false,
            link_up: false,
            deferred_join_state: DeferredJoinState::Disabled,
            host_eapol_rx_packets: 0,
            rx_frame: Box::new([0; FRAME_BUF_LEN]),
            tx_frame: Box::new([0; FRAME_BUF_LEN]),
            control_response: Box::new([0; CONTROL_RESPONSE_BUF_LEN]),
        };

        info!("[cyw43] step: init_control_plane");
        if let Err(err) = device.init_control_plane(firmware, credentials, !defer_join_completion) {
            if control_plane_bootstrap_needs_full_replay_retry(&err) {
                warn!(
                    "[cyw43] init_control_plane hard-retry armed reason={err} action=replay-firmware-control-bootstrap"
                );
                if let Err(retry_err) =
                    device.replay_control_plane_bootstrap(firmware, retry_credentials, &err)
                {
                    let snapshot_exact_error = log_cyw43_init_failure(
                        "cyw43-init-control-plane-hard-retry-fail",
                        device.state.as_mut(),
                        &retry_err,
                        true,
                    );
                    return Err(preserve_cyw43_init_failure_exact_error(
                        retry_err,
                        snapshot_exact_error,
                    ));
                }
                info!(
                    "[cyw43] init_control_plane hard-retry recovered action=replay-firmware-control-bootstrap"
                );
            } else {
                let snapshot_exact_error = log_cyw43_init_failure(
                    "cyw43-init-control-plane-fail",
                    device.state.as_mut(),
                    &err,
                    true,
                );
                return Err(preserve_cyw43_init_failure_exact_error(
                    err,
                    snapshot_exact_error,
                ));
            }
        }
        if defer_join_completion {
            info!("[cyw43] join completion deferred action=event-pump-association");
        }
        if device.state.cyw43_experimental_no_ht_transport() {
            info!("[cyw43] step: promote_control_transport");
            device.state.finish_cyw43_experimental_transport_probe();
            let promoted_clock_hz =
                device
                    .state
                    .set_clock_hz(control_plane_data_clock_target_hz(
                        device.state.recommended_data_clock_hz(),
                    ))?;
            device.probe.effective_clock_hz = promoted_clock_hz;
            info!(
                "[cyw43] control transport promoted clock={}Hz bus_width=4 mode=strict",
                promoted_clock_hz
            );
        }
        info!(
            "[cyw43] ready: mac={} clock={}Hz bus_width={} ioex=0x{:02x}",
            device.mac,
            device.probe.effective_clock_hz,
            match device.probe.bus_width {
                SdioBusWidth::OneBit => "1",
                SdioBusWidth::FourBit => "4",
            },
            device.probe.ioex,
        );
        Ok(device)
    }

    #[must_use]
    pub const fn probe_report(&self) -> ProbeReport {
        self.probe
    }

    fn reset_control_plane_bootstrap_state(&mut self) {
        self.sdpcm_seq = 0;
        self.sdpcm_seq_max = 1;
        self.ioctl_id = 0;
        self.control_tx_ext_header = false;
        self.link_up = false;
        self.deferred_join_state = DeferredJoinState::Disabled;
        self.host_eapol_rx_packets = 0;
        self.rx_frame.fill(0);
        self.tx_frame.fill(0);
        self.control_response.fill(0);
    }

    fn replay_control_plane_bootstrap(
        &mut self,
        firmware: WifiFirmwareBundle<'static>,
        credentials: WifiCredentials,
        reason: &DriverError,
    ) -> Result<(), DriverError> {
        warn!(
            "[cyw43] control-plane bootstrap hard-retry action=replay-firmware-control-bootstrap reason={reason} current_clock={}Hz write_chunk_limit={} reply_chunk_limit={} mode={}",
            self.probe.effective_clock_hz,
            self.state.cyw43_control_plane_chunk_limit(),
            self.state.cyw43_control_plane_reply_chunk_limit(),
            initial_control_plane_bootstrap_policy_label(
                self.state.cyw43_experimental_no_ht_transport(),
                self.probe.effective_clock_hz,
            ),
        );

        info!("[cyw43] step: control_plane_hard_retry(recover_transport)");
        recover_startup_transport(
            self.state.as_mut(),
            "init_transport(control-plane-hard-retry)",
        )?;
        info!("[cyw43] step: control_plane_hard_retry(set_bus_width=4bit)");
        self.state.set_bus_width(SdioBusWidth::FourBit)?;
        info!("[cyw43] step: control_plane_hard_retry(prepare_firmware_upload_transport)");
        self.state.prepare_cyw43_firmware_upload_transport()?;
        info!("[cyw43] step: control_plane_hard_retry(load_firmware)");
        self.state.load_cyw43_firmware()?;
        let (data_clock_hz, bus_width) =
            prepare_initial_control_plane_transport(self.state.as_mut())?;
        self.probe.effective_clock_hz = data_clock_hz;
        self.probe.bus_width = bus_width;
        self.probe.ioex = self
            .state
            .io_direct_read(SdioFunction::Function0, SDIO_CCCR_IOEX)?;
        self.reset_control_plane_bootstrap_state();
        info!(
            "[cyw43] control-plane bootstrap hard-retry ready clock={}Hz bus_width={} ioex=0x{:02x}",
            self.probe.effective_clock_hz,
            match self.probe.bus_width {
                SdioBusWidth::OneBit => "1",
                SdioBusWidth::FourBit => "4",
            },
            self.probe.ioex,
        );
        self.init_control_plane(firmware, credentials, true)
    }

    fn init_control_plane(
        &mut self,
        firmware: WifiFirmwareBundle<'static>,
        credentials: WifiCredentials,
        wait_for_join_completion: bool,
    ) -> Result<(), DriverError> {
        macro_rules! control_step {
            ($name:literal, $expr:expr) => {{
                info!("[cyw43] control-plane step={} action=begin", $name);
                match $expr {
                    Ok(value) => {
                        info!("[cyw43] control-plane step={} action=ready", $name);
                        value
                    }
                    Err(err) => {
                        warn!("[cyw43] control-plane step={} action=fail err={err}", $name);
                        return Err(err);
                    }
                }
            }};
        }
        macro_rules! optional_iovar_step {
            ($name:literal, $iovar:literal, $expr:expr) => {{
                info!(
                    "[cyw43] control-plane step={} action=begin optional=yes",
                    $name
                );
                match $expr {
                    Ok(value) => {
                        info!(
                            "[cyw43] control-plane step={} action=ready optional=yes",
                            $name
                        );
                        value
                    }
                    Err(err) if optional_control_plane_iovar_allows_failure($iovar, &err) => {
                        warn!(
                            "[cyw43] control-plane step={} action=skip optional=yes err={err}",
                            $name
                        );
                    }
                    Err(err) => {
                        warn!("[cyw43] control-plane step={} action=fail err={err}", $name);
                        return Err(err);
                    }
                }
            }};
        }

        let linux_probe_order = linux_attach_control_plane_probe_order();
        info!(
            "[cyw43] control-plane linux-first-iovar-order={}>{}>{}>{}>{}",
            linux_probe_order[0],
            linux_probe_order[1],
            linux_probe_order[2],
            linux_probe_order[3],
            linux_probe_order[4]
        );
        control_step!(
            "linux-startup-status-drain",
            self.drain_linux_startup_status_frames()
        );
        control_step!(
            "linux-first-iovar-txglomalign",
            self.set_iovar_u32("bus:txglomalign", 8)
        );
        control_step!(
            "linux-first-iovar-ulp-sdioctrl",
            self.get_optional_linux_iovar("ulp_sdioctrl", &mut [0u8; 16])
        );
        control_step!(
            "linux-first-iovar-rxglom",
            self.set_iovar_u32("bus:rxglom", 1)
        );
        self.enable_control_tx_extension_header("linux-first-iovar-rxglom");
        info!("[cyw43] control-plane step=read-mac action=begin order=linux-before-clm");
        self.mac = match self.read_mac_address() {
            Ok(mac) => mac,
            Err(err) => {
                warn!("[cyw43] control-plane step=read-mac action=fail err={err}");
                return Err(err);
            }
        };
        info!(
            "[cyw43] control-plane step=read-mac action=ready order=linux-before-clm mac={}",
            self.mac
        );
        control_step!("revinfo", self.read_revinfo());

        if let Some(clm) = firmware.clm_blob {
            control_step!("clm-download", self.load_clm(clm));
            control_step!("firmware-version", self.read_firmware_version());
            control_step!("clm-version", self.read_clm_version());
        } else {
            info!("[cyw43] control-plane step=clm-download action=skip");
        }

        if linux_station_path_keeps_txglom_configured_before_preinit() {
            info!(
                "[cyw43] control-plane step=bus-txglom-disable action=skip optional=yes reason=linux-keeps-sdio-preinit-glom-before-mpc"
            );
        } else {
            optional_iovar_step!(
                "bus-txglom-disable",
                "bus:txglom",
                self.set_iovar_u32("bus:txglom", 0)
            );
        }
        if linux_station_path_keeps_rxglom_configured_before_preinit() {
            info!(
                "[cyw43] control-plane step=bus-rxglom-disable-bounded-rx action=skip reason=linux-keeps-sdio-preinit-rxglom-before-mpc"
            );
        } else {
            control_step!(
                "bus-rxglom-disable-bounded-rx",
                self.set_iovar_u32("bus:rxglom", 0)
            );
        }
        if linux_station_path_enables_apsta() {
            optional_iovar_step!("apsta-enable", "apsta", self.set_iovar_u32("apsta", 1));
        } else {
            info!(
                "[cyw43] control-plane step=apsta-enable action=skip optional=yes reason=linux-station-path-does-not-enable-apsta"
            );
        }
        let country_reason = if linux_station_path_sets_country() {
            "linux-station-path-country-set-disabled"
        } else {
            "linux-station-path-queries-country-later"
        };
        info!(
            "[cyw43] control-plane step=country-worldwide action=skip optional=yes reason={country_reason}"
        );
        control_step!(
            "linux-preinit-defaults",
            self.apply_linux_preinit_defaults()
        );
        if linux_station_path_sets_antdiv_before_join() {
            control_step!(
                "antenna-diversity",
                self.ioctl_set_u32(Ioctl::SetAntdiv, 0, 0)
            );
        } else {
            info!(
                "[cyw43] control-plane step=antenna-diversity action=skip optional=yes reason=linux-station-path-does-not-set-antdiv-before-join"
            );
        }
        if linux_station_path_sets_ampdu_limits_before_join() {
            optional_iovar_step!(
                "ampdu-ba-window",
                "ampdu_ba_wsize",
                self.set_iovar_u32("ampdu_ba_wsize", 8)
            );
            optional_iovar_step!(
                "ampdu-mpdu",
                "ampdu_mpdu",
                self.set_iovar_u32("ampdu_mpdu", 4)
            );
        } else {
            info!(
                "[cyw43] control-plane step=ampdu-limits action=skip optional=yes reason=linux-station-path-does-not-set-ampdu-limits-before-join"
            );
        }
        control_step!("event-mask", self.enable_join_event_messages());
        control_step!("up", self.ioctl_raw(IoctlType::Set, Ioctl::Up, 0, &[]));
        control_step!("post-up-event-drain", self.drain_post_up_events());
        if linux_station_path_sets_legacy_gmode() {
            control_step!("gmode", self.ioctl_set_u32(Ioctl::SetGmode, 0, 1));
        } else {
            info!(
                "[cyw43] control-plane step=gmode action=skip optional=yes reason=linux-station-path-does-not-set-legacy-gmode"
            );
        }
        if linux_station_path_sets_legacy_band() {
            control_step!("band", self.ioctl_set_u32(Ioctl::SetBand, 0, 0));
        } else {
            info!(
                "[cyw43] control-plane step=band action=skip optional=yes reason=linux-station-path-does-not-set-legacy-band"
            );
        }
        if linux_station_path_sets_power_mode_before_join() {
            control_step!("power-mode", self.ioctl_set_u32(Ioctl::SetPm, 0, 0));
        } else {
            info!(
                "[cyw43] control-plane step=power-mode action=skip optional=yes reason=linux-station-path-does-not-set-power-mode-before-join"
            );
        }
        control_step!("join", self.join(credentials, wait_for_join_completion));
        info!("[cyw43] control-plane step=init-complete action=ready");
        Ok(())
    }

    fn load_clm(&mut self, clm: &[u8]) -> Result<(), DriverError> {
        let mut offset = 0usize;
        while offset < clm.len() {
            let chunk_len = core::cmp::min(clm.len() - offset, CLM_CHUNK_SIZE);
            let mut flags = DOWNLOAD_FLAG_HANDLER_VER;
            if offset == 0 {
                flags |= DOWNLOAD_FLAG_BEGIN;
            }
            if offset + chunk_len == clm.len() {
                flags |= DOWNLOAD_FLAG_END;
            }
            let payload_len = clm_setvar_payload_len(chunk_len);
            {
                let payload = self.payload_mut(payload_len)?;
                payload[..8].copy_from_slice(b"clmload\0");
                put_u16_le(payload, 8, flags);
                put_u16_le(payload, 10, DOWNLOAD_TYPE_CLM);
                put_u32_le(
                    payload,
                    12,
                    u32::try_from(chunk_len).map_err(|_| DriverError::FrameTooLarge)?,
                );
                put_u32_le(payload, 16, 0);
                payload[20..20 + chunk_len].copy_from_slice(&clm[offset..offset + chunk_len]);
            }
            let _ = self.ioctl_encoded(IoctlType::Set, Ioctl::SetVar, 0, payload_len)?;
            offset += chunk_len;
        }
        info!("[cyw43] clm loaded bytes={}", clm.len());
        Ok(())
    }

    fn read_firmware_version(&mut self) -> Result<(), DriverError> {
        let mut version = [0u8; 256];
        let response_len = self.get_iovar("ver", &mut version)?;
        if response_len == 0 {
            return Err(DriverError::Protocol("ver-empty"));
        }
        let printable_len = version[..response_len]
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(response_len);
        if printable_len == 0 {
            return Err(DriverError::Protocol("ver-empty"));
        }
        info!("[cyw43] firmware version bytes={}", printable_len);
        Ok(())
    }

    fn read_clm_version(&mut self) -> Result<(), DriverError> {
        let mut version = [0u8; 256];
        let response_len = self.get_iovar("clmver", &mut version)?;
        if response_len == 0 {
            return Err(DriverError::Protocol("clmver-empty"));
        }
        let printable_len = version[..response_len]
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(response_len);
        if printable_len == 0 {
            return Err(DriverError::Protocol("clmver-empty"));
        }
        info!("[cyw43] clm version bytes={}", printable_len);
        Ok(())
    }

    fn apply_linux_preinit_defaults(&mut self) -> Result<(), DriverError> {
        macro_rules! preinit_step {
            ($name:literal, $expr:expr) => {{
                info!("[cyw43] control-plane preinit step={} action=begin", $name);
                match $expr {
                    Ok(value) => {
                        info!("[cyw43] control-plane preinit step={} action=ready", $name);
                        value
                    }
                    Err(err) => {
                        warn!(
                            "[cyw43] control-plane preinit step={} action=fail err={err}",
                            $name
                        );
                        return Err(err);
                    }
                }
            }};
        }

        preinit_step!("mpc", self.set_iovar_u32("mpc", 1));
        preinit_step!(
            "join-pref",
            self.set_iovar_bytes("join_pref", &LINUX_JOIN_PREF_DEFAULT)
        );
        preinit_step!("if-event-message", self.enable_linux_if_event_message());
        preinit_step!(
            "scan-channel-time",
            self.ioctl_set_u32(Ioctl::SetScanChannelTime, 0, DEFAULT_SCAN_CHANNEL_TIME_MS)
        );
        preinit_step!(
            "scan-unassoc-time",
            self.ioctl_set_u32(Ioctl::SetScanUnassocTime, 0, DEFAULT_SCAN_UNASSOC_TIME_MS)
        );
        info!("[cyw43] control-plane preinit step=txbf action=begin optional=yes");
        match self.set_iovar_u32("txbf", 1) {
            Ok(()) => info!("[cyw43] control-plane preinit step=txbf action=ready optional=yes"),
            Err(err) if optional_txbf_allows_failure(&err) => warn!(
                "[cyw43] control-plane preinit step=txbf action=skip optional=yes reason=unsupported err={err}"
            ),
            Err(err) => {
                warn!(
                    "[cyw43] control-plane preinit step=txbf action=fail optional=yes err={err}"
                );
                return Err(err);
            }
        }
        Ok(())
    }

    fn enable_linux_if_event_message(&mut self) -> Result<(), DriverError> {
        let mut mask = [0u8; EVENT_MASK_LEN];
        let response_len = self.get_iovar("event_msgs", &mut mask)?;
        set_event_mask_bit(&mut mask, EVENT_IF)?;
        let required_len = usize::from(EVENT_IF / 8).saturating_add(1);
        self.set_iovar_bytes(
            "event_msgs",
            &mask[..core::cmp::max(response_len, required_len)],
        )
    }

    fn enable_join_event_messages(&mut self) -> Result<(), DriverError> {
        let mask = linux_join_event_mask()?;
        match self.set_event_msgs_ext_mask(&mask) {
            Ok(()) => {
                info!("[cyw43] event-mask path=event_msgs_ext action=ready len={EVENT_MASK_LEN}");
                Ok(())
            }
            Err(err) if ioctl_failed_status(&err) == Some(BCME_UNSUPPORTED) => {
                warn!(
                    "[cyw43] event-mask path=event_msgs_ext unsupported action=global-event_msgs"
                );
                self.set_global_event_mask(&mask)?;
                info!("[cyw43] event-mask path=event_msgs action=ready len={EVENT_MASK_LEN}");
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn set_event_msgs_ext_mask(&mut self, mask: &[u8; EVENT_MASK_LEN]) -> Result<(), DriverError> {
        let payload_len = event_msgs_ext_payload_len();
        {
            let payload = self.payload_mut(payload_len)?;
            write_event_msgs_ext_payload(payload, mask)?;
        }
        self.set_iovar_from_payload("event_msgs_ext", payload_len)
    }

    fn set_global_event_mask(
        &mut self,
        required_mask: &[u8; EVENT_MASK_LEN],
    ) -> Result<(), DriverError> {
        let mut mask = [0u8; EVENT_MASK_LEN];
        let response_len = self.get_iovar("event_msgs", &mut mask)?;
        for (slot, required) in mask.iter_mut().zip(required_mask.iter()) {
            *slot |= *required;
        }
        self.set_iovar_bytes(
            "event_msgs",
            &mask[..core::cmp::max(response_len, EVENT_MASK_LEN)],
        )
    }

    fn join(
        &mut self,
        credentials: WifiCredentials,
        wait_for_completion: bool,
    ) -> Result<(), DriverError> {
        let ssid = credentials.ssid().map_err(DriverError::Config)?;
        let psk = credentials.psk().map_err(DriverError::Config)?;
        let secure = !psk.is_empty();
        self.deferred_join_state = DeferredJoinState::Disabled;
        if !secure {
            self.set_primary_bsscfg_u32("auth", AUTH_OPEN)?;
            self.set_primary_bsscfg_u32("wsec", 0)?;
            self.set_primary_bsscfg_u32("wpa_auth", WPA_AUTH_DISABLED)?;
            info!(
                "[cyw43] join: auth=open ssid_len={} control=linux-primary-bsscfg order=auth-wsec-wpa_auth",
                ssid.len()
            );
            self.program_join_request(ssid)?;
        } else {
            self.set_iovar_bytes("wpaie", &WPA2_PSK_CCMP_RSN_IE)?;
            self.set_join_security_wpa_auth(WPA_AUTH_WPA2_PSK_OR_UNSPECIFIED)?;
            self.set_primary_bsscfg_u32("auth", AUTH_OPEN)?;
            self.set_primary_bsscfg_u32("wsec", WSEC_AES)?;
            if linux_wpa2_join_sets_mfp_without_rsn_ie() {
                self.set_optional_iovar_u32("mfp", MFP_CAPABLE)?;
            }
            self.set_join_security_wpa_auth(WPA_AUTH_WPA2_PSK)?;
            let firmware_supplicant_path = self.enable_wpa2_firmware_supplicant()?;
            let completion_rule = firmware_supplicant_path.completion_rule();
            let (pmk_kind, pmk_action) = if matches!(
                firmware_supplicant_path,
                FirmwareSupplicantPath::Unsupported
            ) {
                info!(
                    "[cyw43] join: pmk action=skip reason=firmware-supplicant-unsupported completion_rule=host-eapol-required"
                );
                (WsecPmkKind::HostEapolDeferred, "skip-host-eapol")
            } else {
                (
                    self.set_wsec_pmk(ssid.as_bytes(), psk.as_bytes())?,
                    "set-wsec-pmk",
                )
            };
            info!(
                "[cyw43] join: auth=wpa2-psk ssid_len={} psk_len={} pmk_kind={} pmk_action={} control=linux-primary-bsscfg fwsup_path={} completion_rule={} order=wpaie-wpa_auth-initial-auth-wsec-wpa_auth-final-{}-{} rsn_ie_len={} initial_wpa_auth=0x{WPA_AUTH_WPA2_PSK_OR_UNSPECIFIED:04x} final_wpa_auth=0x{WPA_AUTH_WPA2_PSK:04x}",
                ssid.len(),
                psk.len(),
                pmk_kind.label(),
                pmk_action,
                firmware_supplicant_path.label(),
                completion_rule.label(),
                firmware_supplicant_path.order_label(),
                pmk_action,
                WPA2_PSK_CCMP_RSN_IE.len(),
            );
            self.program_join_request(ssid)?;
            if matches!(completion_rule, JoinCompletionRule::HostEapolRequired) {
                self.arm_host_eapol_proof_window("join-submit", ssid.len(), psk.len());
                self.service_host_eapol_proof_window("join-submit");
                self.mark_host_eapol_required("join-submit", ssid.len(), psk.len());
                return if wait_for_completion {
                    Err(DriverError::Protocol("host-eapol-required"))
                } else {
                    Ok(())
                };
            }
            if wait_for_completion {
                return self.wait_for_join(secure, completion_rule);
            }
            self.link_up = false;
            self.deferred_join_state = DeferredJoinState::Pending {
                completion: JoinCompletionState::default(),
                polls: 0,
                secure,
                completion_rule,
            };
            info!(
                "[cyw43] join pending mode=deferred polls=0 ssid_len={} psk_len={} secure=yes fwsup={} completion_rule={}",
                ssid.len(),
                psk.len(),
                if completion_rule.firmware_supplicant_required() { "yes" } else { "no" },
                completion_rule.label(),
            );
            return Ok(());
        }

        if wait_for_completion {
            self.wait_for_join(secure, JoinCompletionRule::SetSsid)
        } else {
            self.link_up = false;
            self.deferred_join_state = DeferredJoinState::Pending {
                completion: JoinCompletionState::default(),
                polls: 0,
                secure,
                completion_rule: JoinCompletionRule::SetSsid,
            };
            info!(
                "[cyw43] join pending mode=deferred polls=0 ssid_len={} psk_len={} secure={}",
                ssid.len(),
                psk.len(),
                if secure { "yes" } else { "no" },
            );
            Ok(())
        }
    }

    fn arm_host_eapol_proof_window(&mut self, mode: &'static str, ssid_len: usize, psk_len: usize) {
        self.link_up = false;
        self.deferred_join_state = DeferredJoinState::Pending {
            completion: JoinCompletionState::default(),
            polls: 0,
            secure: true,
            completion_rule: JoinCompletionRule::HostEapolRequired,
        };
        self.state
            .promote_cached_control_plane_exact_error("host-eapol-required");
        info!(
            "[cyw43] host-eapol proof window armed mode={mode} polls={} rx_poll=eapol-only dhcp=blocked tx=blocked ssid_len={ssid_len} psk_len={psk_len}",
            HOST_EAPOL_JOIN_SUBMIT_PROOF_POLLS,
        );
    }

    fn service_host_eapol_proof_window(&mut self, mode: &'static str) {
        let start_eapol = self.host_eapol_rx_packets;
        let mut event_frames = 0usize;
        let mut control_frames = 0usize;
        let mut empty_polls = 0usize;
        let mut error: Option<DriverError> = None;

        for _ in 0..HOST_EAPOL_JOIN_SUBMIT_PROOF_POLLS {
            match self.process_next_frame(false) {
                Ok(RxFrameResult::Event(_)) => event_frames = event_frames.saturating_add(1),
                Ok(RxFrameResult::Control { .. }) => {
                    control_frames = control_frames.saturating_add(1)
                }
                Ok(RxFrameResult::Data(_)) => {}
                Ok(RxFrameResult::None) => {
                    empty_polls = empty_polls.saturating_add(1);
                    spin_loop();
                }
                Err(err) => {
                    error = Some(err);
                    break;
                }
            }
            if self.host_eapol_rx_packets != start_eapol {
                break;
            }
        }

        let eapol_delta = self.host_eapol_rx_packets.saturating_sub(start_eapol);
        match error {
            Some(err) => warn!(
                "[cyw43] host-eapol proof window result=error mode={mode} eapol_rx_delta={eapol_delta} eapol_rx_total={} events={event_frames} control={control_frames} empty_polls={empty_polls} err={err} action=terminal-host-eapol-required",
                self.host_eapol_rx_packets,
            ),
            None => info!(
                "[cyw43] host-eapol proof window result={} mode={mode} eapol_rx_delta={eapol_delta} eapol_rx_total={} events={event_frames} control={control_frames} empty_polls={empty_polls} action=terminal-host-eapol-required",
                if eapol_delta == 0 { "not-yet-seen" } else { "eapol-seen" },
                self.host_eapol_rx_packets,
            ),
        }
    }

    fn mark_host_eapol_required(&mut self, mode: &'static str, ssid_len: usize, psk_len: usize) {
        self.link_up = false;
        self.deferred_join_state = DeferredJoinState::Failed {
            reason: "host-eapol-required",
        };
        self.state
            .promote_cached_control_plane_exact_error("host-eapol-required");
        warn!(
            "[cyw43] join failed reason=host-eapol-required rx_poll=disabled dhcp=blocked tx=blocked mode={mode} ssid_len={ssid_len} psk_len={psk_len} eapol_rx={}",
            self.host_eapol_rx_packets,
        );
    }

    fn program_join_request(&mut self, ssid: &str) -> Result<(), DriverError> {
        match self.program_bsscfg_join_request(ssid) {
            Ok(()) => {
                info!(
                    "[cyw43] join request path=primary-bsscfg:join action=ready ssid_len={}",
                    ssid.len()
                );
                return Ok(());
            }
            Err(err) => {
                if !join_iovar_fallback_allows_set_ssid(&err) {
                    warn!(
                        "[cyw43] join request path=primary-bsscfg:join action=fail-no-fallback err={err}"
                    );
                    return Err(err);
                }
                warn!(
                    "[cyw43] join request path=primary-bsscfg:join action=fallback-set-ssid err={err}"
                );
            }
        }
        self.program_set_ssid_request(ssid)
    }

    fn program_bsscfg_join_request(&mut self, ssid: &str) -> Result<(), DriverError> {
        {
            let payload = self.payload_mut(LINUX_BSSCFG_JOIN_PAYLOAD_LEN)?;
            write_linux_bsscfg_join_payload(payload, ssid)?;
        }
        self.set_iovar_from_payload("join", LINUX_BSSCFG_JOIN_PAYLOAD_LEN)
    }

    fn program_set_ssid_request(&mut self, ssid: &str) -> Result<(), DriverError> {
        let payload_len = legacy_set_ssid_payload_len(ssid)?;
        {
            let payload = self.payload_mut(payload_len)?;
            write_legacy_set_ssid_payload(payload, ssid)?;
        }
        let _ = self.ioctl_encoded(IoctlType::Set, Ioctl::SetSsid, 0, payload_len)?;
        info!(
            "[cyw43] join request path=set-ssid action=ready ssid_len={}",
            ssid.len()
        );
        Ok(())
    }

    fn enable_wpa2_firmware_supplicant(&mut self) -> Result<FirmwareSupplicantPath, DriverError> {
        match self.set_iovar_u32("sup_wpa", 1) {
            Ok(()) => {
                self.set_optional_iovar_u32("sup_wpa2_eapver", u32::MAX)?;
                self.set_optional_iovar_u32("sup_wpa_tmo", 2500)?;
                info!(
                    "[cyw43] join: firmware-supplicant path=primary-plain requested=yes enabled=yes"
                );
                Ok(FirmwareSupplicantPath::PrimaryPlain)
            }
            Err(err)
                if ioctl_failed_status(&err) == Some(BCME_UNSUPPORTED)
                    && firmware_supplicant_wrapper_fallback_allowed() =>
            {
                warn!(
                    "[cyw43] join: firmware-supplicant path=primary-plain unsupported status=0x{BCME_UNSUPPORTED:08x} action=try-bsscfg-wrapper reason=known-good-cyw43-fwsup-shape"
                );
                self.enable_wpa2_firmware_supplicant_bsscfg_wrapper()
            }
            Err(err) if ioctl_failed_status(&err) == Some(BCME_UNSUPPORTED) => {
                warn!(
                    "[cyw43] join: firmware-supplicant path=primary-plain unsupported status=0x{BCME_UNSUPPORTED:08x} action=continue-host-eapol-required reason=firmware-offload-unavailable"
                );
                Ok(FirmwareSupplicantPath::Unsupported)
            }
            Err(err) => Err(err),
        }
    }

    fn enable_wpa2_firmware_supplicant_bsscfg_wrapper(
        &mut self,
    ) -> Result<FirmwareSupplicantPath, DriverError> {
        match self.set_iovar_u32x2("bsscfg:sup_wpa", BSSCFG_PRIMARY_INDEX, 1) {
            Ok(()) => {
                self.set_optional_iovar_u32x2(
                    "bsscfg:sup_wpa2_eapver",
                    BSSCFG_PRIMARY_INDEX,
                    u32::MAX,
                )?;
                self.set_optional_iovar_u32x2("bsscfg:sup_wpa_tmo", BSSCFG_PRIMARY_INDEX, 2500)?;
                info!(
                    "[cyw43] join: firmware-supplicant path=bsscfg-wrapper requested=yes enabled=yes bsscfgidx={BSSCFG_PRIMARY_INDEX}"
                );
                Ok(FirmwareSupplicantPath::BsscfgWrapper)
            }
            Err(err) if ioctl_failed_status(&err) == Some(BCME_UNSUPPORTED) => {
                warn!(
                    "[cyw43] join: firmware-supplicant path=bsscfg-wrapper unsupported status=0x{BCME_UNSUPPORTED:08x} action=continue-host-eapol-required reason=firmware-offload-unavailable"
                );
                Ok(FirmwareSupplicantPath::Unsupported)
            }
            Err(err) => Err(err),
        }
    }

    fn set_wsec_pmk(&mut self, ssid: &[u8], psk: &[u8]) -> Result<WsecPmkKind, DriverError> {
        let pmk_kind;
        {
            let payload = self.payload_mut(WSEC_PMK_PAYLOAD_LEN)?;
            pmk_kind = write_wsec_pmk_payload(payload, ssid, psk)?;
        }
        match self.ioctl_encoded(IoctlType::Set, Ioctl::SetWsecPmk, 0, WSEC_PMK_PAYLOAD_LEN) {
            Ok(_) => Ok(pmk_kind),
            Err(DriverError::IoctlFailed { cmd, status })
                if cmd == Ioctl::SetWsecPmk as u32
                    && status == BCME_BADARG
                    && wsec_pmk_legacy_hex_fallback_allowed(psk) =>
            {
                warn!(
                    "[cyw43] join: linux-pmk rejected action=retry-legacy-hex-pmk status=0x{status:08x}"
                );
                {
                    let payload = self.payload_mut(WSEC_PMK_PAYLOAD_LEN)?;
                    write_wsec_legacy_hex_pmk_payload(payload, ssid, psk)?;
                }
                let _ =
                    self.ioctl_encoded(IoctlType::Set, Ioctl::SetWsecPmk, 0, WSEC_PMK_PAYLOAD_LEN)?;
                Ok(WsecPmkKind::LegacyHexPmk)
            }
            Err(err) => Err(err),
        }
    }

    fn wait_for_join(
        &mut self,
        secure: bool,
        completion_rule: JoinCompletionRule,
    ) -> Result<(), DriverError> {
        let mut completion = JoinCompletionState::default();
        for _ in 0..JOIN_WAIT_LOOPS {
            match self.process_next_frame(false)? {
                RxFrameResult::Event(event) => {
                    if let Some(result) =
                        join_event_result(event, secure, completion_rule, &mut completion)
                    {
                        result?;
                        self.link_up = true;
                        info!(
                            "[cyw43] join complete mode=blocking secure={} completion_rule={} set_ssid={} fwsup={} psk_sup={} psk_status=0x{:08x} carrier={}",
                            if secure { "yes" } else { "no" },
                            completion_rule.label(),
                            if completion.set_ssid_completed { "yes" } else { "no" },
                            if completion_rule.firmware_supplicant_required() { "yes" } else { "no" },
                            if completion.psk_completed { "yes" } else { "no" },
                            if completion.psk_completed { STATUS_UNSOLICITED } else { 0 },
                            if completion.carrier_confirmed { "yes" } else { "no" },
                        );
                        return Ok(());
                    }
                }
                RxFrameResult::None | RxFrameResult::Control { .. } | RxFrameResult::Data(_) => {}
            }
            spin_loop();
        }
        let reason = join_completion_timeout_reason(completion_rule);
        if matches!(completion_rule, JoinCompletionRule::HostEapolRequired) {
            self.state.promote_cached_control_plane_exact_error(reason);
        }
        Err(DriverError::Protocol(reason))
    }

    fn service_deferred_join(&mut self) {
        let DeferredJoinState::Pending {
            mut completion,
            mut polls,
            secure,
            completion_rule,
        } = self.deferred_join_state
        else {
            return;
        };

        for _ in 0..DEFERRED_JOIN_FRAME_BUDGET {
            match self.process_next_frame(false) {
                Ok(RxFrameResult::Event(event)) => {
                    if let Some(result) =
                        join_event_result(event, secure, completion_rule, &mut completion)
                    {
                        match result {
                            Ok(()) => {
                                self.link_up = true;
                                self.deferred_join_state = DeferredJoinState::Disabled;
                                info!(
                                    "[cyw43] join complete mode=deferred polls={polls} secure={} completion_rule={} set_ssid={} fwsup={} psk_sup={} psk_status=0x{:08x} carrier={}",
                                    if secure { "yes" } else { "no" },
                                    completion_rule.label(),
                                    if completion.set_ssid_completed { "yes" } else { "no" },
                                    if completion_rule.firmware_supplicant_required() { "yes" } else { "no" },
                                    if completion.psk_completed { "yes" } else { "no" },
                                    if completion.psk_completed { STATUS_UNSOLICITED } else { 0 },
                                    if completion.carrier_confirmed { "yes" } else { "no" },
                                );
                            }
                            Err(DriverError::JoinFailed { status, .. }) => {
                                self.link_up = false;
                                self.deferred_join_state = DeferredJoinState::Failed {
                                    reason: "join-failed",
                                };
                                warn!(
                                    "[cyw43] join failed mode=deferred status=0x{:08x} auth_status=0x{:08x} polls={polls}",
                                    status,
                                    completion.auth_status,
                                );
                            }
                            Err(err) => {
                                self.link_up = false;
                                self.deferred_join_state = DeferredJoinState::Failed {
                                    reason: "join-event-error",
                                };
                                warn!(
                                    "[cyw43] join failed mode=deferred reason=event-error auth_status=0x{:08x} polls={polls} err={err}",
                                    completion.auth_status,
                                );
                            }
                        }
                        return;
                    }
                }
                Ok(RxFrameResult::None) => break,
                Ok(RxFrameResult::Control { .. }) | Ok(RxFrameResult::Data(_)) => {}
                Err(err) => {
                    self.link_up = false;
                    self.deferred_join_state = DeferredJoinState::Failed {
                        reason: "join-progress-error",
                    };
                    warn!(
                        "[cyw43] join failed mode=deferred reason=progress-error auth_status=0x{:08x} polls={polls} err={err}",
                        completion.auth_status,
                    );
                    return;
                }
            }
        }

        polls = polls.saturating_add(1);
        if polls >= DEFERRED_JOIN_POLL_LIMIT {
            self.link_up = false;
            let reason = join_completion_timeout_reason(completion_rule);
            self.deferred_join_state = DeferredJoinState::Failed { reason };
            if matches!(completion_rule, JoinCompletionRule::HostEapolRequired) {
                self.state.promote_cached_control_plane_exact_error(reason);
            }
            warn!(
                "[cyw43] join failed mode=deferred reason={} auth_status=0x{:08x} polls={polls} completion_rule={} eapol_rx={}",
                reason,
                completion.auth_status,
                completion_rule.label(),
                self.host_eapol_rx_packets,
            );
            return;
        }

        self.deferred_join_state = DeferredJoinState::Pending {
            completion,
            polls,
            secure,
            completion_rule,
        };
    }

    fn read_mac_address(&mut self) -> Result<EthernetAddress, DriverError> {
        let mut response = [0u8; 6];
        let response_len = self.get_iovar("cur_etheraddr", &mut response)?;
        let mut mac = [0u8; 6];
        let mac_len = mac.len();
        if response_len >= mac_len {
            mac.copy_from_slice(&response[..mac_len]);
        }
        firmware_mac_address_from_response(mac, response_len)
    }

    fn read_revinfo(&mut self) -> Result<(), DriverError> {
        let response = [0u8; LINUX_REVINFO_LEN];
        let response_len = self.ioctl_raw(IoctlType::Get, Ioctl::GetRevInfo, 0, &response)?;
        if response_len != LINUX_REVINFO_LEN {
            return Err(DriverError::Protocol("revinfo-len"));
        }

        let response = self.control_response.as_ref();
        let chip = get_u32_le(response, 0).unwrap_or(0);
        let chip_rev = get_u32_le(response, 8).unwrap_or(0);
        let board_type = get_u32_le(response, 16).unwrap_or(0);
        let board_rev = get_u32_le(response, 20).unwrap_or(0);
        info!(
            "[cyw43] revinfo chip=0x{chip:08x} chip_rev=0x{chip_rev:08x} board_type=0x{board_type:08x} board_rev=0x{board_rev:08x} len={response_len}"
        );
        Ok(())
    }

    fn set_iovar_u32(&mut self, name: &str, value: u32) -> Result<(), DriverError> {
        {
            let payload = self.payload_mut(4)?;
            put_u32_le(payload, 0, value);
        }
        self.set_iovar_from_payload(name, 4)
    }

    fn set_iovar_u32x2(&mut self, name: &str, first: u32, second: u32) -> Result<(), DriverError> {
        {
            let payload = self.payload_mut(8)?;
            put_u32_le(payload, 0, first);
            put_u32_le(payload, 4, second);
        }
        self.set_iovar_from_payload(name, 8)
    }

    fn set_primary_bsscfg_u32(&mut self, name: &str, value: u32) -> Result<(), DriverError> {
        self.set_iovar_u32(name, value)
    }

    fn set_join_security_wpa_auth(&mut self, value: u32) -> Result<(), DriverError> {
        match self.set_primary_bsscfg_u32("wpa_auth", value) {
            Ok(()) => Ok(()),
            Err(err) => {
                if let Some(exact_error) = join_security_wpa_auth_failure_exact_error(value, &err) {
                    let stage = join_security_wpa_auth_stage(value);
                    warn!(
                        "[cyw43] iovar set failed name=wpa_auth stage={stage} value=0x{value:04x} err={err} exact={exact_error}"
                    );
                    Err(DriverError::Hal(HalError::Unsupported(exact_error)))
                } else {
                    Err(err)
                }
            }
        }
    }

    fn set_optional_iovar_u32(
        &mut self,
        name: &'static str,
        value: u32,
    ) -> Result<(), DriverError> {
        match self.set_iovar_u32(name, value) {
            Ok(()) => Ok(()),
            Err(err) if optional_control_plane_iovar_allows_failure(name, &err) => {
                warn!("[cyw43] optional iovar skipped name={name} err={err}");
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn set_optional_iovar_u32x2(
        &mut self,
        name: &'static str,
        first: u32,
        second: u32,
    ) -> Result<(), DriverError> {
        match self.set_iovar_u32x2(name, first, second) {
            Ok(()) => Ok(()),
            Err(err) if optional_control_plane_iovar_allows_failure(name, &err) => {
                warn!("[cyw43] optional iovar skipped name={name} err={err}");
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn set_iovar_bytes(&mut self, name: &str, value: &[u8]) -> Result<(), DriverError> {
        {
            let payload = self.payload_mut(value.len())?;
            payload[..value.len()].copy_from_slice(value);
        }
        self.set_iovar_from_payload(name, value.len())
    }

    fn set_iovar_from_payload(&mut self, name: &str, value_len: usize) -> Result<(), DriverError> {
        let name_len = name.len();
        if name_len + 1 + value_len > self.payload_capacity() {
            return Err(DriverError::FrameTooLarge);
        }
        let payload_offset = self.control_tx_payload_offset();
        self.tx_frame.copy_within(
            payload_offset..payload_offset + value_len,
            payload_offset + name_len + 1,
        );
        {
            let payload =
                &mut self.tx_frame[payload_offset..payload_offset + name_len + 1 + value_len];
            payload[..name_len].copy_from_slice(name.as_bytes());
            payload[name_len] = 0;
        }
        if join_security_iovar_name(name) {
            info!("[cyw43] iovar set begin name={name} len={value_len}");
        }
        match self.ioctl_encoded(IoctlType::Set, Ioctl::SetVar, 0, name_len + 1 + value_len) {
            Ok(_) => {
                if join_security_iovar_name(name) {
                    info!("[cyw43] iovar set ready name={name} len={value_len}");
                }
                Ok(())
            }
            Err(err) => {
                if let Some(exact_error) = join_security_iovar_failure_exact_error(name, &err) {
                    warn!("[cyw43] iovar set failed name={name} err={err} exact={exact_error}");
                    Err(DriverError::Hal(HalError::Unsupported(exact_error)))
                } else {
                    warn!("[cyw43] iovar set failed name={name} err={err}");
                    Err(err)
                }
            }
        }
    }

    fn get_iovar(&mut self, name: &str, out: &mut [u8]) -> Result<usize, DriverError> {
        let payload_len = iovar_get_payload_len(name, out.len())?;
        if payload_len > self.payload_capacity() {
            return Err(DriverError::FrameTooLarge);
        }
        {
            let payload = self.payload_mut(payload_len)?;
            payload.fill(0);
            payload[..name.len()].copy_from_slice(name.as_bytes());
            payload[name.len()] = 0;
        }
        let response_len = self.ioctl_encoded(IoctlType::Get, Ioctl::GetVar, 0, payload_len)?;
        let copy_len = core::cmp::min(out.len(), response_len);
        out[..copy_len].copy_from_slice(&self.control_response[..copy_len]);
        Ok(copy_len)
    }

    fn get_optional_linux_iovar(
        &mut self,
        name: &'static str,
        out: &mut [u8],
    ) -> Result<usize, DriverError> {
        match self.get_iovar(name, out) {
            Ok(len) => Ok(len),
            Err(DriverError::IoctlFailed { cmd, status })
                if linux_optional_iovar_allows_unsupported(name, cmd, status) =>
            {
                info!(
                    "[cyw43] control-plane optional linux iovar unsupported name={name} status=0x{status:08x}"
                );
                Ok(0)
            }
            Err(err) => Err(err),
        }
    }

    fn ioctl_set_u32(&mut self, cmd: Ioctl, iface: u32, value: u32) -> Result<(), DriverError> {
        let bytes = value.to_le_bytes();
        let _ = self.ioctl_raw(IoctlType::Set, cmd, iface, &bytes)?;
        Ok(())
    }

    fn ioctl_raw(
        &mut self,
        kind: IoctlType,
        cmd: Ioctl,
        iface: u32,
        payload: &[u8],
    ) -> Result<usize, DriverError> {
        if payload.len() > self.payload_capacity() {
            return Err(DriverError::FrameTooLarge);
        }
        {
            let slot = self.payload_mut(payload.len())?;
            slot[..payload.len()].copy_from_slice(payload);
        }
        self.ioctl_encoded(kind, cmd, iface, payload.len())
    }

    fn ioctl_encoded(
        &mut self,
        kind: IoctlType,
        cmd: Ioctl,
        iface: u32,
        payload_len: usize,
    ) -> Result<usize, DriverError> {
        let control_plane_probe_pending = self.state.cyw43_control_plane_probe_pending();
        let experimental_no_ht_transport = self.state.cyw43_experimental_no_ht_transport();
        match self.ioctl_encoded_once(kind, cmd, iface, payload_len, false) {
            Ok(response_len) => Ok(response_len),
            Err(err) => {
                if let Some(target_clock_hz) =
                    control_plane_retry_after_promoted_timeout_target_clock_hz(
                        experimental_no_ht_transport,
                        control_plane_probe_pending,
                        self.probe.effective_clock_hz,
                        &err,
                    )
                {
                    self.retry_first_control_plane_ioctl_on_startup_link(
                        kind,
                        cmd,
                        iface,
                        payload_len,
                        target_clock_hz,
                        "control-plane probe retry",
                        "control-plane-probe-retry-original-reply",
                        "control-plane-probe-retry-resend",
                        &err,
                    )
                } else if let Some(target_clock_hz) =
                    control_plane_retry_after_startup_link_reply_failure_target_clock_hz(
                        experimental_no_ht_transport,
                        control_plane_probe_pending,
                        self.probe.effective_clock_hz,
                        &err,
                    )
                {
                    self.retry_first_control_plane_ioctl_on_startup_link(
                        kind,
                        cmd,
                        iface,
                        payload_len,
                        target_clock_hz,
                        "control-plane startup-link reply rescue",
                        "control-plane-startup-link-reply-rescue-original-reply",
                        "control-plane-startup-link-reply-rescue-resend",
                        &err,
                    )
                } else {
                    Err(err)
                }
            }
        }
    }

    fn retry_first_control_plane_ioctl_on_startup_link(
        &mut self,
        kind: IoctlType,
        cmd: Ioctl,
        iface: u32,
        payload_len: usize,
        target_clock_hz: u32,
        retry_label: &'static str,
        original_reply_stage: &'static str,
        resend_stage: &'static str,
        reason: &DriverError,
    ) -> Result<usize, DriverError> {
        warn!(
            "[cyw43] {retry_label} cmd=0x{:08x} iface={} len={} target_clock={}Hz chunk_limit={} mode=bounded-no-ht reason={reason}",
            cmd as u32,
            iface,
            payload_len,
            target_clock_hz,
            self.state.cyw43_control_plane_chunk_limit(),
        );
        let effective_clock_hz = self.state.set_clock_hz(target_clock_hz)?;
        self.probe.effective_clock_hz = effective_clock_hz;
        rearm_control_plane_after_reply_wait(
            self.state.as_mut(),
            self.probe.effective_clock_hz,
            original_reply_stage,
        )?;
        info!(
            "[cyw43] {retry_label} awaiting original reply cmd=0x{:08x} iface={} len={} ioctl_id={} clock={}Hz chunk_limit={} mode=bounded-no-ht",
            cmd as u32,
            iface,
            payload_len,
            self.ioctl_id,
            self.probe.effective_clock_hz,
            self.state.cyw43_control_plane_chunk_limit(),
        );
        match self.wait_for_ioctl_response(cmd as u32, self.ioctl_id) {
            Ok(response_len) => {
                info!(
                    "[cyw43] {retry_label} recovered-original-reply cmd=0x{:08x} iface={} len={} ioctl_id={} clock={}Hz chunk_limit={} mode=bounded-no-ht",
                    cmd as u32,
                    iface,
                    payload_len,
                    self.ioctl_id,
                    self.probe.effective_clock_hz,
                    self.state.cyw43_control_plane_chunk_limit(),
                );
                Ok(response_len)
            }
            Err(wait_err)
                if control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                    &wait_err,
                ) =>
            {
                let resend_clock_hz = self.state.set_clock_hz(
                    control_plane_retry_after_reply_wait_resend_target_clock_hz(
                        self.state.cyw43_experimental_no_ht_transport(),
                        self.probe.effective_clock_hz,
                        reason,
                    ),
                )?;
                self.probe.effective_clock_hz = resend_clock_hz;
                rearm_control_plane_after_reply_wait(
                    self.state.as_mut(),
                    self.probe.effective_clock_hz,
                    resend_stage,
                )?;
                let resend_bootstrap = if resend_clock_hz > SDIO_STARTUP_CLOCK_HZ {
                    "promoted-first-reply-retry"
                } else {
                    "startup-link-passive-retry"
                };
                info!(
                    "[cyw43] {retry_label} resend armed cmd=0x{:08x} iface={} len={} clock={}Hz bootstrap={} chunk_limit={} mode=bounded-no-ht",
                    cmd as u32,
                    iface,
                    payload_len,
                    self.probe.effective_clock_hz,
                    resend_bootstrap,
                    self.state.cyw43_control_plane_chunk_limit(),
                );
                warn!(
                    "[cyw43] {retry_label} resending cmd=0x{:08x} iface={} len={} clock={}Hz bootstrap={} chunk_limit={} mode=bounded-no-ht reason={wait_err}",
                    cmd as u32,
                    iface,
                    payload_len,
                    self.probe.effective_clock_hz,
                    resend_bootstrap,
                    self.state.cyw43_control_plane_chunk_limit(),
                );
                let response_len = self.ioctl_encoded_once(kind, cmd, iface, payload_len, true)?;
                info!(
                    "[cyw43] {retry_label} ok cmd=0x{:08x} iface={} len={} clock={}Hz chunk_limit={} mode=bounded-no-ht",
                    cmd as u32,
                    iface,
                    payload_len,
                    self.probe.effective_clock_hz,
                    self.state.cyw43_control_plane_chunk_limit(),
                );
                Ok(response_len)
            }
            Err(wait_err) => Err(wait_err),
        }
    }

    fn ioctl_encoded_once(
        &mut self,
        kind: IoctlType,
        cmd: Ioctl,
        iface: u32,
        payload_len: usize,
        allow_speculative_retry_credit: bool,
    ) -> Result<usize, DriverError> {
        self.wait_for_credit(allow_speculative_retry_credit)?;

        let control_header_len = self.control_tx_header_len();
        let total_len = control_header_len
            .checked_add(CDC_HEADER_LEN)
            .and_then(|value| value.checked_add(payload_len))
            .ok_or(DriverError::FrameTooLarge)?;
        let request_len = sdpcm_control_tx_request_len(total_len);
        if request_len > self.tx_frame.len() {
            return Err(DriverError::FrameTooLarge);
        }
        let tail_pad = request_len
            .checked_sub(total_len)
            .ok_or(DriverError::FrameTooLarge)?;

        let sdpcm_seq = self.sdpcm_seq;
        self.sdpcm_seq = self.sdpcm_seq.wrapping_add(1);
        self.ioctl_id = self.ioctl_id.wrapping_add(1);

        let packet_len = u16::try_from(request_len).map_err(|_| DriverError::FrameTooLarge)?;
        write_sdpcm_control_tx_header(
            self.tx_frame.as_mut(),
            control_header_len,
            packet_len,
            total_len,
            tail_pad,
            sdpcm_seq,
        )?;

        put_u32_le(&mut self.tx_frame[control_header_len..], 0, cmd as u32);
        put_u32_le(
            &mut self.tx_frame[control_header_len..],
            4,
            u32::try_from(payload_len).map_err(|_| DriverError::FrameTooLarge)?,
        );
        put_u16_le(
            &mut self.tx_frame[control_header_len..],
            8,
            (kind as u16) | (u16::try_from(iface).unwrap_or(0) << 12),
        );
        put_u16_le(&mut self.tx_frame[control_header_len..], 10, self.ioctl_id);
        put_u32_le(&mut self.tx_frame[control_header_len..], 12, 0);

        self.tx_frame[total_len..request_len].fill(0);
        trace!(
            "[cyw43] ioctl tx cmd=0x{:08x} iface={} len={} seq={}",
            cmd as u32,
            iface,
            payload_len,
            sdpcm_seq
        );
        if self.state.cyw43_control_plane_probe_pending() {
            let transport_mode = if self.state.cyw43_experimental_no_ht_transport() {
                "bounded-no-ht"
            } else {
                "strict"
            };
            let sw_header_offset = control_tx_sw_header_offset_for_header(control_header_len);
            info!(
                "[cyw43] control tx probe cmd=0x{:08x} iface={} payload_len={} packet_len={} len_inv=0x{:04x} unpadded_len={} request_len={} tail_pad={} seq={}/{} ioctl_id={} channel={} header_channel={} header_len={} cdc_flags=0x{:04x} write_chunk_limit={} reply_chunk_limit={} mode={transport_mode}",
                cmd as u32,
                iface,
                payload_len,
                packet_len,
                !packet_len,
                total_len,
                request_len,
                tail_pad,
                sdpcm_seq,
                self.sdpcm_seq_max,
                self.ioctl_id,
                CHANNEL_CONTROL,
                self.tx_frame[sw_header_offset + 1] & 0x0f,
                self.tx_frame[sw_header_offset + 3],
                get_u16_le(&self.tx_frame[control_header_len..], 8).unwrap_or(0),
                self.state.cyw43_control_plane_write_chunk_limit(),
                self.state.cyw43_control_plane_reply_chunk_limit(),
            );
        }
        self.state
            .write_cyw43_frame(&mut self.tx_frame[..request_len])?;
        if self.state.cyw43_experimental_no_ht_transport() && allow_speculative_retry_credit {
            if control_plane_retry_after_promoted_timeout_resend_uses_startup_link(
                true,
                allow_speculative_retry_credit,
                self.probe.effective_clock_hz,
            ) {
                self.state
                    .resume_cyw43_control_plane_reply_probe_on_startup_link(
                        "control-plane-probe-retry-resend",
                    );
            }
        }
        self.wait_for_ioctl_response(cmd as u32, self.ioctl_id)
    }

    fn wait_for_ioctl_response(
        &mut self,
        cmd: u32,
        expected_id: u16,
    ) -> Result<usize, DriverError> {
        let startup_link_stabilized = self.state.cyw43_control_plane_startup_link_stabilized();
        let startup_link_rescue_cycles =
            self.state.cyw43_control_plane_startup_link_rescue_cycles();
        let control_plane_probe_pending = self.state.cyw43_control_plane_probe_pending();
        let wait_budget = ioctl_wait_loops(
            startup_link_stabilized,
            startup_link_rescue_cycles,
            control_plane_probe_pending,
        );
        let mut no_progress_polls = 0usize;
        let mut nonmatching_frames = 0usize;
        for _ in 0..wait_budget {
            match self.process_next_frame(false) {
                Err(err) => {
                    let (reply_mode, reply_attempts, reply_empty_polls, promoted_probe_pending) =
                        self.state.cyw43_control_plane_reply_rearm_diag();
                    let credit_ready = self.has_credit();
                    warn!(
                        "[cyw43] ioctl response error cmd=0x{:08x} id={} seq={}/{} credit={} startup_link_stabilized={} startup_link_rescue_cycles={} reply_mode={} reply_attempts={} reply_empty_polls={} promoted_probe_pending={} no_ht={} write_chunk_limit={} reply_chunk_limit={} err={err}",
                        cmd,
                        expected_id,
                        self.sdpcm_seq,
                        self.sdpcm_seq_max,
                        credit_ready,
                        startup_link_stabilized,
                        startup_link_rescue_cycles,
                        reply_mode,
                        reply_attempts,
                        reply_empty_polls,
                        promoted_probe_pending,
                        self.state.cyw43_experimental_no_ht_transport(),
                        self.state.cyw43_control_plane_write_chunk_limit(),
                        self.state.cyw43_control_plane_reply_chunk_limit(),
                    );
                    self.log_pending_ioctl_frame("ioctl-response-error");
                    self.state
                        .log_cyw43_control_plane_snapshot("ioctl-response-error");
                    self.state.finish_cyw43_control_plane_reply_wait();
                    return Err(err);
                }
                Ok(
                    result @ RxFrameResult::Control {
                        id,
                        status,
                        response_len,
                        ..
                    },
                ) if control_response_matches(&result, cmd, expected_id) => {
                    if status != STATUS_SUCCESS {
                        self.state.finish_cyw43_control_plane_reply_wait();
                        return Err(DriverError::IoctlFailed { cmd, status });
                    }
                    self.state.finish_cyw43_control_plane_reply_wait();
                    return Ok(response_len);
                }
                Ok(RxFrameResult::None) => {
                    no_progress_polls = no_progress_polls.saturating_add(1);
                    if ioctl_no_progress_after_nonmatching_frames(
                        nonmatching_frames,
                        no_progress_polls,
                    ) {
                        let cached_exact_error = self
                            .state
                            .cyw43_cached_control_plane_exact_error()
                            .unwrap_or("none");
                        warn!(
                            "[cyw43] ioctl no-progress-after-frame cmd=0x{:08x} id={} no_progress_polls={} nonmatching_frames={} cached_exact_error={} action=fail-fast",
                            cmd,
                            expected_id,
                            no_progress_polls,
                            nonmatching_frames,
                            cached_exact_error,
                        );
                        self.log_pending_ioctl_frame("ioctl-no-progress-after-frame");
                        self.state
                            .log_cyw43_control_plane_snapshot("ioctl-no-progress-after-frame");
                        self.state.finish_cyw43_control_plane_reply_wait();
                        return Err(DriverError::Protocol("ioctl-no-progress-after-frame"));
                    }
                }
                Ok(RxFrameResult::Control { .. })
                | Ok(RxFrameResult::Event(_))
                | Ok(RxFrameResult::Data(_)) => {
                    nonmatching_frames = nonmatching_frames.saturating_add(1);
                    no_progress_polls = 0;
                }
            }
            spin_loop();
        }
        let preserved_exact_error = startup_link_ioctl_timeout_preserved_exact_error(
            startup_link_stabilized,
            control_plane_probe_pending,
            self.state.cyw43_cached_control_plane_exact_error(),
        );
        let (reply_mode, reply_attempts, reply_empty_polls, promoted_probe_pending) =
            self.state.cyw43_control_plane_reply_rearm_diag();
        let credit_ready = self.has_credit();
        warn!(
            "[cyw43] ioctl timeout cmd=0x{:08x} id={} seq={}/{} credit={} wait_budget={} startup_link_stabilized={} startup_link_rescue_cycles={} reply_mode={} reply_attempts={} reply_empty_polls={} promoted_probe_pending={} no_ht={} write_chunk_limit={} reply_chunk_limit={} preserved_exact_error={}",
            cmd,
            expected_id,
            self.sdpcm_seq,
            self.sdpcm_seq_max,
            credit_ready,
            wait_budget,
            startup_link_stabilized,
            startup_link_rescue_cycles,
            reply_mode,
            reply_attempts,
            reply_empty_polls,
            promoted_probe_pending,
            self.state.cyw43_experimental_no_ht_transport(),
            self.state.cyw43_control_plane_write_chunk_limit(),
            self.state.cyw43_control_plane_reply_chunk_limit(),
            preserved_exact_error.unwrap_or("none"),
        );
        self.log_pending_ioctl_frame("ioctl-timeout");
        self.state.log_cyw43_control_plane_snapshot("ioctl-timeout");
        self.state.finish_cyw43_control_plane_reply_wait();
        Err(DriverError::Protocol(
            preserved_exact_error.unwrap_or("ioctl-timeout"),
        ))
    }

    fn drain_linux_startup_status_frames(&mut self) -> Result<(), DriverError> {
        for attempt in 0..LINUX_STARTUP_STATUS_DRAIN_BUDGET {
            let frame_len = self.state.read_cyw43_frame(self.rx_frame.as_mut())?;
            if frame_len == 0 {
                info!(
                    "[cyw43] control-plane startup-drain action=idle attempt={}/{}",
                    attempt + 1,
                    LINUX_STARTUP_STATUS_DRAIN_BUDGET
                );
                return Ok(());
            }

            let header = parse_rx_sdpcm_header(self.rx_frame.as_ref(), frame_len)?;
            if sdpcm_header_only_status_frame(header) {
                self.update_credit(header);
                info!(
                    "[cyw43] control-plane startup-drain action=drain-header-only channel={} packet_len={} credit={} attempt={}/{}",
                    header.channel,
                    header.packet_len,
                    header.credit,
                    attempt + 1,
                    LINUX_STARTUP_STATUS_DRAIN_BUDGET
                );
                continue;
            }

            let result = self.process_frame(frame_len, false)?;
            info!(
                "[cyw43] control-plane startup-drain action=process-frame result={} frame_len={} attempt={}/{}",
                rx_frame_result_name(&result),
                frame_len,
                attempt + 1,
                LINUX_STARTUP_STATUS_DRAIN_BUDGET
            );
        }
        Ok(())
    }

    fn drain_post_up_events(&mut self) -> Result<(), DriverError> {
        let mut drained = 0usize;
        let mut idle = 0usize;
        for _ in 0..POST_UP_EVENT_DRAIN_BUDGET {
            match self.process_next_frame(false)? {
                RxFrameResult::None => {
                    idle = idle.saturating_add(1);
                    if idle >= 2 {
                        break;
                    }
                }
                result => {
                    drained = drained.saturating_add(1);
                    idle = 0;
                    info!(
                        "[cyw43] control-plane post-up event-drain action=drain result={} drained={drained}",
                        rx_frame_result_name(&result)
                    );
                }
            }
            spin_loop();
        }
        info!(
            "[cyw43] control-plane post-up event-drain action=ready drained={drained} idle_polls={idle}"
        );
        Ok(())
    }

    fn log_pending_ioctl_frame(&self, stage: &'static str) {
        let tx_frame = self.tx_frame.as_ref();
        let packet_len = get_u16_le(tx_frame, 0).unwrap_or(0);
        let len_inv = get_u16_le(tx_frame, 2).unwrap_or(0);
        let hwext = if self.control_tx_ext_header {
            get_u32_le(tx_frame, SDPCM_HWHDR_LEN).unwrap_or(0)
        } else {
            0
        };
        let tail_pad = if self.control_tx_ext_header {
            get_u32_le(tx_frame, SDPCM_HWHDR_LEN + 4).unwrap_or(0) >> 16
        } else {
            0
        };
        let control_header_len = self.control_tx_header_len();
        let cdc = &self.tx_frame[control_header_len..];
        let sw_header_offset = control_tx_sw_header_offset_for_header(control_header_len);
        let cdc_cmd = get_u32_le(cdc, 0).unwrap_or(0);
        let cdc_len = get_u32_le(cdc, 4).unwrap_or(0);
        let cdc_flags = get_u16_le(cdc, 8).unwrap_or(0);
        let cdc_id = get_u16_le(cdc, 10).unwrap_or(0);
        let cdc_status = get_u32_le(cdc, 12).unwrap_or(0);
        warn!(
            "[cyw43] ioctl frame {stage} packet_len={} len_inv=0x{:04x} hwext=0x{hwext:08x} tail_pad={} seq=0x{:02x} channel=0x{:02x} header_len={} cdc_cmd=0x{:08x} cdc_len={} cdc_flags=0x{:04x} cdc_id={} cdc_status=0x{:08x}",
            packet_len,
            len_inv,
            tail_pad,
            self.tx_frame[sw_header_offset],
            self.tx_frame[sw_header_offset + 1] & 0x0f,
            self.tx_frame[sw_header_offset + 3],
            cdc_cmd,
            cdc_len,
            cdc_flags,
            cdc_id,
            cdc_status,
        );
    }

    fn wait_for_credit(&mut self, allow_speculative_retry_credit: bool) -> Result<(), DriverError> {
        for _ in 0..CREDIT_WAIT_LOOPS {
            if self.has_credit() {
                return Ok(());
            }
            let _ = self.process_next_frame(false)?;
            spin_loop();
        }
        if let Some(speculative_sdpcm_seq_max) =
            speculative_credit_window_after_promoted_timeout_retry(
                allow_speculative_retry_credit,
                self.sdpcm_seq,
                self.sdpcm_seq_max,
            )
        {
            warn!(
                "[cyw43] control-plane probe retry forcing speculative credit seq={}/{} -> {}",
                self.sdpcm_seq, self.sdpcm_seq_max, speculative_sdpcm_seq_max,
            );
            self.sdpcm_seq_max = speculative_sdpcm_seq_max;
            return Ok(());
        }
        let (reply_mode, reply_attempts, reply_empty_polls, promoted_probe_pending) =
            self.state.cyw43_control_plane_reply_rearm_diag();
        warn!(
            "[cyw43] sdpcm credit timeout seq={}/{} credit={} reply_mode={} reply_attempts={} reply_empty_polls={} promoted_probe_pending={} no_ht={} write_chunk_limit={} reply_chunk_limit={}",
            self.sdpcm_seq,
            self.sdpcm_seq_max,
            self.has_credit(),
            reply_mode,
            reply_attempts,
            reply_empty_polls,
            promoted_probe_pending,
            self.state.cyw43_experimental_no_ht_transport(),
            self.state.cyw43_control_plane_write_chunk_limit(),
            self.state.cyw43_control_plane_reply_chunk_limit(),
        );
        self.state
            .log_cyw43_control_plane_snapshot("sdpcm-credit-timeout");
        Err(DriverError::Protocol("sdpcm-credit-timeout"))
    }

    fn has_credit(&self) -> bool {
        has_sdpcm_credit(self.sdpcm_seq, self.sdpcm_seq_max)
    }

    fn process_next_frame(&mut self, allow_data: bool) -> Result<RxFrameResult, DriverError> {
        let frame_len = self.state.read_cyw43_frame(self.rx_frame.as_mut())?;
        if frame_len == 0 {
            return Ok(RxFrameResult::None);
        }
        self.process_frame(frame_len, allow_data)
    }

    fn process_frame(
        &mut self,
        frame_len: usize,
        allow_data: bool,
    ) -> Result<RxFrameResult, DriverError> {
        let header = parse_rx_sdpcm_header(self.rx_frame.as_ref(), frame_len)?;
        self.update_credit(header);

        match header.channel {
            CHANNEL_CONTROL => self.process_control_frame(header.payload_start, header.packet_len),
            CHANNEL_EVENT => self.process_event_frame(header.payload_start, header.packet_len),
            CHANNEL_DATA => self.process_data_or_event_frame(
                header.payload_start,
                header.packet_len,
                allow_data,
            ),
            CHANNEL_GLOM => {
                warn!(
                    "[cyw43] rx glom frame unsupported len={} descriptor={} action=drop reason=rxglom-disabled-bounded-rx",
                    header.packet_len,
                    sdpcm_glom_descriptor(self.rx_frame.as_ref()),
                );
                Err(DriverError::Protocol("cyw43-rxglom-unsupported"))
            }
            _ => Ok(RxFrameResult::None),
        }
    }

    fn process_control_frame(
        &mut self,
        payload_start: usize,
        payload_end: usize,
    ) -> Result<RxFrameResult, DriverError> {
        let payload = &self.rx_frame[payload_start..payload_end];
        if payload.is_empty() {
            info!("[cyw43] control-plane header-only control frame drained");
            return Ok(RxFrameResult::None);
        }
        if payload.len() < CDC_HEADER_LEN {
            return Err(DriverError::Protocol("cdc-short-header"));
        }
        let response_cmd = get_u32_le(payload, 0).ok_or(DriverError::Protocol("cdc-cmd"))?;
        let response_len = usize::try_from(
            get_u32_le(payload, 4).ok_or(DriverError::Protocol("cdc-response-len"))?,
        )
        .map_err(|_| DriverError::ResponseTooLarge)?;
        let status = get_u32_le(payload, 12).ok_or(DriverError::Protocol("cdc-status"))?;
        let id = get_u16_le(payload, 10).ok_or(DriverError::Protocol("cdc-id"))?;
        let payload_available = payload.len().saturating_sub(CDC_HEADER_LEN);
        let copy_len = control_response_copy_len(
            response_len,
            payload_available,
            self.control_response.len(),
        )?;
        self.control_response[..copy_len]
            .copy_from_slice(&payload[CDC_HEADER_LEN..CDC_HEADER_LEN.saturating_add(copy_len)]);
        info!(
            "[cyw43] control-plane reply cmd=0x{response_cmd:08x} id={} status=0x{status:08x} response_len={} copied={} sdpcm_seq={} sdpcm_credit={}",
            id,
            response_len,
            copy_len,
            self.sdpcm_seq,
            self.sdpcm_seq_max,
        );
        Ok(RxFrameResult::Control {
            cmd: response_cmd,
            id,
            status,
            response_len: copy_len,
        })
    }

    fn process_event_frame(
        &mut self,
        payload_start: usize,
        payload_end: usize,
    ) -> Result<RxFrameResult, DriverError> {
        let payload = &self.rx_frame[payload_start..payload_end];
        let Some(event) = parse_event_payload(payload)? else {
            return Ok(RxFrameResult::None);
        };
        self.apply_event(event);
        info!(
            "[cyw43] event type={} flags=0x{:04x} status=0x{:08x} reason=0x{:08x} auth=0x{:08x}",
            event.event_type, event.flags, event.status, event.reason, event.auth_type
        );
        Ok(RxFrameResult::Event(event))
    }

    fn process_data_or_event_frame(
        &mut self,
        payload_start: usize,
        payload_end: usize,
        allow_data: bool,
    ) -> Result<RxFrameResult, DriverError> {
        let payload = &self.rx_frame[payload_start..payload_end];
        let Some(packet) = bdc_payload(payload) else {
            trace!(
                "[cyw43] data/event frame ignored reason=bdc-header-unavailable len={}",
                payload.len()
            );
            return Ok(RxFrameResult::None);
        };
        if let Some(event) = parse_broadcom_event(packet)? {
            self.apply_event(event);
            info!(
                "[cyw43] event type={} flags=0x{:04x} status=0x{:08x} reason=0x{:08x} auth=0x{:08x}",
                event.event_type, event.flags, event.status, event.reason, event.auth_type
            );
            return Ok(RxFrameResult::Event(event));
        }
        if ethernet_ethertype(packet) == Some(ETH_P_EAPOL)
            && deferred_join_requires_host_eapol(self.deferred_join_state)
        {
            self.host_eapol_rx_packets = self.host_eapol_rx_packets.saturating_add(1);
            self.state
                .promote_cached_control_plane_exact_error("host-eapol-required");
            let proof = host_eapol_frame_proof(packet);
            warn!(
                "[cyw43] host-eapol proof count={} msg={} len={} eapol_ver={} type={} body_len={} body_ok={} key_desc={} key_info=0x{:04x} key_ver={} replay={} kde_len={} action=drop status=host-eapol-required",
                self.host_eapol_rx_packets,
                proof.message,
                packet.len(),
                proof.eapol_version,
                proof.packet_type,
                proof.body_len,
                if proof.body_len_valid { "yes" } else { "no" },
                proof.descriptor_type,
                proof.key_info,
                proof.key_version,
                if proof.replay_counter_nonzero { "yes" } else { "no" },
                proof.key_data_len,
            );
            return Ok(RxFrameResult::None);
        }
        if !allow_data {
            return Ok(RxFrameResult::None);
        }
        if packet.len() > MAX_FRAME_LEN {
            return Err(DriverError::FrameTooLarge);
        }
        let mut frame = HeaplessVec::new();
        frame
            .extend_from_slice(packet)
            .map_err(|_| DriverError::FrameTooLarge)?;
        Ok(RxFrameResult::Data(frame))
    }

    fn apply_event(&mut self, event: Cyw43Event) {
        if let Some(link_up) =
            event_link_state_update(event, deferred_join_is_pending(self.deferred_join_state))
        {
            self.link_up = link_up;
        }
    }

    fn update_credit(&mut self, header: RxSdpcmHeader) {
        let mut sdpcm_seq_max = header.credit;
        if header.channel < 3 {
            if sdpcm_seq_max.wrapping_sub(self.sdpcm_seq) > 0x40 {
                sdpcm_seq_max = self.sdpcm_seq.wrapping_add(2);
            }
            self.sdpcm_seq_max = sdpcm_seq_max;
        }
    }

    fn poll_rx(&mut self) -> Option<HeaplessVec<u8, MAX_FRAME_LEN>> {
        if !deferred_join_allows_rx_polling(self.deferred_join_state) {
            return None;
        }

        if matches!(self.deferred_join_state, DeferredJoinState::Pending { .. }) {
            self.service_deferred_join();
            if !self.link_up {
                return None;
            }
        }
        for _ in 0..RX_PUMP_LIMIT {
            match self.process_next_frame(true) {
                Ok(RxFrameResult::Data(frame)) => {
                    self.rx_packets = self.rx_packets.saturating_add(1);
                    return Some(frame);
                }
                Ok(
                    RxFrameResult::None | RxFrameResult::Control { .. } | RxFrameResult::Event(_),
                ) => {}
                Err(err) => {
                    warn!("[cyw43] rx error: {err}");
                    return None;
                }
            }
        }
        None
    }

    fn transmit(&mut self, packet: &[u8]) -> Result<(), DriverError> {
        self.wait_for_credit(false)?;
        let total_len = SDPCM_HEADER_LEN
            .checked_add(DATA_PADDING_LEN)
            .and_then(|value| value.checked_add(BDC_HEADER_LEN))
            .and_then(|value| value.checked_add(packet.len()))
            .ok_or(DriverError::FrameTooLarge)?;
        let aligned_len = align4(total_len);
        if aligned_len > self.tx_frame.len() {
            return Err(DriverError::FrameTooLarge);
        }

        let seq = self.sdpcm_seq;
        self.sdpcm_seq = self.sdpcm_seq.wrapping_add(1);

        put_u16_le(
            self.tx_frame.as_mut(),
            0,
            u16::try_from(total_len).map_err(|_| DriverError::FrameTooLarge)?,
        );
        put_u16_le(
            self.tx_frame.as_mut(),
            2,
            !(u16::try_from(total_len).map_err(|_| DriverError::FrameTooLarge)?),
        );
        self.tx_frame[4] = seq;
        self.tx_frame[5] = CHANNEL_DATA;
        self.tx_frame[6] = 0;
        self.tx_frame[7] = u8::try_from(SDPCM_HEADER_LEN + DATA_PADDING_LEN)
            .map_err(|_| DriverError::FrameTooLarge)?;
        self.tx_frame[8] = 0;
        self.tx_frame[9] = 0;
        self.tx_frame[10] = 0;
        self.tx_frame[11] = 0;
        self.tx_frame[SDPCM_HEADER_LEN..SDPCM_HEADER_LEN + DATA_PADDING_LEN].fill(0);
        self.tx_frame[SDPCM_HEADER_LEN + DATA_PADDING_LEN] = BDC_VERSION << BDC_VERSION_SHIFT;
        self.tx_frame[SDPCM_HEADER_LEN + DATA_PADDING_LEN + 1] = 0;
        self.tx_frame[SDPCM_HEADER_LEN + DATA_PADDING_LEN + 2] = 0;
        self.tx_frame[SDPCM_HEADER_LEN + DATA_PADDING_LEN + 3] = 0;
        self.tx_frame[SDPCM_HEADER_LEN + DATA_PADDING_LEN + BDC_HEADER_LEN
            ..SDPCM_HEADER_LEN + DATA_PADDING_LEN + BDC_HEADER_LEN + packet.len()]
            .copy_from_slice(packet);
        self.tx_frame[total_len..aligned_len].fill(0);
        self.state
            .write_cyw43_frame(&mut self.tx_frame[..aligned_len])?;
        self.tx_packets = self.tx_packets.saturating_add(1);
        Ok(())
    }

    fn payload_mut(&mut self, payload_len: usize) -> Result<&mut [u8], DriverError> {
        if payload_len > self.payload_capacity() {
            return Err(DriverError::FrameTooLarge);
        }
        let payload_offset = self.control_tx_payload_offset();
        Ok(&mut self.tx_frame[payload_offset..payload_offset + payload_len])
    }

    const fn control_tx_header_len(&self) -> usize {
        if self.control_tx_ext_header {
            SDPCM_CONTROL_TX_EXT_HEADER_LEN
        } else {
            SDPCM_CONTROL_TX_HEADER_LEN
        }
    }

    const fn control_tx_payload_offset(&self) -> usize {
        control_tx_payload_offset_for_header(self.control_tx_header_len())
    }

    fn enable_control_tx_extension_header(&mut self, stage: &'static str) {
        if !self.control_tx_ext_header {
            self.control_tx_ext_header = true;
            info!(
                "[cyw43] control-plane step={stage} action=enable-sdpcm-tx-hwext header_len={}",
                self.control_tx_header_len()
            );
        }
    }

    const fn payload_capacity(&self) -> usize {
        FRAME_BUF_LEN - self.control_tx_payload_offset()
    }
}

impl phy::RxToken for RxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(self.frame.as_slice())
    }
}

impl<'a> phy::TxToken for TxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut temp = [0u8; MAX_FRAME_LEN];
        let frame = &mut temp[..len.min(MAX_FRAME_LEN)];
        let result = f(frame);
        if let Err(err) = self.device.transmit(frame) {
            self.device.tx_drops = self.device.tx_drops.saturating_add(1);
            warn!("[cyw43] tx error: {err}");
        }
        result
    }
}

impl Device for Cyw43NetDevice {
    type RxToken<'a>
        = RxToken
    where
        Self: 'a;
    type TxToken<'a>
        = TxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        self.poll_rx()
            .map(|frame| (RxToken { frame }, TxToken { device: self }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if self.link_up {
            Some(TxToken { device: self })
        } else {
            None
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = MAX_FRAME_LEN;
        caps.medium = smoltcp::phy::Medium::Ethernet;
        caps
    }
}

impl NetDevice for Cyw43NetDevice {
    type Error = DriverError;

    fn create<H>(_: &mut H) -> Result<Self, Self::Error>
    where
        H: Hardware<Error = HalError>,
        Self: Sized,
    {
        Err(DriverError::Config("wifi-config-required"))
    }

    fn create_with_stage<H>(
        hal: &mut H,
        config: &ConsoleNetConfig,
        _stage: crate::net::NetStage,
    ) -> Result<Self, Self::Error>
    where
        H: Hardware<Error = HalError>,
        Self: Sized,
    {
        Self::new(hal, config)
    }

    fn mac(&self) -> EthernetAddress {
        self.mac
    }

    fn tx_drop_count(&self) -> u32 {
        self.tx_drops
    }

    fn name() -> &'static str
    where
        Self: Sized,
    {
        "cyw43455"
    }

    fn interface_label(&self) -> &'static str {
        "wifi"
    }

    fn bringup_status_label(&self) -> Option<&'static str> {
        deferred_join_bringup_status_label(self.deferred_join_state, self.link_up)
    }

    fn debug_snapshot(&mut self) {
        debug!(
            "[cyw43] snapshot mac={} seq={}/{} link_up={} rx={} tx={} drops={} eapol_rx={} probe={:?}",
            self.mac,
            self.sdpcm_seq,
            self.sdpcm_seq_max,
            self.link_up,
            self.rx_packets,
            self.tx_packets,
            self.tx_drops,
            self.host_eapol_rx_packets,
            self.probe
        );
    }

    fn counters(&self) -> NetDeviceCounters {
        NetDeviceCounters {
            rx_packets: self.rx_packets,
            tx_packets: self.tx_packets,
            rx_used_advances: self.rx_packets,
            tx_used_advances: self.tx_packets,
            tx_submit: self.tx_packets,
            tx_complete: self.tx_packets,
            tx_free: 1,
            tx_in_flight: 0,
            tx_double_submit: 0,
            tx_zero_len_attempt: 0,
            dropped_zero_len_tx: 0,
            tx_dup_publish_blocked: 0,
            tx_dup_used_ignored: 0,
            tx_invalid_used_state: 0,
            tx_alloc_blocked_inflight: 0,
        }
    }

    fn buffer_bounds(&self) -> Option<Range<usize>> {
        let state = self.state.as_ref() as *const Pi4WifiState as usize
            ..self.state.as_ref() as *const Pi4WifiState as usize + size_of::<Pi4WifiState>();
        let rx = self.rx_frame.as_ptr() as usize..self.rx_frame.as_ptr() as usize + FRAME_BUF_LEN;
        let tx = self.tx_frame.as_ptr() as usize..self.tx_frame.as_ptr() as usize + FRAME_BUF_LEN;
        let control = self.control_response.as_ptr() as usize
            ..self.control_response.as_ptr() as usize + CONTROL_RESPONSE_BUF_LEN;
        let start = state.start.min(rx.start).min(tx.start).min(control.start);
        let end = state.end.max(rx.end).max(tx.end).max(control.end);
        Some(start..end)
    }
}

fn validate_sdpcm_packet_len(frame: &[u8], frame_len: usize) -> Result<usize, DriverError> {
    if frame_len < SDPCM_HEADER_LEN {
        return Err(DriverError::Protocol("sdpcm-short-header"));
    }
    let raw_packet_len =
        usize::from(get_u16_le(frame, 0).ok_or(DriverError::Protocol("sdpcm-len"))?);
    if raw_packet_len > frame_len {
        return Err(DriverError::Protocol("sdpcm-len-overflow"));
    }
    let len_inv = get_u16_le(frame, 2).ok_or(DriverError::Protocol("sdpcm-len-inv"))?;
    if len_inv != !u16::try_from(raw_packet_len).unwrap_or(u16::MAX) {
        return Err(DriverError::Protocol("sdpcm-len-mismatch"));
    }
    Ok(raw_packet_len)
}

fn parse_rx_sdpcm_header(frame: &[u8], frame_len: usize) -> Result<RxSdpcmHeader, DriverError> {
    if frame_len > frame.len() {
        return Err(DriverError::Protocol("sdpcm-frame-overflow"));
    }
    let packet_len = validate_sdpcm_packet_len(frame, frame_len)?;
    let channel = frame[5] & 0x0f;
    let payload_start = usize::from(frame[7]);
    if payload_start > packet_len || payload_start < SDPCM_HEADER_LEN {
        return Err(DriverError::Protocol("sdpcm-header-length"));
    }
    Ok(RxSdpcmHeader {
        packet_len,
        payload_start,
        channel,
        credit: frame[9],
    })
}

#[inline]
fn sdpcm_glom_descriptor(frame: &[u8]) -> bool {
    frame.get(5).is_some_and(|value| (value & 0x80) != 0)
}

fn control_response_copy_len(
    response_len: usize,
    payload_available: usize,
    buffer_capacity: usize,
) -> Result<usize, DriverError> {
    if response_len > payload_available {
        return Err(DriverError::Protocol("cdc-response-truncated"));
    }
    if response_len > buffer_capacity {
        return Err(DriverError::ResponseTooLarge);
    }
    Ok(response_len)
}

#[inline]
const fn sdpcm_header_only_status_frame(header: RxSdpcmHeader) -> bool {
    header.payload_start == header.packet_len
        && matches!(header.channel, CHANNEL_CONTROL | CHANNEL_EVENT)
}

#[inline]
fn rx_frame_result_name(result: &RxFrameResult) -> &'static str {
    match result {
        RxFrameResult::None => "none",
        RxFrameResult::Control { .. } => "control",
        RxFrameResult::Event(_) => "event",
        RxFrameResult::Data(_) => "data",
    }
}

#[inline]
const fn clm_iovar_data_len(chunk_len: usize) -> usize {
    CLM_IOVAR_HEADER_LEN + chunk_len
}

#[inline]
const fn clm_setvar_payload_len(chunk_len: usize) -> usize {
    CLM_IOVAR_NAME_LEN + clm_iovar_data_len(chunk_len)
}

const fn align4(len: usize) -> usize {
    (len + 3) & !3
}

const fn control_tx_payload_offset_for_header(header_len: usize) -> usize {
    header_len + CDC_HEADER_LEN
}

const fn control_tx_sw_header_offset_for_header(header_len: usize) -> usize {
    if header_len == SDPCM_CONTROL_TX_EXT_HEADER_LEN {
        SDPCM_HEADER_LEN
    } else {
        SDPCM_HWHDR_LEN
    }
}

const fn sdpcm_control_tx_request_len(unpadded_len: usize) -> usize {
    if unpadded_len > SDPCM_CONTROL_TX_BLOCK_SIZE {
        let remainder = unpadded_len % SDPCM_CONTROL_TX_BLOCK_SIZE;
        if remainder == 0 {
            unpadded_len
        } else {
            unpadded_len + (SDPCM_CONTROL_TX_BLOCK_SIZE - remainder)
        }
    } else {
        align4(unpadded_len)
    }
}

fn write_sdpcm_control_tx_header(
    frame: &mut [u8],
    header_len: usize,
    packet_len: u16,
    unpadded_len: usize,
    tail_pad: usize,
    sdpcm_seq: u8,
) -> Result<(), DriverError> {
    if !matches!(
        header_len,
        SDPCM_CONTROL_TX_HEADER_LEN | SDPCM_CONTROL_TX_EXT_HEADER_LEN
    ) {
        return Err(DriverError::Protocol("sdpcm-control-tx-header-len"));
    }
    if frame.len() < header_len {
        return Err(DriverError::FrameTooLarge);
    }
    let tail_pad = u16::try_from(tail_pad).map_err(|_| DriverError::FrameTooLarge)?;
    let unpadded_len = u32::try_from(unpadded_len).map_err(|_| DriverError::FrameTooLarge)?;
    let packet_len_for_header = if header_len == SDPCM_CONTROL_TX_EXT_HEADER_LEN {
        packet_len
    } else {
        u16::try_from(unpadded_len).map_err(|_| DriverError::FrameTooLarge)?
    };
    let hw_header_len = u32::try_from(SDPCM_HWHDR_LEN).map_err(|_| DriverError::FrameTooLarge)?;
    let data_offset = u32::try_from(header_len).map_err(|_| DriverError::FrameTooLarge)?;
    let hwext_len = unpadded_len
        .checked_sub(hw_header_len)
        .ok_or(DriverError::FrameTooLarge)?;
    put_u16_le(frame, 0, packet_len_for_header);
    put_u16_le(frame, 2, !packet_len_for_header);
    let sw_header_offset = if header_len == SDPCM_CONTROL_TX_EXT_HEADER_LEN {
        put_u32_le(
            frame,
            SDPCM_HWHDR_LEN,
            hwext_len | SDPCM_CONTROL_TX_LAST_FRAME,
        );
        put_u32_le(frame, SDPCM_HWHDR_LEN + 4, u32::from(tail_pad) << 16);
        SDPCM_HEADER_LEN
    } else {
        control_tx_sw_header_offset_for_header(header_len)
    };
    put_u32_le(
        frame,
        sw_header_offset,
        u32::from(sdpcm_seq) | (u32::from(CHANNEL_CONTROL) << 8) | (data_offset << 24),
    );
    put_u32_le(frame, sw_header_offset + 4, 0);
    Ok(())
}

fn bdc_payload(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() < BDC_HEADER_LEN {
        return None;
    }
    if payload[0] >> BDC_VERSION_SHIFT != BDC_VERSION {
        return None;
    }
    let data_offset_words = usize::from(payload[3]);
    let start = BDC_HEADER_LEN.checked_add(data_offset_words.checked_mul(4)?)?;
    payload.get(start..)
}

fn ethernet_ethertype(packet: &[u8]) -> Option<u16> {
    if packet.len() < ETH_HEADER_LEN {
        return None;
    }
    get_u16_be(packet, 12)
}

fn parse_event_payload(payload: &[u8]) -> Result<Option<Cyw43Event>, DriverError> {
    let Some(packet) = bdc_payload(payload) else {
        trace!(
            "[cyw43] event frame ignored reason=bdc-header-unavailable len={}",
            payload.len()
        );
        return Ok(None);
    };
    if packet.len() < 72 {
        trace!(
            "[cyw43] event frame ignored reason=event-short len={}",
            packet.len()
        );
        return Ok(None);
    }
    parse_broadcom_event(packet)
}

fn parse_broadcom_event(packet: &[u8]) -> Result<Option<Cyw43Event>, DriverError> {
    if packet.len() < 72 {
        return Ok(None);
    }
    if get_u16_be(packet, 12) != Some(ETH_P_LINK_CTL)
        || get_u16_be(packet, 14) != Some(BCMILCP_SUBTYPE_VENDOR_LONG)
        || packet.get(19..22) != Some(&BROADCOM_OUI)
        || get_u16_be(packet, 22) != Some(BCMILCP_BCM_SUBTYPE_EVENT)
    {
        return Ok(None);
    }

    Ok(Some(Cyw43Event {
        flags: get_u16_be(packet, 26).ok_or(DriverError::Protocol("event-flags"))?,
        event_type: packet[31],
        status: get_u32_be(packet, 32).ok_or(DriverError::Protocol("event-status"))?,
        reason: get_u32_be(packet, 36).ok_or(DriverError::Protocol("event-reason"))?,
        auth_type: get_u32_be(packet, 40).ok_or(DriverError::Protocol("event-auth"))?,
    }))
}

const fn event_msgs_ext_payload_len() -> usize {
    EVENTMSGS_EXT_HEADER_LEN + EVENT_MASK_LEN
}

fn linux_join_event_mask() -> Result<[u8; EVENT_MASK_LEN], DriverError> {
    let mut mask = LINUX_EVENTMSGS_EXT_MASK;
    for &event in JOIN_COMPLETION_EVENTS.iter() {
        set_event_mask_bit(&mut mask, event)?;
    }
    Ok(mask)
}

fn write_event_msgs_ext_payload(
    payload: &mut [u8],
    mask: &[u8; EVENT_MASK_LEN],
) -> Result<(), DriverError> {
    if payload.len() != event_msgs_ext_payload_len() {
        return Err(DriverError::Config("event-msgs-ext-payload-len"));
    }
    payload.fill(0);
    payload[0] = EVENTMSGS_EXT_VER;
    payload[1] = EVENTMSGS_EXT_SET_MASK;
    payload[2] = EVENT_MASK_LEN as u8;
    payload[3] = EVENTMSGS_EXT_MAX_GET_SIZE;
    payload[EVENTMSGS_EXT_HEADER_LEN..].copy_from_slice(mask);
    Ok(())
}

fn set_event_mask_bit(mask: &mut [u8], event: u8) -> Result<(), DriverError> {
    let index = usize::from(event / 8);
    let bit = event % 8;
    let Some(slot) = mask.get_mut(index) else {
        return Err(DriverError::Config("wifi-event-mask-too-short"));
    };
    *slot |= 1 << bit;
    Ok(())
}

fn is_zeroed_mac(mac: &[u8; 6]) -> bool {
    mac.iter().all(|byte| *byte == 0)
}

fn firmware_mac_address_from_response(
    mac: [u8; 6],
    response_len: usize,
) -> Result<EthernetAddress, DriverError> {
    if response_len != mac.len() {
        return Err(DriverError::Protocol("cur-etheraddr-len"));
    }
    if is_zeroed_mac(&mac) {
        return Err(DriverError::Protocol("cur-etheraddr-zero"));
    }
    Ok(EthernetAddress(mac))
}

fn iovar_get_payload_len(name: &str, response_capacity: usize) -> Result<usize, DriverError> {
    name.len()
        .checked_add(1)
        .and_then(|name_len| name_len.checked_add(response_capacity))
        .ok_or(DriverError::FrameTooLarge)
}

const fn control_plane_data_clock_target_hz(recommended_data_clock_hz: u32) -> u32 {
    if recommended_data_clock_hz < SDIO_DATA_CLOCK_HZ {
        recommended_data_clock_hz
    } else {
        SDIO_DATA_CLOCK_HZ
    }
}

const fn initial_control_plane_data_clock_target_hz(
    recommended_data_clock_hz: u32,
    experimental_no_ht_transport: bool,
) -> u32 {
    if experimental_no_ht_transport {
        return if recommended_data_clock_hz < SDIO_STARTUP_CLOCK_HZ {
            recommended_data_clock_hz
        } else {
            SDIO_STARTUP_CLOCK_HZ
        };
    }

    control_plane_data_clock_target_hz(recommended_data_clock_hz)
}

#[inline]
const fn initial_control_plane_bootstrap_policy_label(
    experimental_no_ht_transport: bool,
    effective_clock_hz: u32,
) -> &'static str {
    if experimental_no_ht_transport {
        if effective_clock_hz > SDIO_STARTUP_CLOCK_HZ {
            "data-link-first-reply"
        } else {
            "startup-link-until-first-reply"
        }
    } else {
        "strict-data-link"
    }
}

#[inline]
const fn ioctl_wait_loops(
    startup_link_stabilized: bool,
    startup_link_rescue_cycles: u8,
    control_plane_probe_pending: bool,
) -> usize {
    if startup_link_stabilized && !control_plane_probe_pending {
        IOCTL_WAIT_LOOPS_STARTUP_LINK_FINAL_BOUNDED
    } else if startup_link_stabilized {
        startup_link_rescue_wait_loops(startup_link_rescue_cycles)
    } else {
        IOCTL_WAIT_LOOPS
    }
}

#[inline]
const fn startup_link_rescue_wait_loops(startup_link_rescue_cycles: u8) -> usize {
    match startup_link_rescue_cycles {
        0 => IOCTL_WAIT_LOOPS_STARTUP_LINK_STABILIZED,
        1 => IOCTL_WAIT_LOOPS_STARTUP_LINK_RESCUE,
        _ => IOCTL_WAIT_LOOPS_STARTUP_LINK_RESCUE_REPEAT,
    }
}

#[inline]
const fn has_sdpcm_credit(sdpcm_seq: u8, sdpcm_seq_max: u8) -> bool {
    sdpcm_seq != sdpcm_seq_max && (sdpcm_seq_max.wrapping_sub(sdpcm_seq) & 0x80) == 0
}

fn control_response_matches(result: &RxFrameResult, expected_cmd: u32, expected_id: u16) -> bool {
    matches!(
        result,
        RxFrameResult::Control { cmd, id, .. } if *cmd == expected_cmd && *id == expected_id
    )
}

fn put_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    if let Some(slot) = buf.get_mut(offset..offset + 2) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

fn put_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    if let Some(slot) = buf.get_mut(offset..offset + 4) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

fn get_u16_le(buf: &[u8], offset: usize) -> Option<u16> {
    let slot = buf.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([slot[0], slot[1]]))
}

fn get_u16_be(buf: &[u8], offset: usize) -> Option<u16> {
    let slot = buf.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([slot[0], slot[1]]))
}

fn get_u32_le(buf: &[u8], offset: usize) -> Option<u32> {
    let slot = buf.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slot[0], slot[1], slot[2], slot[3]]))
}

fn get_u32_be(buf: &[u8], offset: usize) -> Option<u32> {
    let slot = buf.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([slot[0], slot[1], slot[2], slot[3]]))
}

#[cfg(test)]
mod tests {
    use super::{
        align4, bdc_payload, clm_iovar_data_len, clm_setvar_payload_len,
        control_plane_bootstrap_needs_full_replay_retry, control_plane_data_clock_target_hz,
        control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait,
        control_plane_retry_after_promoted_timeout_resend_uses_startup_link,
        control_plane_retry_after_promoted_timeout_target_clock_hz,
        control_plane_retry_after_reply_wait_resend_target_clock_hz,
        control_plane_retry_after_reply_wait_uses_promoted_link,
        control_plane_retry_after_startup_link_reply_failure_target_clock_hz,
        control_response_copy_len, control_response_matches, control_tx_payload_offset_for_header,
        deferred_join_allows_rx_polling, deferred_join_bringup_status_label,
        deferred_join_requires_host_eapol, derive_wpa2_psk_pmk, event_link_state_update,
        event_msgs_ext_payload_len, firmware_mac_address_from_response,
        first_control_plane_retry_after_promoted_timeout,
        first_control_plane_retry_after_startup_link_reply_failure, has_sdpcm_credit,
        host_eapol_frame_proof, initial_control_plane_bootstrap_policy_label,
        initial_control_plane_data_clock_target_hz, ioctl_no_progress_after_nonmatching_frames,
        ioctl_wait_loops, iovar_get_payload_len, is_transport_retryable,
        join_completion_timeout_reason, join_event_result, join_iovar_fallback_allows_set_ssid,
        join_security_iovar_failure_exact_error, join_security_iovar_name,
        join_security_wpa_auth_failure_exact_error, join_security_wpa_auth_stage,
        linux_attach_control_plane_probe_order, linux_first_control_plane_iovar_order,
        linux_join_event_mask, linux_optional_iovar_allows_unsupported,
        linux_station_path_enables_apsta,
        linux_station_path_keeps_rxglom_configured_before_preinit,
        linux_station_path_keeps_txglom_configured_before_preinit,
        linux_station_path_sets_ampdu_limits_before_join,
        linux_station_path_sets_antdiv_before_join, linux_station_path_sets_country,
        linux_station_path_sets_legacy_band, linux_station_path_sets_legacy_gmode,
        linux_station_path_sets_power_mode_before_join, linux_wpa2_join_sets_mfp_without_rsn_ie,
        optional_control_plane_iovar_allows_failure, optional_txbf_allows_failure,
        parse_event_payload, parse_rx_sdpcm_header, preserve_cyw43_init_failure_exact_error,
        promoted_cyw43_init_failure_exact_error, psk_is_hex_pmk, put_u16_le,
        sdpcm_control_tx_request_len, sdpcm_glom_descriptor, sdpcm_header_only_status_frame,
        set_event_mask_bit, speculative_credit_window_after_promoted_timeout_retry,
        startup_link_ioctl_timeout_preserved_exact_error, startup_link_reply_rescue_reason,
        startup_transport_recovery_should_reset_experimental_state, validate_sdpcm_packet_len,
        write_event_msgs_ext_payload, write_legacy_set_ssid_payload,
        write_linux_bsscfg_join_payload, write_sdpcm_control_tx_header,
        write_wsec_legacy_hex_pmk_payload, write_wsec_pmk_payload,
        wsec_pmk_legacy_hex_fallback_allowed, Cyw43Event, DeferredJoinState, DriverError,
        FirmwareSupplicantPath, Ioctl, JoinCompletionRule, JoinCompletionState, RxFrameResult,
        RxSdpcmHeader, WsecPmkKind, BCME_BADARG, BCME_UNSUPPORTED, BDC_VERSION, BDC_VERSION_SHIFT,
        BSSCFG_PRIMARY_INDEX, CDC_HEADER_LEN, CHANNEL_CONTROL, CHANNEL_DATA, CHANNEL_EVENT,
        CHANNEL_GLOM, CLM_CHUNK_SIZE, EAPOL_HEADER_LEN, EAPOL_KEY_INFO_ACK,
        EAPOL_KEY_INFO_ENCRYPTED_KEY_DATA, EAPOL_KEY_INFO_INSTALL, EAPOL_KEY_INFO_KEY_TYPE,
        EAPOL_KEY_INFO_MIC, EAPOL_KEY_INFO_SECURE, EAPOL_KEY_MIN_BODY_LEN, EAPOL_PACKET_TYPE_KEY,
        ETH_HEADER_LEN, ETH_P_EAPOL, EVENTMSGS_EXT_SET_MASK, EVENTMSGS_EXT_VER, EVENT_ASSOC,
        EVENT_ASSOC_IND, EVENT_AUTH, EVENT_DISASSOC_IND, EVENT_FLAG_LINK, EVENT_IF, EVENT_LINK,
        EVENT_MASK_LEN, EVENT_MIC_ERROR, EVENT_PSK_SUP, EVENT_REASSOC, EVENT_REASSOC_IND,
        EVENT_ROAM, EVENT_SET_SSID, FRAME_BUF_LEN, HOST_EAPOL_JOIN_SUBMIT_PROOF_POLLS,
        IOCTL_NO_PROGRESS_AFTER_NONMATCHING_LIMIT, IOCTL_WAIT_LOOPS,
        IOCTL_WAIT_LOOPS_STARTUP_LINK_FINAL_BOUNDED, IOCTL_WAIT_LOOPS_STARTUP_LINK_RESCUE,
        IOCTL_WAIT_LOOPS_STARTUP_LINK_RESCUE_REPEAT, IOCTL_WAIT_LOOPS_STARTUP_LINK_STABILIZED,
        LINUX_BSSCFG_JOIN_PAYLOAD_LEN, LINUX_EXT_JOIN_ASSOC_OFFSET, LINUX_EXT_JOIN_PARAMS_LEN,
        LINUX_EXT_JOIN_SCAN_OFFSET, LINUX_EXT_JOIN_SSID_OFFSET, LINUX_REVINFO_LEN,
        SDIO_DATA_CLOCK_HZ, SDIO_STARTUP_CLOCK_HZ, SDPCM_CONTROL_TX_EXT_HEADER_LEN,
        SDPCM_CONTROL_TX_HEADER_LEN, SDPCM_HEADER_LEN, STATUS_ABORT, STATUS_NO_NETWORKS,
        STATUS_SUCCESS, STATUS_UNSOLICITED, WPA2_PSK_CCMP_RSN_IE, WPA_AUTH_WPA2_PSK,
        WPA_AUTH_WPA2_PSK_OR_UNSPECIFIED, WSEC_FLAG_PASSPHRASE, WSEC_LEGACY_HEX_PMK_LEN,
        WSEC_PMK_LEN, WSEC_PMK_PAYLOAD_LEN,
    };
    use crate::hal::HalError;

    fn unsupported_reason(err: &DriverError) -> Option<&'static str> {
        match err {
            DriverError::Hal(HalError::Unsupported(reason)) => Some(*reason),
            _ => None,
        }
    }

    #[test]
    fn align4_rounds_up() {
        assert_eq!(align4(0), 0);
        assert_eq!(align4(1), 4);
        assert_eq!(align4(4), 4);
        assert_eq!(align4(5), 8);
    }

    #[test]
    fn bdc_payload_respects_optional_offset() {
        let mut frame = [0u8; 16];
        frame[0] = BDC_VERSION << BDC_VERSION_SHIFT;
        frame[3] = 1;
        assert_eq!(bdc_payload(&frame), Some(&frame[8..]));
    }

    #[test]
    fn bdc_payload_rejects_wrong_version() {
        let mut frame = [0u8; 16];
        frame[0] = 1 << BDC_VERSION_SHIFT;
        assert_eq!(bdc_payload(&frame), None);
    }

    #[test]
    fn short_control_plane_event_frames_are_drained() {
        assert!(parse_event_payload(&[])
            .expect("empty event is drained")
            .is_none());

        let mut header_only = [0u8; 4];
        header_only[0] = BDC_VERSION << BDC_VERSION_SHIFT;
        assert!(parse_event_payload(&header_only)
            .expect("header-only event is drained")
            .is_none());
    }

    #[test]
    fn linux_startup_status_frames_are_header_only_and_drainable() {
        let header = RxSdpcmHeader {
            packet_len: SDPCM_HEADER_LEN,
            payload_start: SDPCM_HEADER_LEN,
            channel: CHANNEL_EVENT,
            credit: 0x15,
        };
        assert!(sdpcm_header_only_status_frame(header));

        let control_header = RxSdpcmHeader {
            channel: CHANNEL_CONTROL,
            ..header
        };
        assert!(sdpcm_header_only_status_frame(control_header));

        let data_header = RxSdpcmHeader {
            channel: CHANNEL_DATA,
            ..header
        };
        assert!(!sdpcm_header_only_status_frame(data_header));

        let payload_header = RxSdpcmHeader {
            packet_len: SDPCM_HEADER_LEN + CDC_HEADER_LEN,
            payload_start: SDPCM_HEADER_LEN,
            channel: CHANNEL_CONTROL,
            credit: 0x15,
        };
        assert!(!sdpcm_header_only_status_frame(payload_header));
    }

    #[test]
    fn event_constants_match_expected_values() {
        assert_eq!(EVENT_SET_SSID, 0);
        assert_eq!(EVENT_AUTH, 3);
        assert_eq!(EVENT_ASSOC, 7);
        assert_eq!(EVENT_ASSOC_IND, 8);
        assert_eq!(EVENT_REASSOC, 9);
        assert_eq!(EVENT_REASSOC_IND, 10);
        assert_eq!(EVENT_DISASSOC_IND, 12);
        assert_eq!(EVENT_LINK, 16);
        assert_eq!(EVENT_ROAM, 19);
        assert_eq!(EVENT_MIC_ERROR, 33);
        assert_eq!(EVENT_PSK_SUP, 46);
        assert_eq!(EVENT_IF, 54);
    }

    #[test]
    fn set_event_mask_bit_sets_expected_if_bit() {
        let mut mask = [0u8; 8];
        set_event_mask_bit(&mut mask, EVENT_IF).expect("event bit should fit");
        assert_eq!(mask[usize::from(EVENT_IF / 8)], 1 << (EVENT_IF % 8));
    }

    #[test]
    fn set_event_mask_bit_sets_secure_join_completion_bit() {
        let mut mask = [0u8; 8];
        set_event_mask_bit(&mut mask, EVENT_PSK_SUP).expect("PSK event bit should fit");
        assert_eq!(
            mask[usize::from(EVENT_PSK_SUP / 8)],
            1 << (EVENT_PSK_SUP % 8)
        );
    }

    #[test]
    fn event_msgs_ext_payload_uses_linux_capture_shape_plus_join_events() {
        let mask = linux_join_event_mask().expect("join event mask should fit");
        let mut payload = [0u8; 31];
        write_event_msgs_ext_payload(&mut payload, &mask).expect("event_msgs_ext payload");

        assert_eq!(event_msgs_ext_payload_len(), 31);
        assert_eq!(EVENT_MASK_LEN, 27);
        assert_eq!(payload[0], EVENTMSGS_EXT_VER);
        assert_eq!(payload[1], EVENTMSGS_EXT_SET_MASK);
        assert_eq!(payload[2], EVENT_MASK_LEN as u8);
        assert_eq!(payload[3], 0);
        assert_eq!(payload[4], 0xe9);
        assert_eq!(payload[5], 0x1f);
        assert_eq!(payload[6], 0x0b);
        assert_eq!(payload[8], 0x02);
        assert_eq!(payload[9], 0x42);
        assert_eq!(payload[27], 0x78);

        for event in [
            EVENT_SET_SSID,
            EVENT_AUTH,
            EVENT_ASSOC,
            EVENT_ASSOC_IND,
            EVENT_REASSOC,
            EVENT_REASSOC_IND,
            EVENT_DISASSOC_IND,
            EVENT_LINK,
            EVENT_PSK_SUP,
        ] {
            assert_ne!(
                mask[usize::from(event / 8)] & (1u8 << (event % 8)),
                0,
                "event {event} must be enabled"
            );
        }
    }

    #[test]
    fn set_event_mask_bit_rejects_short_mask() {
        let mut mask = [0u8; 1];
        assert!(matches!(
            set_event_mask_bit(&mut mask, EVENT_IF),
            Err(DriverError::Config("wifi-event-mask-too-short"))
        ));
    }

    #[test]
    fn secure_join_waits_for_psk_supplicant_completion() {
        let mut completion = JoinCompletionState::default();
        let set_ssid_success = Cyw43Event {
            event_type: EVENT_SET_SSID,
            status: STATUS_SUCCESS,
            ..Cyw43Event::default()
        };
        assert!(join_event_result(
            set_ssid_success,
            true,
            JoinCompletionRule::FirmwareSupplicantPskSup,
            &mut completion
        )
        .is_none());
        let mut open_completion = JoinCompletionState::default();
        assert!(matches!(
            join_event_result(
                set_ssid_success,
                false,
                JoinCompletionRule::SetSsid,
                &mut open_completion
            ),
            Some(Ok(()))
        ));

        let psk_keyed = Cyw43Event {
            event_type: EVENT_PSK_SUP,
            status: STATUS_UNSOLICITED,
            ..Cyw43Event::default()
        };
        assert!(matches!(
            join_event_result(
                psk_keyed,
                true,
                JoinCompletionRule::FirmwareSupplicantPskSup,
                &mut completion
            ),
            Some(Ok(()))
        ));
    }

    #[test]
    fn secure_join_rejects_non_completed_psk_supplicant_statuses() {
        for status in [STATUS_ABORT, 5, 7, 8] {
            let mut completion = JoinCompletionState::default();
            let event = Cyw43Event {
                event_type: EVENT_PSK_SUP,
                status,
                ..Cyw43Event::default()
            };

            assert!(matches!(
                join_event_result(
                    event,
                    true,
                    JoinCompletionRule::FirmwareSupplicantPskSup,
                    &mut completion
                ),
                Some(Err(DriverError::JoinFailed {
                    status: observed,
                    ..
                })) if observed == status
            ));
            assert!(!completion.psk_completed);
        }
    }

    #[test]
    fn secure_join_allows_psk_and_set_ssid_in_either_order() {
        let set_ssid_success = Cyw43Event {
            event_type: EVENT_SET_SSID,
            status: STATUS_SUCCESS,
            ..Cyw43Event::default()
        };
        let psk_keyed = Cyw43Event {
            event_type: EVENT_PSK_SUP,
            status: STATUS_UNSOLICITED,
            ..Cyw43Event::default()
        };
        let mut completion = JoinCompletionState::default();
        assert!(join_event_result(
            psk_keyed,
            true,
            JoinCompletionRule::FirmwareSupplicantPskSup,
            &mut completion
        )
        .is_none());
        assert!(matches!(
            join_event_result(
                set_ssid_success,
                true,
                JoinCompletionRule::FirmwareSupplicantPskSup,
                &mut completion
            ),
            Some(Ok(()))
        ));
    }

    #[test]
    fn secure_join_link_down_event_defers_dhcp_even_after_psk_sup() {
        let mut completion = JoinCompletionState::default();
        let link_down = Cyw43Event {
            event_type: EVENT_LINK,
            status: STATUS_SUCCESS,
            ..Cyw43Event::default()
        };
        let set_ssid_success = Cyw43Event {
            event_type: EVENT_SET_SSID,
            status: STATUS_SUCCESS,
            ..Cyw43Event::default()
        };
        let psk_keyed = Cyw43Event {
            event_type: EVENT_PSK_SUP,
            status: STATUS_UNSOLICITED,
            ..Cyw43Event::default()
        };
        let link_up = Cyw43Event {
            flags: EVENT_FLAG_LINK,
            event_type: EVENT_LINK,
            status: STATUS_SUCCESS,
            ..Cyw43Event::default()
        };

        assert!(join_event_result(
            link_down,
            true,
            JoinCompletionRule::FirmwareSupplicantPskSup,
            &mut completion
        )
        .is_none());
        assert!(join_event_result(
            set_ssid_success,
            true,
            JoinCompletionRule::FirmwareSupplicantPskSup,
            &mut completion
        )
        .is_none());
        assert!(join_event_result(
            psk_keyed,
            true,
            JoinCompletionRule::FirmwareSupplicantPskSup,
            &mut completion
        )
        .is_none());
        assert!(matches!(
            join_event_result(
                link_up,
                true,
                JoinCompletionRule::FirmwareSupplicantPskSup,
                &mut completion
            ),
            Some(Ok(()))
        ));
    }

    #[test]
    fn secure_join_host_eapol_rule_does_not_complete_on_set_ssid() {
        let mut completion = JoinCompletionState::default();
        let set_ssid_success = Cyw43Event {
            event_type: EVENT_SET_SSID,
            status: STATUS_SUCCESS,
            ..Cyw43Event::default()
        };
        assert!(join_event_result(
            set_ssid_success,
            true,
            JoinCompletionRule::HostEapolRequired,
            &mut completion
        )
        .is_none());
        assert!(completion.set_ssid_completed);
    }

    #[test]
    fn secure_join_completion_rule_requires_psk_sup() {
        assert_eq!(
            JoinCompletionRule::FirmwareSupplicantPskSup.label(),
            "firmware-supplicant-psk-sup"
        );
        assert_eq!(JoinCompletionRule::SetSsid.label(), "set-ssid");
        assert_eq!(
            FirmwareSupplicantPath::Unsupported.completion_rule(),
            JoinCompletionRule::HostEapolRequired
        );
        assert_eq!(
            join_completion_timeout_reason(JoinCompletionRule::HostEapolRequired),
            "host-eapol-required"
        );
    }

    #[test]
    fn host_eapol_timeout_preserves_bringup_status_label() {
        let pending = DeferredJoinState::Pending {
            completion: JoinCompletionState::default(),
            polls: 0,
            secure: true,
            completion_rule: JoinCompletionRule::HostEapolRequired,
        };
        assert_eq!(
            deferred_join_bringup_status_label(pending, false),
            Some("wifi-host-eapol-required")
        );
        assert_eq!(
            deferred_join_bringup_status_label(
                DeferredJoinState::Failed {
                    reason: "host-eapol-required",
                },
                false,
            ),
            Some("wifi-host-eapol-required")
        );
        assert_eq!(
            deferred_join_bringup_status_label(
                DeferredJoinState::Failed {
                    reason: "join-timeout",
                },
                false,
            ),
            Some("wifi-association-failed")
        );
    }

    #[test]
    fn failed_join_state_blocks_normal_rx_polling() {
        let pending = DeferredJoinState::Pending {
            completion: JoinCompletionState::default(),
            polls: 0,
            secure: true,
            completion_rule: JoinCompletionRule::HostEapolRequired,
        };
        assert!(deferred_join_allows_rx_polling(DeferredJoinState::Disabled));
        assert!(deferred_join_allows_rx_polling(pending));
        assert!(!deferred_join_allows_rx_polling(
            DeferredJoinState::Failed {
                reason: "host-eapol-required",
            }
        ));
        assert!(!deferred_join_allows_rx_polling(
            DeferredJoinState::Failed {
                reason: "join-timeout",
            }
        ));
    }

    #[test]
    fn host_eapol_terminal_state_still_allows_explicit_eapol_proof() {
        assert!(HOST_EAPOL_JOIN_SUBMIT_PROOF_POLLS <= 32);
        assert!(deferred_join_requires_host_eapol(
            DeferredJoinState::Failed {
                reason: "host-eapol-required",
            }
        ));
        assert!(!deferred_join_requires_host_eapol(
            DeferredJoinState::Failed {
                reason: "join-timeout",
            }
        ));
    }

    #[test]
    fn host_eapol_proof_classifies_pairwise_message_1() {
        let packet = host_eapol_key_packet(EAPOL_KEY_INFO_KEY_TYPE | EAPOL_KEY_INFO_ACK, 0);

        let proof = host_eapol_frame_proof(&packet);

        assert_eq!(proof.eapol_version, 2);
        assert_eq!(proof.packet_type, EAPOL_PACKET_TYPE_KEY);
        assert_eq!(proof.body_len, EAPOL_KEY_MIN_BODY_LEN as u16);
        assert!(proof.body_len_valid);
        assert_eq!(proof.descriptor_type, 2);
        assert_eq!(proof.message, "m1");
        assert_eq!(proof.key_version, 0);
        assert!(proof.replay_counter_nonzero);
    }

    #[test]
    fn host_eapol_proof_classifies_pairwise_message_3() {
        let packet = host_eapol_key_packet(
            EAPOL_KEY_INFO_KEY_TYPE
                | EAPOL_KEY_INFO_ACK
                | EAPOL_KEY_INFO_MIC
                | EAPOL_KEY_INFO_INSTALL
                | EAPOL_KEY_INFO_SECURE
                | EAPOL_KEY_INFO_ENCRYPTED_KEY_DATA
                | 2,
            24,
        );

        let proof = host_eapol_frame_proof(&packet);

        assert_eq!(proof.message, "m3");
        assert_eq!(proof.key_version, 2);
        assert_eq!(proof.key_data_len, 24);
    }

    #[test]
    fn host_eapol_proof_rejects_truncated_key_body() {
        let mut packet = host_eapol_key_packet(EAPOL_KEY_INFO_KEY_TYPE | EAPOL_KEY_INFO_ACK, 0);
        put_u16_be(
            &mut packet,
            ETH_HEADER_LEN + 2,
            EAPOL_KEY_MIN_BODY_LEN as u16 + 1,
        );

        let proof = host_eapol_frame_proof(&packet);

        assert_eq!(proof.message, "short-key");
        assert!(!proof.body_len_valid);
    }

    #[test]
    fn host_eapol_proof_rejects_short_declared_key_body() {
        let mut packet = host_eapol_key_packet(EAPOL_KEY_INFO_KEY_TYPE | EAPOL_KEY_INFO_ACK, 0);
        put_u16_be(
            &mut packet,
            ETH_HEADER_LEN + 2,
            EAPOL_KEY_MIN_BODY_LEN as u16 - 1,
        );

        let proof = host_eapol_frame_proof(&packet);

        assert_eq!(proof.message, "short-key");
        assert!(!proof.body_len_valid);
    }

    #[test]
    fn deferred_join_link_event_waits_for_join_completion_state() {
        let link_success = Cyw43Event {
            flags: EVENT_FLAG_LINK,
            event_type: EVENT_LINK,
            status: STATUS_SUCCESS,
            ..Cyw43Event::default()
        };
        assert_eq!(event_link_state_update(link_success, false), Some(true));
        assert_eq!(event_link_state_update(link_success, true), Some(false));

        let link_down = Cyw43Event {
            event_type: EVENT_LINK,
            status: STATUS_SUCCESS,
            ..Cyw43Event::default()
        };
        assert_eq!(event_link_state_update(link_down, false), Some(false));
    }

    fn host_eapol_key_packet(
        key_info: u16,
        key_data_len: u16,
    ) -> [u8; ETH_HEADER_LEN + EAPOL_HEADER_LEN + EAPOL_KEY_MIN_BODY_LEN] {
        let mut packet = [0u8; ETH_HEADER_LEN + EAPOL_HEADER_LEN + EAPOL_KEY_MIN_BODY_LEN];
        packet[0..6].copy_from_slice(&[0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10]);
        packet[6..12].copy_from_slice(&[0x02, 0x43, 0x4f, 0x48, 0x58, 0x55]);
        put_u16_be(&mut packet, 12, ETH_P_EAPOL);
        packet[ETH_HEADER_LEN] = 2;
        packet[ETH_HEADER_LEN + 1] = EAPOL_PACKET_TYPE_KEY;
        put_u16_be(
            &mut packet,
            ETH_HEADER_LEN + 2,
            EAPOL_KEY_MIN_BODY_LEN as u16,
        );
        let body = ETH_HEADER_LEN + EAPOL_HEADER_LEN;
        packet[body] = 2;
        put_u16_be(&mut packet, body + 1, key_info);
        packet[body + 12] = 1;
        put_u16_be(&mut packet, body + 93, key_data_len);
        packet
    }

    fn put_u16_be(buf: &mut [u8], offset: usize, value: u16) {
        if let Some(slot) = buf.get_mut(offset..offset + 2) {
            slot.copy_from_slice(&value.to_be_bytes());
        }
    }

    #[test]
    fn join_event_reports_no_networks_status() {
        let mut completion = JoinCompletionState::default();
        let no_networks = Cyw43Event {
            event_type: EVENT_SET_SSID,
            status: STATUS_NO_NETWORKS,
            ..Cyw43Event::default()
        };
        assert!(matches!(
            join_event_result(
                no_networks,
                true,
                JoinCompletionRule::FirmwareSupplicantPskSup,
                &mut completion
            ),
            Some(Err(DriverError::JoinFailed { status, .. })) if status == STATUS_NO_NETWORKS
        ));
    }

    #[test]
    fn clm_chunk_shape_matches_pi4_linux_capture() {
        assert_eq!(CLM_CHUNK_SIZE, 1400);
        assert_eq!(clm_iovar_data_len(CLM_CHUNK_SIZE), 1412);
        assert_eq!(clm_setvar_payload_len(CLM_CHUNK_SIZE), 1420);
        assert_eq!(clm_iovar_data_len(2676 - CLM_CHUNK_SIZE), 1288);
        assert_eq!(clm_setvar_payload_len(2676 - CLM_CHUNK_SIZE), 1296);
    }

    #[test]
    fn first_linux_iovar_uses_plain_sdpcm_header_before_rxglom() {
        let payload_len = "bus:txglomalign".len() + 1 + 4;
        let unpadded_len = SDPCM_CONTROL_TX_HEADER_LEN + CDC_HEADER_LEN + payload_len;
        let request_len = sdpcm_control_tx_request_len(unpadded_len);
        let tail_pad = request_len - unpadded_len;
        assert_eq!(unpadded_len, 48);
        assert_eq!(request_len, 48);
        assert_eq!(tail_pad, 0);

        let mut frame = [0u8; FRAME_BUF_LEN];
        write_sdpcm_control_tx_header(
            &mut frame,
            SDPCM_CONTROL_TX_HEADER_LEN,
            u16::try_from(request_len).expect("request length fits"),
            unpadded_len,
            tail_pad,
            0,
        )
        .expect("header writes");
        assert_eq!(
            &frame[..SDPCM_CONTROL_TX_HEADER_LEN],
            &[0x30, 0x00, 0xcf, 0xff, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x00],
        );
        assert_eq!(
            control_tx_payload_offset_for_header(SDPCM_CONTROL_TX_HEADER_LEN),
            SDPCM_HEADER_LEN + CDC_HEADER_LEN
        );
    }

    #[test]
    fn linux_iovar_get_request_len_includes_name_and_response_buffer() {
        assert_eq!(
            iovar_get_payload_len("cur_etheraddr", 6).expect("cur_etheraddr request fits"),
            20
        );
        assert_eq!(
            iovar_get_payload_len("ulp_sdioctrl", 16).expect("ulp_sdioctrl request fits"),
            29
        );
        assert_eq!(
            iovar_get_payload_len("event_msgs", EVENT_MASK_LEN).expect("event mask request fits"),
            38
        );
        assert_eq!(
            iovar_get_payload_len("clmver", 256).expect("clmver request fits"),
            263
        );
    }

    #[test]
    fn extended_control_tx_header_shape_matches_pi4_linux_clm_capture() {
        let payload_len = clm_setvar_payload_len(CLM_CHUNK_SIZE);
        let unpadded_len = SDPCM_CONTROL_TX_EXT_HEADER_LEN + CDC_HEADER_LEN + payload_len;
        let request_len = sdpcm_control_tx_request_len(unpadded_len);
        let tail_pad = request_len - unpadded_len;
        assert_eq!(unpadded_len, 1456);
        assert_eq!(request_len, 1536);
        assert_eq!(tail_pad, 80);

        let mut frame = [0u8; FRAME_BUF_LEN];
        write_sdpcm_control_tx_header(
            &mut frame,
            SDPCM_CONTROL_TX_EXT_HEADER_LEN,
            u16::try_from(request_len).expect("request length fits"),
            unpadded_len,
            tail_pad,
            4,
        )
        .expect("header writes");
        assert_eq!(
            &frame[..SDPCM_CONTROL_TX_EXT_HEADER_LEN],
            &[
                0x00, 0x06, 0xff, 0xf9, 0xac, 0x05, 0x00, 0x01, 0x00, 0x00, 0x50, 0x00, 0x04, 0x00,
                0x00, 0x14, 0x00, 0x00, 0x00, 0x00,
            ],
        );
        assert_eq!(
            control_tx_payload_offset_for_header(SDPCM_CONTROL_TX_EXT_HEADER_LEN),
            SDPCM_CONTROL_TX_EXT_HEADER_LEN + CDC_HEADER_LEN
        );
    }

    #[test]
    fn little_endian_helpers_write_expected_bytes() {
        let mut buf = [0u8; 4];
        put_u16_le(&mut buf, 1, 0x1234);
        assert_eq!(buf, [0x00, 0x34, 0x12, 0x00]);
    }

    #[test]
    fn sdpcm_len_validation_rejects_raw_packet_larger_than_received_frame() {
        let mut frame = [0u8; 64];
        put_u16_le(&mut frame, 0, 48);
        put_u16_le(&mut frame, 2, !48u16);
        assert_eq!(
            validate_sdpcm_packet_len(&frame, 48).expect("valid frame"),
            48
        );

        put_u16_le(&mut frame, 0, 64);
        put_u16_le(&mut frame, 2, !48u16);
        let err = validate_sdpcm_packet_len(&frame, 48).expect_err("overflow rejected");
        assert!(matches!(err, DriverError::Protocol("sdpcm-len-overflow")));
    }

    #[test]
    fn rx_sdpcm_header_matches_linux_clm_reply_shape() {
        let mut frame = [0u8; FRAME_BUF_LEN];
        frame[..12].copy_from_slice(&[
            0xa8, 0x05, 0x57, 0xfa, 0x06, 0x00, 0x00, 0x0c, 0x00, 0x19, 0x00, 0x00,
        ]);
        let header = parse_rx_sdpcm_header(&frame, 0x05a8).expect("linux clm rx header parses");
        assert_eq!(header.packet_len, 0x05a8);
        assert_eq!(header.payload_start, 12);
        assert_eq!(header.channel, CHANNEL_CONTROL);
        assert_eq!(header.credit, 0x19);
    }

    #[test]
    fn rx_sdpcm_header_recognizes_linux_glom_descriptor_channel() {
        let mut frame = [0u8; 16];
        put_u16_le(&mut frame, 0, 16);
        put_u16_le(&mut frame, 2, !16u16);
        frame[5] = CHANNEL_GLOM | 0x80;
        frame[7] = SDPCM_HEADER_LEN as u8;

        let header = parse_rx_sdpcm_header(&frame, 16).expect("glom header parses");

        assert_eq!(header.channel, CHANNEL_GLOM);
        assert!(sdpcm_glom_descriptor(&frame));
    }

    #[test]
    fn rx_sdpcm_header_rejects_hal_length_past_buffer() {
        let mut frame = [0u8; 16];
        put_u16_le(&mut frame, 0, 16);
        put_u16_le(&mut frame, 2, !16u16);
        let err = parse_rx_sdpcm_header(&frame, 17).expect_err("oversized HAL length rejected");
        assert!(matches!(err, DriverError::Protocol("sdpcm-frame-overflow")));
    }

    #[test]
    fn control_response_copy_rejects_truncation_and_oversize() {
        assert_eq!(
            control_response_copy_len(6, 6, 8).expect("exact payload fits"),
            6
        );
        assert!(matches!(
            control_response_copy_len(6, 5, 8),
            Err(DriverError::Protocol("cdc-response-truncated"))
        ));
        assert!(matches!(
            control_response_copy_len(9, 9, 8),
            Err(DriverError::ResponseTooLarge)
        ));
    }

    #[test]
    fn control_response_matching_requires_command_and_id() {
        let response = RxFrameResult::Control {
            cmd: Ioctl::SetVar as u32,
            id: 7,
            status: STATUS_SUCCESS,
            response_len: 0,
        };
        assert!(control_response_matches(&response, Ioctl::SetVar as u32, 7));
        assert!(!control_response_matches(
            &response,
            Ioctl::GetVar as u32,
            7
        ));
        assert!(!control_response_matches(
            &response,
            Ioctl::SetVar as u32,
            8
        ));
    }

    #[test]
    fn txbf_optional_skip_is_limited_to_unsupported_status() {
        assert!(optional_txbf_allows_failure(&DriverError::IoctlFailed {
            cmd: Ioctl::SetVar as u32,
            status: BCME_UNSUPPORTED,
        }));
        assert!(!optional_txbf_allows_failure(&DriverError::IoctlFailed {
            cmd: Ioctl::SetVar as u32,
            status: 1,
        }));
        assert!(!optional_txbf_allows_failure(&DriverError::Hal(
            HalError::Unsupported("sdhci-command-error")
        )));
    }

    #[test]
    fn transport_retry_matches_enumeration_failures() {
        assert!(is_transport_retryable(&HalError::Unsupported(
            "sdhci-command-error"
        )));
        assert!(is_transport_retryable(&HalError::Unsupported(
            "sdio-ocr-timeout"
        )));
        assert!(!is_transport_retryable(&HalError::Unsupported(
            "mailbox-protocol"
        )));
    }

    #[test]
    fn control_plane_data_clock_target_respects_runtime_cap() {
        assert_eq!(SDIO_DATA_CLOCK_HZ, 50_000_000);
        assert_eq!(control_plane_data_clock_target_hz(400_000), 400_000);
        assert_eq!(
            control_plane_data_clock_target_hz(SDIO_DATA_CLOCK_HZ),
            SDIO_DATA_CLOCK_HZ
        );
        assert_eq!(
            control_plane_data_clock_target_hz(SDIO_DATA_CLOCK_HZ * 2),
            SDIO_DATA_CLOCK_HZ
        );
    }

    #[test]
    fn initial_control_plane_data_clock_uses_startup_link_for_bounded_no_ht_write() {
        assert_eq!(
            initial_control_plane_data_clock_target_hz(SDIO_DATA_CLOCK_HZ, true),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            initial_control_plane_data_clock_target_hz(400_000, true),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            initial_control_plane_data_clock_target_hz(SDIO_DATA_CLOCK_HZ, false),
            SDIO_DATA_CLOCK_HZ
        );
    }

    #[test]
    fn linux_first_control_plane_iovar_order_precedes_clm() {
        assert_eq!(
            linux_first_control_plane_iovar_order(),
            ["bus:txglomalign", "ulp_sdioctrl", "bus:rxglom"]
        );
        assert_eq!(
            linux_attach_control_plane_probe_order(),
            [
                "bus:txglomalign",
                "ulp_sdioctrl",
                "bus:rxglom",
                "cur_etheraddr",
                "revinfo"
            ]
        );
        assert_eq!(Ioctl::GetRevInfo as u32, 98);
        assert_eq!(LINUX_REVINFO_LEN, 68);
    }

    #[test]
    fn linux_sdio_preinit_glom_state_is_preserved_until_preinit_mpc() {
        assert!(linux_station_path_keeps_txglom_configured_before_preinit());
        assert!(linux_station_path_keeps_rxglom_configured_before_preinit());
    }

    #[test]
    fn linux_optional_ulp_sdioctrl_accepts_unsupported_status() {
        assert!(linux_optional_iovar_allows_unsupported(
            "ulp_sdioctrl",
            Ioctl::GetVar as u32,
            BCME_UNSUPPORTED
        ));
        assert!(!linux_optional_iovar_allows_unsupported(
            "bus:rxglom",
            Ioctl::GetVar as u32,
            BCME_UNSUPPORTED
        ));
        assert!(!linux_optional_iovar_allows_unsupported(
            "ulp_sdioctrl",
            Ioctl::SetVar as u32,
            BCME_UNSUPPORTED
        ));
        assert!(!linux_optional_iovar_allows_unsupported(
            "ulp_sdioctrl",
            Ioctl::GetVar as u32,
            STATUS_SUCCESS
        ));
    }

    #[test]
    fn optional_non_captured_iovars_accept_only_firmware_unsupported() {
        assert!(optional_control_plane_iovar_allows_failure(
            "bus:txglom",
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_UNSUPPORTED,
            }
        ));
        assert!(optional_control_plane_iovar_allows_failure(
            "ampdu_ba_wsize",
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_UNSUPPORTED,
            }
        ));
        assert!(optional_control_plane_iovar_allows_failure(
            "ampdu_mpdu",
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_UNSUPPORTED,
            }
        ));
        assert!(!optional_control_plane_iovar_allows_failure(
            "country",
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_UNSUPPORTED,
            }
        ));
        assert!(!optional_control_plane_iovar_allows_failure(
            "apsta",
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_UNSUPPORTED,
            }
        ));
        assert!(!optional_control_plane_iovar_allows_failure(
            "bsscfg:event_msgs",
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_UNSUPPORTED,
            }
        ));
        assert!(!optional_control_plane_iovar_allows_failure(
            "bsscfg:sup_wpa",
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_UNSUPPORTED,
            }
        ));
        assert!(!optional_control_plane_iovar_allows_failure(
            "bus:txglomalign",
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_UNSUPPORTED,
            }
        ));
        assert!(!optional_control_plane_iovar_allows_failure(
            "bus:txglom",
            &DriverError::Hal(HalError::Unsupported("sdio-cmd53-r5-error"))
        ));
    }

    #[test]
    fn linux_join_pref_payload_matches_capture() {
        assert_eq!(
            super::LINUX_JOIN_PREF_DEFAULT,
            [0x04, 0x02, 0x08, 0x01, 0x01, 0x02, 0x00, 0x00]
        );
    }

    #[test]
    fn linux_primary_bsscfg_join_payload_matches_brcmfmac_unpinned_join_shape() {
        let mut payload = [0xa5u8; LINUX_BSSCFG_JOIN_PAYLOAD_LEN];
        write_linux_bsscfg_join_payload(&mut payload, "cohesix")
            .expect("linux bsscfg join payload should encode");

        assert_eq!(
            &payload[LINUX_EXT_JOIN_SSID_OFFSET..LINUX_EXT_JOIN_SSID_OFFSET + 4],
            &7u32.to_le_bytes()
        );
        assert_eq!(
            &payload[LINUX_EXT_JOIN_SSID_OFFSET + 4..LINUX_EXT_JOIN_SSID_OFFSET + 11],
            b"cohesix"
        );
        assert_eq!(payload[LINUX_EXT_JOIN_SCAN_OFFSET], 0xff);
        assert_eq!(
            &payload[LINUX_EXT_JOIN_SCAN_OFFSET + 4..LINUX_EXT_JOIN_SCAN_OFFSET + 8],
            &u32::MAX.to_le_bytes()
        );
        assert_eq!(
            &payload[LINUX_EXT_JOIN_ASSOC_OFFSET..LINUX_EXT_JOIN_ASSOC_OFFSET + 6],
            &[0xff; 6]
        );
        assert_eq!(
            &payload[LINUX_EXT_JOIN_ASSOC_OFFSET + 8..LINUX_EXT_JOIN_ASSOC_OFFSET + 12],
            &0u32.to_le_bytes()
        );
    }

    #[test]
    fn legacy_set_ssid_payload_remains_linux_fallback_shape() {
        let mut payload = [0xa5u8; 36];
        write_legacy_set_ssid_payload(&mut payload, "cohesix")
            .expect("legacy set ssid payload should encode");

        assert_eq!(&payload[0..4], &7u32.to_le_bytes());
        assert_eq!(&payload[4..11], b"cohesix");
        assert!(payload[11..].iter().copied().all(|byte| byte == 0));
    }

    #[test]
    fn secure_join_uses_linux_wpa2_auth_masks_in_order() {
        assert_eq!(WPA_AUTH_WPA2_PSK_OR_UNSPECIFIED, 0x00c0);
        assert_eq!(WPA_AUTH_WPA2_PSK, 0x0080);
        assert!(!linux_wpa2_join_sets_mfp_without_rsn_ie());
    }

    #[test]
    fn secure_join_rsn_ie_matches_wpa2_psk_ccmp_shape() {
        assert_eq!(WPA2_PSK_CCMP_RSN_IE.len(), 22);
        assert_eq!(&WPA2_PSK_CCMP_RSN_IE[0..2], &[0x30, 0x14]);
        assert_eq!(&WPA2_PSK_CCMP_RSN_IE[2..4], &1u16.to_le_bytes());
        assert_eq!(&WPA2_PSK_CCMP_RSN_IE[4..8], &[0x00, 0x0f, 0xac, 0x04]);
        assert_eq!(&WPA2_PSK_CCMP_RSN_IE[10..14], &[0x00, 0x0f, 0xac, 0x04]);
        assert_eq!(&WPA2_PSK_CCMP_RSN_IE[16..20], &[0x00, 0x0f, 0xac, 0x02]);
    }

    #[test]
    fn join_security_iovar_logging_covers_current_gate_names() {
        for name in [
            "auth",
            "wpaie",
            "wsec",
            "wpa_auth",
            "sup_wpa",
            "sup_wpa2_eapver",
            "sup_wpa_tmo",
            "bsscfg:sup_wpa",
            "bsscfg:sup_wpa2_eapver",
            "bsscfg:sup_wpa_tmo",
        ] {
            assert!(join_security_iovar_name(name));
        }
        assert!(!join_security_iovar_name("join_pref"));
        assert!(!join_security_iovar_name("event_msgs_ext"));
    }

    #[test]
    fn wsec_ioctl_timeout_preserves_join_security_gate() {
        assert_eq!(
            join_security_iovar_failure_exact_error(
                "wsec",
                &DriverError::Protocol("ioctl-timeout"),
            ),
            Some("cyw43-join-security-wsec-first-loop")
        );
        assert_eq!(
            join_security_iovar_failure_exact_error(
                "wsec",
                &DriverError::Protocol("ioctl-no-progress-after-frame"),
            ),
            Some("cyw43-join-security-wsec-first-loop")
        );
        assert_eq!(
            join_security_iovar_failure_exact_error(
                "wpa_auth",
                &DriverError::Protocol("ioctl-timeout"),
            ),
            None
        );
    }

    #[test]
    fn wpa_auth_ioctl_timeout_preserves_join_security_stage_gate() {
        assert_eq!(
            join_security_wpa_auth_stage(WPA_AUTH_WPA2_PSK_OR_UNSPECIFIED),
            "initial"
        );
        assert_eq!(join_security_wpa_auth_stage(WPA_AUTH_WPA2_PSK), "final");
        assert_eq!(
            join_security_wpa_auth_failure_exact_error(
                WPA_AUTH_WPA2_PSK_OR_UNSPECIFIED,
                &DriverError::Protocol("ioctl-no-progress-after-frame"),
            ),
            Some("cyw43-join-security-wpa-auth-initial-loop")
        );
        assert_eq!(
            join_security_wpa_auth_failure_exact_error(
                WPA_AUTH_WPA2_PSK,
                &DriverError::Protocol("ioctl-timeout"),
            ),
            Some("cyw43-join-security-wpa-auth-final-loop")
        );
    }

    #[test]
    fn wpaie_ioctl_timeout_preserves_join_security_gate() {
        assert_eq!(
            join_security_iovar_failure_exact_error(
                "wpaie",
                &DriverError::Protocol("ioctl-no-progress-after-frame"),
            ),
            Some("cyw43-join-security-wpaie-loop")
        );
    }

    #[test]
    fn bsscfg_supplicant_wrapper_timeout_preserves_join_security_gate() {
        assert_eq!(
            join_security_iovar_failure_exact_error(
                "sup_wpa",
                &DriverError::Protocol("ioctl-no-progress-after-frame"),
            ),
            Some("cyw43-join-security-sup-wpa-loop")
        );
        assert_eq!(
            join_security_iovar_failure_exact_error(
                "bsscfg:sup_wpa",
                &DriverError::Protocol("ioctl-no-progress-after-frame"),
            ),
            Some("cyw43-join-security-bsscfg-sup-wpa-loop")
        );
        assert_eq!(
            FirmwareSupplicantPath::BsscfgWrapper.label(),
            "bsscfg-wrapper"
        );
        assert_eq!(
            FirmwareSupplicantPath::BsscfgWrapper.order_label(),
            "sup_wpa-or-bsscfg_sup_wpa"
        );
    }

    #[test]
    fn join_iovar_fallback_excludes_transport_failures() {
        assert!(join_iovar_fallback_allows_set_ssid(
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_UNSUPPORTED,
            },
        ));
        assert!(join_iovar_fallback_allows_set_ssid(
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_BADARG,
            },
        ));
        assert!(!join_iovar_fallback_allows_set_ssid(
            &DriverError::Protocol("ioctl-no-progress-after-frame"),
        ));
        assert!(!join_iovar_fallback_allows_set_ssid(
            &DriverError::Protocol("ioctl-timeout"),
        ));
    }

    #[test]
    fn ioctl_no_progress_after_nonmatching_frame_is_bounded() {
        assert!(!ioctl_no_progress_after_nonmatching_frames(
            0,
            IOCTL_NO_PROGRESS_AFTER_NONMATCHING_LIMIT
        ));
        assert!(!ioctl_no_progress_after_nonmatching_frames(
            1,
            IOCTL_NO_PROGRESS_AFTER_NONMATCHING_LIMIT - 1
        ));
        assert!(ioctl_no_progress_after_nonmatching_frames(
            1,
            IOCTL_NO_PROGRESS_AFTER_NONMATCHING_LIMIT
        ));
    }

    #[test]
    fn linux_station_attach_does_not_enable_apsta() {
        assert!(!linux_station_path_enables_apsta());
    }

    #[test]
    fn linux_station_attach_does_not_set_country() {
        assert!(!linux_station_path_sets_country());
    }

    #[test]
    fn linux_station_attach_skips_legacy_gmode_and_band() {
        assert!(!linux_station_path_sets_legacy_gmode());
        assert!(!linux_station_path_sets_legacy_band());
    }

    #[test]
    fn linux_station_attach_skips_uncaptured_tail_writes_before_join() {
        assert!(!linux_station_path_sets_antdiv_before_join());
        assert!(!linux_station_path_sets_ampdu_limits_before_join());
        assert!(!linux_station_path_sets_power_mode_before_join());
    }

    #[test]
    fn cur_etheraddr_is_mandatory_linux_attach_proof() {
        assert_eq!(
            firmware_mac_address_from_response([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10], 6)
                .expect("valid firmware MAC"),
            smoltcp::wire::EthernetAddress([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10])
        );
        assert!(matches!(
            firmware_mac_address_from_response([0; 6], 6),
            Err(DriverError::Protocol("cur-etheraddr-zero"))
        ));
        assert!(matches!(
            firmware_mac_address_from_response([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10], 4),
            Err(DriverError::Protocol("cur-etheraddr-len"))
        ));
        assert!(matches!(
            firmware_mac_address_from_response([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10], 8),
            Err(DriverError::Protocol("cur-etheraddr-len"))
        ));
    }

    #[test]
    fn wpa2_psk_pbkdf2_matches_ieee_vector() {
        let mut pmk = [0u8; WSEC_PMK_LEN];
        derive_wpa2_psk_pmk(b"password", b"IEEE", &mut pmk);
        assert_eq!(
            pmk,
            [
                0xf4, 0x2c, 0x6f, 0xc5, 0x2d, 0xf0, 0xeb, 0xef, 0x9e, 0xbb, 0x4b, 0x90, 0xb3, 0x8a,
                0x5f, 0x90, 0x2e, 0x83, 0xfe, 0x1b, 0x13, 0x5a, 0x70, 0xe2, 0x3a, 0xed, 0x76, 0x2e,
                0x97, 0x10, 0xa1, 0x2e,
            ]
        );
    }

    #[test]
    fn wsec_pmk_payload_derives_passphrase_into_linux_pmk_shape() {
        let mut payload = [0xa5u8; WSEC_PMK_PAYLOAD_LEN];
        let kind = write_wsec_pmk_payload(&mut payload, b"IEEE", b"password")
            .expect("passphrase pmk payload should encode");

        assert_eq!(kind, WsecPmkKind::Pbkdf2Passphrase);
        assert_eq!(&payload[0..2], &(WSEC_PMK_LEN as u16).to_le_bytes());
        assert_eq!(&payload[2..4], &0u16.to_le_bytes());
        assert_eq!(
            &payload[4..4 + WSEC_PMK_LEN],
            &[
                0xf4, 0x2c, 0x6f, 0xc5, 0x2d, 0xf0, 0xeb, 0xef, 0x9e, 0xbb, 0x4b, 0x90, 0xb3, 0x8a,
                0x5f, 0x90, 0x2e, 0x83, 0xfe, 0x1b, 0x13, 0x5a, 0x70, 0xe2, 0x3a, 0xed, 0x76, 0x2e,
                0x97, 0x10, 0xa1, 0x2e,
            ]
        );
        assert!(payload[4 + WSEC_PMK_LEN..]
            .iter()
            .copied()
            .all(|byte| byte == 0));
    }

    #[test]
    fn wsec_pmk_payload_decodes_hex_psk_without_passphrase_flag() {
        let mut payload = [0u8; WSEC_PMK_PAYLOAD_LEN];
        let kind = write_wsec_pmk_payload(
            &mut payload,
            b"cohesix",
            b"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        )
        .expect("hex pmk payload should encode");

        assert_eq!(kind, WsecPmkKind::HexPmk);
        assert_eq!(&payload[0..2], &(WSEC_PMK_LEN as u16).to_le_bytes());
        assert_eq!(&payload[2..4], &0u16.to_le_bytes());
        assert_eq!(
            &payload[4..4 + WSEC_PMK_LEN],
            &[
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
                0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
                0x1c, 0x1d, 0x1e, 0x1f,
            ]
        );
    }

    #[test]
    fn wsec_legacy_hex_pmk_payload_matches_old_brcmfmac_shape() {
        let mut payload = [0xa5u8; WSEC_PMK_PAYLOAD_LEN];
        write_wsec_legacy_hex_pmk_payload(&mut payload, b"IEEE", b"password")
            .expect("fallback legacy hex pmk payload should encode");

        assert_eq!(
            &payload[0..2],
            &(WSEC_LEGACY_HEX_PMK_LEN as u16).to_le_bytes()
        );
        assert_eq!(&payload[2..4], &WSEC_FLAG_PASSPHRASE.to_le_bytes());
        assert_eq!(
            &payload[4..4 + WSEC_LEGACY_HEX_PMK_LEN],
            b"f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e"
        );
        assert!(payload[4 + WSEC_LEGACY_HEX_PMK_LEN..]
            .iter()
            .copied()
            .all(|byte| byte == 0));
    }

    #[test]
    fn wsec_legacy_hex_fallback_accepts_passphrase_and_hex_pmk() {
        assert!(wsec_pmk_legacy_hex_fallback_allowed(b"F33dM3!W00f!"));
        assert!(psk_is_hex_pmk(
            b"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
        ));
        assert!(wsec_pmk_legacy_hex_fallback_allowed(
            b"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
        ));
        assert!(!wsec_pmk_legacy_hex_fallback_allowed(b"short"));
    }

    #[test]
    fn primary_bsscfg_zero_uses_plain_iovar_payloads() {
        assert_eq!(BSSCFG_PRIMARY_INDEX, 0);
        assert_eq!(LINUX_EXT_JOIN_SSID_OFFSET, 0);
        assert_eq!(LINUX_BSSCFG_JOIN_PAYLOAD_LEN, LINUX_EXT_JOIN_PARAMS_LEN);
    }

    #[test]
    fn secure_firmware_supplicant_unsupported_is_not_optional() {
        assert!(!optional_control_plane_iovar_allows_failure(
            "bsscfg:sup_wpa",
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_UNSUPPORTED,
            }
        ));
        assert!(!optional_control_plane_iovar_allows_failure(
            "bsscfg:sup_wpa",
            &DriverError::Hal(HalError::Unsupported("sdio-cmd53-r5-error"))
        ));
        assert!(optional_control_plane_iovar_allows_failure(
            "sup_wpa",
            &DriverError::IoctlFailed {
                cmd: Ioctl::SetVar as u32,
                status: BCME_UNSUPPORTED,
            }
        ));
    }

    #[test]
    fn initial_control_plane_bootstrap_policy_reports_startup_link_hold() {
        assert_eq!(
            initial_control_plane_bootstrap_policy_label(true, SDIO_STARTUP_CLOCK_HZ),
            "startup-link-until-first-reply"
        );
        assert_eq!(
            initial_control_plane_bootstrap_policy_label(true, SDIO_DATA_CLOCK_HZ),
            "data-link-first-reply"
        );
        assert_eq!(
            initial_control_plane_bootstrap_policy_label(false, SDIO_DATA_CLOCK_HZ),
            "strict-data-link"
        );
    }

    #[test]
    fn first_control_plane_retry_after_promoted_timeout_is_precise() {
        assert!(first_control_plane_retry_after_promoted_timeout(
            true,
            true,
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-promoted-rearm-timeout"
            )),
        ));
        assert!(!first_control_plane_retry_after_promoted_timeout(
            false,
            true,
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-promoted-rearm-timeout"
            )),
        ));
        assert!(!first_control_plane_retry_after_promoted_timeout(
            true,
            false,
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-promoted-rearm-timeout"
            )),
        ));
        assert!(!first_control_plane_retry_after_promoted_timeout(
            true,
            true,
            &DriverError::Hal(HalError::Unsupported("ioctl-timeout")),
        ));
    }

    #[test]
    fn promoted_timeout_retry_targets_startup_clock() {
        assert_eq!(
            control_plane_retry_after_promoted_timeout_target_clock_hz(
                true,
                true,
                SDIO_DATA_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-promoted-rearm-timeout"
                )),
            ),
            Some(SDIO_STARTUP_CLOCK_HZ)
        );
        assert_eq!(
            control_plane_retry_after_promoted_timeout_target_clock_hz(
                true,
                true,
                SDIO_STARTUP_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-promoted-rearm-timeout"
                )),
            ),
            Some(SDIO_STARTUP_CLOCK_HZ)
        );
        assert_eq!(
            control_plane_retry_after_promoted_timeout_target_clock_hz(
                false,
                true,
                SDIO_DATA_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-promoted-rearm-timeout"
                )),
            ),
            None
        );
    }

    #[test]
    fn startup_link_reply_failure_retry_stays_startup_safe_until_f2_succeeds() {
        assert_eq!(
            control_plane_retry_after_startup_link_reply_failure_target_clock_hz(
                true,
                true,
                SDIO_STARTUP_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready"
                )),
            ),
            Some(SDIO_STARTUP_CLOCK_HZ)
        );
        assert_eq!(
            control_plane_retry_after_startup_link_reply_failure_target_clock_hz(
                true,
                true,
                SDIO_DATA_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready"
                )),
            ),
            Some(SDIO_STARTUP_CLOCK_HZ)
        );
        assert_eq!(
            control_plane_retry_after_startup_link_reply_failure_target_clock_hz(
                true,
                true,
                SDIO_DATA_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready"
                )),
            ),
            Some(SDIO_STARTUP_CLOCK_HZ)
        );
        assert_eq!(
            control_plane_retry_after_startup_link_reply_failure_target_clock_hz(
                true,
                true,
                SDIO_DATA_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-no-reply-linux-f2-armed"
                )),
            ),
            Some(SDIO_STARTUP_CLOCK_HZ)
        );
        assert_eq!(
            control_plane_retry_after_startup_link_reply_failure_target_clock_hz(
                true,
                true,
                SDIO_STARTUP_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-sideband-unreadable"
                )),
            ),
            Some(SDIO_STARTUP_CLOCK_HZ)
        );
        assert_eq!(
            control_plane_retry_after_startup_link_reply_failure_target_clock_hz(
                true,
                true,
                SDIO_DATA_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-linux-interrupts-deferred"
                )),
            ),
            Some(SDIO_STARTUP_CLOCK_HZ)
        );
        assert_eq!(
            control_plane_retry_after_startup_link_reply_failure_target_clock_hz(
                true,
                true,
                SDIO_STARTUP_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-passive-startup-link-timeout"
                )),
            ),
            Some(SDIO_STARTUP_CLOCK_HZ)
        );
        assert_eq!(
            control_plane_retry_after_startup_link_reply_failure_target_clock_hz(
                true,
                true,
                SDIO_DATA_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-startup-link-reply-timeout"
                )),
            ),
            Some(SDIO_STARTUP_CLOCK_HZ)
        );
        assert_eq!(
            control_plane_retry_after_startup_link_reply_failure_target_clock_hz(
                false,
                true,
                SDIO_DATA_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready"
                )),
            ),
            None
        );
    }

    #[test]
    fn control_plane_bootstrap_replay_retry_stays_closed_for_first_reply_failures() {
        assert!(!control_plane_bootstrap_needs_full_replay_retry(
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-pure-f2-startup-link-no-reply"
            ))
        ));
        assert!(!control_plane_bootstrap_needs_full_replay_retry(
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-sideband-read-stall-no-buffer-ready"
            ))
        ));
        assert!(!control_plane_bootstrap_needs_full_replay_retry(
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready"
            ))
        ));
        assert!(!control_plane_bootstrap_needs_full_replay_retry(
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-hintless-firstread-no-irq"
            ))
        ));
        assert!(!control_plane_bootstrap_needs_full_replay_retry(
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-startup-link-reply-timeout"
            ))
        ));
        assert!(!control_plane_bootstrap_needs_full_replay_retry(
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-passive-startup-link-timeout"
            ))
        ));
        assert!(!control_plane_bootstrap_needs_full_replay_retry(
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-startup-link-rescue-budget-exhausted"
            ))
        ));
        assert!(!control_plane_bootstrap_needs_full_replay_retry(
            &DriverError::Protocol("ioctl-timeout")
        ));
        assert!(!control_plane_bootstrap_needs_full_replay_retry(
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-promoted-rearm-timeout"
            ))
        ));
    }

    #[test]
    fn startup_transport_recovery_resets_only_when_bootstrap_state_is_stale() {
        assert!(startup_transport_recovery_should_reset_experimental_state(
            true, false, false
        ));
        assert!(startup_transport_recovery_should_reset_experimental_state(
            false, true, false
        ));
        assert!(startup_transport_recovery_should_reset_experimental_state(
            false, false, true
        ));
        assert!(!startup_transport_recovery_should_reset_experimental_state(
            false, false, false
        ));
    }

    #[test]
    fn promoted_timeout_retry_only_resends_after_reply_wait_timeout() {
        assert!(
            control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                &DriverError::Protocol("ioctl-timeout")
            )
        );
        assert!(
            control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready"
                ))
            )
        );
        assert!(
            control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready"
                ))
            )
        );
        assert!(
            control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-no-reply-linux-f2-armed"
                ))
            )
        );
        assert!(
            control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-sideband-unreadable"
                ))
            )
        );
        assert!(
            control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-passive-startup-link-timeout"
                ))
            )
        );
        assert!(
            !control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-reply-read-stall-no-buffer-ready"
                ))
            )
        );
        assert!(
            control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                &DriverError::Hal(HalError::Unsupported("sdio-function2-ready-timeout"))
            )
        );
        assert!(
            control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-startup-link-reply-timeout"
                ))
            )
        );
        assert!(
            !control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-promoted-rearm-timeout"
                ))
            )
        );
        assert!(
            !control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait(
                &DriverError::Protocol("sdpcm-credit-timeout")
            )
        );
    }

    #[test]
    fn direct_function2_reply_blockers_do_not_trigger_startup_link_reply_rescue() {
        assert!(!startup_link_reply_rescue_reason(
            "cyw43-function2-reply-read-stall-no-buffer-ready"
        ));
        assert!(startup_link_reply_rescue_reason(
            "cyw43-function2-enable-latched-not-ready"
        ));
        assert!(startup_link_reply_rescue_reason(
            "cyw43-control-plane-sideband-read-stall-no-buffer-ready"
        ));
        assert!(!first_control_plane_retry_after_startup_link_reply_failure(
            true,
            true,
            &DriverError::Hal(HalError::Unsupported(
                "cyw43-function2-reply-read-stall-no-buffer-ready"
            )),
        ));
    }

    #[test]
    fn reply_wait_resend_stays_on_startup_link_until_first_reply() {
        assert_eq!(
            control_plane_retry_after_reply_wait_resend_target_clock_hz(
                true,
                SDIO_DATA_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready"
                )),
            ),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            control_plane_retry_after_reply_wait_resend_target_clock_hz(
                true,
                SDIO_STARTUP_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready"
                )),
            ),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            control_plane_retry_after_reply_wait_resend_target_clock_hz(
                true,
                SDIO_STARTUP_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready"
                )),
            ),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            control_plane_retry_after_reply_wait_resend_target_clock_hz(
                true,
                SDIO_STARTUP_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-reply-read-stall-no-buffer-ready"
                )),
            ),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            control_plane_retry_after_reply_wait_resend_target_clock_hz(
                true,
                SDIO_STARTUP_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-pure-f2-startup-link-no-reply"
                )),
            ),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            control_plane_retry_after_reply_wait_resend_target_clock_hz(
                true,
                SDIO_STARTUP_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-no-reply-linux-f2-armed"
                )),
            ),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            control_plane_retry_after_reply_wait_resend_target_clock_hz(
                true,
                SDIO_STARTUP_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-sideband-unreadable"
                )),
            ),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            control_plane_retry_after_reply_wait_resend_target_clock_hz(
                true,
                SDIO_DATA_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-passive-startup-link-timeout"
                )),
            ),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            control_plane_retry_after_reply_wait_resend_target_clock_hz(
                false,
                SDIO_DATA_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready"
                )),
            ),
            SDIO_DATA_CLOCK_HZ
        );
    }

    #[test]
    fn hard_retry_failure_preserves_more_specific_snapshot_error() {
        let retry_err = DriverError::Hal(HalError::Unsupported(
            "cyw43-control-plane-no-reply-linux-f2-armed",
        ));
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                retry_err,
                Some(
                    "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",
                ),
            )),
            Some("cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",)
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-no-reply-linux-f2-armed",
                )),
                Some(""),
            )),
            Some("cyw43-control-plane-no-reply-linux-f2-armed")
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported("sdio-function2-ready-timeout")),
                Some(
                    "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",
                ),
            )),
            Some("sdio-function2-ready-timeout")
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-reply-read-stall-no-buffer-ready",
                )),
                Some(
                    "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",
                ),
            )),
            Some("cyw43-function2-reply-read-stall-no-buffer-ready")
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-reply-read-stall-no-buffer-ready",
                )),
                Some("cyw43-function2-reply-read-stall-no-buffer-ready",),
            )),
            Some("cyw43-function2-reply-read-stall-no-buffer-ready")
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-pure-f2-startup-link-no-reply",
                )),
                Some(
                    "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",
                ),
            )),
            Some("cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",)
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-startup-link-rescue-budget-exhausted",
                )),
                Some(
                    "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",
                ),
            )),
            Some("cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",)
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-sideband-read-stall-no-buffer-ready",
                )),
                Some("cyw43-function2-reply-read-stall-no-buffer-ready"),
            )),
            Some("cyw43-function2-reply-read-stall-no-buffer-ready")
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready",
                )),
                Some("cyw43-function2-reply-read-stall-no-buffer-ready"),
            )),
            Some("cyw43-function2-reply-read-stall-no-buffer-ready")
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",
                )),
                Some("cyw43-function2-reply-read-stall-no-buffer-ready"),
            )),
            Some("cyw43-function2-reply-read-stall-no-buffer-ready")
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-sideband-read-stall-no-buffer-ready",
                )),
                Some(
                    "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",
                ),
            )),
            Some("cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",)
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-sideband-read-stall-no-buffer-ready",
                )),
                Some("cyw43-function2-enable-latched-not-ready-read-stall-no-buffer-ready"),
            )),
            Some("cyw43-function2-enable-latched-not-ready-read-stall-no-buffer-ready")
        );
        assert_eq!(
            unsupported_reason(&preserve_cyw43_init_failure_exact_error(
                DriverError::Hal(HalError::Unsupported(
                    "cyw43-function2-enable-latched-not-ready-sideband-read-stall-no-buffer-ready",
                )),
                Some("cyw43-function2-enable-latched-not-ready-read-stall-no-buffer-ready"),
            )),
            Some("cyw43-function2-enable-latched-not-ready-read-stall-no-buffer-ready")
        );
    }

    #[test]
    fn promoted_cyw43_init_failure_exact_error_returns_unsupported_reason() {
        assert_eq!(
            promoted_cyw43_init_failure_exact_error(&DriverError::Hal(HalError::Unsupported(
                "cyw43-function2-reply-read-stall-no-buffer-ready",
            ))),
            Some("cyw43-function2-reply-read-stall-no-buffer-ready")
        );
        assert_eq!(
            promoted_cyw43_init_failure_exact_error(&DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-pure-f2-startup-link-no-reply",
            ))),
            Some("cyw43-control-plane-pure-f2-startup-link-no-reply")
        );
        assert_eq!(
            promoted_cyw43_init_failure_exact_error(&DriverError::IoctlFailed {
                cmd: Ioctl::SetWsecPmk as u32,
                status: BCME_BADARG,
            }),
            Some("wsec-pmk-bad-argument")
        );
    }

    #[test]
    fn promoted_cyw43_init_failure_exact_error_ignores_non_unsupported_errors() {
        assert_eq!(
            promoted_cyw43_init_failure_exact_error(&DriverError::Hal(HalError::NoPci)),
            None
        );
        assert_eq!(
            promoted_cyw43_init_failure_exact_error(&DriverError::IoctlFailed {
                cmd: Ioctl::SetWsecPmk as u32,
                status: BCME_UNSUPPORTED,
            }),
            None
        );
    }

    #[test]
    fn promoted_timeout_retry_resend_only_uses_startup_link_at_startup_clock() {
        assert!(
            control_plane_retry_after_promoted_timeout_resend_uses_startup_link(
                true,
                true,
                SDIO_STARTUP_CLOCK_HZ,
            )
        );
        assert!(
            control_plane_retry_after_promoted_timeout_resend_uses_startup_link(
                true,
                true,
                SDIO_STARTUP_CLOCK_HZ / 2,
            )
        );
        assert!(
            !control_plane_retry_after_promoted_timeout_resend_uses_startup_link(
                true,
                true,
                SDIO_DATA_CLOCK_HZ,
            )
        );
        assert!(
            !control_plane_retry_after_promoted_timeout_resend_uses_startup_link(
                true,
                false,
                SDIO_STARTUP_CLOCK_HZ,
            )
        );
        assert!(
            !control_plane_retry_after_promoted_timeout_resend_uses_startup_link(
                false,
                true,
                SDIO_STARTUP_CLOCK_HZ,
            )
        );
    }

    #[test]
    fn reply_wait_rearm_uses_promoted_link_only_above_startup_clock() {
        assert!(control_plane_retry_after_reply_wait_uses_promoted_link(
            true,
            SDIO_DATA_CLOCK_HZ,
        ));
        assert!(!control_plane_retry_after_reply_wait_uses_promoted_link(
            true,
            SDIO_STARTUP_CLOCK_HZ,
        ));
        assert!(!control_plane_retry_after_reply_wait_uses_promoted_link(
            false,
            SDIO_DATA_CLOCK_HZ,
        ));
    }

    #[test]
    fn speculative_credit_window_after_promoted_timeout_retry_is_narrow() {
        assert_eq!(
            speculative_credit_window_after_promoted_timeout_retry(true, 1, 1),
            Some(2)
        );
        assert_eq!(
            speculative_credit_window_after_promoted_timeout_retry(true, u8::MAX, u8::MAX),
            Some(0)
        );
        assert_eq!(
            speculative_credit_window_after_promoted_timeout_retry(false, 1, 1),
            None
        );
        assert_eq!(
            speculative_credit_window_after_promoted_timeout_retry(true, 1, 2),
            None
        );
    }

    #[test]
    fn sdpcm_credit_window_respects_sequence_bounds() {
        assert!(!has_sdpcm_credit(1, 1));
        assert!(has_sdpcm_credit(1, 2));
        assert!(has_sdpcm_credit(u8::MAX, 0));
        assert!(!has_sdpcm_credit(2, 1));
    }

    #[test]
    fn ioctl_wait_budget_is_fixed_for_strict_path() {
        assert_eq!(ioctl_wait_loops(false, 0, true), IOCTL_WAIT_LOOPS);
        assert_eq!(ioctl_wait_loops(false, 3, false), IOCTL_WAIT_LOOPS);
    }

    #[test]
    fn ioctl_wait_budget_keeps_short_first_startup_link_window() {
        assert_eq!(
            ioctl_wait_loops(true, 0, true),
            IOCTL_WAIT_LOOPS_STARTUP_LINK_STABILIZED
        );
        assert_eq!(IOCTL_WAIT_LOOPS_STARTUP_LINK_STABILIZED, 32_000);
    }

    #[test]
    fn ioctl_wait_budget_ratchets_down_after_startup_link_rescues() {
        assert_eq!(
            ioctl_wait_loops(true, 1, true),
            IOCTL_WAIT_LOOPS_STARTUP_LINK_RESCUE
        );
        assert_eq!(
            ioctl_wait_loops(true, 2, true),
            IOCTL_WAIT_LOOPS_STARTUP_LINK_RESCUE_REPEAT
        );
        assert_eq!(
            ioctl_wait_loops(true, u8::MAX, true),
            IOCTL_WAIT_LOOPS_STARTUP_LINK_RESCUE_REPEAT
        );
        assert_eq!(IOCTL_WAIT_LOOPS_STARTUP_LINK_RESCUE, 8_000);
        assert_eq!(IOCTL_WAIT_LOOPS_STARTUP_LINK_RESCUE_REPEAT, 2_000);
    }

    #[test]
    fn ioctl_wait_budget_collapses_once_the_final_bounded_probe_is_armed() {
        assert_eq!(
            ioctl_wait_loops(true, 0, false),
            IOCTL_WAIT_LOOPS_STARTUP_LINK_FINAL_BOUNDED
        );
        assert_eq!(
            ioctl_wait_loops(true, 2, false),
            IOCTL_WAIT_LOOPS_STARTUP_LINK_FINAL_BOUNDED
        );
        assert_eq!(IOCTL_WAIT_LOOPS_STARTUP_LINK_FINAL_BOUNDED, 1_000);
    }

    #[test]
    fn startup_link_ioctl_timeout_preserves_cached_exact_error_only_for_known_blockers() {
        assert_eq!(
            startup_link_ioctl_timeout_preserved_exact_error(
                true,
                true,
                Some("cyw43-control-plane-sideband-read-stall-no-buffer-ready"),
            ),
            Some("cyw43-control-plane-sideband-read-stall-no-buffer-ready")
        );
        assert_eq!(
            startup_link_ioctl_timeout_preserved_exact_error(
                true,
                true,
                Some("cyw43-function2-enable-latched-not-ready"),
            ),
            Some("cyw43-function2-enable-latched-not-ready")
        );
        assert_eq!(
            startup_link_ioctl_timeout_preserved_exact_error(true, true, Some("ioctl-timeout")),
            None
        );
        assert_eq!(
            startup_link_ioctl_timeout_preserved_exact_error(
                true,
                false,
                Some("cyw43-control-plane-sideband-read-stall-no-buffer-ready"),
            ),
            Some("cyw43-control-plane-sideband-read-stall-no-buffer-ready")
        );
        assert_eq!(
            startup_link_ioctl_timeout_preserved_exact_error(
                false,
                true,
                Some("cyw43-control-plane-sideband-read-stall-no-buffer-ready"),
            ),
            None
        );
    }
}
