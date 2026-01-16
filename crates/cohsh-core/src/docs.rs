// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Render shared cohsh-core documentation snippets.
// Author: Lukas Bower

//! Render shared cohsh-core documentation snippets.

use alloc::string::String;
use core::fmt::Write as _;

use crate::command::MAX_TICKET_LEN;
use crate::verb::VERB_SPECS;

/// Render the console grammar snippet for docs.
#[must_use]
pub fn render_console_grammar_doc() -> String {
    let mut contents = String::new();
    writeln!(contents, "<!-- Author: Lukas Bower -->").ok();
    writeln!(
        contents,
        "<!-- Purpose: Generated cohsh grammar snippet consumed by docs/USERLAND_AND_CLI.md. -->"
    )
    .ok();
    writeln!(contents).ok();
    writeln!(contents, "### cohsh console grammar (generated)").ok();
    for spec in VERB_SPECS.iter() {
        writeln!(contents, "- `{}`", spec.usage).ok();
    }
    writeln!(contents).ok();
    writeln!(
        contents,
        "_Generated from cohsh-core verb specs ({} verbs)._",
        VERB_SPECS.len()
    )
    .ok();
    contents
}

/// Render the ticket policy snippet for docs.
#[must_use]
pub fn render_ticket_policy_doc() -> String {
    let mut contents = String::new();
    writeln!(contents, "<!-- Author: Lukas Bower -->").ok();
    writeln!(
        contents,
        "<!-- Purpose: Generated cohsh ticket policy snippet consumed by docs/USERLAND_AND_CLI.md. -->"
    )
    .ok();
    writeln!(contents).ok();
    writeln!(contents, "### cohsh ticket policy (generated)").ok();
    writeln!(contents, "- `ticket.max_len`: `{}`", MAX_TICKET_LEN).ok();
    writeln!(
        contents,
        "- `queen` tickets are optional; TCP validates claims when present, NineDoor passes through."
    )
    .ok();
    writeln!(
        contents,
        "- `worker-*` tickets are required; role must match and subject identity is mandatory."
    )
    .ok();
    writeln!(contents).ok();
    writeln!(contents, "_Generated from cohsh-core ticket policy._").ok();
    contents
}
