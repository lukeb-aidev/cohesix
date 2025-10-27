// Author: Lukas Bower
#![allow(dead_code)]

use crate::bootstrap::ipcbuf_view::IpcBufView;
use crate::bp;
use crate::sel4::KernelEnv;

/// Maps the init thread IPC buffer and binds it to the provided TCB.
#[allow(clippy::missing_errors_doc)]
pub fn install_ipc_buffer(
    env: &mut KernelEnv<'_>,
    tcb_cap: sel4_sys::seL4_CPtr,
    ipc_vaddr: usize,
) -> Result<IpcBufView, i32> {
    bp!("ipcbuf.begin");
    ::log::trace!(
        "B2: about to map IPC buffer vaddr=0x{ipc_vaddr:08x}",
        ipc_vaddr = ipc_vaddr,
    );
    match env.map_ipc_buffer(ipc_vaddr) {
        Ok(()) => {
            ::log::trace!("B2.ret = Ok");
        }
        Err(err) => {
            ::log::trace!("B2.ret = Err({name})", name = crate::sel4::error_name(err));
            let code = err as i32;
            ::log::error!(
                "[boot] ipcbuf.map failed vaddr=0x{ipc_vaddr:08x} err={code} ({name})",
                name = crate::sel4::error_name(err)
            );
            return Err(code);
        }
    }

    match env.bind_ipc_buffer(tcb_cap, ipc_vaddr) {
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
