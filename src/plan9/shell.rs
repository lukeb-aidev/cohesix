// CLASSIFICATION: COMMUNITY
// Filename: shell.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Plan 9–style shell interface for Cohesix.
//! Provides command execution, input dispatch, and basic parsing.

use std::collections::VecDeque;

/// Represents a parsed shell command and arguments.
#[derive(Debug)]
pub struct Command {
    pub name: String,
    pub args: Vec<String>,
}

/// A simple shell interface.
pub struct Shell {
    pub history: VecDeque<String>,
}

impl Shell {
    /// Create a new shell instance.
    pub fn new() -> Self {
        Shell {
            history: VecDeque::new(),
        }
    }

    /// Parse a raw input line into a Command struct.
    pub fn parse_input(&self, input: &str) -> Option<Command> {
        let tokens: Vec<String> = input.split_whitespace().map(|s| s.to_string()).collect();
        if tokens.is_empty() {
            return None;
        }
        Some(Command {
            name: tokens[0].clone(),
            args: tokens[1..].to_vec(),
        })
    }

    /// Execute a shell command.
    pub fn execute(&mut self, input: &str) {
        self.history.push_back(input.to_string());

        match self.parse_input(input) {
            Some(cmd) => {
                println!("[shell] executing: {} {:?}", cmd.name, cmd.args);
                // TODO(cohesix): dispatch command to system or BusyBox
            }
            None => {
                println!("[shell] empty input");
            }
        }
    }
}
