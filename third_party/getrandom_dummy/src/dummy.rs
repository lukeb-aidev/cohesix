// CLASSIFICATION: COMMUNITY
// Filename: dummy.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-25

//! Deterministic RNG for unsupported targets.
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::Error;

static STATE: AtomicU64 = AtomicU64::new(0x1234_5678_9ABC_DEF0);

pub fn getrandom_inner(dest: &mut [MaybeUninit<u8>]) -> Result<(), Error> {
    let mut s = STATE.load(Ordering::Relaxed);
    for b in dest.iter_mut() {
        s ^= s.wrapping_shl(13);
        s ^= s.wrapping_shr(7);
        s ^= s.wrapping_shl(17);
        b.write((s & 0xFF) as u8);
    }
    STATE.store(s, Ordering::Relaxed);
    Ok(())
}
