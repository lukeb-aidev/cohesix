// CLASSIFICATION: COMMUNITY
// Filename: sh_loop.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-09-23

//! Wrapper for launching the Cohesix interactive shell.

#[cfg(feature = "busybox")]
use crate::shell::busybox_runner::spawn_shell;

/// Enter the interactive shell loop. Returns when the shell exits.
pub fn run() {
    #[cfg(feature = "busybox")]
    {
        spawn_shell();
    }
    #[cfg(not(feature = "busybox"))]
    {
        println!("[sh_loop] busybox feature disabled");
    }
}
