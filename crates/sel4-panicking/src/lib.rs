// Author: Lukas Bower
#![no_std]

#[cfg(feature = "panic-handler")]
use core::fmt::{self, Write};
#[cfg(feature = "panic-handler")]
use core::panic::PanicInfo;

#[cfg(sel4_config_printing)]
use sel4_sys::seL4_DebugPutChar;

#[cfg(not(sel4_config_printing))]
mod fallback {
    use core::fmt::Write;
    use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

    use heapless::{Deque, String as HeaplessString};
    use spin::Mutex;

    /// Maximum number of bytes retained while no debug sink is registered.
    const BUFFER_CAPACITY: usize = 1024;

    #[derive(Clone, Copy)]
    #[repr(C)]
    pub struct DebugSink {
        pub context: *mut (),
        pub emit: unsafe extern "C" fn(*mut (), u8),
    }

    impl DebugSink {
        pub const fn null() -> Self {
            Self {
                context: core::ptr::null_mut(),
                emit: noop_emit,
            }
        }
    }

    unsafe extern "C" fn noop_emit(_: *mut (), _: u8) {}

    static SINK_CONTEXT: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());
    static SINK_EMIT: AtomicUsize = AtomicUsize::new(0);
    static BUFFER: Mutex<Deque<u8, BUFFER_CAPACITY>> = Mutex::new(Deque::new());

    const MIN_VALID_PTR: usize = 0x1000;
    const PTR_ALIGN_MASK: usize = 0b11;

    #[inline(always)]
    fn pointer_sane(addr: usize) -> bool {
        addr > MIN_VALID_PTR && addr & PTR_ALIGN_MASK == 0
    }

    fn context_sane(context: *mut ()) -> bool {
        context.is_null() || (context as usize) > MIN_VALID_PTR
    }

    fn log_sink_corruption(kind: &str, emit_addr: usize, context: *mut ()) {
        let mut line = HeaplessString::<160>::new();
        let _ = writeln!(
            line,
            "[sel4-panicking] {kind} emit=0x{emit:016x} ctx=0x{ctx:016x}",
            kind = kind,
            emit = emit_addr,
            ctx = context as usize,
        );
        buffer_message(line.as_str());
    }

    #[inline(always)]
    fn current_sink() -> Option<DebugSink> {
        let emit_ptr = SINK_EMIT.load(Ordering::SeqCst);
        if emit_ptr == 0 {
            return None;
        }
        let context = SINK_CONTEXT.load(Ordering::SeqCst);
        if !pointer_sane(emit_ptr) {
            log_sink_corruption("invalid_debug_sink", emit_ptr, context);
            SINK_EMIT.store(0, Ordering::SeqCst);
            SINK_CONTEXT.store(core::ptr::null_mut(), Ordering::SeqCst);
            return None;
        }
        if !context_sane(context) {
            log_sink_corruption("invalid_debug_context", emit_ptr, context);
            SINK_EMIT.store(0, Ordering::SeqCst);
            SINK_CONTEXT.store(core::ptr::null_mut(), Ordering::SeqCst);
            return None;
        }
        let emit =
            unsafe { core::mem::transmute::<usize, unsafe extern "C" fn(*mut (), u8)>(emit_ptr) };
        Some(DebugSink { context, emit })
    }

    fn buffer_message(message: &str) {
        let mut guard = BUFFER.lock();
        for &byte in message.as_bytes() {
            if guard.push_back(byte).is_err() {
                let _ = guard.pop_front();
                let _ = guard.push_back(byte);
            }
        }
    }

    fn log_sink_registration(emit_addr: usize, context: *mut ()) {
        let mut line = HeaplessString::<128>::new();
        let _ = writeln!(
            line,
            "[sel4-panicking] install_debug_sink emit=0x{emit:016x} ctx=0x{ctx:016x}",
            emit = emit_addr,
            ctx = context as usize,
        );
        buffer_message(line.as_str());
    }

    pub fn install_sink(sink: DebugSink) {
        let emit_addr = sink.emit as usize;
        if emit_addr & 0b11 != 0 {
            panic!(
                "debug sink emit pointer not 4-byte aligned: 0x{emit:016x}",
                emit = emit_addr,
            );
        }
        if emit_addr <= 0x1000 {
            panic!(
                "debug sink emit pointer unexpectedly low: 0x{emit:016x}",
                emit = emit_addr,
            );
        }
        log_sink_registration(emit_addr, sink.context);
        SINK_CONTEXT.store(sink.context, Ordering::SeqCst);
        SINK_EMIT.store(sink.emit as usize, Ordering::SeqCst);
        drain_with(|byte| unsafe {
            (sink.emit)(sink.context, byte);
        });
    }

    pub fn emit(byte: u8) {
        if let Some(sink) = current_sink() {
            unsafe {
                (sink.emit)(sink.context, byte);
            }
            return;
        }

        let mut guard = BUFFER.lock();
        if guard.push_back(byte).is_err() {
            let _ = guard.pop_front();
            let _ = guard.push_back(byte);
        }
    }

    pub fn drain_with(mut f: impl FnMut(u8)) {
        let mut guard = BUFFER.lock();
        while let Some(byte) = guard.pop_front() {
            f(byte);
        }
    }

    pub use DebugSink as Sink;

    #[cfg(test)]
    pub(crate) fn reset_sink() {
        SINK_EMIT.store(0, Ordering::SeqCst);
        SINK_CONTEXT.store(core::ptr::null_mut(), Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn install_raw_sink(context: *mut (), emit: usize) {
        SINK_CONTEXT.store(context, Ordering::SeqCst);
        SINK_EMIT.store(emit, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn buffer_contents() -> heapless::Vec<u8, BUFFER_CAPACITY> {
        let mut snapshot = heapless::Vec::new();
        let guard = BUFFER.lock();
        for byte in guard.iter().copied() {
            let _ = snapshot.push(byte);
        }
        snapshot
    }
}

#[cfg(not(sel4_config_printing))]
pub use fallback::Sink as DebugSink;

#[cfg(test)]
use fallback::{buffer_contents, install_raw_sink, reset_sink};

#[cfg(feature = "panic-handler")]
struct DebugWriter;

#[cfg(feature = "panic-handler")]
impl Write for DebugWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            write_debug_byte(byte);
        }
        Ok(())
    }
}

#[inline(always)]
pub fn write_debug_byte(byte: u8) {
    write_impl(byte);
}

#[cfg(sel4_config_printing)]
#[inline(always)]
fn write_impl(byte: u8) {
    unsafe {
        seL4_DebugPutChar(byte);
    }
}

#[cfg(not(sel4_config_printing))]
#[inline(always)]
fn write_impl(byte: u8) {
    fallback::emit(byte);
}

#[cfg(not(sel4_config_printing))]
pub fn install_debug_sink(sink: DebugSink) {
    fallback::install_sink(sink);
}

#[cfg(sel4_config_printing)]
#[inline(always)]
pub fn install_debug_sink(_sink: ()) {}

#[cfg(not(sel4_config_printing))]
pub fn drain_debug_bytes(f: impl FnMut(u8)) {
    fallback::drain_with(f);
}

#[cfg(sel4_config_printing)]
#[inline(always)]
pub fn drain_debug_bytes<F>(_f: F)
where
    F: FnMut(u8),
{
}

#[cfg(feature = "panic-handler")]
#[cfg_attr(all(not(test), target_os = "none"), panic_handler)]
#[cfg_attr(not(all(not(test), target_os = "none")), allow(dead_code))]
fn panic(info: &PanicInfo) -> ! {
    let mut writer = DebugWriter;
    let _ = writeln!(writer, "[sel4-panicking] panic: {info}");
    loop {
        write_debug_byte(b'!');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;
    use core::sync::atomic::{AtomicU8, Ordering};

    unsafe extern "C" fn capture_emit(context: *mut (), byte: u8) {
        let slot_ptr = context as *const AtomicU8;
        if let Some(slot) = unsafe { slot_ptr.as_ref() } {
            slot.store(byte, Ordering::SeqCst);
        }
    }

    fn clear_debug_buffer() {
        drain_debug_bytes(|_| {});
    }

    #[test]
    fn invalid_emit_pointer_is_buffered() {
        clear_debug_buffer();
        reset_sink();
        install_raw_sink(ptr::null_mut(), 0x2);
        write_debug_byte(b'A');
        let snapshot = buffer_contents();
        assert_eq!(snapshot.as_slice().last(), Some(&b'A'));
        clear_debug_buffer();
        reset_sink();
    }

    #[test]
    fn valid_emit_pointer_is_used() {
        clear_debug_buffer();
        reset_sink();
        static CAPTURED: AtomicU8 = AtomicU8::new(0);
        let context = &CAPTURED as *const AtomicU8 as *mut ();
        install_raw_sink(context, capture_emit as usize);
        write_debug_byte(b'B');
        assert_eq!(CAPTURED.load(Ordering::SeqCst), b'B');
        clear_debug_buffer();
        reset_sink();
    }

    #[test]
    fn invalid_context_pointer_is_buffered() {
        clear_debug_buffer();
        reset_sink();
        static CAPTURED: AtomicU8 = AtomicU8::new(0);
        let bogus_context = 0x10usize as *mut ();
        install_raw_sink(bogus_context, capture_emit as usize);
        write_debug_byte(b'C');
        assert_eq!(CAPTURED.load(Ordering::SeqCst), 0);
        let snapshot = buffer_contents();
        assert_eq!(snapshot.as_slice().last(), Some(&b'C'));
        clear_debug_buffer();
        reset_sink();
    }
}
