// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate coh mount path and offset guards.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use coh::mount::{AppendOnlyTracker, MountValidator};
use coh::policy::{
    CohBreadcrumbPolicy, CohLeasePolicy, CohMountPolicy, CohPolicy, CohRetryPolicy, CohRunPolicy,
    CohTelemetryPolicy,
};

fn test_policy() -> CohPolicy {
    CohPolicy {
        mount: CohMountPolicy {
            root: "/".to_owned(),
            allowlist: vec!["/log".to_owned(), "/proc".to_owned()],
        },
        telemetry: CohTelemetryPolicy {
            root: "/queen/telemetry".to_owned(),
            max_devices: 1,
            max_segments_per_device: 1,
            max_bytes_per_segment: 1024,
            max_total_bytes_per_device: 1024,
        },
        run: CohRunPolicy {
            lease: CohLeasePolicy {
                schema: "gpu-lease/v1".to_owned(),
                active_state: "ACTIVE".to_owned(),
                max_bytes: 256,
            },
            breadcrumb: CohBreadcrumbPolicy {
                schema: "gpu-breadcrumb/v1".to_owned(),
                max_line_bytes: 256,
                max_command_bytes: 128,
            },
        },
        retry: CohRetryPolicy {
            max_attempts: 1,
            backoff_ms: 1,
            ceiling_ms: 1,
            timeout_ms: 1,
        },
    }
}

#[test]
fn mount_validator_rejects_invalid_paths() {
    let policy = test_policy();
    let validator = MountValidator::from_policy(&policy).expect("validator");
    assert!(validator.resolve_remote("/log/queen.log").is_ok());
    assert!(validator.resolve_remote("/proc").is_ok());
    assert!(validator.resolve_remote("/../secret").is_err());
    assert!(validator.resolve_remote("/secret").is_err());
}

#[test]
fn append_only_offsets_are_enforced() {
    let mut tracker = AppendOnlyTracker::new();
    tracker.check_and_advance(0, 8).expect("first append");
    assert!(tracker.check_and_advance(4, 4).is_err());
    tracker.check_and_advance(8, 4).expect("second append");
}
