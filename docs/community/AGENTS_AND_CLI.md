// CLASSIFICATION: COMMUNITY
// Filename: AGENTS_AND_CLI.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-12

# Agents and CLI Usage

Cohesix automation revolves around a set of AI-driven agents defined in a YAML schema. Each agent specifies its role, prompt templates, input/output schemas, and test cases.

## Agent Schema
```yaml
- id: <string>
  role: <string>
  description: <string>
  language: <string>
  batch: <string | array>
  batch_class: <minor|major|multi-arch|demo-critical>
  prompt_template:
    system: <string>
    user: <string>
  input_schema: <JSON-schema>
  output_schema: <JSON-schema>
  test_cases: [...]
  metadata:
    CODEX_BATCH: YES
    BATCH_ORIGIN: <string>
    BATCH_SIZE: <integer>
```
Agents checkpoint every 10 files and log to `codex_logs/` for auditability.

## CLI Highlights
Key commands from `cohcli` and `cohup`:
- `cohup patch <target> <binary>` – apply a live patch.
- `cohcli agent migrate <agent_id> --to <node>` – move an agent.
- `cohup join --peer <queen>` – join a federation.
- `cohup list-peers` – list known peers.
- `cohrun kiosk_start` – deploy kiosk UI bundle.
- `cohtrace kiosk_ping` – trigger kiosk federation event.

## Codex Workflow
1. Install prerequisites (`openai`, `gh`, `cohcli`, VS Code extension).
2. Run agents with `cohcli codex run <agent_id> --file <path>`.
3. Store all outputs in `codex_logs/` and ensure `validate_metadata_sync.py` passes.
4. Require human code review before merging Codex-generated changes.

## Tooling
- `tools/validate_batch.sh` and `tools/annotate_batch.py` verify batch headers and metadata.
- `tools/replay_batch.sh` replays hydration logs after crashes.
- `tools/perf_log.sh` collects build and boot metrics on Jetson and AWS targets.

## Remote Access Basics
To connect a home Worker to a cloud Queen:
1. Configure dynamic DNS if you lack a static IP.
2. Set up SSH keys and forward a port on your router.
3. Harden `sshd` and firewall rules on the Worker.
4. Optionally mount the Queen over TLS or create a reverse SSH tunnel.
5. Validate with `coh-svc ping --worker <id>` and check logs for errors.

These practices keep Cohesix deployments reproducible and secure across both local and cloud nodes.
