<!-- Author: Lukas Bower -->
<!-- Purpose: Define the bounded M27 operator projections and their evidence authority. -->
<!-- Copyright 2026 Lukas Bower -->
# Operator inspection and evidence

Milestone 27 adds host commands over the existing console, gateway projection,
and canonical evidence-pack layout. These commands collect observations and
explain differences; they grant no authority and do not repair target state.
Missing optional facilities remain missing or unknown. They never become a
health, execution, hardware, or release verdict.

## Commands

Credentials use the existing host-tool environment resolution. When a gateway
owns the target console, use its REST URL rather than opening a second console.
For example, with that gateway already running:

```sh
coh inspect --rest-url http://127.0.0.1:8080 --json
coh evidence pack --rest-url http://127.0.0.1:8080 --out out/incident
coh inspect --input out/incident --json
coh diff --left out/before --right out/incident
coh diff --left out/incident --right http://127.0.0.1:8080
coh attest --input out/incident --json
coh evidence timeline --input out/incident --scenario incident
```

`inspect` accepts existing connection options or `--input DIR` for offline
inspection. It lists lifecycle, root, session, pressure, attestation, scheduling,
and lease roots and reads bounded children, boot, spool status, and policy rules.
The output orders source paths and records `observed`, `missing`, `error`, or
`unknown`, sanitized content, and stable reasons. Inventory contradictions,
unreadable captured files, malformed bounded inputs, and impossible known field
values produce a nonzero exit. Optional absence alone does not. JSON and text
carry the same observations, without a healthy/unhealthy label.

The bounded `/proc` listing establishes which optional roots are advertised;
an absent root is recorded as missing without issuing an invalid file read.
A failed listing or a failed read of an advertised node remains an error.
`/proc/lease/by-id` is a directory, including when it contains no active leases.

`diff --left SOURCE --right SOURCE` accepts directories (optionally `pack:`),
`tcp://host:port`, or HTTP(S) gateway URLs. It emits ordered exact field changes
as JSON; missing values remain absent and object keys use JSON-pointer escaping.
Collector metadata remains labeled as host metadata, separate from observed
target files. Differences themselves exit zero; invalid sources exit nonzero.

`bundle` is an alias for `evidence pack` with identical arguments and layout.
Optional `--manifest FILE`, `--resolved-manifest FILE`, `--serial-log FILE`, and
`--trace FILE` add sanitized host attachments. Manifest TOML is retained as JSON;
its original SHA-256 and retained-byte SHA-256 are separate. Trace attachments
must be validated version-2 canonical captures. `artifact_refs.json` records
byte counts, source classes, hashes, and `proof=none`. The pack adds an
`attestation.json` result, a sorted `checksums.json`, and `pack.sha256`; the CLI
prints the final pack digest. This digest detects byte changes when compared
with a trusted copy; it is not a signature. Captured inventory files and
attachments are never inferred from arbitrary neighboring files.
Directory listings are inventory-addressed leaves at
`namespace/<source-path>/.listing`, so parent and child directories can both
be retained. Readers continue to accept older inventory-selected listing paths.

Inputs reuse compiler-owned diagnostic limits: currently 1 MiB per aggregate
inspection and attachment input, with 682 inventory entries derived from the
directory and path limits. Each file must be regular and bounded; traversal,
control characters, symlink components, duplicate namespace paths, and excessive
depth are rejected. Output files use unique same-directory temporary files and
atomic replacement. A failed filesystem operation can leave a subset of a
multi-file export; consumers must validate its inventory. Legacy pack-v1 and
timeline-v1 inputs remain readable, with absent inventories classified unknown.

## Canonical traces

The existing `cohsh-core` trace container has an additive version-2 capture
header; the original version-1 Secure9P fixtures remain byte-identical.
Version 2 retains redacted shell output and read observations in the same
bounded frame/ACK container. It is consumed by `cohsh`, `coh trace`,
`coh-status::captured_namespace`, and SwarmUI's trace replay backend.

```sh
cohsh --transport rest --rest-url http://127.0.0.1:8080 --role queen \
  --record-trace out/session.trace \
  --trace-target-id expected-target --trace-session-id expected-boot
coh trace --input out/session.trace
cohsh --replay-trace out/session.trace
```

Use an appropriate read-only script for incident collection. Recording does
not change the script's behavior or block its writes. TCP uses the same recorder
and existing `--transport tcp --tcp-host HOST --tcp-port PORT` options. Live
capture works without the `in-process` feature; version-1 fixture execution
retains that feature requirement. Replay opens no target transport. It exposes
only retained reads; unavailable reads fail and every write is rejected.
SwarmUI reads those observations through its normal console parsers without
network access. It cannot reconstruct observations that were never captured.

The header binds backend, caller-supplied target/session label hashes, optional
`--trace-manifest-sha256` and `--trace-image-sha256`, capture time, completion,
and generated policy/redaction digest. Zero identity hashes mean unknown.
These are expected identities supplied by the collector, not device-attested
identities. Classify actual QEMU, Pi, and host-projection captures using external
exact-target provenance; transport choice alone never proves target class.
Version-1 traces and checked-in samples are offline fixtures only.

Generated schema 1.19 adds `client_policies.trace.max_duration_ms` (default
60000). Byte, frame, ACK, and duration limits stop recording without altering
the underlying session. Completion values are 0 complete, 1 partial/error,
2 byte/frame limit, and 3 duration limit; incomplete CLI recordings/replays
exit nonzero. A partial line is not committed. The digest covers metadata and
all retained bytes. Replay rejects malformed counts, tamper, wrong policy,
oversize input, and records that do not satisfy the shared redaction rules.

Authentication exchanges and raw write payloads are omitted. Secret-bearing
JSON fields (including nested JSON strings) are redacted and tickets are hashed;
unstructured payloads without a supported field contract are withheld. Known
connection credentials are also removed before persistence. ACK/ERR/END order
is exact **after redaction**. Host fixture and loopback transport tests do not
satisfy fresh QEMU or Pi acceptance.

## Case summaries

`evidence timeline --scenario generic|incident|change|maintenance|rollout|federation`
adds `case.json` and `case.md` alongside the existing timeline files. Scenario
changes the review framing only. Records correlate by the full request,
idempotency, source-hive, and target-hive tuple; incomplete identities remain
separate instead of guessing a join.

Each chain covers request, decision, state/lease/lifecycle, host result,
receipt/dead-letter, and federation relay stages. A stage is `observed`,
`missing`, `error`, `unknown`, or `ambiguous`. Source links include the original
namespace path, timeline event index, sequence, and SHA-256 of that canonical
event. Conflicting terminal records remain ambiguous. Outcomes are limited to
`recorded-terminal`, `refused`, `deadlettered`, `incomplete`, and `ambiguous`.
An observed host result does not establish execution, and a missing linked
decision is not synthesized from a later status. Authoritative receipt and
execution verification remain Milestone 27b.

Python `CohesixClient.evidence_case(pack_dir)` and
`CohesixClient.attestation_result(pack_dir)` read these canonical records
offline. The shared operator fixture binds Rust output and Python source-link
validation. Unknown additive fields are sanitized, absent optional records
raise `CohesixError`, and malformed required structure is rejected. Python retains
traces as opaque bounded hash references and never parses or replays them.

## Signed attestation

`coh attest --trust-policy <file>` uses the shared TPM2 quote and certificate
verifier. Live mode submits one fresh authenticated ephemeral challenge only
when the target advertises an issuer. `--record <file>` retains its public
request and signed response; `coh evidence pack --attestation-record <file>`
adds it to the canonical pack. `coh attest --input <pack> --trust-policy <file>`
verifies it offline with an explicit historical-signature scope.

Results include PASS/FAIL/UNAVAILABLE, source and evidence class, proof scope,
and verified evidence/policy/nonce digests. A PASS for an offline fixture does
not establish hardware identity or fresh target state. The selected Pi profile
reports measurement-only under the owner's stock-Pi exemption; selected images
without a device provider advertise unavailable and receive no challenge.
The [signed device contract](ATTESTATION.md) defines exact bounds, algorithms,
nonce and PCR binding, enrollment, revocation and remaining device-runtime work.
