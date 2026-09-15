<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Guide Queen setup, serial diagnostics, cohsh operation and checked automation using the implemented console contracts. -->
<!-- Author: Lukas Bower -->

# Cohesix serial console and cohsh — user guide

Use the **serial console** to bring up a Queen, check its hardware and network,
and recover when a host connection is unavailable. Use **`cohsh` on your Mac or
Linux host** to inspect the Queen, retain logs, submit authorised work, upload
telemetry and run repeatable checks.

**Setting up a Queen?** Start with [Bring up and check the Queen](#bring-up-and-check-the-queen),
then [Connect cohsh](#connect-cohsh) and [Test the connection](#test-the-connection).
**Already connected?** Go to [Do useful work](#do-useful-work).

This guide describes the current source interfaces, not a promise that an older
release implements every option. Keep the target image, host binaries and
selected generated configuration together. Check the running image's `help`
and the installed binary's `--help`. Historical fixture credentials are not
valid deployment credentials; see [release migration](M27A_AUTHORITY.md#release-migration-and-historical-errata).

## Find what you need

| Task | Go to |
| --- | --- |
| Open serial, choose network settings and check startup | [Queen bring-up](#bring-up-and-check-the-queen) |
| Connect directly or through an existing gateway | [Connect cohsh](#connect-cohsh) |
| Check health without intentionally changing workloads | [Connection checks](#test-the-connection) |
| Inspect state, save logs or append an operator note | [State and logs](#inspect-state-and-save-logs) |
| Upload a local telemetry file | [Telemetry upload](#upload-a-telemetry-file) |
| Start, observe and stop a Worker | [Worker requests](#submit-and-observe-worker-requests) |
| Make a production Queen control request safely | [Strict intents](#submit-a-production-queen-intent) |
| Cordon, drain or resume a Queen | [Lifecycle control](#control-the-queen-lifecycle) |
| Validate and run a command batch | [Scripts](#coh-scripts) |
| Look up a command, option or failure | [Command reference](#command-reference), [CLI options](#cli-options-and-credentials), [Troubleshooting](#troubleshooting) |
| Interpret low-level Pi counters | [Advanced diagnostic reference](#advanced-diagnostic-reference) |

## Know which prompt you are using

| Where you type | What it is | Example |
| --- | --- | --- |
| Your host terminal, before launching `cohsh` | Bash, zsh or another host shell | `"$COH_BIN/cohsh" --transport tcp ...` |
| `Cohesix boot menu` or the advanced U-Boot shell | Pi bootloader, before Cohesix starts | Choose the network and boot option |
| `cohesix>` | The target's root console, through serial or an admitted local USB keyboard | `netstats` |
| `coh>` | The host-side `cohsh` application | `cat /proc/lifecycle/state` |

Commands below omit the prompt so they can be copied. Each procedure says where
to enter them. **Bash examples run on the host**, from the installation or
repository root. Shell variables such as `COH_BIN` are conveniences for these
examples; they do not expand inside `cohsh` or `.coh` files. Set `COH_BIN` and
the relevant connection variables again in each new host terminal.

Neither Cohesix console is a POSIX shell. There is no `cd`, `sudo`, package
manager, shell pipeline, command substitution or general-purpose program
launcher at `cohesix>` or `coh>`. Namespace paths are absolute. The host's
`out/operator/queen-log.txt` and the Queen's `/log/queen.log` are different
files on different machines.

## Bring up and check the Queen

### 1. Prepare the target and capture serial

Start with an image built and staged for the actual target. Image construction,
SD-card flashing, readback and boot identity are covered by
[Hardware bring-up](HARDWARE_BRINGUP.md). Do not reflash a working installation
merely to change its network settings.

For a Pi, connect the serial adapter described by that hardware setup and
identify its actual host device. Only one terminal or capture program may own
that device. On **the Mac host**, this example captures a new boot:

```bash
RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)"
SERIAL_DEVICE="/dev/cu.usbserial-0001"  # Replace with your verified device.
EVIDENCE_DIR="$PWD/out/pi4-proof/$RUN_ID"
umask 077
mkdir -p "$EVIDENCE_DIR"
minicom -D "$SERIAL_DEVICE" -b 115200 -o \
  -C "$EVIDENCE_DIR/pi4-serial.log"
```

Use 115200 baud, 8 data bits, no parity, one stop bit and no flow control.
Start capture before powering on when retaining boot evidence. Do not open
another serial client while minicom is running. Logs and packet captures can
contain sensitive deployment information; review them before sharing.

For an already running QEMU Queen, use the serial console in its launcher
terminal. Host TCP normally connects to the launcher's forwarded address and
port, rather than the guest's internal address. The separate `cohsh --transport
qemu` mode is a diagnostic boot/log transport, not the normal live shell.

### 2. Choose the Pi's network settings

The staged Pi image stops at the **Cohesix boot menu**. Its first option says
whether saved settings or the selected manifest's defaults are active.

For first setup, choose **Change network settings**, then **Automatic (DHCP)**
or **Manual (static IPv4)** and **Ethernet (wired)** or **Wi-Fi (wireless)**.
Manual setup collects the address, subnet prefix length and optional gateway.
Review the result, then choose **Boot once without saving** for a temporary
check or **Save settings and restart** to retain it. After restart, select the
boot option with the saved settings.

The default image uses **USB keyboard input for the boot menu**. A visible menu
on serial does not mean serial keystrokes are its selected input. Wi-Fi name
and password entry requires the local USB keyboard and HDMI display; the
bootloader deliberately refuses serial-only password entry. Do not type a PSK
into minicom, a captured command or shell history. When local input is
unavailable, follow the private `cohesix.env` editing procedure in
[First-boot network policy](HARDWARE_BRINGUP.md#4-set-first-boot-network-policy).

Network choices do not provision console tokens or ticket signing keys. Those
must match the selected image's deployment configuration. Do not reset saved
settings as a first response to an authentication error.

### 3. Check the root console

Wait for `cohesix>`. On **the target serial console**, enter these commands
**one at a time**, waiting for each response to finish before sending the next:

```text
help
ping
bi
caps
mem
smp
netstats
```

| Check | What to look for | What it does not establish |
| --- | --- | --- |
| `help`, `ping` | A complete response and a responsive console | USB keyboard or TCP readiness |
| `bi` | BootInfo and generated-profile identity appropriate to this image | Media readback or another boot's identity |
| `caps`, `mem` | Bounded capability and memory summaries, without a reported fault | A stress-test pass |
| `smp` | Userspace activity and assigned runtime/driver observations | Kernel CPU utilisation or performance acceptance |
| `netstats` | The selected active driver, address/listener state and current counters | Successful host authentication |

Record the **actual address and port** reported for this boot. The examples
below use `192.168.10.50:31337`; change them to match your Queen. In Wi-Fi mode,
`profile_backend=bcmgenet-v5` with `active_driver=cyw43` is not by itself a
mismatch: one describes the profile and the other the selected physical path.

`netstats` is diagnostic, not a non-interfering performance sampler. The Pi
direct-GENET implementation can perform a bounded causal owner refresh. Do not
mix repeated diagnostic commands into a timed benchmark.

A serial prompt does not prove the local USB/HDMI seat is ready. The Pi HDMI
prompt is withheld until keyboard command admission and display readiness are
established. For a local-seat check, type a real command on the **USB keyboard**
and confirm its echo and response. Use `usb status` from serial to inspect the
result; startup history alone is not current input proof.

### 4. Escalate only the relevant diagnostic

| Symptom | Commands at `cohesix>` | Interpretation |
| --- | --- | --- |
| Keyboard or display not ready | `usb status`, then `usb diag` | Separate USB input progress from HDMI completion; inspect the reported blocker. |
| Wi-Fi did not become usable | `wifi diag`, then `wifi dump-state` | Start with the first failing gate. Later `not-reached` gates are not independent failures. |
| Runtime assignment or scheduling looks wrong | `caps mcs`, `smp mcs` | Distinguish generated assignments, kernel facts and copied runtime observations. |
| A TCP session stalls | `netstats`, `smp poll-time` on a supported Pi image | Retain its generation/connection before a new connection replaces the snapshot. |
| Need recent cache operations | `cachelog 9` | A bounded diagnostic snapshot, not a health verdict. |

These are profile-gated commands. `usb enable-kbd` and `usb probe-kbd` are
**active** operations, not passive inspection. Legacy `wifi retry`,
`wifi load-fw` and `wifi probe-ht` return an ownership refusal on the linked
runtime; they are not recovery procedures. Use the reported blocker and
[Failure modes](FAILURE_MODES.md), rather than repeatedly issuing them.

## Connect cohsh

### Select the matching host binary

From an extracted **Mac or Linux host bundle root**:

```bash
./scripts/setup_environment.sh --check
export COH_BIN="$PWD/bin"
"$COH_BIN/cohsh" --help
```

For a new installation, run `./scripts/setup_environment.sh` first as described
in [Quickstart](QUICKSTART.md); it can install host dependencies. A Pi image
bundle alone does not provide your Mac's host executables.

From an **already built source checkout root**, use this instead:

```bash
export COH_BIN="$PWD/out/cohesix/host-tools"
"$COH_BIN/cohsh" --help
```

Source developers can substitute `cargo run -p cohsh --` for
`"$COH_BIN/cohsh"`. That builds the current Cargo feature selection; it does not
make an old target match a new host binary. Use the selected generated policy,
and remain in the installation root so relative configuration and local file
paths resolve correctly.

### Direct TCP: one shell owns the connection

Use this when operating the Queen with one host process. Quit a direct SwarmUI,
bridge, mount or other shell first. Do not leave a gateway connected while
starting a competing direct client. Serial remains an independent operator
surface and can stay open.

Obtain the console credential provisioned for this image. In **host Bash**,
select its existing private file and the address verified above:

```bash
export COH_AUTH_TOKEN_REF="file:$HOME/.config/cohesix/queen-console.token"
export COH_TARGET_HOST="192.168.10.50"
export COH_TARGET_PORT="31337"
unset COHSH_TCP_HOST COHSH_TCP_PORT

"$COH_BIN/cohsh" --transport tcp \
  --tcp-host "$COH_TARGET_HOST" --tcp-port "$COH_TARGET_PORT" \
  --role queen
```

The credential file must already contain the deployment token; setting its
path does not create or discover a token. Restrict access to the file. An
`env:NAME` reference is also supported. Do not substitute a published fixture,
`bootstrap` or `changeme`.

For a local QEMU instance, set `COH_TARGET_HOST=127.0.0.1` and use its forwarded
port. The explicit `unset` avoids inherited `COHSH_TCP_*` overrides; see
[Credential and environment precedence](#credential-and-environment-precedence).

A successful start reports an attached Queen session and displays `coh>`.
Interactive auto-attachment also attempts a bounded Queen log tail. The prompt
alone is not proof of attachment: a failed connection can leave a detached
shell. Run `ping` to confirm.

Without `--role`, the shell starts detached. At `coh>`, use `attach queen`.
To change attachment, use `detach`, then `attach <role> [ticket]`. `detach`
closes the current transport session but keeps the shell open; `quit` exits.
Worker attachments require a valid role-matching ticket with a subject.

### REST: use the gateway that already owns TCP

For simultaneous shell, UI and automation work, start one `hive-gateway` using
[Host tools: connect and verify](HOST_TOOLS.md#connect-and-verify). Select its
Pi or QEMU runtime profile correctly. **The gateway, not `cohsh`, owns the
Queen's TCP connection.**

In another **host terminal**, set `COH_BIN` again and connect:

```bash
export COH_REST_URL="http://127.0.0.1:8080"
"$COH_BIN/cohsh" --transport rest --rest-url "$COH_REST_URL" --role queen
```

Read operations inherit the gateway's upstream scope. REST writes additionally
need both the gateway request-authentication token and a delegated caller
ticket. Load their values privately before starting a write-capable shell:

```bash
: "${COH_REST_AUTH_TOKEN:?load the gateway request-authentication token}"
: "${COH_REST_TICKET:?load a valid delegated caller ticket}"
export COH_REST_AUTH_TOKEN COH_REST_TICKET
"$COH_BIN/cohsh" --transport rest --rest-url "$COH_REST_URL" --role queen
```

The ticket needs an unexpired lifetime, subject and write scope covering the
actual destination. Permission to write `/log` does not permit a production
intent at `/queen/intents/ctl`. The gateway verifies the caller ticket and
intersects it with its upstream authority; `--role queen` does not grant
unrestricted access. The current REST attachment selector accepts only Queen.

Keep gateway HTTP on loopback or behind an approved authenticated tunnel/TLS
boundary. Neither this console transport nor the gateway supplies TLS itself.
On a Jetson, `127.0.0.1` refers to that Jetson, not your Mac.

## Test the connection

### Start with a small read-only check

At **`coh>`**, run:

```text
ping
ls /
ls /proc
cat /proc/lifecycle/state
cat /proc/authority
tail /log/queen.log 16
```

Confirm that every operation completes, the namespace is readable under the
intended role, and the lifecycle and authority fields match your deployment.
Read the returned state; an `OK CAT` acknowledgement does not mean the value
was `ONLINE`. A policy refusal is different from an unavailable transport.

If an optional path is absent, inspect the selected profile and source version
rather than treating it as an empty value. Do not continue into mutation tests
until attachment, lifecycle and policy are understood.

### Run and finish the network self-test

Use **direct TCP `cohsh`** for the authenticated network exercise. At `coh>`:

```text
nettest
```

Record the positive `run_generation` from
`OK NETTEST detail=started run_generation=...`. That line means **started**, not
passed. Keep the same connection open, allow the bounded 15-second test window
to finish, exercise it with `ping`, then read `netstats`:

```text
ping
netstats
```

Require a terminal verdict for that **same run generation**, with
`running=false`. `pass` and `peer-assisted-pass` identify different supported
test paths; read their component fields. `running`, `none`, a failed verdict
or a different run is not a pass. With an isolated console-network child,
peer-assisted success depends on post-admission response drain, TX and later
RX/TCP progress for the matching authenticated connection. Its UDP echo field
can truthfully be false. ICMP reachability is a separate check.

Do not translate this wait into `WAIT 15000` in a `.coh` file: its local wait
limit is 2000 ms. A serial-only `nettest` admission cannot substitute for the
required authenticated peer interaction.

### Run the installed self-tests deliberately

First inspect what this image publishes at **`coh>`**:

```text
ls /proc/tests
cat /proc/tests/selftest_negative.coh
cat /proc/tests/selftest_quick.coh
```

Then run the reduced-mutation quick check:

```text
test --mode quick --no-mutate
```

**`--no-mutate` is not a universal read-only sandbox.** It skips `spawn`, `kill`
and Worker telemetry tails in the installed scripts, plus their associated
assertions. It still runs negative tests, including attempted writes that are
expected to be refused. Review installed scripts before using this mode on a
working deployment. Use the earlier explicit read-only commands when even
attempted writes are inappropriate.

| Mode | Selected script, after `selftest_negative.coh` | Use |
| --- | --- | --- |
| `quick` (default) | `/proc/tests/selftest_quick.coh` | Small installed regression sequence |
| `full` | `/proc/tests/selftest_full.coh` | Broader sequence; inspect mutations first |
| `smp` | `/proc/tests/selftest_smp.coh` | Installed SMP-specific checks |

For an authorised test environment, examples are `test --mode full --timeout
120` and `test --mode smp --no-mutate`. The default timeout is 30 seconds;
accepted values are 1–120 seconds. A timeout is a failed check, not a reason to
raise every retry setting.

Human output reports `selftest PASS` or `selftest FAIL` and the first failed
check. `--json` emits a single-line report object with `ok`, `mode`,
`elapsed_ms`, `checks` and schema `version`. Other shell output can surround
that JSON line; do not treat the whole stdout stream as one JSON document.
Automation must require `ok=true` and a successful script exit.

Internal scripts may close their session. `cohsh` attempts to restore the
previous attachment afterwards, preserving outer script response bookkeeping;
a failed restoration is reported. Verify attachment before continuing work.

These are operational checks, **not release acceptance**. A QEMU result does
not qualify a physical Pi, and mock success qualifies neither. For milestone
or release claims use the staged, provenance-bound [Test plan](TEST_PLAN.md).

## Do useful work

### Inspect state and save logs

At **`coh>`**, browse before assuming a path or object exists:

```text
ls /queen
ls /shard
cat /proc/lifecycle/state
cat /proc/authority
log
tail /log/queen.log 16
cat /log/queen.log
```

`log` is a convenience for the newest 64 retained Queen log records.
`tail <path> [lines]` returns a **finite snapshot**, not `tail -f`; explicit
counts are 1–256. Reissue a bounded tail when needed. `cat /log/queen.log`
also includes the retained trusted boot-audit reserve in sequence order.
Neither operation promises logs that have already been evicted.

To save the retained log, create a directory in **host Bash** before starting
the shell:

```bash
mkdir -p out/operator
```

Then, at **`coh>`**:

```text
log dump out/operator/queen-log.txt
```

This creates a **local `.txt` file**, containing log payload rather than wire
`OK`/`END` framing. The parent directory must exist, and an existing destination
is refused. Use a new filename for each capture. `log dump
out/operator/queen-log.txt --force` explicitly replaces an existing capture.
A failed or incomplete target read is not a complete log export.

### Append an operator note

This intentionally changes the Queen log. Use a Queen attachment with the
necessary write authority; through REST, also supply a ticket scoped to `/log`.
At **`coh>`**:

```text
echo operator-check: serial-and-host-readback-complete > /log/queen.log
tail /log/queen.log 4
```

Confirm the note is present in returned data. The `>` is `cohsh` command syntax,
not host redirection, and the operation **appends** rather than truncates.
One outer pair of quotes may surround the text, but this is not shell quoting:
there is no variable expansion, and a literal `>` inside the payload is not
supported by this parser. Newline, carriage return and NUL payloads are refused.

The equivalent **serial-console** write is path-first:

```text
echo /log/queen.log operator-check: serial-note
```

It remains subject to the physical console's role and write policy. Do not
paste the `cohsh` spelling into serial or the serial spelling into `cohsh`.

### Upload a telemetry file

Use this to retain a bounded host observation under a device identifier, not to
copy arbitrary files into a general-purpose target filesystem. In **host Bash**,
create a small example file (explicitly labelled as an operator observation):

```bash
mkdir -p out/operator
printf '%s\n' '{"source":"operator","event":"connectivity-check-complete"}' \
  > out/operator/queen-check.ndjson
```

At **`coh>`**, using authorised telemetry write access:

```text
telemetry push out/operator/queen-check.ndjson --device operator-check
ls /queen/telemetry/operator-check/seg
```

The acknowledgement reports `seg_id`, record count, encoded bytes, original
source bytes and `mode=inline|reference`. Read the segment named in that
acknowledgement. For example, **only when the returned ID is `seg-000001`**:

```text
cat /queen/telemetry/operator-check/seg/seg-000001
```

The source must be non-empty. Supported extensions are `.txt` and `.log`
(`text/plain`), `.json` (`application/json`), `.ndjson`
(`application/x-ndjson`) and `.csv` (`text/csv`). Local paths in these commands
must not contain whitespace; `cohsh` is not a general shell argument parser.

Small, bounded UTF-8 content uses inline `cohsh-telemetry-push/v1` records.
Binary input or content exceeding the inline envelope budget uses
`coh-ref-c/v1` chunk references. **Reference mode uploads offsets, lengths and
hashes, not the referenced file bytes.** Retain the unchanged source file under
your content-retention policy. The generated limits also cap reference size,
entry count and per-device retention; repeated uploads can evict older segments.

### Submit and observe Worker requests

A Worker request manages a declared Cohesix control-plane task. It does not
install Linux, launch an arbitrary application or execute CUDA inside the Queen.
GPU discovery, actual GPU programs and PEFT tooling remain host-side; follow
[Host tools](HOST_TOOLS.md) for those workflows.

First inspect `/proc/authority` and `/proc/lifecycle/state`. The convenience
commands below write **compatibility `/queen/ctl`**. They do not automatically
construct a strict intent. Production profiles that disable the compatibility
path require [the next procedure](#submit-a-production-queen-intent) instead.

In a compatibility-enabled test deployment, at **`coh>`**:

```text
spawn heartbeat ticks=100 ttl_s=120 ops=500
ls /shard
```

Read the returned control outcome and discover the published Worker through
its shard directory. **Do not assume the next Worker has a particular ID.**
For a concrete illustration, when the selected eight-bit shard layout publishes
`worker-1`, its canonical shard is `13`:

```text
ls /shard/13/worker
cat /shard/13/worker/worker-1/telemetry
```

Substitute the actual published label and ID in your session. The canonical
shape is `/shard/<label>/worker/<id>/telemetry`. `/worker/<id>/telemetry` is only
a compatibility alias when the selected manifest enables it.

An accepted control append is **admission, not READY**. Inspect the structured
Worker observation and keep these axes separate:

| Field | What it establishes |
| --- | --- |
| Declaration | Whether that role is executable or model-only in the selected profile |
| Lifecycle | `queued`, `starting`, `ready`, `closing`, `faulted`, `terminal` or absence |
| Artifact | Whether the declared artifact is missing, verified or mismatched |
| Receipt | Whether a runtime receipt is pending, confirmed, rejected, stale or absent |
| Execution proof | `none`, `host-model`, `qemu` or `fresh-pi`; not interchangeable |

Once the specific test Worker has been identified, `kill worker-1` submits its
termination request in a compatibility deployment. Replace that ID; never kill
a guessed Worker. Observe its subsequent state or removal. Termination admission
is not proof that teardown has finished.

Other accepted convenience forms are:

```text
spawn gpu gpu_id=GPU-0 mem_mb=4096 streams=2 ttl_s=120 priority=1
spawn lora
```

`GPU-0` is illustrative: use a GPU identifier actually published for the
selected deployment and a resource request its policy permits. These commands
are not evidence that a GPU exists or that training has run.

| Role selectors | Required `key=value` arguments | Optional arguments |
| --- | --- | --- |
| `heartbeat`, `worker`, `worker-heartbeat` | `ticks` | `ttl_s`, `ops` |
| `gpu`, `worker-gpu` | `gpu_id`, `mem_mb`, `streams`, `ttl_s` | `priority`, `budget_ttl_s`, `budget_ops` |
| `lora`, `worker-lora` | None | None |

Unknown or duplicate keys are rejected. `spawn bus` and `spawn worker-bus`
return a model-only refusal without a control write. Role availability and
capacity remain generated-profile decisions.

`ls /shard` is a bounded view of up to 64 active shard labels, **not a complete
large-fleet index**. Complete discovery enumerates the generated shard address
space and reads each shard's Worker directory, as SwarmUI and the benchmark
harness do. Empty shards do not imply failed discovery; a failed read or
malformed/misplaced Worker record does.

### Submit a production Queen intent

Use `/queen/intents/ctl` when strict intents are required. An envelope contains
`schema`, `id`, `idempotency_key`, `issued_unix_ms`, the selected `writer_epoch`
and **`cmd` as a JSON string containing the existing command object**. A nested
JSON object in `cmd` is not the same format.

Before proceeding, confirm the selected epoch with `cat /proc/authority`,
confirm that a new heartbeat Worker is appropriate, and obtain write authority
for `/queen/intents/ctl`. This procedure deliberately creates one request. It
does not bypass lifecycle, policy approval, ticket or capacity checks.

In **host Bash**, enter the verified epoch and create a saved command file
**once**. Python supplies the current issue time and new identifiers; exclusive
file creation prevents accidentally replacing a request whose outcome is unknown:

```bash
mkdir -p out/operator
umask 077
read -r -p 'Writer epoch reported by this Queen: ' COH_WRITER_EPOCH
export COH_WRITER_EPOCH
python3 - <<'PY'
import json
import os
from pathlib import Path
import time
import uuid

raw_epoch = os.environ['COH_WRITER_EPOCH']
if not raw_epoch.isascii() or not raw_epoch.isdecimal():
    raise SystemExit('Writer epoch must be the unsigned integer read from /proc/authority')
epoch = int(raw_epoch)
if not 0 <= epoch <= (1 << 64) - 1:
    raise SystemExit('Writer epoch is outside the unsigned 64-bit range')
intent = {
    'schema': 'queen-intent/v1',
    'id': 'heartbeat-' + uuid.uuid4().hex,
    'idempotency_key': uuid.uuid4().hex,
    'issued_unix_ms': time.time_ns() // 1_000_000,
    'writer_epoch': epoch,
    'cmd': json.dumps({'spawn': 'heartbeat', 'ticks': 100}, separators=(',', ':')),
}
payload = json.dumps(intent, separators=(',', ':'))
if len(payload.encode('utf-8')) > 2048:
    raise SystemExit('Intent exceeds the console echo payload bound')
path = Path('out/operator/spawn-heartbeat.coh')
with path.open('x', encoding='utf-8') as stream:
    stream.write('# Author: Lukas Bower\n')
    stream.write('# Copyright 2026 Lukas Bower\n')
    stream.write('# SPDX-License-Identifier: Apache-2.0\n')
    stream.write('# Purpose: Submit one retained, explicitly identified heartbeat request.\n')
    stream.write('echo ' + payload + ' > /queen/intents/ctl\nEXPECT OK\nquit\n')
print('Created', path, 'with intent id', intent['id'])
PY
"$COH_BIN/cohsh" --check out/operator/spawn-heartbeat.coh
```

Inspect the saved file locally. Then, with the direct interactive client
closed and the correct console credential still selected, execute it from
**host Bash**:

```bash
"$COH_BIN/cohsh" --transport tcp \
  --tcp-host "$COH_TARGET_HOST" --tcp-port "$COH_TARGET_PORT" \
  --role queen --script out/operator/spawn-heartbeat.coh
```

For an existing gateway, use `--transport rest --rest-url "$COH_REST_URL"`
instead of the TCP options, with the request-auth token and appropriately
scoped delegated ticket loaded. Do not run both alternatives.

After successful submission, reconnect and inspect `/proc/queen/dedupe`, the
Queen log and the actual Worker observation. The dedupe view is bounded; an
entry missing from its recent list is not proof that no effect occurred.

**An interrupted write has an unknown outcome, not an automatic failure.**
Reconcile the retained outcome before retrying. An authorised strict retry
must reuse the exact saved request, including both identifiers, issue time,
epoch and command bytes. Do not rerun the generator to manufacture a new
identity. Exact duplicates return the retained outcome; changed fields under
the same identity produce an idempotency conflict. Dedupe capacity is bounded,
and its authority lifetime does not survive a VM restart as a durable promise.
Do not reboot to clear it and resubmit. Full details are in
[M27a authority](M27A_AUTHORITY.md#strict-queen-commands).

The same envelope mechanism can carry an existing `kill`, `bind` or `mount`
Queen command object. It is not a universal wrapper for every control file:
use each namespace's own documented payload contract.

### Control the Queen lifecycle

These commands change node state and can affect ongoing work. First read the
state at **`coh>`**:

```text
cat /proc/lifecycle/state
```

For a Queen currently `ONLINE` or `DEGRADED`, an authorised maintenance sequence
begins:

```text
lifecycle cordon
cat /proc/lifecycle/state
```

Only after confirming `DRAINING`, request `lifecycle drain` and inspect the
state again. Resume only when maintenance and workload policy permit it:
`lifecycle resume`, followed by another state read. Do not paste all transitions
blindly into a live fleet.

| Command | Client-side state check | Important distinction |
| --- | --- | --- |
| `lifecycle cordon` | `ONLINE` or `DEGRADED` | Maintenance admission control, not a hardware shutdown |
| `lifecycle drain` | `DRAINING` | State and workload progress still need observation |
| `lifecycle resume` | Not already `ONLINE` | Server policy can still refuse |
| `lifecycle quiesce` | `ONLINE`, `DEGRADED` or `DRAINING` | A lifecycle transition, not a platform reboot |
| `lifecycle reset` | Not already `BOOTING` | Resets lifecycle state; **does not reboot the Pi** |

`cohsh` writes the lifecycle control namespace, not a shell command on the host.
The server remains authoritative for every transition. Use `reboot` only for an
intentional platform restart, with Queen authority and a supported backend,
after retaining evidence and considering work in progress.

<a id="coh-scripts-coh"></a>

## .coh scripts

A `.coh` file is a deterministic sequence of `cohsh` commands and assertions.
It has no variables, substitutions, branching, loops, includes, macros or
runtime downloads. One command per line; blank lines are ignored. `#` begins
a comment, including inline comments, so do not put a literal `#` in a payload
that must survive script parsing. Use host tooling to prepare concrete files.

### Create, check and run a health script

In **host Bash**:

```bash
mkdir -p out/operator
cat > out/operator/health.coh <<'COH'
# Author: Lukas Bower
# Copyright 2026 Lukas Bower
# SPDX-License-Identifier: Apache-2.0
# Purpose: Check attachment and read bounded Queen state without workload writes.
ping
EXPECT OK
ls /proc
EXPECT OK
cat /proc/lifecycle/state
EXPECT OK
cat /proc/authority
EXPECT OK
tail /log/queen.log 16
EXPECT OK
quit
COH
"$COH_BIN/cohsh" --check out/operator/health.coh
```

`--check` reads local policy and validates script structure without connecting
to a target. It does not prove that a path exists, credentials are valid,
commands are authorised or the target will accept a payload.

Close any direct interactive owner, then run the file in **host Bash**:

```bash
"$COH_BIN/cohsh" --transport tcp \
  --tcp-host "$COH_TARGET_HOST" --tcp-port "$COH_TARGET_PORT" \
  --role queen --script out/operator/health.coh
```

Retain a transcript without losing the command's failure status:

```bash
set -o pipefail
"$COH_BIN/cohsh" --transport tcp \
  --tcp-host "$COH_TARGET_HOST" --tcp-port "$COH_TARGET_PORT" \
  --role queen --script out/operator/health.coh \
  2>&1 | tee out/operator/health-transcript.txt
```

This pipeline is **host Bash**, not `.coh` syntax. Choose a new transcript
filename when preserving an earlier run. A script stops on an unexpected
command, transport or assertion failure and exits non-zero. Failure details
include the source line, last command, response source and bounded history.

### Use the right assertion

| Statement | Meaning |
| --- | --- |
| `EXPECT OK` | Recorded response begins with `OK` |
| `EXPECT ERR` | Recorded response begins with `ERR` |
| `EXPECT SUBSTR <text>` | Case-sensitive substring in the recorded response |
| `EXPECT NOT <text>` | Case-sensitive substring absent from the recorded response |
| `WAIT <ms>` | Local delay of at most 2000 ms; no target command |
| `WAIT <ms> TAIL <path> SUBSTR <text>` | Poll completed authenticated tail data for a substring, for at most the bounded polling interval |

The recorded response normally prefers a transport acknowledgement over later
payload output. Therefore `cat /proc/lifecycle/state` followed by `EXPECT OK`
checks that the read succeeded; it does **not** assert `state=ONLINE`.

To assert returned data, use the read-condition form. For a deployment expected
to be online, this is a `.coh` fragment:

```text
WAIT 2000 TAIL /proc/lifecycle/state SUBSTR state=ONLINE
EXPECT OK
```

Use the same pattern with the actual Worker telemetry path and its documented
READY representation after asynchronous admission. The condition is checked
against completed data, not merely an ACK. A refusal or transport failure stops
immediately; the wait never repeats a mutation. Each read retains its own
transport response timeout, so 2000 ms is not an override of network timeouts.

A deliberate negative test may place `EXPECT ERR` immediately after the command
whose refusal is expected. Do not use it to hide an unexpected production write
failure. Scripts contain at most 256 non-empty statements. Generated scripts,
such as [boot_v0.coh](../scripts/cohsh/boot_v0.coh), must be regenerated rather
than edited manually.

### Automate a self-test

In a test environment where the installed negative checks are appropriate,
create a file whose command is `test --mode quick --no-mutate --json` and run
it with `--script` and an explicit attached role, just like `health.coh`.
Inspect the report's `ok` field as well as process exit status. In interactive
mode a printed failure report does not by itself make the eventual shell exit
status a failed test run. Prefer `--script` for automation.

### Retain a live trace or rehearse offline

To record the read-only health script against the live target, close another
direct owner and run in **host Bash**:

```bash
"$COH_BIN/cohsh" --transport tcp \
  --tcp-host "$COH_TARGET_HOST" --tcp-port "$COH_TARGET_PORT" \
  --role queen --script out/operator/health.coh \
  --record-trace out/operator/health.trace
```

Live recording supports TCP and REST. Add `--trace-target-id`,
`--trace-session-id`, `--trace-manifest-sha256` and `--trace-image-sha256` only
with values known for the captured target; unspecified identity remains
unknown, not inferred proof. Captures are bounded by generated size and duration
limits. Incomplete capture is an error, not a complete trace.

Replay that live capture **offline**, without reconnecting to the Queen:

```bash
"$COH_BIN/cohsh" --replay-trace out/operator/health.trace
```

For an isolated in-process rehearsal instead:

```bash
"$COH_BIN/cohsh" --transport mock --role queen
```

Mock state exists only in that `cohsh` process. A separate mock tool or a later
process does not see its Workers or notes. Legacy fixture replay and mock GPU
seeding also depend on compiled features. Neither replay nor mock is fresh
hardware evidence. See [Operator evidence](OPERATOR_EVIDENCE.md) for capture,
inspection, attestation and evidence-pack contracts.

## Command reference

### Root console

Run `help` on the active image for profile availability. These commands are
entered at **`cohesix>`**, not in host Bash.

| Command | Action and boundary |
| --- | --- |
| `help` | Active image's console inventory |
| `ping` | Root-console liveness |
| `bi` | BootInfo summary and source-labelled `[bi:v2]` records |
| `caps`, `caps mcs` | Capability summary or bounded MCS authority/object counts |
| `mem` | RAM/device untyped-memory summary |
| `smp`, `smp activity` | Equivalent bounded userspace activity reports |
| `smp mcs` | Generated and live MCS topology, plus supported retained Pi diagnostics |
| `smp poll-time` | Pi root elapsed-time, Yield and receive observations; unsupported elsewhere |
| `smp dump` | Debug-only raw kernel scheduler dump; unavailable after linked-UART cutover |
| `cachelog [n]` | Bounded recent cache-operation snapshot |
| `nettest`, `netstats` | Network-test admission and separately observed terminal result/state |
| `attach <role> [ticket]` | Select an authorised role for namespace operations |
| `ls <path>`, `cat <path>`, `tail <path> [lines]` | Bounded namespace operations, subject to role/profile |
| `log` | Retained Queen log tail |
| `echo <path> <payload>` | **Path-first** append, subject to write policy |
| `spawn <payload>`, `kill <worker>` | Raw compatibility shortcuts; production refuses these |
| `reboot` | Queen-authorised platform restart when a backend is available |
| `quit` | Event-pump session termination; early bootstrap reports unsupported |

Root `test` directs you to host `cohsh`; it is not the host self-test runner.
Pi-only `usb` and `wifi` families are described in the bring-up and advanced
sections. Arbitrary-memory `hexdump` is disabled in production/release policy;
a separately enabled debug profile permits only bounded immutable root-code
inspection. It is not a normal operator command.

### cohsh interactive commands

Enter these at **`coh>`** or as command lines in a `.coh` file. Support of a
forwarded diagnostic still depends on the selected transport and target.

| Command | Purpose |
| --- | --- |
| `help` | Local shell inventory; not the target's raw HELP response |
| `attach <role> [ticket]`, `login ...` | Attach; roles are Queen, worker-heartbeat (alias worker), worker-gpu, worker-bus, worker-lora |
| `detach`, `quit` | Close attachment and stay in the shell, or close and exit |
| `ping` | Check the active attachment |
| `bi`, `caps [mcs]`, `smp [activity\|mcs\|poll-time\|dump]` | Forward supported root diagnostics |
| `ls <path>`, `cat <path>` | List or read one absolute namespace path |
| `tail <path> [lines]`, `log` | Finite bounded tail; not a background follower |
| `log dump <file.txt> [--force]` | Save Queen log payload on the host |
| `echo <text> > <path>` | Append a single line; not filesystem truncation |
| `spawn <role> <key=value>...`, `kill <id>` | Compatibility Queen requests; no automatic strict envelope |
| `bind <src> <dst>`, `mount <service> <path>` | Compatibility namespace-control requests through `/queen/ctl`; not host mount commands |
| `lifecycle <cordon\|drain\|resume\|quiesce\|reset>` | Validated lifecycle control request |
| `telemetry push <src> --device <id>` | Upload local telemetry content or reference records |
| `test [--mode quick\|full\|smp] [--json] [--timeout <s>] [--no-mutate]` | Installed self-test runner |
| `nettest`, `netstats`, `reboot` | Supported console operations; reboot requires Queen authority |
| `tcp-diag [port]` | TCP connection diagnostics, not authentication proof |
| `pool bench <options>` | Mutating host session-pool benchmark; use the [benchmark workflow](BENCHMARKS.md), not as a first health check |

**`mem`, `cachelog`, `usb`, `wifi` and `hexdump` are not dispatched by the current
host `cohsh` command handler.** Use the physical root console for them, even
though some appear in the compiler-generated shared grammar below. Shared
parser vocabulary and host CLI implementation are not identical inventories.

The `qemu` transport launches staged artifacts and rejects writes; use TCP or
REST for actual namespace work. REST is a bounded gateway projection, not an
arbitrary raw-console relay. A locally listed diagnostic can be unsupported
by that transport; use direct TCP when it is free, or serial, rather than
assuming a locally displayed help item guarantees a REST operation.

### Input limits

The shared console currently bounds a complete command line to 2304 bytes, an
absolute path to 96 bytes, a ticket to 224 bytes and an `echo` payload to 2048
bytes. Raw console `spawn` JSON has a separate 192-byte bound; this is not the
strict-intent envelope limit. Namespace walks are bounded to eight components,
and selected namespace/profile policy can impose smaller limits. Keep payloads
on one line and allow for the newline appended by `cohsh`.

A general `msize=8192` transport budget does not enlarge the console's smaller
path, line or payload bounds. Split work using the owning interface's supported
segmentation, not invented continuation syntax.

### Response and session rules

Target acknowledgements use `OK <VERB>` and typed `ERR <VERB>` details.
Streaming `LS`, `CAT` and `TAIL` responses complete with `END`. `cohsh` handles
the transport framing and labels acknowledgements `[console]`; it also prints
local status and payload lines. Do not send a plain terminal/netcat session to
the framed TCP endpoint and expect serial behaviour.

A refusal normally has no side effect unless its owning interface says
otherwise. An **absent response** is not a refusal. Batches containing writes
are not replayable merely because a connection failed part-way through them.
Retry/backoff policy never grants authority to duplicate an uncertain mutation.

TCP `quit` is complete only after exact `OK QUIT`, client write-half close and
peer EOF on that same connection. Missing ACK, extra post-terminal frames,
timeout or missing EOF fails script mode; QUIT is not retried on a replacement
connection. Do not interpret the host process disappearing as proof of a clean
remote close.

## CLI options and credentials

Use `cohsh --help` for compiled options. Most sessions need only transport,
endpoint, role and a privately supplied credential.

| Option group | Options |
| --- | --- |
| Connection | `--transport tcp\|rest\|mock\|qemu`, `--tcp-host`, `--tcp-port`, `--rest-url` |
| Attachment | `--role`, `--ticket` |
| Authentication | `--auth-token`, `--rest-auth-token`; prefer private environment/reference sources to literal secrets in arguments |
| Automation | `--script FILE`, `--check FILE` (mutually exclusive) |
| Diagnostics | `-v` / `--verbose`, `--tcp-debug` |
| Selected policy | `--policy FILE` |
| Response/retry policy | `--retry-max-attempts`, `--retry-backoff-ms`, `--retry-ceiling-ms`, `--retry-timeout-ms`, `--rest-response-timeout-ms` |
| Pool/heartbeat policy | `--pool-control-sessions`, `--pool-telemetry-sessions`, `--heartbeat-interval-ms` |
| Trace | `--record-trace FILE`, `--replay-trace FILE`, optional capture identity/hash fields |
| Diagnostic QEMU boot | `--qemu-bin`, `--qemu-out-dir`, `--qemu-gic-version`, repeated `--qemu-arg` |
| Mock GPU namespace | `--mock-seed-gpu` when the required features are compiled |
| Ticket issuer tooling | `--mint-ticket`, `--ticket-subject`, `--ticket-config`, `--ticket-secret`, `--ticket-write-scope`, `--ticket-ttl-s`, `--ticket-ops` |

Ticket minting is an issuer operation requiring the correct signing material;
it is not how an ordinary user bypasses missing access. It requires `--role`
and conflicts with an input ticket, script/check and trace modes. Worker
subjects are required; delegated write tickets additionally need a subject,
scope and bounded lifetime. Follow [Authority](M27A_AUTHORITY.md) and
[Host tools authentication](HOST_TOOLS.md#authentication-layers).

### Credential and environment precedence

| Setting | Resolution order or implementation caveat |
| --- | --- |
| TCP credential | `--auth-token`, then `COH_AUTH_TOKEN_REF`, `COH_AUTH_TOKEN`, `COHSH_AUTH_TOKEN` |
| TCP host | `COHSH_TCP_HOST` replaces a parsed host equal to `127.0.0.1`, including that explicitly supplied value |
| TCP port | A valid numeric `COHSH_TCP_PORT` overrides the parsed port, including an explicit flag |
| REST URL | `--rest-url`, `COHSH_REST_URL`, `COH_REST_URL`, `HIVE_GATEWAY_URL` |
| REST request token | `--rest-auth-token`, `COHSH_REST_AUTH_TOKEN`, `COH_REST_AUTH_TOKEN`, `HIVE_GATEWAY_REQUEST_AUTH_TOKEN` |
| REST caller ticket | Explicit attachment ticket or `COH_REST_TICKET`; reattachment replaces the binding |
| Policy file | `--policy`, `COHSH_POLICY`, installation default |
| Ticket signing source | `--ticket-secret`, `COHSH_TICKET_SECRET`, then selected ticket config |
| Ticket config | `--ticket-config`, `COHSH_TICKET_CONFIG`, `configs/root_task.toml` |

TCP credential references are exactly `env:NAME` or `file:/absolute/path`.
A selected reference that cannot be read or validated **fails without falling
back** to a different credential. Reads are bounded to 4096 bytes; empty,
malformed, placeholder and whitespace/control-containing live token values are
refused. File sources are reread on new authentication for rotation.

Pool, retry and heartbeat overrides also have corresponding `COHSH_*`
environment forms (for example `COHSH_RETRY_TIMEOUT_MS`); explicit flags take
precedence for those numeric settings. Do not infer that rule for TCP port,
whose implementation behaves differently as shown above.

REST filesystem response windows compose 5000 ms queue admission, the larger
of the gateway control/telemetry response bounds, and 5000 ms delivery grace.
The canonical `120000/120000 ms` gateway profile therefore needs a 130000 ms
client envelope. `--rest-response-timeout-ms` precedes
`COHSH_REST_RESPONSE_TIMEOUT_MS`; the selected window cannot be smaller than
the composed gateway envelope. This is not a retry allowance and does not
change the separate connection, metadata or body-transfer bounds.

## Troubleshooting

| What you see | First useful check | Next action |
| --- | --- | --- |
| No `cohesix>` after power-on | Is output still firmware/U-Boot, or has the root task started? | Retain the first blocker and boot identity; use the hardware runbook. A Pi splash screen is not a Cohesix boot. |
| Boot menu visible but serial typing does nothing | Selected menu input; default is USB | Use the local USB keyboard. Do not send Wi-Fi secrets through serial. |
| Serial text appears but commands are garbled or lost | Correct device, line settings, competing owner, paste rate | Use one owner and paced single commands. Root queue-drop zero does not exclude child/FIFO loss. |
| HDMI prompt missing while serial works | `usb status`, display blocker and actual USB key response | Distinguish input admission from display health; preserve serial access. |
| `coh>` appears but `ping` says not attached | Earlier attach error | Correct the endpoint/credentials, then `attach queen`; the prompt itself is local. |
| TCP connection refused or times out | Actual boot address, port, network/listener state and current console owner | Stop competing direct clients; inspect serial `netstats` before changing retry limits. |
| `tcp-diag` succeeds but attach fails | TCP is reachable; authentication and role are still unproven | Check the exact selected secret source and ticket policy, not just the socket. |
| Token missing/invalid despite another token variable being set | Precedence and selected `env:`/`file:` source | Repair that source. It intentionally does not fall back. Never print the token to debug it. |
| Connection goes to the wrong port/localhost | Inherited `COHSH_TCP_HOST` / `COHSH_TCP_PORT` | Clear stale overrides and restart with the intended endpoint. |
| REST reads work, writes fail | Request-auth token, caller ticket, TTL, subject, scope, mount and quotas | Supply both authentication layers; `--role queen` alone is insufficient. |
| `spawn`, `kill`, `bind` or `mount` refused in production | `/proc/authority`, compatibility policy | Use an authorised strict intent for the existing Queen command object. |
| `OK` after spawn but no usable Worker | Published structured Worker observation | Inspect lifecycle/artifact/receipt/proof independently; do not manufacture READY from admission. |
| Worker path is missing | Actual shard label, ID, profile and enabled aliases | Discover the canonical path; do not assume `/worker` or a guessed sequential ID. |
| `nettest` says started but there is no pass | Same connection/run generation and terminal `netstats` verdict | Complete the required peer interaction; another run's result cannot satisfy this one. |
| `test --no-mutate` still attempts writes | Installed negative script | This mode is selective skipping, not a read-only security boundary. Use explicit reads when necessary. |
| `EXPECT SUBSTR` misses text visibly printed by `cat` | Recorded response source | Use `WAIT ... TAIL ... SUBSTR ...` for a data condition, not an ACK-text assertion. |
| `log dump` fails | `.txt` suffix, parent directory and existing destination | Create the host directory; use a new filename or intentional `--force`. |
| Telemetry reports `mode=reference` | Encoded inline budget/source format | Retain original bytes; only reference records were uploaded. |
| Timeout or disconnect during a mutation | Retained request, logs, dedupe/result state | Treat outcome as unknown. Reconcile before any retry; never give the same work a fresh identity automatically. |
| Unexpected `busy`, `quota`, `cut` or `policy` | Exact typed detail and relevant lifecycle/ticket/budget | Fix the specific cause. Larger client retries cannot widen target authority. |

Capture the failing command, complete bounded response, target image/profile,
boot/connection identity and relevant serial context. Keep credentials out of
reports. Preserve the first fault before rebooting or opening another TCP
connection that replaces latest-session evidence.

## Advanced diagnostic reference

The following expandable sections retain field interpretation for incident and
driver analysis. They are not a required reading path for ordinary shell work.
Use the matching image's records; unavailable, stale and unobserved values are
not zero-valued proof. Detailed driver contracts are in [DRIVERS.md](DRIVERS.md),
external schemas in [INTERFACES.md](INTERFACES.md), and acceptance requirements
in [TEST_PLAN.md](TEST_PLAN.md).

<details>
<summary>Activity, USB and local-seat readiness</summary>

`smp` and `smp activity` are equivalent. Selected driver snapshots use seven
bounded `[smp] driver v=1 part=<turn|outcome|sched|retry|cache|traffic|role>`
rows, each fitting the 256-byte console line ABI. They do not replace the
complete 1024-byte `DRIVER_TASK_COUNTER` provenance record. A missing selected
driver projection is missing evidence, not permission to substitute another.

`serial_rx_drop` and `serial_rx_backpressure` describe the root serial queue,
not the isolated serial runtime queue or mini-UART FIFO. Per-core USB rates
`seatPoll_s`, `kbdB_s`, `seat_drop_s`, `seat_no_reply_s` belong to the USB core;
`hdmiB_s` and `hdmi_drop_s` belong to the display core. Display drops are not
keyboard drops, and neither driver's rates are duplicated onto the other.

The Pi startup banner and bounded elapsed counter do not gate boot readiness.
The isolated display runtime owns rendering. The HDMI interactive prompt is
released only after USB command input is admitted, the display is healthy and
`Cohesix console ready` is queued before the prompt. A later passive
`[drivers] USB console ready` record does not control that order. Loss of
readiness retracts the prompt/ready banner while retaining the typed suffix;
fresh readiness is required to restore them. Serial can be ready earlier.

`usb status`, `usb dump-state` and `usb diag` inspect retained state.
`usb diag` also arms a post-command liveness observation without polling the
device. Its ten-gate history is not current keyboard proof. After a real USB
key, a one-shot pass requires linked HID-byte, parser-acceptance, parser-drain
and echo counters to advance without a new drop. `usb enable-kbd` and
`usb probe-kbd` may change polling or advance a retained probe slice. An
`attached` probe result requires its live service turn to finish; a cached
ready latch with pending work remains `keyboard-unavailable continuation=pending`.

HDMI queue state and the display owner's completion receipt are separate.
No boot framebuffer reports `state=unavailable`,
`blocker=framebuffer-not-admitted`, `receipt=none`,
`next_action=reboot-with-display-connected`. Counters without a registered
owner report `state=unproven`, `blocker=driver-task-owner-unproven` and
`receipt=none`. A ready receipt needs framebuffer, registered owner, completed
turn, no outstanding turn and healthy retry state.

Mapped USB and HDMI runtimes expose `usb: command_frontier` (request/command/
completion sequence, lease phase, issued/admission, capability generation,
producers/sends), `usb: command_wait` (notification binding, cap generation,
prompted slice, absent/wait/ack state and exact request/slice match), and
`usb: command_progress` (validity, sequence, phase and auxiliary value).
`domain` distinguishes `usb-runtime` from `hdmi-display`. These are snapshots,
not timeout or readiness verdicts.

Every passive USB response begins with this adjacent atomic pair:

```text
USB_OLDGOOD_RETAINED v=1 task=<u32> token=0x<8hex> link_epoch=<u32> link_token=0x<8hex> epoch=<u32> seq=<u32> mask=0x<8hex> topology=0x<8hex> input_gen=<u32> commit=<u32> source=<linked-runtime-hid|none>
USB_OLDGOOD_CURRENT contracts=usb-local-seat+pcie-root owners=<driver-owned|missing>+<driver-owned|missing> descriptors=<sealed|missing>+<sealed|missing> command_ready=<yes|no> proof_gate=<0|14> blocker=<none|receipt-missing|usb-owner-missing|pcie-owner-missing|usb-descriptor-missing|pcie-descriptor-missing|command-not-ready> root_pointer=no
```

Pairs are USB then PCIe. A complete receipt needs `mask=0x00003fff`, `commit`
equal to `seq`, `source=linked-runtime-hid`, both owners, both sealed
descriptors, command readiness, `proof_gate=14` and `blocker=none`. Missing
evidence retains version 1 with zero identity/body fields and `source=none`.
Active enable/probe commands do not emit this pair. The passive command does
not create a hardware transition. Linked local-seat readiness cannot release
pre-proof input until both USB and PCIe owner/descriptor chains are current;
failed service/recovery clears that retained readiness evidence.

</details>

<details>
<summary>Wi-Fi triage, recovery and packet-receipt snapshots</summary>

`wifi diag` has at most eight preflighted body lines plus terminal/status/ACK
output. It leads with the first known failed gate and labels its snapshot
`best-effort-multi-record`. Later gates are `not-reached` rather than falsely
current. `wifi dump-state` is the verbose acceptance/DPC/association/maintenance/
queue/TX/Gate 7–8 surface. Its `wifi: pair_handoff` records retain separately
owned CYW43 and SDIO first-child traces; `observed=no` is missing or unstable
information. Legacy probe/load/retry commands return typed ownership refusal
without invoking a physical operation.

Physical-console `smp` in Wi-Fi mode can prepend a passive atomic 37-line
old-good batch: current serial, USB, HDMI, PCIe, CYW43 and SDIO owner records,
then the contiguous 31-line `WIFI_OLDGOOD_RETAINED_BEGIN`/hash/26-step/
`WIFI_OLDGOOD_RETAINED_END` transaction. It needs a complete matching attempt,
pair, generation, firmware and ordered association/EAPOL/DHCP receipt. Its
presence does no device work; its absence is missing evidence. Fresh network,
authenticated TCP, terminal `nettest` and DPC observations must still follow.

A retained `wifi: deferred_recovery scheduler_root` uses a 16-digit hex `site`:
upper 32 bits are source tag 1=`hal/driver_task.rs`,
2=`drivers/driver_task_net.rs`, 3=`event/mod.rs`, 4=`userland/mod.rs`,
15=other; lower 32 bits are the exact-build line. Zero is unavailable.
`terminal=yes` binds code/detail/result to the retained root command sequence;
with `terminal=no`, zero operands are unavailable. The first request survives
recovery and clears at accepted Gate 8. The atomic recovery batch is at most
13 rows; compact `wifi diag` retains its eight-body-line bound.

`wifi: rx_reject schema=v1` is one `retained=no` row when absent, otherwise
11 rows: identity, four before/after queue samples, two header samples and
four entry-half rows. Failure stages are `envelope`, `queue-before`,
`generation`, `header`, `initial-identity`, `queue-after`, `final-identity`,
`count`. These name failed checks, not root causes. Queue samples are the
existing stable-read pair; headers are the original two reads. `observed=false`
means placeholders cannot be used as evidence. Metadata is fixed-width hex
except sample/half indices; booleans are `true`/`false`. Queue `abi` is
version/size and `depth` level/capacity; header `count` is count/remaining,
`valid` body-valid/committed; entries are offset/length/flags/source-counter-low
word in FIFO slot order. Payload is not retained. Same-generation in-progress
publication defers without fabricating rejection; malformed, unavailable,
poisoned and identity-invalid records remain distinct. The original deadline
still bounds publication. Records survive recovery scrub and clear at Gate 8.

`wifi rx-trace <0..5>` requires one ASCII page digit, is serial/local-seat only,
and returns a header, flow row, up to 16 records and
`OK WIFI detail=subcommand=rx-trace scope=serial-local`. The 96-record journal
contains SYN/FIN/RST and data headers for the latest control flow, not pure
ACKs or payload. New generation or SYN identity resets it; retransmissions keep
separate receipt ordinals/IPv4 IDs. `first`/`next` expose eviction, and `ignored`
counts other flows. Collect all six pages after connection close, before a new
connection; reconcile generation, flow and ordinal headers across snapshots.
IPs, sequence/ACK/flags, IPv4 ID and `s/q/r/d` are hex; ports, lengths, ordinals
and header counts are decimal. `none` means unavailable; zero is valid.
`s` is source-counter low word, `q` packed Q11 metadata, `r` root-copy counter
low word and `d` full dequeue CNTVCT. Source time is not wire arrival.

Pi `netstats` additionally retains `wifi_ack_admission`, `wifi_ack_last`,
`wifi_ack_fin` and `wifi_ack_before_fin` version-1 records. These describe a
structurally valid, nonfragmented IPv4 ACK-only header (flags `0x10`, no TCP
payload), stage/returned-signal identity and exact child-ingress completion.
`consumed=yes` does **not** prove smoltcp processed or retired that ACK.
A received SYN clears latest/close identity without resetting cumulative
counts; a new Wi-Fi generation resets both. `wifi_ack_before_fin` freezes the
same-flow ACK receipt immediately before the first matching FIN; later ACKs,
repeated FINs and completions cannot rewrite that cut. Missing data is
`absent=yes`. Collect through serial before another connection and reconcile
with the boot-paired packet capture; no row proves on-air delivery.

</details>

<details>
<summary>Network verdicts, isolated progress and GENET causal refresh</summary>

The complete terminal/running network-test record is:

```text
nettest: generation=<connection> run_generation=<run> enabled=<bool> running=<bool> verdict=<none|running|pass|peer-assisted-pass|fail> tx_ok=<bool|na> udp_echo_ok=<bool|na> tcp_ok=<bool|na> console_ok=<bool|na> peer_assisted_ok=<bool|na>
```

The positive run generation must match admission. Target addresses/backend
identity are emitted separately as `nettargets:`. `profile_backend` is resolved
manifest selection; `active_driver` is this boot's selected physical/virtual
driver; compatibility `backend` aliases `active_driver`.

Isolated profiles also emit `isolated_progress`, `isolated_units` and
`isolated_state`: child turns, last material progress, bounded queues, pending
egress/drain and ingress backpressure/drop. Direct-GENET additions are:

```text
isolated_progress: pcont=<candidates>/<admitted>/<rejected> peff_us=<n> preason=0x<n>
isolated_units: output_ok=<n>
isolated_state: ycalls=<n> ycredit_us=<n> yinvalid=0x<n>
```

`peff_us` is root elapsed observation, not admission authority. `preason` bits
are fence `0x01`, cap `0x02`, clock `0x04`, policy `0x08`, counter `0x10`,
arithmetic `0x20`, reserved retired `0x40`, token `0x80`. `output_ok` counts
durable output-stage success, not attempts. `yinvalid` bits are pre-drain
`0x01`, counter/frequency `0x02`, syscall result `0x04`, overflow `0x08`.
These fields are zero outside that exact direct-GENET path.

On the exact Pi direct-GENET generation, `netstats` performs one bounded causal
refresh with an idempotent `DGHO` replay. It may wake the owner and permit normal
idle service to drain durable RX: **not a passive performance sample**.
Available rows are emitted in this order:

```text
netstats: genet_direct ...
netstats: genet_direct_flags ...
netstats: genet_direct_before ...
netstats: genet_direct_before_ring ...
netstats: genet_direct_irq ...
netstats: genet_direct_irq_source ...
netstats: genet_direct_notification ...
netstats: genet_direct_dpc ...
netstats: genet_direct_dma ...
netstats: genet_direct_ring ...
netstats: genet_direct_peer ...
```

`refresh` is `fresh`, `ready-stale`, `ready-unverified`, `ready-missing`,
`timeout`, `rejected` or `inactive`; a replacement can be
`phase=pre-idle-service`. The `before` rows retain pre-replay owner/ring state;
`ready-unverified` means no stable pre-replay record existed to prove freshness.
Missing optional rows remain missing. DPC timing/reason fields diagnose bounded
service, not qualified WCET or throughput. Component `active=yes` cannot
replace canonical `NET_ACTIVE` backend selection. These rows establish neither
DHCP/ARP/ICMP/TCP success nor Pi acceptance.

</details>

<details>
<summary>Pi fast-path counters and causal seam histograms</summary>

The bounded `netstats` fast-path record grammar is:

```text
netstats: cyw43_publication schema=v1 candidates=<u64> minted=<u64> consumed=<u64> rejected=<u64> reasons=0x<hex>
netstats: cyw43_publication_cut schema=v1 probe=<u64> entry=<u64> pre_network=<u64> revoked=<u64>
netstats: cyw43_productive_window schema=v1 opened=<u64> idle_admitted=<u64> closed=<u64> ready_rechecks=<u64>
netstats: genet_compact schema=v1 stage=<u64> deferred=<u64> fault=<u64> unsupported=<u64> dispatch=<u64> stage_turns=<u64> rotations=<u64>
netstats: genet_compose schema=v1 composed=<u64> no_pending=<u64> not_sealed=<u64> backpressure=<u64> identity_drift=<u64>
netstats: genet_defer schema=v1 passive=<u64> command=<u64> compose_open=<u64> compose_backpressure=<u64> fence=<u64> prior_batch=<u64> control_busy=<u64> output_missing=<u64> stage_backpressure=<u64>
netstats: isolated_seam schema=v2 name=<command-created-root-observe|command-created-publish|command-publish-root-observe|dispatch-stage|stage-control-observe|stage-output-drained|output-drained-root-observe> n=<u64> bad=<u64> ms=<total>/<last>/<max> h=<hex>/<hex>/<hex>/<hex>/<hex> hs=<0|1> [pairs=<u64>]
```

Publication rejection bits are snapshot/lifetime drift `0x1`, operator/recovery
fence `0x2`, final pre-Network drift `0x4`, empty/non-material publication `0x8`.
Cut counters classify rejections, not retries. `idle_admitted` is a retained
schema field for a retired path and stays zero under event-backed continuation.
`ready_rechecks` spends the existing one-shot recheck allowance. Full Operator,
Driver and attached Network turns spend the unchanged 64 logical material-work
units; productive Driver/Network progress is independently capped at 64.
Counters do not grant continuation, refill, readiness or device authority.

The exact single-command credit-rebase case must preserve lifetime, identity,
service and all operator/recovery fences while admitting one command, one newly
sealed completed response and one bounded same-connection flush. Other changes
are rejection, not an extra continuation path.

`genet_compose composed` means a sealed `SyncCapture` moved. `no_pending` is not
by itself permission to advance: an immediate terminal's already queued lane
must independently satisfy exact generation, connection, authentication,
recovery, flush and batch-drain predicates, including one completed response,
stage readiness, terminal queued and producer closed. Otherwise the result is
`output_missing`; identity drift remains fail-closed. Each aggregate compact
Deferred increments exactly one of the nine `genet_defer` classes; their sum
matches the aggregate deferred count. `compose_open` corresponds to NotSealed.

The seven optional version-2 seam names separate creation, publication, root
observation, dispatch, durable StageOutput, observed control-consumption
watermark and OutputDrained publication. `pairs` appears only on
`command-created-publish`. Counts and `ms=total/last/max` are decimal saturating
u64; histogram bins are 0, 1, 2–3, 4–7 and at least 8 milliseconds. Display bins
clip at `ffff`, with `hs=1`, while internal bins retain full width. Zero age is
valid with nonzero endpoints; missing/backwards endpoints increment `bad`.
Mean age is `total/n` when `n` is nonzero; rows fit 256 bytes.

Version 1's `command-publish-root-observe` included pre-publication waiting;
version 2 separates those timestamps. `stage-control-observe` includes child
consumption and later observation; `stage-output-drained` includes peer TCP
ACK retirement and publication waiting. **Intervals overlap: do not sum them
into wire latency.** Pi uses absolute CNTVCT with generated `TIMER_CLOCK_HZ`;
host tests retain caller-time fallback and QEMU omits this Pi accounting.
External framed TCP measurements remain authoritative for target latency.

</details>

<details>
<summary>MCS topology, composer/Yield accounting and latest-session evidence</summary>

`smp mcs` labels generated admission `source=generated`, BootInfo `source=kernel`
and copied live state `source=runtime`. Unavailable state is not a fabricated
missing registration. Pi live registrations use paired `[smp:registry/v1]`
rows, indexed by `base` into the same snapshot's ordered non-Worker task rows.
`count` is one or two; registration/terminal lists match it. Missing generation
is `none`, terminal `unknown`, not zero-valued evidence. For example:

```text
[smp:registry/v1] base=0 count=2 registration=present,missing generation0=1/1/1 generation1=none terminal=yes,unknown
```

The selected four-core Pi Wi-Fi body fits the 64-line synchronous TCP capture
and 69-line physical body, retaining paired registrations, owner CPU rows,
passive-timeout receipt, timing/idle records and the end marker. Protocol
terminal space remains separate. QEMU retains its per-task registration format.

Despite their `netstats:` prefix, the following **17 lifetime rows belong to
`smp mcs`, not `netstats`**:

```text
netstats: mcs_quantum schema=v1 hz=<u64> samples=<u64> material=<u64> periods=<u64> invalid=<u64> invalid_period=<u64>
netstats: mcs_quantum_lane schema=v1 wifi=<u64> genet=<u64>
netstats: mcs_quantum_total schema=v1 period_us=<u64> run_us=<u64>
netstats: mcs_quantum_timing schema=v1 period_avg_us=<u64> period_max_us=<u64> run_avg_us=<u64> run_max_us=<u64>
netstats: mcs_quantum_period schema=v1 bounds_us=1000,3000,6000,9000,12000,20000 buckets=<u64>,<u64>,<u64>,<u64>,<u64>,<u64>,<u64>
netstats: mcs_quantum_run schema=v1 bounds_us=1000,3000,6000,9000,12000,20000 buckets=<u64>,<u64>,<u64>,<u64>,<u64>,<u64>,<u64>
netstats: mcs_quantum_last schema=v1 lane=<wifi|genet> generation=<u64> conn=<u64> progress=0x<hex> pending=0x<hex>->0x<hex> period_us=<u64> run_us=<u64> exit=<YIELD|RETAIN|FENCE|FAULT>
netstats: mcs_quantum_exit schema=v2 yields=<u64> retains=<u64> fences=<u64> faults=<u64> pending=<u64> stalled=<u64>
netstats: mcs_command_dispatch schema=v1 samples=<u64> invalid=<u64> avg_ms=<u64> last_ms=<u64> max_ms=<u64>
netstats: mcs_observe_dispatch schema=v1 samples=<u64> invalid=<u64> avg_ms=<u64> last_ms=<u64> max_ms=<u64>
netstats: mcs_yield schema=v1 hz=<u64> samples=<u64> invalid=<u64> pending=<u64> wifi=<u64> genet=<u64>
netstats: mcs_yield_timing schema=v1 total_us=<u64> avg_us=<u64> max_us=<u64>
netstats: mcs_yield_hist schema=v1 bounds_us=1000,3000,6000,9000,12000,20000 buckets=<u64>,<u64>,<u64>,<u64>,<u64>,<u64>,<u64>
netstats: mcs_yield_cause schema=v2 reserve=<u64> no_successor=<u64> passive=<u64> recovery=<u64> operator=<u64> other=<u64>
netstats: mcs_yield_last schema=v1 lane=<wifi|genet> generation=<u64> conn=<u64> pending=0x<hex> trigger=<RESERVE_GUARD|NO_PRODUCTIVE_SUCCESSOR|PASSIVE_ADMISSION|RECOVERY_FENCE|OPERATOR_ROTATION|OTHER_BOUNDARY> hiatus_us=<u64>
netstats: mcs_budget_guard schema=v2 totals=<u64>,<u64>,<u64>,<u64> pending=<u64>,<u64>,<u64>,<u64>
netstats: mcs_budget_reason schema=v1 cap=<u64> clock=<u64> reserve=<u64> policy=<u64> mask=0x<hex>
```

Progress bits: command `0x1`, child `0x2`, stage `0x4`, drain `0x8`, ingress
`0x10`, token `0x20`, queue `0x40`. Pending bits: command queue `0x1`, root
output `0x2`, child control `0x4`, child egress `0x8`, child event `0x10`,
continuation `0x20`, Wi-Fi driver `0x40`, passive admission `0x80`, operator
`0x100`, recovery `0x200`. Budget lists are activation, attached, bootstrap
operator, bootstrap driver. Reason bits are cap `0x1`, clock `0x2`, reserve
`0x4`, policy `0x8`. Former split v1 state/cause/pending records and emitted
legend rows are folded into these version-2 records and fixed legends.

Three global idle rows follow: `mcs_idle schema=v1` has `before`, `after`,
`timer_reject`, `clear=<before>/<after>`, `last_cut=<0|1|2>` (before enable,
after enable, timer rejected), `mask`; two `mcs_idle_fences schema=v2`
`base=<0|8> counts=<eight decimal u64 values>` rows retain all 16 independently
saturating fence counts. Bits 0–15 are inexact topology, unavailable child
level, staged IPC, physical input, serial output, display, reboot,
recovery/containment, handoff, passive admission, local fault, physical
response, retained output, network work, child publication service (including
owed ACK), timer-enable rejection. Co-occurring fences are not exclusive.
A clear after-enable cut permits a wait but does not prove the syscall slept.

Composer `run` brackets its leaf; `period` is start-to-start from the previous
valid composer observation to the current material quantum. First observation
has no period; backwards starts increment `invalid_period`. `pending` is
material work at entry; `stalled` means pending before and after without
progress. These are not kernel activations, SC refills or CPU consumption.
Yield timing brackets the Pi's explicit `CNTVCT -> svc -> CNTVCT` interval,
with one exclusive trigger per sample. Raw 54 MHz counter ticks are converted
only when rendering. Missing/backwards counters or invalid frequency are
counted separately; the final histogram bucket is `>=20000 us`. Under selected
NaturalPostpone, retained clock/reserve fields are diagnostic/historical, not
productive admission authority. Large wall gaps do not identify a kernel refill.

`[smp:consumed/v1]` is different: retained per-owner differences of cumulative
kernel Consumed receipts for the latest observed TCP lifecycle. Header decimal
fields are generation/connection/frequency; `ended` is boolean; selected/pending/
claimed masks are hex. Role bits 0–8 are root-control, console-network, GENET,
CYW43, SDIO, serial, USB, HDMI, PCIe. GENET selects seven owners and Wi-Fi eight.
Per-owner `cpu_us` is decimal; cap-generation and begin/end bracket pairs are
hex. `valid=false` invalidates numeric placeholders. Missing samples, errors,
backwards clocks/totals and generation changes invalidate pairs. Asynchronous
owner sampling and observed connection boundaries differ from host benchmark
boundaries. Read before another TCP lifecycle replaces the snapshot.
`[smp:passive-timeout/v1]` retains cumulative `resumes`, resettable
`last_sc_consumed_us` and `limit_per_call=1`. **Reading these snapshots performs
no new kernel Consumed/accounting operation.** QEMU does not collect Pi receipts.

Eight latest-session `mcs_session*` v1 rows instead belong to Pi **`netstats`**:
summary, four fence rows (`base=0|4|8|12`), operator predicates, Yield summary
and maximum Yield cut. They retain the latest nonzero generation/connection
after disconnect; zero identity does not erase it. Operator counts are
`serial_rx`, `serial_line`, `local_line`, `local_chunk`, `usb_bytes`,
`usb_service`, and can co-occur. Yield summary has samples/total/max/invalid and
six decimal cause counts in reserve, no-successor, passive, recovery, operator,
other order. Cause counts saturate at u32 maximum; aggregates at u64 maximum.

`mcs_session_yield_cut` retains the first maximum valid sample or `absent=yes`.
Pending/command/stage/drain/tick pairs are hex. Decimal phase maps Serial=0,
Dispatch=1, ContainmentDiagnostic=2, Network=3, LocalSeat=4, Display=5;
publication maps unknown=0, empty=1, durable child publication=2. A shorter or
invalid sample cannot replace the maximum. These are pre-Yield root
observations, not synchronised child snapshots or continuation authority.
Later serial typing with no active TCP identity cannot contaminate them.
Zero idle cuts do not prove an idle-free session; these wall measurements do
not prove consumption, refill exhaustion or that a permitted wait blocked.

</details>

<details>
<summary>Pi poll-time, Yield trace and slow receive intervals</summary>

`smp poll-time` starts with eight rows: a `poll_time schema=v1` header with
decimal generation, connection, frequency and invalid count, then seven phases
`serial`, `dispatch`, `containment`, `network`, `local-seat`, `display`,
`between`. Phase-row `n`, `sum`, `max`, `cmd` and `ticks` are hexadecimal.
Sum/max are microseconds; `cmd` is the accepted-command count at the maximum,
and `ticks` brackets it. These wall intervals include preemption; `between`
includes work/waits outside ordinary observed polls. The exclusive post-Yield
passive-admission poll is deliberately not instrumented. The latest nonzero
identity is retained after disconnect. Reading does not reset CPU accounting.

`yield_trace schema=v1` follows with up to 32 earliest valid explicit Yield
intervals for that session. Header generation/connection/kept/total/omitted/
invalid are decimal; row `n`, `ctx`, `phase`, `pub`, `hz` are decimal and
pending/command/stage/drain/tick fields hex. `cause` names the existing trigger;
`ctx=0` means unavailable context. Invalid intervals are counted, not included.
A zero kept count or omitted tail cannot establish that no scheduling delay
occurred. Context bits and low-word wrap rules are exact-image diagnostic
contracts, not permission to change scheduling decisions; consult
[the recorder implementation](../apps/root-task/src/pi4_mcs_recorder.rs).

A `receive_trace schema=v1` header and up to 16 slowest receive/endpoint-handler
brackets follow, descending by whole microseconds with earlier samples first
on ties. Header counts and `sum_us`, row `n`, `us`, `hz` are decimal; row `cmd`
and tick pairs are hex. Outcome is `empty`, `endpoint`, `fanin` or
`unavailable`. Two counter reads bracket an existing receive only while a
nonzero Pi TCP identity is live; no receive is added. These spans include
endpoint handling and preemption, not necessarily sleep. Invalid clocks are
counted; omitted means valid observations outside bounded retention. The whole
command is bounded to 58 body rows before its terminal acknowledgement.

</details>

## Compiler-generated reference

These marker-delimited blocks are retained mirrors of the standalone
`coh-rtc` snippets. They describe the generated configuration, not proof that
your currently running target has that configuration. Do not edit their
contents by hand; update manifest/IR inputs and regenerate affected outputs.
In particular, the shared grammar is not the complete host dispatcher and
its path-first `echo` is not interactive `cohsh` syntax.

<!-- markdownlint-disable MD022 MD031 MD032 MD033 -->

<details>
<summary>cohsh client policy</summary>

<!-- coh-rtc:cohsh-policy:start -->
### cohsh client policy (generated)
- `manifest.sha256`: `91a0c04d4d6591ac87f6e228ba4f0f4ec79cdb0b87acd00e584450ea776cae53`
- `policy.sha256`: `60addb4122ed4a42343fdfab653b83c8567af0314081b6a146cc45f2832f5244`
- `cohsh.pool.control_sessions`: `2`
- `cohsh.pool.telemetry_sessions`: `24`
- `cohsh.tail.poll_ms_default`: `1000`
- `cohsh.tail.poll_ms_min`: `250`
- `cohsh.tail.poll_ms_max`: `10000`
- `cohsh.host_telemetry.nvidia_poll_ms`: `1000`
- `cohsh.host_telemetry.systemd_poll_ms`: `2000`
- `cohsh.host_telemetry.docker_poll_ms`: `2000`
- `cohsh.host_telemetry.k8s_poll_ms`: `5000`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
- `heartbeat.interval_ms`: `15000`
- `trace.max_bytes`: `1048576`
- `trace.max_duration_ms`: `60000`

_Generated from `configs/root_task.toml` (sha256: `91a0c04d4d6591ac87f6e228ba4f0f4ec79cdb0b87acd00e584450ea776cae53`)._
<!-- coh-rtc:cohsh-policy:end -->

</details>

<details>
<summary>cohsh client defaults</summary>

<!-- coh-rtc:cohsh-client:start -->
### cohsh client defaults (generated)
- `manifest.sha256`: `91a0c04d4d6591ac87f6e228ba4f0f4ec79cdb0b87acd00e584450ea776cae53`
- `worker.task_abi_schema`: `worker-task-abi/v2`
- `worker.task_abi_version`: `2`
- `worker.observation_schema`: `cohesix-worker-observation/v1`
- `worker.integration_evidence_schema`: `cohesix-worker-integration-evidence/v1`
- `worker.maximum_live_tasks`: `256`
- `worker.canonical_telemetry_template`: `/shard/<label>/worker/<id>/telemetry`
- `worker.shard_bits`: `8`
- `worker.legacy_worker_alias`: `true`
- `worker.lifecycle`: `absent, queued, starting, ready, closing, faulted, terminal`
- `worker.receipt`: `none, pending, confirmed, rejected, stale`
- `worker.artifact`: `missing, verified, mismatch`
- `worker.execution_proof`: `none, host-model, qemu, fresh-pi`
- `worker.role.worker-heartbeat`: declaration=`executable`, executable_slots=`1`
- `worker.role.worker-gpu`: declaration=`executable`, executable_slots=`127`
- `worker.role.worker-bus`: declaration=`model-only`, executable_slots=`0`
- `worker.role.worker-lora`: declaration=`executable`, executable_slots=`128`
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `trace.max_bytes`: `1048576`
- `trace.max_duration_ms`: `60000`
- `client_paths.queen_ctl`: `/queen/ctl`
- `client_paths.queen_lifecycle_ctl`: `/queen/lifecycle/ctl`
- `client_paths.queen_schedule_ctl`: `/queen/schedule/ctl`
- `client_paths.queen_lease_ctl`: `/queen/lease/ctl`
- `client_paths.queen_export_ctl`: `/queen/export/ctl`
- `client_paths.policy_ctl`: `/policy/ctl`
- `client_paths.log`: `/log/queen.log`
- `telemetry_ingest.max_segments_per_device`: `4`
- `telemetry_ingest.max_bytes_per_segment`: `131072`
- `telemetry_ingest.max_total_bytes_per_device`: `524288`
- `telemetry_ingest.max_reference_entries_per_segment`: `1024`
- `telemetry_ingest.max_reference_manifest_bytes_per_segment`: `131072`
- `telemetry_ingest.max_reference_bytes_per_segment`: `1073741824`
- `telemetry_ingest.eviction_policy`: `evict-oldest`

_Generated from `configs/root_task.toml` (sha256: `91a0c04d4d6591ac87f6e228ba4f0f4ec79cdb0b87acd00e584450ea776cae53`)._
<!-- coh-rtc:cohsh-client:end -->

</details>

<details>
<summary>cohsh console grammar</summary>

<!-- coh-rtc:cohsh-grammar:start -->
### cohsh console grammar (generated)
- `help`
- `bi`
- `caps [mcs]`
- `smp [activity|mcs|poll-time|dump]`
- `mem`
- `ping`
- `test`
- `nettest`
- `netstats`
- `reboot`
- `log`
- `cachelog [n]`
- `quit`
- `tail <path> [lines]`
- `cat <path>`
- `ls <path>`
- `echo <path> <payload>`
- `attach <role> [ticket]`
- `spawn <payload>`
- `kill <worker>`

_Generated from cohsh-core verb specs (20 verbs)._
<!-- coh-rtc:cohsh-grammar:end -->

</details>

<details>
<summary>cohsh ticket policy and quotas</summary>

<!-- coh-rtc:cohsh-ticket-policy:start -->
### cohsh ticket policy (generated)
- `ticket.max_len`: `224`
- `queen` tickets are optional; TCP validates claims when present, NineDoor passes through.
- `worker-*` tickets are required; role must match and subject identity is mandatory.

_Generated from cohsh-core ticket policy._
<!-- coh-rtc:cohsh-ticket-policy:end -->

<!-- coh-rtc:ticket-quotas:start -->
### Ticket quota limits (generated)
- `ticket_limits.max_scopes`: `8`
- `ticket_limits.max_scope_path_len`: `128`
- `ticket_limits.max_scope_rate_per_s`: `64` (0 = unlimited)
- `ticket_limits.bandwidth_bytes`: `8388608` (0 = unlimited)
- `ticket_limits.cursor_resumes`: `16` (0 = unlimited)
- `ticket_limits.cursor_advances`: `256` (0 = unlimited)

_Generated by coh-rtc (sha256: `3b1501e34fd9367b624534459a6f036cd1990d2e663f21189fa7c85781584da9`)._
<!-- coh-rtc:ticket-quotas:end -->

</details>

<details>
<summary>coh policy and doctor defaults</summary>

<!-- coh-rtc:coh-policy:start -->
### coh policy defaults (generated)
- `manifest.sha256`: `91a0c04d4d6591ac87f6e228ba4f0f4ec79cdb0b87acd00e584450ea776cae53`
- `policy.sha256`: `378407153b033cd71dbfafac850a491b33b43bc376d3a886caa2663b4026f168`
- `coh.worker.task_abi_schema`: `worker-task-abi/v2`
- `coh.worker.task_abi_version`: `2`
- `coh.worker.observation_schema`: `cohesix-worker-observation/v1`
- `coh.worker.integration_evidence_schema`: `cohesix-worker-integration-evidence/v1`
- `coh.worker.maximum_live_tasks`: `256`
- `coh.worker.canonical_telemetry_template`: `/shard/<label>/worker/<id>/telemetry`
- `coh.worker.shard_bits`: `8`
- `coh.worker.legacy_worker_alias`: `true`
- `coh.worker.lifecycle`: `absent, queued, starting, ready, closing, faulted, terminal`
- `coh.worker.receipt`: `none, pending, confirmed, rejected, stale`
- `coh.worker.artifact`: `missing, verified, mismatch`
- `coh.worker.execution_proof`: `none, host-model, qemu, fresh-pi`
- `coh.worker.role.worker-heartbeat`: declaration=`executable`, executable_slots=`1`
- `coh.worker.role.worker-gpu`: declaration=`executable`, executable_slots=`127`
- `coh.worker.role.worker-bus`: declaration=`model-only`, executable_slots=`0`
- `coh.worker.role.worker-lora`: declaration=`executable`, executable_slots=`128`
- `coh.mount.root`: `/`
- `coh.mount.allowlist`: `/proc, /queen, /shard, /worker, /log, /gpu, /host`
- `coh.telemetry.root`: `/queen/telemetry`
- `coh.telemetry.max_devices`: `32`
- `coh.telemetry.max_segments_per_device`: `4`
- `coh.telemetry.max_bytes_per_segment`: `131072`
- `coh.telemetry.max_total_bytes_per_device`: `524288`
- `coh.run.lease.schema`: `gpu-lease/v1`
- `coh.run.lease.active_state`: `ACTIVE`
- `coh.run.lease.max_bytes`: `1024`
- `coh.run.breadcrumb.schema`: `gpu-breadcrumb/v1`
- `coh.run.breadcrumb.max_line_bytes`: `512`
- `coh.run.breadcrumb.max_command_bytes`: `256`
- `coh.peft.export.root`: `/queen/export/lora_jobs`
- `coh.peft.export.max_telemetry_bytes`: `131072`
- `coh.peft.export.max_policy_bytes`: `8192`
- `coh.peft.export.max_base_model_bytes`: `1024`
- `coh.peft.import.registry_root`: `out/model_registry`
- `coh.peft.import.max_adapter_bytes`: `67108864`
- `coh.peft.import.max_lora_bytes`: `65536`
- `coh.peft.import.max_metrics_bytes`: `65536`
- `coh.peft.import.max_manifest_bytes`: `8192`
- `coh.peft.activate.max_model_id_bytes`: `128`
- `coh.peft.activate.max_state_bytes`: `4096`
- `retry.max_attempts`: `3`
- `retry.backoff_ms`: `200`
- `retry.ceiling_ms`: `2000`
- `retry.timeout_ms`: `5000`
<!-- coh-rtc:coh-policy:end -->

<!-- coh-rtc:coh-doctor:start -->
### coh doctor checks (generated)
- `check=policy` validates `coh_policy.toml` against manifest + policy hashes.
- `check=ticket` uses `ticket.max_len=224` and TCP policy (queen tickets optional, worker tickets required).
- `check=mount` validates allowlist under `coh.mount.root` and requires FUSE when not `--mock`.
- `check=nvml` prefers NVML when not `--mock`; Jetson-class NVML falls back to CUDA discovery.
- `check=runtime` checks `python3` and `qemu-system-aarch64` (QEMU skipped with `--mock`).
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `coh.mount.allowlist`: `/proc, /queen, /shard, /worker, /log, /gpu, /host`

_Generated by coh-rtc (sha256: `8ff5f5a73c1e4d454f1263e3235d01d2bde35adb6553bd578b64ae9f496b3b4b`)._
<!-- coh-rtc:coh-doctor:end -->

</details>

<details>
<summary>Python client defaults</summary>

<!-- coh-rtc:cohesix-py:start -->
### Cohesix Python defaults (generated)
- `manifest.sha256`: `91a0c04d4d6591ac87f6e228ba4f0f4ec79cdb0b87acd00e584450ea776cae53`
- `cohesix.defaults.sha256`: `d1f930abb8866ba2fd1436f2f7df663dc4cd72b0ede60c38887dc0ad8156e1ae`
- `secure9p.msize`: `8192`
- `secure9p.walk_depth`: `8`
- `console.max_line_len`: `2304`
- `console.max_path_len`: `96`
- `console.max_json_len`: `192`
- `console.max_echo_len`: `2048`
- `telemetry_ingest.max_bytes_per_segment`: `131072`
- `telemetry_ingest.max_total_bytes_per_device`: `524288`
- `telemetry_ingest.max_reference_entries_per_segment`: `1024`
- `telemetry_ingest.max_reference_manifest_bytes_per_segment`: `131072`
- `telemetry_ingest.max_reference_bytes_per_segment`: `1073741824`
- `coh.mount.root`: `/`
- `coh.mount.allowlist`: `/proc, /queen, /shard, /worker, /log, /gpu, /host`
- `coh.telemetry.root`: `/queen/telemetry`
- `coh.run.breadcrumb.max_line_bytes`: `512`
- `coh.peft.import.registry_root`: `out/model_registry`

_Generated by coh-rtc (sha256: `49abefef66bda68b2ec088b654dbcf8e3067041c07fc55b8de8d06608c45fd1f`)._
<!-- coh-rtc:cohesix-py:end -->

</details>

<details>
<summary>SwarmUI defaults</summary>

<!-- coh-rtc:swarmui-defaults:start -->
### SwarmUI defaults (generated)
- `manifest.sha256`: `91a0c04d4d6591ac87f6e228ba4f0f4ec79cdb0b87acd00e584450ea776cae53`
- `swarmui.defaults.sha256`: `b4a8a9848f72d543ad64c26fadaa78635ebf999f18319bb303d5d7501c834a08`
- `swarmui.ticket_scope`: `per-ticket`
- `swarmui.cache.enabled`: `false`
- `swarmui.cache.max_bytes`: `262144`
- `swarmui.cache.ttl_s`: `3600`
- `swarmui.hive.frame_cap_fps`: `30`
- `swarmui.hive.step_ms`: `16`
- `swarmui.hive.lod_zoom_out`: `0.7`
- `swarmui.hive.lod_zoom_in`: `1.25`
- `swarmui.hive.lod_event_budget`: `512`
- `swarmui.hive.snapshot_max_events`: `4096`
- `swarmui.hive.overlay_lines`: `3`
- `swarmui.hive.detail_lines`: `50`
- `swarmui.hive.line_cap_bytes`: `160`
- `swarmui.hive.per_worker_bytes`: `2048`
- `swarmui.hive.pending_lines_per_worker`: `64`
- `swarmui.hive.pending_event_cap`: `4096`
- `swarmui.hive.poll_workers_per_tick`: `32`
- `swarmui.hive.status_poll_ms`: `500`
- `swarmui.hive.degrade_pressure`: `1.0`
- `swarmui.paths.telemetry_root`: `/worker`
- `swarmui.paths.proc_ingest_root`: `/proc/ingest`
- `swarmui.paths.worker_root`: `/shard`
- `swarmui.paths.namespace_roots`: `/proc, /queen, /shard, /worker, /log, /gpu`
- `swarmui.worker_runtime.maximum_live_tasks`: `256`
- `swarmui.worker_runtime.canonical_telemetry_template`: `/shard/<label>/worker/<id>/telemetry`
- `swarmui.worker_runtime.shard_bits`: `8`
- `swarmui.worker_runtime.legacy_worker_alias`: `true`
- `swarmui.worker_runtime.role.worker-heartbeat`: declaration=`executable`, executable_slots=`1`
- `swarmui.worker_runtime.role.worker-gpu`: declaration=`executable`, executable_slots=`127`
- `swarmui.worker_runtime.role.worker-bus`: declaration=`model-only`, executable_slots=`0`
- `swarmui.worker_runtime.role.worker-lora`: declaration=`executable`, executable_slots=`128`
- `trace.max_bytes`: `1048576`

_Generated from `configs/root_task.toml` (sha256: `91a0c04d4d6591ac87f6e228ba4f0f4ec79cdb0b87acd00e584450ea776cae53`)._
<!-- coh-rtc:swarmui-defaults:end -->

</details>

<!-- markdownlint-enable MD022 MD031 MD032 MD033 -->

## Implementation and related references

| Contract | Source or authoritative guide |
| --- | --- |
| Host options, environment resolution, script/trace startup | [cohsh main](../apps/cohsh/src/main.rs) |
| Actual host dispatcher, payload builders, scripts, self-tests and local file export | [cohsh core](../apps/cohsh/src/lib.rs) |
| Shared target parser and bounds | [cohsh-core command parser](../crates/cohsh-core/src/command.rs) |
| TCP connection, acknowledgement and close handling | [TCP transport](../apps/cohsh/src/transport/tcp.rs) |
| Gateway projection and caller binding | [REST transport](../apps/cohsh/src/transport/rest.rs), [API guidelines](API_GUIDELINES.md) |
| Worker role/path projection and independent state axes | [Worker helpers](../apps/cohsh/src/worker.rs) |
| Installed test content | [resources/proc_tests](../resources/proc_tests) |
| Namespace schemas and control payloads | [Interfaces](INTERFACES.md) |
| Production mutation/delegation and retry identity | [M27a authority](M27A_AUTHORITY.md) |
| Host-side GPU work, bridges, gateway and UI | [Host tools](HOST_TOOLS.md) |
| Live workflow and advanced recipes | [Operator walkthrough](OPERATOR_WALKTHROUGH.md), [Operator recipes](OPERATOR_RECIPES.md) |
| Evidence packs, offline inspection and trace contracts | [Operator evidence](OPERATOR_EVIDENCE.md) |
| Signed evidence, `/proc/attest`, verifier trust and unavailable modes | [Attestation](ATTESTATION.md) |
| Image-qualified release and performance checks | [Test plan](TEST_PLAN.md), [Benchmarks](BENCHMARKS.md) |

When changing the CLI, update the owning parser/transport, affected tests and
this guide together. Keep examples explicit about their prompt, transport,
authority, side effects and success condition. Regenerate the marker blocks
instead of editing them. Documentation examples are procedures, not records
that a particular Queen or release has passed them.
