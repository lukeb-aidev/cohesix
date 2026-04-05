// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide a bounded CYW43455 Wi-Fi driver for Pi 4 console networking over HAL-owned SDIO transport.
// Author: Lukas Bower

//! HAL-bound CYW43455 bring-up and Ethernet datapath for Raspberry Pi 4.

use core::fmt;
use core::hint::spin_loop;

use heapless::Vec as HeaplessVec;
use log::{debug, info, trace, warn};
use smoltcp::phy::{self, Device, DeviceCapabilities};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;

use crate::hal::pi4_wifi::Pi4WifiState;
use crate::hal::{
    HalError, Hardware, SdioBusWidth, SdioFunction, WifiFirmwareBundle, WifiPowerState,
    WifiResetState,
};
#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
use crate::local_seat_pi4::{wifi_progress_begin, wifi_progress_finish, wifi_progress_tick};
use crate::net::{ConsoleNetConfig, NetDevice, NetDeviceCounters, NetDriverError, WifiCredentials};
use crate::net_consts::MAX_FRAME_LEN;

const SDIO_STARTUP_CLOCK_HZ: u32 = 400_000;
// Pi 4 bring-up is currently stable at 12.5 MHz; 25 MHz consistently trips
// the first high-speed SDIO block transfer with data CRC/end-bit faults.
const SDIO_DATA_CLOCK_HZ: u32 = 12_500_000;
const SDIO_CCCR_IOEX: u32 = 0x02;
const DEFAULT_WIFI_MAC: [u8; 6] = [0x02, 0x43, 0x4f, 0x48, 0x58, 0x55];

const SDPCM_HEADER_LEN: usize = 12;
const CDC_HEADER_LEN: usize = 16;
const BDC_HEADER_LEN: usize = 4;
const DATA_PADDING_LEN: usize = 2;

const FRAME_BUF_LEN: usize = 2048;
const CONTROL_RESPONSE_BUF_LEN: usize = 512;
const CLM_CHUNK_SIZE: usize = 1024;
const IOCTL_WAIT_LOOPS: usize = 8_000;
// Linux brcmfmac waits up to 2500 ms for an SDIO control response.
// Use a wider bounded spin budget only on the fragile startup-link fallback
// path so we can observe whether firmware eventually replies without forcing a
// destructive reattach first.
const IOCTL_WAIT_LOOPS_STARTUP_LINK_STABILIZED: usize = 2_500_000;
const JOIN_WAIT_LOOPS: usize = 64_000;
const CREDIT_WAIT_LOOPS: usize = 2_000;
const RX_PUMP_LIMIT: usize = 8;

const CHANNEL_CONTROL: u8 = 0;
const CHANNEL_EVENT: u8 = 1;
const CHANNEL_DATA: u8 = 2;

const DOWNLOAD_FLAG_BEGIN: u16 = 0x0002;
const DOWNLOAD_FLAG_END: u16 = 0x0004;
const DOWNLOAD_FLAG_HANDLER_VER: u16 = 0x1000;
const DOWNLOAD_TYPE_CLM: u16 = 2;

const ETH_P_LINK_CTL: u16 = 0x886c;
const BCMILCP_SUBTYPE_VENDOR_LONG: u16 = 32769;
const BCMILCP_BCM_SUBTYPE_EVENT: u16 = 1;
const BROADCOM_OUI: [u8; 3] = [0x00, 0x10, 0x18];

const EVENT_SET_SSID: u8 = 0;
const EVENT_AUTH: u8 = 3;
const EVENT_DEAUTH: u8 = 5;
const EVENT_DISASSOC: u8 = 11;
const EVENT_LINK: u8 = 16;
const STATUS_SUCCESS: u32 = 0;

const BDC_VERSION: u8 = 2;
const BDC_VERSION_SHIFT: u8 = 4;

const WSEC_AES: u32 = 0x04;
const AUTH_OPEN: u32 = 0x00;
const MFP_CAPABLE: u32 = 1;
const WPA_AUTH_DISABLED: u32 = 0x0000;
const WPA_AUTH_WPA2_PSK: u32 = 0x0080;

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
    SetInfra = 20,
    SetAuth = 22,
    SetSsid = 26,
    SetAntdiv = 64,
    SetPm = 86,
    SetGmode = 110,
    SetWsec = 134,
    SetBand = 142,
    SetWpaAuth = 165,
    GetVar = 262,
    SetVar = 263,
    SetWsecPmk = 268,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Cyw43Event {
    event_type: u8,
    status: u32,
    reason: u32,
    auth_type: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RxFrameResult {
    None,
    Control {
        id: u16,
        status: u32,
        response_len: usize,
    },
    Event(Cyw43Event),
    Data(HeaplessVec<u8, MAX_FRAME_LEN>),
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
            DriverError::Hal(HalError::Unsupported(
                "cyw43-function2-enable-latched-not-ready"
            )) | DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-passive-startup-link-timeout"
            ))
        )
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
    matches!(
        err,
        DriverError::Protocol("ioctl-timeout")
            | DriverError::Hal(HalError::Unsupported(
                "cyw43-function2-enable-latched-not-ready"
            ))
            | DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-passive-startup-link-timeout"
            ))
            | DriverError::Hal(HalError::Unsupported("sdio-function2-ready-timeout"))
            | DriverError::Hal(HalError::Unsupported(
                "cyw43-control-plane-startup-link-reply-timeout"
            ))
    )
}

#[inline]
const fn control_plane_retry_after_promoted_timeout_resend_target_clock_hz(
    experimental_no_ht_transport: bool,
    effective_clock_hz: u32,
) -> u32 {
    if experimental_no_ht_transport && effective_clock_hz > SDIO_STARTUP_CLOCK_HZ {
        SDIO_STARTUP_CLOCK_HZ
    } else {
        effective_clock_hz
    }
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

fn prepare_initial_control_plane_transport(
    state: &mut Pi4WifiState,
) -> Result<(u32, SdioBusWidth), DriverError> {
    info!("[cyw43] step: set_clock(data)");
    let recommended_data_clock_hz = state.recommended_data_clock_hz();
    let data_clock_target_hz = initial_control_plane_data_clock_target_hz(
        recommended_data_clock_hz,
        state.cyw43_experimental_no_ht_transport(),
    );
    let data_clock_hz = state.set_clock_hz(data_clock_target_hz)?;
    let transport_mode = if state.cyw43_experimental_no_ht_transport() {
        "bounded-no-ht"
    } else {
        "strict"
    };
    info!(
        "[cyw43] control transport ready clock={}Hz target={}Hz recommended={}Hz bus_width=4 mode={transport_mode} first_probe_policy=stable-data-link write_chunk_limit={} reply_chunk_limit={}",
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
) {
    warn!("[cyw43] init failure stage={stage} err={err}");
    match state.debug_dump_state(stage) {
        Ok(snapshot) => {
            warn!("[cyw43] init snapshot stage={stage} snapshot={snapshot:?}");
        }
        Err(snapshot_err) => {
            warn!("[cyw43] init snapshot unavailable stage={stage} err={snapshot_err}");
        }
    }
    if include_control_plane_snapshot {
        state.log_cyw43_control_plane_snapshot(stage);
    }
}

pub(crate) fn debug_load_firmware_from_transport(
    state: &mut Pi4WifiState,
) -> Result<u32, HalError> {
    info!("[cyw43] debug: set_bus_width(4bit)");
    state.set_bus_width(SdioBusWidth::FourBit)?;
    info!("[cyw43] debug: load_firmware(startup-clock)");
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

pub struct Cyw43NetDevice {
    state: Pi4WifiState,
    probe: ProbeReport,
    mac: EthernetAddress,
    tx_drops: u32,
    rx_packets: u64,
    tx_packets: u64,
    sdpcm_seq: u8,
    sdpcm_seq_max: u8,
    ioctl_id: u16,
    link_up: bool,
    rx_frame: [u8; FRAME_BUF_LEN],
    tx_frame: [u8; FRAME_BUF_LEN],
    control_response: [u8; CONTROL_RESPONSE_BUF_LEN],
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
        H: Hardware<Error = HalError>,
    {
        let credentials = config
            .wifi_credentials
            .ok_or(DriverError::Config("wifi-credentials-missing"))?;

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
        info!("[cyw43] step: load_firmware(startup-clock)");
        // Keep the control path at startup clock, but switch to the final bus
        // width before bulk upload so the first high-speed CMD53 uses the
        // stable 4-bit transport configuration.
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
            state,
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
            link_up: false,
            rx_frame: [0; FRAME_BUF_LEN],
            tx_frame: [0; FRAME_BUF_LEN],
            control_response: [0; CONTROL_RESPONSE_BUF_LEN],
        };

        info!("[cyw43] step: init_control_plane");
        if let Err(err) = device.init_control_plane(firmware, credentials) {
            log_cyw43_init_failure(
                "cyw43-init-control-plane-fail",
                &mut device.state,
                &err,
                true,
            );
            return Err(err);
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

    fn init_control_plane(
        &mut self,
        firmware: WifiFirmwareBundle<'static>,
        credentials: WifiCredentials,
    ) -> Result<(), DriverError> {
        if let Some(clm) = firmware.clm_blob {
            self.load_clm(clm)?;
        }

        self.set_iovar_u32("bus:txglom", 0)?;
        self.set_iovar_u32("apsta", 1)?;
        self.set_country_worldwide()?;
        self.ioctl_set_u32(Ioctl::SetAntdiv, 0, 0)?;
        self.set_iovar_u32("ampdu_ba_wsize", 8)?;
        self.set_iovar_u32("ampdu_mpdu", 4)?;
        self.set_event_mask(&[EVENT_SET_SSID, EVENT_AUTH, EVENT_LINK])?;
        self.ioctl_raw(IoctlType::Set, Ioctl::Up, 0, &[])?;
        self.ioctl_set_u32(Ioctl::SetGmode, 0, 1)?;
        self.ioctl_set_u32(Ioctl::SetBand, 0, 0)?;
        self.ioctl_set_u32(Ioctl::SetPm, 0, 0)?;
        self.mac = self.read_mac_address();
        self.join(credentials)?;
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
            let payload_len = 8 + 12 + chunk_len;
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

    fn set_country_worldwide(&mut self) -> Result<(), DriverError> {
        let payload_len = 12;
        {
            let payload = self.payload_mut(payload_len)?;
            payload[..4].copy_from_slice(b"XX\0\0");
            put_i32_le(payload, 4, -1);
            payload[8..12].copy_from_slice(b"XX\0\0");
        }
        self.set_iovar_from_payload("country", payload_len)
    }

    fn set_event_mask(&mut self, events: &[u8]) -> Result<(), DriverError> {
        let payload_len = 4 + 24;
        {
            let payload = self.payload_mut(payload_len)?;
            payload.fill(0);
            put_u32_le(payload, 0, 0);
            for &event in events {
                let index = usize::from(event / 8);
                let bit = event % 8;
                if let Some(slot) = payload.get_mut(4 + index) {
                    *slot |= 1 << bit;
                }
            }
        }
        self.set_iovar_from_payload("bsscfg:event_msgs", payload_len)
    }

    fn join(&mut self, credentials: WifiCredentials) -> Result<(), DriverError> {
        let ssid = credentials.ssid().map_err(DriverError::Config)?;
        let psk = credentials.psk().map_err(DriverError::Config)?;
        if psk.is_empty() {
            self.ioctl_set_u32(Ioctl::SetWsec, 0, 0)?;
            self.set_iovar_u32x2("bsscfg:sup_wpa", 0, 0)?;
            self.ioctl_set_u32(Ioctl::SetInfra, 0, 1)?;
            self.ioctl_set_u32(Ioctl::SetAuth, 0, 0)?;
            self.ioctl_set_u32(Ioctl::SetWpaAuth, 0, WPA_AUTH_DISABLED)?;
            info!("[cyw43] join: auth=open ssid_len={}", ssid.len());
        } else {
            self.ioctl_set_u32(Ioctl::SetWsec, 0, WSEC_AES)?;
            self.set_iovar_u32x2("bsscfg:sup_wpa", 0, 1)?;
            self.set_iovar_u32x2("bsscfg:sup_wpa2_eapver", 0, u32::MAX)?;
            self.set_iovar_u32x2("bsscfg:sup_wpa_tmo", 0, 2500)?;
            self.set_wsec_pmk(psk.as_bytes())?;
            self.ioctl_set_u32(Ioctl::SetInfra, 0, 1)?;
            self.ioctl_set_u32(Ioctl::SetAuth, 0, AUTH_OPEN)?;
            self.set_iovar_u32("mfp", MFP_CAPABLE)?;
            self.ioctl_set_u32(Ioctl::SetWpaAuth, 0, WPA_AUTH_WPA2_PSK)?;
            info!(
                "[cyw43] join: auth=wpa2-psk ssid_len={} psk_len={}",
                ssid.len(),
                psk.len(),
            );
        }

        let payload_len = 36;
        {
            let payload = self.payload_mut(payload_len)?;
            payload.fill(0);
            put_u32_le(
                payload,
                0,
                u32::try_from(ssid.len()).map_err(|_| DriverError::Config("wifi-ssid-too-long"))?,
            );
            payload[4..4 + ssid.len()].copy_from_slice(ssid.as_bytes());
        }
        let _ = self.ioctl_encoded(IoctlType::Set, Ioctl::SetSsid, 0, payload_len)?;
        self.wait_for_join()
    }

    fn set_wsec_pmk(&mut self, psk: &[u8]) -> Result<(), DriverError> {
        if psk.is_empty() {
            return Err(DriverError::Config("wifi-psk-empty"));
        }
        let payload_len = 68;
        {
            let payload = self.payload_mut(payload_len)?;
            payload.fill(0);
            put_u16_le(
                payload,
                0,
                u16::try_from(psk.len()).map_err(|_| DriverError::Config("wifi-psk-too-long"))?,
            );
            put_u16_le(payload, 2, 1);
            payload[4..4 + psk.len()].copy_from_slice(psk);
        }
        let _ = self.ioctl_encoded(IoctlType::Set, Ioctl::SetWsecPmk, 0, payload_len)?;
        Ok(())
    }

    fn wait_for_join(&mut self) -> Result<(), DriverError> {
        let mut auth_status = 0;
        for _ in 0..JOIN_WAIT_LOOPS {
            match self.process_next_frame(false)? {
                RxFrameResult::Event(event) => {
                    if event.event_type == EVENT_AUTH && event.status != STATUS_SUCCESS {
                        auth_status = event.status;
                    } else if event.event_type == EVENT_SET_SSID {
                        if event.status == STATUS_SUCCESS {
                            self.link_up = true;
                            info!("[cyw43] join complete");
                            return Ok(());
                        }
                        return Err(DriverError::JoinFailed {
                            status: event.status,
                            auth_status,
                        });
                    }
                }
                RxFrameResult::None | RxFrameResult::Control { .. } | RxFrameResult::Data(_) => {}
            }
            spin_loop();
        }
        Err(DriverError::Protocol("join-timeout"))
    }

    fn read_mac_address(&mut self) -> EthernetAddress {
        let mut mac = [0u8; 6];
        match self.get_iovar("cur_etheraddr", &mut mac) {
            Ok(6) if !is_zeroed_mac(&mac) => EthernetAddress(mac),
            Ok(_) | Err(_) => EthernetAddress(DEFAULT_WIFI_MAC),
        }
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

    fn set_iovar_from_payload(&mut self, name: &str, value_len: usize) -> Result<(), DriverError> {
        let name_len = name.len();
        if name_len + 1 + value_len > self.payload_capacity() {
            return Err(DriverError::FrameTooLarge);
        }
        self.tx_frame.copy_within(
            SDPCM_HEADER_LEN + CDC_HEADER_LEN..SDPCM_HEADER_LEN + CDC_HEADER_LEN + value_len,
            SDPCM_HEADER_LEN + CDC_HEADER_LEN + name_len + 1,
        );
        {
            let payload = &mut self.tx_frame[SDPCM_HEADER_LEN + CDC_HEADER_LEN
                ..SDPCM_HEADER_LEN + CDC_HEADER_LEN + name_len + 1 + value_len];
            payload[..name_len].copy_from_slice(name.as_bytes());
            payload[name_len] = 0;
        }
        let _ = self.ioctl_encoded(IoctlType::Set, Ioctl::SetVar, 0, name_len + 1 + value_len)?;
        Ok(())
    }

    fn get_iovar(&mut self, name: &str, out: &mut [u8]) -> Result<usize, DriverError> {
        let payload_len = core::cmp::max(name.len() + 1, out.len());
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
        self.state.rearm_cyw43_control_plane_slow_link()?;
        self.state
            .resume_cyw43_control_plane_reply_probe_on_startup_link(original_reply_stage);
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
                // Keep the first bounded-no-HT resend on the slow startup
                // link. The latest Pi 4 traces still show that re-promoting
                // before any control ioctl completes just reproduces the same
                // early reply-path stall.
                let resend_clock_hz = self.state.set_clock_hz(
                    control_plane_retry_after_promoted_timeout_resend_target_clock_hz(
                        self.state.cyw43_experimental_no_ht_transport(),
                        self.probe.effective_clock_hz,
                    ),
                )?;
                self.probe.effective_clock_hz = resend_clock_hz;
                self.state.rearm_cyw43_control_plane_slow_link()?;
                self.state
                    .resume_cyw43_control_plane_reply_probe_on_startup_link(resend_stage);
                info!(
                    "[cyw43] {retry_label} slow-link resend armed cmd=0x{:08x} iface={} len={} clock={}Hz chunk_limit={} mode=bounded-no-ht",
                    cmd as u32,
                    iface,
                    payload_len,
                    self.probe.effective_clock_hz,
                    self.state.cyw43_control_plane_chunk_limit(),
                );
                warn!(
                    "[cyw43] {retry_label} resending cmd=0x{:08x} iface={} len={} clock={}Hz chunk_limit={} mode=bounded-no-ht reason={wait_err}",
                    cmd as u32,
                    iface,
                    payload_len,
                    self.probe.effective_clock_hz,
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

        let total_len = SDPCM_HEADER_LEN
            .checked_add(CDC_HEADER_LEN)
            .and_then(|value| value.checked_add(payload_len))
            .ok_or(DriverError::FrameTooLarge)?;
        let aligned_len = align4(total_len);
        if aligned_len > self.tx_frame.len() {
            return Err(DriverError::FrameTooLarge);
        }

        let sdpcm_seq = self.sdpcm_seq;
        self.sdpcm_seq = self.sdpcm_seq.wrapping_add(1);
        self.ioctl_id = self.ioctl_id.wrapping_add(1);

        let packet_len = u16::try_from(total_len).map_err(|_| DriverError::FrameTooLarge)?;
        let len_inv = !packet_len;
        put_u16_le(&mut self.tx_frame, 0, packet_len);
        put_u16_le(&mut self.tx_frame, 2, len_inv);
        self.tx_frame[4] = sdpcm_seq;
        self.tx_frame[5] = CHANNEL_CONTROL;
        self.tx_frame[6] = 0;
        self.tx_frame[7] =
            u8::try_from(SDPCM_HEADER_LEN).map_err(|_| DriverError::FrameTooLarge)?;
        self.tx_frame[8] = 0;
        self.tx_frame[9] = 0;
        self.tx_frame[10] = 0;
        self.tx_frame[11] = 0;

        put_u32_le(&mut self.tx_frame[SDPCM_HEADER_LEN..], 0, cmd as u32);
        put_u32_le(
            &mut self.tx_frame[SDPCM_HEADER_LEN..],
            4,
            u32::try_from(payload_len).map_err(|_| DriverError::FrameTooLarge)?,
        );
        put_u16_le(
            &mut self.tx_frame[SDPCM_HEADER_LEN..],
            8,
            (kind as u16) | (u16::try_from(iface).unwrap_or(0) << 12),
        );
        put_u16_le(&mut self.tx_frame[SDPCM_HEADER_LEN..], 10, self.ioctl_id);
        put_u32_le(&mut self.tx_frame[SDPCM_HEADER_LEN..], 12, 0);

        self.tx_frame[total_len..aligned_len].fill(0);
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
            info!(
                "[cyw43] control tx probe cmd=0x{:08x} iface={} payload_len={} packet_len={} len_inv=0x{:04x} aligned_len={} seq={}/{} ioctl_id={} channel={} header_len={} cdc_flags=0x{:04x} write_chunk_limit={} reply_chunk_limit={} mode={transport_mode}",
                cmd as u32,
                iface,
                payload_len,
                packet_len,
                len_inv,
                aligned_len,
                sdpcm_seq,
                self.sdpcm_seq_max,
                self.ioctl_id,
                CHANNEL_CONTROL,
                self.tx_frame[7],
                get_u16_le(&self.tx_frame[SDPCM_HEADER_LEN..], 8).unwrap_or(0),
                self.state.cyw43_control_plane_write_chunk_limit(),
                self.state.cyw43_control_plane_reply_chunk_limit(),
            );
        }
        self.state
            .write_cyw43_frame(&mut self.tx_frame[..aligned_len])?;
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
        let wait_budget = ioctl_wait_loops(startup_link_stabilized);
        for _ in 0..wait_budget {
            match self.process_next_frame(false) {
                Err(err) => {
                    let (reply_mode, reply_attempts, reply_empty_polls, promoted_probe_pending) =
                        self.state.cyw43_control_plane_reply_rearm_diag();
                    let credit_ready = self.has_credit();
                    warn!(
                        "[cyw43] ioctl response error cmd=0x{:08x} id={} seq={}/{} credit={} startup_link_stabilized={} reply_mode={} reply_attempts={} reply_empty_polls={} promoted_probe_pending={} no_ht={} write_chunk_limit={} reply_chunk_limit={} err={err}",
                        cmd,
                        expected_id,
                        self.sdpcm_seq,
                        self.sdpcm_seq_max,
                        credit_ready,
                        startup_link_stabilized,
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
                    return Err(err);
                }
                Ok(RxFrameResult::Control {
                    id,
                    status,
                    response_len,
                }) if id == expected_id => {
                    if status != STATUS_SUCCESS {
                        return Err(DriverError::IoctlFailed { cmd, status });
                    }
                    return Ok(response_len);
                }
                Ok(RxFrameResult::None) => {}
                Ok(RxFrameResult::Control { .. })
                | Ok(RxFrameResult::Event(_))
                | Ok(RxFrameResult::Data(_)) => {}
            }
            spin_loop();
        }
        let (reply_mode, reply_attempts, reply_empty_polls, promoted_probe_pending) =
            self.state.cyw43_control_plane_reply_rearm_diag();
        let credit_ready = self.has_credit();
        warn!(
            "[cyw43] ioctl timeout cmd=0x{:08x} id={} seq={}/{} credit={} wait_budget={} startup_link_stabilized={} reply_mode={} reply_attempts={} reply_empty_polls={} promoted_probe_pending={} no_ht={} write_chunk_limit={} reply_chunk_limit={}",
            cmd,
            expected_id,
            self.sdpcm_seq,
            self.sdpcm_seq_max,
            credit_ready,
            wait_budget,
            startup_link_stabilized,
            reply_mode,
            reply_attempts,
            reply_empty_polls,
            promoted_probe_pending,
            self.state.cyw43_experimental_no_ht_transport(),
            self.state.cyw43_control_plane_write_chunk_limit(),
            self.state.cyw43_control_plane_reply_chunk_limit(),
        );
        self.log_pending_ioctl_frame("ioctl-timeout");
        self.state.log_cyw43_control_plane_snapshot("ioctl-timeout");
        Err(DriverError::Protocol("ioctl-timeout"))
    }

    fn log_pending_ioctl_frame(&self, stage: &'static str) {
        let packet_len = get_u16_le(&self.tx_frame, 0).unwrap_or(0);
        let len_inv = get_u16_le(&self.tx_frame, 2).unwrap_or(0);
        let cdc = &self.tx_frame[SDPCM_HEADER_LEN..];
        let cdc_cmd = get_u32_le(cdc, 0).unwrap_or(0);
        let cdc_len = get_u32_le(cdc, 4).unwrap_or(0);
        let cdc_flags = get_u16_le(cdc, 8).unwrap_or(0);
        let cdc_id = get_u16_le(cdc, 10).unwrap_or(0);
        let cdc_status = get_u32_le(cdc, 12).unwrap_or(0);
        warn!(
            "[cyw43] ioctl frame {stage} packet_len={} len_inv=0x{:04x} seq=0x{:02x} channel=0x{:02x} header_len={} credit=0x{:02x} cdc_cmd=0x{:08x} cdc_len={} cdc_flags=0x{:04x} cdc_id={} cdc_status=0x{:08x}",
            packet_len,
            len_inv,
            self.tx_frame[4],
            self.tx_frame[5],
            self.tx_frame[7],
            self.tx_frame[9],
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
        let frame_len = self.state.read_cyw43_frame(&mut self.rx_frame)?;
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
        if frame_len < SDPCM_HEADER_LEN {
            return Err(DriverError::Protocol("sdpcm-short-header"));
        }
        let packet_len =
            usize::from(get_u16_le(&self.rx_frame, 0).ok_or(DriverError::Protocol("sdpcm-len"))?);
        let packet_len = core::cmp::min(packet_len, frame_len);
        let len_inv =
            get_u16_le(&self.rx_frame, 2).ok_or(DriverError::Protocol("sdpcm-len-inv"))?;
        if len_inv != !u16::try_from(packet_len).unwrap_or(u16::MAX) {
            return Err(DriverError::Protocol("sdpcm-len-mismatch"));
        }
        self.update_credit();

        let channel = self.rx_frame[5] & 0x0f;
        let header_length = usize::from(self.rx_frame[7]);
        if header_length > packet_len || header_length < SDPCM_HEADER_LEN {
            return Err(DriverError::Protocol("sdpcm-header-length"));
        }
        match channel {
            CHANNEL_CONTROL => self.process_control_frame(header_length, packet_len),
            CHANNEL_EVENT => self.process_event_frame(header_length, packet_len),
            CHANNEL_DATA if allow_data => self.process_data_frame(header_length, packet_len),
            CHANNEL_DATA => Ok(RxFrameResult::None),
            _ => Ok(RxFrameResult::None),
        }
    }

    fn process_control_frame(
        &mut self,
        payload_start: usize,
        payload_end: usize,
    ) -> Result<RxFrameResult, DriverError> {
        let payload = &self.rx_frame[payload_start..payload_end];
        if payload.len() < CDC_HEADER_LEN {
            return Err(DriverError::Protocol("cdc-short-header"));
        }
        let response_len = usize::try_from(
            get_u32_le(payload, 4).ok_or(DriverError::Protocol("cdc-response-len"))?,
        )
        .map_err(|_| DriverError::ResponseTooLarge)?;
        let status = get_u32_le(payload, 12).ok_or(DriverError::Protocol("cdc-status"))?;
        let id = get_u16_le(payload, 10).ok_or(DriverError::Protocol("cdc-id"))?;
        let payload_available = payload.len().saturating_sub(CDC_HEADER_LEN);
        let copy_len = core::cmp::min(
            response_len.min(payload_available),
            self.control_response.len(),
        );
        self.control_response[..copy_len]
            .copy_from_slice(&payload[CDC_HEADER_LEN..CDC_HEADER_LEN + copy_len]);
        Ok(RxFrameResult::Control {
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
        let packet = bdc_payload(payload).ok_or(DriverError::Protocol("bdc-event"))?;
        if packet.len() < 72 {
            return Err(DriverError::Protocol("event-short"));
        }
        if get_u16_be(packet, 12) != Some(ETH_P_LINK_CTL)
            || get_u16_be(packet, 14) != Some(BCMILCP_SUBTYPE_VENDOR_LONG)
            || packet.get(19..22) != Some(&BROADCOM_OUI)
            || get_u16_be(packet, 22) != Some(BCMILCP_BCM_SUBTYPE_EVENT)
        {
            return Ok(RxFrameResult::None);
        }

        let event = Cyw43Event {
            event_type: packet[31],
            status: get_u32_be(packet, 32).ok_or(DriverError::Protocol("event-status"))?,
            reason: get_u32_be(packet, 36).ok_or(DriverError::Protocol("event-reason"))?,
            auth_type: get_u32_be(packet, 40).ok_or(DriverError::Protocol("event-auth"))?,
        };
        match event.event_type {
            EVENT_SET_SSID | EVENT_LINK => {
                self.link_up = event.status == STATUS_SUCCESS;
            }
            EVENT_DEAUTH | EVENT_DISASSOC => {
                self.link_up = false;
            }
            _ => {}
        }
        debug!(
            "[cyw43] event type={} status=0x{:08x} reason=0x{:08x} auth=0x{:08x}",
            event.event_type, event.status, event.reason, event.auth_type
        );
        Ok(RxFrameResult::Event(event))
    }

    fn process_data_frame(
        &mut self,
        payload_start: usize,
        payload_end: usize,
    ) -> Result<RxFrameResult, DriverError> {
        let payload = &self.rx_frame[payload_start..payload_end];
        let packet = bdc_payload(payload).ok_or(DriverError::Protocol("bdc-data"))?;
        if packet.len() > MAX_FRAME_LEN {
            return Err(DriverError::FrameTooLarge);
        }
        let mut frame = HeaplessVec::new();
        frame
            .extend_from_slice(packet)
            .map_err(|_| DriverError::FrameTooLarge)?;
        Ok(RxFrameResult::Data(frame))
    }

    fn update_credit(&mut self) {
        let mut sdpcm_seq_max = self.rx_frame[9];
        if (self.rx_frame[5] & 0x0f) < 3 {
            if sdpcm_seq_max.wrapping_sub(self.sdpcm_seq) > 0x40 {
                sdpcm_seq_max = self.sdpcm_seq.wrapping_add(2);
            }
            self.sdpcm_seq_max = sdpcm_seq_max;
        }
    }

    fn poll_rx(&mut self) -> Option<HeaplessVec<u8, MAX_FRAME_LEN>> {
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
            &mut self.tx_frame,
            0,
            u16::try_from(total_len).map_err(|_| DriverError::FrameTooLarge)?,
        );
        put_u16_le(
            &mut self.tx_frame,
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
        Ok(&mut self.tx_frame
            [SDPCM_HEADER_LEN + CDC_HEADER_LEN..SDPCM_HEADER_LEN + CDC_HEADER_LEN + payload_len])
    }

    const fn payload_capacity(&self) -> usize {
        FRAME_BUF_LEN - SDPCM_HEADER_LEN - CDC_HEADER_LEN
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
        Some(TxToken { device: self })
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

    fn debug_snapshot(&mut self) {
        debug!(
            "[cyw43] snapshot mac={} seq={}/{} link_up={} rx={} tx={} drops={} probe={:?}",
            self.mac,
            self.sdpcm_seq,
            self.sdpcm_seq_max,
            self.link_up,
            self.rx_packets,
            self.tx_packets,
            self.tx_drops,
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
}

fn align4(len: usize) -> usize {
    (len + 3) & !3
}

fn bdc_payload(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() < BDC_HEADER_LEN {
        return None;
    }
    let data_offset_words = usize::from(payload[3]);
    let start = BDC_HEADER_LEN.checked_add(data_offset_words.checked_mul(4)?)?;
    payload.get(start..)
}

fn is_zeroed_mac(mac: &[u8; 6]) -> bool {
    mac.iter().all(|byte| *byte == 0)
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
    // Firmware-ready is stable on the startup clock, but the latest Pi 4
    // traces show the first live control-plane exchange stalling there after
    // Function 2 is armed. Start the bounded probe on the same stable data
    // link we use for the rest of control traffic, and let the retry path
    // fall back to the startup link only if that first exchange still fails.
    let _ = experimental_no_ht_transport;
    control_plane_data_clock_target_hz(recommended_data_clock_hz)
}

#[inline]
const fn ioctl_wait_loops(startup_link_stabilized: bool) -> usize {
    if startup_link_stabilized {
        IOCTL_WAIT_LOOPS_STARTUP_LINK_STABILIZED
    } else {
        IOCTL_WAIT_LOOPS
    }
}

#[inline]
const fn has_sdpcm_credit(sdpcm_seq: u8, sdpcm_seq_max: u8) -> bool {
    sdpcm_seq != sdpcm_seq_max && (sdpcm_seq_max.wrapping_sub(sdpcm_seq) & 0x80) == 0
}

fn put_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    if let Some(slot) = buf.get_mut(offset..offset + 2) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
}

fn put_i32_le(buf: &mut [u8], offset: usize, value: i32) {
    if let Some(slot) = buf.get_mut(offset..offset + 4) {
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
        align4, bdc_payload, control_plane_data_clock_target_hz,
        control_plane_retry_after_promoted_timeout_can_resend_after_reply_wait,
        control_plane_retry_after_promoted_timeout_resend_uses_startup_link,
        control_plane_retry_after_promoted_timeout_target_clock_hz,
        first_control_plane_retry_after_promoted_timeout, has_sdpcm_credit,
        initial_control_plane_data_clock_target_hz, ioctl_wait_loops, is_transport_retryable,
        put_u16_le, DriverError, EVENT_AUTH, EVENT_SET_SSID, IOCTL_WAIT_LOOPS,
        IOCTL_WAIT_LOOPS_STARTUP_LINK_STABILIZED, SDIO_DATA_CLOCK_HZ, SDIO_STARTUP_CLOCK_HZ,
    };
    use crate::hal::HalError;

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
        frame[3] = 1;
        assert_eq!(bdc_payload(&frame), Some(&frame[8..]));
    }

    #[test]
    fn event_constants_match_expected_values() {
        assert_eq!(EVENT_SET_SSID, 0);
        assert_eq!(EVENT_AUTH, 3);
    }

    #[test]
    fn little_endian_helpers_write_expected_bytes() {
        let mut buf = [0u8; 4];
        put_u16_le(&mut buf, 1, 0x1234);
        assert_eq!(buf, [0x00, 0x34, 0x12, 0x00]);
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
    fn initial_control_plane_data_clock_uses_stable_data_link_for_first_probe() {
        assert_eq!(
            initial_control_plane_data_clock_target_hz(SDIO_DATA_CLOCK_HZ, true),
            SDIO_DATA_CLOCK_HZ
        );
        assert_eq!(
            initial_control_plane_data_clock_target_hz(400_000, true),
            400_000
        );
        assert_eq!(
            initial_control_plane_data_clock_target_hz(SDIO_DATA_CLOCK_HZ, false),
            SDIO_DATA_CLOCK_HZ
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
    fn startup_link_reply_failure_retry_targets_startup_clock() {
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
                SDIO_STARTUP_CLOCK_HZ,
                &DriverError::Hal(HalError::Unsupported(
                    "cyw43-control-plane-passive-startup-link-timeout"
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
                    "cyw43-control-plane-passive-startup-link-timeout"
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
    fn promoted_timeout_retry_resend_targets_startup_clock_during_bounded_probe() {
        assert_eq!(
            control_plane_retry_after_promoted_timeout_resend_target_clock_hz(
                true,
                SDIO_DATA_CLOCK_HZ,
            ),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            control_plane_retry_after_promoted_timeout_resend_target_clock_hz(
                true,
                SDIO_STARTUP_CLOCK_HZ,
            ),
            SDIO_STARTUP_CLOCK_HZ
        );
        assert_eq!(
            control_plane_retry_after_promoted_timeout_resend_target_clock_hz(
                false,
                SDIO_DATA_CLOCK_HZ,
            ),
            SDIO_DATA_CLOCK_HZ
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
        assert_eq!(ioctl_wait_loops(false), IOCTL_WAIT_LOOPS);
    }

    #[test]
    fn ioctl_wait_budget_expands_for_startup_link_stabilized_path() {
        assert_eq!(
            ioctl_wait_loops(true),
            IOCTL_WAIT_LOOPS_STARTUP_LINK_STABILIZED
        );
    }
}
