// Author: Lukas Bower

#![cfg(feature = "kernel")]

#[path = "dispatch_support.rs"]
mod dispatch_support;

use dispatch_support::{
    build_message, AuditCapture, DummySerial, NoopTimer, RecordingHandlers, TestDispatcher,
};
use root_task::event::BootstrapOp;
use root_task::event::{DispatchOutcome, EventPump, TicketTable};
use root_task::serial::{
    SerialPort, DEFAULT_LINE_CAPACITY, DEFAULT_RX_CAPACITY, DEFAULT_TX_CAPACITY,
};

#[test]
fn staged_message_dispatches_log_handler() {
    let op_word = BootstrapOp::Log.encode();
    let words = [op_word, 0xDEADBEEFDEADBEEF, 0x0011_2233_4455_6677];
    let (message, expected_payload) = build_message(&words);

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

    assert_eq!(handler.log_calls, 1, "log handler invoked once");
    assert_eq!(handler.attach_calls, 0);
    assert_eq!(handler.spawn_calls, 0);
    assert_eq!(
        handler.outcomes.as_slice(),
        &[DispatchOutcome::Handled(BootstrapOp::Log)]
    );
    assert_eq!(handler.last_payload.len(), expected_payload.len());
    assert!(handler
        .last_payload
        .iter()
        .zip(expected_payload.iter())
        .all(|(a, b)| a == b));
}
