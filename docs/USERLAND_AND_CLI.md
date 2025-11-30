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

### Script mode
`--script <file>` feeds newline-delimited commands; blank lines and lines starting with `#` are ignored. Errors abort the script and bubble up as a non-zero exit.【F:apps/cohsh/src/lib.rs†L594-L605】

## End-to-End Workflow: QEMU + `cohsh` over TCP
This section covers the development harness for running Cohesix on QEMU; production deployments target physical ARM64 hardware booted via UEFI with equivalent console and `cohsh` semantics.
### Terminal 1 – build and boot under QEMU
Run the build wrapper to compile components, stage host tools, and launch QEMU with PL011 serial plus a user-mode TCP forward to `127.0.0.1:<port>`:
```
SEL4_BUILD_DIR=$HOME/seL4/build ./scripts/cohesix-build-run.sh \
  --sel4-build "$HOME/seL4/build" \
  --out-dir out/cohesix \
  --profile release \
  --root-task-features kernel,bootstrap-trace,serial-console,net \
  --cargo-target aarch64-unknown-none \
  --transport tcp
```
The script builds `root-task` with the serial console and net features, compiles NineDoor and workers, copies host tools (`cohsh`, `gpu-bridge-host`) into `out/cohesix/host-tools/`, and assembles the CPIO payload.【F:scripts/cohesix-build-run.sh†L369-L454】【F:scripts/cohesix-build-run.sh†L402-L442】
QEMU runs with `-serial mon:stdio` plus `-netdev user,id=net0,hostfwd=tcp:127.0.0.1:<port>-10.0.2.15:<port>` so the TCP console inside the development VM is reachable from the host.【F:scripts/cohesix-build-run.sh†L521-L553】 The script prints the ready command for `cohsh` once QEMU is live.【F:scripts/cohesix-build-run.sh†L548-L553】 In deployment, the same console and `cohsh` flows apply to UEFI-booted ARM64 hardware without the VM wrapper.

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
