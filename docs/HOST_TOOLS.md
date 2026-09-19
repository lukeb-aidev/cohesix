<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Explain how to install, connect, operate and troubleshoot the Cohesix host-tool suite without widening target authority. -->
<!-- Author: Lukas Bower -->

# Cohesix host tools — user guide

Use this guide to operate a Queen from a Mac or Linux host, share its connection
between tools, publish host observations, run authorised host work, and retain
results for later review. The Queen runs Cohesix; **the tools in this guide run
on your host, not at the Queen's serial prompt**. CUDA, model files, containers,
REST and the desktop UI remain host-side.

Start with [Quickstart](QUICKSTART.md) to install a matching host bundle and
boot a target. Then follow [Connect and verify](#connect-and-verify). For normal
work involving more than one tool, use a gateway rather than opening competing
TCP sessions.

This guide describes the current source interfaces. An older release can have
different options, credentials or authority policy. Keep its binaries and
configuration together, check its `--help`, and read the
[M27a migration notes](M27A_AUTHORITY.md#release-migration-and-historical-errata)
before using historical deployment examples. Published fixture credentials are
not deployment credentials.

## Find the right tool

| Your job | Tool or workflow | Where the effect occurs |
| --- | --- | --- |
| Browse the Queen, issue commands or run a checked script | [`cohsh`](#cohsh) | Target, through TCP or REST |
| Share one Queen between a shell, UI and automation | [`hive-gateway`](#hive-gateway) | Host service owning the target connection |
| Inspect state, compare captures or export evidence | [`coh`](#coh), [evidence workflow](#capture-and-review-evidence) | Target reads and local files |
| Use existing file-oriented software | [`coh mount`](#mount-a-namespace) | Host FUSE mount of permitted target paths |
| Discover a real GPU and publish its inventory | [`gpu-bridge-host`](#gpu-bridge-host) | Discovery on the GPU host; snapshot on the Queen |
| Run a local program after checking a GPU lease | [`coh run`](#run-a-host-program) | The host where `coh` is running |
| Export a LoRA job and manage an adapter registry | [`coh peft`](#manage-peft-adapters) | Export reads, host registry and GPU projection |
| Publish Docker, systemd, Kubernetes or other host observations | [`host-sidecar-bridge`](#host-sidecar-bridge) | Observations of the publishing host |
| Execute admitted, allowlisted host actions | [`host-ticket-agent`](#host-ticket-agent) | The host running the agent |
| Package and upload a small CAS update | [`cas-tool`](#cas-tool) | Local bundle, then target update store |
| View the hive and use a graphical operator console | [SwarmUI](#swarmui) | Host desktop, using the same target contracts |
| Integrate Cohesix into a Python application | [Python package](#cohesix-python-package) | Explicit REST, TCP, filesystem or mock backend |

The eight operator executables are `cohsh`, `coh`, `hive-gateway`,
`gpu-bridge-host`, `host-sidecar-bridge`, `host-ticket-agent`, `cas-tool` and
`swarmui`. Shared crates such as `coh-status`, `sidecar-bus` and
`console-ack-wire` support the suite; you do not need to start additional
services for them.

**In this guide:** [Setup](#set-up-your-host) ·
[Connection ownership](#choose-one-live-topology) ·
[First connection](#connect-and-verify) ·
[Write credentials](#authentication-layers) ·
[Tool reference](#tool-catalog) ·
[Troubleshooting](#troubleshooting) ·
[Shutdown](#shut-down-without-losing-state) ·
[Release maintainer appendix](#release-factory)

## Set up your host

### Use one matching installation

For a release installation, extract the Mac or Linux host archive into its own
directory and follow the bundle's `QUICKSTART.md`. The Pi image archive does
not contain host executables: operating a Pi also requires the appropriate
Mac or Linux host bundle. From the **host bundle root**, prepare and check the
runtime:

```bash
./scripts/setup_environment.sh
./scripts/setup_environment.sh --check
source .venv/bin/activate
export COH_BIN="$PWD/bin"
```

The first command can install host packages. The second checks an existing
installation. Neither installs an NVIDIA driver or builds a Queen image.
Verify the archive's `MANIFEST.sha256` as described in Quickstart before
running its contents.

For an already built source checkout, work from the **repository root**:

```bash
source "$HOME/.cargo/env"
source .venv/bin/activate
export COH_BIN="$PWD/out/cohesix/host-tools"
```

When those binaries do not yet exist, follow
[Build from source](QUICKSTART.md#build-from-source). Source developers can
substitute `cargo run -p TOOL --` for `"$COH_BIN/TOOL"`. FUSE and GPU backends
are build features: a minimal Cargo build is not necessarily equivalent to a
native release build. Use the selected manifest's generated configuration,
not policy copied from a different target or release.

The examples below use **Bash**. In each new terminal, return to the same
installation root and set `COH_BIN` and the relevant connection variables.
Variables defined in one terminal are not automatically available in another.
`COH_BIN` and `COH_TARGET_HOST` below are conveniences for these examples, not
settings automatically understood by every executable.

### Check the local tools

```bash
"$COH_BIN/coh" --help
"$COH_BIN/cohsh" --help
"$COH_BIN/hive-gateway" --help
"$COH_BIN/coh" doctor
```

`coh doctor` checks local policy/ticket, mount, GPU and runtime prerequisites.
**It is not a target connection test:** its implementation does not use the
connection options to probe the Queen. A missing optional GPU or FUSE
prerequisite is different from a failed TCP connection; read the individual
check results.

For a deliberately simulated local check:

```bash
"$COH_BIN/coh" doctor --mock
"$COH_BIN/cohsh" --transport mock --role queen
```

At `coh>`, type `help`, `ls /`, then `quit`. Mock state is not target evidence.
Rust in-process mocks are separate for each process; one mock executable does
not populate another. Python's filesystem-backed mock can retain state at its
chosen directory. Also, **mock does not mean every tool is free of host side
effects**: `coh run --mock`, for example, can still launch the supplied local
program.

## Choose one live topology

The Queen's authenticated TCP console accepts **one client**. Choose one of
these alternatives:

```text
Direct:   one foreground tool ----------------------> Queen TCP console

Shared:   cohsh / coh / SwarmUI / Python / bridges
                         |
                         +---- REST ---> hive-gateway ---> Queen TCP console
```

| Mode | Console owner | How to use it safely |
| --- | --- | --- |
| Direct TCP | One shell, UI, mount or bridge | Finish that process before opening another direct client. |
| Shared gateway | `hive-gateway` | Point every other live tool at its REST URL. This is the normal multi-tool setup. |
| Mounted filesystem | The `coh mount` process uses TCP or REST | Keep the mount process running; filesystem clients inherit its access. |
| Mock or offline replay | No live target | Keep the output labelled as model or historical data. |

A direct publisher with `--watch` or `--interval-ms` keeps its connection.
Starting a direct shell beside it is not concurrency; it is contention for
the only console. A gateway does not add an in-target listener or a second
control authority. The target's serial/local-seat console remains a separate
operator surface; its guide is [Userland and CLI](USERLAND_AND_CLI.md).

## Connect and verify

### 1. Select the actual target

For a Pi, use the address verified at that Pi's current console. This is an
example address, not automatic discovery:

```bash
export COH_TARGET_HOST="192.168.10.50"
export COH_TARGET_PORT="31337"
export COH_WORKER_PROFILE="pi4-production"
export COH_REST_URL="http://127.0.0.1:8080"
```

For a locally running QEMU Queen, use these values instead:

```bash
export COH_TARGET_HOST="127.0.0.1"
export COH_TARGET_PORT="31337"
export COH_WORKER_PROFILE="qemu-smp-production"
export COH_REST_URL="http://127.0.0.1:8080"
```

Use the forwarded port selected by the QEMU launcher if it differs.
`--worker-runtime-profile` selects the gateway's generated Worker bounds; it
does not detect or qualify the target. Select `pi4-production` explicitly for
Pi even when the gateway itself runs on a Mac.

### 2. Start the shared gateway

First quit any direct TCP client. On the gateway host, obtain the Queen console
credential and a **different** REST request-authentication token from the
operator responsible for this deployment. For write-capable operation, also
configure the delegated-ticket issuer reference. Do not put secret values in
source files or shell command arguments.

In the gateway terminal:

```bash
: "${COH_AUTH_TOKEN:?load the console credential for this target}"
: "${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:?load the gateway request-auth token}"
: "${HIVE_GATEWAY_DELEGATION_KEY_REF:?set the provisioned env: or file: issuer reference}"
export COH_AUTH_TOKEN HIVE_GATEWAY_REQUEST_AUTH_TOKEN HIVE_GATEWAY_DELEGATION_KEY_REF

"$COH_BIN/hive-gateway" \
  --bind 127.0.0.1:8080 \
  --tcp-host "$COH_TARGET_HOST" \
  --tcp-port "$COH_TARGET_PORT" \
  --worker-runtime-profile "$COH_WORKER_PROFILE" \
  --delegation-key-ref "$HIVE_GATEWAY_DELEGATION_KEY_REF"
```

Leave this process running. Its upstream role defaults to `queen`; use its
`--role` and `--ticket` only for the deployment's intended upstream attachment.
A narrower upstream ticket also narrows every downstream client.

Keep the bind on loopback. Neither the console nor the gateway supplies TLS.
For clients on another machine, use an approved authenticated tunnel, VPN or
TLS-terminating proxy. A remote Jetson's `127.0.0.1` refers to that Jetson, not
to your Mac. Give it a URL reachable through the secured connection. Do not
solve reachability by exposing an unauthenticated network boundary;
`--allow-non-loopback-bind` is an explicit exposure opt-in, not encryption.

### 3. Verify the gateway and the Queen separately

In another terminal, set `COH_BIN` and `COH_REST_URL` as above. Obtain a
private operator-provisioned header file containing `Authorization: Bearer TOKEN`
and `x-cohesix-ticket: TICKET`, with the actual request credential and delegated
read ticket for these paths. Set `COH_READ_HEADERS` to its absolute path; keep
its contents private and never enable a compatibility bypass just to read.
Then run:

```bash
: "${COH_READ_HEADERS:?set the private read-credential header file}"
curl --header "@$COH_READ_HEADERS" --fail-with-body --silent --show-error \
  "$COH_REST_URL/v1/meta/status"

curl --header "@$COH_READ_HEADERS" --fail-with-body --silent --show-error \
  "$COH_REST_URL/v1/meta/bounds"

curl --header "@$COH_READ_HEADERS" --fail-with-body --silent --show-error --get \
  --data-urlencode 'path=/proc/boot' \
  --data-urlencode 'max_bytes=1024' \
  "$COH_REST_URL/v1/fs/cat"
```

Require `connected: true`, the expected backend/profile and a successful
`/proc/boot` read identifying the intended Queen. `/v1/meta/bounds` describes
**compiled host policy**; `/proc/boot` is **target-reported identity**. Compare
both with the selected deployment rather than treating either one alone as
proof of parity. A connected mock gateway is still a model.

Non-public reads require request authentication and delegated `Read` or
`ReadWrite` scope, as described under [Delegated namespace reads](#delegated-namespace-reads).
They remain bounded by the gateway's upstream target authority. Public read
classification is generated; a request-auth token alone is not a delegated read
grant. Protect the gateway endpoint and provision scoped credentials before
using the read examples.

### 4. Do a first useful read

```bash
"$COH_BIN/cohsh" --transport rest --rest-url "$COH_REST_URL" --role queen
```

At the **`coh>` prompt**, enter only the following commands, without a prompt
prefix:

```text
ls /
cat /proc/boot
cat /proc/root/reachable
cat /proc/schedule/summary
cat /proc/lease/summary
tail /log/queen.log 32
quit
```

A successful read means that operation completed. Inspect the returned state;
`OK` does not mean every service is healthy. The gateway stays running after
`quit`, ready for the UI, another shell or an evidence capture.

## Authentication layers

There are several independent credentials and checks. Confusing them is a
common cause of “connected, but writes fail”.

| Layer | What you supply | What it permits |
| --- | --- | --- |
| Console authentication | This Queen's console credential, commonly `COH_AUTH_TOKEN` | Opening the authenticated TCP console |
| Upstream attachment | Gateway/direct-tool role and, when required, capability ticket | Target namespace identity, scope and budget |
| REST request authentication | Gateway token; clients commonly use `COH_REST_AUTH_TOKEN` or `HIVE_GATEWAY_REQUEST_AUTH_TOKEN` | Calling the host HTTP mutation boundary |
| REST caller delegation | Signed `COH_REST_TICKET`, sent as `x-cohesix-ticket` | Finite caller-specific write scopes and quotas beneath the upstream ceiling |
| Target policy | Enabled paths, approvals, writer epoch and current lifecycle state | Whether the concrete operation is allowed now |
| Host execution request | A `host-ticket/v1` or other supported action record | Asking the agent for a particular allowlisted host action; this is not a REST credential |

Every REST mutation, including a batch, needs **both request authentication
and a delegated caller ticket**. A console password, `--role queen`, or the
REST token alone is insufficient. The gateway verifies the caller ticket;
the VM still sees the gateway's upstream principal. This is gateway-enforced
caller identity, not VM-verified individual caller identity.

### Delegated namespace reads

non-public REST reads, including gateway status, require
request authentication and a delegated `Read` or `ReadWrite` scope. Mint with
`cohsh --mint-ticket --role queen --ticket-subject <subject> --ticket-read-scope <prefix>`
and an existing protected signing-key reference. Combining read and write on
the same prefix creates `ReadWrite`; both consume one shared finite quota.
An explicitly issued Queen read scope `/` is administrative authority.
`--read-compatibility` permits gateway-Queen reads of admin-class paths using
request auth in a single-caller installation. Ticket-scoped paths still require
delegation. The default enables no compatibility bypass. Rust and Python REST
clients, REST FUSE and SwarmUI use the same delegated read header. Native direct
console/FUSE transports retain their own authenticated session authority.

### Prepare a writing client

Obtain a signed ticket for this client's job, with the needed path scope,
subject, lifetime and operation budget. In the client terminal:

```bash
: "${COH_REST_AUTH_TOKEN:?load the request-auth token for this gateway}"
: "${COH_REST_TICKET:?load the signed delegated ticket for this job}"
export COH_REST_AUTH_TOKEN COH_REST_TICKET
```

Use a clean, deployment-specific environment. Different tools give different
precedence to the request-token aliases; an old
`HIVE_GATEWAY_REQUEST_AUTH_TOKEN` or `COHSH_REST_AUTH_TOKEN` can override the
intended value. Do not leave conflicting aliases set.

Start the writing shell from that environment:

```bash
"$COH_BIN/cohsh" --transport rest --rest-url "$COH_REST_URL" --role queen
```

REST attachment reads `COH_REST_TICKET`; an explicit `--ticket` overrides that
binding, but exposes a bearer credential in process arguments. Prefer the
environment on a private operator host. Changing the shell's REST attachment
replaces its caller binding, not the gateway's upstream role. Current REST
shell and `coh` transports accept only the local `queen` role; that local label
does not widen the gateway's upstream authority.

### Enrolled external identities

Use `coh identity --mapping ID --jwks FILE` (JWT on stdin) and
`coh identity --mapping ID --local` (kernel effective uid). Both normally render a
verified subject's finite ticket request. An enrolled issuer may add
`--issuer-key-ref env:NAME` or `file:/absolute/path` to issue a gateway-only
ticket. The authenticated gateway exchange provides the remote JWT path. Enrollment is
compiler-owned under `providers.identity_mappings`; defaults are disabled.
See [identity enrollment](IDENTITY_MAPPING.md) and
[SIEM boundaries](SECURITY.md) before enabling a deployment.

### Issue a narrow ticket — issuer administrators only

A publisher needs its signed ticket, **not the issuer's signing key**. On a
trusted issuer host with the provisioned key source available, this example
creates a ticket only for GPU snapshot publication:

```bash
: "${HIVE_GATEWAY_DELEGATION_KEY_REF:?set the provisioned issuer reference}"
COH_REST_TICKET=$("$COH_BIN/cohsh" --mint-ticket --role queen \
  --ticket-secret "$HIVE_GATEWAY_DELEGATION_KEY_REF" \
  --ticket-subject gpu-publisher \
  --ticket-write-scope /gpu/bridge/ctl \
  --ticket-ttl-s 300 --ticket-ops 128) || exit 1
export COH_REST_TICKET
```

The key must match the gateway's delegated-ticket verifier. `env:NAME` reads a
provisioned environment variable; `file:/absolute/path` reads a provisioned
file. The `--ticket-secret` argument above contains a reference, not key bytes.
A missing or invalid selected source fails rather than falling back. Transfer
the resulting ticket through the deployment's secret-management channel.

This ticket cannot authorise a host action, CAS upload or arbitrary Queen
command. Issue separate scopes for separate jobs. `--ticket-write-scope` takes
one prefix; do not assume it is a repeatable option. TTL and operation counts
must fit the selected policy and the planned work, including each record in a
multi-record publication. Long-running clients need an explicit renewal and
restart procedure; a short-lived ticket does not renew itself.

### Production Queen commands

Production authority profiles disable `/queen/ctl` and the legacy `spawn` and
`kill` shortcuts. They use `/queen/intents/ctl` with a bounded
`queen-intent/v1` envelope, immutable retry identity, current timestamp and the
selected `writer_epoch`. Its `cmd` field is a **JSON string containing the
Queen command**, not a nested command object. Required policy approvals remain
separate.

The current `coh gpu lease` helper still targets the legacy `/queen/ctl` path.
It is therefore a **compatibility-profile helper, not a production strict-intent
client**. Do not disable production policy to make an example work. Use the
strict-intent contract and the corresponding shell/Python interfaces in
[M27a authority](M27A_AUTHORITY.md#strict-queen-commands),
[Userland and CLI](USERLAND_AND_CLI.md) and [Python support](PYTHON_SUPPORT.md).
A rejected legacy write is not evidence of a broken GPU connection.

## Tool catalog

Choose a tool below for its workflows, options and result checks. Commands use
the installation and gateway selected above unless marked local or offline.

### `cohsh`

Use `cohsh` for interactive namespace work, checked `.coh` scripts, ticket
minting and trace capture/replay. Full console grammar belongs in
[Userland and CLI](USERLAND_AND_CLI.md); host invocation is shown here.

| Invocation option | Use |
| --- | --- |
| `--transport tcp --tcp-host HOST --tcp-port PORT` | Own one live TCP session directly |
| `--transport rest --rest-url URL` | Share the gateway |
| `--role queen` | Attach on startup |
| `--script FILE` | Execute a `.coh` script and exit |
| `--check FILE` | Validate script syntax without executing it |
| `--policy FILE` | Select a generated shell policy |
| `--record-trace FILE` / `--replay-trace FILE` | Capture a bounded trace or replay one offline |
| `--transport mock` | Use this process's in-memory model |
| `--transport qemu` | Use the shell's QEMU-launch transport; not an alias for connecting to an already running TCP Queen |

For a direct session, first stop the gateway and every other direct owner:

```bash
: "${COH_AUTH_TOKEN:?load the console credential for this Queen}"
export COH_AUTH_TOKEN
"$COH_BIN/cohsh" --transport tcp \
  --tcp-host "$COH_TARGET_HOST" --tcp-port "$COH_TARGET_PORT" --role queen
```

For repeatable work, create a small script in your local output directory:

```bash
mkdir -p out/operator
cat > out/operator/health.coh <<'COH'
# Author: Lukas Bower
# Purpose: Read a bounded Queen health baseline without mutation.
# Copyright 2026 Lukas Bower
ping
EXPECT OK
cat /proc/boot
EXPECT OK
cat /proc/root/reachable
EXPECT OK
cat /proc/schedule/summary
EXPECT OK
tail /log/queen.log 16
EXPECT OK
quit
COH

"$COH_BIN/cohsh" --check out/operator/health.coh &&
"$COH_BIN/cohsh" --transport rest --rest-url "$COH_REST_URL" \
  --role queen --script out/operator/health.coh > out/operator/health.txt
```

Check the exit status and transcript. Script failures identify the source line
and recent responses. These assertions test successful reads, not the meaning
of every health field. Add explicit assertions for your deployment's required
state using the documented `.coh` grammar. `.coh` is not Bash: it has no shell
variables, loops or command substitution. Never paste Bash assignments into
`coh>` or copy the visible prompt into a command.

The default Queen-log tail is the newest 64 records; an explicit tail accepts
1–256 lines. Use an evidence pack for the complete retained log rather than
mistaking a short tail for a full capture. Native clients reassemble bounded
chunked `CAT` records; applications should use those clients rather than
implementing ad hoc line joining.

For incident tracing, see [Operator evidence](OPERATOR_EVIDENCE.md). Live
capture accepts TCP or REST and explicit target/session identity fields;
caller-supplied labels are not independently verified identity. `coh trace
--input FILE` validates and summarises a saved trace without opening a target.

#### Read the installed manual

At `coh>`, use `man`, `man ls`, or `man spawn`. The manual covers complete
arguments, namespace layout, production authority, examples and recovery.
From the host terminal, `cohsh --man spawn` prints the same page and exits
without opening a transport; `cohsh --man` lists topics. Use the host pager
if desired: `cohsh --man spawn | less`.

### `coh`

`coh` combines local diagnostics, target inspection, evidence export, FUSE,
GPU lease helpers, host-command receipts and PEFT registry operations.

Place its top-level `--policy`, `--role` and `--ticket` options **before** the
subcommand. Connection options belong to the operation:

```bash
: "${COH_POLICY:?set the selected generated coh policy file}"
"$COH_BIN/coh" --policy "$COH_POLICY" inspect --rest-url "$COH_REST_URL"
```

Use the installation default when no explicit policy selection is needed. `coh` has no `--transport` option:
`--rest-url` selects REST; without a configured REST URL, applicable live
operations use `--host` and `--port`. `--mock` selects the model. Check inherited
`COH_REST_URL` settings before intending to use direct TCP.

| Command | Example arguments after the command | Result |
| --- | --- | --- |
| `doctor` | `--mock` for a simulated check | Local prerequisite report, not connection health |
| `inspect` | `--rest-url URL --json` or `--input PACK --json` | Source-labelled live or offline state |
| `diff` | `--left BEFORE --right AFTER` | Comparison of packs or explicit `tcp://` / HTTP target URLs |
| `attest` | `--rest-url URL --trust-policy FILE --record FILE` | Signed-evidence verification when the target and trust policy support it |
| `trace` | `--input TRACE` | Offline trace validation and summary |
| `bundle` | `--rest-url URL --out DIR` | Alias for `evidence pack` |
| `evidence pack` | `--rest-url URL --out DIR` | Bounded evidence directory |
| `evidence timeline` | `--input DIR` | `timeline.ndjson` and `timeline.md` |
| `telemetry pull` | `--rest-url URL --out DIR` | Local copy of Queen telemetry bundles |
| `fleet` | Repeated `--hive NAME=URL`, then `status`, `lease-summary` or `pressure` | Read-only multi-gateway report |
| `mount` | `--rest-url URL --at DIR` | Foreground FUSE mount |
| `gpu` | Connection options, then `list`, `status --gpu ID` or `lease ...` | Published inventory/status, or a legacy lease request |
| `run` | `--rest-url URL --gpu ID -- PROGRAM ARGS` | Local program after lease validation |
| `peft` | `export`, `import`, `activate` or `rollback` | Job export and host registry lifecycle |

`coh doctor` reports the compiled provider graph, declared provider availability,
and exporter configuration without equating registration with installation.
Local GPU, FUSE and QEMU checks are selected explicitly with `--local-gpu`,
`--require-fuse` and `--developer-tools`. A Mac operator controlling a remote
CUDA node therefore does not probe local NVML. GPU executor profiles use
`--local-gpu` to exercise NVML and the deterministic CUDA fallback.

#### Capture and review evidence

Use a new output directory for each observation. Keep the gateway running:

```bash
run_id="case-$(date -u +%Y%m%dT%H%M%SZ)"
pack="$PWD/out/evidence/$run_id"

"$COH_BIN/coh" inspect --rest-url "$COH_REST_URL" --json
"$COH_BIN/coh" evidence pack --rest-url "$COH_REST_URL" --out "$pack"
```

Continue to review only after checking the exporter result. A capture error
returns nonzero and retains a partial `summary.json`; preserve that directory
for diagnosis rather than overwriting it. A missing optional path is not the
same as a failed read. Optional capture errors also make the export fail.

These commands read the pack without a live Queen:

```bash
"$COH_BIN/coh" inspect --input "$pack" --json
"$COH_BIN/coh" evidence timeline --input "$pack"
```

| Retained file | What to check |
| --- | --- |
| `summary.json` | Which paths were captured, missing or errored? |
| `meta.json`, `bounds.json` | Which exporter and generated host policy produced the capture? |
| `proc/boot`, `proc/authority` when available | Which target identity and authority were reported? |
| `proc/schedule/`, `proc/lease/` | What scheduler and lease state was retained? |
| `log/queen.log` | What remains of the Queen's bounded log? |
| `timeline.md`, `timeline.ndjson` | Human-readable and machine-readable offline correlation |

Add `--with-telemetry` only when needed. `--manifest`, `--resolved-manifest`,
`--serial-log`, `--trace` and `--attestation-record` attach supported local
context. Capture implements bounded redaction; still review the resulting pack
before sharing it. A valid empty audit stream is saved as an empty file after
a positive-size read, not treated as an automatic failure or fabricated read.

Compare two retained observations:

```bash
: "${BEFORE_PACK:?set the before-change evidence directory}"
: "${AFTER_PACK:?set the after-change evidence directory}"
"$COH_BIN/coh" diff --left "$BEFORE_PACK" --right "$AFTER_PACK"
```

A diff is a comparison, not an automatic go/no-go decision. Keep differing
image, manifest and target identities visible. The full inventory, redaction
and CI/SIEM contract is in
[Evidence packs](OPERATOR_RECIPES.md#evidence-packs-ci-and-siem).

#### Verify, export and deliver signed evidence

`coh evidence verify --input GRAPH --trust TRUST --cas CAS`
uses the shared `cohesix-evidence` host verifier. `TRUST` is an operator-supplied
file outside the evidence pack, with exact ticket/subject/action/epoch,
manifest/component/graph hashes, phase-scoped public keys and an explicit
verification time. Every referenced object must exist under its SHA-256 CAS
filename and match its declared size and digest. Historical verification at a
recorded time does not establish current admission authority. The optional
gateway and ticket-agent `--evidence-enrollment-dir` path signs validated native
v1 request observations and native terminal readbacks using distinct enrolled
keys. See [causal evidence custody](CAUSAL_EVIDENCE.md) for enrollment, bounds,
restart behavior and proof classes. Provider results and Worker witnesses have separate custody and proof
requirements; native systemd evidence is not device attestation or Worker proof.

`coh evidence export` takes the same `--input`, `--trust`, and `--cas`, plus
`--format prometheus|otel|cloudevents|in_toto|siem --out FILE`. It emits bounded
derived records, withholding raw payloads, credentials and native identity
text. SLSA output refuses actions without an implemented build provenance
contract. The projections follow [OTLP JSON](https://opentelemetry.io/docs/specs/otlp/),
[CloudEvents 1.0](https://github.com/cloudevents/spec/blob/v1.0.2/cloudevents/spec.md)
and the in-toto statement envelope; a generic action is never described as a
[SLSA build](https://slsa.dev/spec/v1.2/build-provenance). These commands write
local export artifacts. Destination delivery and acknowledgements remain separate.

Evidence packs include the generated provider registry and seal every retained
file. Inspection refuses changed, missing, unexpected and symlinked files in a
sealed pack. Historical unsealed packs remain unverified review inputs. Add
`--causal-graph GRAPH --evidence-trust TRUST --evidence-cas CAS` to `coh evidence
pack` to attach a graph after shared validation; the copied verification result
remains a derived projection. `cohesix.evidence_graph.verify_graph` invokes the
same installed Rust verifier from Python using an explicit executable path.

`coh evidence deliver --input GRAPH --trust TRUST --cas CAS --state-dir STATE`
verifies a causal graph, renders the allowlisted SIEM NDJSON, and attempts
delivery to the generated `providers.siem_delivery` HTTPS destination. The
private state directory retains projections, bounded cursor/WAL, attempt and
retry state, deadletters and exact receiver acknowledgements. Repeat with the
same input after the reported retry time to resume safely. The sink must honor
the documented idempotency and acknowledgement contract. A disabled destination
returns `not_enabled`; no local-file copy is presented as live SIEM delivery.
An optional `providers.siem_delivery.ca_certificate_path_ref` selects an
environment or file reference whose value is an absolute PEM certificate path.
That one CA replaces the default trust roots for this destination; unreadable,
oversized or malformed certificates fail without fallback. The receiver must
commit its idempotency record before acknowledging. A lost reply retries the
same graph/payload hashes after durable backoff, while an acknowledged WAL entry
does not transmit again. A missing WAL in an existing delivery store requires
reconciliation and cannot reset its sequence. Delivery of historically verified
evidence proves receiver acceptance of that record, not fresh provider execution.

#### Pull telemetry or read a small fleet

```bash
"$COH_BIN/coh" telemetry pull --rest-url "$COH_REST_URL" \
  --out out/telemetry/queen

"$COH_BIN/coh" fleet \
  --hive lab-pi=http://127.0.0.1:8080 \
  --hive lab-qemu=http://127.0.0.1:8081 \
  status
```

The fleet example requires **two separately configured gateways**, each owning
its own target; port 8081 is not created automatically. Substitute
`lease-summary` or `pressure` for `status` as needed. Inspect each named row:
a failed hive can be reported within the result, so process completion alone
does not mean the entire fleet is healthy. Fleet reads do not grant cross-hive
write authority.

#### Mount a namespace

Use a mount when an existing program needs files instead of REST. The selected
policy's root and allowlist determine what appears. The mount does not turn
Cohesix into an unrestricted POSIX filesystem.

The binary must include FUSE support. macOS needs the macFUSE/FUSE3 runtime
selected by the native host setup; Linux requires FUSE 3 and usable `/dev/fuse`. Source developers enable this with `cargo build -p coh --features
fuse`. See [the pinned mount integration](../third_party/fuser).

In the mount terminal:

```bash
mount_dir="$PWD/out/mount/cohesix"
mkdir -p "$mount_dir"
"$COH_BIN/coh" mount --rest-url "$COH_REST_URL" --at "$mount_dir"
```

Keep it running. In another terminal at the same installation root:

```bash
mount_dir="$PWD/out/mount/cohesix"
sed -n '1,40p' "$mount_dir/proc/boot"
```

The mount is private to the mounting host user. Only one REST mount may hold
the host-side lock for a given gateway URL. Reads fetch current remote data;
a remote error must not be mistaken for a successfully read empty file.
Writes require the mount process's delegated credentials and the target's
policy. Prefer explicit CLI workflows for control writes instead of tools
that replace, truncate or rename files.

Stop filesystem users, then unmount before terminating the server:

```bash
# Linux
fusermount3 -u "$mount_dir"
```

```bash
# macOS
umount "$mount_dir"
```

After an abrupt server loss, clear the disconnected mount with the same host
unmount procedure. Do not start another server over a still-mounted directory.

The direct and REST FUSE backends share append semantics. `O_APPEND` writes
use the remote atomic append operation even when the kernel has a stale or
nonzero EOF; other write handles require sequential offsets from zero. Cursors
advance only for acknowledged bytes, preserving failed and partial-write
recovery. Canonical `/shard/<label>/worker/<id>/...` paths use the generated
mount allowlist and the same component checks in both backends. Focused mount
checks do not by themselves qualify a physical FUSE deployment.

On the tested macFUSE 5.3.3 VFS backend, OPEN and WRITE omit the caller's
`O_APPEND` flag. `coh mount` accepts the exact EOF it last published through
file attributes as an append position, as well as sequential offsets within an
open handle. Other seek positions fail. All remote writes still use the
namespace's atomic append operation; the filesystem cannot overwrite retained
records. The Linux path preserves the kernel's explicit append flag.

#### Read GPUs and request a compatibility lease

Read the **Queen's published** GPU inventory:

```bash
"$COH_BIN/coh" gpu --rest-url "$COH_REST_URL" list
```

Select an actual returned identifier, not an assumed `GPU-0`:

```bash
: "${COH_GPU_ID:?set a GPU identifier returned by the Queen}"
"$COH_BIN/coh" gpu --rest-url "$COH_REST_URL" status --gpu "$COH_GPU_ID"
```

`coh gpu --nvml list` uses host discovery through a local in-process namespace;
it is not a live Queen read. For the clearest local inventory check, use
`gpu-bridge-host --list`.

**Compatibility profiles only:** after obtaining the required fresh policy
approval and REST authority, a bounded lease request has this syntax:

```bash
"$COH_BIN/coh" gpu --rest-url "$COH_REST_URL" lease \
  --gpu "$COH_GPU_ID" --mem-mb 1024 --streams 1 --ttl-s 60 \
  --priority 1 --receipt-out out/operator/gpu-lease.json
```

Create `out/operator` first. Memory is requested in MiB. The example's size,
stream count and duration must fit the advertised device and selected policy.
The command does not issue its own policy approval or wait for a Worker to
become READY. A receipt records the request result and available lease
observations, not completed GPU work. Production strict-intent profiles refuse
this helper, as explained under [Production Queen commands](#production-queen-commands).

#### Run a host program

Run this **on the machine that should execute the program**, after the
selected GPU has an appropriate active lease. The client needs write scope
for the GPU status breadcrumbs as well as access to read its lease.

```bash
mkdir -p out/operator
: "${COH_GPU_ID:?set the leased GPU identifier}"
"$COH_BIN/coh" run --rest-url "$COH_REST_URL" --gpu "$COH_GPU_ID" \
  --receipt-out out/operator/host-run.json \
  -- python3 -c 'print("host program executed")'
```

This small program verifies the wrapper's local launch path without pretending
to perform CUDA inference. Replace it with your already validated program and
arguments. Arguments after `--` are passed to a host process, not interpreted
as a `cohsh` script.

The wrapper reads and validates the lease, publishes a START breadcrumb, runs
the command, waits for it, and publishes EXIT. It does **not** provide a sandbox,
select CUDA device affinity for the program, or continuously enforce lease
expiry, memory use or revocation. Arrange those protections in the host runtime.
A failed EXIT publication can occur after the program has run; inspect the
local result and retained state before resubmitting anything. Do not put
secrets in program arguments: command information appears in breadcrumbs and
receipts.

Local operation reports: `coh gpu lease --report-out FILE` and
`coh run --report-out FILE` emit `cohesix-operation-report/v1`, explicitly
`authoritative=false`, `proof_class=operation_report`, `mode=client_local`,
and `source_identity=client-local`. The `--receipt-out` spelling and Rust
`*_with_receipt` names remain compatibility aliases. A successful local command
or lease request does not establish CUDA execution or a Worker/provider receipt.

#### Manage PEFT adapters

For an admitted native HF import/training release with actual serving, evaluation
and verified rollback, use [`coh peft release`](PRIVATE_LORA_RELEASE.md). It has
`plan/apply/watch/explain/verify/recover` modes and shares the durable host journal
and signed verifier. The file-registry commands below retain their existing scope.

Use this on the host that owns the training outputs and model registry. It
moves bounded job/registry information; training and inference remain outside
the Queen. Before starting, confirm the selected manifest exposes the LoRA
export paths and that the job has been admitted and made available for export.

Supply real paths and identifiers:

```bash
: "${COH_PEFT_JOB:?set the available LoRA job identifier}"
: "${COH_PEFT_MODEL:?set the model or adapter identifier}"
: "${COH_PEFT_EXPORT:?set the local job export directory}"
: "${COH_PEFT_ADAPTER:?set the trained adapter directory}"
: "${COH_GPU_REGISTRY:?set the host registry directory}"

"$COH_BIN/coh" peft export --rest-url "$COH_REST_URL" \
  --job "$COH_PEFT_JOB" --out "$COH_PEFT_EXPORT"
```

Train and evaluate with the host runtime. The import directory must contain
`adapter.safetensors`, `lora.json` and `metrics.json` in the supported formats,
with provenance consistent with the exported job. Import into the local
registry first:

```bash
"$COH_BIN/coh" peft import \
  --model "$COH_PEFT_MODEL" --from "$COH_PEFT_ADAPTER" \
  --job "$COH_PEFT_JOB" --export "$COH_PEFT_EXPORT" \
  --registry "$COH_GPU_REGISTRY"
```

Without `--publish`, import is local. To publish during import, add
`--publish --rest-url "$COH_REST_URL"` with the required GPU bridge write
credentials. Alternatively publish explicitly with `gpu-bridge-host`.

Activation and rollback in the **`coh` CLI** commit the host registry pointer
and then attempt to publish the registry through the existing GPU bridge:

```bash
"$COH_BIN/coh" peft activate --rest-url "$COH_REST_URL" \
  --model "$COH_PEFT_MODEL" --registry "$COH_GPU_REGISTRY"
```

Verify with a REST-backed `cohsh`:

```text
ls /gpu/models/available
cat /gpu/models/active
```

Separately instruct the actual inference runtime to reload the adapter, run its
canary and retain that result. The active registry pointer does not prove a
reload or a successful inference. To restore the previous pointer:

```bash
"$COH_BIN/coh" peft rollback --rest-url "$COH_REST_URL" \
  --registry "$COH_GPU_REGISTRY"
```

Do not run a continuous registry publisher concurrently with these publication
steps. A local commit can succeed before target publication fails. Reconcile
the registry and the target view; when only projection is pending, publish the
existing registry rather than blindly repeating activation or rollback:

```bash
"$COH_BIN/gpu-bridge-host" --registry "$COH_GPU_REGISTRY" \
  --publish --rest-url "$COH_REST_URL"
```

Python and ticket-agent PEFT helpers differ: their local activation/rollback
reports `execution_location=host projection=pending`; a configured publisher
must update the target view. Never write directly to the read-only
`/gpu/models/active` projection. For the full data/provenance procedure, see
[Adapter rollout](OPERATOR_RECIPES.md#stage-and-reverse-a-private-adapter-rollout).

<a id="signed-device-evidence-availability"></a>
#### Verify signed evidence, not just measurements

`coh attest` without `--trust-policy` inspects attestation-related state; it
does not establish a trusted signature. For a supported, enrolled deployment:

```bash
: "${COH_TRUST_POLICY:?set the independently enrolled trust-policy file}"
mkdir -p out/operator
"$COH_BIN/coh" attest --rest-url "$COH_REST_URL" \
  --trust-policy "$COH_TRUST_POLICY" \
  --record out/operator/attestation-record.json
```

A non-PASS verification returns nonzero. Include a successful retained record
in an evidence pack with `--attestation-record`; later verify that pack with
`coh attest --input PACK --trust-policy FILE`. The trust policy must come from
the verifier, not from untrusted target evidence. Optional profiles can report
unavailable or measurement-only; a stock Pi 4 does not acquire positive signed
attestation merely by running this command. See [Attestation](ATTESTATION.md).

### `hive-gateway`

The gateway owns the one TCP console connection and serves bounded REST
projections. Use the [startup sequence](#connect-and-verify), not a second
gateway for each host application.

| Option | Operational meaning |
| --- | --- |
| `--bind ADDRESS:PORT` | Local HTTP listener; default `127.0.0.1:8080` |
| `--tcp-host HOST --tcp-port PORT` | Upstream Queen; pass these explicitly |
| `--worker-runtime-profile PROFILE` | `qemu-smp-production` or `pi4-production`; match the target |
| `--role ROLE --ticket TICKET` | Upstream namespace authority, not per-caller delegation |
| `--delegation-key-ref REF` | Provisioned issuer source for delegated write verification |
| `--allow-non-loopback-bind` | Explicitly permit network exposure; does not add TLS |
| `--mock` | Own an in-process model instead of a physical or QEMU Queen |

Useful endpoints are `/v1/meta/status`, `/v1/meta/bounds`, `/v1/fs/ls`,
`/v1/fs/cat`, `/v1/fs/tail` and `/v1/fs/echo`. The served OpenAPI contract is at
`/v1/openapi.yaml`; full request, status, batch and refusal semantics are in
[API Guidelines](API_GUIDELINES.md). Use positive read limits and the returned
bounds instead of guessing larger payload sizes.

The broker serialises work over the existing target connection and provides
bounded progress for host-ticket ingress, control/receipts and telemetry.
Concurrent clients share its upstream budget; they do not get independent
copies of that budget. The default shared session allowance is 8 MiB, and a
narrower explicit ticket remains narrower. Retained logs and repeated polling
consume real allowance.

Keep compiled timeout defaults unless a deployment deliberately selects a
different profile. The canonical gateway control and telemetry response
windows are each 120,000 ms; `cohsh`'s REST operation window is 130,000 ms,
including queue and return headroom. An earlier client timeout can leave a
write outcome unconfirmed. Increasing retries or starting additional gateways
is not a safe remedy for overload, exhausted authority or ambiguous writes.

### `gpu-bridge-host`

Run the bridge **on the NVIDIA/CUDA host whose devices and registry you want
the Queen to see**. Running it on a Mac does not discover a remote Jetson.
Backend availability depends on the native build and that host's driver stack.

First inspect without contacting a target:

```bash
"$COH_BIN/gpu-bridge-host" --list
```

Then publish one snapshot through the shared gateway. This requires a delegated
ticket permitting `/gpu/bridge/ctl` and the gateway request token:

```bash
"$COH_BIN/gpu-bridge-host" --publish --rest-url "$COH_REST_URL"
```

For a real model registry, include its host path:

```bash
: "${COH_GPU_REGISTRY:?set the validated registry root on this GPU host}"
"$COH_BIN/gpu-bridge-host" --registry "$COH_GPU_REGISTRY" \
  --publish --rest-url "$COH_REST_URL"
```

A one-shot publisher exits after sending the snapshot. Add `--interval-ms 5000`
for repeated publication only after the one-shot path works and the credentials
cover the intended run. Stop it with Ctrl-C before replacing its credentials or
performing another registry publication workflow.

Verify through the gateway-backed shell:

```text
ls /gpu
cat /gpu/bridge/status
ls /gpu/models/available
```

`--list` alone never publishes. No registry means explicit empty/unavailable
model state, not a demo model or a guessed active selection. An invalid registry
fails. `--mock` selects fixture inventory; an operational target rejects that
publication and it cannot qualify real GPU integration. A successful inventory
snapshot proves neither CUDA execution nor a Worker completion.

For direct mode, use `--tcp-host`, `--tcp-port` and the documented console
credential; that process must be the sole owner. Explicit `--rest-url` is
required to select the REST publishing branch; setting a common URL variable
without passing this option is not a universal transport switch.

#### Admitted GPU workload execution

The optional GPU executor uses the generated `providers.gpu_executor` contract:
a private Unix socket, HMAC-SHA256 authenticated request and response frames,
16 KiB frame bound, one active CUDA context, 64 retained jobs, and a 4 MiB WAL.
Its explicit `cohesix-gpu-executor-config/v1` deployment file binds the GPU ID,
physical CUDA UUID, exact helper SHA-256, provider graph, writer epoch, socket,
private state root, and secret reference. The reference profile is CUDA 13.2.2
on Orin Nano; MIG is unavailable on that profile. Native host administration
remains outside this authority boundary.

Run `gpu-bridge-host --workload-config /absolute/config.json` as the owner of
that private state. Select `host-ticket-agent --gpu-executor-socket PATH
--gpu-executor-credential-ref env:NAME --gpu-request-root PATH --execution-lanes 2` on the same
host. The secret value is never an argument or evidence field. The agent reads
only root-admitted v2 requests, checks the exact ready Worker and active root
lease, and renews a one-second bridge grant while both remain current. A changed
lease sequence, Worker generation, expired ticket, lost agent, or disconnected
control plane revokes execution. Cancellation completes only after the CUDA
child has been killed and reaped. Bridge restart records interruption and never
replays the operation. Retained successful output hashes are checked again
before returning a stored result; full retention produces backpressure.

`coh gpu [connection options] workload --action gpu.workload.submit --spec FILE`
(and the corresponding cancel/observe actions) writes only `/host/tickets/spec`.
Python `client.gpu_workload_ticket(spec)` follows the same path. A submission ACK
is not a provider or Worker terminal receipt. Input CAS JSON uses the canonical
Rust `workload::Input` serialization, is limited to 8192 bytes, and is stored as
`<sha256>.json`. The agent cannot select an executable path through a ticket.
The bridge independently rechecks device topology, free-memory headroom, every
output element, and the expected output digest before reporting success.

When the GPU executor is selected, lane zero is reserved for lease, cancel,
and observe operations. Submissions and other provider work use the remaining
lanes. This keeps cancellation serviceable during CUDA execution. The durable
lane topology includes this selection; changing it requires a fresh journal
root after existing operations are reconciled.

Native workload publication uses `gpu-bridge-host --publish
--native-inventory-config /absolute/executor.json`. This selects the same pinned
helper, CUDA UUID and provider graph as the local executor. The
`cohesix-gpu-device/v1` `execution_identity` in `/gpu/<id>/info` carries the
native topology digest and publisher epoch. `host-ticket-agent` checks it before
execution and throughout lease renewal. Snapshot expiry withdraws the device;
replacement or changed identity revokes an outstanding grant. Legacy inventory
without this identity remains useful for discovery but cannot admit workloads.
Model-catalog availability in `/gpu/bridge/status` is separate from physical
GPU inventory: an empty catalog does not supply model or inference evidence.

### `host-sidecar-bridge`

Use the sidecar to publish observations of the host on which it runs. It does
not execute service restarts, container changes or Kubernetes mutations; those
belong to host-ticket execution.

| Provider selector | What to provision | Continuous `--watch` |
| --- | --- | --- |
| `systemd` | Accessible systemd state on that host | Yes |
| `docker` | Access to the intended Docker runtime | Yes |
| `k8s` | The intended cluster context and permissions | Yes |
| `nvidia` | The supported NVIDIA telemetry stack | Yes |
| `jetson` | Jetson-specific host telemetry | No; one-shot |
| `net` | Host network observations | No; one-shot |

The selected target manifest must expose the matching host namespace/provider.
Choose a provider explicitly rather than assuming all providers are available:

```bash
"$COH_BIN/host-sidecar-bridge" --rest-url "$COH_REST_URL" --provider net
```

Inspect its projection with `cohsh`:

```text
ls /host
ls /host/net
```

For an already working scheduled provider:

```bash
"$COH_BIN/host-sidecar-bridge" --rest-url "$COH_REST_URL" \
  --provider docker --watch
```

Repeat `--provider` to select several. `--policy FILE` selects polling policy;
`--mount PATH` changes the host namespace mount and must match the target.
`net` and `jetson` are not scheduled in watch mode; selecting only those with
`--watch` fails rather than providing continuous updates.

The bridge needs write credentials for its selected publication paths. Provider
failures may be represented as bounded unknown/error observations; a successful
publication is not proof that the provider itself is healthy. In direct mode,
pass the TCP endpoint and `--auth-token` explicitly; do not assume this CLI
reads the same console-token aliases as `cohsh`. A direct watcher occupies the
only console until it exits.

#### Native provider discovery

The read-only macOS network provider uses the compiled
`scripts/providers/network_macos.swift` helper for native interface/link/address
and error-counter state, plus SystemConfiguration default routes. A bounded
native routing-table export retains other routes. Build with
`swiftc -O scripts/providers/network_macos.swift -o network-observer` and set
`COHESIX_MACOS_NETWORK_HELPER` to its absolute installed path. It performs no
network changes and does not probe local NVIDIA devices. Jetson discovery uses
read-only cpufreq/devfreq and thermal cooling-device sources for clock and
throttling observations, including typed unavailable readings when a node is
not supported.

Kubernetes native adapters require `COHESIX_K8S_API_URL`,
`COHESIX_K8S_CA_FILE`, and `COHESIX_K8S_TOKEN_REF` in the agent service's
protected deployment environment. The token reference resolves through the
existing secret resolver. Node UID/resourceVersion fencing, bounded inventory,
and disruption-budget-aware eviction replace CLI table parsing and force drain.
No cluster or service account is provisioned implicitly.

Federation's `forwarded` counter records target write acknowledgement;
`terminal_returned` records the uniquely correlated target outcome returned to
the source. An acknowledged operation stays in `queue_depth` until that return
is durable. Provision each peer's generated request-auth and delegated ticket
environment references (for example `COHESIX_RELAY_HIVE_B_TOKEN` and
`COHESIX_RELAY_HIVE_B_TICKET`). The peer ticket needs the exact write scope for
ticket submission and read scopes for status/deadletter observation. No global
client ticket is substituted. Missing results or conflicts retain recovery
state; they do not trigger another provider execution after an acknowledged write.

`host-sidecar-bridge --native-systemd-unit <unit.service>` reads exact Manager
D-Bus properties without connecting to a Cohesix target. Ticket-agent systemd
start/stop/restart uses the Manager API and waits for the matching native
postcondition. Restart requires a new invocation id and no pending job; an
unobservable dispatched action reports ambiguity; the durable journal prevents
blind replay. Version-1 compatibility records retain their existing failed/ambiguous
terminal classification, while version-2 journals retain pending execution.
The existing `/host/systemd/<unit>/status` text shape is preserved.

#### Source-scoped snapshots

Native source-scoped observations are published by `host-sidecar-bridge` with
explicit `--source-id` and private `--state-dir`; see
[host snapshots](HOST_SNAPSHOTS.md) for generated enrollment, native prerequisites,
withdrawal semantics and Python point-in-time reads. The legacy fixture `/host`
paths do not supply native discovery input.

The `host-sidecar-bridge` Rust library uses the same enrolled `snapshots::Publisher`
as the CLI. `publish_live` takes that publisher and visits its compiled providers;
`publish_live_provider` takes an exact provider id. Both reject a bridge mount
that differs from the compiled enrollment. Publication binds source, sequence, epoch and TTL rather than treating
legacy status-file contents as native discovery input.
`discover_topology` returns bounded native diagnostic entries, never target-seeded
state or publication authority. NVIDIA helper discovery honours an explicitly
enrolled MIG instance and its complete topology fence.

### `sidecar-bus`

Runs compiled MODBUS RTU/TCP and DNP3 point maps with an acknowledgement-aware
private WAL. Build with `--features live,modbus,dnp3`; read requests use
`--state-dir` and `--request`. Controls require an exact admitted ticket and
independent signed enrollment. No request supplies a native address, function,
write value or executable. The normal host-ticket-agent path dispatches these
same adapters, and snapshot publication remains separate from action evidence.
See [FIELD_BUS.md](FIELD_BUS.md) for protocol bounds, commands, service enrollment
and the independent native conformance workflow. WorkerBus remains model-only.

### `host-ticket-agent`

The agent consumes admitted host-action records, checks their scope and
lifecycle, executes supported actions, and publishes receipts/status. Run it
on the host that should perform those actions, with that host's intentionally
limited provider permissions.

**Starting the agent can execute pending actions.** `--run-once` means one
processing pass, not a dry run and not necessarily one ticket. Review the
pending specification snapshot, provider allowlists and selected manifest
before launching it. Begin with a non-mutating provider action such as an
approved `systemd.status-check`, not a service restart.

#### Start with explicit deployment state

Point at the **resolved manifest for this Queen**, not a convenient unrelated
checkout default. Pick a persistent, private state directory on the agent host:

```bash
: "${COH_RESOLVED_MANIFEST:?set the resolved manifest path for this deployment}"
: "${COH_AGENT_STATE:?set a persistent private directory for this agent}"
umask 077
mkdir -p "$COH_AGENT_STATE"

"$COH_BIN/host-ticket-agent" --rest-url "$COH_REST_URL" \
  --manifest "$COH_RESOLVED_MANIFEST" \
  --cursor "$COH_AGENT_STATE/cursor.json" \
  --execution-journal "$COH_AGENT_STATE/execution-journal.json" \
  --agent-lock "$COH_AGENT_STATE/agent.lock" \
  --run-once
```

Supply request auth and the agent's delegated status/receipt write authority in
its environment. Producer and executor are distinct jobs; do not hand either
an issuer signing key. When host tickets are disabled by the manifest, the
agent reports `tickets disabled` and exits successfully without doing work.
Check that message and the per-ticket result, not just the exit code.

For continuous processing, remove `--run-once`; `--poll-ms` defaults to 1000.
Keep one lane initially. `--execution-lanes` accepts 1–64, but lane topology is
bound to durable state. Through the gateway, all lanes still share one
upstream connection and bounded provider capacity. Use REST for multi-lane
operation; the mock
supports exactly one lane. Conflicting provider resources remain serialised.

#### Request and check a non-mutating host observation

The following Python example submits one **host-action request**, not an
arbitrary shell command. Prerequisites: the SDK is installed, the producer's
REST credentials permit `/host/tickets/spec`, the selected policy allows
`systemd.status-check`, and the named unit exists on the agent host. Confirm
`COH_WRITER_EPOCH` against the selected resolved manifest and `/proc/authority`;
do not guess it.

```bash
: "${COH_HOST_UNIT:?set an allowed unit, such as cohesix-agent.service}"
: "${COH_WRITER_EPOCH:?set the current writer epoch for this deployment}"
export COH_HOST_UNIT COH_WRITER_EPOCH
python3 - <<'PY'
import json
import os
import uuid
from cohesix import RestBackend

request_id = "status-" + uuid.uuid4().hex
record = {
    "schema": "host-ticket/v1",
    "id": request_id,
    "idempotency_key": request_id,
    "writer_epoch": int(os.environ["COH_WRITER_EPOCH"]),
    "action": "systemd.status-check",
    "target": f"/host/systemd/{os.environ['COH_HOST_UNIT']}/status",
}
backend = RestBackend(
    base_url=os.environ["COH_REST_URL"],
    request_auth_token=os.environ["COH_REST_AUTH_TOKEN"],
    delegated_ticket=os.environ["COH_REST_TICKET"],
)
print("Submitting host request:", request_id, flush=True)
backend.write_append(
    "/host/tickets/spec",
    json.dumps(record, separators=(",", ":")).encode("utf-8"),
)
print("Request accepted; check agent status for completion:", request_id)
PY
```

Retain the printed identity. Do not rerun this snippet after a timeout: it
creates a new request identity each time. Inspect the original request before
deciding whether further action is authorised. Production requires the current
writer epoch independently of the REST ticket's validity.

After an agent pass, inspect both result destinations in a REST-backed shell:

```text
cat /host/tickets/status.snapshot
cat /host/tickets/deadletter.snapshot
```

Find the exact request ID and its terminal result. An admitted specification
is not a completed status check. A failed or dead-lettered request is a result
to investigate, not a reason to broaden provider permissions automatically.

The ticket agent retains native operation observations under
`--provider-evidence-root` (default `out/provider-evidence`, private mode 0700).
Records are bounded to 64 KiB and 4096 retained objects; a full store refuses
new native dispatch. Results carry `native_observation=sha256:<digest>` and the
corresponding `<digest>.json` stores ticket/action/writer and provider graph
bindings plus exact native identity. File and directory sync precede result
publication. These records remain non-authoritative native evidence until a
complete admitted result graph is validated; they never assert Worker proof.

Signed GPU workload v2 operations also select
`--worker-evidence-enrollment-dir` on host-ticket-agent. The separate witness
custodian consumes the same verified prefix and private CAS; it can sign only
Worker and terminal phases. Native result publication is durable before waiting
for an exact Root Worker completion. Missing evidence holds the journal/cursor
for reconciliation. See [Causal evidence custody](CAUSAL_EVIDENCE.md) for key,
image-enrollment and non-attestation limits.

#### Preserve recovery state

Reuse the same cursor, journal, lock and any relay WAL across restarts. Never
share these files between independent concurrent agents or delete them to
“clear” replay protection. Prepared work, executing work and persisted receipts
have different recovery rules: a retained receipt can be republished; an
ambiguous execution without a receipt can be dead-lettered as
`replay-ambiguous` rather than executed twice.

PEFT execution can use `--registry-root`, `--export-root` and `--adapter-root`
for confined host locations. Federation requires manifest-declared peers and
scope; `--relay --relay-wal FILE` does not invent authority. The single-writer
production Release A profile disables federation. Use the full
[host-ticket contract](INTERFACES.md) and
[authority recovery rules](M27A_AUTHORITY.md#host-execution-and-writer-ownership)
before changing epochs, state placement or execution topology.

### `cas-tool`

Use CAS for the supported **small, bounded update payloads**, not as a general
file-transfer service for model weights or a release archive. Packaging is
local; uploading writes the target's update store.

#### Package locally

Provide the matching generated template, an actual payload and a unique update
epoch. For a signed deployment, select the private signing-key reference whose
public key is trusted by the target:

```bash
: "${COH_CAS_PAYLOAD:?set the input payload file}"
: "${COH_CAS_EPOCH:?set the intended update epoch label}"
: "${COH_CAS_TEMPLATE:?set the matching generated cas_manifest_template.json}"
: "${COH_CAS_SIGNING_KEY_REF:?set a provisioned env: or absolute file: signing-key reference}"

cas_bundle="$PWD/out/cas/$COH_CAS_EPOCH"
"$COH_BIN/cas-tool" pack \
  --epoch "$COH_CAS_EPOCH" --input "$COH_CAS_PAYLOAD" \
  --template "$COH_CAS_TEMPLATE" --out-dir "$cas_bundle" \
  --signing-key "$COH_CAS_SIGNING_KEY_REF"
```

The bundle contains `manifest.cbor` and content-addressed chunks. Omit
`--signing-key` only for a profile that explicitly permits unsigned updates;
source examples using a fixture key are not a production trust chain.
`--delta-base DIR` selects an existing base bundle when creating a supported
delta manifest.

Manifest-v1 allows at most eight chunks. The default 128-byte template therefore
admits at most **1,024 payload bytes**. `--chunk-bytes` only confirms the selected
template's value; it does not override target limits. Oversized payloads fail
before a bundle is written. The template's `limits` fields are tooling metadata,
not extra manifest-v1 wire fields.

#### Upload and verify

With a delegated ticket covering this update's paths:

```bash
"$COH_BIN/cas-tool" upload --bundle "$cas_bundle" --rest-url "$COH_REST_URL"
```

REST delegation comes from `COH_REST_TICKET`; the upload CLI's `--ticket`
option is used for direct TCP attachment, not as its REST caller binding.
Upload validates the manifest, chunk sizes and hashes before contacting the
Queen, then uploads chunks and the manifest. It does not itself perform an
activation or platform reboot. Inspect the matching `/updates` state and follow
[CAS updates](INTERFACES.md) for any subsequent control action.

A structurally valid bundle can still get a typed `buffer-full` refusal because
the target's independent global CAS store is occupied. Preserve that outcome;
do not truncate, relabel or automatically retry a partially delivered bundle.
For direct upload, stop the gateway and use `--host`, `--port` and an explicit
`--auth-token` source. Keep private signing material out of the bundle.

### SwarmUI

SwarmUI is a desktop application, so it needs a graphical session. A headless
Jetson can run the gateway, publishers and CLI while the UI runs on your Mac
through a secured REST connection.

To join the existing gateway without taking its TCP connection:

```bash
SWARMUI_TRANSPORT=rest SWARMUI_REST_URL="$COH_REST_URL" \
  "$COH_BIN/swarmui"
```

Select the intended role/attachment in the UI. Use the namespace and console to
verify `/proc/boot` before relying on the Live Hive display. Reading panels does
not grant write authority; embedded console mutations still need the REST
request token, delegated caller binding and target policy.

| Configuration | Meaning |
| --- | --- |
| `SWARMUI_TRANSPORT=rest` or `gateway` | Share a gateway; use `SWARMUI_REST_URL` or `COH_REST_URL` |
| `SWARMUI_TRANSPORT=console` or `tcp` | Own the TCP console directly; this is the default |
| `SWARMUI_9P_HOST`, `SWARMUI_9P_PORT` | Endpoint variables also used by direct **console** mode, despite their names |
| `SWARMUI_TRANSPORT=9p` or `secure9p` | Use an explicitly supplied host-side Secure9P endpoint, not a new Queen listener |
| `--replay FILE` | Load a retained Hive CBOR snapshot |
| `--replay-trace FILE` | Load a retained trace for offline replay |

SwarmUI does not accept `cohsh`'s `--transport` command-line option. Configure
transport in the launching environment. Do not launch it in default direct mode
while a gateway or direct publisher is connected.

Worker discovery reads the generated shard addresses and actual Worker records;
a bounded aggregate `/shard` listing is not a full fleet census. An empty shard
is valid, but a refused read or invalid Worker record is not an empty successful
result. The selected Worker's details refresh with live polling. Offline
snapshots and replay overlays describe retained observations, not current
readiness. Generated display/cache policy is documented in
[SwarmUI defaults](snippets/swarmui_defaults.md).

SwarmUI's console also supports `man [command]` using the same embedded source.
Its own `help` lists its supported subset and write gates. Shared manuals do
not enable host-only commands; SwarmUI uses raw JSON for `spawn`, while cohsh
accepts the documented role and `key=value` arguments.

### Cohesix Python package

Use the package when your application needs structured calls rather than
terminal scraping. Install into an isolated Python 3.11+ environment. Release
runtime setup installs the bundle's exact wheel; a prepared source checkout can
use:

```bash
python3 -m pip install -e tools/cohesix-py
```

That editable path exists only in a source checkout. See
[Python support](PYTHON_SUPPORT.md) for optional integration and ML dependencies.
Do not substitute a different wheel or target contract to repair a missing
feature in an older bundle.

#### Read from the shared gateway

Choose the backend explicitly so an inherited mock or mount setting cannot
silently change what your application reads:

```bash
python3 - <<'PY'
import os
from cohesix import RestBackend

backend = RestBackend(base_url=os.environ["COH_REST_URL"])
print("Root entries:", backend.list_dir("/"))
print(backend.read_file("/proc/boot", 1024).decode("utf-8"))
print(backend.read_file("/proc/root/reachable", 128).decode("utf-8"))
PY
```

Reads are bounded; use the gateway's metadata and selected contract for larger
operations, not unbounded reads. A REST client validates responses and raises
`CohesixError` for transport or server failures. Mutating calls require
`request_auth_token=` and `delegated_ticket=` or their supported environment
sources; they do not automatically retry ambiguous writes.

| Backend | Use it when | Important boundary |
| --- | --- | --- |
| `RestBackend` | Several tools share one Queen | Inherits gateway authority; separate write delegation |
| `TcpBackend` | Python is the sole console owner | Explicit authentication/attachment; close it when finished |
| `FilesystemBackend` | `coh mount` is already running | Does not create or own the mount |
| `MockBackend` | You need deterministic local model data | Selected filesystem root can persist; not live evidence |

`CohesixClient` and `CohesixOrchestrator` add typed control, Worker, host-ticket,
evidence and PEFT helpers. Live Worker operations require the explicit
compiler-generated target contract, such as
`configs/generated/cohesix_python_pi4_production.json` for Pi or
`configs/generated/cohesix_python_qemu_smp_production.json` for the selected
native QEMU build. The target-neutral wheel's defaults are not a live target
identity. Production requests also need strict intents and the correct writer
epoch; a Python object is not an admission grant.

To explore supported integration patterns without a live target:

```bash
cohesix-playbook --list
cohesix-playbook --playbook mixed-closed-loop-ai-factory --dry-run --mock
```

These are control-model plans, not proof of training, inference, provider
execution or production use-case acceptance.

#### Generated workflow foundation

`coh plan ID` and `coh explain ID` inspect the compiler-owned workflow without
opening a transport. `coh apply ID --deployment FILE`, `watch`, `verify` and
`recover` share a bounded `cohesix-workflow-deployment/v1` file. It binds the
generated graph, controller/target/provider topology, installed signed package,
ordered ticket requests and separately enrolled graph/trust/CAS paths. Use
connection options for live operations and `--ticket-ref file:/absolute/path`
for delegated credentials.

Apply submits only the next unverified action with its durable request and
idempotency identity. Progress requires the shared causal verifier and an exact
signed intent matching the supplied request. Watch observes bounded status;
verify is offline and never refreshes authority. Recover requires its own
current admitted request and a link to the original terminal graph. It never
infers permission for compensation. These reports are explicitly
non-authoritative and keep `production_use_case_accepted=false`.

The generated catalogue retains preflight/admit/execute/observe/verify/recover
stages and explicit external owners. Current domain workflows requiring an
undeployed external application refuse apply with `not_enabled`; generic
control writes cannot stand in for that application. This preserves prior
implementation under its domain-workflow owners, with separate recipe qualification.
Python's `execute_workflow` delegates to this same installed verifier. Existing
live control-model playbooks require explicit `--rehearsal`.

#### Recoverable CUDA recipes

Use `coh plan cuda-reference --recipe` to inspect the generated contract from
`configs/cuda_recipe.toml`. A `cohesix-cuda-recipe/v1` deployment composes existing
`gpu.workload.submit` tickets, pinned provider inputs and separately enrolled
causal evidence. CUDA runs through the host-ticket agent and GPU bridge. The
controller never invokes a native executable or creates an authoritative receipt.

Each deployment supplies `operation_id`, `contract_sha256`, the explicit
`controller`/`target-hive`/`provider-host` topology, an absolute private `journal`
directory, dependency-ordered `stages`, and optional `recovery` cancellations.
Each stage contains `id`, `after`, an absolute `input` path, exact numeric CUDA
`runtime.driver_version` and `runtime.runtime_version`, and `execution` with the
existing `request`, `graph`, `trust` and `cas` fields. `input` contains canonical
`cohesix-gpu-workload-input/v1` bytes, including its pinned helper artifact,
selected device/topology, checked `vadd` or `matmul` configuration and expected
output hash. Its SHA-256 must match the ticket's `args.request_sha256`. These
entrypoints generate their documented deterministic vector/matrix inputs; a
dependency is an ordering and reuse dependency, not an implicit data transfer.
A compatible adopter-built helper must implement this same allowlisted ABI and
be pinned by the existing executor deployment. Arbitrary commands are unavailable.

The checked [Python example](../tools/cohesix-py/examples/cuda_recipe.py) assembles
one or more enrolled stage files and obtains the contract digest from the
installed `coh`. Provision the native request CAS and independent graph trust
through the existing provider workflow. Supply fresh inventory immediately before
initial submission; the executor's five-second inventory freshness remains in force.

```sh
coh plan cuda-reference --recipe --deployment /absolute/recipe.json
coh --ticket-ref file:/absolute/operator.ticket apply cuda-reference --recipe --deployment /absolute/recipe.json --rest-url http://127.0.0.1:8080
coh watch cuda-reference --recipe --deployment /absolute/recipe.json
coh verify cuda-reference --recipe --deployment /absolute/recipe.json
coh --ticket-ref file:/absolute/operator.ticket recover cuda-reference --recipe --deployment /absolute/recipe.json --rest-url http://127.0.0.1:8080
```

Resolve gateway authentication through `COH_REST_AUTH_TOKEN=env:NAME` or an
explicit protected file reference. `plan` durably creates identity/configuration
before dispatch. `apply` persists each attempt and reservation before its one
ticket write. `watch`, `explain` and `verify` read the local journal and independently
reverify evidence and output CAS bytes. They do not refresh a grant. Copy the
provider's actual `output.bin` to the referenced CAS under its signed output hash;
metadata alone is insufficient. `verify` returns nonzero until every stage has a
compatible verified output. Read-only `recover` reconciles the same original
idempotency through current scoped ticket reads and retained signed evidence.

A short write, disconnect or lost ACK remains ambiguous. Retain the journal and
run the existing host-ticket agent against its original execution WAL; restart
that owner to reconcile the native job. Do not replace the operation ID or repeat
submission to clear an ambiguity. To cancel unresolved work, add a `recovery`
entry naming `stage` and a separately enrolled `gpu.workload.cancel` execution
whose `args.job_id` is the original submission ID, then use
`recover ... --cancel-stage STAGE`. Recovery tickets can be refreshed without
changing workload configuration. A cancellation request retains the reservation
until signed native evidence confirms termination. A verified failed/cancelled
stage can receive a new ticket after replanning; fresh execution always rechecks
expiry, published compatibility and native/root authority.

Reuse remains within the same protected operation journal. Code/helper, deterministic
data/parameters, runtime, device/topology, predecessor keys and predecessor output
digests enter the cache key. Changing a stage invalidates it and its descendants;
independent verified stages survive. Reuse across revisions is limited to one hour,
requires current scoped visibility and compatible published device/runtime, and
rechecks every CAS hash. Reuse never imports an old execution grant. Keep the same
operation ID and journal when replanning; unresolved work must be reconciled first.

The generated bounds allow eight stages, 32 cumulative attempts/revisions, one
active workload, zero automatic retries, a 256 KiB journal, 256 KiB per output and
8 MiB retained output across attempts. Reservations, observed allocations and
confirmed releases remain cumulative across failures and revisions. Unknown
allocation measurements are counted explicitly. Native enforcement records name
cgroup controls, one CUDA context and child kill/reap; allocation admission is
not a hard GPU memory partition. Successful child termination provides resource
cleanup. The recipe retains bounded immutable outputs/evidence references for
review and performs no automatic CAS deletion or native compensation.

For a failed or ambiguous stage, retain its JSON report and use the canonical pack:

```sh
coh --ticket-ref file:/absolute/operator.ticket evidence pack --rest-url http://127.0.0.1:8080 --out out/case --recipe-report /absolute/failed-report.json
coh evidence timeline --input out/case --scenario incident
```

The existing `case.json`/`case.md` identify the stage, original ticket/idempotency,
remaining uncertainty and permitted recovery. `attachments/recipe.json` contains
only sanitized local observations, with proof `none`. If capture returns a
nonzero partial-pack result, preserve its summary/errors and attach the report
with `coh evidence timeline --input out/case --scenario incident --recipe-report
/absolute/failed-report.json`. The diagnostic never upgrades incomplete capture. Signed causal graphs may
also be attached using the existing `--causal-graph`, `--evidence-trust` and
`--evidence-cas` options. No second pack or receipt format is introduced.

Python's `execute_workflow("cuda-reference", ..., recipe=True)` and
`python -m cohesix.playbook_cli plan --playbook cuda-reference --recipe` use the
same installed Rust implementation. The recipe options belong to external `coh`;
root-shell/cohsh/SwarmUI console grammars do not gain workflow commands.

#### Build and validate the Python distribution

Python distribution builds use `scripts/install/build_python_package.py` and the
`release.python_artifacts` list emitted by `coh-rtc`. The builder copies only
listed SDK modules, `pyproject.toml`, and the package README into a private
temporary directory. It inspects both the wheel and sdist for exact source
hashes, archive entry types, byte bounds, package version, and wheel RECORD
hashes. Tests, caches, example state, keys and credentials are excluded.
Run the builder with a Python environment containing setuptools, wheel and
packaging, then run the existing focused installation smoke:

```sh
python3 scripts/install/build_python_package.py --out out/python-distributions
scripts/ci/python_compat_run.sh --wheel-smoke \
  --wheel-dir out/python-distributions \
  --package-manifest out/python-distributions/python-package.json \
  --state-dir out/python-package-smoke
```

The distribution report records observed contents and hashes; release signing
and target qualification remain separate records.

## Common environment variables

Pass endpoints explicitly in scripts. There is **no suite-wide environment
parser**: an alias supported by one executable need not be read by another.

| Variable | Use and caveat |
| --- | --- |
| `COH_BIN` | Convenience in this guide: directory containing the selected executables |
| `COH_AUTH_TOKEN`, `COHSH_AUTH_TOKEN` | Console credential aliases used by `cohsh`, gateway and selected clients; not a universal substitute for `--auth-token` |
| `COH_AUTH_TOKEN_REF` | Explicit credential reference recognised by `cohsh`, GPU bridge and Python before compatibility token aliases |
| `COH_REST_URL` | Common gateway URL; this guide also passes `--rest-url` so bridges select REST explicitly |
| `COH_REST_AUTH_TOKEN`, `COHSH_REST_AUTH_TOKEN`, `HIVE_GATEWAY_REQUEST_AUTH_TOKEN` | REST request-token aliases; precedence differs, so avoid conflicting values |
| `COH_REST_TICKET` | Signed delegated caller ticket for REST mutations; not the console password or a host-action record |
| `HIVE_GATEWAY_DELEGATION_KEY_REF` | Issuer key reference on the gateway/issuer host; do not distribute the key to clients |
| `COH_POLICY`, `COHSH_POLICY` | Selected generated policy for `coh` or `cohsh` |
| `COHSH_TICKET_CONFIG`, `COHSH_TICKET_SECRET` | Explicit minting configuration/key source; issuer-side material |
| `COH_CAS_SIGNING_KEY_REF` | CAS private signing source; public verification material is a separate artefact |
| `SWARMUI_TRANSPORT`, `SWARMUI_REST_URL` | UI connection selection |
| `SWARMUI_REST_AUTH_TOKEN` | UI-specific request-auth alias; also accepts the common aliases documented above |

For direct `cohsh` and GPU bridge authentication, an explicit option takes
precedence, then `COH_AUTH_TOKEN_REF`, `COH_AUTH_TOKEN`, and
`COHSH_AUTH_TOKEN`. CAS upload, sidecar and ticket-agent direct examples should
use their explicit token option rather than relying on that precedence.
Credentials can use supported `env:NAME` or `file:/absolute/path` sources;
a failed selected reference must not fall back to a different credential.
Never print tokens, commit populated environment files or include secrets in
support transcripts. Use private deployment storage outside the source tree.

## Operational checks

Before changing state, confirm the target identity and enabled feature, the
sole connection owner, both REST write credentials, complete path scope and
quota, and the operation's approval/epoch/lifecycle requirements. Capture a
baseline when the change needs a rollback or incident record.

A local prerequisite check is not a connection test; a control ACK is not
Worker READY; a published snapshot is not executed work; and an offline pack
is not fresh live state. Require the exact terminal result for the job you
submitted. Do not infer broader acceptance from a narrower success.

The default shared session budget is finite and cumulative. Plan captures and
polling accordingly. When it legitimately exhausts, stop clients and establish
a fresh authorised session under the approved operating policy. Do not restart
services to bypass ticket quotas, replay protection or a frozen test's limits.

## Troubleshooting

Investigate the **first failed boundary**, retain its exact error, and avoid
changing several settings at once. More detailed routing is in
[Failure modes](FAILURE_MODES.md).

| Symptom | Check first | Appropriate response |
| --- | --- | --- |
| Executable or policy file not found | Installation root, `COH_BIN`, generated configuration and native architecture | Use one complete matching installation; do not borrow files from another release. |
| Doctor succeeds but reads fail | Doctor is local; target address, readiness, sole TCP owner and authentication | Run the gateway status and `/proc/boot` checks. |
| TCP refused, busy or gateway disconnected | Existing direct shell/UI/watch/mount, endpoint and selected network | Release the old owner; keep one gateway and use REST clients. |
| `ERR AUTH` or rejected placeholder | Credential for the exact target and selected reference | Correct the provisioned source; do not use fixture secrets. |
| Reads work but writes return `EPERM` | Request token, delegated ticket, scope/mount, expiry, role and target policy | Correct the specific missing authority; `--role queen` alone is not a fix. |
| Legacy spawn or `coh gpu lease` is denied | Production strict-intent policy | Use the versioned intent path; do not weaken the production profile. |
| `ELIMIT`, quota or bounded backpressure | Payload size, queued work, ticket operations/bytes and shared session usage | Respect the typed limit; pause producers and investigate before changing budgets. |
| Timeout after a mutation | Whether the target/agent retained the original identity and result | Treat delivery as unconfirmed; inspect audit/status before any retry. |
| GPU visible locally, absent on Queen | Whether a real publish ran, its credentials and target `/gpu` feature | Publish from the actual GPU host and inspect `/gpu/bridge/status`. |
| Sidecar exits or reports unknown provider | Explicit provider selection, host access, manifest and watch support | Test one supported provider; `net`/`jetson` are one-shot. |
| Agent exits zero without execution | `tickets disabled`, pending queue, matching manifest and cursor | Check each ticket's status/dead letter; do not delete durable state. |
| PEFT command fails after changing the registry | Local pointer state versus target publication | Reconcile and publish the existing state; do not blindly activate/rollback again. |
| CAS preflight passes but upload gets `buffer-full` | Independent target store occupancy | Preserve the partial outcome and use the documented update lifecycle. |
| FUSE reports a lock or disconnected mount | Existing mount owner and host mount table | Stop users and unmount cleanly before starting another mount. |
| UI is empty or disagrees with CLI | UI transport, target identity, offline mode and underlying read errors | Trust the identified source record, not cached presentation state. |
| Evidence exporter returns nonzero | Retained `summary.json` and first required/optional read error | Preserve the partial pack; use a new directory for any later capture. |

A raw append must not be retried automatically after an ambiguous response.
Where the strict-intent contract permits an explicit retry, reuse the exact
original envelope and identity; a new ID is a new request. Neither increasing
client retries nor restarting the Queen establishes exactly-once external work.

## Shut down without losing state

Stop new producers first. Let admitted work reach its documented terminal state
and retain the needed evidence. Stop continuous GPU/sidecar publishers and
agents, preserving their journals and cursors. Quit REST shells and the UI;
stop filesystem users and unmount FUSE. Stop the gateway **last**, after clients
have finished using its connection.

Stopping a gateway or host process does not roll back an external action,
revoke an already running CUDA program or reboot the Queen. Use the target's
[maintenance lifecycle](OPERATOR_RECIPES.md#run-a-maintenance-window) for a
planned target shutdown or reboot.

## Provider contracts and conformance

`coh providers`, `cohsh --provider-registry`, and the admin-only
`GET /v1/meta/providers` endpoint expose the same compiled contract without
provider dispatch. Registration and declared requirements are separate from
native observations and verified execution.

For macOS service control, [macOS native providers](MACOS_PROVIDERS.md) describes
the compiled service map, measured process helper, lifecycle postconditions and
owned-service reference check. `launchd.start`, `stop`, `restart` and
`status-check` require exact configured targets; an empty map remains unavailable.

The compiler extends the stable integration graph with
`configs/generated/provider_registry.json` and the Python
`cohesix.providers` projection. These describe required provider identity,
target grammar, admission, lifecycle and evidence fields; registration cannot
assert live execution. The Jetson reference is Ubuntu 24.04, L4T 39.2.1,
JetPack 7.2.1 and CUDA toolkit 13.2.2. Unsupported accelerators remain explicit.

For read-only host discovery, run
`scripts/ci/provider_conformance_run.sh --native-providers --live-reference
--host-profile jetson-orin-nano-jp7 --state-dir <fresh-directory>` on the
selected Linux host. Jetson identity/package/thermal/power observations,
network address/link/route counters and a version-negotiated Docker Engine
inventory have finite byte, row and time bounds. Docker requires an authorized
local socket. Discovery mode returns
`INCOMPLETE` with exit code 2, and lists missing execution phases; it cannot
mark a provider or use case production-proven.

`configs/provider_conformance.toml` selects focused host contract checks from
the same generated registry. Run `scripts/ci/provider_conformance_run.sh
--matrix configs/provider_conformance.toml --evidence-only --state-dir
<fresh-directory>` for one group, or select `--executors-only`,
`--observability-only`, `--packaging-only`, `--identity-only` or
`--registry-only`. `--provider federation` selects relay recovery contracts.
`--validate-only` records the exact selection without executing it. The runner
rejects unknown profiles/providers, duplicate cases, incomplete lifecycle
obligations, unscoped commands and oversized output. A zero-exit command that
ran no tests is not PASS. Host-contract PASS preserves generated availability;
native execution, Worker results, installation and receiver delivery retain
their own evidence. Use the repository Python environment with pytest installed
on PATH when selecting Python checks.

For two local hive instances, the QEMU launcher accepts distinct `--tcp-port`,
`--udp-echo-port` and `--tcp-smoke-port` host ports in `1..65535`. Explicit smoke
ports do not silently fall back. Guest diagnostic ports and image identity are
unchanged. Each hive still has one gateway owning its console connection.

The live federation check requires two disposable provisioned hives with empty
ticket streams, a running target native agent, and the exact generated source
and target manifests. It compares both Root manifest measurements before
submitting the single read-only `systemd.status-check`. The source's selected
peer URL must be a free IPv4 loopback port for the owned fault proxy; the real
target URL uses TLS or loopback. The proxy drops the first accepted-forward
reply. Three source-agent processes must return one exact target terminal,
recover their WAL and avoid another send after acknowledgement:

```sh
scripts/ci/provider_conformance_run.sh --provider federation --live-reference \
  --matrix configs/provider_conformance.toml --state-dir out/provider-conformance/federation-01 \
  --source-agent /opt/cohesix/a/bin/host-ticket-agent \
  --source-manifest /opt/cohesix/a/config/root_task_resolved.json \
  --source-policy /opt/cohesix/a/config/coh_policy.toml \
  --target-manifest /opt/cohesix/b/config/root_task_resolved.json \
  --source-url http://127.0.0.1:8080 --target-url https://hive-b.example \
  --source-auth-ref env:HIVE_A_REQUEST_AUTH --source-ticket-ref env:HIVE_A_TICKET \
  --target-auth-ref env:HIVE_B_REQUEST_AUTH --target-ticket-ref env:HIVE_B_TICKET \
  --native-unit ssh.service
```

This check observes correlated terminal return. The target's native evidence,
signed custody and any Worker proof require their separate validators; the
runner never fabricates a target result. It stops only its owned fault proxy
and source-agent processes. The independently deployed hives and target agent
remain under their existing supervisors.

## Generated integration truth

The selected manifest and compiled policy determine which paths, providers,
roles and bounds exist. `coh-rtc` compiles
[`configs/host_integration_acceptance.toml`](../configs/host_integration_acceptance.toml)
into the [dependency graph](../configs/generated/host_integration_dependency.json)
and [support table](snippets/host_integration_dependency.md). Consult those
records when a packaged tool exists but a live feature is unavailable.

Package presence, mock success, provider availability, Worker READY,
correlated execution and use-case acceptance are independent states. Optional
Pi diagnostic rows remain diagnostic text; their presence, absence or elapsed
counters must not be converted into CPU utilisation, readiness or performance
claims. QEMU/model evidence does not qualify a physical Pi or its selected
network path.

<a id="milestone-27-compatibility-review"></a>
### Maintaining this guide

This is an operator reference, not a running regression journal. Preserve
command, failure, authority and recovery contracts here; keep individual
restoration histories in their owning audit/test records and Git history.
Changes to public functionality must review the eight executables, shared
status/transport/provider libraries, Python package, `.coh` workloads and
benchmark consumers together. This documentation rewrite changes no runtime,
wire contract, generated default or benchmark workload.

| Need the exact contract? | Read |
| --- | --- |
| Serial console, shell grammar and scripts | [Userland and CLI](USERLAND_AND_CLI.md) |
| REST endpoints, deadlines and errors | [API Guidelines](API_GUIDELINES.md) |
| Namespace and control record schemas | [Interfaces](INTERFACES.md) |
| Delegation, strict intents, epochs and migration | [M27a authority](M27A_AUTHORITY.md) |
| Full situation-based workflows | [Operator walkthrough](OPERATOR_WALKTHROUGH.md), [Operator recipes](OPERATOR_RECIPES.md) |
| Python installation and typed APIs | [Python support](PYTHON_SUPPORT.md) |
| Evidence, replay and signed-device trust | [Operator evidence](OPERATOR_EVIDENCE.md), [Attestation](ATTESTATION.md) |
| Physical target bring-up and acceptance | [Hardware bring-up](HARDWARE_BRINGUP.md), [Test plan](TEST_PLAN.md) |
| Performance methodology, not health guesses | [Benchmarks](BENCHMARKS.md) |

Implementation references for checking installed options:
[`coh`](../apps/coh/src/main.rs), [`cohsh`](../apps/cohsh/src/main.rs),
[`gateway`](../apps/hive-gateway/src/main.rs),
[`GPU bridge`](../apps/gpu-bridge-host/src/main.rs),
[`sidecar`](../apps/host-sidecar-bridge/src/main.rs),
[`ticket agent`](../apps/host-ticket-agent/src/main.rs),
[`CAS`](../apps/cas-tool/src/main.rs),
[`SwarmUI`](../apps/swarmui/src-tauri/main.rs), and
[`Python backends`](../tools/cohesix-py/cohesix/backends.py).

## Release factory

**Maintainers only.** Operating an installation does not require building a
release. This appendix retains the assembly entry point used by
[Quickstart](QUICKSTART.md#build-from-source). The compiler inventory owns
version, notes and exact file selection; never relabel immutable historical
release evidence.

<details>
<summary>Native inputs, production preflight, assembly and qualification</summary>

Signed host deployment profiles come from the same compiler registry.
`coh package build`, `verify`, and `install` bind the exact file set, source
inventory, configuration hashes, native format/architecture and file SBOM to an
independently enrolled Ed25519 signer. `coh doctor --package` additionally checks
the profile's exact credential-reference map. See [host package deployment](../packaging/README.md)
for staging, external trust, systemd/LaunchAgent enrollment and the distinction
between package validation and native provider execution. The renderer verifies
its input package and writes service files without activating them.

### Prepare exact native inputs

Build from one clean selected source and provisioned manifest. Assembly requires
`--release-manifest` naming that source manifest and rejects development
authority profiles, including in build-only mode. Provision private secret
sources and the deployment public verification key as described in
[M27a authority](M27A_AUTHORITY.md#production-profile-and-secret-sources).
The factory does not infer SSH, user, NVMe, Cargo or credential locations.

Mac uses `qemu_smp_production`, HVF and a 24 MHz counter. Native Linux uses
`qemu_smp_kvm_production`, KVM, a native 31.25 MHz counter and `-cpu host`.
Do not distribute the Mac guest as the Linux guest or force a different KVM
counter. Native tools include `coh` FUSE on both hosts and NVML on Linux.
After building the selected Linux seL4 profile, run its native regression lane:

```bash
COHESIX_SEL4_PROFILE=qemu_smp_kvm_production \
SEL4_BUILD_DIR="$PWD/out/sel4/profile-v2/qemu-smp-kvm-production" \
COHSH_LOG_ROOT="$PWD/out/regression-logs/release-linux" \
  scripts/cohsh/run_regression_batch.sh
```

Retain each native evidence tree intact: logs, relative paths,
`qemu-artifact.json`, matching passing `base.json` and
`release-configs/configs/generated/`. `scripts/release_inputs.py` verifies the
source, native profile and guest/tool hashes against the result. Downloading
Linux records to a Mac is archival verification, not a native Linux test.

Staged context v2 and assembly share
`scripts/ci/qemu_artifact.py source-digest`. Historical v1 differences require
new qualification, not edited digests. The native QEMU Python contracts share
a filename but must retain their correct HVF/KVM target-profile binding.
Stage the Pi image from the same clean source. Build one target-neutral wheel
and run this in each native checkout after its selected-profile build:

```bash
scripts/ci/python_compat_run.sh --wheel-smoke
```

Retain `PYTHON_PACKAGE_MANIFEST` for Mac and
`LINUX_PYTHON_PACKAGE_MANIFEST` for the downloaded Linux record. Both must bind
the exact wheel and their native retained configuration.

### Preflight and assemble

These are templates: replace every angle-bracket field with the retained path
or provisioned setting before executing. First validate the inputs:

```text
scripts/release_bundle.sh --check-manifest --linux \
  --release-manifest <provisioned-source.toml> \
  --macos-artifact <mac-qemu-artifact.json> --macos-result <mac-base.json> \
  --linux-artifact <linux-qemu-artifact.json> --linux-result <linux-base.json> \
  --linux-builder-max-glibc <major.minor> --pi4-stage-dir <local-pi4-stage>
```

Then assemble on Mac, using the selected remote ARM64 builder for Linux archive
compression:

```text
scripts/release_bundle.sh --name Cohesix-1.0.0-beta --version 1.0.0-beta \
  --release-manifest <provisioned-source.toml> \
  --linux --linux-use-accepted-tools \
  --macos-artifact <mac-qemu-artifact.json> --macos-result <mac-base.json> \
  --linux-artifact <linux-qemu-artifact.json> --linux-result <linux-base.json> \
  --pi4-stage-dir <local-pi4-stage> \
  --linux-builder-host <host> --linux-builder-user <user> \
  --linux-builder-release-dir <remote-release-root> \
  --linux-builder-max-glibc <major.minor>
```

Add `--linux-builder-key PATH` only when SSH agent/config authentication is
insufficient. Existing output requires `--force`. `--linux-use-accepted-tools`
copies the tested binaries; rebuilding remotely additionally requires
`--linux-builder-build-dir`, `--linux-builder-cargo`, `--linux-builder-cargo-home`,
`--linux-host-tools-dir` and `--linux-host-tools-manifest`. Changed binaries
require new qualification.

The independent `scripts/linux_host_tools_sync.sh build-tools` workflow ships
the complete clean tree, including `third_party/fuser`, verifies archive/tree
hashes and regenerates KVM-owned policies and Python defaults. `--no-clean`
retains Cargo cache, never stale source; each build replaces source from the
exact archive.

### Keep assembly modes distinct

| Mode | Requirements | What the result means |
| --- | --- | --- |
| Tested assembly | Exact native artefacts and matching passing TCP results | Packages those qualified bytes; still requires distribution validation below. |
| Owner-approved publication reuse | `--qualified-source-root PATH`; clean tested ancestor and clean publication checkout; exact delta accepted by `scripts/release_publication.py`; Linux accepted tools | Retains original qualification and adds publication provenance. No tests or new PASS. |
| Explicit build-only | `--build-only`; fresh native artefact records; production manifest; omit result flags; no `--qualified-source-root` | `assembly_mode=build-only`, `test_status=NOT_RUN`, null result hash. No acceptance carried forward. |

Publication reuse must preserve runtime, manifest/policy, lock, generated
contract and test/evidence identities outside the guard's allowed publication
delta. It is not a general waiver to reuse stale results. Provenance retains
both source identities and allowed before/after file hashes.

For build-only, use `scripts/cohesix-build-run.sh --no-run`, stage Pi with
`scripts/pi4-image-build.sh` without flash options, and build native Linux tools
with the canonical sync workflow. Record fresh artefacts through
`scripts/ci/qemu_artifact.py record`, action `release.build-only`, with the clean
source digest and no test attempt. `BUILD_PROVENANCE.json` records source,
native profile/timer and exact guest/tool/configuration identities in all modes.

### Validate the distributed bytes

The Pi archive contains a compact raw MBR/FAT32 image, SHA-256 sidecar and
`cohesix-pi4-portable-sd-image/v2` metadata. `minimum_target_bytes` sets the
minimum card capacity; extra space stays unallocated. Embedded-file integrity
is packaging evidence, not physical boot evidence.

All archives carry the maintained root `QUICKSTART.md`, with links relocated
by the factory rather than a separate invented Pi procedure. After assembly,
run **Conditional G** in the [Test Plan](TEST_PLAN.md): validate each packaged
QEMU launcher on its native host and perform Pi image readback plus a fresh
configured boot of the distributed image. Archive creation or build-only
success is not release acceptance.

</details>
