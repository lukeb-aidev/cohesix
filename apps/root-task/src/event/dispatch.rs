// Author: Lukas Bower

#![cfg(feature = "kernel")]

//! Bootstrap IPC dispatcher that decodes opcodes and invokes typed handlers.

use sel4_sys::seL4_Word;

use crate::event::handlers::BootstrapHandlers;
use crate::event::op::BootstrapOp;

/// Result of attempting to dispatch a bootstrap IPC payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// No words were supplied in the message payload.
    Empty,
    /// The opcode was recognised and the appropriate handler executed.
    Handled(BootstrapOp),
    /// The opcode was invalid and no handler was invoked.
    Unknown(seL4_Word),
}

/// Dispatches a bootstrap IPC payload to the handler implementation.
#[must_use]
pub fn dispatch_message(
    words: &[seL4_Word],
    handlers: &mut dyn BootstrapHandlers,
) -> DispatchOutcome {
    let Some(&opcode_word) = words.first() else {
        log::warn!("[bootstrap-ipc] empty payload");
        return DispatchOutcome::Empty;
    };

    let Some(opcode) = BootstrapOp::decode(opcode_word) else {
        log::warn!(
            "[bootstrap-ipc] unknown opcode=0x{opcode:02x}",
            opcode = opcode_word
        );
        return DispatchOutcome::Unknown(opcode_word);
    };

    match opcode {
        BootstrapOp::Attach => handlers.on_attach(words),
        BootstrapOp::Spawn => handlers.on_spawn(words),
        BootstrapOp::Log => handlers.on_log(words),
    }

    DispatchOutcome::Handled(opcode)
}
