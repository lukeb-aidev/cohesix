<!-- Author: Lukas Bower -->
<!-- Purpose: Retain the exact M28c Mac developer-workflow evidence, signed app identity and proof boundaries. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# M28c completion record — Mac developer actions and local Metal

```text
Title/ID: m28c-mac-developer-journey
Milestone: 28c / m28c-mac-developer-journey
Goal: Qualify a directly distributed Mac app that performs authenticated developer actions and bounded local Metal work.
Inputs: macOS 27.0 (26A428), Apple M4, Xcode 27.0, selected private KVM reference, pinned Qwen2.5-1.5B MLX model/data/adapter, installed signed vMLX 1.6.65, Team KB88FQXUX2.
Changes:
  - apps/swarmui/src-tauri/main.rs + apps/swarmui/src/workbench.rs + frontend/workbench/mlx.js + frontend/index.html — fixed local MLX helper invocation and a visible inference/evaluation desk, with local proof class separate from the signed release projection.
  - tools/cohesix-py/cohesix/mlx_workbench.py — bounded private selection, unchanged input check and native Metal observations.
  - docs/BUILD_PLAN.md + docs/TEST_PLAN.md + docs/STATUS.md + docs/HOST_TOOLS.md + docs/SWARMUI.md + docs/TOOLCHAIN_MAC_ARM64.md — developer-value scope, 28c1 successor and exact as-built use.
Commands: PYTHONPATH=tools/cohesix-py out/m28c/mlx-venv/bin/python -m pytest -q tools/cohesix-py/tests/test_mlx_workbench.py tools/cohesix-py/tests/test_mlx_native.py; PYTHONPATH=tools/cohesix-py out/m28c/mlx-venv/bin/python -m pytest -q tools/cohesix-py/tests/test_vmlx_compat.py tools/cohesix-py/tests/test_vmlx_runtime.py; node --test apps/swarmui/tests/mlx_frontend.test.mjs; cargo test --locked -p swarmui --test workbench; cargo check --locked -p swarmui --bin swarmui; cargo fmt --all -- --check; scripts/check-generated.sh; cargo build --locked --release -p swarmui -p coh; exact stage/sign/notarise commands below.
Checks: Focused owner tests pass; real installed SwarmUI inference/evaluation observes Apple M4 and pinned hashes; the final app and extension pass Developer ID signing, Apple notarisation, stapler, Gatekeeper and installed-byte readback. Earlier selected service start/inspect/cancel/revocation and Foundation Models explanation remain component evidence from an unchanged native/gateway source path.
Deliverables: Installed SwarmUI-M28c-Build2.app, retained ignored signing/live reports, this record, and 28c Complete status with explicit 28c1 deployment limits.
```

## Exact installed candidate

The final package was staged at ignored `out/m28c/canonical-stage-12` from the
release `swarmui`/`coh` binaries, compiler-generated package inventory and the
Release App Intents extension rebuilt with matching app/extension build number
`2`. Staging retained 44 local UI assets.
The unsigned staged app executable matched `target/release/swarmui` at
SHA-256 `2b7068e0cd942f7787391b22f9a36da1d3aa033e3345e8a50f1319bb5996b355`;
the staged extension executable matched its Xcode input at
`e71c23de067fb8b63c9fc58ecf96bc561253cb55730f76cf9e09982c67e4b5df`.
The signing script validated exact app and extension provisioning profiles,
Team `KB88FQXUX2`, shared Keychain group, hardened runtime, timestamps and
nested signatures. The final signed app executable is
`c9ae9fa50c62beb7f2d73cd61e8ab10959774e5c029c38c029a92a934da607fa`;
the extension executable is
`faa202d77eb0fc1bc56799e9955f3980e36dd7ff7076d511aefce2f3a48ec8e4`.
Apple accepted notarisation submission
`b14be979-3330-4770-94f1-73fce245bf6e` for archive SHA-256
`22d06f92e14fcad3671e62e1be2f653df73f722bbac0bc49c3758068806aea2f`.
The retained ignored `developer-id-signing/summary.json` and
`notarization/summary.json` bind both binaries, profiles and submission.

The selected distribution commands, from the worktree root, were:

```bash
xcodebuild -project apps/swarmui/native/apple/SwarmUIApple.xcodeproj -scheme SwarmUIIntents -configuration Release -derivedDataPath out/m28c/DerivedData-build2 CODE_SIGNING_ALLOWED=NO build
python3 scripts/install/stage_swarmui.py --repo "$PWD" --generated-root "$PWD" --bin-dir "$PWD/target/release" --profile macos-desktop --apple-extension-dir "$PWD/out/m28c/DerivedData-build2/Build/Products/Release/SwarmUIIntents.appex" --out "$PWD/out/m28c/canonical-stage-12"
python3 scripts/install/sign_swarmui_macos.py sign --app "$PWD/out/m28c/canonical-stage-12/package-input/SwarmUI.app" --team-id KB88FQXUX2 --identity-sha1 EEF1BF26D2081CEB18AF1F5B0C4A6D720C1F2B98 --app-profile /Users/lukasbower/Downloads/Cohesix_SwarmUI_Developer_ID.provisionprofile --extension-profile /Users/lukasbower/Downloads/Cohesix_SwarmUI_App_Intents_Developer_ID.provisionprofile --state-dir "$PWD/out/m28c/canonical-stage-12/developer-id-signing"
python3 scripts/install/sign_swarmui_macos.py notarize --app "$PWD/out/m28c/canonical-stage-12/package-input/SwarmUI.app" --team-id KB88FQXUX2 --notary-profile cohesix-m28c --state-dir "$PWD/out/m28c/canonical-stage-12/notarization"
```

`~/Applications/SwarmUI-M28c-Build2.app` was copied from that accepted app.
Its two executable hashes matched the signed stage. `codesign --verify --deep
--strict`, `xcrun stapler validate` and `spctl --assess --type execute --verbose`
passed on the installed copy; Gatekeeper reported `source=Notarized Developer ID`.
`pluginkit` registered exactly one `com.cohesix.swarmui.intents` extension
from the build-2 app. Earlier build-1 copies sharing the bundle ID left the
saved action unresolved after replacement. With matching build-2 app/extension
versions, the installed app launched, Shortcuts resolved the saved read-only
job-status action and the gateway returned HTTP 403 after the earlier scope
revocation. This is an expected refusal, not evidence of a fresh authorised
job. The retained ignored `out/m28c/canonical-stage-12/shortcuts-revoked-status.ax.txt`
has SHA-256 `374811a6288f5d74c1eefe1c44d49dc6cd347a7d2ee73afa859eb650ba58d3fc`.

The exact newly changed source surfaces at staging have SHA-256:
`main.rs` `4a59698e39e166ba9b7e91cc02eece7e4331374ccc75e013b9dfdedb916f4815`;
`workbench.rs` `96b3e3cd178d600561682404a2ce85584e0426d055c87d4f8790bc1862373c86`;
`frontend/index.html` `10ab08b1eca8119032d8db4e64a38fe30888c3cca3ab1ccf35c2b9f52934a24c`;
`frontend/workbench/mlx.js` `a9113f00e61f02af381045c3b3a933e7e16c94b61bcee8eab037fbadc3687d97`;
`mlx_workbench.py` `14c4f9c52a78ab23bfe7cd6159e17d7d965aa3fc0c1bb27e155731bec63dcb7e`.
The native Swift selected-service action source did not change after the
private service observations in the [actions record](M28C_APPLE_ACTIONS_RECORD.md).
Later gateway changes added release admission and a selected-generation fence;
focused gateway standing/job tests passed on this branch. The final saved
status Shortcut's HTTP 403 remains an explicit refusal, not an inferred live
success against that later gateway build.

## Live developer work and focused checks

The earlier installed development-signed app started and inspected private KVM
service admission `mac-service-2524-shortcuts-02`. The host agent settled it as
`confirmed/acknowledged`; `coh` resolved the same admission and signed result.
Foundation Models explained that job using separately labelled gateway facts
and model interpretation. A separate held admission
`mac-service-2524-cancel-01` settled as `refused_no_effect/acknowledged`
after the corrected cancellation request, without service dispatch. Revocation
removed the native scope choice. The [actions record](M28C_APPLE_ACTIONS_RECORD.md)
retains its private target and accessibility evidence, including the initial
HTTP 404 defect and correction. These are selected component observations,
not a Release B reference or a new final-binary service effect.

The local model is the pinned Qwen2.5-1.5B instruction model tree
`8b40b6d325ea432fda5d9d9612813d203d44bde7c232b465b0af32454b1d4791`;
the private 16/4/16-row dataset tree is
`6b2ef5031d278330c0e4db1a40f5de3ddcc2647df74e2fa7a1c2d0f3229670b7`;
the 48-step rank-8 Metal LoRA adapter tree is
`a9b837751e207b3c2c52d166f4fb341921cd8d5082197a82915d88f1ea498850`.
The [native component record](M28C_MLX_COMPONENT_RECORD.md) holds the frozen
training, resource and four-question quality checks, with repeated answer
templates stated as a limitation. The final installed SwarmUI panel invoked
`cohesix.mlx_workbench` from the selected local Python environment without a
hive credential. On Apple M4, its answer to the bounded canary question began
“No”; peak Metal allocation was 974,217,888 bytes and elapsed generation
was 1,511 ms. The same installed panel evaluated 16 held-out rows with loss
`0.8091070055961609` and peak Metal allocation 1,138,331,988 bytes.
The UI labelled both as `Local Metal observation · no Cohesix admission or
promotion`. Retained ignored accessibility reads are
`out/m28c/canonical-stage-12/installed-mlx-infer.ax.txt` (SHA-256
`66924f7cc60b6cc2e8838a2352b5b621c9c835438de98774a6907586b2a5ff85`)
and `installed-mlx-evaluate.ax.txt`
(`043720c90d819f5a45ef3d75c8c88a30f1098f5756222a39942c014422782c6a`).
Focused Python MLX input/refusal tests passed 6/6, vMLX compatibility tests
12/12, frontend projection tests 3/3, SwarmUI workbench tests 12/12,
`cargo check`, Rust formatting and generated consistency passed.
The full test suite was not run, as requested.

Installed signed vMLX 1.6.65 served a disposable fused copy of this adapter
on loopback. Four predeclared operational responses exactly matched direct
fused MLX inference within the 5-second bound. Its source model remained
`b75efc50e56aec7c671357ee6eec7d4df55b8c82896a4308d941e0b86e629777`;
the vMLX-repaired loaded copy was
`a2e3c00622388097a0e8ca3c2b8e600ba765ff44be7d56c88aca5bca43f7cec4`.
The [native component record](M28C_MLX_COMPONENT_RECORD.md) and ignored
`out/m28c/qwen-vmlx-session.json` retain engine/process/request evidence.
Its `g1` label is diagnostic. No Cohesix generation, signed canary or governed
rollback was created.

## Compatibility and proof boundary

The local UI adds a fixed Python module and JSON result; it changes no target
grammar, gateway API, `coh` operation schema, Python REST request or benchmark
measurement. The existing `coh` release/job forms and Python REST client retain
their admission semantics. The compiler-derived implementation, provider and
host-integration inventories were regenerated and checked. Native Mac MLX
admission, durable phases, generation-fenced vMLX rollout and rollback are
planned under [28c1](../BUILD_PLAN.md#28c1). Spoken Siri invocation is outside
the M28c developer gate. This record makes no Pi, mixed MLX/CUDA, integrated
Release B or general model-quality claim.

Material AI assistance contributed the implementation, tests, scope revision
and this record. The retained evidence distinguishes actual installed actions,
local Metal output, diagnostic vMLX serving and signed Cohesix outcomes.
