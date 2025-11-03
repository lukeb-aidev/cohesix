// Author: Lukas Bower
#![allow(unsafe_code)]

use sel4_sys::*;

#[inline]
pub fn cap_is_valid(ep: seL4_CPtr) -> bool {
    ep != seL4_CapNull
}

#[inline]
pub fn send_if_valid(ep: seL4_CPtr, info: seL4_MessageInfo) {
    if !cap_is_valid(ep) {
        log::warn!("[ipc] send skipped: null ep");
        return;
    }
    unsafe { seL4_Send(ep, info) }
}

#[inline]
pub fn call_if_valid(ep: seL4_CPtr, info: seL4_MessageInfo) -> seL4_MessageInfo {
    if !cap_is_valid(ep) {
        log::warn!("[ipc] call skipped: null ep");
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }
    unsafe { seL4_Call(ep, info) }
}

#[inline]
pub fn signal_if_valid(ep: seL4_CPtr) {
    if !cap_is_valid(ep) {
        log::warn!("[ipc] signal skipped: null ep");
        return;
    }
    unsafe { seL4_Signal(ep) }
}
