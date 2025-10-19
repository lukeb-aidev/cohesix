// Author: Lukas Bower

use sel4_sys as sys;

use super::cspace::CSpaceCtx;

/// Retypes a single kernel object from an untyped capability into the init CSpace.
pub fn retype_one(
    ctx: &mut CSpaceCtx,
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_ObjectType,
    obj_bits: u8,
) -> Result<sys::seL4_CPtr, sys::seL4_Error> {
    let slot = match ctx.alloc_slot_checked() {
        Ok(slot) => slot,
        Err(err) => {
            ctx.log_slot_failure(err);
            return Err(sys::seL4_RangeError);
        }
    };

    let err = ctx.retype_to_slot(
        untyped,
        obj_type as sys::seL4_Word,
        obj_bits as sys::seL4_Word,
        slot,
    );
    if err != sys::seL4_NoError {
        return Err(err);
    }

    Ok(slot)
}
