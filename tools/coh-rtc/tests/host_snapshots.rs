// Author: Lukas Bower
// Purpose: Keep generated snapshot source enrollment inside independent console, memory and publisher bounds.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use coh_rtc::ir::{load_manifest, HostSnapshotPublisher};
use std::path::PathBuf;

#[test]
fn host_snapshot_enrollment_rejects_unregistered_ambiguous_and_oversized_profiles() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../configs");
    let original = load_manifest(&base.join("root_task.toml")).unwrap();
    original.validate_with_base(Some(&base)).unwrap();
    for defect in [
        "bytes",
        "ttl",
        "value",
        "source",
        "duplicate-source",
        "duplicate-provider",
        "provider",
        "host-disabled",
        "snapshots-disabled",
        "slots",
    ] {
        let mut manifest = original.clone();
        let snapshots = &mut manifest.ecosystem.host.snapshots;
        match defect {
            "bytes" => snapshots.max_bytes = 8193,
            "ttl" => snapshots.max_ttl_ms = 30001,
            "value" => snapshots.max_value_bytes = 4097,
            "source" => snapshots.publishers[0].source_id = "s".repeat(33),
            "duplicate-source" => snapshots.publishers.push(snapshots.publishers[0].clone()),
            "duplicate-provider" => snapshots.publishers[0].providers.push("systemd".into()),
            "provider" => snapshots.publishers[0].providers[0] = "future-provider".into(),
            "host-disabled" => manifest.ecosystem.host.enable = false,
            "snapshots-disabled" => snapshots.enable = false,
            "slots" => {
                snapshots.publishers = (0..8)
                    .map(|index| HostSnapshotPublisher {
                        source_id: format!("source-{index}"),
                        providers: vec!["systemd".into(), "network".into(), "docker".into()],
                    })
                    .collect()
            }
            _ => unreachable!(),
        }
        assert!(
            manifest.validate_with_base(Some(&base)).is_err(),
            "{defect}"
        );
    }
}
