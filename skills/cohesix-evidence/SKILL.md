---
name: cohesix-evidence
description: Inspect, capture, compare and verify Cohesix evidence for an incident or operation outcome. Use for recorded packs, partial exports and signed causal graphs; do not treat mock reports or historical acceptance as a new live result.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Preserve provenance and distinguish diagnostic captures from signed outcome proof. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Review Cohesix evidence

Answer what happened, which source establishes it, and what remains unknown.
Prefer an existing recorded pack before contacting a live system. Capture and
analysis are separate from acceptance under the [Test Plan](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/TEST_PLAN.md).

## Select the source and tools

Set `COH_BIN` to the absolute executable directory of one matching installation
and run from its root. Identify its version/manifest and command help before
using these source-oriented examples. `inspect` and signed verification may be
absent from older bundles; use their bundled documentation rather than swapping
binaries. Use same-revision local references or pin `main` URLs below to the
selected source commit/tag. Web links work outside the source checkout.

For an operator-supplied recorded pack (no target or credential needed):

```bash
: "${PACK:?set the existing evidence-pack directory}"
"$COH_BIN/coh" inspect --input "$PACK" --json
```

Inspect `summary.json` for captured, missing and errored entries before drawing
conclusions. Keep mock/model, source declaration, target observation and signed
proof labels. Do not fill a missing observation with a default or extrapolate
beyond a bounded log's retention.

If no pack exists, an explicitly labelled mock exercise creates local files:

```bash
ADOPTION_TMP=$(mktemp -d)
"$COH_BIN/coh" evidence pack --mock --out "$ADOPTION_TMP/mock-pack"
"$COH_BIN/coh" inspect --input "$ADOPTION_TMP/mock-pack" --json
```

Check the exporter exit status before proceeding. This creates a model pack,
not target evidence. It does not run a GPU job. Retain the temporary path for
review; delete only that newly created directory when it is no longer needed.

## Capture an already-configured Queen

Obtain the expected target identity, existing gateway URL and scoped read
credentials. For current non-public REST reads, the operator securely loads
`HIVE_GATEWAY_REQUEST_AUTH_TOKEN` and `COH_REST_TICKET` with the necessary
`Read`/`ReadWrite` paths and finite quota. Do not widen scope to complete a pack. Verify connection/profile first using [host tools](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/HOST_TOOLS.md).
The gateway remains the sole TCP console owner. Do not start another owner or
reconfigure a live system to obtain a pack. Capture performs target reads and
writes a bounded directory locally:

```bash
: "${COH_REST_URL:?set the existing gateway base URL}"
: "${PACK_OUT:?set a new private output directory}"
"$COH_BIN/coh" evidence pack --rest-url "$COH_REST_URL" --out "$PACK_OUT"
```

A nonzero exporter result can leave a useful partial `summary.json`. Preserve it
and report the failed reads; do not overwrite it or label it complete. Missing
optional paths and failed optional reads differ. Record capture time, tools,
source/image/profile and target identity. Redaction is bounded: review before
sharing and never attach private keys or secret-bearing configuration.

## Derive a timeline or compare observations

Timeline generation writes `timeline.ndjson`, `timeline.md`, `case.json` and
`case.md` **inside its input directory**. For a sealed/original pack, first copy
it to a new private working directory and set `PACK_WORK` to that copy. Derived
case summaries are explanations, not newly signed receipts.

```bash
: "${PACK_WORK:?set a writable working copy of the pack}"
"$COH_BIN/coh" evidence timeline --input "$PACK_WORK"
```

For two existing packs, this comparison is read-only:

```bash
: "${BEFORE_PACK:?set the earlier evidence directory}"
: "${AFTER_PACK:?set the later evidence directory}"
"$COH_BIN/coh" diff --left "$BEFORE_PACK" --right "$AFTER_PACK"
```

Identify differences in image/profile/authority as well as values. An unchanged
capture is not a liveness proof. A partial pack can explain failure without
establishing that an operation never ran. [Host tools](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/HOST_TOOLS.md)
and [operator recipes](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/OPERATOR_RECIPES.md) own the capture format.

## Verify an operation's signed outcome

A diagnostic pack is not itself a signed causal graph. Obtain the producer's
graph, retained CAS objects and independently enrolled public trust policy
outside the pack. Do not trust keys merely because they arrived with evidence,
create replacement signatures or weaken expected identity/time bindings.

```bash
: "${GRAPH:?set the producer graph JSON path}"
: "${TRUST:?set the independently enrolled trust policy path}"
: "${CAS:?set the retained content-addressed object directory}"
"$COH_BIN/coh" evidence verify --input "$GRAPH" --trust "$TRUST" --cas "$CAS"
```

Require exact subject/ticket/action/epoch, manifest/components, causal phases,
artifact sizes/hashes and custody. Inspect terminal outcome: a verified failed
or cancelled action is not the requested successful workload. Historical
verification uses an explicit recorded time; it does not renew present authority.
Missing/expired trust, corrupt CAS, mismatched binding or incomplete phases mean
verification failure or unknown outcome. Preserve the error and route to the
operator/custodian; do not rerun the workload to obtain a cleaner report.

[Causal evidence](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/CAUSAL_EVIDENCE.md) explains gateway/native/Worker
custody. A signed gateway observation can still have `device_attested=false`;
native completion without the required Worker witness is not Worker proof.
Return proven facts, remaining uncertainty, exact evidence references and safe
next steps. Keep target acceptance, full release acceptance and newly measured
performance distinct from these focused checks.
