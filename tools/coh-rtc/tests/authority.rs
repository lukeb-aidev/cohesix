// Author: Lukas Bower
// Purpose: Verify production authority prerequisites and deferred milestone refusals.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use coh_rtc::{authority, ir};
use std::path::PathBuf;

fn manifest() -> ir::Manifest {
    ir::load_manifest(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../configs/root_task.toml"),
    )
    .expect("canonical source")
}

#[test]
fn release_a_requires_references_strict_control_and_audit() {
    let temp = tempfile::TempDir::new().expect("temporary keys");
    let key_path = temp.path().join("verification.hex");
    let public = ed25519_dalek::SigningKey::from_bytes(&[42; 32]).verifying_key();
    std::fs::write(&key_path, hex::encode(public.to_bytes())).expect("test public key");
    let profile = authority::release_a(manifest(), &key_path, 4).expect("profile");
    authority::validate(&profile).expect("complete authority floor");
    assert_eq!(profile.authority.writer_epoch, 4);
    assert!(profile.ecosystem.audit.enable && profile.ecosystem.audit.replay_enable);
    assert!(profile
        .tickets
        .iter()
        .all(|ticket| ticket.secret.is_empty() && ticket.secret_ref.is_some()));
    for change in [0, 1, 2, 3, 4] {
        let mut invalid = profile.clone();
        match change {
            0 => invalid.tickets[0].secret = "bootstrap".into(),
            1 => invalid.authority.legacy_queen_ctl = true,
            2 => invalid.ecosystem.audit.replay_enable = false,
            3 => invalid.authority.writer_epoch_required = false,
            _ => invalid.authority.debug_memory = true,
        }
        assert!(
            authority::validate(&invalid).is_err(),
            "production gate {change}"
        );
    }
}

#[test]
fn future_authority_cannot_be_claimed_by_target_profiles() {
    for index in 0..6 {
        let mut profile = manifest();
        match index {
            0 => profile.authority.production_worker_ledger = true,
            1 => profile.authority.production_driver_ledger = true,
            2 => profile.authority.structured_quarantine = true,
            3 => profile.authority.vm_verified_delegation = true,
            4 => profile.authority.host_ai = true,
            _ => profile.authority.production_failover = true,
        }
        assert!(
            authority::validate(&profile).is_err(),
            "unaccepted milestone claim {index}"
        );
    }
}

#[test]
fn selected_federation_preserves_the_production_fence_and_audit_floor() {
    let temp = tempfile::TempDir::new().expect("temporary public key");
    let key = temp.path().join("verification.hex");
    std::fs::write(&key, "public-key-shape-only").expect("public fixture");
    let mut selected = authority::release_a(manifest(), &key, 7).expect("authority profile");
    assert!(!selected.ecosystem.host.federation.enable);
    selected.ecosystem.host.federation.enable = true;
    authority::validate(&selected).expect("M27b host relay selection is not a use-case claim");
    for field in 0..3 {
        let mut invalid = selected.clone();
        match field {
            0 => invalid.authority.execution_wal_required = false,
            1 => invalid.authority.writer_epoch_required = false,
            _ => invalid.ecosystem.audit.replay_enable = false,
        }
        assert!(authority::validate(&invalid).is_err());
    }
}

#[test]
fn published_cas_key_is_not_a_production_trust_root() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let profile = authority::release_a(
        manifest(),
        &root.join("resources/keys/cas_verification_key.hex"),
        1,
    )
    .expect("shape validation");
    let error = profile
        .validate_with_base(Some(&root))
        .expect_err("published fixture must fail");
    assert!(error.to_string().contains("published fixture"));
}
