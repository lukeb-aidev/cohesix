<!-- Author: Lukas Bower -->
<!-- Purpose: Retain scoped M27b implementation observations and unresolved work without promoting diagnostics to milestone acceptance. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# M27b implementation record

Status: **Complete** for the selected Executable Host Foundation on 19 September
2026. The final closure section supersedes historical progress notes below;
those notes preserve their original scope and evidence identity. The owner
requested every specified task, selected the installed CUDA 13.2.2 reference,
removed milestone activation gates, and requested focused testing rather than
the full suite. No broader performance, target or use-case acceptance is inferred.

```text
Title/ID: m27b-native-workload-and-package-convergence
Milestone: 27b / m27b-gpu-workload-and-mig-executor,
  m27b-external-executor-conformance, m27b-packaging-deployment-profiles
Goal: Bind root-admitted CUDA work to native execution and Worker receipts,
  and preserve canonical host filesystem and package behavior.
Inputs: production QEMU authority profile at writer epoch 7; Merlin Linux
  AArch64 CUDA 13.2.2, driver/runtime 13020, Orin compute capability 8.7.
Changes:
  - GPU bridge and host-ticket-agent — exact published UUID/topology/helper,
    Worker reservation checks, bounded execution and matching control lane.
  - root-task NineDoor — one matching pending workload control request;
    canonical Worker directory listing and bounded CAT over the retained ring.
  - coh mount — acknowledged append offsets and macFUSE published-EOF placement.
  - compiler release inventory — explicit SDK product sources, excluding tests.
  - scripts/install/build_python_package.py — isolated wheel/sdist build,
    exact archive contents, source digests, types and wheel RECORD verification.
Commands: See the focused command list below.
Checks: Native first-kernel and terminal state; exact root/Worker identity;
  canonical shard bytes and write refusals on four mounted paths; clean
  distribution inventory and supported-interpreter installation.
Deliverables: Source changes, retained local evidence, and as-built docs.
```

The live GPU records at `out/m27b/live-session-02` correlate root admission on
the provisioned QEMU image with Merlin execution and the executable Worker.
`gpu-worker-result-02.json` records lease grant, vector-add and matrix-multiply.
`gpu-worker-cancel-result-01.json` records a CUDA child paused after its first
completed kernel, followed by a matching admitted cancel: the submit is rejected
and the cancel confirmed, with completed Worker receipt sequences. The native
state copy is `out/m27b/native-live-02`. Fixture-admission IPC tests remain
separate observations and are not substituted for these root/Worker records.
The signed causal evidence producer chain is still outstanding.

`out/m27b/fuse-parity.json` binds the four mounted macOS/Linux direct/REST lanes
to `out/m27b/qemu-production-03`. Each reads the same 288-byte canonical Worker
telemetry window, SHA-256
`84dd0960b70fe055c91b4ca6a70b8d1b2b5ea28f1397b62150bdd7d20c941553`.
Each acknowledges a valid finite-lifetime host-ticket append, rejects an invalid
schema and a path outside the mount allowlist, and unmounts successfully. The
append is explicitly not an external provider execution claim. The gateway
releases its sole TCP console connection before direct mounts; all owned
session processes are retired afterward.

The original FUSE failure is retained: root listed the shard and Worker but
neither listed the Worker directory nor supported CAT on its telemetry leaf.
Both now reuse existing bounded telemetry state. A separate native macFUSE
probe found that `O_APPEND` is absent from OPEN and WRITE; the host fix accepts
only sequential offsets or the exact EOF last reported to that kernel. The
probe is under ignored `out/`; it is not a shipped integration surface.

`out/m27b/python-distributions-02/distributions.json` records a target-neutral
wheel and sdist built from the compiler's explicit SDK source list in a private
temporary directory. The wheel SHA-256 is
`344ce6c5b48f20f532eee467e37a0538502d97adb045dbc64ffe39a331d2bec6`.
`out/m27b/python-package-smoke-01` records clean installation on CPython 3.11
and 3.13. This package inspection is not release signing or target proof.

Focused commands run for these changes:

```sh
SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production" \
  cargo test -p root-task --lib --no-default-features \
  --features driver-tests-qemu,mock-sel4,test-support \
  ninedoor::tests::host_ticket_workload_admits -- --nocapture
SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-production" \
  cargo test -p root-task --lib --no-default-features \
  --features driver-tests-qemu,mock-sel4,test-support \
  ninedoor::tests::worker_directory_and_cat -- --nocapture
cargo test -p coh --features fuse --test mount
cargo build -p coh --features fuse
.venv/bin/python -m pytest -q tests/test_python_package.py
.venv/bin/python scripts/install/build_python_package.py \
  --out out/m27b/python-distributions-02
scripts/ci/python_compat_run.sh --wheel-smoke \
  --wheel-dir out/m27b/python-distributions-02 \
  --package-manifest out/m27b/python-distributions-02/python-package.json \
  --state-dir out/m27b/python-package-smoke-01
```

The production QEMU build commands and exact source/image/manifest observations
are retained under `out/m27b/qemu-production-02` and `qemu-production-03`.
Merlin source copies `m27b-host-source-16` and `m27b-host-source-17` are separately
retained; the latter compiles `coh` for `aarch64-unknown-linux-gnu`. Focused host
checks and live convergence evidence do not replace the unimplemented tasks.

Still required: complete signed authority/provider/Worker producers, live native
snapshot publication, capable-host MIG and macOS provider actions, generated
nine-playbook DAGs and public claim matrix, container/Kubernetes deployment
assets and release integration, full selected conformance matrix, live SIEM/federation
acknowledgement recovery, MODBUS/DNP3 restoration under reopened M18, and targeted
provider/exporter timing. The complete host-tool, SDK and benchmark compatibility
review and final generated/documentation checks remain open.

Signed host packaging implementation (15 September): the compiler declares
four bounded deployment profiles. `coh package` validates Ed25519 signatures
against external trust, exact file inventories, executable entry/segment and
architecture bounds, selected configuration hashes, production authority,
secret references and the CycloneDX file SBOM. The installer publishes a fresh
verified snapshot, and the service renderer consumes signed templates without
activating them. `coh doctor --package` reports credential resolution separately
from unobserved service health.

Focused package tests pass: four signing/inventory/native-boundary tests, three
service-rendering tests, the REST read/write credential-file test, and three
shared secret-reference tests. The macOS production host build and package
round trip are retained under `out/m27b/package-mac-01` and
`out/m27b/package-mac-proof-02`: 15 files, 190716300 artifact bytes, manifest
SHA-256 `3a4673473da7199e7bac2a1625c97e50cd2a4bad1ffa0c551aa4b140aa785f29`.
Both rendered plists pass `plutil -lint`; no service was activated. The signer
is a freshly generated conformance-only key, not a production release signer.
The temporary signing seed and credential values were removed after the check.
Historical failed attempts remain recorded: an invalid source-digest prefix was
refused before signing, and oversized Linux debug artifacts were refused before
packaging. `out/m27b/package-linux-proof-02` records the separate successful
Linux AArch64 CUDA release package check: 21 artifacts, 39372189 bytes, manifest
SHA-256 `5d25284259505cbe2668ff73d204a7e2000b878b6ed1a71c55be5503d0ae6dcd`.
The six rendered systemd units pass `systemd-analyze verify`. Both package
records bind provider graph
`9a40f3c63aa2f74725a443e58af4eb610d365524322330d895ad62573b4b0e1d`. The native source copy is
`/mnt/nvme/cohesix-dev/m27b-host-source-19`; release compilation uses
`aarch64-unknown-linux-gnu` and the exact production generated files retained
in `out/m27b/package-mac-01/generated`. Package checks do not update the earlier
root/Worker/CUDA runtime evidence graph or assert service activation.

```sh
cargo test -p coh --lib package::tests --features fuse
cargo test -p cohesix-authority --features std secret::tests
cargo test -p cohesix-rest credential_files_resolve -- --nocapture
.venv/bin/python -m pytest -q tests/test_host_services.py
cargo clippy -p coh --features fuse --all-targets -- -D warnings
cargo build --locked -p coh --features coh/fuse -p cohsh -p hive-gateway \
  -p host-ticket-agent -p host-sidecar-bridge
```

The follow-up package review refuses a trust file located inside the package
itself, caps package path depth at 16 components, and makes an installed
`config/coh_policy.toml` or `config/cohsh_policy.toml` take precedence over a
checkout's development policy. Four focused package tests and the compiler
profile test pass; the macOS `coh` clippy check passes. The updated host tools
also pass `cargo check --locked --target aarch64-unknown-linux-gnu` on Merlin
from source copy `m27b-host-source-20`. These final boundary checks extend the
earlier package round-trip observations; they do not relabel the earlier
artifact digests as new builds. The full workspace suite was not run.

## Identity exchange and ticket compatibility

```text
Title/ID: m27b-generated-identity-issuance
Milestone: 27b / m27b-identity-mapping, m27b-read-visibility-classes
Goal: Issue finite host-mapped tickets and enforce current provider permissions
  before the gateway forwards any caller request.
Inputs: generated providers.identity_mappings, independently enrolled public
  JWKS digests, gateway delegation secret, existing gateway authority ceiling.
Changes:
  - cohesix-identity — kernel/JWT mapping, original issuer time, policy-bound
    subjects, gateway-only signing domain and finite compact ticket issuance.
  - cohesix-ticket — explicit canonical v2 integer codec; v1 defaults retained.
  - hive-gateway — bounded authenticated exchange and current generated action
    checks for every mapped write/batch line before charging or forwarding.
  - coh — explicit enrolled issuer option, proposal default and redacted audit.
  - Python identity/ticket helpers — bounded exchange, no redirects/proxies,
    independent response bindings and compatible v1/v2 claim inspection.
  - compiler release inventory — include the new product identity module.
  - OpenAPI and canonical host/security/API docs — issuance/refusal contracts,
    read visibility, offline docs and explicit gateway restart limitations.
Commands:
  cargo test -p cohesix-ticket --lib compact_
  cargo test --locked -p cohesix-identity --test mapping
  cargo test --locked -p hive-gateway --bin hive-gateway identity_exchange_
  cargo test --locked -p hive-gateway --bin hive-gateway auth::tests
  .venv/bin/python -m pytest -q tools/cohesix-py/tests/test_identity.py tools/cohesix-py/tests/test_ticket.py
  cargo clippy --locked -p hive-gateway -p coh --features coh/fuse -p cohesix-identity -p cohesix-ticket --all-targets -- -D warnings
  cargo check --locked -p cohesix-ticket --target aarch64-unknown-none
  cargo check --locked -p hive-gateway
  cargo run -p coh-rtc -- configs/root_task.toml --out apps/root-task/src/generated --manifest configs/generated/root_task_resolved.json --embed-userland-doc docs/USERLAND_AND_CLI.md
  ssh wizard@merlin2.local 'cd /mnt/nvme/cohesix-dev/m27b-host-source-21 && CARGO_TARGET_DIR=/mnt/nvme/cohesix-dev/builds/m27b-host /home/wizard/.cargo/bin/cargo check --locked -p hive-gateway -p coh --features coh/fuse'
Checks: Signature-domain isolation; finite original expiry; exact policy/action
  and batch checks; duplicate/overbroad/unknown/stale refusals; quota reuse;
  kernel uid provenance; bounded response parsing; no credential redirects.
Deliverables: Source, identity API documentation and focused evidence logs.
```

`identity-compact-ticket-tests-01.log` records two independent Rust wire/ULEB128
checks. `identity-mapping-tests-03.log` records three signature/claim/kernel
mapping checks; `identity-gateway-tests-02.log` records three issuance/action/HTTP
checks; `identity-delegation-regression-01.log` records six existing delegation
invariants; `identity-python-tests-02.log` records four SDK/wire checks.
All are under `out/m27b/` and pass. `identity-clippy-01.log` passes with existing
vendored fuser warnings. `identity-ticket-target-check-01.log` compiles the
shared no_std codec for AArch64. `native-identity-check-21.log` compiles the
selected private Linux AArch64 gateway/CLI profile; its exact source inventory
is `native-source-21.json`. This native compile precedes the later HTTP-body
413 response classification, separately checked on macOS in
`identity-gateway-check-02.log`. No external enterprise issuer is enabled or
claimed qualified. The policy defaults remain disabled. `regenerate-20.log`
records compiler regeneration after adding the SDK product module; final
release packages must be rebuilt from the final source and inventory.

The identity compatibility review covered every catalogued host-tool family.
`coh` adds explicit issuance; `cohsh`, FUSE mounts, the GPU bridge, host-ticket
agent, host-sidecar bridge and SwarmUI retain their existing ordinary issued
tickets and transports through the shared Rust decoder. They receive no
implicit identity exchange or refresh. `cas-tool` has no delegation parser or
identity issuance authority. Python now inspects v2 and offers an explicit
exchange helper. Rust and Python collectors still redact the unchanged
`cohesix-ticket-` prefix. The REST benchmark workloads and schemas, gateway
performance probe, Worker evidence parser and frozen M26e action expectations
need no identity-specific changes: they use their already enrolled tickets and
do not measure an identity provider. Existing generated v1 fixture tickets
remain unchanged. Manifest schema stays 1.22 because the handwritten ticket
payload has its own explicit version and no generated default/bound changes.

## Native host snapshot receiver (in progress)

Schema 1.23 introduces `ecosystem.host.snapshots` and source-specific
`/host/snapshots/<provider>/<source>/{ctl,status,snapshot}` paths under existing
namespace authority. The compiler bounds sources, pairs, byte/entry/value
counts, TTL, canonical provider registry membership and console paths. The
shared no_std validator refuses noncanonical records, wrong source/provider/
epoch, sequence or observation-time regression, duplicate paths and excessive
bounds. The root receiver retains an atomic bounded upload per pair; expiry
withdraws data while retaining the replay fence. First-byte receiver time
consumes TTL, and an exact retry cannot renew it. No native producer or target
acceptance is inferred from these source changes.

Focused initial evidence under `out/m27b/`:

- `host-snapshot-contract-tests-02.log`: two deterministic record, bounds,
  replay, failed-source and first-byte/TTL contracts pass.
- `host-snapshot-root-tests-03.log`: the pure root namespace upload/source
  separation/withdrawal contract passes; this does not simulate seL4 execution.
- `host-snapshot-compiler-tests-01.log`: one deterministic generated enrollment
  bounds/refusal contract passes.
- `host-snapshot-check-01.log`: the initial shared module compiles no_std for
  AArch64. Later receiver work still requires its exact target build.
- `regenerate-21.log` and `regenerate-22.log` retain compiler transitions. The
  later `regenerate-23.log` uses canonical provider id `network`, linking its
  publisher selection to the existing provider registry instead of introducing
  a new `net` alias. Initial test logs precede that identifier alignment.

Native collector wiring, durable publisher sequence state, SDK consumption,
whole-host compatibility closure and fresh target publication/expiry evidence
remain required. Previously signed packages and GPU/FUSE evidence retain their
original schema/profile and digest bindings; they are not relabeled as 1.23
qualification.

## Native snapshot publication and receiver check

Milestone 27b / `m27b-native-provider-discovery-and-actions` now connects the
schema-1.23 receiver to source-enrolled native publishers and Python reads.
The CLI requires an explicit source and private state root. A source/epoch lock
and fsync-reserved sequence prevent publisher restart from reusing a transmitted
sequence; missing/corrupt existing cursors refuse startup. Capture, disk
reservation and upload consume freshness. Native failure or excessive data
publishes an empty typed withdrawal. Source labels remain authenticated
correlation under the existing Queen/delegated path authority, not signed
execution evidence.

Native APIs cover selected systemd Manager D-Bus, Docker Engine, Kubernetes,
CUDA-helper/NVML inventory, Jetson device/package/clock/thermal/power state,
Linux link/address/IPv4-route tables, and macOS interface/address/default-route
state. Column tables and bounded route-row groups preserve selected rows and
native counters without repeated JSON names. Missing native fields remain null;
unknown native counter/address column names are retained. The launchd collector
requires explicit system service labels and exports no environment values.
No Mac-local NVIDIA probe runs under the Mac source enrollment.

Commands and focused results under `out/m27b`:

- `cargo test --locked -p host-sidecar-bridge --lib snapshots::tests::durable_cursor_refuses_reuse_missing_state_and_foreign_domains -- --test-threads=1`:
  `host-snapshot-publisher-tests-02.log`, one PASS. The first fixture run refused
  an insufficiently private directory; the fixture was corrected to mode 0700.
- `cargo test --locked -p host-sidecar-bridge --lib observations::tests:: -- --test-threads=1`:
  `host-snapshot-native-bound-tests-01.log`, two PASS including child timeout
  after stdout EOF and descendant pipe ownership. The later column contract
  test is `host-snapshot-network-column-tests-02.log`, one PASS, covering native
  address fields, 64-bit counters and complete route-row grouping.
- `cargo clippy --locked -p host-sidecar-bridge --all-targets -- -D warnings`:
  `host-snapshot-sidecar-clippy-04.log`. Final package compiles will include the
  subsequent early refusal of unenrolled library publication requests.
- Python snapshot/service tests: `host-snapshot-python-services-tests-02.log`,
  five PASS; `host-snapshot-service-enrollment-tests-01.log`, one PASS. Service
  rendering now binds the source/systemd pair to the verified package manifest.
- Exact QEMU build: `qemu-production-04/build.log` and `source.json` retain the
  production epoch-7 schema-1.23 target profile, binary/image identities and
  restored canonical generated outputs. This build compiled affected target
  components and exercised the new receiver in the following live checks.
- Mac native build and exact sidecar source hashes:
  `host-snapshot-mac-01/provenance.json`; binary SHA-256
  `a014a5251971d3b2392e6dfd20aa4fe49c4875576e1904955e60d3a5de3c8712`.
  `live-session-04/snapshots-mac-01/summary.json` is PASS: 48 real network
  records, sequence 1 then 2 after process restart, explicit unenrolled-launchd
  withdrawal, client source refusal and receiver expiry after 31 seconds.
- Linux native build: `native-source-24.json` and
  `native-snapshot-build-24.log`; binary SHA-256
  `5a60288af10084f6ec11c252db0d6fcfa1747fec3c4bf2cc832f82a4b5ce32bf`.
  `live-session-04/snapshots-linux-02/summary.json` is PASS: systemd, Docker,
  network, Jetson and CUDA-helper NVIDIA records reached QEMU and passed Python
  digest/domain/status checks. Network has 26 records; Jetson has 25. Kubernetes
  is explicitly `not_enabled` because no API is enrolled. Target expiry passes.
  Sequence 7–12 continues the same private source cursor from the retained
  first attempt; it was not reset. Docker used only the owned process's docker
  group through the authorized administrative credentials, with no account or
  daemon configuration changes.

The first Linux run remains a failure: network projection size exceeded the
receiver bound and the host had no nvidia-smi. Both published honest withdrawals.
The corrected projection retains every selected row, and the NVIDIA lane uses
the already-tested, digest-pinned CUDA 13.2.2 reference helper with exact UUID
checking. Historical package, GPU, identity and FUSE artifacts retain their
original source/profile/digests. The owned QEMU, gateway and tunnel were stopped
after this check; the full staged suite was not run.

Compatibility review: cohsh direct/REST framing uses the existing ECHO and CAT
bounds; hive-gateway applies existing delegated write/read visibility; coh/FUSE
and SwarmUI can enumerate the new generated namespace without new write powers.
The Python wheel inventory includes the new explicit-target SnapshotReader.
GPU bridge, host-ticket-agent, Worker schemas, telemetry ingest and benchmark
report contracts are unchanged by these read-only snapshots. The host-sidecar
CLI and systemd service template require source/state enrollment. Legacy
unversioned library publication helpers remain compatibility surfaces and are
not used by the new live CLI; further public-surface/deployment closure remains
part of M27b. This check proves native observations and QEMU receiver behavior,
not provider actions, Pi execution or a complete production use case.

## Native executor result delivery correction (in progress)

The live snapshot work exposed an M27b integration defect: systemd, Docker and
Kubernetes executors observed native completion and then attempted writes to
legacy `/host` control/status fixtures. The production root makes those status
projections read-only, so an already committed provider operation could be
misreported as pending or failed solely because that obsolete mirror failed.
The adapters now retain the complete native observation in the private CAS and
return its compact digest through the host-ticket agent's existing durable
result channel. Read-only systemd/Docker checks use the same bounded reference
rather than unbounded inline JSON. Native operation postcondition checks remain
unchanged; the independent source-scoped sidecar owns current host observations.

`native-executor-result-check-01.log` records the focused host compilation.
Native ticket-to-result integration and final profile compilation for this
correction are still pending. No completion or action receipt is claimed from
this source edit alone.

The first production ticket-to-result attempt is retained as
`native-ticket-results-01.log`: Root refused the request with `invalid-payload`
before the native agent ran. Inspection found the version-1 Root validator
still uses its original flat JSON parser: it rejects both the documented
`args` object and the M27a `writer_epoch`/`admission` fields. The production
agent requires the current writer epoch, so dropping that field cannot repair
the contract. Version-1 result validation has the same field mismatch. A strict
bounded typed parser must reconcile those documented request/result fields,
retain unknown/duplicate/type refusal, enforce the selected writer epoch, and
continue treating admission correlation as non-authoritative. That Root
correction and new exact target proof are the next unfinished work. Session 05
used the existing QEMU image and has been stopped; no native effect or receipt
was produced by this refused attempt.

The parser restoration is authorized under the reopened
`27a / m27a-host-ticket-validation-replay`, discovered by
`27b / m27b-native-provider-discovery-and-actions`. Root now uses strict typed
version-1 request/result records, a duplicate-rejecting primitive argument map,
structurally checked admission correlation and the generated writer fence.
Nested argument values fail directly in a serde visitor without recursively
allocating an untagged intermediate tree. The unrelated flat JSON grammars are
unchanged. `native-ticket-root-parser-tests-02.log` records one focused PASS for
nested argument acceptance, duplicate/type/null refusal and stale/missing writer
rejection. The new exact target build is `qemu-production-05`; native integration
and restoration closure are still pending.

Restoration closure: `qemu-production-05/build.log` compiled the corrected Root
for the exact production QEMU profile, with canonical generated outputs restored.
Its resolved manifest SHA-256 is
`b6c5494504b068c41ab95e9ccc23bca4580c691914b50b7adec62a1b2a6f3f88`;
the manifest and provider registry are byte-identical to QEMU build 04. The
source-matched Merlin agent build is `native-source-25.json` /
`native-ticket-result-build-25.log`; focused Mac lint is
`native-executor-result-clippy-01.log`. This native build also compiles the
snapshot publisher's early unenrolled-provider refusal.

`native-ticket-results-02.log` and
`live-session-06/native-ticket-results-02/summary.json` are PASS: Root admits
systemd and Docker read-only native tickets with structured arguments and epoch 7;
Merlin observes the actual APIs, retains private content-addressed observations,
and publishes two succeeded results without writing legacy status projections.
`native-ticket-epoch-negative-01.log` proves live Root refusal of missing and
stale version-1 epochs before native execution.
`native-ticket-correlation-01.log` proves admission correlation round-trip,
including a state epoch distinct from the writer epoch. Correlation remains
non-authoritative; its historical expiry is not used as a grant. The latter
run uses a fresh agent journal and refuses replay of the two previously terminal
records (`failed=2`, no repeated native operation), while the new read succeeds.
This replay refusal is retained rather than relabeled as a new success.

The scoped `27a / m27a-host-ticket-validation-replay` restoration is Complete.
M27b's native executor mirror correction is also verified for the read-only
result path. Provider mutation postconditions retain their earlier native helper
checks; this check does not claim newly verified mutation execution, executable
Worker completion, signed producer authority or physical Pi behavior. Session 06
and its gateway/tunnel were stopped. The first refused attempt and earlier
qualification artifacts remain unchanged. No full suite was run.

## Enrolled native evidence producers (15 September 2026)

Title/ID: `m27b-native-custody-producers`
Milestone: `27b / m27b-authoritative-receipt-and-evidence-core`
Goal: Extend an independently enrolled gateway admission witness with real
native observations and a verified Root terminal readback, preserving phase
custody and incomplete-chain refusal.

Changes: `cohesix-evidence::producer` adds private per-operation enrollment,
separate gateway/native/Worker-witness custody, immutable CAS, locked durable
prefix journals, exact binding/component checks and fixed expiry. The shared
verifier's terminal requirements are preserved; its internal prefix validator
cannot produce a terminal projection. Gateway and ticket-agent signing flags
refuse mock mode. The gateway signs a validated native v1 request append only
after exact Root readback and manifest measurement. Native systemd, Docker and
Kubernetes adapters carry observed native identity into their signed records.
The native agent checks the entire grant prefix and exact request before I/O,
then requires its observation hash in the exact Root terminal row. These are
host custodian witnesses, with device attestation and Worker proof explicitly
false. `docs/CAUSAL_EVIDENCE.md` defines the enrollment and operational contract.

Focused commands/checks:

- `cargo test -p cohesix-evidence --test producer --test graph`:
  `out/m27b/evidence-producer-tests-03.log`, 4 PASS. The added enrollment/profile,
  request semantic and fixed-expiry test brings producer coverage to 2 PASS in
  `evidence-producer-enrollment-tests-04.log`. The first producer test failed
  because the temporary directory lacked the required private permissions;
  only the fixture permissions were corrected, and that failure is retained.
- `cargo test -p hive-gateway admission_witness_requires_exact_root_fields_and_one_record`:
  `evidence-gateway-tests-02.log`, 1 PASS, 84 filtered. Exact fields, duplicate
  rows, changed epoch and changed native operands remain strict refusals.
- `cargo clippy -p cohesix-evidence -p hive-gateway -p host-ticket-agent --all-targets -- -D warnings`:
  `evidence-producer-host-clippy-03.log`, PASS.
- `python3 out/scripts/m27b-build-evidence-mac-02.py` builds release `coh` and
  gateway with the private QEMU profile, retains binary/provenance under
  `evidence-producer-mac-02`, and restores canonical generated outputs.
- `python3 out/scripts/m27b-sync-native-26.py`; on Merlin,
  `cd /mnt/nvme/cohesix-dev/m27b-host-source-26 && CARGO_TARGET_DIR=/mnt/nvme/cohesix-dev/builds/m27b-host /home/wizard/.cargo/bin/cargo build --locked --release -p host-ticket-agent`:
  `native-evidence-build-26.log`, PASS. Existing vendored FUSE warnings are
  retained without modifying vendor code.
- `python3 out/scripts/m27b-live-session-07.py` and
  `python3 out/scripts/m27b-signed-native-live-01.py`:
  `signed-native-live-01.log`, PASS. Exact QEMU build 05 accepts a read-only
  systemd request; Merlin reports `seen=1 succeeded=1 failed=0`. The shared
  installed `coh evidence verify` accepts eight phases under distinct gateway
  and native public keys. An exact gateway retry preserves all signatures and
  leaves one Root request row. A forged signature and a missing terminal are
  refused. No new target build or full suite was needed.

The live graph SHA-256 is
`97b327e50f435a87e3578909224441d2e25c9967e05022d497f803bd1bcd2fd2`.
Its gateway binary is
`28c616f68f2eda3cdcfac222647a8ea8e1c670989941c7f9bc866eb595ddeaea`;
its native agent binary is
`be172db092d6eb619a59e87f3125f32f08dc4a5b0c83fe8c5fd8477afbf7fffe`.
`live-session-07/signed-native-01/summary.json`, `trust.json`, the two private
stores and `verified.json` retain source/manifest/component/phase provenance.
The use-case binding fingerprints the generated host-integration dependency
artifact containing its frozen use-case rows; this read does not qualify a
production use case. Both temporary conformance private keys were deleted.
The owned QEMU/gateway/tunnel session was stopped.

A subsequent recovery correction reads for an exact existing successful Root
result before retrying a signed terminal append, so a lost result ACK cannot
create a duplicate terminal row. This is narrower than native operation replay:
the existing agent WAL still owns dispatch and never re-executes this operation.
`cargo test -p host-ticket-agent lost_terminal_ack_requires_exact_single_root_record_before_retry_is_suppressed`
is the focused independent predicate check; its logs are
`evidence-native-retry-tests-02.log` and `evidence-native-retry-clippy-02.log`.
The corresponding native profile check is `native-evidence-retry-check-27.log`
against the exact source inventory `native-source-27.json`. This correction is
not retroactively attributed to the earlier source-26 live run.

Compatibility review: existing unsigned operation-report and native observation
schemas, transport result text and public control grammar remain unchanged.
`coh`/Python evidence consumers and exporters share the unchanged terminal
verification contract; `cohsh`, FUSE, sidecar snapshot publication, `cas-tool`,
GPU IPC, REST read scopes and SwarmUI receive no new authority. Benchmark
workloads and report fields require no change for this opt-in signing path.
The remaining executable Worker witness, negative terminal-chain, generated
workflow/deployment and broader conformance tasks remain open; no device,
Worker, Pi, MIG or production-use-case acceptance is inferred from this run.

## SIEM TLS acknowledgement recovery (15 September 2026)

Title/ID: `m27b-siem-tls-durable-ack`
Milestone: `27b / m27b-observability-exporters`
Goal: Deliver a verifier-accepted graph over TLS and reconcile a lost receiver
acknowledgement across both process restarts.

The generated SIEM policy now accepts an optional explicit CA-certificate path
reference. The reference resolves to one bounded absolute PEM file, which must
contain exactly one valid certificate. An invalid explicit source never falls
back to system roots. The default policy omits this optional field and preserves
the existing graph. Sender state requires an owned private directory, bounded
regular files, no symlink/hardlink substitution or writable foreign files, and
a retained WAL once the store has been initialized. Missing WAL is a refusal.

`python3 out/scripts/m27b-siem-live.py` built the private generated-policy
release `coh` and restored every canonical generated output. The owned loopback
TLS receiver committed an exact graph id, hash and payload using SQLite FULL
synchronization before dropping its first response. After receiver restart, a
fresh sender process obtained the exact acknowledgement; a third sender process
read its durable acknowledged state without sending again. The result is three
sender processes, two receiver process lifetimes, two HTTP requests and one
unique durable acceptance. Temporary TLS private keys were deleted and the
owned receiver stopped. `out/m27b/siem-live-01/summary.json`, the sender WAL/ACK,
receiver database, source inventory and process logs retain the observations.

The proof class is `live_receiver_delivery_of_historical_verified_native_graph`.
It uses graph `97b327e50f435a87e3578909224441d2e25c9967e05022d497f803bd1bcd2fd2`,
release binary `cfd9cdad81309b05608b4569202027639ae32d8945add7e8caad409e05eca5da`,
and acknowledgement `0d24d565663de4bcd5b4f6962ccf9a17d75abc33ce3c4a20222883bd7c3b9b57`.
This proves delivery to the owned contract receiver; it does not claim a vendor
SIEM installation, fresh native execution, device attestation or Worker proof.

Focused checks: `siem-durable-tls-tests-03.log` (two sender-state/CA tests),
`siem-tls-compiler-tests-01.log` (one generated CA-reference test),
`siem-clippy-01.log` (coh/coh-rtc all targets), and
`native-siem-check-28.log` (Linux AArch64 coh) pass. The initial malformed-PEM
fixture exposed lazy certificate parsing; the explicit bundle parse fixes the
implementation, with the failure preserved in `siem-durable-tls-tests-01.log`.
Two retained payload/ACK reads were subsequently switched to the bounded file
reader and covered by the final focused checks; the earlier live binary is not
relabelled as containing that later correction. Canonical regeneration 25 passes.

Compatibility review: the acknowledgement and exporter payload schemas, shared
Rust/Python terminal verifier, gateway/console/FUSE scopes, GPU executor, sidecar
snapshots, CAS and SwarmUI authority remain unchanged. Optional CA selection is
documented in HOST_TOOLS and SECURITY; other host tools do not deliver SIEM
payloads. Provider/exporter performance remains a distinct later task.

## Federation, matrix and provider overhead (15 September 2026)

Title/ID: `m27b-federation-matrix-overhead`
Milestones: `27b / m27b-federation-conformance`,
`m27b-ecosystem-conformance-matrix`, `m27b-provider-exporter-performance`
Goal: Recover the exact native terminal across two actual hives after a lost
forward acknowledgement, expose focused canonical contract selection, and
measure the changed host paths without a full performance suite.

The scoped compiler change replaces M27a's unconditional production-federation
activation ban with M27b's implemented host relay. The existing production
strict-intent, audit/replay, writer epoch, WAL, raw-debug and secret-reference
requirements remain mandatory; the Release A profile generator still defaults
federation off. `federation-compiler-test-01.log` records one passing positive
and floor-refusal contract test. This is selection, not a use-case promotion.

Two disposable provisioned profiles explicitly enable federation. Exact builds
`qemu-federation-a-09` and `qemu-federation-b-10` pass and restore all canonical
generated outputs. `native-federation-b-build-30.log` records the matching
Linux AArch64 release agent build, with its inventory in `native-source-30.json`.
The first attempts are retained: run 01 encountered the launcher's fixed
diagnostic-port collision; run 02 passed those new arguments incorrectly to
the gateway; run 03 selected an intentionally disabled federation profile;
builds 07/08 then identified the obsolete unconditional compiler ban. The
launcher now accepts bounded explicit UDP-echo and TCP-smoke ports, while
preserving guest ports/image identity and the default smoke fallback. An
explicit smoke port never silently falls back. The independent port-range
check passes in `matrix-port-tests-01.log` (four selected tests total).

`python3 out/scripts/m27b-federation-live-04.py` runs the canonical
`scripts/ci/provider_federation.py` against both actual QEMU hives and a native
Merlin agent. The runner refuses nonempty ticket streams, checks both measured
Root manifest hashes and uses separate source/peer delegation. It submits one
read-only `systemd.status-check` for `ssh.service`; the target executes it once.
The owned proxy drops the first HTTP 200 forward reply. Source process one
retains a pending WAL entry; process two restarts, retries forwarding and
returns the target terminal exactly; process three sends nothing further. The
retained native observation hash matches the target result. All owned hives,
gateways, tunnel and native agent were stopped; the final remote process check
finds zero owned agents. No account, network service or native unit was changed.

`out/m27b/federation-live-04/conformance/summary.json`, three source logs and
WAL snapshots, both identical terminal records, proxy events, the native
observation object and `owner-summary.json` bind the result. Terminal SHA-256
is `97c0473b5d2f4aeaa932c3840bd31b037050c9ba3b93426793f58bf51972c558`.
Source agent SHA-256 is
`b6131ad71458af4657976768f3110744ff1c86028c13dea96f6d2644d538e34f`;
target native agent is
`b9ec7b589a1db9727d6d5feb8f55ac13557bee3b09d59002ab4724c724aaa0a9`.
The source/target Root images are respectively
`958c0b24a803d8b0b24e9d75a50bd1b445937abd44c71f759d893d4e683f878a` and
`1b4260fa4ecee1f589a3be36c7f337e5956ba7d1983a7bb4fa70973efb63d5e1`.
This is observed correlated native terminal return, with signed-graph, Worker,
Pi and production-use-case proof explicitly false. Earlier pure WAL/queue,
correlation and deadline checks retain their separate scope.

The new `configs/provider_conformance.toml` inherits exact versions, lifecycle,
authentication and evidence requirements from the generated registry rather
than defining another action catalog. `provider_matrix.py` validates bounded
case/profile/provider identity, scoped cargo/pytest selection, all seven
lifecycle phases and separate proof lanes. It records source files, commands,
output hashes, counts, duration and generated availability, stops on the first
failed selected contract, refuses zero-test success and does not promote
generated availability. `--validate-only` emits NOT_RUN rows; the selection
record is `provider-matrix-selection-01`. Five focused matrix/terminal/endpoint
tests pass in `matrix-federation-tests-03.log`; the prior port test is retained.
No full matrix or full suite was run. HOST_TOOLS and TEST_PLAN document these
entry points, including explicit live federation enrollment and proof limits.

`.venv/bin/python scripts/ci/provider_conformance_run.py --perf-only --state-dir out/m27b/provider-overhead-01`
passes the opt-in release host probe. It records ten warmups and 100 samples
per operation over a 12,527-byte, eight-node signed fixture and 168,338-byte
registry. p95 is 2.500 microseconds for registry lookup, 2.625 for target
validation/refusal, 5.084 for local identity mapping, 602.584 for signed graph
verification and 13.625 for receipt rendering. Exporter p95 is 0.334
microseconds for Prometheus, 118.167 for OpenTelemetry, 37.375 for CloudEvents,
37.083 for in-toto and 26.334 for SIEM. Output sizes range from 380 to 7,406
bytes. `performance.json` retains every sample, artifact size and proof limit;
`provider-overhead-clippy-01.log` passes the exact test-target Clippy check.
There is no comparable prior baseline, native execution, network/disk cost,
or target throughput claim. BENCHMARKS owns the methodology.

Compatibility review: the complete host-tool suite, Python verifier/projection,
REST/OpenAPI, FUSE, CAS, GPU IPC, snapshot publication and SwarmUI retain their
existing authority and wire formats. The added launcher ports are host-only.
The existing hardware benchmark scripts require no workload/schema change;
provider overhead is a separate opt-in report. The conformance scripts are
invoked by the canonical documented runner and are excluded from live packages
unless a generated deployment profile explicitly selects them. M27b remains
In Progress for its remaining provider, workflow, deployment and evidence tasks.

## Native MIG implementation and Orin regression (15 September 2026)

Title/ID: `m27b-gpu-workload-and-mig-executor`. The native helper now discovers
bounded NVML parent/GI/CI UUIDs, profiles, memory and placements. A selected
`nvidia-mig-cuda13` executor requires independent enrollment of the complete
native topology digest and exact CI. Fresh NVML checks fence topology changes;
the isolated CUDA child has one visible MIG UUID and checks the unique native
CI UUID through the versioned CUDA driver API. Discovery and execution share
cancellation/deadline bounds. The unchanged local GPU transport requires the
existing ticket/lease/epoch/TTL/resource/request/helper bindings. GI memory may
be shared by sibling CIs; no independent CI memory isolation is claimed.

Focused command evidence:

- `cargo test --locked -p gpu-bridge-host --lib mig::`:
  `out/m27b/mig-tests-01.log`, two passing tests covering UUID/placement/profile,
  whole-generation changes, duplicate native identities, unavailable state and
  canonical ordering.
- `cargo test --locked -p cohesix-authority --lib gpu::tests::mig_profile` and
  `cargo test --locked -p coh-rtc --lib provider_registry::tests::mig_executor`:
  one passing contract test each; changing profile cannot weaken ABI bounds.
- `cargo clippy --locked -p gpu-bridge-host --all-targets -- -D warnings`:
  `out/m27b/mig-clippy-02.log` passes. The first initializer-style finding is
  retained in `mig-clippy-01.log`.
- Native source copies 31 and 32 compile the CUDA 13.2.2 Orin helper and release
  bridge with `--features cuda,nvml,rest`. Source 32 additionally compiles the
  compute-80 PTX MIG variant with `-DCOH_REFERENCE_MIG=1`; this is compilation
  evidence, not execution on a MIG-capable host.
- `out/scripts/m27b-native-mig-orin-01.py` exercises actual Orin vadd, matmul,
  wrong-device, over-budget, stale-inventory, timeout and cancellation through
  the canonical CUDA reference runner. All seven pass. Source 31 initially
  reports MIG unavailable because this host's parent UUID does not have the
  datacenter UUID format.
- Source 32 checks MIG mode before parsing a parent UUID. The fresh
  `out/scripts/m27b-native-mig-orin-02.py` query returns exactly
  `not_supported`, NVML error 3, with no instances. The corrected helper does
  not change physical CUDA execution. The diagnostic records and source/build
  hashes are retained in `out/m27b/mig-native-evidence/` and the corresponding
  `native-mig-*.log` files.

Source 32 bridge SHA-256 is
`caddc11d738179240e1b23288a8b745773e64fb510f04131bdb51d18afc7f581`;
physical helper is
`c46203b46f5f5f6f2f8411ae2ec3c92c8527ce3bd659f913a0ffe888b0c2e3b7`;
compile-only MIG variant is
`03f4bf186919079c914d54cc6f645354232d78ef578b0597b1162bbbd2efdd48`.
The unchanged registry graph is
`cc91ca1fd9f74bdb0f39a153ee1dd489f6a432109cbd4a8c06c5ba58786e7ed6`.
No MIG-capable device is available here. Native MIG execution/isolation remains
unqualified; Orin, Worker, Pi and use-case proof classes stay distinct.

Compatibility review: the generated optional executor profile preserves all
transport/resource defaults. Existing physical configuration omits the new
optional MIG enrollment. `coh`, `cohsh`, host-ticket-agent, Python and benchmark
workloads continue to use the existing admitted device/topology/request ABI;
only gpu-bridge-host owns native MIG selection. The matrix adds the focused
identity/refusal case. Build manifests retain schema v1 and now bind the helper
header as well as its translation unit and executable. Package builders consume
the measured helper binary; rebuilding final packages remains part of closure.

## Field-bus codec, spool and independent DNP3 work (in progress)

Scope: reopened `m18-live-modbus-dnp3-sidecars`, discovered under the production
surface audit and implemented for `m27b-industry-sidecar-contracts`.
`cohesix-authority::bus` owns exact bounded endpoint/point maps; `coh-rtc`
validates the optional field-bus registry extension. The new native sidecar-bus
feature implements MODBUS RTU/TCP and the documented single-fragment DNP3 subset,
native deadline-bound TCP/serial ownership, exact protocol response checks and a
private atomic spool. The old model always queues; native adapter names no
longer alias the in-memory model.

`fieldbus-protocol-tests-02.log` records three passing codec tests;
`fieldbus-map-tests-02.log` records the exact-range/control-map test;
`fieldbus-wal-tests-01.log` records three passing lost-ACK/restart/corruption tests;
`fieldbus-model-test-01.log` verifies the online model cannot report delivery.
The missing no_std test macro import is retained in `fieldbus-map-tests-01.log`.
`fieldbus-clippy-04.log` passes the focused native crate check after correcting
style findings. A later DNP3 public decoder guard for invalid operation bounds
is covered by `fieldbus-protocol-tests-02.log`; it is not retroactively attributed
to the earlier reference binary.

`out/scripts/m27b-fieldbus-profile-build.py` generates a private exact endpoint
profile, compiles the native release client and restores canonical generated
files. `out/m27b/fieldbus-native-01/interop-02/summary.json` records a real TCP
exchange with separately built OpenDNP3 3.1.2 at commit
`c1dc7165a79cc08edbf4b55d2ff4162efb176f92`. The client refuses a remote restart
indication, reads analog 42 with ONLINE quality after explicit reference-fixture
setup, and returns the identical durable result without a second wire read on
process restart. Client SHA-256 is
`e8a5560979ddc7ac7cea1e64ecb7a5676ee3d208a8c4f9059bdaacb703063242`;
reference outstation SHA-256 is
`177863aebfcf6720fe5c581289a890e06f9fe5f3609b9ed18251631f148513c8`.
The initial reference attempt, including its additional need-time IIN bit, is
retained in `interop-01`; fixture time setup is explicit in the second attempt.
The released client does not set time or clear restart indications. Both owned
reference outstations exited after the runs.

This is native host interoperability with a named software reference, not
physical field-device, Worker or production-use-case acceptance. MODBUS native
interoperability, endpoint/action registry wiring, authenticated TTL publication,
control admission integration, Linux compilation, packaging and canonical
conformance routing remain to be completed for this task.

## Field-bus integration and native references (15 September 2026)

M18 `m18-live-modbus-dnp3-sidecars` and M27b
`m27b-industry-sidecar-contracts` now include compiler-validated endpoint/point
maps, v1 read/control ticket dispatch, signed control grants, exact map/graph
fences, read-only bounded retry, and freshness-preserving snapshot projection.
Schema 1.24 adds the four ticket actions without adding in-VM protocol I/O.
The SDK validates the same compiled maps. Native package profiles include
sidecar-bus and FIELD_BUS.md; service rendering requires signed enrollment for
bus tickets and grants only exact configured serial devices.

Focused evidence: `fieldbus-authority-test-01`, `fieldbus-snapshot-test-01`,
`fieldbus-agent-test-01` and `fieldbus-compiler-test-01` each pass their selected
contract test. `fieldbus-services-test-01` passes three rendering tests.
`fieldbus-matrix-03` passes codec and WAL tests; its SDK failure was an inline
import shadowing the global validator, corrected and passed in
`fieldbus-matrix-sdk-04`. `fieldbus-matrix-parser-test-01` passes three tests.
The earlier comma-filter and missing-pytest failures remain in runs 01/02.
`sidecar-library-test-01` and `fieldbus-integrated-clippy-04` pass after removing
superseded library publication paths and adding the compiled mount fence. The
initial error conversion compile finding remains in Clippy run 03.

The canonical `scripts/ci/field_bus_conformance.py` owns independent PyModbus
3.11.4 and OpenDNP3 3.1.2 references. `fieldbus-canonical-macos-01` passes native
TCP/RTU-PTY/DNP3 reads, exception/restart refusal, signed controls and exact ACK
replay. `fieldbus-canonical-linux-05` passes those cases plus ACK-time-preserving
DNP3 snapshot projection on Linux AArch64. Linux source 38 client SHA-256 is
`412306a8ee22ee95d24a2c05644f2678a6962b270a9f0e6f1229ba518d4dd2e8`;
OpenDNP3 outstation is
`da12e0a1caade1850be34cfb2b02bf281424ffaf1479b19bfae9dd175a961cc6`.
Both bind the independently checked OpenDNP3 commit
`c1dc7165a79cc08edbf4b55d2ff4162efb176f92`, private generated maps, source inventory,
wire bytes and durable records. Ephemeral fixture signing keys are deleted;
owned references exit. Linux attempts 01–04 stopped before wire I/O for incomplete
Gitless source/index/release inputs; the final runner verifies a complete source
manifest and supplies a temporary Git index solely for compiler inventory.
No synthetic commit is created. Selected licence/release support files are
preserved unchanged in the native archive.

These records establish host interoperability, not physical UART/device, Worker
or production use-case proof. Final exact Root integration, package rebuilding
and milestone closure are still pending. The sidecar library now shares the CLI
publisher, removing stale legacy status writes and typed-unavailable Jetson/net
branches superseded by actual native observations. Complete suite review found
no change required to coh/cohsh, gateway, FUSE, SwarmUI or hardware benchmark wire
formats for this library migration. SDK field-bus validation and generated
registry/snapshot contracts were updated together; final suite-wide review
remains part of M27b closure.

## Worker v2 producer wiring (19 September 2026; live closure pending)

Scope: `m27b-authoritative-receipt-and-evidence-core` and
`m27b-gpu-workload-and-mig-executor`. The shared evidence crate now owns exact
caller-versus-Root v2 normalization, correlation-path encoding and independently
enrolled Worker binding checks. The gateway signs only the exact resolved
snapshot, retains a pending Worker sequence, and captures bounded GPU/lease
facts. Retries preserve original facts and grants. The agent validates the exact
signed grant before I/O, checks distinct gateway/native/Worker public keys, and
requires a separate Worker witness enrollment. Native publication is persisted
before witness attempts; cursor/compaction waits for exact Root completion.
Signed retained Worker phases can finish after restart without changing bytes.

GPU native results retain their original durable terminal time and immutable job
identity. Actual failure/cancel/revoke/interruption observations enter verification
with the corresponding outcome; missing native evidence remains incomplete.
Control/observe receipts retain their own postcondition and leave the original
submit outcome unchanged. The service renderer supplies the separate enrollment
option without embedding keys. CAUSAL_EVIDENCE, HOST_TOOLS and packaging guidance
document the non-attested Root witness and independently enrolled image hash.

Focused commands/evidence:

- `cargo test --locked -p cohesix-evidence --lib ticket::`:
  `worker-evidence-ticket-test-01.log`, one passing normalization/refusal test.
- `cargo test --locked -p cohesix-evidence --test producer`:
  `worker-evidence-producer-test-02.log`, three tests pass, including independent
  Worker custody for success/failure/cancel/expiry, missing phases and forged
  generation. The initial test-only Event clone compile error is retained in 01.
- `cargo test --locked -p hive-gateway --bin hive-gateway evidence::tests`:
  `worker-evidence-gateway-test-01.log`, two exact admission/refusal tests pass.
- `cargo test --locked -p host-ticket-agent --lib causal::tests`:
  `worker-evidence-agent-test-01.log`, two tests pass for lost result ACKs and
  pinned/new/completed Worker receipts across success/failure/expiry.
- `cargo clippy --locked -p cohesix-evidence -p hive-gateway -p host-ticket-agent --all-targets -- -D warnings`:
  `worker-evidence-clippy-03.log` passes. Run 01's style finding was corrected;
  run 02 also passed before the final producer test addition.
- `.venv/bin/python -m pytest -q tests/test_host_services.py -k worker_witness`:
  `worker-evidence-service-test-01.log`, one focused rendering test passes.

The canonical matrix routes these evidence contracts. No full suite ran. These
are host contract results, not a claim that the new signed Worker chain has yet
passed its combined QEMU/native-CUDA lane. Exact target/native rebuild, live
success and negative chains, final package validation and M27b closure remain.
The shared verifier already owns Rust/Python/REST/UI/export projections; no new
wire schema or parallel receipt validator was introduced. Hardware benchmark
workloads/schemas remain unchanged. Source and package inventories need their
final regeneration after remaining provider/workflow tasks.

## Signed CUDA-to-Worker live chain (19 September 2026)

The exact QEMU image in `out/m27b/qemu-worker-evidence-11/artifact` and native
Merlin source archive 39 passed separately enrolled gateway, CUDA-agent and
Worker-witness signing for vector addition and matrix multiplication. Retained
evidence is `out/m27b/live-session-09/signed-worker-02/summary.json`, with all
nine graph phases, exact image/component hashes, CAS artifacts, trust inputs,
verification outputs and deterministic missing/forged Worker refusals. The
Worker receipt/completion sequences were respectively 2/2 and 3/3 with no
outstanding control. The vector-add graph is
`77e06fd55f3955c8f66970d9cf02686371523b542c3c70b841c5c2f3f5d1735c`;
matrix multiplication is
`72546bf967d2ace5df6d589f2bd0d31b37115003a16496a3e2ea23e9ffbbf99e`.

The first diagnostic correctly refused to sign while the Worker completion was
pending (`live-session-08/signed-worker-01`). The second waited for Root's
terminal observation, restarted the agent, and completed from its durable
native result without redispatch. Further agent and gateway retries left graph
bytes unchanged. All private signing keys were removed and owned native/QEMU
processes stopped. This proves authenticated Root witness custody with an
independently enrolled Worker image; it is not device attestation, a Pi result,
or qualification of an entire production use case. Lease setup used the
unsigned native path and is explicitly excluded from the signed chain claim.

The GPU control reconciliation envelope now has an explicit read-only
`control_status` operation. It preserves the admitted cancel/observe binding,
cannot dispatch cancel, and checks the same Worker/resource generations.
Cancellation dispatch happens once, followed by read-only completion checks.
`cargo test -p gpu-bridge-host --lib workload::tests -- --nocapture` passed three
focused tests in `out/m27b/worker-evidence-control-test-01.log`; the new regression
checks unchanged WAL bytes, no WAL file creation, and changed-generation refusal.
Exact Linux release compilation passed in
`out/m27b/worker-evidence-native-build-01.log` with CUDA/NVML/REST features.
QEMU compile/restoration evidence is `qemu-worker-evidence-11/source.json`.
No full suite ran. The remaining macOS/provider, packaging and workflow tasks
still prevent marking M27b Complete.

## Native launchd lifecycle (19 September 2026)

Scope: `m27b-native-provider-discovery-and-actions` and the associated registry,
SDK and conformance surfaces. Manifest schema 1.25 adds optional compiler-owned
launchd target maps and selectable lifecycle actions. The Rust agent requires
one exact target id; the SDK follows the same generated argument map. Discovery
uses these configured targets instead of environment label lists. An empty map
remains unavailable and the default Root allowlist is not expanded.

The native adapter binds plist label/content, selected executable bytes,
launchd invocation counts and a measured Swift libproc helper's PID/uid/birth
identity. Lifecycle postconditions require real process state, including native
absence after stop. Separate root/native custody and the version-1 durable
non-replay journal remain authoritative. No new unsafe Rust, raw commands, native
paths in ticket inputs, or in-VM dependency is introduced.

Focused evidence:

- `launchd-map-test-03.log`: one exact map/refusal test passes. Initial isolated
  compilation exposed missing alloc imports and missing explicit serde std
  features; both were fixed, preserving the target no_std feature closure.
- `launchd-native-contract-test-01.log`: one native incarnation/postcondition
  contract test passes; an unused import was removed afterward.
- `launchd-clippy-01.log`: scoped sidecar, ticket-agent and compiler Clippy passes.
- `launchd-no-std-01.log`: authority without std compiles.
- `native-launchd-03/summary.json`: canonical `--provider launchd --live-reference`
  PASS for pre-dispatch executable mismatch refusal and start/restart/stop of
  one owned `/bin/sleep` job. The measured helper, exact plist/executable hashes,
  selected SDK, native process observations and successful job removal are retained.
  This is host-native reference proof, not Root/Worker/package qualification.
- `native-launchd-01` retains the initial negative-test deadline failure after
  successful lifecycle observations. The negative was moved before dispatch,
  where the real executable hash now refuses it without starting another job.
- `launchd-generate-04.log`: canonical source generation passes. The concurrent
  operator-manual integration temporarily restored base files during compilation;
  earlier logs retain its transient missing API error. Restored source timestamps
  were refreshed before rebuilding. Current main includes `49b61ca4c` and the
  upstream operator-guide commits; those changes remain intact.

No full suite ran. The native runner is now tracked, routed through the canonical
matrix entry point and documented in MACOS_PROVIDERS. Package helper wiring,
exact Root integration, macOS release/endpoint actions and generated workflows
remain active M27b work.

## Retained macOS and workflow foundation (19 September 2026)

Main `1383c9630` narrows 27b to the Executable Host Foundation. The work below
was implemented under the earlier broad activation and is preserved under the
27c/28a owners recorded in BUILD_PLAN. Historical evidence and task ids above
retain their original identity; they are not upgraded by this ownership change.

Schema 1.26 adds optional exact macOS release/endpoint target maps. Source tree
and artifact hashing, private native attempts, Xcode structured result checks,
certificate-bound signing checks, correlated Apple job/upload/checksum checks,
and fixed endpoint observations are implemented. Empty maps stay unavailable.
The native reference at `out/m27b/native-macos-release-01/summary.json` passes
Xcode build, one executed test and archive, plus duplicate attempt refusal.
Apple credentialed signing/notarization/upload and Root/package qualification
remain unexecuted. `macos-sdk-matrix-02.log` records five passing SDK/map cases;
`macos-clippy-02.log` records scoped compiler/agent/sidecar Clippy. Public native
summaries subsequently redact device/test/path details; raw xcresult artifacts
remain hash-bound. No cloud qualification or endpoint certification is claimed.

Generated workflow IR retains all nine previously specified DAGs, finite stage
order, exact actions/topology and explicit external dependencies. `coh` and the
SDK provide plan/apply/watch/explain/verify/recover foundations. Every advance
requires exact request equality inside an independently signed graph; missing
terminal data stays pending. Recovery has separately enrolled current authority
and an original terminal link. Current workflows needing undeployed external
applications refuse apply. Legacy live control-model execution now requires an
explicit rehearsal flag. These are retained implementation foundations, not
complete recipes or domain-use-case proof.

Focused evidence under `out/m27b`:

- `workflow-ir-test-01.log`: canonical DAGs and missing-stage, unknown-action,
  orphan-topology and implicit-recovery refusals pass.
- `workflow-evidence-test-01.log`: signed completion, forged request refusal
  and missing-terminal pending contract pass.
- `workflow-sdk-test-01.log`: ten generated/rehearsal/credential boundary cases pass.
- `workflow-clippy-01.log`: scoped coh, compiler, agent and sidecar Clippy passes.
- `cuda-safe-oom-01.log`: isolated deterministic CUDA OOM fault mapping passes
  without allocating shared GPU RAM; another CUDA error cannot impersonate OOM.
- `cuda-enforcement-01.log`: selected finite cgroup controls and unsupported
  unlimited/oversized controls pass. The explicit local lane stays unqualified.
- `foundation-service-test-01.log`: exact CUDA device policy and selected
  process limits pass without activating any service.

Compatibility review covers coh/cohsh, swarmui's generated inputs, gateway,
REST/OpenAPI, GPU bridge, ticket agent, native sidecar/bus, CAS/evidence tools,
Python library/CLI and performance harnesses. All share regenerated schema and
provider/evidence contracts; CLI workflow and provider changes have public
usage documentation. SwarmUI rendering and benchmark workload/report schemas
need no change for these host-only operations. No hardware benchmark or Pi
acceptance is inferred. The SDK remains target-neutral; native references,
QEMU integration and earlier package checks retain their separate source ids.

## Selected executable foundation closure (19 September 2026)

**Complete.** Scope is the five current 27b tasks in BUILD_PLAN at main
`1383c9630`, with the owner's CUDA 13.2.2 selection and focused-testing instruction.
The former broader activation's working implementation is preserved and mapped
to its current later owners. No recipe, UI, credentialed Apple release, capable
MIG host or broad deployment/package qualification is implied by this closure.

| Current task | Closure evidence |
| --- | --- |
| `m27b-provider-action-registry` | One regenerated schema 1.26 action/target/provider contract; exact selected native identities, finite controls and shared consumer/refusal contracts. |
| `m27b-authoritative-receipt-and-evidence-core` | Shared nine-phase verifier; separate gateway/native/Worker enrollment; exact retries, missing/forged phases and CAS checks; live native artifacts correlate below. |
| `m27b-scoped-authority-and-read-visibility` | Existing focused identity/expiry/revocation/writer/action/read-scope negatives above; production Root v1 repair and v2 live dispatch; secret references and protected custody. |
| `m27b-jetson-orin-nano-live-conformance` | Both admitted systemd and NVIDIA-container vadd/matmul paths pass with independent output, native identity, controls and terminal observations. |
| `m27b-operator-manuals-and-reference-maintenance` | Prior operator manual record plus current exact host/QEMU compilation and shared generated/Test Plan checks. |

Fresh host observation is `out/m27b/foundation-host-versions-02.log`: Ubuntu
24.04.5 LTS, AArch64 kernel 6.8.12-1021-tegra, JetPack 7.2.1-b49, L4T
39.2.1-20260806224157, CUDA toolkit 13.2.2-1, nvcc 13.2.86-1 and NVIDIA container
toolkit 1.19.1-1. Both native results report CUDA driver/runtime 13020 and device
UUID `8cc9c4072cc0587a8d93b7e2754559cb`. Jetson is the tested reference for the
portable Linux AArch64 contract; no new platform was introduced.

The selected QEMU image is `out/m27b/qemu-worker-evidence-12/artifact`, built from
source digest `e4f48b785f0e25078fca94faef958ee9f4bec260e86ff8b2e0ddbbbd19bd32d9`.
Its provisioned input manifest SHA-256 is
`302600a95502694d479186d11060b1fc1075b3d07793dadbaaf87544160fba90`; resolved target
manifest is `d502977c8b61c5ecb273d5bbcaced92e1b4b631d60e47641e98bb950d0d20de9`.
Native archive 40 compiles those same selected generated contracts. Source
archive records and each graph bind Root, Worker image, gateway, agent, bridge,
helper and generated graphs independently. Later documentation/catalogue
classification repairs are not relabelled as changes to this executed image.

| Admitted lane | Retained summary and SHA-256 | Native identity |
| --- | --- | --- |
| systemd | `out/m27b/live-session-10/signed-systemd-01/summary.json`, `dcc57a76b229dd78639437f3b0ede1e97a9db994d7095ff2645cccff6aff1f91` | D-Bus invocation `46676ab62af54ddbb4521b429a76647d` for `cohesix-foundation-m27b-systemd-01.service`. |
| NVIDIA Docker | `out/m27b/live-session-11/signed-docker-02/summary.json`, `5b3d3e5e52148809e87cc27ab0e0cad6cb9e1bde13d93629af9f81e261cba5ac` | Engine API 1.56 container `5ce96de7f3e8d3ba4f64c80aa35772c396c0a0c1c81b6c4f8c0c73035bd4e17d`. |

Container image digest is
`sha256:f98584bf0500d678d58ee89a693fee30c8f7305ea1bdd9177332183be90ffee1`.
Each directory's `native-correlation.json` independently checks signed CAS
artifacts against manager identity, exact device, verified output and kernel
controls: 1 GiB memory, 200000/100000 microsecond CPU quota, 64 tasks and
NoNewPrivs. One CUDA context enforces concurrency; child kill/reap enforces the
finite deadline/current grant. Requested allocations and actual allocation/free
memory remain separate from cgroup controls. The 2 GiB reserved OS headroom and
soft CUDA allocation budget are not hardware GPU partitioning. Docker is
unprivileged, read-only, without network or added capabilities. Owned services
and containers were stopped and private signing keys removed.

Both lanes pass gateway and agent retries without changing the verified graph,
plus forged/missing Worker refusal. Systemd graph hashes are
`2c2a23c6e64e3cd53c0497ae14911706964b0fe832dabe18793c6f0cb4279239` (vadd) and
`8e1ac65cd9dd920e05901dc02ad07091b8ce9b4237ed8a9178ab56d8c0907f2b` (matmul).
Docker hashes are
`afa38887cbc972f401c66b81d5db315d4a7cb81468935c624967947d3d020fc7` (vadd) and
`57c42ad7689c51acf81436988c46557c52311bf61354c4753cee7773ac66657d` (matmul).
An initial second-lane attempt reused the first target's ticket history with a
fresh agent and correctly refused an old Worker identity; its FAIL diagnostic
remains in `live-session-10/signed-docker-01`. The passing Docker run uses a fresh
QEMU session with the same immutable image, not relaxed identity checks.

Minimum focused verification reuses the original physical reference negatives
in `mig-native-evidence` (wrong device, stale inventory, oversize, deadline and
cancel), admitted post-first-kernel cancellation in `live-session-02`, and
lost-result-ACK/restart/custody tests and live delayed Worker reconciliation in
`live-session-09`. They retain their earlier component hashes and proof layers.
New finite-cgroup tests and isolated deterministic OOM mapping cover the added
controls without exhausting shared RAM. No full workspace suite or staged
release campaign was run; no stage markers or Pi evidence were synthesized.
Admission remains `operator_approved`; machine-checked admission is unavailable.
Host recovery is not Queen reboot persistence or production ticket-to-bundle proof.

Final focused commands and results under `out/m27b`:

- `foundation-qemu-build-01.log` / `qemu-worker-evidence-12/build.log`: exact
  release-qemu Root/driver/Worker and complete macOS host-tool build PASS.
- `foundation-native-build-01.log`: Linux AArch64 release agent, sidecar and
  GPU bridge with CUDA/NVML/REST PASS.
- `foundation-systemd-live-01.log`, `foundation-docker-live-02.log`: both
  admitted native workload pairs and signed graph verification PASS.
- `foundation-format-01.log`: `cargo fmt --all -- --check` PASS.
- `foundation-clippy-01.log`: GPU bridge scoped Clippy with warnings denied PASS.
- `foundation-matrix-test-01.log`: three canonical matrix routing tests PASS.
- `foundation-inventory-test-01.log`: SDK source test classification regression PASS.
- `foundation-generated-check-03.log`: `scripts/check-generated.sh
  --update-pi4-test-profile` PASS, including surface catalogue, host integration,
  Test Plan and NIST evidence consistency. Earlier logs retain catalogue drift
  and a stale security-nist build-cache path; classification was repaired and
  that one package rebuilt. No check was weakened.
- `foundation-test-plan-02.log`: `scripts/ci/check_test_plan.sh` PASS after
  refreshing manifest fingerprints from generated outputs.

The complete host-tool/SDK/benchmark compatibility review is recorded directly
above. Broader preserved implementation is source-available with explicit
unqualified modes; all five selected foundation tasks are complete.
