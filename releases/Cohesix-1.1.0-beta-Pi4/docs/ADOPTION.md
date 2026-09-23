<!-- Author: Lukas Bower -->
<!-- Purpose: Install and operate selected signed CUDA and LoRA journeys from self-contained host packages with explicit deployment ownership. -->
<!-- Copyright 2026 Lukas Bower -->

# Install and run an approved CUDA or LoRA workflow

A controller runs the CLI/Python tools. A control target runs Cohesix admission and
Workers. A Linux AArch64 NVIDIA CUDA executor owns GPU drivers, weights, data and
native execution. The reference is a Mac controller, Pi control target and Jetson
executor; the selected native LoRA receipt profile is currently QEMU. Pi LoRA release
must be selected and qualified separately. A Linux controller may share a host with
the executor using separate state and credentials. macOS needs no local NVIDIA device,
FUSE or QEMU merely to control a remote enrolled target. CUDA 13.2.2 is the selected
native CUDA reference; the separate pinned HF stack is described in
[Private LoRA release](PRIVATE_LORA_RELEASE.md).

## Verify and install

Obtain a selected `macos-controller`, `linux-aarch64-controller` or
`linux-aarch64-cuda` **1.1.0-beta component package**, an independently trusted matching
`coh` verifier, and the source-bound signer enrollment. These component packages are
not the integrated 1.1 release qualification. The selected profile has exact files,
architecture, versions, schemas, hashes and a signed CycloneDX file inventory.
No private keys, datasets or credentials belong in a package. See
[package trust and services](../packaging/README.md) for the exact trust JSON and
least-privilege service enrollment. Optional breadth remains explicitly unavailable.

```sh
/trusted/coh package verify --input /downloads/package --trust /private/package-trust.json
/trusted/coh package install --input /downloads/package --trust /private/package-trust.json \
  --out /opt/cohesix/selected
```

Use a fresh destination and an operator-controlled parent. The trusted verifier
must match the package compiler graph and selected target policy. Do not trust a
verifier or public key merely because it arrived inside the unverified package.
A missing/unexpected artifact, wrong architecture/profile/version, secret-bearing
manifest, fixture signer, native stub or bad signature fails before installation.
Package owner and service accounts remain distinct. Service accounts receive read-only
package access and write only their private state/CAS; credentials live in a separate
private tree. Installation never activates services or grants host privileges.

The package includes explicit Python source, its build manifest and offline guides.
With Python 3.11+ and the `setuptools>=68`, `wheel` and `packaging` build tools already
installed (use an approved offline wheelhouse when disconnected), build both wheel
and sdist from the **verified signed sources**, then install the checked wheel:

```sh
python3 /opt/cohesix/selected/scripts/install/build_python_package.py \
  --repo /opt/cohesix/selected \
  --inventory /opt/cohesix/selected/contracts/implementation_surface_inventory.json \
  --out /private/python-distributions
python3 -m venv /private/cohesix-venv
/private/cohesix-venv/bin/pip install --no-index --no-deps \
  /private/python-distributions/cohesix-0.2.0a2-py3-none-any.whl
/private/cohesix-venv/bin/cohesix-journey --help
```

The builder copies only the registered product sources, checks every wheel RECORD
and source hash and the complete sdist membership, and writes `distributions.json`.
It needs no source checkout, network, editable install or optional provider packages.
The native package signature authenticates the build inputs; locally built distributions
are checked derivatives, not independently release-signed wheels. Do not substitute
an externally downloaded wheel based on its filename or self-authored digest manifest.
The qualified Python 3.10 HF runtime may use the shipped standalone `hf_native.py`
source; controller SDK installation remains Python 3.11+.

## Configure one operation

Keep enrolled target identity, native artifact/helper digests, admission, source custody
and independent gateway/native/Worker evidence enrollment under operator control.
The package includes [host workflow inputs](HOST_TOOLS.md#recoverable-cuda-recipes),
[LoRA preparation](PRIVATE_LORA_RELEASE.md) and [causal evidence](CAUSAL_EVIDENCE.md).
Use only allowlisted CUDA workload inputs or the pinned native adapter import format;
no request can supply an arbitrary shell command. The shipped examples are under
`tools/cohesix-py/examples/`. CUDA stage files contain the original exact host ticket,
input file, runtime, graph/trust/CAS locations and dependencies. LoRA preparation uses
licensed documents, pinned local base files and independently enrolled source keys.
All input and evidence files are explicit deployment inputs, not package secrets.

Prepare the workload portion of the deployment, with a temporary operation ID, then
calculate its stable identity **before** issuing its admission ticket:

```sh
cohesix-journey identity --kind cuda --purpose nightly-vector-check \
  --rerun initial --deployment /private/deployment.json
# For LoRA select --kind lora. Copy operation_id to the native request and ticket.
```

Set the deployment journal to `/private/ci-state/<operation_id>/journal`, issue the
fresh exact scoped ticket, and freeze the deployment. The journal's parent must be
an owner-only durable directory outside ephemeral runner storage. Write this config
using your absolute installed paths and original operation purpose:

```json
{
  "schema": "cohesix-journey/v1",
  "kind": "cuda",
  "purpose": "nightly-vector-check",
  "rerun": "initial",
  "coh": "/opt/cohesix/selected/bin/coh",
  "deployment": "/private/deployment.json",
  "state": "/private/ci-state",
  "host": "192.0.2.10",
  "port": 31337,
  "auth_ref": "file:/private/credentials/tcp-auth",
  "ticket_ref": "file:/private/credentials/operator-ticket",
  "package": "/opt/cohesix/selected",
  "trust": "/private/package-trust.json"
}
```

`192.0.2.10` is an example address: select your enrolled target. REST uses `rest_url`
instead of `host`/`port` with the gateway caller authentication reference. `kind: lora`
selects the existing PEFT release deployment, comparison and serving verifier.
Keep native and controller CAS/journals bounded, private and durable. Reserve resource
headroom for both helper and serving runtime; CUDA allocation admission is not a hard
GPU partition. Least-privilege service and GPU node allowlists are in the package guide.

## Doctor, plan, approve, run, verify and recover

```sh
cohesix-journey validate --config /private/journey.json
cohesix-journey doctor --config /private/journey.json
coh plan cuda-reference --recipe --deployment /private/deployment.json
# Or: coh peft release plan --deployment /private/deployment.json
# Review exact inputs, target/Worker, finite bounds, evidence trust and rollback baseline.
cohesix-journey run --config /private/journey.json --submit --wait-seconds 900
cohesix-journey run --config /private/journey.json
```

Doctor distinguishes local controller/package drift, secret availability/expiry,
cache/evidence/planner bounds and remote target/executor/runtime/resource observations.
`not_observed` is actionable missing evidence, not health. On the native CUDA host use
`coh --policy /opt/cohesix/selected/config/coh_policy.toml doctor --local-gpu` with the
appropriate global ticket reference. Inspect actual service limits and pinned HF
versions there; an unavailable optional field-bus or FUSE provider does not block CUDA.
Approval and ticket issuance remain the existing authenticated operator workflow;
these tools cannot approve their own requests or mint execution authority.

Watch/verify use the original signed graph and exact output hashes. Retry the same
command/config from a replacement runner with the original durable state. Lost ACK
is pending/ambiguous until independently reconciled. Do not create a new operation or
training job to make a retry green. Explicit rerun requires a new reviewed `rerun`
value and admission. For cancellation, renewed authority or failed-canary rollback,
follow the existing CUDA recovery or LoRA recovery-only contract and retain the failed
candidate. A successfully restored baseline does not mean that candidate succeeded.
The [CI guide](CI_WORKFLOWS.md) specifies exit codes, deadlines and trusted-job isolation.

## Build selected packages from source

Maintainers compile native binaries for the selected production manifest and features
listed in the package guide, retaining exact source/generated digests. Then stage the
explicit registered contents using the public entrypoint:

```sh
python3 scripts/install/stage_host_package.py --repo /src/cohesix \
  --generated-root /src/cohesix --bin-dir /src/cohesix/target/release \
  --profile macos-controller --out /private/package-stage
coh package build --input /private/package-stage --out /private/package \
  --profile macos-controller --source-sha256 "$SOURCE_SHA256" \
  --key-id enrolled-release-key --signing-key-ref file:/private/package-seed
```

`--generated-root` may be an exact selected build overlay with repository-relative
paths. It must match the binaries and verifier; it is not a policy override. Staging
reports `staged-unverified`. Only signed construction and independent verification
establish package integrity. Preserve the distribution build/install and walkthrough
results separately from native workload and complete release qualification.
