// CLASSIFICATION: COMMUNITY
// Filename: cohcc.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-16

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = cohesix::coh_cc::run(args) {
        eprintln!("cohcc error: {e}");
        std::process::exit(1);
    }
}
