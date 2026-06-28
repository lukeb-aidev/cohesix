// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Expose LoRA VM receipt-loop helpers without in-VM training execution.
// Author: Lukas Bower

//! LoRA VM receipt-loop helpers.

pub use worker_heart::worker_loop::{
    AttachRequest, BoundedRun, EndpointCap, EndpointInvocation, InvocationKind, LifecycleEvent,
    PressureLevel, ReceiptKind, ReceiptRecord, ReceiptRequest, StepOutcome, WorkerBadge,
    WorkerEndpointAction, WorkerEvent, WorkerIdentity, WorkerLoop, WorkerLoopError,
    WorkerNotification, WorkerProgress, WorkerRole, WorkerState,
};

/// LoRA receipt-only VM worker loop.
pub type LoraReceiptLoop = WorkerLoop;

/// Build a LoRA receipt-only worker loop for an existing identity.
pub fn lora_receipt_loop(identity: WorkerIdentity) -> Result<LoraReceiptLoop, WorkerLoopError> {
    WorkerLoop::lora_receipts(identity)
}

/// Build a LoRA worker identity for endpoint-badge modeling.
#[must_use]
pub const fn lora_identity(instance: u32, lease_epoch: u16, cap_generation: u16) -> WorkerIdentity {
    WorkerIdentity::new(WorkerRole::Lora, instance, lease_epoch, cap_generation)
}

/// Build a LoRA control receipt event.
#[must_use]
pub const fn lora_control_receipt_event(now_ms: u64, receipt_id: u64) -> WorkerEvent {
    WorkerEvent::Receipt(ReceiptRequest {
        now_ms,
        receipt_id,
        kind: ReceiptKind::LoraControl,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attach(identity: WorkerIdentity, now_ms: u64, ttl_ms: u64) -> WorkerEvent {
        WorkerEvent::Attach(AttachRequest {
            endpoint: EndpointCap::new(0x66, identity.badge()).expect("valid endpoint"),
            now_ms,
            lease_ttl_ms: ttl_ms,
        })
    }

    #[test]
    fn lora_vm_loop_emits_receipts_without_training_execution() {
        let identity = lora_identity(2, 3, 4);
        let mut loop_state = lora_receipt_loop(identity).expect("lora loop");
        loop_state.step(attach(identity, 0, 100)).expect("attach");

        let receipt = loop_state
            .step(lora_control_receipt_event(1, 42))
            .expect("receipt")
            .output
            .expect("receipt output");
        assert_eq!(
            receipt.badge,
            identity.badge_for(WorkerEndpointAction::Receipt)
        );
        assert!(matches!(
            receipt.kind,
            InvocationKind::Receipt(ReceiptRecord {
                receipt_id: 42,
                kind: ReceiptKind::LoraControl,
                ..
            })
        ));
    }

    #[test]
    fn lora_shutdown_is_terminal_for_late_receipts() {
        let identity = lora_identity(8, 1, 1);
        let mut loop_state = lora_receipt_loop(identity).expect("lora loop");
        loop_state.step(attach(identity, 0, 100)).expect("attach");
        let shutdown = loop_state
            .step(WorkerEvent::Notify(WorkerNotification::Shutdown {
                now_ms: 1,
            }))
            .expect("shutdown");
        assert_eq!(shutdown.progress, WorkerProgress::Terminal);
        assert_eq!(loop_state.state(), WorkerState::Shutdown);
        let err = loop_state
            .step(lora_control_receipt_event(2, 43))
            .expect_err("terminal state");
        assert_eq!(err, WorkerLoopError::TerminalState);
    }
}
