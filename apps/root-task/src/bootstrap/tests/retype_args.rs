// Author: Lukas Bower

use crate::bootstrap::cspace::DestCNode;
use crate::bootstrap::retype::{call_retype, last_retype_args};
use sel4_sys as sys;

#[test]
fn packs_correct_args_for_root_level_insert() {
    let dest = DestCNode {
        root: 0x2,
        root_bits: 13,
        empty_start: 0x0103,
        empty_end: 0x0200,
        slot_offset: 0x0103,
    };
    dest.assert_sane();

    let rc = call_retype(0xe3, 0x1, 0, &dest, 1);
    assert_eq!(rc, sys::seL4_NoError);

    let last = last_retype_args();
    assert_eq!(last.ut, 0xe3);
    assert_eq!(last.obj, 0x1);
    assert_eq!(last.size_bits, 0);
    assert_eq!(last.root, 0x2);
    assert_eq!(last.idx, 0);
    assert_eq!(last.depth, dest.root_bits);
    assert_eq!(last.off, 0x0103);
    assert_eq!(last.n, 1);
}
