// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

use core::fmt::{self, Write};
use core::panic::PanicInfo;

/// seL4 console writer backed by `seL4_DebugPutChar`.
struct DebugConsole;

impl DebugConsole {
    const PREFIX: &'static str = "[cohesix:root-task] ";

    #[inline(always)]
    fn new() -> Self {
        Self
    }

    #[inline(always)]
    fn write_raw(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            unsafe {
                seL4_DebugPutChar(byte);
            }
        }
    }

    fn writeln_prefixed(&mut self, body: &str) {
        let _ = self.write_str(Self::PREFIX);
        let _ = self.write_str(body);
        self.newline();
    }

    fn newline(&mut self) {
        // seL4's serial console expects CRLF for neat output in QEMU.
        self.write_raw(b"\r\n");
    }

    fn report_bootinfo(&mut self, info_ptr: *const BootInfoHeader) {
        if info_ptr.is_null() {
            self.writeln_prefixed("bootinfo pointer is NULL (kernel handover failed)");
            return;
        }

        let addr = info_ptr as usize;
        let header = unsafe { *info_ptr };
        let extra_words = header.extra_len;
        let header_bytes = core::mem::size_of::<BootInfoHeader>();
        let extra_bytes = extra_words * core::mem::size_of::<usize>();
        let extra_start = addr + header_bytes;
        let extra_end = extra_start + extra_bytes;

        let _ = write!(
            self,
            "{prefix}bootinfo @ 0x{addr:016x} (header {header_bytes} bytes)\r\n",
            prefix = Self::PREFIX,
            addr = addr,
            header_bytes = header_bytes,
        );
        let _ = write!(
            self,
            "{prefix}bootinfo.extraLen = {extra_words} words ({extra_bytes} bytes)\r\n",
            prefix = Self::PREFIX,
            extra_words = extra_words,
            extra_bytes = extra_bytes,
        );
        let _ = write!(
            self,
            "{prefix}bootinfo.extra region [0x{start:016x}..0x{end:016x})\r\n",
            prefix = Self::PREFIX,
            start = extra_start,
            end = extra_end,
        );
        let _ = write!(
            self,
            "{prefix}node_id={} nodes={} ipc_buffer=0x{ipc:016x}\r\n",
            prefix = Self::PREFIX,
            header.node_id,
            header.num_nodes,
            ipc = header.ipc_buffer,
        );
    }
}

impl Write for DebugConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_raw(s.as_bytes());
        Ok(())
    }
}

/// Minimal projection of `seL4_BootInfo` used for early diagnostics.
#[repr(C)]
#[derive(Copy, Clone)]
struct BootInfoHeader {
    extra_len: usize,
    node_id: usize,
    num_nodes: usize,
    num_io_pt_levels: usize,
    ipc_buffer: usize,
    init_thread_cnode_size_bits: usize,
    init_thread_domain: usize,
    extra_bi_pages: usize,
}

extern "C" {
    fn seL4_DebugPutChar(character: u8);
}

/// Root task entry point invoked by seL4 after kernel initialisation.
#[no_mangle]
pub extern "C" fn _start(bootinfo: *const BootInfoHeader) -> ! {
    let mut console = DebugConsole::new();
    console.writeln_prefixed("entered from seL4 (stage0)");
    console.report_bootinfo(bootinfo);

    // TODO: replace this placeholder loop with capability bootstrap logic.
    loop {
        core::hint::spin_loop();
    }
}

/// Panic handler that emits diagnostics before halting.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let mut console = DebugConsole::new();
    let _ = write!(
        console,
        "{prefix}panic: {info}\r\n",
        prefix = DebugConsole::PREFIX,
        info = info
    );
    loop {
        core::hint::spin_loop();
    }
}
