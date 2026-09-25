<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Retain measured M28c Apple SDK, local compute and signing observations without promoting detached probes to acceptance. -->
<!-- Author: Lukas Bower -->

# Milestone 28c platform feasibility record

`m28c-apple-platform-feasibility` is **complete at its platform-probe scope**.
The focused `m28c-platform-live` case observed source `9dafa2bd0634cbcb5749417ccf014b2ba1d43022`
on the selected macOS 27 Mac with an installed, provisioned development-signed
app. The local support action also executed in Shortcuts and returned Apple
assistance and Metal availability without requesting a hive action. No Siri or
Shortcuts job operation, admitted MLX release, or final distribution outcome is claimed.

```text
Title/ID: m28c-apple-platform-feasibility
Milestone: 28c / macOS 27: Siri/App Intents, Apple AI and Metal-Backed Workflows
Goal: Establish public SDK, actual-device and signed-distribution feasibility before dependent implementation.
Inputs: accepted M28/M28a/M28b contracts; macOS 27 Mac; Xcode 27 SDK; current SwarmUI source and Mac package profile.
Changes:
  - apps/swarmui/native/apple/ + apps/swarmui/src/workbench.rs + apps/swarmui/frontend/ — public App Intents extension, in-app Keychain enrollment source, native capability and synthetic assistance probes, generated Xcode project specification and focused checks.
  - docs/BUILD_PLAN.md and docs/STATUS.md — expose the incomplete platform gate without promoting a host probe to workflow acceptance.
Commands: sw_vers; xcodebuild -version; xcrun --sdk macosx --show-sdk-version; swift test --package-path apps/swarmui/native/apple; swift run --package-path apps/swarmui/native/apple CohesixAssistanceProbe; xcodebuild -project apps/swarmui/native/apple/SwarmUIApple.xcodeproj -scheme SwarmUIIntents -configuration Release -derivedDataPath out/m28c/DerivedData CODE_SIGNING_ALLOWED=NO build; cargo build --locked -p swarmui --bin swarmui; cargo build --locked -p coh -p hive-gateway; python3 scripts/install/stage_swarmui.py --repo "$PWD" --generated-root "$PWD" --bin-dir "$PWD/target/debug" --profile macos-desktop --apple-extension-dir "$PWD/out/m28c/DerivedData/Build/Products/Release/SwarmUIIntents.appex" --out "$PWD/out/m28c/canonical-stage-02"; codesign --verify --deep --strict out/m28c/SwarmUI-v4.app; pluginkit -m -v -i com.cohesix.swarmui.intents.
Checks: Public SDK builds, focused native contract tests, local capability probes, provisioned app/extension signing, installed four-action Shortcuts discovery and one local support action execution pass. The source-bound m28c-platform-live runner passes. Later diagnostic Developer ID notarisation and provisioned Keychain sharing pass independently. The new in-app enrollment path, Siri invocation and final journey distribution remain outstanding.
Deliverables: Source feasibility probe and this measured support/blocker record; no M28c completion claim.
```

| Observation | Result | Proof limit |
| --- | --- | --- |
| Host | macOS 27.0 build `26A428`, Xcode 27.0 build `27A266a`, macOS SDK 27.0; Apple M4 with 24 GiB; locale `en_AU` | One Mac only. |
| Foundation Models | `SystemLanguageModel.default` was available for `en_AU`. A synthetic, scoped `uncertain/pending` fixture produced an on-device typed explanation and an inspect proposal. | The synthetic fixture is not a live Cohesix run or a signed outcome. |
| Metal | MLX 0.32.2 reported default `Device(gpu, 0)`, an available Apple M4 GPU and a 19,069,665,280-byte recommended working set. An explicit 512×512 GPU multiplication completed; MLX-LM 0.31.3 performed local inference. | One Mac and model only; the detector is not a hard partition or an admitted provider receipt. |
| Detached LoRA | Pinned `mlx-community/SmolLM2-135M-Instruct` revision `422de227b90002f443a21a58b1087f6ee7632731` trained four LoRA steps on a local smoke dataset, saved two checkpoints and a final adapter, then evaluated test loss 6.853 and perplexity 946.921. | Genuine local training, but poor small-fixture quality and no independent 28b comparison, serve/canary/rollback or Cohesix admission. |
| Installed vMLX | `/Applications/vMLX.app` 1.6.65 (`net.vmlx.app`, Team `55KGF2S5AY`) includes engine commit `22f9c77711fb580df32f4d40bbaea989c2d5421b`. Its app gateway at loopback port 8080 reported zero backends. A separate bundled-engine process loaded the scratch SmolLM2 model on port 18080; `/health` reported healthy, and a bounded `/v1/chat/completions` call returned HTTP 200, selected model `cohesix-smollm2-135m`, 38 prompt and 24 completion tokens. The response did not satisfy the requested exact phrase. | Real local transport/inference observation only. No vMLX adapter canary, verified generation, Cohesix ticket, held-out quality or fallback result. The diagnostic server was stopped after the test. |
| Cohesix vMLX client | `tools/cohesix-py/cohesix/vmlx_compat.py` made a second actual local call against a separate content-bound model copy on port 18081: selected model, `stop` finish, 37 prompt tokens, 10 completion tokens, nonempty result. It emitted only prompt/output digests in its evidence view. Five focused refusal/response tests passed. The copied model SHA-256 remained `61840936148403ac34b1dd9a8e7eb71eade5d898587b158914c65e7b36113f60`, and the diagnostic server was stopped. | Client transport and bounded parsing are observed; no Cohesix admission, signed result, adapter serving, fixed held-out quality or rollback has been tested. |
| vMLX model mutation | On first load, the engine repaired 272 misaligned tensor records in the ignored local `model.safetensors` copy. Its SHA-256 changed from `989e3ef746b41999ca096055a60c87057b8e842824253391bc0d9ed8dfc6936e` to `61840936148403ac34b1dd9a8e7eb71eade5d898587b158914c65e7b36113f60`; the engine reported retaining original remote metadata separately. | This changed the scratch artifact and invalidated byte identity for that file. Future compatibility tests must start from a separate content-bound copy and record any rewrite before a Cohesix comparison. |
| Native Swift checks | `swift test` passed eight focused capability, gateway request, response-bound and proposed-action checks. | Pure contract only; fake HTTP responses do not accept a gateway. |
| App Intents extension | Xcode Release extension build passed after the data-protection Keychain update; SwarmUI, `coh` and Hive Gateway binaries built with Cargo. Extracted metadata marks probe, inspect, cancel and explain discoverable. | Compilation and metadata are not installed execution or Siri operation. |
| Canonical input staging | `stage_swarmui.py` copied the compiler-registered macOS desktop file set, built extension metadata and 43 offline UI assets into ignored `out/m28c/canonical-stage-02`. | Staging is not an installed signed package or live action. The first attempt lacked `coh`; building the selected host binaries resolved that input. |
| Local package | Extension and app passed `codesign --verify --deep --strict` with the local Apple Development identity. `pluginkit` lists `com.cohesix.swarmui.intents` from the installed app, and the Shortcuts action search shows “Check Cohesix Apple Support”. | This establishes action discovery on this Mac; no shortcut or spoken invocation was run. The diagnostic bundle was assembled outside the canonical package pipeline. |
| Extension sandbox | `com.apple.security.app-sandbox`, `com.apple.security.network.client` and a shared Keychain group are on the diagnostic extension signature. An earlier bundle without the sandbox entitlement was not listed by `pluginkit`. | The sandbox difference is the observed registration fix. A valid signature and `pluginkit` listing do not show that the shared Keychain code can run. |
| Keychain launch | The manually development-signed enrollment helper with a shared access-group entitlement passed `codesign --verify` but exited 137 before its own argument check. The same helper signed without the restricted entitlement ran and returned its expected missing-argument error. | The controlled difference points to missing provisioning for the restricted entitlement. No real token was entered or stored. |
| Provisioned Keychain sharing | A later disposable Swift probe used the exact app and extension development profiles and their selected entitlements. Both signatures passed strict verification. The app identity wrote a random item through the default Keychain group, the extension identity read the same bytes through its default group, and the app deleted the item; all three operations exited zero. Ignored probe source SHA-256: `8e67315f90cb3da14b1e2593ef239b890411f00c2ebc21a47fce9edf284b6d8c`. | The test proves the provisioned cross-identity Keychain boundary on this Mac. It did not invoke SwarmUI's enrollment command, the real extension's `GatewayKeychain.load`, or a delegated gateway job. The probe contained no live credential. |
| Distribution | The chosen channel is direct distribution outside the Mac App Store. Xcode issued exact macOS development profiles for both App IDs. The canonical staged app and extension then passed nested development signing with shared Keychain entitlements and `codesign --verify --deep --strict`; the app executable launches with `--help`. On 25 September, Xcode created a valid `Developer ID Application: Lukas Bower (KB88FQXUX2)` signing identity, SHA-1 `EEF1BF26D2081CEB18AF1F5B0C4A6D720C1F2B98`. Apple issued exact Developer ID profiles for both bundle IDs; fresh staging `canonical-stage-06` passed nested Developer ID signing with hardened runtime and secure timestamp. `notarytool` accepted submission `6b4fdfe9-bbe2-4162-af79-239e3d2d5f2f`; stapler validation and Gatekeeper assessment passed. | This is a diagnostic copy, not the final installed developer journey. [Apple's direct-distribution requirements](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) require final candidate notarisation. The Keychain credential is outside the repository. |
| Apple App IDs | The Apple Developer team `KB88FQXUX2` lists explicit `com.cohesix.swarmui` and `com.cohesix.swarmui.intents` App IDs. | Registration alone does not provision or sign either executable. |
| macOS capabilities | The extension registration screen showed In-App Purchase on by default and no other capability selected. It had no Keychain Sharing checkbox. SwarmUI and the extension declare the same Keychain access group in their signing entitlements. Apple's current [macOS guidance](https://developer.apple.com/design/human-interface-guidelines/app-shortcuts) supports App Intents actions inside user-created Shortcuts, not automatic App Shortcuts; [SiriKit capability guidance](https://developer.apple.com/documentation/xcode/configuring-siri-support) excludes macOS. The unused `AppShortcutsProvider` was removed from the macOS extension source. | A user-created Shortcut still needs a live Mac execution and spoken Siri invocation before any M28c action acceptance. No optional portal capability was selected for the extension. |
| Installed action | The old diagnostic app was closed. The provisioned build at `/Users/lukasbower/Applications/SwarmUI-M28c-Provisioned.app` launched, and `pluginkit` persistently listed its extension. After restarting Shortcuts, all four source actions appeared once. The support action executed and returned `Apple assistance: available; local Metal: available. No hive action was requested.` | Actual platform invocation is observed; no authenticated job action, Siri invocation or enrolled hive connection has been tested. |

The signer accepts an Apple profile that authorizes the selected Keychain group
either exactly or through the team's `TEAMID.*` wildcard, while still requiring
the exact macOS application identifier. Apple's [provisioning profile
contract](https://developer.apple.com/documentation/technotes/tn3125-inside-code-signing-provisioning-profiles)
allows the team wildcard to authorize a specific group. The focused signer
fixture passes; an Apple-issued SwarmUI profile has not yet been tested.
The signer also checks that the selected signing certificate is listed in each
profile before changing the staged app. Xcode's extension profile UUID is
`da62f7cf-32f5-4bae-be2b-4435a0963277`; the parent profile UUID is
`fcb38246-504e-40ac-9348-cc46bf782f63`. Both authorize the exact bundle ID
and `KB88FQXUX2.*` Keychain group, and include the selected development
certificate. After removing the unsupported macOS `AppShortcutsProvider`, eight
focused Swift tests, the unsigned Release extension build and
`scripts/check-generated.sh` passed. The provisioned extension build and
canonical development signer passed after fixing a `codesign` diagnostic-stream
parser defect; eight focused signer/platform tests passed. Installed action
execution and the real SwarmUI enrollment path still require observation.

`coh-rtc` regenerated the canonical host/package/source projections after the
new files were staged, and `scripts/check-generated.sh` passed. The final
source and documentation set must pass it again; this probe is not a clean,
immutable merge candidate yet.

The public [App Intents contract](https://developer.apple.com/documentation/appintents/appintent)
supports extension-defined discoverable actions. The public
[Foundation Models availability contract](https://developer.apple.com/documentation/foundationmodels/systemlanguagemodel)
requires an actual availability check before use. The probe uses those public
APIs and the Xcode 27 App Intents extension template's
`com.apple.appintents-extension` point. Inspect and cancel use the selected
Gateway's existing delegated job endpoints; the model may propose but cannot
execute an action.

The `m28c-platform-live` summary is retained at ignored
`out/m28c/platform-live-provisioned/summary.json` with SHA-256
`ac04fa3ee47a55c99b0c11c180ed0f8340d570cfa330f16944055a8e09961cdf`.
It binds app binary SHA-256
`b21dc15e25b3384a93a3db86a63e88f2c126c5bc5f190cdceb2e26eb85e6d0b8`,
extension binary SHA-256
`f1bc7f19de19336c9171ef8295596266435ed42ff3a0782b16a8a94db31617a5`,
team `KB88FQXUX2`, and the four-action Shortcuts search. The separate
Shortcuts probe-result accessibility capture has SHA-256
`98dd311934f627010621189f2dd942c768a498252513db7d3612b4ec5a5b416a`.
Actual SwarmUI enrollment and authenticated job actions are dependent
implementation work, not a claim from this feasibility probe. The later
Developer ID notarisation is a separate diagnostic distribution observation.
The fresh Developer ID signing report is retained at ignored
`out/m28c/canonical-stage-06/developer-id-signing/summary.json`. Its app and
extension profile UUIDs are `de5bbf6c-f387-48e0-810a-fec100cff52c` and
`78b00f0b-888f-4aea-a7c9-db6d149b8834`; both authorize the selected
certificate and shared group. The separate notarisation report is retained at
`out/m28c/canonical-stage-06/notarization/summary.json`. It records accepted
submission `6b4fdfe9-bbe2-4162-af79-239e3d2d5f2f` and submitted archive
SHA-256 `65dcfcd210844cb4d9e9f33de84d6586e17698a8664ca7e4cf435a588cd84011`.
The signer verified the nested signatures, Apple's acceptance log, stapling
and Gatekeeper assessment. These reports establish diagnostic distribution
feasibility, not an accepted installed workflow or M28c completion.
Dependent `m28c-native-apple-actions`, `m28c-mlx-metal-provider`,
`m28c-vmlx-compatibility` and `m28c-foundation-models-assistance` remain
outside acceptance. The host-tool, Python and benchmark
compatibility review finds no changed shared operation yet: the Swift
projection calls existing Gateway job endpoints, while the Python vMLX client
uses a separately selected loopback server without target or ticket authority.
`coh`, `cohsh`, Hive Gateway and benchmark operation contracts are unchanged.
The package inventory and generated host graph were regenerated for this
source set and must be checked again after any later edits.

Material AI assistance contributed the Swift probe, gateway projection, focused checks and this
record. The local bundle under `out/m28c` is diagnostic, not a distributable
artifact or retained release evidence.
