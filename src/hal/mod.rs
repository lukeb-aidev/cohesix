//! Portable hardware-abstraction layer façade.
//
// The actual implementation lives in `arm64` or `x86_64` sub-modules,
// selected via `cfg(target_arch = …)`.

#[cfg(target_arch = "aarch64")]
pub mod arm64;
#[cfg(target_arch = "x86_64")]
pub mod x86_64;
