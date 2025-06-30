// CLASSIFICATION: COMMUNITY
// Filename: dummy.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-25

//! Dummy RNG implementation for unsupported targets. Always fails.
use core::mem::MaybeUninit;
use crate::Error;

pub fn getrandom_inner(_: &mut [MaybeUninit<u8>]) -> Result<(), Error> {
    Err(Error::UNSUPPORTED)
}
