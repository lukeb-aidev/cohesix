// CLASSIFICATION: COMMUNITY
// Filename: boot_args_fuzz.rs v0.1
// Date Modified: 2025-07-07
// Author: Lukas Bower

use cohesix::bootloader::args::parse_cmdline;
use proptest::prelude::*;

proptest! {
    #[test]
    fn fuzz_parse_cmdline_random(input in "[a-z0-9= ]*") {
        let _ = parse_cmdline(&input);
    }
}
