// Author: Lukas Bower
#![allow(unsafe_code)]

use core::fmt::Write;

use crate::bootstrap::cspace_sys;
use crate::sel4;
use crate::trace::{dec_u32, hex_u64, DebugPutc};
use sel4_sys::{
    seL4_CPtr, seL4_Error, seL4_NoError, seL4_ObjectType, seL4_Untyped_Retype, seL4_Word,
};

#[inline]
fn debug_retype_log(
    phase: &'static str,
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
    let _ = writer.write_str(" type=");
    let _ = writer.write_str(sel4::object_type_name(obj_type));
    let _ = writer.write_str(" sz=");
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
        let _ = writer.write_str(" -> err=");
        let _ = writer.write_str(sel4::error_name(error));
    }
    let _ = writer.write_str("\n");

    #[cfg(feature = "bootstrap-trace")]
    {
        crate::trace::bootstrap::record_retype_event(
            phase,
            untyped,
            obj_type,
            size_bits,
            dst_cnode,
            node_index,
            node_depth,
            node_offset,
            num_objects,
            err,
        );
    }
}

/// Retypes an untyped capability, emitting debug traces before and after the kernel call.
#[inline]
pub fn traced_retype_into_slot(
    untyped: seL4_CPtr,
    obj_type: seL4_ObjectType,
    size_bits: u32,
    dst_root: seL4_CPtr,
    node_index: seL4_CPtr,
    node_depth: u8,
    node_offset: seL4_CPtr,
) -> Result<(), seL4_Error> {
    if dst_root == sel4::seL4_CapInitThreadCNode {
        let slot = if node_offset != 0 {
            node_offset
        } else {
            node_index
        };
        let (root, canonical_index, canonical_depth, canonical_offset) =
            cspace_sys::init_cnode_retype_dest(slot);
        debug_assert_eq!(root, dst_root);
        debug_retype_log(
            "pre",
            untyped,
            obj_type,
            size_bits,
            dst_root,
            canonical_index as seL4_CPtr,
            canonical_depth as u8,
            canonical_offset as seL4_CPtr,
            1,
            None,
        );

        let result = cspace_sys::untyped_retype_into_init_root(
            untyped,
            obj_type as seL4_Word,
            size_bits as seL4_Word,
            slot,
        );

        debug_retype_log(
            "post",
            untyped,
            obj_type,
            size_bits,
            dst_root,
            canonical_index as seL4_CPtr,
            canonical_depth as u8,
            canonical_offset as seL4_CPtr,
            1,
            Some(result),
        );

        if result == seL4_NoError {
            Ok(())
        } else {
            Err(result)
        }
    } else {
        debug_retype_log(
            "pre",
            untyped,
            obj_type,
            size_bits,
            dst_root,
            node_index,
            node_depth,
            node_offset,
            1,
            None,
        );

        let result = unsafe {
            seL4_Untyped_Retype(
                untyped,
                obj_type as seL4_Word,
                size_bits as seL4_Word,
                dst_root,
                node_index,
                node_depth as seL4_Word,
                node_offset,
                1,
            )
        };

        debug_retype_log(
            "post",
            untyped,
            obj_type,
            size_bits,
            dst_root,
            node_index,
            node_depth,
            node_offset,
            1,
            Some(result),
        );

        if result == seL4_NoError {
            Ok(())
        } else {
            Err(result)
        }
    }
}

/// Retypes an untyped capability into an endpoint object at the destination slot.
pub fn retype_endpoint_into_slot(
    untyped: seL4_CPtr,
    dst_root: seL4_CPtr,
    node_index: seL4_CPtr,
    node_depth: u8,
    node_offset: seL4_CPtr,
) -> Result<(), seL4_Error> {
    traced_retype_into_slot(
        untyped,
        sel4_sys::seL4_ObjectType::seL4_EndpointObject,
        0,
        dst_root,
        node_index,
        node_depth,
        node_offset,
    )
}
