<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document Cohesix test fixtures, hashes, and convergence guardrails. -->
<!-- Author: Lukas Bower -->

# Test Plan

## Manifest fingerprints
- `out/manifests/root_task_resolved.json` — `sha256:6b2cb0b8a80b57f75acb5049fe17f2aae92e0d7adf690b34fc30eb01b084710c`

## Convergence harness (Milestone 20e)
- Script: `scripts/regression/transcript_compare.sh`.
- CI job definition: `scripts/ci/convergence_tests.sh`.
- Timing tolerance: 50 ms (test harness tolerance; not a protocol contract).
- Fixture normalization: transcripts include only `OK ...`, `ERR ...`, and `END` lines; `OK/ERR AUTH` lines are excluded.
- The converge sequence runs `help -> attach -> log -> spawn -> tail -> quit` with `spawn` executed via `/queen/ctl` echo payloads.

## Transcript fixture hashes
- `tests/fixtures/transcripts/boot_v0/serial.txt` — `sha256:f2d228dc21c98ee36acaf1d18370b3fc758407d97b7062d8c0e99036fe783a08`
- `tests/fixtures/transcripts/boot_v0/core.txt` — `sha256:f2d228dc21c98ee36acaf1d18370b3fc758407d97b7062d8c0e99036fe783a08`
- `tests/fixtures/transcripts/boot_v0/tcp.txt` — `sha256:f2d228dc21c98ee36acaf1d18370b3fc758407d97b7062d8c0e99036fe783a08`
- `tests/fixtures/transcripts/abuse/serial.txt` — `sha256:8b674462606ff7d0d324d7678d8d3700583611296f83e32af1a041790e84b6c8`
- `tests/fixtures/transcripts/abuse/core.txt` — `sha256:8b674462606ff7d0d324d7678d8d3700583611296f83e32af1a041790e84b6c8`
- `tests/fixtures/transcripts/abuse/tcp.txt` — `sha256:8b674462606ff7d0d324d7678d8d3700583611296f83e32af1a041790e84b6c8`
- `tests/fixtures/transcripts/converge_v0/serial.txt` — `sha256:88f1e92d403e4a041ab3a6bf3af40faae0c81a088d4ee3e4270584b9f6002b7e`
- `tests/fixtures/transcripts/converge_v0/core.txt` — `sha256:88f1e92d403e4a041ab3a6bf3af40faae0c81a088d4ee3e4270584b9f6002b7e`
- `tests/fixtures/transcripts/converge_v0/tcp.txt` — `sha256:88f1e92d403e4a041ab3a6bf3af40faae0c81a088d4ee3e4270584b9f6002b7e`
- `tests/fixtures/transcripts/converge_v0/cohsh.txt` — `sha256:88f1e92d403e4a041ab3a6bf3af40faae0c81a088d4ee3e4270584b9f6002b7e`
- `tests/fixtures/transcripts/converge_v0/swarmui.txt` — `sha256:88f1e92d403e4a041ab3a6bf3af40faae0c81a088d4ee3e4270584b9f6002b7e`
- `tests/fixtures/transcripts/converge_v0/coh-status.txt` — `sha256:88f1e92d403e4a041ab3a6bf3af40faae0c81a088d4ee3e4270584b9f6002b7e`

## Guard
- `scripts/ci/check_test_plan.sh` verifies hashes above match on-disk fixtures and manifest fingerprints; `scripts/check-generated.sh` invokes it.
