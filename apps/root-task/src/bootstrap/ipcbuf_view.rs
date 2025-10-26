// Author: Lukas Bower

use core::slice;

/// Immutable view over the single-page seL4 IPC buffer.
#[derive(Clone, Copy)]
pub struct IpcBufView {
    base: *const u8,
}

impl IpcBufView {
    /// Number of bits in the IPC buffer page size as reported by the kernel.
    pub const PAGE_BITS: usize = sel4_sys::seL4_PageBits as usize;
    /// Number of bytes spanned by the IPC buffer page.
    pub const PAGE_LEN: usize = 1 << Self::PAGE_BITS;

    /// Construct a new view over the IPC buffer at the supplied base pointer.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `base` points to the first byte of the
    /// mapped IPC buffer page and remains valid for the lifetime of the view.
    pub const unsafe fn new(base: *const u8) -> Self {
        Self { base }
    }

    /// Returns the IPC buffer contents as a byte slice capped to a single page.
    #[inline(always)]
    pub fn as_bytes(&self) -> &'static [u8] {
        assert!(!self.base.is_null(), "IPC buffer base must be non-null");
        unsafe { slice::from_raw_parts(self.base, Self::PAGE_LEN) }
    }

    /// Returns a prefix of the IPC buffer bounded by the provided length.
    #[inline(always)]
    pub fn prefix(&self, len: usize) -> &'static [u8] {
        let limit = len.min(Self::PAGE_LEN);
        &self.as_bytes()[..limit]
    }
}
