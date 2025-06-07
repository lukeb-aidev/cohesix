// CLASSIFICATION: COMMUNITY
// Filename: TRACE_CONSENSUS.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-05

# Trace Consensus Protocol

Peer queens exchange trace segments and converge on a
canonical log. Divergence triggers `ConsensusError` and
artifacts are stored under `/srv/trace/consensus/`.
