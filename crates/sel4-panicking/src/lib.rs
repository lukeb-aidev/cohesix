// Author: Lukas Bower
#![no_std]

use core::fmt::{self, Write};
use core::panic::PanicInfo;

#[cfg(sel4_config_printing)]
use sel4_sys::seL4_DebugPutChar;

#[cfg(not(sel4_config_printing))]
mod fallback {
    use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

    use heapless::Deque;
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

    #[inline(always)]
    fn current_sink() -> Option<DebugSink> {
        let emit_ptr = SINK_EMIT.load(Ordering::SeqCst);
        if emit_ptr == 0 {
            return None;
        }
        let emit =
            unsafe { core::mem::transmute::<usize, unsafe extern "C" fn(*mut (), u8)>(emit_ptr) };
        let context = SINK_CONTEXT.load(Ordering::SeqCst);
        Some(DebugSink { context, emit })
    }

    pub fn install_sink(sink: DebugSink) {
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
}

#[cfg(not(sel4_config_printing))]
pub use fallback::Sink as DebugSink;

struct DebugWriter;

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

#[cfg_attr(all(not(test), target_os = "none"), panic_handler)]
#[cfg_attr(not(all(not(test), target_os = "none")), allow(dead_code))]
fn panic(info: &PanicInfo) -> ! {
    let mut writer = DebugWriter;
    let _ = writeln!(writer, "[sel4-panicking] panic: {info}");
    loop {
        write_debug_byte(b'!');
    }
}
