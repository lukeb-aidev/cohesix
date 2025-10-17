// Author: Lukas Bower

use sel4_sys as sys;

use core::convert::TryFrom;

use super::ffi::{cnode_delete, cnode_mint_allrights};

/// Prove 'slot' is writable by Minting a duplicate of the TCB cap into it, then Delete it.
pub fn probe_slot_writable(
    root: sys::seL4_CPtr,
    depth_bits: u8,
    slot: u32,
) -> Result<(), sys::seL4_Error> {
    let src_root = root;
    let src_index = sys::seL4_CapInitThreadTCB; // a known good source cap in init CSpace
    let slot_index = usize::try_from(slot).map_err(|_| sys::seL4_RangeError)?;

    // Mint a dup with AllRights, badge=0, then delete it.
    let r = cnode_mint_allrights(
        root, slot_index, depth_bits, src_root, src_index, depth_bits,
    );
    if r != sys::seL4_NoError {
        return Err(r);
    }
    let rd = cnode_delete(root, slot_index, depth_bits);
    if rd != sys::seL4_NoError {
        return Err(rd);
    }
    Ok(())
}
