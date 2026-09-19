<!-- Author: Lukas Bower -->
<!-- Purpose: Bind private LoRA release qualification to exact native artifacts, focused transaction tests and honest recovery outcomes. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Private LoRA release implementation record

**Status: Complete — 19 September 2026.**

Scope: Milestone 27d, `m27d-live-peft-reference-paths` and
`m27d-evaluation-promotion-and-rollback`. The owner's 19 September 2026 request
activates implementation, focused completion testing and commit/push to main.
The full workspace suite and staged release campaign are outside this component
claim; TEST_PLAN assigns integrated release qualification to 27g.

## Implemented contract

[Private LoRA release](../PRIVATE_LORA_RELEASE.md) defines the pinned stack,
source custody, exact input and evaluation bindings, first-deployment approval,
actual serving, failure preservation and fresh-authority compensation. The
native phases extend the existing recipe journal/lock/atomic-write primitive,
existing systemd execution mechanism, root admission and signed causal verifier.
No new VM listener, direct Worker call or in-VM training/evaluator is introduced.

Manifest schema 1.27 selects the dedicated `peft.release` action and WorkerLora
receipt code 0x0305 in the QEMU and regression source profiles. Existing PEFT
scopes do not acquire native release authority. Pi retains its selected legacy
four-action PEFT matrix; optional release vocabulary cannot enable a target.

## Compatibility review

- `coh`: adds the native release lifecycle; legacy file-registry operations and
  CUDA recipes retain their contracts. Shared journal persistence is regression
  checked. Python calls the same CLI and verifier.
- `host-ticket-agent`, `cohsh`, Hive Gateway, root NineDoor and executable Workers:
  selected action/argument/receipt matrices and independently signed native
  evidence are aligned. Existing wire schemas and framing bounds remain intact.
- SwarmUI and root/cohsh console help: shared manuals link the host command;
  no console execution verb or native UI is claimed.
- `tools/cohesix-py`: profile validation distinguishes the selected four/five PEFT
  actions, parses the common receipt vocabulary and adds bounded native helpers,
  examples and the shared release wrapper.
- `gpu-bridge-host`, `host-sidecar-bridge`, `cas-tool`, audit/identity/attestation,
  telemetry, federation/exporter and other host utilities retain their existing
  surfaces. The native serving registry does not masquerade as `/gpu/models/active`.
- Performance scripts and report schemas retain their transport authority,
  workloads, limits and acceptance thresholds. Native evaluation/canary timings
  are workload observations, not raw-TCP or release performance qualification.
- Generated deployment profiles advance their selected manifest/policy version
  to 1.27. Immutable release bundles are unchanged; no new packaged release or
  complete domain use-case qualification is claimed.

## Evidence collection

Focused evidence is retained under `out/m27d/`. Failed setup attempts remain
retained and do not establish completion. Native source archives and generated
target overlays record exact file digests; every live receipt must match its
actual admitted manifest and measured native agent binary.

Native execution uses Merlin2 as the Linux AArch64 NVIDIA reference host, not
as a Jetson-only product contract. The pinned Python/HF/runtime configuration and
model revision are recorded in the operator guide. Initial CUDA memory headroom
was 5,560,721,408 of 7,849,246,720 bytes (`native-setup-01.log`). The serving
process and helper have separately observed systemd identities and resource
bounds; CUDA allocation is not a hard GPU partition.

The initial native source archive records parent
`12dc11e34bd4c77964bc68b546acdd534973f757`. The independent Build Plan consolidation
in `770492229b0d1ce876bc77eb5e4a846d0130d260` is preserved in the publication parent;
its generated inventory is included in the exact selected target overlay.
`native-source-01.json` and its archive, target overlays and native recovery delta
record file-level source digests. `native-built-source-check-02.json` compares
29 affected implementation/build paths against their final native checkout,
using the exact admitted generated overlay where appropriate. Native helper
SHA-256 is `fc95ec6a1151d0503315447878ec559e0a654abb821e782968dcd0c9f9182632`.
The final verifier consistency fix is compiled separately in controller build 02
and native host build 07 and rechecks retained signed outcomes. The native agent
binary remains `c7dece8ad79b411cd9e2ac235900249b7016a4bdefa0ee66cf463414fe7570c0`;
the final provisioned controller is
`a2a15e20603680a5ee14d0fde7ce4179a678b82bc337247fbdce39698e0bf4de`.
`final-controller-positive-verification.json` retains successful re-verification
of the first-deployment, final training and native import signed graphs.

The provisioned QEMU build is `qemu-04`; the source overlay and all generated
outputs are retained alongside its artifact. Its resolved manifest SHA-256 is
`20978a614c2cad6a37dc36aaea908592e37b3dc17fe60e9a9d361ce0b926bac7`, root task is
`84eb409e283bfb2fb61015d190d97df1954f81a0f29e815bca76480daa4f7124` and WorkerLora
image is `4c137a37685f47d31dc04073013c55ee231da6c1f95b295662fc0956fb7698c9`.
The live path pins Worker `worker-1`, supervisor generation 129 and capability
generation 1. Gateway, native and Worker custody use distinct enrollment keys;
source attestation uses an explicitly local independent reference key.

## Focused checks and execution commands

The following scoped checks passed; logs are under `out/m27d/`:

- `cargo test --locked -p coh --test peft_release`: seven pure/controller tests,
  including both entry paths, every phase crash/duplicate/lost ACK, authority
  refusal at each boundary, comparison freshness/context/sample/metric guards,
  changed baseline, rollback failure and fresh compensation. Training interruption
  before evaluation retains its original journal and permits only restoration.
  Evidence: `focused-tests-08.log`.
- `cargo test --locked -p coh --lib native_success_requires_successful_verified_terminal_evidence`:
  native success and signed terminal success must agree, including cancellation
  and expiry refusals. Evidence: `terminal-outcome-tests-01.log`.
- Existing `coh` recipe and workflow integration selections passed, preserving
  the shared persistence and submission guards: `focused-tests-05.log`.
- Shared `cohesix-authority` release argument and `worker-task-abi` PEFT receipt
  selections passed: `action-tests-01.log`, `worker-tests-01.log`.
- `cargo test --locked -p root-task --no-default-features --features driver-tests-qemu --lib host_ticket_v2_admission_is_strict_root_owned_and_stable -- --test-threads=1`:
  one selected Root admission test passed (`root-admission-tests-02.log`).
- `cargo test --locked -p coh-rtc --test cas_validation --test python_profile --test roundtrip --test sidecar_collision`:
  21 compiler contract tests passed (`compiler-tests-02.log`).
- Python release wrapper: one selected test passed (`python-tests-01.log`).
  Generated profile, Worker and evidence receipt selections: 43 passed
  (`python-profile-tests-02.log`). The independent expected vocabulary includes
  the existing GPU workload actions as well as the new PEFT action.
- Native qualified-environment `unittest` discovery for `test_hf_native.py`:
  five tests passed with no skips (`native-input-tests-03.log`), covering CAS
  corruption/size/path/symlink bounds, unsafe import formats, independently keyed
  source-attestation bindings, the delayed-forward compensation fence and the
  absolute admitted deadline boundary.
- `cargo test --locked -p tests --test audit_ledgers`: two passed
  (`audit-ledgers-01.log`). `scripts/ci/rust_risk_gate.sh` passed without raising
  any production risk ceiling (`rust-risk-02.log`, final source).
- `cargo fmt --all -- --check`, focused `cargo clippy --locked -p coh -p host-ticket-agent --all-targets -- -D warnings`,
  compiler Clippy, source metadata/new local links, generated consistency and
  Test Plan integrity pass at their recorded final source identities.

Exact implementation builds are macOS ARM64 `cargo build --locked --release
-p coh -p host-ticket-agent` (`mac-build-02.log`), Linux AArch64 the same package
selection (`native-host-build-06.log`, then controller-only library correction
in `native-host-build-07.log`), and selected QEMU `release-qemu` root and Worker
images (`qemu-04/build.log`). The provisioned Mac controller is compiled against
that QEMU image's generated policy, with canonical generated files restored
and checked afterward (`controller-build-02.log`). A default-policy CLI refusal
before admission is retained as `m27d-import-02`; it is not native evidence.

Native conformance is driven by the retained diagnostic
`out/scripts/m27d-signed-release-03.py`, using the actual `coh peft release`
plan/apply/recover/verify commands and existing admitted host-agent `--run-once`.
Each case retains the complete request, matching trust and signed graph/CAS,
original HF Trainer reports, native observations and Root Worker completion.
Duplicates preserve the graph digest; forged or missing Worker records fail.
The diagnostic uses no production test flags or fabricated scores.

## Native acceptance and cleanup

All required outcomes pass in `qualified-cases.json`. These are native workload
and QEMU admission/Worker receipt claims, with no physical Pi or integrated
release acceptance. Each comparison uses the same 16 held-out samples, seed,
preprocessing, base/tokenizer, native evaluator and resource/runtime identity.
The predeclared loss ceiling is 8.0 and allowed regression is zero.

| Native case | Candidate / baseline loss | Outcome and generation | Signed graph SHA-256 |
| --- | --- | --- | --- |
| `m27d-train-05` | 4.928388596 / 4.941473961 | `succeeded`, 0 → 1; Worker `confirmed` | `e901094857590ed2f757044809a490985b07152d58bc610f5389b91ba15766c6` |
| `m27d-train-06` | 4.928388596 / 4.928388596 | `succeeded`, 2 → 3; Worker `confirmed` | `9619de3804e1b03c1d4583e4f65860e06301c827da305f60a3c7ce69e1e44ee8` |
| `m27d-import-03` | 4.928388596 / 4.928388596 | `succeeded`, 3 → 4; Worker `confirmed` | `8daf8cc168e84e59013f5cbccc79e8894508799eda10aad663252601f5b99a82` |
| `m27d-regression-01` | 4.939754009 / 4.928388596 | `failed`, 4 → 4; Worker `rejected` | `6d480077f97e7c05f3932d80105ccb93ed8f0814c480f5292d404c512038302b` |
| `m27d-canary-02` | 4.928388596 / 4.928388596 | `recovered_failure`, 4 → 4; Worker `rejected` | `7658280f10f88be60d2dca2231171be624739675d6c8c0e365ef39c55362f8fe` |

The first-deployment record explicitly compares the unadapted qualified base,
retains generation zero and uses the owner's approved reference task. Its earlier
helper/agent identities remain in that graph; the final transaction is rerun as
`m27d-train-06` and `m27d-import-03` with the final native helper and agent. The
final controller successfully re-verifies all three positive graphs.
The imported adapter has genuine native HF source-job provenance, source custody
and an independent source signature; the import invents no Cohesix training job.

`m27d-regression-01` evaluates a genuine one-step native adapter and rejects its
measured loss increase before scan/stage/load. Neither its score nor the bound
is changed to obtain this refusal. The accepted generation remains four.

`m27d-canary-02` loads candidate
`85636674efec2d9ad2c6ef813d13b6898bf05e6631ab6a84fb0c72fb5b57ea35`, then an external
diagnostic stops the owned real serving unit after the successful Load
observation. The native canary fails. Compensation fences delayed forward work,
restores accepted adapter
`ed96796a6a89d2ce522677102332234593c17a5468daa602afd5545f13d0973f` at generation four,
restarts the actual service and verifies exact rollback bundle, active invocation,
identity, greedy outputs and latency. The native result is `recovered_failure`,
Root records a rejected candidate and `coh peft release verify` refuses success.
The earlier `m27d-canary-01` proves same-adapter service recovery only; the distinct
artifact case supplies the required restoration evidence.

Each final live graph has nine correctly bound phase records. Duplicate
controller submissions and agent reconciliations keep its digest unchanged.
Forged or missing Worker evidence is refused. The deterministic phase tests
supply interruption/expiry/revocation boundaries; they do not claim live process
crashes at every boundary. Native checkpoint resume remains explicitly
unqualified and refused; fresh compensation retains original identities and
cannot replay candidate work. No remaining milestone requirement depends on it.

`native-cleanup-01.json` verifies the owned serving unit inactive with PID zero,
its former process absent, port 38527 closed, no active native phase units,
no unresolved registry owner/blocker, and the installed reference unit removed.
The source-attestation private key and per-case enrollment keys are removed.
`local-cleanup-01.json` verifies the owned QEMU, gateway and tunnel absent,
their local ports closed and disposable private credential copies removed.
Native CAS, accepted/rollback bundles, signed source provenance and all failed
attempts remain retained. `native-provenance/` retains digest-checked metadata,
attestations and input snapshots; model weights remain on the native host.

The retained diagnostic scripts live only under ignored `out/scripts/`.
Source/profile/policy setup failures remain failed evidence; none substitutes
for the admitted successful cases. The full workspace suite, staged release
campaign, physical Pi qualification, native checkpoint resume, production
Worker ticket-to-bundle binding, broad provider/model support and complete
package/UI adoption are not claimed by this focused milestone closure.


`qualified-evidence-manifest.json` seals the retained focused evidence and final
changed source-file digests. Private credential copies are excluded and removed.
