// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Apply manifest-driven SMP affinity hints for root-task TCBs.
// Author: Lukas Bower
#![allow(dead_code)]

use core::fmt;

use crate::generated;
use heapless::String as HeaplessString;

#[cfg(feature = "kernel")]
use crate::sel4::seL4_CPtr;
#[cfg(feature = "kernel")]
use crate::sel4::BootInfoExt;
#[cfg(feature = "kernel")]
use sel4_sys;

/// Logical affinity roles mapped to manifest core pools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AffinityRole {
    Authority,
    NineDoor,
    Provider,
    Worker,
}

impl AffinityRole {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Authority => "authority",
            Self::NineDoor => "ninedoor",
            Self::Provider => "provider",
            Self::Worker => "worker",
        }
    }
}

/// Physical hardware driver with a manifest-selected TCB affinity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverAffinityTarget {
    Serial,
    UsbLocalSeat,
    HdmiText,
    BcmGenetV5,
    Cyw43455,
    Rtl8139,
    VirtioNet,
    SdioHost,
    PcieRoot,
}

impl DriverAffinityTarget {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Serial => "serial",
            Self::UsbLocalSeat => "usb-local-seat",
            Self::HdmiText => "hdmi-text",
            Self::BcmGenetV5 => "bcmgenet-v5",
            Self::Cyw43455 => "cyw43455",
            Self::Rtl8139 => "rtl8139",
            Self::VirtioNet => "virtio-net",
            Self::SdioHost => "sdio-host",
            Self::PcieRoot => "pcie-root",
        }
    }
}

/// Errors surfaced when validating or applying affinity hints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AffinityError {
    NodesMismatch {
        expected: u8,
        observed: u8,
    },
    InvalidCore {
        role: AffinityRole,
        core: u8,
        max_cores: u8,
    },
    InvalidDriverCore {
        driver: DriverAffinityTarget,
        core: u8,
        max_cores: u8,
    },
    DriverOnRootCore {
        driver: DriverAffinityTarget,
        core: u8,
    },
    Syscall {
        role: AffinityRole,
        core: u8,
        err: i32,
    },
    DriverSyscall {
        driver: DriverAffinityTarget,
        core: u8,
        err: i32,
    },
}

impl fmt::Display for AffinityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NodesMismatch { expected, observed } => write!(
                f,
                "affinity max_cores {} does not match kernel nodes {}",
                expected, observed
            ),
            Self::InvalidCore {
                role,
                core,
                max_cores,
            } => write!(
                f,
                "affinity core {} for role {} exceeds max_cores {}",
                core,
                role.label(),
                max_cores
            ),
            Self::InvalidDriverCore {
                driver,
                core,
                max_cores,
            } => write!(
                f,
                "affinity core {} for driver {} exceeds max_cores {}",
                core,
                driver.label(),
                max_cores
            ),
            Self::DriverOnRootCore { driver, core } => write!(
                f,
                "affinity core {} for driver {} is the root authority core",
                core,
                driver.label()
            ),
            Self::Syscall { role, core, err } => write!(
                f,
                "affinity syscall failed role={} core={} err={}",
                role.label(),
                core,
                err
            ),
            Self::DriverSyscall { driver, core, err } => write!(
                f,
                "affinity syscall failed driver={} core={} err={}",
                driver.label(),
                core,
                err
            ),
        }
    }
}

pub fn policy() -> generated::AffinityPolicy {
    generated::affinity_policy()
}

pub fn validate_policy(
    policy: &generated::AffinityPolicy,
    observed_nodes: u8,
) -> Result<(), AffinityError> {
    if !policy.enabled {
        return Ok(());
    }
    if policy.max_cores != observed_nodes {
        return Err(AffinityError::NodesMismatch {
            expected: policy.max_cores,
            observed: observed_nodes,
        });
    }
    if let Some(core) = policy.authority_core {
        if core >= policy.max_cores {
            return Err(AffinityError::InvalidCore {
                role: AffinityRole::Authority,
                core,
                max_cores: policy.max_cores,
            });
        }
    }
    for &core in policy.ninedoor_cores {
        if core >= policy.max_cores {
            return Err(AffinityError::InvalidCore {
                role: AffinityRole::NineDoor,
                core,
                max_cores: policy.max_cores,
            });
        }
    }
    for &core in policy.provider_cores {
        if core >= policy.max_cores {
            return Err(AffinityError::InvalidCore {
                role: AffinityRole::Provider,
                core,
                max_cores: policy.max_cores,
            });
        }
    }
    for &core in policy.worker_cores {
        if core >= policy.max_cores {
            return Err(AffinityError::InvalidCore {
                role: AffinityRole::Worker,
                core,
                max_cores: policy.max_cores,
            });
        }
    }
    let root_core = policy.authority_core.unwrap_or(0);
    for target in DRIVER_AFFINITY_TARGETS {
        validate_driver_core(policy, target, root_core)?;
    }
    Ok(())
}

pub fn select_core(
    policy: &generated::AffinityPolicy,
    role: AffinityRole,
    index: usize,
) -> Option<u8> {
    if !policy.enabled {
        return None;
    }
    match role {
        AffinityRole::Authority => policy.authority_core,
        AffinityRole::NineDoor => pick_core(policy.ninedoor_cores, index),
        AffinityRole::Provider => pick_core(policy.provider_cores, index),
        AffinityRole::Worker => pick_core(policy.worker_cores, index),
    }
}

/// Returns the manifest-selected non-root CPU core for a physical driver.
pub fn select_driver_core(
    policy: &generated::AffinityPolicy,
    driver: DriverAffinityTarget,
) -> Option<u8> {
    if !policy.enabled {
        return None;
    }
    match driver {
        DriverAffinityTarget::Serial => policy.drivers.serial,
        DriverAffinityTarget::UsbLocalSeat => policy.drivers.usb_local_seat,
        DriverAffinityTarget::HdmiText => policy.drivers.hdmi_text,
        DriverAffinityTarget::BcmGenetV5 => policy.drivers.bcmgenet_v5,
        DriverAffinityTarget::Cyw43455 => policy.drivers.cyw43455,
        DriverAffinityTarget::Rtl8139 => policy.drivers.rtl8139,
        DriverAffinityTarget::VirtioNet => policy.drivers.virtio_net,
        DriverAffinityTarget::SdioHost => policy.drivers.sdio_host,
        DriverAffinityTarget::PcieRoot => policy.drivers.pcie_root,
    }
}

const DRIVER_AFFINITY_TARGETS: [DriverAffinityTarget; 9] = [
    DriverAffinityTarget::Serial,
    DriverAffinityTarget::UsbLocalSeat,
    DriverAffinityTarget::HdmiText,
    DriverAffinityTarget::BcmGenetV5,
    DriverAffinityTarget::Cyw43455,
    DriverAffinityTarget::Rtl8139,
    DriverAffinityTarget::VirtioNet,
    DriverAffinityTarget::SdioHost,
    DriverAffinityTarget::PcieRoot,
];

fn validate_driver_core(
    policy: &generated::AffinityPolicy,
    driver: DriverAffinityTarget,
    root_core: u8,
) -> Result<(), AffinityError> {
    let Some(core) = select_driver_core(policy, driver) else {
        return Ok(());
    };
    if core >= policy.max_cores {
        return Err(AffinityError::InvalidDriverCore {
            driver,
            core,
            max_cores: policy.max_cores,
        });
    }
    if core == root_core {
        return Err(AffinityError::DriverOnRootCore { driver, core });
    }
    Ok(())
}

fn pick_core(cores: &[u8], index: usize) -> Option<u8> {
    if cores.is_empty() {
        None
    } else {
        Some(cores[index % cores.len()])
    }
}

pub fn format_core_assignments(
    policy: &generated::AffinityPolicy,
    core: u8,
) -> HeaplessString<128> {
    let mut buf = HeaplessString::new();
    push_core_assignment(&mut buf, policy.authority_core == Some(core), "authority");
    push_core_assignment(
        &mut buf,
        core_slice_has(policy.ninedoor_cores, core),
        "ninedoor",
    );
    push_core_assignment(
        &mut buf,
        core_slice_has(policy.provider_cores, core),
        "provider",
    );
    push_core_assignment(
        &mut buf,
        core_slice_has(policy.worker_cores, core),
        "worker",
    );
    push_core_assignment(&mut buf, policy.drivers.serial == Some(core), "serial");
    push_core_assignment(&mut buf, policy.drivers.usb_local_seat == Some(core), "usb");
    push_core_assignment(&mut buf, policy.drivers.hdmi_text == Some(core), "hdmi");
    push_core_assignment(&mut buf, policy.drivers.bcmgenet_v5 == Some(core), "genet");
    push_core_assignment(&mut buf, policy.drivers.cyw43455 == Some(core), "cyw43");
    push_core_assignment(&mut buf, policy.drivers.rtl8139 == Some(core), "rtl8139");
    push_core_assignment(&mut buf, policy.drivers.virtio_net == Some(core), "virtio");
    push_core_assignment(&mut buf, policy.drivers.sdio_host == Some(core), "sdio");
    push_core_assignment(&mut buf, policy.drivers.pcie_root == Some(core), "pcie");
    if buf.is_empty() {
        let _ = buf.push_str("none");
    }
    buf
}

#[cfg(feature = "kernel")]
pub fn format_core_assignments_for_live_driver_profile(
    policy: &generated::AffinityPolicy,
    core: u8,
) -> HeaplessString<128> {
    let mut buf = HeaplessString::new();
    push_core_assignment(&mut buf, policy.authority_core == Some(core), "authority");
    push_core_assignment(
        &mut buf,
        core_slice_has(policy.ninedoor_cores, core),
        "ninedoor",
    );
    push_core_assignment(
        &mut buf,
        core_slice_has(policy.provider_cores, core),
        "provider",
    );
    push_core_assignment(
        &mut buf,
        core_slice_has(policy.worker_cores, core),
        "worker",
    );
    push_live_driver_core_assignment(
        &mut buf,
        policy.drivers.serial == Some(core),
        "serial",
        crate::hal::driver_task::SERIAL_DRIVER_TASK_CONTRACT,
    );
    push_live_driver_core_assignment(
        &mut buf,
        policy.drivers.usb_local_seat == Some(core),
        "usb",
        crate::hal::driver_task::USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    );
    push_live_driver_core_assignment(
        &mut buf,
        policy.drivers.hdmi_text == Some(core),
        "hdmi",
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
    );
    push_live_driver_core_assignment(
        &mut buf,
        policy.drivers.bcmgenet_v5 == Some(core),
        "genet",
        crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
    );
    push_live_driver_core_assignment(
        &mut buf,
        policy.drivers.cyw43455 == Some(core),
        "cyw43",
        crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
    );
    push_live_driver_core_assignment(
        &mut buf,
        policy.drivers.rtl8139 == Some(core),
        "rtl8139",
        crate::hal::driver_task::RTL8139_DRIVER_TASK_CONTRACT,
    );
    push_live_driver_core_assignment(
        &mut buf,
        policy.drivers.virtio_net == Some(core),
        "virtio",
        crate::hal::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT,
    );
    push_live_driver_core_assignment(
        &mut buf,
        policy.drivers.sdio_host == Some(core),
        "sdio",
        crate::hal::driver_task::SDIO_HOST_DRIVER_TASK_CONTRACT,
    );
    push_live_driver_core_assignment(
        &mut buf,
        policy.drivers.pcie_root == Some(core),
        "pcie",
        crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
    );
    if buf.is_empty() {
        let _ = buf.push_str("none");
    }
    buf
}

fn push_core_assignment<const N: usize>(buf: &mut HeaplessString<N>, assigned: bool, label: &str) {
    if !assigned {
        return;
    }
    if !buf.is_empty() {
        let _ = buf.push(',');
    }
    let _ = buf.push_str(label);
}

#[cfg(feature = "kernel")]
fn push_live_driver_core_assignment<const N: usize>(
    buf: &mut HeaplessString<N>,
    assigned: bool,
    label: &str,
    contract: crate::hal::driver_task::DriverTaskContract,
) {
    if !crate::hal::driver_task::driver_task_contract_active_for_current_profile(contract) {
        return;
    }
    push_core_assignment(buf, assigned, label);
}

fn core_slice_has(cores: &[u8], core: u8) -> bool {
    cores.contains(&core)
}

#[cfg(feature = "kernel")]
fn apply_role_affinity(
    tcb: seL4_CPtr,
    role: AffinityRole,
    index: usize,
    policy: &generated::AffinityPolicy,
) -> Option<u8> {
    if !policy.enabled {
        return None;
    }
    let Some(core) = select_core(policy, role, index) else {
        return None;
    };
    if core >= policy.max_cores {
        ::log::error!(
            "[affinity] role={} index={} core={} exceeds max_cores={}",
            role.label(),
            index,
            core,
            policy.max_cores
        );
        return None;
    }
    if let Err(err) = crate::sel4::set_tcb_affinity(tcb, core) {
        ::log::error!(
            "[affinity] role={} index={} core={} apply failed err={}",
            role.label(),
            index,
            core,
            err
        );
        return None;
    }
    ::log::info!(
        "[affinity] role={} index={} core={} applied",
        role.label(),
        index,
        core
    );
    Some(core)
}

#[cfg(feature = "kernel")]
pub fn with_role_affinity<T>(
    role: AffinityRole,
    index: usize,
    policy: &generated::AffinityPolicy,
    f: impl FnOnce() -> T,
) -> T {
    let tcb = sel4_sys::seL4_CapInitThreadTCB;
    let applied = apply_role_affinity(tcb, role, index, policy);
    let result = f();
    if let Some(authority_core) = policy.authority_core {
        if applied.is_some() && Some(authority_core) != applied {
            if let Err(err) = crate::sel4::set_tcb_affinity(tcb, authority_core) {
                ::log::error!(
                    "[affinity] restore authority core={} failed err={}",
                    authority_core,
                    err
                );
            }
        }
    }
    result
}

#[cfg(not(feature = "kernel"))]
pub fn with_role_affinity<T>(
    _role: AffinityRole,
    _index: usize,
    _policy: &generated::AffinityPolicy,
    f: impl FnOnce() -> T,
) -> T {
    f()
}

#[cfg(all(feature = "kernel", sel4_config_debug_build))]
pub fn debug_dump_per_core<F>(policy: &generated::AffinityPolicy, mut emit: F)
where
    F: FnMut(&str),
{
    fn dump_kernel_scheduler_uart_locked() {
        crate::bootstrap::log::with_raw_uart_lock(|| {
            crate::sel4::debug_dump_scheduler();
            crate::sel4::debug_dump_cpu_info();
        });
    }

    let tcb = sel4_sys::seL4_CapInitThreadTCB;
    if !policy.enabled {
        emit("[smp] affinity disabled; dumping scheduler once");
        emit("[smp] note: kernel scheduler/CPU dump text is UART-only");
        dump_kernel_scheduler_uart_locked();
        return;
    }

    emit("[smp] note: kernel scheduler/CPU dump text is UART-only");

    let mut probe = |core: u8, tasks: &str| {
        let mut line = HeaplessString::<192>::new();
        let _ = fmt::write(
            &mut line,
            format_args!(
                "[smp] affinity probe core={} tasks={} task_allocation=manifest policy=live-driver-filter live_view=smp-activity",
                core, tasks
            ),
        );
        emit(line.as_str());
        if let Err(err) = crate::sel4::set_tcb_affinity_silent(tcb, core) {
            let mut err_line = HeaplessString::<96>::new();
            let _ = fmt::write(
                &mut err_line,
                format_args!("[smp] affinity core={} set failed err={}", core, err),
            );
            emit(err_line.as_str());
            return;
        }
        // Yield to give the scheduler a deterministic window to migrate the TCB.
        for _ in 0..2 {
            crate::sel4::yield_now();
        }
        dump_kernel_scheduler_uart_locked();
    };

    for core in 0..policy.max_cores {
        let tasks = format_core_assignments_for_live_driver_profile(policy, core);
        if tasks.as_str() != "none" {
            probe(core, tasks.as_str());
        }
    }

    if let Some(core) = policy.authority_core {
        let _ = crate::sel4::set_tcb_affinity_silent(tcb, core);
    }
}

#[cfg(not(all(feature = "kernel", sel4_config_debug_build)))]
pub fn debug_dump_per_core<F>(_policy: &generated::AffinityPolicy, mut emit: F)
where
    F: FnMut(&str),
{
    emit("ERR reason=unsupported");
}

#[cfg(feature = "kernel")]
pub fn apply_tcb_affinity(
    tcb: crate::sel4::seL4_CPtr,
    role: AffinityRole,
    index: usize,
    policy: &generated::AffinityPolicy,
) -> Result<Option<u8>, AffinityError> {
    let core = match select_core(policy, role, index) {
        Some(core) => core,
        None => return Ok(None),
    };
    if core >= policy.max_cores {
        return Err(AffinityError::InvalidCore {
            role,
            core,
            max_cores: policy.max_cores,
        });
    }
    if let Err(err) = crate::sel4::set_tcb_affinity(tcb, core) {
        return Err(AffinityError::Syscall {
            role,
            core,
            err: err as i32,
        });
    }
    Ok(Some(core))
}

#[cfg(feature = "kernel")]
pub fn apply_driver_tcb_affinity(
    tcb: crate::sel4::seL4_CPtr,
    driver: DriverAffinityTarget,
    policy: &generated::AffinityPolicy,
) -> Result<Option<u8>, AffinityError> {
    let Some(core) = select_driver_core(policy, driver) else {
        return Ok(None);
    };
    let root_core = policy.authority_core.unwrap_or(0);
    validate_driver_core(policy, driver, root_core)?;
    if let Err(err) = crate::sel4::set_tcb_affinity(tcb, core) {
        return Err(AffinityError::DriverSyscall {
            driver,
            core,
            err: err as i32,
        });
    }
    Ok(Some(core))
}

#[cfg(feature = "kernel")]
pub fn apply_boot_policy(view: &crate::sel4::BootInfoView) -> Result<Option<u8>, AffinityError> {
    let policy = policy();
    if !policy.enabled {
        return Ok(None);
    }
    let observed_nodes = view.header().numNodes as u8;
    validate_policy(&policy, observed_nodes)?;
    let tcb = view.header().init_tcb_cap();
    apply_tcb_affinity(tcb, AffinityRole::Authority, 0, &policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    static NINEDOOR: [u8; 2] = [1, 2];
    static PROVIDER: [u8; 1] = [3];
    static WORKER: [u8; 2] = [2, 3];

    fn sample_policy() -> generated::AffinityPolicy {
        generated::AffinityPolicy {
            enabled: true,
            max_cores: 4,
            authority_core: Some(0),
            ninedoor_cores: &NINEDOOR,
            provider_cores: &PROVIDER,
            worker_cores: &WORKER,
            drivers: generated::DriverAffinityPolicy {
                serial: Some(1),
                usb_local_seat: Some(1),
                hdmi_text: Some(2),
                bcmgenet_v5: Some(3),
                cyw43455: Some(3),
                rtl8139: Some(2),
                virtio_net: Some(3),
                sdio_host: Some(3),
                pcie_root: Some(2),
            },
        }
    }

    #[test]
    fn select_core_round_robin() {
        let policy = sample_policy();
        assert_eq!(select_core(&policy, AffinityRole::NineDoor, 0), Some(1));
        assert_eq!(select_core(&policy, AffinityRole::NineDoor, 1), Some(2));
        assert_eq!(select_core(&policy, AffinityRole::NineDoor, 2), Some(1));
        assert_eq!(select_core(&policy, AffinityRole::Provider, 7), Some(3));
        assert_eq!(select_core(&policy, AffinityRole::Worker, 0), Some(2));
        assert_eq!(select_core(&policy, AffinityRole::Worker, 1), Some(3));
        assert_eq!(select_core(&policy, AffinityRole::Worker, 2), Some(2));
    }

    #[test]
    fn select_driver_core_uses_per_driver_manifest_fields() {
        let policy = sample_policy();
        assert_eq!(
            select_driver_core(&policy, DriverAffinityTarget::Serial),
            Some(1)
        );
        assert_eq!(
            select_driver_core(&policy, DriverAffinityTarget::BcmGenetV5),
            Some(3)
        );
        assert_eq!(
            select_driver_core(&policy, DriverAffinityTarget::Cyw43455),
            Some(3)
        );
    }

    #[test]
    fn format_core_assignments_lists_multiple_allocations() {
        let policy = sample_policy();
        assert_eq!(
            format_core_assignments(&policy, 1).as_str(),
            "ninedoor,serial,usb"
        );
        assert_eq!(
            format_core_assignments(&policy, 2).as_str(),
            "ninedoor,worker,hdmi,rtl8139,pcie"
        );
        assert_eq!(
            format_core_assignments(&policy, 3).as_str(),
            "provider,worker,genet,cyw43,virtio,sdio"
        );
    }

    #[test]
    fn validate_policy_requires_node_match() {
        let policy = sample_policy();
        let err = validate_policy(&policy, 2).unwrap_err();
        assert!(matches!(err, AffinityError::NodesMismatch { .. }));
    }

    #[test]
    fn validate_policy_rejects_driver_on_root_core() {
        let mut policy = sample_policy();
        policy.drivers.serial = Some(0);
        let err = validate_policy(&policy, 4).unwrap_err();
        assert!(matches!(err, AffinityError::DriverOnRootCore { .. }));
    }
}
