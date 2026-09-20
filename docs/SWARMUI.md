<!-- Author: Lukas Bower -->
<!-- Purpose: Operate the native desktop through scoped connections, reviewed forms and independently verified evidence. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# SwarmUI operator guide

SwarmUI brings connection setup, Cohesix operations, namespace exploration and
verified run stories into one desktop. Spectrum provides controls and accessible
focus; PixiJS renders the hive and causal ownership map. All assets are local.

## Connect your hive

Open **SwarmUI.app** from the verified macOS desktop package, or **bin/swarmui**
from the Linux AArch64 desktop package in a graphical session. Jetson uses the
same Linux desktop contract. Keep the package together: its `bin/coh` supplies
the matching validated host operations. **Settings → Installed tools** can select
another matching installation with the native folder chooser.

1. Select **Connect a hive**. Choose **Hive Gateway** for shared access, or
   **Queen directly** for a single-owner TCP console.
2. Enter the gateway base URL (`https://gateway.example:8443`) or Queen address
   (`tcp://192.0.2.10:31337`). A gateway URL has no path, query or embedded password.
3. Enter your request-auth credential for a gateway, or TCP authentication
   credential for a Queen. `env:NAME` and `file:/absolute/path` references work;
   placeholder credentials fail before connecting. Select your role and, where
   required, a separately scoped delegated ticket or secret reference. The gateway
   transport supports the Queen role with scoped delegation; Worker roles use
   direct TCP. Gateway connection verifies authenticated status and upstream
   reachability before displaying LIVE.
4. Select **Connect**. The app reports authentication refusal without becoming
   live. A successful connection shows **LIVE**, endpoint and role. A named
   profile saves only the endpoint, transport, role and name. It saves no secrets.

A Queen console has one owner. Close another direct client before connecting, or
use a Hive Gateway which owns that console and multiplexes clients. Live host
operations require a gateway so an invoked tool cannot steal a direct session.
Gateway status and pressure appear in the hive; exact refusals remain in the
transcript. Disconnect explicitly before switching operators. Changing connection,
replay or installed tools invalidates an outstanding action review.

Environment-based launch configuration (`SWARMUI_TRANSPORT`, `SWARMUI_9P_HOST`,
`SWARMUI_9P_PORT`, `SWARMUI_REST_URL`, and the documented auth references) remains
available for existing deployments. The ordinary graphical journey needs none.
Raw Secure9P launch mode remains an advanced compatibility transport.

## Find help

Hover or focus the **?** beside a screen title for a short next-step hint in plain
English; tap it
on a touch screen. **Escape** dismisses the hint. Presentation, transcript,
Recent activity and snapshot naming have similarly brief tooltips. Important
input and authority guidance stays visible in the forms. **Find an action**
searches operations, and the optional console provides `help` and `man` usage.

## Explore published state

Open **Namespaces**, choose a root and select an entry. Directories open as lists;
files open as bounded reads. **Read file** and **Recent activity** remain above the
listing. A path-kind refusal permits a read-only file probe; authentication and
capability refusals stop immediately. The transcript retains both operations, and
new selections clear the previous listing so it cannot be mistaken for new state.

## Choose an operation

**Operations** is generated from the same Rust argument schema as the installed
`coh`. Search by purpose or command, select the operation, fill its labelled
fields, then **Review action**. Path fields have native file/folder selectors.
The review shows the exact argument vector without credentials. Confirming runs
that one reviewed request; editing context or replaying a confirmation refuses it.
The app checks the installed tool's schema before execution.

| Task | Where to go |
| --- | --- |
| Inspect providers, plan CUDA work, apply/watch/explain/verify/recover a run | Operations |
| Plan, apply, inspect or recover a private LoRA release | Operations → Private LoRA release |
| GPU discovery, status, leases, model and PEFT registry operations | Operations |
| Collect telemetry, attest, build/inspect/export/verify evidence | Operations or Evidence |
| Worker lifecycle, budgets, scheduling, leases, bind/mount, policy and approvals | Tickets & policy |
| Read target state and control-file status | Namespace |
| Read command usage and recovery guidance | Field help or Expert console → `man` |
| Replay a verified CUDA or LoRA journey | Run story → reference walkthrough |

Controls use existing console records and the selected generated paths. Required
identities, numbers, enumerations and hashes are validated before submission.
The target still owns role, scope, production-policy and lifecycle admission.
A visible control never grants permission. Legacy spawn/kill can be refused by a
production profile; use its admitted workflow. An ACK means admission, not Worker
readiness, completed execution or independently verified success.

Some host operations are deliberately outside a desktop subprocess lifetime:
arbitrary shell-backed `coh run`, a persistent FUSE mount and identity-service
administration explain their owning service boundary in the catalog. They are
not alternate executors. Native preparation and persistent executor/service
installation follow [host installation](ADOPTION.md); once enrolled, the
accepted CUDA and LoRA controller journeys run through the desktop forms.

Only one host operation runs at a time. Output is bounded to 1 MiB per stream;
the controller process has a 120-second bound. Timeout is **interrupted/unknown**:
terminating a controller does not prove cancellation of remote work. Retain the
operation identity and deployment file, then use its **Watch**, **Explain** or
**Recover** form. Do not create a new identity merely to retry an ambiguous run.
The window refuses to close while the bounded controller is running.

## Explore and inspect

**Namespace** offers generated roots, breadcrumbs, list/read/tail actions, copy
path and transcript access. Paths are absolute, at most eight components and 96
bytes; relative walks, `.` and `..` are refused. Role/ticket scope is enforced by
the owning transport. Canonical Worker telemetry lives at
`/shard/<label>/worker/<id>/telemetry`; `/worker` is labelled legacy and offered
only when the generated compatibility alias is enabled. Missing optional
providers remain unavailable, rather than appearing as an empty healthy fleet.

**Tickets & policy** opens ticket status/deadletters, pressure, approvals, policy
and audit records. Generated features and the target decide which writes exist.
**Expert console** preserves existing console grammar and shared manuals; root
diagnostics use the direct authenticated Queen console. Gateway users inspect
published state through Namespace; the gateway does not expose root diagnostics.
The expert console is optional.
The bottom **Transcript** drawer retains bounded command results. Credentials are
redacted; raw prompts, completions and retrieved content are withheld from report
inspection.

## Follow a run and its proof

**Run story** separates Queen admission, target, Worker binding, external host,
native runtime, artifacts and evidence. The PixiJS ownership map is a causal map
of the selected records, not a fabricated network topology. Select any phase or
ribbon segment to inspect its exact source and artifact references. Declaration,
executable lifecycle, external result and target proof remain separate facts.

**GPU flight deck** displays recorded host/runtime identity, selected model,
resources and artifact facts. Unrecorded utilization, temperature, power, TTFT,
decode rate and NVMe health say **unavailable**. Historical metrics never imply
current readiness. A successful rollback of a failed candidate stays a failed
candidate with a visible recovery path.

The included CUDA, LoRA and recovery references contain immutable signed graphs,
public enrolled trust and content-addressed observations. The owning `coh evidence
story --input GRAPH --trust TRUST --cas DIR` verifies signatures, sequence and CAS
before returning a bounded redacted inspection. Verification uses the enrollment's
recorded time; this is historical proof, not a fresh authority or execution claim.
Tampered materialization is refused without overwriting the evidence.

**Evidence** opens canonical packs, runs canonical inspection, timeline and export
forms, and links the report to its source. **Replay** opens bounded trace and Hive
CBOR snapshots using the existing parsers, including capture-policy validation.
Replay disconnects the live session and disables network writes. Return to live
work through an explicit connection. The app retains **LIVE**, **REPLAY**,
**FIXTURE** or **OFFLINE** labels in normal and presentation layouts.

Use **Presentation view** for the expanded canvas. Escape exits; Command/Control-K
opens the action palette. Alt-1/2/3 opens Overview/Operations/Namespace. Reduced
motion freezes visual drift while preserving observations; recorded walkthroughs
advance one selected phase at a time. Leaving or hiding a view stops its polling
and walkthrough timer.

## Native acceptance walkthrough

This is a focused desktop integration lane, separate from target/release acceptance.
Provision one accepted QEMU image and its authenticated Hive Gateway as sole console
owner using the selected target configuration. Keep credentials in private local
files. Launch `scripts/ci/swarmui_native_e2e.sh --target qemu --state-dir DIR` with
exact `--app`, `--coh`, `--gateway`, `--registry`, `--qemu-artifact` and
`--source-record` file paths. `--release-dir` supplies default packaged binary paths;
macOS supplies its `.app/Contents/MacOS/swarmui` explicitly. The runner creates new
private evidence and starts the real native app, with no injected bridge responses.

Through the visible native app:

1. Refuse a placeholder credential, then a non-placeholder incorrect credential.
2. Connect to the authenticated gateway. Browse `/shard`; read `/proc/boot`.
3. Observe the Live Hive and its gateway pressure/Worker proof fields. Switch desks
   to stop polling. Run **Providers** through the reviewed installed-tool form.
4. Disconnect and reconnect. Open the verified LoRA reference, inspect a phase and
   its exact evidence, then open the failed-canary recovery reference.
5. Try a reviewed control in replay and observe the offline refusal. Capture the
   real native window into the evidence directory, keeping its mode label visible.
6. Close the app, then run the same entrypoint with `--verify --target qemu
   --state-dir DIR`. Preserve its provenance, bounded observations and result.

Repeat against the extracted candidate's exact app/tools. A separate Jetson native
walkthrough checks the Linux AArch64 desktop against the same isolated gateway;
this does not turn retained CUDA/LoRA records into a new GPU execution claim.
Use the existing Playwright presentation gate for source and staged `ui/swarmui`
assets, including its performance cases. Its simulated bridge is always **FIXTURE**
and cannot satisfy the native lane.

For candidate assembly, use `scripts/install/stage_swarmui.py --repo REPO
--generated-root GENERATED --bin-dir BIN --profile macos-desktop --out CANDIDATE`
(or `linux-aarch64-desktop`). Sign, verify and install `CANDIDATE/package-input`
with the existing `coh package build/verify/install` workflow and independently
enrolled package trust from [Adoption](ADOPTION.md). The app embeds its frontend
and reference evidence. `CANDIDATE/ui/swarmui` is an exact, hashed browser-test
companion, separate from signed native package membership.

After native acceptance, save at least two original PNG or JPEG frames in its evidence
directory and verify the lane. `scripts/ci/swarmui_showcase_capture.sh --result
DIR/result.json --out CAPTURE.html` builds a portable deterministic walkthrough of
those exact native frames. It verifies their hashes and preserves every pixel,
mode label and proof boundary; it does not manufacture events or topology.
