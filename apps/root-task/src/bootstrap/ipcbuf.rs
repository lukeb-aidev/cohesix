// Author: Lukas Bower
#![allow(dead_code)]

use crate::bootstrap::ktry;
use crate::bp;
use crate::sel4::KernelEnv;

/// Maps the init thread IPC buffer and binds it to the provided TCB.
#[allow(clippy::missing_errors_doc)]
pub fn install_ipc_buffer(
    env: &mut KernelEnv<'_>,
    tcb_cap: sel4_sys::seL4_CPtr,
    ipc_vaddr: usize,
) -> Result<(), i32> {
    bp!("ipcbuf.begin");
    match env.map_ipc_buffer(ipc_vaddr) {
        Ok(()) => {}
        Err(err) => {
            let code = err as i32;
            log::error!(
                "[boot] ipcbuf.map failed vaddr=0x{ipc_vaddr:08x} err={code} ({name})",
                name = crate::sel4::error_name(err)
            );
            return Err(code);
        }
    }

    let rc = unsafe {
        sel4_sys::seL4_TCB_SetIPCBuffer(tcb_cap, ipc_vaddr, sel4_sys::seL4_CapInitThreadIPCBuffer)
    };
    ktry("tcb.set_ipcbuf", rc as i32)?;
    bp!("tcb.set_ipcbuf");
    bp!("ipcbuf.done");
    Ok(())
}
