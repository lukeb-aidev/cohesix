// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate control-plane transcript for schedule/lease/export/policy surfaces.
// Author: Lukas Bower
#![forbid(unsafe_code)]

#[path = "../../../tests/fixtures/transcripts/support.rs"]
mod transcript_support;

use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport};
use cohsh_core::wire::{render_ack, AckLine, AckStatus, END_LINE};
use cohsh_core::{role_label, ConsoleVerb};
use nine_door::{HostNamespaceConfig, NineDoor, PolicyConfig, PolicyLimits};
use secure9p_codec::OpenMode;

const SCENARIO: &str = "control_plane_v0";
const SCHEDULE_PAYLOAD: &str =
    r#"{"id":"sched-1","role":"worker-gpu","priority":2,"ticks":3,"budget_ms":120}"#;
const LEASE_GRANT_PAYLOAD: &str =
    r#"{"op":"grant","id":"lease-1","subject":"queen","resource":"gpu0","ttl_s":300,"priority":5}"#;
const LEASE_PREEMPT_PAYLOAD: &str = r#"{"op":"preempt","id":"lease-1","reason":"timeout"}"#;
const EXPORT_OPEN_PAYLOAD: &str = r#"{"op":"open","id":"export-1","ttl_s":900}"#;
const EXPORT_CLOSE_PAYLOAD: &str = r#"{"op":"close","id":"export-1","reason":"window-complete"}"#;
const POLICY_APPLY_PAYLOAD: &str = r#"{"op":"apply","id":"rev-2026-02-03","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
const POLICY_ROLLBACK_PAYLOAD: &str = r#"{"op":"rollback","id":"rev-2026-02-03"}"#;

const PROC_SCHEDULE_SUMMARY: &str = "/proc/schedule/summary";
const PROC_SCHEDULE_QUEUE: &str = "/proc/schedule/queue";
const PROC_LEASE_SUMMARY: &str = "/proc/lease/summary";
const PROC_LEASE_ACTIVE: &str = "/proc/lease/active";
const PROC_LEASE_PREEMPTIONS: &str = "/proc/lease/preemptions";

#[test]
fn control_plane_transcript_matches_fixture() -> Result<()> {
    let policy = PolicyConfig::enabled(Vec::new(), PolicyLimits::default());
    let server = NineDoor::new_with_host_and_policy_config(HostNamespaceConfig::disabled(), policy);
    let connection = server.connect().expect("connect");
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;
    let schedule_queue_max = generated_limit("control_plane.schedule.queue_max_entries");
    let lease_active_max = generated_limit("control_plane.lease.active_max_entries");
    let lease_preemptions_max = generated_limit("control_plane.lease.preemptions_max_entries");

    let mut transcript = Vec::new();
    let detail = format!("role={}", role_label(Role::Queen));
    transcript.push(render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Attach.ack_label(),
        Some(detail.as_str()),
    ));

    write_payload(
        &mut client,
        "/queen/schedule/ctl",
        SCHEDULE_PAYLOAD.as_bytes(),
    )?;
    transcript.push(render_echo_ack("/queen/schedule/ctl", SCHEDULE_PAYLOAD));

    let schedule_summary = read_payload(&mut client, PROC_SCHEDULE_SUMMARY)?;
    assert_eq!(
        schedule_summary,
        format!("queue=1 dequeued=0 dropped=0 max_entries={schedule_queue_max}\n")
    );
    transcript.push(render_cat_ack(PROC_SCHEDULE_SUMMARY));
    transcript.push(END_LINE.to_owned());

    let schedule_queue = read_payload(&mut client, PROC_SCHEDULE_QUEUE)?;
    assert_eq!(
        schedule_queue,
        "id=sched-1 role=worker-gpu priority=2 ticks=3 budget_ms=120 seq=1\n"
    );
    transcript.push(render_cat_ack(PROC_SCHEDULE_QUEUE));
    transcript.push(END_LINE.to_owned());

    write_payload(
        &mut client,
        "/queen/lease/ctl",
        LEASE_GRANT_PAYLOAD.as_bytes(),
    )?;
    transcript.push(render_echo_ack("/queen/lease/ctl", LEASE_GRANT_PAYLOAD));

    write_payload(
        &mut client,
        "/queen/lease/ctl",
        LEASE_PREEMPT_PAYLOAD.as_bytes(),
    )?;
    transcript.push(render_echo_ack("/queen/lease/ctl", LEASE_PREEMPT_PAYLOAD));

    let lease_summary = read_payload(&mut client, PROC_LEASE_SUMMARY)?;
    assert_eq!(
        lease_summary,
        format!(
            "active=0 preemptions=1 quotas=0 max_active={lease_active_max} max_preemptions={lease_preemptions_max}\n"
        )
    );
    transcript.push(render_cat_ack(PROC_LEASE_SUMMARY));
    transcript.push(END_LINE.to_owned());

    let lease_active = read_payload(&mut client, PROC_LEASE_ACTIVE)?;
    assert_eq!(lease_active, "");
    transcript.push(render_cat_ack(PROC_LEASE_ACTIVE));
    transcript.push(END_LINE.to_owned());

    let lease_preemptions = read_payload(&mut client, PROC_LEASE_PREEMPTIONS)?;
    assert_eq!(
        lease_preemptions,
        "id=lease-1 subject=queen resource=gpu0 reason=timeout seq=2\n"
    );
    transcript.push(render_cat_ack(PROC_LEASE_PREEMPTIONS));
    transcript.push(END_LINE.to_owned());

    write_payload(
        &mut client,
        "/queen/export/ctl",
        EXPORT_OPEN_PAYLOAD.as_bytes(),
    )?;
    transcript.push(render_echo_ack("/queen/export/ctl", EXPORT_OPEN_PAYLOAD));

    write_payload(
        &mut client,
        "/queen/export/ctl",
        EXPORT_CLOSE_PAYLOAD.as_bytes(),
    )?;
    transcript.push(render_echo_ack("/queen/export/ctl", EXPORT_CLOSE_PAYLOAD));

    write_payload(&mut client, "/policy/ctl", POLICY_APPLY_PAYLOAD.as_bytes())?;
    transcript.push(render_echo_ack("/policy/ctl", POLICY_APPLY_PAYLOAD));

    write_payload(
        &mut client,
        "/policy/ctl",
        POLICY_ROLLBACK_PAYLOAD.as_bytes(),
    )?;
    transcript.push(render_echo_ack("/policy/ctl", POLICY_ROLLBACK_PAYLOAD));

    transcript.push(render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Quit.ack_label(),
        None,
    ));

    transcript_support::compare_transcript("cohsh", SCENARIO, "cohsh.txt", &transcript);

    Ok(())
}

fn write_payload<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
    payload: &[u8],
) -> Result<()> {
    let fid = client.open(path, OpenMode::write_append())?;
    let written = client.write(fid, u64::MAX, payload)?;
    let clunk_result = client.clunk(fid);
    if written as usize != payload.len() {
        return Err(anyhow!(
            "short write to {path}: expected {} bytes, wrote {written}",
            payload.len()
        ));
    }
    clunk_result?;
    Ok(())
}

fn read_payload<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
) -> Result<String> {
    let fid = client.open(path, OpenMode::read_only())?;
    let payload = client.read(fid, 0, client.negotiated_msize())?;
    client.clunk(fid)?;
    Ok(String::from_utf8(payload)?)
}

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cohsh has workspace parent")
        .parent()
        .expect("workspace root has parent")
        .join(path)
}

fn generated_limit(key: &str) -> usize {
    let snippet = fs::read_to_string(repo_path("docs/snippets/root_task_manifest.md"))
        .expect("read generated root-task manifest snippet");
    let needle = format!("- `{key}`: `");
    for line in snippet.lines() {
        if let Some(value) = line
            .strip_prefix(&needle)
            .and_then(|rest| rest.split('`').next())
        {
            return value.parse().expect("generated manifest limit is numeric");
        }
    }
    panic!("generated manifest snippet is missing {key}");
}

fn render_echo_ack(path: &str, payload: &str) -> String {
    let detail = format!("path={path} bytes={}", payload.len());
    render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Echo.ack_label(),
        Some(detail.as_str()),
    )
}

fn render_cat_ack(path: &str) -> String {
    let detail = format!("path={path}");
    render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Cat.ack_label(),
        Some(detail.as_str()),
    )
}

fn render_ack_line(status: AckStatus, verb: &str, detail: Option<&str>) -> String {
    let ack = AckLine {
        status,
        verb,
        detail,
    };
    let mut line = String::new();
    render_ack(&mut line, &ack).expect("render ack line");
    line
}
