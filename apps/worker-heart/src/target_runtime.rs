// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Run isolated Workers over the sealed notification-only Worker ABI.
// Author: Lukas Bower

//! Shared seL4 runtime for the Heartbeat, GPU, and LoRA Worker images.

#[cfg(not(target_arch = "aarch64"))]
compile_error!("isolated Worker target runtime requires AArch64");

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicBool, AtomicU64, AtomicUsize, Ordering};

use worker_task_abi::{
    GpuLeaseReceiptRecord, PeftReceiptRecord, WorkerAction, WorkerCompletionRecord,
    WorkerCompletionStatus, WorkerControlRecord, WorkerLifecycleEvent, WorkerReadyRecord,
    WorkerRole, WorkerRuntimeInit, WorkerSharedPage, WORKER_SHARED_PAGE_ALIGNMENT,
};

const READY_SEQUENCE: u64 = 1;

static SHARED_PAGE_ADDRESS: AtomicUsize = AtomicUsize::new(0);
static LIFECYCLE_NOTIFICATION_SLOT: AtomicUsize = AtomicUsize::new(0);
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
/// IPC buffer before resuming the child. The child receives no endpoint caps:
/// normal operation uses a receive-only lifecycle notification, a send-only
/// supervisor wake notification, and durable shared records.
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
    let mut badge = signal_supervisor_and_wait_for_lifecycle(init);
    loop {
        let event = match init.lifecycle_bits.classify(badge) {
            Ok(event) => event,
            Err(_) => publish_fault_and_trap(
                page,
                init,
                next_sequence(last_sequence),
                WorkerCompletionStatus::InvalidControl,
            ),
        };
        match event {
            WorkerLifecycleEvent::Control => {
                let Some(control) = read_stable_control(page) else {
                    badge = wait_for_lifecycle(init);
                    continue;
                };
                if control.sequence <= last_sequence {
                    badge = wait_for_lifecycle(init);
                    continue;
                }
                let Some(expected_sequence) = last_sequence.checked_add(1) else {
                    publish_fault_and_trap(
                        page,
                        init,
                        last_sequence,
                        WorkerCompletionStatus::InvalidControl,
                    );
                };
                if control.sequence != expected_sequence || control.validate_for(init).is_err() {
                    publish_fault_and_trap(
                        page,
                        init,
                        expected_sequence,
                        WorkerCompletionStatus::InvalidControl,
                    );
                }
                CURRENT_CONTROL_SEQUENCE.store(control.sequence, Ordering::Release);
                process_control(page, init, control);
                last_sequence = control.sequence;
                LAST_CONTROL_SEQUENCE.store(last_sequence, Ordering::Release);
                CURRENT_CONTROL_SEQUENCE.store(0, Ordering::Release);
                badge = signal_supervisor_and_wait_for_lifecycle(init);
            }
            WorkerLifecycleEvent::Timeout => {
                let sequence = pending_or_next_sequence(page, last_sequence);
                publish_completion(
                    page,
                    WorkerCompletionRecord::staged_terminal(
                        sequence,
                        init.identity,
                        WorkerCompletionStatus::Timeout,
                    ),
                );
                signal_supervisor(init);
                park_for_teardown(init);
            }
            WorkerLifecycleEvent::Shutdown => {
                let sequence = next_sequence(last_sequence);
                publish_completion(
                    page,
                    WorkerCompletionRecord::staged_terminal(
                        sequence,
                        init.identity,
                        WorkerCompletionStatus::Shutdown,
                    ),
                );
                signal_supervisor(init);
                park_for_teardown(init);
            }
            WorkerLifecycleEvent::Revoke => {
                let sequence = next_sequence(last_sequence);
                publish_completion(
                    page,
                    WorkerCompletionRecord::staged_terminal(
                        sequence,
                        init.identity,
                        WorkerCompletionStatus::Revoked,
                    ),
                );
                signal_supervisor(init);
                park_for_teardown(init);
            }
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
    let lifecycle_slot = LIFECYCLE_NOTIFICATION_SLOT.load(Ordering::Acquire);
    let wake_slot = SUPERVISOR_WAKE_NOTIFICATION_SLOT.load(Ordering::Acquire);
    if page_address != 0 && lifecycle_slot != 0 && wake_slot != 0 {
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
    LIFECYCLE_NOTIFICATION_SLOT.store(init.lifecycle_notification_slot as usize, Ordering::Release);
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
) {
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
    publish_completion(page, WorkerCompletionRecord::staged_for_control(control));
}

fn pending_or_next_sequence(page: *mut WorkerSharedPage, last_sequence: u64) -> u64 {
    if let Some(control) = read_stable_control(page) {
        if control.sequence > last_sequence {
            return control.sequence;
        }
    }
    next_sequence(last_sequence)
}

fn next_sequence(sequence: u64) -> u64 {
    sequence.saturating_add(1)
}

fn wait_for_lifecycle(init: WorkerRuntimeInit) -> u64 {
    let mut badge: sel4_sys::seL4_Word = 0;
    // SAFETY: Strict init validation proves the nonzero receive-only lifecycle
    // notification slot. The supervisor installs it before resume. Wait blocks
    // the active-SC Worker when idle and carries no IPC reply/donation path.
    let _ = unsafe {
        sel4_sys::seL4_Wait(
            init.lifecycle_notification_slot as sel4_sys::seL4_CPtr,
            &mut badge,
        )
    };
    badge as u64
}

fn signal_supervisor_and_wait_for_lifecycle(init: WorkerRuntimeInit) -> u64 {
    let mut badge: sel4_sys::seL4_Word = 0;
    fence(Ordering::Release);
    // SAFETY: Strict init validation proves a send-only supervisor wake slot
    // and a distinct receive-only lifecycle slot. NBSendWait performs a
    // nonblocking notification signal and a notification Wait in one atomic
    // MCS syscall. It cannot donate this Worker's bound SC, installs no Reply
    // object, and closes the post-completion window in which a new control
    // signal could otherwise bypass an actual blocking point.
    let _ = unsafe {
        sel4_sys::seL4_NBSendWait(
            init.supervisor_wake_notification_slot as sel4_sys::seL4_CPtr,
            sel4_sys::seL4_MessageInfo::new(0, 0, 0, 0),
            init.lifecycle_notification_slot as sel4_sys::seL4_CPtr,
            &mut badge,
        )
    };
    badge as u64
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

fn park_for_teardown(init: WorkerRuntimeInit) -> ! {
    loop {
        let mut ignored_badge: sel4_sys::seL4_Word = 0;
        // SAFETY: The validated lifecycle notification remains mapped until
        // root suspends and deletes this child. Re-waiting performs no service
        // and prevents a coalesced extra edge from becoming a busy loop.
        let _ = unsafe {
            sel4_sys::seL4_Wait(
                init.lifecycle_notification_slot as sel4_sys::seL4_CPtr,
                &mut ignored_badge,
            )
        };
    }
}

fn enter_standard_fault() -> ! {
    // SAFETY: `brk` deliberately transfers control to the supervisor-installed
    // standard fault endpoint. It does not access memory or broaden authority,
    // and this function is used only after the Worker has stopped service.
    unsafe {
        core::arch::asm!("brk #0", options(noreturn, nostack, nomem));
    }
}
