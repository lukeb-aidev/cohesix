// CLASSIFICATION: PRIVATE
// Filename: FEDERATION.md v1.0
// Author: Lukas Bower
// Date Modified: 2025-07-31

# Federation Lifecycle

Queens form trust groups by exchanging signed handshakes using the keyring
located under `/srv/federation/known_hosts/`. Agent migrations copy snapshot
files to `/srv/federation/state/<peer>/incoming` before being restored on the
target node.

All federation events (join, promote, retire, failover) are traced via `cohtrace` and logged to `/log/federation/`. Validator replay is required after quorum loss.

This document describes peer discovery, key rotation, and fault isolation in the
federated cluster.

## Trace and Replay

Federation transitions must produce a trace and snapshot suitable for validator replay. Required files:
- `/log/federation/<ts>.log`
- `/history/snapshots/federation_<ts>.json`
- `/srv/federation/state/<peer>/incoming/` (if applicable)

The CI harness will fail federation-related tests if trace output is missing or incomplete.
