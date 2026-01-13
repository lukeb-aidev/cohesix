// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task dispatch_invalid_opcode.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

#[path = "dispatch_support.rs"]
mod dispatch_support;

use dispatch_support::{
    build_message, AuditCapture, DummySerial, NoopTimer, RecordingHandlers, TestDispatcher,
};
use root_task::event::{DispatchOutcome, EventPump, TicketTable};
use root_task::serial::{
    SerialPort, DEFAULT_LINE_CAPACITY, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY,
};

#[test]
fn bad_opcode_is_rejected() {
    let words = [0xFF, 0xABCD_EF01_2345_6789];
    let (message, _copy) = build_message(&words);

    let serial: SerialPort<
        _,
        { DEFAULT_RX_CAPACITY },
        { DEFAULT_TX_CAPACITY },
        { DEFAULT_LINE_CAPACITY },
    > = SerialPort::new(DummySerial);
    let timer = NoopTimer::default();
    let dispatcher = TestDispatcher::new(message);
    let mut tickets: TicketTable<1> = TicketTable::new();
    tickets
        .register(cohesix_ticket::Role::Queen, "bootstrap")
        .expect("ticket table has capacity");
    let mut audit = AuditCapture::new();
    let mut handler = RecordingHandlers::new();

    let mut pump = EventPump::new(serial, timer, dispatcher, tickets, &mut audit);
    pump = pump.with_bootstrap_handler(&mut handler);
    pump.poll();

    assert_eq!(handler.log_calls, 0);
    assert_eq!(handler.attach_calls, 0);
    assert_eq!(handler.spawn_calls, 0);
    assert_eq!(
        handler.outcomes.as_slice(),
        &[DispatchOutcome::BadCommand(0xFF)],
    );
}
