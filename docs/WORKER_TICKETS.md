<!-- Copyright © 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Explain Worker session tickets, safe host-side minting and their limits. -->
<!-- Author: Lukas Bower -->
# Worker Tickets

Use a Worker ticket when a client needs a Worker role's namespace view. The
ticket narrows an authenticated session; it does not create, start or prove a
target Worker. [Roles and scheduling](ROLES_AND_SCHEDULING.md) explains the
separate executable lifecycle. [Host action tickets](HOST_TOOLS.md) and
[REST delegated tickets](M27A_AUTHORITY.md) authorize different host-side
paths. Every REST mutation needs a finite scoped delegated ticket, including
one made by a Queen caller. A direct Queen session's optional Worker ticket
does not replace it.

The selected target profiles have executable Worker and baseline isolated
driver authority, but model/session roles alone have no VM capabilities.
Production Worker ticket/lease ledger binding, the complete selected-driver
ledger and structured quarantine remain outside the current selected profile;
the compiler rejects claims that require them. See the [Build Plan](BUILD_PLAN.md)
for the owning work.

For path enforcement and limits, see [Secure9P](SECURE9P.md),
[Userland and CLI](USERLAND_AND_CLI.md) and [Security](SECURITY.md).

The selected QEMU and Pi profiles declare `worker-heartbeat`, `worker-gpu`,
and `worker-lora` as executable target roles. Root constructs their generated
seL4 bundles suspended, and the Worker supervisor resumes a role only after a
separate bounded `/queen/ctl` admission. `worker-bus` remains model-only. A
ticket authorizes a role-scoped session and namespace view; attachment alone
does not load, admit, resume, or prove a target Worker task.

## 1. Why worker tickets exist
Worker tickets are the application-layer authority boundary for worker-role
sessions. They:
- enforce role-scoped access to Secure9P namespaces (no ad-hoc RPC or shared memory shortcuts).
- bind a worker identity (subject) to the session, so telemetry and leases are attributable.
- carry optional scopes and quotas that NineDoor enforces deterministically.
- keep ticket issuance on the host; the target verifies the ticket during
  authenticated attachment.

## 1a. Ticket vs auth token
- **Auth token** (`COH_AUTH_TOKEN` / `COHSH_AUTH_TOKEN`) authenticates the console session.
- **Worker ticket** authorizes the role and namespace slice during `attach`.
- A session can require both; missing either yields deterministic `ERR` on attach.

## 2. Source of truth
- Ticket inventory and credential references come from the selected source and
  resolved manifests under `[[tickets]]`. Checked-in development literals
  are fixtures, not live credentials. The host issuer and target verifier
  receive matching key material from the selected deployment.
- Ticket limits and quotas are manifest-driven and emitted by `coh-rtc` into canonical docs:
  - `docs/USERLAND_AND_CLI.md` (ticket policy + limits)
  - `docs/SECURITY.md` (ticket quota limits)
- The root task registers the secrets at boot and validates presented tickets during `attach`.

## 3. Ticket structure (claims)
Worker tickets use the `cohesix-ticket` format and are MACed with a BLAKE3 keyed hash. Claims include:
- role (`worker-heartbeat`, `worker-gpu`, `worker-bus`, `worker-lora`;
  WorkerBus is a model/session role, not an executable target Worker)
- budget (ticks/ops/ttl)
- subject identity (required for worker roles)
- mounts (optional)
- issued_at_ms (millisecond timestamp)
- optional UI scopes + per-ticket quotas (must respect manifest limits)

The encoded token string has the form:
`cohesix-ticket-<payload_hex>.<mac_hex>`

## 4. Minting a worker ticket (host-side)
1. Choose the worker role and subject identity (for example, `worker-1`).
2. Resolve the role's enrolled issuer secret on the host. Do not copy a
   checked-in development literal into a live deployment.
3. Build `TicketClaims` with the role, subject, budget, and issued_at_ms timestamp.
4. Sign and encode with `TicketIssuer::new(secret).issue(claims)?.encode()?`.
5. Pass the resulting token to `cohsh` or SwarmUI when attaching.

### Example (Rust, host tooling)
```rust
use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer};
use std::time::{SystemTime, UNIX_EPOCH};

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn mint_worker_heartbeat(secret: &str, subject: &str) -> Result<String, cohesix_ticket::TicketError> {
    let claims = TicketClaims::new(
        Role::WorkerHeartbeat,
        BudgetSpec::default_heartbeat(),
        Some(subject.to_owned()),
        MountSpec::empty(),
        unix_time_ms(),
    );
    let token = TicketIssuer::new(secret).issue(claims)?.encode()?;
    Ok(token)
}
```

### Attach with the minted token
```bash
./cohsh --transport tcp --tcp-host 127.0.0.1 --tcp-port 31337 \
  --role worker-heartbeat --ticket "$WORKER_TICKET"
```

## 5. Host tooling shortcuts
- `cohsh --mint-ticket --role worker-heartbeat --ticket-subject worker-1` prints a token and exits.
- `swarmui --mint-ticket --role worker-heartbeat --ticket-subject worker-1` does the same; the UI also exposes a "Mint ticket" button.
- Override secrets with `COHSH_TICKET_CONFIG` / `COHSH_TICKET_SECRET` or `SWARMUI_TICKET_CONFIG` / `SWARMUI_TICKET_SECRET` (fallback to `COHSH_*`).

## 6. Operational notes
- Worker tickets are mandatory for worker roles; queen tickets are optional.
- Subject identity is required for worker roles and is used to build the attach identity.
- Ticket length and quota limits are enforced by `cohsh` and NineDoor; ensure scopes/quotas stay within the manifest limits.
- The TCP console auth token is separate from worker tickets; both may be required in a single session.
- Ticket acceptance proves only session authority. It does not satisfy target
  admission, `READY`, receipt, teardown, QEMU, or fresh-Pi evidence.
- An executable target Worker becomes live only through the generated
  supervisor lifecycle: exact role admission, resume, identity-matched
  `READY`, bounded work, and complete generation-fenced containment.
- Static executable declarations for QEMU and Pi are not execution proof. The
  matching target acceptance record must bind the exact kernel, root image,
  Worker archive, image manifest, role image, and five-part Worker identity.

## 7. Attach flow (operator mental model)
1. Client opens a TCP console session and authenticates with the auth token.
2. Client issues `ATTACH <role> <ticket?>`.
3. NineDoor validates ticket MAC, role, subject, and mount table.
4. On success, the session is bound to the role-specific namespace. This does
   not start or attach a target Worker task; `/queen/ctl` admission and
   identity-matched `READY` are separate operations.

## 8. Worker tickets versus host action tickets

Worker tickets authorize application sessions. The `host-ticket/v1` and
`host-ticket/v2` records on `/host/tickets/*` authorize host-side actions and
are a different contract.

Receipt-bearing GPU lease and PEFT actions require version 2. Root accepts the
caller record only after it matches an exact live `worker-gpu` or
`worker-lora`, then publishes a normalized read-only `spec.snapshot` containing
the resolved slot, lease epoch, supervisor generation, cap generation, and a
globally increasing admission sequence. Root separately derives the executable
Worker control sequence from that exact Worker's last accepted control and
admits at most one unresolved receipt action per identity, so the Worker's
single-slot control page cannot skip or overwrite a control. The host agent
claims only that root-owned snapshot. A terminal result is digest-checked
against the pinned identity before root publishes a confirmed, rejected, or
stale Worker receipt; the host agent never creates or tears down the receipt
Worker.

GPU lease reconciliation reads `/proc/lease/by-id/<operation_id>` rather than
scanning the byte-bounded `/proc/lease/active` aggregate. Each exact lookup is
bounded by the same per-record byte limit, returns either one identity-bound
record or an empty payload, and preserves one unresolved operation per receipt
Worker. This permits independent GPU Workers to hold independent leases
without making transaction recovery depend on an aggregate display window.

## 9. Common errors and recovery

### 1) `ERR ATTACH` with valid auth token
**Signal**
- Attach fails even though the console token is correct.

**Impact**
- Ticket is missing, malformed, expired by policy, or does not match the role/subject.

**Recovery**
- Mint a fresh ticket for the correct role and subject, then retry attach.

### 2) Access denied after attach
**Signal**
- `Permission` or `EPERM` on paths outside the worker’s namespace.

**Impact**
- Ticket scope does not include the requested path.

**Recovery**
- Attach as the correct role or update the mount spec in the ticket.

## 10. Security hygiene
- Treat ticket secrets like signing keys. Keep them out of logs, issues and
  client arguments; the selected target verifier still needs its matching
  key material.
- Rotate a deployed issuer through its selected credential reference and
  rebuild/restart the affected target or gateway according to the owning
  authority contract. Keep the old and new ticket domains separate.
