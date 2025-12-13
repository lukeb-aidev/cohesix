// Author: Lukas Bower

#![cfg(feature = "kernel")]

//! Bootstrap IPC dispatcher that decodes opcodes and invokes typed handlers.

use sel4_sys::seL4_Word;

use crate::debug_uart::debug_uart_str;
use crate::event::handlers::{call_handler, HandlerTable};
use crate::event::op::BootstrapOp;

/// Result of attempting to dispatch a bootstrap IPC payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// No words were supplied in the message payload.
    Empty,
    /// The opcode was recognised and the appropriate handler executed.
    Handled(BootstrapOp),
    /// The opcode was invalid and no handler was invoked.
    BadCommand(seL4_Word),
}

/// Dispatches a bootstrap IPC payload to the handler implementation.
#[must_use]
pub fn dispatch_message(words: &[seL4_Word], handlers: &HandlerTable) -> DispatchOutcome {
    let Some(&opcode_word) = words.first() else {
        log::warn!("[bootstrap-ipc] empty payload");
        return DispatchOutcome::Empty;
    };

    let Some(opcode) = BootstrapOp::decode(opcode_word) else {
        debug_uart_str("[dbg] EP 0x0130: recv/dispatch error\n");
        log::error!(
            "[ipc] EP 0x0130: dispatch error bad opcode=0x{opcode:02x}",
            opcode = opcode_word
        );
        return DispatchOutcome::BadCommand(opcode_word);
    };

    log::info!(
        "[ipc] EP 0x0130: dispatch msg kind={opcode:?} words={count}",
        count = words.len()
    );
    let result = match opcode {
        BootstrapOp::Attach => call_handler(handlers.attach, words),
        BootstrapOp::Spawn => call_handler(handlers.spawn, words),
        BootstrapOp::Log => call_handler(handlers.log, words),
    };

    if let Err(err) = result {
        log::error!("[bootstrap-ipc] handler for {opcode:?} failed: {err}");
    }

    DispatchOutcome::Handled(opcode)
}
