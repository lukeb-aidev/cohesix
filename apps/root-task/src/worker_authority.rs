// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Decode generated worker endpoint, notification, and scheduling evidence.
// Author: Lukas Bower

use cohesix_ticket::Role;

use crate::generated::{self, WorkerSchedulingProfile};

/// Worker endpoint actions that require a generated badged cap invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerEndpointAction {
    /// Worker attach path.
    Attach,
    /// Worker telemetry append path.
    Telemetry,
    /// Lease renewal path.
    LeaseRenewal,
    /// Receipt publication path.
    Receipt,
    /// Revocation-sensitive path.
    Revoke,
}

/// Worker lifecycle notifications described by the generated manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerNotificationEvent {
    /// Revocation signal.
    Revoke,
    /// Shutdown signal.
    Shutdown,
    /// Lease-expiry signal.
    LeaseExpiry,
    /// Telemetry backpressure signal.
    TelemetryPressure,
    /// Driver IRQ wake signal, when a worker role consumes one.
    Irq,
}

/// A decoded endpoint-cap invocation badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerEndpointObservation {
    /// Endpoint action namespace.
    pub action: WorkerEndpointAction,
    /// Role encoded by the badge.
    pub role: Role,
    /// Lease or authority epoch encoded by the badge.
    pub epoch: u64,
    /// Original seL4 sender badge.
    pub badge: u64,
}

/// Profile-qualified scheduling evidence exported from generated truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerSchedulingEvidence {
    /// Scheduling profile selected by the manifest.
    pub profile: WorkerSchedulingProfile,
    /// Non-MCS TCB priority evidence.
    pub priority: u8,
    /// Non-MCS domain evidence.
    pub domain: u8,
    /// Non-MCS bounded service-turn fallback.
    pub service_turn_budget: u16,
    /// MCS budget in microseconds, or zero for non-MCS profiles.
    pub mcs_budget_us: u32,
    /// MCS period in microseconds, or zero for non-MCS profiles.
    pub mcs_period_us: u32,
    /// Timeout endpoint badge, or zero for non-MCS profiles.
    pub timeout_endpoint_badge: u64,
    /// Whether consumed-budget evidence is generated for this profile.
    pub consumed_budget_evidence: bool,
}

/// Worker authority validation failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerAuthorityError {
    /// The manifest requires a badged endpoint invocation and none was supplied.
    MetadataOnly,
    /// The role is not implemented as a VM worker role in the generated manifest.
    RoleNotImplemented,
    /// The badge did not fall into any generated worker endpoint range.
    BadgeUnknown,
    /// The badge decoded to a different action than the caller required.
    BadgeActionMismatch,
    /// The badge decoded to a different role than the caller required.
    BadgeRoleMismatch,
    /// The badge decoded to a different lease or authority epoch.
    BadgeEpochMismatch,
    /// The notification badge did not match the expected generated event.
    NotificationMismatch,
}

/// Return true when `role` is implemented as a VM-side worker role.
#[must_use]
pub fn role_is_implemented(role: Role) -> bool {
    generated::worker_runtime_roles()
        .iter()
        .any(|entry| entry.role == role && entry.implemented)
}

/// Compute the expected endpoint badge for an action/role/epoch tuple.
#[must_use]
pub fn endpoint_badge(action: WorkerEndpointAction, role: Role, epoch: u64) -> Option<u64> {
    let config = generated::worker_runtime_config();
    if !config.endpoint_caps.required || !role_is_implemented(role) {
        return None;
    }
    let epoch_bits = config.endpoint_caps.epoch_bits;
    let role_bits = config.endpoint_caps.role_bits;
    let epoch_limit = 1u64.checked_shl(u32::from(epoch_bits))?;
    let role_limit = 1u64.checked_shl(u32::from(role_bits))?;
    let role_index = role_index(role)?;
    if epoch >= epoch_limit || u64::from(role_index) >= role_limit {
        return None;
    }
    let base = endpoint_base(action);
    base.checked_add((u64::from(role_index) << epoch_bits) | epoch)
}

/// Decode a seL4 endpoint sender badge against generated worker ranges.
pub fn observe_endpoint_badge(
    badge: u64,
) -> Result<WorkerEndpointObservation, WorkerAuthorityError> {
    let config = generated::worker_runtime_config();
    if !config.endpoint_caps.required {
        return Err(WorkerAuthorityError::MetadataOnly);
    }
    let epoch_bits = config.endpoint_caps.epoch_bits;
    let role_bits = config.endpoint_caps.role_bits;
    let span_bits = u32::from(epoch_bits) + u32::from(role_bits);
    let span = 1u64
        .checked_shl(span_bits)
        .ok_or(WorkerAuthorityError::BadgeUnknown)?;
    let epoch_mask = (1u64 << epoch_bits).saturating_sub(1);
    for action in [
        WorkerEndpointAction::Attach,
        WorkerEndpointAction::Telemetry,
        WorkerEndpointAction::LeaseRenewal,
        WorkerEndpointAction::Receipt,
        WorkerEndpointAction::Revoke,
    ] {
        let base = endpoint_base(action);
        let Some(end) = base.checked_add(span) else {
            continue;
        };
        if badge < base || badge >= end {
            continue;
        }
        let offset = badge - base;
        let epoch = offset & epoch_mask;
        let role_raw = (offset >> epoch_bits) & ((1u64 << role_bits).saturating_sub(1));
        let role = role_from_index(role_raw as u8).ok_or(WorkerAuthorityError::BadgeUnknown)?;
        if !role_is_implemented(role) {
            return Err(WorkerAuthorityError::RoleNotImplemented);
        }
        return Ok(WorkerEndpointObservation {
            action,
            role,
            epoch,
            badge,
        });
    }
    Err(WorkerAuthorityError::BadgeUnknown)
}

/// Require a matching endpoint-cap invocation for a worker action.
pub fn require_endpoint_invocation(
    action: WorkerEndpointAction,
    role: Role,
    epoch: u64,
    badge: Option<u64>,
) -> Result<WorkerEndpointObservation, WorkerAuthorityError> {
    if !role_is_implemented(role) {
        return Err(WorkerAuthorityError::RoleNotImplemented);
    }
    let Some(badge) = badge else {
        return Err(WorkerAuthorityError::MetadataOnly);
    };
    let observation = observe_endpoint_badge(badge)?;
    if observation.action != action {
        return Err(WorkerAuthorityError::BadgeActionMismatch);
    }
    if observation.role != role {
        return Err(WorkerAuthorityError::BadgeRoleMismatch);
    }
    if observation.epoch != epoch {
        return Err(WorkerAuthorityError::BadgeEpochMismatch);
    }
    Ok(observation)
}

/// Return the generated notification badge for a lifecycle event.
#[must_use]
pub fn notification_badge(event: WorkerNotificationEvent) -> u64 {
    let notifications = generated::worker_runtime_config().notifications;
    match event {
        WorkerNotificationEvent::Revoke => notifications.revoke_badge,
        WorkerNotificationEvent::Shutdown => notifications.shutdown_badge,
        WorkerNotificationEvent::LeaseExpiry => notifications.lease_expiry_badge,
        WorkerNotificationEvent::TelemetryPressure => notifications.telemetry_pressure_badge,
        WorkerNotificationEvent::Irq => notifications.irq_badge,
    }
}

/// Verify that a notification badge matches the expected generated lifecycle event.
pub fn verify_notification_badge(
    event: WorkerNotificationEvent,
    badge: u64,
) -> Result<(), WorkerAuthorityError> {
    if notification_badge(event) == badge {
        Ok(())
    } else {
        Err(WorkerAuthorityError::NotificationMismatch)
    }
}

/// Return generated profile-qualified scheduling evidence.
#[must_use]
pub fn scheduling_evidence() -> WorkerSchedulingEvidence {
    let scheduling = generated::worker_runtime_config().scheduling;
    WorkerSchedulingEvidence {
        profile: scheduling.profile,
        priority: scheduling.priority,
        domain: scheduling.domain,
        service_turn_budget: scheduling.service_turn_budget,
        mcs_budget_us: scheduling.mcs_budget_us,
        mcs_period_us: scheduling.mcs_period_us,
        timeout_endpoint_badge: scheduling.timeout_endpoint_badge,
        consumed_budget_evidence: scheduling.consumed_budget_evidence,
    }
}

fn endpoint_base(action: WorkerEndpointAction) -> u64 {
    let endpoint_caps = generated::worker_runtime_config().endpoint_caps;
    match action {
        WorkerEndpointAction::Attach => endpoint_caps.attach_badge_base,
        WorkerEndpointAction::Telemetry => endpoint_caps.telemetry_badge_base,
        WorkerEndpointAction::LeaseRenewal => endpoint_caps.lease_badge_base,
        WorkerEndpointAction::Receipt => endpoint_caps.receipt_badge_base,
        WorkerEndpointAction::Revoke => endpoint_caps.revoke_badge_base,
    }
}

fn role_index(role: Role) -> Option<u8> {
    match role {
        Role::WorkerHeartbeat => Some(1),
        Role::WorkerGpu => Some(2),
        Role::WorkerBus => Some(3),
        Role::WorkerLora => Some(4),
        Role::Queen => None,
    }
}

fn role_from_index(index: u8) -> Option<Role> {
    match index {
        1 => Some(Role::WorkerHeartbeat),
        2 => Some(Role::WorkerGpu),
        3 => Some(Role::WorkerBus),
        4 => Some(Role::WorkerLora),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_only_worker_authority_is_rejected() {
        let err = require_endpoint_invocation(
            WorkerEndpointAction::Attach,
            Role::WorkerHeartbeat,
            1,
            None,
        )
        .expect_err("metadata-only worker attach must fail");
        assert_eq!(err, WorkerAuthorityError::MetadataOnly);
    }

    #[test]
    fn matching_endpoint_badge_decodes_role_action_and_epoch() {
        let badge = endpoint_badge(WorkerEndpointAction::Telemetry, Role::WorkerGpu, 7)
            .expect("generated badge");
        let observation = require_endpoint_invocation(
            WorkerEndpointAction::Telemetry,
            Role::WorkerGpu,
            7,
            Some(badge),
        )
        .expect("valid worker badge");
        assert_eq!(observation.action, WorkerEndpointAction::Telemetry);
        assert_eq!(observation.role, Role::WorkerGpu);
        assert_eq!(observation.epoch, 7);
    }

    #[test]
    fn stale_epoch_badge_is_rejected() {
        let badge = endpoint_badge(WorkerEndpointAction::Attach, Role::WorkerHeartbeat, 3)
            .expect("generated badge");
        let err = require_endpoint_invocation(
            WorkerEndpointAction::Attach,
            Role::WorkerHeartbeat,
            4,
            Some(badge),
        )
        .expect_err("stale epoch must fail");
        assert_eq!(err, WorkerAuthorityError::BadgeEpochMismatch);
    }

    #[test]
    fn deferred_worker_bus_is_not_implemented() {
        assert!(!role_is_implemented(Role::WorkerBus));
        let badge = endpoint_badge(WorkerEndpointAction::Attach, Role::WorkerBus, 0);
        assert_eq!(badge, None);
    }

    #[test]
    fn notification_badges_match_generated_events() {
        let revoke = notification_badge(WorkerNotificationEvent::Revoke);
        verify_notification_badge(WorkerNotificationEvent::Revoke, revoke)
            .expect("revoke badge should match");
        let err = verify_notification_badge(WorkerNotificationEvent::Shutdown, revoke)
            .expect_err("wrong event must fail");
        assert_eq!(err, WorkerAuthorityError::NotificationMismatch);
    }

    #[test]
    fn non_mcs_scheduling_evidence_has_no_mcs_claims() {
        let evidence = scheduling_evidence();
        assert_eq!(evidence.profile, WorkerSchedulingProfile::NonMcs);
        assert!(evidence.service_turn_budget > 0);
        assert_eq!(evidence.mcs_budget_us, 0);
        assert_eq!(evidence.timeout_endpoint_badge, 0);
        assert!(!evidence.consumed_budget_evidence);
    }
}
