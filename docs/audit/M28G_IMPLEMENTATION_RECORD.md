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
`m28g-release-b-qualification`, `m28g-mcp-tool-catalog`,
`m28g-a2a-agent-facade`, `m28g-integration-live`.

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

The A2A Agent Card now advertises the selected `1.2.0-beta` release version.
A focused cross-contract test binds that value to the generated release
inventory; the two Python wheel distributions remain `1.2.0b0`.

M28g also still requires frozen adoption and quality budgets, independently
evaluated clean installation, the four effective protocol modes, native CUDA,
PEFT, Apple and NeMo journeys from installed bytes, full applicable QEMU and
physical Pi evidence, and assembled release due diligence. These gates must
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

## Exact-source 020a qualification and remaining release gates

The selected checkpoint is clean commit `020a516300b75e55f91606a77106c76ef74f665c`,
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

M28g remains **In Progress**. Stage 05 cannot pass without a new human-owned
decision for `EX-2026-0024` or verified remediation of its underlying unsafe
boundary; the selected Jetson KVM pressure control also remains unexecuted.
Native signed/notarized Mac and independently signed Ubuntu ARM64
packages, clean installs on every advertised host, installed-client parity,
the full live acceptance matrix, frozen adoption and quality budgets, and
named release-owner approval remain absent. No exact assembled Release B
candidate or publication is claimed. Material AI assistance produced this
checkpoint and its scoped validation record.
