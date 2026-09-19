<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Author: Lukas Bower -->
<!-- Purpose: Record the scope, compatibility review and validation limits of operator manual maintenance. -->
# Operator manual maintenance record

Task: `m27b-operator-manuals-and-reference-maintenance`, Milestone 27b — Host
Integration Registry + Provider/Executor + Use-Case Conformance.

## Scope and provenance

Work was performed on `main` with the ongoing M27b changes retained. The two
operator guides were fetched from `origin/main` at `0139e855a` and overwritten
from that revision before integrating current M27b usage by topic. Original
local versions are retained under `out/help-cleanup/before/`. No branch switch,
reset, staging, commit, release modification or target boot was performed.

The shared embedded source is `apps/cohsh/src/manual.md`: 29 topics covering
all interactive cohsh commands, namespace navigation, production authority and
scripts. `login` resolves to `attach`. `man` and `cohsh --man` resolve locally;
invalid requests fail without transport initialization. SwarmUI uses that same
source with its own capability notice, help index and raw-JSON spawn syntax.
Root help distinguishes bootstrap diagnostics from event-pump session commands;
serial, mirrored and atomic output use the same bounded lists.

Architecture, security, interface, REST/API, GPU, Python and benchmark references
organize authority and provider material by subject. Historical qualification
records retain their authority. The charter requires integrated current usage
and aligned help in every relevant implementation change.

## Compatibility review

- `cohsh`: additive local manual dispatch and offline CLI option; existing
  target commands, aliases, authority checks and transport framing are retained.
- SwarmUI: both console backends render shared manuals locally, including in
  offline mode. Its help and UI test fixture agree; write gates are retained.
- Root shell: help text and list composition only. No parser, protocol,
  namespace, scheduling, device, authentication or response-terminal change.
- `coh`, `coh-status`, Hive Gateway, GPU bridge, host-ticket-agent,
  host-sidecar-bridge, sidecar-bus, cas-tool and console-ack-wire: reviewed as
  consumers of existing commands/records; no implementation changes needed.
- `tools/cohesix-py`, playbooks, `.coh` scripts, raw/REST benchmarks and Worker
  pressure scripts: existing wire, authority, data and measurement contracts are
  unchanged. No fixture, workload, threshold or report-schema adjustment needed.
- Generated console grammar remains unchanged: `man` belongs to host clients,
  not the target wire protocol. Operator-guide generated blocks were refreshed
  through `coh-rtc --embed-userland-doc`, with compiler outputs isolated in a
  temporary directory.

## Validation

Passed:

```sh
cargo test -p cohsh -p cohsh-core -p swarmui --lib
cargo test -p cohsh --test manual_cli
cargo test -p cohsh-core --test doc_snippets
cargo test -p coh-rtc --test swarmui_docs --test observability_docs \
  --test cas_docs --test cbor_docs --test gpu_docs
cargo fmt --all -- --check
cargo clippy -p cohsh -p cohsh-core -p swarmui --all-targets -- -D warnings
```

These runs passed 147 tests in total. Coverage includes command-manual inventory,
complete spawn example payloads, aliases and invalid topics, no-attachment shell
manuals, offline CLI lookup, both SwarmUI backends, UI help fixture parity, root
help coverage and compiler-owned documentation blocks. Edited reference links
resolve locally, code fences balance, and scoped `git diff --check` passes.

The shared root-help consumer compiled for the QEMU production AArch64 profile:

```sh
SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production" \
COHESIX_WORKER_IMAGE_MANIFEST="$PWD/out/m27b/qemu-worker-evidence-11/artifact/worker-images/cohesix-worker-image-manifest.json" \
COHESIX_WORKER_IMAGE_ARCHIVE="$PWD/out/m27b/qemu-worker-evidence-11/artifact/worker-images/cohesix-worker-images.cpio" \
COHESIX_NINEDOOR_RUNTIME_IMAGE="$PWD/out/m27b/qemu-worker-evidence-11/artifact/staging/cohesix/bin/nine-door-runtime" \
COHESIX_CONSOLE_NETWORK_RUNTIME_IMAGE="$PWD/out/m27b/qemu-worker-evidence-11/artifact/staging/cohesix/artifacts/console-network-runtime" \
cargo check -p root-task --target aarch64-unknown-none \
  --no-default-features --features release-qemu
```

This is compilation evidence, not new QEMU execution, Pi compilation/boot,
physical help rendering, performance or milestone acceptance.

The shared checkout still fails these repository consistency checks:

```sh
scripts/check-generated.sh
scripts/ci/check_test_plan.sh
```

The generated check finds a stale `configs/generated/cohsh_policy.toml`
manifest fingerprint (`7e8839fc...` versus resolved manifest `73acb7d0...`).
The Test Plan catalog and catalog documentation pass, but recorded hashes for
`configs/root_task.toml`, its resolved manifest and the Pi source manifest do
not match the ongoing M27b state. This cleanup does not modify those source
manifests, generated policy files or historical Test Plan qualification hashes.
Their owner must regenerate the selected coherent artifact set and reconcile
provenance under the normal M27b workflow before merge. No acceptance checks
were weakened or historical evidence relabelled to obtain a pass.

Logs and the pre-edit copies are under `out/help-cleanup/`. Full workspace
Clippy/check/test/audit/deny and human review remain pre-merge requirements;
this record does not claim release or M27b closure.

## Publication boundary

The cleanup commit is based on GitHub `0139e855a`. Its CLI change is limited to
manual lookup; concurrent provider-registry, transport-selection and delegated
read changes remain in the main working directory. References are reorganized
against that published baseline, with the broader M27b reference integration
retained locally beside its uncommitted implementation. Compiler-owned guide
blocks in the commit retain the published profile. The 147-test record above
describes the shared working state; publication checks are recorded separately.

Publication validation passed 132 library tests, two offline CLI tests, workspace
formatting, `scripts/check-generated.sh`, and `scripts/ci/check_test_plan.sh`.
The compiler regenerated the implementation inventory and its dependent graph
to include this audit record. The refreshed guide preserves explicit ACK-before-
payload ordering and bounded reason tags. Local documentation links resolve.
The shared M27b failures above remain historical observations of that separate
working state, not failures of this isolated publication tree.
