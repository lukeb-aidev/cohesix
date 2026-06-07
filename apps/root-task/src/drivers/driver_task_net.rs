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

use smoltcp::phy::{self, Device, DeviceCapabilities, RxToken as _, TxToken as _};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;
use spin::Mutex;

use crate::drivers::bcmgenet::{BcmGenetDevice, DriverError as BcmGenetDriverError};
use crate::drivers::cyw43::{Cyw43NetDevice, DriverError as Cyw43DriverError};
use crate::hal::driver_task::{
    DriverFrameDescriptor, DriverTaskBudgetGrant, DriverTaskCommandRecord,
    DriverTaskCompletionCode, DriverTaskCompletionRecord, DriverTaskContract, DriverTaskFaultCode,
    DriverTaskHotPath, CYW43_WIFI_DRIVER_TASK_CONTRACT, GENET_DRIVER_TASK_CONTRACT,
    MAX_DRIVER_TASK_FRAME_BYTES, SDIO_HOST_DRIVER_TASK_CONTRACT,
};
use crate::hal::{pi4_wifi, HalError, Hardware};
use crate::net::{
    ConsoleNetConfig, NetDevice, NetDeviceCounters, NetDriverError, NetInterfacePolicy, NetStage,
    MAX_FRAME_LEN,
};
use pi4_driver_abi::{
    DriverRuntimeCyw43CommandDescriptor, DriverRuntimeSdioCommandDescriptor,
    DRIVER_RUNTIME_CYW43_COMMAND_AUX, DRIVER_RUNTIME_CYW43_FLAG_CONTROL_EXT_HEADER,
    DRIVER_RUNTIME_CYW43_FLAG_FORCE_BYTE_MODE, DRIVER_RUNTIME_CYW43_OP_CONTROL_FRAME,
    DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL, DRIVER_RUNTIME_CYW43_OP_ETH_TX,
    DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK, DRIVER_RUNTIME_CYW43_OP_FIRMWARE_PREP,
    DRIVER_RUNTIME_CYW43_OP_NVRAM_CHUNK, DRIVER_RUNTIME_CYW43_OP_NVRAM_TAIL,
    DRIVER_RUNTIME_CYW43_OP_RELEASE, DRIVER_RUNTIME_CYW43_OP_RX_POLL,
    DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT, DRIVER_RUNTIME_NET_INIT_AUX,
    DRIVER_RUNTIME_SDIO_FLAG_RESP_NONE, DRIVER_RUNTIME_SDIO_FLAG_RESP_OCR,
    DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT, DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT_BUSY,
    DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG, DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES,
};

const GENET_DRIVER_TASK_MAC: EthernetAddress =
    EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x31]);
const CYW43_DRIVER_TASK_MAC: EthernetAddress =
    EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x32]);
const DRIVER_TASK_NET_STATUS: &str = "driver-task-ring-client";
const CYW43_RAM_BASE_4345: u32 = 0x0019_8000;
const CYW43_RAM_SIZE_4345_PI4: u32 = 0x000c_8000;
// Keep root-to-runtime firmware chunks aligned to the linked runtime's declared
// SDIO owner window so pre-release upload starts with Function 1 block-mode CMD53
// turns and retains byte-mode only for the explicit retry lane.
const CYW43_RUNTIME_FIRMWARE_STREAM_CHUNK_BYTES: usize =
    DRIVER_RUNTIME_SDIO_SHARED_PAYLOAD_BYTES as usize;
const CYW43_RUNTIME_STREAM_PROGRESS_INTERVAL: usize = 32 * 1024;
const CYW43_RUNTIME_STREAM_COMMAND_RETRIES: usize = 2;
const CYW43_BACKPLANE_ADDRESS_MASK: u32 = 0x7fff;
const CYW43_BACKPLANE_WINDOW_MASK: u32 = 0xffff_8000;
const CYW43_BACKPLANE_32BIT_FLAG: u32 = 0x8000;
const SDIO_CMD53_BYTE_MODE_MAX: u16 = 512;
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
const SDIO_GO_IDLE_COMMAND_INDEX: u32 = 0;
const SDIO_CMD5_OCR_COMMAND_INDEX: u32 = 5;
const SDIO_CMD3_RCA_COMMAND_INDEX: u32 = 3;
const SDIO_CMD7_SELECT_COMMAND_INDEX: u32 = 7;
const SDIO_R4_READY: u32 = 1 << 31;
const SDIO_OCR_3V2_3V4: u32 = 0x00ff_8000;
const SDIO_CMD5_READY_ATTEMPTS: usize = 16;
const SDIO_CMD7_SELECT_FALLBACK_COMMAND_INDEX: u32 = SDIO_CMD7_SELECT_COMMAND_INDEX;
const SDIO_CMD7_SELECT_ATTEMPTS: usize = 4;
const SDIO_CMD7_SELECT_SETTLE_SPINS: usize = 10_000;
const SDIO_STARTUP_CLOCK_HZ: u32 = 400_000;
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
static CYW43_HOST_EAPOL_SECURE: AtomicU32 = AtomicU32::new(0);
static SDIO_LINKED_RUNTIME_READY: AtomicU32 = AtomicU32::new(0);
static SDIO_HAL_RESOURCE_READY: AtomicU32 = AtomicU32::new(0);
static GENET_RUNTIME: Mutex<Option<BcmGenetDevice>> = Mutex::new(None);
static CYW43_RUNTIME: Mutex<Option<Cyw43NetDevice>> = Mutex::new(None);
static NET_RUNTIME_INIT_LEASE: Mutex<Option<NetRuntimeInitLease>> = Mutex::new(None);
#[cfg(feature = "kernel")]
static CYW43_LAST_RUNTIME_COMMAND_FAULT: Mutex<Option<Cyw43RuntimeCommandFaultStatus>> =
    Mutex::new(None);
#[cfg(feature = "kernel")]
static CYW43_LAST_SDIO_OWNER_FAULT: Mutex<Option<Cyw43SdioOwnerFaultStatus>> = Mutex::new(None);

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Cyw43RuntimeCommandFaultStatus {
    pub stage: &'static str,
    pub op: u16,
    pub flags: u16,
    pub target_addr: u32,
    pub payload_len: u16,
    pub total_len: u32,
    pub detail: u16,
    pub reason: &'static str,
    pub result: u32,
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
pub(crate) fn latest_cyw43_runtime_command_fault_status() -> Option<Cyw43RuntimeCommandFaultStatus>
{
    *CYW43_LAST_RUNTIME_COMMAND_FAULT.lock()
}

#[cfg(feature = "kernel")]
pub(crate) fn latest_cyw43_sdio_owner_fault_status() -> Option<Cyw43SdioOwnerFaultStatus> {
    *CYW43_LAST_SDIO_OWNER_FAULT.lock()
}

#[cfg(feature = "kernel")]
fn clear_cyw43_runtime_command_fault_status() {
    *CYW43_LAST_RUNTIME_COMMAND_FAULT.lock() = None;
    *CYW43_LAST_SDIO_OWNER_FAULT.lock() = None;
}

#[cfg(feature = "kernel")]
fn record_sdio_runtime_command_fault_status(
    stage: &'static str,
    completion: DriverTaskCompletionRecord,
) {
    if completion.code != DriverTaskCompletionCode::Fault.as_u16() {
        return;
    }
    *CYW43_LAST_RUNTIME_COMMAND_FAULT.lock() = Some(Cyw43RuntimeCommandFaultStatus {
        stage,
        op: 0,
        flags: 0,
        target_addr: 0,
        payload_len: 0,
        total_len: 0,
        detail: completion.detail,
        reason: cyw43_runtime_fault_reason(completion.detail),
        result: completion.result,
    });
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
    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "NET_DRIVER_TASK_REPLAY_STATUS role={} selected={} policy={} attempted=yes stage={} blocker={}",
        hot_path.as_str(),
        selected,
        config.policy.interface.as_str(),
        stage,
        status,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_sdio_driver_task_replay_status(stage: &'static str, status: &'static str) {
    use core::fmt::Write;

    let mut line = heapless::String::<160>::new();
    let _ = write!(
        line,
        "SDIO_DRIVER_TASK_REPLAY_STATUS role=sdio-host selected=wifi-owner-link attempted=yes stage={} blocker={}",
        stage, status,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

type NetRuntimeInitFn = unsafe fn(
    usize,
    ConsoleNetConfig,
    NetStage,
    DriverTaskHotPath,
) -> Result<(), DriverTaskNetError>;

#[derive(Clone, Copy)]
struct NetRuntimeInitLease {
    hal_ptr: usize,
    config: ConsoleNetConfig,
    stage: NetStage,
    init: NetRuntimeInitFn,
}

// SAFETY: The physical Pi driver-task path serializes access through one
// contract ring service turn at a time. Root holds only the ring client; the
// service state is protected by `GENET_RUNTIME`.
unsafe impl Send for BcmGenetDevice {}

// SAFETY: The physical Pi driver-task path serializes access through one
// contract ring service turn at a time. Root holds only the ring client; the
// service state is protected by `CYW43_RUNTIME`.
unsafe impl Send for Cyw43NetDevice {}

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

    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        contract,
        hot_path.as_u32() as usize,
        runtime_ring_service,
    );
    *NET_RUNTIME_INIT_LEASE.lock() = Some(NetRuntimeInitLease {
        hal_ptr: hal as *mut H as usize,
        config,
        stage,
        init: init_runtime_for_hal::<H>,
    });
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
    let result = run_driver_task_net_service(contract, command).is_some_and(|completion| {
        completion.code == DriverTaskCompletionCode::Progress.as_u16() && completion.result == 1
    });
    if result {
        Ok(())
    } else {
        let _ = NET_RUNTIME_INIT_LEASE.lock().take();
        Err(DriverTaskNetError::RuntimeInit(hot_path.as_str()))
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
    clear_cyw43_runtime_command_fault_status();
    let mut recovered = false;
    loop {
        match complete_cyw43_linked_runtime_firmware_once(hal, contract, bundle, reset_vector) {
            Ok(()) => {
                clear_cyw43_runtime_command_fault_status();
                return Ok(());
            }
            Err(err) => {
                if !recovered {
                    if let Some(completion) = err.recoverable_completion() {
                        let resume_offset =
                            latest_cyw43_runtime_command_fault_status().and_then(|fault| {
                                cyw43_firmware_resume_offset(fault, bundle.firmware.len())
                            });
                        crate::hal::driver_task::emit_driver_task_resource_init_status(
                            contract,
                            DriverTaskHotPath::Cyw43Wifi,
                            "cyw43-firmware-recover",
                            "sdio-owner-replay",
                            Some(completion),
                        );
                        replay_sdio_host_linked_runtime_preserving_hal(
                            hal,
                            "cyw43-firmware-recover",
                        )?;
                        if let Some(resume_offset) = resume_offset {
                            crate::hal::driver_task::emit_driver_task_resource_init_status(
                                contract,
                                DriverTaskHotPath::Cyw43Wifi,
                                "cyw43-firmware-recover",
                                "resume-retained-stage",
                                Some(completion),
                            );
                            complete_cyw43_linked_runtime_firmware_from_offset(
                                hal,
                                contract,
                                bundle,
                                reset_vector,
                                resume_offset,
                            )
                            .map_err(Cyw43FirmwareInitError::into_net_error)?;
                            clear_cyw43_runtime_command_fault_status();
                            return Ok(());
                        }
                        recovered = true;
                        continue;
                    }
                }
                return Err(err.into_net_error());
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
    reset_cyw43_control_plane_state();
    let bundle = hal
        .wifi_firmware_bundle()
        .map_err(|_| DriverTaskNetError::RuntimeInit("cyw43-control-firmware-bundle"))?;
    let device = Cyw43NetDevice::new_driver_task_runtime(&config, bundle)
        .map_err(|_err: Cyw43DriverError| DriverTaskNetError::RuntimeInit("cyw43-control-plane"))?;
    publish_cyw43_control_plane_counters(device.counters());
    *CYW43_RUNTIME.lock() = Some(device);
    Ok(())
}

#[cfg(feature = "kernel")]
fn reset_cyw43_control_plane_state() {
    CYW43_CONTROL_PLANE_READY.store(0, Ordering::Release);
    CYW43_ASSOCIATED.store(0, Ordering::Release);
    CYW43_LINK_UP.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_RX.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_START.store(0, Ordering::Release);
    CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);
}

#[cfg(feature = "kernel")]
fn publish_cyw43_control_plane_counters(counters: NetDeviceCounters) {
    CYW43_CONTROL_PLANE_READY.store(1, Ordering::Release);
    CYW43_ASSOCIATED.store(
        if counters.wifi_assoc != 0 { 1 } else { 0 },
        Ordering::Release,
    );
    CYW43_LINK_UP.store(
        if counters.wifi_link_up != 0 { 1 } else { 0 },
        Ordering::Release,
    );
    CYW43_HOST_EAPOL_RX.store(
        counters.wifi_host_eapol_rx.min(u64::from(u32::MAX)) as u32,
        Ordering::Release,
    );
    CYW43_HOST_EAPOL_START.store(
        counters.wifi_host_eapol_start.min(u64::from(u32::MAX)) as u32,
        Ordering::Release,
    );
    CYW43_HOST_EAPOL_SECURE.store(
        if counters.wifi_host_eapol_secure != 0 {
            1
        } else {
            0
        },
        Ordering::Release,
    );
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
    submit_cyw43_runtime_command_checked(
        contract,
        DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_TRANSPORT_INIT,
            ..DriverRuntimeCyw43CommandDescriptor::empty()
        },
        &[],
    )
    .map_err(Cyw43FirmwareInitError::Command)?;
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
fn complete_cyw43_linked_runtime_firmware_from_offset<H>(
    hal: &mut H,
    contract: DriverTaskContract,
    bundle: crate::hal::WifiFirmwareBundle<'_>,
    reset_vector: u32,
    resume_offset: usize,
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
        false,
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
    let end = offset.checked_add(usize::from(fault.payload_len))?;
    if end <= firmware_len {
        Some(offset)
    } else {
        None
    }
}

#[cfg(feature = "kernel")]
fn prepare_sdio_hal_resources<H>(hal: &mut H) -> Result<(), DriverTaskNetError>
where
    H: Hardware<Error = HalError>,
{
    if SDIO_HAL_RESOURCE_READY.load(Ordering::Acquire) != 0 {
        emit_sdio_driver_task_replay_status("hal-resource-prep", "cached-ready");
        return Ok(());
    }
    emit_sdio_driver_task_replay_status("hal-resource-prep", "begin");
    match pi4_wifi::prepare_driver_task_sdio_resources(hal) {
        Ok(()) => {
            SDIO_HAL_RESOURCE_READY.store(1, Ordering::Release);
            emit_sdio_driver_task_replay_status("hal-resource-prep", "ready");
            Ok(())
        }
        Err(_) => {
            emit_sdio_driver_task_replay_status("hal-resource-prep", "failed");
            Err(DriverTaskNetError::RuntimeInit(
                "sdio-host-hal-resource-prep",
            ))
        }
    }
}

#[cfg(feature = "kernel")]
fn init_sdio_host_linked_runtime<H>(hal: &mut H) -> Result<(), DriverTaskNetError>
where
    H: Hardware<Error = HalError>,
{
    if SDIO_LINKED_RUNTIME_READY.load(Ordering::Acquire) != 0 {
        return Ok(());
    }
    let contract = SDIO_HOST_DRIVER_TASK_CONTRACT;
    let _ = crate::hal::driver_task::register_pi4_bus_ring_service(contract);
    prepare_sdio_hal_resources(hal)?;
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
    let status = driver_task_resource_completion_status(completion, initialized);
    emit_sdio_driver_task_replay_status("engine-init", status);
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::SdioHost,
        "sdio-engine-init",
        status,
        completion,
    );
    let card_init_ok = initialized && submit_sdio_card_init_probe(contract);
    if card_init_ok
        && crate::hal::driver_task::register_driver_task_runtime_owner_state(
            DriverTaskHotPath::SdioHost,
        )
    {
        SDIO_LINKED_RUNTIME_READY.store(1, Ordering::Release);
        emit_sdio_driver_task_replay_status("owner-state", "ready");
        Ok(())
    } else {
        if card_init_ok {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::SdioHost,
                "sdio-owner-state",
                "descriptor-rejected",
                None,
            );
            emit_sdio_driver_task_replay_status("owner-state", "descriptor-rejected");
        }
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
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        CYW43_WIFI_DRIVER_TASK_CONTRACT,
        DriverTaskHotPath::Cyw43Wifi,
        stage,
        "begin",
        None,
    );
    if sdio_owner_recovery_can_preserve_ready_state() {
        emit_sdio_driver_task_replay_status("hal-resource-prep", "preserved-ready");
        prepare_sdio_hal_resources(hal)?;
        emit_sdio_driver_task_replay_status("owner-state", "preserved-ready");
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::Cyw43Wifi,
            stage,
            "ready",
            None,
        );
        return Ok(());
    }
    SDIO_LINKED_RUNTIME_READY.store(0, Ordering::Release);
    emit_sdio_driver_task_replay_status("hal-resource-prep", "preserved-ready");
    match init_sdio_host_linked_runtime(hal) {
        Ok(()) => {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                CYW43_WIFI_DRIVER_TASK_CONTRACT,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "ready",
                None,
            );
            Ok(())
        }
        Err(err) => {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                CYW43_WIFI_DRIVER_TASK_CONTRACT,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "failed",
                None,
            );
            Err(err)
        }
    }
}

#[cfg(feature = "kernel")]
fn sdio_owner_recovery_can_preserve_ready_state() -> bool {
    SDIO_LINKED_RUNTIME_READY.load(Ordering::Acquire) != 0
}

#[cfg(feature = "kernel")]
fn submit_sdio_card_init_probe(contract: DriverTaskContract) -> bool {
    if !submit_sdio_host_config_probe(
        contract,
        "sdio-host-startup-config",
        SDIO_STARTUP_CLOCK_HZ,
        0,
    ) {
        return false;
    }
    if submit_sdio_command_probe(
        contract,
        "sdio-cmd0-go-idle",
        SDIO_GO_IDLE_COMMAND_INDEX,
        0,
        DRIVER_RUNTIME_SDIO_FLAG_RESP_NONE,
        true,
    )
    .is_none()
    {
        return false;
    }
    let Some(ocr) = submit_sdio_command_probe(
        contract,
        "sdio-cmd5-ocr",
        SDIO_CMD5_OCR_COMMAND_INDEX,
        0,
        DRIVER_RUNTIME_SDIO_FLAG_RESP_OCR,
        false,
    ) else {
        return false;
    };
    if ocr & SDIO_OCR_3V2_3V4 == 0 {
        return false;
    }
    let desired_ocr = ocr & SDIO_OCR_3V2_3V4;
    let mut ready_ocr = 0;
    for _ in 0..SDIO_CMD5_READY_ATTEMPTS {
        let Some(response) = submit_sdio_command_probe(
            contract,
            "sdio-cmd5-ready",
            SDIO_CMD5_OCR_COMMAND_INDEX,
            desired_ocr,
            DRIVER_RUNTIME_SDIO_FLAG_RESP_OCR,
            false,
        ) else {
            return false;
        };
        ready_ocr = response;
        if ready_ocr & SDIO_R4_READY != 0 {
            break;
        }
    }
    if ready_ocr & SDIO_R4_READY == 0 {
        return false;
    }
    let Some(rca_response) = submit_sdio_command_probe(
        contract,
        "sdio-cmd3-rca",
        SDIO_CMD3_RCA_COMMAND_INDEX,
        0,
        DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT,
        false,
    ) else {
        return false;
    };
    let rca = rca_response & 0xffff_0000;
    if rca == 0 {
        return false;
    }
    submit_sdio_cmd7_select_probe(contract, rca)
}

#[cfg(feature = "kernel")]
fn sdio_cmd7_select_settle() {
    for _ in 0..SDIO_CMD7_SELECT_SETTLE_SPINS {
        core::hint::spin_loop();
    }
}

#[cfg(feature = "kernel")]
fn submit_sdio_command_probe_raw(
    contract: DriverTaskContract,
    command_index: u32,
    argument: u32,
    response_flags: u16,
) -> Option<DriverTaskCompletionRecord> {
    let mut command = DriverTaskCommandRecord::pi4_hot_path(
        0,
        DriverTaskHotPath::SdioHost,
        DriverTaskBudgetGrant::from_contract(contract),
        DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: 0,
        },
    );
    command.aux0 = (command_index << 16) | u32::from(response_flags);
    command.aux1 = argument;
    run_driver_task_net_service(contract, command)
}

#[cfg(feature = "kernel")]
fn sdio_command_probe_ready(
    completion: Option<DriverTaskCompletionRecord>,
    allow_zero_result: bool,
) -> bool {
    completion.is_some_and(|completion| {
        completion.code == DriverTaskCompletionCode::Progress.as_u16()
            && (allow_zero_result || completion.result != 0)
    })
}

#[cfg(feature = "kernel")]
fn report_sdio_command_probe(
    contract: DriverTaskContract,
    stage: &'static str,
    completion: Option<DriverTaskCompletionRecord>,
    ready: bool,
) {
    if ready {
        clear_cyw43_runtime_command_fault_status();
    } else if let Some(completion) = completion {
        record_sdio_runtime_command_fault_status(stage, completion);
    }
    let status = driver_task_resource_completion_status(completion, ready);
    emit_sdio_driver_task_replay_status(stage, status);
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        DriverTaskHotPath::SdioHost,
        stage,
        status,
        completion,
    );
}

#[cfg(feature = "kernel")]
fn submit_sdio_cmd7_select_probe(contract: DriverTaskContract, rca: u32) -> bool {
    let mut last_completion = None;
    emit_sdio_driver_task_replay_status("sdio-cmd7-select", "begin");
    for attempt in 0..SDIO_CMD7_SELECT_ATTEMPTS {
        if attempt != 0
            && !submit_sdio_host_config_probe(
                contract,
                "sdio-cmd7-select-host-recover",
                SDIO_STARTUP_CLOCK_HZ,
                0,
            )
        {
            break;
        }
        let completion = submit_sdio_command_probe_raw(
            contract,
            SDIO_CMD7_SELECT_COMMAND_INDEX,
            rca,
            DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT_BUSY,
        );
        if sdio_command_probe_ready(completion, true) {
            report_sdio_command_probe(contract, "sdio-cmd7-select", completion, true);
            return true;
        }
        last_completion = completion;
        sdio_cmd7_select_settle();
    }

    emit_sdio_driver_task_replay_status("sdio-cmd7-select-r1-fallback", "begin");
    for attempt in 0..SDIO_CMD7_SELECT_ATTEMPTS {
        if attempt != 0
            && !submit_sdio_host_config_probe(
                contract,
                "sdio-cmd7-select-r1-host-recover",
                SDIO_STARTUP_CLOCK_HZ,
                0,
            )
        {
            break;
        }
        let completion = submit_sdio_command_probe_raw(
            contract,
            SDIO_CMD7_SELECT_FALLBACK_COMMAND_INDEX,
            rca,
            DRIVER_RUNTIME_SDIO_FLAG_RESP_SHORT,
        );
        if sdio_command_probe_ready(completion, true) {
            report_sdio_command_probe(contract, "sdio-cmd7-select-r1-fallback", completion, true);
            return true;
        }
        last_completion = completion;
        sdio_cmd7_select_settle();
    }
    report_sdio_command_probe(
        contract,
        "sdio-cmd7-select-r1-fallback",
        last_completion,
        false,
    );
    false
}

#[cfg(feature = "kernel")]
fn submit_sdio_host_config_probe(
    contract: DriverTaskContract,
    stage: &'static str,
    target_hz: u32,
    flags: u16,
) -> bool {
    emit_sdio_driver_task_replay_status(stage, "begin");
    let descriptor = DriverRuntimeSdioCommandDescriptor {
        op: DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG,
        addr: target_hz,
        flags,
        timeout_us: 100_000,
        ..DriverRuntimeSdioCommandDescriptor::empty()
    };
    if !descriptor.valid() {
        report_sdio_command_probe(contract, stage, None, false);
        return false;
    }
    let desc_size = core::mem::size_of::<DriverRuntimeSdioCommandDescriptor>();
    let mut scratch = [0u8; core::mem::size_of::<DriverRuntimeSdioCommandDescriptor>()];
    encode_sdio_descriptor(&mut scratch, descriptor);
    let Some(staged) = crate::hal::driver_task::stage_driver_task_ring_frame(contract, &scratch, 0)
    else {
        report_sdio_command_probe(contract, stage, None, false);
        return false;
    };
    let command = DriverTaskCommandRecord::pi4_hot_path(
        0,
        DriverTaskHotPath::SdioHost,
        DriverTaskBudgetGrant::from_contract(contract),
        DriverFrameDescriptor {
            offset: staged.offset,
            len: desc_size as u16,
            flags: staged.flags,
        },
    );
    let completion = run_driver_task_net_service(contract, command);
    let ready = sdio_command_probe_ready(completion, false);
    report_sdio_command_probe(contract, stage, completion, ready);
    ready
}

#[cfg(feature = "kernel")]
fn submit_sdio_command_probe(
    contract: DriverTaskContract,
    stage: &'static str,
    command_index: u32,
    argument: u32,
    response_flags: u16,
    allow_zero_result: bool,
) -> Option<u32> {
    emit_sdio_driver_task_replay_status(stage, "begin");
    let completion =
        submit_sdio_command_probe_raw(contract, command_index, argument, response_flags);
    let ready = sdio_command_probe_ready(completion, allow_zero_result);
    report_sdio_command_probe(contract, stage, completion, ready);
    completion
        .filter(|_| ready)
        .map(|completion| completion.result)
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
        if force_first_chunk_byte_mode && offset == start_offset {
            descriptor.flags |= DRIVER_RUNTIME_CYW43_FLAG_FORCE_BYTE_MODE;
        }
        let mut attempts = 0usize;
        loop {
            match submit_cyw43_runtime_command_checked(
                contract,
                descriptor,
                &payload[offset..offset + chunk_len],
            ) {
                Ok(_) => break,
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
            crate::hal::driver_task::stage_driver_task_shared_payload(contract, payload, 0)
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
        let Some(staged) = crate::hal::driver_task::stage_driver_task_ring_frame(
            contract,
            &ring_payload[..desc_size + payload.len()],
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
        return submit_staged_cyw43_runtime_descriptor(
            contract, descriptor, stage, desc_size, staged, None,
        );
    }
    let Some(staged) = crate::hal::driver_task::stage_driver_task_ring_payload_at(
        contract,
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
    submit_staged_cyw43_runtime_descriptor(
        contract,
        descriptor,
        stage,
        desc_size,
        staged,
        use_shared_payload.then_some(payload),
    )
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
    let Some(completion) = run_driver_task_net_service(contract, command) else {
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
    if completion.code == DriverTaskCompletionCode::Progress.as_u16() && completion.result != 0 {
        if cyw43_command_stage_always_logs_success(descriptor.op) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                DriverTaskHotPath::Cyw43Wifi,
                stage,
                "ready",
                Some(completion),
            );
        }
        Ok(completion)
    } else {
        let status = if completion.code == DriverTaskCompletionCode::Fault.as_u16() {
            "fault"
        } else {
            "unexpected-completion"
        };
        emit_cyw43_runtime_command_fault(contract, stage, descriptor, completion, producer_payload);
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            DriverTaskHotPath::Cyw43Wifi,
            stage,
            status,
            Some(completion),
        );
        Err(Cyw43CommandSubmitError::Completion(completion))
    }
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
    *CYW43_LAST_RUNTIME_COMMAND_FAULT.lock() = Some(Cyw43RuntimeCommandFaultStatus {
        stage,
        op: descriptor.op,
        flags: descriptor.flags,
        target_addr: descriptor.target_addr,
        payload_len: descriptor.payload_len,
        total_len: descriptor.total_len,
        detail: completion.detail,
        reason: cyw43_runtime_fault_reason(completion.detail),
        result: completion.result,
    });
    *CYW43_LAST_SDIO_OWNER_FAULT.lock() = None;
    let mut line = heapless::String::<320>::new();
    let _ = write!(
        line,
        "CYW43_DRIVER_TASK_COMMAND_FAULT contract={} stage={} op={} flags=0x{:04x} target=0x{:08x} payload_len={} total_len={} detail={} reason={} result={}",
        contract.name,
        stage,
        descriptor.op,
        descriptor.flags,
        descriptor.target_addr,
        descriptor.payload_len,
        descriptor.total_len,
        completion.detail,
        cyw43_runtime_fault_reason(completion.detail),
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
        "forced-byte-mode"
    } else if !snapshot.cmd53_block_mode() {
        if detail == 0x5329 && snapshot.len < SDIO_CMD53_BYTE_MODE_MAX {
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
const fn cyw43_fault_detail_allows_sdio_owner_recovery(detail: u16) -> bool {
    matches!(
        detail,
        0x5101
            | 0x5102
            | 0x5103
            | 0x5104
            | 0x5310
            | 0x531a
            | 0x531b
            | 0x531c
            | 0x531d
            | 0x531e
            | 0x531f
            | 0x5321
            | 0x5323
            | 0x5329
            | 0x532a
            | 0x532b
            | 0x532c
    )
}

#[cfg(feature = "kernel")]
const fn cyw43_fault_detail_allows_same_command_retry(detail: u16) -> bool {
    matches!(detail, 0x5103)
}

#[cfg(feature = "kernel")]
pub(crate) const fn cyw43_runtime_fault_reason(detail: u16) -> &'static str {
    match detail {
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
        0x53ff => "cyw43-command",
        _ => "unknown",
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

unsafe fn init_runtime_for_hal<H>(
    hal_ptr: usize,
    config: ConsoleNetConfig,
    stage: NetStage,
    hot_path: DriverTaskHotPath,
) -> Result<(), DriverTaskNetError>
where
    H: Hardware<Error = HalError>,
{
    // SAFETY: `init_runtime_via_driver_task` publishes this temporary HAL lease
    // immediately before a synchronous driver-task init command. Root does not
    // touch the borrowed HAL again until the completion record is observed, and
    // the lease is cleared before steady-state service begins.
    let hal = unsafe { &mut *(hal_ptr as *mut H) };
    match hot_path {
        DriverTaskHotPath::GenetNic => {
            let device = BcmGenetDevice::create_with_stage(hal, &config, stage)
                .map_err(|_err: BcmGenetDriverError| DriverTaskNetError::RuntimeInit("genet"))?;
            *GENET_RUNTIME.lock() = Some(device);
            Ok(())
        }
        DriverTaskHotPath::Cyw43Wifi => {
            let device = Cyw43NetDevice::create_with_stage(hal, &config, stage)
                .map_err(|_err: Cyw43DriverError| DriverTaskNetError::RuntimeInit("cyw43"))?;
            *CYW43_RUNTIME.lock() = Some(device);
            Ok(())
        }
        _ => Err(DriverTaskNetError::RuntimeInit("net-hot-path")),
    }
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
    if command.frame.len != 0 {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::RejectedCommand,
        );
    }
    let Some(lease) = NET_RUNTIME_INIT_LEASE.lock().take() else {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::DeviceUnavailable,
        );
    };
    // SAFETY: The lease was installed by the synchronous root-side init caller
    // for this exact driver-task turn and is consumed before steady state.
    match unsafe { (lease.init)(lease.hal_ptr, lease.config, lease.stage, hot_path) } {
        Ok(()) => DriverTaskCompletionRecord::progress(command.sequence, 1),
        Err(_) => DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::DeviceUnavailable,
        ),
    }
}

fn service_genet(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    let mut runtime = GENET_RUNTIME.lock();
    let Some(device) = runtime.as_mut() else {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::DeviceUnavailable,
        );
    };
    service_device(
        GENET_DRIVER_TASK_CONTRACT,
        DriverTaskHotPath::GenetNic,
        device,
        command,
    )
}

fn service_cyw43(command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
    let mut runtime = CYW43_RUNTIME.lock();
    let Some(device) = runtime.as_mut() else {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::DeviceUnavailable,
        );
    };
    service_device(
        CYW43_WIFI_DRIVER_TASK_CONTRACT,
        DriverTaskHotPath::Cyw43Wifi,
        device,
        command,
    )
}

fn runtime_ready(hot_path: DriverTaskHotPath) -> bool {
    match hot_path {
        DriverTaskHotPath::GenetNic => {
            GENET_LINKED_RUNTIME_READY.load(Ordering::Acquire) != 0
                || GENET_RUNTIME.lock().is_some()
        }
        DriverTaskHotPath::Cyw43Wifi => {
            CYW43_LINKED_RUNTIME_READY.load(Ordering::Acquire) != 0
                || CYW43_RUNTIME.lock().is_some()
        }
        _ => false,
    }
}

fn cyw43_driver_task_bringup_status_label() -> Option<&'static str> {
    if !runtime_ready(DriverTaskHotPath::Cyw43Wifi) {
        return Some(DRIVER_TASK_NET_STATUS);
    }
    if CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) != 0 {
        return None;
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
        DriverTaskHotPath::GenetNic => GENET_RUNTIME.lock().as_ref().map(NetDevice::mac),
        DriverTaskHotPath::Cyw43Wifi => CYW43_RUNTIME.lock().as_ref().map(NetDevice::mac),
        _ => None,
    }
}

fn service_device<D>(
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
    device: &mut D,
    command: DriverTaskCommandRecord,
) -> DriverTaskCompletionRecord
where
    D: NetDevice,
{
    if command.opcode != hot_path.opcode().as_u16()
        || command.arg0 != hot_path.as_u32()
        || command.arg1 != hot_path.role_bit() as u32
    {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::RejectedCommand,
        );
    }

    if command.frame.len != 0 {
        return service_tx(contract, device, command);
    }
    service_rx(contract, device, command)
}

fn service_tx<D>(
    contract: DriverTaskContract,
    device: &mut D,
    command: DriverTaskCommandRecord,
) -> DriverTaskCompletionRecord
where
    D: NetDevice,
{
    let Some(frame) =
        crate::hal::driver_task::driver_task_ring_frame_bytes(contract, command.frame)
    else {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::RejectedCommand,
        );
    };
    let Some(tx) = device.transmit(Instant::from_millis(0)) else {
        return DriverTaskCompletionRecord::idle(command.sequence);
    };
    let len = frame.len().min(MAX_FRAME_LEN);
    tx.consume(len, |buffer| {
        buffer[..len].copy_from_slice(&frame[..len]);
    });
    DriverTaskCompletionRecord::progress(command.sequence, len as u32)
}

fn service_rx<D>(
    contract: DriverTaskContract,
    device: &mut D,
    command: DriverTaskCommandRecord,
) -> DriverTaskCompletionRecord
where
    D: NetDevice,
{
    let Some((rx, _tx)) = device.receive(Instant::from_millis(0)) else {
        return DriverTaskCompletionRecord::idle(command.sequence);
    };
    let mut scratch = [0u8; MAX_FRAME_LEN];
    let len = rx.consume(|frame| {
        let len = frame.len().min(MAX_FRAME_LEN);
        scratch[..len].copy_from_slice(&frame[..len]);
        len
    });
    let Some(descriptor) =
        crate::hal::driver_task::stage_driver_task_ring_frame(contract, &scratch[..len], 0)
    else {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::DeviceUnavailable,
        );
    };
    DriverTaskCompletionRecord::frame_ready(command.sequence, descriptor)
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
    let Some(descriptor) =
        crate::hal::driver_task::stage_driver_task_ring_frame(contract, frame, 0)
    else {
        return false;
    };
    let command = DriverTaskCommandRecord::pi4_hot_path(
        0,
        hot_path,
        DriverTaskBudgetGrant::from_contract(contract),
        descriptor,
    );
    run_driver_task_net_service(contract, command).is_some_and(driver_task_tx_completion_submitted)
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
pub(crate) fn submit_cyw43_driver_task_eth_payload(frame: &[u8]) -> bool {
    submit_cyw43_driver_task_eth_frame(CYW43_WIFI_DRIVER_TASK_CONTRACT, frame)
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
    if !payload.is_empty() {
        let payload_offset = crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET + 512;
        let staged_payload = crate::hal::driver_task::stage_driver_task_ring_payload_at(
            contract,
            payload_offset,
            payload,
            0,
        )?;
        descriptor.payload_offset = u16::try_from(staged_payload.offset).ok()?;
        descriptor.payload_len = staged_payload.len;
        if descriptor.total_len == 0 {
            descriptor.total_len = u32::from(staged_payload.len);
        }
    } else {
        descriptor.payload_offset = 0;
        descriptor.payload_len = 0;
        descriptor.total_len = 0;
    }
    let mut scratch = [0u8; core::mem::size_of::<DriverRuntimeCyw43CommandDescriptor>()];
    encode_cyw43_descriptor(&mut scratch, descriptor);
    let staged_descriptor = crate::hal::driver_task::stage_driver_task_ring_payload_at(
        contract,
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
    run_driver_task_net_service(contract, command)
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
    let contract = CYW43_WIFI_DRIVER_TASK_CONTRACT;
    let completion = run_cyw43_runtime_descriptor_command(
        contract,
        DriverRuntimeCyw43CommandDescriptor {
            op: DRIVER_RUNTIME_CYW43_OP_CONTROL_POLL,
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
    Some((completion.frame.flags, DriverTaskNetRxToken { len, buffer }))
}

#[cfg(feature = "kernel")]
pub(crate) fn poll_cyw43_driver_task_data_frame() -> Option<(u16, DriverTaskNetRxToken)> {
    let contract = CYW43_WIFI_DRIVER_TASK_CONTRACT;
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
                    && CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) == 0
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
                    && CYW43_HOST_EAPOL_SECURE.load(Ordering::Acquire) == 0
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
        CYW43_HOST_EAPOL_SECURE.store(0, Ordering::Release);
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
        assert!(!cyw43_fault_detail_allows_sdio_owner_recovery(0x5302));
        assert!(!cyw43_fault_detail_allows_sdio_owner_recovery(0x5306));
        assert!(!cyw43_fault_detail_allows_sdio_owner_recovery(0x53ff));
        assert!(cyw43_fault_detail_allows_same_command_retry(0x5103));
        assert!(!cyw43_fault_detail_allows_same_command_retry(0x5102));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn sdio_host_config_descriptor_encoder_matches_runtime_abi() {
        let descriptor = DriverRuntimeSdioCommandDescriptor {
            op: DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG,
            addr: SDIO_STARTUP_CLOCK_HZ,
            flags: DriverRuntimeSdioCommandDescriptor::FLAG_HOST_BUS_WIDTH_4BIT,
            timeout_us: 100_000,
            ..DriverRuntimeSdioCommandDescriptor::empty()
        };
        let mut bytes = [0u8; core::mem::size_of::<DriverRuntimeSdioCommandDescriptor>()];

        encode_sdio_descriptor(&mut bytes, descriptor);

        assert_eq!(
            &bytes[0..2],
            &DRIVER_RUNTIME_SDIO_OP_HOST_CONFIG.to_le_bytes()
        );
        assert_eq!(bytes[2], 0);
        assert_eq!(bytes[3], pi4_driver_abi::DRIVER_RUNTIME_SDIO_RESP_NONE);
        assert_eq!(&bytes[4..8], &SDIO_STARTUP_CLOCK_HZ.to_le_bytes());
        assert_eq!(
            &bytes[16..18],
            &DriverRuntimeSdioCommandDescriptor::FLAG_HOST_BUS_WIDTH_4BIT.to_le_bytes()
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
            "byte-narrow-fallback"
        );
        assert_eq!(
            cyw43_owner_retry_label(byte, descriptor, 0x5329),
            "byte-narrow-fallback-exhausted"
        );
        assert_eq!(
            cyw43_owner_retry_label(byte512, descriptor, 0x5103),
            "byte-fallback"
        );
        assert_eq!(
            cyw43_owner_retry_label(byte512, descriptor, 0x5329),
            "byte-fallback-exhausted"
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
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_firmware_resume_offset_accepts_only_matching_firmware_faults() {
        let fault = Cyw43RuntimeCommandFaultStatus {
            stage: "cyw43-firmware-chunk",
            op: DRIVER_RUNTIME_CYW43_OP_FIRMWARE_CHUNK,
            flags: DRIVER_RUNTIME_CYW43_FLAG_FORCE_BYTE_MODE,
            target_addr: CYW43_RAM_BASE_4345 + 0x1c00,
            payload_len: 1024,
            total_len: 609_309,
            detail: 0x5103,
            reason: "sdio-descriptor-transfer-failed",
            result: 0x0500_0100,
        };

        assert_eq!(cyw43_firmware_resume_offset(fault, 609_309), Some(0x1c00));

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
