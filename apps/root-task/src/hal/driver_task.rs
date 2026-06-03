// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define HAL-enforced hardware driver task scheduling contracts.
// Author: Lukas Bower

//! Scheduling contracts for hardware drivers.
//!
//! These contracts are the HAL-facing bridge for the Milestone 26a/26b
//! dedicated seL4 driver-task model. Drivers must declare the contract they
//! consume before runtime code may service them.

#[cfg(feature = "kernel")]
use core::cell::UnsafeCell;
#[cfg(all(feature = "kernel", not(target_arch = "aarch64")))]
use core::sync::atomic::AtomicU64;
#[cfg(feature = "kernel")]
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use heapless::Deque;
#[cfg(feature = "kernel")]
use pi4_driver_abi::{
    DriverRuntimeFramebufferDescriptor, DriverRuntimeInitDescriptor,
    DRIVER_RUNTIME_BUS_LINK_PCIE_ENDPOINT_SLOT, DRIVER_RUNTIME_BUS_LINK_SDIO_ENDPOINT_SLOT,
    DRIVER_RUNTIME_ENGINE_INIT_AUX, DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888,
    DRIVER_RUNTIME_FRAMEBUFFER_VADDR, DRIVER_RUNTIME_INIT_AUX, DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX,
    DRIVER_RUNTIME_USB_ENUMERATE_AUX,
};
use pi4_driver_abi::{
    DRIVER_RUNTIME_BUS_LINK_PCIE_RING_VADDR, DRIVER_RUNTIME_BUS_LINK_SDIO_RING_VADDR,
    DRIVER_RUNTIME_COMMAND_FLAG_ONE_WAY,
};

/// Hardware driver instance covered by a scheduling contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskKind {
    /// Physical UART or serial-console driver.
    Serial,
    /// USB xHCI/HID local-seat input path.
    LocalSeatUsb,
    /// HDMI text output sink.
    HdmiText,
    /// Wired Ethernet NIC.
    WiredNic,
    /// CYW43/CYW43455 Wi-Fi NIC.
    WifiNic,
    /// Virtio or emulator NIC used by QEMU compatibility profiles.
    VirtualNic,
    /// SDIO host controller used beneath Wi-Fi.
    SdioHost,
    /// PCIe root complex or host bridge service.
    PcieRoot,
}

/// Runtime family used to decide whether compatibility dispatch is allowed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskRuntimeProfile {
    /// Physical Pi 4 hardware profile.
    Pi4Hardware,
    /// QEMU/virt compatibility profile.
    QemuCompatibility,
    /// Host tests and non-kernel builds.
    HostTest,
}

/// Network driver selected for pre-root physical Pi proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pi4PreRootNetBootstrapSelection {
    /// No network driver is selected for pre-root proof.
    Disabled,
    /// Wired GENET is the selected pre-root NIC.
    Wired,
    /// CYW43 Wi-Fi is the selected pre-root NIC.
    Wifi,
}

impl Pi4PreRootNetBootstrapSelection {
    #[cfg(feature = "kernel")]
    const fn as_u32(self) -> u32 {
        match self {
            Self::Disabled => 0,
            Self::Wired => 1,
            Self::Wifi => 2,
        }
    }

    #[cfg(feature = "kernel")]
    const fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::Wired,
            2 => Self::Wifi,
            _ => Self::Disabled,
        }
    }
}

impl DriverTaskRuntimeProfile {
    /// Stable diagnostic label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pi4Hardware => "pi4-hardware",
            Self::QemuCompatibility => "qemu-compatibility",
            Self::HostTest => "host-test",
        }
    }
}

/// Current profile used by steady-state driver service admission.
pub const CURRENT_DRIVER_TASK_RUNTIME_PROFILE: DriverTaskRuntimeProfile = if cfg!(all(
    feature = "kernel",
    target_arch = "aarch64",
    target_os = "none",
    not(feature = "net-backend-virtio")
)) {
    DriverTaskRuntimeProfile::Pi4Hardware
} else if cfg!(all(
    feature = "kernel",
    target_arch = "aarch64",
    target_os = "none",
    feature = "net-backend-virtio"
)) {
    DriverTaskRuntimeProfile::QemuCompatibility
} else {
    DriverTaskRuntimeProfile::HostTest
};

/// Whether this build may compile steady-state compatibility service state.
///
/// The physical Pi 4 profile must not carry callback-pointer service slots for
/// hardware turns. QEMU and host-test builds keep the narrow compatibility ABI
/// so the architecture can be tested before every Pi-only driver is migrated.
pub const STEADY_STATE_COMPAT_SERVICE_COMPILED: bool = cfg!(any(
    not(feature = "kernel"),
    not(target_arch = "aarch64"),
    not(target_os = "none"),
    feature = "net-backend-virtio"
));

#[cfg(feature = "kernel")]
static PI4_PRE_ROOT_NET_BOOTSTRAP_SELECTION: AtomicU32 =
    AtomicU32::new(Pi4PreRootNetBootstrapSelection::Disabled.as_u32());

/// Publish the U-Boot/manifest-selected NIC for physical Pi diagnostics.
#[cfg(feature = "kernel")]
pub fn publish_pi4_pre_root_net_bootstrap_selection(selection: Pi4PreRootNetBootstrapSelection) {
    PI4_PRE_ROOT_NET_BOOTSTRAP_SELECTION.store(selection.as_u32(), Ordering::Release);
}

/// Return the selected NIC advertised by U-Boot/manifest policy.
#[cfg(feature = "kernel")]
#[must_use]
pub fn pi4_pre_root_net_bootstrap_selection() -> Pi4PreRootNetBootstrapSelection {
    Pi4PreRootNetBootstrapSelection::from_u32(
        PI4_PRE_ROOT_NET_BOOTSTRAP_SELECTION.load(Ordering::Acquire),
    )
}

/// Whether this build is the physical Pi 4 owner-state cutover profile.
///
/// In this profile steady-state hardware progress must come from the
/// driver-task ring path. Root may still keep emergency serial writes alive for
/// boot diagnostics, but it must not construct or service normal Pi 4 hardware
/// drivers through root-owned runtime structs.
#[must_use]
pub const fn physical_pi_driver_task_only_owner_state_active() -> bool {
    cfg!(all(
        feature = "kernel",
        target_arch = "aarch64",
        target_os = "none",
        not(feature = "net-backend-virtio")
    ))
}

/// Whether normal Pi 4 driver-task bootstrap must use isolated child VSpaces.
///
/// Physical Pi 4 builds must not create shared-root service TCBs for normal
/// hardware progress. QEMU/host compatibility builds may keep the transitional
/// root-image path so virtual networking and smoke tests remain available.
#[must_use]
pub const fn physical_pi_driver_task_bootstrap_requires_isolated_vspace() -> bool {
    physical_pi_driver_task_only_owner_state_active()
}

impl DriverTaskKind {
    /// Stable role label used by Pi 4 driver-task proof tooling.
    #[must_use]
    pub const fn proof_role(self) -> &'static str {
        match self {
            Self::Serial => "serial",
            Self::LocalSeatUsb => "usb",
            Self::HdmiText => "display",
            Self::WiredNic | Self::WifiNic | Self::VirtualNic => "net",
            Self::SdioHost => "sdio",
            Self::PcieRoot => "pcie",
        }
    }
}

/// Returns the required closure role bit represented by a driver kind.
#[must_use]
pub const fn driver_task_role_bit(kind: DriverTaskKind) -> usize {
    match kind {
        DriverTaskKind::Serial => DRIVER_TASK_ROLE_SERIAL_BIT,
        DriverTaskKind::LocalSeatUsb => DRIVER_TASK_ROLE_USB_BIT,
        DriverTaskKind::HdmiText => DRIVER_TASK_ROLE_DISPLAY_BIT,
        DriverTaskKind::WiredNic | DriverTaskKind::WifiNic | DriverTaskKind::VirtualNic => {
            DRIVER_TASK_ROLE_NET_BIT
        }
        DriverTaskKind::SdioHost => DRIVER_TASK_ROLE_SDIO_BIT,
        DriverTaskKind::PcieRoot => DRIVER_TASK_ROLE_PCIE_BIT,
    }
}

/// Runtime snapshot of the seL4 driver-task substrate.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DriverTaskRuntimeProof {
    /// Whether the root bootstrap created at least one driver TCB.
    pub substrate_active: bool,
    /// Driver TCBs configured during bootstrap.
    pub configured_count: usize,
    /// Driver TCBs that failed during bootstrap.
    pub failed_count: usize,
    /// Driver TCBs that resumed and executed their entry trampoline.
    pub live_tcb_count: usize,
    /// Role coverage for live TCBs.
    pub live_tcb_role_mask: usize,
    /// Role coverage for hot paths actually serviced by dedicated TCBs.
    pub hot_path_role_mask: usize,
    /// Role coverage serviced through pointer-free rings before isolated
    /// driver-owned state is proved.
    pub shared_ring_service_role_mask: usize,
    /// Role coverage whose hardware-owned state is registered through
    /// pointer-free owner-state descriptors rather than root pointers.
    pub owner_state_role_mask: usize,
    /// Pi 4 hot-path coverage whose hardware-owned state is registered through
    /// pointer-free owner-state descriptors.
    pub owner_state_hot_path_mask: usize,
    /// Role coverage still observed on root-task compatibility service turns.
    pub compatibility_service_role_mask: usize,
    /// Whether minted driver CSpaces contain only declared caps.
    pub capset_proof: bool,
    /// Whether driver fault endpoints were installed.
    pub fault_proof: bool,
    /// Whether revocation/rollback state exists for created driver caps.
    pub revoke_proof: bool,
    /// Whether scheduling parameters were successfully installed.
    pub sched_proof: bool,
    /// Driver TCBs with explicit per-driver manifest affinity configured.
    pub affinity_configured_count: usize,
    /// Driver TCBs whose per-driver manifest affinity was applied.
    pub affinity_applied_count: usize,
    /// Whether every configured per-driver affinity was applied successfully.
    pub affinity_proof: bool,
    /// Whether active driver TCBs use isolated driver VSpaces.
    pub vspace_proof: bool,
    /// Whether driver service turns use pointer-free shared command rings.
    pub pointer_free_ipc_proof: bool,
    /// Whether hardware-owned driver state lives behind driver-task service
    /// rings instead of root-owned runtime structs.
    pub owner_state_proof: bool,
    /// Count of broad authority caps intentionally leaked into driver CSpaces.
    pub broad_caps_leaked: usize,
}

/// Bootstrap report published by the HAL after creating driver TCBs.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DriverTaskBootstrapReport {
    /// Driver TCBs configured during bootstrap.
    pub configured_count: usize,
    /// Driver TCBs that failed during bootstrap.
    pub failed_count: usize,
    /// Driver TCBs that reached their entry trampoline.
    pub live_tcb_count: usize,
    /// Role coverage for live TCBs.
    pub live_tcb_role_mask: usize,
    /// Whether minted driver CSpaces contain only declared caps.
    pub capset_proof: bool,
    /// Whether fault endpoints were installed.
    pub fault_proof: bool,
    /// Whether created driver caps are tracked for revocation.
    pub revoke_proof: bool,
    /// Whether priorities/scheduling parameters were installed.
    pub sched_proof: bool,
    /// Driver TCBs with explicit per-driver manifest affinity configured.
    pub affinity_configured_count: usize,
    /// Driver TCBs whose per-driver manifest affinity was applied.
    pub affinity_applied_count: usize,
    /// Whether every configured per-driver affinity was applied successfully.
    pub affinity_proof: bool,
    /// Driver TCBs whose TCB space uses a non-root VSpace.
    pub isolated_vspace_count: usize,
    /// Driver TCBs that completed a fixed-layout command/completion ring proof.
    pub pointer_free_ipc_count: usize,
    /// Driver TCBs with a declared Pi 4 runtime-image mapping contract.
    pub runtime_image_declared_count: usize,
    /// Declared Pi 4 runtime images whose transport pages were mapped.
    pub runtime_image_transport_mapped_count: usize,
    /// Declared Pi 4 runtime images eligible for owner-state acceptance.
    pub runtime_image_acceptance_count: usize,
    /// Pi 4 hot-path mask covered by runtime-image declarations.
    pub runtime_image_declared_hot_path_mask: usize,
    /// Pi 4 hot-path mask whose isolated transport pages were mapped.
    pub runtime_image_transport_mapped_hot_path_mask: usize,
    /// Role coverage whose hardware-owned state is registered through
    /// pointer-free owner-state descriptors rather than root pointers.
    pub owner_state_role_mask: usize,
    /// Pi 4 hot-path coverage whose hardware-owned state is registered through
    /// pointer-free owner-state descriptors.
    pub owner_state_hot_path_mask: usize,
    /// Whether driver TCBs run in isolated driver VSpaces.
    pub vspace_proof: bool,
    /// Whether driver service turns use pointer-free shared command rings.
    pub pointer_free_ipc_proof: bool,
    /// Whether hardware-owned driver state lives behind driver-task rings.
    pub owner_state_proof: bool,
    /// Count of broad authority caps intentionally leaked into driver CSpaces.
    pub broad_caps_leaked: usize,
}

/// Publish the seL4 driver-task substrate state for later boot proof.
#[cfg(feature = "kernel")]
pub fn publish_driver_task_bootstrap_report(report: DriverTaskBootstrapReport) {
    DRIVER_TASK_SUBSTRATE_ACTIVE.store((report.configured_count > 0) as usize, Ordering::Release);
    DRIVER_TASK_CONFIGURED_COUNT.store(report.configured_count, Ordering::Release);
    DRIVER_TASK_FAILED_COUNT.store(report.failed_count, Ordering::Release);
    DRIVER_TASK_LIVE_TCB_COUNT.store(report.live_tcb_count, Ordering::Release);
    DRIVER_TASK_LIVE_TCB_ROLE_MASK.store(report.live_tcb_role_mask, Ordering::Release);
    DRIVER_TASK_OWNER_STATE_ROLE_MASK.store(report.owner_state_role_mask, Ordering::Release);
    DRIVER_TASK_OWNER_STATE_HOT_PATH_MASK
        .store(report.owner_state_hot_path_mask, Ordering::Release);
    DRIVER_TASK_CAPSET_PROOF.store(report.capset_proof as usize, Ordering::Release);
    DRIVER_TASK_FAULT_PROOF.store(report.fault_proof as usize, Ordering::Release);
    DRIVER_TASK_REVOKE_PROOF.store(report.revoke_proof as usize, Ordering::Release);
    DRIVER_TASK_SCHED_PROOF.store(report.sched_proof as usize, Ordering::Release);
    DRIVER_TASK_AFFINITY_CONFIGURED_COUNT
        .store(report.affinity_configured_count, Ordering::Release);
    DRIVER_TASK_AFFINITY_APPLIED_COUNT.store(report.affinity_applied_count, Ordering::Release);
    DRIVER_TASK_AFFINITY_PROOF.store(report.affinity_proof as usize, Ordering::Release);
    DRIVER_TASK_VSPACE_PROOF.store(report.vspace_proof as usize, Ordering::Release);
    DRIVER_TASK_POINTER_FREE_IPC_PROOF
        .store(report.pointer_free_ipc_proof as usize, Ordering::Release);
    DRIVER_TASK_OWNER_STATE_PROOF.store(report.owner_state_proof as usize, Ordering::Release);
    DRIVER_TASK_BROAD_CAPS_LEAKED.store(report.broad_caps_leaked, Ordering::Release);
}

/// Snapshot the current runtime proof state.
#[must_use]
pub fn driver_task_runtime_proof() -> DriverTaskRuntimeProof {
    #[cfg(feature = "kernel")]
    {
        return DriverTaskRuntimeProof {
            substrate_active: DRIVER_TASK_SUBSTRATE_ACTIVE.load(Ordering::Acquire) != 0,
            configured_count: DRIVER_TASK_CONFIGURED_COUNT.load(Ordering::Acquire),
            failed_count: DRIVER_TASK_FAILED_COUNT.load(Ordering::Acquire),
            live_tcb_count: DRIVER_TASK_LIVE_TCB_COUNT.load(Ordering::Acquire),
            live_tcb_role_mask: DRIVER_TASK_LIVE_TCB_ROLE_MASK.load(Ordering::Acquire),
            hot_path_role_mask: DRIVER_TASK_HOT_PATH_ROLE_MASK.load(Ordering::Acquire),
            shared_ring_service_role_mask: DRIVER_TASK_SHARED_RING_SERVICE_ROLE_MASK
                .load(Ordering::Acquire),
            owner_state_role_mask: DRIVER_TASK_OWNER_STATE_ROLE_MASK.load(Ordering::Acquire),
            owner_state_hot_path_mask: DRIVER_TASK_OWNER_STATE_HOT_PATH_MASK
                .load(Ordering::Acquire),
            compatibility_service_role_mask: DRIVER_TASK_COMPAT_SERVICE_ROLE_MASK
                .load(Ordering::Acquire),
            capset_proof: DRIVER_TASK_CAPSET_PROOF.load(Ordering::Acquire) != 0,
            fault_proof: DRIVER_TASK_FAULT_PROOF.load(Ordering::Acquire) != 0,
            revoke_proof: DRIVER_TASK_REVOKE_PROOF.load(Ordering::Acquire) != 0,
            sched_proof: DRIVER_TASK_SCHED_PROOF.load(Ordering::Acquire) != 0,
            affinity_configured_count: DRIVER_TASK_AFFINITY_CONFIGURED_COUNT
                .load(Ordering::Acquire),
            affinity_applied_count: DRIVER_TASK_AFFINITY_APPLIED_COUNT.load(Ordering::Acquire),
            affinity_proof: DRIVER_TASK_AFFINITY_PROOF.load(Ordering::Acquire) != 0,
            vspace_proof: DRIVER_TASK_VSPACE_PROOF.load(Ordering::Acquire) != 0,
            pointer_free_ipc_proof: DRIVER_TASK_POINTER_FREE_IPC_PROOF.load(Ordering::Acquire) != 0,
            owner_state_proof: DRIVER_TASK_OWNER_STATE_PROOF.load(Ordering::Acquire) != 0,
            broad_caps_leaked: DRIVER_TASK_BROAD_CAPS_LEAKED.load(Ordering::Acquire),
        };
    }

    #[cfg(not(feature = "kernel"))]
    {
        DriverTaskRuntimeProof::default()
    }
}

/// Records which execution path serviced a hardware driver turn.
#[cfg(feature = "kernel")]
pub fn record_driver_task_service(contract: DriverTaskContract, isolation: DriverTaskIsolation) {
    let role_bit = driver_task_role_bit(contract.kind);
    if role_bit == 0 {
        return;
    }
    if driver_task_service_counts_as_hot_path(isolation) {
        DRIVER_TASK_HOT_PATH_ROLE_MASK.fetch_or(role_bit, Ordering::AcqRel);
    } else {
        DRIVER_TASK_COMPAT_SERVICE_ROLE_MASK.fetch_or(role_bit, Ordering::AcqRel);
    }
}

/// Records a service turn that completed through the pointer-free ring ABI.
///
/// Shared-ring dispatch is necessary but not sufficient for strongest driver
/// isolation. It is credited as a dedicated hot path only after the runtime also
/// proves isolated driver VSpaces, pointer-free IPC, and no root-context
/// dependency for this specific service turn. Otherwise it remains a distinct
/// shared-ring diagnostic that does not satisfy acceptance.
#[cfg(feature = "kernel")]
pub fn record_driver_task_ring_service(
    contract: DriverTaskContract,
    owner_state_credit_eligible: bool,
) {
    let role_bit = driver_task_role_bit(contract.kind);
    if role_bit == 0 {
        return;
    }
    DRIVER_TASK_SHARED_RING_SERVICE_ROLE_MASK.fetch_or(role_bit, Ordering::AcqRel);
    if owner_state_credit_eligible
        && DRIVER_TASK_VSPACE_PROOF.load(Ordering::Acquire) != 0
        && DRIVER_TASK_POINTER_FREE_IPC_PROOF.load(Ordering::Acquire) != 0
        && DRIVER_TASK_OWNER_STATE_PROOF.load(Ordering::Acquire) != 0
    {
        DRIVER_TASK_HOT_PATH_ROLE_MASK.fetch_or(role_bit, Ordering::AcqRel);
    }
}

#[must_use]
pub const fn driver_task_service_counts_as_hot_path(isolation: DriverTaskIsolation) -> bool {
    match isolation {
        DriverTaskIsolation::DedicatedSeL4Task => true,
        DriverTaskIsolation::RootTaskCompatibility => false,
    }
}

/// Returns whether the transitional callback-pointer ABI may serve a
/// steady-state hardware turn for a profile.
///
/// Physical Pi 4 builds must not use callback dispatch for steady-state
/// hardware paths. Early/emergency UART writes are outside this policy because
/// they run before the driver-task substrate exists.
#[must_use]
pub const fn callback_dispatch_allowed_for_profile(profile: DriverTaskRuntimeProfile) -> bool {
    match profile {
        DriverTaskRuntimeProfile::Pi4Hardware => false,
        DriverTaskRuntimeProfile::QemuCompatibility | DriverTaskRuntimeProfile::HostTest => true,
    }
}

/// Returns whether a root-owned compatibility hot path may run for a profile.
#[must_use]
pub const fn root_fallback_allowed_for_profile(profile: DriverTaskRuntimeProfile) -> bool {
    match profile {
        DriverTaskRuntimeProfile::Pi4Hardware => false,
        DriverTaskRuntimeProfile::QemuCompatibility | DriverTaskRuntimeProfile::HostTest => true,
    }
}

/// Returns whether a pre-root physical-Pi bootstrap must defer runtime-init service.
///
/// The physical Pi 4 path keeps bootstrap runtime-init turns bounded and
/// reserves blocking reply-path service for post-prompt steady state. Network
/// runtimes, USB local-seat proof, SDIO bus-owner proof, and PCIe root proof
/// are allowed to make progress only after the root shell is available, so a
/// selected or fallback NIC, wedged local-seat runtime, SDIO bus-owner runtime,
/// or unresponsive PCIe root runtime cannot starve serial and display console
/// availability.
#[must_use]
pub const fn pre_root_runtime_init_deferred_for_profile(
    profile: DriverTaskRuntimeProfile,
    _selection: Pi4PreRootNetBootstrapSelection,
    contract: DriverTaskContract,
) -> bool {
    if !matches!(profile, DriverTaskRuntimeProfile::Pi4Hardware) {
        return false;
    }
    matches!(
        contract.kind,
        DriverTaskKind::Serial
            | DriverTaskKind::LocalSeatUsb
            | DriverTaskKind::WiredNic
            | DriverTaskKind::WifiNic
            | DriverTaskKind::SdioHost
            | DriverTaskKind::PcieRoot
    )
}

/// Current-build pre-root runtime-init deferral policy.
#[cfg(feature = "kernel")]
#[must_use]
pub fn pre_root_runtime_init_deferred_for_shell(contract: DriverTaskContract) -> bool {
    pre_root_runtime_init_deferred_for_profile(
        CURRENT_DRIVER_TASK_RUNTIME_PROFILE,
        pi4_pre_root_net_bootstrap_selection(),
        contract,
    )
}

/// Current-build admission for callback-pointer steady-state service turns.
#[must_use]
pub const fn steady_state_callback_dispatch_allowed(_contract: DriverTaskContract) -> bool {
    callback_dispatch_allowed_for_profile(CURRENT_DRIVER_TASK_RUNTIME_PROFILE)
}

/// Current-build admission for root-owned steady-state compatibility turns.
#[must_use]
pub const fn steady_state_root_fallback_allowed(_contract: DriverTaskContract) -> bool {
    root_fallback_allowed_for_profile(CURRENT_DRIVER_TASK_RUNTIME_PROFILE)
}

/// Admit and record a root-owned compatibility service turn when the current
/// profile is explicitly allowed to use one.
///
/// This is the only steady-state root-fallback admission point. Physical Pi 4
/// builds return false, forcing the caller to fail closed until the relevant
/// hardware path is serviced by a ring-backed driver task.
#[cfg(feature = "kernel")]
pub fn admit_root_task_compatibility_service(contract: DriverTaskContract) -> bool {
    if !steady_state_root_fallback_allowed(contract) {
        return false;
    }
    record_driver_task_service(contract, DriverTaskIsolation::RootTaskCompatibility);
    true
}

#[cfg(all(
    feature = "kernel",
    any(
        not(target_arch = "aarch64"),
        not(target_os = "none"),
        feature = "net-backend-virtio"
    )
))]
fn record_driver_task_callback_compatibility(contract: DriverTaskContract) {
    let role_bit = driver_task_role_bit(contract.kind);
    if role_bit != 0 {
        DRIVER_TASK_COMPAT_SERVICE_ROLE_MASK.fetch_or(role_bit, Ordering::AcqRel);
    }
}

/// Type-erased service callback executed by a driver TCB.
///
/// The argument is a caller-owned context pointer and the return value is a
/// small role-specific status word. The callback ABI is intentionally narrow so
/// hot paths can be moved one driver at a time without adding a second driver
/// framework.
#[cfg(feature = "kernel")]
pub type DriverTaskServiceHandler = unsafe fn(usize) -> usize;

/// Registered service owner for fixed-layout shared-ring commands.
#[cfg(feature = "kernel")]
pub type DriverTaskRingServiceHandler =
    unsafe fn(usize, DriverTaskCommandRecord) -> DriverTaskCompletionRecord;

/// Ring-service dispatch class installed for a driver task.
///
/// Root-context services keep existing Pi hardware working while the
/// driver-local runtime image is still being built, but they can never satisfy
/// owner-state proof. Pointer-free selector services are the only class that can
/// become acceptance evidence, and only after VSpace, IPC, and owner-state
/// descriptor proof are also present.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum DriverTaskRingServiceKind {
    /// No shared-ring service handler is registered.
    None = 0,
    /// Handler receives a root pointer or root stack context.
    RootContextDiagnostic = 1,
    /// Handler receives only primitive selector/context values.
    PointerFreeSelector = 2,
}

impl DriverTaskRingServiceKind {
    /// Decode the atomic representation stored in a command slot.
    #[must_use]
    pub const fn from_usize(value: usize) -> Self {
        match value {
            1 => Self::RootContextDiagnostic,
            2 => Self::PointerFreeSelector,
            _ => Self::None,
        }
    }

    /// Atomic representation for command-slot storage.
    #[must_use]
    pub const fn as_usize(self) -> usize {
        self as usize
    }

    /// Stable diagnostic label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::RootContextDiagnostic => "root-context-diagnostic",
            Self::PointerFreeSelector => "pointer-free-selector",
        }
    }

    /// Whether this dispatch class may ever credit owner-state hot paths.
    #[must_use]
    pub const fn owner_state_credit_allowed(self) -> bool {
        matches!(self, Self::PointerFreeSelector)
    }
}

/// Service IPC ABI installed for driver-task dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskIpcAbi {
    /// Transitional ABI: root stores a function pointer and root-memory context.
    CallbackPointer,
    /// Final isolation ABI: commands and completions live in shared bounded rings.
    SharedRingCommand,
}

impl DriverTaskIpcAbi {
    /// Stable boot-proof label for this ABI.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CallbackPointer => "callback-pointer",
            Self::SharedRingCommand => "shared-ring-command",
        }
    }

    /// Whether this ABI can cross isolated driver VSpaces.
    #[must_use]
    pub const fn is_pointer_free(self) -> bool {
        matches!(self, Self::SharedRingCommand)
    }
}

/// Current as-built service ABI.
///
/// Physical Pi 4 builds use fixed command/completion rings for steady-state
/// service turns. QEMU/host compatibility builds retain the callback ABI so
/// existing virtual-device paths can keep running while isolated runtime images
/// grow full hardware handlers.
pub const CURRENT_DRIVER_TASK_IPC_ABI: DriverTaskIpcAbi = if STEADY_STATE_COMPAT_SERVICE_COMPILED {
    DriverTaskIpcAbi::CallbackPointer
} else {
    DriverTaskIpcAbi::SharedRingCommand
};

/// Temporary priority for Pi 4 linked runtimes during bounded bootstrap turns.
///
/// Pre-root runtime init uses nonblocking sends plus explicit yields instead
/// of `seL4_Call`, so the child must be schedulable while root is polling the
/// fixed completion record. The HAL restores the contract priority immediately
/// after the bounded init/proof turn.
pub const PI4_BOUNDED_BOOTSTRAP_PRIORITY: u8 = 255;

/// Entry point for bootstrap-created driver TCBs.
#[cfg(feature = "kernel")]
pub extern "C" fn driver_task_entry(task_key: usize) -> ! {
    let role_bit = driver_task_task_key_role_bit(task_key).unwrap_or(0);
    DRIVER_TASK_STARTED_ROLE_MASK.fetch_or(role_bit, Ordering::AcqRel);
    if task_key < usize::BITS as usize {
        DRIVER_TASK_STARTED_TASK_MASK.fetch_or(1usize << task_key, Ordering::AcqRel);
    }
    loop {
        let mut badge: sel4_sys::seL4_Word = 0;
        let _ = crate::sel4::recv(DRIVER_TASK_CHILD_COMMAND_SLOT, &mut badge);
        let _ = badge;
        let result = service_pending_driver_task_command(task_key);
        // SAFETY: The command was delivered by `seL4_Call`; the kernel
        // installed a reply capability for this TCB, and the single reply word
        // mirrors the already-published completion slot result.
        unsafe {
            sel4_sys::seL4_SetMR(0, result as sel4_sys::seL4_Word);
        }
        crate::sel4::reply(sel4_sys::seL4_MessageInfo::new(0, 0, 0, 1));
        DRIVER_TASK_ENTRY_HEARTBEATS.fetch_add(1, Ordering::AcqRel);
    }
}

#[cfg(all(
    feature = "kernel",
    target_arch = "aarch64",
    target_os = "none",
    not(sel4_config_kernel_mcs)
))]
core::arch::global_asm!(
    r#"
    .section .driver_task_text, "ax", %progbits
    .balign 16
    .global cohesix_driver_task_isolated_entry
    .type cohesix_driver_task_isolated_entry, %function
cohesix_driver_task_isolated_entry:
    mov x20, x0
1:
    mov x0, {child_command_slot}
    ldr x7, ={sys_recv}
    svc #0

    ldr x9, ={ring_vaddr}
    ldr w10, [x9]
    mov w12, {completion_code}
    mov w23, w20
    ldr w13, [x9, #8]
    cbz w13, 6f
    cmp w13, {serial_hot_path}
    b.ne 5f
    mov w12, {idle_code}
    mov w23, wzr
    ldrh w14, [x9, #36]
    cbz w14, 6f
    ldr w15, [x9, #32]
    cmp w15, {frame_offset}
    b.lo 6f
    add w24, w15, w14
    cmp w24, {ring_page_bytes}
    b.hi 6f
    add x16, x9, x15
    ldr x22, ={mmio_vaddr}
2:
    cbz w14, 6f
    ldrb w17, [x16], #1
    mov x18, #1024
3:
    ldr w19, [x22, #{mini_uart_lsr_offset}]
    tst w19, #{mini_uart_lsr_tx_empty}
    b.ne 4f
    subs x18, x18, #1
    b.ne 3b
    b 6f
4:
    str w17, [x22, #{mini_uart_io_offset}]
    mov w12, {completion_code}
    add w23, w23, #1
    subs w14, w14, #1
    b 2b
5:
    mov w12, {idle_code}
    mov w23, wzr
6:
    add x11, x9, {completion_offset}
    str w10, [x11]
    strh w12, [x11, #4]
    strh wzr, [x11, #6]
    str w23, [x11, #8]
    str xzr, [x11, #12]

    b 1b
    .size cohesix_driver_task_isolated_entry, . - cohesix_driver_task_isolated_entry
    "#,
    child_command_slot = const DRIVER_TASK_CHILD_COMMAND_SLOT,
    completion_code = const DriverTaskCompletionCode::Progress as u16,
    idle_code = const DriverTaskCompletionCode::Idle as u16,
    completion_offset = const DRIVER_TASK_RING_COMPLETION_OFFSET,
    ring_vaddr = const DRIVER_TASK_RING_VADDR,
    mmio_vaddr = const DRIVER_TASK_DEVICE_MMIO_VADDR,
    frame_offset = const DRIVER_TASK_RING_FRAME_OFFSET,
    ring_page_bytes = const DRIVER_TASK_RING_PAGE_BYTES,
    mini_uart_io_offset = const crate::serial::bcm2711_mini_uart::MU_IO_OFFSET,
    mini_uart_lsr_offset = const crate::serial::bcm2711_mini_uart::MU_LSR_OFFSET,
    mini_uart_lsr_tx_empty = const 1 << 5,
    serial_hot_path = const DriverTaskHotPath::SerialConsole as u32,
    sys_recv = const sel4_sys::seL4_SysRecv,
);

#[cfg(all(
    feature = "kernel",
    target_arch = "aarch64",
    target_os = "none",
    sel4_config_kernel_mcs
))]
core::arch::global_asm!(
    r#"
    .section .driver_task_text, "ax", %progbits
    .balign 16
    .global cohesix_driver_task_isolated_entry
    .type cohesix_driver_task_isolated_entry, %function
cohesix_driver_task_isolated_entry:
1:
    wfe
    b 1b
    .size cohesix_driver_task_isolated_entry, . - cohesix_driver_task_isolated_entry
    "#,
);

/// Whether the driver-local trampoline can complete the ring smoke ABI.
#[cfg(feature = "kernel")]
#[must_use]
pub const fn isolated_trampoline_supported() -> bool {
    cfg!(all(
        target_arch = "aarch64",
        target_os = "none",
        not(sel4_config_kernel_mcs)
    ))
}

/// Returns the entry PC for the driver-local isolated trampoline.
#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
#[must_use]
pub fn isolated_trampoline_entry() -> usize {
    extern "C" {
        fn cohesix_driver_task_isolated_entry();
    }
    cohesix_driver_task_isolated_entry as *const () as usize
}

/// Host-build placeholder for tests that inspect the layout without a kernel.
#[cfg(any(
    not(feature = "kernel"),
    not(target_arch = "aarch64"),
    not(target_os = "none")
))]
#[must_use]
pub const fn isolated_trampoline_entry() -> usize {
    0
}

/// Returns the page-aligned linker section containing only trampoline code.
#[cfg(all(feature = "kernel", target_os = "none"))]
#[must_use]
pub fn isolated_trampoline_range() -> core::ops::Range<usize> {
    extern "C" {
        static __driver_task_text_start: u8;
        static __driver_task_text_end: u8;
    }
    let start = core::ptr::addr_of!(__driver_task_text_start) as usize;
    let end = core::ptr::addr_of!(__driver_task_text_end) as usize;
    start..end
}

/// Host-build placeholder for tests that inspect the layout without a kernel.
#[cfg(any(not(feature = "kernel"), not(target_os = "none")))]
#[must_use]
pub const fn isolated_trampoline_range() -> core::ops::Range<usize> {
    0..0
}

/// Wait briefly for a newly resumed driver TCB to execute its entry trampoline.
#[cfg(feature = "kernel")]
#[must_use]
pub fn wait_for_driver_task_start(task_key: usize, spins: usize) -> bool {
    let mask = if task_key < usize::BITS as usize {
        1usize << task_key
    } else {
        0
    };
    if mask == 0 {
        return false;
    }
    for _ in 0..spins {
        if DRIVER_TASK_STARTED_TASK_MASK.load(Ordering::Acquire) & mask != 0 {
            return true;
        }
        crate::sel4::yield_now();
    }
    DRIVER_TASK_STARTED_TASK_MASK.load(Ordering::Acquire) & mask != 0
}

/// Maximum bounded IPC/event queue admitted by the HAL contract layer.
pub const MAX_DRIVER_TASK_QUEUE_DEPTH: u16 = 256;

/// Number of active hardware driver-task contracts required before reopened
/// Pi 4 acceptance may claim dedicated driver-task isolation.
pub const MIN_DEDICATED_PI4_DRIVER_TASKS: usize = 7;

/// Number of declared Pi 4 hardware hot paths in the generated migration
/// catalog.
pub const REQUIRED_PI4_OWNER_STATE_HOT_PATHS: usize = 7;

/// Number of Pi 4 hardware-owner hot paths that can currently satisfy
/// owner-state acceptance.
pub const REQUIRED_PI4_ACCEPTANCE_HOT_PATHS: usize = 7;

/// Maximum Ethernet-sized frame admitted through a dedicated driver-task ring.
pub const MAX_DRIVER_TASK_FRAME_BYTES: usize = 1536;

/// Ring command flag used by transitional handlers that still carry a root
/// pointer or root-stack context despite using the fixed command/completion
/// transport. These commands may prove the ring ABI but never owner-state
/// isolation.
pub const DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE: u16 = 1 << 15;
/// Ring command flag used by runtime initialization descriptor submissions.
///
/// Runtime init proves descriptor transport only. It must not credit hardware
/// owner-state progress by itself because the driver has not serviced a device
/// turn yet.
pub const DRIVER_TASK_RING_FLAG_INIT_DESCRIPTOR_NON_ACCEPTANCE: u16 = 1 << 14;
/// Command flag used for send-only bootstrap/nonblocking turns.
///
/// The linked runtime must not issue `Reply` for these commands because `NbSend`
/// does not install a reply cap. Completion still travels through the shared ring.
pub const DRIVER_TASK_RING_FLAG_ONE_WAY: u16 = DRIVER_RUNTIME_COMMAND_FLAG_ONE_WAY;
/// Any ring flag that prevents owner-state credit.
pub const DRIVER_TASK_RING_NON_ACCEPTANCE_FLAGS: u16 =
    DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE
        | DRIVER_TASK_RING_FLAG_INIT_DESCRIPTOR_NON_ACCEPTANCE;

/// Current as-built state of the seL4 driver-task creation substrate.
///
/// This is true only because boot now creates live driver TCBs, gives them
/// restricted child CSpaces, installs command/fault IPC, and dispatches runtime
/// service callbacks through those TCBs. VSpace isolation remains a separate
/// proof field and must not be inferred from this constant.
pub const DEDICATED_DRIVER_TASK_SUBSTRATE_READY: bool = true;

/// Dedicated driver-task mode is the default requested hardware-driver policy.
///
/// This is deliberately separate from `DEDICATED_DRIVER_TASK_SUBSTRATE_READY`:
/// the build should ask for dedicated driver tasks by default, while boot proof
/// and acceptance must still fail closed until live TCB-backed hot paths exist.
pub const DEDICATED_DRIVER_TASKS_DEFAULT_ENABLED: bool = true;

/// Current as-built live-hot-path state for the dedicated driver-task default.
///
/// Live TCB-backed callback dispatch is not strong isolation because it still
/// passes root-memory pointers. This stays false until every Pi 4 hardware
/// hot path is owned by a driver task and serviced through the pointer-free
/// command/completion ring ABI.
pub const DEDICATED_DRIVER_TASK_LIVE_HOT_PATHS_READY: bool = false;

/// Stable rejection reason emitted while default-dedicated mode lacks live TCBs.
pub const DEDICATED_DRIVER_TASK_LIVE_HOT_PATHS_MISSING: &str =
    "driver-task-live-tcb-hot-paths-missing";

/// Child CSpace slot used for a badged fault endpoint.
#[cfg(feature = "kernel")]
pub const DRIVER_TASK_CHILD_FAULT_SLOT: sel4_sys::seL4_CPtr = 1;

/// Child CSpace slot used for the root-to-driver command endpoint.
#[cfg(feature = "kernel")]
pub const DRIVER_TASK_CHILD_COMMAND_SLOT: sel4_sys::seL4_CPtr = 2;

/// Child CSpace slot used for device/doorbell notification delivery.
#[cfg(feature = "kernel")]
pub const DRIVER_TASK_CHILD_NOTIFICATION_SLOT: sel4_sys::seL4_CPtr = 3;

/// Child CSpace slot used by CYW43 to call the SDIO bus-owner runtime.
#[cfg(feature = "kernel")]
pub const DRIVER_TASK_CHILD_SDIO_BUS_ENDPOINT_SLOT: sel4_sys::seL4_CPtr =
    DRIVER_RUNTIME_BUS_LINK_SDIO_ENDPOINT_SLOT as sel4_sys::seL4_CPtr;
/// Child CSpace slot used by USB to call the PCIe/VL805 bus-owner runtime.
#[cfg(feature = "kernel")]
pub const DRIVER_TASK_CHILD_PCIE_BUS_ENDPOINT_SLOT: sel4_sys::seL4_CPtr =
    DRIVER_RUNTIME_BUS_LINK_PCIE_ENDPOINT_SLOT as sel4_sys::seL4_CPtr;

/// Fixed driver-local virtual address for the root/driver command page.
pub const DRIVER_TASK_RING_VADDR: usize = 0x7000_0000;

/// Fixed driver-local virtual address for the seL4 IPC buffer page.
pub const DRIVER_TASK_IPC_VADDR: usize = 0x7000_1000;

/// Fixed driver-local virtual address for the bottom of the trampoline stack.
pub const DRIVER_TASK_STACK_BOTTOM_VADDR: usize = 0x7000_2000;

/// Fixed driver-local virtual address for the top of the trampoline stack.
pub const DRIVER_TASK_STACK_TOP_VADDR: usize = 0x7001_2000;

/// First fixed driver-local virtual address reserved for explicit MMIO pages.
pub const DRIVER_TASK_DEVICE_MMIO_VADDR: usize = 0x7020_0000;

/// First fixed driver-local virtual address reserved for explicit DMA pages.
pub const DRIVER_TASK_DMA_BUFFER_VADDR: usize = 0x7080_0000;

/// First fixed driver-local virtual address reserved for shared RX/TX/control
/// buffers outside the command ring page.
pub const DRIVER_TASK_SHARED_BUFFER_VADDR: usize = 0x70c0_0000;

/// Fixed CYW43-local virtual address for the SDIO owner command ring page.
pub const DRIVER_TASK_SDIO_BUS_RING_VADDR: usize = DRIVER_RUNTIME_BUS_LINK_SDIO_RING_VADDR as usize;
/// Fixed USB-local virtual address for the PCIe owner command ring page.
pub const DRIVER_TASK_PCIE_BUS_RING_VADDR: usize = DRIVER_RUNTIME_BUS_LINK_PCIE_RING_VADDR as usize;

/// Offset of the first fixed-layout completion record within the ring page.
pub const DRIVER_TASK_RING_COMPLETION_OFFSET: usize = 64;

/// Offset of the role-owned shared payload area within the ring page.
pub const DRIVER_TASK_RING_FRAME_OFFSET: usize = 256;

/// One page is enough for the current smoke command and completion records.
pub const DRIVER_TASK_RING_PAGE_BYTES: usize = 4096;
/// Shared owner pages exposed through a linked bus-owner transport.
pub const DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY: usize = 2;
/// Bytes in the CYW43-to-SDIO owner shared data window.
pub const DRIVER_TASK_SDIO_BUS_SHARED_DATA_BYTES: usize =
    DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY * DRIVER_TASK_RING_PAGE_BYTES;

/// Offset reserved for owner-state descriptors in the ring page.
pub const DRIVER_TASK_OWNER_STATE_OFFSET: usize = 128;

/// Bytes reserved for owner-state descriptors in the ring page.
pub const DRIVER_TASK_OWNER_STATE_BYTES: usize = 128;
/// Default primitive metadata bytes reserved for linked-runtime owner proof.
pub const DRIVER_TASK_OWNER_STATE_METADATA_BYTES: u16 = 16;

/// Owner-state descriptor flag: the hot path runs from a driver-local runtime
/// image rather than a root-owned callback handler.
pub const DRIVER_TASK_OWNER_STATE_FLAG_RUNTIME_IMAGE: u16 = 1 << 0;
/// Owner-state descriptor flag: the runtime owns explicit MMIO/device mappings.
pub const DRIVER_TASK_OWNER_STATE_FLAG_DEVICE_MAPPED: u16 = 1 << 1;
/// Owner-state descriptor flag: RX/TX/control work uses shared ring buffers.
pub const DRIVER_TASK_OWNER_STATE_FLAG_SHARED_BUFFERS: u16 = 1 << 2;
/// Owner-state descriptor flag: no root pointer or root stack context is used
/// for steady-state hardware progress.
pub const DRIVER_TASK_OWNER_STATE_FLAG_NO_ROOT_POINTERS: u16 = 1 << 3;
/// Required owner-state descriptor flags for strongest Pi 4 hot-path proof.
pub const DRIVER_TASK_OWNER_STATE_REQUIRED_FLAGS: u16 = DRIVER_TASK_OWNER_STATE_FLAG_RUNTIME_IMAGE
    | DRIVER_TASK_OWNER_STATE_FLAG_DEVICE_MAPPED
    | DRIVER_TASK_OWNER_STATE_FLAG_SHARED_BUFFERS
    | DRIVER_TASK_OWNER_STATE_FLAG_NO_ROOT_POINTERS;

/// Maximum explicitly declared runtime regions per driver-local image.
pub const DRIVER_TASK_RUNTIME_REGION_CAPACITY: usize = 8;

/// Driver-local runtime mapping region class.
#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskRuntimeRegionKind {
    /// Executable runtime image page.
    Code = 1,
    /// Driver-local stack page.
    Stack = 2,
    /// seL4 IPC buffer page.
    Ipc = 3,
    /// Command/completion ring page.
    Ring = 4,
    /// Explicit device MMIO page.
    Mmio = 5,
    /// Explicit device-owned DMA buffer page.
    Dma = 6,
    /// Root/driver shared RX/TX/control buffer page.
    SharedBuffer = 7,
}

impl DriverTaskRuntimeRegionKind {
    /// Stable diagnostic label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Stack => "stack",
            Self::Ipc => "ipc",
            Self::Ring => "ring",
            Self::Mmio => "mmio",
            Self::Dma => "dma",
            Self::SharedBuffer => "shared-buffer",
        }
    }

    /// Bit used in compact runtime-image mapping proof masks.
    #[must_use]
    pub const fn mask_bit(self) -> u16 {
        1u16 << ((self as u16) - 1)
    }
}

/// Runtime-image regions that must be mapped before the transport substrate can
/// prove an isolated command/completion turn.
pub const DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK: u16 = DriverTaskRuntimeRegionKind::Code
    .mask_bit()
    | DriverTaskRuntimeRegionKind::Stack.mask_bit()
    | DriverTaskRuntimeRegionKind::Ipc.mask_bit()
    | DriverTaskRuntimeRegionKind::Ring.mask_bit();

/// One declared mapping range for a driver-local runtime image.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTaskRuntimeRegion {
    /// Region kind.
    pub kind: DriverTaskRuntimeRegionKind,
    /// Driver-local virtual base address.
    pub vaddr: usize,
    /// Number of 4 KiB pages in the range.
    pub pages: u16,
    /// Primitive flags reserved for mapping/cache attributes.
    pub flags: u16,
}

impl DriverTaskRuntimeRegion {
    /// Construct one page-aligned runtime mapping range.
    #[must_use]
    pub const fn new(
        kind: DriverTaskRuntimeRegionKind,
        vaddr: usize,
        pages: u16,
        flags: u16,
    ) -> Option<Self> {
        if pages == 0 || vaddr & 0xfff != 0 {
            return None;
        }
        Some(Self {
            kind,
            vaddr,
            pages,
            flags,
        })
    }

    /// Region span in bytes.
    #[must_use]
    pub const fn bytes(self) -> usize {
        (self.pages as usize) << 12
    }
}

/// Static runtime-image contract for one Pi 4 hardware hot path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTaskRuntimeImageSpec {
    /// Hot path covered by this image contract.
    pub hot_path: DriverTaskHotPath,
    /// Declared driver-local mapping regions.
    pub regions: [Option<DriverTaskRuntimeRegion>; DRIVER_TASK_RUNTIME_REGION_CAPACITY],
    /// Whether the live implementation still dereferences root-owned state.
    pub root_context_required: bool,
    /// Whether the real hardware state has been moved into this runtime image.
    pub hardware_state_migrated: bool,
}

impl DriverTaskRuntimeImageSpec {
    /// Construct a runtime-image spec with the common code/stack/IPC/ring pages
    /// plus explicit MMIO, DMA, and shared-buffer ranges.
    #[must_use]
    pub const fn new(
        hot_path: DriverTaskHotPath,
        code_pages: u16,
        stack_pages: u16,
        mmio_pages: u16,
        dma_pages: u16,
        shared_buffer_pages: u16,
        root_context_required: bool,
        hardware_state_migrated: bool,
    ) -> Self {
        let mut regions = [None; DRIVER_TASK_RUNTIME_REGION_CAPACITY];
        regions[0] = DriverTaskRuntimeRegion::new(
            DriverTaskRuntimeRegionKind::Code,
            isolated_runtime_code_vaddr(),
            code_pages,
            0,
        );
        regions[1] = DriverTaskRuntimeRegion::new(
            DriverTaskRuntimeRegionKind::Stack,
            DRIVER_TASK_STACK_BOTTOM_VADDR,
            stack_pages,
            0,
        );
        regions[2] = DriverTaskRuntimeRegion::new(
            DriverTaskRuntimeRegionKind::Ipc,
            DRIVER_TASK_IPC_VADDR,
            1,
            0,
        );
        regions[3] = DriverTaskRuntimeRegion::new(
            DriverTaskRuntimeRegionKind::Ring,
            DRIVER_TASK_RING_VADDR,
            1,
            0,
        );
        regions[4] = DriverTaskRuntimeRegion::new(
            DriverTaskRuntimeRegionKind::Mmio,
            DRIVER_TASK_DEVICE_MMIO_VADDR,
            mmio_pages,
            0,
        );
        regions[5] = DriverTaskRuntimeRegion::new(
            DriverTaskRuntimeRegionKind::Dma,
            DRIVER_TASK_DMA_BUFFER_VADDR,
            dma_pages,
            0,
        );
        regions[6] = DriverTaskRuntimeRegion::new(
            DriverTaskRuntimeRegionKind::SharedBuffer,
            DRIVER_TASK_SHARED_BUFFER_VADDR,
            shared_buffer_pages,
            0,
        );
        Self {
            hot_path,
            regions,
            root_context_required,
            hardware_state_migrated,
        }
    }

    /// Returns true only when this spec can back owner-state proof.
    #[must_use]
    pub const fn acceptance_eligible(self) -> bool {
        !self.root_context_required
            && self.hardware_state_migrated
            && self.region_pages(DriverTaskRuntimeRegionKind::Code) != 0
            && self.region_pages(DriverTaskRuntimeRegionKind::Stack) != 0
            && self.region_pages(DriverTaskRuntimeRegionKind::Ipc) != 0
            && self.region_pages(DriverTaskRuntimeRegionKind::Ring) != 0
            && self.region_pages(DriverTaskRuntimeRegionKind::SharedBuffer) != 0
    }

    /// Bitmask of region kinds declared by this runtime-image contract.
    #[must_use]
    pub const fn declared_region_mask(self) -> u16 {
        let mut index = 0;
        let mut mask = 0u16;
        while index < DRIVER_TASK_RUNTIME_REGION_CAPACITY {
            if let Some(region) = self.regions[index] {
                mask |= region.kind.mask_bit();
            }
            index += 1;
        }
        mask
    }

    /// Number of distinct mapping descriptors declared by this image.
    #[must_use]
    pub const fn declared_region_count(self) -> u8 {
        let mut index = 0;
        let mut count = 0u8;
        while index < DRIVER_TASK_RUNTIME_REGION_CAPACITY {
            if self.regions[index].is_some() {
                count = count.saturating_add(1);
            }
            index += 1;
        }
        count
    }

    /// Total 4 KiB pages declared by this image contract.
    #[must_use]
    pub const fn declared_page_count(self) -> u16 {
        let mut index = 0;
        let mut pages = 0u16;
        while index < DRIVER_TASK_RUNTIME_REGION_CAPACITY {
            if let Some(region) = self.regions[index] {
                pages = pages.saturating_add(region.pages);
            }
            index += 1;
        }
        pages
    }

    /// Whether the declared transport pages are present in the mapping list.
    #[must_use]
    pub const fn declares_transport_regions(self) -> bool {
        self.declared_region_mask() & DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK
            == DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK
    }

    /// Total pages declared for a region kind.
    #[must_use]
    pub const fn region_pages(self, kind: DriverTaskRuntimeRegionKind) -> u16 {
        let mut index = 0;
        let mut pages = 0u16;
        while index < DRIVER_TASK_RUNTIME_REGION_CAPACITY {
            if let Some(region) = self.regions[index] {
                if region.kind as u16 == kind as u16 {
                    pages = pages.saturating_add(region.pages);
                }
            }
            index += 1;
        }
        pages
    }

    /// Stable non-acceptance reason for diagnostics/tests.
    #[must_use]
    pub const fn non_acceptance_reason(self) -> Option<&'static str> {
        if self.root_context_required {
            Some("root-context-required")
        } else if !self.hardware_state_migrated {
            Some("hardware-state-not-migrated")
        } else if !self.acceptance_eligible() {
            Some("runtime-region-incomplete")
        } else {
            None
        }
    }
}

/// Sentinel used when the executable image is the linker-provided trampoline.
///
/// The actual child VSpace mapping address is discovered from
/// [`isolated_trampoline_range`] by the HAL at boot. A zero value here must not
/// be logged as a real code mapping address.
#[must_use]
pub const fn isolated_runtime_code_vaddr() -> usize {
    0
}

/// Number of required Pi 4 hardware runtime-image declarations.
pub const PI4_DRIVER_TASK_RUNTIME_IMAGE_SPEC_COUNT: usize = 7;

#[cfg(feature = "kernel")]
static DRIVER_RUNTIME_PAYLOAD_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_RUNTIME_PAYLOAD_LEN: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "kernel")]
mod embedded_runtime_payload {
    include!(concat!(env!("OUT_DIR"), "/pi4_driver_runtime_payload.rs"));
}
#[cfg(feature = "kernel")]
static HDMI_RUNTIME_FRAMEBUFFER_PADDR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static HDMI_RUNTIME_FRAMEBUFFER_WIDTH: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static HDMI_RUNTIME_FRAMEBUFFER_HEIGHT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static HDMI_RUNTIME_FRAMEBUFFER_PITCH: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static HDMI_RUNTIME_ROOT_FRAMEBUFFER_VADDR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static HDMI_RUNTIME_ROOT_FRAMEBUFFER_MAP_LEN: AtomicUsize = AtomicUsize::new(0);

/// Root diagnostic framebuffer mapping published while HDMI ownership is still
/// being proved by the isolated runtime.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HdmiRuntimeRootFramebufferMapping {
    /// Physical framebuffer base reported by firmware/DT.
    pub paddr: usize,
    /// Root-task virtual address of the visible framebuffer start.
    pub vaddr: usize,
    /// Visible width in pixels.
    pub width: usize,
    /// Visible height in pixels.
    pub height: usize,
    /// Pitch in bytes.
    pub pitch: usize,
    /// Total mapped byte span including the first-page offset.
    pub map_len: usize,
}

/// Publish the boot payload that may contain linked Pi 4 driver runtime images.
#[cfg(feature = "kernel")]
pub fn publish_driver_runtime_payload(payload: &'static [u8]) {
    DRIVER_RUNTIME_PAYLOAD_PTR.store(payload.as_ptr() as usize, Ordering::Release);
    DRIVER_RUNTIME_PAYLOAD_LEN.store(payload.len(), Ordering::Release);
}

/// Publish bootloader-discovered framebuffer metadata for the linked HDMI runtime.
#[cfg(feature = "kernel")]
pub fn publish_hdmi_runtime_framebuffer_hint(
    paddr: usize,
    width: usize,
    height: usize,
    pitch: usize,
) {
    HDMI_RUNTIME_FRAMEBUFFER_PADDR.store(paddr, Ordering::Release);
    HDMI_RUNTIME_FRAMEBUFFER_WIDTH.store(width, Ordering::Release);
    HDMI_RUNTIME_FRAMEBUFFER_HEIGHT.store(height, Ordering::Release);
    HDMI_RUNTIME_FRAMEBUFFER_PITCH.store(pitch, Ordering::Release);
}

/// Publish a root diagnostic mapping for the framebuffer pages also copied into
/// the HDMI driver runtime VSpace.
#[cfg(feature = "kernel")]
pub fn publish_hdmi_runtime_root_framebuffer_mapping(vaddr: usize, map_len: usize) {
    HDMI_RUNTIME_ROOT_FRAMEBUFFER_VADDR.store(vaddr, Ordering::Release);
    HDMI_RUNTIME_ROOT_FRAMEBUFFER_MAP_LEN.store(map_len, Ordering::Release);
}

/// Return the bootloader framebuffer metadata staged for the linked HDMI runtime.
#[cfg(feature = "kernel")]
pub fn hdmi_runtime_framebuffer_hint() -> Option<DriverRuntimeFramebufferDescriptor> {
    let paddr = HDMI_RUNTIME_FRAMEBUFFER_PADDR.load(Ordering::Acquire);
    let width = HDMI_RUNTIME_FRAMEBUFFER_WIDTH.load(Ordering::Acquire);
    let height = HDMI_RUNTIME_FRAMEBUFFER_HEIGHT.load(Ordering::Acquire);
    let pitch = HDMI_RUNTIME_FRAMEBUFFER_PITCH.load(Ordering::Acquire);
    if paddr == 0 || width == 0 || height == 0 || pitch == 0 {
        return None;
    }
    let descriptor = DriverRuntimeFramebufferDescriptor {
        vaddr: DRIVER_RUNTIME_FRAMEBUFFER_VADDR,
        paddr: paddr as u64,
        width: width as u32,
        height: height as u32,
        pitch: pitch as u32,
        format: DRIVER_RUNTIME_FRAMEBUFFER_FORMAT_XRGB8888,
    };
    descriptor.valid().then_some(descriptor)
}

/// Return the root diagnostic framebuffer mapping, if HAL published one.
#[cfg(feature = "kernel")]
#[must_use]
pub fn hdmi_runtime_root_framebuffer_mapping() -> Option<HdmiRuntimeRootFramebufferMapping> {
    let paddr = HDMI_RUNTIME_FRAMEBUFFER_PADDR.load(Ordering::Acquire);
    let vaddr = HDMI_RUNTIME_ROOT_FRAMEBUFFER_VADDR.load(Ordering::Acquire);
    let width = HDMI_RUNTIME_FRAMEBUFFER_WIDTH.load(Ordering::Acquire);
    let height = HDMI_RUNTIME_FRAMEBUFFER_HEIGHT.load(Ordering::Acquire);
    let pitch = HDMI_RUNTIME_FRAMEBUFFER_PITCH.load(Ordering::Acquire);
    let map_len = HDMI_RUNTIME_ROOT_FRAMEBUFFER_MAP_LEN.load(Ordering::Acquire);
    if paddr == 0 || vaddr == 0 || width == 0 || height == 0 || pitch == 0 || map_len == 0 {
        return None;
    }
    Some(HdmiRuntimeRootFramebufferMapping {
        paddr,
        vaddr,
        width,
        height,
        pitch,
        map_len,
    })
}

#[cfg(feature = "kernel")]
fn driver_runtime_payload() -> Option<&'static [u8]> {
    let ptr = DRIVER_RUNTIME_PAYLOAD_PTR.load(Ordering::Acquire);
    let len = DRIVER_RUNTIME_PAYLOAD_LEN.load(Ordering::Acquire);
    if ptr == 0 || len == 0 {
        return None;
    }
    // SAFETY: `publish_driver_runtime_payload` stores the pointer/length for
    // the kernel-provided bootinfo extra slice, which is mapped for the root
    // task lifetime. The atomics are write-once during early boot.
    Some(unsafe { core::slice::from_raw_parts(ptr as *const u8, len) })
}

#[cfg(feature = "kernel")]
fn embedded_driver_runtime_payload() -> Option<&'static [u8]> {
    let payload = embedded_runtime_payload::EMBEDDED_PI4_DRIVER_RUNTIME_PAYLOAD;
    (!payload.is_empty()).then_some(payload)
}

#[cfg(feature = "kernel")]
fn read_cpio_hex(bytes: &[u8]) -> Option<usize> {
    let mut value = 0usize;
    for &byte in bytes {
        value = value.checked_mul(16)?;
        value = value.checked_add(match byte {
            b'0'..=b'9' => usize::from(byte - b'0'),
            b'a'..=b'f' => usize::from(byte - b'a' + 10),
            b'A'..=b'F' => usize::from(byte - b'A' + 10),
            _ => return None,
        })?;
    }
    Some(value)
}

#[cfg(feature = "kernel")]
fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|v| v & !3)
}

#[cfg(feature = "kernel")]
fn cpio_entry_data<'a>(archive: &'a [u8], name: &str) -> Option<&'a [u8]> {
    const HEADER_LEN: usize = 110;
    const MAGIC: &[u8; 6] = b"070701";

    let mut cursor = 0usize;
    while cursor.checked_add(HEADER_LEN)? <= archive.len() {
        let header = &archive[cursor..cursor + HEADER_LEN];
        if &header[..6] != MAGIC {
            return None;
        }
        let filesize = read_cpio_hex(&header[54..62])?;
        let namesize = read_cpio_hex(&header[94..102])?;
        if namesize == 0 {
            return None;
        }
        let name_start = cursor.checked_add(HEADER_LEN)?;
        let name_end = name_start.checked_add(namesize)?;
        if name_end > archive.len() {
            return None;
        }
        let raw_name = &archive[name_start..name_end.saturating_sub(1)];
        let entry_name = core::str::from_utf8(raw_name).ok()?;
        let data_start = align4(name_end)?;
        let data_end = data_start.checked_add(filesize)?;
        if data_end > archive.len() {
            return None;
        }
        if entry_name == "TRAILER!!!" {
            return None;
        }
        if entry_name == name || entry_name.strip_prefix("./") == Some(name) {
            return Some(&archive[data_start..data_end]);
        }
        cursor = align4(data_end)?;
    }
    None
}

#[cfg(feature = "kernel")]
fn cpio_entry_data_with_optional_wrapper<'a>(payload: &'a [u8], name: &str) -> Option<&'a [u8]> {
    if let Some(data) = cpio_entry_data(payload, name) {
        return Some(data);
    }
    let search_len = payload.len().min(4096);
    let mut offset = 1usize;
    while offset.checked_add(6)? <= search_len {
        if payload.get(offset..offset + 6) == Some(b"070701") {
            if let Some(data) = cpio_entry_data(&payload[offset..], name) {
                return Some(data);
            }
        }
        offset = offset.saturating_add(1);
    }
    None
}

/// Return the linked driver runtime image bytes for a Pi 4 hot path.
#[cfg(feature = "kernel")]
pub fn driver_runtime_image_bytes(hot_path: DriverTaskHotPath) -> Option<&'static [u8]> {
    let generated = generated_runtime_image_spec_for_hot_path(hot_path)?;
    let physical_pi = physical_pi_driver_task_only_owner_state_active();
    driver_runtime_image_bytes_from_payloads(
        generated.artifact,
        if physical_pi {
            None
        } else {
            driver_runtime_payload()
        },
        embedded_driver_runtime_payload(),
    )
}

#[cfg(feature = "kernel")]
fn driver_runtime_image_bytes_from_payloads<'a>(
    artifact: &str,
    bootinfo_payload: Option<&'a [u8]>,
    embedded_payload: Option<&'a [u8]>,
) -> Option<&'a [u8]> {
    driver_runtime_image_bytes_from_payloads_for_profile(
        artifact,
        bootinfo_payload,
        embedded_payload,
        physical_pi_driver_task_only_owner_state_active(),
    )
}

#[cfg(feature = "kernel")]
fn driver_runtime_image_bytes_from_payloads_for_profile<'a>(
    artifact: &str,
    bootinfo_payload: Option<&'a [u8]>,
    embedded_payload: Option<&'a [u8]>,
    require_embedded_payload: bool,
) -> Option<&'a [u8]> {
    if require_embedded_payload {
        return embedded_payload
            .and_then(|payload| cpio_entry_data_with_optional_wrapper(payload, artifact));
    }
    if let Some(payload) = bootinfo_payload {
        if let Some(image) = cpio_entry_data_with_optional_wrapper(payload, artifact) {
            return Some(image);
        }
    }
    embedded_payload.and_then(|payload| cpio_entry_data_with_optional_wrapper(payload, artifact))
}

fn generated_runtime_image_spec_for_hot_path(
    hot_path: DriverTaskHotPath,
) -> Option<crate::generated::DriverRuntimeImageSpec> {
    crate::generated::driver_runtime_image_for_hot_path(hot_path.as_str())
}

fn fallback_runtime_image_spec(hot_path: DriverTaskHotPath) -> DriverTaskRuntimeImageSpec {
    DriverTaskRuntimeImageSpec::new(hot_path, 1, 1, 0, 0, 0, true, false)
}

fn runtime_image_spec_from_generated(
    hot_path: DriverTaskHotPath,
    generated: crate::generated::DriverRuntimeImageSpec,
) -> DriverTaskRuntimeImageSpec {
    DriverTaskRuntimeImageSpec::new(
        hot_path,
        generated.code_pages,
        generated.stack_pages,
        generated.mmio_pages,
        generated.dma_pages,
        generated.shared_buffer_pages,
        generated.root_context_required,
        generated.hardware_state_migrated,
    )
}

/// Runtime-image specs for every Pi 4 hardware hot path.
///
/// These are generated manifest contracts, not fresh Pi hardware proof. They
/// make each linked runtime acceptance-eligible once code, stack, IPC, ring,
/// MMIO/DMA, and shared-buffer mappings are declared for the isolated image.
#[must_use]
pub fn pi4_driver_task_runtime_image_specs() -> [DriverTaskRuntimeImageSpec; 7] {
    [
        pi4_driver_task_runtime_image_spec(DriverTaskHotPath::SerialConsole),
        pi4_driver_task_runtime_image_spec(DriverTaskHotPath::UsbKeyboard),
        pi4_driver_task_runtime_image_spec(DriverTaskHotPath::HdmiText),
        pi4_driver_task_runtime_image_spec(DriverTaskHotPath::GenetNic),
        pi4_driver_task_runtime_image_spec(DriverTaskHotPath::Cyw43Wifi),
        pi4_driver_task_runtime_image_spec(DriverTaskHotPath::SdioHost),
        pi4_driver_task_runtime_image_spec(DriverTaskHotPath::PcieRoot),
    ]
}

/// Returns the runtime-image spec for a Pi 4 hot path.
#[must_use]
pub fn pi4_driver_task_runtime_image_spec(
    hot_path: DriverTaskHotPath,
) -> DriverTaskRuntimeImageSpec {
    generated_runtime_image_spec_for_hot_path(hot_path)
        .map(|generated| runtime_image_spec_from_generated(hot_path, generated))
        .unwrap_or_else(|| fallback_runtime_image_spec(hot_path))
}

/// Returns the Pi 4 runtime-image spec for a driver-task contract when the
/// contract owns one of the required hardware hot paths.
#[must_use]
pub fn pi4_driver_task_runtime_image_spec_for_contract(
    contract: DriverTaskContract,
) -> Option<DriverTaskRuntimeImageSpec> {
    for spec in pi4_driver_task_runtime_image_specs() {
        if spec.hot_path.contract() == contract {
            return Some(spec);
        }
    }
    None
}

/// Small dedicated CSpace radix for bootstrap driver tasks.
#[cfg(feature = "kernel")]
pub const DRIVER_TASK_CHILD_CNODE_RADIX_BITS: u8 = 4;

/// Role bit required for serial dedicated-task proof.
pub const DRIVER_TASK_ROLE_SERIAL_BIT: usize = 1 << 0;
/// Role bit required for USB/local-seat dedicated-task proof.
pub const DRIVER_TASK_ROLE_USB_BIT: usize = 1 << 1;
/// Role bit required for display dedicated-task proof.
pub const DRIVER_TASK_ROLE_DISPLAY_BIT: usize = 1 << 2;
/// Role bit required for active network dedicated-task proof.
pub const DRIVER_TASK_ROLE_NET_BIT: usize = 1 << 3;
/// Role bit required for the SDIO host dedicated-task proof.
pub const DRIVER_TASK_ROLE_SDIO_BIT: usize = 1 << 4;
/// Role bit required for the PCIe root dedicated-task proof.
pub const DRIVER_TASK_ROLE_PCIE_BIT: usize = 1 << 5;
/// Required role coverage for reopened 26a/26b closure.
pub const REQUIRED_DRIVER_TASK_ROLE_MASK: usize = DRIVER_TASK_ROLE_SERIAL_BIT
    | DRIVER_TASK_ROLE_USB_BIT
    | DRIVER_TASK_ROLE_DISPLAY_BIT
    | DRIVER_TASK_ROLE_NET_BIT
    | DRIVER_TASK_ROLE_SDIO_BIT
    | DRIVER_TASK_ROLE_PCIE_BIT;

/// Current role coverage required for owner-state acceptance.
pub const REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK: usize = DRIVER_TASK_ROLE_SERIAL_BIT
    | DRIVER_TASK_ROLE_USB_BIT
    | DRIVER_TASK_ROLE_DISPLAY_BIT
    | DRIVER_TASK_ROLE_NET_BIT
    | DRIVER_TASK_ROLE_SDIO_BIT
    | DRIVER_TASK_ROLE_PCIE_BIT;

#[cfg(feature = "kernel")]
static DRIVER_TASK_SUBSTRATE_ACTIVE: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_CONFIGURED_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_FAILED_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_LIVE_TCB_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_LIVE_TCB_ROLE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_STARTED_ROLE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_STARTED_TASK_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_HOT_PATH_ROLE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_SHARED_RING_SERVICE_ROLE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_OWNER_STATE_ROLE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_OWNER_STATE_HOT_PATH_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_COMPAT_SERVICE_ROLE_MASK: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_CAPSET_PROOF: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_FAULT_PROOF: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_REVOKE_PROOF: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_SCHED_PROOF: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_AFFINITY_CONFIGURED_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_AFFINITY_APPLIED_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_AFFINITY_PROOF: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_VSPACE_PROOF: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_POINTER_FREE_IPC_PROOF: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_OWNER_STATE_PROOF: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_BROAD_CAPS_LEAKED: AtomicUsize = AtomicUsize::new(usize::MAX);
#[cfg(feature = "kernel")]
static DRIVER_TASK_ENTRY_HEARTBEATS: AtomicUsize = AtomicUsize::new(0);

/// Stable key for the serial driver TCB.
pub const DRIVER_TASK_KEY_SERIAL: usize = 0;
/// Stable key for the USB/local-seat driver TCB.
pub const DRIVER_TASK_KEY_USB_LOCAL_SEAT: usize = 1;
/// Stable key for the HDMI text driver TCB.
pub const DRIVER_TASK_KEY_HDMI_TEXT: usize = 2;
/// Stable key for the GENET driver TCB.
pub const DRIVER_TASK_KEY_BCMGENET_V5: usize = 3;
/// Stable key for the CYW43 Wi-Fi driver TCB.
pub const DRIVER_TASK_KEY_CYW43455: usize = 4;
/// Stable key for the RTL8139 driver TCB.
pub const DRIVER_TASK_KEY_RTL8139: usize = 5;
/// Stable key for the virtio-net driver TCB.
pub const DRIVER_TASK_KEY_VIRTIO_NET: usize = 6;
/// Stable key for the SDIO host driver TCB.
pub const DRIVER_TASK_KEY_SDIO_HOST: usize = 7;
/// Stable key for the PCIe root driver TCB.
pub const DRIVER_TASK_KEY_PCIE_ROOT: usize = 8;

/// Number of built-in driver TCBs expected for full substrate bootstrap.
pub const EXPECTED_DRIVER_TASK_BOOTSTRAP_COUNT: usize = 9;
/// Bounded send/yield attempts for the first linked-runtime ring handshake.
pub const DRIVER_TASK_BOOTSTRAP_RING_ATTEMPTS: usize = 4096;
const DRIVER_TASK_PROMPT_RING_ATTEMPTS: usize = 128;
const DRIVER_TASK_USB_PROMPT_POLL_RING_ATTEMPTS: usize = 8;
const DRIVER_TASK_USB_PROMPT_INIT_RING_ATTEMPTS: usize = 512;
const DRIVER_TASK_USB_PROMPT_ENUM_RING_ATTEMPTS: usize = 16_384;
const DRIVER_TASK_LONG_INIT_RING_ATTEMPTS: usize = 262_144;

#[cfg(feature = "kernel")]
struct DriverTaskCommandSlot {
    tcb: AtomicUsize,
    steady_priority: AtomicUsize,
    steady_priority_active: AtomicUsize,
    endpoint: AtomicUsize,
    ring_root_ptr: AtomicUsize,
    ring_frame_cap: AtomicUsize,
    shared_frame_count: AtomicUsize,
    shared_frame_caps: [AtomicUsize; DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY],
    request_seq: AtomicUsize,
    active: AtomicUsize,
    ring_handler: AtomicUsize,
    ring_context: AtomicUsize,
    ring_service_kind: AtomicUsize,
    #[cfg(any(
        not(target_arch = "aarch64"),
        not(target_os = "none"),
        feature = "net-backend-virtio"
    ))]
    handler: AtomicUsize,
    #[cfg(any(
        not(target_arch = "aarch64"),
        not(target_os = "none"),
        feature = "net-backend-virtio"
    ))]
    context: AtomicUsize,
    #[cfg(any(
        not(target_arch = "aarch64"),
        not(target_os = "none"),
        feature = "net-backend-virtio"
    ))]
    done_seq: AtomicUsize,
    #[cfg(any(
        not(target_arch = "aarch64"),
        not(target_os = "none"),
        feature = "net-backend-virtio"
    ))]
    result: AtomicUsize,
}

#[cfg(feature = "kernel")]
impl DriverTaskCommandSlot {
    const fn new() -> Self {
        Self {
            tcb: AtomicUsize::new(0),
            steady_priority: AtomicUsize::new(0),
            steady_priority_active: AtomicUsize::new(0),
            endpoint: AtomicUsize::new(0),
            ring_root_ptr: AtomicUsize::new(0),
            ring_frame_cap: AtomicUsize::new(0),
            shared_frame_count: AtomicUsize::new(0),
            shared_frame_caps: [AtomicUsize::new(0), AtomicUsize::new(0)],
            request_seq: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            ring_handler: AtomicUsize::new(0),
            ring_context: AtomicUsize::new(0),
            ring_service_kind: AtomicUsize::new(DriverTaskRingServiceKind::None.as_usize()),
            #[cfg(any(
                not(target_arch = "aarch64"),
                not(target_os = "none"),
                feature = "net-backend-virtio"
            ))]
            handler: AtomicUsize::new(0),
            #[cfg(any(
                not(target_arch = "aarch64"),
                not(target_os = "none"),
                feature = "net-backend-virtio"
            ))]
            context: AtomicUsize::new(0),
            #[cfg(any(
                not(target_arch = "aarch64"),
                not(target_os = "none"),
                feature = "net-backend-virtio"
            ))]
            done_seq: AtomicUsize::new(0),
            #[cfg(any(
                not(target_arch = "aarch64"),
                not(target_os = "none"),
                feature = "net-backend-virtio"
            ))]
            result: AtomicUsize::new(0),
        }
    }
}

#[cfg(feature = "kernel")]
static DRIVER_TASK_SLOT_SERIAL: DriverTaskCommandSlot = DriverTaskCommandSlot::new();
#[cfg(feature = "kernel")]
static DRIVER_TASK_SLOT_USB_LOCAL_SEAT: DriverTaskCommandSlot = DriverTaskCommandSlot::new();
#[cfg(feature = "kernel")]
static DRIVER_TASK_SLOT_HDMI_TEXT: DriverTaskCommandSlot = DriverTaskCommandSlot::new();
#[cfg(feature = "kernel")]
static DRIVER_TASK_SLOT_BCMGENET_V5: DriverTaskCommandSlot = DriverTaskCommandSlot::new();
#[cfg(feature = "kernel")]
static DRIVER_TASK_SLOT_CYW43455: DriverTaskCommandSlot = DriverTaskCommandSlot::new();
#[cfg(feature = "kernel")]
static DRIVER_TASK_SLOT_RTL8139: DriverTaskCommandSlot = DriverTaskCommandSlot::new();
#[cfg(feature = "kernel")]
static DRIVER_TASK_SLOT_VIRTIO_NET: DriverTaskCommandSlot = DriverTaskCommandSlot::new();
#[cfg(feature = "kernel")]
static DRIVER_TASK_SLOT_SDIO_HOST: DriverTaskCommandSlot = DriverTaskCommandSlot::new();
#[cfg(feature = "kernel")]
static DRIVER_TASK_SLOT_PCIE_ROOT: DriverTaskCommandSlot = DriverTaskCommandSlot::new();

#[cfg(feature = "kernel")]
static DRIVER_TASK_OBSERVED_US_SERIAL: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_OBSERVED_US_USB_LOCAL_SEAT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_OBSERVED_US_HDMI_TEXT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_OBSERVED_US_BCMGENET_V5: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_OBSERVED_US_CYW43455: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_OBSERVED_US_RTL8139: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_OBSERVED_US_VIRTIO_NET: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_OBSERVED_US_SDIO_HOST: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static DRIVER_TASK_OBSERVED_US_PCIE_ROOT: AtomicU32 = AtomicU32::new(0);
#[cfg(all(feature = "kernel", not(target_arch = "aarch64")))]
static DRIVER_TASK_TEST_COUNTER_TICKS: AtomicU64 = AtomicU64::new(0);

/// Return the stable driver-task key for a contract.
#[must_use]
pub fn driver_task_contract_key(contract: DriverTaskContract) -> Option<usize> {
    match contract.name {
        "serial" => Some(DRIVER_TASK_KEY_SERIAL),
        "usb-local-seat" => Some(DRIVER_TASK_KEY_USB_LOCAL_SEAT),
        "hdmi-text" => Some(DRIVER_TASK_KEY_HDMI_TEXT),
        "bcmgenet-v5" => Some(DRIVER_TASK_KEY_BCMGENET_V5),
        "cyw43455" => Some(DRIVER_TASK_KEY_CYW43455),
        "rtl8139" => Some(DRIVER_TASK_KEY_RTL8139),
        "virtio-net" => Some(DRIVER_TASK_KEY_VIRTIO_NET),
        "sdio-host" => Some(DRIVER_TASK_KEY_SDIO_HOST),
        "pcie-root" => Some(DRIVER_TASK_KEY_PCIE_ROOT),
        _ => None,
    }
}

/// Return the role mask bit covered by a stable driver-task key.
#[must_use]
pub const fn driver_task_task_key_role_bit(task_key: usize) -> Option<usize> {
    match task_key {
        DRIVER_TASK_KEY_SERIAL => Some(DRIVER_TASK_ROLE_SERIAL_BIT),
        DRIVER_TASK_KEY_USB_LOCAL_SEAT => Some(DRIVER_TASK_ROLE_USB_BIT),
        DRIVER_TASK_KEY_HDMI_TEXT => Some(DRIVER_TASK_ROLE_DISPLAY_BIT),
        DRIVER_TASK_KEY_BCMGENET_V5
        | DRIVER_TASK_KEY_CYW43455
        | DRIVER_TASK_KEY_RTL8139
        | DRIVER_TASK_KEY_VIRTIO_NET => Some(DRIVER_TASK_ROLE_NET_BIT),
        DRIVER_TASK_KEY_SDIO_HOST => Some(DRIVER_TASK_ROLE_SDIO_BIT),
        DRIVER_TASK_KEY_PCIE_ROOT => Some(DRIVER_TASK_ROLE_PCIE_BIT),
        _ => None,
    }
}

#[cfg(feature = "kernel")]
fn slot_for_task_key(task_key: usize) -> Option<&'static DriverTaskCommandSlot> {
    match task_key {
        DRIVER_TASK_KEY_SERIAL => Some(&DRIVER_TASK_SLOT_SERIAL),
        DRIVER_TASK_KEY_USB_LOCAL_SEAT => Some(&DRIVER_TASK_SLOT_USB_LOCAL_SEAT),
        DRIVER_TASK_KEY_HDMI_TEXT => Some(&DRIVER_TASK_SLOT_HDMI_TEXT),
        DRIVER_TASK_KEY_BCMGENET_V5 => Some(&DRIVER_TASK_SLOT_BCMGENET_V5),
        DRIVER_TASK_KEY_CYW43455 => Some(&DRIVER_TASK_SLOT_CYW43455),
        DRIVER_TASK_KEY_RTL8139 => Some(&DRIVER_TASK_SLOT_RTL8139),
        DRIVER_TASK_KEY_VIRTIO_NET => Some(&DRIVER_TASK_SLOT_VIRTIO_NET),
        DRIVER_TASK_KEY_SDIO_HOST => Some(&DRIVER_TASK_SLOT_SDIO_HOST),
        DRIVER_TASK_KEY_PCIE_ROOT => Some(&DRIVER_TASK_SLOT_PCIE_ROOT),
        _ => None,
    }
}

#[cfg(feature = "kernel")]
fn observed_service_us_cell(contract: DriverTaskContract) -> Option<&'static AtomicU32> {
    match driver_task_contract_key(contract)? {
        DRIVER_TASK_KEY_SERIAL => Some(&DRIVER_TASK_OBSERVED_US_SERIAL),
        DRIVER_TASK_KEY_USB_LOCAL_SEAT => Some(&DRIVER_TASK_OBSERVED_US_USB_LOCAL_SEAT),
        DRIVER_TASK_KEY_HDMI_TEXT => Some(&DRIVER_TASK_OBSERVED_US_HDMI_TEXT),
        DRIVER_TASK_KEY_BCMGENET_V5 => Some(&DRIVER_TASK_OBSERVED_US_BCMGENET_V5),
        DRIVER_TASK_KEY_CYW43455 => Some(&DRIVER_TASK_OBSERVED_US_CYW43455),
        DRIVER_TASK_KEY_RTL8139 => Some(&DRIVER_TASK_OBSERVED_US_RTL8139),
        DRIVER_TASK_KEY_VIRTIO_NET => Some(&DRIVER_TASK_OBSERVED_US_VIRTIO_NET),
        DRIVER_TASK_KEY_SDIO_HOST => Some(&DRIVER_TASK_OBSERVED_US_SDIO_HOST),
        DRIVER_TASK_KEY_PCIE_ROOT => Some(&DRIVER_TASK_OBSERVED_US_PCIE_ROOT),
        _ => None,
    }
}

#[cfg(feature = "kernel")]
fn observed_service_us_for_contract(contract: DriverTaskContract) -> u32 {
    observed_service_us_cell(contract)
        .map(|cell| cell.load(Ordering::Acquire))
        .unwrap_or(0)
}

#[cfg(not(feature = "kernel"))]
const fn observed_service_us_for_contract(_contract: DriverTaskContract) -> u32 {
    0
}

#[must_use]
const fn driver_task_elapsed_us(start_ticks: u64, end_ticks: u64, counter_frequency: u64) -> u32 {
    if counter_frequency == 0 {
        return 1;
    }
    let delta = end_ticks.saturating_sub(start_ticks);
    let micros = (delta as u128)
        .saturating_mul(1_000_000u128)
        .saturating_div(counter_frequency as u128);
    if micros == 0 {
        1
    } else if micros > u32::MAX as u128 {
        u32::MAX
    } else {
        micros as u32
    }
}

#[cfg(feature = "kernel")]
fn record_observed_service_us(contract: DriverTaskContract, observed_us: u32) {
    let Some(cell) = observed_service_us_cell(contract) else {
        return;
    };
    let mut current = cell.load(Ordering::Acquire);
    while observed_us > current {
        match cell.compare_exchange(current, observed_us, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(next) => current = next,
        }
    }
}

#[cfg(feature = "kernel")]
#[inline]
fn driver_task_counter_frequency() -> Option<u64> {
    #[cfg(all(target_arch = "aarch64", feature = "timers-arch-counter"))]
    {
        Some(read_cntfrq())
    }
    #[cfg(all(target_arch = "aarch64", not(feature = "timers-arch-counter")))]
    {
        None
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        Some(1_000_000)
    }
}

#[cfg(feature = "kernel")]
#[inline]
fn driver_task_counter_ticks() -> Option<u64> {
    #[cfg(all(target_arch = "aarch64", feature = "timers-arch-counter"))]
    {
        Some(read_cntvct())
    }
    #[cfg(all(target_arch = "aarch64", not(feature = "timers-arch-counter")))]
    {
        None
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        Some(DRIVER_TASK_TEST_COUNTER_TICKS.fetch_add(1, Ordering::Relaxed))
    }
}

#[cfg(all(feature = "kernel", target_arch = "aarch64"))]
#[inline]
fn read_cntvct() -> u64 {
    let value: u64;
    // SAFETY: CNTVCT_EL0 is the EL0-readable architectural virtual counter
    // exposed by seL4 for userspace timing. This is read-only telemetry and
    // does not change device, kernel, or capability authority.
    unsafe {
        core::arch::asm!("mrs {value}, cntvct_el0", value = out(reg) value);
    }
    value
}

#[cfg(all(feature = "kernel", target_arch = "aarch64"))]
#[inline]
fn read_cntfrq() -> u64 {
    let value: u64;
    // SAFETY: CNTFRQ_EL0 is a read-only architectural frequency register. The
    // value is used only to convert local service timing into proof telemetry.
    unsafe {
        core::arch::asm!("mrs {value}, cntfrq_el0", value = out(reg) value);
    }
    value
}

/// Publish the TCB cap and steady priority backing one driver task.
#[cfg(feature = "kernel")]
pub fn publish_driver_task_scheduler(
    contract: DriverTaskContract,
    tcb: usize,
    steady_priority: u8,
) {
    let Some(task_key) = driver_task_contract_key(contract) else {
        return;
    };
    let Some(slot) = slot_for_task_key(task_key) else {
        return;
    };
    slot.tcb.store(tcb, Ordering::Release);
    slot.steady_priority
        .store(usize::from(steady_priority), Ordering::Release);
    slot.steady_priority_active.store(0, Ordering::Release);
}

/// Mark that one driver task has left bootstrap priority and is in steady state.
#[cfg(feature = "kernel")]
pub fn publish_driver_task_steady_priority_active(contract: DriverTaskContract) {
    let Some(task_key) = driver_task_contract_key(contract) else {
        return;
    };
    let Some(slot) = slot_for_task_key(task_key) else {
        return;
    };
    slot.steady_priority_active.store(1, Ordering::Release);
}

/// Publish the root-side command endpoint for a created driver TCB.
#[cfg(feature = "kernel")]
pub fn publish_driver_task_command_endpoint(contract: DriverTaskContract, endpoint: usize) {
    let Some(task_key) = driver_task_contract_key(contract) else {
        return;
    };
    let Some(slot) = slot_for_task_key(task_key) else {
        return;
    };
    slot.endpoint.store(endpoint, Ordering::Release);
}

/// Publish the root mapping of the fixed command/completion ring for a driver.
#[cfg(feature = "kernel")]
pub fn publish_driver_task_ring(contract: DriverTaskContract, ring_root_ptr: usize) {
    let Some(task_key) = driver_task_contract_key(contract) else {
        return;
    };
    let Some(slot) = slot_for_task_key(task_key) else {
        return;
    };
    slot.ring_root_ptr.store(ring_root_ptr, Ordering::Release);
}

/// Publish the root CSpace cap for the frame backing a driver command ring.
#[cfg(feature = "kernel")]
pub fn publish_driver_task_ring_frame_cap(contract: DriverTaskContract, ring_frame_cap: usize) {
    let Some(task_key) = driver_task_contract_key(contract) else {
        return;
    };
    let Some(slot) = slot_for_task_key(task_key) else {
        return;
    };
    slot.ring_frame_cap.store(ring_frame_cap, Ordering::Release);
}

/// Publish one root CSpace cap backing a driver shared-buffer page.
#[cfg(feature = "kernel")]
pub fn publish_driver_task_shared_frame_cap(
    contract: DriverTaskContract,
    page_index: usize,
    shared_frame_cap: usize,
) {
    if page_index >= DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY {
        return;
    }
    let Some(task_key) = driver_task_contract_key(contract) else {
        return;
    };
    let Some(slot) = slot_for_task_key(task_key) else {
        return;
    };
    slot.shared_frame_caps[page_index].store(shared_frame_cap, Ordering::Release);
    let required_count = page_index.saturating_add(1);
    let current = slot.shared_frame_count.load(Ordering::Acquire);
    if required_count > current {
        slot.shared_frame_count
            .store(required_count, Ordering::Release);
    }
}

/// Return the endpoint and ring-frame caps for a linked bus-owner runtime.
#[cfg(feature = "kernel")]
#[must_use]
pub fn driver_task_bus_owner_transport_caps(
    contract: DriverTaskContract,
) -> Option<(sel4_sys::seL4_CPtr, sel4_sys::seL4_CPtr)> {
    let task_key = driver_task_contract_key(contract)?;
    let slot = slot_for_task_key(task_key)?;
    let endpoint = slot.endpoint.load(Ordering::Acquire);
    let ring_frame_cap = slot.ring_frame_cap.load(Ordering::Acquire);
    if endpoint == 0 || ring_frame_cap == 0 {
        return None;
    }
    Some((
        endpoint as sel4_sys::seL4_CPtr,
        ring_frame_cap as sel4_sys::seL4_CPtr,
    ))
}

/// Return the endpoint, ring-frame cap, and bounded shared-buffer caps for a
/// linked bus-owner runtime.
#[cfg(feature = "kernel")]
#[must_use]
pub fn driver_task_bus_owner_transport_caps_with_shared(
    contract: DriverTaskContract,
    min_shared_pages: usize,
) -> Option<(
    sel4_sys::seL4_CPtr,
    sel4_sys::seL4_CPtr,
    [sel4_sys::seL4_CPtr; DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY],
)> {
    if min_shared_pages > DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY {
        return None;
    }
    let (endpoint, ring_frame_cap) = driver_task_bus_owner_transport_caps(contract)?;
    let task_key = driver_task_contract_key(contract)?;
    let slot = slot_for_task_key(task_key)?;
    if slot.shared_frame_count.load(Ordering::Acquire) < min_shared_pages {
        return None;
    }
    let mut shared_frame_caps = [0; DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY];
    let mut index = 0usize;
    while index < min_shared_pages {
        let cap = slot.shared_frame_caps[index].load(Ordering::Acquire);
        if cap == 0 {
            return None;
        }
        shared_frame_caps[index] = cap as sel4_sys::seL4_CPtr;
        index = index.saturating_add(1);
    }
    Some((endpoint, ring_frame_cap, shared_frame_caps))
}

/// Clear a partially published driver-task transport after bootstrap failure.
#[cfg(feature = "kernel")]
pub fn clear_driver_task_transport(contract: DriverTaskContract) {
    let Some(task_key) = driver_task_contract_key(contract) else {
        return;
    };
    let Some(slot) = slot_for_task_key(task_key) else {
        return;
    };
    slot.endpoint.store(0, Ordering::Release);
    slot.ring_root_ptr.store(0, Ordering::Release);
    slot.ring_frame_cap.store(0, Ordering::Release);
    slot.shared_frame_count.store(0, Ordering::Release);
    for shared_frame_cap in &slot.shared_frame_caps {
        shared_frame_cap.store(0, Ordering::Release);
    }
    slot.tcb.store(0, Ordering::Release);
    slot.steady_priority.store(0, Ordering::Release);
    slot.steady_priority_active.store(0, Ordering::Release);
    slot.active.store(0, Ordering::Release);
    slot.request_seq.store(0, Ordering::Release);
}

#[cfg(feature = "kernel")]
struct DriverTaskPriorityRestore {
    contract: DriverTaskContract,
    tcb: usize,
    steady_priority: u8,
}

#[cfg(feature = "kernel")]
impl Drop for DriverTaskPriorityRestore {
    fn drop(&mut self) {
        let tcb = self.tcb as sel4_sys::seL4_CPtr;
        let priority = self.steady_priority;
        let sched = crate::sel4::set_tcb_sched_params(
            tcb,
            sel4_sys::seL4_CapInitThreadTCB,
            priority,
            priority,
        );
        let prio = crate::sel4::set_tcb_priority(tcb, sel4_sys::seL4_CapInitThreadTCB, priority);
        if sched.is_err() || prio.is_err() {
            let mut line = heapless::String::<192>::new();
            let _ = core::fmt::write(
                &mut line,
                format_args!(
                    "DRIVER_TASK_PRIORITY_RESTORE contract={} tcb=0x{:04x} priority={} status=failed",
                    self.contract.name, self.tcb, priority,
                ),
            );
            crate::bootstrap::log::force_uart_line_raw(line.as_str());
        }
    }
}

#[cfg(feature = "kernel")]
fn boost_driver_task_priority_for_bounded_turn(
    contract: DriverTaskContract,
) -> Option<DriverTaskPriorityRestore> {
    if !physical_pi_driver_task_only_owner_state_active() {
        return None;
    }
    let task_key = driver_task_contract_key(contract)?;
    let slot = slot_for_task_key(task_key)?;
    let tcb = slot.tcb.load(Ordering::Acquire);
    let steady_priority = slot.steady_priority.load(Ordering::Acquire);
    if tcb == 0 || steady_priority == 0 || slot.steady_priority_active.load(Ordering::Acquire) == 0
    {
        return None;
    }
    let steady_priority = steady_priority as u8;
    let boost = PI4_BOUNDED_BOOTSTRAP_PRIORITY;
    if steady_priority >= boost {
        return None;
    }
    let tcb_cap = tcb as sel4_sys::seL4_CPtr;
    if crate::sel4::set_tcb_sched_params(tcb_cap, sel4_sys::seL4_CapInitThreadTCB, boost, boost)
        .is_err()
        || crate::sel4::set_tcb_priority(tcb_cap, sel4_sys::seL4_CapInitThreadTCB, boost).is_err()
    {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::write(
            &mut line,
            format_args!(
                "DRIVER_TASK_PRIORITY_BOOST contract={} tcb=0x{:04x} priority={} status=failed",
                contract.name, tcb, boost,
            ),
        );
        crate::bootstrap::log::force_uart_line_raw(line.as_str());
        return None;
    }
    Some(DriverTaskPriorityRestore {
        contract,
        tcb,
        steady_priority,
    })
}

#[cfg(feature = "kernel")]
fn driver_task_bounded_turn_bus_owner(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskContract> {
    if matches!(contract.kind, DriverTaskKind::WifiNic)
        && command.arg0 == DriverTaskHotPath::Cyw43Wifi.as_u32()
    {
        Some(SDIO_HOST_DRIVER_TASK_CONTRACT)
    } else if matches!(contract.kind, DriverTaskKind::LocalSeatUsb)
        && command.arg0 == DriverTaskHotPath::UsbKeyboard.as_u32()
    {
        Some(PCIE_ROOT_DRIVER_TASK_CONTRACT)
    } else {
        None
    }
}

#[cfg(feature = "kernel")]
fn boost_driver_task_priorities_for_bounded_turn(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> (
    Option<DriverTaskPriorityRestore>,
    Option<DriverTaskPriorityRestore>,
) {
    let primary = boost_driver_task_priority_for_bounded_turn(contract);
    let bus_owner = driver_task_bounded_turn_bus_owner(contract, command)
        .and_then(boost_driver_task_priority_for_bounded_turn);
    (primary, bus_owner)
}

#[cfg(feature = "kernel")]
fn register_driver_task_ring_service_with_kind(
    contract: DriverTaskContract,
    context: usize,
    handler: DriverTaskRingServiceHandler,
    kind: DriverTaskRingServiceKind,
) -> bool {
    let Some(task_key) = driver_task_contract_key(contract) else {
        return false;
    };
    let Some(slot) = slot_for_task_key(task_key) else {
        return false;
    };
    slot.ring_context.store(context, Ordering::Release);
    slot.ring_handler
        .store(handler as *const () as usize, Ordering::Release);
    slot.ring_service_kind
        .store(kind.as_usize(), Ordering::Release);
    true
}

/// Register a transitional shared-ring handler that receives root context.
///
/// This keeps the physical Pi 4 service path explicit while the live hardware
/// state still resides in root-owned structs. Commands submitted through this
/// registration are forced into root-context non-acceptance and cannot satisfy
/// owner-state proof.
#[cfg(feature = "kernel")]
pub fn register_driver_task_root_context_ring_service(
    contract: DriverTaskContract,
    context: usize,
    handler: DriverTaskRingServiceHandler,
) -> bool {
    register_driver_task_ring_service_with_kind(
        contract,
        context,
        handler,
        DriverTaskRingServiceKind::RootContextDiagnostic,
    )
}

/// Register a pointer-free shared-ring handler.
///
/// The context word must be a primitive selector, not a root pointer. This
/// class is necessary but not sufficient for owner-state proof; proof is still
/// gated by isolated VSpace, pointer-free IPC, and per-hot-path owner-state
/// descriptors.
#[cfg(feature = "kernel")]
pub fn register_driver_task_pointer_free_ring_service(
    contract: DriverTaskContract,
    selector: usize,
    handler: DriverTaskRingServiceHandler,
) -> bool {
    register_driver_task_ring_service_with_kind(
        contract,
        selector,
        handler,
        DriverTaskRingServiceKind::PointerFreeSelector,
    )
}

#[cfg(feature = "kernel")]
fn driver_task_ring_service_kind(contract: DriverTaskContract) -> DriverTaskRingServiceKind {
    let Some(task_key) = driver_task_contract_key(contract) else {
        return DriverTaskRingServiceKind::None;
    };
    let Some(slot) = slot_for_task_key(task_key) else {
        return DriverTaskRingServiceKind::None;
    };
    DriverTaskRingServiceKind::from_usize(slot.ring_service_kind.load(Ordering::Acquire))
}

/// Returns whether a completed ring service may credit owner-state proof.
#[must_use]
pub const fn driver_task_ring_service_owner_state_credit_eligible(
    kind: DriverTaskRingServiceKind,
    command: DriverTaskCommandRecord,
) -> bool {
    kind.owner_state_credit_allowed() && command.owner_state_credit_eligible()
}

/// Pointer-free descriptor proving a hardware owner's state boundary.
///
/// The descriptor is intentionally primitive-only. It identifies the ring-backed
/// hardware owner and the bounded shared-buffer region used to exchange work
/// with root, but it never carries a root pointer or callback context.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTaskOwnerStateDescriptor {
    /// Hot path owned by the driver task.
    pub hot_path: DriverTaskHotPath,
    /// Offset of the command/metadata region within the shared ring page.
    pub state_offset: u32,
    /// Bytes reserved for driver-owned state metadata.
    pub state_len: u16,
    /// Offset of the shared RX/TX/control buffer region.
    pub buffer_offset: u32,
    /// Bytes reserved for shared buffers.
    pub buffer_len: u16,
    /// Descriptor flags reserved for future ownership variants.
    pub flags: u16,
}

impl DriverTaskOwnerStateDescriptor {
    /// Construct a bounded owner-state descriptor.
    #[must_use]
    pub const fn new(
        hot_path: DriverTaskHotPath,
        state_offset: u32,
        state_len: u16,
        buffer_offset: u32,
        buffer_len: u16,
        flags: u16,
    ) -> Option<Self> {
        let state_end = state_offset as usize + state_len as usize;
        let buffer_end = buffer_offset as usize + buffer_len as usize;
        let state_in_owner_region = (state_offset as usize) >= DRIVER_TASK_OWNER_STATE_OFFSET
            && state_end <= DRIVER_TASK_OWNER_STATE_OFFSET + DRIVER_TASK_OWNER_STATE_BYTES;
        if state_len == 0
            || buffer_len == 0
            || state_offset as usize >= DRIVER_TASK_RING_PAGE_BYTES
            || buffer_offset as usize >= DRIVER_TASK_RING_PAGE_BYTES
            || state_end > DRIVER_TASK_RING_PAGE_BYTES
            || buffer_end > DRIVER_TASK_RING_PAGE_BYTES
            || !state_in_owner_region
            || (buffer_offset as usize) < DRIVER_TASK_RING_FRAME_OFFSET
        {
            return None;
        }
        Some(Self {
            hot_path,
            state_offset,
            state_len,
            buffer_offset,
            buffer_len,
            flags,
        })
    }

    /// Returns whether this descriptor represents a real isolated runtime
    /// ownership boundary rather than ring-shape scaffolding.
    #[must_use]
    pub const fn has_required_runtime_flags(self) -> bool {
        self.flags & DRIVER_TASK_OWNER_STATE_REQUIRED_FLAGS
            == DRIVER_TASK_OWNER_STATE_REQUIRED_FLAGS
    }
}

/// Register pointer-free owner-state proof for one driver-task hot path.
#[cfg(feature = "kernel")]
pub fn register_driver_task_owner_state_descriptor(
    contract: DriverTaskContract,
    descriptor: DriverTaskOwnerStateDescriptor,
) -> bool {
    if descriptor.hot_path.contract() != contract {
        return false;
    }
    if !descriptor.has_required_runtime_flags() {
        return false;
    }
    let Some(spec) = pi4_driver_task_runtime_image_spec_for_contract(contract) else {
        return false;
    };
    if spec.hot_path != descriptor.hot_path || !spec.acceptance_eligible() {
        return false;
    }
    let role_bit = driver_task_role_bit(contract.kind);
    if role_bit == 0 {
        return false;
    }
    DRIVER_TASK_OWNER_STATE_ROLE_MASK.fetch_or(role_bit, Ordering::AcqRel);
    DRIVER_TASK_OWNER_STATE_HOT_PATH_MASK
        .fetch_or(descriptor.hot_path.owner_state_bit(), Ordering::AcqRel);
    DRIVER_TASK_SHARED_RING_SERVICE_ROLE_MASK.fetch_or(role_bit, Ordering::AcqRel);
    DRIVER_TASK_HOT_PATH_ROLE_MASK.fetch_or(role_bit, Ordering::AcqRel);
    refresh_driver_task_owner_state_proof();
    true
}

/// Register the standard linked-runtime owner-state descriptor after hardware progress.
#[cfg(feature = "kernel")]
pub fn register_driver_task_runtime_owner_state(hot_path: DriverTaskHotPath) -> bool {
    let Some(descriptor) = DriverTaskOwnerStateDescriptor::new(
        hot_path,
        DRIVER_TASK_OWNER_STATE_OFFSET as u32,
        DRIVER_TASK_OWNER_STATE_METADATA_BYTES,
        DRIVER_TASK_RING_FRAME_OFFSET as u32,
        MAX_DRIVER_TASK_FRAME_BYTES as u16,
        DRIVER_TASK_OWNER_STATE_REQUIRED_FLAGS,
    ) else {
        return false;
    };
    register_driver_task_owner_state_descriptor(hot_path.contract(), descriptor)
}

/// Return whether one runtime hot path has registered pointer-free owner state.
#[cfg(feature = "kernel")]
#[must_use]
pub fn driver_task_runtime_owner_state_registered(hot_path: DriverTaskHotPath) -> bool {
    DRIVER_TASK_OWNER_STATE_HOT_PATH_MASK.load(Ordering::Acquire) & hot_path.owner_state_bit() != 0
}

#[cfg(feature = "kernel")]
fn refresh_driver_task_owner_state_proof() {
    let owner_hot_paths = DRIVER_TASK_OWNER_STATE_HOT_PATH_MASK.load(Ordering::Acquire);
    let ready = owner_hot_paths & REQUIRED_PI4_ACCEPTANCE_HOT_PATH_MASK
        == REQUIRED_PI4_ACCEPTANCE_HOT_PATH_MASK
        && DRIVER_TASK_VSPACE_PROOF.load(Ordering::Acquire) != 0
        && DRIVER_TASK_POINTER_FREE_IPC_PROOF.load(Ordering::Acquire) != 0;
    DRIVER_TASK_OWNER_STATE_PROOF.store(ready as usize, Ordering::Release);
}

/// Register the pointer-free default service handler for Pi 4 bus owner roles.
#[cfg(feature = "kernel")]
pub fn register_pi4_bus_ring_service(contract: DriverTaskContract) -> bool {
    let hot_path = if contract == SDIO_HOST_DRIVER_TASK_CONTRACT {
        DriverTaskHotPath::SdioHost
    } else if contract == PCIE_ROOT_DRIVER_TASK_CONTRACT {
        DriverTaskHotPath::PcieRoot
    } else {
        return false;
    };
    register_driver_task_pointer_free_ring_service(
        contract,
        hot_path.as_u32() as usize,
        pi4_bus_ring_service_driver_task,
    )
}

/// Stage a bounded payload into the driver-task ring shared-buffer area.
#[cfg(feature = "kernel")]
pub fn stage_driver_task_ring_frame(
    contract: DriverTaskContract,
    payload: &[u8],
    flags: u16,
) -> Option<DriverFrameDescriptor> {
    if payload.len() > MAX_DRIVER_TASK_FRAME_BYTES {
        return None;
    }
    let task_key = driver_task_contract_key(contract)?;
    let slot = slot_for_task_key(task_key)?;
    let ring_root_ptr = slot.ring_root_ptr.load(Ordering::Acquire);
    if ring_root_ptr == 0 {
        return None;
    }
    let end = DRIVER_TASK_RING_FRAME_OFFSET.checked_add(payload.len())?;
    if end > DRIVER_TASK_RING_PAGE_BYTES {
        return None;
    }
    let dst = (ring_root_ptr + DRIVER_TASK_RING_FRAME_OFFSET) as *mut u8;
    // SAFETY: The destination lies in the HAL-owned ring page after the fixed
    // command/completion records. Bounds above keep the copy page-local, and the
    // root TCB owns writes before it submits the command sequence.
    unsafe {
        core::ptr::copy_nonoverlapping(payload.as_ptr(), dst, payload.len());
    }
    DriverFrameDescriptor::new(
        DRIVER_TASK_RING_FRAME_OFFSET as u32,
        payload.len() as u16,
        flags,
    )
    .ok()
}

/// Stage a pointer-free runtime initialization descriptor into the shared ring.
#[cfg(feature = "kernel")]
pub fn stage_driver_runtime_init_descriptor(
    contract: DriverTaskContract,
    descriptor: &DriverRuntimeInitDescriptor,
) -> Option<DriverFrameDescriptor> {
    if !descriptor.valid() {
        return None;
    }
    // SAFETY: `DriverRuntimeInitDescriptor` is `repr(C)`, primitive-only, and
    // bounded by the shared ABI crate. The resulting byte view is copied
    // immediately into the HAL-owned ring page and does not outlive
    // `descriptor`.
    let bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(descriptor).cast::<u8>(),
            core::mem::size_of::<DriverRuntimeInitDescriptor>(),
        )
    };
    stage_driver_task_ring_frame(contract, bytes, 0)
}

/// Build a runtime init command for a Pi 4 hot path.
#[cfg(feature = "kernel")]
#[must_use]
pub const fn runtime_init_command(
    hot_path: DriverTaskHotPath,
    budget: DriverTaskBudgetGrant,
    frame: DriverFrameDescriptor,
) -> DriverTaskCommandRecord {
    DriverTaskCommandRecord {
        sequence: 0,
        opcode: DriverTaskOpcode::Service.as_u16(),
        flags: DRIVER_TASK_RING_FLAG_INIT_DESCRIPTOR_NON_ACCEPTANCE,
        arg0: hot_path.as_u32(),
        arg1: hot_path.role_bit() as u32,
        aux0: DRIVER_RUNTIME_INIT_AUX,
        aux1: 0,
        budget,
        frame,
    }
}

/// Build a runtime engine-init service command for a Pi 4 hot path.
#[cfg(feature = "kernel")]
#[must_use]
pub const fn runtime_engine_init_command(
    hot_path: DriverTaskHotPath,
    budget: DriverTaskBudgetGrant,
) -> DriverTaskCommandRecord {
    DriverTaskCommandRecord {
        sequence: 0,
        opcode: DriverTaskOpcode::Service.as_u16(),
        flags: 0,
        arg0: hot_path.as_u32(),
        arg1: hot_path.role_bit() as u32,
        aux0: DRIVER_RUNTIME_ENGINE_INIT_AUX,
        aux1: 0,
        budget,
        frame: DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: 0,
        },
    }
}

/// Record a runtime-init descriptor that must be replayed after the shell.
#[cfg(feature = "kernel")]
pub fn record_deferred_runtime_init_descriptor(
    contract: DriverTaskContract,
    descriptor: DriverRuntimeInitDescriptor,
) -> bool {
    let Some(hot_path) = DriverTaskHotPath::from_u32(descriptor.hot_path) else {
        return false;
    };
    if hot_path.contract() != contract
        || descriptor.role_bit != hot_path.role_bit() as u32
        || !descriptor.valid()
    {
        emit_driver_task_resource_init_status(
            contract,
            hot_path,
            "runtime-descriptor-record",
            "invalid-descriptor",
            None,
        );
        return false;
    }
    deferred_runtime_init_slot(hot_path).store(descriptor);
    emit_driver_task_resource_init_status(
        contract,
        hot_path,
        "runtime-descriptor-record",
        "deferred",
        None,
    );
    true
}

/// Replay a shell-deferred runtime-init descriptor before steady service.
#[cfg(feature = "kernel")]
pub fn ensure_deferred_runtime_init_descriptor(
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
) -> bool {
    if !physical_pi_driver_task_only_owner_state_active() {
        return true;
    }
    if hot_path.contract() != contract {
        emit_driver_task_resource_init_status(
            contract,
            hot_path,
            "runtime-descriptor-replay",
            "wrong-contract",
            None,
        );
        return false;
    }
    let slot = deferred_runtime_init_slot(hot_path);
    if slot.initialized.load(Ordering::Acquire) != 0 {
        return true;
    }
    if slot.pending.load(Ordering::Acquire) == 0 {
        return true;
    }
    let descriptor = slot.load();
    if descriptor.hot_path != hot_path.as_u32()
        || descriptor.role_bit != hot_path.role_bit() as u32
        || !descriptor.valid()
    {
        emit_driver_task_resource_init_status(
            contract,
            hot_path,
            "runtime-descriptor-replay",
            "invalid-descriptor",
            None,
        );
        emit_deferred_runtime_init_status(contract, hot_path, "invalid-descriptor");
        return false;
    }
    let Some(frame) = stage_driver_runtime_init_descriptor(contract, &descriptor) else {
        emit_driver_task_resource_init_status(
            contract,
            hot_path,
            "runtime-descriptor-replay",
            "stage-failed",
            None,
        );
        emit_deferred_runtime_init_status(contract, hot_path, "stage-failed");
        return false;
    };
    let command = runtime_init_command(
        hot_path,
        DriverTaskBudgetGrant::from_contract(contract),
        frame,
    );
    let completion = if deferred_runtime_init_replay_must_be_bounded(hot_path) {
        run_driver_task_ring_command_nonblocking(contract, command)
    } else {
        run_driver_task_ring_command(contract, command)
    };
    let complete = completion.is_some_and(|completion| {
        completion.code == DriverTaskCompletionCode::Progress.as_u16()
            && completion.result == hot_path.as_u32()
    });
    let status = if complete {
        "ready"
    } else if completion.is_some() {
        "unexpected-completion"
    } else {
        "no-reply"
    };
    emit_driver_task_resource_init_status(
        contract,
        hot_path,
        "runtime-descriptor-replay",
        status,
        completion,
    );
    if complete {
        slot.initialized.store(1, Ordering::Release);
        slot.pending.store(0, Ordering::Release);
        emit_deferred_runtime_init_status(contract, hot_path, "resumed");
        true
    } else {
        emit_deferred_runtime_init_status(contract, hot_path, "pending");
        false
    }
}

/// Returns whether descriptor replay still uses nonblocking sends after prompt.
///
/// Shell-first deferral is the prompt-safety boundary on physical Pi 4. The
/// first prompt must stay responsive even when a deferred SDIO, PCIe, or NIC
/// runtime never replies, so replay uses bounded sends while temporarily
/// boosting the target runtime TCB.
#[must_use]
pub const fn deferred_runtime_init_replay_must_be_bounded(hot_path: DriverTaskHotPath) -> bool {
    let _ = hot_path;
    true
}

#[cfg(feature = "kernel")]
fn emit_deferred_runtime_init_status(
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
    status: &'static str,
) {
    use core::fmt::Write;
    use heapless::String;

    let mut line = String::<224>::new();
    let action = if status == "resumed" {
        "steady-service-enabled"
    } else {
        "serial-shell"
    };
    let _ = write!(
        line,
        "DRIVER_TASK_RUNTIME_INIT_DEFERRED contract={} hot_path={} status={} action={}",
        contract.name,
        hot_path.as_str(),
        status,
        action,
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
}

/// Borrow a staged shared-ring payload for the current synchronous service turn.
#[cfg(feature = "kernel")]
pub fn driver_task_ring_frame_bytes(
    contract: DriverTaskContract,
    frame: DriverFrameDescriptor,
) -> Option<&'static [u8]> {
    if frame.len as usize > MAX_DRIVER_TASK_FRAME_BYTES {
        return None;
    }
    let task_key = driver_task_contract_key(contract)?;
    let slot = slot_for_task_key(task_key)?;
    let ring_root_ptr = slot.ring_root_ptr.load(Ordering::Acquire);
    if ring_root_ptr == 0 {
        return None;
    }
    let offset = frame.offset as usize;
    let end = offset.checked_add(frame.len as usize)?;
    if offset < DRIVER_TASK_RING_FRAME_OFFSET || end > DRIVER_TASK_RING_PAGE_BYTES {
        return None;
    }
    // SAFETY: The descriptor was bounds-checked against the same HAL-owned ring
    // page. The returned slice is consumed synchronously by the driver service
    // handler before root mutates the frame area for another command.
    Some(unsafe {
        core::slice::from_raw_parts((ring_root_ptr + offset) as *const u8, frame.len as usize)
    })
}

#[cfg(feature = "kernel")]
fn emit_driver_task_ring_call_begin(
    contract: DriverTaskContract,
    endpoint: usize,
    request: usize,
    command: DriverTaskCommandRecord,
) {
    use core::fmt::Write;
    use heapless::String;

    let mut line = String::<320>::new();
    let _ = write!(
        line,
        "DRIVER_TASK_RING_CALL_BEGIN contract={} endpoint=0x{:04x} request={} opcode={} flags=0x{:04x} arg0={} arg1={} aux0=0x{:08x} aux1={} frame_len={}",
        contract.name,
        endpoint,
        request,
        command.opcode,
        command.flags,
        command.arg0,
        command.arg1,
        command.aux0,
        command.aux1,
        command.frame.len,
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_driver_task_ring_call_return(
    contract: DriverTaskContract,
    endpoint: usize,
    request: usize,
    completion: DriverTaskCompletionRecord,
) {
    use core::fmt::Write;
    use heapless::String;

    let mut line = String::<256>::new();
    let _ = write!(
        line,
        "DRIVER_TASK_RING_CALL_RETURN contract={} endpoint=0x{:04x} request={} sequence={} code={} detail={} result={}",
        contract.name,
        endpoint,
        request,
        completion.sequence,
        completion.code,
        completion.detail,
        completion.result,
    );
    if completion.code == DriverTaskCompletionCode::Fault.as_u16() {
        crate::bootstrap::log::force_uart_line_raw(line.as_str());
    } else {
        crate::bootstrap::log::force_uart_line(line.as_str());
    }
}

#[cfg(feature = "kernel")]
fn emit_driver_task_ring_call_timeout(
    contract: DriverTaskContract,
    endpoint: usize,
    request: usize,
    command: DriverTaskCommandRecord,
    mode: DriverTaskRingCommandMode,
    attempts: usize,
) {
    use core::fmt::Write;
    use heapless::String;

    let mut line = String::<320>::new();
    let _ = write!(
        line,
        "DRIVER_TASK_RING_CALL_TIMEOUT contract={} endpoint=0x{:04x} request={} mode={} attempts={} opcode={} arg0={} aux0=0x{:08x} frame_len={}",
        contract.name,
        endpoint,
        request,
        mode.as_str(),
        attempts,
        command.opcode,
        command.arg0,
        command.aux0,
        command.frame.len,
    );
    crate::bootstrap::log::force_uart_line_raw(line.as_str());
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DriverTaskRingCommandMode {
    Steady,
    Bootstrap,
    BootstrapCall,
    NonBlocking,
    PromptSlice,
}

#[cfg(feature = "kernel")]
impl DriverTaskRingCommandMode {
    const fn as_str(self) -> &'static str {
        match self {
            DriverTaskRingCommandMode::Steady => "steady",
            DriverTaskRingCommandMode::Bootstrap => "bootstrap",
            DriverTaskRingCommandMode::BootstrapCall => "bootstrap-call",
            DriverTaskRingCommandMode::NonBlocking => "nonblocking",
            DriverTaskRingCommandMode::PromptSlice => "prompt-slice",
        }
    }

    const fn records_latency(self) -> bool {
        matches!(
            self,
            DriverTaskRingCommandMode::Steady
                | DriverTaskRingCommandMode::NonBlocking
                | DriverTaskRingCommandMode::PromptSlice
        )
    }
}

#[cfg(feature = "kernel")]
fn driver_task_ring_call_trace_enabled(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
    _mode: DriverTaskRingCommandMode,
) -> bool {
    if command.aux0 != 0
        || command.flags & DRIVER_TASK_RING_FLAG_INIT_DESCRIPTOR_NON_ACCEPTANCE != 0
        || command.frame.flags & DRIVER_TASK_RING_FLAG_INIT_DESCRIPTOR_NON_ACCEPTANCE != 0
    {
        return true;
    }
    if matches!(
        contract.kind,
        DriverTaskKind::Serial | DriverTaskKind::LocalSeatUsb | DriverTaskKind::HdmiText
    ) {
        return false;
    }
    true
}

#[cfg(feature = "kernel")]
fn emit_driver_task_ring_resource_submit_status(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
    stage: &'static str,
    status: &'static str,
) {
    if !driver_task_ring_call_trace_enabled(contract, command, DriverTaskRingCommandMode::Steady) {
        return;
    }
    if let Some(hot_path) = DriverTaskHotPath::from_u32(command.arg0) {
        emit_driver_task_resource_init_status(contract, hot_path, stage, status, None);
    }
}

#[cfg(feature = "kernel")]
const fn driver_task_ring_mode_uses_bounded_send(mode: DriverTaskRingCommandMode) -> bool {
    matches!(
        mode,
        DriverTaskRingCommandMode::NonBlocking
            | DriverTaskRingCommandMode::Bootstrap
            | DriverTaskRingCommandMode::PromptSlice
    )
}

#[cfg(feature = "kernel")]
fn driver_task_ring_attempt_limit(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
    mode: DriverTaskRingCommandMode,
) -> usize {
    if !driver_task_ring_mode_uses_bounded_send(mode) {
        return DRIVER_TASK_BOOTSTRAP_RING_ATTEMPTS;
    }
    if mode == DriverTaskRingCommandMode::PromptSlice
        && matches!(contract.kind, DriverTaskKind::LocalSeatUsb)
        && command.aux0 == DRIVER_RUNTIME_USB_ENUMERATE_AUX
    {
        return DRIVER_TASK_USB_PROMPT_ENUM_RING_ATTEMPTS;
    }
    if mode == DriverTaskRingCommandMode::PromptSlice
        && matches!(contract.kind, DriverTaskKind::LocalSeatUsb)
        && command.aux0 == DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX
    {
        return DRIVER_TASK_USB_PROMPT_INIT_RING_ATTEMPTS;
    }
    if mode == DriverTaskRingCommandMode::PromptSlice {
        return DRIVER_TASK_PROMPT_RING_ATTEMPTS;
    }
    if mode == DriverTaskRingCommandMode::NonBlocking
        && command.aux0 == 0
        && matches!(contract.kind, DriverTaskKind::LocalSeatUsb)
    {
        return DRIVER_TASK_USB_PROMPT_POLL_RING_ATTEMPTS;
    }
    if mode == DriverTaskRingCommandMode::NonBlocking
        && command.aux0 == 0
        && matches!(
            contract.kind,
            DriverTaskKind::Serial | DriverTaskKind::HdmiText
        )
    {
        return DRIVER_TASK_PROMPT_RING_ATTEMPTS;
    }
    if mode == DriverTaskRingCommandMode::NonBlocking
        && matches!(contract.kind, DriverTaskKind::LocalSeatUsb)
        && matches!(
            command.aux0,
            DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX | DRIVER_RUNTIME_USB_ENUMERATE_AUX
        )
    {
        return DRIVER_TASK_LONG_INIT_RING_ATTEMPTS;
    }
    if matches!(contract.kind, DriverTaskKind::WifiNic) && command.aux0 != 0 {
        DRIVER_TASK_LONG_INIT_RING_ATTEMPTS
    } else {
        DRIVER_TASK_BOOTSTRAP_RING_ATTEMPTS
    }
}

#[cfg(feature = "kernel")]
const fn driver_task_ring_flags_for_mode(mode: DriverTaskRingCommandMode, flags: u16) -> u16 {
    if driver_task_ring_mode_uses_bounded_send(mode) {
        flags | DRIVER_TASK_RING_FLAG_ONE_WAY
    } else {
        flags
    }
}

/// Execute a fixed-layout command over the pointer-free shared-ring ABI.
///
/// This transport is intentionally narrower than the transitional callback
/// service path. It is used by the isolated QEMU smoke task to prove the ABI
/// mechanics without crediting a hardware hot path until the driver state has
/// moved behind that ring.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_command(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    run_driver_task_ring_command_with_mode(contract, command, DriverTaskRingCommandMode::Steady)
}

/// Execute the first linked-runtime command without letting root block forever.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_command_bootstrap(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    run_driver_task_ring_command_with_mode(contract, command, DriverTaskRingCommandMode::Bootstrap)
}

/// Execute a bootstrap command through a reply rendezvous without latency proof.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_command_bootstrap_call(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    run_driver_task_ring_command_with_mode(
        contract,
        command,
        DriverTaskRingCommandMode::BootstrapCall,
    )
}

/// Execute a linked-runtime command with bounded nonblocking sends.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_command_nonblocking(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    run_driver_task_ring_command_with_mode(
        contract,
        command,
        DriverTaskRingCommandMode::NonBlocking,
    )
}

/// Execute one prompt-side slice of a linked-runtime command.
///
/// Unlike [`run_driver_task_ring_command_nonblocking`], this keeps the ring
/// active when the driver has accepted a long hardware turn but has not yet
/// published a completion. Later prompt slices poll the same request instead
/// of overwriting the command frame.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_command_prompt_slice(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    run_driver_task_ring_command_with_mode(
        contract,
        command,
        DriverTaskRingCommandMode::PromptSlice,
    )
}

#[cfg(feature = "kernel")]
fn run_driver_task_ring_command_with_mode(
    contract: DriverTaskContract,
    mut command: DriverTaskCommandRecord,
    mode: DriverTaskRingCommandMode,
) -> Option<DriverTaskCompletionRecord> {
    let Some(task_key) = driver_task_contract_key(contract) else {
        emit_driver_task_ring_resource_submit_status(
            contract,
            command,
            "runtime-ring-submit",
            "invalid-contract",
        );
        return None;
    };
    let Some(slot) = slot_for_task_key(task_key) else {
        emit_driver_task_ring_resource_submit_status(
            contract,
            command,
            "runtime-ring-submit",
            "slot-missing",
        );
        return None;
    };
    let endpoint = slot.endpoint.load(Ordering::Acquire);
    let ring_root_ptr = slot.ring_root_ptr.load(Ordering::Acquire);
    if endpoint == 0 {
        emit_driver_task_ring_resource_submit_status(
            contract,
            command,
            "runtime-ring-submit",
            "no-endpoint",
        );
        return None;
    }
    if ring_root_ptr == 0 {
        emit_driver_task_ring_resource_submit_status(
            contract,
            command,
            "runtime-ring-submit",
            "ring-missing",
        );
        return None;
    }

    command.flags = driver_task_ring_flags_for_mode(mode, command.flags);
    let command_ptr = ring_root_ptr as *mut DriverTaskCommandRecord;
    let completion_ptr =
        (ring_root_ptr + DRIVER_TASK_RING_COMPLETION_OFFSET) as *mut DriverTaskCompletionRecord;

    let prompt_slice_resume =
        mode == DriverTaskRingCommandMode::PromptSlice && slot.active.load(Ordering::Acquire) != 0;
    if slot.active.swap(1, Ordering::AcqRel) != 0 && !prompt_slice_resume {
        emit_driver_task_ring_resource_submit_status(
            contract,
            command,
            "runtime-ring-submit",
            "busy",
        );
        return None;
    }

    let request = if prompt_slice_resume {
        slot.request_seq.load(Ordering::Acquire)
    } else {
        let request = slot
            .request_seq
            .load(Ordering::Relaxed)
            .wrapping_add(1)
            .max(1);
        slot.request_seq.store(request, Ordering::Release);
        command.sequence = request as u32;
        let completion_reset =
            DriverTaskCompletionRecord::fault(0, DriverTaskFaultCode::RejectedCommand);
        // SAFETY: `ring_root_ptr` is the root mapping of one HAL-owned frame that
        // was also mapped into the driver VSpace at `DRIVER_TASK_RING_VADDR`. The
        // fixed records are page-local, primitive-only, and naturally aligned.
        unsafe {
            core::ptr::write_volatile(completion_ptr, completion_reset);
            core::ptr::write_volatile(command_ptr, command);
        }
        request
    };
    if request == 0 {
        slot.active.store(0, Ordering::Release);
        return None;
    }

    // SAFETY: MR0 carries only the current ring request sequence. Rewriting it
    // before each send is harmless and keeps resumed prompt slices aligned with
    // the already staged command.
    unsafe {
        sel4_sys::seL4_SetMR(0, request as sel4_sys::seL4_Word);
    }

    // SAFETY: The completion pointer addresses the validated shared ring page.
    let mut completion = unsafe { core::ptr::read_volatile(completion_ptr) };
    let mut start_ticks = None;
    let trace_call = driver_task_ring_call_trace_enabled(contract, command, mode);
    let _priority_restore = if driver_task_ring_mode_uses_bounded_send(mode) {
        boost_driver_task_priorities_for_bounded_turn(contract, command)
    } else {
        (None, None)
    };

    if driver_task_ring_mode_uses_bounded_send(mode) {
        if trace_call && !prompt_slice_resume {
            emit_driver_task_ring_call_begin(contract, endpoint, request, command);
        }
        if mode.records_latency() && !prompt_slice_resume {
            start_ticks = driver_task_counter_ticks();
        }
        let attempts = driver_task_ring_attempt_limit(contract, command, mode);
        if completion.sequence != request as u32 {
            let info = sel4_sys::seL4_MessageInfo::new(0, 0, 0, 1);
            for _ in 0..attempts {
                crate::sel4::send_nb_unchecked(endpoint as sel4_sys::seL4_CPtr, info);
                crate::sel4::yield_now();
                // SAFETY: The completion pointer addresses the same validated ring
                // page. A matching sequence means the isolated runtime observed the
                // nonblocking send and published the primitive completion record.
                completion = unsafe { core::ptr::read_volatile(completion_ptr) };
                if completion.sequence == request as u32 {
                    if trace_call {
                        emit_driver_task_ring_call_return(contract, endpoint, request, completion);
                    }
                    break;
                }
            }
        } else if trace_call {
            emit_driver_task_ring_call_return(contract, endpoint, request, completion);
        }
        if completion.sequence != request as u32 && trace_call {
            emit_driver_task_ring_call_timeout(
                contract, endpoint, request, command, mode, attempts,
            );
        }
    } else if physical_pi_driver_task_only_owner_state_active() && cfg!(not(sel4_config_kernel_mcs))
    {
        // A blocking call is required on physical Pi builds so lower-priority
        // driver TCBs receive CPU without relying on cross-priority yield
        // behavior; the linked runtime replies after publishing the primitive
        // completion record.
        if trace_call {
            emit_driver_task_ring_call_begin(contract, endpoint, request, command);
        }
        if mode.records_latency() {
            start_ticks = driver_task_counter_ticks();
        }
        // SAFETY: The fixed ABI uses MR0 as the request sequence. Re-writing it
        // immediately before the blocking call keeps diagnostic UART emission
        // out of the message-register contract.
        unsafe {
            sel4_sys::seL4_SetMR(0, request as sel4_sys::seL4_Word);
        }
        let _ = crate::sel4::call_unchecked(
            endpoint as sel4_sys::seL4_CPtr,
            sel4_sys::seL4_MessageInfo::new(0, 0, 0, 1),
        );
        // SAFETY: The completion pointer addresses the same validated ring
        // page; the reply boundary guarantees the isolated runtime had a chance
        // to publish the shared-frame result.
        completion = unsafe { core::ptr::read_volatile(completion_ptr) };
        if trace_call {
            emit_driver_task_ring_call_return(contract, endpoint, request, completion);
        }
    } else {
        start_ticks = driver_task_counter_ticks();
        let info = sel4_sys::seL4_MessageInfo::new(0, 0, 0, 1);
        for _ in 0..256 {
            crate::sel4::send_nb_unchecked(endpoint as sel4_sys::seL4_CPtr, info);
            crate::sel4::yield_now();
            // SAFETY: The completion pointer addresses the same validated ring
            // page; a matching sequence means the isolated trampoline observed the
            // command through the shared frame.
            completion = unsafe { core::ptr::read_volatile(completion_ptr) };
            if completion.sequence == request as u32 {
                break;
            }
        }
    }

    if completion.sequence == request as u32 || mode != DriverTaskRingCommandMode::PromptSlice {
        slot.active.store(0, Ordering::Release);
    }
    if completion.sequence == request as u32 {
        if let (Some(start_ticks), Some(end_ticks), Some(counter_frequency)) = (
            start_ticks,
            driver_task_counter_ticks(),
            driver_task_counter_frequency(),
        ) {
            let elapsed_us = driver_task_elapsed_us(start_ticks, end_ticks, counter_frequency);
            record_observed_service_us(contract, elapsed_us);
        }
        Some(completion)
    } else {
        None
    }
}

/// Execute one registered driver service turn through the shared-ring ABI.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_service(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    run_driver_task_ring_service_with_mode(contract, command, DriverTaskRingCommandMode::Steady)
}

/// Execute a pre-root service turn without sampling hardware timing registers.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_service_bootstrap(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    run_driver_task_ring_service_with_mode(contract, command, DriverTaskRingCommandMode::Bootstrap)
}

/// Execute a bootstrap service turn through a reply rendezvous without latency proof.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_service_bootstrap_call(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    run_driver_task_ring_service_with_mode(
        contract,
        command,
        DriverTaskRingCommandMode::BootstrapCall,
    )
}

/// Execute one registered driver service turn through bounded nonblocking IPC.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_service_nonblocking(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    run_driver_task_ring_service_with_mode(
        contract,
        command,
        DriverTaskRingCommandMode::NonBlocking,
    )
}

/// Execute one prompt-side service slice without monopolising the root shell.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_service_prompt_slice(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    run_driver_task_ring_service_with_mode(
        contract,
        command,
        DriverTaskRingCommandMode::PromptSlice,
    )
}

/// Emit a non-acceptance resource-initialization breadcrumb for one hot path.
#[cfg(feature = "kernel")]
pub fn emit_driver_task_resource_init_status(
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
    stage: &'static str,
    status: &'static str,
    completion: Option<DriverTaskCompletionRecord>,
) {
    use core::fmt::Write;
    use heapless::String;

    let mut line = String::<320>::new();
    if let Some(completion) = completion {
        let _ = write!(
            line,
            "DRIVER_TASK_RESOURCE_INIT contract={} hot_path={} stage={} status={} acceptance=no code={} detail={} result={} frame_len={}",
            contract.name,
            hot_path.as_str(),
            stage,
            status,
            completion.code,
            completion.detail,
            completion.result,
            completion.frame.len,
        );
    } else {
        let _ = write!(
            line,
            "DRIVER_TASK_RESOURCE_INIT contract={} hot_path={} stage={} status={} acceptance=no code=none detail=none result=none frame_len=0",
            contract.name,
            hot_path.as_str(),
            stage,
            status,
        );
    }
    if driver_task_resource_status_requires_uart(status) {
        crate::bootstrap::log::force_uart_line_raw(line.as_str());
    } else {
        crate::bootstrap::log::force_uart_line(line.as_str());
    }
    emit_driver_task_resource_init_hdmi_progress(contract, hot_path, stage, status, completion);
}

#[cfg(feature = "kernel")]
fn driver_task_resource_status_requires_uart(status: &str) -> bool {
    status == "failed"
        || status == "no-reply"
        || status == "invalid-contract"
        || status == "slot-missing"
        || status == "no-endpoint"
        || status == "ring-missing"
        || status == "busy"
        || status.starts_with("blocked")
}

#[cfg(feature = "kernel")]
fn emit_driver_task_resource_init_hdmi_progress(
    contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
    stage: &'static str,
    status: &'static str,
    completion: Option<DriverTaskCompletionRecord>,
) {
    if let Some(line) =
        driver_task_resource_hdmi_progress_line(contract, hot_path, stage, status, completion)
    {
        crate::local_seat::mirror_driver_start_progress_line(line.as_str());
    }
}

fn driver_task_resource_hdmi_progress_line(
    _contract: DriverTaskContract,
    hot_path: DriverTaskHotPath,
    stage: &'static str,
    status: &'static str,
    completion: Option<DriverTaskCompletionRecord>,
) -> Option<heapless::String<192>> {
    use core::fmt::Write;

    if !driver_task_resource_status_mirrors_to_hdmi(hot_path, stage, status) {
        return None;
    }
    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "[drivers] {} {} {}",
        driver_task_hdmi_progress_label(hot_path),
        stage,
        status,
    );
    if let Some(completion) = completion {
        let _ = write!(
            line,
            " detail=0x{:04x} result={}",
            completion.detail, completion.result
        );
    }
    Some(line)
}

fn driver_task_resource_status_mirrors_to_hdmi(
    hot_path: DriverTaskHotPath,
    stage: &str,
    status: &str,
) -> bool {
    if hot_path == DriverTaskHotPath::HdmiText {
        return false;
    }
    if stage == "cyw43-firmware-chunk" && status == "ready" {
        return false;
    }
    status == "begin"
        || status == "ready"
        || status == "deferred"
        || status == "pending"
        || status == "resumed"
        || status == "fault"
        || status == "failed"
        || status == "no-reply"
        || status == "unexpected-completion"
        || status == "descriptor-rejected"
        || status.ends_with("failed")
        || status.starts_with("blocked")
}

const fn driver_task_hdmi_progress_label(hot_path: DriverTaskHotPath) -> &'static str {
    match hot_path {
        DriverTaskHotPath::SerialConsole => "Serial",
        DriverTaskHotPath::UsbKeyboard => "USB",
        DriverTaskHotPath::HdmiText => "HDMI",
        DriverTaskHotPath::GenetNic => "GENET",
        DriverTaskHotPath::Cyw43Wifi => "WiFi",
        DriverTaskHotPath::SdioHost => "SDIO",
        DriverTaskHotPath::PcieRoot => "PCIe",
    }
}

/// Host-test/no-kernel variant for call sites that share control flow.
#[cfg(not(feature = "kernel"))]
pub fn emit_driver_task_resource_init_status(
    _contract: DriverTaskContract,
    _hot_path: DriverTaskHotPath,
    _stage: &'static str,
    _status: &'static str,
    _completion: Option<DriverTaskCompletionRecord>,
) {
}

#[cfg(feature = "kernel")]
fn run_driver_task_ring_service_with_mode(
    contract: DriverTaskContract,
    mut command: DriverTaskCommandRecord,
    mode: DriverTaskRingCommandMode,
) -> Option<DriverTaskCompletionRecord> {
    let service_kind = driver_task_ring_service_kind(contract);
    if service_kind == DriverTaskRingServiceKind::RootContextDiagnostic {
        command.flags |= DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE;
        command.frame.flags |= DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE;
    }
    let owner_state_credit_eligible =
        driver_task_ring_service_owner_state_credit_eligible(service_kind, command);
    let completion = run_driver_task_ring_command_with_mode(contract, command, mode)?;
    if completion.code != DriverTaskCompletionCode::Fault.as_u16() {
        record_driver_task_ring_service(
            contract,
            owner_state_credit_eligible && driver_task_completion_has_hardware_progress(completion),
        );
    }
    Some(completion)
}

#[cfg(feature = "kernel")]
fn driver_task_completion_has_hardware_progress(completion: DriverTaskCompletionRecord) -> bool {
    if completion.code == DriverTaskCompletionCode::Progress.as_u16() {
        return completion.result != 0;
    }
    if completion.code != DriverTaskCompletionCode::FrameReady.as_u16() {
        return false;
    }
    if completion.frame.len == 0
        || completion.result != completion.frame.len as u32
        || completion.frame.root_context_non_acceptance()
    {
        return false;
    }
    let offset = completion.frame.offset as usize;
    let len = completion.frame.len as usize;
    let Some(end) = offset.checked_add(len) else {
        return false;
    };
    offset >= DRIVER_TASK_RING_FRAME_OFFSET
        && end <= DRIVER_TASK_RING_PAGE_BYTES
        && len <= MAX_DRIVER_TASK_FRAME_BYTES
}

#[cfg(feature = "kernel")]
fn service_pending_driver_task_ring_command(task_key: usize) -> Option<usize> {
    let slot = slot_for_task_key(task_key)?;
    let ring_root_ptr = slot.ring_root_ptr.load(Ordering::Acquire);
    if ring_root_ptr == 0 {
        return None;
    }
    let handler_word = slot.ring_handler.load(Ordering::Acquire);
    if handler_word == 0 {
        return None;
    }
    let context = slot.ring_context.load(Ordering::Acquire);
    let command_ptr = ring_root_ptr as *const DriverTaskCommandRecord;
    let completion_ptr =
        (ring_root_ptr + DRIVER_TASK_RING_COMPLETION_OFFSET) as *mut DriverTaskCompletionRecord;
    // SAFETY: The ring page is HAL-owned and page-local. Root writes the command
    // before sending IPC to this TCB; volatile access preserves that boundary.
    let command = unsafe { core::ptr::read_volatile(command_ptr) };
    if command.sequence == 0 {
        return None;
    }
    // SAFETY: Same page-local completion record as above.
    let current = unsafe { core::ptr::read_volatile(completion_ptr) };
    if current.sequence == command.sequence {
        return Some(current.result as usize);
    }

    // SAFETY: Ring-service registration stores only function pointers with the
    // exact `DriverTaskRingServiceHandler` ABI. The integer round trip keeps the
    // slot atomically publishable to the service TCB.
    let handler: DriverTaskRingServiceHandler =
        unsafe { core::mem::transmute::<usize, DriverTaskRingServiceHandler>(handler_word) };
    // SAFETY: The registered owner controls the context lifetime. Root submits a
    // single synchronous command at a time (`active` gate) and does not mutate the
    // driver-owned state until the completion sequence is published.
    let mut completion = unsafe { handler(context, command) };
    if completion.sequence != command.sequence {
        completion.sequence = command.sequence;
    }
    // SAFETY: Completion record is page-local and naturally aligned.
    unsafe {
        core::ptr::write_volatile(completion_ptr, completion);
    }
    Some(completion.result as usize)
}

#[cfg(all(
    feature = "kernel",
    any(
        not(target_arch = "aarch64"),
        not(target_os = "none"),
        feature = "net-backend-virtio"
    )
))]
fn service_pending_driver_task_command(task_key: usize) -> usize {
    if let Some(result) = service_pending_driver_task_ring_command(task_key) {
        return result;
    }
    let Some(slot) = slot_for_task_key(task_key) else {
        return usize::MAX;
    };
    let request = slot.request_seq.load(Ordering::Acquire);
    if request == 0 || slot.done_seq.load(Ordering::Acquire) == request {
        return usize::MAX;
    }
    let handler_word = slot.handler.load(Ordering::Acquire);
    let context = slot.context.load(Ordering::Acquire);
    let result = if handler_word == 0 {
        usize::MAX
    } else {
        // SAFETY: `run_driver_task_service` stores only function pointers with
        // the exact `DriverTaskServiceHandler` ABI in `handler`. The integer
        // round trip is used because the slot is shared across TCBs through
        // atomics; no data pointer is interpreted as code.
        let handler: DriverTaskServiceHandler =
            unsafe { core::mem::transmute::<usize, DriverTaskServiceHandler>(handler_word) };
        // SAFETY: The caller owns the context object, waits synchronously until
        // `done_seq` reaches `request`, and does not access the pointed-to
        // driver state while this callback executes on the driver TCB.
        unsafe { handler(context) }
    };
    slot.result.store(result, Ordering::Release);
    slot.done_seq.store(request, Ordering::Release);
    result
}

#[cfg(all(
    feature = "kernel",
    target_arch = "aarch64",
    target_os = "none",
    not(feature = "net-backend-virtio")
))]
fn service_pending_driver_task_command(task_key: usize) -> usize {
    service_pending_driver_task_ring_command(task_key).unwrap_or(usize::MAX)
}

/// Execute a bounded compatibility callback on the contract's live driver TCB.
///
/// Returns `None` unless the current runtime profile explicitly admits
/// QEMU/host compatibility dispatch.
#[cfg(feature = "kernel")]
pub unsafe fn try_driver_task_compat_service(
    contract: DriverTaskContract,
    context: usize,
    handler: DriverTaskServiceHandler,
) -> Option<usize> {
    if !steady_state_callback_dispatch_allowed(contract) {
        return None;
    }
    // SAFETY: The profile gate above admits only QEMU/host compatibility turns.
    // The caller still owns the synchronous context lifetime required by the
    // compatibility ABI.
    unsafe { run_driver_task_service(contract, context, handler) }
}

/// Execute a bounded driver service callback on the contract's live driver TCB.
///
/// Returns `None` when the task is not available or the command does not finish
/// within the bounded wait. This compatibility ABI is compiled only for QEMU
/// and host-test profiles; physical Pi 4 hardware builds use the no-op variant.
#[cfg(all(
    feature = "kernel",
    any(
        not(target_arch = "aarch64"),
        not(target_os = "none"),
        feature = "net-backend-virtio"
    )
))]
unsafe fn run_driver_task_service(
    contract: DriverTaskContract,
    context: usize,
    handler: DriverTaskServiceHandler,
) -> Option<usize> {
    let task_key = driver_task_contract_key(contract)?;
    driver_task_task_key_role_bit(task_key)?;
    let slot = slot_for_task_key(task_key)?;
    if DRIVER_TASK_STARTED_TASK_MASK.load(Ordering::Acquire) & (1usize << task_key) == 0 {
        return None;
    }
    let endpoint = slot.endpoint.load(Ordering::Acquire);
    if endpoint == 0 {
        return None;
    }
    if slot.active.swap(1, Ordering::AcqRel) != 0 {
        return None;
    }
    let request = slot
        .request_seq
        .load(Ordering::Relaxed)
        .wrapping_add(1)
        .max(1);
    slot.context.store(context, Ordering::Release);
    slot.handler
        .store(handler as *const () as usize, Ordering::Release);
    slot.result.store(0, Ordering::Release);
    slot.request_seq.store(request, Ordering::Release);

    // SAFETY: `endpoint` is the root-held command endpoint cap published by
    // `KernelHal::create_driver_task`; the call carries no caps and all service
    // payload is in the shared command slot above. Blocking the root here is
    // deliberate: it hands CPU time to lower-priority driver TCBs instead of
    // relying on `Yield`, which is not a cross-priority rendezvous.
    unsafe {
        sel4_sys::seL4_SetMR(0, request as sel4_sys::seL4_Word);
        let _ = crate::sel4::call_unchecked(
            endpoint as sel4_sys::seL4_CPtr,
            sel4_sys::seL4_MessageInfo::new(0, 0, 0, 1),
        );
    }

    let completed = (slot.done_seq.load(Ordering::Acquire) == request)
        .then(|| slot.result.load(Ordering::Acquire));
    slot.active.store(0, Ordering::Release);
    if completed.is_some() {
        record_driver_task_callback_compatibility(contract);
    }
    completed
}

/// Physical Pi 4 fail-closed compatibility boundary.
///
/// # Safety
///
/// This variant never dereferences `context` and never invokes `handler`.
#[cfg(all(
    feature = "kernel",
    target_arch = "aarch64",
    target_os = "none",
    not(feature = "net-backend-virtio")
))]
unsafe fn run_driver_task_service(
    _contract: DriverTaskContract,
    _context: usize,
    _handler: DriverTaskServiceHandler,
) -> Option<usize> {
    None
}

/// Scheduling class used when seL4 assigns budgets and priorities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskClass {
    /// Must preempt all other hardware work to preserve physical input.
    RealtimeInput,
    /// Console output path with bounded, cooperative TX.
    ConsoleOutput,
    /// Network control traffic such as DHCP, EAPOL, ARP, and TCP ACK progress.
    NetworkControl,
    /// Bulk network data path work.
    NetworkData,
    /// Display refresh work that may lag behind input and control.
    DisplayRefresh,
    /// Low-priority diagnostics and background probes.
    Background,
}

impl DriverTaskClass {
    /// Stable diagnostic label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RealtimeInput => "realtime-input",
            Self::ConsoleOutput => "console-output",
            Self::NetworkControl => "network-control",
            Self::NetworkData => "network-data",
            Self::DisplayRefresh => "display-refresh",
            Self::Background => "background",
        }
    }

    /// seL4-style priority value, where larger numbers run first.
    #[must_use]
    pub const fn sel4_priority(self) -> u8 {
        match self {
            Self::RealtimeInput => 240,
            Self::ConsoleOutput => 220,
            Self::NetworkControl => 200,
            Self::NetworkData => 160,
            Self::DisplayRefresh => 120,
            Self::Background => 80,
        }
    }

    /// Cooperative root-task service order, where smaller numbers run first.
    #[must_use]
    pub const fn service_order(self) -> u8 {
        match self {
            Self::RealtimeInput => 0,
            Self::ConsoleOutput => 1,
            Self::NetworkControl => 2,
            Self::NetworkData => 3,
            Self::DisplayRefresh => 4,
            Self::Background => 5,
        }
    }
}

/// Authority exposed to a driver task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskAuthority {
    /// Device service only; no parser, namespace, or policy authority.
    DeviceOnly,
    /// Console byte transport without command authority.
    ConsoleTransport,
    /// Network frame transport without listener/protocol authority.
    NetworkFrameTransport,
    /// Display sink without console parser authority.
    DisplaySink,
}

/// Current isolation state for a hardware driver service path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskIsolation {
    /// Current in-root compatibility path while the dedicated seL4 task is staged.
    RootTaskCompatibility,
    /// Dedicated seL4 task with explicit caps, IPC, and scheduling context.
    DedicatedSeL4Task,
}

impl DriverTaskIsolation {
    /// Stable diagnostic label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RootTaskCompatibility => "root-task-compatibility",
            Self::DedicatedSeL4Task => "dedicated-sel4-task",
        }
    }
}

/// Per-service budget enforced at the HAL boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTaskBudget {
    /// Maximum HAL operations allowed in one service turn.
    pub max_ops_per_turn: u16,
    /// Maximum bytes moved in one service turn.
    pub max_bytes_per_turn: u32,
    /// Maximum packets, frames, reports, or display rows in one service turn.
    pub max_frames_per_turn: u16,
    /// Maximum bounded spin count allowed during bootstrap-only operations.
    pub max_blocking_spins: u32,
    /// Whether a blocking wait is permitted at all.
    pub allow_blocking_waits: bool,
    /// Whether the operation is required to expose preemption points.
    pub preemptible: bool,
}

impl DriverTaskBudget {
    /// Constructs a budget for a preemptible service path with no blocking waits.
    #[must_use]
    pub const fn preemptible(
        max_ops_per_turn: u16,
        max_bytes_per_turn: u32,
        max_frames_per_turn: u16,
    ) -> Self {
        Self {
            max_ops_per_turn,
            max_bytes_per_turn,
            max_frames_per_turn,
            max_blocking_spins: 0,
            allow_blocking_waits: false,
            preemptible: true,
        }
    }
}

/// Static hardware driver scheduling contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTaskContract {
    /// Stable driver label surfaced in diagnostics.
    pub name: &'static str,
    /// Hardware class covered by this contract.
    pub kind: DriverTaskKind,
    /// Scheduler class used for priority and budget assignment.
    pub class: DriverTaskClass,
    /// Authority exposed to the task.
    pub authority: DriverTaskAuthority,
    /// Current isolation state for this driver service path.
    pub isolation: DriverTaskIsolation,
    /// Per-turn budget.
    pub budget: DriverTaskBudget,
    /// Maximum inbound IPC/event queue depth.
    pub queue_depth: u16,
}

impl DriverTaskContract {
    /// Validates contract invariants before the driver is serviced.
    pub fn validate(self) -> Result<(), DriverTaskContractError> {
        if self.name.is_empty() {
            return Err(DriverTaskContractError::MissingName);
        }
        if self.queue_depth == 0 {
            return Err(DriverTaskContractError::ZeroQueueDepth);
        }
        if self.queue_depth > MAX_DRIVER_TASK_QUEUE_DEPTH {
            return Err(DriverTaskContractError::QueueDepthTooLarge);
        }
        if self.budget.max_ops_per_turn == 0 {
            return Err(DriverTaskContractError::ZeroOperationBudget);
        }
        if self.budget.max_bytes_per_turn == 0 {
            return Err(DriverTaskContractError::ZeroByteBudget);
        }
        if self.budget.max_frames_per_turn == 0 {
            return Err(DriverTaskContractError::ZeroFrameBudget);
        }
        if !self.budget.preemptible {
            return Err(DriverTaskContractError::NotPreemptible);
        }
        if self.budget.allow_blocking_waits && self.budget.max_blocking_spins == 0 {
            return Err(DriverTaskContractError::UnboundedBlockingWait);
        }
        if self.budget.allow_blocking_waits
            && matches!(
                self.class,
                DriverTaskClass::RealtimeInput | DriverTaskClass::NetworkData
            )
        {
            return Err(DriverTaskContractError::BlockingWaitNotAdmittedForClass);
        }
        if !self.authority_matches_kind() {
            return Err(DriverTaskContractError::InvalidAuthority);
        }
        if !self.class_matches_kind() {
            return Err(DriverTaskContractError::InvalidClass);
        }
        if matches!(self.isolation, DriverTaskIsolation::DedicatedSeL4Task)
            && !DEDICATED_DRIVER_TASK_SUBSTRATE_READY
        {
            return Err(DriverTaskContractError::DedicatedSubstrateNotReady);
        }
        Ok(())
    }

    /// Returns true when this contract is allowed to run before network data.
    #[must_use]
    pub const fn preempts_network_data(self) -> bool {
        matches!(
            self.class,
            DriverTaskClass::RealtimeInput
                | DriverTaskClass::ConsoleOutput
                | DriverTaskClass::NetworkControl
        )
    }

    /// seL4-style priority value for this contract's scheduling class.
    #[must_use]
    pub const fn sel4_priority(self) -> u8 {
        self.class.sel4_priority()
    }

    /// seL4 priority used before the child runtime has proved it can receive.
    #[must_use]
    pub const fn bootstrap_priority(self, profile: DriverTaskRuntimeProfile) -> u8 {
        match profile {
            DriverTaskRuntimeProfile::Pi4Hardware => PI4_BOUNDED_BOOTSTRAP_PRIORITY,
            DriverTaskRuntimeProfile::QemuCompatibility | DriverTaskRuntimeProfile::HostTest => {
                self.sel4_priority()
            }
        }
    }

    /// Cooperative root-task service order for this contract's class.
    #[must_use]
    pub const fn service_order(self) -> u8 {
        self.class.service_order()
    }

    /// Requested isolation under the default hardware-driver policy.
    #[must_use]
    pub const fn requested_isolation(self) -> DriverTaskIsolation {
        if DEDICATED_DRIVER_TASKS_DEFAULT_ENABLED {
            DriverTaskIsolation::DedicatedSeL4Task
        } else {
            self.isolation
        }
    }

    /// Nominal per-turn service latency budget surfaced in Pi 4 proof logs.
    #[must_use]
    pub const fn max_service_us(self) -> u32 {
        match self.class {
            DriverTaskClass::RealtimeInput => 250,
            DriverTaskClass::ConsoleOutput => 500,
            DriverTaskClass::NetworkControl => 750,
            DriverTaskClass::NetworkData => 1_000,
            DriverTaskClass::DisplayRefresh => 2_000,
            DriverTaskClass::Background => 5_000,
        }
    }

    /// Returns true when the declared authority is narrow enough for this role.
    #[must_use]
    pub const fn authority_matches_kind(self) -> bool {
        matches!(
            (self.kind, self.authority),
            (
                DriverTaskKind::Serial,
                DriverTaskAuthority::ConsoleTransport
            ) | (
                DriverTaskKind::LocalSeatUsb,
                DriverTaskAuthority::DeviceOnly
            ) | (DriverTaskKind::HdmiText, DriverTaskAuthority::DisplaySink)
                | (
                    DriverTaskKind::WiredNic | DriverTaskKind::WifiNic | DriverTaskKind::VirtualNic,
                    DriverTaskAuthority::NetworkFrameTransport
                )
                | (
                    DriverTaskKind::SdioHost | DriverTaskKind::PcieRoot,
                    DriverTaskAuthority::DeviceOnly
                )
        )
    }

    /// Returns true when the scheduling class matches the hardware role.
    #[must_use]
    pub const fn class_matches_kind(self) -> bool {
        matches!(
            (self.kind, self.class),
            (
                DriverTaskKind::Serial,
                DriverTaskClass::RealtimeInput | DriverTaskClass::ConsoleOutput
            ) | (DriverTaskKind::LocalSeatUsb, DriverTaskClass::RealtimeInput)
                | (DriverTaskKind::HdmiText, DriverTaskClass::DisplayRefresh)
                | (
                    DriverTaskKind::WiredNic | DriverTaskKind::WifiNic | DriverTaskKind::VirtualNic,
                    DriverTaskClass::NetworkData
                )
                | (
                    DriverTaskKind::SdioHost | DriverTaskKind::PcieRoot,
                    DriverTaskClass::NetworkControl | DriverTaskClass::Background
                )
        )
    }
}

/// seL4 priority used while a newly resumed driver runtime reaches `Recv`.
#[must_use]
pub const fn driver_task_bootstrap_priority(contract: DriverTaskContract) -> u8 {
    contract.bootstrap_priority(CURRENT_DRIVER_TASK_RUNTIME_PROFILE)
}

/// Contract validation failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskContractError {
    /// Driver label is empty.
    MissingName,
    /// Queue depth is zero.
    ZeroQueueDepth,
    /// Queue depth exceeds the HAL admission bound.
    QueueDepthTooLarge,
    /// Operation budget is zero.
    ZeroOperationBudget,
    /// Byte budget is zero.
    ZeroByteBudget,
    /// Frame/report budget is zero.
    ZeroFrameBudget,
    /// Service path does not expose preemption points.
    NotPreemptible,
    /// Blocking wait is permitted without a finite spin bound.
    UnboundedBlockingWait,
    /// Blocking waits are not admitted for this scheduling class.
    BlockingWaitNotAdmittedForClass,
    /// Authority does not match the isolated driver-task model.
    InvalidAuthority,
    /// Scheduling class does not match the hardware role.
    InvalidClass,
    /// Dedicated isolation was requested before the seL4 task substrate exists.
    DedicatedSubstrateNotReady,
}

impl DriverTaskContractError {
    /// Stable diagnostic reason.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::MissingName => "driver-task-contract-missing-name",
            Self::ZeroQueueDepth => "driver-task-contract-zero-queue-depth",
            Self::QueueDepthTooLarge => "driver-task-contract-queue-depth-too-large",
            Self::ZeroOperationBudget => "driver-task-contract-zero-op-budget",
            Self::ZeroByteBudget => "driver-task-contract-zero-byte-budget",
            Self::ZeroFrameBudget => "driver-task-contract-zero-frame-budget",
            Self::NotPreemptible => "driver-task-contract-not-preemptible",
            Self::UnboundedBlockingWait => "driver-task-contract-unbounded-blocking-wait",
            Self::BlockingWaitNotAdmittedForClass => {
                "driver-task-contract-blocking-wait-not-admitted-for-class"
            }
            Self::InvalidAuthority => "driver-task-contract-invalid-authority",
            Self::InvalidClass => "driver-task-contract-invalid-class",
            Self::DedicatedSubstrateNotReady => {
                "driver-task-contract-dedicated-substrate-not-ready"
            }
        }
    }
}

/// Mutable runtime budget for one service turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverServiceBudget {
    contract: DriverTaskContract,
    ops_left: u16,
    bytes_left: u32,
    frames_left: u16,
    blocking_spins_left: u32,
}

impl DriverServiceBudget {
    /// Starts one service turn from a validated contract.
    pub fn new(contract: DriverTaskContract) -> Result<Self, DriverTaskContractError> {
        contract.validate()?;
        Ok(Self {
            contract,
            ops_left: contract.budget.max_ops_per_turn,
            bytes_left: contract.budget.max_bytes_per_turn,
            frames_left: contract.budget.max_frames_per_turn,
            blocking_spins_left: contract.budget.max_blocking_spins,
        })
    }

    /// Returns the contract covered by this budget.
    #[must_use]
    pub const fn contract(self) -> DriverTaskContract {
        self.contract
    }

    /// Charges HAL operations to this service turn.
    pub fn charge_ops(&mut self, count: u16) -> Result<(), DriverServiceBudgetError> {
        if count == 0 {
            return Err(DriverServiceBudgetError::ZeroCharge);
        }
        self.ops_left = self
            .ops_left
            .checked_sub(count)
            .ok_or(DriverServiceBudgetError::OperationsExhausted)?;
        Ok(())
    }

    /// Charges bytes moved through HAL-owned buffers.
    pub fn charge_bytes(&mut self, count: u32) -> Result<(), DriverServiceBudgetError> {
        if count == 0 {
            return Err(DriverServiceBudgetError::ZeroCharge);
        }
        self.bytes_left = self
            .bytes_left
            .checked_sub(count)
            .ok_or(DriverServiceBudgetError::BytesExhausted)?;
        Ok(())
    }

    /// Charges frames, packets, reports, or rows.
    pub fn charge_frames(&mut self, count: u16) -> Result<(), DriverServiceBudgetError> {
        if count == 0 {
            return Err(DriverServiceBudgetError::ZeroCharge);
        }
        self.frames_left = self
            .frames_left
            .checked_sub(count)
            .ok_or(DriverServiceBudgetError::FramesExhausted)?;
        Ok(())
    }

    /// Charges bounded blocking spins.
    pub fn charge_blocking_spins(&mut self, count: u32) -> Result<(), DriverServiceBudgetError> {
        if count == 0 {
            return Err(DriverServiceBudgetError::ZeroCharge);
        }
        if !self.contract.budget.allow_blocking_waits {
            return Err(DriverServiceBudgetError::BlockingForbidden);
        }
        self.blocking_spins_left = self
            .blocking_spins_left
            .checked_sub(count)
            .ok_or(DriverServiceBudgetError::BlockingExhausted)?;
        Ok(())
    }

    /// Remaining operation budget for diagnostics.
    #[must_use]
    pub const fn ops_left(self) -> u16 {
        self.ops_left
    }

    /// Remaining byte budget for diagnostics.
    #[must_use]
    pub const fn bytes_left(self) -> u32 {
        self.bytes_left
    }

    /// Remaining frame/report budget for diagnostics.
    #[must_use]
    pub const fn frames_left(self) -> u16 {
        self.frames_left
    }

    /// Remaining bounded spin budget for diagnostics.
    #[must_use]
    pub const fn blocking_spins_left(self) -> u32 {
        self.blocking_spins_left
    }
}

/// Runtime budget exhaustion reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverServiceBudgetError {
    /// Charge amount is zero and would not prove forward progress.
    ZeroCharge,
    /// Operation budget exhausted.
    OperationsExhausted,
    /// Byte budget exhausted.
    BytesExhausted,
    /// Frame/report budget exhausted.
    FramesExhausted,
    /// Blocking waits are forbidden by this contract.
    BlockingForbidden,
    /// Blocking spin budget exhausted.
    BlockingExhausted,
}

impl DriverServiceBudgetError {
    /// Stable diagnostic reason.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::ZeroCharge => "driver-service-budget-zero-charge",
            Self::OperationsExhausted => "driver-service-budget-ops-exhausted",
            Self::BytesExhausted => "driver-service-budget-bytes-exhausted",
            Self::FramesExhausted => "driver-service-budget-frames-exhausted",
            Self::BlockingForbidden => "driver-service-budget-blocking-forbidden",
            Self::BlockingExhausted => "driver-service-budget-blocking-exhausted",
        }
    }
}

/// Trait implemented by drivers with a HAL scheduling contract.
pub trait ScheduledHardwareDriver {
    /// Returns the static HAL scheduling contract for this driver.
    fn driver_task_contract() -> DriverTaskContract;
}

/// Shared-buffer descriptor passed over bounded driver-task rings.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverFrameDescriptor {
    /// Offset into the role-owned shared buffer arena.
    pub offset: u32,
    /// Valid payload length at `offset`.
    pub len: u16,
    /// Role-specific flags. The root task owns interpretation.
    pub flags: u16,
}

impl DriverFrameDescriptor {
    /// Creates a bounded frame descriptor for driver-task IPC rings.
    pub const fn new(offset: u32, len: u16, flags: u16) -> Result<Self, DriverTaskRingError> {
        if len as usize > MAX_DRIVER_TASK_FRAME_BYTES {
            return Err(DriverTaskRingError::FrameTooLarge);
        }
        Ok(Self { offset, len, flags })
    }

    /// Returns whether this frame descriptor explicitly depends on root
    /// context state for the current service turn.
    #[must_use]
    pub const fn root_context_non_acceptance(self) -> bool {
        self.flags & DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE != 0
    }

    /// Returns whether this frame descriptor blocks owner-state credit.
    #[must_use]
    pub const fn non_acceptance(self) -> bool {
        self.flags & DRIVER_TASK_RING_NON_ACCEPTANCE_FLAGS != 0
    }
}

/// Primitive budget grant encoded in the pointer-free shared-ring ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTaskBudgetGrant {
    /// Maximum HAL operations admitted for the command.
    pub max_ops: u16,
    /// Maximum frames, packets, reports, or rows admitted for the command.
    pub max_frames: u16,
    /// Maximum bytes admitted for the command.
    pub max_bytes: u32,
}

impl DriverTaskBudgetGrant {
    /// Encodes a contract budget for shared-ring dispatch.
    #[must_use]
    pub const fn from_contract(contract: DriverTaskContract) -> Self {
        Self {
            max_ops: contract.budget.max_ops_per_turn,
            max_frames: contract.budget.max_frames_per_turn,
            max_bytes: contract.budget.max_bytes_per_turn,
        }
    }
}

/// Command opcode encoded in the pointer-free shared-ring ABI.
#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskOpcode {
    /// Service pending device work up to the supplied budget.
    Service = 1,
    /// Acknowledge a badged IRQ/notification event.
    Irq = 2,
    /// Transmit or render a shared-buffer frame.
    SubmitFrame = 3,
    /// Flush completion state without admitting bulk data progress.
    Flush = 4,
    /// Stop accepting work so root can suspend/revoke the task.
    Shutdown = 5,
}

impl DriverTaskOpcode {
    /// Primitive wire value for shared-ring records.
    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self as u16
    }
}

/// Pi 4 hardware hot paths that must move behind pointer-free rings before
/// strongest dedicated-driver isolation may be claimed.
#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskHotPath {
    /// UART receive/transmit service.
    SerialConsole = 1,
    /// USB HID keyboard polling and report delivery.
    UsbKeyboard = 2,
    /// HDMI text/framebuffer submission.
    HdmiText = 3,
    /// GENET RX/TX descriptor service.
    GenetNic = 4,
    /// CYW43 SDPCM RX/TX frame service.
    Cyw43Wifi = 5,
    /// SDIO command/data/interrupt service beneath CYW43.
    SdioHost = 6,
    /// PCIe root/VL805 doorbell and configuration service.
    PcieRoot = 7,
}

impl DriverTaskHotPath {
    /// Decode a primitive ring argument into a known Pi 4 hot-path role.
    #[must_use]
    pub const fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::SerialConsole),
            2 => Some(Self::UsbKeyboard),
            3 => Some(Self::HdmiText),
            4 => Some(Self::GenetNic),
            5 => Some(Self::Cyw43Wifi),
            6 => Some(Self::SdioHost),
            7 => Some(Self::PcieRoot),
            _ => None,
        }
    }

    /// Primitive wire identifier carried in `DriverTaskCommandRecord::arg0`.
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self as u32
    }

    /// Stable diagnostic label for the migration target.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SerialConsole => "serial-console",
            Self::UsbKeyboard => "usb-keyboard",
            Self::HdmiText => "hdmi-text",
            Self::GenetNic => "genet-nic",
            Self::Cyw43Wifi => "cyw43-wifi",
            Self::SdioHost => "sdio-host",
            Self::PcieRoot => "pcie-root",
        }
    }

    /// Driver-task contract that owns this hot-path target.
    #[must_use]
    pub const fn contract(self) -> DriverTaskContract {
        match self {
            Self::SerialConsole => SERIAL_DRIVER_TASK_CONTRACT,
            Self::UsbKeyboard => USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            Self::HdmiText => HDMI_TEXT_DRIVER_TASK_CONTRACT,
            Self::GenetNic => GENET_DRIVER_TASK_CONTRACT,
            Self::Cyw43Wifi => CYW43_WIFI_DRIVER_TASK_CONTRACT,
            Self::SdioHost => SDIO_HOST_DRIVER_TASK_CONTRACT,
            Self::PcieRoot => PCIE_ROOT_DRIVER_TASK_CONTRACT,
        }
    }

    /// Shared-ring opcode admitted for this hot path.
    #[must_use]
    pub const fn opcode(self) -> DriverTaskOpcode {
        match self {
            Self::HdmiText => DriverTaskOpcode::SubmitFrame,
            _ => DriverTaskOpcode::Service,
        }
    }

    /// Role bit that must be credited by the hardware-owned ring service.
    #[must_use]
    pub const fn role_bit(self) -> usize {
        driver_task_role_bit(self.contract().kind)
    }

    /// Hot-path bit used for concrete owner-state descriptor coverage.
    #[must_use]
    pub const fn owner_state_bit(self) -> usize {
        1usize << ((self as usize) - 1)
    }
}

/// Complete Pi 4 hot-path migration catalog.
pub const PI4_DRIVER_TASK_HOT_PATHS: [DriverTaskHotPath; 7] = [
    DriverTaskHotPath::SerialConsole,
    DriverTaskHotPath::UsbKeyboard,
    DriverTaskHotPath::HdmiText,
    DriverTaskHotPath::GenetNic,
    DriverTaskHotPath::Cyw43Wifi,
    DriverTaskHotPath::SdioHost,
    DriverTaskHotPath::PcieRoot,
];

#[cfg(feature = "kernel")]
struct DeferredRuntimeInitSlot {
    descriptor: UnsafeCell<DriverRuntimeInitDescriptor>,
    pending: AtomicU32,
    initialized: AtomicU32,
}

#[cfg(feature = "kernel")]
// SAFETY: Root is the only writer for deferred runtime-init descriptors, and it
// serializes commands per contract through the ring `active` gate. Driver tasks
// only see a copied descriptor after root stages it into the command ring.
unsafe impl Sync for DeferredRuntimeInitSlot {}

#[cfg(feature = "kernel")]
impl DeferredRuntimeInitSlot {
    const fn new() -> Self {
        Self {
            descriptor: UnsafeCell::new(DriverRuntimeInitDescriptor::empty()),
            pending: AtomicU32::new(0),
            initialized: AtomicU32::new(0),
        }
    }

    fn store(&self, descriptor: DriverRuntimeInitDescriptor) {
        // SAFETY: See the `Sync` invariant; the descriptor is primitive-only
        // and copied before the pending bit is published.
        unsafe {
            core::ptr::write_volatile(self.descriptor.get(), descriptor);
        }
        self.initialized.store(0, Ordering::Release);
        self.pending.store(1, Ordering::Release);
    }

    fn load(&self) -> DriverRuntimeInitDescriptor {
        // SAFETY: The pending bit is acquired before callers load the
        // descriptor, and root is the sole writer.
        unsafe { core::ptr::read_volatile(self.descriptor.get()) }
    }
}

#[cfg(feature = "kernel")]
static DEFERRED_RUNTIME_INIT_SERIAL: DeferredRuntimeInitSlot = DeferredRuntimeInitSlot::new();
#[cfg(feature = "kernel")]
static DEFERRED_RUNTIME_INIT_USB: DeferredRuntimeInitSlot = DeferredRuntimeInitSlot::new();
#[cfg(feature = "kernel")]
static DEFERRED_RUNTIME_INIT_HDMI: DeferredRuntimeInitSlot = DeferredRuntimeInitSlot::new();
#[cfg(feature = "kernel")]
static DEFERRED_RUNTIME_INIT_GENET: DeferredRuntimeInitSlot = DeferredRuntimeInitSlot::new();
#[cfg(feature = "kernel")]
static DEFERRED_RUNTIME_INIT_CYW43: DeferredRuntimeInitSlot = DeferredRuntimeInitSlot::new();
#[cfg(feature = "kernel")]
static DEFERRED_RUNTIME_INIT_SDIO: DeferredRuntimeInitSlot = DeferredRuntimeInitSlot::new();
#[cfg(feature = "kernel")]
static DEFERRED_RUNTIME_INIT_PCIE: DeferredRuntimeInitSlot = DeferredRuntimeInitSlot::new();

#[cfg(feature = "kernel")]
fn deferred_runtime_init_slot(hot_path: DriverTaskHotPath) -> &'static DeferredRuntimeInitSlot {
    match hot_path {
        DriverTaskHotPath::SerialConsole => &DEFERRED_RUNTIME_INIT_SERIAL,
        DriverTaskHotPath::UsbKeyboard => &DEFERRED_RUNTIME_INIT_USB,
        DriverTaskHotPath::HdmiText => &DEFERRED_RUNTIME_INIT_HDMI,
        DriverTaskHotPath::GenetNic => &DEFERRED_RUNTIME_INIT_GENET,
        DriverTaskHotPath::Cyw43Wifi => &DEFERRED_RUNTIME_INIT_CYW43,
        DriverTaskHotPath::SdioHost => &DEFERRED_RUNTIME_INIT_SDIO,
        DriverTaskHotPath::PcieRoot => &DEFERRED_RUNTIME_INIT_PCIE,
    }
}

/// Concrete owner-state hot-path mask required for strongest Pi 4 isolation.
pub const REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK: usize = DriverTaskHotPath::SerialConsole
    .owner_state_bit()
    | DriverTaskHotPath::UsbKeyboard.owner_state_bit()
    | DriverTaskHotPath::HdmiText.owner_state_bit()
    | DriverTaskHotPath::GenetNic.owner_state_bit()
    | DriverTaskHotPath::Cyw43Wifi.owner_state_bit()
    | DriverTaskHotPath::SdioHost.owner_state_bit()
    | DriverTaskHotPath::PcieRoot.owner_state_bit();

/// Current owner-state hot-path mask admitted for acceptance.
pub const REQUIRED_PI4_ACCEPTANCE_HOT_PATH_MASK: usize = REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK;

/// Pointer-free service handler for bus-owner roles whose concrete hardware
/// queues are not allowed to fall back to root-owned pointer contexts.
///
/// The `context` word carries the expected [`DriverTaskHotPath`] id instead of
/// a root pointer. This lets SDIO and PCIe driver-task service turns reject
/// malformed ring commands through the same ABI that will later carry their
/// bounded bus descriptors.
#[cfg(feature = "kernel")]
pub unsafe fn pi4_bus_ring_service_driver_task(
    context: usize,
    command: DriverTaskCommandRecord,
) -> DriverTaskCompletionRecord {
    let Some(expected_hot_path) = DriverTaskHotPath::from_u32(context as u32) else {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::InternalInvariant,
        );
    };
    if expected_hot_path != DriverTaskHotPath::SdioHost
        && expected_hot_path != DriverTaskHotPath::PcieRoot
    {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::RejectedCommand,
        );
    }
    if command.opcode != expected_hot_path.opcode().as_u16()
        || command.arg0 != expected_hot_path.as_u32()
        || command.arg1 != expected_hot_path.role_bit() as u32
    {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::RejectedCommand,
        );
    }
    if command.frame.len != 0 {
        return DriverTaskCompletionRecord::fault(
            command.sequence,
            DriverTaskFaultCode::RejectedCommand,
        );
    }

    DriverTaskCompletionRecord::idle(command.sequence)
}

/// Fault code encoded in the pointer-free shared-ring ABI.
#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskFaultCode {
    /// No specific fault.
    None = 0,
    /// Command opcode or arguments were not admitted by the driver task.
    RejectedCommand = 1,
    /// The driver exhausted its assigned service budget.
    BudgetExhausted = 2,
    /// Device state made the command impossible to complete.
    DeviceUnavailable = 3,
    /// Driver task observed an internal invariant violation.
    InternalInvariant = 4,
}

impl DriverTaskFaultCode {
    /// Primitive wire value for shared-ring records.
    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    /// Stable diagnostic label for host-side proof tooling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::RejectedCommand => "rejected-command",
            Self::BudgetExhausted => "budget-exhausted",
            Self::DeviceUnavailable => "device-unavailable",
            Self::InternalInvariant => "internal-invariant",
        }
    }
}

/// Command record for the final pointer-free driver-task shared-ring ABI.
///
/// The record intentionally contains only fixed-width integer fields and
/// shared-buffer offsets. It is suitable for mapping into isolated driver
/// VSpaces once the live callback dispatch path is replaced.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTaskCommandRecord {
    /// Monotonic root-assigned sequence number.
    pub sequence: u32,
    /// `DriverTaskOpcode` encoded as a primitive value.
    pub opcode: u16,
    /// Role-specific primitive flags.
    pub flags: u16,
    /// Opcode-specific primitive argument.
    pub arg0: u32,
    /// Second opcode-specific primitive argument.
    pub arg1: u32,
    /// Auxiliary primitive argument for role-specific service handlers.
    pub aux0: u32,
    /// Second auxiliary primitive argument for role-specific service handlers.
    pub aux1: u32,
    /// Per-command service budget.
    pub budget: DriverTaskBudgetGrant,
    /// Shared-buffer descriptor for frame-bearing commands.
    pub frame: DriverFrameDescriptor,
}

impl DriverTaskCommandRecord {
    /// Builds a service command.
    #[must_use]
    pub const fn service(sequence: u32, budget: DriverTaskBudgetGrant) -> Self {
        Self {
            sequence,
            opcode: DriverTaskOpcode::Service.as_u16(),
            flags: 0,
            arg0: 0,
            arg1: 0,
            aux0: 0,
            aux1: 0,
            budget,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        }
    }

    /// Builds an IRQ acknowledgement command.
    #[must_use]
    pub const fn irq(sequence: u32, irq: u32, budget: DriverTaskBudgetGrant) -> Self {
        Self {
            sequence,
            opcode: DriverTaskOpcode::Irq.as_u16(),
            flags: 0,
            arg0: irq,
            arg1: 0,
            aux0: 0,
            aux1: 0,
            budget,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        }
    }

    /// Builds a shared-frame submission command.
    #[must_use]
    pub const fn submit_frame(
        sequence: u32,
        frame: DriverFrameDescriptor,
        budget: DriverTaskBudgetGrant,
    ) -> Self {
        Self {
            sequence,
            opcode: DriverTaskOpcode::SubmitFrame.as_u16(),
            flags: frame.flags,
            arg0: 0,
            arg1: 0,
            aux0: 0,
            aux1: 0,
            budget,
            frame,
        }
    }

    /// Builds a flush command.
    #[must_use]
    pub const fn flush(sequence: u32, budget: DriverTaskBudgetGrant) -> Self {
        Self {
            sequence,
            opcode: DriverTaskOpcode::Flush.as_u16(),
            flags: 0,
            arg0: 0,
            arg1: 0,
            aux0: 0,
            aux1: 0,
            budget,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        }
    }

    /// Builds a pointer-free service command for a declared Pi 4 hot path.
    ///
    /// `arg0` carries the `DriverTaskHotPath` wire id and `arg1` carries the
    /// required role bit. Frame-bearing commands must provide a descriptor;
    /// non-frame commands use a zero-length descriptor.
    #[must_use]
    pub const fn pi4_hot_path(
        sequence: u32,
        hot_path: DriverTaskHotPath,
        budget: DriverTaskBudgetGrant,
        frame: DriverFrameDescriptor,
    ) -> Self {
        Self {
            sequence,
            opcode: hot_path.opcode().as_u16(),
            flags: frame.flags,
            arg0: hot_path.as_u32(),
            arg1: hot_path.role_bit() as u32,
            aux0: 0,
            aux1: 0,
            budget,
            frame,
        }
    }

    /// Builds a shutdown command.
    #[must_use]
    pub const fn shutdown(sequence: u32) -> Self {
        Self {
            sequence,
            opcode: DriverTaskOpcode::Shutdown.as_u16(),
            flags: 0,
            arg0: 0,
            arg1: 0,
            aux0: 0,
            aux1: 0,
            budget: DriverTaskBudgetGrant {
                max_ops: 1,
                max_frames: 1,
                max_bytes: 1,
            },
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        }
    }

    /// Returns whether this command may be credited toward owner-state proof.
    #[must_use]
    pub const fn owner_state_credit_eligible(self) -> bool {
        self.flags & DRIVER_TASK_RING_NON_ACCEPTANCE_FLAGS == 0 && !self.frame.non_acceptance()
    }
}

/// Completion code encoded in the pointer-free shared-ring ABI.
#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskCompletionCode {
    /// Device service made progress.
    Progress = 1,
    /// A frame/report is available for root-owned protocol processing.
    FrameReady = 2,
    /// Command completed without more work.
    Idle = 3,
    /// The driver exhausted its assigned service budget.
    BudgetExhausted = 4,
    /// The driver task faulted or rejected a command.
    Fault = 5,
}

impl DriverTaskCompletionCode {
    /// Primitive wire value for shared-ring records.
    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self as u16
    }
}

/// Completion record for the final pointer-free driver-task shared-ring ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriverTaskCompletionRecord {
    /// Command sequence number being completed.
    pub sequence: u32,
    /// `DriverTaskCompletionCode` encoded as a primitive value.
    pub code: u16,
    /// `DriverTaskFaultCode` or role-specific primitive detail.
    pub detail: u16,
    /// Role-specific primitive result.
    pub result: u32,
    /// Shared-buffer descriptor for frame-bearing completions.
    pub frame: DriverFrameDescriptor,
}

impl DriverTaskCompletionRecord {
    /// Builds a progress completion.
    #[must_use]
    pub const fn progress(sequence: u32, result: u32) -> Self {
        Self {
            sequence,
            code: DriverTaskCompletionCode::Progress.as_u16(),
            detail: DriverTaskFaultCode::None.as_u16(),
            result,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        }
    }

    /// Builds a frame-ready completion.
    #[must_use]
    pub const fn frame_ready(sequence: u32, frame: DriverFrameDescriptor) -> Self {
        Self {
            sequence,
            code: DriverTaskCompletionCode::FrameReady.as_u16(),
            detail: DriverTaskFaultCode::None.as_u16(),
            result: frame.len as u32,
            frame,
        }
    }

    /// Builds an idle completion.
    #[must_use]
    pub const fn idle(sequence: u32) -> Self {
        Self {
            sequence,
            code: DriverTaskCompletionCode::Idle.as_u16(),
            detail: DriverTaskFaultCode::None.as_u16(),
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        }
    }

    /// Builds a budget-exhausted completion.
    #[must_use]
    pub const fn budget_exhausted(sequence: u32, reason: DriverServiceBudgetError) -> Self {
        Self {
            sequence,
            code: DriverTaskCompletionCode::BudgetExhausted.as_u16(),
            detail: match reason {
                DriverServiceBudgetError::ZeroCharge => 1,
                DriverServiceBudgetError::OperationsExhausted => 2,
                DriverServiceBudgetError::BytesExhausted => 3,
                DriverServiceBudgetError::FramesExhausted => 4,
                DriverServiceBudgetError::BlockingForbidden => 5,
                DriverServiceBudgetError::BlockingExhausted => 6,
            },
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        }
    }

    /// Builds a fault completion.
    #[must_use]
    pub const fn fault(sequence: u32, fault: DriverTaskFaultCode) -> Self {
        Self {
            sequence,
            code: DriverTaskCompletionCode::Fault.as_u16(),
            detail: fault.as_u16(),
            result: 0,
            frame: DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        }
    }
}

/// Command sent from root to a dedicated hardware driver task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskCommand {
    /// Service pending device work up to the supplied contract budget.
    Service,
    /// Acknowledge a badged IRQ/notification event.
    Irq(u32),
    /// Transmit or render a shared-buffer frame.
    SubmitFrame(DriverFrameDescriptor),
    /// Flush completion state without admitting bulk data progress.
    Flush,
    /// Stop accepting work so root can suspend/revoke the task.
    Shutdown,
}

/// Completion published by a dedicated driver task.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskCompletion {
    /// Device service made progress.
    Progress,
    /// A frame/report is available for root-owned protocol processing.
    FrameReady(DriverFrameDescriptor),
    /// Command completed without more work.
    Idle,
    /// The driver exhausted its assigned service budget.
    BudgetExhausted(DriverServiceBudgetError),
    /// The driver task faulted or rejected a command.
    Fault(DriverTaskFaultCode),
}

/// Bounded no-alloc model ring used by driver-task admission tests.
///
/// This is not the fixed-layout shared-memory ABI; live isolated VSpace IPC
/// must use `DriverTaskCommandRecord` and `DriverTaskCompletionRecord`.
pub struct DriverTaskRing<T, const N: usize> {
    queue: Deque<T, N>,
    drops: u64,
}

impl<T, const N: usize> DriverTaskRing<T, N> {
    /// Creates an empty bounded ring.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            queue: Deque::new(),
            drops: 0,
        }
    }

    /// Returns the static ring capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Returns the number of queued entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns true when the ring is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Returns true when the ring cannot accept another entry.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.queue.is_full()
    }

    /// Returns the number of entries dropped because the ring was full.
    #[must_use]
    pub const fn drops(&self) -> u64 {
        self.drops
    }

    /// Pushes one entry without allocation.
    pub fn push(&mut self, item: T) -> Result<(), DriverTaskRingError> {
        if N == 0 || N > usize::from(MAX_DRIVER_TASK_QUEUE_DEPTH) {
            self.drops = self.drops.saturating_add(1);
            return Err(DriverTaskRingError::InvalidDepth);
        }
        self.queue.push_back(item).map_err(|_| {
            self.drops = self.drops.saturating_add(1);
            DriverTaskRingError::Full
        })
    }

    /// Pops the oldest entry.
    pub fn pop(&mut self) -> Option<T> {
        self.queue.pop_front()
    }

    /// Removes all entries and preserves the cumulative drop counter.
    pub fn clear(&mut self) {
        self.queue.clear();
    }
}

impl<T, const N: usize> Default for DriverTaskRing<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Driver-task ring admission failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverTaskRingError {
    /// Ring capacity is zero or exceeds the HAL admission bound.
    InvalidDepth,
    /// Ring has no free entries.
    Full,
    /// Frame descriptor exceeds the HAL frame bound.
    FrameTooLarge,
}

/// Physical serial console driver-task contract.
pub const SERIAL_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "serial",
    kind: DriverTaskKind::Serial,
    class: DriverTaskClass::RealtimeInput,
    authority: DriverTaskAuthority::ConsoleTransport,
    isolation: DriverTaskIsolation::DedicatedSeL4Task,
    budget: DriverTaskBudget::preemptible(1024, 1024, 1024),
    queue_depth: 64,
};

/// Local USB keyboard driver-task contract.
pub const USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "usb-local-seat",
    kind: DriverTaskKind::LocalSeatUsb,
    class: DriverTaskClass::RealtimeInput,
    authority: DriverTaskAuthority::DeviceOnly,
    isolation: DriverTaskIsolation::DedicatedSeL4Task,
    budget: DriverTaskBudget::preemptible(256, 4096, 128),
    queue_depth: 128,
};

/// HDMI text sink driver-task contract.
pub const HDMI_TEXT_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "hdmi-text",
    kind: DriverTaskKind::HdmiText,
    class: DriverTaskClass::DisplayRefresh,
    authority: DriverTaskAuthority::DisplaySink,
    isolation: DriverTaskIsolation::DedicatedSeL4Task,
    budget: DriverTaskBudget::preemptible(64, 4096, 64),
    queue_depth: 64,
};

/// GENET wired NIC driver-task contract.
pub const GENET_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "bcmgenet-v5",
    kind: DriverTaskKind::WiredNic,
    class: DriverTaskClass::NetworkData,
    authority: DriverTaskAuthority::NetworkFrameTransport,
    isolation: DriverTaskIsolation::DedicatedSeL4Task,
    budget: DriverTaskBudget::preemptible(256, 131_072, 128),
    queue_depth: 128,
};

/// CYW43 Wi-Fi NIC driver-task contract.
pub const CYW43_WIFI_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "cyw43455",
    kind: DriverTaskKind::WifiNic,
    class: DriverTaskClass::NetworkData,
    authority: DriverTaskAuthority::NetworkFrameTransport,
    isolation: DriverTaskIsolation::DedicatedSeL4Task,
    budget: DriverTaskBudget::preemptible(192, 65_536, 64),
    queue_depth: 128,
};

/// QEMU RTL8139 compatibility NIC contract.
pub const RTL8139_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "rtl8139",
    kind: DriverTaskKind::VirtualNic,
    class: DriverTaskClass::NetworkData,
    authority: DriverTaskAuthority::NetworkFrameTransport,
    isolation: DriverTaskIsolation::DedicatedSeL4Task,
    budget: DriverTaskBudget::preemptible(128, 65_536, 64),
    queue_depth: 64,
};

/// QEMU virtio compatibility NIC contract.
pub const VIRTIO_NET_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "virtio-net",
    kind: DriverTaskKind::VirtualNic,
    class: DriverTaskClass::NetworkData,
    authority: DriverTaskAuthority::NetworkFrameTransport,
    isolation: DriverTaskIsolation::DedicatedSeL4Task,
    budget: DriverTaskBudget::preemptible(256, 131_072, 128),
    queue_depth: 128,
};

/// SDIO host driver-task contract beneath CYW43.
pub const SDIO_HOST_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "sdio-host",
    kind: DriverTaskKind::SdioHost,
    class: DriverTaskClass::NetworkControl,
    authority: DriverTaskAuthority::DeviceOnly,
    isolation: DriverTaskIsolation::DedicatedSeL4Task,
    budget: DriverTaskBudget::preemptible(256, 65_536, 64),
    queue_depth: 64,
};

/// PCIe root driver-task contract beneath VL805 and PCI NICs.
pub const PCIE_ROOT_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "pcie-root",
    kind: DriverTaskKind::PcieRoot,
    class: DriverTaskClass::NetworkControl,
    authority: DriverTaskAuthority::DeviceOnly,
    isolation: DriverTaskIsolation::DedicatedSeL4Task,
    budget: DriverTaskBudget::preemptible(128, 16_384, 32),
    queue_depth: 32,
};

/// Built-in hardware contracts that must remain valid before driver service.
pub const BUILTIN_DRIVER_TASK_CONTRACTS: &[DriverTaskContract] = &[
    SERIAL_DRIVER_TASK_CONTRACT,
    SDIO_HOST_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT,
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    HDMI_TEXT_DRIVER_TASK_CONTRACT,
    GENET_DRIVER_TASK_CONTRACT,
    CYW43_WIFI_DRIVER_TASK_CONTRACT,
    RTL8139_DRIVER_TASK_CONTRACT,
    VIRTIO_NET_DRIVER_TASK_CONTRACT,
];

/// Snapshot of built-in driver-task isolation mode counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DriverTaskIsolationSummary {
    /// Valid contracts declared by built-in hardware paths.
    pub contracts: usize,
    /// Contracts that default policy requests as dedicated seL4 tasks.
    pub requested_dedicated_sel4_tasks: usize,
    /// Contracts still serviced in root-task compatibility mode.
    pub root_task_compatibility: usize,
    /// Contracts backed by dedicated seL4 task isolation.
    pub dedicated_sel4_tasks: usize,
}

/// Count built-in contract isolation modes after validation.
#[must_use]
pub fn builtin_isolation_summary() -> DriverTaskIsolationSummary {
    let mut summary = DriverTaskIsolationSummary::default();
    for contract in BUILTIN_DRIVER_TASK_CONTRACTS {
        if contract.validate().is_err() {
            continue;
        }
        summary.contracts = summary.contracts.saturating_add(1);
        if matches!(
            contract.requested_isolation(),
            DriverTaskIsolation::DedicatedSeL4Task
        ) {
            summary.requested_dedicated_sel4_tasks =
                summary.requested_dedicated_sel4_tasks.saturating_add(1);
        }
        match contract.isolation {
            DriverTaskIsolation::RootTaskCompatibility => {
                summary.root_task_compatibility = summary.root_task_compatibility.saturating_add(1);
            }
            DriverTaskIsolation::DedicatedSeL4Task => {
                summary.dedicated_sel4_tasks = summary.dedicated_sel4_tasks.saturating_add(1);
            }
        }
    }
    summary
}

/// Whether current built-in hardware paths satisfy the dedicated-task
/// acceptance bar.
#[must_use]
pub fn dedicated_driver_task_acceptance_ready() -> bool {
    let summary = builtin_isolation_summary();
    let proof = driver_task_runtime_proof();
    driver_task_acceptance_ready_for(summary, proof)
}

/// Evaluates dedicated-driver-task acceptance from explicit proof inputs.
#[must_use]
pub const fn driver_task_acceptance_ready_for(
    summary: DriverTaskIsolationSummary,
    proof: DriverTaskRuntimeProof,
) -> bool {
    proof.substrate_active
        && proof.capset_proof
        && proof.fault_proof
        && proof.revoke_proof
        && proof.sched_proof
        && proof.affinity_proof
        && proof.vspace_proof
        && proof.pointer_free_ipc_proof
        && proof.owner_state_proof
        && proof.owner_state_role_mask & REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
            == REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
        && proof.owner_state_hot_path_mask & REQUIRED_PI4_ACCEPTANCE_HOT_PATH_MASK
            == REQUIRED_PI4_ACCEPTANCE_HOT_PATH_MASK
        && proof.broad_caps_leaked == 0
        && proof.live_tcb_role_mask & REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
            == REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
        && proof.hot_path_role_mask & REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
            == REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
        && proof.compatibility_service_role_mask & REQUIRED_DRIVER_TASK_ROLE_MASK == 0
        && summary.dedicated_sel4_tasks >= MIN_DEDICATED_PI4_DRIVER_TASKS
        && summary.root_task_compatibility == 0
}

/// Emit compact scheduling-contract proof breadcrumbs for Pi 4 gate tooling.
#[cfg(feature = "kernel")]
pub fn emit_boot_contract_proof() {
    use core::fmt::Write;

    use heapless::String;

    let proof = driver_task_runtime_proof();
    let proof_ipc_abi = if proof.pointer_free_ipc_proof {
        DriverTaskIpcAbi::SharedRingCommand
    } else {
        CURRENT_DRIVER_TASK_IPC_ABI
    };
    let mut line = String::<192>::new();
    let _ = write!(
        line,
        "DRIVER_TASK_DEFAULT requested={} required={} substrate_active={} live_hot_paths={}",
        if DEDICATED_DRIVER_TASKS_DEFAULT_ENABLED {
            "dedicated"
        } else {
            "compatibility"
        },
        if DEDICATED_DRIVER_TASKS_DEFAULT_ENABLED {
            "yes"
        } else {
            "no"
        },
        if proof.substrate_active { "yes" } else { "no" },
        if proof.hot_path_role_mask & REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
            == REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
        {
            "yes"
        } else {
            "no"
        },
    );
    crate::bootstrap::log::force_uart_line(line.as_str());

    let mut line = String::<384>::new();
    let _ = write!(
        line,
        "DRIVER_TASK_SUBSTRATE active={} profile=pi4-uboot-aarch64 task_count={} failed_count={} live_tcb_count={} root_authority_retained=yes fault_endpoint_ready={} revoke_ready={} broad_caps_leaked={} sched={} affinity={} affinity_configured={} affinity_applied={} vspace={} ipc_abi={} pointer_free_ipc={} owner_state={} live_hot_paths={}",
        if proof.substrate_active { "yes" } else { "no" },
        proof.configured_count,
        proof.failed_count,
        proof.live_tcb_count,
        if proof.fault_proof { "yes" } else { "no" },
        if proof.revoke_proof { "yes" } else { "no" },
        proof.broad_caps_leaked,
        if proof.sched_proof { "yes" } else { "no" },
        if proof.affinity_proof { "per-driver" } else { "missing" },
        proof.affinity_configured_count,
        proof.affinity_applied_count,
        if proof.vspace_proof { "isolated" } else { "shared-root" },
        proof_ipc_abi.as_str(),
        if proof.pointer_free_ipc_proof { "yes" } else { "no" },
        if proof.owner_state_proof {
            "driver-owned"
        } else {
            "root-owned"
        },
        if proof.hot_path_role_mask & REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
            == REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
        {
            "yes"
        } else {
            "no"
        },
    );
    crate::bootstrap::log::force_uart_line(line.as_str());

    for contract in BUILTIN_DRIVER_TASK_CONTRACTS {
        let mut line = String::<384>::new();
        let status = if contract.validate().is_ok() {
            "valid"
        } else {
            "invalid"
        };
        let role_bit = driver_task_role_bit(contract.kind);
        let live_tcb = role_bit != 0 && proof.live_tcb_role_mask & role_bit != 0;
        let hot_path = role_bit != 0 && proof.hot_path_role_mask & role_bit != 0;
        let observed_service_us = observed_service_us_for_contract(*contract);
        if observed_service_us == 0 {
            let _ = write!(
                line,
                "SCHED_CONTRACT contract={} status={} service_class={} isolation={} requested_isolation={} live_tcb={} hot_path={} priority={} service_order={} max_ops={} max_bytes={} max_frames={} max_service_us={} vspace={} ipc_abi={} pointer_free_ipc={}",
                contract.name,
                status,
                contract.class.as_str(),
                contract.isolation.as_str(),
                contract.requested_isolation().as_str(),
                if live_tcb { "yes" } else { "no" },
                if hot_path { "dedicated" } else { "root-task-compatibility" },
                contract.sel4_priority(),
                contract.service_order(),
                contract.budget.max_ops_per_turn,
                contract.budget.max_bytes_per_turn,
                contract.budget.max_frames_per_turn,
                contract.max_service_us(),
                if proof.vspace_proof {
                    "isolated"
                } else {
                    "shared-root"
                },
                proof_ipc_abi.as_str(),
                if proof.pointer_free_ipc_proof {
                    "yes"
                } else {
                    "no"
                },
            );
        } else {
            let _ = write!(
                line,
                "SCHED_CONTRACT contract={} status={} service_class={} isolation={} requested_isolation={} live_tcb={} hot_path={} priority={} service_order={} max_ops={} max_bytes={} max_frames={} max_service_us={} observed_service_us={} vspace={} ipc_abi={} pointer_free_ipc={}",
                contract.name,
                status,
                contract.class.as_str(),
                contract.isolation.as_str(),
                contract.requested_isolation().as_str(),
                if live_tcb { "yes" } else { "no" },
                if hot_path { "dedicated" } else { "root-task-compatibility" },
                contract.sel4_priority(),
                contract.service_order(),
                contract.budget.max_ops_per_turn,
                contract.budget.max_bytes_per_turn,
                contract.budget.max_frames_per_turn,
                contract.max_service_us(),
                observed_service_us,
                if proof.vspace_proof {
                    "isolated"
                } else {
                    "shared-root"
                },
                proof_ipc_abi.as_str(),
                if proof.pointer_free_ipc_proof {
                    "yes"
                } else {
                    "no"
                },
            );
        }
        crate::bootstrap::log::force_uart_line(line.as_str());

        let mut line = String::<320>::new();
        let _ = write!(
            line,
            "DRIVER_TASK role={} contract={} isolation={} requested_isolation={} live_tcb={} hot_path={} capset={} fault_probe={} revoke_ready={} priority={} vspace={} ipc_abi={} pointer_free_ipc={}",
            contract.kind.proof_role(),
            contract.name,
            contract.isolation.as_str(),
            contract.requested_isolation().as_str(),
            if live_tcb { "yes" } else { "no" },
            if hot_path { "dedicated" } else { "root-task-compatibility" },
            if proof.capset_proof { "pass" } else { "fail" },
            if proof.fault_proof { "pass" } else { "fail" },
            if proof.revoke_proof { "yes" } else { "no" },
            contract.sel4_priority(),
            if proof.vspace_proof {
                "isolated"
            } else {
                "shared-root"
            },
            proof_ipc_abi.as_str(),
            if proof.pointer_free_ipc_proof {
                "yes"
            } else {
                "no"
            },
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
    }

    let summary = builtin_isolation_summary();
    let mut line = String::<320>::new();
    let _ = write!(
        line,
        "DRIVER_TASK_SUMMARY contracts={} requested_dedicated={} dedicated={} compatibility={} live_tcb_roles=0x{:x} hot_path_roles=0x{:x} shared_ring_roles=0x{:x} owner_state_roles=0x{:x} owner_state_hot_paths=0x{:x} compatibility_roles=0x{:x}",
        summary.contracts,
        summary.requested_dedicated_sel4_tasks,
        summary.dedicated_sel4_tasks,
        summary.root_task_compatibility,
        proof.live_tcb_role_mask,
        proof.hot_path_role_mask,
        proof.shared_ring_service_role_mask,
        proof.owner_state_role_mask,
        proof.owner_state_hot_path_mask,
        proof.compatibility_service_role_mask,
    );
    crate::bootstrap::log::force_uart_line(line.as_str());

    for hot_path in PI4_DRIVER_TASK_HOT_PATHS {
        let contract = hot_path.contract();
        let present = proof.owner_state_hot_path_mask & hot_path.owner_state_bit() != 0;
        let mut line = String::<192>::new();
        let _ = write!(
            line,
            "DRIVER_TASK_OWNER_STATE contract={} hot_path={} owner_state={} descriptor={} root_pointer={}",
            contract.name,
            hot_path.as_str(),
            if present { "driver-owned" } else { "missing" },
            if present { "present" } else { "missing" },
            if present { "no" } else { "unknown" },
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
    }

    let mut line = String::<384>::new();
    let ready = dedicated_driver_task_acceptance_ready();
    let reason = if ready {
        "dedicated-sel4-substrate-active"
    } else if !proof.substrate_active {
        "dedicated-sel4-substrate-not-active"
    } else if proof.failed_count != 0 {
        "driver-task-bootstrap-failures"
    } else if !proof.affinity_proof {
        "driver-task-affinity-not-proven"
    } else if !proof.vspace_proof {
        "driver-task-vspace-isolation-not-proven"
    } else if !proof.pointer_free_ipc_proof {
        "driver-task-pointer-free-ipc-not-proven"
    } else if !proof.owner_state_proof {
        "driver-task-owner-state-not-proven"
    } else if proof.hot_path_role_mask & REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
        != REQUIRED_DRIVER_TASK_ACCEPTANCE_ROLE_MASK
    {
        DEDICATED_DRIVER_TASK_LIVE_HOT_PATHS_MISSING
    } else if summary.root_task_compatibility != 0 {
        "root-task-compatibility-contracts-active"
    } else {
        "insufficient-dedicated-driver-tasks"
    };
    let _ = write!(
        line,
        "DRIVER_TASK_ACCEPTANCE dedicated_ready={} reason={} required={} dedicated={} compatibility={} substrate={} capset={} fault={} revoke={} sched={} affinity={} vspace={} ipc_abi={} pointer_free_ipc={} owner_state={} owner_state_hot_paths=0x{:x} live_tcb_roles=0x{:x} hot_path_roles=0x{:x} compatibility_roles=0x{:x}",
        if ready { "yes" } else { "no" },
        reason,
        MIN_DEDICATED_PI4_DRIVER_TASKS,
        summary.dedicated_sel4_tasks,
        summary.root_task_compatibility,
        if proof.substrate_active { "active" } else { "inactive" },
        if proof.capset_proof { "pass" } else { "fail" },
        if proof.fault_proof { "pass" } else { "fail" },
        if proof.revoke_proof { "pass" } else { "fail" },
        if proof.sched_proof { "pass" } else { "fail" },
        if proof.affinity_proof { "pass" } else { "fail" },
        if proof.vspace_proof { "isolated" } else { "shared-root" },
        proof_ipc_abi.as_str(),
        if proof.pointer_free_ipc_proof { "yes" } else { "no" },
        if proof.owner_state_proof {
            "driver-owned"
        } else {
            "root-owned"
        },
        proof.owner_state_hot_path_mask,
        proof.live_tcb_role_mask,
        proof.hot_path_role_mask,
        proof.compatibility_service_role_mask,
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "kernel")]
    use core::sync::atomic::Ordering;

    #[test]
    fn builtin_driver_task_contracts_are_valid_and_dedicated() {
        for contract in BUILTIN_DRIVER_TASK_CONTRACTS {
            assert_eq!(contract.validate(), Ok(()), "{contract:?}");
            assert_eq!(contract.isolation, DriverTaskIsolation::DedicatedSeL4Task);
            assert!(driver_task_contract_key(*contract).is_some());
            assert!(contract.authority_matches_kind(), "{contract:?}");
            assert!(contract.class_matches_kind(), "{contract:?}");
            assert!(contract.budget.preemptible);
            assert!(!contract.budget.allow_blocking_waits);
        }
    }

    #[test]
    fn priority_order_matches_sel4_and_cooperative_service_rules() {
        assert!(
            SERIAL_DRIVER_TASK_CONTRACT.sel4_priority()
                > SDIO_HOST_DRIVER_TASK_CONTRACT.sel4_priority()
        );
        assert!(
            SDIO_HOST_DRIVER_TASK_CONTRACT.sel4_priority()
                > GENET_DRIVER_TASK_CONTRACT.sel4_priority()
        );
        assert!(
            GENET_DRIVER_TASK_CONTRACT.sel4_priority()
                > HDMI_TEXT_DRIVER_TASK_CONTRACT.sel4_priority()
        );
        assert!(
            SERIAL_DRIVER_TASK_CONTRACT.service_order()
                < SDIO_HOST_DRIVER_TASK_CONTRACT.service_order()
        );
        assert!(
            SDIO_HOST_DRIVER_TASK_CONTRACT.service_order()
                < GENET_DRIVER_TASK_CONTRACT.service_order()
        );

        assert!(SERIAL_DRIVER_TASK_CONTRACT.preempts_network_data());
        assert!(USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT.preempts_network_data());
        assert!(SDIO_HOST_DRIVER_TASK_CONTRACT.preempts_network_data());
        assert!(!CYW43_WIFI_DRIVER_TASK_CONTRACT.preempts_network_data());
        assert!(!GENET_DRIVER_TASK_CONTRACT.preempts_network_data());
    }

    #[test]
    fn pi4_bootstrap_priority_is_schedulable_for_bounded_bootstrap() {
        assert_eq!(
            SERIAL_DRIVER_TASK_CONTRACT.bootstrap_priority(DriverTaskRuntimeProfile::Pi4Hardware),
            PI4_BOUNDED_BOOTSTRAP_PRIORITY
        );
        assert!(
            SERIAL_DRIVER_TASK_CONTRACT.bootstrap_priority(DriverTaskRuntimeProfile::Pi4Hardware)
                > SERIAL_DRIVER_TASK_CONTRACT.sel4_priority()
        );
        assert_eq!(
            SERIAL_DRIVER_TASK_CONTRACT
                .bootstrap_priority(DriverTaskRuntimeProfile::QemuCompatibility),
            SERIAL_DRIVER_TASK_CONTRACT.sel4_priority()
        );
        assert_eq!(
            SERIAL_DRIVER_TASK_CONTRACT.bootstrap_priority(DriverTaskRuntimeProfile::HostTest),
            SERIAL_DRIVER_TASK_CONTRACT.sel4_priority()
        );
    }

    #[test]
    fn pi4_pre_root_runtime_init_defers_serial_usb_network_sdio_and_pcie_before_shell() {
        assert!(pre_root_runtime_init_deferred_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware,
            Pi4PreRootNetBootstrapSelection::Wifi,
            SERIAL_DRIVER_TASK_CONTRACT
        ));
        assert!(pre_root_runtime_init_deferred_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware,
            Pi4PreRootNetBootstrapSelection::Wifi,
            USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT
        ));
        assert!(pre_root_runtime_init_deferred_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware,
            Pi4PreRootNetBootstrapSelection::Wifi,
            GENET_DRIVER_TASK_CONTRACT
        ));
        assert!(pre_root_runtime_init_deferred_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware,
            Pi4PreRootNetBootstrapSelection::Wifi,
            CYW43_WIFI_DRIVER_TASK_CONTRACT
        ));
        assert!(pre_root_runtime_init_deferred_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware,
            Pi4PreRootNetBootstrapSelection::Wired,
            CYW43_WIFI_DRIVER_TASK_CONTRACT
        ));
        assert!(pre_root_runtime_init_deferred_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware,
            Pi4PreRootNetBootstrapSelection::Wired,
            GENET_DRIVER_TASK_CONTRACT
        ));
        assert!(pre_root_runtime_init_deferred_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware,
            Pi4PreRootNetBootstrapSelection::Wifi,
            SDIO_HOST_DRIVER_TASK_CONTRACT
        ));
        assert!(pre_root_runtime_init_deferred_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware,
            Pi4PreRootNetBootstrapSelection::Wifi,
            PCIE_ROOT_DRIVER_TASK_CONTRACT
        ));
        assert!(!pre_root_runtime_init_deferred_for_profile(
            DriverTaskRuntimeProfile::QemuCompatibility,
            Pi4PreRootNetBootstrapSelection::Wifi,
            GENET_DRIVER_TASK_CONTRACT
        ));
        assert!(!pre_root_runtime_init_deferred_for_profile(
            DriverTaskRuntimeProfile::HostTest,
            Pi4PreRootNetBootstrapSelection::Wired,
            GENET_DRIVER_TASK_CONTRACT
        ));
    }

    #[test]
    fn deferred_runtime_init_replay_stays_bounded_after_prompt() {
        assert!(deferred_runtime_init_replay_must_be_bounded(
            DriverTaskHotPath::SdioHost
        ));
        assert!(deferred_runtime_init_replay_must_be_bounded(
            DriverTaskHotPath::Cyw43Wifi
        ));
        assert!(deferred_runtime_init_replay_must_be_bounded(
            DriverTaskHotPath::PcieRoot
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn clear_driver_task_transport_removes_partial_bootstrap_endpoint() {
        let contract = USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT;
        publish_driver_task_command_endpoint(contract, 0x1234);
        publish_driver_task_ring(contract, 0x7000_0000);
        publish_driver_task_scheduler(contract, 0x4321, 240);
        publish_driver_task_steady_priority_active(contract);

        clear_driver_task_transport(contract);

        let task_key = driver_task_contract_key(contract).expect("task key");
        let slot = slot_for_task_key(task_key).expect("slot");
        assert_eq!(slot.endpoint.load(Ordering::Acquire), 0);
        assert_eq!(slot.ring_root_ptr.load(Ordering::Acquire), 0);
        assert_eq!(slot.tcb.load(Ordering::Acquire), 0);
        assert_eq!(slot.steady_priority.load(Ordering::Acquire), 0);
        assert_eq!(slot.steady_priority_active.load(Ordering::Acquire), 0);
        assert_eq!(slot.active.load(Ordering::Acquire), 0);
        assert_eq!(slot.request_seq.load(Ordering::Acquire), 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn bus_owner_transport_caps_require_published_shared_window_pages() {
        let contract = SDIO_HOST_DRIVER_TASK_CONTRACT;
        clear_driver_task_transport(contract);
        publish_driver_task_command_endpoint(contract, 0x1234);
        publish_driver_task_ring_frame_cap(contract, 0x2000);
        assert!(driver_task_bus_owner_transport_caps_with_shared(
            contract,
            DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY
        )
        .is_none());

        publish_driver_task_shared_frame_cap(contract, 0, 0x3000);
        assert!(driver_task_bus_owner_transport_caps_with_shared(
            contract,
            DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY
        )
        .is_none());

        publish_driver_task_shared_frame_cap(contract, 1, 0x4000);
        let (endpoint, ring, shared) = driver_task_bus_owner_transport_caps_with_shared(
            contract,
            DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY,
        )
        .expect("shared bus-owner caps");
        assert_eq!(endpoint, 0x1234);
        assert_eq!(ring, 0x2000);
        assert_eq!(shared, [0x3000, 0x4000]);

        clear_driver_task_transport(contract);
        assert!(driver_task_bus_owner_transport_caps_with_shared(
            contract,
            DRIVER_TASK_BUS_LINK_SHARED_FRAME_CAPACITY
        )
        .is_none());
    }

    #[test]
    fn builtin_isolation_summary_requires_runtime_proof_for_acceptance() {
        let summary = builtin_isolation_summary();
        assert_eq!(summary.contracts, BUILTIN_DRIVER_TASK_CONTRACTS.len());
        assert_eq!(
            summary.dedicated_sel4_tasks,
            BUILTIN_DRIVER_TASK_CONTRACTS.len()
        );
        assert_eq!(summary.root_task_compatibility, 0);
        assert_eq!(
            DEDICATED_DRIVER_TASK_SUBSTRATE_READY,
            summary.dedicated_sel4_tasks > 0
        );
        const {
            assert!(!DEDICATED_DRIVER_TASK_LIVE_HOT_PATHS_READY);
        }
        assert!(!dedicated_driver_task_acceptance_ready());
    }

    #[test]
    fn isolated_vspace_still_requires_pointer_free_ipc_and_owner_state_for_acceptance() {
        let summary = DriverTaskIsolationSummary {
            contracts: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            requested_dedicated_sel4_tasks: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            root_task_compatibility: 0,
            dedicated_sel4_tasks: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
        };
        let proof = DriverTaskRuntimeProof {
            substrate_active: true,
            configured_count: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            failed_count: 0,
            live_tcb_count: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            live_tcb_role_mask: REQUIRED_DRIVER_TASK_ROLE_MASK,
            hot_path_role_mask: REQUIRED_DRIVER_TASK_ROLE_MASK,
            shared_ring_service_role_mask: REQUIRED_DRIVER_TASK_ROLE_MASK,
            owner_state_role_mask: REQUIRED_DRIVER_TASK_ROLE_MASK,
            owner_state_hot_path_mask: REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK,
            compatibility_service_role_mask: 0,
            capset_proof: true,
            fault_proof: true,
            revoke_proof: true,
            sched_proof: true,
            affinity_configured_count: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            affinity_applied_count: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            affinity_proof: true,
            vspace_proof: true,
            pointer_free_ipc_proof: false,
            owner_state_proof: false,
            broad_caps_leaked: 0,
        };
        assert!(!driver_task_acceptance_ready_for(summary, proof));

        let proof = DriverTaskRuntimeProof {
            pointer_free_ipc_proof: true,
            ..proof
        };
        assert!(!driver_task_acceptance_ready_for(summary, proof));

        let proof = DriverTaskRuntimeProof {
            owner_state_proof: true,
            owner_state_hot_path_mask: REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK
                & !DriverTaskHotPath::PcieRoot.owner_state_bit(),
            ..proof
        };
        assert!(!driver_task_acceptance_ready_for(summary, proof));

        let proof = DriverTaskRuntimeProof {
            owner_state_proof: true,
            owner_state_hot_path_mask: REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK,
            ..proof
        };
        assert!(driver_task_acceptance_ready_for(summary, proof));
    }

    #[test]
    fn shared_root_ring_service_does_not_satisfy_hot_path_acceptance() {
        let summary = DriverTaskIsolationSummary {
            contracts: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            requested_dedicated_sel4_tasks: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            root_task_compatibility: 0,
            dedicated_sel4_tasks: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
        };
        let proof = DriverTaskRuntimeProof {
            substrate_active: true,
            configured_count: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            failed_count: 0,
            live_tcb_count: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            live_tcb_role_mask: REQUIRED_DRIVER_TASK_ROLE_MASK,
            hot_path_role_mask: 0,
            shared_ring_service_role_mask: REQUIRED_DRIVER_TASK_ROLE_MASK,
            owner_state_role_mask: 0,
            owner_state_hot_path_mask: 0,
            compatibility_service_role_mask: 0,
            capset_proof: true,
            fault_proof: true,
            revoke_proof: true,
            sched_proof: true,
            affinity_configured_count: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            affinity_applied_count: BUILTIN_DRIVER_TASK_CONTRACTS.len(),
            affinity_proof: true,
            vspace_proof: true,
            pointer_free_ipc_proof: true,
            owner_state_proof: false,
            broad_caps_leaked: 0,
        };

        assert!(!driver_task_acceptance_ready_for(summary, proof));
    }

    #[test]
    fn current_driver_task_ipc_abi_is_transitional_callback_pointer() {
        assert_eq!(
            CURRENT_DRIVER_TASK_IPC_ABI,
            DriverTaskIpcAbi::CallbackPointer
        );
        assert_eq!(CURRENT_DRIVER_TASK_IPC_ABI.as_str(), "callback-pointer");
        assert!(!CURRENT_DRIVER_TASK_IPC_ABI.is_pointer_free());
        assert!(DriverTaskIpcAbi::SharedRingCommand.is_pointer_free());
    }

    #[test]
    fn physical_pi4_builds_do_not_compile_steady_state_compat_service() {
        if matches!(
            CURRENT_DRIVER_TASK_RUNTIME_PROFILE,
            DriverTaskRuntimeProfile::Pi4Hardware
        ) {
            assert!(!STEADY_STATE_COMPAT_SERVICE_COMPILED);
        }
        assert!(!callback_dispatch_allowed_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware
        ));
        assert!(!root_fallback_allowed_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware
        ));
    }

    #[test]
    fn physical_pi_owner_state_cutover_helper_matches_runtime_profile() {
        assert_eq!(
            physical_pi_driver_task_only_owner_state_active(),
            matches!(
                CURRENT_DRIVER_TASK_RUNTIME_PROFILE,
                DriverTaskRuntimeProfile::Pi4Hardware
            )
        );
    }

    #[test]
    fn physical_pi_bootstrap_isolation_helper_tracks_owner_state_cutover() {
        assert_eq!(
            physical_pi_driver_task_bootstrap_requires_isolated_vspace(),
            physical_pi_driver_task_only_owner_state_active(),
        );
    }

    #[test]
    fn service_budget_fails_closed_on_exhaustion() {
        let mut budget = DriverServiceBudget::new(SERIAL_DRIVER_TASK_CONTRACT).unwrap();
        assert_eq!(
            budget.charge_ops(SERIAL_DRIVER_TASK_CONTRACT.budget.max_ops_per_turn),
            Ok(())
        );
        assert_eq!(budget.ops_left(), 0);
        assert_eq!(
            budget.charge_ops(1),
            Err(DriverServiceBudgetError::OperationsExhausted)
        );

        let mut budget = DriverServiceBudget::new(SERIAL_DRIVER_TASK_CONTRACT).unwrap();
        assert_eq!(
            budget.charge_bytes(0),
            Err(DriverServiceBudgetError::ZeroCharge)
        );
        assert_eq!(
            budget.charge_blocking_spins(1),
            Err(DriverServiceBudgetError::BlockingForbidden)
        );
        assert_eq!(
            DriverServiceBudgetError::BlockingForbidden.reason(),
            "driver-service-budget-blocking-forbidden"
        );
    }

    #[test]
    fn driver_task_elapsed_us_never_reports_zero_for_completed_service() {
        assert_eq!(driver_task_elapsed_us(100, 100, 1_000_000), 1);
        assert_eq!(driver_task_elapsed_us(100, 101, 1_000_000), 1);
        assert_eq!(driver_task_elapsed_us(100, 200, 1_000_000), 100);
        assert_eq!(driver_task_elapsed_us(200, 100, 1_000_000), 1);
        assert_eq!(driver_task_elapsed_us(100, 200, 0), 1);
    }

    #[test]
    fn driver_task_ring_is_bounded_and_counts_drops() {
        let mut ring: DriverTaskRing<DriverTaskCommand, 2> = DriverTaskRing::new();
        assert_eq!(ring.capacity(), 2);
        assert!(ring.is_empty());

        assert_eq!(ring.push(DriverTaskCommand::Service), Ok(()));
        assert_eq!(ring.push(DriverTaskCommand::Flush), Ok(()));
        assert!(ring.is_full());
        assert_eq!(
            ring.push(DriverTaskCommand::Shutdown),
            Err(DriverTaskRingError::Full)
        );
        assert_eq!(ring.drops(), 1);
        assert_eq!(ring.pop(), Some(DriverTaskCommand::Service));
        assert_eq!(ring.pop(), Some(DriverTaskCommand::Flush));
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn driver_task_frame_descriptor_rejects_oversize_frames() {
        let descriptor = DriverFrameDescriptor::new(64, MAX_DRIVER_TASK_FRAME_BYTES as u16, 0);
        assert_eq!(
            descriptor,
            Ok(DriverFrameDescriptor {
                offset: 64,
                len: MAX_DRIVER_TASK_FRAME_BYTES as u16,
                flags: 0,
            })
        );

        assert_eq!(
            DriverFrameDescriptor::new(64, (MAX_DRIVER_TASK_FRAME_BYTES + 1) as u16, 0),
            Err(DriverTaskRingError::FrameTooLarge)
        );
    }

    #[test]
    fn shared_ring_wire_records_are_fixed_pointer_free_layout() {
        assert_eq!(core::mem::size_of::<DriverFrameDescriptor>(), 8);
        assert_eq!(core::mem::align_of::<DriverFrameDescriptor>(), 4);
        assert_eq!(core::mem::size_of::<DriverTaskOwnerStateDescriptor>(), 20);
        assert_eq!(core::mem::align_of::<DriverTaskOwnerStateDescriptor>(), 4);
        assert_eq!(core::mem::size_of::<DriverTaskBudgetGrant>(), 8);
        assert_eq!(core::mem::align_of::<DriverTaskBudgetGrant>(), 4);
        assert_eq!(core::mem::size_of::<DriverTaskCommandRecord>(), 40);
        assert_eq!(core::mem::align_of::<DriverTaskCommandRecord>(), 4);
        assert_eq!(core::mem::size_of::<DriverTaskCompletionRecord>(), 20);
        assert_eq!(core::mem::align_of::<DriverTaskCompletionRecord>(), 4);
        assert!(
            DRIVER_TASK_RING_COMPLETION_OFFSET >= core::mem::size_of::<DriverTaskCommandRecord>()
        );
        assert!(
            DRIVER_TASK_RING_COMPLETION_OFFSET + core::mem::size_of::<DriverTaskCompletionRecord>()
                <= DRIVER_TASK_RING_PAGE_BYTES
        );
        assert!(
            DRIVER_TASK_RING_FRAME_OFFSET
                >= DRIVER_TASK_RING_COMPLETION_OFFSET
                    + core::mem::size_of::<DriverTaskCompletionRecord>()
        );
        assert!(
            DRIVER_TASK_OWNER_STATE_OFFSET
                >= DRIVER_TASK_RING_COMPLETION_OFFSET
                    + core::mem::size_of::<DriverTaskCompletionRecord>()
        );
        assert!(
            DRIVER_TASK_OWNER_STATE_OFFSET + DRIVER_TASK_OWNER_STATE_BYTES
                <= DRIVER_TASK_RING_FRAME_OFFSET
        );
        assert!(
            DRIVER_TASK_RING_FRAME_OFFSET + MAX_DRIVER_TASK_FRAME_BYTES
                <= DRIVER_TASK_RING_PAGE_BYTES
        );
        assert_eq!(DRIVER_TASK_RING_VADDR & 0xfff, 0);
        assert_eq!(DRIVER_TASK_IPC_VADDR & 0xfff, 0);
        assert_eq!(DRIVER_TASK_STACK_BOTTOM_VADDR & 0xfff, 0);
        assert_eq!(
            DRIVER_TASK_STACK_TOP_VADDR - DRIVER_TASK_STACK_BOTTOM_VADDR,
            16 * 4096
        );

        let budget = DriverTaskBudgetGrant::from_contract(CYW43_WIFI_DRIVER_TASK_CONTRACT);
        assert_eq!(budget.max_ops, 192);
        assert_eq!(budget.max_frames, 64);
        assert_eq!(budget.max_bytes, 65_536);

        let frame = DriverFrameDescriptor::new(4096, 1500, 0x20).unwrap();
        let command = DriverTaskCommandRecord::submit_frame(7, frame, budget);
        assert_eq!(command.sequence, 7);
        assert_eq!(command.opcode, DriverTaskOpcode::SubmitFrame.as_u16());
        assert_eq!(command.flags, 0x20);
        assert_eq!(command.frame, frame);
        assert!(command.owner_state_credit_eligible());

        let completion = DriverTaskCompletionRecord::frame_ready(7, frame);
        assert_eq!(completion.sequence, 7);
        assert_eq!(
            completion.code,
            DriverTaskCompletionCode::FrameReady.as_u16()
        );
        assert_eq!(completion.result, 1500);
        assert_eq!(completion.frame, frame);

        let fault = DriverTaskCompletionRecord::fault(7, DriverTaskFaultCode::RejectedCommand);
        assert_eq!(fault.code, DriverTaskCompletionCode::Fault.as_u16());
        assert_eq!(fault.detail, DriverTaskFaultCode::RejectedCommand.as_u16());
        assert_eq!(
            DriverTaskFaultCode::RejectedCommand.as_str(),
            "rejected-command"
        );
    }

    #[test]
    fn root_context_ring_commands_are_non_acceptance() {
        let contract = SERIAL_DRIVER_TASK_CONTRACT;
        let frame = DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE,
        };
        let command = DriverTaskCommandRecord::pi4_hot_path(
            1,
            DriverTaskHotPath::SerialConsole,
            DriverTaskBudgetGrant::from_contract(contract),
            frame,
        );
        assert!(command.frame.root_context_non_acceptance());
        assert!(!command.owner_state_credit_eligible());

        let mut flush =
            DriverTaskCommandRecord::flush(2, DriverTaskBudgetGrant::from_contract(contract));
        assert!(flush.owner_state_credit_eligible());
        assert!(driver_task_ring_service_owner_state_credit_eligible(
            DriverTaskRingServiceKind::PointerFreeSelector,
            flush
        ));
        assert!(!driver_task_ring_service_owner_state_credit_eligible(
            DriverTaskRingServiceKind::RootContextDiagnostic,
            flush
        ));
        assert!(!driver_task_ring_service_owner_state_credit_eligible(
            DriverTaskRingServiceKind::None,
            flush
        ));
        flush.flags = DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE;
        assert!(!flush.owner_state_credit_eligible());
        assert!(!driver_task_ring_service_owner_state_credit_eligible(
            DriverTaskRingServiceKind::PointerFreeSelector,
            flush
        ));
        assert_eq!(
            DriverTaskRingServiceKind::RootContextDiagnostic.as_str(),
            "root-context-diagnostic"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_init_command_uses_pointer_free_descriptor_aux() {
        let frame = DriverFrameDescriptor::new(
            DRIVER_TASK_RING_FRAME_OFFSET as u32,
            core::mem::size_of::<DriverRuntimeInitDescriptor>() as u16,
            0,
        )
        .unwrap();
        let command = runtime_init_command(
            DriverTaskHotPath::PcieRoot,
            DriverTaskBudgetGrant::from_contract(PCIE_ROOT_DRIVER_TASK_CONTRACT),
            frame,
        );
        assert_eq!(command.opcode, DriverTaskOpcode::Service.as_u16());
        assert_eq!(command.arg0, DriverTaskHotPath::PcieRoot.as_u32());
        assert_eq!(command.arg1, DriverTaskHotPath::PcieRoot.role_bit() as u32);
        assert_eq!(command.aux0, DRIVER_RUNTIME_INIT_AUX);
        assert_eq!(command.frame, frame);
        assert!(!command.owner_state_credit_eligible());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_engine_init_command_is_service_not_frame_submit() {
        let command = runtime_engine_init_command(
            DriverTaskHotPath::HdmiText,
            DriverTaskBudgetGrant::from_contract(HDMI_TEXT_DRIVER_TASK_CONTRACT),
        );
        assert_eq!(command.opcode, DriverTaskOpcode::Service.as_u16());
        assert_eq!(command.flags, 0);
        assert_eq!(command.arg0, DriverTaskHotPath::HdmiText.as_u32());
        assert_eq!(command.arg1, DriverTaskHotPath::HdmiText.role_bit() as u32);
        assert_eq!(command.aux0, DRIVER_RUNTIME_ENGINE_INIT_AUX);
        assert_eq!(command.aux1, 0);
        assert_eq!(command.frame.len, 0);
        assert!(command.owner_state_credit_eligible());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn steady_ring_trace_suppresses_low_latency_console_turns() {
        let frame = DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: 0,
        };
        let serial = DriverTaskCommandRecord::pi4_hot_path(
            0,
            DriverTaskHotPath::SerialConsole,
            DriverTaskBudgetGrant::from_contract(SERIAL_DRIVER_TASK_CONTRACT),
            frame,
        );
        let usb = DriverTaskCommandRecord::pi4_hot_path(
            0,
            DriverTaskHotPath::UsbKeyboard,
            DriverTaskBudgetGrant::from_contract(USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT),
            frame,
        );
        let hdmi = DriverTaskCommandRecord::pi4_hot_path(
            0,
            DriverTaskHotPath::HdmiText,
            DriverTaskBudgetGrant::from_contract(HDMI_TEXT_DRIVER_TASK_CONTRACT),
            frame,
        );
        assert!(!driver_task_ring_call_trace_enabled(
            SERIAL_DRIVER_TASK_CONTRACT,
            serial,
            DriverTaskRingCommandMode::Steady
        ));
        assert!(!driver_task_ring_call_trace_enabled(
            USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            usb,
            DriverTaskRingCommandMode::Steady
        ));
        assert!(!driver_task_ring_call_trace_enabled(
            HDMI_TEXT_DRIVER_TASK_CONTRACT,
            hdmi,
            DriverTaskRingCommandMode::Steady
        ));
        assert!(!driver_task_ring_call_trace_enabled(
            HDMI_TEXT_DRIVER_TASK_CONTRACT,
            hdmi,
            DriverTaskRingCommandMode::NonBlocking
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn steady_ring_trace_keeps_init_and_bus_turns() {
        let frame = DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: 0,
        };
        let hdmi_init = runtime_engine_init_command(
            DriverTaskHotPath::HdmiText,
            DriverTaskBudgetGrant::from_contract(HDMI_TEXT_DRIVER_TASK_CONTRACT),
        );
        let pcie = DriverTaskCommandRecord::pi4_hot_path(
            0,
            DriverTaskHotPath::PcieRoot,
            DriverTaskBudgetGrant::from_contract(PCIE_ROOT_DRIVER_TASK_CONTRACT),
            frame,
        );
        assert!(driver_task_ring_call_trace_enabled(
            HDMI_TEXT_DRIVER_TASK_CONTRACT,
            hdmi_init,
            DriverTaskRingCommandMode::Steady
        ));
        assert!(driver_task_ring_call_trace_enabled(
            PCIE_ROOT_DRIVER_TASK_CONTRACT,
            pcie,
            DriverTaskRingCommandMode::Steady
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn wifi_descriptor_commands_get_long_bounded_init_window() {
        let mut command = DriverTaskCommandRecord::pi4_hot_path(
            0,
            DriverTaskHotPath::Cyw43Wifi,
            DriverTaskBudgetGrant::from_contract(CYW43_WIFI_DRIVER_TASK_CONTRACT),
            DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: 28,
                flags: 0,
            },
        );
        assert_eq!(
            driver_task_ring_attempt_limit(
                CYW43_WIFI_DRIVER_TASK_CONTRACT,
                command,
                DriverTaskRingCommandMode::NonBlocking
            ),
            DRIVER_TASK_BOOTSTRAP_RING_ATTEMPTS
        );
        command.aux0 = 0x4359_5734;
        assert_eq!(
            driver_task_ring_attempt_limit(
                CYW43_WIFI_DRIVER_TASK_CONTRACT,
                command,
                DriverTaskRingCommandMode::NonBlocking
            ),
            DRIVER_TASK_LONG_INIT_RING_ATTEMPTS
        );
        assert_eq!(
            driver_task_ring_attempt_limit(
                SERIAL_DRIVER_TASK_CONTRACT,
                command,
                DriverTaskRingCommandMode::NonBlocking
            ),
            DRIVER_TASK_BOOTSTRAP_RING_ATTEMPTS
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn prompt_side_local_seat_commands_use_short_nonblocking_window() {
        let command = DriverTaskCommandRecord::pi4_hot_path(
            0,
            DriverTaskHotPath::HdmiText,
            DriverTaskBudgetGrant::from_contract(HDMI_TEXT_DRIVER_TASK_CONTRACT),
            DriverFrameDescriptor {
                offset: DRIVER_TASK_RING_FRAME_OFFSET as u32,
                len: 80,
                flags: 0,
            },
        );

        assert_eq!(
            driver_task_ring_attempt_limit(
                HDMI_TEXT_DRIVER_TASK_CONTRACT,
                command,
                DriverTaskRingCommandMode::NonBlocking
            ),
            DRIVER_TASK_PROMPT_RING_ATTEMPTS
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn usb_keyboard_poll_uses_tiny_prompt_window_without_timeout_spam() {
        let command = DriverTaskCommandRecord::pi4_hot_path(
            0,
            DriverTaskHotPath::UsbKeyboard,
            DriverTaskBudgetGrant::from_contract(USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT),
            DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );

        assert_eq!(
            driver_task_ring_attempt_limit(
                USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
                command,
                DriverTaskRingCommandMode::NonBlocking
            ),
            DRIVER_TASK_USB_PROMPT_POLL_RING_ATTEMPTS
        );
        assert!(!driver_task_ring_call_trace_enabled(
            USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            command,
            DriverTaskRingCommandMode::NonBlocking
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn usb_local_seat_engine_init_uses_prompt_slice_at_shell() {
        let mut command = DriverTaskCommandRecord::pi4_hot_path(
            0,
            DriverTaskHotPath::UsbKeyboard,
            DriverTaskBudgetGrant::from_contract(USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT),
            DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        command.aux0 = DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX;

        assert_eq!(
            driver_task_ring_attempt_limit(
                USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
                command,
                DriverTaskRingCommandMode::NonBlocking
            ),
            DRIVER_TASK_LONG_INIT_RING_ATTEMPTS
        );
        assert_eq!(
            driver_task_ring_attempt_limit(
                USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
                command,
                DriverTaskRingCommandMode::PromptSlice
            ),
            DRIVER_TASK_USB_PROMPT_INIT_RING_ATTEMPTS
        );

        command.aux0 = DRIVER_RUNTIME_USB_ENUMERATE_AUX;
        assert_eq!(
            driver_task_ring_attempt_limit(
                USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
                command,
                DriverTaskRingCommandMode::PromptSlice
            ),
            DRIVER_TASK_USB_PROMPT_ENUM_RING_ATTEMPTS
        );
    }

    #[test]
    fn hdmi_progress_lines_cover_driver_start_without_recursive_display_spam() {
        let progress = driver_task_resource_hdmi_progress_line(
            USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::UsbKeyboard,
            "usb-engine-init",
            "begin",
            None,
        )
        .unwrap();
        assert_eq!(progress.as_str(), "[drivers] USB usb-engine-init begin");

        assert!(driver_task_resource_hdmi_progress_line(
            HDMI_TEXT_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::HdmiText,
            "hdmi-first-draw",
            "ready",
            Some(DriverTaskCompletionRecord::progress(7, 1)),
        )
        .is_none());

        assert!(driver_task_resource_hdmi_progress_line(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::Cyw43Wifi,
            "cyw43-firmware-chunk",
            "ready",
            Some(DriverTaskCompletionRecord::progress(8, 4096)),
        )
        .is_none());

        let fault = driver_task_resource_hdmi_progress_line(
            CYW43_WIFI_DRIVER_TASK_CONTRACT,
            DriverTaskHotPath::Cyw43Wifi,
            "cyw43-transport-init",
            "fault",
            Some(DriverTaskCompletionRecord {
                sequence: 9,
                code: DriverTaskCompletionCode::Fault.as_u16(),
                detail: 0x5323,
                result: 0,
                frame: DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            }),
        )
        .unwrap();
        assert_eq!(
            fault.as_str(),
            "[drivers] WiFi cyw43-transport-init fault detail=0x5323 result=0"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn bounded_turn_boosts_linked_bus_owners() {
        let cyw43 = DriverTaskCommandRecord::pi4_hot_path(
            0,
            DriverTaskHotPath::Cyw43Wifi,
            DriverTaskBudgetGrant::from_contract(CYW43_WIFI_DRIVER_TASK_CONTRACT),
            DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        assert_eq!(
            driver_task_bounded_turn_bus_owner(CYW43_WIFI_DRIVER_TASK_CONTRACT, cyw43),
            Some(SDIO_HOST_DRIVER_TASK_CONTRACT)
        );

        let usb = DriverTaskCommandRecord::pi4_hot_path(
            0,
            DriverTaskHotPath::UsbKeyboard,
            DriverTaskBudgetGrant::from_contract(USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT),
            DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        assert_eq!(
            driver_task_bounded_turn_bus_owner(USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT, usb),
            Some(PCIE_ROOT_DRIVER_TASK_CONTRACT)
        );

        let genet = DriverTaskCommandRecord::pi4_hot_path(
            0,
            DriverTaskHotPath::GenetNic,
            DriverTaskBudgetGrant::from_contract(GENET_DRIVER_TASK_CONTRACT),
            DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        assert_eq!(
            driver_task_bounded_turn_bus_owner(GENET_DRIVER_TASK_CONTRACT, genet),
            None
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn routine_resource_statuses_can_leave_interactive_uart() {
        assert!(!driver_task_resource_status_requires_uart("begin"));
        assert!(!driver_task_resource_status_requires_uart("ready"));
        assert!(driver_task_resource_status_requires_uart(
            "blocked-live-proof-missing"
        ));
        assert!(driver_task_resource_status_requires_uart("no-reply"));
        assert!(driver_task_resource_status_requires_uart("failed"));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn deferred_runtime_init_descriptor_records_for_prompt_replay() {
        let hot_path = DriverTaskHotPath::PcieRoot;
        let mut descriptor = DriverRuntimeInitDescriptor::empty();
        descriptor.hot_path = hot_path.as_u32();
        descriptor.role_bit = hot_path.role_bit() as u32;
        descriptor.flags = pi4_driver_abi::DRIVER_RUNTIME_INIT_REQUIRED_FLAGS
            | pi4_driver_abi::DRIVER_RUNTIME_INIT_FLAG_POLL_ONLY;
        descriptor.shared_page_count = 1;
        descriptor.shared_pages[0] = pi4_driver_abi::DriverRuntimePageDescriptor::new(0x4000_0000);
        assert!(descriptor.valid());

        assert!(record_deferred_runtime_init_descriptor(
            hot_path.contract(),
            descriptor
        ));
        let slot = deferred_runtime_init_slot(hot_path);
        assert_eq!(slot.pending.load(Ordering::Acquire), 1);
        assert_eq!(slot.initialized.load(Ordering::Acquire), 0);
        assert_eq!(slot.load().hot_path, hot_path.as_u32());

        assert!(!record_deferred_runtime_init_descriptor(
            GENET_DRIVER_TASK_CONTRACT,
            descriptor
        ));
        slot.pending.store(0, Ordering::Release);
        slot.initialized.store(0, Ordering::Release);
    }

    #[test]
    fn deferred_runtime_init_replay_uses_bounded_path_after_prompt() {
        assert!(deferred_runtime_init_replay_must_be_bounded(
            DriverTaskHotPath::SdioHost
        ));
        assert!(deferred_runtime_init_replay_must_be_bounded(
            DriverTaskHotPath::PcieRoot
        ));
        assert!(deferred_runtime_init_replay_must_be_bounded(
            DriverTaskHotPath::GenetNic
        ));
        assert!(deferred_runtime_init_replay_must_be_bounded(
            DriverTaskHotPath::Cyw43Wifi
        ));
        assert!(deferred_runtime_init_replay_must_be_bounded(
            DriverTaskHotPath::SerialConsole
        ));
        assert!(deferred_runtime_init_replay_must_be_bounded(
            DriverTaskHotPath::HdmiText
        ));
        assert!(deferred_runtime_init_replay_must_be_bounded(
            DriverTaskHotPath::UsbKeyboard
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn bootstrap_ring_mode_remains_bounded() {
        assert!(driver_task_ring_mode_uses_bounded_send(
            DriverTaskRingCommandMode::Bootstrap
        ));
        assert!(driver_task_ring_mode_uses_bounded_send(
            DriverTaskRingCommandMode::NonBlocking
        ));
        assert!(driver_task_ring_mode_uses_bounded_send(
            DriverTaskRingCommandMode::PromptSlice
        ));
        assert!(!driver_task_ring_mode_uses_bounded_send(
            DriverTaskRingCommandMode::Steady
        ));
        assert!(!driver_task_ring_mode_uses_bounded_send(
            DriverTaskRingCommandMode::BootstrapCall
        ));
        assert_eq!(
            driver_task_ring_flags_for_mode(DriverTaskRingCommandMode::Bootstrap, 0),
            DRIVER_TASK_RING_FLAG_ONE_WAY
        );
        assert_eq!(
            driver_task_ring_flags_for_mode(DriverTaskRingCommandMode::NonBlocking, 0),
            DRIVER_TASK_RING_FLAG_ONE_WAY
        );
        assert_eq!(
            driver_task_ring_flags_for_mode(DriverTaskRingCommandMode::PromptSlice, 0),
            DRIVER_TASK_RING_FLAG_ONE_WAY
        );
        assert_eq!(
            driver_task_ring_flags_for_mode(DriverTaskRingCommandMode::Steady, 0),
            0
        );
        assert_eq!(
            driver_task_ring_flags_for_mode(DriverTaskRingCommandMode::BootstrapCall, 0),
            0
        );
        assert!(!DriverTaskRingCommandMode::Bootstrap.records_latency());
        assert!(!DriverTaskRingCommandMode::BootstrapCall.records_latency());
        assert!(DriverTaskRingCommandMode::PromptSlice.records_latency());
    }

    #[test]
    fn owner_state_descriptors_are_pointer_free_bounded_and_complete() {
        let mut role_mask = 0usize;
        let mut hot_path_mask = 0usize;
        for hot_path in PI4_DRIVER_TASK_HOT_PATHS {
            let descriptor = DriverTaskOwnerStateDescriptor::new(
                hot_path,
                DRIVER_TASK_OWNER_STATE_OFFSET as u32,
                16,
                DRIVER_TASK_RING_FRAME_OFFSET as u32,
                128,
                DRIVER_TASK_OWNER_STATE_REQUIRED_FLAGS,
            )
            .unwrap();
            assert_eq!(descriptor.hot_path, hot_path);
            assert_eq!(descriptor.hot_path.contract(), hot_path.contract());
            assert!(descriptor.has_required_runtime_flags());
            role_mask |= hot_path.role_bit();
            hot_path_mask |= hot_path.owner_state_bit();
        }

        assert_eq!(
            role_mask & REQUIRED_DRIVER_TASK_ROLE_MASK,
            REQUIRED_DRIVER_TASK_ROLE_MASK
        );
        assert_eq!(hot_path_mask, REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK);
        assert_eq!(
            REQUIRED_PI4_OWNER_STATE_HOT_PATHS,
            PI4_DRIVER_TASK_HOT_PATHS.len()
        );
        assert!(DriverTaskOwnerStateDescriptor::new(
            DriverTaskHotPath::SerialConsole,
            DRIVER_TASK_OWNER_STATE_OFFSET as u32 - 1,
            16,
            DRIVER_TASK_RING_FRAME_OFFSET as u32,
            128,
            0,
        )
        .is_none());
        assert!(DriverTaskOwnerStateDescriptor::new(
            DriverTaskHotPath::SerialConsole,
            DRIVER_TASK_OWNER_STATE_OFFSET as u32,
            DRIVER_TASK_OWNER_STATE_BYTES as u16 + 1,
            DRIVER_TASK_RING_FRAME_OFFSET as u32,
            128,
            0,
        )
        .is_none());
        assert!(DriverTaskOwnerStateDescriptor::new(
            DriverTaskHotPath::SerialConsole,
            DRIVER_TASK_OWNER_STATE_OFFSET as u32,
            16,
            64,
            128,
            0,
        )
        .is_none());
        let scaffolding = DriverTaskOwnerStateDescriptor::new(
            DriverTaskHotPath::SerialConsole,
            DRIVER_TASK_OWNER_STATE_OFFSET as u32,
            16,
            DRIVER_TASK_RING_FRAME_OFFSET as u32,
            128,
            0,
        )
        .unwrap();
        assert!(!scaffolding.has_required_runtime_flags());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn owner_state_registration_accepts_only_migrated_runtime_specs() {
        publish_driver_task_bootstrap_report(DriverTaskBootstrapReport {
            configured_count: PI4_DRIVER_TASK_HOT_PATHS.len(),
            live_tcb_count: PI4_DRIVER_TASK_HOT_PATHS.len(),
            isolated_vspace_count: PI4_DRIVER_TASK_HOT_PATHS.len(),
            pointer_free_ipc_count: PI4_DRIVER_TASK_HOT_PATHS.len(),
            vspace_proof: true,
            pointer_free_ipc_proof: true,
            ..DriverTaskBootstrapReport::default()
        });
        for hot_path in PI4_DRIVER_TASK_HOT_PATHS.iter().copied() {
            let descriptor = DriverTaskOwnerStateDescriptor::new(
                hot_path,
                DRIVER_TASK_OWNER_STATE_OFFSET as u32,
                16,
                DRIVER_TASK_RING_FRAME_OFFSET as u32,
                128,
                DRIVER_TASK_OWNER_STATE_REQUIRED_FLAGS,
            )
            .unwrap();

            let registered =
                register_driver_task_owner_state_descriptor(hot_path.contract(), descriptor);
            let spec = pi4_driver_task_runtime_image_spec(hot_path);
            assert!(spec.acceptance_eligible(), "{hot_path:?}");
            assert!(registered, "{hot_path:?}");
        }
        let proof = driver_task_runtime_proof();
        assert_eq!(
            proof.owner_state_hot_path_mask,
            REQUIRED_PI4_ACCEPTANCE_HOT_PATH_MASK
        );
        assert!(proof.owner_state_proof);
        publish_driver_task_bootstrap_report(DriverTaskBootstrapReport::default());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn hardware_progress_credit_excludes_idle_zero_progress_and_bad_frames() {
        assert!(driver_task_completion_has_hardware_progress(
            DriverTaskCompletionRecord::progress(1, 1)
        ));
        assert!(!driver_task_completion_has_hardware_progress(
            DriverTaskCompletionRecord::progress(2, 0)
        ));
        assert!(!driver_task_completion_has_hardware_progress(
            DriverTaskCompletionRecord::idle(3)
        ));

        let valid_frame =
            DriverFrameDescriptor::new(DRIVER_TASK_RING_FRAME_OFFSET as u32, 8, 0).unwrap();
        assert!(driver_task_completion_has_hardware_progress(
            DriverTaskCompletionRecord::frame_ready(4, valid_frame)
        ));

        let zero_frame =
            DriverFrameDescriptor::new(DRIVER_TASK_RING_FRAME_OFFSET as u32, 0, 0).unwrap();
        assert!(!driver_task_completion_has_hardware_progress(
            DriverTaskCompletionRecord::frame_ready(5, zero_frame)
        ));

        let root_frame = DriverFrameDescriptor::new(
            DRIVER_TASK_RING_FRAME_OFFSET as u32,
            8,
            DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE,
        )
        .unwrap();
        assert!(!driver_task_completion_has_hardware_progress(
            DriverTaskCompletionRecord::frame_ready(6, root_frame)
        ));

        let bad_offset = DriverFrameDescriptor::new(0, 8, 0).unwrap();
        assert!(!driver_task_completion_has_hardware_progress(
            DriverTaskCompletionRecord::frame_ready(7, bad_offset)
        ));
    }

    #[test]
    fn pi4_runtime_image_specs_keep_sdio_acceptance_explicit() {
        let generated_policy = crate::generated::driver_runtime_image_policy();
        assert!(generated_policy.required);
        assert_eq!(
            generated_policy.images.len(),
            PI4_DRIVER_TASK_RUNTIME_IMAGE_SPEC_COUNT
        );
        let specs = pi4_driver_task_runtime_image_specs();
        assert_eq!(specs.len(), PI4_DRIVER_TASK_RUNTIME_IMAGE_SPEC_COUNT);
        let mut hot_path_mask = 0usize;
        for spec in specs {
            hot_path_mask |= spec.hot_path.owner_state_bit();
            assert_ne!(
                spec.region_pages(DriverTaskRuntimeRegionKind::Code),
                0,
                "{:?}",
                spec.hot_path
            );
            assert_ne!(
                spec.region_pages(DriverTaskRuntimeRegionKind::Stack),
                0,
                "{:?}",
                spec.hot_path
            );
            assert_ne!(
                spec.region_pages(DriverTaskRuntimeRegionKind::Ipc),
                0,
                "{:?}",
                spec.hot_path
            );
            assert_ne!(
                spec.region_pages(DriverTaskRuntimeRegionKind::Ring),
                0,
                "{:?}",
                spec.hot_path
            );
            assert_ne!(
                spec.region_pages(DriverTaskRuntimeRegionKind::SharedBuffer),
                0,
                "{:?}",
                spec.hot_path
            );
            assert!(spec.declares_transport_regions(), "{:?}", spec.hot_path);
            assert_ne!(spec.declared_region_count(), 0, "{:?}", spec.hot_path);
            assert_ne!(spec.declared_page_count(), 0, "{:?}", spec.hot_path);
            match spec.hot_path {
                DriverTaskHotPath::SerialConsole => {
                    assert_eq!(
                        spec.region_pages(DriverTaskRuntimeRegionKind::SharedBuffer),
                        4
                    );
                }
                DriverTaskHotPath::UsbKeyboard => {
                    assert_eq!(spec.region_pages(DriverTaskRuntimeRegionKind::Dma), 128);
                    assert_eq!(
                        spec.region_pages(DriverTaskRuntimeRegionKind::SharedBuffer),
                        32
                    );
                }
                DriverTaskHotPath::HdmiText => {
                    assert_eq!(spec.region_pages(DriverTaskRuntimeRegionKind::Dma), 0);
                    assert_eq!(
                        spec.region_pages(DriverTaskRuntimeRegionKind::SharedBuffer),
                        16
                    );
                }
                DriverTaskHotPath::GenetNic => {
                    assert_eq!(spec.region_pages(DriverTaskRuntimeRegionKind::Dma), 64);
                    assert_eq!(
                        spec.region_pages(DriverTaskRuntimeRegionKind::SharedBuffer),
                        32
                    );
                }
                DriverTaskHotPath::Cyw43Wifi => {
                    assert_eq!(spec.region_pages(DriverTaskRuntimeRegionKind::Dma), 0);
                    assert_eq!(
                        spec.region_pages(DriverTaskRuntimeRegionKind::SharedBuffer),
                        64
                    );
                }
                DriverTaskHotPath::SdioHost => {
                    assert_eq!(spec.region_pages(DriverTaskRuntimeRegionKind::Dma), 0);
                    assert_eq!(
                        spec.region_pages(DriverTaskRuntimeRegionKind::SharedBuffer),
                        32
                    );
                    assert_eq!(spec.region_pages(DriverTaskRuntimeRegionKind::Mmio), 1);
                }
                DriverTaskHotPath::PcieRoot => {
                    assert_eq!(
                        spec.region_pages(DriverTaskRuntimeRegionKind::SharedBuffer),
                        16
                    );
                }
            }
            assert!(!spec.root_context_required, "{:?}", spec.hot_path);
            assert!(spec.hardware_state_migrated, "{:?}", spec.hot_path);
            assert!(spec.acceptance_eligible(), "{:?}", spec.hot_path);
            assert_eq!(spec.non_acceptance_reason(), None, "{:?}", spec.hot_path);
        }
        assert_eq!(hot_path_mask, REQUIRED_PI4_OWNER_STATE_HOT_PATH_MASK);
        assert_eq!(
            DriverTaskRuntimeRegionKind::SharedBuffer.as_str(),
            "shared-buffer"
        );
        assert_eq!(
            DRIVER_TASK_RUNTIME_TRANSPORT_REGION_MASK,
            DriverTaskRuntimeRegionKind::Code.mask_bit()
                | DriverTaskRuntimeRegionKind::Stack.mask_bit()
                | DriverTaskRuntimeRegionKind::Ipc.mask_bit()
                | DriverTaskRuntimeRegionKind::Ring.mask_bit()
        );
    }

    #[test]
    fn pi4_runtime_image_spec_lookup_is_hot_path_specific() {
        let genet = pi4_driver_task_runtime_image_spec(DriverTaskHotPath::GenetNic);
        let cyw43 = pi4_driver_task_runtime_image_spec(DriverTaskHotPath::Cyw43Wifi);
        let sdio = pi4_driver_task_runtime_image_spec(DriverTaskHotPath::SdioHost);
        let pcie = pi4_driver_task_runtime_image_spec(DriverTaskHotPath::PcieRoot);
        assert_eq!(genet.hot_path, DriverTaskHotPath::GenetNic);
        assert_eq!(cyw43.hot_path, DriverTaskHotPath::Cyw43Wifi);
        assert_eq!(sdio.hot_path, DriverTaskHotPath::SdioHost);
        assert_eq!(pcie.hot_path, DriverTaskHotPath::PcieRoot);
        assert!(genet.region_pages(DriverTaskRuntimeRegionKind::Mmio) >= 6);
        assert_eq!(cyw43.region_pages(DriverTaskRuntimeRegionKind::Mmio), 0);
        assert_eq!(sdio.region_pages(DriverTaskRuntimeRegionKind::Mmio), 1);
        assert!(pcie.region_pages(DriverTaskRuntimeRegionKind::Mmio) >= 10);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cpio_runtime_payload_lookup_accepts_uimage_wrapped_archive() {
        fn pad4(bytes: &mut Vec<u8>) {
            while bytes.len() % 4 != 0 {
                bytes.push(0);
            }
        }

        fn append_entry(bytes: &mut Vec<u8>, name: &str, data: &[u8]) {
            let namesize = name.len() + 1;
            let header = format!(
                "070701{ino:08x}{mode:08x}{uid:08x}{gid:08x}{nlink:08x}{mtime:08x}{filesize:08x}{devmajor:08x}{devminor:08x}{rdevmajor:08x}{rdevminor:08x}{namesize:08x}{check:08x}",
                ino = 1,
                mode = 0o100755,
                uid = 0,
                gid = 0,
                nlink = 1,
                mtime = 0,
                filesize = data.len(),
                devmajor = 0,
                devminor = 0,
                rdevmajor = 0,
                rdevminor = 0,
                namesize = namesize,
                check = 0,
            );
            bytes.extend_from_slice(header.as_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(0);
            pad4(bytes);
            bytes.extend_from_slice(data);
            pad4(bytes);
        }

        let mut archive = vec![0xa5; 64];
        append_entry(
            &mut archive,
            "cohesix/bin/pi4-driver-serial",
            b"serial-runtime-elf",
        );
        append_entry(&mut archive, "TRAILER!!!", &[]);

        assert_eq!(
            cpio_entry_data_with_optional_wrapper(&archive, "cohesix/bin/pi4-driver-serial"),
            Some(&b"serial-runtime-elf"[..])
        );
        assert_eq!(
            cpio_entry_data_with_optional_wrapper(&archive, "cohesix/bin/missing"),
            None
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn physical_pi_runtime_payload_lookup_requires_embedded_cpio() {
        fn pad4(bytes: &mut Vec<u8>) {
            while bytes.len() % 4 != 0 {
                bytes.push(0);
            }
        }

        fn append_entry(bytes: &mut Vec<u8>, name: &str, data: &[u8]) {
            let namesize = name.len() + 1;
            let header = format!(
                "070701{ino:08x}{mode:08x}{uid:08x}{gid:08x}{nlink:08x}{mtime:08x}{filesize:08x}{devmajor:08x}{devminor:08x}{rdevmajor:08x}{rdevminor:08x}{namesize:08x}{check:08x}",
                ino = 1,
                mode = 0o100755,
                uid = 0,
                gid = 0,
                nlink = 1,
                mtime = 0,
                filesize = data.len(),
                devmajor = 0,
                devminor = 0,
                rdevmajor = 0,
                rdevminor = 0,
                namesize = namesize,
                check = 0,
            );
            bytes.extend_from_slice(header.as_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(0);
            pad4(bytes);
            bytes.extend_from_slice(data);
            pad4(bytes);
        }

        let mut primary = Vec::new();
        append_entry(
            &mut primary,
            "cohesix/bin/pi4-driver-serial",
            b"serial-runtime-elf",
        );
        append_entry(&mut primary, "TRAILER!!!", &[]);

        let mut embedded = Vec::new();
        append_entry(
            &mut embedded,
            "cohesix/bin/pi4-driver-sdio",
            b"sdio-runtime-elf",
        );
        append_entry(&mut embedded, "TRAILER!!!", &[]);

        assert_eq!(
            driver_runtime_image_bytes_from_payloads_for_profile(
                "cohesix/bin/pi4-driver-serial",
                Some(&primary),
                Some(&embedded),
                false,
            ),
            Some(&b"serial-runtime-elf"[..])
        );
        assert_eq!(
            driver_runtime_image_bytes_from_payloads_for_profile(
                "cohesix/bin/pi4-driver-sdio",
                Some(&primary),
                Some(&embedded),
                false,
            ),
            Some(&b"sdio-runtime-elf"[..])
        );
        assert_eq!(
            driver_runtime_image_bytes_from_payloads_for_profile(
                "cohesix/bin/pi4-driver-serial",
                Some(&primary),
                Some(&embedded),
                true,
            ),
            None
        );
        assert_eq!(
            driver_runtime_image_bytes_from_payloads_for_profile(
                "cohesix/bin/pi4-driver-sdio",
                Some(&primary),
                Some(&embedded),
                true,
            ),
            Some(&b"sdio-runtime-elf"[..])
        );
        assert_eq!(
            driver_runtime_image_bytes_from_payloads_for_profile(
                "cohesix/bin/pi4-driver-missing",
                Some(&primary),
                Some(&embedded),
                true,
            ),
            None
        );
    }

    #[test]
    fn pi4_hot_path_command_catalog_is_pointer_free_and_complete() {
        assert_eq!(PI4_DRIVER_TASK_HOT_PATHS.len(), 7);
        let mut role_mask = 0usize;
        let mut saw_serial = false;
        let mut saw_usb = false;
        let mut saw_display = false;
        let mut saw_genet = false;
        let mut saw_cyw43 = false;
        let mut saw_sdio = false;
        let mut saw_pcie = false;

        for (index, hot_path) in PI4_DRIVER_TASK_HOT_PATHS.iter().copied().enumerate() {
            let contract = hot_path.contract();
            assert_eq!(contract.validate(), Ok(()), "{hot_path:?}");
            let role_bit = hot_path.role_bit();
            assert_ne!(role_bit, 0, "{hot_path:?}");
            role_mask |= role_bit;

            let budget = DriverTaskBudgetGrant::from_contract(contract);
            let frame = if hot_path == DriverTaskHotPath::HdmiText {
                DriverFrameDescriptor::new(256, 80, 0x1).unwrap()
            } else {
                DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                }
            };
            let command =
                DriverTaskCommandRecord::pi4_hot_path(index as u32 + 1, hot_path, budget, frame);
            assert_eq!(command.sequence, index as u32 + 1);
            assert_eq!(command.opcode, hot_path.opcode().as_u16());
            assert_eq!(command.arg0, hot_path.as_u32());
            assert_eq!(command.arg1, role_bit as u32);
            assert_eq!(command.budget, budget);
            assert_eq!(command.frame, frame);

            match hot_path {
                DriverTaskHotPath::SerialConsole => saw_serial = true,
                DriverTaskHotPath::UsbKeyboard => saw_usb = true,
                DriverTaskHotPath::HdmiText => saw_display = true,
                DriverTaskHotPath::GenetNic => saw_genet = true,
                DriverTaskHotPath::Cyw43Wifi => saw_cyw43 = true,
                DriverTaskHotPath::SdioHost => saw_sdio = true,
                DriverTaskHotPath::PcieRoot => saw_pcie = true,
            }
        }

        assert_eq!(
            role_mask & REQUIRED_DRIVER_TASK_ROLE_MASK,
            REQUIRED_DRIVER_TASK_ROLE_MASK
        );
        assert!(saw_serial);
        assert!(saw_usb);
        assert!(saw_display);
        assert!(saw_genet);
        assert!(saw_cyw43);
        assert!(saw_sdio);
        assert!(saw_pcie);
    }

    #[test]
    fn mixed_console_display_and_network_pressure_keeps_input_prioritized() {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        enum PressureKind {
            SerialInput,
            UsbInput,
            SerialOutput,
            HdmiOutput,
            NetworkTx,
            NetworkRx,
        }

        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        struct PressureTurn {
            sequence: u32,
            kind: PressureKind,
            hot_path: DriverTaskHotPath,
            frame_len: u16,
        }

        impl PressureTurn {
            const fn input(self) -> bool {
                matches!(
                    self.kind,
                    PressureKind::SerialInput | PressureKind::UsbInput
                )
            }

            const fn network(self) -> bool {
                matches!(self.kind, PressureKind::NetworkTx | PressureKind::NetworkRx)
            }

            fn command(self) -> DriverTaskCommandRecord {
                let frame = if self.frame_len == 0 {
                    DriverFrameDescriptor {
                        offset: 0,
                        len: 0,
                        flags: 0,
                    }
                } else {
                    DriverFrameDescriptor::new(
                        DRIVER_TASK_RING_FRAME_OFFSET as u32,
                        self.frame_len,
                        0,
                    )
                    .unwrap()
                };
                DriverTaskCommandRecord::pi4_hot_path(
                    self.sequence,
                    self.hot_path,
                    DriverTaskBudgetGrant::from_contract(self.hot_path.contract()),
                    frame,
                )
            }

            fn completion(self, command: DriverTaskCommandRecord) -> DriverTaskCompletionRecord {
                match self.kind {
                    PressureKind::SerialInput
                    | PressureKind::UsbInput
                    | PressureKind::NetworkRx => {
                        let frame = DriverFrameDescriptor::new(
                            DRIVER_TASK_RING_FRAME_OFFSET as u32,
                            self.frame_len,
                            0,
                        )
                        .unwrap();
                        DriverTaskCompletionRecord::frame_ready(command.sequence, frame)
                    }
                    PressureKind::SerialOutput
                    | PressureKind::HdmiOutput
                    | PressureKind::NetworkTx => DriverTaskCompletionRecord::progress(
                        command.sequence,
                        u32::from(self.frame_len.max(1)),
                    ),
                }
            }
        }

        const ROUNDS: usize = 1_000;
        const SERIAL_INPUT_PER_ROUND: usize = 2;
        const USB_INPUT_PER_ROUND: usize = 2;
        const SERIAL_OUTPUT_PER_ROUND: usize = 1;
        const HDMI_OUTPUT_PER_ROUND: usize = 1;
        const NET_TX_PER_ROUND: usize = 3;
        const NET_RX_PER_ROUND: usize = 3;

        let mut sequence = 1u32;
        let mut serial_input = 0usize;
        let mut usb_input = 0usize;
        let mut serial_output = 0usize;
        let mut hdmi_output = 0usize;
        let mut network_tx = 0usize;
        let mut network_rx = 0usize;
        let mut max_input_service_index = 0usize;
        let mut max_network_service_index = 0usize;

        for round in 0..ROUNDS {
            let mut pending = Vec::new();
            for _ in 0..SERIAL_INPUT_PER_ROUND {
                pending.push(PressureTurn {
                    sequence,
                    kind: PressureKind::SerialInput,
                    hot_path: DriverTaskHotPath::SerialConsole,
                    frame_len: 1,
                });
                sequence += 1;
            }
            for _ in 0..USB_INPUT_PER_ROUND {
                pending.push(PressureTurn {
                    sequence,
                    kind: PressureKind::UsbInput,
                    hot_path: DriverTaskHotPath::UsbKeyboard,
                    frame_len: 8,
                });
                sequence += 1;
            }
            for _ in 0..SERIAL_OUTPUT_PER_ROUND {
                pending.push(PressureTurn {
                    sequence,
                    kind: PressureKind::SerialOutput,
                    hot_path: DriverTaskHotPath::SerialConsole,
                    frame_len: 96,
                });
                sequence += 1;
            }
            for _ in 0..HDMI_OUTPUT_PER_ROUND {
                pending.push(PressureTurn {
                    sequence,
                    kind: PressureKind::HdmiOutput,
                    hot_path: DriverTaskHotPath::HdmiText,
                    frame_len: 160,
                });
                sequence += 1;
            }
            for _ in 0..NET_TX_PER_ROUND {
                pending.push(PressureTurn {
                    sequence,
                    kind: PressureKind::NetworkTx,
                    hot_path: if round % 2 == 0 {
                        DriverTaskHotPath::GenetNic
                    } else {
                        DriverTaskHotPath::Cyw43Wifi
                    },
                    frame_len: 512,
                });
                sequence += 1;
            }
            for _ in 0..NET_RX_PER_ROUND {
                pending.push(PressureTurn {
                    sequence,
                    kind: PressureKind::NetworkRx,
                    hot_path: if round % 2 == 0 {
                        DriverTaskHotPath::Cyw43Wifi
                    } else {
                        DriverTaskHotPath::GenetNic
                    },
                    frame_len: 384,
                });
                sequence += 1;
            }

            pending.sort_by_key(|turn| (turn.hot_path.contract().service_order(), turn.sequence));

            let last_input_index = pending.iter().rposition(|turn| turn.input()).unwrap();
            let first_network_index = pending.iter().position(|turn| turn.network()).unwrap();
            assert!(last_input_index < first_network_index);

            for (service_index, turn) in pending.into_iter().enumerate() {
                let command = turn.command();
                assert_eq!(command.sequence, turn.sequence);
                assert_eq!(command.opcode, turn.hot_path.opcode().as_u16());
                assert_eq!(command.arg0, turn.hot_path.as_u32());
                assert_eq!(command.arg1, turn.hot_path.role_bit() as u32);
                assert!(command.owner_state_credit_eligible());
                if command.frame.len != 0 {
                    let offset = command.frame.offset as usize;
                    let len = command.frame.len as usize;
                    assert!(offset >= DRIVER_TASK_RING_FRAME_OFFSET);
                    assert!(len <= MAX_DRIVER_TASK_FRAME_BYTES);
                    assert!(offset + len <= DRIVER_TASK_RING_PAGE_BYTES);
                }

                let completion = turn.completion(command);
                assert_eq!(completion.sequence, command.sequence);
                assert_ne!(completion.code, DriverTaskCompletionCode::Fault.as_u16());
                assert_ne!(
                    completion.code,
                    DriverTaskCompletionCode::BudgetExhausted.as_u16()
                );
                if completion.code == DriverTaskCompletionCode::FrameReady.as_u16() {
                    assert_ne!(completion.frame.len, 0);
                    assert_eq!(completion.result, u32::from(completion.frame.len));
                    assert!(!completion.frame.root_context_non_acceptance());
                } else {
                    assert_eq!(completion.code, DriverTaskCompletionCode::Progress.as_u16());
                    assert_ne!(completion.result, 0);
                }

                if turn.input() {
                    max_input_service_index = max_input_service_index.max(service_index);
                }
                if turn.network() {
                    max_network_service_index = max_network_service_index.max(service_index);
                }

                match turn.kind {
                    PressureKind::SerialInput => serial_input += 1,
                    PressureKind::UsbInput => usb_input += 1,
                    PressureKind::SerialOutput => serial_output += 1,
                    PressureKind::HdmiOutput => hdmi_output += 1,
                    PressureKind::NetworkTx => network_tx += 1,
                    PressureKind::NetworkRx => network_rx += 1,
                }
            }
        }

        assert_eq!(serial_input, ROUNDS * SERIAL_INPUT_PER_ROUND);
        assert_eq!(usb_input, ROUNDS * USB_INPUT_PER_ROUND);
        assert_eq!(serial_output, ROUNDS * SERIAL_OUTPUT_PER_ROUND);
        assert_eq!(hdmi_output, ROUNDS * HDMI_OUTPUT_PER_ROUND);
        assert_eq!(network_tx, ROUNDS * NET_TX_PER_ROUND);
        assert_eq!(network_rx, ROUNDS * NET_RX_PER_ROUND);
        assert!(max_input_service_index < max_network_service_index);
        assert!(max_input_service_index < SERIAL_INPUT_PER_ROUND + USB_INPUT_PER_ROUND);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn sdio_and_pcie_bus_ring_handlers_are_pointer_free_and_fail_closed() {
        for hot_path in [DriverTaskHotPath::SdioHost, DriverTaskHotPath::PcieRoot] {
            let command = DriverTaskCommandRecord::pi4_hot_path(
                hot_path.as_u32(),
                hot_path,
                DriverTaskBudgetGrant::from_contract(hot_path.contract()),
                DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            );

            let completion =
                unsafe { pi4_bus_ring_service_driver_task(hot_path.as_u32() as usize, command) };
            assert_eq!(completion.sequence, hot_path.as_u32());
            assert_eq!(completion.code, DriverTaskCompletionCode::Idle.as_u16());
            assert_eq!(completion.result, 0);

            let bad_context = unsafe {
                pi4_bus_ring_service_driver_task(
                    DriverTaskHotPath::GenetNic.as_u32() as usize,
                    command,
                )
            };
            assert_eq!(bad_context.code, DriverTaskCompletionCode::Fault.as_u16());
            assert_eq!(
                bad_context.detail,
                DriverTaskFaultCode::RejectedCommand.as_u16()
            );

            let bad_command = DriverTaskCommandRecord::flush(
                42,
                DriverTaskBudgetGrant::from_contract(hot_path.contract()),
            );
            let completion = unsafe {
                pi4_bus_ring_service_driver_task(hot_path.as_u32() as usize, bad_command)
            };
            assert_eq!(completion.sequence, 42);
            assert_eq!(completion.code, DriverTaskCompletionCode::Fault.as_u16());
            assert_eq!(
                completion.detail,
                DriverTaskFaultCode::RejectedCommand.as_u16()
            );
        }
    }

    #[test]
    fn callback_pointer_services_do_not_credit_strong_hot_paths() {
        assert!(!driver_task_service_counts_as_hot_path(
            DriverTaskIsolation::RootTaskCompatibility
        ));
        assert!(driver_task_service_counts_as_hot_path(
            DriverTaskIsolation::DedicatedSeL4Task
        ));
        assert!(!CURRENT_DRIVER_TASK_IPC_ABI.is_pointer_free());
    }

    #[test]
    fn pi4_hardware_profile_disallows_steady_state_compatibility_paths() {
        assert_eq!(
            DriverTaskRuntimeProfile::Pi4Hardware.as_str(),
            "pi4-hardware"
        );
        assert!(!callback_dispatch_allowed_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware
        ));
        assert!(!root_fallback_allowed_for_profile(
            DriverTaskRuntimeProfile::Pi4Hardware
        ));
        assert!(callback_dispatch_allowed_for_profile(
            DriverTaskRuntimeProfile::QemuCompatibility
        ));
        assert!(root_fallback_allowed_for_profile(
            DriverTaskRuntimeProfile::QemuCompatibility
        ));
        assert!(callback_dispatch_allowed_for_profile(
            DriverTaskRuntimeProfile::HostTest
        ));
        assert!(root_fallback_allowed_for_profile(
            DriverTaskRuntimeProfile::HostTest
        ));
    }

    #[test]
    fn invalid_contracts_explain_rejection() {
        let mut invalid = SERIAL_DRIVER_TASK_CONTRACT;
        invalid.queue_depth = 0;
        let err = invalid.validate().unwrap_err();
        assert_eq!(err.reason(), "driver-task-contract-zero-queue-depth");

        invalid = SERIAL_DRIVER_TASK_CONTRACT;
        invalid.queue_depth = MAX_DRIVER_TASK_QUEUE_DEPTH + 1;
        let err = invalid.validate().unwrap_err();
        assert_eq!(err.reason(), "driver-task-contract-queue-depth-too-large");

        invalid = SERIAL_DRIVER_TASK_CONTRACT;
        invalid.budget.allow_blocking_waits = true;
        invalid.budget.max_blocking_spins = 0;
        let err = invalid.validate().unwrap_err();
        assert_eq!(err.reason(), "driver-task-contract-unbounded-blocking-wait");

        invalid = SERIAL_DRIVER_TASK_CONTRACT;
        invalid.budget.allow_blocking_waits = true;
        invalid.budget.max_blocking_spins = 1;
        let err = invalid.validate().unwrap_err();
        assert_eq!(
            err.reason(),
            "driver-task-contract-blocking-wait-not-admitted-for-class"
        );

        invalid = SERIAL_DRIVER_TASK_CONTRACT;
        invalid.authority = DriverTaskAuthority::NetworkFrameTransport;
        let err = invalid.validate().unwrap_err();
        assert_eq!(err.reason(), "driver-task-contract-invalid-authority");

        invalid = USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT;
        invalid.class = DriverTaskClass::NetworkData;
        let err = invalid.validate().unwrap_err();
        assert_eq!(err.reason(), "driver-task-contract-invalid-class");
    }
}
