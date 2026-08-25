// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Run isolated Workers over the sealed passive Call/Reply Worker ABI.
// Author: Lukas Bower

//! Shared seL4 runtime for the Heartbeat, GPU, and LoRA Worker images.

#[cfg(not(target_arch = "aarch64"))]
compile_error!("isolated Worker target runtime requires AArch64");

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicBool, AtomicU64, AtomicUsize, Ordering};

use worker_task_abi::{
    GpuLeaseReceiptRecord, PeftReceiptRecord, WorkerAction, WorkerCallOperation,
    WorkerCompletionRecord, WorkerCompletionStatus, WorkerControlRecord, WorkerReadyRecord,
    WorkerRole, WorkerRuntimeInit, WorkerSharedPage, WORKER_CALL_SUCCESS_LABEL,
    WORKER_SHARED_PAGE_ALIGNMENT,
};

const READY_SEQUENCE: u64 = 1;

static SHARED_PAGE_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static SERVICE_ENDPOINT_SLOT: AtomicUsize = AtomicUsize::new(0);
static SERVICE_REPLY_SLOT: AtomicUsize = AtomicUsize::new(0);
static SUPERVISOR_WAKE_NOTIFICATION_SLOT: AtomicUsize = AtomicUsize::new(0);
static LAST_CONTROL_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static CURRENT_CONTROL_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static PANIC_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Stable external-QEMU evidence hook reached on every admitted control turn.
///
/// The hook has no authority and is present only in explicitly instrumented
/// QEMU Worker images. GDB may break here before redirecting the same child to
/// its existing standard-fault path.
#[cfg(feature = "qemu-evidence")]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_worker_qemu_evidence_control_handler() {
    core::hint::black_box(cohesix_worker_qemu_evidence_standard_fault as *const ());
    core::hint::black_box(cohesix_worker_qemu_evidence_timeout_spin as *const ());
    core::hint::black_box(());
}

/// Stable external-QEMU target for a standard Worker fault injection.
#[cfg(feature = "qemu-evidence")]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_worker_qemu_evidence_standard_fault() -> ! {
    enter_standard_fault()
}

/// Stable external-QEMU target that exhausts the admitted MCS budget.
#[cfg(feature = "qemu-evidence")]
#[inline(never)]
#[no_mangle]
pub extern "C" fn cohesix_worker_qemu_evidence_timeout_spin() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Enter one isolated Worker using the shared page address in the first entry register.
///
/// The supervisor must map one [`WorkerSharedPage`] and the descriptor-declared
/// IPC buffer before resuming the child. Normal work arrives only through the
/// generated receive endpoint and single-owner Reply object; the send-only
/// notification is a bounded READY/fault scheduling hint.
pub fn run(expected_role: WorkerRole, shared_page_address: usize) -> ! {
    let page = match validate_shared_page_address(shared_page_address) {
        Some(page) => page,
        None => enter_standard_fault(),
    };
    let init = match read_stable_init(page) {
        Some(init) if init.validate_for_role(expected_role).is_ok() => init,
        _ => enter_standard_fault(),
    };

    install_ipc_buffer(init.ipc_buffer_vaddr);
    install_panic_context(page, init);
    clear_worker_publications(page);
    publish_ready(page, init);

    let mut last_sequence = 0u64;
    let mut badge = 0;
    let mut message_registers = [0; 4];
    let mut tag = signal_supervisor_and_receive(init, &mut badge, &mut message_registers);
    loop {
        let operation = match init.call_labels.classify(tag.label()) {
            Ok(operation) => operation,
            Err(_) => publish_fault_and_trap(
                page,
                init,
                next_sequence(last_sequence),
                WorkerCompletionStatus::InvalidControl,
            ),
        };
        let sequence = match validate_call(init, badge, tag, message_registers) {
            Some(sequence) => sequence,
            None => publish_fault_and_trap(
                page,
                init,
                next_sequence(last_sequence),
                WorkerCompletionStatus::InvalidControl,
            ),
        };
        let Some(expected_sequence) = last_sequence.checked_add(1) else {
            publish_fault_and_trap(
                page,
                init,
                last_sequence,
                WorkerCompletionStatus::InvalidControl,
            );
        };
        if sequence != expected_sequence {
            publish_fault_and_trap(
                page,
                init,
                expected_sequence,
                WorkerCompletionStatus::InvalidControl,
            );
        }
        CURRENT_CONTROL_SEQUENCE.store(sequence, Ordering::Release);
        let (status, terminal) = match operation {
            WorkerCallOperation::Control => {
                let Some(control) = read_stable_control(page) else {
                    publish_fault_and_trap(
                        page,
                        init,
                        sequence,
                        WorkerCompletionStatus::InvalidControl,
                    );
                };
                if control.sequence != sequence || control.validate_for(init).is_err() {
                    publish_fault_and_trap(
                        page,
                        init,
                        sequence,
                        WorkerCompletionStatus::InvalidControl,
                    );
                }
                (process_control(page, init, control), false)
            }
            WorkerCallOperation::Shutdown => {
                publish_completion(
                    page,
                    WorkerCompletionRecord::staged_terminal(
                        sequence,
                        init.identity,
                        WorkerCompletionStatus::Shutdown,
                    ),
                );
                (WorkerCompletionStatus::Shutdown, true)
            }
            WorkerCallOperation::Revoke => {
                publish_completion(
                    page,
                    WorkerCompletionRecord::staged_terminal(
                        sequence,
                        init.identity,
                        WorkerCompletionStatus::Revoked,
                    ),
                );
                (WorkerCompletionStatus::Revoked, true)
            }
        };
        last_sequence = sequence;
        LAST_CONTROL_SEQUENCE.store(last_sequence, Ordering::Release);
        CURRENT_CONTROL_SEQUENCE.store(0, Ordering::Release);
        message_registers = [
            sequence as sel4_sys::seL4_Word,
            status as sel4_sys::seL4_Word,
            init.identity.supervisor_generation as sel4_sys::seL4_Word,
            init.identity.cap_generation as sel4_sys::seL4_Word,
        ];
        tag = reply_receive(init, &mut badge, &mut message_registers);
        if terminal {
            publish_fault_and_trap(
                page,
                init,
                next_sequence(last_sequence),
                WorkerCompletionStatus::InvalidControl,
            );
        }
    }
}

/// Publish a bounded panic completion, wake the supervisor, and fault the child.
///
/// The panic path never resumes service. A second panic skips shared-memory
/// publication and immediately enters the standard seL4 fault path.
pub fn contain_panic() -> ! {
    if PANIC_ACTIVE.swap(true, Ordering::AcqRel) {
        enter_standard_fault();
    }
    let page_address = SHARED_PAGE_ADDRESS.load(Ordering::Acquire);
    let endpoint_slot = SERVICE_ENDPOINT_SLOT.load(Ordering::Acquire);
    let reply_slot = SERVICE_REPLY_SLOT.load(Ordering::Acquire);
    let wake_slot = SUPERVISOR_WAKE_NOTIFICATION_SLOT.load(Ordering::Acquire);
    if page_address != 0 && endpoint_slot != 0 && reply_slot != 0 && wake_slot != 0 {
        let page = page_address as *mut WorkerSharedPage;
        let Some(init) = read_stable_init(page) else {
            enter_standard_fault();
        };
        let current = CURRENT_CONTROL_SEQUENCE.load(Ordering::Acquire);
        let last = LAST_CONTROL_SEQUENCE.load(Ordering::Acquire);
        let sequence = if current == 0 {
            next_sequence(last)
        } else {
            current
        };
        publish_completion(
            page,
            WorkerCompletionRecord::staged_terminal(
                sequence,
                init.identity,
                WorkerCompletionStatus::Panic,
            ),
        );
        signal_slot(wake_slot);
    }
    enter_standard_fault()
}

fn validate_shared_page_address(address: usize) -> Option<*mut WorkerSharedPage> {
    if address == 0 || !address.is_multiple_of(WORKER_SHARED_PAGE_ALIGNMENT) {
        None
    } else {
        Some(address as *mut WorkerSharedPage)
    }
}

fn install_ipc_buffer(address: u64) {
    // SAFETY: `WorkerRuntimeInit::validate_for_role` proves a nonzero,
    // ABI-aligned address. The supervisor maps the child-owned IPC-buffer frame
    // at that exact address before resume and retains the mapping for the TCB.
    unsafe {
        sel4_sys::seL4_SetIPCBuffer(address as *mut sel4_sys::seL4_IPCBuffer);
    }
}

fn install_panic_context(page: *mut WorkerSharedPage, init: WorkerRuntimeInit) {
    SHARED_PAGE_ADDRESS.store(page as usize, Ordering::Release);
    SERVICE_ENDPOINT_SLOT.store(init.service_endpoint_slot as usize, Ordering::Release);
    SERVICE_REPLY_SLOT.store(init.service_reply_slot as usize, Ordering::Release);
    SUPERVISOR_WAKE_NOTIFICATION_SLOT.store(
        init.supervisor_wake_notification_slot as usize,
        Ordering::Release,
    );
    LAST_CONTROL_SEQUENCE.store(0, Ordering::Release);
    CURRENT_CONTROL_SEQUENCE.store(0, Ordering::Release);
    PANIC_ACTIVE.store(false, Ordering::Release);
}

fn read_stable_init(page: *mut WorkerSharedPage) -> Option<WorkerRuntimeInit> {
    // SAFETY: The caller validated page alignment. Root owns the mapping and
    // keeps the entire ABI page mapped while the child is runnable. Volatile
    // reads plus acquire fences implement the documented sequence-last intake.
    unsafe {
        let commit = addr_of!((*page).init.committed_sequence);
        let first = read_volatile(commit);
        if first == 0 {
            return None;
        }
        fence(Ordering::Acquire);
        let snapshot = read_volatile(addr_of!((*page).init));
        fence(Ordering::Acquire);
        let second = read_volatile(commit);
        (first == second
            && snapshot.descriptor_sequence == first
            && snapshot.committed_sequence == first)
            .then_some(snapshot)
    }
}

fn read_stable_control(page: *mut WorkerSharedPage) -> Option<WorkerControlRecord> {
    // SAFETY: Init validation establishes the full shared-page mapping. Root is
    // the sole control producer. The two commit reads and acquire fences reject
    // an early, zero, or torn record before any action is projected.
    unsafe {
        let commit = addr_of!((*page).control.committed_sequence);
        let first = read_volatile(commit);
        if first == 0 {
            return None;
        }
        fence(Ordering::Acquire);
        let snapshot = read_volatile(addr_of!((*page).control));
        fence(Ordering::Acquire);
        let second = read_volatile(commit);
        (first == second && snapshot.sequence == first && snapshot.committed_sequence == first)
            .then_some(snapshot)
    }
}

fn clear_worker_publications(page: *mut WorkerSharedPage) {
    // SAFETY: The validated ABI mapping gives this child sole producer
    // authority over READY, completion, and its role-selected receipt field.
    // Clearing happens before READY, so root cannot yet treat the instance as
    // live. The root-owned init and control fields are not modified.
    unsafe {
        write_volatile(
            addr_of_mut!((*page).completion),
            WorkerCompletionRecord::EMPTY,
        );
        write_volatile(addr_of_mut!((*page).ready), WorkerReadyRecord::EMPTY);
        write_volatile(
            addr_of_mut!((*page).gpu_receipt),
            GpuLeaseReceiptRecord::EMPTY,
        );
        write_volatile(addr_of_mut!((*page).peft_receipt), PeftReceiptRecord::EMPTY);
        fence(Ordering::Release);
    }
}

fn publish_ready(page: *mut WorkerSharedPage, init: WorkerRuntimeInit) {
    let staged = WorkerReadyRecord::staged(init, READY_SEQUENCE);
    // SAFETY: This child is the sole READY producer. The body is published with
    // a zero commit, followed by a release fence and the sequence-last word.
    unsafe {
        write_volatile(addr_of_mut!((*page).ready), staged);
        fence(Ordering::Release);
        write_volatile(
            addr_of_mut!((*page).ready.committed_sequence),
            staged.sequence,
        );
    }
}

fn publish_completion(page: *mut WorkerSharedPage, staged: WorkerCompletionRecord) {
    // SAFETY: Init validation establishes the mapped completion field and this
    // child is its sole producer. The supervisor reads it with the reciprocal
    // acquire/stable-sequence protocol.
    unsafe {
        write_volatile(addr_of_mut!((*page).completion), staged);
        fence(Ordering::Release);
        write_volatile(
            addr_of_mut!((*page).completion.committed_sequence),
            staged.sequence,
        );
    }
}

fn publish_gpu_receipt(page: *mut WorkerSharedPage, staged: GpuLeaseReceiptRecord) {
    // SAFETY: A validated WorkerGpu init gives this child sole producer
    // authority over the fixed GPU receipt field. Sequence is committed last.
    unsafe {
        write_volatile(addr_of_mut!((*page).gpu_receipt), staged);
        fence(Ordering::Release);
        write_volatile(
            addr_of_mut!((*page).gpu_receipt.committed_sequence),
            staged.sequence,
        );
    }
}

fn publish_peft_receipt(page: *mut WorkerSharedPage, staged: PeftReceiptRecord) {
    // SAFETY: A validated WorkerLora init gives this child sole producer
    // authority over the fixed PEFT receipt field. Sequence is committed last.
    unsafe {
        write_volatile(addr_of_mut!((*page).peft_receipt), staged);
        fence(Ordering::Release);
        write_volatile(
            addr_of_mut!((*page).peft_receipt.committed_sequence),
            staged.sequence,
        );
    }
}

fn process_control(
    page: *mut WorkerSharedPage,
    init: WorkerRuntimeInit,
    control: WorkerControlRecord,
) -> WorkerCompletionStatus {
    #[cfg(feature = "qemu-evidence")]
    cohesix_worker_qemu_evidence_control_handler();
    let action = match control.worker_action() {
        Ok(action) => action,
        Err(_) => publish_fault_and_trap(
            page,
            init,
            control.sequence,
            WorkerCompletionStatus::InvalidControl,
        ),
    };
    match action {
        WorkerAction::HeartbeatPublish => {}
        WorkerAction::GpuLeaseGrant
        | WorkerAction::GpuLeaseRenew
        | WorkerAction::GpuLeaseRelease => {
            let receipt = match GpuLeaseReceiptRecord::staged(control) {
                Ok(receipt) => receipt,
                Err(_) => publish_fault_and_trap(
                    page,
                    init,
                    control.sequence,
                    WorkerCompletionStatus::InvalidControl,
                ),
            };
            publish_gpu_receipt(page, receipt);
        }
        WorkerAction::PeftExport
        | WorkerAction::PeftImport
        | WorkerAction::PeftActivate
        | WorkerAction::PeftRollback => {
            let receipt = match PeftReceiptRecord::staged(control) {
                Ok(receipt) => receipt,
                Err(_) => publish_fault_and_trap(
                    page,
                    init,
                    control.sequence,
                    WorkerCompletionStatus::InvalidControl,
                ),
            };
            publish_peft_receipt(page, receipt);
        }
    }
    let completion = WorkerCompletionRecord::staged_for_control(control);
    let status = match WorkerCompletionStatus::from_raw(completion.status) {
        Ok(status) => status,
        Err(_) => publish_fault_and_trap(
            page,
            init,
            control.sequence,
            WorkerCompletionStatus::InvalidControl,
        ),
    };
    publish_completion(page, completion);
    status
}

fn next_sequence(sequence: u64) -> u64 {
    sequence.saturating_add(1)
}

fn validate_call(
    init: WorkerRuntimeInit,
    badge: sel4_sys::seL4_Word,
    tag: sel4_sys::seL4_MessageInfo,
    message_registers: [sel4_sys::seL4_Word; 4],
) -> Option<u64> {
    if badge as u64 != init.request_badge
        || tag.length() != 4
        || tag.extra_caps() != 0
        || tag.caps_unwrapped() != 0
        || message_registers[0] == 0
        || message_registers[1] != init.identity.slot as sel4_sys::seL4_Word
        || message_registers[2] != init.identity.supervisor_generation as sel4_sys::seL4_Word
        || message_registers[3] != init.identity.cap_generation as sel4_sys::seL4_Word
    {
        None
    } else {
        Some(message_registers[0] as u64)
    }
}

fn signal_supervisor_and_receive(
    init: WorkerRuntimeInit,
    badge: &mut sel4_sys::seL4_Word,
    message_registers: &mut [sel4_sys::seL4_Word; 4],
) -> sel4_sys::seL4_MessageInfo {
    fence(Ordering::Release);
    // SAFETY: Strict init validation proves the send-only supervisor wake cap,
    // receive-only endpoint, and single-owner Reply object. NBSendRecv
    // publishes READY before atomically entering the receive boundary. Once
    // root unbinds the bootstrap SC, every normal activation is backed only by
    // the executor's depth-one donated SC.
    unsafe {
        let [mr0, mr1, mr2, mr3] = message_registers;
        sel4_sys::seL4_NBSendRecvWithMRs(
            init.supervisor_wake_notification_slot as sel4_sys::seL4_CPtr,
            sel4_sys::seL4_MessageInfo::new(0, 0, 0, 0),
            init.service_endpoint_slot as sel4_sys::seL4_CPtr,
            badge,
            mr0,
            mr1,
            mr2,
            mr3,
            init.service_reply_slot as sel4_sys::seL4_CPtr,
        )
    }
}

fn reply_receive(
    init: WorkerRuntimeInit,
    badge: &mut sel4_sys::seL4_Word,
    message_registers: &mut [sel4_sys::seL4_Word; 4],
) -> sel4_sys::seL4_MessageInfo {
    fence(Ordering::Release);
    let tag = sel4_sys::seL4_MessageInfo::new(WORKER_CALL_SUCCESS_LABEL, 0, 0, 4);
    // SAFETY: The runtime owns this Reply cap for exactly the outstanding Call.
    // MCS ReplyRecv releases that donor exactly once and atomically returns the
    // passive Worker to its fixed endpoint before another request can arrive.
    unsafe {
        let [mr0, mr1, mr2, mr3] = message_registers;
        sel4_sys::seL4_ReplyRecvWithMRs(
            init.service_endpoint_slot as sel4_sys::seL4_CPtr,
            tag,
            badge,
            mr0,
            mr1,
            mr2,
            mr3,
            init.service_reply_slot as sel4_sys::seL4_CPtr,
        )
    }
}

fn signal_supervisor(init: WorkerRuntimeInit) {
    signal_slot(init.supervisor_wake_notification_slot as usize);
}

fn signal_slot(slot: usize) {
    fence(Ordering::Release);
    // SAFETY: Strict init validation proves the nonzero send-only supervisor
    // wake slot. The minted one-hot badge is fixed in the cap, not selected by
    // the child. All durable output is committed before this scheduling hint.
    unsafe {
        sel4_sys::seL4_Signal(slot as sel4_sys::seL4_CPtr);
    }
}

fn publish_fault_and_trap(
    page: *mut WorkerSharedPage,
    init: WorkerRuntimeInit,
    sequence: u64,
    status: WorkerCompletionStatus,
) -> ! {
    publish_completion(
        page,
        WorkerCompletionRecord::staged_terminal(sequence, init.identity, status),
    );
    signal_supervisor(init);
    enter_standard_fault()
}

fn enter_standard_fault() -> ! {
    // SAFETY: `brk` deliberately transfers control to the supervisor-installed
    // standard fault endpoint. It does not access memory or broaden authority,
    // and this function is used only after the Worker has stopped service.
    unsafe {
        core::arch::asm!("brk #0", options(noreturn, nostack, nomem));
    }
}
