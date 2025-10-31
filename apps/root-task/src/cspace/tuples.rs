// Author: Lukas Bower
//! Canonical tuple helpers for seL4 CSpace operations during early bootstrap.
#![allow(unsafe_code)]

use core::fmt::Write as _;

use sel4_sys::{
    seL4_AllRights, seL4_CNode_Copy, seL4_CPtr, seL4_DebugPutChar, seL4_Error, seL4_IPCBuffer,
    seL4_Untyped_Retype, seL4_Word,
};

/// Tuple describing the canonical addressing required for init CNode operations.
#[derive(Copy, Clone, Debug)]
pub struct CNodeTuple {
    /// Root capability designating the init thread CNode.
    pub root: seL4_CPtr,
    /// Depth (in bits) required when addressing slots in the init CNode.
    pub depth: u8,
}

/// Tuple describing the destination parameters for `seL4_Untyped_Retype`.
#[derive(Copy, Clone, Debug)]
pub struct RetypeTuple {
    /// Root capability supplied as `root` to `seL4_Untyped_Retype`.
    pub node_root: seL4_CPtr,
    /// Capability pointer supplied as `nodeIndex` (must equal `node_root`).
    pub node_index: seL4_CPtr,
    /// Destination depth supplied as `nodeDepth` (always zero for init CNode).
    pub node_depth: u8,
}

/// Construct the canonical init CNode tuple.
#[inline(always)]
pub fn make_cnode_tuple(init_cnode: seL4_CPtr, init_bits: u8) -> CNodeTuple {
    CNodeTuple {
        root: init_cnode,
        depth: init_bits,
    }
}

/// Construct the canonical tuple for retype destinations.
#[inline(always)]
pub fn make_retype_tuple(init_cnode: seL4_CPtr) -> RetypeTuple {
    RetypeTuple {
        node_root: init_cnode,
        node_index: init_cnode,
        node_depth: 0,
    }
}

fn debug_puts(message: &str) {
    for &byte in message.as_bytes() {
        unsafe {
            seL4_DebugPutChar(byte);
        }
    }
}

fn debug_hex(label: &str, value: seL4_Word) {
    let mut buffer = heapless::String::<32>::new();
    let _ = write!(buffer, "{label}{value:016x}");
    debug_puts(buffer.as_str());
}

fn heartbeat(tag: u8) {
    unsafe {
        seL4_DebugPutChar(tag);
    }
}

/// Emit a non-blocking proof copy of a capability to validate addressing tuples.
pub fn try_cnode_copy_proof(
    cn: &CNodeTuple,
    slot_free: seL4_Word,
    src_cap: seL4_CPtr,
) -> seL4_Error {
    heartbeat(b'c');
    let result = unsafe {
        seL4_CNode_Copy(
            cn.root,
            slot_free,
            cn.depth as seL4_Word,
            cn.root,
            src_cap,
            cn.depth as seL4_Word,
            seL4_AllRights,
        )
    };
    heartbeat(b'C');
    if result != sel4_sys::seL4_NoError {
        debug_puts("[rt-fix] cnode.copy fail #");
        debug_hex(" dest=0x", slot_free);
        debug_hex(" src=0x", src_cap);
        debug_hex(" depth=0x", cn.depth as seL4_Word);
        debug_puts("\n");
    }
    result
}

/// Retype a single endpoint object into the supplied slot using canonical arguments.
pub fn retype_endpoint_into_slot(ut: seL4_CPtr, slot: seL4_Word, rt: &RetypeTuple) -> seL4_Error {
    heartbeat(b'r');
    let result = unsafe {
        seL4_Untyped_Retype(
            ut,
            sel4_sys::seL4_ObjectType_seL4_EndpointObject,
            0,
            rt.node_root,
            rt.node_index,
            rt.node_depth as seL4_Word,
            slot,
            1,
        )
    };
    heartbeat(b'R');
    if result != sel4_sys::seL4_NoError {
        debug_puts("[rt-fix] retype:endpoint fail #");
        debug_hex(" ut=0x", ut);
        debug_hex(" slot=0x", slot);
        debug_hex(" root=0x", rt.node_root);
        debug_puts("\n");
    }
    result
}

/// Validate that the kernel-reported IPC buffer matches the runtime accessor.
pub fn assert_ipc_buffer_matches_bootinfo(bootinfo: &sel4_sys::seL4_BootInfo) {
    unsafe {
        let ipc_ptr = sel4_sys::seL4_GetIPCBuffer() as *const seL4_IPCBuffer as usize;
        let bi_ptr = bootinfo
            .ipc_buffer_ptr()
            .map_or(0usize, |ptr| ptr.as_ptr() as usize);
        if ipc_ptr != bi_ptr {
            seL4_DebugPutChar(b'!');
            seL4_DebugPutChar(b'!');
            seL4_DebugPutChar(b'!');
            panic!(
                "[ipc] seL4_GetIPCBuffer()=0x{ipc_ptr:016x} != bootinfo.ipcBuffer=0x{bi_ptr:016x}",
            );
        }
    }
}
