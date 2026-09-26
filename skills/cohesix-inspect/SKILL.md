---
name: cohesix-inspect
description: Inspect an existing Cohesix Queen or explore its read-only host model, including Python access. Use for identity, scheduler, lease and connection diagnosis; not target installation or repository development.
license: Apache-2.0
---
<!-- Author: Lukas Bower -->
<!-- Purpose: Keep read-only onboarding explicit about source, ownership and state limits. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Inspect a Cohesix Queen

Use this to answer “what system am I looking at, what is visible, and what is
still unknown?” It does not establish GPU execution or release acceptance.

## Select the installation and backend

On macOS or Linux, use the matching installed host bundle, or tools built for
that host from one source revision. The read-only workflow is the same on both;
use each host's own executable and Python environment.
Check the host OS, architecture, bundle version and command availability
before connecting. If a binary or Python package does not match, explain the
mismatch and point to the correct host bundle or matching Python environment;
keep target inspection read-only while setup is unresolved.
Set `COH_BIN` to its **absolute executable directory** (bundle `bin/`, or your
source build's output). Run from that installation's root. Examples use Bash;
Python needs 3.11+ and the matching installed `cohesix` package in its environment.
Check `"$COH_BIN/coh" --help` and `"$COH_BIN/cohsh" --help`. If a command is
missing, use the matching bundled guide; do not mix in a newer binary.
[Host tools](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/HOST_TOOLS.md) owns installation, credentials and topology.

For source-oriented links in this skill, use the corresponding local file at
your selected revision, or replace `main` in the URL with its commit/tag.
Absolute web links also work when this skill folder is copied out of the repo.
Do not assume `main` describes an older release.

## First useful read without a target

```bash
"$COH_BIN/cohsh" --transport mock --role queen
```

At `coh>` enter these commands individually:

```text
ls /
cat /proc/boot
cat /proc/schedule/summary
cat /proc/lease/summary
quit
```

Expect a namespace and bounded model records. Label the result **mock**, with
no live Queen contacted. Missing model paths mean unavailable, not live failure.
This tour issues no control writes. Mock is not a general sandbox: Python mock
constructors write local files, and `coh run --mock` can still execute host code.

For Python-only exploration, this snippet explicitly chooses fresh temporary
model storage, reads it, then removes only that storage:

```python
from tempfile import TemporaryDirectory
from cohesix import MockBackend

with TemporaryDirectory(prefix="cohesix-inspect-") as directory:
    backend = MockBackend(root=directory)
    print("source=host-model", backend.list_dir("/"))
    print(backend.read_file("/gpu/GPU-0/info", 1024).decode("utf-8"))
```

The mock GPU is synthetic. Do not use environment auto-selection for an
unfamiliar deployment: a stale mock or mount variable can mask the intended REST
backend. See [Python support](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/PYTHON_SUPPORT.md) for selection precedence.

## Inspect an already-configured live Queen

Obtain the gateway URL, intended target identity/profile and read credentials
from the operator. Non-public reads need the gateway request-auth token and a
delegated ticket with `Read` or `ReadWrite` scope for the requested paths and
finite quota. Do not mint a broader ticket or enable `--read-compatibility`.
Use the existing gateway: it owns the single target TCP console. Do not start a second gateway or direct TCP client, disconnect an owner,
change network settings or request elevated credentials to make inspection work.

```bash
: "${COH_REST_URL:?set the operator-supplied gateway base URL}"
: "${COH_READ_HEADERS:?set a private operator-provisioned HTTP header file}"
curl --header "@$COH_READ_HEADERS" --fail-with-body --silent --show-error --max-time 15 \
  "$COH_REST_URL/v1/meta/status"
curl --header "@$COH_READ_HEADERS" --fail-with-body --silent --show-error --max-time 15 \
  "$COH_REST_URL/v1/meta/bounds"
curl --header "@$COH_READ_HEADERS" --fail-with-body --silent --show-error --max-time 15 --get \
  --data-urlencode 'path=/proc/boot' --data-urlencode 'max_bytes=1024' \
  "$COH_REST_URL/v1/fs/cat"
```

`COH_READ_HEADERS` contains two header lines: `Authorization: Bearer TOKEN`
and `x-cohesix-ticket: TICKET`, with real operator-provisioned values, not those
placeholders. Keep this file private; do not print it or put secrets on argv.

Require `connected: true` **and a live console backend**, then compare target `/proc/boot` identity with the
operator's expected image/profile. Gateway bounds describe compiled host policy,
not target identity. Check any cache/freshness metadata; one snapshot does not
prove continuing progress. Require filesystem JSON `status: "OK"` and terminal `end: true`; HTTP 200 alone
can carry a target error. A refused or timed-out read remains unknown.
A connected mock gateway is still a model. REST reads remain bounded by both
delegated read scope and the gateway's upstream role; read credentials are not
permission to mutate. Use the operator's
secured endpoint/tunnel; do not send secrets to an unverified URL.

For CLI/Python reads, have the operator load `HIVE_GATEWAY_REQUEST_AUTH_TOKEN`
and `COH_REST_TICKET` into the protected process environment (the latter is the
actual delegated ticket for Python, not a file reference). Avoid shell tracing
and conflicting credential aliases. Open the same namespace tour:

```bash
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?load the request-auth credential securely}"
: "${COH_REST_TICKET:?load the delegated read ticket securely}"
"$COH_BIN/cohsh" --transport rest --rest-url "$COH_REST_URL" --role queen
```

Run the read commands above, then quit. `--role queen` does not elevate the
gateway's upstream authority. On source versions with `inspect`, a bounded
summary is available via `"$COH_BIN/coh" inspect --rest-url "$COH_REST_URL" --json`.

Equivalent explicit Python read (no control write):

```python
import os
from cohesix import RestBackend

backend = RestBackend(
    base_url=os.environ["COH_REST_URL"],
    request_auth_token=os.environ["HIVE_GATEWAY_REQUEST_AUTH_TOKEN"],
    delegated_ticket=os.environ["COH_REST_TICKET"],
    max_attempts=1,
)
print(backend.read_file("/proc/boot", 1024).decode("utf-8"))
```

## Diagnose and report

| Observation | Safe next step |
| --- | --- |
| Connection refused, timeout or `connected: false` | Confirm URL/tunnel and existing gateway owner with the operator; retain the error. Do not restart services or take TCP ownership automatically. |
| AUTH failure, denied/EPERM | Confirm the intended role and credential provenance through the operator. Do not print tokens, use fixture secrets or widen authority. |
| Missing path, disabled provider or mismatched manifest | Check the selected version/profile and role visibility; report unavailable or mismatch, not zero activity. |
| Busy, overload, stale state or ambiguous write reported by another tool | Capture bounded diagnostics; stop new mutations and reconcile the original operation identity. |

Return the source class (mock, recorded or live), tool/target identity, observed
state, freshness limits, exact errors and next operator action. Namespace entries
and provider declarations do not prove Worker admission, READY or completion.
The [API](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/HOST_API.md) and [interfaces](https://raw.githubusercontent.com/lukeb-aidev/cohesix/main/docs/INTERFACES.md) own schemas.
