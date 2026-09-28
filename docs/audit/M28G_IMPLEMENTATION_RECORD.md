<!-- Author: Lukas Bower -->
<!-- Purpose: Retain the M28g implementation checkpoint and its distinct source, installer and release proof limits. -->
<!-- Copyright 2026 Lukas Bower -->

# M28g implementation and 1.2.0 release record

This chronological record begins with the 1.2.0 source candidate rebased onto
GitHub `main` at `2d8ce6a9f5627b0bb16ffd6c4c9b0619f1d2c749` and ends with
the owner-selected release evidence below. Intermediate statements retain
their original checkpoint date and scope. The 1.1.0-beta release record
remains at its original scope.

## Task record

Title/ID: `m28g-macos-native-installer`, `m28g-ubuntu-arm64-native-installer`,
`m28g-host-clients-as-built-alignment`, `m28g-installation-and-integrated-adoption`,
`m28g-release-b-qualification`, `m28g-mcp-tool-catalog`,
`m28g-a2a-agent-facade`, `m28g-integration-live`,
`m28g-pypi-1.2.0b0-publication`.

Milestone: 28g — Installation, Integrated User Qualification and Release B.

Goal: Build one version-bound host candidate and qualify native installation,
cross-client operation and the assembled release on every selected profile.

Inputs: M28–M28f source and component records; selected compiler inventory and
host integration matrix; 1.2.0 release source; local Mac, Linux ARM64 and
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
case, physical Pi release qualification, integrated Test Plan run or release-owner
approval. A later Pi read/protocol diagnostic is recorded below at its own
source and image identity.

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

At clean commit `522463fa1ade7fcf01092ab4754825fe71dcf5dd` (source digest
`sha256:a730a325a7055c5614e9b93dffc6342db67462046c3183c26e2c1283621dfc73`),
the canonical QEMU and physical Pi 4 plans each passed Stages 01–05. The QEMU
Stage 05 record is under ignored `out/m28g/full-plan-522-qemu`; the Pi record
is under `out/m28g/full-plan-522-pi4-v3`. Both due-diligence gates passed with
the owner's renewed `EX-2026-0024` exception. The earlier failed Stage 05
attempts remain failed at their original identities. Pi runtime/DMA acceptance
uses same-boot UART and authenticated Queen-log captures, whose separate hashes
and composition are recorded in `pi4-stage5-proof-provenance.json`; it is not
one uninterrupted serial capture. These staged results do not qualify a native
installer, Jetson KVM pressure, packaged SD image or assembled Release B.

The `020a516300` to `522463fa1ade` change contains only BUILD_PLAN, STATUS,
HOST_TOOLS and audit prose, two operator skills, and the renewed exception
register/finding. The tracked runtime, manifest, generated contracts, pressure
runner and benchmark workload are byte-identical across those commits. For the
Mac HVF pressure contract, this is a nonmaterial source change: retain the
original medium and high results under `out/m28g-pressure-020a-r1/` in the
pressure checkout, with summary SHA-256 values
`130403cc91d43e064d521cb467437782671cd1621b9bdcb5632a89677fd2cd92`
and `8ccbb60a5678a040bebe098854d6c1321e9d4f6da38d9abc9e34814c279f7cef`.
This equivalence decision does not rename those results as `522463fa1ade`
tests, supply Jetson KVM pressure or apply automatically to later code changes.

During the M28g Jetson KVM preflight, the M26e pressure runner's process-name
probe matched the runner or its launching shell when the pinned QEMU path or
Cargo path appeared in its own arguments. The canonical check stopped before
measurement; selecting the same binary through a short private alias proved
that the original false match was in process scanning. A separate attempt
correctly refused a live log descriptor under `out/`; its rerun writes the
live log outside the checkout. Under `m28g-kvm-pressure-runner-portability`,
the runner now exempts only its own process ancestry and still checks unrelated
processes, open output writers and ports. Bash syntax and 60 focused pressure
tests passed. The Jetson replay at source `522463fa1ade` passed authenticated
NineDoor target operation, three service fault injections, critical duties and
three Worker fault injections. It then failed before medium-load acceptance:
host integration selected `macos-arm64` on the Linux AArch64 KVM host. That run
remains a failed KVM acceptance attempt; it cannot qualify this repair or a
later release source. The selected Linux AArch64 matrix now includes only the
three target-runtime rows, and the runner passes `linux-aarch64` explicitly.
The focused host-profile and pressure CLI tests and a fresh exact-source KVM
replay are required before this restoration task can close.

Six pre-existing Jetson user services were stopped for the KVM quiescence
preflight. Three GPU session services and the original LoRA reference service
restarted. Two older LoRA unit files referenced an absent source checkout;
their service processes therefore failed on restart. The `hf_native.py` file
at that path was restored byte-for-byte from the M28b implementation commit
`8c0147bad1c7dc893bac84d791da0166ec3d2c48` (SHA-256
`b4504c4807f9fc7c3af0d9f0e8981cdf9bc1d110ee16ba45cbcc5040aa6855e7`).
Both units then reported `active/running`. This is test-bed restoration, not
new provider or release acceptance evidence.

On 27 September 2026, Apple issued the G2 Developer ID Installer certificate
`CJ48JZP37S` for team `KB88FQXUX2`, valid through 17 September 2031. Its
public key matches the locally generated CSR; the private key stays in the
login keychain. `security find-identity` lists the installed identity
`5F2F0CCE752EE22658176BBE0A99111F03878A8C`. A disposable probe package
passed `productsign` and `pkgutil --check-signature` with Apple's trusted
timestamp, and the stored `cohesix-m28c` notarytool profile authenticated to
the same team. These checks establish usable signing credentials, not a signed
Release B installer or installed lifecycle acceptance. The available Linux
ARM64 hosts have no selected publisher signing key or public Debian maintainer
identity. Ubuntu 22.04 package candidates were
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

The selected QEMU and Pi manifests now both enable the host MCP/A2A protocol
ceiling. This selection changes the Pi host profile and its generated Python
contract and target fixture. The schema remains disabled when a switch is
absent, and the compiler still requires the master and each protocol switch
to be true. The production-profile test guards the selected values. The
earlier Pi-selected diagnostic run used a separate source candidate with
both protocols disabled and stopped after approximately eight minutes at the
owner's request. Its authenticated TCP and read-only observations remain
diagnostic evidence only. The later enabled Pi run below supplies a fresh exact
image, matching host tools and empty MCP/A2A discovery. It does not supply
native protocol jobs or the M28g integrated matrix required for release claims.

At the earlier beta-source checkpoint, the A2A Agent Card advertised
`1.2.0-beta` and both Python wheels carried `1.2.0b0`. The later stable-version
change below supersedes these packaging values; the earlier observations
remain bound to their original source.

## M28g protocol-mode deployment repair

Discovery task: `m28g-release-b-qualification`. Restoration task:
`m28g-integration-live`. The selected release builds MCP and A2A on, but the
host gateway previously rejected every launch setting for those routes. A
single installed gateway could therefore serve only the both-enabled mode;
the required MCP-only, A2A-only and neither modes needed different compiled
catalogues. The gateway now validates both generated catalogues against their
selected manifest before accepting the exact launch value `false` for the
master, MCP or A2A switch. Each value only closes a route for that process;
other values fail startup, and generated-disabled routes cannot be opened.
The focused live-process test starts the actual gateway binary in all four
modes, checks its route statuses and retained authenticated REST route, and
checks malformed or widening launch values. Three focused tests passed at the
working source. These use a mock Queen and prove gateway routing only; they
are not `m28g-integration-live` native-target, job, revocation or recovery
evidence. Host CLI, Python and benchmark clients retain the same endpoint
contracts and original identities; only gateway route availability narrows.

Under `m28g-installation-and-integrated-adoption`, the conformance matrix and
Test Plan now select a bounded `m28g-adoption-live` verifier. It binds the
assembled release qualification, both native installer results and a named
evaluator's raw, hashed walkthrough at one source commit. Lukas Bower may
evaluate the sole-developer release as owner-evaluated, without an
independence claim. The walkthrough records the three journeys, native desktop
launch, doctor,
data-preserving rollback/uninstall and per-host steps, time, core downloads
and interventions. The numerical limits were frozen in BENCHMARKS before any
integrated case: the comparable 1.1.0-beta archive baselines are
26,501,100 Mac and 28,513,510 Linux bytes. The new adoption verifier reports
`live_host` only and explicitly leaves native target outcome reverification to
the owning provider cases. Focused matrix, parser and catalog checks passed.
At this checkpoint, no evaluator walkthrough or native Release B installer
result existed; later receipts are recorded below, while adoption remains
open.

The `m28g-integration-live` matrix case now starts the exact installed host
gateway against one private-address Queen in all four effective modes. It
requires the same authenticated boot manifest and terminal result bytes after
each process restart. Its retained ordinary MCP and A2A SDK records must share
the original native admission and independently checked CUDA terminal; the
separate subject, revocation, budget, Queen-loss and latency observations must
bind their raw files and stay inside the frozen BENCHMARKS limits. The test
runner rejects public target addresses and passes only credential references
to the gateway launch. Focused mode-selection, identity, malformed input and
latency tests use controlled host fixtures. This is an executable acceptance
contract, not a passing live case: installed 1.2.0 bundles, a final-source
Queen, native result and external observations remain to be supplied and run.

M28g also still requires frozen adoption and quality budgets, a named,
raw-backed clean installation evaluation, the four effective protocol modes,
native CUDA, PEFT, Apple and NeMo journeys from installed bytes, packaged QEMU,
physical Pi installation evidence, and assembled release due diligence. These gates must
bind one final source and package identity before the milestone can be marked
Complete. Material AI assistance produced this implementation checkpoint;
every claim above is limited to the executed source and host checks.

During the requested 60-minute operational burn-in setup, the exact Pi image
builder refused the `d38ab5a4f` source after Pi-profile code generation changed
the MCP and A2A catalogues. Its generated-output allowlist omitted both files.
The builder now classifies those compiler-owned outputs correctly while still
rejecting unregistered changes, and the focused guard test passed. This repair
does not make the in-progress burn-in, Pi image, or release qualification pass.

The first protocol-enabled Pi candidate at `1870cca48` booted from verified
RAM bytes and its first authenticated TCP read returned the selected manifest
hash. The Mac gateway connected, but MCP job discovery and the A2A Agent Card
returned 403 because no native standing ledger was configured on that host.
This is a deployment gap, not a Pi protocol-switch failure. The gateway now
answers authenticated discovery with no job tools, templates or skills when
standing authority is absent; it continues to refuse job admission. A focused
gateway integration test covers both protocol projections. This repair needs a
new exact-source image and host binary for the replacement burn-in; the earlier
boot is diagnostic evidence for its own source only.

## Direct host compute alongside the Pi diagnostic

At source `05e1facdd008240a12dfe133c033912dcc87d5f5`, the selected
`1.2.0b0` NeMo kit wheel was installed without editable source on the Jetson
Orin Nano (`sm_87`, driver 595.78). Its wheel SHA-256 is
`91535a61c428021fe0167087fa8a6e7a5cfddd857ee56bb225364b4bd293f7e2`;
the pinned dependency lock and `pip check` passed. The maintained CUDA
reference runner built the native helper and bridge from that source and
passed real vector addition, matrix multiplication, wrong-device, over-budget,
stale-inventory, timeout and cancel cases. The helper SHA-256 is
`495eb3cd0a8c0591a1c03e59dd32ef3aff2adbf8a29c1ad3c0033a930722271d`;
the bridge SHA-256 is
`72780ce2aff934dbef6eead8ab78f10d4c2b5fe8de94bbb65b4ea5a3cd5de6f4`.
The maintained summary explicitly marks this seven-case reference result
non-authoritative, without a Worker receipt or admitted workload transport.

On the Mac's Apple M4, pinned MLX 0.32.2 ran a real Metal matrix product and
matched every element against an independent NumPy result. The 1.5B Qwen2.5
four-bit model at revision `8b403126fc14f14cfc99bb4cfa72ecbc129ea677`
was copied to ordinary local files and bound by model-tree SHA-256
`c0a2253c519413b712780076c9188676bd006859bfa55bf3acb5818cd3274927`.
The bounded Cohesix MLX service generated the expected one-character answer
to a fixed arithmetic question on Apple M4 Metal, with output SHA-256
`4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a`
and observed 967,437,720 bytes peak Metal allocation.

The Jetson's NeMo Toolkit 1.9.0 then ran its native direct evaluation against
the same Mac model through a protected reverse SSH tunnel and MLX-LM's local
OpenAI-compatible server. Its native profiler recorded `WORKFLOW_START`,
`LLM_START`, `LLM_END` and `WORKFLOW_END`, no error, and the expected answer;
the private evaluation log SHA-256 is
`2104745e1f67d09ce19fe22aa795d6f476e36bccae467c750a751f07e2786a2f`.
An earlier diagnostic request to Cohesix's deliberately bounded MLX service
was refused because NeMo sent system and user messages while omitting explicit
token and temperature fields; its native eval process still exited zero, but
the profiler had no `LLM_END`. That attempt is failed compatibility evidence,
not a successful model evaluation. The second server supports NeMo's request
shape for this direct host check. Neither model text nor the CUDA reference
result is an admitted Cohesix job, signed provider outcome or installed-release
acceptance. Private redacted observations are retained under
`out/burn-in/20260926-m28g-60min-mcp-a2a/` on the Mac and
`/mnt/nvme/cohesix-dev/m28g-burnin-05e/` on the Jetson.

Under `m28g-host-clients-as-built-alignment`, the NeMo kit now refuses a
zero-exit native evaluation when a fixed dataset row has no completed model
response in Toolkit's bounded profiler CSV. The CLI preserves the private
trace and reports `evaluation_failed` with a nonzero process exit; it does not
interpret model text as a provider result. The focused NeMo kit and release
wheel tests passed 12 cases and `scripts/check-generated.sh` passed for this
source repair. The fresh `d3e64749f` wheel at SHA-256
`252e8e7739ec47c2820b08466008bd40197f623920d3bf08bb81f4bfd5b39ada`
passed the compiler-selected source verifier and installed on the Jetson with
`pip check`. The refused service request now exits 2 with
`native_profile_complete=false`; the supported local MLX-LM route exits zero
with a real `LLM_END` and the expected answer. These installed-client runs
remain direct model evaluation, not a Pi admission or release result. The
Pi diagnostic image remains bound to its earlier `05e1facdd` source.

## Timed physical Pi protocol diagnostic

The Pi 4 GENET Queen at source `05e1facdd008240a12dfe133c033912dcc87d5f5`
ran from a RAM-only TFTP image with ID
`8e4212e7a8282c0657ad873b97f36a519dc6bf292b16c4554631bdb0680c7251`
and SHA-256
`3a58570dfc69007c5a002cec7ef22840435581e7a17d4c4482d9bc4b51da64ac`.
The private selected manifest SHA-256 is
`bff6b94e8f5bf66e1521360b400fa6bb0f30330a9310598ac0fbeb225c5a6085`;
master, MCP and A2A are all enabled. Host SHA-256 and Pi RAM CRC checks,
the target `[BUILD]` marker and the first authenticated TCP `/proc/boot` read
bound the live target to those bytes. No SD image was written.

The requested 60-minute read/protocol diagnostic ran from
2026-09-26T08:51:13Z to 09:51:13Z, 3,600.008 seconds. Its 60 minute-cadence
cycles, 19 operator health checks, ten Jetson availability samples and six
delegated read-ticket renewals all passed; there were zero failed cycles and
zero gateway reconnects. Authenticated MCP `tools/list` returned HTTP 200
with `tools=[]`, and the A2A Agent Card returned HTTP 200 with `skills=[]`
and version `1.2.0-beta`. Planned quiet windows occurred at minutes 20–21
and 40–41. The retained event log SHA-256 is
`86229d76b0ab17e29f052f4e8516946be45751a2ca182ddf06fa992aefc3a159`.
The run-owned Mac gateway, protected SSH tunnel and TFTP server were stopped
after the finish event; the Pi RAM image and private evidence were preserved.

This is PASS for the executed physical Pi authenticated-read and empty-discovery
diagnostic. The requested burn-in and M28g release profile remain INCOMPLETE:
the Mac gateway had no co-located standing ledger, so no MCP-only or A2A-only
useful job could be admitted and no native task/provider result or recovery
was measured. The direct Jetson CUDA/NeMo and Mac Metal checks above have
separate host-only proof. The candidate source after the NeMo repair is
`d3e64749f`; the timed Pi image was built from its `05e1facdd` predecessor
and is not final-source target qualification. Full source-bound staged,
installed-client, native package, Worker VM CUDA and release gates remain
open. The private `out/burn-in/20260926-m28g-60min-mcp-a2a/` directory retains
`run-record.md`, `burn-analysis.json`, the event log and host observations.

## Operator skill portability checkpoint

Under `m28g-operator-skill-portability`, the nine repository-managed operator
skills were checked against Mac and Jetson Ubuntu 24.04 execution contexts.
All nine passed `skill-creator` quick validation; 31 local document links and
anchors resolved; 13 Bash snippets passed `bash -n` on each host. The host
integration inventory retained 47 advertised surfaces, 29 dependencies, nine
playbooks and six use cases. `scripts/check-generated.sh` and `git diff --check`
passed. This is instruction and source consistency evidence.

The Mac source-built `cohsh` completed mock Queen `/proc/boot`, schedule and
lease inspection from `/tmp`, including an absolute executable path containing
spaces. Mac source-built `coh` help and mock evidence commands worked from
`/tmp`. On Jetson, retained **1.1.0-beta** `coh` created and inspected a mock
pack from `/tmp`; a separate stale development binary correctly refused a
compiled policy hash mismatch. The pinned NeMo Agent Toolkit 1.9.0 executable
worked by absolute path in a non-login SSH shell where `nat` was absent from
`PATH`. Mac Codex CLI help exposed MCP stdio and Streamable HTTP setup; that
help alone did not establish the two Cohesix HTTP headers or a live job.

The instructions now route copied skills to same-version guides, resolve
installed executables without a source-directory assumption, select one
gateway process for the Queen connection, and require both private headers and
separate verified subjects for multi-user claims. These checks cover two hosts
and two observed agent/client families at limited capability scope. They do
not prove Claude, Gemini or other vendor clients, cross-user isolation,
admitted MCP/A2A work, installed Release B, useful native provider outcomes or
release reliability. Those require the remaining installed-client and live
acceptance matrix at one final source and package identity.

## Full-plan pressure defect and bounded repair

Discovery task: `m28g-release-b-qualification`. Restoration task:
`m26e-host-worker-integration`, for the Root admission and host-agent
delivery boundary found during the selected 26e pressure gate. The initial
`e1fe34f8c` QEMU Test Plan passed Stages 01–04, including authenticated
target integration. Stage 05 failed because `EX-2026-0024` expired on
2026-09-25; the due-diligence subchecks for audit, advisories, attestation,
secret scan and authority floor passed. This exception requires a new human
decision and cannot be revived by a source or documentation edit.

The canonical Mac HVF medium-pressure run at that same source accepted
admissions but left 56 GPU and 57 LoRA Worker receipts pending beyond its
15-second correlated terminal bound. The host ticket agent logged the first
global snapshot gap at expected admission 404 versus observed 405 and then
repeated the gap while newer admissions arrived. The 64-line moving Root
snapshot can omit a sequence between reads; the agent previously could not
recover it, so one lane stopped making progress. The failed run and raw logs
remain in ignored `out/m28g-pressure-e1fe-r2/` in the disposable pressure
checkout. Aggregate HTTP error rate and zero gateway reconnects do not
override the failed Worker receipt gate.

The repair adds a Queen-only exact admission read backed by Root's bounded
256-identity admission window. The agent parses each missing record as a
Root-admitted version-2 spec, checks the exact sequence and follows its
existing execution journal and lane assignment. Unavailable, malformed,
mismatched or 256 or more missing identities fail closed. The gateway sends
recovery reads on the control lane without caching the record. Focused tests
cover recovery, no provider replay, absent-record refusal, canonical path
parsing and gateway routing. This source repair has no pressure, Pi or release
acceptance until the selected plan is rerun against its committed identity.
`coh`, `cohsh`, `tools/cohesix-py` and benchmark result formats consume the
existing ticket identity and result paths; the new read is internal to the
agent's recovery and adds no client action or success state. Their applicable
regressions remain in the Test Plan.

The first full Test Plan attempt at repair commit `2202a95a5` passed Stage
01 metadata, generated consistency, formatting, Clippy and workspace check,
then failed four `cohsh` TCP script tests. The authenticated plan environment
provided `COH_AUTH_TOKEN_REF`; the test children set their fixture
`COHSH_AUTH_TOKEN`, which has lower resolver precedence. Each mock server
therefore waited for a different `AUTH` frame and the scripts timed out at
attach. This is test isolation, not a target or provider failure. The TCP
script harness now removes both higher-priority ambient token variables from
each child, preserving production resolver precedence. All six focused TCP
script cases passed with the private plan token reference deliberately set
in the parent environment. Stage 01 and downstream gates still require a
fresh run at the resulting committed source.

The next full-plan attempt at `d3100e7cb` advanced through the same Stage 01
Rust and target-build actions. Its Python action completed 2,915 tests and
116 subtests, with two failing REST harness argument tests. Those tests cleared
the inline console token variables but inherited the plan's private
`COH_AUTH_TOKEN_REF`, so their expected empty token assertion was false. The
tests now also clear that reference in their fixture environment; both focused
cases pass with the private reference deliberately present in the parent.
Production token precedence is unchanged. The failed Stage 01 attempt is
retained under `out/m28g/full-plan-d310-qemu/`; no later stage or release
acceptance follows from it. Fresh full-plan and pressure runs remain required.

The committed `2202a95a5` pressure rerun passed the exact QEMU image,
authenticated control operation, fault injections and live Worker preflight,
then failed the medium workload: 78 GPU and 55 LoRA operations lacked terminal
Worker receipts. The first missing admission was sequence 163. The gateway
recorded repeated `ERR CAT reason=quota detail=buffer-full` for the exact
admission read, so the agent could not recover the gap and eventually exceeded
Root's bounded identity window. The read had serialized the admitted JSON as
one console line even when it exceeded the line cap. It now uses the existing
bounded CAT chunk framing; the production-feature Root test reconstructs a
real admitted spec from its frames and passes. The failed medium run remains
under `out/m28g-pressure-2202/` in its disposable checkout. A new exact-source
pressure result is required before this repair can be accepted.

At `0024e6995`, the complete QEMU plan passed Stages 01 and 02, including the
full Python gate and target-qualified Root build. Stage 03 built both exact
images and passed thirteen preceding authenticated scripts, then stopped in
`shard_1k.coh`: `WAIT TAIL` read a Worker telemetry path before asynchronous
Worker construction and received `ERR TAIL ... invalid-path`. The script's
read-condition contract correctly treats that refusal as final. The fixture
now waits for the role-specific `WORKER_TASK_READY` record on the existing
Queen log before reading either sharded Worker path; the public CLI guide
documents that order. The six script-catalog tests and generated-consistency
check pass. The failed Stage 03 attempt is retained under
`out/m28g/full-plan-0024-qemu/`; it does not establish complete transport
acceptance. A fresh source-bound plan run must confirm the changed script.

At `1679e23d7`, a fresh canonical pressure run rebuilt the selected Mac HVF
image and passed same-boot preflight in separate medium and high boots. The
medium report recorded 48,869 operations with zero errors and 256 successful
correlated Worker receipts; the high report recorded 72,885 operations with
101 bounded lease-quota refusals (0.1386%, under its frozen 1% budget) and
256 successful correlated Worker receipts. Medium covered 144 GPU and 112 LoRA
operations; high covered 132 GPU and 124 LoRA operations. These immutable run
summaries are retained under `out/m28g-pressure-1679-r2/` in the disposable
checkout. The runner
then entered the complete staged QEMU plan, where Stage 01 passed metadata,
generated consistency, formatting, Clippy and workspace check but failed the
`shard_1k` host-model integration tests. The updated script correctly waited
for a target `WORKER_TASK_READY` record; the synchronous NineDoor host model
created its Worker namespace without emitting an equivalent readiness record.
The host model now logs a role-specific `WORKER_TASK_READY` with explicit
`mode=host-model` after creating the namespace. Both focused shard tests pass,
including the disabled-alias refusal. The pressure runner did not reach its
final collector or establish full Test Plan acceptance at `1679e23d7`; the
repair requires a new exact-source run. Host CLI, Python and benchmark result
schemas are unchanged; the extra mock log record is explicitly labelled and
does not satisfy native Worker evidence.

On the Jetson Orin Nano at that same source, a clean native CUDA helper build
and bridge build passed all seven direct reference cases. The maintained
conformance summary remains `INCOMPLETE` for production: it has no admitted
Worker transport, physical lease/revoke, bridge restart, authoritative result
graph, Worker receipt or NVIDIA container-lane proof. The direct result is
retained under `/mnt/nvme/cohesix-dev/m28g-1679-source/out/m28g-1679-cuda-conformance/`.

## Earlier exact-source 020a qualification

This earlier checkpoint is clean commit `020a516300b75e55f91606a77106c76ef74f665c`,
source digest `sha256:a098dd349e07e80249019f6b915289baa826dfda865e6c273e26b41d8b117b64`.
It includes the Root admission-read and host-model readiness repairs above. The
selected QEMU and Pi production manifests have the master, MCP and A2A switches
enabled; both selected Cohesix and NeMo kit wheels carry `1.2.0b0`, the package
version for Release B `1.2.0-beta`.

The canonical QEMU Test Plan passed Stages 01–04 at this checkpoint,
including the full common host gate, exact-image target build, authenticated
TCP regressions and REST multiplexer. Stage 05 verified the earlier stage
attestations and passed its audit and advisory checks, then failed the release
guardrail because `EX-2026-0024` expired on 2026-09-25. Its immutable attempt
is under `out/m28g/full-plan-020a-qemu/`. A separate exact-source Mac HVF
pressure run passed medium and high: 49,598 operations with zero errors and
72,268 operations with 61 bounded errors respectively, 256 correlated
successful Worker receipts in each run and no gateway reconnects. The high
error fraction was 0.0844%, within the frozen 1% budget. Its evidence is under
`out/m28g-pressure-020a-r1/` in the pressure checkout. Pressure success does
not renew an expired risk exception. The existing `EX-2026-0024` controls also
require Jetson KVM medium/high pressure at the selected identity. That lane has
not run at `020a516300`: the Jetson checkout has KVM access and the pinned QEMU
binary but lacks the governed seL4 pressure build and image. Its runner's
read-only preflight stopped on missing `out/sel4`; a prior-source KVM build
cannot be relabelled as this checkpoint's pressure evidence.

The physical Pi 4 GENET booted a private RAM image with SHA-256
`630b35269c483df678c1ef1cad936c04c007297e4587a97fd339e45907fe3ec6`
and the same `[BUILD]` source identity. Its 60-minute MCP/A2A diagnostic ran
for 3,600 seconds with four MCP and six A2A original jobs confirmed and
acknowledged by the Pi target, native Jetson CUDA outputs independently
verified, 122 health samples and no failed health events. A planned publisher
epoch handoff at 30 minutes completed with a fresh Worker generation and
resource reservation. Three preceding attempts remain recorded as failures:
standing-scope attempt exhaustion, publisher epoch removal of the GPU
reservation, and delegated caller-ticket operation exhaustion. The fourth
attempt's collector was amended during its timed window to wait for exact
Worker READY telemetry and bind the fresh generation; its executed helper
hash and the deviation are recorded in the private run directory. The final
event log SHA-256 is
`49dbd1d5eec70d976e3e1656d3accb2fb92120b2f06fdc99671acd48e033531c`.
This is a completed diagnostic with a disclosed run deviation, not an
unchanged frozen installed-release profile: the Pi ran from RAM, and the run
did not cover installed Mac/Linux packages, FUSE, native NeMo protocol clients,
PEFT or diverse vendor agents.

The same Pi source/image then passed canonical Stages 01–04. Stage 03 ran 17
authenticated TCP scripts across four fresh RAM boots, and Stage 04 verified
same-boot continuity before CLI and Python REST checks through a bound Mac
gateway. The first Stage 03 preflight and Stage 01 retry exposed stage
environment selection errors; both failed attempts remain retained, and the
corrected stage-stable environment produced the recorded passes. Stage 05
failed the same `EX-2026-0024` release guardrail. The attempts and exact
source/image attestations are under `out/m28g/full-plan-020a-pi4/`. These
RAM-boot checks do not establish a cold SD installed-image series. The
run-owned gateway and Jetson services were stopped after their tests.

Direct host checks at this checkpoint passed seven native Jetson CUDA
reference cases, an Apple M4 MLX Metal matrix product with independent NumPy
parity, and NeMo Toolkit 1.9.0 direct evaluation against the Mac MLX-LM model
endpoint. The CUDA reference conformance summary remains `INCOMPLETE` where it
requires admitted Worker transport; the direct MLX and NeMo results also do
not establish Cohesix admission or an installed Release B client matrix. Their
retained evidence is in the checkpoint's ignored `out/m28g/` roots and the
Jetson source checkout.

Under `m28g-operator-skill-portability`, the operator guidance now distinguishes
standing-scope cumulative attempts from caller-ticket operation quotas and
requires a fresh Worker reservation after a publisher epoch change. The
amended delegation and GPU skills passed `skill-creator` validation, and the
implementation inventory still matches the selected release. The earlier
nine-skill Mac/Linux syntax, links and mock checks remain limited to those
setups; they do not prove live Cohesix operation across AI-agent vendors.
This guidance refinement changes no shipped host tool, Python API or benchmark
measurement contract; the 47-surface host integration inventory still agrees
with the selected compiler output.

After the failed 020a Stage 05 attempts, Lukas Bower renewed the existing
`EX-2026-0024` P2 scope on 2026-09-27 through 2026-10-27, as recorded in the
[exception register](EXCEPTIONS.md#ex-2026-0024-renewal-2026-09-27). The
earlier attempts remain failed. The fresh `522463fa1ade` QEMU and Pi Stage 05
governance results above close their staged lane; the selected Jetson KVM
pressure control remains unexecuted.

## Optional QEMU setup and Python index preparation

Under `m28g-macos-native-installer`, `m28g-ubuntu-arm64-native-installer` and
`m28g-pypi-1.2.0b0-publication`, the release setup now makes QEMU an explicit
`--with-qemu` choice. Its Python probe starts the selected four-core HVF or KVM
machine briefly, rather than accepting version and accelerator listings alone.
The Mac fallback builds from the pinned upstream QEMU 10.1.0 source archive
and the selected HVF state-sync patch in a user-owned prefix; it is separate
from the native Cohesix installer. Ubuntu ARM64 selects apt's
`qemu-system-arm`. The repository setup skill and quickstart describe that
choice and distinguish a startup check from a booted, authenticated guest.
On Mac, installed Homebrew QEMU 11.0.3 advertised HVF but aborted this startup
probe; the previously selected patched 10.1.0 passed. On Jetson Ubuntu 24.04,
apt QEMU 8.2.2 booted the earlier exact-source 522 guest and completed an
authenticated Queen ping. Neither check qualifies every advertised host.

The Python index workflow now selects the future annotated `v1.2.0-beta` tag
on main, builds `cohesix` 1.2.0b0 from the compiler-selected source inventory,
retains wheel/sdist hashes, and hands the exact files to the protected `pypi`
environment for OIDC publishing. The source README carries the matching pip
command. A local candidate wheel/sdist build, normalized version check,
Twine metadata check and 38 focused setup/release tests passed. An isolated
copy of the bundle setup installed the 1.2.0b0 wheel, selected the known-good
Mac QEMU binary and passed `--check --with-qemu`. The new probe also passed on
the Jetson's apt 8.2.2 KVM binary, where the copied setup script passed
`--check --headless --with-qemu` without requiring graphical runtime packages.
The first from-source Mac fallback build
failed because QEMU's unused Apple Paravirtualized Graphics module calls APIs
obsoleted by the installed macOS 27 SDK; the builder now disables that feature,
and its corrected build is pending. Public PyPI upload remains pending the
final approved release tag and independently checked public hashes. The
generated inventory was refreshed through `coh-rtc` and
`scripts/check-generated.sh` passed.
The [current PyPI project](https://pypi.org/pypi/cohesix/json) still lists
`1.1.0b1` and the `Cohesix` user owner, with no organization owner; the
historical organization request and transfer remain pending. GitHub's `pypi`
environment still requires the named `lukeb-aidev` reviewer. The new workflow
passed `actionlint` before publication.

On 27 September 2026 the release owner changed Release B from the planned
`1.2.0-beta` to stable `1.2.0`. The source inventory, both wheel metadata,
NeMo kit, A2A Agent Card, installer names, release notes, PyPI workflow and
build-plan acceptance text now select the stable version. Earlier
`1.2.0-beta`/`1.2.0b0` observations above remain historical evidence for
their original source and artifacts. They do not qualify the new `1.2.0`
candidate; source-bound release checks, installed package checks and live
acceptance must be repeated on the exact stable source.

The release owner also authorized a dedicated Debian signing key. A new
passphrase-protected Ed25519 key was created in a separate macOS GPG home,
with its passphrase in the login keychain. Its fingerprint is
`7E27A4AB355D2EA565718CA41059A516E53B0B70`; an independent `gpgv`
probe verified a detached signature against the exported public key.
The public key copy is tracked under `releases/`. This probe establishes key
control and verification mechanics, not a signed or qualified `.deb` package.
The encrypted key was imported into the selected Jetson builder's private GPG
home. A Jetson-created detached signature was independently verified on the
Mac with `gpgv` against the exported public key. The native builder can read
the passphrase through inherited standard input, keeping it out of arguments,
environment values and the package payload.

The pinned Mac QEMU 10.1.0 source builder completed after preserving upstream's
`com.apple.security.hypervisor` signing entitlement on the staged executable.
Its new user-owned prefix passed the four-core HVF startup check and
`scripts/setup_environment.sh --check --with-qemu`; the codesign entitlement
was read back from the final binary. Homebrew 11.0.3 remains the observed
failing selection on this Mac. This does not yet prove a booted stable
`1.2.0` Queen or every supported Mac.

The stable source's focused Python release, NeMo, installer, setup and archive
checks passed after a NeMo wheel filename selector and packaged public-key
link were corrected. `cohesix-1.2.0` wheel and sdist and the
`cohesix_nemo_kit-1.2.0` wheel built from the selected inventory; strict
Twine metadata passed for the SDK distributions. The selected A2A Agent Card
version Rust test and an isolated stable SDK install, import and both CLI help
checks passed. `actionlint`, `shellcheck`, `git diff --check` and
`scripts/check-generated.sh` passed. All are source checks; the stable
candidate's installed and live acceptance gates remain outstanding.

M28g remains **In Progress**. The `522463fa1ade` staged results are source
and target evidence, not qualification of the installed release.
Native signed/notarized Mac and independently signed Ubuntu ARM64
packages, clean installs on every advertised host, installed-client parity,
the full live acceptance matrix, frozen adoption and quality budgets, and
named release-owner approval remain absent. No exact assembled Release B
candidate or publication is claimed. Material AI assistance produced this
checkpoint and its scoped validation record.

## Pi 4 SD refresh compatibility

Title/ID: `m28g-release-b-qualification`.

Milestone: 28g / m28g-release-b-qualification.

Goal: Refresh the selected physical Pi card without relaxing whole-disk,
partition, filesystem or private-policy checks.

Inputs: Removable 63.9 GB SD card in the Mac's built-in reader; one MBR FAT32
`COHESIX` child; exact staged Pi image; the card's existing private boot policy.

Changes:

- `scripts/pi4-image-build.sh` accepts macOS `diskutil`'s observed
  `Windows_FAT_32` content spelling as well as `DOS_FAT_32`, and still requires
  the mounted child's `msdos` filesystem type, exact parent and label.
- `tests/test_pi4_image_build.py` exercises both valid spellings through the
  non-destructive refresh, and confirms a non-FAT32 child or mismatched
  mounted filesystem is rejected before the card is copied or its policy
  changes.

Commands: `.venv/bin/python -m pytest -q tests/test_pi4_image_build.py`;
`bash -n scripts/pi4-image-build.sh`; `scripts/check-generated.sh`;
`git diff --check`.

Checks: An existing exact FAT32 child is refreshed without repartitioning;
other filesystems are refused; the existing private policy is preserved and
read back; physical proof remains subject to a fresh exact-image boot and
separate hardware acceptance.

Deliverables: Compatible flash validator, regression tests and retained
failure logs under ignored `out/m28g/`. AI assistance identified and repaired
the observed macOS spelling mismatch; no physical acceptance is inferred from
the focused host tests.

## Staged fixture isolation and idle Pi menu

Title/ID: `m28g-release-b-qualification`.

Milestone: 28g / m28g-release-b-qualification.

Goal: Keep synthetic Test Plan stages isolated from live host selectors and
allow the Pi operator helper to read an already idle U-Boot menu.

Inputs: The source-bound `eb1b6f03c9d5` Stage 01 attempt, which retained two
Python fixture failures when `COHESIX_GATEWAY_URL` leaked into fake Stage 04;
the exact-image Pi first boot, whose U-Boot menu was visible before the serial
helper opened the port.

Changes:

- `scripts/ci/test_test_plan_evidence.py` removes inherited gateway, Pi, QEMU
  and target-evidence selectors from its synthetic runner environment before
  applying each test's explicit overrides.
- `scripts/pi4_serial_reboot.py` redraws an already displayed choice menu with
  an invalid, non-persistent selection before reading its state. The helper
  still refuses to choose a lane until it has read the current menu page.
- `tests/test_pi4_serial_reboot.py`, the Test Plan fixture test and
  `docs/HARDWARE_BRINGUP.md` cover and explain those behavior boundaries.

Commands: `COHESIX_GATEWAY_URL=http://127.0.0.1:18080 .venv/bin/python -m
pytest -q scripts/ci/test_test_plan_evidence.py -k
'stage_five_refreshes_while_other_stages_resume or
upstream_rerun_invalidates_downstream_active_evidence or
fixture_isolates_external_target_selectors'`; `.venv/bin/python -m pytest -q
tests/test_pi4_serial_reboot.py -k
'run_returns_nonzero_after_diagnostic_failure or
initial_menu_state_reads_current_menu_before_selecting_genet'`;
`git diff --check`.

Checks: The three selected staged-fixture tests pass under an inherited live
gateway URL, and both selected serial-helper tests pass. The failed Stage 01
attempt remains historical; a new exact-source full Test Plan and Pi boot are
still required after this source repair.

Deliverables: Hermetic Test Plan fixture, menu redraw behavior, operator
guidance and preserved failed/PASS logs. AI assistance investigated and
repaired both observed host workflow defects; no staged or release acceptance
was inferred from the focused checks.

## Detached Jetson KVM replay checkout

Title/ID: `m28g-kvm-pressure-runner-portability`.

Milestone: 28g / m28g-kvm-pressure-runner-portability.

Goal: Run the selected non-cleaning Linux KVM replay from an isolated,
exact-commit Jetson checkout without presenting that checkout as `main`.

Inputs: The `1a9dc310db4a` guest and clean Jetson source checkout, the
canonical `--reuse-artifacts --check-only` refusal, and the retained
`m28g-kvm-pressure-runner-portability` branch and quiescence contracts.

Changes: The pressure runner applies its `main` branch rule only to the
normal clean-build lane. The replay lane still requires the canonical
transferred output paths, KVM/GDB/`nm`, immutable guest verification,
exclusive output ownership, quiescent host and port checks. A focused test
exercises detached replay acceptance and preserves clean-lane refusal.

Commands: `bash -n scripts/m26e_qemu_pressure.sh`; `.venv/bin/python -m
pytest -q tests/test_m26e_qemu_pressure_cli.py -k
'replay_accepts_detached or quiescent_probe_ignores_runner_ancestors'`;
`.venv/bin/python -m pytest -q tests/test_m26e_qemu_pressure_cli.py` (53
passed); `scripts/check-generated.sh`; `git diff --check`.

Checks: Both focused tests pass. The initial Jetson check-only attempt
correctly refused the detached checkout before this repair. No KVM replay,
pressure result or Release B acceptance is inferred from that preflight.

Deliverables: Honest detached replay selection and regression evidence;
the exact-source Jetson pressure result remains pending.

## A2A catalogue release selection repair

The first canonical `1.2.0` bundle preflight at `bc2f7f80399f` refused the
candidate because the compiler generated `a2a_catalogue.json` but the release
inventory omitted it. This was a real host archive selection defect: the MCP
catalogue was selected while the A2A catalogue would not be shipped. The
`m28g-a2a-catalogue-release-selection` repair adds the A2A catalogue to the
compiler source inventory and a focused release test. Regeneration changes
the inventory, host graph, use-case evidence, MCP/A2A catalogues and provider
projections through their normal hash bindings. The earlier `bc2f7f80399f`
QEMU/Pi boots and protocol observations remain evidence for that source, not
for the repaired bundle. The focused release suite passed 22 tests and
`scripts/check-generated.sh` passed after regeneration. The compatibility
review found no changed CLI, Python call shape, gateway route or benchmark
threshold. The Rust and Python provider projections embed the new graph
binding, however, so the earlier native host binaries, Python wheel and
target artifacts are stale as release inputs. The NeMo kit's source inventory
binding also needs renewed verification. The focused generated-contract and
release suites passed 49 tests; `cohesix-authority` passed 16 unit tests.
Exact repaired-source bundles and their installation checks remain
outstanding.

## Portable production profile for both release builders

Title/ID: `m28g-release-b-qualification`, discovered while preparing the
`m28g-ubuntu-arm64-native-installer` Jetson artifact.

Milestone: 28g / m28g-release-b-qualification.

Goal: Let Mac and Jetson compile the same exact production manifest and public
CAS verification key without recreating a developer-specific filesystem path.

Inputs: The `7c29b8cbb211` QEMU/Pi production profiles; the passing exact
source Mac/Pi release preflight; the source-bound JetPack 7.2.1 ARM64 host-tool
build; the `coh-rtc-authority-profile` output containing a Mac absolute key
path. The `7c29b8cbb211` physical Pi RAM boot, MCP/A2A smoke and QEMU base
transport remain valid only for that original manifest and source.

Changes: The authority-profile CLI requires the deployment public key below
the new profile output directory, checks it there, and serializes a path
relative to that directory. The private signing key remains outside the
profile. The security and authority guides show how to place and move the
public key with a generated profile. Focused tests cover a relocated profile
and refusal of a key outside the selected output directory.

Commands: `cargo test --locked -p coh-rtc --test authority --bin
coh-rtc-authority-profile`; both QEMU and Pi profile CLI generations into a
private ignored output directory; `cargo fmt --package coh-rtc -- --check`;
`scripts/check-generated.sh`; `git diff --check`.

Checks: Seven focused Rust tests passed. Both generated profiles retain
production authority, MCP and A2A while recording the same relative public
key path. Generated consistency passed. New exact-source target artifacts,
native packages, installed use cases and release acceptance remain pending;
earlier artifact identities are not promoted to the repaired source.

Deliverables: Portable profile generator, documentation and retained focused
checks. AI assistance identified and repaired the cross-host build defect.

## EX-2026-0025 bounded renewal for Pi release qualification

Title/ID: `m28g-release-b-qualification`.

Milestone: 28g / m28g-release-b-qualification.

Goal: Restore a current owner decision for the existing Pi direct-GENET P2
unsafe boundary without weakening its controls or promoting an earlier test.

Inputs: The expired `EX-2026-0025` and `DD-2026-0025` entries; the retained
Pi Stage 05 failure at source `7d1ceedb4772`; the selected direct-GENET
boundary; Lukas Bower's 28 September direction to fix the boundary or extend
the exception through 31 October.

Changes: AI-assisted review found no verified safe replacement for fixed
mapped-page atomic access, sealed descriptor reads, suspended-child bootstrap
aliases or terminal fault transfer. The named owner renewed the same P2 scope
through 2026-10-31 in the exceptions and findings registers. The original
unsafe bounds, compensating controls and independent review requirement remain.

Commands: `scripts/ci/due_diligence_gate.sh --check-exceptions-register
docs/audit/findings.csv docs/audit/EXCEPTIONS.md`; `scripts/ci/due_diligence_gate.sh
--check-blocking-findings docs/audit/findings.csv docs/audit/EXCEPTIONS.md`;
`.venv/bin/python -m pytest -q scripts/ci/test_due_diligence_lifecycle.py`;
`git diff --check`.

Checks: Both register gates accept the dated decision, the lifecycle suite
retains expired-exception refusal, and the source delta contains no runtime,
manifest or acceptance-threshold change. The failed Pi Stage 05 attempt is
historical and must be rerun against a clean exact source.

Validation: The register and blocking-findings gates passed; the lifecycle
suite passed 36 tests and 51 subtests. The exact-source staged rerun remains
pending at this checkpoint. No Pi or release PASS is inferred from the renewal.

Deliverables: A bounded, owner-authorized renewal and separate rerun evidence;
no waiver of physical Pi, KVM pressure or installed-release qualification.

## Exact Pi stage binding and KVM fault-probe repair

Title/ID: `m28g-release-b-qualification`.

Milestone: 28g / m28g-release-b-qualification.

Goal: Keep physical Pi proof tied to the image that actually booted and make
the pre-READY KVM fault probe reach a target-observable standard fault.

Inputs: The `5de8fdc4ebb6` Pi RAM boot, its passing Stage 03 and Stage 04
records, and the Stage 05 refusal of a missing live runtime/DMA artifact;
the Jetson KVM pressure preflight where a GDB `zero-x0` marker was followed
by a Worker `ready-timeout` rather than the required Standard fault.

Changes: `pi4_gate_proof.sh` now accepts the exact `--stage-dir` used to boot
the Pi. Before issuing runtime/DMA proof it checks the staged image digest,
stage proof, identity metadata and live `[BUILD]` marker together. This
prevents its former default stage path from silently linking a different
image. The external GDB pre-READY probe now redirects the role-bound Worker
entry to its existing standard-fault hook. The validator still requires the
role- and phase-matched fault from the target; an injection transcript alone
does not pass. The Test Plan names the exact stage input.

Commands: `bash -n scripts/pi4_gate_proof.sh`; `.venv/bin/python -m pytest -q
tests/test_pi4_gate_proof.py`; `.venv/bin/python -m pytest -q
tests/test_worker_task_evidence.py tests/test_rest_perf_harness.py`;
`scripts/pi4_gate_proof.sh --normalize-only --stage-dir
out/m28g/pi4-stage-5de8-dev --manifest
out/m28g/release-eval-5de8-dev/pi4-development.toml --log
out/m28g/full-plan-5de8-pi4-dev/pi4-stage5-base-boot-and-queenlog.raw.log
--require-wired-ready --require-driver-task-proof --runtime-dma-proof-out
out/m28g/full-plan-5de8-pi4-dev/pi4-runtime-dma-proof.env`.

Checks: All 81 Pi proof tests and 404 Worker/performance evidence tests passed.
The exact older Pi boot plus same-boot authenticated Queen log normalizes to
`PI4_RUNTIME_DMA_PROOF=fresh-pi` and `COUNTER_PROOF=counter-qualified`, with
the correct image stage linked. This is a diagnostic run over retained bytes;
it is not the active controlled serial/packet capture required by physical
performance acceptance. The KVM probe change has focused host coverage but
still requires a fresh Jetson run to prove a target Standard fault.

Validation: Source-bound staged tests, physical Pi release profile, KVM
pressure and assembled release acceptance must be rerun after this commit.
The earlier `5de8` Stage 03/04 and failed Stage 05 records stay immutable at
their original source identity.

Deliverables: Exact-stage Pi proof linkage, a fault-probe correction and
explicitly bounded regression evidence. AI assistance identified and
implemented these repairs; independent target qualification remains open.

## Strict-production TCP release input

Title/ID: `m28g-production-tcp-release-proof`; downstream discovery in
`m28g-release-b-qualification`.

Milestone: 28g / m28g-production-tcp-release-proof.

Goal: Keep production Queen authority strict while producing a native QEMU TCP
result that the canonical release factory can bind to the shipped guest.

Inputs: The `4a46989432ae` production Pi TFTP/RAM boot and Stage 01–02 PASS;
the retained Stage 03 failure at `base-telemetry/telemetry_ring.coh`, where
`spawn heartbeat` received the production policy's `EPERM` for `/queen/ctl`;
the development suite's separate legacy-control contract.

Changes: `run_regression_batch.sh` now selects nine production-safe base
scripts only under an explicit, validated strict-production mode and gives
that boot `qemu.production-tcp-smoke` identity. The selector refuses an
incorrect target, group, action, authority policy, MCP/A2A or standing control.
`release_inputs.py` requires the complete safe script set and matching
artifact/action instead of promoting a development Stage 03 result. The
catalog, Test Plan and host release guide distinguish the two proof lanes.
The full development suite and the production denial remain intact.

Commands: `bash -n scripts/cohsh/run_regression_batch.sh`;
`.venv/bin/python -m pytest -q scripts/ci/test_run_regression_batch.py
tests/test_release_inputs.py`; focused catalog and release-bundle tests;
`scripts/ci/test_plan_catalog.py validate` and `check-doc`;
`scripts/check-generated.sh`; `git diff --check`.

Checks: The selector and release-input tests passed 39 cases, focused catalog
and bundle tests passed 37 cases with two subtests, and catalog/generated
consistency passed. No native strict-production QEMU boot has passed at this
checkpoint. The `4a469` production Pi refusal is a failed suite/profile
selection, not evidence that production should enable legacy Queen control.
All target, installed-package and release acceptance gates remain open until
fresh exact-source results pass.

Deliverables: A production-safe proof path and retained failed discovery;
native Mac/Jetson replay and separate physical Pi acceptance are pending.

## Production Pi NETSTATS response repair

Title/ID: `m28g-release-b-qualification`; defect discovered during the
production Pi 4 authenticated TCP acceptance run.

Milestone: 28g / m28g-release-b-qualification.

Goal: Return the complete bounded `netstats` response on the physical Pi 4
without changing the existing namespace or log stream batch contract.

Inputs: Source `5cd8e9ef057f` and its production Pi TFTP/RAM image at
`out/m28g/pi4-stage-5cd8-release-prod`; the retained authenticated TCP
`netstats` refusal in `out/m28g/pi4-prod-5cd8-boot2/netstats.out.log`; and
the same boot's successful 67-body-line serial diagnostic in
`out/m28g/pi4-prod-5cd8-boot2/serial-netstats.log`.

Changes: The root event pump previously held synchronous TCP diagnostic bodies
in a 64-line log-export batch. A live wired Pi emits 67 `netstats` body lines,
so capture replaced the whole response with a typed bounded-overflow error.
Synchronous capture now retains up to the existing 69-line physical-console
body bound. Five extra lines are held separately and drained after the ordinary
64-line batch. Log and namespace streams keep their prior 64-line type and
limit; overflow still fails with one typed terminal.
The host-tool, Python, SwarmUI and benchmark compatibility review found no
client grammar, wire terminal, status, schema, or benchmark definition change;
fresh Pi replay must establish that the selected diagnostic arrives complete.

Commands: `cargo test -p root-task --lib --no-default-features --features
release-pi4 bounded_sync_capture_`; the same command with `release-qemu`;
`cargo fmt --all -- --check`; `git diff --check`.

Checks: Both feature profiles passed the three focused capture tests, including
ordered delivery of 67 body lines and one terminal and deterministic refusal
at the 70th line. Fresh production Pi image build, authenticated TCP replay,
full staged plan and release acceptance remain required for the repaired
source. The prior `5cd8` boot and all its valid observations retain their
original source and image identity; they do not accept this change.

Deliverables: Bounded synchronous-response repair and host-level regression
proof. AI assistance identified the root cause and implemented the repair;
independent physical and release evidence remains open.

## Linux production host-policy selection

Title/ID: `m28g-linux-host-policy-selection`; discovery in
`m28g-production-tcp-release-proof`.

Milestone: 28g / m28g-linux-host-policy-selection.

Goal: Bind native Linux host clients to the selected production KVM policy.

Inputs: Clean source `547a65a9fd4a`; dedicated KVM guest at
`out/m28g/kvm-prod-547a-mac`; native Linux host-tool manifest at
`out/m28g/linux-host-tools-547a.json`; retained failed Jetson TCP attempt at
`out/m28g/jetson-production-tcp-547a`.

Changes: The first Jetson attempt booted the dedicated KVM guest and passed
the authenticated TCP response matrix, then `cohsh` refused `boot_v0.coh`:
the binary expected policy SHA-256 `d5a5d49e62abb9a7353cb8fcb7c8d75df30550174d16546b108516331e8699ad`
while the selected production artifact supplied
`f77a645d43347d50eaef1e832b1b729918d1818d1b7f603b4988928036cd321c`.
The native builder generated host policies from `configs/root_task.toml`
instead of the guest's selected production manifest. The builder now accepts
that manifest as an explicit input, verifies its transfer digest, and records
the manifest and generated policy hashes. The release factory forwards its
selected manifest and checks native-tool provenance against it. No policy
validation or production authority was relaxed.
The first selected-manifest rebuild refused before host compilation because
the compiler resolved a public CAS key relative to the temporary manifest in
the remote build root. Staging the manifest at the source root exposed the
tracked fixture public key and production validation correctly refused it.
The builder now transfers only the manifest's referenced public verification
key beside the external manifest, independently verifies both digests, and
leaves the tracked source tree unchanged.

Commands: `bash -n scripts/linux_host_tools_sync.sh scripts/release_bundle.sh`;
`.venv/bin/python -m pytest -q tests/test_linux_host_tools_sync.py
tests/test_release_bundle.py`; `scripts/check-generated.sh`; `git diff --check`.

Checks: Shell syntax passed, 31 focused tests passed and generated consistency
passed for the repair working tree. The original KVM attempt remains failed.
Fresh clean-source Linux host-tool build, native KVM TCP replay and all
assembled-release gates remain pending.

Deliverables: Corrected native builder and factory binding, focused tests,
host guide and retained failure. AI assistance traced and repaired the
builder mismatch; no target acceptance is inferred from the repair tests.

## Exact-image Pi Wi-Fi smoke

At source `547a65a9fd4a`, the production Pi image in
`out/m28g/pi4-stage-547a-release-prod` was loaded through TFTP/RAM with three
completed transfers and post-reset CRC checks. The SD card's saved policy
remained Ethernet, so a private local Wi-Fi policy was applied for this boot
through a redacted serial handoff without writing the card. The retained
`wifi-smoke-private-ram-attempt3` receipt binds the exact build marker and
records a settled Wi-Fi supervisor with no runtime recovery. Same-boot serial
`netstats` reports active Wi-Fi and a bound DHCP lease. The first TCP client
attempt was rejected because its transport token was a minted ticket; the
retry used the selected transport credential and received `OK AUTH`, Queen
attach and `OK PING reply=pong`. Retained serial/TCP logs were checked for
the private Wi-Fi values and Queen credential. This is a quick same-image
Wi-Fi smoke, not the full physical Pi, pressure, repeatability or final SD
release gate. The subsequent source change for Linux host-tool selection
requires its own exact-source qualification.

## Debian publisher key alignment

Title/ID: `m28g-debian-publisher-key-alignment`; discovery in
`m28g-ubuntu-arm64-native-installer`.

Milestone: 28g / m28g-debian-publisher-key-alignment.

Goal: Ship the public verification key that corresponds to the selected
private 1.2.0 Debian signing key.

Inputs: The earlier Ed25519 key probe recorded above, the selected dedicated
passphrase-protected RSA publisher key, the tracked public export and install
guidance.

Changes: The earlier public key did not correspond to the private key selected
for the final native signing flow. Its probe remains a valid historical result
for that identity, but it cannot establish trust in a signature made by
another key.
The tracked public export and reader-facing fingerprint now identify
`684071A2AF498008930CF6AEEBB8ECC49397F8C2`. The private key and
passphrase remain outside the repository. No package or release acceptance is
inferred from changing the public trust file.

Commands: `gpg --show-keys --with-colons
releases/cohesix-debian-publisher-2026.asc`; independent `gpgv` signature
verification; focused installer and bundle tests; generated consistency.

Checks: The tracked key parses with the selected fingerprint. A fresh
public-only `gpgv` probe accepted a detached signature from the selected
private key and rejected it with the earlier tracked key; its receipt is at
`out/m28g/debian-publisher-key-alignment/result.json`. All 26 focused
installer/bundle tests, generated consistency, Test Plan catalog validation
and `git diff --check` passed. Signed-package verification on JetPack 7.2.1 /
L4T 39.2.1 and the complete installer lifecycle remain pending at the final
source.

Deliverables: Aligned public key, quickstart and release notes. AI assistance
identified the trust mismatch; the native package gate remains open.

## Linux selected-policy provenance correction

Title/ID: `m28g-linux-host-policy-selection`; continuation after the native
Jetson build at `b5d6897e002f`.

Milestone: 28g / m28g-linux-host-policy-selection.

Goal: Make the installed Linux policy file and builder provenance identify
the policy actually embedded in the native host clients.

Inputs: The completed eight-tool JetPack 7.2.1 native build, its provenance,
the selected production manifest and independent native `cohsh --check`.

Changes: The `b5d6897e002f` builder did compile `cohsh` against the selected
production policy and the native binary accepted that policy at SHA-256
`f77a645d43347d50eaef1e832b1b729918d1818d1b7f603b4988928036cd321c`.
Its provenance instead reported the untouched tracked default policy at
`ad39ddfc70f5e0519738403304069384e8fbdd9a9eabc8d3e9ce84edc7d4a769`.
The compiler had written the selected external TOML under `out/` while the
builder read `configs/generated/`. The builder now directs selected `cohsh`,
`coh` and SwarmUI policy files to their native default paths, and requires
the compiled `cohsh` to accept the selected external policy before packaging.
The earlier native build remains a failed provenance attempt.

Commands: `bash -n scripts/linux_host_tools_sync.sh`;
`.venv/bin/python -m pytest -q tests/test_linux_host_tools_sync.py`;
`scripts/check-generated.sh`; exact-source native Jetson rebuild and KVM TCP
replay pending.

Checks: Shell syntax, nine focused tests and generated consistency passed.
The updated builder has not yet produced a new native host-tool set or a
passing KVM TCP result, so the release gate remains open.

Deliverables: Source and focused regression repair with preserved native
failure evidence. AI assistance traced the provenance mismatch and corrected
the selected external policy paths; live qualification still controls closure.

## Exact 1.2.0 candidate and native installer correction

Title/ID: `m28g-release-b-qualification` and `m28g-macos-native-installer`;
downstream discoveries in their canonical archive and installer checks.

Milestone: 28g / m28g-release-b-qualification.

Goal: Bind the first exact `b68a2c2a1a38` release candidate to selected Mac,
Jetson and Pi bytes, and repair defects revealed by qualifying those bytes.

Inputs: Clean source `b68a2c2a1a38d6f6b1dfe430845dfc4f0f024298`
with source digest
`sha256:a01c2742283505b0287fffbf6b95dd666fe6088c883e7d55d382b8e3f626c5ba`;
selected production manifests; independently built Mac and Jetson QEMU guests,
native host tools, Python/NeMo wheels and the Pi 4 stage.

Changes: The canonical bundle preflight found serial smoke logs alongside
the Pi stage. A separate clean staging copy preserved those logs while
packaging the same Pi payload bytes. The Mac and Jetson host qualifiers found
that a relative output path failed after the packaged Python smoke changed
working directory; `release_qualify.py` now makes that path absolute. The
first Apple installer submission was rejected because seven copied command
line Mach-O tools lacked Developer ID signatures. The package builder now
requires a separate Developer ID Application identity, signs each copied tool
with hardened runtime and secure timestamp before freezing its installed-file
manifest, and checks its UUID against the selected archive binary. Installed
readback independently requires matching UUID, Developer ID team, runtime,
timestamp and exact signed bytes. The rejected Apple submission remains a
failed attempt; the corrected package was accepted and stapled. The
publication bridge names only the changed release tooling and focused tests
as eligible non-runtime changes.

Commands: Exact Mac/Jetson strict-production TCP replays; physical Pi wired
and private Wi-Fi TFTP/RAM smokes; canonical `release_bundle.sh
--check-manifest` and Mac/Linux/Pi assembly; `release_qualify.py host` on both
native hosts with pinned QEMU 10.1.0; `gpgv` against only the tracked Debian
publisher key; Developer ID signing, Apple notarization and stapler; focused
`tests/test_release_bundle.py`, `tests/test_release_qualify.py` and
`tests/test_macos_installer.py`; `scripts/check-generated.sh` with unfinished
release outputs preserved outside `releases/`.

Checks: The exact Mac and Jetson production TCP results passed. The Pi image
SHA-256 `5e23f461695a064fea1a0fc9147f54e39dca15c351a0123a7006f65616af962d`
passed same-image TFTP/RAM boot and quick Wi-Fi checks; the release `.img`
metadata names the same payload hash and its compact MBR/FAT32 layout passed.
Both extracted host archives passed manifest, archive, native tools, replay,
isolated Python wheel, authenticated TCP and SwarmUI browser checks. The Mac
archive SHA-256 is `b8fe858b6b3846750397bbd7ceb1e6b8861674b3e5365746ad609bbfcd1dc4ca`;
Linux is `67a98f0aebc40fde7965420e7a9bfa0ec19a5e5fe1013b6f2db1ce663bba8d3d`;
Pi is `a1cac6542b61521f7be336e83206a3486d78a14aeb4376ea5f496ddf3abcc988`. The corrected
Mac `.pkg` SHA-256 is `e81886fddec649e1a584695d47744c8d3442719a2e488dd06c399daf71212d0c`
with accepted Apple submission `a4d0164f-8a76-4674-b4f4-ab72e44ad869`.
The two JetPack `.deb` hashes are
`6f9ed66565100ccc8733495c5217e758897daa758dc043d9867df3e62afc6af3`
and `ee57cd9372ac8b1579a0d9c6ad92c694d6478f6fdc30ba127da88911bad6f260`;
their detached manifest signature passed independent `gpgv`. The focused
release tests passed 43/43 and the Mac installer/release qualifier tests
passed 30/30. Generated consistency passed after the incomplete release
directories were moved to ignored candidate storage.

Deliverables: Exact source-bound 1.2.0 archives, portable Pi `.img`,
signed/notarized Mac and manifest-signed Linux installer candidates, focused
repair tests and retained failures. Native privileged installs, installed
readback and GUI launch, final SD readback/boot, Jetson pressure, full
integrated adoption, owner approval, tag and PyPI publication remain open.
AI assistance traced and corrected the path and nested-signing defects;
host/package evidence retains its original proof class.

## Owner-evaluated installation walkthrough and installed receipts

Title/ID: `m28g-installation-and-integrated-adoption`; continuation of
`m28g-release-b-qualification`.

Milestone: 28g / m28g-installation-and-integrated-adoption.

Goal: Let the sole developer and tester evaluate the 1.2.0 installation
without claiming that his own walkthrough is independent.

Inputs: The named-evaluator adoption schema, frozen budgets in BENCHMARKS,
the `b68a2c2a1a38` qualified source, and native Mac and Jetson installer
receipts for the `869737b03982` publication candidate.

Changes: `scripts/ci/provider_m28g_adoption.py` accepts `Lukas Bower` as a
person with `independent=false` and reports `evaluation_scope=owner`. Other
evaluators retain the independence requirement. BUILD_PLAN, TEST_PLAN and
BENCHMARKS name the owner exception and continue to require raw walkthrough
attachments, three native journeys, GUI launch, budgets and lifecycle checks.
The schema advances to `cohesix-m28g-adoption-walkthrough/v2` so an older
walkthrough cannot silently inherit the changed evaluator rule. The publication
bridge classifies only this verifier and its tests as adoption proof tooling,
and admits only the exact BENCHMARKS evaluator wording while refusing any
changed numeric limit. Host CLI, Python SDK, SwarmUI and benchmark measurement
paths are unaffected.

Commands: `.venv/bin/python -m pytest -q
tests/test_provider_m28g_adoption.py tests/test_worker_task_evidence.py
tests/test_release_bundle.py`; `scripts/check-generated.sh`; `git diff --check`;
native `scripts/release_qualify.py installer` on Mac and Jetson, followed by
`scripts/release_qualify.py verify` with their absolute result paths.

Checks: The 158 focused tests and generated consistency passed. Both native
installer receipt verifiers and the strict five-part release verifier passed
for the prior `869737b03982` candidate, with exact evidence retained under
ignored `out/m28g/`. Lukas Bower reported that he evaluated the installations
and marked them PASS. That report and the installed-byte checks do not supply
the still-required raw GUI, three-journey, budget, rollback and uninstall
walkthrough. A new publication commit and rebuilt archives will require their
own Conditional G comparison before promotion.

Deliverables: Owner-evaluation verifier, regression tests, aligned acceptance
text and native installer receipts with explicit proof limits. Material AI
assistance traced the stale independence rule and implemented the narrow
identity exception; adoption-live and release promotion remain open.

## Jetson KVM pressure collector correction and accepted replay

Title/ID: `m28g-kvm-pressure-runner-portability`; defect discovered under
`m28g-release-b-qualification` and restored in the same pressure gate.

Milestone: 28g / m28g-kvm-pressure-runner-portability.

Goal: Make pre-READY Standard fault injection observable under the selected
Jetson KVM debugger without weakening the native fault requirement.

Inputs: Clean target source `b68a2c2a1a38d6f6b1dfe430845dfc4f0f024298`,
selected Linux KVM profile, pinned QEMU 10.1.0 SHA-256
`c30027b9805176ad0d85d732c356eac4d7485fab3c1748a797fc193b6bc3a46e`,
and the failed r2/r3 pre-READY diagnostic attempts under ignored Jetson
`out/m28g-pressure-b68-*` roots.

Changes: The pre-READY collector now redirects the selected Worker PC to
unmapped address zero. The previous breakpoint symbol could be consumed by
attached GDB under KVM and leave the target at READY timeout. The validator
still requires the role-matched target Standard fault and terminal teardown;
an injection marker or timeout cannot pass. During-IPC fault and budget
injections retain their separate target evidence paths. The release publication
bridge classifies only the pressure collector and focused test as a proof-tool
delta from the immutable qualified guest source. Host CLI, Python SDK,
SwarmUI, provider and benchmark workload code are unaffected.

Commands: `bash -n scripts/m26e_qemu_pressure.sh`;
`.venv/bin/python -m pytest -q tests/test_worker_task_evidence.py
tests/test_provider_m28g_adoption.py tests/test_release_bundle.py`;
`.venv/bin/python -m pytest -q tests/test_m26e_qemu_pressure_cli.py
tests/test_host_integration_run.py -k 'm26e_qemu_pressure or linux_aarch64 or
quiescent_probe'`; canonical `scripts/m26e_qemu_pressure.sh --reuse-artifacts`
on Jetson with the exact selected artifacts, `/usr/bin/gdb`, AArch64 `nm` and
the pinned stripped QEMU; `scripts/check-generated.sh`; local and remote
SHA-256 comparison of the final result and both pressure summaries.

Checks: 158 collector/release/adoption tests and 54 targeted pressure/host
integration tests passed. The remote pressure collector SHA-256
`dd4b415066fd03996235bcd326d1782d07a6543a4c2441adddd88e3f67ca3f27`
matched the local repair. The r4 canonical runner exited zero and its
`final/host-replay-result.json` reports `PASS` with KVM. Medium and high each
recorded 256 requested, discovered and READY executable Workers, two minutes
of load, strict control errors, no transient retries, a passing 1% error
budget and actual error rate zero. The final result SHA-256 is
`1513bd3b8f28091fb3067f812cb6f67ed327e969cca929a44e3d2819d56ba6a3`;
the medium and high summary hashes are
`4ced7d7aca597e34e0033fec2f297b4f22ef8bf9b30b312e2f145a8f345a0e2b`
and `4d774271402e8a479f281ee54d5e3152e27b77d1e476abdb90a6d115a2fc985e`.
Copies under ignored `out/m28g/pressure/r4-evidence/` match the Jetson originals.
The test runner stopped its QEMU and gateway processes after validation.

Deliverables: Accepted Jetson KVM pressure record and narrowly corrected
collector. This closes the KVM pressure subtask at its original target source,
without promoting the still-open integrated adoption or overall Release B.
Material AI assistance traced the GDB-consumed breakpoint behavior and
verified the real target fault path.

## Owner manual adoption attestation

Title/ID: `m28g-installation-and-integrated-adoption`; owner-directed
acceptance-route change under `m28g-release-b-qualification`.

Milestone: 28g / m28g-installation-and-integrated-adoption.

Goal: Record the sole developer's personally completed full adoption
walkthrough without inventing measurements or confusing it with native proof.

Inputs: Lukas Bower's 28 September 2026 statements, “I did the walkthrough
and marked it PASS,” “I did it manually,” and his confirmation that it covered
the full 28g adoption walkthrough; qualified source
`b68a2c2a1a38d6f6b1dfe430845dfc4f0f024298`; the five-part prior-candidate
release verifier and native Mac/Jetson installed-package receipts.

Changes: TEST_PLAN and BUILD_PLAN allow a written owner manual PASS for the
qualitative walkthrough. The adoption verifier accepts a separate bounded
`cohesix-m28g-owner-manual-attestation/v1` packet, checks Lukas Bower's identity,
seven attested areas, exact source and both installed package hashes, and emits
`owner_attested_host`. The earlier measured, raw-backed walkthrough route remains
available. BENCHMARKS retains every numeric limit but labels the absent manual
measurements `not-recorded`; no numerical result is claimed. The release
publication bridge admits only this exact benchmark prose change. Host CLI,
Python SDK, SwarmUI, native verifier, target and protocol integration behavior
are unaffected.

Commands: `scripts/ci/provider_conformance_run.sh --matrix
configs/provider_conformance.toml --case m28g-adoption-live
--reference-config out/m28g/owner-manual-adoption-reference-20260928.json
--host-profile mac-apple-m4-macos27 --state-dir
out/m28g/owner-manual-adoption-20260928-final`;
`.venv/bin/python -m pytest -q tests/test_provider_m28g_adoption.py
tests/test_release_bundle.py`; `scripts/check-generated.sh`; `git diff --check`.

Checks: The owner-attested adoption runner passed at the prior `869737b03982`
candidate, source `b68a2c2a1a38`. Its retained private attestation SHA-256 is
`2f4cf955b16838388e687add3d8760ebbbb0c835fdbc3d205aa031a0388c1d5c`;
the final summary SHA-256 is
`73a6187068c9f011a3419e6b4f64697a799ff9c4ef084217bc2f0e150b5ca038`.
The summary explicitly reports `measurement_status=not-recorded` and
`native_outcome_reverified=false`. The manual statement does not fill the
still-open four-mode `m28g-integration-live` case, native provider results or
final publication-byte comparison.

Deliverables: Named owner attestation, bounded verifier and focused regression
tests, aligned acceptance documents and retained result with its exact proof
scope. AI assistance implemented the separate manual route and verified the
package/source bindings; the owner supplied the human evaluation.

## Publication-inventory package input correction

Title/ID: `m28g-release-b-qualification`; package-input mismatch found in
final canonical release-manifest preflight.

Milestone: 28g / m28g-release-b-qualification.

Goal: Bind the version-aligned NeMo and Python distributions to the inventory
actually included in the publication archive, while retaining their tested
source and installed-byte identities.

Inputs: Clean qualified source `b68a2c2a1a38`, publication inventory SHA-256
`5a284e05493cb426fc0133adf043401563884deeab1c73c6c562a29657e87851`,
the prior installed `1.2.0` package receipts, and the failed preflight using
the qualified-source NeMo distribution report. Historical release rows changed
the publication inventory digest without changing the selected NeMo wheel.

Changes: The attempted qualified-source verification shortcut was withdrawn:
the canonical factory continues to verify the package report against the
publication inventory it will ship. A publication-source NeMo report was
rebuilt from that inventory; its wheel SHA-256 remains
`526a16c54dac26d5a0099ad9ce78a819bb1acedac90d731d89d3214d9f5af4f6`.
The earlier installed Python wheel is selected for final assembly only after
its unpacked members are compared with a fresh publication-source build; its
source/package report must remain exact. No host, target or provider runtime
code changed.

Commands: `build_nemo_kit.py --out` and `build_python_package.py --out` against
the publication inventory; unpacked wheel member comparison; canonical
`scripts/release_bundle.sh --check-manifest --linux
--linux-use-accepted-tools --qualified-source-root <clean-b68-source>` with
the exact retained Mac/Linux QEMU artifacts and passing TCP results, clean Pi
stage, selected production source manifest, installed package reports and
publication-inventory wheel reports; `git diff --check`.

Checks: The initial preflight refused `NeMo distribution differs from selected
release source` because its input report named the qualified inventory. A
temporary factory change made that preflight pass but produced a bundle whose
NeMo report did not match the bundled inventory; that candidate is retained
outside `releases/` and is not qualified. The attempted code change was
reversed. The publication-source NeMo wheel has the same bytes as the
qualified wheel. The freshly built publication Python wheel has identical
unpacked members to the earlier installed wheel; its ZIP bytes differ, so
the installed wheel is retained as the exact distribution input. Final
canonical preflight with the publication-inventory NeMo report, installed
Python wheel and both installed package reports passed: the compiler inventory,
source selection and release manifest agreed, the NeMo verifier reported
`PASS`, and the factory reported `Native artifacts and exact compiler-selected
release inputs verified (build-only=0)`. Archive qualification remains
pending at this record.

Deliverables: Corrected package-input selection with preserved failed
candidate and no factory behavior change. AI assistance identified and
reversed the inappropriate verifier shortcut before publication.

## Owner-selected 1.2.0 release evidence

Title/ID: `m28g-release-b-qualification`, `m28g-host-clients-as-built-alignment`,
`m28g-installation-and-integrated-adoption`.

Milestone: 28g / stable 1.2.0 release acceptance.

Goal: Apply the owner's 28 September 2026 focused release selection to the
exact tested archives and installed packages.

Inputs: Qualified runtime source `b68a2c2a1a38d6f6b1dfe430845dfc4f0f024298`,
the `869737b03982a87e4a14c85a605659b48cbf16cc` publication archives,
and the existing Stage 05, hardware, host, native installer, pressure,
component-live, protocol-control and adoption records.

Changes: BUILD_PLAN and TEST_PLAN select the completed evidence at its original
identity for 1.2.0. The three sealed archives and their matching signed
installers were copied byte-for-byte into `releases/`. The release notes
describe the selected user workflows and JetPack reference in plain English.

Commands and results:

| Check | Retained result |
| --- | --- |
| QEMU and Pi Stages 01–05 | PASS at clean `522463fa1ade`; `out/m28g/full-plan-522-qemu` and `out/m28g/full-plan-522-pi4-v3`, under renewed `EX-2026-0024`. |
| Packaged Pi SD readback, fresh boot and TCP | PASS at `b68a2c2a1a38`; `out/m28g/release-qualify-pi4-sd-8697/result.json`, SHA-256 `cb981f832a54e4ba24c3c948fd818cac127a5720c370cdff5cf65eab19af62c1`. |
| Mac and Jetson host archives | PASS at `b68a2c2a1a38`; `out/m28g/release-qualify-macos-8697/result.json` and `out/m28g/release-qualify-linux-8697/result.json`, SHA-256 `143520e99e00457d6a0beff4a5924e583e637299d6ab9340b9800fb6b1117cca` and `3283da9c1da81ff81c72f1364270168bfc3fcdc5b7dc655a49797a5d022c4e9d`. |
| Native installers | Developer ID signed/notarized/stapled Mac `.pkg` and publisher-signed JetPack `.deb` set passed package-manager receipt and installed-byte readback at the package hashes below. Local `pkgutil`, `stapler` and independent `gpgv` checks accepted the restored files. |
| Jetson KVM pressure | Medium and high PASS at `b68a2c2a1a38`; `out/m28g/pressure/r4-evidence/host-replay-result.json`, SHA-256 `1513bd3b8f28091fb3067f812cb6f67ed327e969cca929a44e3d2819d56ba6a3`. |
| Agent and provider components | M28d MCP, M28e A2A and M28f NeMo live results, independent Jetson CUDA/PEFT checks and Mac MLX host checks retain the source and proof scope in their component records above. `cargo test --locked -p hive-gateway --test agent_protocol_controls` passed 3/3 real-process protocol-control tests covering the four effective route modes. |
| Owner adoption | Lukas Bower's full manual walkthrough passed as `owner_attested_host` against the installed package receipts; `out/m28g/owner-manual-adoption-20260928-final/summary.json`, SHA-256 `73a6187068c9f011a3419e6b4f64697a799ff9c4ef084217bc2f0e150b5ca038`. Its numerical measurements are `not-recorded`. |
| Assembled release | Strict Mac/Linux/Pi plus both-installer verifier `pass` at publication `869737b03982`; `out/m28g/release-verify-8697/result.json`, SHA-256 `0601a3836c32a2a39451a5f4fab7a013550e69c51559df635f02214445f5e124`. |

Distributed archive SHA-256 values are Mac
`663ef55d9360aab536dbbee2486770171ea28b89093c01cfaffbf86eb3514d6d`,
JetPack Linux
`28115fa5153938ee64925142706c3eed01b74f9a63e2f5aed3714989801a6674`,
and Pi 4 `128917dbf6f67188f5143c22c0563bfdb82626db894b5ac4f257fc687961e505`.
The Mac `.pkg` SHA-256 is
`416004f05ff419cca8116255824d2dec8f1c0eed842289c6cace7c4b35ccb77c`.
The controller and SwarmUI `.deb` SHA-256 values are
`76a76cde3805d0efb4b50aa886d05c0dd917967dd62626edf070acf8d83b1826`
and `1f5b5b4dd21bed2e92465d70392e06ac119266a1ba07b1d983b98902ca65c10a`.

Checks: `scripts/check-generated.sh` passed with the sealed artifacts retained
outside the compiler's source scan, and the 27 focused adoption/bundle tests
passed. `git diff --check` passed. The current source changed release prose
and proof collectors after the qualified runtime commit; selected runtime
binaries, image, packages and signed distribution bytes remain the exact
qualified set.

Deliverables: Exact sealed release files, the selected Test Plan gate, owner
walkthrough attestation and hash-bound acceptance index. PyPI publication and
its public distribution hashes receive a separate post-tag record.
