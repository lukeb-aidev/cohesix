// CLASSIFICATION: COMMUNITY
// Filename: 9P_README.md v0.2
// Date Modified: 2025-06-15
// Author: Lukas Bower

# Cohesix 9P and Secure9P Overview

This document describes the core 9P server provided by Cohesix, as well as Secure9P, a hardened variant under development. These protocols form the basis for IPC, namespace sharing, and agent sandbox enforcement within the runtime.

---

## 1. Standard 9P Server

The base 9P service is implemented in Rust using the `ninep` crate. It supports standard operations required by the runtime and agent communication layer.

### Supported Operations

- `walk`
- `open`
- `read`
- `write`
- `clunk`
- `stat`

Remote namespaces may be joined via `mount_remote()`, which proxies requests to a remote 9P server over TCP. The following paths are checked during write attempts:

- `/proc`
- `/mnt`
- `/srv`
- `/history`

Reads are allowed everywhere by default. Write operations to restricted paths without capability grants are rejected.

### Limitations

- No authentication in base 9P
- No extended attribute support
- No TLS wrapping or encryption
- Minimal logging beyond access trace

---

## 2. Secure9P (Hardened Variant)

Secure9P adds encryption, authentication, and fine-grained namespace enforcement to the base protocol.

### Features

- TLS-wrapped 9P transport with planned client certificate auth
- Capability token enforcement from `/etc/cohcap.json`
- Namespace root resolution based on agent role and ID
- Path sandboxing and rule validation
- Full trace integration to `/log/net_trace_secure9p.log`

### Components

- `secure_9p_server.rs` — TLS listener and 9P message parser
- `auth_handler.rs` — Extracts agent identity via mTLS or JWT
- `namespace_resolver.rs` — Maps agent to virtual root
- `sandbox.rs` — Path validator
- `policy_engine.rs` — Capability and rule checker
- `validator_hook.rs` — Emits trace entries to replay engine

### Current Status

- Validator hooks complete
- Namespace and capability checks in beta
- TLS listener operational with test CA
- Planned integration with `cohrole` and `/srv` sandbox during boot

---

## 3. Security and Trace Integration

All `walk`, `read`, and `write` calls (from both 9P and Secure9P) are traced via `cohtrace` and replayable via the validator.

- Unauthorized write attempts are denied and logged
- Secure9P traffic is logged separately to `/log/net_trace_secure9p.log`
- Base 9P activity logs to `/log/net_trace.log`

---

## 4. Roadmap

| Feature                         | Status       | Target |
|----------------------------------|--------------|--------|
| Capability enforcement           | ✅ Stable     | v0.2   |
| Secure9P TLS transport           | 🧪 Testing    | v0.3   |
| Namespace root resolution        | 🧪 Testing    | v0.3   |
| Validator trace replay           | ✅ Complete   | v0.2   |
| Secure9P + boot sandbox          | 🚧 Planned    | v0.4   |
