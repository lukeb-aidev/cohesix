<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Describe Cohesix Python SDK installation, orchestration playbooks, and integration adapters. -->
<!-- Author: Lukas Bower -->
# cohesix (Python)

`cohesix` is a thin, non-authoritative Python SDK for Cohesix control-plane
operations. It mirrors existing control-file and console semantics; it does not
introduce new protocol behavior.

## Install

Editable source install:

```bash
python3 -m pip install -e tools/cohesix-py
```

With host integration adapters:

```bash
python3 -m pip install -e 'tools/cohesix-py[integrations]'
```

With PEFT/LoRA helper package probes:

```bash
python3 -m pip install -e 'tools/cohesix-py[ml]'
```

The core wheel requires Python 3.11 or later. Release compatibility is tested
on CPython 3.11 and 3.13. The wheel is target-neutral; it does not bundle or
select a QEMU/Pi manifest.

## Backends

- `TcpBackend`: direct console (`AUTH` + `ATTACH`) for single-client workflows.
- `RestBackend`: hive-gateway REST projection for multiplexed clients,
  including request-auth headers.
- `FilesystemBackend`: mounted Secure9P namespace (`coh mount`).
- `MockBackend`: deterministic `host-model` backend for tests and local demos;
  never target evidence.

Optional REST Worker bounds are declarations only, and missing bounds or
`backend_class` remain `unknown`. No backend connection, control ACK, local
file, or JSON object creates Worker READY or target proof.

## Milestone 26e Worker compatibility

Worker APIs require an explicit generated `cohesix-python-profile/v1`
contract. Use the QEMU contract only with `qemu_smp_production` and the Pi
contract only with `pi4_production`:

```python
from cohesix import CohesixClient, MockBackend, load_profile_contract

contract = load_profile_contract(
    "configs/generated/cohesix_python_qemu_smp_production.json",
    expected_target="qemu",
)
client = CohesixClient(
    MockBackend("out/examples/worker-model"),
    profile_contract=contract,
)

admitted = client.worker_spawn("gpu", "gpu-receipt-1")
ready = client.worker_wait_ready("gpu", "gpu-receipt-1")
closing = client.worker_teardown("gpu", "gpu-receipt-1")
```

`admitted.lifecycle == "queued"` is only request admission. `ready` is a
separate bounded telemetry observation; under Mock its proof class is
`host-model`. Heartbeat, GPU, and LoRA are executable. WorkerBus is model-only
and is refused before any backend write. Telemetry uses the generated canonical
`/shard/<label>/worker/<id>/telemetry` path; legacy `/worker` is gated by the
selected contract. A bound `MockBackend` derives the same target-specific shard
width from that contract before it creates any Worker observation.

`parse_receipt` preserves version-1 compatibility and accepts the
generation-bound GPU/LoRA receipt encodings only when classified as
`source="local-admitted"`. The exact actions are GPU grant/renew/release and
PEFT export/import/activate/rollback. Python receipt objects remain
non-authoritative; stale identity is reported as `stale`, never rebound.

The independent state axes are request admission, READY, provider completion,
receipt, artifact, execution proof, Python projection compatibility,
runtime-release acceptance, and production-use-case acceptance. The last two
remain false until their separate evidence gates promote them.

## High-level orchestration

- `CohesixOrchestrator`: typed schedule/lease/export/approval controls.
- `/proc` observability snapshots for scheduler and lease state.
- Host integration probes for `systemd`, Docker, Kubernetes, NVML, and PEFT
  runtime versions.
- Native evidence + receipt APIs on `CohesixClient`:

  - `evidence_pack(...)` and `evidence_timeline(...)`
  - `gpu_lease_with_receipt(...)` and `run_command_with_receipt(...)`

## Playbooks (1k-worker use-case coverage)

Built-in playbooks cover:

- 1000 Mac use cases: release factory, private PEFT grid, endpoint compliance.
- 1000 Jetson use cases: traffic safety, manufacturing QA/safety, critical
  infrastructure mesh.
- Mixed fleet use cases: closed-loop AI factory, medical edge AI, logistics digital twin.

List playbooks:

```bash
cohesix-playbook --list
```

Dry-run a playbook with no control writes:

```bash
cohesix-playbook --playbook mixed-closed-loop-ai-factory --dry-run --mock
```

Execute against live TCP console:

```bash
cohesix-playbook --playbook jetson-traffic-safety --tcp-host 127.0.0.1 --tcp-port 31337
```

Artifacts are written under `out/examples/playbooks/<playbook-id>/`.

## Existing examples

```bash
python3 tools/cohesix-py/examples/lease_run.py --mock
python3 tools/cohesix-py/examples/peft_roundtrip.py --mock
python3 tools/cohesix-py/examples/telemetry_write_pull.py --mock
```

## Evidence pack integration kits (Milestone 25e)

These examples operate on an evidence pack directory produced by
`coh evidence pack` and run offline once the pack exists.

```bash
cargo run -p coh -- --mock evidence pack --out out/evidence/mock
python3 tools/cohesix-py/examples/ci_evidence_pack.py --pack out/evidence/mock \
  --out out/evidence/mock/ci_summary.json
python3 tools/cohesix-py/examples/siem_export_ndjson.py --pack out/evidence/mock \
  --out out/evidence/mock/siem.ndjson
```

## Notes

- Target-neutral fallback bounds are generated in `cohesix/generated.py`;
  Worker identity and bounds come from an explicit target profile contract.
- Keep one TCP console client at a time (or use REST via `hive-gateway`).
- `RestBackend` sends request-auth headers when `request_auth_token` is set or
  when `HIVE_GATEWAY_REQUEST_AUTH_TOKEN`, `COHSH_REST_AUTH_TOKEN`, or
  `COH_REST_AUTH_TOKEN` is present.

Build and verify the target-neutral wheel on both supported interpreters:

```bash
python3 -m pip wheel --no-deps --wheel-dir out/python-wheels tools/cohesix-py
scripts/ci/python_compat_run.sh \
  --wheel-smoke \
  --wheel-dir out/python-wheels \
  --package-manifest out/python-compat/m26e-python-package.json \
  --state-dir out/python-compat/m26e-wheel
```

See [`docs/PYTHON_SUPPORT.md`](../../docs/PYTHON_SUPPORT.md) for target
projection commands and proof-boundary details.
