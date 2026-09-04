// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Bound isolated console-network work to one unit per outer Network visit.
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

/// Passive readiness for the existing copied-device units, not device authority.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct IsolatedCopiedNetworkReadyWork {
    pub(crate) deferred_diagnostic: bool,
    pub(crate) pending_egress: bool,
    pub(crate) stage_output: bool,
    pub(crate) disconnect: bool,
    pub(crate) child_publication_or_ack: bool,
    pub(crate) ingress: bool,
}

impl IsolatedCopiedNetworkReadyWork {
    fn select(
        self,
        lower_cursor: IsolatedNetworkLowerCursor,
    ) -> Option<IsolatedNetworkTurnSelection> {
        let unit = if self.deferred_diagnostic {
            IsolatedNetworkTurnUnit::DeferredDiagnostic
        } else if self.pending_egress {
            // The single retained frame must leave before another child
            // publication can expose egress and overwrite it.
            IsolatedNetworkTurnUnit::TransmitEgress
        } else if self.stage_output {
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput)
        } else if self.disconnect {
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Disconnect)
        } else if self.child_publication_or_ack {
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild)
        } else if self.ingress {
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Ingress)
        } else {
            return None;
        };
        Some(IsolatedNetworkTurnSelection {
            unit,
            successor: lower_cursor,
        })
    }
}

/// Prefer already available copied CYW43 work within the same one-unit budget.
///
/// One-slot control/ingress admission and retained egress still impose their
/// existing backpressure. Idle service keeps observation, ingress, and timer
/// probes; only locally disproven output/disconnect probes are skipped. A
/// successful child signal retains the existing ObserveChild successor rule.
pub(crate) fn select_isolated_copied_network_turn_for_contract(
    exact_cyw43_contract: bool,
    ready: IsolatedCopiedNetworkReadyWork,
    lower_cursor: IsolatedNetworkLowerCursor,
) -> IsolatedNetworkTurnSelection {
    if exact_cyw43_contract {
        ready
            .select(lower_cursor)
            .unwrap_or_else(|| select_isolated_response_turn(false, false, true, lower_cursor))
    } else {
        select_isolated_network_turn(
            ready.deferred_diagnostic,
            ready.pending_egress,
            lower_cursor,
        )
    }
}

/// Apply the same copied CYW43 readiness order during a bounded response turn.
pub(crate) fn select_isolated_copied_response_turn_for_contract(
    exact_cyw43_contract: bool,
    ready: IsolatedCopiedNetworkReadyWork,
    response_progress_outstanding: bool,
    lower_cursor: IsolatedNetworkLowerCursor,
) -> IsolatedNetworkTurnSelection {
    if exact_cyw43_contract {
        if let Some(selection) = ready.select(lower_cursor) {
            return selection;
        }
    }
    select_isolated_response_turn(
        ready.pending_egress,
        ready.stage_output,
        response_progress_outstanding,
        lower_cursor,
    )
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

/// Select one useful direct-GENET control unit without changing QEMU's strict
/// direct-VirtIO rotor.
///
/// Direct GENET has an exact root-side output/disconnect readiness oracle and
/// child-owned RX/TX. Exact control work preempts the two blind responsibilities;
/// otherwise observation and the timer wake alternate one unit at a time.
pub(crate) fn select_isolated_direct_genet_network_turn(
    stage_output_ready: bool,
    disconnect_ready: bool,
    child_publication_pending: bool,
    lower_cursor: IsolatedNetworkLowerCursor,
) -> IsolatedNetworkTurnSelection {
    if stage_output_ready {
        return IsolatedNetworkTurnSelection {
            unit: IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput),
            successor: lower_cursor,
        };
    }
    if disconnect_ready {
        return IsolatedNetworkTurnSelection {
            unit: IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Disconnect),
            successor: lower_cursor,
        };
    }
    if child_publication_pending {
        return IsolatedNetworkTurnSelection {
            unit: IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild),
            successor: lower_cursor,
        };
    }

    // Direct data-plane RX/TX wakes the child without root mediation. The only
    // blind root responsibilities are therefore notification observation and
    // the timer wake. Alternate those exact units while selecting locally
    // provable control work above them. This removes empty Stage/Disconnect
    // rotor visits without weakening either one-slot control predicate.
    if lower_cursor.unit() == IsolatedNetworkLowerUnit::ServiceTick {
        IsolatedNetworkTurnSelection {
            unit: IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ServiceTick),
            successor: IsolatedNetworkLowerCursor::new(),
        }
    } else {
        IsolatedNetworkTurnSelection {
            unit: IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild),
            successor: IsolatedNetworkLowerCursor::for_unit(IsolatedNetworkLowerUnit::ServiceTick),
        }
    }
}

/// Route exact direct-GENET profiles to their control-ready selector while
/// preserving the already-qualified direct-VirtIO selector byte-for-byte for
/// every non-GENET contract.
pub(crate) fn select_isolated_direct_network_turn_for_contract(
    exact_genet_contract: bool,
    stage_output_ready: bool,
    disconnect_ready: bool,
    child_publication_pending: bool,
    lower_cursor: IsolatedNetworkLowerCursor,
) -> IsolatedNetworkTurnSelection {
    if exact_genet_contract {
        select_isolated_direct_genet_network_turn(
            stage_output_ready,
            disconnect_ready,
            child_publication_pending,
            lower_cursor,
        )
    } else {
        select_isolated_direct_network_turn(lower_cursor)
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

/// Prioritize an exact direct-GENET child publication without changing the
/// qualified direct-VirtIO response selector.
///
/// A sequence-last child level is useful work, while the notification that
/// exposed it is only a scheduling hint. Exact output staging retains priority;
/// otherwise the durable publication preempts a blind timer wake and preserves
/// the interrupted lower-unit debt.
pub(crate) fn select_isolated_direct_response_turn_for_contract(
    exact_genet_contract: bool,
    child_publication_pending: bool,
    stage_output_ready: bool,
    response_progress_outstanding: bool,
    lower_cursor: IsolatedNetworkLowerCursor,
) -> IsolatedNetworkTurnSelection {
    if exact_genet_contract && child_publication_pending && !stage_output_ready {
        IsolatedNetworkTurnSelection {
            unit: IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild),
            successor: lower_cursor,
        }
    } else {
        select_isolated_direct_response_turn(
            stage_output_ready,
            response_progress_outstanding,
            lower_cursor,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_cyw43_ready_order_preserves_retained_egress_and_control_backpressure() {
        let all_ready = IsolatedCopiedNetworkReadyWork {
            deferred_diagnostic: true,
            pending_egress: true,
            stage_output: true,
            disconnect: true,
            child_publication_or_ack: true,
            ingress: true,
        };
        let after_diagnostic = IsolatedCopiedNetworkReadyWork {
            deferred_diagnostic: false,
            ..all_ready
        };
        let after_egress = IsolatedCopiedNetworkReadyWork {
            pending_egress: false,
            ..after_diagnostic
        };
        let control_occupied = IsolatedCopiedNetworkReadyWork {
            stage_output: false,
            ..after_egress
        };
        let no_disconnect = IsolatedCopiedNetworkReadyWork {
            disconnect: false,
            ..control_occupied
        };
        let no_publication = IsolatedCopiedNetworkReadyWork {
            child_publication_or_ack: false,
            ..no_disconnect
        };
        let cases = [
            (all_ready, IsolatedNetworkTurnUnit::DeferredDiagnostic),
            (after_diagnostic, IsolatedNetworkTurnUnit::TransmitEgress),
            (
                after_egress,
                IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput),
            ),
            (
                control_occupied,
                IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Disconnect),
            ),
            (
                no_disconnect,
                IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild),
            ),
            (
                no_publication,
                IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Ingress),
            ),
        ];
        for lower_unit in [
            IsolatedNetworkLowerUnit::ObserveChild,
            IsolatedNetworkLowerUnit::StageOutput,
            IsolatedNetworkLowerUnit::Disconnect,
            IsolatedNetworkLowerUnit::Ingress,
            IsolatedNetworkLowerUnit::ServiceTick,
        ] {
            let cursor = IsolatedNetworkLowerCursor::for_unit(lower_unit);
            for (ready, expected) in cases {
                let ordinary =
                    select_isolated_copied_network_turn_for_contract(true, ready, cursor);
                assert_eq!(ordinary.unit(), expected);
                assert_eq!(ordinary.successor(), cursor);
                let (next, activity) = ordinary.finish(IsolatedNetworkTurnOutcome::complete(false));
                assert_eq!(
                    next, cursor,
                    "backpressure must preserve the interrupted lower position"
                );
                assert!(!activity);
                for response_outstanding in [false, true] {
                    let response = select_isolated_copied_response_turn_for_contract(
                        true,
                        ready,
                        response_outstanding,
                        cursor,
                    );
                    assert_eq!(
                        response, ordinary,
                        "exact copied work has the same authority in either lane"
                    );
                }
            }
        }
    }

    #[test]
    fn copied_cyw43_empty_turns_retain_ingress_and_timer_service() {
        let mut cursor = IsolatedNetworkLowerCursor::new();
        for expected in [
            IsolatedNetworkLowerUnit::ObserveChild,
            IsolatedNetworkLowerUnit::Ingress,
            IsolatedNetworkLowerUnit::ServiceTick,
            IsolatedNetworkLowerUnit::ObserveChild,
        ] {
            let selection = select_isolated_copied_network_turn_for_contract(
                true,
                IsolatedCopiedNetworkReadyWork::default(),
                cursor,
            );
            assert_eq!(selection.unit(), IsolatedNetworkTurnUnit::Lower(expected));
            assert_eq!(
                selection,
                select_isolated_copied_response_turn_for_contract(
                    true,
                    IsolatedCopiedNetworkReadyWork::default(),
                    true,
                    cursor
                ),
            );
            (cursor, _) = selection.finish(IsolatedNetworkTurnOutcome::complete(false));
        }
    }

    #[test]
    fn copied_cyw43_ready_ingress_keeps_signal_observation_and_one_slot_admission() {
        let cursor = IsolatedNetworkLowerCursor::for_unit(IsolatedNetworkLowerUnit::ServiceTick);
        let ready = IsolatedCopiedNetworkReadyWork {
            ingress: true,
            ..IsolatedCopiedNetworkReadyWork::default()
        };
        let selection = select_isolated_copied_network_turn_for_contract(true, ready, cursor);
        assert_eq!(
            selection.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Ingress)
        );
        let (next, activity) =
            selection.finish(IsolatedNetworkTurnOutcome::child_signal_attempt(true));
        assert!(activity);
        assert_eq!(next.unit(), IsolatedNetworkLowerUnit::ObserveChild);
        let awaiting_child = select_isolated_copied_network_turn_for_contract(
            true,
            IsolatedCopiedNetworkReadyWork::default(),
            next,
        );
        assert_eq!(
            awaiting_child.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild)
        );
    }

    #[test]
    fn non_cyw43_copied_contract_retains_every_original_selection() {
        for lower_unit in [
            IsolatedNetworkLowerUnit::ObserveChild,
            IsolatedNetworkLowerUnit::StageOutput,
            IsolatedNetworkLowerUnit::Disconnect,
            IsolatedNetworkLowerUnit::Ingress,
            IsolatedNetworkLowerUnit::ServiceTick,
        ] {
            let cursor = IsolatedNetworkLowerCursor::for_unit(lower_unit);
            for mask in 0u8..64 {
                let ready = IsolatedCopiedNetworkReadyWork {
                    deferred_diagnostic: mask & 1 != 0,
                    pending_egress: mask & 2 != 0,
                    stage_output: mask & 4 != 0,
                    disconnect: mask & 8 != 0,
                    child_publication_or_ack: mask & 16 != 0,
                    ingress: mask & 32 != 0,
                };
                assert_eq!(
                    select_isolated_copied_network_turn_for_contract(false, ready, cursor),
                    select_isolated_network_turn(
                        ready.deferred_diagnostic,
                        ready.pending_egress,
                        cursor
                    )
                );
                for response_outstanding in [false, true] {
                    assert_eq!(
                        select_isolated_copied_response_turn_for_contract(
                            false,
                            ready,
                            response_outstanding,
                            cursor
                        ),
                        select_isolated_response_turn(
                            ready.pending_egress,
                            ready.stage_output,
                            response_outstanding,
                            cursor
                        )
                    );
                }
            }
        }
    }

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
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput),
            "direct response output must preempt blind work"
        );
        let observe = select_isolated_direct_response_turn(false, true, cursor);
        assert!(matches!(
            observe.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild)
                | IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ServiceTick)
        ));
    }

    #[test]
    fn direct_genet_selector_prioritizes_exact_control_work() {
        let cursor = IsolatedNetworkLowerCursor::new();
        let observe = select_isolated_direct_genet_network_turn(false, false, false, cursor);
        assert_eq!(
            observe.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild)
        );
        let tick =
            select_isolated_direct_genet_network_turn(false, false, false, observe.successor());
        assert_eq!(
            tick.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ServiceTick)
        );

        let stage = select_isolated_direct_genet_network_turn(true, true, true, cursor);
        assert_eq!(
            stage.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput),
            "retained response output must preempt publication, disconnect, and blind work"
        );
        assert_eq!(
            stage.successor(),
            cursor,
            "exact control work cannot consume the blind lower-unit debt",
        );
        let disconnect = select_isolated_direct_genet_network_turn(false, true, true, cursor);
        assert_eq!(
            disconnect.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Disconnect),
            "an exact drained disconnect must preempt publication and blind work"
        );
        assert_eq!(
            disconnect.successor(),
            cursor,
            "exact disconnect work cannot consume the blind lower-unit debt",
        );
    }

    #[test]
    fn direct_genet_pending_publication_preempts_blind_service_tick() {
        let service_tick =
            IsolatedNetworkLowerCursor::for_unit(IsolatedNetworkLowerUnit::ServiceTick);

        let no_publication =
            select_isolated_direct_genet_network_turn(false, false, false, service_tick);
        assert_eq!(
            no_publication.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ServiceTick),
            "the existing blind timer responsibility remains when no child level is ready",
        );

        let publication =
            select_isolated_direct_genet_network_turn(false, false, true, service_tick);
        assert_eq!(
            publication.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild),
            "a durable child level must preempt the blind timer wake",
        );
        assert_eq!(
            publication.successor(),
            service_tick,
            "publication priority must preserve the interrupted timer debt",
        );
    }

    #[test]
    fn direct_genet_pending_publication_preempts_blind_response_tick() {
        let service_tick =
            IsolatedNetworkLowerCursor::for_unit(IsolatedNetworkLowerUnit::ServiceTick);

        let no_publication = select_isolated_direct_response_turn_for_contract(
            true,
            false,
            false,
            true,
            service_tick,
        );
        assert_eq!(
            no_publication.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ServiceTick),
        );

        let publication = select_isolated_direct_response_turn_for_contract(
            true,
            true,
            false,
            true,
            service_tick,
        );
        assert_eq!(
            publication.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild),
        );
        assert_eq!(publication.successor(), service_tick);

        let staged_output =
            select_isolated_direct_response_turn_for_contract(true, true, true, true, service_tick);
        assert_eq!(
            staged_output.unit(),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput),
            "exact staged output retains priority over a simultaneous child level",
        );
    }

    #[test]
    fn non_genet_contract_routes_every_state_to_unchanged_direct_selector() {
        for cursor in [
            IsolatedNetworkLowerCursor::new(),
            IsolatedNetworkLowerCursor::for_unit(IsolatedNetworkLowerUnit::ObserveChild),
            IsolatedNetworkLowerCursor::for_unit(IsolatedNetworkLowerUnit::StageOutput),
            IsolatedNetworkLowerCursor::for_unit(IsolatedNetworkLowerUnit::Disconnect),
            IsolatedNetworkLowerCursor::for_unit(IsolatedNetworkLowerUnit::Ingress),
            IsolatedNetworkLowerCursor::for_unit(IsolatedNetworkLowerUnit::ServiceTick),
        ] {
            for child_publication_pending in [false, true] {
                for (stage_output_ready, disconnect_ready) in
                    [(false, false), (false, true), (true, false), (true, true)]
                {
                    assert_eq!(
                        select_isolated_direct_network_turn_for_contract(
                            false,
                            stage_output_ready,
                            disconnect_ready,
                            child_publication_pending,
                            cursor,
                        ),
                        select_isolated_direct_network_turn(cursor),
                        "a non-GENET contract must retain the direct-VirtIO selector for cursor={cursor:?} stage={stage_output_ready} disconnect={disconnect_ready} publication={child_publication_pending}",
                    );
                }

                for (stage_output_ready, response_progress_outstanding) in
                    [(false, false), (false, true), (true, false), (true, true)]
                {
                    assert_eq!(
                        select_isolated_direct_response_turn_for_contract(
                            false,
                            child_publication_pending,
                            stage_output_ready,
                            response_progress_outstanding,
                            cursor,
                        ),
                        select_isolated_direct_response_turn(
                            stage_output_ready,
                            response_progress_outstanding,
                            cursor,
                        ),
                        "a non-GENET contract must retain the direct-VirtIO response selector for cursor={cursor:?} stage={stage_output_ready} response={response_progress_outstanding} publication={child_publication_pending}",
                    );
                }
            }
        }
    }
}
