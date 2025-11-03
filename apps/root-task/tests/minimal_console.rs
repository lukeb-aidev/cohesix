// Author: Lukas Bower
use root_task::ipc::{ep_is_valid, seL4_CapNull, seL4_MessageInfo, try_send, FAILED_LOOKUP_ERROR};

#[test]
fn ep_is_invalid_when_null() {
    assert!(!ep_is_valid(seL4_CapNull));
}

#[test]
fn try_send_on_null_endpoint_returns_failed_lookup() {
    let info = seL4_MessageInfo::new(0, 0, 0, 0);
    let err = try_send(seL4_CapNull, info).expect_err("expected guard failure");
    assert_eq!(err, FAILED_LOOKUP_ERROR);
}
