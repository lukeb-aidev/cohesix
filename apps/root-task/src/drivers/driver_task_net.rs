// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide Pi 4 NIC clients that route hardware ownership to driver tasks.
// Author: Lukas Bower

//! Driver-task-owned Pi 4 NIC client shells.
//!
//! These devices intentionally do not map GENET, CYW43, or SDIO registers into
//! root. They give the root TCP console a smoltcp-compatible client endpoint
//! while physical hardware ownership is cut over to isolated driver-task
//! runtimes. Until those runtimes service real RX/TX rings, these clients are
//! inert and report the runtime-pending bring-up state.

#![allow(unsafe_code)]

use core::fmt;

use smoltcp::phy::{self, Device, DeviceCapabilities};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;

use crate::hal::driver_task::{
    DriverTaskContract, CYW43_WIFI_DRIVER_TASK_CONTRACT, GENET_DRIVER_TASK_CONTRACT,
};
use crate::hal::{HalError, Hardware};
use crate::net::{
    ConsoleNetConfig, NetDevice, NetDeviceCounters, NetDriverError, NetStage, MAX_FRAME_LEN,
};

const GENET_DRIVER_TASK_MAC: EthernetAddress =
    EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x31]);
const CYW43_DRIVER_TASK_MAC: EthernetAddress =
    EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x32]);
const DRIVER_TASK_NET_STATUS: &str = "driver-task-runtime-pending";

/// Error surfaced by driver-task NIC clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverTaskNetError {
    /// The isolated driver runtime is not yet servicing hardware rings.
    RuntimePending(&'static str),
}

impl fmt::Display for DriverTaskNetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimePending(role) => {
                write!(f, "{role} driver-task runtime is pending hardware service")
            }
        }
    }
}

impl NetDriverError for DriverTaskNetError {
    fn is_absent(&self) -> bool {
        false
    }
}

/// Inert RX token used only to satisfy the smoltcp device trait.
pub struct DriverTaskNetRxToken;

impl phy::RxToken for DriverTaskNetRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&[])
    }
}

/// Inert TX token used only to satisfy the smoltcp device trait.
pub struct DriverTaskNetTxToken;

impl phy::TxToken for DriverTaskNetTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut scratch = [0u8; MAX_FRAME_LEN];
        f(&mut scratch[..len.min(MAX_FRAME_LEN)])
    }
}

macro_rules! driver_task_nic {
    ($name:ident, $label:literal, $iface:literal, $mac:ident, $contract:ident) => {
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
                None
            }

            fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
                self.tx_drops = self.tx_drops.saturating_add(1);
                None
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
                $mac
            }

            fn tx_drop_count(&self) -> u32 {
                self.tx_drops
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
                Some(DRIVER_TASK_NET_STATUS)
            }

            fn driver_task_runtime_client() -> bool
            where
                Self: Sized,
            {
                true
            }

            fn debug_snapshot(&mut self) {}

            fn counters(&self) -> NetDeviceCounters {
                NetDeviceCounters::default()
            }
        }
    };
}

driver_task_nic!(
    GenetDriverTaskDevice,
    "bcmgenet-v5-driver-task",
    "wired",
    GENET_DRIVER_TASK_MAC,
    GENET_DRIVER_TASK_CONTRACT
);
driver_task_nic!(
    Cyw43DriverTaskDevice,
    "cyw43455-driver-task",
    "wifi",
    CYW43_DRIVER_TASK_MAC,
    CYW43_WIFI_DRIVER_TASK_CONTRACT
);
