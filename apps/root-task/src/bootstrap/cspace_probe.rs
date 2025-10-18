// Author: Lukas Bower

use sel4_sys as sys;

use super::ffi::{cnode_delete, cnode_mint_allrights};

/// Bit width of `seL4_Word` used when traversing single-level CNodes.
const WORD_BITS: u8 = (core::mem::size_of::<sys::seL4_Word>() * 8) as u8;

/// Prove `slot` is writable by Minting a duplicate of the TCB cap into it, then Delete it.
///
/// The init thread CSpace is a single-level CNode, so both the source and destination paths
/// must supply the full machine word width when invoking CNode operations. Using the root
/// CNode's size bits here would truncate the path and trigger `seL4_IllegalOperation`.
pub fn probe_slot_writable(
    root: sys::seL4_CPtr,
    slot: sys::seL4_CPtr,
) -> Result<(), sys::seL4_Error> {
    let src_root = root;
    let src_index = sys::seL4_CapInitThreadTCB; // a known good source cap in init CSpace

    // Mint a dup with AllRights, badge=0, then delete it.
    let r = cnode_mint_allrights(root, slot, WORD_BITS, src_root, src_index, WORD_BITS);
    if r != sys::seL4_NoError {
        return Err(r);
    }
    let rd = cnode_delete(root, slot, WORD_BITS);
    if rd != sys::seL4_NoError {
        return Err(rd);
    }
    Ok(())
}
