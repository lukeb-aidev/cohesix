// CLASSIFICATION: COMMUNITY
// Filename: NETWORKING.md v0.3
// Author: Lukas Bower
// Date Modified: 2025-06-15

# Cohesix 9P Server Overview

The 9P server is a core part of Cohesix IPC (inter-process communication) and remote namespace access.

A hardened variant called Secure9P is also under development, adding TLS transport, authentication, and namespace isolation.

---

## Basic Operation

- Listens on TCP port 564 by default
- Exposes a 9P service mountable by Workers or other agents
- Maps namespace from `/srv` and logs activity to `/log/net_trace.log`

---

## Security and Trace Integration

All `walk`, `read`, and `write` calls are traced via `cohtrace` and appear in trace replays for validator inspection.

All write operations are subject to capability enforcement via `/etc/cohcap.json`.

Unauthorized write attempts are rejected and logged.

Future extensions may add TLS-wrapped transport and role-scoped namespace enforcement as defined in `SECURITY_POLICY.md`.

For enhanced authentication and encrypted transport, the Secure9P variant is under active development. Secure9P wraps 9P traffic in TLS and uses capability tokens and role-aware namespace resolution as described in `SECURE9P_OVERVIEW.md`.
