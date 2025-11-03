// Author: Lukas Bower
#![allow(unsafe_code)]

use crate::sel4::{self, seL4_CPtr, seL4_CapNull, seL4_MessageInfo};

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
    sel4::send_unchecked(ep, info)
}

#[inline]
pub fn call_if_valid(ep: seL4_CPtr, info: seL4_MessageInfo) -> seL4_MessageInfo {
    if !cap_is_valid(ep) {
        log::warn!("[ipc] call skipped: null ep");
        return seL4_MessageInfo::new(0, 0, 0, 0);
    }
    sel4::call_unchecked(ep, info)
}

#[inline]
pub fn signal_if_valid(ep: seL4_CPtr) {
    if !cap_is_valid(ep) {
        log::warn!("[ipc] signal skipped: null ep");
        return;
    }
    sel4::signal_unchecked(ep)
}
