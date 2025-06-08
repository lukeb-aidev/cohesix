// CLASSIFICATION: COMMUNITY
// Filename: cli_target.rs v0.1
// Date Modified: 2025-07-11
// Author: Lukas Bower

use cohesix::cli::args::build_cli;

#[test]
fn parse_target_option() {
    let matches = build_cli().try_get_matches_from([
        "cohcc",
        "--input", "foo.ir",
        "--target", "x86_64",
    ]).expect("cli args parse");
    assert_eq!(matches.get_one::<String>("target").unwrap(), "x86_64");
}
