// Author: Lukas Bower
// Purpose: Verify exactly-once MCS command Reply recovery across driver faults.
// Copyright 2026 Lukas Bower

use pi4_driver_abi::DRIVER_RUNTIME_MCS_FAULTED_CALL_RESULT;
use root_task::hal::driver_task::{
    DriverFaultedCallError, DriverFaultedCallPhase, DriverFaultedCallRecovery,
    DriverFaultedCallReply,
};

fn finish_containment(recovery: &mut DriverFaultedCallRecovery) {
    recovery.suspend_faulted_tcb().expect("suspend");
    recovery
        .verify_associations_clear()
        .expect("associations clear");
    recovery.revoke_generation().expect("revoke generation");
    assert_eq!(recovery.phase(), DriverFaultedCallPhase::Revoked);
    assert!(!recovery.associations_live());
    assert!(!recovery.admission_open());
}

#[test]
fn fault_during_call_returns_one_typed_failure_then_revokes() {
    let mut recovery = DriverFaultedCallRecovery::new(4);
    recovery.admit_call(17).expect("admit call");
    recovery.publish_fault(17).expect("publish fault");
    assert!(!recovery.admission_open());
    assert_eq!(
        recovery.reply_typed_failure().expect("failure reply"),
        DriverFaultedCallReply::TypedFailure {
            sequence: 17,
            result: DRIVER_RUNTIME_MCS_FAULTED_CALL_RESULT,
        }
    );
    assert_eq!(
        recovery.reply_typed_failure(),
        Err(DriverFaultedCallError::InvalidPhase)
    );
    assert_eq!(recovery.reply_counts(), (0, 1));
    finish_containment(&mut recovery);
    recovery.reconstruct(5).expect("new generation");
    assert_eq!(recovery.phase(), DriverFaultedCallPhase::Ready);
    assert_eq!(recovery.generation(), 5);
    assert!(recovery.admission_open());
}

#[test]
fn faults_before_or_after_call_do_not_fabricate_command_reply() {
    let mut before = DriverFaultedCallRecovery::new(1);
    before.publish_fault(0).expect("fault before call");
    assert_eq!(
        before.reply_typed_failure().expect("no caller"),
        DriverFaultedCallReply::NoBlockedCaller
    );
    assert_eq!(before.reply_counts(), (0, 0));
    finish_containment(&mut before);

    let mut after = DriverFaultedCallRecovery::new(2);
    after.admit_call(9).expect("admit");
    after.reply_normal(9).expect("normal reply");
    after.publish_fault(0).expect("fault after call");
    assert_eq!(
        after.reply_typed_failure().expect("no blocked caller"),
        DriverFaultedCallReply::NoBlockedCaller
    );
    assert_eq!(after.reply_counts(), (1, 0));
    finish_containment(&mut after);
}

#[test]
fn caller_cancellation_clears_association_before_later_fault() {
    let mut recovery = DriverFaultedCallRecovery::new(8);
    recovery.admit_call(31).expect("admit");
    recovery.cancel_call(31).expect("cancel");
    assert!(!recovery.associations_live());
    recovery.publish_fault(0).expect("later independent fault");
    assert_eq!(
        recovery.reply_typed_failure().expect("no caller"),
        DriverFaultedCallReply::NoBlockedCaller
    );
    finish_containment(&mut recovery);
}

#[test]
fn normal_and_fault_replies_are_mutually_exclusive() {
    let mut normal = DriverFaultedCallRecovery::new(1);
    normal.admit_call(1).expect("admit");
    normal.reply_normal(1).expect("normal reply");
    assert_eq!(
        normal.reply_normal(1),
        Err(DriverFaultedCallError::InvalidPhase)
    );
    assert_eq!(normal.reply_counts(), (1, 0));

    let mut faulted = DriverFaultedCallRecovery::new(1);
    faulted.admit_call(2).expect("admit");
    faulted.publish_fault(2).expect("fault");
    assert_eq!(
        faulted.reply_normal(2),
        Err(DriverFaultedCallError::InvalidPhase)
    );
    faulted.reply_typed_failure().expect("typed failure");
    assert_eq!(faulted.reply_counts(), (0, 1));
}
