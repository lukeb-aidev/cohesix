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
use core::sync::atomic::{AtomicUsize, Ordering};

use heapless::Deque;

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
    /// Whether driver TCBs run in isolated driver VSpaces.
    pub vspace_proof: bool,
    /// Whether driver service turns use pointer-free shared command rings.
    pub pointer_free_ipc_proof: bool,
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
/// proves isolated driver VSpaces and pointer-free IPC; otherwise it remains a
/// distinct shared-ring diagnostic that does not satisfy acceptance.
#[cfg(feature = "kernel")]
pub fn record_driver_task_ring_hot_path(contract: DriverTaskContract) {
    let role_bit = driver_task_role_bit(contract.kind);
    if role_bit == 0 {
        return;
    }
    DRIVER_TASK_SHARED_RING_SERVICE_ROLE_MASK.fetch_or(role_bit, Ordering::AcqRel);
    if DRIVER_TASK_VSPACE_PROOF.load(Ordering::Acquire) != 0
        && DRIVER_TASK_POINTER_FREE_IPC_PROOF.load(Ordering::Acquire) != 0
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
    add x11, x9, {completion_offset}
    str w10, [x11]
    mov w12, {completion_code}
    strh w12, [x11, #4]
    strh wzr, [x11, #6]
    str w20, [x11, #8]
    str xzr, [x11, #12]

    b 1b
    .size cohesix_driver_task_isolated_entry, . - cohesix_driver_task_isolated_entry
    "#,
    child_command_slot = const DRIVER_TASK_CHILD_COMMAND_SLOT,
    completion_code = const DriverTaskCompletionCode::Progress as u16,
    completion_offset = const DRIVER_TASK_RING_COMPLETION_OFFSET,
    ring_vaddr = const DRIVER_TASK_RING_VADDR,
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

/// Number of active hardware driver roles required before reopened Pi 4
/// acceptance may claim dedicated driver-task isolation.
pub const MIN_DEDICATED_PI4_DRIVER_TASKS: usize = 6;

/// Maximum Ethernet-sized frame admitted through a dedicated driver-task ring.
pub const MAX_DRIVER_TASK_FRAME_BYTES: usize = 1536;

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

/// Fixed driver-local virtual address for the root/driver command page.
pub const DRIVER_TASK_RING_VADDR: usize = 0x7000_0000;

/// Fixed driver-local virtual address for the seL4 IPC buffer page.
pub const DRIVER_TASK_IPC_VADDR: usize = 0x7000_1000;

/// Fixed driver-local virtual address for the bottom of the trampoline stack.
pub const DRIVER_TASK_STACK_BOTTOM_VADDR: usize = 0x7000_2000;

/// Fixed driver-local virtual address for the top of the trampoline stack.
pub const DRIVER_TASK_STACK_TOP_VADDR: usize = 0x7000_3000;

/// Offset of the first fixed-layout completion record within the ring page.
pub const DRIVER_TASK_RING_COMPLETION_OFFSET: usize = 64;

/// Offset of the role-owned shared payload area within the ring page.
pub const DRIVER_TASK_RING_FRAME_OFFSET: usize = 256;

/// One page is enough for the current smoke command and completion records.
pub const DRIVER_TASK_RING_PAGE_BYTES: usize = 4096;

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

#[cfg(feature = "kernel")]
struct DriverTaskCommandSlot {
    endpoint: AtomicUsize,
    ring_root_ptr: AtomicUsize,
    request_seq: AtomicUsize,
    active: AtomicUsize,
    ring_handler: AtomicUsize,
    ring_context: AtomicUsize,
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
            endpoint: AtomicUsize::new(0),
            ring_root_ptr: AtomicUsize::new(0),
            request_seq: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            ring_handler: AtomicUsize::new(0),
            ring_context: AtomicUsize::new(0),
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

/// Register the driver-owned state machine for fixed shared-ring service turns.
#[cfg(feature = "kernel")]
pub fn register_driver_task_ring_service(
    contract: DriverTaskContract,
    context: usize,
    handler: DriverTaskRingServiceHandler,
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
    true
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
    register_driver_task_ring_service(
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

/// Execute a fixed-layout command over the pointer-free shared-ring ABI.
///
/// This transport is intentionally narrower than the transitional callback
/// service path. It is used by the isolated QEMU smoke task to prove the ABI
/// mechanics without crediting a hardware hot path until the driver state has
/// moved behind that ring.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_command(
    contract: DriverTaskContract,
    mut command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    let task_key = driver_task_contract_key(contract)?;
    let slot = slot_for_task_key(task_key)?;
    let endpoint = slot.endpoint.load(Ordering::Acquire);
    let ring_root_ptr = slot.ring_root_ptr.load(Ordering::Acquire);
    if endpoint == 0 || ring_root_ptr == 0 {
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
    command.sequence = request as u32;
    let completion_reset =
        DriverTaskCompletionRecord::fault(0, DriverTaskFaultCode::RejectedCommand);
    let command_ptr = ring_root_ptr as *mut DriverTaskCommandRecord;
    let completion_ptr =
        (ring_root_ptr + DRIVER_TASK_RING_COMPLETION_OFFSET) as *mut DriverTaskCompletionRecord;

    // SAFETY: `ring_root_ptr` is the root mapping of one HAL-owned frame that
    // was also mapped into the driver VSpace at `DRIVER_TASK_RING_VADDR`. The
    // fixed records are page-local, primitive-only, and naturally aligned.
    unsafe {
        core::ptr::write_volatile(completion_ptr, completion_reset);
        core::ptr::write_volatile(command_ptr, command);
        sel4_sys::seL4_SetMR(0, request as sel4_sys::seL4_Word);
    }

    let info = sel4_sys::seL4_MessageInfo::new(0, 0, 0, 1);
    let mut completion = completion_reset;
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

    slot.active.store(0, Ordering::Release);
    (completion.sequence == request as u32).then_some(completion)
}

/// Execute one registered driver service turn through the shared-ring ABI.
#[cfg(feature = "kernel")]
pub fn run_driver_task_ring_service(
    contract: DriverTaskContract,
    command: DriverTaskCommandRecord,
) -> Option<DriverTaskCompletionRecord> {
    let completion = run_driver_task_ring_command(contract, command)?;
    if completion.code != DriverTaskCompletionCode::Fault.as_u16() {
        record_driver_task_ring_hot_path(contract);
    }
    Some(completion)
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

    // SAFETY: `register_driver_task_ring_service` stores only function pointers
    // with the exact `DriverTaskRingServiceHandler` ABI. The integer round trip
    // keeps the slot atomically publishable to the service TCB.
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
    budget: DriverTaskBudget::preemptible(64, 512, 64),
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
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
    HDMI_TEXT_DRIVER_TASK_CONTRACT,
    GENET_DRIVER_TASK_CONTRACT,
    CYW43_WIFI_DRIVER_TASK_CONTRACT,
    RTL8139_DRIVER_TASK_CONTRACT,
    VIRTIO_NET_DRIVER_TASK_CONTRACT,
    SDIO_HOST_DRIVER_TASK_CONTRACT,
    PCIE_ROOT_DRIVER_TASK_CONTRACT,
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
        && proof.broad_caps_leaked == 0
        && proof.live_tcb_role_mask & REQUIRED_DRIVER_TASK_ROLE_MASK
            == REQUIRED_DRIVER_TASK_ROLE_MASK
        && proof.hot_path_role_mask & REQUIRED_DRIVER_TASK_ROLE_MASK
            == REQUIRED_DRIVER_TASK_ROLE_MASK
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
        if proof.hot_path_role_mask & REQUIRED_DRIVER_TASK_ROLE_MASK
            == REQUIRED_DRIVER_TASK_ROLE_MASK
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
        "DRIVER_TASK_SUBSTRATE active={} profile=pi4-uboot-aarch64 task_count={} failed_count={} live_tcb_count={} root_authority_retained=yes fault_endpoint_ready={} revoke_ready={} broad_caps_leaked={} sched={} affinity={} affinity_configured={} affinity_applied={} vspace={} ipc_abi={} pointer_free_ipc={} live_hot_paths={}",
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
        if proof.hot_path_role_mask & REQUIRED_DRIVER_TASK_ROLE_MASK
            == REQUIRED_DRIVER_TASK_ROLE_MASK
        {
            "yes"
        } else {
            "no"
        },
    );
    crate::bootstrap::log::force_uart_line(line.as_str());

    for contract in BUILTIN_DRIVER_TASK_CONTRACTS {
        let mut line = String::<320>::new();
        let status = if contract.validate().is_ok() {
            "valid"
        } else {
            "invalid"
        };
        let role_bit = driver_task_role_bit(contract.kind);
        let live_tcb = role_bit != 0 && proof.live_tcb_role_mask & role_bit != 0;
        let hot_path = role_bit != 0 && proof.hot_path_role_mask & role_bit != 0;
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
    let mut line = String::<256>::new();
    let _ = write!(
        line,
        "DRIVER_TASK_SUMMARY contracts={} requested_dedicated={} dedicated={} compatibility={} live_tcb_roles=0x{:x} hot_path_roles=0x{:x} shared_ring_roles=0x{:x} compatibility_roles=0x{:x}",
        summary.contracts,
        summary.requested_dedicated_sel4_tasks,
        summary.dedicated_sel4_tasks,
        summary.root_task_compatibility,
        proof.live_tcb_role_mask,
        proof.hot_path_role_mask,
        proof.shared_ring_service_role_mask,
        proof.compatibility_service_role_mask,
    );
    crate::bootstrap::log::force_uart_line(line.as_str());

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
    } else if proof.hot_path_role_mask & REQUIRED_DRIVER_TASK_ROLE_MASK
        != REQUIRED_DRIVER_TASK_ROLE_MASK
    {
        DEDICATED_DRIVER_TASK_LIVE_HOT_PATHS_MISSING
    } else if summary.root_task_compatibility != 0 {
        "root-task-compatibility-contracts-active"
    } else {
        "insufficient-dedicated-driver-tasks"
    };
    let _ = write!(
        line,
        "DRIVER_TASK_ACCEPTANCE dedicated_ready={} reason={} required={} dedicated={} compatibility={} substrate={} capset={} fault={} revoke={} sched={} affinity={} vspace={} ipc_abi={} pointer_free_ipc={} live_tcb_roles=0x{:x} hot_path_roles=0x{:x} compatibility_roles=0x{:x}",
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
        proof.live_tcb_role_mask,
        proof.hot_path_role_mask,
        proof.compatibility_service_role_mask,
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn isolated_vspace_still_requires_pointer_free_ipc_for_acceptance() {
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
            broad_caps_leaked: 0,
        };
        assert!(!driver_task_acceptance_ready_for(summary, proof));

        let proof = DriverTaskRuntimeProof {
            pointer_free_ipc_proof: true,
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
    fn service_budget_fails_closed_on_exhaustion() {
        let mut budget = DriverServiceBudget::new(SERIAL_DRIVER_TASK_CONTRACT).unwrap();
        assert_eq!(budget.charge_ops(64), Ok(()));
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
            DRIVER_TASK_RING_FRAME_OFFSET + MAX_DRIVER_TASK_FRAME_BYTES
                <= DRIVER_TASK_RING_PAGE_BYTES
        );
        assert_eq!(DRIVER_TASK_RING_VADDR & 0xfff, 0);
        assert_eq!(DRIVER_TASK_IPC_VADDR & 0xfff, 0);
        assert_eq!(DRIVER_TASK_STACK_BOTTOM_VADDR & 0xfff, 0);
        assert_eq!(
            DRIVER_TASK_STACK_TOP_VADDR - DRIVER_TASK_STACK_BOTTOM_VADDR,
            4096
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
