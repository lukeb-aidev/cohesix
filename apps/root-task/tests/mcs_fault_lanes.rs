// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify acyclic MCS fault routing and single-owner Reply-lane transitions.
// Author: Lukas Bower

use root_task::critical_tcb::{
    mcs_extra_refills, validate_critical_temporal_graph, FaultClass, FaultRegistration,
    FaultReplyLane, FaultReplyLaneError, FaultReplyLaneState, GenerationIdentity,
};
use root_task::generated;

fn registration(terminal: bool) -> FaultRegistration {
    FaultRegistration {
        task_index: 1,
        identity: GenerationIdentity {
            slot: 1,
            lease_epoch: 1,
            supervisor_generation: 1,
            cap_generation: 1,
        },
        standard_badge: 0x1001,
        timeout_badge: 0x2001,
        tcb_cap: 0x3001,
        terminal,
    }
}

fn source_section<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source.find(start).expect("source section start");
    let end = source[start..]
        .find(end)
        .map(|offset| start + offset)
        .expect("source section end");
    &source[start..end]
}

#[test]
fn target_runtime_blocks_on_one_shared_endpoint_and_one_reply() {
    let source = include_str!("../src/hal/critical_tcb.rs");

    assert!(
        source.contains("sel4::recv_with_reply(CHILD_INBOX_SLOT, &mut badge, CHILD_REPLY_SLOT)")
    );
    assert!(source.contains("let (registration, fault_class) = resolve_target_fault(badge)?;"));
    assert!(source.contains("sel4::wait(CHILD_DRIVER_RELEASE_SLOT, &mut observed_badge)"));
    assert!(source.contains("sel4::signal_unchecked(CHILD_DRIVER_RELEASE_SIGNAL_SLOT)"));
    assert!(!source.contains("nb_recv_with_reply"));
    assert!(!source.contains("CHILD_TIMEOUT_INBOX_SLOT"));
    assert!(!source.contains("CHILD_TIMEOUT_REPLY_SLOT"));
}

#[test]
fn root_fault_cold_activation_primes_receive_before_any_fault_association() {
    let source = include_str!("../src/hal/critical_tcb.rs");
    assert!(source.contains("AtomicUsize::new(RootFaultCriticalTurn::PrimeReceive as usize)"));

    let cursor_start = source
        .find("enum RootFaultCriticalTurn {")
        .expect("root-fault cursor");
    let cursor_end = source[cursor_start..]
        .find("fn commit_root_fault_turn(")
        .map(|offset| cursor_start + offset)
        .expect("root-fault cursor commit after enum");
    let cursor = &source[cursor_start..cursor_end];
    let prime_variant = cursor
        .find("PrimeReceive = 0")
        .expect("cold-prime cursor discriminant");
    let receive_variant = cursor
        .find("Receive = 1")
        .expect("recurring receive cursor discriminant");
    assert!(prime_variant < receive_variant);

    let entry_start = source
        .find("extern \"C\" fn root_fault_entry")
        .expect("root-fault entrypoint");
    let entry_end = source[entry_start..]
        .find("extern \"C\" fn root_emergency_entry")
        .map(|offset| entry_start + offset)
        .expect("root-emergency entrypoint after root-fault");
    let entry = &source[entry_start..entry_end];
    let prime_start = entry
        .find("RootFaultCriticalTurn::PrimeReceive =>")
        .expect("cold-prime turn");
    let receive_start = entry
        .find("RootFaultCriticalTurn::Receive =>")
        .expect("recurring receive turn");
    assert!(prime_start < receive_start);

    let prime_turn = &entry[prime_start..receive_start];
    let commit_receive = prime_turn
        .find("commit_root_fault_turn(RootFaultCriticalTurn::Receive);")
        .expect("receive successor commit");
    let prime_yield = prime_turn
        .find("sel4::yield_now();")
        .expect("cold-prime replenishment boundary");
    assert!(commit_receive < prime_yield);
    assert_eq!(prime_turn.matches("sel4::yield_now();").count(), 1);
    for forbidden in [
        "recv_with_reply",
        "CHILD_INBOX_SLOT",
        "CHILD_REPLY_SLOT",
        "PendingTargetFault",
        "publish_pending_target_fault",
        "take_pending_target_fault",
        "fault_label",
        "fault_badge",
        "root_fault_tcb_control_cap",
        "suspend_tcb",
        "signal_unchecked",
    ] {
        assert!(
            !prime_turn.contains(forbidden),
            "cold-prime turn must not create or retain fault state: {forbidden}"
        );
    }
}

#[test]
fn terminal_critical_fault_commits_one_resumable_action_per_refill() {
    let source = include_str!("../src/hal/critical_tcb.rs");
    let handler_start = source
        .find("fn handle_target_fault(")
        .expect("target fault handler");
    let entry_start = source[handler_start..]
        .find("extern \"C\" fn root_fault_entry")
        .map(|offset| handler_start + offset)
        .expect("root-fault entrypoint");
    let handler = &source[handler_start..entry_start];
    let critical_start = handler
        .find("TemporalTaskKind::RootControl")
        .expect("critical terminal branch");
    let service_start = handler[critical_start..]
        .find("TemporalTaskKind::Service")
        .map(|offset| critical_start + offset)
        .expect("service branch after critical branch");
    let critical_branch = &handler[critical_start..service_start];

    assert!(critical_branch.contains("FaultReplyDisposition::CriticalTerminal"));
    assert!(!critical_branch.contains("suspend_tcb"));
    assert!(!critical_branch.contains("signal_unchecked"));
    assert!(!handler.contains("yield_now"));

    let entry_end = source[entry_start..]
        .find("extern \"C\" fn root_emergency_entry")
        .map(|offset| entry_start + offset)
        .expect("root-emergency entrypoint after root-fault");
    let entry = &source[entry_start..entry_end];
    let pending_start = source
        .find("struct PendingTargetFault {")
        .expect("value-only pending fault record");
    let pending_end = source[pending_start..]
        .find("fn publish_pending_target_fault(")
        .map(|offset| pending_start + offset)
        .expect("pending fault publication after value-only record");
    let pending_record = &source[pending_start..pending_end];
    let cursor_start = source[pending_start..]
        .find("enum RootFaultCriticalTurn {")
        .map(|offset| pending_start + offset)
        .expect("root-fault cursor after pending record");
    let cursor_end = source[cursor_start..]
        .find("fn handle_target_fault(")
        .map(|offset| cursor_start + offset)
        .expect("target fault handler after cursor");
    let pending_and_cursor = &source[pending_start..cursor_end];
    assert!(pending_record.contains("fault_label: seL4_Word"));
    assert!(pending_record.contains("fault_badge: seL4_Word"));
    assert!(!pending_record.contains("seL4_MessageInfo"));
    assert!(!pending_record.contains("seL4_CPtr"));
    assert!(!pending_record.contains("CHILD_REPLY_SLOT"));
    assert!(pending_and_cursor.contains("fn commit_root_fault_turn("));
    assert!(pending_and_cursor.contains("TARGET_ROOT_FAULT_TURN.store("));
    assert!(pending_and_cursor.contains("fn commit_root_fault_suspend("));
    assert!(pending_and_cursor.contains("TARGET_ROOT_FAULT_CRITICAL_TASK.store("));

    let prime_start = entry
        .find("RootFaultCriticalTurn::PrimeReceive =>")
        .expect("cold-prime turn");
    let receive_start = entry
        .find("RootFaultCriticalTurn::Receive =>")
        .expect("receive turn");
    let classify_start = entry
        .find("RootFaultCriticalTurn::Classify =>")
        .expect("classification turn");
    let resolve_service_start = entry
        .find("RootFaultCriticalTurn::ResolveService =>")
        .expect("service resolution turn");
    let resume_service_start = entry
        .find("RootFaultCriticalTurn::ResumeService =>")
        .expect("passive timeout resume turn");
    let suspend_start = entry
        .find("RootFaultCriticalTurn::SuspendCritical =>")
        .expect("suspend turn");
    let signal_start = entry
        .find("RootFaultCriticalTurn::SignalEmergency =>")
        .expect("emergency signal turn");
    assert!(
        prime_start < receive_start
            && receive_start < classify_start
            && classify_start < resume_service_start
            && resume_service_start < resolve_service_start
            && resolve_service_start < suspend_start
            && suspend_start < signal_start
    );

    let receive_turn = &entry[receive_start..classify_start];
    let receive = receive_turn
        .find("sel4::recv_with_reply(")
        .expect("blocking fault receive");
    let commit_classify = receive_turn
        .find("commit_root_fault_turn(RootFaultCriticalTurn::Classify);")
        .expect("classification successor commit");
    let publish = receive_turn
        .find("publish_pending_target_fault(PendingTargetFault {")
        .expect("published value-only fault record");
    let receive_yield = receive_turn
        .rfind("sel4::yield_now();")
        .expect("receive-to-classification refill boundary");
    assert!(commit_classify < receive);
    assert!(receive < publish && publish < receive_yield);
    assert!(!receive_turn.contains("handle_target_fault("));
    assert!(!receive_turn.contains("root_fault_tcb_control_cap("));

    let classify_turn = &entry[classify_start..resume_service_start];
    let classify = classify_turn
        .find("let disposition = match handle_target_fault(")
        .expect("sealed-registry classification");
    let released_start = classify_turn
        .find("FaultReplyDisposition::Released =>")
        .expect("released disposition");
    let retained_start = classify_turn
        .find("FaultReplyDisposition::RetainedByDriver =>")
        .expect("retained-driver disposition");
    let critical_start = classify_turn
        .find("FaultReplyDisposition::CriticalTerminal { task_index } =>")
        .expect("critical terminal disposition");
    assert!(released_start < retained_start && retained_start < critical_start);

    let released_branch = &classify_turn[released_start..retained_start];
    assert!(released_branch.contains("commit_root_fault_turn(RootFaultCriticalTurn::Receive);"));
    assert_eq!(released_branch.matches("sel4::yield_now();").count(), 1);

    let retained_branch = &classify_turn[retained_start..critical_start];
    let release_wait = retained_branch
        .find("sel4::wait(CHILD_DRIVER_RELEASE_SLOT, &mut observed_badge)")
        .expect("driver release wait");
    let release_validation = retained_branch
        .find("DRIVER_FAULT_REPLY_BUSY.load(Ordering::Acquire)")
        .expect("driver release validation");
    let retained_yield = retained_branch
        .rfind("sel4::yield_now();")
        .expect("driver-release-to-receive refill boundary");
    assert!(release_wait < release_validation && release_validation < retained_yield);
    let retained_commit = retained_branch
        .find("commit_root_fault_turn(RootFaultCriticalTurn::Receive);")
        .expect("driver release successor commit");
    assert!(release_validation < retained_commit && retained_commit < retained_yield);
    assert_eq!(retained_branch.matches("sel4::yield_now();").count(), 1);

    let critical_branch = &classify_turn[critical_start..];
    let commit_suspend = classify_turn
        .find("commit_root_fault_suspend(task_index);")
        .expect("critical suspend successor commit");
    let classify_yield = classify_turn
        .rfind("sel4::yield_now();")
        .expect("classification-to-suspend refill boundary");
    assert!(classify < commit_suspend && commit_suspend < classify_yield);
    assert_eq!(critical_branch.matches("sel4::yield_now();").count(), 1);
    assert_eq!(
        classify_turn.matches("sel4::yield_now();").count(),
        4,
        "one service route plus the three legacy dispositions"
    );
    assert!(!classify_turn.contains("sel4::recv_with_reply("));
    assert!(!classify_turn.contains("sel4::suspend_tcb("));
    assert!(!classify_turn.contains("sel4::signal_unchecked(CHILD_EMERGENCY_SIGNAL_SLOT)"));

    let suspend_turn = &entry[suspend_start..signal_start];
    let commit_signal = suspend_turn
        .find("commit_root_fault_turn(RootFaultCriticalTurn::SignalEmergency);")
        .expect("emergency successor commit");
    let resolve_cap = suspend_turn
        .find("root_fault_tcb_control_cap(task_index)")
        .expect("exact child-local TCB cap lookup");
    let suspend = suspend_turn
        .find("sel4::suspend_tcb(fault_handler_tcb_cap)")
        .expect("terminal critical suspension");
    let suspend_yield = suspend_turn
        .find("sel4::yield_now();")
        .expect("suspend-to-signal refill boundary");
    assert!(commit_signal < resolve_cap && resolve_cap < suspend && suspend < suspend_yield);
    assert!(!suspend_turn.contains("RootFaultCriticalTurn::Receive"));

    let signal_turn = &entry[signal_start..];
    let commit_receive = signal_turn
        .find("commit_root_fault_turn(RootFaultCriticalTurn::Receive);")
        .expect("receive successor commit");
    let signal = signal_turn
        .find("sel4::signal_unchecked(CHILD_EMERGENCY_SIGNAL_SLOT)")
        .expect("terminal emergency signal");
    let signal_yield = signal_turn
        .find("sel4::yield_now();")
        .expect("signal-to-receive refill boundary");
    assert!(commit_receive < signal && signal < signal_yield);
    assert_eq!(entry.matches("sel4::recv_with_reply(").count(), 1);
}

#[test]
fn generated_console_and_ninedoor_faults_select_exact_service_units() {
    let ninedoor = generated::temporal_tasks()
        .iter()
        .find(|task| task.id == "ninedoor-service")
        .expect("generated NineDoor task");
    let console = generated::temporal_tasks()
        .iter()
        .find(|task| task.id == "console-network-service")
        .expect("generated console-network task");
    let ninedoor_config = generated::ninedoor_service_config();
    let console_config = generated::console_network_service_config();

    assert_eq!(ninedoor.kind, generated::TemporalTaskKind::Service);
    assert_eq!(ninedoor.execution, generated::TemporalExecution::Passive);
    assert_eq!(
        ninedoor.timeout_policy,
        generated::TimeoutPolicy::ReturnError
    );
    assert_eq!(console.kind, generated::TemporalTaskKind::Service);
    assert_eq!(console.execution, generated::TemporalExecution::Active);
    assert_eq!(
        console.timeout_policy,
        generated::TimeoutPolicy::NaturalPostpone
    );
    assert_ne!(
        console.timeout_policy,
        generated::TimeoutPolicy::ReplenishOnce,
        "standard console faults retain terminal no-Reply containment",
    );
    assert_eq!(ninedoor.timeout_badge, ninedoor_config.timeout_badge);
    assert_eq!(console.timeout_badge, console_config.timeout_badge);
    assert_eq!(
        root_task::critical_tcb::generated_standard_fault_badge(ninedoor.id),
        Some(ninedoor_config.fault_badge)
    );
    assert_eq!(
        root_task::critical_tcb::generated_standard_fault_badge(console.id),
        Some(console_config.fault_badge)
    );
    assert_ne!(ninedoor_config.fault_badge, console_config.fault_badge);
    assert_ne!(ninedoor_config.timeout_badge, console_config.timeout_badge);

    let source = include_str!("../src/critical_tcb.rs");
    let classifier = source_section(
        source,
        "pub(crate) fn is_generated_service_fault_badge(",
        "/// Resolve one unique generated service owner",
    );
    for selected in [
        "badge == ninedoor.fault_badge",
        "badge == ninedoor.timeout_badge",
        "badge == console.fault_badge",
        "badge == console.timeout_badge",
    ] {
        assert!(
            classifier.contains(selected),
            "missing exact selector: {selected}"
        );
    }
    assert!(!classifier.contains("try_lock"));
    assert!(!classifier.contains("resolve_target_fault"));
}

#[test]
fn service_fault_cursor_retries_without_loss_and_admits_one_action_per_refill() {
    let source = include_str!("../src/hal/critical_tcb.rs");
    let entry = source_section(
        source,
        "extern \"C\" fn root_fault_entry",
        "extern \"C\" fn root_emergency_entry",
    );
    let classify = source_section(
        entry,
        "RootFaultCriticalTurn::Classify =>",
        "RootFaultCriticalTurn::ResumeService =>",
    );
    let service_route = source_section(
        classify,
        "if is_generated_service_fault_badge(fault_badge) {",
        "} else {",
    );
    let route_commit = service_route
        .find("commit_root_fault_turn(successor);")
        .expect("service classification successor");
    let route_yield = service_route
        .find("sel4::yield_now();")
        .expect("service classification boundary");
    assert!(route_commit < route_yield);
    assert_eq!(service_route.matches("sel4::yield_now();").count(), 1);
    assert!(service_route.contains("Ok(true) => RootFaultCriticalTurn::ResumeService"));
    assert!(service_route.contains("Ok(false) => RootFaultCriticalTurn::ResolveService"));
    assert!(service_route.contains("FaultHandoffError::Contended"));
    assert!(service_route.contains("RootFaultCriticalTurn::Classify"));
    for forbidden in [
        "clear_pending_target_fault",
        "resolve_target_fault",
        "suspend_tcb",
        "recover_target_passive_service_call",
        "publish_target_service_fault",
    ] {
        assert!(
            !service_route.contains(forbidden),
            "classification cannot perform recovery or lose the fault: {forbidden}"
        );
    }

    let resume = source_section(
        entry,
        "RootFaultCriticalTurn::ResumeService =>",
        "RootFaultCriticalTurn::ResolveService =>",
    );
    let resume_commit = resume
        .find("commit_root_fault_turn(RootFaultCriticalTurn::Receive);")
        .expect("resume commits the recurring receive before Reply");
    let resume_reply = resume.find("sel4::reply_to(").expect("kernel fault Reply");
    assert!(resume_commit < resume_reply);
    assert_eq!(resume.matches("sel4::reply_to(").count(), 1);
    assert!(resume.contains("CHILD_REPLY_SLOT"));
    assert!(resume.contains("seL4_MessageInfo::new(0, 0, 0, 0)"));
    assert_eq!(resume.matches("sel4::yield_now();").count(), 1);
    for forbidden in [
        "suspend_tcb",
        "configure",
        "recover_target_passive_service_call(",
    ] {
        assert!(
            !resume.contains(forbidden),
            "resume must preserve the Call: {forbidden}"
        );
    }

    let resolve = source_section(
        entry,
        "RootFaultCriticalTurn::ResolveService =>",
        "RootFaultCriticalTurn::SuspendService =>",
    );
    let resolve_commit = resolve
        .find("commit_root_fault_turn(RootFaultCriticalTurn::SuspendService);")
        .expect("suspend successor before resolution");
    let resolve_action = resolve
        .find("match prepare_target_service_fault(")
        .expect("one registry resolution action");
    let snapshot_publish = resolve
        .find("publish_pending_target_service_fault(pending)")
        .expect("persistent service snapshot");
    let copied_clear = resolve
        .find("clear_pending_target_fault();")
        .expect("copied fault clear after snapshot");
    let retry = resolve
        .find("commit_root_fault_turn(RootFaultCriticalTurn::ResolveService)")
        .expect("resolution contention retry");
    assert!(resolve_commit < resolve_action);
    assert!(resolve_action < snapshot_publish && snapshot_publish < copied_clear);
    assert!(copied_clear < retry);
    assert_eq!(resolve.matches("prepare_target_service_fault(").count(), 1);
    assert_eq!(resolve.matches("sel4::yield_now();").count(), 1);
    assert!(!resolve.contains("suspend_tcb"));
    assert!(!resolve.contains("recover_target_passive_service_call"));
    assert!(!resolve.contains("publish_target_service_fault("));

    let suspend = source_section(
        entry,
        "RootFaultCriticalTurn::SuspendService =>",
        "RootFaultCriticalTurn::RecoverPassiveService =>",
    );
    let suspend_commit = suspend
        .find("commit_root_fault_turn(successor);")
        .expect("service successor before suspension");
    let suspend_action = suspend
        .find("sel4::suspend_tcb_bounded(pending.fault_handler_tcb_cap)")
        .expect("one quiet bounded suspend");
    let suspend_retry = suspend
        .find("commit_root_fault_turn(RootFaultCriticalTurn::SuspendService);")
        .expect("suspend retry cursor");
    assert!(suspend_commit < suspend_action && suspend_action < suspend_retry);
    assert_eq!(suspend.matches("suspend_tcb_bounded(").count(), 1);
    assert_eq!(suspend.matches("sel4::yield_now();").count(), 1);
    assert!(!suspend.contains("sel4::suspend_tcb("));
    assert!(!suspend.contains("recover_target_passive_service_call"));
    assert!(!suspend.contains("publish_target_service_fault"));

    let recover = source_section(
        entry,
        "RootFaultCriticalTurn::RecoverPassiveService =>",
        "RootFaultCriticalTurn::PublishService =>",
    );
    let recover_commit = recover
        .find("commit_root_fault_turn(RootFaultCriticalTurn::PublishService);")
        .expect("publish successor before passive recovery");
    let recover_action = recover
        .find("recover_target_passive_service_call(pending.record)")
        .expect("one passive recovery action");
    let recover_retry = recover
        .find("commit_root_fault_turn(RootFaultCriticalTurn::RecoverPassiveService);")
        .expect("passive recovery retry cursor");
    assert!(recover_commit < recover_action && recover_action < recover_retry);
    assert_eq!(
        recover
            .matches("recover_target_passive_service_call(")
            .count(),
        1
    );
    assert_eq!(recover.matches("sel4::yield_now();").count(), 1);
    assert!(!recover.contains("suspend_tcb"));
    assert!(!recover.contains("publish_target_service_fault"));

    let publish = source_section(
        entry,
        "RootFaultCriticalTurn::PublishService =>",
        "RootFaultCriticalTurn::SuspendCritical =>",
    );
    let publish_commit = publish
        .find("commit_root_fault_turn(RootFaultCriticalTurn::Receive);")
        .expect("receive successor before mailbox publication");
    let publish_action = publish
        .find("publish_target_service_fault(pending.record)")
        .expect("one durable mailbox publication");
    let service_clear = publish
        .find("clear_pending_target_service_fault();")
        .expect("snapshot clear after publication");
    let publish_retry = publish
        .find("commit_root_fault_turn(RootFaultCriticalTurn::PublishService);")
        .expect("mailbox publication retry cursor");
    assert!(publish_commit < publish_action);
    assert!(publish_action < service_clear && service_clear < publish_retry);
    assert_eq!(publish.matches("publish_target_service_fault(").count(), 1);
    assert_eq!(publish.matches("sel4::yield_now();").count(), 1);
    assert!(!publish.contains("recv_with_reply"));
    assert!(!publish.contains("reply_to"));
    assert!(!publish.contains("suspend_tcb"));
    assert_eq!(
        entry
            .matches("clear_pending_target_service_fault();")
            .count(),
        1,
        "publication retry must retain the sole scalar snapshot"
    );
}

#[test]
fn passive_service_recovery_replies_once_during_call_and_zero_between_calls() {
    let source = include_str!("../src/hal/critical_tcb.rs");
    let recovery = source_section(
        source,
        "fn recover_target_passive_service_call(",
        "fn recover_target_passive_worker_call(",
    );
    let reply_slot = recovery
        .find("let reply_slot = TARGET_SERVICE_RECOVERY_SLOTS")
        .expect("reply authority validation");
    let transition = recovery
        .find("compare_exchange(")
        .expect("single recovery state transition");
    let already_replied = recovery
        .find("Err(SERVICE_RECOVERY_REPLIED) => return Ok(())")
        .expect("idempotent completed retry");
    let sequence = recovery
        .find("let sequence = TARGET_SERVICE_CALL_SEQUENCES")
        .expect("one in-flight sequence take");
    let between_calls = recovery
        .find("if sequence == 0")
        .expect("between-Call zero-Reply branch");
    let reply = recovery
        .find("sel4::reply_to(")
        .expect("during-Call recovery Reply");
    assert!(reply_slot < transition);
    assert!(transition < already_replied && already_replied < sequence);
    assert!(sequence < between_calls && between_calls < reply);
    assert_eq!(recovery.matches("sel4::reply_to(").count(), 1);
    assert!(!recovery.contains("log::"));
    assert!(!recovery.contains("debug_uart"));

    let prepare = source_section(
        source,
        "fn prepare_target_service_fault(",
        "fn handle_target_fault(",
    );
    assert_eq!(prepare.matches("resolve_target_fault(badge)?").count(), 1);
    assert!(prepare.contains("task.execution == TemporalExecution::Passive"));
    assert!(prepare.contains("TimeoutPolicy::ReturnError | TimeoutPolicy::ResumeOnceReturnError"));
    assert!(!prepare.contains("suspend_tcb"));
    assert!(!prepare.contains("reply_to"));
    assert!(!prepare.contains("publish_target_service_fault"));
}

#[test]
fn driver_containment_clears_the_fault_association_before_reply_release() {
    let containment_source = include_str!("../src/hal/driver_task.rs");
    let containment = source_section(
        containment_source,
        "pub fn root_driver_supervisor_contain_fault(",
        "/// Classic kernels cannot consume the MCS driver-supervisor hook.",
    );
    let suspend = containment
        .find("crate::sel4::suspend_tcb(")
        .expect("driver TCB suspension");
    let association_clear = containment
        .find("DRIVER_TASK_MCS_CALL_ASSOCIATIONS_CLEAR")
        .expect("driver fault-association clear state");
    let admission_close = containment
        .find("slot.mcs_command_admission_open.store(0")
        .expect("root command admission fence");
    let endpoint_fence = containment
        .find("slot.endpoint.store(0")
        .expect("published endpoint fence");
    let producer_drain = containment
        .find("mcs_nonblocking_root_producers")
        .expect("active nonblocking root-producer drain fence");
    let command_revoke = containment
        .find("DRIVER_SUPERVISOR_DIAG_STAGE_REVOKE_COMMAND")
        .expect("retained command-origin revoke");
    let success = containment.find("Ok(())").expect("successful containment");
    assert!(admission_close < endpoint_fence);
    assert!(endpoint_fence < producer_drain);
    assert!(producer_drain < command_revoke);
    assert!(suspend < association_clear);
    assert!(association_clear < success);
    assert_eq!(containment.matches("slot.endpoint.store(0").count(), 1);

    let root_fault_source = include_str!("../src/hal/critical_tcb.rs");
    let supervisor_start = root_fault_source
        .find("extern \"C\" fn root_driver_supervisor_entry")
        .expect("driver supervisor entrypoint");
    let supervisor = &root_fault_source[supervisor_start..];
    let contain = supervisor
        .find("root_driver_supervisor_contain_fault(")
        .expect("driver containment call");
    let producer_deferred = supervisor
        .find("DriverSupervisorContainmentError::RootProducerActive")
        .expect("active producer defers containment");
    let retain_record = supervisor
        .find("deferred_record = Some(record)")
        .expect("exact fault record retained across drain turn");
    let clear_busy = supervisor
        .find(".compare_exchange(true, false")
        .expect("Reply association busy clear");
    let signal_release = supervisor
        .find("sel4::signal_unchecked(CHILD_DRIVER_RELEASE_SIGNAL_SLOT)")
        .expect("root-fault Reply release signal");
    assert!(contain < producer_deferred);
    assert!(producer_deferred < retain_record);
    assert!(retain_record < clear_busy);
    assert!(clear_busy < signal_release);

    let command_path = source_section(
        containment_source,
        "fn run_driver_task_ring_command_with_mode_and_staging_deadline(",
        "/// Execute a bounded compatibility callback on the contract's live driver TCB.",
    );
    let acquire_producer = command_path
        .find("acquire_mcs_nonblocking_root_producer(contract, slot, mode)")
        .expect("nonblocking MCS producer acquisition");
    let load_endpoint = command_path
        .find("let endpoint = slot.endpoint.load(Ordering::Acquire);")
        .expect("endpoint load after producer acquisition");
    assert!(acquire_producer < load_endpoint);
    let retry_loop = command_path
        .find("for attempt in 0..attempts {")
        .expect("bounded nonblocking retry loop");
    let retry_path = &command_path[retry_loop..];
    let admission_recheck = retry_path
        .find("mcs_nonblocking_root_producer_allows_send(slot, endpoint)")
        .expect("admission recheck inside retry loop");
    let send = retry_path
        .find("crate::sel4::send_nb_unchecked(endpoint")
        .expect("nonblocking endpoint send");
    assert!(admission_recheck < send);
}

#[test]
fn restricted_critical_ipc_uses_explicit_registers_and_bound_extra_cap_lane() {
    let critical = include_str!("../src/hal/critical_tcb.rs");
    let root_fault_start = critical
        .find("extern \"C\" fn root_fault_entry")
        .expect("root-fault entrypoint");
    let root_fault_end = critical[root_fault_start..]
        .find("extern \"C\" fn root_emergency_entry")
        .map(|offset| root_fault_start + offset)
        .expect("root-emergency follows root-fault");
    let root_fault = &critical[root_fault_start..root_fault_end];
    assert!(root_fault.contains("let (info, message_registers) ="));
    assert!(root_fault.contains("fault_mr0: if fault_length > 0 {"));
    assert!(root_fault.contains("message_registers[0]"));
    assert!(root_fault.contains("message_registers[1]"));
    assert!(!root_fault.contains("sel4::message_register("));

    let construction = source_section(
        critical,
        "fn construct_restricted_child(",
        "fn install_permanent_cnode_retention(",
    );
    assert!(construction.contains("let ipc_buffer_vaddr = ipc_frame.ptr().as_ptr() as usize;"));
    assert!(construction.contains("ipc_buffer_vaddr as seL4_Word,"));

    let driver = include_str!("../src/hal/driver_task.rs");
    let containment = source_section(
        driver,
        "pub fn root_driver_supervisor_contain_fault(",
        "/// Classic kernels cannot consume the MCS driver-supervisor hook.",
    );
    assert!(containment.contains("Some(ipc_buffer_vaddr)"));
    assert!(containment.contains("crate::sel4::reply_to("));
    assert!(!containment.contains("set_message_register"));

    let syscall = include_str!("../src/sel4/syscall.rs");
    assert!(syscall.contains("seL4_RecvWithMRs"));
    assert!(syscall.contains("seL4_MCS_ReplyWithMRs"));
}

#[test]
fn worker_supervisor_fanin_follows_durable_publication() {
    let critical = include_str!("../src/hal/critical_tcb.rs");
    assert!(critical.contains("const CHILD_ROOT_CONTROL_WAKE_SLOT: seL4_CPtr = 5;"));
    let fanin_helper = source_section(
        critical,
        "fn signal_root_control_fanin_hint()",
        "fn publish_target_worker_fault(",
    );
    for required in [
        "feature = \"release-pi4\"",
        "target_arch = \"aarch64\"",
        "target_os = \"none\"",
        "sel4_config_kernel_mcs",
        "sel4::signal_unchecked(CHILD_ROOT_CONTROL_WAKE_SLOT)",
    ] {
        assert!(fanin_helper.contains(required), "missing {required}");
    }
    assert_eq!(
        critical
            .matches("sel4::signal_unchecked(CHILD_ROOT_CONTROL_WAKE_SLOT)")
            .count(),
        1,
        "only the Pi-gated helper may issue the fan-in syscall",
    );
    let construction = source_section(
        critical,
        "fn construct_restricted_child(",
        "fn configure_active_sc(",
    );
    let worker_fanin = source_section(
        construction,
        "if task.id == WORKER_SUPERVISOR_ID {\n            mint_child_cap(",
        "        if task.id == DRIVER_SUPERVISOR_ID {",
    );
    for required in [
        "CHILD_ROOT_CONTROL_WAKE_SLOT",
        "root_control_wake",
        "write_only_rights()",
        "critical.worker-supervisor-root-control-wake",
    ] {
        assert!(worker_fanin.contains(required), "missing {required}");
    }
    assert!(worker_fanin.contains("                1,"));

    let entry = source_section(
        critical,
        "extern \"C\" fn root_worker_supervisor_entry",
        "extern \"C\" fn root_worker_executor_gpu_entry",
    );
    let completion_publish = entry
        .find("drain_target_wake(badge)")
        .expect("durable role-wake publication");
    let completion_signal = entry[completion_publish..]
        .find("signal_root_control_fanin_hint()")
        .map(|offset| completion_publish + offset)
        .expect("root fan-in after role-wake publication");
    assert!(completion_publish < completion_signal);

    let transfer = source_section(
        entry,
        "let result = match item {",
        "        let retry = match TARGET_HANDOFF.try_lock()",
    );
    let fault_publish = transfer
        .find("drain_critical_fault(record)")
        .expect("durable fault publication");
    let validated_control = transfer
        .find("WorkerSupervisorItem::Control(_) => Ok(())")
        .expect("existing validated-control FIFO retained");
    let fanin = transfer
        .find("signal_root_control_fanin_hint()")
        .expect("coalesced root fan-in");
    assert!(fault_publish < fanin && validated_control < fanin);
    assert_eq!(
        transfer.matches("signal_root_control_fanin_hint()").count(),
        1
    );

    let service_publication = source_section(
        critical,
        "fn publish_target_service_fault(",
        "fn publish_target_driver_fault(",
    );
    let service_publish = service_publication
        .find("handoff.publish_service_fault(record)?;")
        .expect("durable service-fault publication");
    let service_unlock = service_publication
        .find("drop(handoff);")
        .expect("service-fault publication unlock");
    let service_fanin = service_publication
        .find("signal_root_control_fanin_hint();")
        .expect("service-fault root fan-in");
    assert!(service_publish < service_unlock && service_unlock < service_fanin);

    let driver_publication = source_section(
        critical,
        "fn publish_target_driver_fault(",
        "fn resolve_target_fault(",
    );
    let driver_publish = driver_publication
        .find("handoff.publish_driver_fault(runtime_slot, record)?;")
        .expect("durable driver-fault publication");
    let driver_unlock = driver_publication
        .find("drop(handoff);")
        .expect("driver-fault publication unlock");
    let driver_semantic_wake = driver_publication
        .find("sel4::signal_unchecked(CHILD_DRIVER_SIGNAL_SLOT);")
        .expect("driver-supervisor semantic wake");
    let driver_fanin = driver_publication
        .find("signal_root_control_fanin_hint();")
        .expect("driver-fault root fan-in");
    assert!(
        driver_publish < driver_unlock
            && driver_unlock < driver_semantic_wake
            && driver_semantic_wake < driver_fanin
    );

    let worker_task = include_str!("../src/hal/worker_task.rs");
    let immediate = source_section(
        worker_task,
        "fn handle_one_immediate_work(",
        "fn handle_one_deadline(",
    );
    assert!(immediate.contains("take_validated_worker_control"));
}

#[test]
fn terminal_fault_suspends_without_reply_or_early_reuse() {
    let mut lane = FaultReplyLane::default();
    lane.begin(registration(true), FaultClass::Standard)
        .expect("associate");
    assert_eq!(
        lane.reply_recoverable_timeout(true),
        Err(FaultReplyLaneError::ReplyForbidden)
    );
    assert_eq!(
        lane.finish_terminal(false, true),
        Err(FaultReplyLaneError::AssociationNotCleared)
    );
    assert_eq!(
        lane.begin(registration(true), FaultClass::Standard),
        Err(FaultReplyLaneError::Busy)
    );
    lane.finish_terminal(true, true)
        .expect("suspend cleared lane");
    assert_eq!(lane.state(), FaultReplyLaneState::Free);
}

#[test]
fn recoverable_timeout_replies_exactly_once_and_graph_is_acyclic() {
    validate_critical_temporal_graph().expect("acyclic generated graph");
    let root_fault = generated::temporal_tasks()
        .iter()
        .find(|task| task.id == "root-fault")
        .expect("root-fault");
    let root_emergency = generated::temporal_tasks()
        .iter()
        .find(|task| task.id == "root-emergency")
        .expect("root-emergency");
    assert_eq!(root_fault.fault_handler, "root-emergency");
    assert!(root_emergency.fault_handler.is_empty());
    assert_eq!(mcs_extra_refills(root_fault.max_refills), Ok(0));

    let mut lane = FaultReplyLane::default();
    lane.begin(registration(false), FaultClass::Timeout)
        .expect("associate");
    assert_eq!(
        lane.reply_recoverable_timeout(false),
        Err(FaultReplyLaneError::ReplyForbidden)
    );
    lane.reply_recoverable_timeout(true).expect("one reply");
    assert_eq!(
        lane.reply_recoverable_timeout(true),
        Err(FaultReplyLaneError::ReplyAlreadyIssued)
    );
    assert_eq!(
        lane.finish_recoverable(false),
        Err(FaultReplyLaneError::AssociationNotCleared)
    );
    lane.finish_recoverable(true).expect("association clear");
}
