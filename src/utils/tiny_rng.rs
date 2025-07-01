// CLASSIFICATION: COMMUNITY
// Filename: tiny_rng.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

use crate::prelude::*;
/// Minimal deterministic RNG used for UEFI builds.
/// Implements a simple Xorshift64* generator.

#[derive(Clone, Copy)]
pub struct TinyRng(u64);

impl TinyRng {
    /// Create a new RNG with the given `seed`.
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    #[inline]
    fn step(&mut self) {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
    }

    /// Return the next `u64` value.
    pub fn next_u64(&mut self) -> u64 {
        self.step();
        self.0
    }

    /// Return the next `u32` value.
    pub fn next_u32(&mut self) -> u32 {
        self.step();
        (self.0 >> 32) as u32
    }

    /// Fill `dest` with random bytes.
    pub fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(8) {
            let val = self.next_u64().to_le_bytes();
            for (b, v) in chunk.iter_mut().zip(val.iter()) {
                *b = *v;
            }
        }
    }

    /// Generate a float in the range [0.0, 1.0).
    pub fn next_f32(&mut self) -> f32 {
        const SCALE: f32 = 1.0 / (u32::MAX as f32);
        self.next_u32() as f32 * SCALE
    }

    /// Generate a float in the range `low..high`.
    pub fn gen_range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next_f32()
    }
}

