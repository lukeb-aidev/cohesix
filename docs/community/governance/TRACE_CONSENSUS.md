// CLASSIFICATION: COMMUNITY
// Filename: TRACE_CONSENSUS.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-31


The Trace Consensus Protocol is used by QueenPrimary and RegionalQueen roles to ensure agreement on event traces generated across distributed nodes.

## Overview

Each participating Queen node maintains a trace ring buffer and periodically exchanges signed segments with peers. A consensus vote is triggered under these conditions:
- New trace segment exceeds 4KB or 1s age
- Peer checkpoint hash mismatch
- Explicit sync command issued by operator or CI agent

## Protocol Stages

1. **Segment Exchange** – Each node transmits its signed segment and nonce.
2. **Hash Verification** – All peers compute Merkle root of current window.
3. **Quorum Vote** – Quorum (≥2/3) of peer signatures required for acceptance.
4. **Reconciliation** – If quorum fails, fallback to last-good snapshot. Conflict triggers `ConsensusError`.

## Filesystem

- Consensus artifacts: `/srv/trace/consensus/<ts>.log`
- Failed reconciliations: `/srv/trace/fault/<ts>.error`
- Live consensus snapshot: `/srv/trace/current.snapshot`

## Security

- Segments are signed with node's ephemeral session key
- TLS required for segment exchange
- Faults are validated against policy in `SECURITY_POLICY.md`
