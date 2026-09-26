<!-- Author: Lukas Bower -->
<!-- Purpose: Retain the M28g implementation checkpoint and its distinct source, installer and release proof limits. -->
<!-- Copyright 2026 Lukas Bower -->

# M28g implementation record — in progress

This record describes an unqualified 1.2.0-beta source candidate rebased onto
GitHub `main` at `2d8ce6a9f5627b0bb16ffd6c4c9b0619f1d2c749`. It has no sealed final source
commit, native installer artifact, assembled target identity or Release B
acceptance. The 1.1.0-beta release record remains at its original scope.

## Task record

Title/ID: `m28g-macos-native-installer`, `m28g-ubuntu-arm64-native-installer`,
`m28g-host-clients-as-built-alignment`, `m28g-installation-and-integrated-adoption`,
`m28g-release-b-qualification`.

Milestone: 28g — Installation, Integrated User Qualification and Release B.

Goal: Build one version-bound host candidate and qualify native installation,
cross-client operation and the assembled release on every selected profile.

Inputs: M28–M28f source and component records; selected compiler inventory and
host integration matrix; 1.2.0-beta release source; local Mac, Linux ARM64 and
Pi 4 test-bed descriptions. Private signing identities, publisher trust and
target credentials remain outside this record.

Changes:

- Native Mac and Ubuntu ARM64 package builders stage only an independently
  pinned archive, keep guest artifacts optional, and publish output only after
  package signing or notarization succeeds. Their staged code permissions allow
  ordinary users to read and launch installed tools, with the copied Mac app
  signature rechecked after permission normalization.
- The compiler-selected host archives now include Cohesix Python and NeMo kit
  wheels at `1.2.0b0`, the PEP 440 spelling of Release B `1.2.0-beta`, plus
  the NeMo lock, installer and digest report. Both wheel versions must match
  the selected release before an installer stages them.
- SwarmUI resolves installed tools from the fixed OS location and stages a
  native Mac icon; app and extension version metadata identify 1.2.0.
- `coh doctor` reports the selected MCP/A2A switches without claiming a live
  endpoint probe. SwarmUI help fixtures follow the implemented console grammar.
- Public candidate, package and Python instructions identify 1.1.0-beta as
  the published release and keep 1.2.0-beta qualification explicit.

Commands: `scripts/check-generated.sh`; focused installer, release, Python
and NeMo pytest files; `cargo fmt --all -- --check`;
`cargo check --workspace`; `cargo test --workspace`;
`cargo clippy --workspace --all-targets -- -D warnings`; `cargo audit`;
`cargo deny check advisories`; `scripts/ci/test_plan_run.sh --list`.

Checks: Generated inventory, selected wheel bytes, source/manifest membership,
installer path and byte bounds, failed-signing output isolation, Rust and
Python contract tests, and exact proof-class separation. Package staging and
synthetic tests cannot establish installed-file, desktop, target or release
acceptance.

Deliverables: Source-level candidate and focused test logs retained in ignored
`out/m28g/`. There is no passing native installer qualification, live adoption
case, physical Pi result, integrated Test Plan run or release-owner approval.

## Current validation and blockers

The focused Python installer, release bundle, release qualification, package
and provider-matrix tests passed 96 cases after rebasing; generated consistency,
Test Plan catalog, Rust formatting, workspace check and tests, audit and
advisory checks passed on the rebased candidate. Focused
`coh` doctor and SwarmUI help tests passed after correcting stale selected
contract assertions. The rebased Rust suite output is retained in ignored
`out/m28g/cargo-test-rebased.log`. No exact target or installed-package claim
follows from it.

The release-version wheel alignment passed 47 focused NeMo, Python, package
and bundle tests. `scripts/check-generated.sh` passed after regenerating the
compiler inventory, and both built wheel METADATA records report `1.2.0b0`.
The fresh QEMU seL4 production profile in this isolated checkout passed
release-mode source/artifact validation. A QEMU image built at predecessor
commit `616d34b86eab` booted four cores, reached the serial console and
answered `ping` with `PONG`. The rootserver's `[BUILD]` marker matched that
commit. This is boot liveness only, without authenticated TCP, live job or
final-source release evidence.

The installed Rust toolchain exposed Clippy warnings in the rebased source.
The candidate now repairs the large ledger enum and argument grouping without
changing admission, job or compiler semantics; it also removes narrow style
warnings. Full `cargo clippy --workspace --all-targets -- -D warnings` passed,
along with focused ledger, gateway, GPU child and compiler tests. No lint
suppression was added.

The first Stage 01 retry passed generated consistency, formatting, Clippy,
workspace check/tests, SwarmUI, `coh`, QEMU and Pi feature tests, Pi runtime
tests and the AArch64 target check. Its Python action failed 35 cases: 33
needed the isolated checkout's repository `.venv`, one exposed the existing
QEMU/Pi gateway-control difference in the parity test, and one found the
wheel inspector's stale optional-extra list. After provisioning the pinned
`.venv` and repairing those test contracts, the focused 85-case set passed.
The next Stage 01 attempt passed every preceding action and reached 2,913
passing Python tests; three release-bundle fixture cases still used the old
wheel metadata. The fixture now describes the selected `1.2.0b0` wheel and
the affected nine focused tests passed. On clean commit `bf0fd4bda`, Stage
01 passed completely, including 2,916 Python tests, 116 Python subtests and
the Rust risk ratchet. Provisioned QEMU Stage 02 passed against the same
source digest after the Test Plan correctly required a fresh attestation for
the selected private Queen credential and manifest copies.

QEMU Stage 03 then built separate base/gated exact-source images and ran all
19 authenticated TCP regression scripts successfully. Its aggregate and both
artifact verifiers passed, but the stage refused final PASS when generated
MCP and A2A catalogues differed from their initial contents. The batch
runner's generated-output snapshot inventory omitted those two newer files.
The inventory now includes both catalogues, and all 21 focused batch-wrapper
tests plus `scripts/check-generated.sh` pass after restoring the committed
generated outputs. The 19-script attempt remains a retained transport result,
not accepted Stage 03 evidence; the corrected source requires fresh stages.

The Mac test bed lacks a Developer ID Installer identity; a signed, notarized
`.pkg` and clean Finder/Spotlight/Dock lifecycle observations cannot yet be
produced. The available Linux ARM64 hosts have no selected publisher signing
key or public Debian maintainer identity. Ubuntu 22.04 package candidates were
observed in a Jammy chroot and Ubuntu 24.04 packages on a native host; neither
is a clean signed `.deb` installation. No Ubuntu 26.04 ARM64 installation host
has been selected. The selected Python and NeMo wheels are source candidates;
there is no final exact 1.2 archive or installed cross-client parity report.

The source candidate now has an `installer` qualifier for independently trusted
publisher signatures, package-manager receipts and installed byte readback. It
has not run on a signed package or installed host and does not cover GUI launch
or package lifecycle. The Release B archive verifier now requires both native
installer results at the same source and archive hashes. The conformance matrix still has no
`m28g-adoption-live` or `m28g-integration-live` case runner. The required
capability-by-capability installed-client parity matrix and frozen numeric
adoption, quality, recovery and control-latency budgets have not been written.
These source and evidence gaps remain even after signing identities and a
Ubuntu 26.04 test host become available.

The selected QEMU manifest enables the host MCP/A2A protocol ceiling while
the selected Pi manifest disables it. The exact values are now guarded by the
production-profile test and documented as a target difference. Protocol
composition on a Pi-selected gateway cannot be inferred from QEMU checks;
the required M28g live matrix must identify its actual selected gateway
profile and cannot promote a disabled Pi surface.

M28g also still requires frozen adoption and quality budgets, independently
evaluated clean installation, the four effective protocol modes, native CUDA,
PEFT, Apple and NeMo journeys from installed bytes, full applicable QEMU and
physical Pi evidence, and assembled release due diligence. These gates must
bind one final source and package identity before the milestone can be marked
Complete. Material AI assistance produced this implementation checkpoint;
every claim above is limited to the executed source and host checks.
