// CLASSIFICATION: COMMUNITY
// Filename: interactive_aibooth.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-10-28

use crate::prelude::*;
//! Initialization routines for the InteractiveAiBooth role.

use std::fs::{self, OpenOptions};
use std::io::Write;
use crate::runtime::env::init::detect_cohrole;
#[cfg(all(feature = "cuda", not(feature = "no-cuda")))]
use crate::cuda::runtime::CudaRuntime;
use crate::runtime::ServiceRegistry;

fn log(msg: &str) {
    match OpenOptions::new().append(true).open("/srv/devlog") {
        Ok(mut f) => {
            let _ = writeln!(f, "{}", msg);
        }
        Err(_) => println!("{msg}"),
    }
}

/// Start the Interactive AI Booth environment.
pub fn start() {
    if detect_cohrole() != "InteractiveAiBooth" {
        log("access denied: wrong role");
        return;
    }

    // Ensure Secure9P namespaces are mounted
    for p in ["/input/mic", "/input/cam", "/mnt/ui_in", "/mnt/speak", "/mnt/face_match"] {
        fs::create_dir_all(p).ok();
    }

    // Initialize CUDA runtime if available
    #[cfg(all(feature = "cuda", not(feature = "no-cuda")))]
    match CudaRuntime::try_new() {
        Ok(rt) => {
            if rt.is_present() {
                log("[aibooth] CUDA available");
            } else {
                log("[aibooth] CUDA unavailable");
            }
        }
        Err(e) => log(&format!("cuda init error: {e}")),
    }
    #[cfg(any(not(feature = "cuda"), feature = "no-cuda"))]
    {
        log("[aibooth] CUDA pipeline disabled");
    }
    let _ = ServiceRegistry::register_service("aibooth", "/srv/aibooth");
    log("[aibooth] startup complete");
}

