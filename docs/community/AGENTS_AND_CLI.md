// CLASSIFICATION: COMMUNITY
// Filename: AGENTS_AND_CLI.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-06-20

# Agents and CLI

Cohesix relies on Codex-driven agents and a small set of CLI tools for automation and orchestration. Agents follow the YAML schema in `AGENTS.md` and are executed through `cohcli` commands.

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

## CLI Summary
- **cohcli** – main interface for status, dispatching SLMs, and running agents
- **cohrun** – demo launcher and orchestrator helper
- **cohtrace** – trace inspection and federation utilities
- **cohcc** – compiler front-end for Cohesix IR
- **cohcap** – capability management for demo scenarios

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
