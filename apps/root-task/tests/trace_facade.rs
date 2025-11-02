// Author: Lukas Bower
#![cfg(feature = "bootstrap-trace")]

use root_task::trace::{trace_snapshot_json, TraceLevel};

#[test]
fn trace_macro_records_events() {
    root_task::trace!(TraceLevel::Info, "boot", "trace facade ready");
    root_task::trace!(TraceLevel::Debug, "boot", Some("worker-1"), "worker online");
    let snapshot = trace_snapshot_json();
    assert!(snapshot
        .iter()
        .any(|line| line.contains("trace facade ready")));
    assert!(snapshot.iter().any(|line| line.contains("worker online")));
}
