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

### CLI flags (current)
Key options from `--help`:
- `--role <role>` and `--ticket <ticket>` to auto-attach on startup.
- `--script <file>` to execute commands non-interactively.
- `--transport <mock|qemu|tcp>` to choose backend; TCP exposes `--tcp-host` / `--tcp-port` (defaults `127.0.0.1:31337`).【F:apps/cohsh/src/main.rs†L44-L132】
- QEMU helpers: `--qemu-bin`, `--qemu-out-dir`, `--qemu-gic-version`, `--qemu-arg` (dev/CI convenience).【F:apps/cohsh/src/main.rs†L52-L131】
- `--auth-token` forwards the TCP console authentication secret; defaults to `changeme`.【F:apps/cohsh/src/main.rs†L78-L115】

### Interactive shell surface
Startup banner and prompt:
```
Welcome to Cohesix. Type 'help' for commands.
detached shell: run 'attach <role>' to connect
coh>
```

Commands and status:
- `help` – show the command list.【F:apps/cohsh/src/lib.rs†L663-L699】
- `attach <role> [ticket]` / `login` – attach to a NineDoor session. Valid roles today: `queen`, `worker-heartbeat`; missing roles, unknown roles, too many args, or re-attaching emit errors via the parser and shell.【F:apps/cohsh/src/lib.rs†L572-L592】【F:apps/cohsh/src/lib.rs†L785-L801】
- `tail <path>` – stream a file; `log` tails `/log/queen.log`. Requires attachment.【F:apps/cohsh/src/lib.rs†L706-L716】
- `ping` – reports attachment status; errors when detached or when given arguments.【F:apps/cohsh/src/lib.rs†L717-L730】
- `echo <text> > <path>` – append a newline-terminated payload to an absolute path via NineDoor.【F:apps/cohsh/src/lib.rs†L732-L740】【F:apps/cohsh/src/lib.rs†L803-L817】
- Planned (not implemented): `ls`, `cat`, `spawn`, `kill`, `bind`, `mount`; the shell prints explicit “planned” errors today.【F:apps/cohsh/src/lib.rs†L700-L705】
- `quit` – prints `closing session` and exits the shell loop.【F:apps/cohsh/src/lib.rs†L697-L699】
- Attachments are designed so a single queen session (interactive or scripted) can drive orchestration for many workers without switching tools.

Attachment semantics:
- No role argument → `attach requires a role`.
- Unknown role string → `unknown role '<x>'`.
- More than two args → `attach takes at most two arguments: role and optional ticket`.
- Attempting a second attach without quitting → `already attached; run 'quit' to close the current session`.【F:apps/cohsh/src/lib.rs†L572-L592】【F:apps/cohsh/src/lib.rs†L785-L801】

Connection handling (TCP transport):
- Successful connect logs `[cohsh][tcp] connected to <host>:<port> (connects=N)` before presenting the prompt.【F:apps/cohsh/src/transport/tcp.rs†L43-L50】
- Disconnects log `[cohsh][tcp] connection lost: …` and trigger reconnect attempts with incremental back-off, emitting `[cohsh][tcp] reconnect attempt #<n> …`. The shell remains usable in interactive mode; in `--script` mode errors propagate and stop the run.【F:apps/cohsh/src/transport/tcp.rs†L52-L63】

### Acknowledgements and heartbeats
- The root-task event pump emits `OK <VERB> [detail]` or `ERR <VERB> reason=<cause>` for every console command, sharing one dispatcher across serial and TCP so both transports see the same lines before any payload (for example, `OK TAIL path=…` precedes streamed data).【F:apps/root-task/src/event/mod.rs†L743-L769】
- `PING` always yields `PONG` without affecting state, keeping automation healthy when idle, while TCP adds a 15-second heartbeat cadence on top of the shared grammar so the client can detect stalls without blocking serial progress.【F:apps/root-task/src/event/mod.rs†L790-L799】【F:apps/cohsh/src/transport/tcp.rs†L19-L30】
- `cohsh` parses acknowledgement lines using a shared helper, surfaces details inline with shell output, and preserves the order produced by the root-task dispatcher so scripted `attach`/`tail`/`log` flows match serial transcripts byte-for-byte.【F:apps/cohsh/src/proto.rs†L5-L44】【F:apps/cohsh/src/lib.rs†L581-L605】

### Script mode
`--script <file>` feeds newline-delimited commands; blank lines and lines starting with `#` are ignored. Errors abort the script and bubble up as a non-zero exit.【F:apps/cohsh/src/lib.rs†L594-L605】

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
spawn gpu ttl_s=60 streams=1
EXPECT OK
EXPECT SUBSTR worker-
tail /worker/last/telemetry
EXPECT OK
WAIT 500
kill last
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
  --root-task-features kernel,bootstrap-trace,serial-console,net,net-console \
  --cargo-target aarch64-unknown-none \
  --transport tcp
```
The script builds `root-task` with the serial and TCP console features, compiles NineDoor and workers, copies host tools (`cohsh`, `gpu-bridge-host`) into `out/cohesix/host-tools/`, and assembles the CPIO payload.【F:scripts/cohesix-build-run.sh†L369-L454】【F:scripts/cohesix-build-run.sh†L402-L442】
QEMU runs with `-serial mon:stdio`, `-global virtio-mmio.force-legacy=false`, and a user-net device that forwards TCP/UDP ports 31337–31339 into the guest so the TCP console and self-tests are reachable from the host.【F:scripts/cohesix-build-run.sh†L518-L553】 The script prints the ready command for `cohsh` once QEMU is live.【F:scripts/cohesix-build-run.sh†L546-L553】 In deployment, the same console and `cohsh` flows apply to UEFI-booted ARM64 hardware without the VM wrapper.

### Virtio-MMIO modes (modern by default)
- **Modern v2 (default)**: no extra flags are required; the build wrapper forces `virtio-mmio.force-legacy=false` so QEMU exposes the modern header and the driver accepts it by default.【F:scripts/cohesix-build-run.sh†L518-L544】【F:apps/root-task/src/drivers/virtio/net.rs†L118-L157】 Use the host forwards above to reach the TCP console (31337), UDP echo self-test (31338), and TCP smoke test (31339).
- **Legacy v1 (only for debugging)**: export `VIRTIO_MMIO_FORCE_LEGACY=1` before invoking the script **and** rebuild with `--features virtio-mmio-legacy`. The wrapper will switch QEMU to `-global virtio-mmio.force-legacy=true`; the driver will reject v1 unless the feature gate is enabled.【F:scripts/cohesix-build-run.sh†L518-L544】【F:apps/root-task/src/drivers/virtio/net.rs†L1379-L1411】 When debugging legacy, prefer bumping QEMU back to modern instead of carrying the feature in normal builds.

### Verify the modern TCP path quickly
- Start QEMU with the default `--transport tcp` flow above.
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
Use `log` to stream `/log/queen.log`, `ping` for health, and `tail <path>` for ad-hoc inspection. If the TCP session resets, `cohsh` reports the error and continues in a detached state; reconnects are attempted automatically with back-off in interactive mode.【F:apps/cohsh/src/transport/tcp.rs†L43-L63】

## Scripted Sessions with `--script`
Example script (`queen.coh`):
```
# Attach and tail the queen log
attach queen
log
quit
```
Run via `./cohsh --transport tcp --tcp-port 31337 --script queen.coh`. The runner stops on the first error (including connection failures) and propagates the error code to the host shell.【F:apps/cohsh/src/lib.rs†L594-L605】

## GUI clients
- A host-side WASM GUI is planned as a hive dashboard. It will speak the same console/NineDoor protocol as `cohsh` (no new verbs, no new in-VM endpoints) and focuses on presentation and workflow rather than new privileges.

## Debugging TCP Console Issues
- **Connection refused / wrong port**: confirm QEMU launched with `--transport tcp` and the `hostfwd` rule; the build script prints the expected port.【F:scripts/cohesix-build-run.sh†L521-L553】
- **Connection reset by peer**: `cohsh` logs the reset and reconnect attempts. Re-run `attach <role>` once the console listener is reachable.【F:apps/cohsh/src/transport/tcp.rs†L43-L63】
- **Authentication failures**: ensure the `--auth-token` (or `COHSH_AUTH_TOKEN`) matches the listener requirement; the TCP transport defaults to `changeme`.【F:apps/cohsh/src/main.rs†L78-L115】
- **Serial vs TCP differences**: the root console is independent of the TCP listener—verify liveness with `ping` on the serial console (`cohesix>`) to isolate network issues.【F:apps/root-task/src/console/mod.rs†L224-L299】

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
