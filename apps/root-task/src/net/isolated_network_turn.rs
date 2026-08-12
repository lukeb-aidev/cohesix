// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Bound one isolated QEMU console-network unit per outer Network visit.
// Author: Lukas Bower

/// One ordinary isolated console-network unit below TX/diagnostic preemption.
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

    const fn can_signal_child(self) -> bool {
        !matches!(self, Self::ObserveChild)
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

    fn advance_after(self, completed: IsolatedNetworkLowerUnit, child_signal: bool) -> Self {
        debug_assert_eq!(self.unit, completed);
        debug_assert!(!child_signal || completed.can_signal_child());
        let unit = if child_signal && completed.can_signal_child() {
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
            IsolatedNetworkTurnUnit::Lower(unit) if outcome.child_signal() => {
                debug_assert!(unit.can_signal_child());
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
            let outcome = if expected_unit.can_signal_child() {
                IsolatedNetworkTurnOutcome::child_signal_attempt(false)
            } else {
                IsolatedNetworkTurnOutcome::complete(false)
            };
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
}
