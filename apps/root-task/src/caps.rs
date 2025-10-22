// Author: Lukas Bower
#![allow(unsafe_code)]

use core::fmt::Write;

use crate::trace::{dec_u32, hex_u64, DebugPutc};
use sel4_sys::{
    seL4_CNode, seL4_CPtr, seL4_Error, seL4_NoError, seL4_ObjectType, seL4_Untyped_Retype,
    seL4_Word,
};

#[inline]
fn debug_retype_log(
    phase: &str,
    untyped: seL4_CPtr,
    obj_type: seL4_ObjectType,
    size_bits: u32,
    dst_cnode: seL4_CPtr,
    node_index: seL4_CPtr,
    node_depth: u8,
    node_offset: seL4_CPtr,
    num_objects: u32,
    err: Option<seL4_Error>,
) {
    let mut writer = DebugPutc;
    let _ = write!(writer, "[retype:{phase}] ut=");
    hex_u64(&mut writer, untyped as u64);
    let _ = write!(writer, " type={:?} sz=", obj_type);
    dec_u32(&mut writer, size_bits);
    let _ = write!(writer, " root=");
    hex_u64(&mut writer, dst_cnode as u64);
    let _ = write!(writer, " idx=");
    hex_u64(&mut writer, node_index as u64);
    let _ = write!(writer, " depth=");
    dec_u32(&mut writer, u32::from(node_depth));
    let _ = write!(writer, " off=");
    hex_u64(&mut writer, node_offset as u64);
    let _ = write!(writer, " n=");
    dec_u32(&mut writer, num_objects);
    if let Some(error) = err {
        let _ = write!(writer, " -> err={:?}", error);
    }
    let _ = writer.write_str("\n");
}

/// Retypes an untyped capability, emitting debug traces before and after the kernel call.
#[inline]
pub fn traced_retype_into_slot(
    untyped: seL4_CPtr,
    obj_type: seL4_ObjectType,
    size_bits: u32,
    dst_cnode: seL4_CPtr,
    dst_slot: seL4_CPtr,
) -> Result<(), seL4_Error> {
    debug_retype_log(
        "pre", untyped, obj_type, size_bits, dst_cnode, 0, 0, dst_slot, 1, None,
    );

    let result = unsafe {
        seL4_Untyped_Retype(
            untyped,
            obj_type as seL4_Word,
            size_bits as seL4_Word,
            dst_cnode,
            0,
            0,
            dst_slot,
            1,
        )
    };

    debug_retype_log(
        "post",
        untyped,
        obj_type,
        size_bits,
        dst_cnode,
        0,
        0,
        dst_slot,
        1,
        Some(result),
    );

    if result == seL4_NoError {
        Ok(())
    } else {
        Err(result)
    }
}

/// Retypes an untyped capability into an endpoint object at the destination slot.
pub fn retype_endpoint_into_slot(
    untyped: seL4_CPtr,
    dst_cnode: seL4_CPtr,
    dst_slot: seL4_CPtr,
) -> Result<(), seL4_Error> {
    traced_retype_into_slot(
        untyped,
        sel4_sys::seL4_ObjectType::seL4_EndpointObject,
        0,
        dst_cnode,
        dst_slot,
    )
}
