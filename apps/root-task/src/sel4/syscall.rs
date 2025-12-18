// Author: Lukas Bower
//! Low-level seL4 syscall wrappers for the root task.
#![allow(dead_code)]
#![cfg(feature = "kernel")]

use sel4_sys::{
    seL4_CallWithMRs, seL4_CPtr, seL4_MessageInfo, seL4_NBRecv, seL4_Recv, seL4_Reply, seL4_Send,
    seL4_Wait, seL4_Word, seL4_Yield,
};

extern "C" {
    fn seL4_ReplyRecv(
        dest: seL4_CPtr,
        msg_info: seL4_MessageInfo,
        sender_badge: *mut seL4_Word,
    ) -> seL4_MessageInfo;
}

pub(super) unsafe fn send(dest: seL4_CPtr, info: seL4_MessageInfo) {
    unsafe {
        seL4_Send(dest, info);
    }
}

pub(super) unsafe fn call_with_mrs(
    dest: seL4_CPtr,
    info: seL4_MessageInfo,
    mr0: *mut seL4_Word,
    mr1: *mut seL4_Word,
    mr2: *mut seL4_Word,
    mr3: *mut seL4_Word,
) -> seL4_MessageInfo {
    unsafe { seL4_CallWithMRs(dest, info, mr0, mr1, mr2, mr3) }
}

pub(super) unsafe fn reply(info: seL4_MessageInfo) {
    unsafe {
        seL4_Reply(info);
    }
}

pub(super) unsafe fn reply_recv(
    dest: seL4_CPtr,
    info: seL4_MessageInfo,
    badge: *mut seL4_Word,
) -> seL4_MessageInfo {
    unsafe { seL4_ReplyRecv(dest, info, badge) }
}

pub(super) unsafe fn recv(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    unsafe { seL4_Recv(dest, badge) }
}

pub(super) unsafe fn wait(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    unsafe { seL4_Wait(dest, badge) }
}

pub(super) unsafe fn nb_recv(dest: seL4_CPtr, badge: *mut seL4_Word) -> seL4_MessageInfo {
    unsafe { seL4_NBRecv(dest, badge) }
}

pub(super) unsafe fn yield_now() {
    unsafe {
        seL4_Yield();
    }
}
