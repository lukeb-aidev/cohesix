<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Catalog Cohesix host tools and define their safe composition rules. -->
<!-- Author: Lukas Bower -->
# Cohesix Host Tools

Cohesix keeps CUDA, NVML, container integrations, REST, desktop UI, packaging,
and automation on the host. Host tools may perform local work, but every target
read or mutation remains a projection of the documented console and Secure9P
semantics. They do not create new in-target listeners or authority paths.

This document owns the host-tool catalog and composition rules. Command grammar
belongs in [USERLAND_AND_CLI.md](USERLAND_AND_CLI.md), REST behavior in
[API_GUIDELINES.md](API_GUIDELINES.md), and the runnable live sequence in
[OPERATOR_WALKTHROUGH.md](OPERATOR_WALKTHROUGH.md). Task-oriented evidence,
mount, ticket, federation, lifecycle, and PEFT procedures live in
[OPERATOR_RECIPES.md](OPERATOR_RECIPES.md).

## Generated integration truth

`coh-rtc` compiles
[`configs/host_integration_acceptance.toml`](../configs/host_integration_acceptance.toml)
into the exact
[`host-integration-dependency/v1`](../configs/generated/host_integration_dependency.json)
graph. The generated [support table](snippets/host_integration_dependency.md)
binds each advertised host binary, library API, use case, and built-in Python
playbook to its required mode, package, evidence lane, and owning milestone.
Executable-Worker proof, provider availability, package presence, mock or
dry-run success, and use-case promotion are independent states.

### V35/V24 adjacent-refill compatibility boundary

Discovery task `m26e-console-network-service-isolation` and reopened Milestone
25 root-service temporal restoration carried by `m26e-root-tcb-target-proof`
select
`m26e-qemu-root-adjacent-refill-natural-postpone-candidate-v35` for QEMU and
`m26e-pi4-root-adjacent-refill-natural-postpone-candidate-v24` for Pi. Both
root-control TCBs use `NaturalPostpone`: generated timeout capabilities,
badges, resources, and registry identities remain reserved, the timeout
endpoint is omitted, and standard faults remain terminal. All scheduling
numerics and the distinct generated QEMU 24 MHz and Pi 54 MHz clocks remain
unchanged; common child provenance remains V18.

This target-internal scheduling-policy repair changes no schema, public API,
wire frame, console grammar, namespace, workload, retry, client or protocol
timeout, host-tool implementation, Python-library contract, benchmark
implementation, evidence record, or report schema. The complete compatibility
review covers `coh`, `cohsh`, `coh-status`, Hive Gateway, SwarmUI,
`gpu-bridge-host`, `host-ticket-agent`, `host-sidecar-bridge`, `sidecar-bus`,
`cas-tool`, every `.coh` script, `tools/cohesix-py`,
`scripts/m26e_qemu_pressure.sh`, and `scripts/rest_perf_harness.py`; no host
surface requires a contract edit. Exact-version execution is still mandatory:
fresh QEMU must pass staged acceptance, the full `.coh` regression harness,
REST core/parity, every host tool, Python, Conditional B2 executable target
pressure, and the companion Conditional D host-model gateway matrix. QEMU
cannot qualify Pi; Pi V24 separately requires a fresh generated 54 MHz build,
exact flash/readback, cold boot, applicable hardware suite, host-tool/Python
checks, and a Pi-selected target-performance proof. Conditional D does not
supply QEMU or Pi target evidence.

The immutable V34/V18 Stage 03 state
`out/test-plan/m26e-console-qemu-v34-v18-oraclefix-20260814T104728Z`, attempt
`20260814T105938.465736Z-11947-27f75501fecd`, passed Stage 01 and Stage 02. Its
base/gated artifact IDs were
`sha256:11921e2eedbf8e9c46f781c500b89acdcb9669ebda42eb6db0ed21a4eb47dac3`
and
`sha256:46ce91c8bffae218f557fedb19ec125cdded39118db641aee70db9e63949163b`.
The fixed matrix passed 7/7, all ten base scripts passed including 9P and
`session_pool.coh`, and a fresh telemetry boot passed `telemetry_ring.coh`;
`telemetry_push_create.coh` then failed after each replacement connection wrote
the complete 18-byte AUTH frame and read zero bytes. Immutable replay proved
task 0, timeout badge `0x26ee0001`, label 5, at
the outer Yield after ordinary Network/Timer, with no trace and tick `356343`,
not divisible by 8,000. The adjacent refill amounts were the exhausted current
`38,090` ticks and already-valid next `93,910` ticks; their sum is the unchanged
`132,000` ticks or `5,500 us` at QEMU 24 MHz. The installed terminal timeout
endpoint converted exhaustion of only the current refill, despite the valid
adjacent refill, into root-fault and downstream fail-stop. This is failure
evidence, not host-tool or Stage 03 qualification.

### Stage 04 REST operation deadline

The subsequent V35/V18 run passed the complete Stage 03 fixed matrix and all 18
selected `.coh` scripts, then exposed a host-only Stage 04 deadline mismatch:
the shared Rust REST client stopped a filesystem operation after three seconds
while Hive Gateway could legally spend five seconds on bounded broker-queue
admission and 120 seconds waiting for the target response. The repair cites
active `m26e-console-network-service-isolation`, reopened
`m24e-rest-client`, `m24e-cohsh-rest-transport`,
`m25-smp-rest-regression-batch`, and the deadline-composition portion of
`m25f-gateway-broker-refactor`.

For filesystem operations, clients compose the response window as
`5,000 ms + max(control_response_ms, telemetry_response_ms) + 5,000 ms`.
The canonical Hive Gateway broker deadlines are `120,000/120,000 ms`, so the
canonical filesystem-operation response window is `130,000 ms`. Metadata,
name resolution, connection establishment, and response-body transfer keep
their short bounds. The underlying HTTP library carries request-send deadlines
into response-header receipt, so the filesystem-operation agent gives request
send, request body, and response-header receipt the same composed window; this
does not enlarge the allowed request body, response body, queue, or namespace.

All eight packaged host executables were reviewed:

- `hive-gateway` owns and reports the two broker response deadlines;
- `cas-tool`, `coh`, `cohsh`, `gpu-bridge-host`, `host-sidecar-bridge`,
  `host-ticket-agent`, and `swarmui` consume the shared safe Rust default when
  they use REST;
- `cohsh` additionally accepts `--rest-response-timeout-ms` or
  `COHSH_REST_RESPONSE_TIMEOUT_MS`; the checked value reaches both the primary
  transport and every pooled REST transport.

The Stage 04 runner applies the same resolved window to its Python
`RestBackend` smoke. This is an explicit test-call setting, not a change to the
Python library's public default. `scripts/rest_perf_harness.py` retains its
existing workloads, no-retry meaning, error budget, and report schema. No
endpoint, HTTP or target wire schema, command grammar, ACK/ERR/END meaning,
authority, batch concurrency, pool size, or retry policy changes. A fresh
Stage 04 run and later exact-version host-tool, Python, and performance gates
remain required; deadline alignment alone is not acceptance.

### Performance backend selection

`hive-gateway` reports `backend_class=host-model` only for its explicit
in-process `--mock`/`HIVE_GATEWAY_MOCK=1` backend. The performance harness's
`--gateway-mock` mode launches that backend and skips target TCP preflight. A TCP-console gateway reports
`backend_class=console-projection`. The REST performance harness requires that
distinction before population work: synthetic high-count `host-model` loads run
only against the host model, while target-backed executable loads use generated
role slots plus accepted target evidence. A mismatch fails before Worker
discovery, policy approval, or `/queen/ctl` mutation; generic target `busy`
remains non-retryable and is not reclassified as capacity.

Conditional D measures the exact packaged gateway and large-reference
telemetry path with a fresh host-model process for each scenario. Conditional
B2 separately owns QEMU's three generated executable roles. The currently
generated QEMU and Pi profiles each declare one Heartbeat, one GPU, and one LoRA
slot, but their manifest identities and target evidence differ. The packaged
QEMU gateway contract and QEMU acceptance validator cannot qualify Pi; a Pi
performance run requires a Pi-selected gateway contract and fresh-Pi evidence.
Conditional D passes `--tail-bytes 8192` explicitly. That is one complete,
fail-closed structured Worker-state read; it does not retry, truncate, or alter
the gateway endpoint. The historical 256-byte harness default predates the
381-byte host-model observation and is not a valid comparator for this lane.
It also passes `--strict-control-errors`, ensuring every typed bounded refusal
is retained as an error rather than relaxed into a successful host-model
operation.

The in-process host model matches target schedule, lease, and export control
mirror behavior: an individually valid JSONL record rotates oldest complete
mirror records within `ctl_max_bytes`, while the independent semantic
queue/list/window capacities still refuse deterministically. The fast ramp
holds the configured Worker/intensity maximum for its final interval; a summary
that stops below that endpoint is not qualifying evidence.

The in-process transport maps only exact `TooBig` schedule, lease, and export
semantic-capacity errors to the canonical
`ERR ECHO reason=quota detail=buffer-full path=<path> error=buffer full`, while
retaining the typed source error. Hive Gateway projects that semantic refusal
as HTTP `200` with `GatewayResponse.status=ERR`; unmatched paths, messages, or
error codes remain generic failures. Conditional D's joint
`--strict-control-errors` plus `--no-retries` contract counts a returned
refusal once without harness retry. The gateway's independent bounded retry
window and counters remain unchanged.

## Choose one live topology

The target TCP console is single-client. Use direct mode for one foreground tool or
gateway mode when tools must operate concurrently.

```mermaid
flowchart LR
  TARGET["Cohesix target\nauthenticated TCP console"]

  subgraph DIRECT["Direct mode: choose one owner"]
    ONE["cohsh, coh, SwarmUI,\nor one bridge"]
  end

  subgraph MULTI["Gateway mode: concurrent host clients"]
    CLIENTS["cohsh, coh, SwarmUI,\nPython, bridges, curl"]
    GW["hive-gateway"]
    CLIENTS -->|"bounded REST projection"| GW
  end

  ONE -->|"sole TCP connection"| TARGET
  GW -->|"sole TCP connection"| TARGET
```

The two incoming TCP arrows are alternatives, not concurrent paths.

### Composition rules

| Mode | Console owner | Safe clients | Important constraint |
| --- | --- | --- | --- |
| Direct TCP | One direct tool | Only that process | A continuous publisher holds the console; stop it before starting another direct client. |
| Gateway | `hive-gateway` | Multiple REST-capable tools | Writes require gateway request authentication and still use the gateway's upstream role/ticket. |
| In-memory mock | The selected Rust executable | That process only | State is not shared across executables and is not live-system evidence. |
| Python `MockBackend` | A local filesystem root | Processes selecting the same root | State can persist and be shared locally, but it is not live-system evidence. |
| Mounted filesystem | `coh mount` over direct TCP or REST | Filesystem consumers | The mount is foreground; only one REST mount lock is allowed per gateway URL. |

Do not start a direct `cohsh` session while a direct `--watch` or
`--interval-ms` publisher is running. Point both at `hive-gateway` instead.

## Authentication layers

| Layer | Configuration | What it proves |
| --- | --- | --- |
| TCP console authentication | `COH_AUTH_TOKEN` or tool-specific equivalent | Access to the target console listener |
| Upstream attach | Gateway/direct-tool role plus optional capability ticket | Target namespace identity, scope, budget, and subject |
| Gateway request authentication | `HIVE_GATEWAY_REQUEST_AUTH_TOKEN`, `COH_REST_AUTH_TOKEN`, or tool-specific equivalent | Permission to call the host HTTP write edge |
| Target policy and lifecycle | Manifest rules and current target state | Whether the requested operation is allowed now |

Gateway request authentication is not delegated target identity. Every REST client
inherits the role and optional ticket with which the gateway attached upstream.
The target remains authoritative for ticket, policy, lifecycle, path, and quota
checks.

Keep the gateway bound to loopback unless an explicitly secured deployment
requires otherwise. The console and gateway do not provide transport-layer TLS;
use an authenticated tunnel, VPN, or TLS-terminating reverse proxy for remote
access.

## Tool catalog

The source commands below use `cargo run -p <package> -- ...`. Release bundles
provide the corresponding executable under `bin/`. Run each executable with
`--help` for the authoritative option list compiled into that build.

### `cohsh`

Interactive and scripted operator shell. It supports direct TCP, REST, QEMU,
and in-process mock transports; performs role attachment; and implements the
command and `.coh` grammar defined in
[USERLAND_AND_CLI.md](USERLAND_AND_CLI.md).

```bash
cargo run -p cohsh -- --help
```

Use direct TCP only when `cohsh` is the sole console owner. For concurrent use,
select `--transport rest` and a running gateway. The current REST transport
accepts only the local `queen` role and still inherits the gateway's upstream
role and optional ticket. The default REST filesystem-operation response window
is 130,000 ms for the canonical 120,000/120,000 ms gateway broker profile. Use
`--rest-response-timeout-ms` or `COHSH_REST_RESPONSE_TIMEOUT_MS` only when the
gateway declares a different profile; the value must be at least
`5,000 + max(control, telemetry) + 5,000` milliseconds.

Direct-TCP `CAT` preserves ordinary lines up to 256 bytes unchanged. A longer
canonical JSON line uses the existing response stream and the exact versioned
wire form `C1:<seq4hex>:<count4hex>:<full_sha256>:<utf8_payload>`. Sequence and
count are four lowercase hexadecimal digits, sequence starts at zero and is
contiguous, count is in `1..=64`, every wire line remains at most 256 bytes,
and every chunk repeats the full lowercase SHA-256 of the reconstructed line.
The reconstructed line is bounded to 2,048 bytes. `cohsh` reassembles this
format before returning `CAT` output and rejects partial, reordered, replayed,
mixed-digest, oversized, or noncanonical groups. This does not add a verb,
path, authority, or larger global console-output queue.

### `coh`

Host integration CLI with these command families:

| Command | Purpose |
| --- | --- |
| `doctor` | Validate local policy/ticket, mount, GPU, and runtime prerequisites. |
| `mount` | Mount the allowed namespace through FUSE. |
| `gpu` | List GPUs and manage GPU leases. |
| `peft` | Export, import, activate, and roll back PEFT adapters. |
| `run` | Run a host command after lease validation and record bounded breadcrumbs. |
| `telemetry pull` | Export Queen telemetry to a local directory. |
| `fleet` | Read status, lease summaries, or pressure from multiple REST gateways. |
| `evidence pack` | Export a deterministic evidence directory. |
| `evidence timeline` | Build NDJSON and Markdown timelines from an evidence pack. |

```bash
cargo run -p coh -- --help
cargo run -p coh -- doctor --help
```

The evidence-pack inventory, redaction behavior, missing-path semantics, and
offline CI/SIEM contract are defined in
[OPERATOR_RECIPES.md#evidence-packs-ci-and-siem](OPERATOR_RECIPES.md#evidence-packs-ci-and-siem).

`coh mount` remains in the foreground. Create the mount point first, keep the
mount process in its own terminal, and use the host's normal FUSE unmount
procedure before terminating it. Generated policy and doctor behavior are in
[snippets/coh_policy.md](snippets/coh_policy.md) and
[snippets/coh_doctor_checks.md](snippets/coh_doctor_checks.md). Verified macOS,
Linux, REST, and direct-mode mount procedures are in
[OPERATOR_RECIPES.md#mounted-namespace-with-fuse](OPERATOR_RECIPES.md#mounted-namespace-with-fuse).

### `hive-gateway`

Host-only REST multiplexer. It owns the one target TCP console connection and
projects `LS`, `CAT`, `TAIL`, and `ECHO`, plus bounded metadata endpoints.

```bash
cargo run -p hive-gateway -- --help
```

Non-mock startup requires both a non-placeholder TCP console token and a
non-placeholder request-auth token. The default bind is `127.0.0.1:8080`;
non-loopback binds require an explicit opt-in. See
[API_GUIDELINES.md](API_GUIDELINES.md) for endpoint, status, and compatibility
rules.

### `gpu-bridge-host`

Discovers host GPUs through the compiled backend, builds the `/gpu` snapshot,
and optionally publishes it through `/gpu/bridge/ctl`.

```bash
# Local inventory only; no target mutation.
cargo run -p gpu-bridge-host -- --list

# One REST publish from a real registry; exits after the snapshot is sent.
cargo run -p gpu-bridge-host -- --registry "$COH_GPU_REGISTRY" \
  --publish --rest-url "$COH_REST_URL"
```

`--list` does not publish. `--publish` without `--interval-ms` is one-shot;
adding an interval runs continuously. A continuous direct-TCP publisher owns
the console for its lifetime. Live publication resolves and rejects placeholder
TCP or REST credentials before opening a connection/request. A missing registry
publishes explicit empty/unavailable state; an invalid registry fails. There is
no demo-catalog or first-active-model fallback. `--mock` selects deterministic
fixture inventory and carries `source_mode=fixture`; the operational target
rejects it, and it cannot satisfy integration, release, attestation, or use-case
evidence.

### `host-sidecar-bridge`

Projects selected host providers into `/host`. Supported provider selectors are
`systemd`, `k8s`, `docker`, `nvidia`, `jetson`, and `net`.

```bash
# One provider collection through the gateway.
cargo run -p host-sidecar-bridge -- \
  --rest-url "$COH_REST_URL" --provider net

# Continuous gateway-backed publication for a scheduled provider.
cargo run -p host-sidecar-bridge -- \
  --rest-url "$COH_REST_URL" --provider docker --watch
```

Provider availability is host-dependent. Failures are published as bounded
unknown/error state where the provider contract permits; they do not authorize
host control. Watch scheduling supports `systemd`, `docker`, `k8s`, and
`nvidia`; `net` and `jetson` are one-shot providers. In direct `--watch` mode
the bridge is the sole TCP owner.

### `host-ticket-agent`

Consumes manifest-enabled host control tickets, validates their lifecycle and
scope, executes supported host actions, records status, and resumes from a
bounded cursor. Optional federation relay behavior is defined entirely by the
resolved manifest; `--relay` does not invent peers or delegated authority.

```bash
cargo run -p host-ticket-agent -- --help
cargo run -p host-ticket-agent -- --rest-url "$COH_REST_URL" --run-once
```

Use a dedicated cursor and relay WAL per deployment. Do not share state files
between concurrently running agents. Ticket schemas and status paths are
defined with the manifest-gated namespaces in
[INTERFACES.md#host-tickets-and-federation](INTERFACES.md#host-tickets-and-federation).
Runnable local and federated examples are in
[OPERATOR_RECIPES.md#host-tickets-and-federation](OPERATOR_RECIPES.md#host-tickets-and-federation).

### `cas-tool`

Packages signed or unsigned content-addressed bundles locally and uploads a
prepared bundle through direct TCP or REST.

```bash
cargo run -p cas-tool -- pack --help
cargo run -p cas-tool -- upload --help
```

`pack` writes a manifest and chunks; it does not contact the target. `upload`
appends the bundle through the documented CAS control paths and is subject to
request authentication, target authority, bounds, and update policy. CAS schemas
are in [INTERFACES.md#cas-updates](INTERFACES.md#cas-updates).

Manifest-v1 uses the shared maximum of eight chunks. With the
default generated template's 128-byte chunks, the largest structurally
eligible payload is therefore 1,024 bytes. The generated JSON template exposes
that host-tool projection as `limits.max_chunks = 8` and
`limits.max_payload_bytes = 1024`; `limits` is tooling metadata and is not an
additional CBOR manifest-v1 wire field. A legacy template without `limits`
falls back to the shared eight-chunk maximum. When `limits` is present,
`max_chunks` must equal the shared maximum exactly and `max_payload_bytes` must
equal `chunk_bytes * max_chunks`; a smaller or larger declaration is rejected
as contract drift. `--chunk-bytes` is only an explicit confirmation and must
equal the selected template's `chunk_bytes`.

`pack` rejects an over-limit payload before writing a bundle. `upload` decodes
and validates a prepared manifest before reading its chunks or opening a TCP or
REST connection, so a legacy or foreign bundle with more than eight chunks is
also a local zero-network failure. This preflight proves only manifest
eligibility. An otherwise valid bundle can still receive the target's typed
`buffer-full` refusal when other chunks or models consume the independent
global CAS store capacity; clients must preserve that refusal and must not
retry, truncate, or silently relabel the payload.

### SwarmUI

SwarmUI is the host desktop application for bounded telemetry, replay, status,
and the shared operator console. Read-only panels and Live Hive rendering do
not add protocol verbs; the embedded console can issue the same authorized
commands as `cohsh`.

```bash
cargo run -p swarmui -- --help
```

| `SWARMUI_TRANSPORT` | Use |
| --- | --- |
| `console` or `tcp` | Direct console mode; SwarmUI is the sole TCP owner. |
| `rest` or `gateway` | Concurrent mode through `hive-gateway`. |
| `9p` or `secure9p` | A documented host-side Secure9P endpoint; never an extra in-target listener. |

For gateway mode, set `SWARMUI_REST_URL` or `COH_REST_URL`. Write auth resolves
from `SWARMUI_REST_AUTH_TOKEN`, `HIVE_GATEWAY_REQUEST_AUTH_TOKEN`,
`COHSH_REST_AUTH_TOKEN`, or `COH_REST_AUTH_TOKEN`. Generated display and cache
defaults are in [snippets/swarmui_defaults.md](snippets/swarmui_defaults.md).

### Cohesix Python package

The Python package supplies filesystem, direct TCP, REST, and deterministic
mock backends plus typed orchestration helpers. Its installation, API, and
backend rules are owned by [PYTHON_SUPPORT.md](PYTHON_SUPPORT.md).

## Common environment variables

| Variable | Consumer | Meaning |
| --- | --- | --- |
| `COH_TCP_HOST`, `COH_TCP_PORT` | Gateway and selected host tools | Target TCP console endpoint |
| `COH_AUTH_TOKEN`, `COHSH_AUTH_TOKEN` | Direct tools and gateway | TCP console authentication |
| `COH_ROLE`, `COH_TICKET` | Gateway and selected tools | Upstream role and optional ticket |
| `COH_REST_URL`, `COHSH_REST_URL`, `HIVE_GATEWAY_URL` | REST-capable clients | Gateway base URL |
| `HIVE_GATEWAY_REQUEST_AUTH_TOKEN`, `COH_REST_AUTH_TOKEN`, `COHSH_REST_AUTH_TOKEN` | Gateway and REST clients | HTTP mutation authentication |

Not every executable accepts every alias; its `--help` and the tool-specific
sections above are authoritative. Prefer deployment-scoped environment files or
a secret manager with restrictive permissions. Never commit a populated secret
file.

## Operational checks

Before adding a tool to a live topology:

1. Confirm whether it uses direct TCP, REST, a mount, or only local files.
2. Confirm the gateway's upstream role/ticket is sufficient for any REST write.
3. Confirm the active manifest exposes the required path and feature gate.
4. Use `/v1/meta/bounds` or generated client policy for request sizing.
5. Check [FAILURE_MODES.md](FAILURE_MODES.md) before retrying a failed mutation.

The full, ordered example is in
[OPERATOR_WALKTHROUGH.md](OPERATOR_WALKTHROUGH.md). Start there for a new live
deployment; use [OPERATOR_RECIPES.md](OPERATOR_RECIPES.md) only after that
topology is healthy.
