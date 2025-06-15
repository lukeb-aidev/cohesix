// CLASSIFICATION: COMMUNITY
// Filename: UTILS_README.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-31

# System Utilities

This guide lists the command line utilities bundled with Cohesix. Each
utility has a corresponding manual page under `docs/man/`.

See `docs/man/MANIFEST.md` for the authoritative list. Key tools include:

- `cohcli`, `cohagent`, `cohtrace` – management CLIs
- BusyBox tools such as `ls`, `df`, `top`, `finger`, `who`, and more
- Networking utilities like `ping`, `ssh`, `wget`
- Package management via `cohpkg`


Refer to the individual man pages for usage details.

## Trace and Validator Integration

All utility invocations are traceable via `cohtrace`. For example, running `cohcli`, `cohtrace`, or `cohpkg` emits trace entries visible in `/log/trace/`. These logs are consumed by the validator for replay and regression checks.

Utilities must use `$TMPDIR` or similar sandbox-safe paths for intermediate files.
