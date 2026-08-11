// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the sel4/syscall module for root-task.
// Author: Lukas Bower
//! Low-level seL4 syscall wrappers for the root task.
#![allow(dead_code)]
#![cfg(feature = "kernel")]

use core::panic::Location;

use super::{ipc_bootstrap_trap, IpcSyscallKind};
#[cfg(target_os = "none")]
use sel4_sys::{
    seL4_CPtr, seL4_CallWithMRs, seL4_MessageInfo, seL4_NBSend, seL4_Send, seL4_Wait, seL4_Word,
    seL4_Yield,
};
#[cfg(not(target_os = "none"))]
use sel4_sys::{
    seL4_CPtr, seL4_CallWithMRs, seL4_MessageInfo, seL4_Poll, seL4_Recv, seL4_Send, seL4_Word,
    seL4_Yield,
};
#[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
use sel4_sys::{seL4_MCS_Reply, seL4_NBRecv, seL4_NBWait, seL4_Recv, seL4_ReplyRecv};
#[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
use sel4_sys::{seL4_NBRecv, seL4_Recv, seL4_Reply, seL4_ReplyRecv};

#[track_caller]
pub(super) unsafe fn send(dest: seL4_CPtr, info: seL4_MessageInfo) {
    if ipc_bootstrap_trap(IpcSyscallKind::Send, dest, Location::caller()) {
        return;
    }

    unsafe { seL4_Send(dest, info) };
}

#[track_caller]
pub(super) unsafe fn nb_send(dest: seL4_CPtr, info: seL4_MessageInfo) {
    if ipc_bootstrap_trap(IpcSyscallKind::NbSend, dest, Location::caller()) {
        return;
    }

    #[cfg(target_os = "none")]
    unsafe {
        seL4_NBSend(dest, info);
    }

    #[cfg(not(target_os = "none"))]
    unsafe {
        seL4_Send(dest, info);
    }
}

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

    unsafe { seL4_CallWithMRs(dest, info, mr0, mr1, mr2, mr3) }
}

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
#[track_caller]
pub(super) unsafe fn reply_to(reply_cap: seL4_CPtr, info: seL4_MessageInfo) {
    if ipc_bootstrap_trap(IpcSyscallKind::Reply, reply_cap, Location::caller()) {
        return;
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    unsafe {
        seL4_MCS_Reply(reply_cap, info);
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    unsafe {
        let _ = reply_cap;
        seL4_Reply(info);
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (reply_cap, info);
        panic!("seL4 reply is unavailable on host targets");
    }
}

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
    unsafe {
        seL4_ReplyRecv(dest, info, badge, reply)
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
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

#[track_caller]
pub(super) unsafe fn recv(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::Recv, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    unsafe {
        seL4_Wait(dest, badge)
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    unsafe {
        seL4_Recv(dest, badge)
    }

    #[cfg(not(target_os = "none"))]
    unsafe {
        seL4_Recv(dest, badge)
    }
}

/// Receive while storing the caller's reply capability in an explicit Reply object.
#[track_caller]
pub(super) unsafe fn recv_with_reply(
    dest: seL4_CPtr,
    badge: *mut seL4_Word,
    reply: seL4_CPtr,
) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::Recv, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    unsafe {
        seL4_Recv(dest, badge, reply)
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    unsafe {
        let _ = reply;
        seL4_Recv(dest, badge)
    }

    #[cfg(not(target_os = "none"))]
    unsafe {
        let _ = reply;
        seL4_Recv(dest, badge)
    }
}

#[track_caller]
pub(super) unsafe fn wait(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::Wait, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(target_os = "none")]
    unsafe {
        seL4_Wait(dest, badge)
    }

    #[cfg(not(target_os = "none"))]
    unsafe {
        seL4_Recv(dest, badge)
    }
}

#[track_caller]
pub(super) unsafe fn nb_recv(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::NbRecv, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    unsafe {
        seL4_NBRecv(dest, badge)
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    unsafe {
        seL4_NBWait(dest, badge)
    }

    #[cfg(not(target_os = "none"))]
    unsafe {
        seL4_Poll(dest, badge)
    }
}

/// Nonblockingly receive with an explicit MCS Reply object.
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
    unsafe {
        seL4_NBRecv(dest, badge, reply)
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    unsafe {
        let _ = reply;
        seL4_NBRecv(dest, badge)
    }

    #[cfg(not(target_os = "none"))]
    unsafe {
        let _ = reply;
        seL4_Poll(dest, badge)
    }
}

#[track_caller]
pub(super) unsafe fn poll(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    if ipc_bootstrap_trap(IpcSyscallKind::NbRecv, dest, Location::caller()) {
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }

    #[cfg(all(target_os = "none", not(sel4_config_kernel_mcs)))]
    unsafe {
        seL4_NBRecv(dest, badge)
    }

    #[cfg(all(target_os = "none", sel4_config_kernel_mcs))]
    unsafe {
        seL4_NBWait(dest, badge)
    }

    #[cfg(not(target_os = "none"))]
    unsafe {
        seL4_Poll(dest, badge)
    }
}

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
