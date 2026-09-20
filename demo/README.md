<!-- Author: Lukas Bower -->
<!-- Purpose: Demonstrate the 1.1.0-beta operator and host workflows with explicit authority and evidence boundaries. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Cohesix 1.1.0 demos

These demos cover the **1.1.0-beta** release line: strict Queen authority,
bounded control and telemetry, recoverable CUDA recipes, verified private LoRA
release, installed Python/CI journeys, and the SwarmUI workbench.
Integrated release qualification remains in progress under
[27g / assembled-journeys-and-recovery](../docs/BUILD_PLAN.md#27g).
A demo transcript is evidence only of the commands and environment it exercised.

Run commands from the **source checkout root**, in host Bash. Each `.coh`
file is an independent script; it has no variables, includes or shell execution.
The host helpers call the existing tools and verifiers. They do not install
services, mint tickets, provision an executor or manufacture completion records.

## Choose a demonstration

| File | What it demonstrates | Preconditions and effects |
| --- | --- | --- |
| [demo_runbook.coh](demo_runbook.coh) | Target identity, writer authority, lifecycle, root reachability, queues, leases, pressure, Worker discovery and retained logs | Read-only; selected host/GPU namespaces available |
| [authority.coh](authority.coh) | Policy visibility, dedupe, invalid strict-envelope refusal and model-only WorkerBus refusal | Policy/AuditFS enabled; deliberately rejected request only |
| [prepare_worker.py](prepare_worker.py) | Fresh strict heartbeat or LoRA request, separate one-shot approval, retained retry identity and explicit Worker cleanup | Writes local files; execution is a separate reviewed step |
| [telemetry.coh](telemetry.coh) | Two synthetic uploads, independent segment readback, latest pointer and ingest pressure | Writes two segments under a fresh `demo-110` device |
| [control_plane.coh](control_plane.coh) | Schedule admission, a named lease, maintenance refusal while leased, preemption, drain/resume, quiesce/resume and export open/close | **Disposable idle Queen only**; changes lifecycle and control state |
| [host_workflows.coh](host_workflows.coh) | Host-ticket requests, status/dead letters, published GPU state, Workers and audit observations | Read-only; use before and after a real CUDA/LoRA operation |
| [evidence.coh](evidence.coh) | Audit journal/export, decisions, dedupe, replay status and retained outcomes | Read-only; AuditFS/replay/host tickets enabled |
| [host_tools.sh](host_tools.sh) | Provider catalog, package verification, live scripts, CUDA/LoRA phases and installed Python journeys | Each invocation selects one explicit action and a new log directory |
| [fixtures/workers.coh](fixtures/workers.coh) | Heartbeat/GPU/LoRA role admission, sharded telemetry and cleanup | Fresh CLI mock only; no native execution |
| [../scripts/cohsh/run_demo.coh](../scripts/cohsh/run_demo.coh) | Historical GPU lease/status serialization | Mock fixture; writes synthetic START/EXIT records |
| [peft_adapter/](peft_adapter/README.md) | Historical file-registry import inputs | Placeholder bytes, not a native safetensors adapter |

## Prepare the host

Use matching tools and generated policy from the selected build or verified
component package. Follow [Quickstart](../docs/QUICKSTART.md),
[installation and adoption](../docs/ADOPTION.md) and
[host tools](../docs/HOST_TOOLS.md). A Mac controller can use a separately enrolled
Linux AArch64 NVIDIA executor; Jetson is one reference configuration.

For local script checking and the mock-only examples:

```bash
cargo build -p cohsh -p coh
export COH_BIN="$PWD/target/debug"
mkdir -p out/demo
"$COH_BIN/cohsh" --check demo/demo_runbook.coh
"$COH_BIN/cohsh" --transport mock --mock-seed-gpu \
  --script demo/fixtures/workers.coh
"$COH_BIN/cohsh" --transport mock --mock-seed-gpu \
  --script scripts/cohsh/run_demo.coh
bash demo/host_tools.sh catalog out/demo/catalog-01
```

A source default build supports the mock. Production-only packages may omit it.
The CLI mock omits host, policy and audit namespaces: use it only for the
explicit fixture, telemetry and control-plane scripts, not as a substitute for
a configured Queen. `--check` validates script structure without connecting.
Missing optional executables in `catalog` are recorded as `not-installed`.

For a selected production build, set `COH_BIN` to its host-tool directory,
`COH_POLICY` to its exact `coh_policy.toml`, and `COHSH_POLICY` to its exact
`cohsh_policy.toml`. Those policies must match the target profile. Source
default binaries are not interchangeable with a separately generated Pi build.

## Inspect a live Queen

The target has one authenticated TCP owner. Choose either direct TCP or the
gateway already owning that connection. Other tools can share the gateway.

For an existing gateway, with its upstream Queen session established:

```bash
export DEMO_TRANSPORT=rest
export COH_REST_URL=http://127.0.0.1:8080
bash demo/host_tools.sh inspect out/demo/before-01
```

For direct TCP, first close competing direct clients and the gateway:

```bash
export DEMO_TRANSPORT=tcp
export COH_TARGET_HOST=127.0.0.1
export COH_TARGET_PORT=31337
export COH_AUTH_TOKEN_REF="file:$HOME/.config/cohesix/queen-console.token"
bash demo/host_tools.sh inspect out/demo/before-tcp-01
```

Use the actual verified Pi address when selecting Pi. The credential file must
already contain the provisioned secret. See
[credential precedence](../docs/USERLAND_AND_CLI.md#credential-and-environment-precedence).
No checked-in fixture secret is a production credential.

Compare `/proc/boot` and `/proc/authority` with the intended image, manifest and
writer epoch. Read Worker telemetry at the IDs and shard labels actually
published; `ls /shard` is a bounded view, not an exhaustive fleet enumeration.
Keep declaration, lifecycle, artifact, receipt and execution proof separate.
An empty host snapshot is an absence of retained observations.

Each helper run creates private logs and `run.txt`, refuses an existing
destination, records source/graph context, and propagates command failure.
The checkout graph hash must be compared with the installed tool catalog;
it is not an assertion that installed binaries match. Logs remain `proof=none`
until assessed with their actual provenance. Keep them private.

## Exercise authority and real Worker requests

REST writes require both request authentication and a delegated caller ticket
covering the actual destination. Read the
[REST authority instructions](../docs/USERLAND_AND_CLI.md#rest-use-the-gateway-that-already-owns-tcp).
Load `COH_REST_AUTH_TOKEN` and `COH_REST_TICKET` privately before running a
mutating `cohsh` script. A Queen role label alone does not grant that scope.

```bash
bash demo/host_tools.sh authority out/demo/authority-01
```

The malformed request has no command and must be refused. For actual work,
read the current epoch from `/proc/authority`, then prepare a new request once:

```bash
read -r -p 'Verified current writer epoch: ' DEMO_WRITER_EPOCH
python3 demo/prepare_worker.py --writer-epoch "$DEMO_WRITER_EPOCH" \
  --out out/demo/heartbeat-01 heartbeat
"$COH_BIN/cohsh" --check out/demo/heartbeat-01/intent.coh
```

Review all three generated files. `approve.coh` grants one `/queen/ctl`
policy action, so run it only when required by the selected policy, under the
operator's approval authority. Run `intent.coh` separately with write authority
for `/queen/intents/ctl`. Example for the existing gateway:

```bash
"$COH_BIN/cohsh" --transport rest --rest-url "$COH_REST_URL" \
  --script out/demo/heartbeat-01/approve.coh
"$COH_BIN/cohsh" --transport rest --rest-url "$COH_REST_URL" \
  --script out/demo/heartbeat-01/intent.coh
"$COH_BIN/cohsh" --transport rest --rest-url "$COH_REST_URL" \
  --script out/demo/heartbeat-01/observe.coh
```

For direct use, substitute the TCP options above. The helper also accepts
`lora` to request a LoRA receipt Worker; this does not train or serve a model.
After admission, inspect the actual Worker telemetry for readiness and identity.
The helper deliberately does not guess a Worker ID or generate a READY record.

Retain `intent.coh` byte-for-byte after an interrupted write. Reconcile
`/proc/queen/dedupe`, logs and Worker state before an authorized retry;
do not rerun the generator or requeue the one-shot approval to make a retry pass.
Cleanup is another fresh, reviewed request targeting only the observed Worker:

```bash
python3 demo/prepare_worker.py --writer-epoch "$DEMO_WRITER_EPOCH" \
  --out out/demo/cleanup-01 kill --worker-id worker-27
```

Replace `worker-27` with the ID you actually created, recheck the current epoch,
review/approve/submit the new scripts, and observe terminal containment.
Full authority and retry semantics are in
[strict Queen intents](../docs/USERLAND_AND_CLI.md#submit-a-production-queen-intent).

## Telemetry, scheduling and maintenance

Use a disposable selected-profile Queen and fresh demo identifiers:

```bash
bash demo/host_tools.sh telemetry out/demo/telemetry-01
# Only after all Workers and other leases are contained, on an idle ONLINE Queen:
bash demo/host_tools.sh control out/demo/control-01
```

Telemetry bytes explicitly say `mode=fixture`. The telemetry script expects
`seg-000001` and `seg-000002`; rerunning it in the same namespace deliberately
fails those expectations. Use a fresh disposable environment, or review and
change the device ID consistently before a new run. No fake device measurement
is promoted to native evidence.

The control script grants only `demo-110-lease` for `demo-resource`. It proves
the outstanding-lease refusal while DRAINING, then preempts that lease, drains
to QUIESCED and resumes ONLINE. It also closes its export window.
Scheduling admission does not establish workload completion or allocate a GPU.
If interrupted, inspect lifecycle and leases first; preempt only this demo's
lease and resume the intended state through the operator. Preserve the failure
transcript and do not continue blindly on a shared hive.

## Recoverable CUDA and verified LoRA release

Prepare an enrolled CUDA deployment using
[recoverable CUDA recipes](../docs/HOST_TOOLS.md#recoverable-cuda-recipes) and
[the Python recipe example](../tools/cohesix-py/examples/cuda_recipe.py).
Prepare a native adapter import or training deployment using
[Private LoRA release](../docs/PRIVATE_LORA_RELEASE.md).
Both require exact provider/package/runtime/trust identities, fresh Worker
bindings, bounded admission, private durable journals, and real output CAS.

Use the existing gateway and a scoped operator ticket reference:

```bash
export COH_TICKET_REF="file:$HOME/.config/cohesix/operator.ticket"
# COH_REST_URL and the protected COH_REST_AUTH_TOKEN reference are already set.
bash demo/host_tools.sh cuda plan /private/cuda-deployment.json out/demo/cuda-plan-01
# Review the exact request and admission before applying.
bash demo/host_tools.sh cuda apply /private/cuda-deployment.json out/demo/cuda-apply-01
bash demo/host_tools.sh cuda watch /private/cuda-deployment.json out/demo/cuda-watch-01
bash demo/host_tools.sh cuda verify /private/cuda-deployment.json out/demo/cuda-verify-01

bash demo/host_tools.sh lora plan /private/lora-deployment.json out/demo/lora-plan-01
bash demo/host_tools.sh lora apply /private/lora-deployment.json out/demo/lora-apply-01
bash demo/host_tools.sh lora watch /private/lora-deployment.json out/demo/lora-watch-01
bash demo/host_tools.sh lora verify /private/lora-deployment.json out/demo/lora-verify-01
```

Use `cohsh --script demo/host_workflows.coh` with the chosen live connection
before and after the operation. These reads correlate control-plane records;
only the existing signed verifier can establish the requested native result.
Watch may finish before an operation completes; verify remains nonzero until
its completion contract is met. The helper never polls or retries a mutation.

After interruption, retain the original deployment and journal:

```bash
bash demo/host_tools.sh cuda recover /private/cuda-deployment.json out/demo/cuda-recover-01
bash demo/host_tools.sh lora recover /private/lora-deployment.json out/demo/lora-recover-01
```

These are the existing reconciliation commands. CUDA cancellation and LoRA
recovery-only compensation need their separately scoped requests described
in the owning guides. A lost ACK is ambiguous. A failed canary followed by
successful rollback remains `recovered_failure`, not a released candidate.
The placeholder [PEFT fixture](peft_adapter/README.md) is never an input for
native `peft release`.

## Installed packages, Python and CI

Verify an actual signed component package with an independently trusted
matching `coh` and signer enrollment:

```bash
bash demo/host_tools.sh package /downloads/cohesix-package \
  /private/package-trust.json out/demo/package-01
```

Build/install the SDK and prepare the durable journey config as documented in
[Adoption](../docs/ADOPTION.md) and [CI workflows](../docs/CI_WORKFLOWS.md).
Select the installed entrypoint with `COH_JOURNEY_BIN` when it is not on PATH.
The config selects CUDA or LoRA and the actual installed `coh`:

```bash
bash demo/host_tools.sh journey validate /private/journey.json out/demo/journey-validate-01
bash demo/host_tools.sh journey doctor /private/journey.json out/demo/journey-doctor-01
# Only for the initial reviewed submission:
bash demo/host_tools.sh journey submit /private/journey.json out/demo/journey-submit-01
# Subsequent observation uses the same config and durable state, with no --submit:
bash demo/host_tools.sh journey run /private/journey.json out/demo/journey-observe-01
```

Doctor's `not_observed` is missing evidence, not readiness. The journey returns
nonzero for pending, ambiguity, failure and recovered failure. The Bash helper
preserves that status and any partial output; it does not turn an ACK into CI success.

## Evidence and SwarmUI

```bash
bash demo/host_tools.sh evidence out/demo/evidence-snapshot-01
"$COH_BIN/coh" --ticket-ref "$COH_TICKET_REF" evidence pack \
  --rest-url "$COH_REST_URL" --out out/demo/evidence-pack-01
"$COH_BIN/coh" evidence timeline --input out/demo/evidence-pack-01 --scenario incident
```

Preserve partial-pack errors and summaries. Attach real recipe/causal evidence
using the existing options in [host evidence tooling](../docs/HOST_TOOLS.md).
A textual snapshot or timeline never replaces signature, output-hash,
native-runtime or exact-target verification.

Open the installed **SwarmUI** and connect to the same gateway. Use
**Namespaces** for the paths above, **Tickets & policy** for approvals,
**Operations** for the existing host workflows, and **Evidence**, **Run story**
and **GPU flight deck** to inspect their outcomes. Open **Replay** for the
signed CUDA, private-adapter and failed-canary references; its persistent
historical mode is separate from a live run. See the
[SwarmUI guide](../docs/SWARMUI.md) and [gallery](../docs/SWARMUI_GALLERY.md).

## Maintain these demos

```bash
bash -n demo/host_tools.sh
python3 -m unittest discover -s tests -p test_demo_tools.py -v
cargo test -p cohsh --test demo_runbooks --test script_catalog
scripts/check-generated.sh
scripts/ci/check_test_plan.sh
```

The focused host test enables the actual NineDoor host contracts and executes
all seven checked-in demo scripts. It proves host script behavior only.
Production-only builds exclude that host-model test. Live QEMU, fresh Pi,
native CUDA/LoRA and assembled release acceptance remain separate TEST_PLAN
lanes. Do not run disruptive demos on a target being used for qualification.
