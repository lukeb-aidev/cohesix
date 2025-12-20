// Author: Lukas Bower

//! Virtio device drivers for smoltcp integration.

pub(crate) mod mmio_policy;
#[cfg(feature = "net-console")]
pub(crate) mod net;
