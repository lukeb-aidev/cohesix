// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define HAL-enforced hardware driver task scheduling contracts.
// Author: Lukas Bower

//! Scheduling contracts for hardware drivers.
//!
//! These contracts are the HAL-facing bridge between the current direct
//! root-task compatibility path and the Milestone 26a/26b dedicated seL4
//! driver-task model. Drivers must declare the contract they consume before
//! runtime code may service them.

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

/// Maximum bounded IPC/event queue admitted by the HAL contract layer.
pub const MAX_DRIVER_TASK_QUEUE_DEPTH: u16 = 256;

/// Number of active hardware driver roles required before reopened Pi 4
/// acceptance may claim dedicated driver-task isolation.
pub const MIN_DEDICATED_PI4_DRIVER_TASKS: usize = 4;

/// Maximum Ethernet-sized frame admitted through a dedicated driver-task ring.
pub const MAX_DRIVER_TASK_FRAME_BYTES: usize = 1536;

/// Current as-built state of the seL4 driver-task creation substrate.
///
/// This remains `false` until root-task can create a separate TCB, install its
/// IPC buffer, set fault handling, grant only declared device caps, and revoke
/// it without running driver hot paths in root.
pub const DEDICATED_DRIVER_TASK_SUBSTRATE_READY: bool = false;

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
    Fault(&'static str),
}

/// Bounded no-alloc ring used at the driver-task IPC boundary.
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
    isolation: DriverTaskIsolation::RootTaskCompatibility,
    budget: DriverTaskBudget::preemptible(64, 512, 64),
    queue_depth: 64,
};

/// Local USB keyboard driver-task contract.
pub const USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "usb-local-seat",
    kind: DriverTaskKind::LocalSeatUsb,
    class: DriverTaskClass::RealtimeInput,
    authority: DriverTaskAuthority::DeviceOnly,
    isolation: DriverTaskIsolation::RootTaskCompatibility,
    budget: DriverTaskBudget::preemptible(256, 4096, 128),
    queue_depth: 128,
};

/// HDMI text sink driver-task contract.
pub const HDMI_TEXT_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "hdmi-text",
    kind: DriverTaskKind::HdmiText,
    class: DriverTaskClass::DisplayRefresh,
    authority: DriverTaskAuthority::DisplaySink,
    isolation: DriverTaskIsolation::RootTaskCompatibility,
    budget: DriverTaskBudget::preemptible(64, 4096, 64),
    queue_depth: 64,
};

/// GENET wired NIC driver-task contract.
pub const GENET_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "bcmgenet-v5",
    kind: DriverTaskKind::WiredNic,
    class: DriverTaskClass::NetworkData,
    authority: DriverTaskAuthority::NetworkFrameTransport,
    isolation: DriverTaskIsolation::RootTaskCompatibility,
    budget: DriverTaskBudget::preemptible(256, 131_072, 128),
    queue_depth: 128,
};

/// CYW43 Wi-Fi NIC driver-task contract.
pub const CYW43_WIFI_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "cyw43455",
    kind: DriverTaskKind::WifiNic,
    class: DriverTaskClass::NetworkData,
    authority: DriverTaskAuthority::NetworkFrameTransport,
    isolation: DriverTaskIsolation::RootTaskCompatibility,
    budget: DriverTaskBudget::preemptible(192, 65_536, 64),
    queue_depth: 128,
};

/// QEMU RTL8139 compatibility NIC contract.
pub const RTL8139_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "rtl8139",
    kind: DriverTaskKind::VirtualNic,
    class: DriverTaskClass::NetworkData,
    authority: DriverTaskAuthority::NetworkFrameTransport,
    isolation: DriverTaskIsolation::RootTaskCompatibility,
    budget: DriverTaskBudget::preemptible(128, 65_536, 64),
    queue_depth: 64,
};

/// QEMU virtio compatibility NIC contract.
pub const VIRTIO_NET_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "virtio-net",
    kind: DriverTaskKind::VirtualNic,
    class: DriverTaskClass::NetworkData,
    authority: DriverTaskAuthority::NetworkFrameTransport,
    isolation: DriverTaskIsolation::RootTaskCompatibility,
    budget: DriverTaskBudget::preemptible(256, 131_072, 128),
    queue_depth: 128,
};

/// SDIO host driver-task contract beneath CYW43.
pub const SDIO_HOST_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "sdio-host",
    kind: DriverTaskKind::SdioHost,
    class: DriverTaskClass::NetworkControl,
    authority: DriverTaskAuthority::DeviceOnly,
    isolation: DriverTaskIsolation::RootTaskCompatibility,
    budget: DriverTaskBudget::preemptible(256, 65_536, 64),
    queue_depth: 64,
};

/// PCIe root driver-task contract beneath VL805 and PCI NICs.
pub const PCIE_ROOT_DRIVER_TASK_CONTRACT: DriverTaskContract = DriverTaskContract {
    name: "pcie-root",
    kind: DriverTaskKind::PcieRoot,
    class: DriverTaskClass::NetworkControl,
    authority: DriverTaskAuthority::DeviceOnly,
    isolation: DriverTaskIsolation::RootTaskCompatibility,
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
    DEDICATED_DRIVER_TASK_SUBSTRATE_READY
        && summary.dedicated_sel4_tasks >= MIN_DEDICATED_PI4_DRIVER_TASKS
        && summary.root_task_compatibility == 0
}

/// Emit compact scheduling-contract proof breadcrumbs for Pi 4 gate tooling.
#[cfg(feature = "kernel")]
pub fn emit_boot_contract_proof() {
    use core::fmt::Write;

    use heapless::String;

    for contract in BUILTIN_DRIVER_TASK_CONTRACTS {
        let mut line = String::<256>::new();
        let status = if contract.validate().is_ok() {
            "valid"
        } else {
            "invalid"
        };
        let _ = write!(
            line,
            "SCHED_CONTRACT contract={} status={} service_class={} isolation={} priority={} service_order={} max_ops={} max_bytes={} max_frames={} max_service_us={}",
            contract.name,
            status,
            contract.class.as_str(),
            contract.isolation.as_str(),
            contract.sel4_priority(),
            contract.service_order(),
            contract.budget.max_ops_per_turn,
            contract.budget.max_bytes_per_turn,
            contract.budget.max_frames_per_turn,
            contract.max_service_us(),
        );
        crate::bootstrap::log::force_uart_line(line.as_str());
    }

    let summary = builtin_isolation_summary();
    let mut line = String::<160>::new();
    let _ = write!(
        line,
        "DRIVER_TASK_SUMMARY contracts={} dedicated={} compatibility={}",
        summary.contracts, summary.dedicated_sel4_tasks, summary.root_task_compatibility,
    );
    crate::bootstrap::log::force_uart_line(line.as_str());

    let mut line = String::<192>::new();
    let ready = dedicated_driver_task_acceptance_ready();
    let reason = if ready {
        "dedicated-sel4-substrate-active"
    } else if !DEDICATED_DRIVER_TASK_SUBSTRATE_READY {
        "dedicated-sel4-substrate-not-active"
    } else if summary.root_task_compatibility != 0 {
        "root-task-compatibility-contracts-active"
    } else {
        "insufficient-dedicated-driver-tasks"
    };
    let _ = write!(
        line,
        "DRIVER_TASK_ACCEPTANCE dedicated_ready={} reason={} required={} dedicated={} compatibility={}",
        if ready { "yes" } else { "no" },
        reason,
        MIN_DEDICATED_PI4_DRIVER_TASKS,
        summary.dedicated_sel4_tasks,
        summary.root_task_compatibility,
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_driver_task_contracts_are_valid_and_mark_current_compatibility() {
        for contract in BUILTIN_DRIVER_TASK_CONTRACTS {
            assert_eq!(contract.validate(), Ok(()), "{contract:?}");
            assert_eq!(
                contract.isolation,
                DriverTaskIsolation::RootTaskCompatibility
            );
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
    fn builtin_isolation_summary_does_not_fake_dedicated_tasks() {
        let summary = builtin_isolation_summary();
        assert_eq!(summary.contracts, BUILTIN_DRIVER_TASK_CONTRACTS.len());
        assert_eq!(summary.dedicated_sel4_tasks, 0);
        assert_eq!(
            summary.root_task_compatibility,
            BUILTIN_DRIVER_TASK_CONTRACTS.len()
        );
        assert!(!DEDICATED_DRIVER_TASK_SUBSTRATE_READY);
        assert!(!dedicated_driver_task_acceptance_ready());
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

        invalid = SERIAL_DRIVER_TASK_CONTRACT;
        invalid.isolation = DriverTaskIsolation::DedicatedSeL4Task;
        let err = invalid.validate().unwrap_err();
        assert_eq!(
            err.reason(),
            "driver-task-contract-dedicated-substrate-not-ready"
        );
    }
}
