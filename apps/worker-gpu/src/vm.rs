// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Expose GPU VM receipt-loop helpers without in-VM GPU execution.
// Author: Lukas Bower

//! GPU VM receipt-loop helpers.

pub use worker_heart::worker_loop::{
    AttachRequest, BoundedRun, EndpointCap, EndpointInvocation, InvocationKind, LifecycleEvent,
    PressureLevel, ReceiptKind, ReceiptRecord, ReceiptRequest, StepOutcome, WorkerBadge,
    WorkerEndpointAction, WorkerEvent, WorkerIdentity, WorkerLoop, WorkerLoopError,
    WorkerNotification, WorkerProgress, WorkerRole, WorkerState,
};

/// GPU receipt-only VM worker loop.
pub type GpuReceiptLoop = WorkerLoop;

/// Build a GPU receipt-only worker loop for an existing identity.
pub fn gpu_receipt_loop(identity: WorkerIdentity) -> Result<GpuReceiptLoop, WorkerLoopError> {
    WorkerLoop::gpu_receipts(identity)
}

/// Build a GPU worker identity for endpoint-badge modeling.
#[must_use]
pub const fn gpu_identity(
    slot: u32,
    lease_epoch: u64,
    supervisor_generation: u64,
    cap_generation: u64,
) -> WorkerIdentity {
    WorkerIdentity::new(
        WorkerRole::Gpu,
        slot,
        lease_epoch,
        supervisor_generation,
        cap_generation,
    )
}

/// Build a GPU control receipt event.
#[must_use]
pub const fn gpu_control_receipt_event(now_ms: u64, receipt_id: u64) -> WorkerEvent {
    WorkerEvent::Receipt(ReceiptRequest {
        now_ms,
        receipt_id,
        kind: ReceiptKind::GpuControl,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attach(identity: WorkerIdentity, now_ms: u64, ttl_ms: u64) -> WorkerEvent {
        WorkerEvent::Attach(AttachRequest {
            endpoint: EndpointCap::new(0x55, identity.badge()).expect("valid endpoint"),
            now_ms,
            lease_ttl_ms: ttl_ms,
        })
    }

    #[test]
    fn gpu_vm_loop_emits_control_receipt_only() {
        let identity = gpu_identity(3, 4, 5, 6);
        let mut loop_state = gpu_receipt_loop(identity).expect("gpu loop");
        let attached = loop_state.step(attach(identity, 10, 100)).expect("attach");
        assert_eq!(attached.progress, WorkerProgress::Attached);

        let receipt = loop_state
            .step(gpu_control_receipt_event(11, 77))
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
                receipt_id: 77,
                kind: ReceiptKind::GpuControl,
                ..
            })
        ));

        let err = loop_state
            .step(WorkerEvent::Receipt(ReceiptRequest {
                now_ms: 12,
                receipt_id: 78,
                kind: ReceiptKind::LoraControl,
            }))
            .expect_err("wrong receipt kind");
        assert_eq!(err, WorkerLoopError::ReceiptMismatch);
    }

    #[test]
    fn gpu_vm_loop_records_pressure_without_running_hardware() {
        let identity = gpu_identity(9, 1, 1, 1);
        let mut loop_state = gpu_receipt_loop(identity).expect("gpu loop");
        loop_state.step(attach(identity, 0, 50)).expect("attach");
        let pressure = loop_state
            .step(WorkerEvent::Notify(WorkerNotification::Pressure {
                now_ms: 1,
                level: PressureLevel::High,
            }))
            .expect("pressure");
        assert_eq!(loop_state.pressure(), PressureLevel::High);
        assert!(matches!(
            pressure.output.expect("pressure output").kind,
            InvocationKind::Lifecycle(worker_heart::worker_loop::LifecycleRecord {
                event: LifecycleEvent::Pressure(PressureLevel::High),
                ..
            })
        ));
    }
}
