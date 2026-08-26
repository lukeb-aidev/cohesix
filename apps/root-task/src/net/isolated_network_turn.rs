// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Bound one isolated QEMU console-network unit per outer Network visit.
// Author: Lukas Bower

/// One ordinary isolated console-network unit below ACK/diagnostic/TX preemption.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IsolatedNetworkLowerUnit {
    ObserveChild,
    StageOutput,
    Disconnect,
    Ingress,
    ServiceTick,
}

impl IsolatedNetworkLowerUnit {
    const fn next(self) -> Self {
        match self {
            Self::ObserveChild => Self::StageOutput,
            Self::StageOutput => Self::Disconnect,
            Self::Disconnect => Self::Ingress,
            Self::Ingress => Self::ServiceTick,
            Self::ServiceTick => Self::ObserveChild,
        }
    }
}

/// Persistent ordinary-unit position preserved across Network visits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IsolatedNetworkLowerCursor {
    unit: IsolatedNetworkLowerUnit,
}

impl IsolatedNetworkLowerCursor {
    pub(crate) const fn new() -> Self {
        Self {
            unit: IsolatedNetworkLowerUnit::ObserveChild,
        }
    }

    pub(crate) const fn unit(self) -> IsolatedNetworkLowerUnit {
        self.unit
    }

    pub(crate) const fn for_unit(unit: IsolatedNetworkLowerUnit) -> Self {
        Self { unit }
    }

    fn advance_after(self, completed: IsolatedNetworkLowerUnit, child_signal: bool) -> Self {
        debug_assert_eq!(self.unit, completed);
        let unit = if child_signal {
            IsolatedNetworkLowerUnit::ObserveChild
        } else {
            completed.next()
        };
        Self { unit }
    }
}

impl Default for IsolatedNetworkLowerCursor {
    fn default() -> Self {
        Self::new()
    }
}

/// One selected unit for the current outer Network visit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IsolatedNetworkTurnUnit {
    DeferredDiagnostic,
    TransmitEgress,
    Lower(IsolatedNetworkLowerUnit),
}

/// Result needed to commit the persistent lower cursor after one attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IsolatedNetworkTurnOutcome {
    activity: bool,
    child_signal: bool,
}

impl IsolatedNetworkTurnOutcome {
    pub(crate) const fn complete(activity: bool) -> Self {
        Self {
            activity,
            child_signal: false,
        }
    }

    pub(crate) const fn child_signaled(activity: bool) -> Self {
        Self {
            activity,
            child_signal: true,
        }
    }

    pub(crate) const fn child_signal_attempt(activity: bool) -> Self {
        if activity {
            Self::child_signaled(true)
        } else {
            Self::complete(false)
        }
    }

    pub(crate) const fn activity(self) -> bool {
        self.activity
    }

    pub(crate) const fn child_signal(self) -> bool {
        self.child_signal
    }
}

/// One selected unit plus the ordinary successor computed before it runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IsolatedNetworkTurnSelection {
    unit: IsolatedNetworkTurnUnit,
    successor: IsolatedNetworkLowerCursor,
}

impl IsolatedNetworkTurnSelection {
    pub(crate) const fn unit(self) -> IsolatedNetworkTurnUnit {
        self.unit
    }

    pub(crate) const fn successor(self) -> IsolatedNetworkLowerCursor {
        self.successor
    }

    /// Apply only the signal-dependent override after the selected unit ends.
    pub(crate) fn finish(
        self,
        outcome: IsolatedNetworkTurnOutcome,
    ) -> (IsolatedNetworkLowerCursor, bool) {
        let successor = match self.unit {
            IsolatedNetworkTurnUnit::Lower(_) if outcome.child_signal() => {
                IsolatedNetworkLowerCursor::new()
            }
            IsolatedNetworkTurnUnit::Lower(_)
            | IsolatedNetworkTurnUnit::DeferredDiagnostic
            | IsolatedNetworkTurnUnit::TransmitEgress => {
                debug_assert!(
                    !outcome.child_signal()
                        || matches!(self.unit, IsolatedNetworkTurnUnit::Lower(_))
                );
                self.successor
            }
        };
        (successor, outcome.activity())
    }
}

/// Select exactly one unit and compute its ordinary successor before work.
pub(crate) fn select_isolated_network_turn(
    deferred_diagnostic: bool,
    pending_egress: bool,
    lower_cursor: IsolatedNetworkLowerCursor,
) -> IsolatedNetworkTurnSelection {
    let unit = if deferred_diagnostic {
        IsolatedNetworkTurnUnit::DeferredDiagnostic
    } else if pending_egress {
        IsolatedNetworkTurnUnit::TransmitEgress
    } else {
        IsolatedNetworkTurnUnit::Lower(lower_cursor.unit())
    };
    let successor = match unit {
        IsolatedNetworkTurnUnit::Lower(unit) => lower_cursor.advance_after(unit, false),
        IsolatedNetworkTurnUnit::DeferredDiagnostic | IsolatedNetworkTurnUnit::TransmitEgress => {
            lower_cursor
        }
    };
    IsolatedNetworkTurnSelection { unit, successor }
}

/// Select one root control-plane unit when the child owns QEMU RX/TX directly.
///
/// The ordinary shared-device rotor retains Ingress because root owns that
/// device. Direct VirtIO makes an ingress or transmit attempt in root both
/// useless and an ownership violation, so this selector advances across only
/// child observation, bounded output control, disconnect, and timer service.
pub(crate) fn select_isolated_direct_network_turn(
    lower_cursor: IsolatedNetworkLowerCursor,
) -> IsolatedNetworkTurnSelection {
    let mut candidate = lower_cursor;
    for _ in 0..5 {
        let unit = candidate.unit();
        let successor = candidate.advance_after(unit, false);
        if !matches!(unit, IsolatedNetworkLowerUnit::Ingress) {
            return IsolatedNetworkTurnSelection {
                unit: IsolatedNetworkTurnUnit::Lower(unit),
                successor,
            };
        }
        candidate = successor;
    }
    IsolatedNetworkTurnSelection {
        unit: IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild),
        successor: IsolatedNetworkLowerCursor {
            unit: IsolatedNetworkLowerUnit::StageOutput,
        },
    }
}

/// Select one useful unit while an exact authenticated response is active.
///
/// Routine diagnostics remain retained for the next ordinary debt turn. An
/// ingress attempt is eligible only while root has exact outstanding response
/// progress; it remains a bounded probe because the VirtIO RX ring has no
/// side-effect-free readiness oracle.
pub(crate) fn select_isolated_response_turn(
    pending_egress: bool,
    stage_output_ready: bool,
    response_progress_outstanding: bool,
    lower_cursor: IsolatedNetworkLowerCursor,
) -> IsolatedNetworkTurnSelection {
    if pending_egress {
        return IsolatedNetworkTurnSelection {
            unit: IsolatedNetworkTurnUnit::TransmitEgress,
            successor: lower_cursor,
        };
    }
    if stage_output_ready {
        return IsolatedNetworkTurnSelection {
            unit: IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput),
            successor: lower_cursor,
        };
    }

    let mut candidate = lower_cursor;
    for _ in 0..5 {
        let unit = candidate.unit();
        let eligible = response_progress_outstanding
            && matches!(
                unit,
                IsolatedNetworkLowerUnit::ObserveChild
                    | IsolatedNetworkLowerUnit::Ingress
                    | IsolatedNetworkLowerUnit::ServiceTick
            );
        if eligible {
            return IsolatedNetworkTurnSelection {
                unit: IsolatedNetworkTurnUnit::Lower(unit),
                successor: candidate.advance_after(unit, false),
            };
        }
        candidate = candidate.advance_after(unit, false);
    }

    // An active lane with no locally retained work can only learn whether the
    // exact child progressed by one bounded notification observation.
    IsolatedNetworkTurnSelection {
        unit: IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild),
        successor: IsolatedNetworkLowerCursor {
            unit: IsolatedNetworkLowerUnit::StageOutput,
        },
    }
}

/// Select useful response progress for a child-owned direct QEMU NIC.
///
/// Root never polls RX or publishes TX frames in this mode. A retained
/// response therefore needs only output staging, child observation, and the
/// rate-limited timer wake that keeps protocol deadlines deterministic.
pub(crate) fn select_isolated_direct_response_turn(
    stage_output_ready: bool,
    response_progress_outstanding: bool,
    lower_cursor: IsolatedNetworkLowerCursor,
) -> IsolatedNetworkTurnSelection {
    if stage_output_ready {
        return IsolatedNetworkTurnSelection {
            unit: IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput),
            successor: lower_cursor,
        };
    }

    let mut candidate = lower_cursor;
    for _ in 0..5 {
        let unit = candidate.unit();
        let successor = candidate.advance_after(unit, false);
        if response_progress_outstanding
            && matches!(
                unit,
                IsolatedNetworkLowerUnit::ObserveChild | IsolatedNetworkLowerUnit::ServiceTick
            )
        {
            return IsolatedNetworkTurnSelection {
                unit: IsolatedNetworkTurnUnit::Lower(unit),
                successor,
            };
        }
        candidate = successor;
    }

    IsolatedNetworkTurnSelection {
        unit: IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild),
        successor: IsolatedNetworkLowerCursor {
            unit: IsolatedNetworkLowerUnit::StageOutput,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_attempts_advance_in_strict_lower_order() {
        let expected = [
            IsolatedNetworkLowerUnit::ObserveChild,
            IsolatedNetworkLowerUnit::StageOutput,
            IsolatedNetworkLowerUnit::Disconnect,
            IsolatedNetworkLowerUnit::Ingress,
            IsolatedNetworkLowerUnit::ServiceTick,
        ];
        let mut cursor = IsolatedNetworkLowerCursor::new();

        for expected_unit in expected {
            let mut calls = 0;
            let selected = select_isolated_network_turn(false, false, cursor);
            calls += 1;
            assert_eq!(
                selected.unit(),
                IsolatedNetworkTurnUnit::Lower(expected_unit)
            );
            let outcome = IsolatedNetworkTurnOutcome::child_signal_attempt(false);
            let (next, activity) = selected.finish(outcome);
            assert_eq!(calls, 1);
            assert!(!activity);
            cursor = next;
        }

        assert_eq!(cursor.unit(), IsolatedNetworkLowerUnit::ObserveChild);
    }

    #[test]
    fn child_signals_force_the_next_lower_observation() {
        let signal_units = [
            IsolatedNetworkLowerUnit::ObserveChild,
            IsolatedNetworkLowerUnit::StageOutput,
            IsolatedNetworkLowerUnit::Disconnect,
            IsolatedNetworkLowerUnit::Ingress,
            IsolatedNetworkLowerUnit::ServiceTick,
        ];

        for signal_unit in signal_units {
            let cursor = IsolatedNetworkLowerCursor { unit: signal_unit };
            let activity = !matches!(signal_unit, IsolatedNetworkLowerUnit::ServiceTick);
            let selected = select_isolated_network_turn(false, false, cursor);
            assert_eq!(selected.unit(), IsolatedNetworkTurnUnit::Lower(signal_unit));
            let outcome = if activity {
                IsolatedNetworkTurnOutcome::child_signal_attempt(true)
            } else {
                IsolatedNetworkTurnOutcome::child_signaled(false)
            };
            let (next, observed_activity) = selected.finish(outcome);
            assert_eq!(next.unit(), IsolatedNetworkLowerUnit::ObserveChild);
            assert_eq!(observed_activity, activity);
        }
    }

    #[test]
    fn tx_and_diagnostic_preempt_without_advancing_lower_cursor() {
        let selected =
            select_isolated_network_turn(false, false, IsolatedNetworkLowerCursor::new());
        assert_eq!(
            selected.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild),
        );
        let (cursor, activity) = selected.finish(IsolatedNetworkTurnOutcome::complete(true));
        assert!(activity);
        assert_eq!(cursor.unit(), IsolatedNetworkLowerUnit::StageOutput);

        let selected = select_isolated_network_turn(false, true, cursor);
        assert_eq!(selected.unit(), IsolatedNetworkTurnUnit::TransmitEgress);
        let (cursor_after_tx, activity) =
            selected.finish(IsolatedNetworkTurnOutcome::complete(true));
        assert!(activity);
        assert_eq!(cursor_after_tx, cursor);

        let selected = select_isolated_network_turn(true, true, cursor_after_tx);
        assert_eq!(selected.unit(), IsolatedNetworkTurnUnit::DeferredDiagnostic);
        let (cursor_after_diagnostic, activity) =
            selected.finish(IsolatedNetworkTurnOutcome::complete(true));
        assert!(activity);
        assert_eq!(cursor_after_diagnostic, cursor);

        let must_observe = IsolatedNetworkLowerCursor {
            unit: IsolatedNetworkLowerUnit::Ingress,
        };
        let selected = select_isolated_network_turn(false, false, must_observe);
        let (must_observe, _) = selected.finish(IsolatedNetworkTurnOutcome::child_signaled(true));
        assert_eq!(must_observe.unit(), IsolatedNetworkLowerUnit::ObserveChild);

        let selected = select_isolated_network_turn(true, true, must_observe);
        assert_eq!(selected.unit(), IsolatedNetworkTurnUnit::DeferredDiagnostic);
        let (after_diagnostic, _) = selected.finish(IsolatedNetworkTurnOutcome::complete(true));
        let selected = select_isolated_network_turn(false, true, after_diagnostic);
        assert_eq!(selected.unit(), IsolatedNetworkTurnUnit::TransmitEgress);
        let (after_tx, _) = selected.finish(IsolatedNetworkTurnOutcome::complete(true));
        assert_eq!(after_diagnostic, must_observe);
        assert_eq!(after_tx, must_observe);
    }

    #[test]
    fn selection_exposes_the_successor_before_work_and_signal_overrides_it() {
        let cursor = IsolatedNetworkLowerCursor {
            unit: IsolatedNetworkLowerUnit::Ingress,
        };
        let selection = select_isolated_network_turn(false, false, cursor);

        assert_eq!(
            selection.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Ingress)
        );
        assert_eq!(
            selection.successor().unit(),
            IsolatedNetworkLowerUnit::ServiceTick,
            "ordinary successor is known before the selected unit executes"
        );
        let (successor, activity) =
            selection.finish(IsolatedNetworkTurnOutcome::child_signal_attempt(true));
        assert!(activity);
        assert_eq!(successor.unit(), IsolatedNetworkLowerUnit::ObserveChild);
    }

    #[test]
    fn retained_output_preempts_lower_work_without_a_separate_ack_turn() {
        let cursor = IsolatedNetworkLowerCursor {
            unit: IsolatedNetworkLowerUnit::Ingress,
        };

        let diagnostic = select_isolated_network_turn(true, true, cursor);
        assert_eq!(
            diagnostic.unit(),
            IsolatedNetworkTurnUnit::DeferredDiagnostic
        );
        assert_eq!(diagnostic.successor(), cursor);

        let transmit = select_isolated_network_turn(false, true, cursor);
        assert_eq!(transmit.unit(), IsolatedNetworkTurnUnit::TransmitEgress);
        assert_eq!(transmit.successor(), cursor);
        let (after_backpressured_tx, activity) =
            transmit.finish(IsolatedNetworkTurnOutcome::complete(false));
        assert!(!activity);
        assert_eq!(after_backpressured_tx, cursor);

        let transmit = select_isolated_network_turn(false, true, after_backpressured_tx);
        assert_eq!(transmit.unit(), IsolatedNetworkTurnUnit::TransmitEgress);
        let (after_tx, activity) = transmit.finish(IsolatedNetworkTurnOutcome::complete(true));
        assert!(activity);
        assert_eq!(after_tx, cursor);

        let observe = select_isolated_network_turn(false, false, after_tx);
        assert_eq!(
            observe.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Ingress)
        );
    }

    #[test]
    fn response_selector_is_useful_prioritized_and_never_selects_routine_diagnostics() {
        let cursor = IsolatedNetworkLowerCursor::new();
        let egress = select_isolated_response_turn(true, true, true, cursor);
        assert_eq!(egress.unit(), IsolatedNetworkTurnUnit::TransmitEgress);

        let stage = select_isolated_response_turn(false, true, false, cursor);
        assert_eq!(
            stage.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput)
        );

        let stage_cursor = IsolatedNetworkLowerCursor {
            unit: IsolatedNetworkLowerUnit::StageOutput,
        };
        let ordinary = select_isolated_network_turn(false, false, stage_cursor);
        assert_eq!(
            ordinary.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput),
            "ordinary service must retain the strict lower rotor"
        );
        let ingress = select_isolated_response_turn(false, false, true, stage_cursor);
        assert_eq!(
            ingress.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Ingress)
        );

        for selected in [egress, stage, ingress] {
            assert_ne!(selected.unit(), IsolatedNetworkTurnUnit::DeferredDiagnostic);
        }
    }

    #[test]
    fn direct_selectors_never_return_root_owned_data_plane_units() {
        let mut cursor = IsolatedNetworkLowerCursor::new();
        let expected = [
            IsolatedNetworkLowerUnit::ObserveChild,
            IsolatedNetworkLowerUnit::StageOutput,
            IsolatedNetworkLowerUnit::Disconnect,
            IsolatedNetworkLowerUnit::ServiceTick,
            IsolatedNetworkLowerUnit::ObserveChild,
        ];
        for expected_unit in expected {
            let selected = select_isolated_direct_network_turn(cursor);
            assert_eq!(
                selected.unit(),
                IsolatedNetworkTurnUnit::Lower(expected_unit)
            );
            let (next, activity) = selected.finish(IsolatedNetworkTurnOutcome::complete(false));
            assert!(!activity);
            cursor = next;
        }

        let stage = select_isolated_direct_response_turn(true, true, cursor);
        assert_eq!(
            stage.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput)
        );
        let observe = select_isolated_direct_response_turn(false, true, cursor);
        assert!(matches!(
            observe.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild)
                | IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ServiceTick)
        ));
    }
}
