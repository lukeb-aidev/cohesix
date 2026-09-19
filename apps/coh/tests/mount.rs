// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate coh mount path and offset guards.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use coh::mount::{AppendOnlyTracker, MountValidator};
use coh::policy::{
    CohBreadcrumbPolicy, CohLeasePolicy, CohMountPolicy, CohPeftActivatePolicy,
    CohPeftExportPolicy, CohPeftImportPolicy, CohPeftPolicy, CohPolicy, CohRetryPolicy,
    CohRunPolicy, CohTelemetryPolicy,
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
        peft: CohPeftPolicy {
            export: CohPeftExportPolicy {
                root: "/queen/export/lora_jobs".to_owned(),
                max_telemetry_bytes: 1024,
                max_policy_bytes: 512,
                max_base_model_bytes: 128,
            },
            import: CohPeftImportPolicy {
                registry_root: "out/model_registry".to_owned(),
                max_adapter_bytes: 2048,
                max_lora_bytes: 512,
                max_metrics_bytes: 512,
                max_manifest_bytes: 512,
            },
            activate: CohPeftActivatePolicy {
                max_model_id_bytes: 64,
                max_state_bytes: 512,
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

#[test]
fn append_placement_and_acknowledged_offsets_match_both_mount_backends() {
    let mut tracker = AppendOnlyTracker::new();
    // O_APPEND placement belongs to the remote append, including stale kernel EOF.
    tracker
        .validate_write(4096, 8, true)
        .expect("existing remote EOF");
    tracker
        .validate_write(0, 8, true)
        .expect("stale cached EOF");
    tracker
        .validate_write(0, 8, false)
        .expect("failed write did not advance");
    tracker.commit_write(8, 3).expect("partial acknowledgement");
    tracker
        .validate_write(3, 5, false)
        .expect("resume after three acknowledged bytes");
    assert!(tracker.validate_write(8, 5, false).is_err());
    assert!(tracker.commit_write(5, 6).is_err());
    tracker
        .validate_write(3, 5, false)
        .expect("invalid acknowledgement did not advance");
}

#[test]
fn canonical_shard_paths_use_component_boundaries() {
    let mut policy = test_policy();
    policy.mount.allowlist.push("/shard".into());
    let validator = MountValidator::from_policy(&policy).unwrap();
    assert_eq!(
        validator
            .resolve_remote("/shard/edge/worker/gpu-1/telemetry")
            .unwrap(),
        "/shard/edge/worker/gpu-1/telemetry"
    );
    assert!(validator
        .resolve_remote("/sharded/edge/worker/gpu-1/telemetry")
        .is_err());
    assert!(validator
        .resolve_remote("/shard/edge/../other/worker/gpu-1")
        .is_err());
}

#[test]
fn omitted_append_flag_accepts_only_a_reported_end_or_sequential_cursor() {
    let mut tracker = AppendOnlyTracker::new();
    assert!(tracker.validate_placement(123, 8, false, Some(123)).is_ok());
    assert!(tracker
        .validate_placement(122, 8, false, Some(123))
        .is_err());
    assert!(tracker
        .validate_placement(124, 8, false, Some(123))
        .is_err());
    assert!(tracker.validate_placement(123, 8, false, None).is_err());
    tracker.commit_write(8, 3).unwrap();
    assert!(tracker.validate_placement(3, 5, false, Some(123)).is_ok());
    // A later getattr may publish a new EOF after another append. Neither
    // that observation nor a failed write changes our acknowledged cursor.
    assert!(tracker.validate_placement(200, 5, false, Some(200)).is_ok());
    assert!(tracker.validate_placement(8, 5, false, Some(200)).is_err());
}
