<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Track Milestone 28 contract migration, focused checks, and exact evidence without promoting unfinished work. -->
<!-- Author: Lukas Bower -->

# Milestone 28 implementation record

Milestone 28 is in progress under `m28-selected-job-contract`,
`m28-standing-authority-and-admission`, and `m28-agent-protocol-controls`.
This record separates source checks from the live GPU and service recovery
observations required by the [definition of done](../BUILD_PLAN.md#28).

## Existing persisted state and migration boundary

| Owner | Existing format and bound | Meaning to preserve |
| --- | --- | --- |
| `host-ticket-agent` compatibility journal | `host-ticket-compat-execution/v1`, 1 MiB and 256 entries | Executing without a result is ambiguous; replaying the provider is forbidden. Its original ticket and idempotency key remain authoritative. |
| `host-ticket-agent` receipt execution journal | `host-ticket-execution-journal/v2` through `/v4`, 1 MiB and 256 active entries | Prepared, executing, result persisted, result published, and terminal are distinct phases. Publication and the root admission cursor fence prove when an entry can be compacted. An older record is upgraded only with its original identity and phase meaning. |
| `host-ticket-agent` federation relay WAL | Version 2, 8 MiB serialized ceiling with manifest supplied entry and byte limits | Target terminal bytes survive until source publication. Only delivered or rejected entries may be removed at the bound; a full pending WAL refuses new work. |
| `coh recipe` | `cohesix-recipe-journal/v1`, 256 KiB and 32 attempts | Exact operation, topology, input, runtime, ticket and reservation stay bound to the original attempt. Local acknowledgement is not signed terminal proof; uncertain work retains its reservation. |
| `coh workflow` and Python orchestration | Version 1 deployment and operation projections, without an independent executor journal | These clients submit existing tickets and inspect signed terminal evidence. They must not create a second scheduler, reset cumulative accounting, or infer a terminal result from a local status. |

The M28 shared job and authority extensions must retain the existing journal
files and decode old identities without treating missing new fields as grants.
Execution termination and result delivery are separate obligations; an
acknowledged delivery may retire only under its declared durable fence. Corrupt
or full state must refuse new effects with a deterministic error.

## Evidence ledger

| Obligation | Result | Retained evidence |
| --- | --- | --- |
| Shared job lifecycle and CLI/REST/Python source parity | Host checks passed; installed live parity pending | `cargo test --locked -p coh --test recipe --test run --test workflow`; `cargo test --locked -p cohesix-rest --lib`; `cargo test --locked -p host-ticket-agent --lib`; focused Python command below. These verify client requests and host journal transitions. |
| Standing scope, atomic accounting and current-state refusal | Host checks passed; one exploratory GPU outcome confirmed; full live refusals pending | `cargo test --locked -p cohesix-authority`; `cargo test --locked -p hive-gateway` (91 unit and two protocol-control tests after the subject fix); `cargo test --locked -p host-ticket-agent --lib`; focused Python command below. |
| Generated protocol flag truth table and startup ceiling | Host checks passed | `cargo test --locked -p coh-rtc`; `cargo test --locked -p hive-gateway --test agent_protocol_controls`; `cargo test --locked -p coh --lib doctor::`. MCP/A2A listeners are not implemented or claimed. |
| Live bounded GPU action and allowlisted service recovery | Native GPU/target path observed in a mixed-source preflight; service and exact-source case reports pending | `out/audit/m28-source-c7ca45655/gpu-job-04-observation.json` and the content-addressed native object named below. Neither `m28-jobs-live` nor `m28-authority-live` has passed. |
| Generated consistency and selected source checks | Canonical generated checks passed; final exact-source target case pending | `scripts/check-generated.sh --update-pi4-test-profile`; `git diff --check`; `cargo fmt --all --check`; focused Python command below. |

The focused Python command passed 61 tests:

```bash
.venv/bin/python -m pytest -q \
  tools/cohesix-py/tests/test_orchestration.py \
  tools/cohesix-py/tests/test_workload.py \
  tools/cohesix-py/tests/test_authority.py \
  tools/cohesix-py/tests/test_selected_jobs.py \
  tests/test_provider_matrix.py tests/test_provider_m28_live.py
```

The selected macOS `qemu_smp_production` seL4 profile built and validated with
source and artifacts required. The Cohesix root task, three Worker images,
kernel, rootserver, CPIO and immutable launch record built under
`out/cohesix-m28/`. That build is construction evidence only. A subsequent
`--launch-existing --raw-qemu` attempt exited before guest boot at the installed
QEMU 11.0.3 HVF `hvf_arch_init_vcpu` assertion; it supplies no target proof.
The separately selected `qemu_smp_kvm_production` seL4 profile built and
validated under `out/sel4/profile-v2/qemu-smp-kvm-production`, with validation
recorded in `out/audit/qemu-smp-kvm-production-m28.json`. Its Cohesix image
built under `out/cohesix-m28-kvm/` and booted on the Linux AArch64 NVIDIA host
through KVM. The retained serial transcript at
`out/audit/qemu-serial.log` on that host reached `root-console.start.ok` and
reported the selected 31,250,000 Hz timer. This preflight is not exact-source
M28 job evidence: the native provider and authenticated target cases had not
yet run, and the Pi driver acceptance line is red in this QEMU guest.

A private KVM derivation from source `83fe1105913ed4324443519fe5aa073fbf2f9001`
then booted with selected manifest SHA-256
`7e7a27ee02720e147fbf7f8386a689c1c6cb678c11377d574423a9157213bd52`.
Authenticated `/proc/boot` readback is retained at
`out/audit/m28-source-c7ca45655/qemu-authenticated-read.txt`. After two
gateway repairs, a native Linux AArch64 CUDA host accepted
`m28-gpu-work-04` under standing scope `m28-gpu`, Worker `worker-3`, and root
lease `m28-lease-04`. The ledger reported `confirmed` execution and
`acknowledged` delivery; the target retained `claimed`, `running`, and one
`succeeded` result for the same admission. The content-addressed native object
`629965998cd77c03d35b018248eddf855c7452ebdb395d0ea935a16ab022aa47`
reports `cuda_output_verified`, the selected device UUID, 768 allocated
bytes, one stream, and the verified output SHA-256. Its exact bytes and the
read-only reconciliation are retained in
`out/audit/m28-source-c7ca45655/`. The earlier rejected admissions were
checked to have no ledger allocation before a fresh identity was submitted.

This is a useful live preflight, not milestone acceptance: the guest image was
built at the initial source commit, while the repaired gateway and evidence
reader came from later commits. The exact-source `m28-jobs-live` and
`m28-authority-live` reports remain unrun. The confined
`cohesix-m28-probe.service` is now installed and active on Merlin2; its
installed bytes match the private source file SHA-256
`a5bcef68274bfe6bd4446233af436a3e2415af1d16ccc7b30b465a9c2f81326c`.
The governed service action and live interrupted-delivery observation remain
to be exercised.

No M28 acceptance or Release B qualification is claimed while these rows are
pending. Material AI assistance: contract inventory and implementation drafting
were performed with Codex; source, test and live evidence remain reviewable by
their exact commands and artifacts.
