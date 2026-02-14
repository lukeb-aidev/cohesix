<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Describe Cohesix Python SDK installation, orchestration playbooks, and integration adapters. -->
<!-- Author: Lukas Bower -->
# cohesix (Python)

`cohesix` is a thin, non-authoritative Python SDK for Cohesix control-plane operations.
It mirrors existing control-file and console semantics; it does not introduce new protocol behavior.

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

## Backends
- `TcpBackend`: direct console (`AUTH` + `ATTACH`) for single-client workflows.
- `RestBackend`: hive-gateway REST projection for multiplexed clients.
- `FilesystemBackend`: mounted Secure9P namespace (`coh mount`).
- `MockBackend`: deterministic backend for tests and local demos.

## High-level orchestration
- `CohesixOrchestrator`: typed schedule/lease/export/approval controls.
- `/proc` observability snapshots for scheduler and lease state.
- Host integration probes for `systemd`, Docker, Kubernetes, NVML, and PEFT runtime versions.

## Playbooks (1k-worker use-case coverage)
Built-in playbooks cover:
- 1000 Mac use cases: release factory, private PEFT grid, endpoint compliance.
- 1000 Jetson use cases: traffic safety, manufacturing QA/safety, critical infrastructure mesh.
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

## Notes
- Bounds are enforced from manifest-derived defaults in `cohesix/generated.py`.
- Keep one TCP console client at a time (or use REST via `hive-gateway`).
