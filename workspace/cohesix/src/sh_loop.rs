// CLASSIFICATION: COMMUNITY
// Filename: sh_loop.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-09-23

/// Wrapper for launching the Cohesix interactive shell.

#[cfg(feature = "busybox_client")]
use crate::shell::busybox_runner::spawn_shell;

/// Enter the interactive shell loop. Returns when the shell exits.
pub fn run() {
    #[cfg(feature = "busybox_client")]
    {
        spawn_shell();
    }
    #[cfg(not(feature = "busybox_client"))]
    {
        println!("[sh_loop] busybox feature disabled");
    }
}
