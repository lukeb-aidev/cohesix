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
- The compiler-selected host archives now include a version-aligned Cohesix
  Python wheel and an exact-source NeMo wheel, lock, installer and digest report.
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

The full Clippy baseline fails on untouched source under the installed Rust
toolchain, including `standing_ledger.rs` large enum, `gpu-bridge-host`
argument count and `coh-rtc` argument count and nested condition warnings.
These are baseline failures, not an M28g test waiver. No lint suppression was
retained in this candidate.

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

M28g also still requires frozen adoption and quality budgets, independently
evaluated clean installation, the four effective protocol modes, native CUDA,
PEFT, Apple and NeMo journeys from installed bytes, full applicable QEMU and
physical Pi evidence, and assembled release due diligence. These gates must
bind one final source and package identity before the milestone can be marked
Complete. Material AI assistance produced this implementation checkpoint;
every claim above is limited to the executed source and host checks.
