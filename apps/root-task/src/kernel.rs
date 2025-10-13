// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

#[cfg(target_arch = "aarch64")]
use core::arch::global_asm;

use core::fmt::{self, Write};
use core::mem;
use core::panic::PanicInfo;
use core::ptr;

use cohesix_ticket::Role;

use crate::event::{AuditSink, EventPump, IpcDispatcher, TickEvent, TicketTable, TimerSource};
#[cfg(feature = "net")]
use crate::net::NetStack;
use crate::sel4::{KernelEnv, RetypeKind, RetypeStatus};
use crate::serial::{
    pl011::Pl011, SerialPort, DEFAULT_LINE_CAPACITY, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY,
};
#[cfg(feature = "net")]
use smoltcp::wire::Ipv4Address;

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
const PL011_PADDR: usize = 0x0900_0000;
const DEVICE_FRAME_BITS: usize = 12;

#[cfg(all(target_os = "none", target_arch = "aarch64"))]
static mut TLS_IMAGE: sel4_sys::TlsImage = sel4_sys::TlsImage::new();

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
    console.writeln_prefixed("Cohesix boot: root-task online");
    console.report_bootinfo(bootinfo);

    console.writeln_prefixed("Cohesix v0 (AArch64/virt)");

    let bootinfo_ref = unsafe { &*(bootinfo as *const sel4_sys::seL4_BootInfo) };

    let empty = bootinfo_ref.empty;
    let mut cnode_line = heapless::String::<160>::new();
    let empty_span = (empty.end - empty.start) as usize;
    let _ = write!(
        cnode_line,
        "bootinfo.empty slots [0x{start:04x}..0x{end:04x}) span={span} root_cnode_bits={bits}",
        start = empty.start,
        end = empty.end,
        span = empty_span,
        bits = bootinfo_ref.initThreadCNodeSizeBits,
    );
    console.writeln_prefixed(cnode_line.as_str());
    unsafe {
        #[cfg(all(target_os = "none", target_arch = "aarch64"))]
        {
            sel4_sys::tls_set_base(core::ptr::addr_of_mut!(TLS_IMAGE));
            debug_assert!(
                sel4_sys::tls_image_mut().is_some(),
                "TLS base must resolve to an image after installation",
            );
        }

        let ipc_ptr = bootinfo_ref.ipcBuffer as *mut sel4_sys::seL4_IPCBuffer;
        ptr::write_bytes(
            ipc_ptr.cast::<u8>(),
            0,
            mem::size_of::<sel4_sys::seL4_IPCBuffer>(),
        );
        sel4_sys::seL4_SetIPCBuffer(ipc_ptr);
        let mut msg = heapless::String::<64>::new();
        let _ = write!(msg, "ipc buffer ptr=0x{:016x}", ipc_ptr as usize);
        console.writeln_prefixed(msg.as_str());
    }
    let mut env = KernelEnv::new(bootinfo_ref);

    #[cfg(target_os = "none")]
    let mut ninedoor = crate::ninedoor::NineDoorBridge::new();

    let uart_region = match env.map_device(PL011_PADDR) {
        Ok(region) => region,
        Err(err) => {
            let error_code = err as i32;
            let mut line = heapless::String::<128>::new();
            let _ = write!(
                line,
                "map_device(0x{addr:08x}) failed with seL4_Error {code}",
                addr = PL011_PADDR,
                code = error_code,
            );
            console.writeln_prefixed(line.as_str());

            let snapshot = env.snapshot();
            let mut window = heapless::String::<160>::new();
            let _ = write!(
                window,
                "device_window base=0x{dbase:08x} cursor=0x{dcursor:08x}; dma_window base=0x{dmabase:08x} cursor=0x{dmacursor:08x}",
                dbase = snapshot.device_base,
                dcursor = snapshot.device_cursor,
                dmabase = snapshot.dma_base,
                dmacursor = snapshot.dma_cursor,
            );
            console.writeln_prefixed(window.as_str());

            let mut cspace = heapless::String::<160>::new();
            let _ = write!(
                cspace,
                "cspace used={used} remaining={remaining} capacity={capacity}",
                used = snapshot.cspace_used,
                remaining = snapshot.cspace_remaining,
                capacity = snapshot.cspace_capacity,
            );
            console.writeln_prefixed(cspace.as_str());

            let mut root_info = heapless::String::<160>::new();
            let _ = write!(
                root_info,
                "cspace.root=0x{root:04x} depth={depth}",
                root = snapshot.cspace_root,
                depth = snapshot.cspace_root_depth,
            );
            console.writeln_prefixed(root_info.as_str());

            let stats = snapshot.untyped;
            let mut untyped = heapless::String::<192>::new();
            let _ = write!(
                untyped,
                "untyped total={total} used={used}; device total={dev_total} used={dev_used}",
                total = stats.total,
                used = stats.used,
                dev_total = stats.device_total,
                dev_used = stats.device_used,
            );
            console.writeln_prefixed(untyped.as_str());

            if let Some(last) = snapshot.last_retype {
                let mut detail = heapless::String::<256>::new();
                match last.status {
                    RetypeStatus::Pending => {
                        let _ = write!(
                            detail,
                            "retype status=pending untyped=0x{ucap:08x} paddr=0x{paddr:08x} size_bits={usize_bits} slot=0x{slot:04x} offset={offset} depth={depth} root=0x{root:04x} node_index=0x{node_index:04x} obj_type={otype} obj_size_bits={obj_bits}",
                            ucap = last.trace.untyped_cap,
                            paddr = last.trace.untyped_paddr,
                            usize_bits = last.trace.untyped_size_bits,
                            slot = last.trace.dest_slot,
                            offset = last.trace.dest_offset,
                            depth = last.trace.cnode_depth,
                            root = last.trace.cnode_root,
                            node_index = last.trace.node_index,
                            otype = last.trace.object_type,
                            obj_bits = last.trace.object_size_bits,
                        );
                    }
                    RetypeStatus::Ok => {
                        let _ = write!(
                            detail,
                            "retype status=ok untyped=0x{ucap:08x} paddr=0x{paddr:08x} size_bits={usize_bits} slot=0x{slot:04x} offset={offset} depth={depth} root=0x{root:04x} node_index=0x{node_index:04x} obj_type={otype} obj_size_bits={obj_bits}",
                            ucap = last.trace.untyped_cap,
                            paddr = last.trace.untyped_paddr,
                            usize_bits = last.trace.untyped_size_bits,
                            slot = last.trace.dest_slot,
                            offset = last.trace.dest_offset,
                            depth = last.trace.cnode_depth,
                            root = last.trace.cnode_root,
                            node_index = last.trace.node_index,
                            otype = last.trace.object_type,
                            obj_bits = last.trace.object_size_bits,
                        );
                    }
                    RetypeStatus::Err(code) => {
                        let _ = write!(
                            detail,
                            "retype status=err({code}) untyped=0x{ucap:08x} paddr=0x{paddr:08x} size_bits={usize_bits} slot=0x{slot:04x} offset={offset} depth={depth} root=0x{root:04x} node_index=0x{node_index:04x} obj_type={otype} obj_size_bits={obj_bits}",
                            code = code as i32,
                            ucap = last.trace.untyped_cap,
                            paddr = last.trace.untyped_paddr,
                            usize_bits = last.trace.untyped_size_bits,
                            slot = last.trace.dest_slot,
                            offset = last.trace.dest_offset,
                            depth = last.trace.cnode_depth,
                            root = last.trace.cnode_root,
                            node_index = last.trace.node_index,
                            otype = last.trace.object_type,
                            obj_bits = last.trace.object_size_bits,
                        );
                    }
                }
                console.writeln_prefixed(detail.as_str());

                let mut kind = heapless::String::<192>::new();
                match last.trace.kind {
                    RetypeKind::DevicePage { paddr } => {
                        let _ = write!(
                            kind,
                            "retype.kind=device_page target_paddr=0x{paddr:08x}",
                            paddr = paddr,
                        );
                    }
                    RetypeKind::DmaPage { paddr } => {
                        let _ = write!(
                            kind,
                            "retype.kind=dma_page target_paddr=0x{paddr:08x}",
                            paddr = paddr,
                        );
                    }
                    RetypeKind::PageTable { vaddr } => {
                        let _ = write!(
                            kind,
                            "retype.kind=page_table base_vaddr=0x{vaddr:08x}",
                            vaddr = vaddr,
                        );
                    }
                }
                console.writeln_prefixed(kind.as_str());
            } else {
                console.writeln_prefixed("no retype trace captured");
            }

            match env.device_coverage(PL011_PADDR, DEVICE_FRAME_BITS) {
                Some(region) => {
                    let mut coverage = heapless::String::<192>::new();
                    let region_state = if region.used { "reserved" } else { "free" };
                    let _ = write!(
                        coverage,
                        "device coverage idx={index} [{base:#010x}..{limit:#010x}) size_bits={size} state={state}",
                        index = region.index,
                        base = region.base,
                        limit = region.limit,
                        size = region.size_bits,
                        state = region_state,
                    );
                    console.writeln_prefixed(coverage.as_str());
                }
                None => {
                    console.writeln_prefixed("no device untyped covers requested PL011 range");
                }
            }

            panic!("PL011 UART mapping failed: {}", err);
        }
    };
    let driver = Pl011::new(uart_region.ptr());
    let serial =
        SerialPort::<_, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY, DEFAULT_LINE_CAPACITY>::new(
            driver,
        );

    #[cfg(all(feature = "net", target_os = "none"))]
    let mut net_stack = NetStack::new(&mut env, Ipv4Address::new(10, 0, 0, 2));
    #[cfg(all(feature = "net", not(target_os = "none")))]
    let (mut net_stack, _) = NetStack::new(Ipv4Address::new(10, 0, 0, 2));
    let timer = KernelTimer::new(5);
    let ipc = KernelIpc;
    let mut tickets: TicketTable<4> = TicketTable::new();
    let _ = tickets.register(Role::Queen, "bootstrap");
    #[cfg(feature = "net")]
    console.writeln_prefixed("network stack initialised");
    console.writeln_prefixed("initialising event pump");
    let mut audit = ConsoleAudit::new(&mut console);
    let mut pump = EventPump::new(serial, timer, ipc, tickets, &mut audit);

    #[cfg(target_os = "none")]
    {
        pump = pump.with_ninedoor(&mut ninedoor);
    }

    #[cfg(feature = "net")]
    {
        pump = pump.with_network(&mut net_stack);
    }

    loop {
        pump.poll();
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

/// Panic handler implementation that emits diagnostics before halting.
pub fn panic_handler(info: &PanicInfo) -> ! {
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

struct KernelTimer {
    tick: u64,
    period_ms: u64,
}

impl KernelTimer {
    fn new(period_ms: u64) -> Self {
        Self { tick: 0, period_ms }
    }
}

impl TimerSource for KernelTimer {
    fn poll(&mut self, now_ms: u64) -> Option<TickEvent> {
        self.tick = self.tick.saturating_add(1);
        Some(TickEvent {
            tick: self.tick,
            now_ms: now_ms.saturating_add(self.period_ms),
        })
    }
}

struct KernelIpc;

impl IpcDispatcher for KernelIpc {
    fn dispatch(&mut self, _now_ms: u64) {}
}

struct ConsoleAudit<'a> {
    console: &'a mut DebugConsole,
}

impl<'a> ConsoleAudit<'a> {
    fn new(console: &'a mut DebugConsole) -> Self {
        Self { console }
    }
}

impl AuditSink for ConsoleAudit<'_> {
    fn info(&mut self, message: &str) {
        self.console.writeln_prefixed(message);
    }

    fn denied(&mut self, message: &str) {
        self.console.writeln_prefixed(message);
    }
}
