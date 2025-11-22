// Author: Lukas Bower
#![cfg(feature = "kernel")]

use core::mem::size_of;

use root_task::bootstrap::cspace_sys::{cnode_depth, enc_index, TupleStyle};
use root_task::sel4::BootInfoExt;
use sel4_sys::{seL4_BootInfo, seL4_SlotRegion, seL4_UntypedDesc, MAX_BOOTINFO_UNTYPEDS};

fn zero_untyped_desc() -> seL4_UntypedDesc {
    seL4_UntypedDesc {
        paddr: 0,
        sizeBits: 0,
        isDevice: 0,
        padding: [0; size_of::<sel4_sys::seL4_Word>() - 2],
    }
}

fn synthetic_bootinfo(bits: u8, empty_start: usize, empty_end: usize) -> seL4_BootInfo {
    seL4_BootInfo {
        extraLen: 0,
        nodeId: 0,
        numNodes: 1,
        numIOPTLevels: 0,
        ipcBuffer: core::ptr::null_mut(),
        empty: seL4_SlotRegion {
            start: empty_start as sel4_sys::seL4_CPtr,
            end: empty_end as sel4_sys::seL4_CPtr,
        },
        sharedFrames: seL4_SlotRegion { start: 0, end: 0 },
        userImageFrames: seL4_SlotRegion { start: 0, end: 0 },
        userImagePaging: seL4_SlotRegion { start: 0, end: 0 },
        ioSpaceCaps: seL4_SlotRegion { start: 0, end: 0 },
        extraBIPages: seL4_SlotRegion { start: 0, end: 0 },
        initThreadCNodeSizeBits: bits,
        _padding_init_cnode_bits: [0; size_of::<sel4_sys::seL4_Word>() - 1],
        initThreadDomain: 0,
        #[cfg(sel4_config_kernel_mcs)]
        schedcontrol: seL4_SlotRegion { start: 0, end: 0 },
        untyped: seL4_SlotRegion { start: 0, end: 0 },
        untypedList: [zero_untyped_desc(); MAX_BOOTINFO_UNTYPEDS],
    }
}

#[test]
fn bootinfo_exposes_init_bits() {
    let bi = synthetic_bootinfo(13, 0x0103, 0x2000);
    assert_eq!(bi.init_cnode_bits(), 13);
    assert_eq!(bi.init_cnode_depth(), 13);
}

#[test]
fn raw_tuple_encodes_direct_indices() {
    let bi = synthetic_bootinfo(13, 0x0103, 0x2000);
    let depth = cnode_depth(&bi, TupleStyle::Raw);
    assert_eq!(depth, 13);
    let encoded = enc_index(0x0103, &bi, TupleStyle::Raw);
    assert_eq!(encoded, 0x0103);
}

#[test]
fn guard_encoded_matches_word_bits_shift() {
    let bi = synthetic_bootinfo(13, 0x0001, 0x2000);
    let depth = cnode_depth(&bi, TupleStyle::GuardEncoded);
    assert_eq!(depth, sel4_sys::seL4_WordBits);
    let encoded = enc_index(0x1, &bi, TupleStyle::GuardEncoded);
    let shift = (sel4_sys::seL4_WordBits as u8) - 13;
    assert_eq!(encoded, (0x1u64) << shift);
}
