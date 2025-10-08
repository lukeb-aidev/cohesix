// CLASSIFICATION: COMMUNITY
// Filename: bootlog.rs v0.1
// Author: Lukas Bower
// Date Modified: 2029-10-08

use core::cell::UnsafeCell;
use core::cmp;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const BOOTLOG_CAPACITY: usize = 4096;

struct BootLogBuffer {
    buf: UnsafeCell<[u8; BOOTLOG_CAPACITY]>,
    next: AtomicUsize,
    tail: AtomicUsize,
    flush_pos: AtomicUsize,
    overflowed: AtomicBool,
}

unsafe impl Sync for BootLogBuffer {}

impl BootLogBuffer {
    const fn new() -> Self {
        Self {
            buf: UnsafeCell::new([0; BOOTLOG_CAPACITY]),
            next: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            flush_pos: AtomicUsize::new(0),
            overflowed: AtomicBool::new(false),
        }
    }

    fn record(&self, byte: u8) {
        let idx = self.next.fetch_add(1, Ordering::AcqRel);
        unsafe {
            (*self.buf.get())[idx % BOOTLOG_CAPACITY] = byte;
        }
        let mut tail = self.tail.load(Ordering::Relaxed);
        if idx + 1 > tail + BOOTLOG_CAPACITY {
            tail = idx + 1 - BOOTLOG_CAPACITY;
            self.tail.store(tail, Ordering::Relaxed);
            self.overflowed.store(true, Ordering::Relaxed);
        }
        let mut flush_pos = self.flush_pos.load(Ordering::Relaxed);
        if flush_pos < tail {
            flush_pos = tail;
            self.flush_pos.store(flush_pos, Ordering::Relaxed);
        }
    }

    fn drain_with<F: FnMut(u8)>(&self, mut sink: F) {
        let tail = self.tail.load(Ordering::Acquire);
        let next = self.next.load(Ordering::Acquire);
        let mut pos = self.flush_pos.load(Ordering::Acquire);
        if pos < tail {
            pos = tail;
        }
        while pos < next {
            let byte = unsafe { (*self.buf.get())[pos % BOOTLOG_CAPACITY] };
            sink(byte);
            pos += 1;
        }
        self.flush_pos.store(pos, Ordering::Release);
    }

    fn read_into(&self, offset: usize, out: &mut [u8]) -> usize {
        if out.is_empty() {
            return 0;
        }
        let tail = self.tail.load(Ordering::Acquire);
        let next = self.next.load(Ordering::Acquire);
        if offset >= next {
            return 0;
        }
        let start = cmp::max(offset, tail);
        let available = next.saturating_sub(start);
        if available == 0 {
            return 0;
        }
        let to_copy = cmp::min(available, out.len());
        for i in 0..to_copy {
            let idx = start + i;
            out[i] = unsafe { (*self.buf.get())[idx % BOOTLOG_CAPACITY] };
        }
        to_copy
    }

    fn snapshot_into(&self, out: &mut [u8]) -> usize {
        if out.is_empty() {
            return 0;
        }
        let tail = self.tail.load(Ordering::Acquire);
        let next = self.next.load(Ordering::Acquire);
        if tail >= next {
            return 0;
        }
        let available = next - tail;
        let to_copy = cmp::min(available, out.len());
        let start = next - to_copy;
        for i in 0..to_copy {
            let idx = start + i;
            out[i] = unsafe { (*self.buf.get())[idx % BOOTLOG_CAPACITY] };
        }
        to_copy
    }

    fn base_offset(&self) -> usize {
        self.tail.load(Ordering::Acquire)
    }

    fn len(&self) -> usize {
        let tail = self.tail.load(Ordering::Acquire);
        let next = self.next.load(Ordering::Acquire);
        next.saturating_sub(tail)
    }

    fn reset(&self) {
        self.next.store(0, Ordering::Release);
        self.tail.store(0, Ordering::Release);
        self.flush_pos.store(0, Ordering::Release);
        self.overflowed.store(false, Ordering::Release);
    }
}

static BOOTLOG: BootLogBuffer = BootLogBuffer::new();

pub(crate) fn record(byte: u8) {
    BOOTLOG.record(byte);
}

pub(crate) fn flush_to_uart_if_ready() {
    if !crate::drivers::uart::is_mmio_ready() {
        return;
    }
    BOOTLOG.drain_with(|b| crate::drivers::uart::write_char(b));
}

pub(crate) fn read_from(offset: usize, out: &mut [u8]) -> usize {
    BOOTLOG.read_into(offset, out)
}

pub(crate) fn snapshot(out: &mut [u8]) -> usize {
    BOOTLOG.snapshot_into(out)
}

pub(crate) fn base_offset() -> usize {
    BOOTLOG.base_offset()
}

pub(crate) fn len() -> usize {
    BOOTLOG.len()
}

pub(crate) fn overflowed() -> bool {
    BOOTLOG.overflowed.load(Ordering::Acquire)
}

#[cfg(test)]
pub(crate) fn test_reset() {
    BOOTLOG.reset();
}

#[cfg(test)]
pub(crate) fn test_drain() -> alloc::vec::Vec<u8> {
    extern crate alloc;
    let mut out = alloc::vec::Vec::new();
    BOOTLOG.drain_with(|b| out.push(b));
    out
}

#[cfg(test)]
pub(crate) fn test_write_slice(slice: &[u8]) {
    for &b in slice {
        record(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_snapshots() {
        test_reset();
        test_write_slice(b"abcd");
        let mut buf = [0u8; 8];
        let len = snapshot(&mut buf);
        assert_eq!(len, 4);
        assert_eq!(&buf[..len], b"abcd");
    }

    #[test]
    fn overwrites_oldest_entries() {
        test_reset();
        let data: [u8; BOOTLOG_CAPACITY + 10] = [0x55; BOOTLOG_CAPACITY + 10];
        for &b in &data {
            record(b);
        }
        assert!(overflowed());
        assert!(base_offset() > 0);
        assert!(len() <= BOOTLOG_CAPACITY);
    }

    #[test]
    fn read_from_handles_offsets() {
        test_reset();
        test_write_slice(b"0123456789");
        let mut buf = [0u8; 4];
        let read = read_from(2, &mut buf);
        assert_eq!(read, 4);
        assert_eq!(&buf, b"2345");
    }
}
