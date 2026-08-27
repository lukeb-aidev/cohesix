// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Prove bounded console-side ownership of the Pi GENET direct-link rings.
// Author: Lukas Bower

#![cfg(feature = "direct-genet")]

#[path = "../src/direct_genet.rs"]
mod direct_genet;

use console_network_runtime::abi::{
    DirectGenetControlPage, DirectGenetCursorRole, DirectGenetDirection, DirectGenetError,
    DirectGenetLayout, DirectGenetSlotPage, DIRECT_GENET_CURSOR_OFFSET,
    DIRECT_GENET_CURSOR_STATE_BYTES, DIRECT_GENET_LAYOUT_BYTES, DIRECT_GENET_LAYOUT_FLAGS,
    DIRECT_GENET_LAYOUT_MAGIC, DIRECT_GENET_LAYOUT_VERSION,
    DIRECT_GENET_PEER_WAKE_NOTIFICATION_SLOT, DIRECT_GENET_POISON_INVALID_CURSOR,
    DIRECT_GENET_RX_PRODUCER_STATE_OFFSET, DIRECT_GENET_RX_SLOT_COUNT, DIRECT_GENET_TX_SLOT_COUNT,
    SHARED_PAGE_BYTES,
};
use direct_genet::DirectGenetLink;

const GENERATION: u64 = 0x4745_4e45_5400_0001;

#[repr(C, align(4096))]
struct SharedPage([u8; SHARED_PAGE_BYTES]);

struct Fixture {
    control: Box<SharedPage>,
    rx: [Box<SharedPage>; DIRECT_GENET_RX_SLOT_COUNT],
    tx: [Box<SharedPage>; DIRECT_GENET_TX_SLOT_COUNT],
}

impl Fixture {
    fn new() -> Self {
        let mut control = Box::new(SharedPage([0; SHARED_PAGE_BYTES]));
        DirectGenetControlPage::initialize_into(&mut control.0, GENERATION)
            .expect("fixed direct-link control page initializes");
        let mut rx: [Box<SharedPage>; DIRECT_GENET_RX_SLOT_COUNT] =
            core::array::from_fn(|_| Box::new(SharedPage([0; SHARED_PAGE_BYTES])));
        let mut tx: [Box<SharedPage>; DIRECT_GENET_TX_SLOT_COUNT] =
            core::array::from_fn(|_| Box::new(SharedPage([0; SHARED_PAGE_BYTES])));
        for page in rx.iter_mut().chain(tx.iter_mut()) {
            DirectGenetSlotPage::initialize_into(&mut page.0)
                .expect("fixed direct-link slot initializes");
        }
        Self { control, rx, tx }
    }

    fn layout(&mut self) -> DirectGenetLayout {
        DirectGenetLayout {
            magic: DIRECT_GENET_LAYOUT_MAGIC,
            version: DIRECT_GENET_LAYOUT_VERSION,
            layout_bytes: DIRECT_GENET_LAYOUT_BYTES as u16,
            flags: DIRECT_GENET_LAYOUT_FLAGS,
            shared_page_bytes: SHARED_PAGE_BYTES as u16,
            rx_slot_count: DIRECT_GENET_RX_SLOT_COUNT as u8,
            tx_slot_count: DIRECT_GENET_TX_SLOT_COUNT as u8,
            generation: GENERATION,
            peer_wake_notification_slot: DIRECT_GENET_PEER_WAKE_NOTIFICATION_SLOT,
            reserved0: 0,
            control_vaddr: self.control.0.as_mut_ptr() as u64,
            rx_vaddrs: core::array::from_fn(|index| self.rx[index].0.as_mut_ptr() as u64),
            tx_vaddrs: core::array::from_fn(|index| self.tx[index].0.as_mut_ptr() as u64),
            seal: 0,
        }
        .sealed()
    }

    fn publish_rx(&mut self, frame: &[u8]) {
        let initial =
            DirectGenetControlPage::snapshot(&self.control.0, GENERATION, DirectGenetDirection::Rx)
                .expect("RX cursors are live");
        let (_, slot_index) = initial.next_producer().expect("RX ring has credit");
        DirectGenetSlotPage::publish_next_into(
            &mut self.rx[slot_index].0,
            DirectGenetDirection::Rx,
            GENERATION,
            initial.producer_cursor,
            frame,
        )
        .expect("driver publishes one bounded RX frame");
        DirectGenetControlPage::commit_producer(&mut self.control.0, initial)
            .expect("driver commits its RX producer cursor");
    }
}

#[test]
fn console_consumes_rx_and_publishes_tx_without_root_packet_pages() {
    let mut fixture = Fixture::new();
    let layout = fixture.layout();
    layout.validate().expect("fixture layout is exact");
    let mut link = DirectGenetLink::new(layout).expect("validated layout constructs the endpoint");

    fixture.publish_rx(b"received-by-genet");
    link.poll().expect("live direct rings poll");
    let mut frame = [0u8; 1536];
    let received = link
        .receive(&mut frame)
        .expect("RX consumption remains valid")
        .expect("one committed RX frame is available");
    assert_eq!(&frame[..received], b"received-by-genet");
    assert_eq!(link.take_peer_wake(), None, "non-full RX needs no rearm");

    assert!(link.can_transmit().expect("TX cursor is live"));
    link.transmit(b"sent-by-console")
        .expect("one bounded TX frame commits");
    assert_eq!(
        link.take_peer_wake(),
        Some(DIRECT_GENET_PEER_WAKE_NOTIFICATION_SLOT),
        "empty-to-nonempty TX causally wakes GENET",
    );
    let tx =
        DirectGenetControlPage::snapshot(&fixture.control.0, GENERATION, DirectGenetDirection::Tx)
            .expect("TX cursor remains live");
    let (sequence, slot_index) = tx.next_consumer().expect("GENET observes TX work");
    let record = DirectGenetSlotPage::decode_next(
        &fixture.tx[slot_index].0,
        DirectGenetDirection::Tx,
        GENERATION,
        sequence - 1,
    )
    .expect("TX slot is exact and sequence-last");
    assert_eq!(record.frame(), b"sent-by-console");
}

#[test]
fn full_rx_ring_consumption_rearms_the_genet_producer() {
    let mut fixture = Fixture::new();
    let layout = fixture.layout();
    let mut link = DirectGenetLink::new(layout).expect("validated layout constructs the endpoint");
    for value in 0..DIRECT_GENET_RX_SLOT_COUNT {
        fixture.publish_rx(&[value as u8 + 1]);
    }

    link.poll().expect("full live RX ring polls");
    let mut frame = [0u8; 1536];
    assert_eq!(
        link.receive(&mut frame).expect("RX consume succeeds"),
        Some(1),
    );
    assert_eq!(frame[0], 1);
    assert_eq!(
        link.take_peer_wake(),
        Some(DIRECT_GENET_PEER_WAKE_NOTIFICATION_SLOT),
        "full-to-not-full RX transition rearms GENET",
    );
}

#[test]
fn cursor_replacement_is_retained_but_stable_poison_fails_closed() {
    let mut fixture = Fixture::new();
    let layout = fixture.layout();
    let mut link = DirectGenetLink::new(layout).expect("validated layout constructs the endpoint");
    let commit_offset = DIRECT_GENET_RX_PRODUCER_STATE_OFFSET + DIRECT_GENET_CURSOR_STATE_BYTES - 8;
    let saved_commit = fixture.control.0[commit_offset..commit_offset + 8].to_vec();
    fixture.control.0[commit_offset..commit_offset + 8].fill(0);
    link.poll()
        .expect("an in-progress sequence-last replacement is transient");
    assert!(
        link.work_pending(),
        "the exact observation remains retained"
    );
    fixture.control.0[commit_offset..commit_offset + 8].copy_from_slice(&saved_commit);

    let rx =
        DirectGenetControlPage::snapshot(&fixture.control.0, GENERATION, DirectGenetDirection::Rx)
            .expect("restored RX state is live");
    DirectGenetControlPage::poison_owner(
        &mut fixture.control.0,
        GENERATION,
        DirectGenetCursorRole::RxProducer,
        rx.producer_state_sequence,
        DIRECT_GENET_POISON_INVALID_CURSOR,
    )
    .expect("driver publishes one stable poison receipt");
    assert!(matches!(
        link.poll(),
        Err(DirectGenetError::Poisoned(poison))
            if poison.role == DirectGenetCursorRole::RxProducer
                && poison.reason == DIRECT_GENET_POISON_INVALID_CURSOR
    ));
}

#[test]
fn stable_invalid_cursor_fails_closed() {
    let mut fixture = Fixture::new();
    let layout = fixture.layout();
    let mut link = DirectGenetLink::new(layout).expect("validated layout constructs the endpoint");
    let cursor_offset = DIRECT_GENET_RX_PRODUCER_STATE_OFFSET + DIRECT_GENET_CURSOR_OFFSET;
    fixture.control.0[cursor_offset..cursor_offset + 8].copy_from_slice(&1u64.to_le_bytes());

    assert_eq!(link.poll(), Err(DirectGenetError::InvalidCursor));
}

#[test]
fn console_fault_poisons_owned_lines_after_peer_progress_and_wakes_once() {
    let mut fixture = Fixture::new();
    let layout = fixture.layout();
    let mut link = DirectGenetLink::new(layout).expect("validated layout constructs the endpoint");

    fixture.publish_rx(b"peer-progress-before-console-fault");
    let cursor_offset = DIRECT_GENET_RX_PRODUCER_STATE_OFFSET + DIRECT_GENET_CURSOR_OFFSET;
    fixture.control.0[cursor_offset..cursor_offset + 8].copy_from_slice(&2u64.to_le_bytes());
    let error = link
        .poll()
        .expect_err("stable reciprocal cursor drift is terminal");
    assert_eq!(error, DirectGenetError::InvalidCursor);
    link.fail_closed(error);

    assert!(matches!(
        DirectGenetControlPage::cursor_state(
            &fixture.control.0,
            GENERATION,
            DirectGenetCursorRole::RxConsumer,
        ),
        Err(DirectGenetError::Poisoned(poison))
            if poison.role == DirectGenetCursorRole::RxConsumer
                && poison.reason == DIRECT_GENET_POISON_INVALID_CURSOR
    ));
    assert!(matches!(
        DirectGenetControlPage::cursor_state(
            &fixture.control.0,
            GENERATION,
            DirectGenetCursorRole::TxProducer,
        ),
        Err(DirectGenetError::Poisoned(poison))
            if poison.role == DirectGenetCursorRole::TxProducer
                && poison.reason == DIRECT_GENET_POISON_INVALID_CURSOR
    ));
    assert_eq!(
        link.take_peer_wake(),
        Some(DIRECT_GENET_PEER_WAKE_NOTIFICATION_SLOT),
        "terminal fencing always signals the paired GENET child",
    );
    assert_eq!(
        link.take_peer_wake(),
        None,
        "the terminal wake is coalesced"
    );
    assert!(
        !link.work_pending(),
        "terminal state cannot resume data work"
    );
}

#[test]
fn malformed_layout_is_rejected_before_shared_memory_access() {
    let mut fixture = Fixture::new();
    let mut layout = fixture.layout();
    layout.control_vaddr = 1;
    layout = layout.sealed();

    assert!(matches!(
        DirectGenetLink::new(layout),
        Err(DirectGenetError::InvalidLayout)
    ));
}
