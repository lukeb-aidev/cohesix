// Author: Lukas Bower

#![cfg(feature = "kernel")]

//! Opcode definitions for bootstrap IPC messages dispatched by the event pump.

use sel4_sys::seL4_Word;

/// Bootstrap operations supported by the root-task dispatcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapOp {
    /// Attach a console session using an authentication ticket.
    Attach,
    /// Spawn a worker task or auxiliary thread.
    Spawn,
    /// Emit a log payload routed through the event pump.
    Log,
}

impl BootstrapOp {
    /// Lowest eight bits of the opcode word mask.
    const MASK: seL4_Word = 0xFF;

    /// Numeric discriminant used when encoding the operation in an IPC payload.
    fn value(self) -> u8 {
        match self {
            Self::Attach => 0x01,
            Self::Spawn => 0x02,
            Self::Log => 0x03,
        }
    }

    /// Attempts to decode an opcode from the provided message word.
    #[must_use]
    pub fn decode(word: seL4_Word) -> Option<Self> {
        match (word & Self::MASK) as u8 {
            0x01 => Some(Self::Attach),
            0x02 => Some(Self::Spawn),
            0x03 => Some(Self::Log),
            _ => None,
        }
    }

    /// Encodes the opcode into the low byte of a message word.
    #[must_use]
    pub fn encode(self) -> seL4_Word {
        seL4_Word::from(self.value())
    }
}
