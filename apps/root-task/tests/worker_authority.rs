// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify generated executable Worker authority remains badge- and MCS-bound.
// Author: Lukas Bower

use cohesix_ticket::Role;
use root_task::generated::WorkerSchedulingProfile;
use root_task::worker_authority::{
    endpoint_badge, notification_badge, require_endpoint_invocation, role_is_implemented,
    scheduling_evidence, verify_notification_badge, WorkerAuthorityError, WorkerEndpointAction,
    WorkerNotificationEvent,
};

#[test]
fn exact_worker_roles_are_executable_but_worker_bus_is_not() {
    for role in [Role::WorkerHeartbeat, Role::WorkerGpu, Role::WorkerLora] {
        assert!(role_is_implemented(role));
        assert!(endpoint_badge(WorkerEndpointAction::Attach, role, 0).is_some());
    }
    assert!(!role_is_implemented(Role::WorkerBus));
    assert_eq!(
        endpoint_badge(WorkerEndpointAction::Attach, Role::WorkerBus, 0),
        None
    );
    let err =
        require_endpoint_invocation(WorkerEndpointAction::Attach, Role::WorkerHeartbeat, 0, None)
            .expect_err("cap-free worker attach must be rejected");
    assert_eq!(err, WorkerAuthorityError::MetadataOnly);
}

#[test]
fn reserved_endpoint_badge_is_not_live_authority() {
    let badge = endpoint_badge(WorkerEndpointAction::Attach, Role::WorkerHeartbeat, 0)
        .expect("generated badge");
    let observation = require_endpoint_invocation(
        WorkerEndpointAction::Attach,
        Role::WorkerHeartbeat,
        0,
        Some(badge),
    )
    .expect("generated endpoint range grants exact authority");
    assert_eq!(observation.badge, badge);
}

#[test]
fn worker_notification_badges_are_enabled_and_exact() {
    let badge =
        notification_badge(WorkerNotificationEvent::LeaseExpiry).expect("lease expiry badge");
    assert_eq!(
        verify_notification_badge(WorkerNotificationEvent::LeaseExpiry, badge),
        Ok(())
    );
}

#[test]
fn worker_scheduling_record_is_mcs_and_consumed_time_bound() {
    let evidence = scheduling_evidence();
    assert_eq!(evidence.profile, WorkerSchedulingProfile::Mcs);
    assert!(evidence.service_turn_budget > 0);
    assert!(evidence.mcs_budget_us > 0);
    assert!(evidence.mcs_period_us >= evidence.mcs_budget_us);
    assert_ne!(evidence.timeout_endpoint_badge, 0);
    assert!(evidence.consumed_budget_evidence);
}
