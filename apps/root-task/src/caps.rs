// Author: Lukas Bower
#![allow(unsafe_code)]

use core::fmt::{self, Write};

use crate::bootstrap::cspace_sys::{self, RetypeArgs, RetypeCallError};
use crate::sel4;
use crate::trace::DebugPutc;
use sel4_sys::{
    seL4_CPtr, seL4_Error, seL4_NoError, seL4_ObjectType, seL4_Untyped_Retype, seL4_Word,
};

const RETYPE_LOG_ENABLED: bool = false;

#[cfg(any(test, feature = "test-support"))]
use heapless::String as HeaplessString;

#[cfg(any(test, feature = "test-support"))]
const RETYPE_LOG_CAPACITY: usize = 160;

fn write_retype_line<W: fmt::Write>(
    writer: &mut W,
    phase: &str,
    args: &RetypeArgs,
    err: Option<seL4_Error>,
) -> fmt::Result {
    write!(
        writer,
        "[retype:{phase}] ut=0x{ut:016x} type=0x{ty:08x} sz={size} root=0x{root:016x} idx=0x{idx:08x} depth={depth} off=0x{off:08x} n={num}",
        phase = phase,
        ut = args.ut as u64,
        ty = args.objtype,
        size = args.size_bits,
        root = args.root as u64,
        idx = args.node_index as u32,
        depth = args.cnode_depth,
        off = args.dest_offset as u32,
        num = args.num_objects,
    )?;
    if let Some(code) = err {
        write!(writer, " err=0x{code:08x}", code = code as u32)?;
    }
    Ok(())
}

#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn render_retype_log_line(
    phase: &str,
    args: &RetypeArgs,
    err: Option<seL4_Error>,
) -> HeaplessString<{ RETYPE_LOG_CAPACITY }> {
    let mut line = HeaplessString::<RETYPE_LOG_CAPACITY>::new();
    if write_retype_line(&mut line, phase, args, err).is_err() {
        // Truncated; retain the partial diagnostic without retrying.
    }
    line
}

fn debug_retype_log(
    phase: &'static str,
    args: &RetypeArgs,
    obj_type: seL4_ObjectType,
    err: Option<seL4_Error>,
) {
    if !RETYPE_LOG_ENABLED {
        return;
    }
    let mut writer = DebugPutc;
    if write_retype_line(&mut writer, phase, args, err).is_err() {
        // UART output is best-effort; ignore truncation.
    }
    let _ = writer.write_char('\n');

    #[cfg(feature = "bootstrap-trace")]
    {
        crate::trace::bootstrap::record_retype_event(
            phase,
            args.ut,
            obj_type,
            args.size_bits as u32,
            args.root,
            args.node_index,
            args.cnode_depth,
            args.dest_offset,
            args.num_objects,
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
        let pre_args = RetypeArgs::new(
            untyped,
            obj_type as seL4_Word,
            size_bits as seL4_Word,
            dst_root,
            canonical_index as seL4_CPtr,
            canonical_depth,
            canonical_offset as seL4_CPtr,
            1,
        );
        debug_retype_log("pre", &pre_args, obj_type, None);

        let result = cspace_sys::untyped_retype_into_init_root(
            untyped,
            obj_type as seL4_Word,
            size_bits as seL4_Word,
            slot,
        );

        let post_err = result.as_ref().err().map(|err| (*err).into_sel4_error());
        debug_retype_log("post", &pre_args, obj_type, post_err);

        result.map_err(RetypeCallError::into_sel4_error)
    } else {
        let pre_args = RetypeArgs::new(
            untyped,
            obj_type as seL4_Word,
            size_bits as seL4_Word,
            dst_root,
            node_index,
            node_depth as seL4_Word,
            node_offset,
            1,
        );
        debug_retype_log("pre", &pre_args, obj_type, None);

        let result = unsafe {
            seL4_Untyped_Retype(
                untyped,
                obj_type as seL4_Word,
                size_bits as seL4_Word,
                dst_root,
                node_index,
                u64::from(node_depth),
                node_offset,
                1,
            )
        };

        debug_retype_log("post", &pre_args, obj_type, Some(result));

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
        sel4_sys::seL4_EndpointObject,
        0,
        dst_root,
        node_index,
        node_depth,
        node_offset,
    )
}
