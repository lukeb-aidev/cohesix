<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Retain the partial M28c native-action implementation and installed Mac evidence without claiming live job acceptance. -->
<!-- Author: Lukas Bower -->

# Milestone 28c native actions component record

`m28c-native-apple-actions` remains **In Progress**. The gateway now derives
one `systemd.restart` ticket from an operator-selected standing scope and
fresh native facts. A stable request ID names the durable admission. On a
lost reply, the same request ID resolves to the existing record without a
second target write. Scope revocation and the existing delegated write check
still gate fresh effects. The route does not admit MLX or a GPU workload.

```text
Title/ID: m28c-native-apple-actions
Milestone: 28c / macOS 27: Siri/App Intents, Apple AI and Metal-Backed Workflows
Goal: Start and inspect governed work from the installed native app and user-created Shortcuts without widening target authority.
Inputs: selected standing scope, Hive Gateway, delegated credential, signed SwarmUI app/extension and supported Mac.
Changes: Gateway approved-scope start route; matching coh, Python and Swift clients; fifth App Intents action; OpenAPI and host guidance.
Commands: cargo test --locked -p hive-gateway --bin hive-gateway jobs::tests::approved_start_derives_only_the_selected_service_and_fresh_facts; cargo test --locked -p cohesix-rest --lib approved_start_sends_only_scope_and_stable_request_identity; cargo test --locked -p coh-cli --lib selected_job_commands_share_one_rest_connection_schema; PYTHONPATH=tools/cohesix-py out/m28c/mlx-venv/bin/python -m pytest -q tools/cohesix-py/tests/test_selected_jobs.py; swift test --package-path apps/swarmui/native/apple; xcodebuild -quiet -project apps/swarmui/native/apple/SwarmUIApple.xcodeproj -scheme SwarmUIIntents -configuration Release -derivedDataPath out/m28c/DerivedData CODE_SIGNING_ALLOWED=NO build.
Checks: Focused gateway, REST, CLI and Python checks passed; 12 Swift checks passed; unsigned extension build passed. A newly development-signed app and extension verified on disk, and Shortcuts displayed the new action. This working-tree installation does not bind an immutable committed source or exercise a delegated live job.
Deliverables: Source action and partial installed discovery; m28c-actions-live and Siri/job acceptance remain open.
```

The installed diagnostic app is
`/Users/lukasbower/Applications/SwarmUI-M28c-ApprovedStart.app`. Its app
binary SHA-256 is
`0e26f933602daea06aa6f2137c7695385f6996c05881f85671e4e6029934193c`;
its extension binary SHA-256 is
`fc3410a195640ca3182c497018af14441cf146cf65bff6896db67549c9dcc5e0`.
Both passed strict nested signing verification under the Apple Development
identity and Team `KB88FQXUX2`. `pluginkit` resolved exactly one registered
`com.cohesix.swarmui.intents` extension from that app. After Shortcuts
restarted, its search showed **Start Approved Cohesix Job** alongside the
four previously observed actions. The ignored screenshot
`out/m28c/approved-start-shortcuts.png` has SHA-256
`2a7fd811cfe2a268d4b0fbb61a1e6ed1cf887d33e646a7bb6ddec074d0824362`.

The action has not started an authenticated job, and the installed app has
not completed in-app Keychain enrollment. No spoken Siri invocation, terminal
result, `coh`/SwarmUI correlation, MLX admission, vMLX canary or final
Developer ID notarisation follows from this observation. A source-bound
`m28c-actions-live` report must replace it before task acceptance.

Host-tool, Python and benchmark compatibility review: the new REST operation
is implemented in `coh`, the Python REST backend and the native extension.
Existing raw job submit, status, cancel and reconcile keep their semantics.
The SwarmUI parser-derived operation catalog exposes `job start-approved` as
a structured host form. No benchmark field, VM grammar or target interface
changed; the target sees the existing exact version-1 service ticket only.
Material AI assistance contributed this implementation, focused checks and
record. No approval or live result is inferred from the code.
