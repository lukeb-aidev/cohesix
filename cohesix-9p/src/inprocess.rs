// CLASSIFICATION: COMMUNITY
// Filename: inprocess.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-12-31

#![cfg(feature = "inprocess")]

use alloc::{collections::VecDeque, sync::Arc, vec::Vec};
use core::cell::RefCell;

/// In-process byte stream for `no_std` targets.
#[derive(Clone)]
pub struct InProcessStream {
    rx: Arc<RefCell<VecDeque<u8>>>,
    tx: Arc<RefCell<VecDeque<u8>>>,
}

impl InProcessStream {
    /// Create paired streams for bidirectional communication.
    pub fn pair() -> (Self, Self) {
        let a_rx = Arc::new(RefCell::new(VecDeque::new()));
        let b_rx = Arc::new(RefCell::new(VecDeque::new()));
        (
            Self { rx: a_rx.clone(), tx: b_rx.clone() },
            Self { rx: b_rx, tx: a_rx },
        )
    }

    /// Send bytes to the remote end.
    pub fn send(&self, data: &[u8]) {
        self.tx.borrow_mut().extend(data);
    }

    /// Receive bytes from the remote end, returning number of bytes read.
    pub fn recv(&self, out: &mut Vec<u8>) -> usize {
        let mut buf = self.rx.borrow_mut();
        let n = buf.len();
        out.extend(buf.drain(..));
        n
    }
}
