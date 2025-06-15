// CLASSIFICATION: COMMUNITY
// Filename: 9P_README.md v0.1
// Date Modified: 2025-07-31
// Author: Lukas Bower

# Cohesix 9P Server Overview

This document summarises the capabilities of the 9P service shipped with
Cohesix. The implementation is based on the Rust `ninep` crate and covers
core protocol messages used by the runtime.

## Supported operations

- `walk`
- `open`
- `read`
- `write`
- `clunk`
- `stat`

Remote namespaces may be joined via `mount_remote()` which proxies requests to a
remote 9P server using TCP. Access to `/proc`, `/mnt`, `/srv`, and `/history`
is validated before writes occur. Reads are allowed everywhere by default.

## Limitations

The server is intentionally minimal. Extended attributes and authentication are
not implemented. Write attempts to restricted paths return permission errors and
are logged.


## Security and Trace Integration

All write operations are subject to capability enforcement via `/etc/cohcap.json`. Attempts to access restricted namespaces such as `/srv` or `/history` without explicit grants are denied and logged.

All `walk`, `read`, and `write` calls are traced via `cohtrace` and appear in trace replays for validator inspection.

Future extensions may add TLS-wrapped transport and role-scoped namespace enforcement as defined in `SECURITY_POLICY.md`.
