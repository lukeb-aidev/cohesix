// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Render bounded MCS operator records that remain readable on the Pi local seat.
// Author: Lukas Bower

use core::fmt::{self, Write};

use heapless::{String, Vec};

use crate::generated::KernelObjectBudget;

/// Maximum record width preserved by the Pi linked-HDMI fallback geometry.
pub(crate) const MCS_OPERATOR_DISPLAY_LINE_CAPACITY: usize = 77;

pub(crate) const CAPS_RUNTIME_RECORDS: usize = 8;
pub(crate) const CAPS_GENERATED_RECORDS: usize = 6;

pub(crate) type OperatorLine = String<MCS_OPERATOR_DISPLAY_LINE_CAPACITY>;

/// Copied registry evidence; absence never supplies invented generation values.
#[cfg(any(feature = "release-pi4", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct McsRegistrationState {
    pub(crate) generation: [u64; 3],
    pub(crate) terminal: bool,
}

/// Pair adjacent non-Worker registrations without increasing console capacity.
/// `base` indexes the ordered generated non-Worker task rows in this snapshot.
#[cfg(any(feature = "release-pi4", test))]
pub(crate) fn mcs_registration_pair_line(
    base: u16,
    registrations: &[Option<McsRegistrationState>],
) -> Result<String<256>, fmt::Error> {
    if registrations.is_empty()
        || registrations.len() > 2
        || base.checked_add((registrations.len() - 1) as u16).is_none()
    {
        return Err(fmt::Error);
    }
    let mut line = String::new();
    write!(
        line,
        "[smp:registry/v1] base={base} count={} registration=",
        registrations.len()
    )?;
    for (index, registration) in registrations.iter().enumerate() {
        if index != 0 {
            line.push(',').map_err(|_| fmt::Error)?;
        }
        line.push_str(if registration.is_some() {
            "present"
        } else {
            "missing"
        })
        .map_err(|_| fmt::Error)?;
    }
    for (index, registration) in registrations.iter().enumerate() {
        match registration {
            Some(state) => write!(
                line,
                " generation{index}={}/{}/{}",
                state.generation[0], state.generation[1], state.generation[2]
            )?,
            None => write!(line, " generation{index}=none")?,
        }
    }
    line.push_str(" terminal=").map_err(|_| fmt::Error)?;
    for (index, registration) in registrations.iter().enumerate() {
        if index != 0 {
            line.push(',').map_err(|_| fmt::Error)?;
        }
        line.push_str(
            registration
                .as_ref()
                .map_or("unknown", |state| yes_no(state.terminal)),
        )
        .map_err(|_| fmt::Error)?;
    }
    Ok(line)
}

/// Copied live authority state rendered by `caps mcs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CapsRuntimeState {
    pub(crate) registry_len: usize,
    pub(crate) registry_capacity: u16,
    pub(crate) registry_sealed: bool,
    pub(crate) fault_receiver_active: bool,
    pub(crate) root_control_active: bool,
    pub(crate) fatal: bool,
    pub(crate) fault_endpoint_present: bool,
    pub(crate) root_fault_cnode_present: bool,
    pub(crate) driver_supervisor_cnode_present: bool,
    pub(crate) pending_fault: bool,
    pub(crate) recovered_timeout_mask: u64,
}

/// Compiler-owned object scope rendered by `caps mcs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapsObjectScope {
    Fixed,
    Capacity,
}

impl CapsObjectScope {
    const fn label(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Capacity => "capacity",
        }
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn active_inactive(value: bool) -> &'static str {
    if value {
        "active"
    } else {
        "inactive"
    }
}

fn present_absent(value: bool) -> &'static str {
    if value {
        "present"
    } else {
        "absent"
    }
}

fn push_record<const N: usize>(lines: &mut Vec<OperatorLine, N>, args: fmt::Arguments<'_>) {
    let mut line = OperatorLine::new();
    let _ = line.write_fmt(args);
    let _ = lines.push(line);
}

/// Render live `caps mcs` state as independently source-labelled display rows.
pub(crate) fn caps_runtime_lines(
    state: CapsRuntimeState,
) -> Vec<OperatorLine, CAPS_RUNTIME_RECORDS> {
    let mut lines = Vec::new();
    push_record(
        &mut lines,
        format_args!(
            "[caps:mcs/v1] source=runtime scope=registry registry={}/{} sealed={}",
            state.registry_len,
            state.registry_capacity,
            yes_no(state.registry_sealed)
        ),
    );
    push_record(
        &mut lines,
        format_args!(
            "[caps:mcs/v1] source=runtime scope=control fault_rx={}",
            active_inactive(state.fault_receiver_active)
        ),
    );
    push_record(
        &mut lines,
        format_args!(
            "[caps:mcs/v1] source=runtime scope=control root_control={} fatal={}",
            active_inactive(state.root_control_active),
            yes_no(state.fatal)
        ),
    );
    push_record(
        &mut lines,
        format_args!(
            "[caps:mcs/v1] source=runtime scope=authority fault_endpoint={}",
            present_absent(state.fault_endpoint_present)
        ),
    );
    push_record(
        &mut lines,
        format_args!(
            "[caps:mcs/v1] source=runtime scope=authority root_fault_cnode={}",
            present_absent(state.root_fault_cnode_present)
        ),
    );
    push_record(
        &mut lines,
        format_args!(
            "[caps:mcs/v1] source=runtime scope=authority driver_supervisor_cnode={}",
            present_absent(state.driver_supervisor_cnode_present)
        ),
    );
    push_record(
        &mut lines,
        format_args!(
            "[caps:mcs/v1] source=runtime scope=authority pending_fault={}",
            yes_no(state.pending_fault)
        ),
    );
    push_record(
        &mut lines,
        format_args!(
            "[caps:mcs/v1] source=runtime scope=recovery timeout_mask=0x{:016x}",
            state.recovered_timeout_mask
        ),
    );
    lines
}

/// Render one generated object scope as independently source-labelled rows.
pub(crate) fn caps_generated_lines(
    scope: CapsObjectScope,
    objects: KernelObjectBudget,
) -> Vec<OperatorLine, CAPS_GENERATED_RECORDS> {
    let scope = scope.label();
    let mut lines = Vec::new();
    for (field, value) in [
        ("tcbs", objects.tcbs),
        ("scs", objects.scheduling_contexts),
        ("replies", objects.reply_objects),
        ("fault_caps", objects.fault_caps),
        ("timeout_fault_caps", objects.timeout_fault_caps),
        ("cspace_slots", objects.cspace_slots),
    ] {
        push_record(
            &mut lines,
            format_args!("[caps:mcs/v1] source=generated scope={scope} {field}={value}"),
        );
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_registry_rows_preserve_maximum_width_generations() {
        let registration = Some(McsRegistrationState {
            generation: [u64::MAX; 3],
            terminal: false,
        });
        let line = mcs_registration_pair_line(65534, &[registration; 2]).unwrap();
        assert_eq!(line.as_str(), "[smp:registry/v1] base=65534 count=2 registration=present,present generation0=18446744073709551615/18446744073709551615/18446744073709551615 generation1=18446744073709551615/18446744073709551615/18446744073709551615 terminal=no,no");
    }

    #[test]
    fn paired_registry_rows_distinguish_missing_and_zero_generations() {
        let state = Some(McsRegistrationState {
            generation: [0; 3],
            terminal: true,
        });
        assert_eq!(mcs_registration_pair_line(0, &[None, state]).unwrap().as_str(), "[smp:registry/v1] base=0 count=2 registration=missing,present generation0=none generation1=0/0/0 terminal=unknown,yes");
        assert_eq!(mcs_registration_pair_line(65535, &[None]).unwrap().as_str(), "[smp:registry/v1] base=65535 count=1 registration=missing generation0=none terminal=unknown");
        assert!(mcs_registration_pair_line(0, &[]).is_err());
        assert!(mcs_registration_pair_line(0, &[None; 3]).is_err());
        assert!(mcs_registration_pair_line(65535, &[None; 2]).is_err());
    }

    fn assert_display_safe(lines: &[OperatorLine]) {
        for line in lines {
            assert!(
                line.len() <= MCS_OPERATOR_DISPLAY_LINE_CAPACITY,
                "record exceeds linked-HDMI fallback width: {line}"
            );
        }
    }

    #[test]
    fn runtime_records_preserve_authority_state_within_display_width() {
        let lines = caps_runtime_lines(CapsRuntimeState {
            registry_len: 64,
            registry_capacity: 64,
            registry_sealed: true,
            fault_receiver_active: true,
            root_control_active: true,
            fatal: false,
            fault_endpoint_present: true,
            root_fault_cnode_present: true,
            driver_supervisor_cnode_present: true,
            pending_fault: false,
            recovered_timeout_mask: u64::MAX,
        });

        assert_eq!(lines.len(), CAPS_RUNTIME_RECORDS);
        assert_display_safe(lines.as_slice());
        assert!(lines
            .iter()
            .any(|line| line.contains("registry=64/64 sealed=yes")));
        assert!(lines.iter().any(|line| line.contains("fault_rx=active")));
        assert!(lines
            .iter()
            .any(|line| line.contains("root_control=active fatal=no")));
        assert!(lines
            .iter()
            .any(|line| line.contains("driver_supervisor_cnode=present")));
        assert!(lines.iter().any(|line| line.contains("pending_fault=no")));
        assert!(lines
            .iter()
            .any(|line| line.contains("scope=recovery timeout_mask=0xffffffffffffffff")));
    }

    #[test]
    fn generated_records_preserve_maximum_counts_within_display_width() {
        let objects = KernelObjectBudget {
            tcbs: u32::MAX,
            scheduling_contexts: u32::MAX,
            reply_objects: u32::MAX,
            fault_caps: u32::MAX,
            timeout_fault_caps: u32::MAX,
            cspace_slots: u32::MAX,
            ..KernelObjectBudget::default()
        };
        let lines = caps_generated_lines(CapsObjectScope::Capacity, objects);

        assert_eq!(lines.len(), CAPS_GENERATED_RECORDS);
        assert_display_safe(lines.as_slice());
        for field in [
            "tcbs=4294967295",
            "scs=4294967295",
            "replies=4294967295",
            "fault_caps=4294967295",
            "timeout_fault_caps=4294967295",
            "cspace_slots=4294967295",
        ] {
            assert!(lines.iter().any(|line| line.contains(field)), "{field}");
        }
    }
}
