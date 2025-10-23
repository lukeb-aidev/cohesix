// Author: Lukas Bower
// compile-flags: --cfg feature="kernel"

#[cfg(feature = "kernel")]
fn main() {
    let ptr = core::ptr::null::<u8>();
    root_task::trace::hex_dump_slice("ptr", ptr, 0);
}

#[cfg(not(feature = "kernel"))]
fn main() {
    let ptr = core::ptr::null::<u8>();
    accepts_slice_only(ptr);
}

#[cfg(not(feature = "kernel"))]
fn accepts_slice_only(_slice: &[u8]) {}
