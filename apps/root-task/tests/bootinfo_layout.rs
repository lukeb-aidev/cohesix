// Author: Lukas Bower
#![cfg(feature = "kernel")]

use core::mem::size_of;

use root_task::bootstrap::bootinfo_snapshot::BootInfoSnapshot;
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

fn synthetic_bootinfo_with_extra(
    bits: u8,
    empty_start: usize,
    empty_end: usize,
    extra_len: usize,
    extra_pages: (usize, usize),
) -> seL4_BootInfo {
    let mut bi = synthetic_bootinfo(bits, empty_start, empty_end);
    bi.extraLen = extra_len as sel4_sys::seL4_Word;
    bi.extraBIPages = seL4_SlotRegion {
        start: extra_pages.0 as sel4_sys::seL4_CPtr,
        end: extra_pages.1 as sel4_sys::seL4_CPtr,
    };
    bi
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

#[test]
fn bootinfo_extra_range_respects_snapshot_layout() {
    const BACKING_BYTES: usize = 0x4000;
    const HEADER_OFFSET: usize = 0x940;
    const EXTRA_LEN: usize = 0x1e21;

    let mut backing: Box<[u8]> = vec![0u8; BACKING_BYTES].into_boxed_slice();
    let header_ptr = unsafe { backing.as_mut_ptr().add(HEADER_OFFSET) as *mut seL4_BootInfo };

    unsafe {
        core::ptr::write(
            header_ptr,
            synthetic_bootinfo_with_extra(13, 0x0103, 0x2000, EXTRA_LEN, (1, 2)),
        );
    }

    let leaked_backing: &'static mut [u8] = Box::leak(backing);
    let header_ref: &'static seL4_BootInfo = unsafe { &*header_ptr };

    let view = root_task::sel4::BootInfoView::new(header_ref)
        .expect("bootinfo view must not reject mapped extra range");

    let header_size = core::mem::size_of::<seL4_BootInfo>();
    let expected_start = header_ref as *const _ as usize + header_size;
    let expected_end = expected_start + EXTRA_LEN;
    let page_base = (header_ref as *const _ as usize) & !(root_task::sel4::IPC_PAGE_BYTES - 1);
    let required_bytes = expected_end - page_base;
    let expected_limit = page_base
        + ((required_bytes + root_task::sel4::IPC_PAGE_BYTES - 1)
            & !(root_task::sel4::IPC_PAGE_BYTES - 1));

    assert_eq!(view.extra_range().start, expected_start);
    assert_eq!(view.extra_range().end, expected_end);
    assert_eq!(view.extra_limit(), expected_limit);
    assert!(view.extra_limit() >= view.extra_range().end);

    let snapshot = BootInfoSnapshot::from_view(&view).expect("snapshot must succeed");
    assert_eq!(snapshot.extra_end, snapshot.post_canary_addr());

    drop(leaked_backing);
}
