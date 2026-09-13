// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Require matching finding dispositions and risk arithmetic across audit ledgers.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

#[test]
fn active_exceptions_reference_current_findings_and_never_close_accepted_risk() {
    let findings: BTreeMap<_, _> = include_str!("../../docs/audit/findings.csv")
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut columns = line.split(',');
            Some((columns.next()?, (columns.next()?, columns.next()?)))
        })
        .collect();
    let exceptions = include_str!("../../docs/audit/EXCEPTIONS.md");
    for row in exceptions
        .lines()
        .filter(|line| line.starts_with("| `EX-") && line.contains("APPROVED_ACTIVE"))
    {
        let id = row.split('|').nth(2).unwrap().trim().trim_matches('`');
        let (severity, status) = findings
            .get(id)
            .expect("active exception must reference a finding");
        assert_ne!(
            *status, "CLOSED_VERIFIED",
            "active exception for {id} contradicts closure"
        );
        if *severity == "P1" {
            assert_eq!(id, "DD-2026-0030");
            assert_eq!(*status, "ACCEPTED_RISK");
            assert!(row.contains("release=1.0.0-beta"));
            assert!(include_str!("../../docs/audit/BLOCKERS.md").contains(id));
        }
    }
    assert!(!exceptions.lines().any(|line| line.trim() == "None"));
}

#[test]
fn risk_baseline_partitions_match_without_raising_any_ceiling() {
    let mut section = "";
    let mut counts = BTreeMap::new();
    for line in include_str!("../../docs/audit/rust_risk_baseline.toml").lines() {
        if line.starts_with('[') {
            section = line.trim_matches(['[', ']']);
        }
        if matches!(
            section,
            "non_test" | "linked_runtime_hal" | "outside_linked_runtime_hal"
        ) {
            if let Some((key, value)) = line.split_once('=') {
                counts.insert(
                    (section, key.trim()),
                    value
                        .trim()
                        .parse::<u64>()
                        .expect("risk counter is an integer"),
                );
            }
        }
    }
    for key in ["unsafe", "unwrap", "expect", "panic"] {
        assert_eq!(
            counts[&("non_test", key)],
            counts[&("linked_runtime_hal", key)] + counts[&("outside_linked_runtime_hal", key)]
        );
    }
}
