// CLASSIFICATION: COMMUNITY
// Filename: PLAN9_HOOKS.md v0.2
// Author: Lukas Bower
// Date Modified: 2026-12-31

# Plan9 Native Cloud Hooks

Cohesix uses Plan9's namespace and 9P services to manage cloud-native hooks
without embedding external interpreters. Hooks are simple `rc` scripts that
communicate over 9P-mounted paths such as `/srv` and `/mnt`.

## Operation

1. Queen nodes mount a writable service at `/srv/cloud` for incoming hook
   instructions.
2. Worker nodes watch specific files (e.g., `/srv/validator/log`) and issue
   events by writing to queues under `/srv/alerts` or `/srv/upload`.
3. External automation may mount these directories via Secure9P to read
   status or provide additional commands.

## Security Model

- Hooks run as `rc` scripts with no privileged binaries.
- All communications occur through 9P file operations, enforcing capability
  checks from the kernel validator.
- No Python or other interpreters are required on the target nodes, reducing
  attack surface and memory usage.

## Example Workflows

- **Trace Upload**: `upload_trace.rc` copies completed trace files from
  `/srv/trace/completed` to an upload mount. A remote system picks them up over
  9P.
- **Validator Alerting**: `watch_validator.rc` tails the validator log and
  appends any violation lines to `/srv/alerts/validator` for the Queen to read.
- **Commit Attestation**: `attest_commit.rc` watches for completed traces and
  writes a simple hash placeholder to `/srv/attest/<trace>` for Secure9P to
  sign.
- **Multi‑agent Alerts**: `watch_multiagent_validator.rc` groups violation
  lines by agent ID under `/srv/alerts/<agent>/log` so that external tooling can
  react per agent.

These hooks maintain Cohesix's minimal footprint while enabling rich
cloud‑native automation through the Plan9 filesystem.

## External Watcher Example

On Linux or macOS you can mount the Cohesix namespace via `9pfuse` and
react to alerts with simple scripting:

```bash
9pfuse localhost /mnt/cohesix
inotifywait -m /mnt/cohesix/srv/alerts -e create --format '%w%f' |
while read -r path; do
    cat "$path"
    echo "[simulated] upload $path to S3"
    rm "$path"
done
```
