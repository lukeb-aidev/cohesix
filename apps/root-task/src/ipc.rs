// Author: Lukas Bower
#![allow(unsafe_code)]

#[cfg(feature = "kernel")]
use crate::sel4;

#[cfg(feature = "kernel")]
mod types {
    pub use crate::sel4::{seL4_CPtr, seL4_CapNull, seL4_Error, seL4_MessageInfo};
    pub const FAILED_LOOKUP: seL4_Error = sel4_sys::seL4_FailedLookup;
}

#[cfg(not(feature = "kernel"))]
#[allow(missing_docs, non_camel_case_types, non_upper_case_globals)]
mod types {
    pub type seL4_CPtr = usize;
    pub type seL4_Error = isize;
    pub const seL4_CapNull: seL4_CPtr = 0;
    pub const FAILED_LOOKUP: seL4_Error = 6;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct seL4_MessageInfo {
        pub words: [usize; 1],
    }

    impl seL4_MessageInfo {
        #[inline(always)]
        #[must_use]
        pub const fn new(
            label: usize,
            caps_unwrapped: usize,
            extra_caps: usize,
            length: usize,
        ) -> Self {
            let mut value = 0usize;
            value |= (label & 0x0fff_ffff_ffff_ffff) << 12;
            value |= (caps_unwrapped & 0x7) << 9;
            value |= (extra_caps & 0x3) << 7;
            value |= length & 0x7f;
            Self { words: [value] }
        }

        #[inline(always)]
        #[must_use]
        pub const fn get_label(self) -> usize {
            (self.words[0] >> 12) & 0x0fff_ffff_ffff_ffff
        }
    }
}

pub use types::{seL4_CPtr, seL4_CapNull, seL4_Error, seL4_MessageInfo};
/// Error code returned when an IPC attempt targets the null endpoint.
pub const FAILED_LOOKUP_ERROR: seL4_Error = types::FAILED_LOOKUP;

#[inline]
/// Returns `true` when the supplied endpoint capability is non-null.
pub fn ep_is_valid(ep: seL4_CPtr) -> bool {
    ep != seL4_CapNull
}

#[inline]
/// Issues an seL4 send when the endpoint is valid, otherwise reports a lookup failure.
pub fn try_send(ep: seL4_CPtr, info: seL4_MessageInfo) -> Result<(), seL4_Error> {
    if !ep_is_valid(ep) {
        log::warn!("[ipc] skipped: null endpoint.");
        return Err(FAILED_LOOKUP_ERROR);
    }

    #[cfg(feature = "kernel")]
    {
        sel4::send_unchecked(ep, info);
    }

    #[cfg(not(feature = "kernel"))]
    {
        let _ = (ep, info);
    }

    Ok(())
}

#[inline]
/// Issues an seL4 call when the endpoint is valid, otherwise reports a lookup failure.
pub fn try_call(ep: seL4_CPtr, info: seL4_MessageInfo) -> Result<seL4_MessageInfo, seL4_Error> {
    if !ep_is_valid(ep) {
        log::warn!("[ipc] skipped: null endpoint.");
        return Err(FAILED_LOOKUP_ERROR);
    }

    #[cfg(feature = "kernel")]
    {
        return Ok(sel4::call_unchecked(ep, info));
    }

    #[cfg(not(feature = "kernel"))]
    {
        let _ = ep;
        Ok(info)
    }
}

#[inline]
/// Issues an seL4 signal when the endpoint is valid, otherwise reports a lookup failure.
pub fn try_signal(ep: seL4_CPtr) -> Result<(), seL4_Error> {
    if !ep_is_valid(ep) {
        log::warn!("[ipc] skipped: null endpoint.");
        return Err(FAILED_LOOKUP_ERROR);
    }

    #[cfg(feature = "kernel")]
    {
        sel4::signal_unchecked(ep);
    }

    #[cfg(not(feature = "kernel"))]
    {
        let _ = ep;
    }

    Ok(())
}
