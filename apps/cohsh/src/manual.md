<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Author: Lukas Bower -->
<!-- Purpose: Canonical embedded operator manuals shared by cohsh and SwarmUI. -->

## man
NAME
  man — read the installed command manual without contacting a Queen.
SYNOPSIS
  man [command]
DESCRIPTION
  With no command, list manual topics. Each page describes current syntax,
  arguments, examples, authority and limits. Read man namespace for paths,
  man authority for production writes, and man scripts for automation.
  Angle brackets mark values to replace; square brackets mean optional input.
  Do not type these brackets. Commands are case-sensitive in cohsh.
EXAMPLES
  man
  man spawn
  man ls
  man authority
  Host terminal: cohsh --man spawn
  Host terminal: cohsh --man
  Host terminal: cohsh --man spawn | less
  --man exits before credentials, policy loading or transport startup.
ERRORS
  An unknown topic or more than one topic returns an error. No request is sent.

## help
NAME
  help — show the concise command index.
SYNOPSIS
  help
EXAMPLES
  help
  man spawn
DESCRIPTION
  Help is rendered locally. Use man for complete arguments and workflows.
  A listed target diagnostic may still be unsupported by the transport/profile.
  Host launch options are documented by cohsh --help in your host terminal.

## attach
NAME
  attach — select a role and optional capability ticket for a session.
SYNOPSIS
  attach <role> [ticket]
  login <role> [ticket]
ARGUMENTS
  Roles: queen, worker-heartbeat (alias worker), worker-gpu, worker-lora,
  worker-bus. WorkerBus is model/session-only, not an executable target role.
  ticket: deployment-issued capability token; maximum 224 bytes in the console.
  Console authentication and role attachment are separate checks. A ticket
  cannot grant authority beyond the selected transport and target policy.
EXAMPLES
  attach queen
  login queen
  attach worker-heartbeat REPLACE_WITH_ISSUED_TICKET
  login worker-gpu REPLACE_WITH_ISSUED_TICKET
  Replace ticket placeholders with real credentials; do not use fixture keys.
DESCRIPTION
  Attach before reading or writing namespace paths. Detach closes the current
  attachment. REST keeps the gateway's upstream identity; a local attach does
  not replace per-request gateway authentication or delegated read/write scopes.
ERRORS
  Unknown roles, extra arguments, invalid/expired tickets and policy denials
  fail attachment. Check credentials and scopes; do not retry with broader roles.
SEE ALSO
  man detach; man authority

## detach
NAME
  detach — close the current attachment while keeping cohsh open.
SYNOPSIS
  detach
EXAMPLES
  detach
  attach queen
DESCRIPTION
  Releases the current session and pool state. Use before changing roles.
  Target/provider jobs are not implicitly cancelled. Use their control contract.
  quit also exits the shell.

## quit
NAME
  quit — close the current session and exit cohsh.
SYNOPSIS
  quit
EXAMPLES
  quit
DESCRIPTION
  Does not shut down the Queen or terminate admitted Workers. In SwarmUI it
  closes the console session while leaving the application open.

## ls
NAME
  ls — list one absolute Cohesix namespace directory.
SYNOPSIS
  ls <path>
ARGUMENTS
  path: absolute target namespace path, at most 96 bytes and eight components.
  There is no current directory, glob expansion, recursive flag, or '..'.
EXAMPLES
  ls /
  ls /proc
  ls /shard
  ls /shard/13/worker
  ls /gpu
  ls /host
NAMESPACE
  /                          selected profile and role determine visibility
  |-- proc/                  read-only state, bounds, authority and counters
  |   |-- authority
  |   |-- lifecycle/state
  |   `-- queen/dedupe        bounded retained strict-intent outcomes
  |-- log/queen.log          Queen diagnostics and operator notes
  |-- queen/
  |   |-- ctl                compatibility control (disabled in production)
  |   |-- intents/ctl        strict, correlated Queen commands
  |   |-- lifecycle/ctl      lifecycle transitions
  |   `-- telemetry/         device ingest and segment records
  |-- shard/<label>/worker/<id>/telemetry   canonical Worker observations
  |-- worker/<id>/           optional compatibility aliases
  |-- gpu/                  host-published GPU/model state, when selected
  |-- host/                 enrolled host services, tickets and snapshots
  |-- policy/               selected governance controls
  |-- audit/                bounded audit records
  `-- updates/              selected content-addressed update surfaces
DESCRIPTION
  This is a capability-controlled service namespace, not the host filesystem.
  Optional families are absent when disabled. Start with ls / and use cat or
  tail on files. ls does not read file contents and needs no write permission.
  /shard lists at most 64 active labels; it is not a complete large-fleet index.
  Complete discovery traverses the generated shard address space. The example
  label 13 corresponds to worker-1 in the eight-bit layout; discover your actual
  ID and shard instead of guessing. Empty shards are valid.
ERRORS
  A missing path can mean a disabled provider, absent Worker, or wrong path.
  A permission refusal requires the correct role/read scope. Do not interpret
  a failed read as an empty fleet. REST visibility is checked before caches.
SEE ALSO
  man cat; man spawn; man authority

## namespace
NAME
  namespace — understand paths and service boundaries.
DESCRIPTION
  Read man ls for the namespace tree and discovery examples. Paths refer to the
  Queen's bounded service records. cat reads them; echo appends according to
  each file's contract. No cd, arbitrary filesystem writes, pipes, variables,
  shell expansion, executable loading, or POSIX utilities run inside cohsh.
  Host files are used only by explicit host operations such as log dump and
  telemetry push. Optional namespace families depend on the selected manifest.
EXAMPLES
  ls /
  cat /proc/authority
  cat /proc/lifecycle/state
  tail /log/queen.log 32

## cat
NAME
  cat — read one bounded namespace file.
SYNOPSIS
  cat <path>
EXAMPLES
  cat /proc/authority
  cat /proc/lifecycle/state
  cat /shard/13/worker/worker-1/telemetry
DESCRIPTION
  Requires an attachment and read authority for the absolute path. Output is
  bounded by transport/profile limits; a snapshot is not a continuous stream.
  Discover the actual Worker path with ls. Use log dump to save Queen logs on
  the host; 'cat path > host-file' is not a supported redirection.
ERRORS
  Missing, unauthorized or malformed paths fail. For directories use ls.
SEE ALSO
  man ls; man tail

## tail
NAME
  tail — read a finite bounded tail, not a background follower.
SYNOPSIS
  tail <path> [lines]
ARGUMENTS
  lines: integer 1..256; omitted uses the transport's bounded default.
  Byte caps can further limit the returned data. There is no -f option.
EXAMPLES
  tail /log/queen.log
  tail /log/queen.log 32
  tail /shard/13/worker/worker-1/telemetry 16
DESCRIPTION
  Requires read authority. The command finishes after its bounded response.
  Repeat deliberately for another snapshot; do not flood a busy target.
SEE ALSO
  man log; man ls

## log
NAME
  log — inspect or save the retained Queen log.
SYNOPSIS
  log
  log dump <file.txt> [--force]
ARGUMENTS
  file.txt: local host destination with a .txt extension; parent must exist.
  --force: explicitly overwrite an existing destination. Otherwise creation is
  exclusive and an existing file is an error.
EXAMPLES
  log
  log dump queen-log.txt
  log dump queen-log.txt --force
DESCRIPTION
  log is a bounded tail of /log/queen.log. log dump writes available log payload
  to a host file; it cannot recover evicted history. Save logs before disruptive
  operations. Output can contain deployment data; retain it appropriately.
  SwarmUI supports log with 64 lines and its native Dump log action.
SEE ALSO
  man tail

## echo
NAME
  echo — append one line to a writable namespace file.
SYNOPSIS
  echo <text> > <path>
ARGUMENTS
  text: one-line payload, optionally enclosed by one matching quote pair.
  path: absolute namespace destination. A newline is appended; '>' does not
  truncate the file. No pipelines, variable expansion or command substitution.
  The console payload bound is 2048 bytes; the framed command must also fit.
EXAMPLES
  echo maintenance-window-open > /log/queen.log
  echo {"spawn":"heartbeat","ticks":100} > /queen/ctl
  The second example requires a compatibility-enabled deployment and Queen
  authority. For production Worker commands, follow man authority instead.
DESCRIPTION
  Files have distinct schemas and write policy. An accepted append establishes
  admission only; inspect the corresponding status/receipt for completion.
  Root serial uses a different, path-first form: echo <path> <payload>.
ERRORS
  Invalid paths/payloads, read-only files, missing scopes or disabled
  compatibility control are refused. An interrupted write has unknown outcome;
  reconcile before retrying a mutation.
SEE ALSO
  man authority; man spawn

## spawn
NAME
  spawn — request a declared control-plane Worker in compatibility mode.
SYNOPSIS
  spawn heartbeat ticks=<u64> [ttl_s=<u64>] [ops=<u64>]
  spawn gpu gpu_id=<id> mem_mb=<u32> streams=<u8> ttl_s=<u32>
      [priority=<u8>] [budget_ttl_s=<u64>] [budget_ops=<u64>]
  spawn lora
ARGUMENTS
  heartbeat aliases: worker, worker-heartbeat; gpu alias: worker-gpu;
  lora alias: worker-lora. bus and worker-bus are model-only and refused.
  ticks: requested heartbeat work count. ttl_s and ops: optional Worker budget.
  gpu_id: actual host-published device ID, not an arbitrary device path.
  mem_mb: requested lease memory in MiB; streams: requested stream count.
  GPU ttl_s: lease duration in seconds; priority: requested lease priority.
  budget_ttl_s and budget_ops: Worker ticket budget, separate from the GPU lease.
  u8/u32/u64 denote unsigned integers of the stated width. Policy applies tighter
  resource bounds; parsing an integer does not imply capacity is available.
  LoRA accepts no options. Unknown, duplicate or inappropriate keys fail.
EXAMPLES
  cat /proc/authority
  cat /proc/lifecycle/state
  spawn heartbeat ticks=100
  spawn heartbeat ticks=100 ttl_s=120 ops=500
  spawn worker-heartbeat ticks=100 ttl_s=120 ops=500
  ls /gpu
  spawn gpu gpu_id=GPU-0 mem_mb=4096 streams=2 ttl_s=120 priority=1 budget_ttl_s=180 budget_ops=500
  spawn lora
  These are independent examples, not a batch to paste into a live fleet.
  Replace GPU-0 with the published ID and request resources allowed by policy.
WORKFLOW
  Attach as an authorized Queen. Check authority, lifecycle and capacity first.
  This convenience command writes /queen/ctl and does not construct a strict
  intent. Production profiles disable it: use man authority for the complete
  retained-envelope procedure; do not turn off production policy to use spawn.
  ACK is admission only, not READY or proof of execution. Discover the actual
  Worker through ls /shard and its published shard directory. For worker-1 in
  the eight-bit layout, inspect:
    cat /shard/13/worker/worker-1/telemetry
  Separate role declaration, lifecycle, artifact identity, runtime receipt and
  proof class (none, host-model, qemu, fresh-pi). A GPU Worker coordinates bounded
  tickets/receipts; CUDA and training execute on the host, never inside the VM.
  LoRA declaration is not evidence of completed training. Kill only the actual
  Worker you created, then inspect teardown. See man kill and man ls.
SWARMUI
  The writable console's write/role/profile gates still apply. It accepts the
  same role and key=value options shown above, for example
  spawn heartbeat ticks=100. The read-only backend refuses spawn. A disabled
  console operation must use the admitted workflow in Operations or Tickets &
  policy, with the required role, transport and profile.

  Durable CUDA recipes use the separate host coh CLI:
    coh plan cuda-reference --recipe --deployment /absolute/recipe.json
  apply/watch/explain/verify/recover use that same journal. These are not root
  console, cohsh or SwarmUI console commands. SwarmUI Operations exposes these
  installed host workflows as reviewed forms. See docs/HOST_TOOLS.md, Recoverable
  CUDA recipes, for exact tickets, signed output verification and cancellation.

  Private LoRA releases also use the host coh CLI:
    coh peft release plan --deployment /absolute/deployment.json
  apply/watch/explain/verify/recover share the same retained release identity.
  Native HF training and serving remain on the CUDA host. A failed canary with
  verified rollback is a failed candidate. See docs/PRIVATE_LORA_RELEASE.md for
  provenance, exact WorkerLora tickets and fresh-authority recovery.

## kill
NAME
  kill — request Worker termination in compatibility mode.
SYNOPSIS
  kill <worker_id>
EXAMPLES
  ls /shard/13/worker
  kill worker-1
  ls /shard/13/worker
DESCRIPTION
  Replace worker-1 with the actual Worker selected for termination. Requires
  Queen authority and an enabled /queen/ctl compatibility path. Production uses
  man authority with cmd {"kill":"worker-1"}. ACK is termination admission;
  subsequent state/removal establishes teardown. It is not a host process kill.

## bind
NAME
  bind — request a Queen namespace binding in compatibility mode.
SYNOPSIS
  bind <src> <dst>
EXAMPLES
  bind /shard/13/worker/worker-1/telemetry /queen/worker-1-telemetry
DESCRIPTION
  Both arguments are absolute namespace paths, not host paths. The destination
  and source must be admitted by the deployment; the example is illustrative.
  Requires Queen authority; target/profile support is independent of parsing.
  Writes {"bind":{"from":"...","to":"..."}} to /queen/ctl.
  Production uses the same command object inside the man authority envelope.
  Inspect the destination after admission. This is not a POSIX bind mount.

## mount
NAME
  mount — request a service namespace mount in compatibility mode.
SYNOPSIS
  mount <service> <path>
EXAMPLES
  mount gpu-bridge /gpu
DESCRIPTION
  service is the deployment's admitted service name; path is an absolute target
  namespace path. The example requires a gpu-bridge service accepted by policy.
  Writes {"mount":{"service":"gpu-bridge","at":"/gpu"}}
  to /queen/ctl. Requires Queen authority and an enabled compatibility path.
  Production uses man authority. This command neither mounts a host disk nor
  starts FUSE; host filesystem access is provided by the separate coh mount tool.

## lifecycle
NAME
  lifecycle — request a bounded Queen lifecycle transition.
SYNOPSIS
  lifecycle <cordon|drain|resume|quiesce|reset>
ARGUMENTS
  cordon: from ONLINE/DEGRADED, enter DRAINING and stop new admission.
  drain: from DRAINING, request bounded drain evaluation; inspect outstanding work.
  resume: request reopening from a state other than ONLINE, subject to target policy.
  quiesce: from ONLINE/DEGRADED/DRAINING, request quiescence.
  reset: request lifecycle reset except during BOOTING; this is not a hardware reboot.
EXAMPLES
  cat /proc/lifecycle/state
  lifecycle cordon
  cat /proc/lifecycle/state
  lifecycle drain
  lifecycle resume
  lifecycle quiesce
  lifecycle reset
  The last four are alternatives requiring the appropriate observed state, not
  an unconditional maintenance script. Do not reset to bypass outstanding work.
DESCRIPTION
  Requires authorized writes to /queen/lifecycle/ctl. The shell validates the
  observed transition and the target enforces its own policy. Read state after
  each admitted transition. A successful write does not establish drain completion.

## telemetry
NAME
  telemetry — upload a local file through bounded Queen telemetry ingest.
SYNOPSIS
  telemetry push <src_file> --device <id>
  telemetry push <src_file> --device=<id>
EXAMPLES
  telemetry push out/operator/sensor.ndjson --device sensor-west
  telemetry push out/operator/sensor.ndjson --device=sensor-west
DESCRIPTION
  src_file is an existing host file; it precedes flags. id identifies the enrolled
  device under /queen/telemetry/<id>. Requires an attached Queen and ingest write
  authority. Limits/chunk sizes and inline versus reference behavior come from
  generated policy. Review the ACK and published device records after upload.
  This is not a general file-copy command. It does not execute uploaded content.
ERRORS
  Missing source, invalid device IDs, unknown flags, oversized records or policy
  refusal stop upload. Inspect retained state before retrying partial ingestion.

## ping
NAME
  ping — check the current session's health.
SYNOPSIS
  ping
EXAMPLES
  attach queen
  ping
DESCRIPTION
  Requires attachment. Not attached is an error, not proof of network failure.
  A pong shows command liveness, not Worker readiness or hardware qualification.
  Root-console ping is local console liveness; host cohsh checks its transport.

## bi
NAME
  bi — show target BootInfo and source-labelled identity summaries.
SYNOPSIS
  bi
EXAMPLES
  bi
DESCRIPTION
  A forwarded diagnostic supported by direct TCP targets. Check kernel/profile
  and image identity before comparing observations. REST is not a general console
  relay; use serial when unsupported. It changes no Worker state.

## caps
NAME
  caps — inspect target capability slots and bounded MCS authority.
SYNOPSIS
  caps [mcs]
EXAMPLES
  caps
  caps mcs
DESCRIPTION
  caps shows capability slots; caps mcs shows generated/live scheduling-context,
  Reply, donor and object authority observations. Counts do not prove temporal
  isolation. Requires target/transport support; use serial if REST refuses it.

## smp
NAME
  smp — inspect scheduler activity and generated/live MCS topology.
SYNOPSIS
  smp [activity|mcs|poll-time|dump]
EXAMPLES
  smp
  smp activity
  smp mcs
  smp poll-time
  smp dump
DESCRIPTION
  No argument and activity show bounded userspace activity, not CPU utilization.
  mcs shows generated and live admission state. poll-time reports Pi root
  elapsed-time, Yield and receive observations; unsupported on other targets.
  dump is a debug-only raw kernel dump, unavailable after linked-UART cutover.
  These are target diagnostics, not host benchmarks. Transport/profile gates
  apply; serial is the recovery surface when a host diagnostic is unsupported.

## nettest
NAME
  nettest — admit a bounded target network self-test.
SYNOPSIS
  nettest
EXAMPLES
  nettest
  netstats
DESCRIPTION
  Run once, wait for its response, then inspect netstats for the terminal result.
  ACK alone is not a completed test. Do not repeatedly re-admit the test while
  it is active. Diagnostic work can affect measured traffic performance.
  Requires a supported target console transport; REST may refuse raw commands.

## netstats
NAME
  netstats — read network counters and network-test progress.
SYNOPSIS
  netstats
EXAMPLES
  netstats
DESCRIPTION
  Inspect the active driver, assigned address/listener, packet counters and last
  network-test result. Profile backend and active physical driver can differ.
  On Pi direct GENET this may perform a bounded owner refresh, so it is not a
  passive performance sampler. A listener alone does not prove authentication.
  Use physical serial if the selected host transport cannot forward it.

## reboot
NAME
  reboot — request an authenticated Queen-authorized platform restart.
SYNOPSIS
  reboot
EXAMPLES
  cat /proc/lifecycle/state
  log dump before-reboot.txt
  reboot
DESCRIPTION
  Drain work according to policy and retain logs first. This disrupts sessions;
  loss of the connection is not proof of a completed reboot. Reconnect and check
  fresh boot identity. Available only when the target/backend supports restart.
  Early bootstrap and unprivileged sessions refuse it. lifecycle reset is a
  different operation. Do not reboot to erase an ambiguous request outcome.

## test
NAME
  test — run host-orchestrated Queen self-tests.
SYNOPSIS
  test [--mode <quick|full|smp>] [--json] [--timeout <seconds>] [--no-mutate]
ARGUMENTS
  --mode: quick (default), full, or SMP diagnostics.
  --json: emit a structured report. --timeout: 1..120 seconds (default 30).
  --mode=value and --timeout=value are also accepted.
  --no-mutate: omit intentional mutation probes; diagnostics still consume work.
EXAMPLES
  test
  test --mode quick --json --timeout 30 --no-mutate
  test --mode full --json --timeout 120 --no-mutate
  test --mode smp --json --timeout 120 --no-mutate
DESCRIPTION
  Attach first and use a transport that supports the selected probes. Full tests
  without --no-mutate may write control records: run deliberately. In scripts a
  failed report fails the command. A host test does not establish fresh physical
  target acceptance or replace the staged Test Plan. Root and SwarmUI do not run
  this host self-test command.

## tcp-diag
NAME
  tcp-diag — inspect a TCP connection without console protocol traffic.
SYNOPSIS
  tcp-diag [port]
EXAMPLES
  tcp-diag
  tcp-diag 31337
DESCRIPTION
  Requires a TCP-enabled cohsh build. Uses the configured TCP endpoint or the
  fallback 127.0.0.1:31337. The optional port is an unsigned 16-bit integer.
  Reuses an active matching console connection where available; otherwise opens
  a socket and reports local/peer addresses. It does not authenticate or attach.
  Do not probe an exclusively owned target while a gateway owns its console.

## pool
NAME
  pool — run a mutating host session-pool benchmark.
SYNOPSIS
  pool bench path=<path> ops=<n> [batch=<n>] [payload=<prefix>]
      [payload_bytes=<n>] [kind=<control|telemetry>] [delay_ms=<n>]
      [inject_failures=<n>] [inject_bytes=<n>] [exhaust=<n>]
ARGUMENTS
  path: authorized append destination; ops: positive operations per phase.
  batch: positive batch size (default 1); payload: nonempty prefix (default pool).
  payload_bytes: optional padded total payload size, within transport bounds and
  at least the generated record length. kind: pool class (default telemetry).
  delay_ms: per-batch delay (default 0). inject_failures: short-write injections
  (default 0); inject_bytes: injected write length (default 8, positive if used).
  exhaust: extra checkout attempts to probe pool exhaustion (default 0).
EXAMPLES
  pool bench path=/log/queen.log ops=10
  pool bench path=/log/queen.log ops=20 batch=2 payload=bench payload_bytes=128 kind=telemetry delay_ms=0 inject_failures=0 inject_bytes=8 exhaust=0
  pool bench path=/log/queen.log ops=20 batch=2 kind=control inject_failures=1 inject_bytes=8 exhaust=1
DESCRIPTION
  Requires an attached, configured session pool and write authority. Appends
  baseline and pooled workloads. Examples are for an isolated test deployment;
  the injection example intentionally stresses failures. TCP bounds concurrency
  to its console ownership; transport support determines injection/readback.
  Reports rates, retries, failures and observed writes. These are host workload
  observations, not raw framed-TCP performance acceptance or physical Pi proof.

## authority
NAME
  authority — prepare production Queen writes and handle uncertain outcomes.
DESCRIPTION
  Check cat /proc/authority and cat /proc/lifecycle/state before mutations.
  Console auth establishes the connection; attach selects role/ticket. REST
  additionally requires request auth and a delegated capability with the exact
  read/write scope. A request-auth token alone is not write authority.
  Production disables /queen/ctl and SPAWN/KILL compatibility shortcuts.
  /queen/intents/ctl accepts queen-intent/v1 with schema, id, idempotency_key,
  issued_unix_ms, writer_epoch and cmd (a JSON STRING containing a command).
EXAMPLES
  At coh>: cat /proc/authority
  In the HOST terminal, replace the observed epoch and create a request once:

    COH_WRITER_EPOCH=REPLACE_WITH_OBSERVED_UNSIGNED_EPOCH python3 - <<'PY'
    import json, os, time, uuid
    from pathlib import Path
    raw = os.environ['COH_WRITER_EPOCH']
    if not raw.isascii() or not raw.isdecimal() or not 0 <= int(raw) < 2**64:
        raise SystemExit('Use the unsigned epoch read from /proc/authority')
    command = {'spawn': 'heartbeat', 'ticks': 100}
    intent = {'schema': 'queen-intent/v1', 'id': 'heartbeat-' + uuid.uuid4().hex,
              'idempotency_key': uuid.uuid4().hex,
              'issued_unix_ms': time.time_ns() // 1_000_000,
              'writer_epoch': int(raw),
              'cmd': json.dumps(command, separators=(',', ':'))}
    payload = json.dumps(intent, separators=(',', ':'))
    if len(payload.encode()) > 2048:
        raise SystemExit('Intent exceeds console payload bound')
    with Path('spawn-heartbeat.coh').open('x', encoding='utf-8') as f:
        f.write('echo ' + payload + ' > /queen/intents/ctl\nEXPECT OK\nquit\n')
    PY

  In the HOST terminal, inspect the file, then validate and submit it with your
  existing deployment credentials and actual target address:
    cohsh --check spawn-heartbeat.coh
    cohsh --transport tcp --tcp-host 192.168.10.50 --tcp-port 31337 --role queen --script spawn-heartbeat.coh
  If a gateway owns TCP, replace the transport/endpoint options with:
    --transport rest --rest-url http://127.0.0.1:8080
  Set request auth via COH_REST_AUTH_TOKEN and delegation via COH_REST_TICKET.
  For direct TCP use the provisioned COHSH_AUTH_TOKEN; do not put secrets in files.
  No shell variables expand at coh>. HOST commands do not run in the UI console.
  To prepare a different new operation, change command and choose a fresh file:
    {'kill': 'worker-1'}
    {'bind': {'from': '/shard/13/worker/worker-1/telemetry', 'to': '/queen/worker-1-telemetry'}}
    {'mount': {'service': 'gpu-bridge', 'at': '/gpu'}}
RECOVERY
  Inspect /proc/queen/dedupe, the log and actual Worker state. ACK is admission,
  not runtime completion. After an interrupted write, retain the exact envelope
  and reconcile before retrying. Authorized retries reuse the same IDs, timestamp,
  epoch and command bytes. Changed bytes under one identity conflict. Dedupe is
  bounded and not a restart-persistent guarantee; absence is not proof of no effect.
  This envelope is for Queen commands only, not every writable control file.

## scripts
NAME
  scripts — validate and run bounded .coh automation from the host.
SYNOPSIS
  cohsh --check <file.coh>
  cohsh <connection-options> --role queen --script <file.coh>
EXAMPLES
  Host terminal: cohsh --check health.coh
  Host terminal: cohsh --transport tcp --tcp-host 192.168.10.50 --tcp-port 31337 --role queen --script health.coh
  health.coh contents:
    ping
    EXPECT OK
    cat /proc/lifecycle/state
    EXPECT OK
    quit
DESCRIPTION
  Use the actual endpoint and deployment credentials. Check validates syntax
  offline, not permission or target readiness. .coh is not Bash: no variables,
  pipes or substitutions. man is local and never requires target attachment.
  Use cohsh --help for launch, trace capture/replay and credential options.
