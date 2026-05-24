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
};
use crate::hal::{HalError, Hardware};
use crate::net::{
    ConsoleNetConfig, NetDevice, NetDeviceCounters, NetDriverError, NetStage, MAX_FRAME_LEN,
};
use pi4_driver_abi::DRIVER_RUNTIME_NET_INIT_AUX;

const GENET_DRIVER_TASK_MAC: EthernetAddress =
    EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x31]);
const CYW43_DRIVER_TASK_MAC: EthernetAddress =
    EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x32]);
const DRIVER_TASK_NET_STATUS: &str = "driver-task-ring-client";
static GENET_TX_SUBMITTED: AtomicU32 = AtomicU32::new(0);
static GENET_TX_DROPPED: AtomicU32 = AtomicU32::new(0);
static GENET_RX_FRAMES: AtomicU32 = AtomicU32::new(0);
static CYW43_TX_SUBMITTED: AtomicU32 = AtomicU32::new(0);
static CYW43_TX_DROPPED: AtomicU32 = AtomicU32::new(0);
static CYW43_RX_FRAMES: AtomicU32 = AtomicU32::new(0);
static GENET_LINKED_RUNTIME_READY: AtomicU32 = AtomicU32::new(0);
static CYW43_LINKED_RUNTIME_READY: AtomicU32 = AtomicU32::new(0);
static GENET_RUNTIME: Mutex<Option<BcmGenetDevice>> = Mutex::new(None);
static CYW43_RUNTIME: Mutex<Option<Cyw43NetDevice>> = Mutex::new(None);
static NET_RUNTIME_INIT_LEASE: Mutex<Option<NetRuntimeInitLease>> = Mutex::new(None);

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
    /// The isolated driver runtime is not yet servicing hardware rings.
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
        let initialized = crate::hal::driver_task::run_driver_task_ring_service(contract, command)
            .is_some_and(|completion| {
                completion.code == DriverTaskCompletionCode::Progress.as_u16()
                    && completion.result == 1
            });
        if initialized {
            match hot_path {
                DriverTaskHotPath::GenetNic => {
                    GENET_LINKED_RUNTIME_READY.store(1, Ordering::Release);
                }
                DriverTaskHotPath::Cyw43Wifi => {
                    CYW43_LINKED_RUNTIME_READY.store(1, Ordering::Release);
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
    let result = crate::hal::driver_task::run_driver_task_ring_service(contract, command)
        .is_some_and(|completion| {
            completion.code == DriverTaskCompletionCode::Progress.as_u16() && completion.result == 1
        });
    if result {
        Ok(())
    } else {
        let _ = NET_RUNTIME_INIT_LEASE.lock().take();
        Err(DriverTaskNetError::RuntimeInit(hot_path.as_str()))
    }
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
    crate::hal::driver_task::run_driver_task_ring_service(contract, command)
        .is_some_and(driver_task_tx_completion_submitted)
}

#[cfg(feature = "kernel")]
fn driver_task_tx_completion_submitted(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    completion.code == DriverTaskCompletionCode::Progress.as_u16() && completion.result != 0
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
    let completion = crate::hal::driver_task::run_driver_task_ring_service(contract, command)?;
    if completion.code != DriverTaskCompletionCode::FrameReady.as_u16() {
        return None;
    }
    let bytes = crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)?;
    let len = bytes.len().min(MAX_FRAME_LEN);
    let mut buffer = [0u8; MAX_FRAME_LEN];
    buffer[..len].copy_from_slice(&bytes[..len]);
    Some(DriverTaskNetRxToken { len, buffer })
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
        let mut dev = Cyw43DriverTaskDevice::default();
        assert!(dev.receive(Instant::from_millis(0)).is_none());
        assert_eq!(dev.bringup_status_label(), Some("driver-task-ring-client"));
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
