// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify generated worker endpoint, notification, and scheduling evidence.
// Author: Lukas Bower

use cohesix_ticket::Role;
use root_task::generated::WorkerSchedulingProfile;
use root_task::worker_authority::{
    endpoint_badge, notification_badge, require_endpoint_invocation, role_is_implemented,
    scheduling_evidence, verify_notification_badge, WorkerAuthorityError, WorkerEndpointAction,
    WorkerNotificationEvent,
};

#[test]
fn worker_endpoint_cap_is_required_for_attach() {
    let err =
        require_endpoint_invocation(WorkerEndpointAction::Attach, Role::WorkerHeartbeat, 0, None)
            .expect_err("metadata-only worker attach must be rejected");
    assert_eq!(err, WorkerAuthorityError::MetadataOnly);
}

#[test]
fn worker_endpoint_badge_must_match_action_role_and_epoch() {
    let badge = endpoint_badge(WorkerEndpointAction::Receipt, Role::WorkerGpu, 9)
        .expect("generated receipt badge");
    let observation = require_endpoint_invocation(
        WorkerEndpointAction::Receipt,
        Role::WorkerGpu,
        9,
        Some(badge),
    )
    .expect("matching endpoint badge");
    assert_eq!(observation.action, WorkerEndpointAction::Receipt);
    assert_eq!(observation.role, Role::WorkerGpu);
    assert_eq!(observation.epoch, 9);

    let err = require_endpoint_invocation(
        WorkerEndpointAction::Telemetry,
        Role::WorkerGpu,
        9,
        Some(badge),
    )
    .expect_err("wrong endpoint action must fail");
    assert_eq!(err, WorkerAuthorityError::BadgeActionMismatch);
}

#[test]
fn worker_endpoint_epoch_is_revocation_sensitive() {
    let badge = endpoint_badge(WorkerEndpointAction::Revoke, Role::WorkerLora, 2)
        .expect("generated revoke badge");
    let err = require_endpoint_invocation(
        WorkerEndpointAction::Revoke,
        Role::WorkerLora,
        3,
        Some(badge),
    )
    .expect_err("stale revoke epoch must fail");
    assert_eq!(err, WorkerAuthorityError::BadgeEpochMismatch);
}

#[test]
fn worker_bus_remains_deferred_not_implemented() {
    assert!(!role_is_implemented(Role::WorkerBus));
    assert_eq!(
        endpoint_badge(WorkerEndpointAction::Attach, Role::WorkerBus, 0),
        None
    );
}

#[test]
fn worker_notification_badges_are_event_specific() {
    let lease_expiry = notification_badge(WorkerNotificationEvent::LeaseExpiry);
    verify_notification_badge(WorkerNotificationEvent::LeaseExpiry, lease_expiry)
        .expect("lease-expiry badge");
    let err = verify_notification_badge(WorkerNotificationEvent::TelemetryPressure, lease_expiry)
        .expect_err("wrong notification event must fail");
    assert_eq!(err, WorkerAuthorityError::NotificationMismatch);
}

#[test]
fn qemu_worker_scheduling_uses_non_mcs_fallback() {
    let evidence = scheduling_evidence();
    assert_eq!(evidence.profile, WorkerSchedulingProfile::NonMcs);
    assert!(evidence.service_turn_budget > 0);
    assert_eq!(evidence.mcs_budget_us, 0);
    assert_eq!(evidence.mcs_period_us, 0);
    assert_eq!(evidence.timeout_endpoint_badge, 0);
    assert!(!evidence.consumed_budget_evidence);
}
