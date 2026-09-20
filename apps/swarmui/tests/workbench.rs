// Author: Lukas Bower
// Purpose: Guard desktop endpoint, structured argv, namespace and control boundaries with independent contract cases.
// Copyright 2026 Lukas Bower
use std::collections::BTreeMap;
use swarmui::workbench::{
    control_line, host_arguments, namespace_command, ConnectionRequest, ControlRequest, HostRequest,
};

fn connection() -> ConnectionRequest {
    ConnectionRequest {
        transport: "rest".into(),
        endpoint: "https://gateway.example:8443".into(),
        credential: "operator-unique-test-credential".into(),
        role: "queen".into(),
        ticket: None,
    }
}
fn request(operation: &[&str], fields: &[(&str, &str)]) -> HostRequest {
    HostRequest {
        operation: operation.iter().map(|s| s.to_string()).collect(),
        values: fields
            .iter()
            .map(|(k, v)| (k.to_string(), vec![v.to_string()]))
            .collect(),
    }
}
#[test]
fn connection_rejects_credentials_in_urls_and_placeholder_auth() {
    for endpoint in [
        "https://token@gateway.example/",
        "https://gateway.example/?secret=x",
        "file:///tmp/host",
        "https://gateway.example/prefix",
        "https://gateway.example/#x",
    ] {
        let mut input = connection();
        input.endpoint = endpoint.into();
        assert!(input.resolve().is_err(), "{endpoint}");
    }
    for credential in ["", "CHANGEME", "bootstrap", "secret\nAUTH injected"] {
        let mut input = connection();
        input.credential = credential.into();
        assert!(input.resolve().is_err());
    }
    let mut input = connection();
    input.transport = "console".into();
    input.endpoint = "tcp://192.0.2.1:31337".into();
    let resolved = input.resolve().expect("valid explicit Queen endpoint");
    assert_eq!(resolved.port, 31337);
    assert_eq!(resolved.host, "192.0.2.1");
}
#[test]
fn host_arguments_reuse_cli_validation_without_shell_or_secret_arguments() {
    let active = connection().resolve().unwrap();
    let args = host_arguments(
        &request(&["gpu", "status"], &[("gpu", "device-1")]),
        Some(&active),
        false,
    )
    .unwrap();
    assert_eq!(
        args,
        vec![
            "coh",
            "--role=queen",
            "gpu",
            "status",
            "--gpu=device-1",
            "--rest-url=https://gateway.example:8443"
        ]
    );
    assert!(!args.iter().any(|s| s.contains(&active.credential)));
    let literal = "/tmp/input $(touch forbidden); x";
    let args = host_arguments(&request(&["trace"], &[("input", literal)]), None, true).unwrap();
    assert_eq!(args, vec!["coh", "trace", &format!("--input={literal}")]);
    assert!(host_arguments(
        &request(
            &["gpu", "lease"],
            &[
                ("gpu", "0"),
                ("mem_mb", "-1"),
                ("streams", "1"),
                ("ttl_s", "10")
            ]
        ),
        Some(&active),
        false
    )
    .is_err());
    assert!(host_arguments(
        &request(
            &["trace"],
            &[("input", "/tmp/trace"), ("auth_token", "injected")]
        ),
        None,
        true
    )
    .is_err());
}
#[test]
fn offline_and_single_owner_refuse_live_host_actions() {
    let active = connection().resolve().unwrap();
    let action = request(&["gpu", "list"], &[]);
    assert!(host_arguments(&action, Some(&active), true).is_err());
    assert!(host_arguments(&action, None, false).is_err());
    let mut direct = active;
    direct.transport = "console".into();
    assert!(host_arguments(&action, Some(&direct), false).is_err());
    assert!(host_arguments(&request(&["run"], &[]), Some(&direct), false).is_err());
    assert!(host_arguments(&request(&["providers"], &[]), None, true).is_ok());
}
#[test]
fn namespace_paths_refuse_ambiguous_or_out_of_profile_walks() {
    let roots = vec!["/proc".into(), "/shard".into()];
    for path in [
        "proc",
        "/proc/..",
        "/proc/.",
        "/proc//boot",
        "/proc/a b",
        "/proc/x\0",
        "/proc/a/b/c/d/e/f/g/h",
        "/worker/1",
        "/processor",
    ] {
        assert!(namespace_command("cat", path, &roots).is_err(), "{path}");
    }
    assert_eq!(
        namespace_command("tail", "/shard/01/worker/opaque/telemetry", &roots).unwrap(),
        "tail /shard/01/worker/opaque/telemetry"
    );
}
#[test]
fn guided_controls_follow_the_independent_console_contract() {
    let roots = vec!["/queen".into()];
    let input = ControlRequest {
        action: "spawn".into(),
        fields: BTreeMap::from([
            ("role".into(), "heartbeat".into()),
            ("ticks".into(), "3".into()),
        ]),
    };
    assert_eq!(
        control_line(&input, &roots).unwrap(),
        "spawn {\"spawn\":\"heartbeat\",\"ticks\":3}"
    );
    let input = ControlRequest {
        action: "budget".into(),
        fields: BTreeMap::from([("ops".into(), "10".into())]),
    };
    assert_eq!(
        control_line(&input, &roots).unwrap(),
        "echo {\"budget\":{\"ops\":10}} > /queen/ctl"
    );
    let input = ControlRequest {
        action: "append".into(),
        fields: BTreeMap::from([
            ("path".into(), "/queen/ctl".into()),
            ("payload".into(), "bad\nquit".into()),
        ]),
    };
    assert!(control_line(&input, &roots).is_err());
}

#[test]
fn reviewed_action_is_single_use_and_invalidated_by_context_or_input_changes() {
    use serde_json::json;
    use swarmui::workbench::ReviewGate;
    let mut gate = ReviewGate::default();
    let action = json!({"host":{"operation":["providers"],"values":{}}});
    let id = gate.preview(action.clone());
    assert!(gate
        .consume(&id, &json!({"host":{"operation":["run"]}}))
        .is_err());
    assert!(gate.consume(&id, &action).is_err());
    let id = gate.preview(action.clone());
    gate.invalidate();
    assert!(gate.consume(&id, &action).is_err());
    let id = gate.preview(action.clone());
    assert!(gate.consume(&id, &action).is_ok());
    assert!(gate.consume(&id, &action).is_err());
}

#[test]
fn structured_controls_serialize_documented_records_and_refuse_ambiguous_values() {
    let roots = vec![
        "/queen".into(),
        "/policy".into(),
        "/actions".into(),
        "/replay".into(),
    ];
    for (action,fields,expected) in [
        ("lifecycle",vec![("action","cordon")],"echo cordon > /queen/lifecycle/ctl"),
        ("dequeue",vec![("id","sched-1")],"echo {\"id\":\"sched-1\",\"op\":\"dequeue\"} > /queen/schedule/ctl"),
        ("approval",vec![("id","decision-1"),("target","/queen/ctl"),("decision","deny")],"echo {\"decision\":\"deny\",\"id\":\"decision-1\",\"target\":\"/queen/ctl\"} > /actions/queue"),
        ("audit-replay",vec![("from","42")],"echo {\"from\":42} > /replay/ctl"),
    ] {
        let request=ControlRequest {action:action.into(),fields:fields.into_iter().map(|(k,v)|(k.into(),v.into())).collect()};
        assert_eq!(control_line(&request,&roots).unwrap(),expected);
    }
    for (action, fields) in [
        ("lifecycle", vec![("action", "cordon reset")]),
        ("dequeue", vec![("id", "x y")]),
        ("audit-replay", vec![("from", "-1")]),
    ] {
        let request = ControlRequest {
            action: action.into(),
            fields: fields
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        };
        assert!(control_line(&request, &roots).is_err());
    }
}

#[test]
fn boot_information_form_uses_the_canonical_bi_verb() {
    assert_eq!(
        control_line(
            &ControlRequest {
                action: "bootinfo".into(),
                fields: BTreeMap::new()
            },
            &[]
        )
        .unwrap(),
        "bi"
    );
}
