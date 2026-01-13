// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the bootstrap/ipcbuf module for root-task.
// Author: Lukas Bower
#![allow(dead_code)]

use crate::bootstrap::ipcbuf_view::IpcBufView;
use crate::bp;
use crate::sel4::KernelEnv;

/// Maps the init thread IPC buffer and binds it to the provided TCB.
/// Assumes the init thread's CSpace and VSpace are intact, uses the boot-provided
/// `initThreadIPCBuffer` slot when possible, and does not allocate additional CNode
/// slots beyond the kernel-advertised empty region.
#[allow(clippy::missing_errors_doc)]
pub fn install_ipc_buffer(
    env: &mut KernelEnv<'_>,
    tcb_cap: sel4_sys::seL4_CPtr,
    ipc_frame: sel4_sys::seL4_CPtr,
    ipc_vaddr: usize,
) -> Result<IpcBufView, i32> {
    bp!("ipcbuf.begin");
    ::log::trace!(
        "B2: binding init IPC buffer vaddr=0x{ipc_vaddr:08x}",
        ipc_vaddr = ipc_vaddr,
    );

    env.log_ipc_buffer_cap(ipc_frame, ipc_vaddr);

    if tcb_cap == sel4_sys::seL4_CapInitThreadTCB
        && ipc_frame == sel4_sys::seL4_CapInitThreadIPCBuffer
    {
        ::log::info!(
            "[boot] using boot-provided IPC buffer without re-binding: tcb=0x{tcb_cap:04x} frame=0x{ipc_frame:04x} vaddr=0x{ipc_vaddr:08x}"
        );
        let view = env.record_boot_ipc_buffer(ipc_frame, ipc_vaddr);
        bp!("ipcbuf.done");
        return Ok(view);
    }

    match env.bind_ipc_buffer(tcb_cap, ipc_frame, ipc_vaddr) {
        Ok(view) => {
            bp!("tcb.set_ipcbuf");
            bp!("ipcbuf.done");
            Ok(view)
        }
        Err(err) => {
            let code = err as i32;
            ::log::error!(
                "[boot] ipcbuf.bind failed tcb=0x{tcb_cap:04x} vaddr=0x{ipc_vaddr:08x} err={code} ({name})",
                name = crate::sel4::error_name(err)
            );
            Err(code)
        }
    }
}
