// Author: Lukas Bower

//! Console acknowledgement formatting utilities shared across the root task.

use heapless::String;

/// ACK/ERR status emitted by console commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AckStatus {
    /// Successful command execution.
    Ok,
    /// Command failed to execute.
    Err,
}

/// Structured representation of an acknowledgement line.
pub struct AckLine<'a> {
    /// Outcome of the command.
    pub status: AckStatus,
    /// Command verb associated with the acknowledgement.
    pub verb: &'a str,
    /// Optional detail payload appended to the line.
    pub detail: Option<&'a str>,
}

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

    let label = match ack.status {
        AckStatus::Ok => "OK",
        AckStatus::Err => "ERR",
    };

    buf.push_str(label)
        .map_err(|_| LineFormatError::Truncated)?;
    buf.push(' ').map_err(|_| LineFormatError::Truncated)?;
    buf.push_str(ack.verb)
        .map_err(|_| LineFormatError::Truncated)?;

    if let Some(detail) = ack.detail {
        if !detail.is_empty() {
            buf.push(' ').map_err(|_| LineFormatError::Truncated)?;
            buf.push_str(detail)
                .map_err(|_| LineFormatError::Truncated)?;
        }
    }

    Ok(())
}
