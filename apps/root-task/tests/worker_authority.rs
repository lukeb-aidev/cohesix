// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify disabled Worker execution metadata does not become live authority.
// Author: Lukas Bower

use cohesix_ticket::Role;
use root_task::generated::WorkerSchedulingProfile;
use root_task::worker_authority::{
    endpoint_badge, notification_badge, require_endpoint_invocation, role_is_implemented,
    scheduling_evidence, verify_notification_badge, WorkerAuthorityError, WorkerEndpointAction,
    WorkerNotificationEvent,
};

#[test]
fn modeled_worker_roles_are_not_executable() {
    for role in [
        Role::WorkerHeartbeat,
        Role::WorkerGpu,
        Role::WorkerBus,
        Role::WorkerLora,
    ] {
        assert!(!role_is_implemented(role));
        assert_eq!(endpoint_badge(WorkerEndpointAction::Attach, role, 0), None);
    }
    let err =
        require_endpoint_invocation(WorkerEndpointAction::Attach, Role::WorkerHeartbeat, 0, None)
            .expect_err("modeled worker attach must be rejected");
    assert_eq!(err, WorkerAuthorityError::RoleNotImplemented);
}

#[test]
fn reserved_endpoint_badge_is_not_live_authority() {
    let err = require_endpoint_invocation(
        WorkerEndpointAction::Attach,
        Role::WorkerHeartbeat,
        0,
        Some(0x260c_1000),
    )
    .expect_err("reserved endpoint range must not grant authority");
    assert_eq!(err, WorkerAuthorityError::RoleNotImplemented);
}

#[test]
fn worker_notification_badges_are_disabled() {
    assert_eq!(
        notification_badge(WorkerNotificationEvent::LeaseExpiry),
        None
    );
    let err = verify_notification_badge(WorkerNotificationEvent::LeaseExpiry, 0x260c_8000)
        .expect_err("reserved notification badge must not be active");
    assert_eq!(err, WorkerAuthorityError::NotificationDisabled);
}

#[test]
fn worker_scheduling_record_is_non_mcs_metadata_only() {
    let evidence = scheduling_evidence();
    assert_eq!(evidence.profile, WorkerSchedulingProfile::NonMcs);
    assert!(evidence.service_turn_budget > 0);
    assert_eq!(evidence.mcs_budget_us, 0);
    assert_eq!(evidence.mcs_period_us, 0);
    assert_eq!(evidence.timeout_endpoint_badge, 0);
    assert!(!evidence.consumed_budget_evidence);
}
