<!-- Copyright © 2025 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document worker ticket rationale and minting process. -->
<!-- Author: Lukas Bower -->
# Worker Tickets

**At a glance**
- Worker tickets are the **capability boundary** for worker roles.
- Tickets are presented during `attach` and determine the namespace slice.
- Tickets are distinct from console auth tokens; both may be required.

**Related docs**
- `docs/ROLES_AND_SCHEDULING.md` — role-to-namespace rules.
- `docs/SECURE9P.md` — AccessPolicy enforcement order.
- `docs/USERLAND_AND_CLI.md` — ticket limits and CLI behavior.
- `docs/SECURITY.md` — security constraints and quota limits.

## 1. Why worker tickets exist
Worker tickets are the capability boundary for worker roles. They:
- enforce role-scoped access to Secure9P namespaces (no ad-hoc RPC or shared memory shortcuts).
- bind a worker identity (subject) to the session, so telemetry and leases are attributable.
- carry optional scopes and quotas that NineDoor enforces deterministically.
- preserve the tiny TCB by keeping authorization off the VM network surface.

## 1a. Ticket vs auth token
- **Auth token** (`COH_AUTH_TOKEN` / `COHSH_AUTH_TOKEN`) authenticates the console session.
- **Worker ticket** authorizes the role and namespace slice during `attach`.
- A session can require both; missing either yields deterministic `ERR` on attach.

## 2. Source of truth
- Ticket inventory and per-role secrets live in `configs/root_task.toml` under `[[tickets]]`.
- Ticket limits and quotas are manifest-driven and emitted by `coh-rtc` into canonical docs:
  - `docs/USERLAND_AND_CLI.md` (ticket policy + limits)
  - `docs/SECURITY.md` (ticket quota limits)
- The root task registers the secrets at boot and validates presented tickets during `attach`.

## 3. Ticket structure (claims)
Worker tickets use the `cohesix-ticket` format and are MACed with a BLAKE3 keyed hash. Claims include:
- role (`worker-heartbeat`, `worker-gpu`, `worker-bus`, `worker-lora`)
- budget (ticks/ops/ttl)
- subject identity (required for worker roles)
- mounts (optional)
- issued_at_ms (millisecond timestamp)
- optional UI scopes + per-ticket quotas (must respect manifest limits)

The encoded token string has the form:
`cohesix-ticket-<payload_hex>.<mac_hex>`

## 4. Minting a worker ticket (host-side)
1. Choose the worker role and subject identity (for example, `worker-1`).
2. Look up the role secret from `configs/root_task.toml` (keep this secret off the VM).
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

## 7. Attach flow (operator mental model)
1. Client opens a TCP console session and authenticates with the auth token.
2. Client issues `ATTACH <role> <ticket?>`.
3. NineDoor validates ticket MAC, role, subject, and mount table.
4. On success, the session is bound to the role-specific namespace.

## 8. Common errors and recovery

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

## 9. Security hygiene
- Treat ticket secrets like signing keys; keep them off the VM and out of logs.
- Rotate secrets by updating `configs/root_task.toml`, regenerating artifacts, and restarting the VM.
