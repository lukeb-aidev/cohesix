// CLASSIFICATION: COMMUNITY
// Filename: fuzzer.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-25

use rand::seq::SliceRandom;
use rand::Rng;
use cohesix::CohError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TraceEvent {
    event: String,
    detail: String,
}

pub struct TraceFuzzer {
    role: String,
}

impl TraceFuzzer {
    pub fn new(role: String) -> Self {
        Self { role }
    }

    pub fn run(&self, input: &Path, iterations: usize) -> Result<(), CohError> {
        let data = fs::read_to_string(input)?;
        let orig: Vec<TraceEvent> = serde_json::from_str(&data)?;
        for i in 0..iterations {
            let mut mutated = orig.clone();
            self.mutate(&mut mutated);
            let out = input.with_extension(format!("{}.fuzz.trc", i));
            fs::write(&out, serde_json::to_string_pretty(&mutated)?)?;
            println!("Wrote {}", out.display());
        }
        Ok(())
    }

    fn mutate(&self, events: &mut Vec<TraceEvent>) {
        let mut rng = rand::thread_rng();
        // reorder some events
        events.shuffle(&mut rng);
        // occasionally inject invalid args
        if let Some(ev) = events.choose_mut(&mut rng) {
            ev.detail.push_str("_invalid");
        }
        // remove a random event to simulate missing binding
        if events.len() > 1 && rng.gen_bool(0.3) {
            let idx = rng.gen_range(0..events.len());
            events.remove(idx);
        }
        // overlap namespace or unauthorized path
        if let Some(ev) = events.choose_mut(&mut rng) {
            if ev.event == "open" {
                ev.detail = format!("/unauth/{}", self.role);
            }
        }
    }
}
