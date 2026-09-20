<!-- Author: Lukas Bower -->
<!-- Purpose: Define compiler-selected macOS service control and native process evidence without conflating controller and GPU execution. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# macOS native providers

These host adapters were implemented under the original broad 27b scope and are
retained under the roadmap's `m28a-native-provider-discovery-and-actions` owner.
A Mac controller controls an explicitly named target hive. CUDA remains
on its selected remote NVIDIA host; choosing a Mac controller never enables
local NVML or creates a local Apple GPU executor.

## launchd service lifecycle

Manifest schema 1.25 adds `launchd.start`, `launchd.stop`, `launchd.restart` and
`launchd.status-check` to the selectable host-ticket action vocabulary. The
selected root manifest must explicitly allow an action. The canonical QEMU and
Pi manifests allow all four lifecycle actions; execution and discovery still
require an exact enrolled service. Configure each native
service in the host integration source, then regenerate all artifacts:

```toml
[[providers.launchd_targets]]
id = "owned-agent"
domain = "gui/501"
label = "org.example.owned-agent"
plist = "/absolute/operator/selected/owned-agent.plist"
plist_sha256 = "<actual SHA-256 of the selected plist>"
executable_sha256 = "<actual SHA-256 of the selected native executable>"
```

These are deployment inputs, not working credentials or a ready-to-install
profile. `system`, `user/<uid>` and `gui/<uid>` are the only domains. Labels,
paths, digests and unique ids are validated by the compiler. Empty maps mean
`not_enabled`; environment label lists no longer select live discovery. The
SDK, gateway metadata and agent consume the same generated action/map contract.

Submit an authorized version-1 host ticket with `target: "owned-agent"` and
`args: {"service": "owned-agent"}`. Native labels, arbitrary commands, paths,
bootstrap domains and signal names are not request arguments. Version-2 Worker
receipts are not supported for launchd. Service management requires the account's
native launchd permission as well as Cohesix admission; administrator access
outside Cohesix remains an explicit bypass, not authority that this adapter removes.

Install the compiled `scripts/providers/process_macos.swift` helper and set
`COHESIX_MACOS_PROCESS_HELPER` to its absolute path and
`COHESIX_MACOS_PROCESS_HELPER_SHA256` to its actual SHA-256. The helper uses
`libproc` for PID, owner and birth time, hashes the executable file on its native
path, and rechecks process incarnation. It exports neither argv nor environment.
The helper is independently measured before use. These are native host
observations, not device attestations or a proof against a compromised host.

The adapter checks the plist label/hash and executable hash before dispatch.
Read-only observation combines bounded `launchctl print` state with kernel
process identity. Start uses `kickstart`, restart uses `kickstart -k`, and stop
uses a fixed `SIGTERM` through the exact launchd service target. Start/restart
require a running native process with a new invocation when appropriate. Stop
requires no current service PID and native absence of the prior incarnation.
Command exit alone is not terminal evidence. KeepAlive, socket-activated and
Mach-activated jobs are refused for lifecycle changes because independent
activation would make the requested stop or invocation ambiguous.

The agent retains before/after native objects in its bounded evidence store.
With separate enrolled signing keys, the gateway/native chain uses the shared
causal verifier. The durable version-1 ticket journal prevents redispatch after
an uncertain restart; an ambiguous result remains failed/unverified and requires
an independently admitted recovery operation. A status check cannot retroactively
prove that a prior command caused an observed state.

## Focused native reference

```sh
scripts/ci/provider_conformance_run.sh --provider launchd --live-reference \
  --state-dir out/provider-conformance/launchd-native
```

This macOS-only lane compiles the helper using the selected Xcode SDK, creates one
private temporary job running `/bin/sleep`, verifies an executable mismatch is
refused before dispatch, then proves start/restart/stop process postconditions.
It removes its own job in a `finally` cleanup and retains exact helper, plist,
executable and process identities. It does not modify existing services. Its
native reference proof is separate from Root admission, Worker execution,
release packaging and use-case qualification. `--provider launchd` without
`--live-reference` selects only the small map, postcondition and SDK tests.

## Xcode, release and endpoint targets

Schema 1.26 adds optional `providers.macos_targets` entries. Each entry has a
unique `id` and an `operation` table with one exact `action`. Tickets select
only `args: {"target_id": "<compiled id>"}` and the same version-1 target id.
The default map selects only `local-endpoint-compliance`, with the read-only
`endpoint_compliance.observe` action enabled in both canonical target manifests.
It observes the local publishing/executing Mac. An empty map is `not_enabled`;
Xcode, signing, notarization and upload require explicit deployment enrollment
and action selection.

`mac_release.build`, `.test` and `.archive` pin a source root/tree digest,
relative `.xcodeproj` and scheme. The adapter verifies and copies the source
into private attempt storage, runs the fixed Xcode operation, and retains
structured xcresult and output digests. Tests require positive executed-test
counts with no failures or skips. Native summaries withhold device identifiers,
test names and paths. `mac_release.codesign` pins an artifact and certificate
SHA-1 identity, signs a private copy with hardened runtime, and independently
checks its signature against that certificate requirement.

`mac_release.notarize` uses a host keychain profile and requires an accepted
Apple job/log matching the submitted artifact. `mac_release.upload` selects
an app/version and protected host private-key/JWT references; it requires the
native delivery id, completed App Store upload, valid linked build and matching
SHA-256 file checksum. Submission alone never means verification. Missing
credentials, unknown native response shapes and uncorrelated jobs fail closed.
`endpoint_compliance.observe` reads fixed FileVault, Gatekeeper and SIP status;
it changes no setting and neither attests the device nor certifies compliance.

The agent records native evidence under its durable attempt identity; replay
cannot create another native operation. Root integration, Apple credentialed
live qualification and package enrollment for these adapters remain open under
28a. Focused native Xcode build/test/archive passed on the owned one-test fixture;
signing, notarization and upload are not claimed as live-qualified.

```sh
scripts/ci/provider_conformance_run.sh --provider mac_release --live-reference \
  --state-dir out/provider-conformance/macos-release-native
```

This requires Xcode and `xcodegen`; it creates a private reference project and
retains native results. It is separate from Root admission and cloud release proof.
