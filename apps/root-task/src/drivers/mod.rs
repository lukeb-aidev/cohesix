// Author: Lukas Bower

//! Leaf device drivers used by the root task.

pub(crate) mod rtl8139;
#[cfg(feature = "kernel")]
pub(crate) mod virtio;
