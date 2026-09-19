<!-- Author: Lukas Bower -->
<!-- Purpose: Define host identity enrollment, gateway-only issuance, action restrictions and ticket wire compatibility. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Host identity mapping

Milestone 27b / `m27b-identity-mapping` maps OIDC JWTs, SPIFFE JWT-SVIDs,
Kubernetes service-account JWTs and local kernel effective uids through
`providers.identity_mappings` in the compiler input. Shipped mappings are
disabled. Enrollment names an exact issuer, one audience, signing algorithm,
public-JWKS SHA-256 allowlist, external subject, required groups, unique
normalized subject, role, paths, provider actions, operation count and TTL.
RS256, ES256 and EdDSA are supported. Tokens cannot provide discovery URLs,
keys, policy, role, lifetime or scope overrides. A write mapping must use the
Queen role and exactly `/host/tickets/spec`; every allowed action must exist
in the compiler-owned provider registry. Read mappings retain path and role
ownership checks. A mapping does not enable an unavailable provider.

`hive-gateway --delegation-key-ref file:/secure/delegation-key
--identity-keysets /etc/cohesix/identity-keysets.json` reads a bounded public
key enrollment once at startup. The JSON object maps enabled policy ids to
absolute JWKS file paths, for example `{"enterprise":"/etc/cohesix/enterprise.jwks"}`.
Duplicate ids, unknown/disabled/local policies, non-regular files, final symlinks,
oversized files and unpinned bytes are refused. Public keys may be distributed
with deployment configuration; the delegation secret remains independently
enrolled. Explicit restart reloads enrollment. Generated mapping or graph
changes invalidate previously mapped tickets.

`POST /v1/identity/exchange` requires ordinary gateway request authentication,
with exactly `mapping_id` and `credential` in its JSON body. It does not require
an existing delegated ticket. The body is limited to 9,216 bytes, the credential
to 8,192 bytes, and concurrent cryptographic verification to four operations.
Verification runs off the async executor. Unknown fields, duplicate required
fields, wrong issuer/audience/groups, forged signatures, unmapped subjects,
future/stale tokens and unsupported remote local-uid assertions fail before
any console or provider request. Public JWKS bytes are pinned; there is no
network discovery. Rotate a pinned keyset by updating compiler enrollment and
restarting with its exact public bytes.

A successful response has schema `cohesix-identity-exchange/v1`, `status=OK`,
`authoritative=false`, `identity_class=gateway_enforced`, `mapping_id`,
`provider_graph_sha256`, `credential_sha256`, `expires_unix_s` and `ticket`.
Every response carries `Cache-Control: no-store`. HTTP 401 means missing/invalid
request authentication; 400 means malformed or bounded input; 403 means identity
or mapping refusal; 429 means the verification capacity is occupied; 503 means
the clock or verification worker is unavailable. The framework rejects oversized
HTTP bodies before issuance. Audit records retain only credential hashes and
fixed refusal classes, never the raw external credential or issued ticket.
Issuance establishes delegated gateway authority, not admission, native execution,
or Worker completion.

The mapped signing domain is derived from SHA-256 of
`cohesix-gateway-mapped-issuer/v1\0 || enrolled-secret`, hex encoded and passed
to the existing BLAKE3 ticket issuer. Thus an issued mapped ticket cannot
authenticate directly to the VM or the ordinary delegated verifier even if
the operator reused the underlying secret. Subjects reserve `mi-` plus
base64url encoding of the first 16 SHA-256 bytes over the serialized tuple
`("cohesix-mapped-delegation/v1", policy_sha256, normalized_subject, provider_graph_sha256)`.
That binding allows the gateway to resolve only the exact current generated
rule. An unknown mapped subject never falls back to ordinary ticket authority.

Both single writes and every line of a batch must name an allowed provider
action before quota charging or upstream dispatch. Duplicate action fields,
trailing JSON and other control paths are refused. Root remains the complete
host-ticket schema/admission validator. Read ownership uses the rule's normalized
subject; ticket accounting uses the original signed token hash. JWT issuer time
and expiry are preserved, so exchanging the same signed claims again cannot
create a fresh quota identity or extend their lifetime. Quotas use the gateway's
existing live-ticket table; issuance does not claim durable accounting across
a gateway restart. Key rotation, missing policy and expiration fail closed.

Local mapping is explicitly host-local. `coh identity --mapping ID --local`
reads this process's kernel effective uid. JWT mapping uses
`coh identity --mapping ID --jwks /absolute/public.jwks` and token bytes on stdin.
Both normally print `cohesix-identity-request/v1`, a verified proposal with no
capability. An already authorized host issuer may add
`--issuer-key-ref file:/secure/delegation-key` to issue the same gateway-only
response and write a credential-hash audit record to stderr. Possession of an
issuer secret is privileged; it is never supplied by a remote exchange caller.
Protect this host, its generated mapping and the credential file as issuer
infrastructure. An externally supplied numeric uid is not local authentication.

The Python `cohesix.exchange_identity` helper accepts the gateway URL, mapping
id, external credential reference, request-auth reference and independently
enrolled provider graph SHA-256. It performs one bounded request, accepts HTTPS
or explicit loopback HTTP, refuses redirects and ambient proxies, and checks
response bindings.
`IdentityTicket.ticket` is excluded from its representation. Pass it explicitly
to `RestBackend(..., delegated_ticket=issued.ticket)`; there is no implicit token
refresh or mutation retry. `cohesix.local_subject()` returns only the kernel euid
for host enrollment diagnostics, never a remote identity assertion.

## Ticket wire version 2

Mapped issuance uses ticket payload version 2 within the unchanged
`cohesix-ticket-<hex-payload>.<64-hex-MAC>` envelope and 224-byte maximum.
Only budget ticks/operations/TTL and `issued_at_ms` use canonical unsigned
LEB128 instead of little-endian fixed-width u64. Each integer is at most ten
bytes; byte ten is at most one. Overflow, truncation and redundant terminal
zero groups are refused. Role, flags, u16-length UTF-8 strings, scope count,
verbs, rates and fixed-width quota fields retain their version-1 layout.
Unknown flag bits, explicit empty scopes/quotas and trailing payload bytes are
noncanonical in version 2. The MAC covers the exact versioned payload.

The shared Rust verifier and Python claim inspector accept both versions.
Ordinary `TicketIssuer::issue` and generated fixtures still emit version 1;
`issue_compact` explicitly selects version 2. Re-encoding preserves the decoded
version, so ticket hash/quota identity cannot change during verification.
Python parsing does not verify a MAC. Tickets too large for the unchanged
console bound fail with `ELIMIT`; multiple scopes are not silently dropped.
This is a versioned handwritten ticket-codec extension, not a change to the
manifest schema, generated ticket defaults, host-ticket request schema, or
Secure9P framing. Manifest schema 1.23 separately introduces the source-scoped
host snapshot interface; it does not change this ticket codec contract.

Focused evidence and the whole-host compatibility review are retained in
[M27b implementation record](audit/M27B_IMPLEMENTATION_RECORD.md).
