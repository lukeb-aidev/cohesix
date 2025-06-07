// CLASSIFICATION: PRIVATE
// Filename: FEDERATION.md v1.0
// Author: Codex
// Date Modified: 2025-06-07

# Federation Lifecycle

Queens form trust groups by exchanging signed handshakes using the keyring
located under `/srv/federation/known_hosts/`. Agent migrations copy snapshot
files to `/srv/federation/state/<peer>/incoming` before being restored on the
target node.

This document describes peer discovery, key rotation, and fault isolation in the
federated cluster.
