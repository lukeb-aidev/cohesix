<!-- Author: Lukas Bower -->
<!-- Purpose: Define enrolled host custody, immutable causal records and the distinct proof carried by native admission witnesses. -->
<!-- Copyright 2026 Lukas Bower -->
# Causal evidence custody

Milestone 27b, `m27b-authoritative-receipt-and-evidence-core`, uses
`cohesix-evidence` for canonical signed records and bounded verification. A
signed record is accepted only with independently configured public keys,
phase custody, exact binding, unexpired chronology and verified CAS bytes.
The final verifier also requires a unique terminal phase and all causal
predecessors. A running journal cannot pass terminal verification.

The shared producer supports three custody classes. Gateway admission custody
owns intent, facts, approval and grant observations. Native operation custody
owns native execution, observation, verification and terminal observations.
Worker witness custody owns Worker and terminal observations. An enrollment
must select a subset of its application's custody. Native keys cannot sign
grants; gateway keys cannot sign native completion.

Root currently exposes manifest measurements with no device signing provider.
The gateway's signed Root observation therefore records
`device_attested=false`. It proves what the enrolled gateway observed through
its authenticated console connection. It does not prove device attestation.
Likewise a native provider's terminal witness sets `worker_proof=false` and
cannot satisfy an executable Worker requirement.

## Enrolled native requests

`hive-gateway --evidence-enrollment-dir DIR` and
`host-ticket-agent --evidence-enrollment-dir DIR` select private operator-owned
enrollments. Without this option their existing transports remain available;
unsigned operation reports cannot enter a signed causal graph. Mock mode
refuses the signing option. Native v1 supports systemd, Docker, Kubernetes,
MODBUS and DNP3 actions. GPU workload v2 operations additionally require
`host-ticket-agent --worker-evidence-enrollment-dir DIR`. This independently
provisioned key may sign only Worker and terminal phases. Gateway, native and
Worker witness public keys must differ; two differently named references to
one key do not establish separate custody. Native v2 keys select execution,
observation and verification; the Worker key owns the final terminal phase.
Other v2 action families without a native producer return typed unavailable
when signing is selected.

Each enrollment file is named by the SHA-256 of the compact JSON array
`[ticket_id,idempotency_key]` and uses `cohesix-producer-enrollment/v1`:

| Field | Contract |
| --- | --- |
| `store` | Existing absolute private directory owned by the custodian UID. |
| `key_id` | One key in the independently provisioned trust policy. |
| `signing_key_ref` | `env:NAME` or `file:/absolute/path`; a hex Ed25519 seed resolved locally. |
| `trust` | `cohesix-evidence-trust/v1`, exact expected binding and public custodian keys. |
| `expires_unix_ms` | Fixed expiry, no later than the request's explicit expiry. |

Public trust is provisioned separately from requests and evidence packs. The
gateway and native host receive the same binding and public keys but retain
different private key references. Enrollment files and stores use mode 0600
and 0700 respectively. Keys never appear in HTTP bodies or evidence packs.
The gateway binds `subject` to the SHA-256 identity of the canonical verified
delegated ticket. Native producers preserve that identity from the verified
gateway prefix. Required component entries are `hive-gateway` and
`host-ticket-agent`, measured from each running executable at startup. The
compiled provider graph, target manifest, action and writer epoch must match
the enrollment. Implementation and use-case graph hashes remain independently
pinned verification inputs.

The gateway reads `/proc/boot` without its read cache and checks the manifest
measurement. After delegated authorization it retains the exact request,
Root facts and authorization decision. It signs the grant only after
the authenticated Root readback matches every caller field. V1 uses
`/host/tickets/spec`; v2 uses `/host/tickets/spec.snapshot` and requires exactly
the Root-resolved slot, lease epoch and admission sequence. The caller cannot
supply these fields. The grant retains the exact pending Worker-current record
before execution. GPU workload facts additionally retain the Root-published
native GPU identity and, for submit, its active lease/resource projections.
These observations do not create a reservation or a Milestone 28a decision.
A transport ACK is insufficient. Retry preserves the original facts, Worker
sequence, signatures and expiry.

Move verified prefix records and their immutable CAS objects to the native
custodian's private store before starting its ticket agent. This transfer
does not convey a private key or enroll trust. The native agent verifies the
entire prefix and exact request before native I/O. It requires enough time
for the generated provider deadline plus one second for result publication.
An existing execution phase requires reconciliation; it cannot dispatch again.
GPU cancel dispatch is followed by read-only `control_status` queries carrying
the original admitted cancel binding. Agent restart uses the same query and
retained native result; it never changes cancel authority into an observe action
or dispatches cancellation again to manufacture a receipt.
The agent's existing durable execution journal remains the owner of replay
and restart decisions.

Native observations retain unit/job/invocation, container/start or Kubernetes
UID/resource-version identity. A read of the Docker Engine itself identifies
the configured local Unix API source. `resource_generation` is the enrolled
agent writer epoch; native incarnation identifiers remain explicit in the
identity and hashed payload. These fields must not be represented as a
hardware generation counter. Execution records describe the native object
observed by the adapter, with its captured before/after state and event refs.
They do not invent a dispatch timestamp.

The agent publishes the native observation hash through its existing durable
ticket result. Only an exact Root result readback, matching the observation
hash, allows a signed terminal record. Before a retry appends its successful
result, it checks for the exact existing terminal row; a lost ACK cannot
create a second terminal append. Conflicting or duplicate terminal rows are
refused. GPU submission success, failed native execution, cancellation, revoke
and bridge interruption retain the original durable job time and native identity.
Cancel/observe action receipts describe their own verified postcondition and do
not change the original submit result. Validation failures or unobserved errors
retain ordinary failed/pending reports and an incomplete causal chain; terminal
exporters refuse them.

For signed v2 operations, native publication is durable before the Worker
witness is attempted. Cursor advancement and journal compaction wait for the
witness. It requires an exact Root result, matching native artifact and outcome,
unchanged Worker id/role/slot/lease/supervisor/cap generations and admission
sequence, a completed receipt newer than the grant's pending snapshot, and no
outstanding control. A retained Worker phase can finish its terminal append
after restart without replacing the original witness. Missing or delayed Worker
evidence never causes native work to execute again. The Worker image hash is
independently enrolled with the selected Root artifact; it is not measured by
this console protocol, and the witness retains `device_attested=false`.

## Retention and verification

Each store contains a shared `cas/` and a directory named by the canonical
binding hash. An operation directory contains `owner.lock` and `graph.json`.
The process holds the operation lock for each update; a store lock serializes
capacity admission and CAS writes. Files reject symlinks, nonregular types,
hard links, wrong ownership and public permissions. Signed graph replacement
fsyncs the file and parent directory. Missing graph state in an existing
operation directory fails closed. Imported prefixes may extend existing
history but cannot replace it. Duplicate phases cannot issue new signatures.

Bounds are 256 KiB per graph, 64 nodes, eight artifacts per node, 64 MiB per
artifact, 4096 store entries and 4096 CAS entries. Custodians must provision
finite private storage and retain incomplete journals for reconciliation.
Large native logs remain immutable CAS objects. Nothing fetches an arbitrary
URL found in a graph.

Verify a completed graph through `coh evidence verify --input GRAPH --trust
TRUST --cas CAS`. Live verification uses a current verification time; offline
reconstruction supplies its explicit historical verification time. The
result and exporter projections remain derived views of the verified signed
records. They cannot be imported as new authoritative receipts.
