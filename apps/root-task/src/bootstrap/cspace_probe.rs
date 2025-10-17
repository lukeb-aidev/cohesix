// Author: Lukas Bower

use sel4_sys as sys;

use super::ffi::{cnode_delete, cnode_mint_allrights};

/// Prove 'slot' is writable by Minting a duplicate of the TCB cap into it, then Delete it.
pub fn probe_slot_writable(
    root: sys::seL4_CPtr,
    depth_bits: u8,
    slot: u32,
) -> Result<(), sys::seL4_Error> {
    let depth = depth_bits as u32;
    let src_root = root;
    let src_index = sys::seL4_CapInitThreadTCB as u64; // a known good source cap in init CSpace

    // Mint a dup with AllRights, badge=0, then delete it.
    let r = unsafe { cnode_mint_allrights(root, slot as u64, depth, src_root, src_index, depth) };
    if r != 0 {
        return Err(r);
    }
    let rd = unsafe { cnode_delete(root, slot as u64, depth) };
    if rd != 0 {
        return Err(rd);
    }
    Ok(())
}
