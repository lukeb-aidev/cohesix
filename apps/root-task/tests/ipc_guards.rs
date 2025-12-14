// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::sel4;
use root_task::sel4::IpcError;
use sel4_sys::seL4_MessageInfo;

#[test]
fn guarded_ipc_reports_ep_not_ready() {
    sel4::clear_ep();
    assert!(
        !sel4::ep_ready(),
        "endpoint should report not ready after clear"
    );

    let info = seL4_MessageInfo::new(0, 0, 0, 0);

    assert_eq!(sel4::send_guarded(info), Err(IpcError::EpNotReady));

    let mut mr0 = 0;
    assert_eq!(
        sel4::call_guarded(info, Some(&mut mr0), None, None, None),
        Err(IpcError::EpNotReady)
    );

    let mut badge = 0;
    assert_eq!(
        sel4::replyrecv_guarded(info, Some(&mut badge)),
        Err(IpcError::EpNotReady)
    );
}

#[test]
fn ep_ready_toggles_with_set_and_clear() {
    sel4::clear_ep();
    assert!(!sel4::ep_ready());
    assert!(!sel4::ep_validated());

    sel4::set_ep(0x1234);
    assert!(sel4::ep_ready());
    assert_eq!(sel4::root_endpoint(), 0x1234);
    assert!(!sel4::ep_validated());

    sel4::set_ep_validated(true);
    assert!(sel4::ep_validated());

    sel4::clear_ep();
    assert!(!sel4::ep_ready());
    assert!(!sel4::ep_validated());
}
