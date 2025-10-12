// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

#[cfg(target_arch = "aarch64")]
use core::arch::global_asm;

use core::fmt::{self, Write};
use core::panic::PanicInfo;

/// seL4 console writer backed by the kernel's `DebugPutChar` system call.
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
            emit_debug_char(byte);
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
            "{prefix}node_id={node_id} nodes={nodes} ipc_buffer=0x{ipc:016x}\r\n",
            prefix = Self::PREFIX,
            node_id = header.node_id,
            nodes = header.num_nodes,
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
pub struct BootInfoHeader {
    extra_len: usize,
    node_id: usize,
    num_nodes: usize,
    num_io_pt_levels: usize,
    ipc_buffer: usize,
    init_thread_cnode_size_bits: usize,
    init_thread_domain: usize,
    extra_bi_pages: usize,
}

#[cfg(not(target_arch = "aarch64"))]
compile_error!("root-task kernel build currently supports only aarch64 targets");

#[cfg(target_arch = "aarch64")]
const ROOT_STACK_SIZE: usize = 16 * 1024;

#[cfg(target_arch = "aarch64")]
global_asm!(
    r#"
    .section .bss.cohesix_root_stack,"aw",@nobits
    .align 16
__cohesix_root_stack:
    .space {stack_size}
__cohesix_root_stack_end:

    .section .text._start,"ax",@progbits
    .global _start
    .type _start,%function
_start:
    adrp    x1, __cohesix_root_stack_end
    add     x1, x1, :lo12:__cohesix_root_stack_end
    mov     sp, x1
    b       kernel_start
    .size _start, . - _start
"#,
    stack_size = const ROOT_STACK_SIZE,
);

/// Root task entry point invoked by seL4 after kernel initialisation.
#[no_mangle]
pub extern "C" fn kernel_start(bootinfo: *const BootInfoHeader) -> ! {
    let mut console = DebugConsole::new();
    console.writeln_prefixed("entered from seL4 (stage0)");
    console.report_bootinfo(bootinfo);

    console.writeln_prefixed("Cohesix v0 (AArch64/virt)");

    for tick in 1..=HEARTBEAT_TICKS {
        busy_wait_cycles(BUSY_WAIT_CYCLES);
        let _ = write!(
            console,
            "{prefix}tick: {tick}\r\n",
            prefix = DebugConsole::PREFIX,
            tick = tick,
        );
    }

    console.writeln_prefixed("PING");
    console.writeln_prefixed("PONG");
    console.writeln_prefixed("root task idling");

    // TODO: replace this placeholder loop with capability bootstrap logic.
    loop {
        core::hint::spin_loop();
    }
}

#[inline(always)]
fn emit_debug_char(byte: u8) {
    #[cfg(all(target_os = "none", target_arch = "aarch64"))]
    unsafe {
        arch::debug_put_char(byte);
    }

    #[cfg(not(all(target_os = "none", target_arch = "aarch64")))]
    {
        let _ = byte;
    }
}

#[cfg(all(target_os = "none", target_arch = "aarch64"))]
mod arch {
    use core::arch::asm;

    const SYS_DEBUG_PUT_CHAR: usize = (-9i64) as usize;

    #[inline(always)]
    pub unsafe fn debug_put_char(byte: u8) {
        asm!(
            "svc #0",
            in("x0") byte as usize,
            in("x1") 0usize,
            in("x2") 0usize,
            in("x3") 0usize,
            in("x4") 0usize,
            in("x5") 0usize,
            in("x7") SYS_DEBUG_PUT_CHAR,
            options(nostack),
        );
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

const HEARTBEAT_TICKS: usize = 3;
const BUSY_WAIT_CYCLES: usize = 5_000_000;

#[inline(never)]
fn busy_wait_cycles(cycles: usize) {
    for _ in 0..cycles {
        core::hint::spin_loop();
    }
}
