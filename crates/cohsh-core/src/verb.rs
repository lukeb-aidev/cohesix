// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Canonical console verb inventory for Cohesix.
// Author: Lukas Bower

//! Canonical console verb inventory for Cohesix.

/// Canonical list of console verbs supported by Cohesix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleVerb {
    /// `help`
    Help,
    /// `bi`
    BootInfo,
    /// `caps`
    Caps,
    /// `mem`
    Mem,
    /// `ping`
    Ping,
    /// `test`
    Test,
    /// `nettest`
    NetTest,
    /// `netstats`
    NetStats,
    /// `log`
    Log,
    /// `cachelog`
    CacheLog,
    /// `quit`
    Quit,
    /// `tail`
    Tail,
    /// `cat`
    Cat,
    /// `ls`
    Ls,
    /// `echo`
    Echo,
    /// `attach`
    Attach,
    /// `spawn`
    Spawn,
    /// `kill`
    Kill,
}

/// Number of console verbs known to the compiler.
pub const VERB_SPEC_COUNT: usize = 18;

/// All console verbs in canonical order.
pub const ALL_VERBS: [ConsoleVerb; VERB_SPEC_COUNT] = [
    ConsoleVerb::Help,
    ConsoleVerb::BootInfo,
    ConsoleVerb::Caps,
    ConsoleVerb::Mem,
    ConsoleVerb::Ping,
    ConsoleVerb::Test,
    ConsoleVerb::NetTest,
    ConsoleVerb::NetStats,
    ConsoleVerb::Log,
    ConsoleVerb::CacheLog,
    ConsoleVerb::Quit,
    ConsoleVerb::Tail,
    ConsoleVerb::Cat,
    ConsoleVerb::Ls,
    ConsoleVerb::Echo,
    ConsoleVerb::Attach,
    ConsoleVerb::Spawn,
    ConsoleVerb::Kill,
];

/// Grammar metadata for a console verb.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerbSpec {
    /// Verb identifier.
    pub verb: ConsoleVerb,
    /// Usage string in canonical console grammar.
    pub usage: &'static str,
    /// Example command line matching the grammar.
    pub example: &'static str,
}

/// Console verb grammar specs (canonical order).
pub const VERB_SPECS: [VerbSpec; VERB_SPEC_COUNT] = [
    VerbSpec {
        verb: ConsoleVerb::Help,
        usage: "help",
        example: "help",
    },
    VerbSpec {
        verb: ConsoleVerb::BootInfo,
        usage: "bi",
        example: "bi",
    },
    VerbSpec {
        verb: ConsoleVerb::Caps,
        usage: "caps",
        example: "caps",
    },
    VerbSpec {
        verb: ConsoleVerb::Mem,
        usage: "mem",
        example: "mem",
    },
    VerbSpec {
        verb: ConsoleVerb::Ping,
        usage: "ping",
        example: "ping",
    },
    VerbSpec {
        verb: ConsoleVerb::Test,
        usage: "test",
        example: "test",
    },
    VerbSpec {
        verb: ConsoleVerb::NetTest,
        usage: "nettest",
        example: "nettest",
    },
    VerbSpec {
        verb: ConsoleVerb::NetStats,
        usage: "netstats",
        example: "netstats",
    },
    VerbSpec {
        verb: ConsoleVerb::Log,
        usage: "log",
        example: "log",
    },
    VerbSpec {
        verb: ConsoleVerb::CacheLog,
        usage: "cachelog [n]",
        example: "cachelog 64",
    },
    VerbSpec {
        verb: ConsoleVerb::Quit,
        usage: "quit",
        example: "quit",
    },
    VerbSpec {
        verb: ConsoleVerb::Tail,
        usage: "tail <path>",
        example: "tail /log/queen.log",
    },
    VerbSpec {
        verb: ConsoleVerb::Cat,
        usage: "cat <path>",
        example: "cat /log/queen.log",
    },
    VerbSpec {
        verb: ConsoleVerb::Ls,
        usage: "ls <path>",
        example: "ls /log",
    },
    VerbSpec {
        verb: ConsoleVerb::Echo,
        usage: "echo <path> <payload>",
        example: "echo /log/queen.log hello",
    },
    VerbSpec {
        verb: ConsoleVerb::Attach,
        usage: "attach <role> [ticket]",
        example: "attach queen",
    },
    VerbSpec {
        verb: ConsoleVerb::Spawn,
        usage: "spawn <payload>",
        example: "spawn {\"spawn\":\"heartbeat\"}",
    },
    VerbSpec {
        verb: ConsoleVerb::Kill,
        usage: "kill <worker>",
        example: "kill worker-1",
    },
];

const _: [(); VERB_SPEC_COUNT] = [(); ALL_VERBS.len()];
const _: [(); VERB_SPEC_COUNT] = [(); VERB_SPECS.len()];

impl ConsoleVerb {
    /// Return the canonical token used when parsing the console verb.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::BootInfo => "bi",
            Self::Caps => "caps",
            Self::Mem => "mem",
            Self::Ping => "ping",
            Self::Test => "test",
            Self::NetTest => "nettest",
            Self::NetStats => "netstats",
            Self::Log => "log",
            Self::CacheLog => "cachelog",
            Self::Quit => "quit",
            Self::Tail => "tail",
            Self::Cat => "cat",
            Self::Ls => "ls",
            Self::Echo => "echo",
            Self::Attach => "attach",
            Self::Spawn => "spawn",
            Self::Kill => "kill",
        }
    }

    /// Return the acknowledgement label used by the console for this verb.
    #[must_use]
    pub const fn ack_label(self) -> &'static str {
        match self {
            Self::Help => "HELP",
            Self::BootInfo => "BOOTINFO",
            Self::Caps => "CAPS",
            Self::Mem => "MEM",
            Self::Ping => "PING",
            Self::Test => "TEST",
            Self::NetTest => "NETTEST",
            Self::NetStats => "NETSTATS",
            Self::Log => "LOG",
            Self::CacheLog => "CACHELOG",
            Self::Quit => "QUIT",
            Self::Tail => "TAIL",
            Self::Cat => "CAT",
            Self::Ls => "LS",
            Self::Echo => "ECHO",
            Self::Attach => "ATTACH",
            Self::Spawn => "SPAWN",
            Self::Kill => "KILL",
        }
    }

    /// Parse a console verb token, matching case-insensitively.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        if token.eq_ignore_ascii_case("help") {
            Some(Self::Help)
        } else if token.eq_ignore_ascii_case("bi") {
            Some(Self::BootInfo)
        } else if token.eq_ignore_ascii_case("caps") {
            Some(Self::Caps)
        } else if token.eq_ignore_ascii_case("mem") {
            Some(Self::Mem)
        } else if token.eq_ignore_ascii_case("ping") {
            Some(Self::Ping)
        } else if token.eq_ignore_ascii_case("test") {
            Some(Self::Test)
        } else if token.eq_ignore_ascii_case("nettest") {
            Some(Self::NetTest)
        } else if token.eq_ignore_ascii_case("netstats") {
            Some(Self::NetStats)
        } else if token.eq_ignore_ascii_case("log") {
            Some(Self::Log)
        } else if token.eq_ignore_ascii_case("cachelog") {
            Some(Self::CacheLog)
        } else if token.eq_ignore_ascii_case("quit") {
            Some(Self::Quit)
        } else if token.eq_ignore_ascii_case("tail") {
            Some(Self::Tail)
        } else if token.eq_ignore_ascii_case("cat") {
            Some(Self::Cat)
        } else if token.eq_ignore_ascii_case("ls") {
            Some(Self::Ls)
        } else if token.eq_ignore_ascii_case("echo") {
            Some(Self::Echo)
        } else if token.eq_ignore_ascii_case("attach") {
            Some(Self::Attach)
        } else if token.eq_ignore_ascii_case("spawn") {
            Some(Self::Spawn)
        } else if token.eq_ignore_ascii_case("kill") {
            Some(Self::Kill)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandParser;

    #[test]
    fn verb_specs_cover_all_verbs() {
        for verb in ALL_VERBS.iter() {
            assert!(VERB_SPECS.iter().any(|spec| spec.verb == *verb));
        }
    }

    #[test]
    fn verb_specs_parse_examples() {
        for spec in VERB_SPECS.iter() {
            let command = CommandParser::parse_line_str(spec.example)
                .unwrap_or_else(|err| panic!("failed to parse {}: {err:?}", spec.example));
            assert_eq!(command.verb(), spec.verb);
        }
    }
}
