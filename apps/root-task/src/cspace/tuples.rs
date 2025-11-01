// Author: Lukas Bower
//! Canonical tuple helpers for seL4 CSpace operations during early bootstrap.
#![allow(unsafe_code)]

use core::convert::TryInto;
use core::fmt::Write as _;

use crate::bootstrap::cspace_sys::encode_cptr_index;
use crate::sel4::{BootInfoExt, WORD_BITS};
use sel4_sys::{
    seL4_CNode_Copy, seL4_CPtr, seL4_CapRights_All, seL4_DebugPutChar, seL4_EndpointObject,
    seL4_Error, seL4_IPCBuffer, seL4_Untyped_Retype, seL4_Word,
};

/// Tuple describing the canonical addressing required for init CNode operations.
#[derive(Copy, Clone, Debug)]
pub struct CNodeTuple {
    /// Root capability designating the init thread CNode.
    pub root: seL4_CPtr,
    /// Radix width (in bits) of the init thread CNode as reported by bootinfo.
    pub init_bits: u8,
}

impl CNodeTuple {
    #[inline(always)]
    fn word_bits_u8(&self) -> u8 {
        WORD_BITS
            .try_into()
            .expect("WORD_BITS must fit within u8 for canonical CSpace addressing")
    }

    #[inline(always)]
    pub fn guard_depth(&self) -> seL4_Word {
        WORD_BITS as seL4_Word
    }

    #[inline(always)]
    pub fn encode_slot(&self, slot: seL4_Word) -> seL4_Word {
        encode_cptr_index(slot, self.init_bits, self.word_bits_u8())
    }
}

/// Tuple describing the destination parameters for `seL4_Untyped_Retype`.
#[derive(Copy, Clone, Debug)]
pub struct RetypeTuple {
    /// Root capability supplied as `root` to `seL4_Untyped_Retype`.
    pub node_root: seL4_CPtr,
    /// Capability pointer supplied as `nodeIndex` (must equal `node_root`).
    pub node_index: seL4_CPtr,
    /// Destination depth supplied as `nodeDepth` (canonical guard depth = `seL4_WordBits`).
    pub node_depth: u8,
    /// Radix width (in bits) of the init thread CNode as reported by bootinfo.
    pub init_bits: u8,
}

/// Construct the canonical init CNode tuple.
#[inline(always)]
pub fn make_cnode_tuple(init_cnode: seL4_CPtr, init_bits: u8) -> CNodeTuple {
    CNodeTuple {
        root: init_cnode,
        init_bits,
    }
}

/// Construct the canonical tuple for retype destinations.
#[inline(always)]
pub fn make_retype_tuple(init_cnode: seL4_CPtr, init_bits: u8) -> RetypeTuple {
    let guard_depth = WORD_BITS
        .try_into()
        .expect("WORD_BITS must fit within u8 for canonical CSpace addressing");
    RetypeTuple {
        node_root: init_cnode,
        node_index: init_cnode,
        node_depth: guard_depth,
        init_bits,
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

/// Emit a non-blocking proof copy of a capability from the init CNode to validate
/// canonical addressing tuples.
pub fn try_cnode_copy_proof(
    cn: &CNodeTuple,
    slot_free: seL4_Word,
    src_slot: seL4_CPtr,
) -> seL4_Error {
    heartbeat(b'c');
    let guard_depth = cn.guard_depth();
    let encoded_dest = cn.encode_slot(slot_free);
    let encoded_src = cn.encode_slot(src_slot as seL4_Word);
    let depth_word = guard_depth;
    let depth_bits: u8 = depth_word
        .try_into()
        .expect("guard depth must fit within u8 for seL4_CNode_Copy");
    let result = unsafe {
        seL4_CNode_Copy(
            cn.root,
            encoded_dest as seL4_CPtr,
            depth_bits,
            cn.root,
            encoded_src as seL4_CPtr,
            depth_bits,
            seL4_CapRights_All,
        )
    };
    heartbeat(b'C');
    if result != sel4_sys::seL4_NoError {
        debug_puts("[rt-fix] cnode.copy fail #");
        debug_hex(" dest=0x", slot_free);
        debug_hex(" dest_enc=0x", encoded_dest);
        debug_hex(" src=0x", src_slot as seL4_Word);
        debug_hex(" src_enc=0x", encoded_src);
        debug_hex(" depth=0x", depth_word);
        debug_puts("\n");
    }
    result
}

/// Retype a single endpoint object into the supplied slot using canonical arguments.
pub fn retype_endpoint_into_slot(ut: seL4_CPtr, slot: seL4_Word, rt: &RetypeTuple) -> seL4_Error {
    heartbeat(b'r');
    let encoded_slot = encode_cptr_index(slot, rt.init_bits, rt.node_depth);
    let result = unsafe {
        seL4_Untyped_Retype(
            ut,
            seL4_EndpointObject,
            0,
            rt.node_root,
            rt.node_index,
            rt.node_depth as seL4_Word,
            encoded_slot as seL4_CPtr,
            1,
        )
    };
    heartbeat(b'R');
    if result != sel4_sys::seL4_NoError {
        debug_puts("[rt-fix] retype:endpoint fail #");
        debug_hex(" ut=0x", ut);
        debug_hex(" slot=0x", slot);
        debug_hex(" slot_enc=0x", encoded_slot);
        debug_hex(" root=0x", rt.node_root);
        debug_hex(" depth=0x", rt.node_depth as seL4_Word);
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
