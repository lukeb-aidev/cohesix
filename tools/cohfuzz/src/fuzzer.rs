// CLASSIFICATION: COMMUNITY
// Filename: fuzzer.rs v0.2
// Author: Lukas Bower
// Date Modified: 2027-08-09

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
        if events.is_empty() {
            return;
        }

        // rotate order deterministically
        let first = events.remove(0);
        events.push(first);

        // append marker to first entry
        if let Some(ev) = events.first_mut() {
            ev.detail.push_str("_invalid");
        }

        // remove last event if more than one
        if events.len() > 1 {
            events.pop();
        }

        // override path on first "open" event
        for ev in events.iter_mut() {
            if ev.event == "open" {
                ev.detail = format!("/unauth/{}", self.role);
                break;
            }
        }
    }
}
