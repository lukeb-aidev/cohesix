// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define truthful bounded self-test state for the isolated console-network service.
// Author: Lukas Bower

use super::NetSelfTestResult;

pub(crate) const ISOLATED_SELF_TEST_WINDOW_MS: u64 = 15_000;

pub(crate) fn finish_poll_with_self_test(
    activity: bool,
    service_self_test: impl FnOnce() -> bool,
) -> bool {
    let self_test_progress = service_self_test();
    activity || self_test_progress
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct IsolatedSelfTestObservation {
    pub(crate) now_ms: u64,
    pub(crate) direct_data_plane: bool,
    pub(crate) tx_complete: u64,
    pub(crate) rx_packets: u64,
    pub(crate) tcp_rx_bytes: u64,
    pub(crate) connection_bytes_read: u64,
    pub(crate) connection_bytes_written: u64,
    pub(crate) response_drains: u64,
    pub(crate) authenticated_connection: Option<u64>,
    pub(crate) listener_ready: bool,
    pub(crate) output_drained: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct IsolatedSelfTestState {
    enabled: bool,
    running: bool,
    run_generation: u64,
    started_ms: u64,
    start_tx_complete: u64,
    start_rx_packets: u64,
    start_tcp_rx_bytes: u64,
    start_connection_bytes_read: u64,
    start_connection_bytes_written: u64,
    start_response_drains: u64,
    bound_connection: Option<u64>,
    bound_connection_retired: bool,
    authenticated_observed: bool,
    command_observed: bool,
    response_written_observed: bool,
    response_drained_observed: bool,
    tx_completion_observed: bool,
    rx_progress_observed: bool,
    listener_ready_observed: bool,
    last_result: Option<NetSelfTestResult>,
}

impl IsolatedSelfTestState {
    pub(crate) const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            running: false,
            run_generation: 0,
            started_ms: 0,
            start_tx_complete: 0,
            start_rx_packets: 0,
            start_tcp_rx_bytes: 0,
            start_connection_bytes_read: 0,
            start_connection_bytes_written: 0,
            start_response_drains: 0,
            bound_connection: None,
            bound_connection_retired: false,
            authenticated_observed: false,
            command_observed: false,
            response_written_observed: false,
            response_drained_observed: false,
            tx_completion_observed: false,
            rx_progress_observed: false,
            listener_ready_observed: false,
            last_result: None,
        }
    }

    pub(crate) fn start(
        &mut self,
        now_ms: u64,
        tx_complete: u64,
        rx_packets: u64,
        tcp_rx_bytes: u64,
        connection_bytes_read: u64,
        connection_bytes_written: u64,
        response_drains: u64,
        authenticated_connection: Option<u64>,
    ) -> bool {
        if !self.enabled {
            return false;
        }
        self.run_generation = self.run_generation.wrapping_add(1).max(1);
        self.running = true;
        self.started_ms = now_ms;
        self.start_tx_complete = tx_complete;
        self.start_rx_packets = rx_packets;
        self.start_tcp_rx_bytes = tcp_rx_bytes;
        // Connected resets both per-connection counters before authentication.
        // A serial-admitted run with no live peer must therefore start the
        // first later authenticated connection at its fresh zero epoch rather
        // than inherit counters retained from a closed peer.
        self.start_connection_bytes_read =
            authenticated_connection.map_or(0, |_| connection_bytes_read);
        self.start_connection_bytes_written =
            authenticated_connection.map_or(0, |_| connection_bytes_written);
        self.start_response_drains = response_drains;
        self.bound_connection = authenticated_connection;
        self.bound_connection_retired = false;
        self.authenticated_observed = authenticated_connection.is_some();
        self.command_observed = false;
        self.response_written_observed = false;
        self.response_drained_observed = false;
        self.tx_completion_observed = false;
        self.rx_progress_observed = false;
        self.listener_ready_observed = false;
        self.last_result = None;
        true
    }

    pub(crate) fn observe(
        &mut self,
        observation: IsolatedSelfTestObservation,
    ) -> Option<NetSelfTestResult> {
        if !self.running {
            return None;
        }
        // A serial-admitted run may begin before the peer connects. Bind the
        // first later authenticated identity once. Its per-connection counters
        // belong to the fresh zero epoch established by Connected.
        if self.bound_connection.is_none() {
            if let Some(connection_id) = observation.authenticated_connection {
                self.bound_connection = Some(connection_id);
                self.start_connection_bytes_read = 0;
                self.start_connection_bytes_written = 0;
                self.authenticated_observed = true;
            }
        }
        if self.authenticated_observed
            && observation.authenticated_connection != self.bound_connection
        {
            // Once the bound peer disappears, neither a replacement connection
            // nor later unrelated NIC progress may complete its proof.
            self.bound_connection_retired = true;
        }
        let connection_matches = !self.bound_connection_retired
            && self.bound_connection.is_some()
            && observation.authenticated_connection == self.bound_connection;
        if connection_matches {
            self.authenticated_observed = true;
            self.command_observed |=
                observation.connection_bytes_read > self.start_connection_bytes_read;
            self.response_written_observed |=
                observation.connection_bytes_written > self.start_connection_bytes_written;
            self.response_drained_observed |= observation.response_drains
                > self.start_response_drains
                && observation.output_drained;
            self.rx_progress_observed |= observation.rx_packets > self.start_rx_packets
                || observation.tcp_rx_bytes > self.start_tcp_rx_bytes;
            self.listener_ready_observed |= observation.listener_ready;
            self.tx_completion_observed |= self.response_drained_observed
                && (observation.direct_data_plane
                    || observation.tx_complete > self.start_tx_complete);
        }
        let tx_ok = self.tx_completion_observed;
        let rx_ok = self.rx_progress_observed;
        let tcp_ok = self.authenticated_observed
            && self.command_observed
            && self.response_written_observed
            && self.response_drained_observed;
        let console_ok = self.listener_ready_observed && tcp_ok;
        let peer_assisted_ok = tx_ok && rx_ok && tcp_ok && console_ok;
        let result = NetSelfTestResult {
            tx_ok,
            udp_echo_ok: false,
            tcp_ok,
            console_ok,
            peer_assisted_ok,
        };
        let deadline_reached =
            observation.now_ms.saturating_sub(self.started_ms) >= ISOLATED_SELF_TEST_WINDOW_MS;
        if !peer_assisted_ok && !deadline_reached {
            return None;
        }
        self.running = false;
        self.last_result = Some(result);
        Some(result)
    }

    pub(crate) const fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) const fn running(&self) -> bool {
        self.running
    }

    pub(crate) const fn run_generation(&self) -> u64 {
        self.run_generation
    }

    pub(crate) const fn last_result(&self) -> Option<NetSelfTestResult> {
        self.last_result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(now_ms: u64) -> IsolatedSelfTestObservation {
        IsolatedSelfTestObservation {
            now_ms,
            tx_complete: 8,
            rx_packets: 4,
            tcp_rx_bytes: 32,
            connection_bytes_read: 32,
            connection_bytes_written: 64,
            response_drains: 3,
            authenticated_connection: Some(7),
            listener_ready: true,
            output_drained: true,
            ..IsolatedSelfTestObservation::default()
        }
    }

    #[test]
    fn physical_peer_assisted_run_requires_response_drain_and_nic_completion() {
        let mut state = IsolatedSelfTestState::new(true);
        assert!(state.start(100, 8, 4, 32, 16, 32, 2, Some(7)));
        assert_eq!(state.run_generation(), 1);
        let mut stale_rx = observation(101);
        stale_rx.tx_complete = 9;
        assert_eq!(
            state.observe(stale_rx),
            None,
            "historical RX counters cannot satisfy a new peer-assisted run",
        );

        let mut complete = observation(102);
        complete.tx_complete = 9;
        complete.rx_packets = 5;
        complete.tcp_rx_bytes = 33;
        let result = state
            .observe(complete)
            .expect("physical proof must conclude");
        assert_eq!(result.verdict(), "peer-assisted-pass");
        assert!(result.tx_ok);
        assert!(result.tcp_ok);
        assert!(result.console_ok);
        assert!(result.peer_assisted_ok);
        assert!(!result.udp_echo_ok);
        assert!(!state.running());
    }

    #[test]
    fn ordinary_activity_cannot_short_circuit_decisive_self_test_observation() {
        let mut state = IsolatedSelfTestState::new(true);
        assert!(state.start(100, 8, 4, 32, 16, 32, 2, Some(7)));
        let mut complete = observation(101);
        complete.tx_complete = 9;
        complete.rx_packets = 5;
        complete.tcp_rx_bytes = 33;
        let mut terminal = None;

        let poll_active = finish_poll_with_self_test(true, || {
            terminal = state.observe(complete);
            terminal.is_some()
        });

        assert!(poll_active);
        let result = terminal.expect("decisive active poll must service self-test");
        assert_eq!(result.verdict(), "peer-assisted-pass");
        assert_eq!(state.last_result(), Some(result));
        assert!(!state.running());
    }

    #[test]
    fn direct_data_plane_uses_exact_child_response_drain_as_tx_boundary() {
        let mut state = IsolatedSelfTestState::new(true);
        assert!(state.start(1, 0, 3, 31, 0, 0, 0, Some(7)));
        let mut complete = observation(2);
        complete.direct_data_plane = true;
        complete.tx_complete = 0;
        let result = state
            .observe(complete)
            .expect("direct child drain must conclude");
        assert!(result.peer_assisted_ok);
    }

    #[test]
    fn run_fails_terminally_at_the_existing_fifteen_second_bound() {
        let mut state = IsolatedSelfTestState::new(true);
        assert!(state.start(50, 8, 4, 32, 64, 64, 3, None));
        let result = state
            .observe(IsolatedSelfTestObservation {
                now_ms: 50 + ISOLATED_SELF_TEST_WINDOW_MS,
                ..IsolatedSelfTestObservation::default()
            })
            .expect("deadline must conclude");
        assert_eq!(result.verdict(), "fail");
        assert_eq!(state.last_result(), Some(result));
    }

    #[test]
    fn late_authenticated_connection_binds_once_and_replacement_cannot_combine_proof() {
        let mut state = IsolatedSelfTestState::new(true);
        assert!(state.start(0, 8, 4, 32, 64, 64, 3, None));

        let first_connection = observation(1);
        assert_eq!(state.observe(first_connection), None);
        assert_eq!(state.bound_connection, Some(7));

        let mut replacement = observation(ISOLATED_SELF_TEST_WINDOW_MS);
        replacement.authenticated_connection = Some(8);
        replacement.tx_complete = 9;
        replacement.rx_packets = 5;
        replacement.tcp_rx_bytes = 33;
        replacement.connection_bytes_written = 65;
        replacement.response_drains = 4;
        let result = state
            .observe(replacement)
            .expect("deadline must reject cross-connection proof");
        assert_eq!(result.verdict(), "fail");
        assert!(!result.tx_ok);
        assert!(!result.tcp_ok);
        assert!(!result.console_ok);
        assert!(!result.peer_assisted_ok);
    }

    #[test]
    fn late_connection_uses_fresh_epoch_instead_of_closed_peer_counters() {
        let mut state = IsolatedSelfTestState::new(true);
        assert!(state.start(100, 8, 4, 32, 50_000, 50_000, 560, None));

        let mut complete = observation(101);
        complete.tx_complete = 9;
        complete.rx_packets = 5;
        complete.tcp_rx_bytes = 68;
        complete.connection_bytes_read = 36;
        complete.connection_bytes_written = 9_454;
        complete.response_drains = 570;
        let result = state
            .observe(complete)
            .expect("fresh authenticated peer proof must conclude");

        assert_eq!(state.bound_connection, Some(7));
        assert_eq!(result.verdict(), "peer-assisted-pass");
        assert!(result.tx_ok);
        assert!(result.tcp_ok);
        assert!(result.console_ok);
        assert!(result.peer_assisted_ok);
        assert!(!result.udp_echo_ok);
    }

    #[test]
    fn exact_connection_facts_remain_latched_after_disconnect() {
        let mut state = IsolatedSelfTestState::new(true);
        assert!(state.start(100, 8, 4, 32, 50_000, 50_000, 560, None));

        let mut command = observation(101);
        command.rx_packets = 5;
        command.tcp_rx_bytes = 68;
        command.connection_bytes_read = 36;
        command.connection_bytes_written = 0;
        command.response_drains = 560;
        command.output_drained = false;
        assert_eq!(state.observe(command), None);

        let mut response = command;
        response.now_ms = 102;
        response.connection_bytes_written = 9_454;
        assert_eq!(state.observe(response), None);

        let mut drain = response;
        drain.now_ms = 103;
        drain.response_drains = 570;
        drain.output_drained = true;
        assert_eq!(state.observe(drain), None);

        assert_eq!(
            state.observe(IsolatedSelfTestObservation {
                now_ms: 104,
                tx_complete: 9,
                authenticated_connection: None,
                ..IsolatedSelfTestObservation::default()
            }),
            None,
        );

        let result = state
            .observe(IsolatedSelfTestObservation {
                now_ms: 100 + ISOLATED_SELF_TEST_WINDOW_MS,
                tx_complete: 10,
                authenticated_connection: None,
                ..IsolatedSelfTestObservation::default()
            })
            .expect("deadline must preserve exact completed connection facts");
        assert_eq!(result.verdict(), "fail");
        assert!(
            !result.tx_ok,
            "post-disconnect NIC traffic cannot complete proof"
        );
        assert!(result.tcp_ok);
        assert!(result.console_ok);
        assert!(!result.peer_assisted_ok);
    }

    #[test]
    fn disabled_state_refuses_without_advancing_generation() {
        let mut state = IsolatedSelfTestState::new(false);
        assert!(!state.start(1, 0, 0, 0, 0, 0, 0, None));
        assert!(!state.enabled());
        assert_eq!(state.run_generation(), 0);
    }
}
