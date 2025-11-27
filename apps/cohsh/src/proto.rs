// Author: Lukas Bower

//! Console acknowledgement parsing helpers shared by Cohsh transports.

/// ACK/ERR status emitted by root-task console commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckStatus {
    /// Indicates success.
    Ok,
    /// Indicates failure.
    Err,
}

/// Parsed acknowledgement line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack<'a> {
    /// Outcome of the command.
    pub status: AckStatus,
    /// Verb associated with the acknowledgement.
    pub verb: &'a str,
    /// Optional trailing detail payload.
    pub detail: Option<&'a str>,
}

/// Parse an acknowledgement line following the `OK VERB ...` grammar.
pub fn parse_ack(line: &str) -> Option<Ack<'_>> {
    let trimmed = line.trim();
    let mut parts = trimmed.splitn(3, ' ');
    let status = match parts.next()? {
        "OK" => AckStatus::Ok,
        "ERR" => AckStatus::Err,
        _ => return None,
    };
    let verb = parts.next()?;
    let detail = parts
        .next()
        .map(str::trim)
        .filter(|detail| !detail.is_empty());
    Some(Ack {
        status,
        verb,
        detail,
    })
}
