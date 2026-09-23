<!-- Author: Lukas Bower -->
<!-- Purpose: Specify signed evidence verification, enrollment trust and optional-device behavior. -->
<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
# Signed device evidence

This contract belongs to Milestone 26 task
`m26-device-identity-attestation-closure` and Milestone 27 task
`m27-attestation-verifier`. `cohesix-attestation` owns the bounded wire parser,
nonce state, TPM quote checks and host certificate verifier. `coh`,
`coh-status` and SwarmUI consumers share this vocabulary.

## Selected profiles and the Pi 4 decision

The stock Pi 4 has no onboard TPM. U-Boot measured boot needs a connected TPM;
enabling a build option cannot supply a missing device. The owner decision of
2026-09-14 excludes the current Pi 4 from the **positive signed-device
acceptance requirement**. The selected Pi manifest uses
`mode = "measurement_only"`, `required = false`. It remains subject to normal
Pi boot, operator, network and trace acceptance. Secure boot and DICE are not
enabled or inferred by this exclusion. Adding a TPM module or a provisioned
DICE boot chain requires a separately declared, isolated device owner and
fresh enrollment before changing this classification.

Schema 1.20 replaces `enabled`/`tpm-or-dice` with explicit
`disabled|measurement_only|tpm2_quote|dice_evidence` modes. It also specifies
`required`, `evidence_max_bytes`, `challenge_max_bytes`, `max_age_ms`,
`trust_anchor_ref`, and `ticket_key_policy`. No automatic fallback exists.
The selected QEMU profile is disabled; the Pi profiles are measurement-only.
Both advertise that a signed-evidence provider is unavailable. The compiler
rejects required unsigned modes, missing TPM declarations, missing secure
boot/trust enrollment, literal ticket secrets in a signed profile, and signed
modes with no implemented device provider. Root independently refuses required
or signed modes before generated ticket registration. Optional profiles retain
their existing development ticket keys and report `development_static`.

There is currently no isolated TPM issuer, DICE CDI handoff or sealed-ticket
runtime in these selected images. The host verifier does not supply those
facilities. Their implementation and physical qualification remain owned by
the reopened Milestone 26 device task; this contract does not claim that task's
full production admission definition of done.

## Namespace and request lifecycle

`/proc/attest/capabilities` reports schema
`cohesix-attestation-capabilities/v1`, booleans `signed_evidence` and
`challenge`, and a bounded reason. `/proc/attest/status` reports
`cohesix-attestation-status/v1`, mode, required state, signed-evidence
availability and ticket-key classification. The selected unavailable profiles
expose only these two read-only nodes. No dummy evidence or writable challenge
is advertised.

A provider implementing this contract exposes `/proc/attest/challenge` and
`/proc/attest/evidence` through existing authenticated Secure9P operations.
The challenge is one append of at most 256 bytes, with exactly:

```json
{"schema":"cohesix-attestation-challenge/v1","nonce":"<64 lowercase hex>","context_sha256":"<64 lowercase hex>"}
```

The verifier obtains the 32-byte nonce from the host OS CSPRNG and issues it
once. Failure, short write, invalid response and success all consume that
request. There is no retry or authority/configuration mutation. A provider
must enforce per-authenticated-session correlation, one outstanding request,
generated rate/byte/time bounds and one uniquely correlated response; it must
never forward arbitrary TPM commands. A consumer must not enable a production
provider merely because it can return this JSON.

The host reads at most 8192 encoded evidence bytes. Live timing uses monotonic
elapsed time, anchored at host issue time, and rejects wall-clock rollback.
The policy window is 1–60000 ms, inclusive at its configured upper limit.
The TPM clock is not interpreted as Unix time.

## TPM2 evidence v1

`cohesix-attestation-evidence/v1` contains `kind = "tpm2_quote"`, `context`,
`quote`, `signature`, and `certificates`. All binary fields use canonical
lowercase hex. Unknown fields, unsupported modes/algorithms, malformed lengths
and trailing TPM bytes are rejected. The quote is the exact `TPMS_ATTEST`
byte string, excluding the TPM2B length prefix; the signature is exact
`TPMT_SIGNATURE`. The supported suite is ECDSA P-256 / SHA-256. Certificates
are DER X.509, leaf AK first, at most four certificates of 2048 bytes each.
The provider never supplies a trusted root.

The context contains a proof `class`, 32-byte `device_id` and `boot_id`, and
eight mandatory artifact SHA-256 identities: `firmware`, `uboot`, `kernel`,
`root_task`, `runtimes`, `resolved_manifest`, `image`, `dtb`. None is a wildcard.
The context digest hashes these bytes in this exact order:

1. ASCII `cohesix-attestation-context/v1` followed by NUL.
2. One class byte: Pi device `1`, QEMU virtual `2`, offline fixture `3`.
3. Device id, boot id, then the eight artifact digests in the order above.

TPM Quote `qualifyingData` is SHA-256 of ASCII
`cohesix-tpm2-quote/v1`, NUL, the 32 nonce bytes, then the 32 context-digest
bytes. Thus neither JSON whitespace nor object key ordering affects the
signature binding.

Verification checks the TPM-generated magic, quote type, enrolled qualified
AK Name, qualifying data, safe clock, expected reset/restart epoch and clock
floor. It requires exactly one SHA-256 PCR bank, a three-byte selection bitmap,
strictly increasing selected PCR indices 0–23, and the SHA-256 digest of their
independently expected values. All eight artifact identities must match the
verifier context. Enrollment must bind that expected PCR policy to the actual
measured boot chain: caller-supplied artifact names alone are not measurements.

## Enrollment and trust policy

`--trust-policy` is an independently supplied
`cohesix-attestation-trust/v1` JSON file, at most 32768 bytes. The canonical
offline fixture demonstrates its exact shape. Policy includes context, trusted
DER CA roots, a pinned AK certificate SHA-256, enrolled qualified AK Name,
PCR values, policy validity window, nonce age limit, reset/restart counters,
minimum TPM clock and a bounded certificate SHA-256 revocation list.

The verifier checks the leaf certificate against its independent pin and
validates its chain, validity and required TCG AIK EKU `2.23.133.8.3`, then
verifies the quote signature. It rejects revoked roots, intermediates or leaf
certificates. CA roots are trust anchors; their permitted lifetime is bounded
by the enrollment policy. There is no network CA/CRL fetching or trust-on-first-use.
Rotate policy and AK pins through the enrollment owner, invalidate old policy
windows and publish explicit revocations. A reset or unexpected restart epoch
requires a new enrolled policy; the verifier never silently resets its floor.

Real enrollment must establish AK possession and device provenance against
the expected endorsement root (including credential activation where
applicable), the restricted signing-key attributes, and the measured boot/PCR
mapping. A locally generated CA or a self-reported device label supplies no
physical TPM provenance. Fixture trust roots may only authorize fixture class.
DICE certificates are deliberately unsupported until a real device-bound CDI
chain and its specific evidence profile are implemented; a generic signature
is never relabelled as DICE.

## Commands and retained records

```sh
coh attest --host <target> --trust-policy <enrolled-policy.json> \
  --record out/attestation-record.json
coh evidence pack --host <target> --out out/incident \
  --attestation-record out/attestation-record.json
coh attest --input out/incident --trust-policy <enrolled-policy.json> --json
```

The optional record is attached at `attachments/attestation-record.json` with
the existing pack inventory and digest rules. It retains the public challenge,
host issue/response times and canonical evidence envelope; the signed TPM and
DER bytes remain unchanged. Arbitrary diagnostic strings and unknown fields
cannot enter this attachment. The envelope digest is over its canonical compact
JSON serialization. Policy is supplied separately, never trusted from the pack.

Result schema `cohesix-attestation-result/v1` retains verdict, reason, source
and evidence classes. Additive `verified` binds evidence, policy and nonce
digests plus clock/reset/restart values; `proof_scope` is `none`,
`offline-signature`, or `live-signature`. PASS means signature verification
within that explicit class and scope. Offline verification uses recorded time
and verifies a historical statement; it does not authenticate those host
timestamps or prove present device state. A fixture never becomes live evidence.
Absent providers return UNAVAILABLE; unsigned measurements and invalid
evidence fail. Only PASS exits zero. Python validates and projects this record
structurally; it never verifies or upgrades its authority.

## Verification and references

Independent Python cryptography/OpenSSL fixtures exercise the exact TPM wire
format and X.509 chain, with fixed offline timestamps. Tests cover nonce reuse,
deadline boundaries, every artifact identity, proof-class substitution, PCR
selection/value changes, reset/clock rollback, wrong/revoked/expired keys,
signature corruption, truncation, trailing bytes and public-hash substitution.
These fixtures are cryptographic conformance evidence, not target execution.

Normative external structures:
[TCG TPM Library Part 2](https://trustedcomputinggroup.org/wp-content/uploads/TPM-Rev-2.0-Part-2-Structures-01.38.pdf),
[TCG AIK enrollment](https://trustedcomputinggroup.org/wp-content/uploads/IWG_CMC_Profile_Cert_Enrollment_v1_r7.pdf),
[TCG DICE architecture](https://trustedcomputinggroup.org/resource/dice-attestation-architecture/).
Platform references:
[Pi 4 specifications](https://www.raspberrypi.com/products/raspberry-pi-4-model-b/specifications/),
[U-Boot measured boot](https://docs.u-boot.org/en/v2024.04/usage/measured_boot.html).
