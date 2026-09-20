// Author: Lukas Bower
// Purpose: Exercise documented demo scripts against explicit host contracts without target claims.
// Copyright 2026 Lukas Bower
#![cfg(all(feature = "in-process", feature = "gpu-bridge"))]

use std::fs;
use std::io::Cursor;
use std::path::Path;

use cohsh::{NineDoorTransport, Shell};
use nine_door::{
    AuditConfig, AuditLimits, HostNamespaceConfig, HostProvider, NineDoor, PolicyConfig,
    PolicyLimits, PolicyRuleSpec, ReplayConfig,
};

#[test]
fn demo_runbooks_execute_against_enabled_host_contracts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for name in [
        "demo_runbook.coh",
        "authority.coh",
        "telemetry.coh",
        "control_plane.coh",
        "host_workflows.coh",
        "evidence.coh",
        "fixtures/workers.coh",
    ] {
        let policy = PolicyConfig::enabled(
            vec![PolicyRuleSpec {
                id: "queen-ctl".to_owned(),
                target: "/queen/ctl".to_owned(),
            }],
            PolicyLimits::default(),
        );
        let audit = AuditConfig::enabled(
            AuditLimits::default(),
            ReplayConfig::enabled(64, 1024, 1024),
        );
        let host = HostNamespaceConfig::enabled(
            "/host",
            &[
                HostProvider::Systemd,
                HostProvider::K8s,
                HostProvider::Docker,
                HostProvider::Nvidia,
            ],
        )
        .expect("valid host fixture configuration");
        let server = if name.starts_with("fixtures/") {
            NineDoor::new()
        } else {
            NineDoor::new_with_host_policy_audit_config(host, policy, audit)
        };
        let snapshot = gpu_bridge_host::auto_bridge(true)
            .expect("mock GPU bridge")
            .serialise_namespace()
            .expect("mock GPU namespace");
        server
            .install_gpu_nodes(&snapshot)
            .expect("install mock GPU");
        let mut source = fs::read_to_string(root.join("demo").join(name)).expect("demo script");
        // Resolve the documented checkout-relative input without changing process cwd.
        source = source.replace(
            "demo/telemetry/demo.txt",
            root.join("demo/telemetry/demo.txt")
                .to_str()
                .expect("UTF-8 path"),
        );
        let mut output = Vec::new();
        let result =
            Shell::new(NineDoorTransport::new(server), &mut output).run_script(Cursor::new(source));
        assert!(
            result.is_ok(),
            "{name}: {result:?}\n{}",
            String::from_utf8_lossy(&output)
        );
    }
}
