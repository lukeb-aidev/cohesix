<!-- Author: Lukas Bower -->
<!-- Purpose: Install and run the selected NeMo Agent Toolkit native-client workflows. -->
<!-- Copyright 2026 Lukas Bower -->
# NeMo Agent Toolkit kit (selected Linux AArch64 profile)

This kit pins NeMo Agent Toolkit 1.9.0, MCP SDK 1.29.1 and A2A SDK 0.3.26.
It runs on the Jetson client host, beside but outside the GPU provider runtime.
Hive Gateway connects to the exact-source Queen and owns Cohesix admission,
standing budgets and receipts. NeMo calls the gateway through its native MCP
and A2A clients; the model endpoint only plans calls. It is never an authority
or a substitute for native provider verification. Apple, NIM, NeMo Framework,
Triton and Kubernetes are not required by this selection.

## Install from a sealed candidate

Build the wheel once on a trusted packaging host:

```sh
python3 -m pip wheel --no-deps --wheel-dir dist integrations/nemo-agent-toolkit
shasum -a 256 dist/cohesix_nemo_kit-0.1.0-py3-none-any.whl
shasum -a 256 integrations/nemo-agent-toolkit/requirements-linux-aarch64.lock
```

Copy the wheel, `requirements-linux-aarch64.lock` and
`scripts/install/install_nemo_agent_toolkit.py` to the Linux AArch64 client.
Verify their recorded digests through the chosen artifact channel. The
installer accepts absolute paths and creates a new venv; it never installs
from an editable checkout:

```sh
python3 install_nemo_agent_toolkit.py \
  --venv /absolute/private/nemo-client-1.9.0 \
  --wheel /absolute/kit/cohesix_nemo_kit-0.1.0-py3-none-any.whl \
  --sha256 EXPECTED_WHEEL_SHA256 \
  --lock /absolute/kit/requirements-linux-aarch64.lock \
  --lock-sha256 EXPECTED_LOCK_SHA256
```

The lock names every selected transitive distribution at its tested version.
Use a new venv for a different Linux, Python or Toolkit release and rerun the
native compatibility checks. `pip check` and the installed console command
must pass before connecting to Cohesix. Keep the accepted CUDA/PEFT container
and the Toolkit client venv separate.

## One subject per process

Give each Toolkit process a private settings JSON file, mode `0600`, and a
gateway credential pair minted for the same delegated subject. Example values
below are references only:

```json
{
  "gateway_url": "http://127.0.0.1:8415",
  "subject": "operator-a",
  "request_auth_ref": "file:/absolute/private/request-auth",
  "delegated_ticket_ref": "file:/absolute/private/operator-a.ticket"
}
```

The gateway URL must be HTTPS or loopback HTTP over a protected host channel.
`subject` labels NeMo's local per-user context; the verified delegated ticket
decides Cohesix authority. Use a second process, credential pair and settings
file for a second user. Client-supplied user IDs cannot grant a Cohesix scope.
Do not forward a gateway ticket, request token or model key to the CUDA
executor. Do not put credentials in tracked YAML or shell arguments.

Run `cohesix-nemo doctor --settings /absolute/private/settings.json` and the
read-only `cohesix-nemo mcp` and `cohesix-nemo a2a` discovery modes first.
The MCP path requires the selected `available_selected_jobs`, `preflight`,
`submit`, `inspect` and `recover` tools. The A2A path requires a scoped Agent
Card with the selected CUDA or PEFT skill. An optional service is unavailable
when absent; discovery never invents it.

## Native-client workflows

Prepare a private request JSON with `scope_id` and a complete immutable
`host-ticket/v2` object from the selected Cohesix workload or PEFT recipe.
The same ticket ID and idempotency key must be used through recovery. Then:

```sh
cohesix-nemo mcp --settings /absolute/private/settings.json \
  --request /absolute/private/cuda-request.json \
  --output /absolute/private/mcp-cuda-submit.json
cohesix-nemo mcp --settings /absolute/private/settings.json \
  --recover-id ORIGINAL_TICKET_ID \
  --output /absolute/private/mcp-cuda-recovered.json

cohesix-nemo a2a --settings /absolute/private/settings.json \
  --request /absolute/private/peft-request.json \
  --output /absolute/private/a2a-peft-submit.json
cohesix-nemo a2a --settings /absolute/private/settings.json \
  --get-task-id ORIGINAL_TICKET_ID \
  --output /absolute/private/a2a-peft-recovered.json
```

The MCP path preflights, submits once and reconnects for the same admission.
The A2A path sends one data part and uses NeMo's native `get_task` helper.
`--cancel-id` issues NeMo's native cancellation helper for the inspected task;
it does not claim native termination. Timeout or lost ACK is uncertain until
the original ID is reconciled. Output files are create-only and `0600`; repeat
a read with a fresh output path. Typed client observations carry the original
ID and result digest, but `provider_verified` remains false for A2A. Run the
independent CUDA byte verifier or `coh peft release --deployment ... verify`
against the native record before reporting success. A failed candidate or
rollback remains `recovered_failure` under the shared verifier.

## Model-backed NeMo agents

The MCP configuration uses Toolkit's `tool_calling_agent`; the A2A
configuration uses its `per_user_react_agent` because a per-user function
group requires a per-user workflow in Toolkit 1.9.0. Both use a pinned
OpenAI-compatible model endpoint. `mcp-agent.yaml` uses the native
`mcp_client` function group with the selected tool list, 30 second call timeout
and one bounded reconnect. `a2a-agent.yaml` uses a per-user native A2A group.
Toolkit 1.9.0 fetches its Agent Card before applying its auth interceptor, and
its generic text sender cannot produce Cohesix's required data part. The
installed `cohesix_a2a_client` plugin supplies only those two wire adaptations
while retaining Toolkit's own card, task lookup and cancellation helpers.
This kit uses separate protected MCP and A2A processes. A combined process is
not advertised without its own authenticated startup test.

Make a private `0600` model settings file after pinning and loading the chosen
model, for example:

```json
{
  "base_url": "http://127.0.0.1:8080/v1",
  "model_name": "SELECTED_LOADED_MODEL_ID",
  "api_key_ref": "file:/absolute/private/model-api-key"
}
```

The launcher probes `/v1/models` for that exact name. Provide a private
`0600` input file containing one fixed task and ticket identity, then run
`cohesix-nemo agent-mcp` or `cohesix-nemo agent-a2a` with `--settings`,
`--model-settings`, `--input-file` and an unused absolute `--trace-log`.
The trace is private because it may contain the task and model output.
These model traces explain planner behavior; they do not replace Cohesix's
signed native outcome. The launcher exposes Toolkit's `nat eval` and profiler
through three modes. Prepare a private `0600` JSON array of one to sixteen
`{"id":"...","question":"...","answer":"..."}` rows, then run the same
file and model settings through direct and governed configurations:

```sh
cohesix-nemo eval-direct --settings /absolute/private/settings.json \
  --model-settings /absolute/private/model.json \
  --dataset /absolute/private/fixed-task.json \
  --eval-dir /absolute/private/eval-direct-01
cohesix-nemo eval-a2a --settings /absolute/private/settings.json \
  --model-settings /absolute/private/model.json \
  --dataset /absolute/private/fixed-task.json \
  --eval-dir /absolute/private/eval-a2a-01
```

`eval-mcp` is also available for a fixed MCP question. Each output directory
must be unused and private; it retains NeMo's native
`.tmp/nat/examples/default/standardized_data_all.csv`, profiler traces and a
private `nat-eval.log`. Inspect `WORKFLOW_START/END` and `TOOL_START/END` in
the CSV and independently check the task ID and signed result. A zero eval
exit code is insufficient if no dataset row or tool call ran. Use the fixed
quality and latency budgets in [BENCHMARKS.md](../../docs/BENCHMARKS.md#nemo-agent-toolkit-190-comparison-m28f).

For two subjects, run two isolated Toolkit processes and repeat both protocol
discovery and cross-subject lookups. The gateway must refuse another subject's
task, adapter and evidence even if the caller changes NeMo's `user_id`.

## Reproduction record

Record the wheel and lock SHA-256, native package versions, gateway/Queen
source and image identity, selected manifest, model name and endpoint, subject
labels and credential modes, exact ticket IDs, request/response trace paths,
native verifier result, denied action, timeout and restart observations.
Report setup effort, credential exposure, control latency, manual recovery and
evidence utility for identical direct and governed jobs. Retain failures and
unknowns. The M28f conformance case consumes those records; package presence,
mock completion and model text are insufficient.
