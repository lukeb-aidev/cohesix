<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Retain M28c native-action implementation and bounded live Mac evidence without claiming Siri or milestone acceptance. -->
<!-- Author: Lukas Bower -->

# Milestone 28c native actions component record

`m28c-native-apple-actions` remains **In Progress**. The gateway now derives
one `systemd.restart` ticket from an operator-selected standing scope and
fresh native facts. A stable request ID names the durable admission. On a
lost reply, the same request ID resolves to the existing record without a
second target write. Scope revocation and the existing delegated write check
still gate fresh effects. The route does not admit MLX or a GPU workload. A
live cancellation check found that the cancel route compared the delegated
ticket fingerprint with the job subject, so an authorized cancel was refused.
The route now uses the verified subject from the charged write principal.

```text
Title/ID: m28c-native-apple-actions
Milestone: 28c / macOS 27: Siri/App Intents, Apple AI and Metal-Backed Workflows
Goal: Start and inspect governed work from the installed native app and user-created Shortcuts without widening target authority.
Inputs: selected standing scope, Hive Gateway, delegated credential, signed SwarmUI app/extension and supported Mac.
Changes: Gateway approved-scope start route; matching coh, Python and Swift clients; fifth App Intents action; OpenAPI and host guidance.
Commands: cargo test --locked -p hive-gateway --bin hive-gateway jobs::tests::approved_start_derives_only_the_selected_service_and_fresh_facts; cargo test --locked -p hive-gateway cancellation_checks_the_verified_subject_and_write_scope --bin hive-gateway; cargo test --locked -p cohesix-rest --lib approved_start_sends_only_scope_and_stable_request_identity; cargo test --locked -p coh-cli --lib selected_job_commands_share_one_rest_connection_schema; PYTHONPATH=tools/cohesix-py out/m28c/mlx-venv/bin/python -m pytest -q tools/cohesix-py/tests/test_selected_jobs.py; swift test --package-path apps/swarmui/native/apple; xcodebuild -quiet -project apps/swarmui/native/apple/SwarmUIApple.xcodeproj -scheme SwarmUIIntents -configuration Release -derivedDataPath out/m28c/DerivedData CODE_SIGNING_ALLOWED=NO build.
Checks: Focused gateway, REST, CLI and Python checks passed; 13 Swift checks passed; unsigned extension build passed. The installed development-signed app and extension passed an exact committed-source m28c-platform-live discovery gate. A later private KVM realm exercised an installed, delegated start, status, cancellation and revoked-scope picker; a focused gateway cancellation-principal test passed. The live record below is component evidence, not the required m28c-actions-live runner or spoken Siri evidence.
Deliverables: Source action, live Shortcuts/service observation, cancellation fix and revoked scope withdrawal; m28c-actions-live and spoken Siri acceptance remain open.
```

The installed diagnostic app is
`/Users/lukasbower/Applications/SwarmUI-M28c-ApprovedStart.app`. Its app
binary SHA-256 is
`29bb486f47a8bf5d58b3db835e249f1a2a8e8254721388ddf18cd003f51e0423`;
its extension binary SHA-256 is
`f320e6823321fa1058863acd2aa0e85ff438ae705a9aed423009c30467ddce7b`.
Both passed strict nested signing verification under the Apple Development
identity and Team `KB88FQXUX2`. `pluginkit` resolved exactly one registered
`com.cohesix.swarmui.intents` extension from that app. After Shortcuts
restarted, its search showed **Start Approved Cohesix Job** alongside the
four previously observed actions. The exact-source platform report is retained
at `out/m28c/platform-live-scope-picker/summary.json` for commit
`952eacef9a01775b55ab135e3fea84841c38c77b`. It reports the local Apple M4,
Metal and Foundation Models availability and explicitly records `siri_invoked:
false` and `notarized: false`. Its full Shortcuts accessibility observation
is retained at `out/m28c/shortcuts-scope-picker-c952.ax.txt` with SHA-256
`3cae31b1a2ce0f9a5454771db8780f235cd9591edfd73406baa4210d560574e3`.

The later installed app completed in-app Keychain enrollment for the delegated
connection and exposed the same authorized subject to its extension. In an
isolated remote KVM realm, its user-created Shortcut submitted
`mac-service-2524-shortcuts-02`. The returned record was initially
`reserved/pending`, then the host agent settled it as
`confirmed/acknowledged` with result SHA-256
`6c1b35b2a17dfc0641e0a117b4088008066d555a172e7da5ee63c1771e95529f`.
`coh job status` and `coh job reconcile` returned the same admission,
action, target and result digest. The selected system service was active with
InvocationID `c76a0892f12d4bf8b398aebf1ba6f03d`. The installed
Foundation Models action explained that real gateway record with separate
gateway evidence and model interpretation, and proposed a typed `inspect`
follow-up. These are selected live component observations; the private KVM
profile and host rebuild are not a Release B reference or the final signed
app journey.

A second bounded scope held `mac-service-2524-cancel-01` before native
dispatch. The installed Start action returned `reserved/pending`; its Cancel
action initially received HTTP 404 because of the gateway subject/fingerprint
bug. After the corrected gateway binary (SHA-256
`d8bb12f9e0bb33144f98a1e721a56c2cb59d4431a9a6532d18e7b4dbdcf1f24c`)
restarted against the same private ledger, the same Cancel Shortcut returned
`reserved/pending; cancellation requested`. The resumed host agent settled the
original admission as `refused_no_effect/acknowledged`, with no dispatch time
or result digest. Reconciliation showed target `claimed` and `running`
transitions without a `succeeded` transition. The system service InvocationID
remained `c76a0892f12d4bf8b398aebf1ba6f03d`. An independently minted
administrative ticket then revoked the scope: `coh job scopes` returned an
empty list, the native picker had no entries, a new admission ID did not
appear, and the service InvocationID still matched. Retained private UI
observations are `out/m28c/actions-live-2524/shortcuts-held-start.ax.txt`
(SHA-256 `b87abe83419d9f4f1a9318d224dce56adf3e6f3780c3e7d2fa2914a3b03cf966`),
`shortcuts-held-cancel.ax.txt`
(`e955e62f3858517b346fff774bca8f16d6839b90f755f804f962877665b36b95`)
and `shortcuts-revoked-picker.ax.txt`
(`df399f04d34c6116ae92c8b110b58bffeb501fd4134918c2e9c11653ad9a8e72`).

No spoken Siri invocation, admitted MLX lifecycle, vMLX canary/rollback or
final installed-journey Developer ID notarisation follows from these checks.
The platform report predates the private live realm; a source-bound
`m28c-actions-live` report must bind the final committed source, target,
installed app and speech observation before this task is accepted.

Host-tool, Python and benchmark compatibility review: the new REST operation
and subject-filtered scope discovery are implemented in `coh`, the Python REST
backend and the native extension.
Existing raw job submit, status and reconcile keep their semantics. Cancel now
uses the verified delegated subject as intended; `coh` and Python use the
same gateway route, so no client schema change is needed.
The SwarmUI parser-derived operation catalog exposes `job start-approved` as
a structured host form. No benchmark field, VM grammar or target interface
changed; the target sees the existing exact version-1 service ticket only.
Material AI assistance contributed this implementation, focused checks and
record. No approval or live result is inferred from the code.
