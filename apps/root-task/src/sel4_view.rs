// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

use sel4_sys::{seL4_BootInfo, seL4_CPtr, seL4_Word};

#[inline(always)]
pub fn init_cnode_cptr(bi: &seL4_BootInfo) -> seL4_CPtr {
    bi.initThreadCNode as seL4_CPtr
}

#[inline(always)]
pub fn init_cnode_bits(bi: &seL4_BootInfo) -> seL4_Word {
    bi.initThreadCNodeSizeBits as seL4_Word
}

#[inline(always)]
pub fn empty_window(bi: &seL4_BootInfo) -> (seL4_Word, seL4_Word) {
    (bi.empty.start as seL4_Word, bi.empty.end as seL4_Word)
}
