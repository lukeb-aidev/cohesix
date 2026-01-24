<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Describe Cohesix userland command surfaces, CLI usage, and console workflows. -->
<!-- Author: Lukas Bower -->
# Cohesix Userland & CLI

## Philosophy
`cohsh` is the canonical operator shell for the entire hive: one Queen orchestrating many workers via a shared Secure9P namespace.

## Overview
Cohesix userland exposes two operator entry points:
- **Root console** on the PL011 UART via QEMU `-serial mon:stdio`, showing the `cohesix>` prompt for on-box bring-up and bootinfo sanity checks.
- **`cohsh` host CLI** (`coh>` prompt) running on the host, speaking to the Cohesix instance over TCP (QEMU for development, UEFI hardware in deployment) or the mock/QEMU transports for development. `cohsh` never executes inside the VM and follows the same pattern on physical hardware.

Use the root console for low-level validation (bootinfo, capability layout, untyped counts) and quick liveness checks. Use `cohsh` for day-to-day operator workflows and NineDoor interactions.

## Root Console (PL011 / QEMU Serial)
### Access and purpose
- Brought up once PL011 initialises; exposed on QEMU `-serial mon:stdio`.
- Prompt: `cohesix>` from the in-kernel console loop.【F:apps/root-task/src/console/mod.rs†L216-L305】
- Intended for local debug/bring-up: verify seL4 bootinfo, CSpace layout, untyped enumeration, and that the root task is alive.

### Commands (current behaviour)
- `help` – list available commands.【F:apps/root-task/src/console/mod.rs†L224-L233】
- `bi` – bootinfo summary (node bits, empty window, IPC buffer if present).【F:apps/root-task/src/console/mod.rs†L234-L250】
- `caps` – key capability slots (root CNode, endpoint, UART).【F:apps/root-task/src/console/mod.rs†L252-L263】
- `mem` – untyped cap counts with RAM vs device breakdown.【F:apps/root-task/src/console/mod.rs†L265-L283】
- `ping` – replies `pong` as a liveness check.【F:apps/root-task/src/console/mod.rs†L285-L293】
- `quit` – currently prints `quit not supported on root console`; the loop continues (no session exit).【F:apps/root-task/src/console/mod.rs†L285-L299】

### Example boot and probe
```
[cohesix:root-task] [uart] init OK
[console] PL011 console online
cohesix> help
Commands:
  help  - Show this help
  bi    - Show bootinfo summary
  caps  - Show capability slots
  mem   - Show untyped summary
  ping  - Respond with pong
  quit  - Exit the console session
cohesix> bi
[bi] node_bits=12 empty=[0x0010..0x0100) ipc=0x7f000000
cohesix> caps
[caps] root=0x0001 ep=0x0002 uart=0x0003
cohesix> mem
[mem] untyped caps=16 ram_ut=14 device_ut=2
cohesix> ping
pong
```
Use this surface to confirm boot-time state before bringing up TCP or NineDoor; it is not the operator-facing control plane.

## `cohsh` Shell (Host CLI)
### What it is
- Rust CLI at `apps/cohsh`, installed to `out/cohesix/host-tools/cohsh` by the build script.【F:scripts/cohesix-build-run.sh†L402-L442】
- Pure client: runs on the host, never inside QEMU.
- Supports transports: `tcp` (primary), `mock` (in-process NineDoor stub), `qemu` (dev convenience to spawn QEMU). Default is `tcp` when built with the TCP feature.【F:apps/cohsh/src/main.rs†L44-L132】
- The TCP console accepts a single client at a time; `cohsh`/SwarmUI take an exclusive host-side lock by default to prevent concurrent attachments.

### CLI flags (current)
Key options from `--help`:
- `--role <role>` and `--ticket <ticket>` to auto-attach on startup.
- `--mint-ticket` to emit a host-side ticket and exit; requires `--role`, accepts `--ticket-subject` (required for worker roles), `--ticket-config` (or `COHSH_TICKET_CONFIG`) and `--ticket-secret` (or `COHSH_TICKET_SECRET`) to override config secrets.
- `--script <file>` to execute commands non-interactively.
- `--record-trace <file>` to record Secure9P frames + ACKs to a trace file (requires `--transport mock`).
- `--replay-trace <file>` to replay a trace file deterministically (requires `--transport mock`; rejects tampered traces).
- `--transport <mock|qemu|tcp>` to choose backend; TCP exposes `--tcp-host` / `--tcp-port` (defaults `127.0.0.1:31337`).【F:apps/cohsh/src/main.rs†L44-L132】
- QEMU helpers: `--qemu-bin`, `--qemu-out-dir`, `--qemu-gic-version`, `--qemu-arg` (dev/CI convenience).【F:apps/cohsh/src/main.rs†L52-L131】
- `--auth-token` forwards the TCP console authentication secret; defaults to `changeme`.【F:apps/cohsh/src/main.rs†L78-L115】
- `--policy <file>` (or `COHSH_POLICY`) selects the manifest-derived client policy TOML; `cohsh` fails fast if the policy hash mismatches compiled defaults. Defaults to `out/cohsh_policy.toml`.
- Pool sizing overrides: `--pool-control-sessions`, `--pool-telemetry-sessions` (env `COHSH_POOL_CONTROL_SESSIONS`, `COHSH_POOL_TELEMETRY_SESSIONS`).
- Retry/heartbeat overrides: `--retry-max-attempts`, `--retry-backoff-ms`, `--retry-ceiling-ms`, `--retry-timeout-ms`, `--heartbeat-interval-ms` (env `COHSH_RETRY_MAX_ATTEMPTS`, `COHSH_RETRY_BACKOFF_MS`, `COHSH_RETRY_CEILING_MS`, `COHSH_RETRY_TIMEOUT_MS`, `COHSH_HEARTBEAT_INTERVAL_MS`).
- `COHSH_CONSOLE_LOCK=0` disables the exclusive TCP console lock (debug-only; concurrent clients will churn).

Manifest-derived policy defaults are emitted by `coh-rtc` into `out/cohsh_policy.toml` and embedded into the CLI at build time. The CLI refuses to start if the policy or manifest hash drifts.

<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->

### cohsh client policy (generated)
- `manifest.sha256`: `ea6ca43101b547b7730d1b706dc19d88ee08e9d428d9e8d5e411b459afa2c547`
- `policy.sha256`: `c1e183f3447f311c443dc4e16087bd25ee9855b06fd6bb717435988edf7c24f7`
- `cohsh.pool.control_sessions`: `2`
- `cohsh.pool.telemetry_sessions`: `4`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
- `heartbeat.interval_ms`: `15000`
- `trace.max_bytes`: `1048576`

_Generated from `configs/root_task.toml` (sha256: `ea6ca43101b547b7730d1b706dc19d88ee08e9d428d9e8d5e411b459afa2c547`)._


Manifest-derived CohClient defaults (paths and Secure9P bounds) are emitted by `coh-rtc`.

<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh client snippet consumed by docs/USERLAND_AND_CLI.md. -->

### cohsh client defaults (generated)
- `manifest.sha256`: `ea6ca43101b547b7730d1b706dc19d88ee08e9d428d9e8d5e411b459afa2c547`
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `trace.max_bytes`: `1048576`
- `client_paths.queen_ctl`: `/queen/ctl`
- `client_paths.log`: `/log/queen.log`
- `telemetry_ingest.max_segments_per_device`: `4`
- `telemetry_ingest.max_bytes_per_segment`: `32768`
- `telemetry_ingest.max_total_bytes_per_device`: `131072`
- `telemetry_ingest.eviction_policy`: `evict-oldest`

_Generated from `configs/root_task.toml` (sha256: `ea6ca43101b547b7730d1b706dc19d88ee08e9d428d9e8d5e411b459afa2c547`)._


Shared console grammar and ticket policy are emitted by `coh-rtc` from `cohsh-core` so CLI and console stay aligned.

<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh grammar snippet consumed by docs/USERLAND_AND_CLI.md. -->

### cohsh console grammar (generated)
- `help`
- `bi`
- `caps`
- `mem`
- `ping`
- `test`
- `nettest`
- `netstats`
- `log`
- `cachelog [n]`
- `quit`
- `tail <path>`
- `cat <path>`
- `ls <path>`
- `echo <path> <payload>`
- `attach <role> [ticket]`
- `spawn <payload>`
- `kill <worker>`

_Generated from cohsh-core verb specs (18 verbs)._


<!-- Author: Lukas Bower -->
<!-- Purpose: Generated cohsh ticket policy snippet consumed by docs/USERLAND_AND_CLI.md. -->

### cohsh ticket policy (generated)
- `ticket.max_len`: `224`
- `queen` tickets are optional; TCP validates claims when present, NineDoor passes through.
- `worker-*` tickets are required; role must match and subject identity is mandatory.

_Generated from cohsh-core ticket policy._

<!-- coh-rtc:ticket-quotas:start -->
<!-- Author: Lukas Bower -->
<!-- Purpose: Generated ticket quota snippet consumed by docs/SECURITY.md and docs/USERLAND_AND_CLI.md. -->

### Ticket quota limits (generated)
- `ticket_limits.max_scopes`: `8`
- `ticket_limits.max_scope_path_len`: `128`
- `ticket_limits.max_scope_rate_per_s`: `64` (0 = unlimited)
- `ticket_limits.bandwidth_bytes`: `131072` (0 = unlimited)
- `ticket_limits.cursor_resumes`: `16` (0 = unlimited)
- `ticket_limits.cursor_advances`: `256` (0 = unlimited)

_Generated by coh-rtc (sha256: `1b869521f68c26d43c1ad278fbc557f2442e438ab12d443a142e53a33e4466fb`)._
<!-- coh-rtc:ticket-quotas:end -->


## coh Host Bridges (Mount / GPU / Telemetry Pull)
- Host-only CLI at `apps/coh` with subcommands `mount`, `gpu`, and `telemetry pull`.
- `coh mount` provides a FUSE view over Secure9P namespaces; `--mock` uses an in-process NineDoor backend, while `--features fuse` is required for live mounts.
- `coh gpu` exposes list/status/lease UX over `/gpu/*` and `/queen/ctl`; `--mock` provides deterministic CI output, `--nvml` (Linux-only, feature-gated) mirrors the host NVML inventory.
- `coh telemetry pull` pulls `/queen/telemetry/*` segments into host storage; resumable and idempotent (per-segment files).
- Policy enforcement is manifest-driven; `COH_POLICY` (or default `out/coh_policy.toml`) must hash-match compiled defaults.

Manifest-derived coh policy defaults are emitted by `coh-rtc`.
<!-- coh-rtc:coh-policy:start -->
<!-- Author: Lukas Bower -->
<!-- Purpose: Generated coh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->

### coh policy defaults (generated)
- `manifest.sha256`: `aeacd14e34c15b39b879af95c0cc5c19de757d368702d1453024ce4cd910a8cb`
- `policy.sha256`: `35df2c524f27ce12f7417360e3f5e0e19fdf8241fd657e7619bf2c5d0223f1cb`
- `coh.mount.root`: `/`
- `coh.mount.allowlist`: `/proc, /queen, /worker, /log, /gpu, /host`
- `coh.telemetry.root`: `/queen/telemetry`
- `coh.telemetry.max_devices`: `32`
- `coh.telemetry.max_segments_per_device`: `4`
- `coh.telemetry.max_bytes_per_segment`: `32768`
- `coh.telemetry.max_total_bytes_per_device`: `131072`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
<!-- coh-rtc:coh-policy:end -->


## SwarmUI Desktop (Host UI)
- Host-only Tauri app at `apps/swarmui`.
- Default transport is the TCP console (`cohsh` transport); set `SWARMUI_TRANSPORT=9p` to use Secure9P (9P/TCP).
- Transport endpoint is `SWARMUI_9P_HOST` / `SWARMUI_9P_PORT` (defaults `127.0.0.1:31337`).
- Console auth token uses `SWARMUI_AUTH_TOKEN` (fallback `COHSH_AUTH_TOKEN`, default `changeme`). No HTTP/REST dependencies.
- SwarmUI enables CSP `script-src 'unsafe-eval'` to support the PixiJS Live Hive renderer.
- Presentation-only frontend: no retries, caching policy, or background polling logic.
- Offline mode reads cached CBOR snapshots from `$DATA_DIR/snapshots/` and never touches the network.
- Trace replay uses `--replay-trace <file>` (relative paths resolved under `$DATA_DIR/traces/`).
- `--mint-ticket` emits a host-side ticket and exits; accepts `--role`, `--ticket-subject`, `--ticket-config`, `--ticket-secret` (env `SWARMUI_TICKET_CONFIG` / `SWARMUI_TICKET_SECRET`, fallback to `COHSH_*`).
- The "Mint ticket" UI button uses the same host-only secrets and places the token into the Ticket field for reuse.
- When caching is enabled, successful panel reads persist CBOR transcripts for offline replay.

Manifest-derived SwarmUI defaults are emitted by `coh-rtc`.
<!-- coh-rtc:swarmui-defaults:start -->
<!-- Author: Lukas Bower -->
<!-- Purpose: Generated SwarmUI defaults snippet consumed by docs/USERLAND_AND_CLI.md. -->

### SwarmUI defaults (generated)
- `manifest.sha256`: `ea6ca43101b547b7730d1b706dc19d88ee08e9d428d9e8d5e411b459afa2c547`
- `swarmui.defaults.sha256`: `9434866ec6914caccf33ada0c419aa98f299d3b1be4eababa327e6234fc2fe02`
- `swarmui.ticket_scope`: `per-ticket`
- `swarmui.cache.enabled`: `false`
- `swarmui.cache.max_bytes`: `262144`
- `swarmui.cache.ttl_s`: `3600`
- `swarmui.hive.frame_cap_fps`: `60`
- `swarmui.hive.step_ms`: `16`
- `swarmui.hive.lod_zoom_out`: `0.7`
- `swarmui.hive.lod_zoom_in`: `1.25`
- `swarmui.hive.lod_event_budget`: `512`
- `swarmui.hive.snapshot_max_events`: `4096`
- `swarmui.paths.telemetry_root`: `/worker`
- `swarmui.paths.proc_ingest_root`: `/proc/ingest`
- `swarmui.paths.worker_root`: `/worker`
- `swarmui.paths.namespace_roots`: `/proc, /queen, /worker, /log, /gpu`
- `trace.max_bytes`: `1048576`

_Generated from `configs/root_task.toml` (sha256: `ea6ca43101b547b7730d1b706dc19d88ee08e9d428d9e8d5e411b459afa2c547`)._
<!-- coh-rtc:swarmui-defaults:end -->

### Interactive shell surface
Startup banner and prompt:
```
Welcome to Cohesix. Type 'help' for commands.
detached shell: run 'attach <role>' to connect
coh>
```

Commands and status:
- `help` – show the command list.【F:apps/cohsh/src/lib.rs†L1125-L1162】
- `attach <role> [ticket]` / `login` – attach to a NineDoor session. Valid roles: `queen`, `worker-heartbeat`, `worker-gpu`, `worker-bus`, `worker-lora` (CLI accepts `worker` as an alias for `worker-heartbeat`); missing roles, unknown roles, too many args, or re-attaching emit errors via the parser and shell.【F:apps/cohsh/src/lib.rs†L711-L729】【F:apps/cohsh/src/lib.rs†L1299-L1317】
- `detach` – close the current session without exiting the shell (required for multi-role scripts).【F:apps/cohsh/src/lib.rs†L1244-L1255】
- `tail <path>` – stream a file; `log` tails `/log/queen.log`. Requires attachment.【F:apps/cohsh/src/lib.rs†L1170-L1179】
- `ping` – reports attachment status; errors when detached or when given arguments.【F:apps/cohsh/src/lib.rs†L1181-L1194】
- `test [--mode <quick|full>] [--json] [--timeout <s>] [--no-mutate]` – run the in-session self-tests sourced from `/proc/tests/` (default mode `quick`, default timeout 30s, hard cap 120s). `--no-mutate` skips spawn/kill steps. When `--json` is supplied, emit the stable schema described below.【F:apps/cohsh/src/lib.rs†L1512-L1763】
  - Note: the bundled self-test scripts end with `quit`; interactive `cohsh` reattaches to the last session when possible, while `--script` runs remain detached and require a fresh `attach`.
- `pool bench <k=v...>` – run the pooled throughput benchmark and retry/exhaustion checks; options include `path`, `ops`, `batch`, `payload`, `payload_bytes`, `delay_ms`, `inject_failures`, `inject_bytes`, `exhaust`, `kind`.
  - On TCP console transports, throughput is informational only; readback uses write acknowledgements (CAT is skipped) and line-length limits apply before `payload_bytes`.
- `echo <text> > <path>` – append a newline-terminated payload to an absolute path via NineDoor.【F:apps/cohsh/src/lib.rs†L1211-L1222】【F:apps/cohsh/src/lib.rs†L1319-L1332】
- `ls <path>` – list directory entries; entries are newline-delimited and returned in lexicographic order.
- `cat <path>` – bounded read of file contents.
- `spawn <role> [opts]` – queue a worker spawn via `/queen/ctl` (e.g. `spawn heartbeat ticks=100`, `spawn gpu gpu_id=GPU-0 mem_mb=4096 streams=2 ttl_s=120`).
- `kill <worker_id>` – queue a worker termination via `/queen/ctl`.
- `bind <src> <dst>` – bind a canonical namespace path to a session-scoped mount point via `/queen/ctl`.
- `mount <service> <path>` – mount a named service namespace via `/queen/ctl`.
- `telemetry push <src_file> --device <id>` – request an OS-named segment under `/queen/telemetry/<device_id>/seg/` and append bounded telemetry records using `cohsh-telemetry-push/v1` envelopes (UTF-8, allowlisted extensions only; chunked to `max_record_bytes=4096` and `telemetry_ingest.max_bytes_per_segment`).
- `quit` – prints `closing session` and exits the shell loop.【F:apps/cohsh/src/lib.rs†L1250-L1252】
- Attachments are designed so a single queen session (interactive or scripted) can drive orchestration for many workers without switching tools.

Attachment semantics:
- No role argument → `attach requires a role`.
- Unknown role string → `unknown role '<x>'`.
- More than two args → `attach takes at most two arguments: role and optional ticket`.
- Attempting a second attach without quitting → `already attached; run 'quit' to close the current session`.【F:apps/cohsh/src/lib.rs†L711-L717】

Connection handling (TCP transport):
- Successful connect logs `[cohsh][tcp] connected to <host>:<port> (connects=N)` before presenting the prompt.【F:apps/cohsh/src/transport/tcp.rs†L54-L60】
- Disconnects log `[cohsh][tcp] connection lost: …` and trigger reconnect attempts with incremental back-off, emitting `[cohsh][tcp] reconnect attempt #<n> …`. The shell remains usable in interactive mode; in `--script` mode errors propagate and stop the run.【F:apps/cohsh/src/transport/tcp.rs†L63-L73】

### Acknowledgements and heartbeats
- The root-task event pump emits `OK <VERB> [detail]` or `ERR <VERB> reason=<cause>` for every console command, sharing one dispatcher across serial and TCP so both transports see the same lines before any payload (for example, `OK TAIL path=…` precedes streamed data).【F:apps/root-task/src/event/mod.rs†L1000-L1018】
- `PING` always yields `PONG` without affecting state, keeping automation healthy when idle, while TCP adds a 15-second heartbeat cadence on top of the shared grammar so the client can detect stalls without blocking serial progress.【F:apps/root-task/src/event/mod.rs†L1170-L1183】【F:apps/cohsh/src/transport/tcp.rs†L21-L24】
- Interactive `cohsh` sessions send periodic silent `PING` keepalives while idle to avoid TCP console inactivity timeouts; acknowledgements are drained and not echoed at the prompt.【F:apps/cohsh/src/lib.rs†L1046-L1955】
- `cohsh` parses acknowledgement lines using a shared helper, surfaces details inline with shell output, and preserves the order produced by the root-task dispatcher so scripted `attach`/`tail`/`log` flows match serial transcripts byte-for-byte.【F:apps/cohsh/src/proto.rs†L5-L44】【F:apps/cohsh/src/lib.rs†L1031-L1044】

### Script mode
`--script <file>` feeds newline-delimited commands; blank lines and lines starting with `#` are ignored. Errors abort the script and bubble up as a non-zero exit.【F:apps/cohsh/src/lib.rs†L732-L763】

## coh scripts (.coh)
### Purpose
- `.coh` is a deterministic, line-oriented scripting format for running `cohsh` command sequences non-interactively (including `coh> test` regression suites) using the exact same command handlers as the interactive `coh>` prompt.

### Non-goals
- No general-purpose shell.
- No variables, loops, branching, includes, macros, or dynamic loading.
- No network fetch of scripts at runtime.
- Not intended as a programming language—only a deterministic batch format for `cohsh` commands plus assertions.

### Execution model
- Scripts run against the current `cohsh` session (already connected); the session is expected to be `AUTH`’d and `ATTACH`’d. Scripts (and `coh> test`) may validate session state and fail fast if invalid.
- Each command line executes exactly as if typed at the `coh>` prompt (identical parsing and handlers, no special RPC path).
- Execution is strict: on the first command failure or failed `EXPECT`, stop immediately and return `FAIL`.
- On failure, report the failing line number, the command text, and the last command response line.

### Syntax
- One statement per line; blank lines are ignored.
- `#` starts a comment to end of line.

Two statement families:

1. **Command line**
   - Any line that does not start with `EXPECT` is interpreted as a `cohsh` command exactly as typed at `coh>`.

2. **Assertion line**
   - Assertions apply only to the **last executed command** and evaluate against the **last command response line** (single line as emitted by `cohsh` for that command).
   - `EXPECT OK` — last command response line must begin with `OK`.
   - `EXPECT ERR` — last command response line must begin with `ERR`.
   - `EXPECT SUBSTR <text>` — last command response line must contain `<text>` as a substring (case-sensitive).
   - `EXPECT NOT <text>` — last command response line must not contain `<text>`.

An optional control statement is provided for bounded waits: `WAIT <ms>` pauses locally (does not issue a server command) for the requested duration.

For streaming commands, the “response line” is the initial acknowledgement line (`OK …` or `ERR …` that starts the stream), not any subsequent streamed payload lines.

### Determinism & bounds
- Max script lines: 256; longer scripts are rejected.
- Max execution time: bounded by `test --timeout`; scripts must not block indefinitely.
- Explicit waiting is allowed via `WAIT <ms>` (line statement), capped at 2000 ms; longer waits are rejected.

### Preinstalled self-test scripts
`coh> test` reads `.coh` scripts from `/proc/tests/`:
- `/proc/tests/selftest_quick.coh`
- `/proc/tests/selftest_full.coh`
- `/proc/tests/selftest_negative.coh`
- Operators must rerun this suite whenever console handling, Secure9P transport, namespace structure, or access policies change.

### `coh> test` JSON schema
When invoked with `--json`, `coh> test` emits:
```
{
  "ok": true,
  "mode": "quick",
  "elapsed_ms": 123,
  "checks": [
    {"name": "preflight/ping", "ok": true, "detail": "OK ping"},
    {"name": "line 4: cat /proc/boot", "ok": true, "detail": "OK"}
  ],
  "version": "1"
}
```

### Security posture
- Scripts do not grant privileges: all actions remain subject to the session’s attached role/ticket and server-side access policy; scripts only automate what an operator could type interactively.

### Examples
Quick check (ping, proc read, and an expected error):
```
# connectivity and auth sanity
ping
EXPECT OK
cat /proc/queen/state
EXPECT OK
echo forbidden > /queen/ctl
EXPECT ERR
```

Disposable worker lifecycle with ID assertion:
```
spawn gpu gpu_id=GPU-0 mem_mb=4096 streams=1 ttl_s=60
EXPECT OK
ls /shard
EXPECT OK
EXPECT SUBSTR path=/shard
tail /shard/<label>/worker/worker-123/telemetry
EXPECT OK
WAIT 500
kill worker-123
EXPECT OK
EXPECT NOT ERR
```

## End-to-End Workflow: QEMU + `cohsh` over TCP
This section covers the development harness for running Cohesix on QEMU; production deployments target physical ARM64 hardware booted via UEFI with equivalent console and `cohsh` semantics.
### Terminal 1 – build and boot under QEMU
Run the build wrapper to compile components, stage host tools, and launch QEMU with PL011 serial plus a user-mode TCP forward to `127.0.0.1:<port>`:
```
SEL4_BUILD_DIR=$HOME/seL4/build ./scripts/cohesix-build-run.sh \
  --sel4-build "$HOME/seL4/build" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features cohesix-dev \
  --cargo-target aarch64-unknown-none \
  --transport tcp
```
The script builds `root-task` with the serial and TCP console features, compiles NineDoor and workers, copies host tools (`cohsh`, `gpu-bridge-host`, `host-sidecar-bridge`) into `out/cohesix/host-tools/`, and assembles the CPIO payload.【F:scripts/cohesix-build-run.sh†L369-L454】【F:scripts/cohesix-build-run.sh†L402-L442】
QEMU runs with `-serial mon:stdio` and a user-net device that forwards TCP/UDP ports 31337–31339 into the guest so the TCP console and self-tests are reachable from the host.【F:scripts/cohesix-build-run.sh†L518-L553】 The wrapper selects the NIC backend from the root-task features: `dev-virt` (via `cohesix-dev`) uses virtio-net by default, which adds `-global virtio-mmio.force-legacy=false` for the modern header; removing `net-backend-virtio` switches the wrapper to RTL8139 instead.【F:scripts/cohesix-build-run.sh†L518-L553】 The script prints the ready command for `cohsh` once QEMU is live.【F:scripts/cohesix-build-run.sh†L546-L553】 In deployment, the same console and `cohsh` flows apply to UEFI-booted ARM64 hardware without the VM wrapper.

### Virtio-MMIO modes (when `net-backend-virtio` is enabled)
- **Modern v2 (default for virtio)**: no extra flags are required; the build wrapper forces `virtio-mmio.force-legacy=false` so QEMU exposes the modern header and the driver accepts it by default.【F:scripts/cohesix-build-run.sh†L518-L544】【F:apps/root-task/src/drivers/virtio/net.rs†L118-L157】 Use the host forwards above to reach the TCP console (31337), UDP echo self-test (31338), and TCP smoke test (31339).
- **Legacy v1 (only for debugging)**: export `VIRTIO_MMIO_FORCE_LEGACY=1` before invoking the script **and** rebuild with `--features virtio-mmio-legacy`. The wrapper will switch QEMU to `-global virtio-mmio.force-legacy=true`; the driver will reject v1 unless the feature gate is enabled.【F:scripts/cohesix-build-run.sh†L518-L544】【F:apps/root-task/src/drivers/virtio/net.rs†L1379-L1411】 When debugging legacy, prefer bumping QEMU back to modern instead of carrying the feature in normal builds.

### Verify the modern TCP path quickly
- Start QEMU with the default `--transport tcp` flow above (virtio-net backend).
- From the host, attach to the TCP console via `./cohsh --transport tcp --tcp-port 31337`.
- Observe forwarded packets (helpful on macOS `lo0`): `sudo tcpdump -i lo0 -n tcp port 31337 or udp port 31338 or tcp port 31339`.
- For smoke testing, send UDP to 31338 or TCP to 31339 and confirm traffic crosses the hostfwd path.

### Terminal 2 – host `cohsh` session over TCP
From `out/cohesix/host-tools/`:
```
./cohsh --transport tcp --tcp-port 31337
Welcome to Cohesix. Type 'help' for commands.
detached shell: run 'attach <role>' to connect
coh> attach queen
[console] OK ATTACH role=Queen session=1
attached session SessionId(1) as Queen
coh>
```
Use `log` to stream `/log/queen.log`, `ping` for health, and `tail <path>` for ad-hoc inspection. If the TCP session resets, `cohsh` reports the error and continues in a detached state; reconnects are attempted automatically with back-off in interactive mode.【F:apps/cohsh/src/transport/tcp.rs†L54-L73】

## Scripted Sessions with `--script`
Example script (`queen.coh`):
```
# Attach and tail the queen log
attach queen
log
quit
```
Run via `./cohsh --transport tcp --tcp-port 31337 --script queen.coh`. The runner stops on the first error (including connection failures) and propagates the error code to the host shell.【F:apps/cohsh/src/lib.rs†L732-L763】
Use `./cohsh --check <script.coh>` to validate `.coh` syntax without executing commands.【F:apps/cohsh/src/main.rs†L28-L138】

## GUI clients
- A host-side WASM GUI is planned as a hive dashboard. It will speak the same console/NineDoor protocol as `cohsh` (no new verbs, no new in-VM endpoints) and focuses on presentation and workflow rather than new privileges.

## Debugging TCP Console Issues
- **Connection refused / wrong port**: confirm QEMU launched with `--transport tcp` and the `hostfwd` rule; the build script prints the expected port.【F:scripts/cohesix-build-run.sh†L521-L553】
- **Connection reset by peer**: `cohsh` logs the reset and reconnect attempts. Re-run `attach <role>` once the console listener is reachable.【F:apps/cohsh/src/transport/tcp.rs†L63-L73】
- **Authentication failures**: ensure the `--auth-token` (or `COHSH_AUTH_TOKEN`) matches the listener requirement; the TCP transport defaults to `changeme`.【F:apps/cohsh/src/main.rs†L78-L115】
- **Serial vs TCP differences**: the root console is independent of the TCP listener—verify liveness with `ping` on the serial console (`cohesix>`) to isolate network issues.【F:apps/root-task/src/console/mod.rs†L214-L320】

## Future Root Console Extensions (ideas)
Not implemented yet, but likely additions for debugging:
- `net` – report virtio-net status and console listener port.
- `tcp` – list active TCP console sessions and counters.
- `9p` – basic NineDoor state (session counts, outstanding requests).
- `trace` – toggle trace categories for boot/net/9p.
Any future commands must remain deterministic, no_std-friendly, and will be documented here when they land.

## References & Cross-links
- Architecture and role model: `docs/ARCHITECTURE.md`.
- Protocol/schema details: `docs/INTERFACES.md` (once stabilised).
- This document stays focused on operator-facing workflows and real behaviours for the root console and `cohsh` CLI.
