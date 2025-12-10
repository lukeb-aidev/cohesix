// Author: Lukas Bower

//! Console acknowledgement formatting utilities shared across the root task.

pub use console_ack_wire::{AckLine, AckStatus};
use console_ack_wire::render_ack as render_wire_ack;
use heapless::String;

/// Errors encountered while formatting acknowledgement lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineFormatError {
    /// The rendered line exceeded the available buffer capacity.
    Truncated,
}

/// Render an acknowledgement line into the provided buffer using the standard grammar.
pub fn render_ack(
    buf: &mut String<{ crate::serial::DEFAULT_LINE_CAPACITY }>,
    ack: &AckLine<'_>,
) -> Result<(), LineFormatError> {
    buf.clear();
    render_wire_ack(buf, ack).map_err(|_| LineFormatError::Truncated)
}
