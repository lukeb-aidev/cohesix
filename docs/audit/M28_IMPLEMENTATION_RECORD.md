<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Record Milestone 28 contract migration, focused checks, and selected live acceptance evidence. -->
<!-- Author: Lukas Bower -->

# Milestone 28 implementation record

Milestone 28 is complete under `m28-selected-job-contract`,
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

The M28 shared job and authority extensions retain the existing journal
files and decode old identities without treating missing new fields as grants.
Execution termination and result delivery remain separate obligations; an
acknowledged delivery may retire only under its declared durable fence. Corrupt
or full state refuses new effects with a deterministic error.

## Evidence ledger

| Obligation | Result | Retained evidence |
| --- | --- | --- |
| Shared job lifecycle and CLI/REST/Python source parity | PASS: focused host contracts and live REST submission, terminal, reconciliation and delivery | `cargo test --locked -p coh --test recipe --test run --test workflow`; `cargo test --locked -p cohesix-rest --lib`; `cargo test --locked -p host-ticket-agent --lib`; focused Python command below; `m28-jobs-live` and interrupted delivery evidence below. |
| Standing scope, atomic accounting and current-state refusal | PASS: focused host contracts, live bounded actions and two stale fact refusals | `cargo test --locked -p cohesix-authority`; `cargo test --locked -p hive-gateway` (91 unit and two protocol-control tests after the subject fix); `cargo test --locked -p host-ticket-agent --lib`; focused Python command below; `m28-authority-live` below. |
| Generated protocol flag truth table and startup ceiling | Host checks passed | `cargo test --locked -p coh-rtc`; `cargo test --locked -p hive-gateway --test agent_protocol_controls`; `cargo test --locked -p coh --lib doctor::`. MCP/A2A listeners are not implemented or claimed. |
| Live bounded GPU action and allowlisted service recovery | PASS: exact selected KVM target plus native Merlin2 GPU and systemd operations | `out/audit/m28-source-97e5dbeef/m28-jobs-live-summary.json`, `m28-authority-live-summary.json` and three digest-named native objects below. |
| Interrupted result delivery | PASS: confirmed service effect remained pending during gateway interruption, then acknowledged under its original admission | `out/audit/m28-source-97e5dbeef/interrupted-delivery.json`; durable agent journal and target terminal retained on Merlin2. |
| Generated consistency and selected source checks | Canonical generated checks and selected source build passed | `scripts/check-generated.sh --update-pi4-test-profile`; `git diff --check`; `cargo fmt --all --check`; selected KVM image build and hashes below. |

The focused Python command passed 61 tests:

```bash
.venv/bin/python -m pytest -q \
  tools/cohesix-py/tests/test_orchestration.py \
  tools/cohesix-py/tests/test_workload.py \
  tools/cohesix-py/tests/test_authority.py \
  tools/cohesix-py/tests/test_selected_jobs.py \
  tests/test_provider_matrix.py tests/test_provider_m28_live.py
```

After adding live QEMU source-image binding, `.venv/bin/python -m pytest -q
tests/test_provider_m28_live.py` passed 10 focused cases. The
`host-ticket-agent` two-lane raw ticket owner check passed with
`cargo test --locked -p host-ticket-agent --bin host-ticket-agent
raw_ticket_owner_polls_with_shared_snapshot_ingress`; the final Merlin2
service run exercised that corrected lane against the target.

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

That preflight remains historical and is not the milestone acceptance run. A
later functional run at host source `e5ceb52c983381af11fb04102ccaaca010d70ec0`
also retained GPU, service and interrupted-delivery results, but the live
guest still carried the prior source marker. It is retained under
`out/audit/m28-source-e5ceb52c9/` as diagnostic evidence, not closure proof.

## Exact selected acceptance

At source revision `97e5dbeefc170cdd9b6bdbae1899d9ef6032325d`, the
selected `qemu_smp_kvm_production` guest ran under KVM on Merlin2. Authenticated
`/proc/boot` and gateway bounds matched the generated manifest SHA-256
`7e7a27ee02720e147fbf7f8386a689c1c6cb678c11377d574423a9157213bd52`.
The live QEMU process's rootserver contained exactly one `[BUILD]` source marker
for `97e5dbeefc17`; its rootserver, elfloader and initrd SHA-256 values were
`4eab00716474ccc131064f2e6bd43e9e4c4d4a8d847a42da941ddb786dbf51ce`,
`b2faa3eecb1198549aa7ab4887713678af61368e55daaae0ba0071e7d5114222`
and `b0b18f0eca027a4926bd8bbcede684f35bf9f55cb5ff2468160170813cae7d5c`.
The rebuilt bytes matched the copied Merlin2 image and its validated Linux
KVM launch record. The live case runner now checks this process and these
loaded artifact paths before accepting a case.
The Merlin2 binaries were bound by SHA-256: gateway
`617ea3ae8077637802b70f7cb5bad5c7961961e1d545457f34527b2142cba034`,
agent `a8cc5fa28abfcd9f7ff4be4102d9f13b08756259f2ad0fa5c65e76a09b09b42e`,
GPU bridge `a846959c45576d55e18bc7038ebc29d0b06ab3f6844fac9fd2fcc930ab79208b`,
and CUDA helper `8240b2a86f08d59db17539fc6565ed87418378858f45149f68986e28cf34a852`.
The selected device UUID was `8cc9c4072cc0587a8d93b7e2754559cb` under
NVIDIA driver 595.78. The probe unit installed on Merlin2 has SHA-256
`b2e311509dafd5a9767ec99a4bbe092cf579083e330e95c4bb3002086b8fd41f`;
its installed polkit rule SHA-256 is
`34d87a8d91fb0e555d9adfd06de614add8b9ef202c24e6df3779ed7d7a73de32`
and allows `wizard` to restart only this unit. The installed unit
was active with `Result=success` after the governed restart.

The selected Test Plan commands passed:

```bash
scripts/ci/provider_conformance_run.sh --matrix configs/provider_conformance.toml \
  --case m28-jobs-live --reference-config out/private/m28/reference-final-01.toml \
  --host-profile jetson-orin-nano-jp7 \
  --state-dir out/private/m28/evidence-final-97e-r1/m28-jobs-live
scripts/ci/provider_conformance_run.sh --matrix configs/provider_conformance.toml \
  --case m28-authority-live --reference-config out/private/m28/reference-final-01.toml \
  --host-profile jetson-orin-nano-jp7 \
  --state-dir out/private/m28/evidence-final-97e-r1/m28-authority-live
```

`m28-jobs-live` admitted `m28-final-gpu-admit-01` and
`m28-final-service-admit-01` under the `m28` delegated subject. Both records
reached `confirmed` execution and `acknowledged` delivery with one exact target
terminal each. Their native evidence objects are
`3165b40c2452dcaeb565fe509419aab6c61ad92bc3c1dd3593a066a9889fee5a`
and `5c48642ab511cd1e129ecdd93a4ee02861f9ddc1414ca6bca5fdbb220648a6dd`.
`m28-authority-live` refused independently mutated GPU and service state epochs
before reservation, with no native effect. A separately admitted
`m28-final-service-admit-02` then restarted the same unit while the gateway was
paused after systemd dispatch. The retained ledger and compatibility journal
showed `confirmed` execution, `pending` delivery and a persisted result. On
gateway resume, the same admission reached `acknowledged` delivery and one
target `succeeded` terminal without effect replay; native evidence object
`025338955c50779313dfcaa7ddd232efa3405043efedeff68a84bdde6fe88397`
binds the before/after systemd invocation IDs and job path. The earlier raw
ticket wake defect was fixed in `host-ticket-agent`; an expired request then
settled as `refused_no_effect/acknowledged` without a restart in the prior
diagnostic ledger. All three retained admissions in the final ledger have
acknowledged delivery and no unresolved allocation.

The summary and native objects are copied to
`out/audit/m28-source-97e5dbeef/`; the original job journal, gateway log,
serial transcript and provider evidence remain under
`/home/wizard/cohesix-m28-staging-20260924/out/private/m28/` on Merlin2.
These are selected component observations, not Pi hardware, sustained pressure,
MCP/A2A endpoint, or Release B acceptance. The full suite was not run.
Material AI assistance: contract inventory, implementation drafting, focused
tests and live evidence review were performed with Codex; the source, commands,
hashes and retained outcomes above remain independently inspectable.
