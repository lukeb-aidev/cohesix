// CLASSIFICATION: COMMUNITY
// Filename: AGENTS_AND_CLI.md v1.1
// Author: Lukas Bower
// Date Modified: 2025-07-18

# Agents and CLI

Cohesix relies on Codex-driven agents and a small set of CLI tools for automation and orchestration. Agents follow the YAML schema below and are executed through `cohcli` commands.

## Agent Schema
```
- id: <string>
  role: <string>
  description: <string>
  language: <string>
  batch: <string|array>
  batch_class: <minor|major|multi-arch|demo-critical>
  prompt_template:
    system: <string>
    user: <string>
  input_schema: <JSON-schema>
  output_schema: <JSON-schema>
  metadata:
    CODEX_BATCH: YES
    BATCH_ORIGIN: <uri>
    BATCH_SIZE: <int>
```
Agents checkpoint every 10 files and log to `codex_logs/`. Recovery uses `tools/replay_batch.sh`.

Agents now respect environment variables such as COHESIX_ENS_TMP and COHESIX_TRACE_TMP to ensure all generated output is written to writable temp directories, especially when executing in restricted or sandboxed environments.

## CLI Summary
All CLI commands are invoked via small wrapper scripts that call the Python sources using `/usr/bin/env`. This keeps the canonical `.py` files compliant with the classification header rule.
- **cohcli** – main interface for status, dispatching SLMs, and running agents
- **cohrun** – demo launcher and orchestrator helper – supports TMPDIR override for isolated boot environments
- **cohtrace** – trace inspection and federation utilities
- **cohcc** – compiler front-end for Cohesix IR
- **cohcap** – capability management for demo scenarios
- **cohshell** – wrapper for Cohesix BusyBox; symlink to `/bin/sh` for minimal rootfs

Example usage:
```bash
# run an agent
cohcli codex run scaffold_service --file new.rs

# launch physics demo
cohrun physics_demo

# view worker list
cohtrace list
```

Validators run automatically via `validate_metadata_sync.py` and CI hooks to ensure all generated files match `METADATA.md`.


## Temporary Directory Behavior

All agents and CLI tools now support use of environment variables to redirect output and working paths to writable temp directories:

- `TMPDIR`: fallback base for all temp paths
- `COHESIX_ENS_TMP`: override for ensemble agent directories
- `COHESIX_TRACE_TMP`: override for validator trace outputs

This allows tests, simulations, and builds to succeed in environments where root or shared paths are not writable.
