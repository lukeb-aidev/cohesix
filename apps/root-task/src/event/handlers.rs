// Author: Lukas Bower

#![cfg(feature = "kernel")]

//! Handler traits for bootstrap IPC message dispatch.

use sel4_sys::seL4_Word;

/// Trait implemented by bootstrap IPC handlers interested in specific opcodes.
pub trait BootstrapHandlers {
    /// Handle an attach request. The provided slice includes the opcode word.
    fn on_attach(&mut self, words: &[seL4_Word]);

    /// Handle a spawn request. The provided slice includes the opcode word.
    fn on_spawn(&mut self, words: &[seL4_Word]);

    /// Handle a log request. The provided slice includes the opcode word.
    fn on_log(&mut self, words: &[seL4_Word]);
}

/// Adapter allowing closures to satisfy [`BootstrapHandlers`] in tests.
pub struct ClosureHandlers<F, G, H>
where
    F: FnMut(&[seL4_Word]),
    G: FnMut(&[seL4_Word]),
    H: FnMut(&[seL4_Word]),
{
    attach: F,
    spawn: G,
    log: H,
}

impl<F, G, H> ClosureHandlers<F, G, H>
where
    F: FnMut(&[seL4_Word]),
    G: FnMut(&[seL4_Word]),
    H: FnMut(&[seL4_Word]),
{
    /// Constructs a handler adapter from the provided closures.
    pub fn new(attach: F, spawn: G, log: H) -> Self {
        Self { attach, spawn, log }
    }
}

impl<F, G, H> BootstrapHandlers for ClosureHandlers<F, G, H>
where
    F: FnMut(&[seL4_Word]),
    G: FnMut(&[seL4_Word]),
    H: FnMut(&[seL4_Word]),
{
    fn on_attach(&mut self, words: &[seL4_Word]) {
        (self.attach)(words);
    }

    fn on_spawn(&mut self, words: &[seL4_Word]) {
        (self.spawn)(words);
    }

    fn on_log(&mut self, words: &[seL4_Word]) {
        (self.log)(words);
    }
}
