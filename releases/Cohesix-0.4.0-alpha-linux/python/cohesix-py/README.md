<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Describe the Cohesix Python client package and usage. -->
<!-- Author: Lukas Bower -->
# cohesix (Python)

The `cohesix` Python package provides a thin, non-authoritative client for the Cohesix
control plane. It mirrors existing console grammar and filesystem semantics and does
not introduce new control-plane behavior.

## Backends
- `TcpBackend`: connects to the TCP console (`AUTH` + `ATTACH`) and issues `LS`/`CAT`/`ECHO`.
- `FilesystemBackend`: operates on a mounted Secure9P namespace (via `coh mount`).
- `MockBackend`: deterministic in-memory filesystem for tests and examples.

## Quick start
```bash
python3 -m pip install -e tools/cohesix-py
python3 tools/cohesix-py/examples/lease_run.py --mock
python3 tools/cohesix-py/examples/peft_roundtrip.py --mock
python3 tools/cohesix-py/examples/telemetry_write_pull.py --mock
```

## Notes
- All limits are bounded and derived from `coh-rtc` generated defaults.
- Example artifacts land under `out/examples/`.
