// CLASSIFICATION: COMMUNITY
// Filename: vision_overlay.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-08

//! Live vision overlay utility.

use std::time::{Duration, Instant};

/// Simple frame counter and overlay tool.
pub struct VisionOverlay {
    frames: u32,
}

impl VisionOverlay {
    /// Create a new overlay handler.
    pub fn new() -> Self {
        Self { frames: 0 }
    }

    /// Capture and process frames for the given duration.
    pub fn run(&mut self, dur: Duration) {
        let start = Instant::now();
        while start.elapsed() < dur {
            self.process_frame();
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn process_frame(&mut self) {
        // stub: increment frame count
        self.frames += 1;
    }

    /// Number of processed frames.
    pub fn frames(&self) -> u32 {
        self.frames
    }
}

