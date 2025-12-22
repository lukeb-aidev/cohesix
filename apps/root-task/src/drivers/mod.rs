// Author: Lukas Bower

//! Leaf device drivers used by the root task.

// NOTE: rtl8139 driver intentionally removed from kernel build.
// It will be reintroduced later once virtio-net is fully stabilised.
#[cfg(feature = "kernel")]
pub(crate) mod virtio;
