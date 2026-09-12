// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define IPC storage, raw-pointer and Reply ownership contracts at seL4 syscall boundaries.
// Author: Lukas Bower
//! Low-level seL4 syscall wrappers for the root task.
//!
//! The userspace IPC-buffer getter is process-global, not per-TCB TLS. Binding
//! an IPC frame to a TCB does not change that getter. Implicit MR access requires
//! the global pointer to select the current caller's exclusively owned buffer;
//! explicit-MR paths use caller storage and the kernel-bound frame for extras.
//! MCS notification/empty Wait returns and target zero-length Send/NBSend skip
//! implicit MR access entirely.
#![allow(dead_code)]
#![cfg(feature = "kernel")]

use core::panic::Location;

use super::{ipc_bootstrap_trap, IpcSyscallKind};
#[cfg(target_os = "none")]
use sel4_sys::{
    seL4_CPtr, seL4_CallWithMRs, seL4_MessageInfo, seL4_NBSend, seL4_RecvWithMRs, seL4_Send,
    seL4_Wait, seL4_Word, seL4_Yield,
};
#[cfg(not(target_os = "none"))]
use sel4_sys::{
    seL4_CPtr, seL4_CallWithMRs, seL4_MessageInfo, seL4_Poll, seL4_Recv, seL4_Send, seL4_Word,
    seL4_Yield,
};
#[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
use sel4_sys::{seL4_MCS_ReplyWithMRs, seL4_NBRecv, seL4_NBWait, seL4_ReplyRecv};
#[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
use sel4_sys::{seL4_NBRecv, seL4_Recv, seL4_Reply, seL4_ReplyRecv, seL4_ReplyWithMRs};

/// Send a message through the selected kernel ABI.
///
/// # Safety
/// On target, nonzero length requires the global userspace IPC pointer to select
/// the current thread's live aligned buffer, with exclusive access and all four
/// fast MR words initialized. Binding another TCB's kernel buffer does not
/// change that getter. Zero length skips the getter and uses private zero words.
/// Any additional message/cap-transfer fields selected by `info` require the
/// current thread's live kernel-bound buffer, initialized within seL4 limits.
/// `dest` must name the caller-owned capability for this operation. Host IPC
/// emulation must be serialized and have its synthetic buffer installed.
#[track_caller]
pub(super) unsafe fn send(dest: seL4_CPtr, info: seL4_MessageInfo) {
    if ipc_bootstrap_trap(IpcSyscallKind::Send, dest, Location::caller()) {
        return;
    }

    // SAFETY: The caller owns `dest` and any bounded kernel-buffer fields. On
    // target, nonempty fast MRs use the caller's live exclusive global buffer;
    // zero length uses private zeros without reading it. Host IPC is serialized.
    unsafe { seL4_Send(dest, info) };
}

/// Attempt a nonblocking send through the selected kernel ABI.
///
/// # Safety
/// The caller must satisfy [`send`]'s capability, nonempty fast-register and
/// kernel-bound extra-field requirements. Zero length skips the target global
/// getter. The host branch uses serialized send emulation; it does not establish
/// target nonblocking scheduling behavior.
#[track_caller]
pub(super) unsafe fn nb_send(dest: seL4_CPtr, info: seL4_MessageInfo) {
    if ipc_bootstrap_trap(IpcSyscallKind::NbSend, dest, Location::caller()) {
        return;
    }

    #[cfg(target_os = "none")]
    // SAFETY: The caller meets Send's cap and initialized-field contracts;
    // nonempty NBSend reads the same live storage, while zero length skips it.
    unsafe {
        seL4_NBSend(dest, info);
    }

    #[cfg(not(target_os = "none"))]
    // SAFETY: The caller serializes host IPC-buffer access. This emulation stores
    // `info` in that live buffer; it does not perform target scheduling.
    unsafe {
        seL4_Send(dest, info);
    }
}

/// Call with optional caller-owned fast message registers.
///
/// # Safety
/// Each non-null MR pointer must address an aligned initialized word, be uniquely
/// writable and remain live for the complete synchronous call, without aliasing
/// another MR or the active IPC-buffer storage. Null omits that register. The
/// target reads inputs selected by `info.length()` and writes every non-null
/// output; the host reads every non-null input regardless of that length. The
/// current thread must own a live kernel-bound IPC buffer for any remaining
/// message/cap-transfer fields, initialized within `info`'s bounds. This target
/// explicit-MR path does not select storage through the global userspace getter.
/// `dest` must have the admitted invocation/Call authority; any MCS donation must
/// follow the caller's single-outstanding-call protocol. Serialize host emulation.
#[track_caller]
pub(super) unsafe fn call_with_mrs(
    dest: seL4_CPtr,
    info: seL4_MessageInfo,
    mr0: *mut seL4_Word,
    mr1: *mut seL4_Word,
    mr2: *mut seL4_Word,
    mr3: *mut seL4_Word,
) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::Call, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    // SAFETY: The caller keeps each supplied MR aligned, initialized, exclusive
    // and live across the call; null MRs are supported. These words are disjoint
    // from each other and from the live IPC buffer used for remaining fields.
    unsafe { seL4_CallWithMRs(dest, info, mr0, mr1, mr2, mr3) }
}

/// Reply through the classic kernel's current implicit reply authority.
///
/// # Safety
/// On the classic target, the current thread must own the unconsumed implicit
/// reply association from its matching receive. The global userspace pointer
/// must select that thread's live kernel-bound IPC buffer, with exclusive access,
/// initialized fast MRs and any additional
/// fields selected by `info`. Reply authority must not already have been used or
/// transferred. MCS and host branches reject this operation rather than replying.
#[track_caller]
pub(super) unsafe fn reply(info: seL4_MessageInfo) {
    if ipc_bootstrap_trap(
        IpcSyscallKind::Reply,
        super::root_endpoint(),
        Location::caller(),
    ) {
        return;
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    // SAFETY: The caller owns the classic implicit reply exactly once and keeps
    // its initialized outgoing IPC-buffer words live until this reply completes.
    unsafe {
        seL4_Reply(info);
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    {
        let _ = info;
        panic!("MCS reply requires an explicit single-use Reply capability");
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = info;
        panic!("seL4_Reply is unavailable on host targets");
    }
}

/// Reply using the explicit Reply capability required by an MCS kernel.
///
/// # Safety
/// The current thread must own exactly the outstanding association in `reply_cap`
/// on MCS, or its matching unconsumed implicit reply association on classic.
/// The initialized `message_registers` borrow must remain readable for the call;
/// no other actor may consume the reply concurrently. The current TCB's live
/// kernel-bound IPC buffer must cover any additional fields selected by `info`,
/// exclusively and within bounds. Target fast MRs use the array directly,
/// not the process-global getter. The host branch rejects replies.
#[track_caller]
pub(super) unsafe fn reply_to(
    reply_cap: seL4_CPtr,
    info: seL4_MessageInfo,
    message_registers: &[seL4_Word; 4],
) {
    if ipc_bootstrap_trap(IpcSyscallKind::Reply, reply_cap, Location::caller()) {
        return;
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    // SAFETY: The caller exclusively owns the outstanding MCS Reply association.
    // The four array elements are initialized and remain immutably borrowed;
    // any additional message fields use the caller's live exclusive IPC buffer.
    unsafe {
        seL4_MCS_ReplyWithMRs(
            reply_cap,
            info,
            &message_registers[0],
            &message_registers[1],
            &message_registers[2],
            &message_registers[3],
        );
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    // SAFETY: The caller owns the current unconsumed classic implicit reply.
    // The four initialized array elements remain readable during ReplyWithMRs;
    // `reply_cap` is unused and cannot substitute for that implicit authority.
    unsafe {
        let _ = reply_cap;
        seL4_ReplyWithMRs(
            info,
            &message_registers[0],
            &message_registers[1],
            &message_registers[2],
            &message_registers[3],
        );
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (reply_cap, info, message_registers);
        panic!("seL4 reply is unavailable on host targets");
    }
}

/// Reply and receive using classic implicit reply authority.
///
/// # Safety
/// The current thread must own its unconsumed implicit reply association and the
/// Read capability `dest`. `badge` may be null; otherwise it must be aligned,
/// uniquely writable and live across the blocking syscall, disjoint from the
/// active IPC buffer. The global getter must select the current TCB's live
/// kernel-bound buffer, exclusively accessible with initialized outgoing
/// MRs/cap fields and valid receive-cap
/// storage when needed. MCS and host branches reject this implicit-reply API.
#[track_caller]
pub(super) unsafe fn reply_recv(
    dest: seL4_CPtr,
    info: seL4_MessageInfo,
    badge: *mut seL4_Word,
) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::ReplyRecv, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    // SAFETY: The caller owns the classic reply association and receive cap;
    // `badge` is null or a live exclusive output disjoint from the installed
    // IPC buffer used for outgoing and returned MRs and any cap transfer.
    unsafe {
        seL4_ReplyRecv(dest, info, badge)
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    {
        let _ = (dest, info, badge);
        panic!("MCS reply-receive requires an explicit single-use Reply capability");
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (dest, info, badge);
        panic!("seL4_ReplyRecv is unavailable on host targets");
    }
}

/// Atomically reply and receive using an explicit MCS Reply object.
///
/// # Safety
/// `reply` must name the current thread's exclusively owned outstanding MCS Reply
/// association and remain available for the next receive; classic instead uses
/// the unconsumed implicit association and ignores `reply`. `dest` must be the
/// admitted Read endpoint. `badge` is null or an aligned, uniquely writable word
/// live across the blocking call and disjoint from the IPC buffer. The thread's
/// global userspace getter must select that TCB's live kernel-bound IPC buffer,
/// exclusively accessible with initialized outgoing fields and receive-cap
/// storage. A TCB binding alone does not select the global getter.
#[track_caller]
pub(super) unsafe fn reply_recv_with_reply(
    dest: seL4_CPtr,
    info: seL4_MessageInfo,
    badge: *mut seL4_Word,
    reply: seL4_CPtr,
) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::ReplyRecv, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    // SAFETY: The caller owns the outstanding MCS Reply association and the
    // next receive endpoint. The live IPC buffer holds initialized outgoing
    // MRs and receives new ones; nullable `badge` is a disjoint exclusive word.
    unsafe {
        seL4_ReplyRecv(dest, info, badge, reply)
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    // SAFETY: Classic uses only the caller's unconsumed implicit reply. Its
    // live IPC buffer and nullable exclusive badge satisfy ReplyRecv; the
    // ignored explicit cap cannot create or replace classic reply authority.
    unsafe {
        let _ = reply;
        seL4_ReplyRecv(dest, info, badge)
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (dest, info, badge, reply);
        panic!("seL4_ReplyRecv is unavailable on host targets");
    }
}

/// Receive without an explicit MCS Reply object.
///
/// # Safety
/// `dest` must have the admitted receive authority; on MCS this Wait path must not
/// be used for a Call that requires a retained Reply association. `badge` is null
/// or an aligned, uniquely writable word live across the wait and disjoint from
/// the active IPC buffer. Nonempty MCS endpoint returns and every classic receive
/// require the global getter to select this TCB's live, exclusively owned bound
/// buffer for returned MRs. Zero-length MCS returns skip the getter. Kernel cap
/// receive uses the actual bound buffer. Retire prior classic replies and
/// serialize host access to its global emulated IPC buffer.
#[track_caller]
pub(super) unsafe fn recv(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::Recv, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    // SAFETY: The caller admits this receive without a new Reply association.
    // Zero-length returns skip the global getter; nonempty endpoint returns
    // require it to select this caller's live exclusive IPC buffer. A non-null
    // badge remains a disjoint exclusive output in either case.
    unsafe {
        seL4_Wait(dest, badge)
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    // SAFETY: The caller owns the Read cap, has retired any prior implicit
    // reply, and retains the exclusive IPC buffer and nullable badge for Recv.
    unsafe {
        seL4_Recv(dest, badge)
    }

    #[cfg(not(target_os = "none"))]
    // SAFETY: Host IPC access is serialized by the caller; the emulated buffer
    // is live and a non-null badge points to an aligned exclusive output word.
    unsafe {
        seL4_Recv(dest, badge)
    }
}

/// Receive while storing the caller's reply capability in an explicit Reply object.
///
/// # Safety
/// `dest` must be the admitted Read endpoint. On MCS, `reply` must be an available
/// Reply object owned exclusively by the current thread, with no outstanding
/// association to overwrite; classic ignores it and requires the previous implicit
/// reply to be retired. `badge` is null or an aligned, uniquely writable word live
/// across receive and disjoint from `message_registers` and the active IPC buffer.
/// The array borrow must remain exclusive throughout. The current TCB's live
/// kernel-bound buffer covers any additional received words/caps; target fast
/// MRs go directly to the array without consulting the global getter. The host
/// branch uses the global emulated buffer and requires serialized ownership.
#[track_caller]
pub(super) unsafe fn recv_with_reply(
    dest: seL4_CPtr,
    badge: *mut seL4_Word,
    reply: seL4_CPtr,
    message_registers: &mut [seL4_Word; 4],
) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::Recv, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    // SAFETY: The caller owns an available MCS Reply object and Read endpoint.
    // Destructuring the exclusive array yields four disjoint live MR outputs;
    // nullable badge is disjoint, and the bound IPC buffer covers any extras.
    unsafe {
        let [mr0, mr1, mr2, mr3] = message_registers;
        seL4_RecvWithMRs(dest, badge, reply, mr0, mr1, mr2, mr3)
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    // SAFETY: The caller has retired the prior classic reply and owns the
    // Read endpoint. The exclusive array yields disjoint MR outputs; nullable
    // badge is disjoint and live, and the IPC buffer covers any received extras.
    unsafe {
        let _ = reply;
        let [mr0, mr1, mr2, mr3] = message_registers;
        seL4_RecvWithMRs(dest, badge, mr0, mr1, mr2, mr3)
    }

    #[cfg(not(target_os = "none"))]
    // SAFETY: The caller serializes host IPC-buffer use and supplies a nullable
    // live exclusive badge. Each array element is distinct, and GetMR indexes
    // 0..4 remain within the emulated IPC-buffer message array.
    unsafe {
        let _ = reply;
        let info = seL4_Recv(dest, badge);
        for (index, value) in message_registers.iter_mut().enumerate() {
            *value = sel4_sys::seL4_GetMR(index as seL4_Word);
        }
        info
    }
}

/// Wait without retaining a new MCS Reply association.
///
/// # Safety
/// `dest` must have the admitted wait/receive authority; callers requiring a reply
/// to a Call must use the explicit-Reply receive path. `badge` is null or an aligned,
/// uniquely writable word live across the wait and disjoint from the IPC buffer.
/// Nonempty MCS endpoint returns and every classic receive require the global
/// getter to select the caller's live, exclusive kernel-bound IPC buffer. MCS
/// zero-length notification/empty returns skip that getter. Kernel receive-cap
/// fields still belong to the actual bound buffer. Retire prior classic replies;
/// host global IPC-buffer emulation must be serialized.
#[track_caller]
pub(super) unsafe fn wait(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::Wait, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(target_os = "none")]
    // SAFETY: The caller retains its wait cap and nullable exclusive badge.
    // MCS zero-length returns skip global MR storage; nonempty endpoint or
    // classic returns require the global getter to select the caller's live
    // exclusive IPC buffer. MCS Wait does not allocate reply authority.
    unsafe {
        seL4_Wait(dest, badge)
    }

    #[cfg(not(target_os = "none"))]
    // SAFETY: The caller serializes the emulated receive and retains the live
    // host IPC buffer and nullable, aligned, exclusive badge output.
    unsafe {
        seL4_Recv(dest, badge)
    }
}

/// Attempt a receive without an explicit MCS Reply object.
///
/// # Safety
/// The caller must satisfy [`recv`]'s capability, badge, live exclusive IPC-buffer
/// and prior-reply requirements. MCS uses NBWait and cannot retain a new Reply
/// association. A return without a message grants no new reply authority; the
/// caller must classify the result before acting on message data.
#[track_caller]
pub(super) unsafe fn nb_recv(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::NbRecv, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    // SAFETY: The caller owns the classic receive cap, has retired the old
    // implicit reply, and provides the live IPC buffer plus nullable exclusive
    // badge. An empty NBRecv does not create a reply obligation.
    unsafe {
        seL4_NBRecv(dest, badge)
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    // SAFETY: The caller supplies the wait cap and nullable exclusive badge.
    // A zero-length return skips global MR storage; otherwise the getter must
    // select the caller's live exclusive IPC buffer. No Reply is allocated.
    unsafe {
        seL4_NBWait(dest, badge)
    }

    #[cfg(not(target_os = "none"))]
    // SAFETY: The host poll writes only a non-null, aligned exclusive badge
    // provided by the caller; it does not model a received kernel message.
    unsafe {
        seL4_Poll(dest, badge)
    }
}

/// Nonblockingly receive with an explicit MCS Reply object.
///
/// # Safety
/// `dest` must be the admitted Read endpoint. The current thread must exclusively
/// own an available MCS `reply` object without a live association to overwrite;
/// classic ignores it and requires the old implicit reply to be retired. `badge`
/// is null or aligned, uniquely writable, live for the syscall and disjoint from
/// the IPC buffer. This NBRecv path always stores fast MRs, so the global getter
/// must select the current TCB's live, exclusively writable kernel-bound buffer,
/// including on an empty return. An empty poll grants no reply authority.
/// Host IPC emulation must be serialized.
#[track_caller]
pub(super) unsafe fn nb_recv_with_reply(
    dest: seL4_CPtr,
    badge: *mut seL4_Word,
    reply: seL4_CPtr,
) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::NbRecv, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    // SAFETY: The caller owns an available MCS Reply object, receive cap and
    // live exclusive IPC buffer. NBRecv stores returned MRs and the optional
    // disjoint badge; only an actual Call establishes a reply association.
    unsafe {
        seL4_NBRecv(dest, badge, reply)
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    // SAFETY: Classic uses its receive cap and implicit reply slot after the
    // prior reply was retired. The live IPC buffer and nullable exclusive badge
    // satisfy NBRecv; the ignored explicit cap grants no reply authority.
    unsafe {
        let _ = reply;
        seL4_NBRecv(dest, badge)
    }

    #[cfg(not(target_os = "none"))]
    // SAFETY: The caller supplies a nullable aligned exclusive badge output.
    // Host Poll ignores the explicit Reply cap and creates no association.
    unsafe {
        let _ = reply;
        seL4_Poll(dest, badge)
    }
}

/// Poll through the selected nonblocking wait/receive ABI.
///
/// # Safety
/// The caller must satisfy [`nb_recv`]'s destination, nullable badge, live exclusive
/// IPC-buffer and prior-reply requirements. This MCS NBWait path retains no new
/// Reply association; a notification or empty result is not an endpoint message.
/// Host emulation requires serialized access to any supplied badge storage.
#[track_caller]
pub(super) unsafe fn poll(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::NbRecv, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    // SAFETY: The caller retains the classic receive cap, live exclusive IPC
    // buffer and nullable disjoint badge, without an old implicit reply to
    // replace. No message means no new reply authority.
    unsafe {
        seL4_NBRecv(dest, badge)
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    // SAFETY: Nullable badge is a live exclusive output. Nonempty endpoint
    // returns require the getter to select the caller's live exclusive IPC
    // buffer; zero-length returns skip global MR storage. NBWait retains no
    // MCS Reply association.
    unsafe {
        seL4_NBWait(dest, badge)
    }

    #[cfg(not(target_os = "none"))]
    // SAFETY: The caller retains the nullable aligned exclusive badge for
    // host Poll; this branch creates no target receive or Reply association.
    unsafe {
        seL4_Poll(dest, badge)
    }
}

/// Yield the current admitted thread without transferring IPC data.
///
/// # Safety
/// Execution must be in a current seL4 thread using the selected kernel ABI.
/// No IPC-buffer, badge, MR or Reply pointer is accessed by Yield, and the caller
/// must not treat scheduling as a memory-publication or ownership-transfer fence.
/// The host branch is only the non-scheduling emulation.
pub(super) unsafe fn yield_now() {
    #[cfg(target_os = "none")]
    // SAFETY: Yield performs the side-effect-only seL4 scheduling syscall and
    // does not dereference user memory.
    unsafe {
        seL4_Yield();
    }
    #[cfg(not(target_os = "none"))]
    seL4_Yield();
}
