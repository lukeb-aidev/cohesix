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
| Shared job lifecycle and CLI/REST/Python source parity | Host checks passed; installed live parity pending | `cargo test --locked -p coh --test recipe --test run --test workflow`; `cargo test --locked -p cohesix-rest --lib`; `cargo test --locked -p host-ticket-agent --lib`; focused Python command below. These verify client requests and host journal transitions, not a target effect. |
| Standing scope, atomic accounting and current-state refusal | Host checks passed; live refusals pending | `cargo test --locked -p cohesix-authority`; `cargo test --locked -p hive-gateway`; `cargo test --locked -p host-ticket-agent --lib`; focused Python command below. |
| Generated protocol flag truth table and startup ceiling | Host checks passed | `cargo test --locked -p coh-rtc`; `cargo test --locked -p hive-gateway --test agent_protocol_controls`; `cargo test --locked -p coh --lib doctor::`. MCP/A2A listeners are not implemented or claimed. |
| Live bounded GPU action and allowlisted service recovery | Pending | — |
| Generated consistency and selected source checks | Passed; target and native host work pending | `scripts/check-generated.sh --update-pi4-test-profile`; `git diff --check`; `cargo fmt --all --check`; focused Python command below. |

The focused Python command passed 60 tests:

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
M28 job evidence: the native provider and authenticated target cases have not
yet passed, and the Pi driver acceptance line is red in this QEMU guest.

No M28 acceptance or Release B qualification is claimed while these rows are
pending. Material AI assistance: contract inventory and implementation drafting
were performed with Codex; source, test and live evidence remain reviewable by
their exact commands and artifacts.
