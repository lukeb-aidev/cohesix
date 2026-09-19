<!-- Author: Lukas Bower -->
<!-- Purpose: Specify signed host package trust, exact deployment inputs and explicit service enrollment. -->
<!-- Copyright 2026 Lukas Bower -->

# Host deployment packages

The compiler emits `contract.deployment_profiles` in
`configs/generated/provider_registry.json`. The selected profile owns every
relative artifact path, format, architecture, version, schema and credential
name. Packages contain host software only. Their signatures attest exact files;
they do not assert provider execution, GPU isolation or an enrolled deployment.

Build host binaries with the selected **production** manifest and the same
generated registry and policies used by the verifier. Enable `coh/fuse` for
controller profiles and `gpu-bridge-host/rest` for CUDA profiles. Build `sidecar-bus`
with `live,modbus,dnp3`; its protocol implementations remain host-only. Build the
CUDA reference child with `scripts/build-gpu-reference.sh` on the enrolled
Linux CUDA 13.2.2 host. A developer manifest, compiled native object/dylib,
wrong architecture, changed generated configuration, or embedded ticket secret
is refused. The signer attests each native binary's source version; inspecting
an ELF or Mach-O header cannot establish source semantics or infer a Cargo
version. Enroll the source inventory digest only after reviewing the exact
build inputs and selected features; never sign a mock/stub build as a release.

Stage exactly the profile paths in a fresh directory, excluding
`package.sbom.json`, which the builder creates. Copy `bin/*` from the exact
host build, `config/*` from that build's selected generated configuration,
`contracts/provider_registry.json` from that build's generated registry, and
`packaging/*`, `scripts/install/render_host_services.py` and
`resources/openapi/hive-gateway.yaml` from the matching source inventory.
Do not recursively copy a repository or a credential directory. Missing and
unexpected files, symlinks, special files and group/world-writable files fail.

Use an Ed25519 signing seed supplied by an explicit secret reference. Keep
the seed and the independently enrolled trust policy outside the package:

```sh
coh package build --input /srv/cohesix/stage --out /srv/cohesix/package \
  --profile linux-aarch64-controller --source-sha256 "$SOURCE_SHA256" \
  --key-id release-2026 --signing-key-ref file:/private/signing/package-seed
coh package verify --input /srv/cohesix/package --trust /private/package-trust.json
coh package install --input /srv/cohesix/package --trust /private/package-trust.json \
  --out /opt/cohesix/release-1
```

The trust JSON has exactly `schema` (`cohesix-host-package-trust/v1`),
`profile_id`, `version`, `source_sha256`, and `keys` (key-id to lowercase
hexadecimal Ed25519 public key). The trusted `coh` verifier must carry the
matching compiler graph. It executes no package binaries while verifying.
Verification permits inspecting a foreign architecture; installation requires
matching OS/architecture and a fresh destination under an operator-controlled
parent. A cooperative directory lock and private staging directory protect
publication. Existing installations are never replaced or activated implicitly.

Installation initially keeps its directory private to the invoking account.
When a distinct service account needs access, an administrator must grant that
account read/traverse access to the verified tree while retaining an independent
package owner: for example, a dedicated read-only service group, directories
and executables mode `0750`, and data files mode `0640`. Re-run verification
after changing ownership or permissions. The service account writes only its
separate state tree and cannot replace installed executables or configuration.

`package.json` is a canonical, domain-separated Ed25519 signed manifest. It
binds the complete profile, provider graph, source inventory and per-file size
and SHA-256. `package.sbom.json` is a CycloneDX 1.6 **file inventory** with exact
names, versions and hashes. It excludes its own circular digest and external
OS/driver/runtime dependencies; it is not a vulnerability assessment. The
manifest authenticates the SBOM itself. Native images require bounded
executable segments and an entry point, not just a magic number.

For Python, use `scripts/install/build_python_package.py`; its separate exact
wheel/sdist manifest and `scripts/ci/python_compat_run.sh --wheel-smoke` check
distribution contents and clean interpreter installation. Native profiles do
not embed an unvalidated wheel.

## Service enrollment

The package includes templates and a bounded renderer. Rendering verifies the
package with an already trusted `coh`, validates substitutions, and writes a
fresh directory. It does not create users, copy secrets, enable services or
change host device permissions. Use distinct service accounts and narrowly
scoped tickets. Create private state directories (`agent`, `cas`, `exports`,
`adapters`, `models`, `evidence`, `siem`; additionally `gpu/requests` for CUDA)
owned by the service account before activation. Linux package/state/credential
paths must remain outside homes because the units set `ProtectHome=yes`.

Enrollment TOML has this shape; every path is absolute and excludes spaces,
shell metacharacters and systemd substitution characters:

```toml
schema = "cohesix-host-service-enrollment/v1"
user = "cohesix"
group = "cohesix"
state = "/var/lib/cohesix"
credentials = "/etc/cohesix/credentials"
target_host = "127.0.0.1"
target_port = 31337
```

`credentials` contains private `tcp_auth`, `root_ticket`, `request_auth`,
`delegation_key`, `agent_ticket`, and (Linux) `sidecar_ticket` files.
These map to uppercase names in the profile's `required_credentials` list.
The gateway's upstream capability and each caller's delegated ticket are
separate credentials. Restrict delegated scopes/TTLs to each service's purpose.
Secret references fail when missing, malformed or placeholders. An environment
reference can resolve through one `file:` reference, which supports systemd
credential injection; reference chains and fallback to other sources fail.

```sh
python3 /opt/cohesix/release-1/scripts/install/render_host_services.py \
  --verifier /trusted/coh --package /opt/cohesix/release-1 \
  --trust /private/package-trust.json --enrollment /private/service-enrollment.toml \
  --out /etc/cohesix/services
```

Review generated files and use the host service manager to install/activate
the chosen services. Linux units require systemd with `LoadCredential` support
and enforce no new privileges, empty capabilities, read-only system paths,
private temporary storage and bounded resources/restart rates. Only the
enrolled state root is writable. CUDA units additionally require an exact
`gpu.device_nodes` allowlist of native character devices, `gpu.id`, and the
32-character lowercase native `gpu.uuid`. The renderer binds helper digest,
writer epoch and provider graph from the verified package and creates separate
executor/inventory configuration files. Both share the explicitly enrolled
`gpu_mac`; the inventory publisher uses `gpu_inventory_ticket`. The agent
reserves its control lane and cannot bypass root admission through this socket.

The systemd sidecar template selects its compiled source's provider list. Its enrollment
requires `snapshot_source` to name a compiled host snapshot publisher and uses
the private state directory for durable sequence reservation. The delegated
sidecar ticket must allow its exact `/host/snapshots/<provider>/<source>/ctl` paths. Additional
native actions need their specific polkit/API policy and protocol evidence.
Do not grant a general Docker socket or broad sudo access merely to make a
provider available. Device and provider access remain external enrollment
requirements; package validation cannot grant or prove them. The SIEM instance
unit consumes `evidence/<ticket-id>/graph.json`, independently enrolled
`evidence-trust.json` and a `siem_token`; its destination must be enabled in the
generated SIEM policy. A missing destination remains `not_enabled`.

Field-bus enrollment uses `field_bus = true` and
`evidence_enrollment_dir = "/var/lib/cohesix/enrollments/native"`. The renderer
requires compiled endpoint maps and gives the agent the private `state/bus`
WAL. Only exact serial devices from the verified registry become `DeviceAllow`
entries; TCP-only agents retain `PrivateDevices=yes`. The publisher reads that
WAL without opening devices, preserves ACK expiry and withdraws stale values.
Controls require independently signed exact grants. See
[FIELD_BUS.md](../docs/FIELD_BUS.md) for supported functions and recovery bounds.

Signed GPU workload v2 operations additionally select
`worker_evidence_enrollment_dir = "/var/lib/cohesix/enrollments/worker"` alongside
the native enrollment directory. The renderer requires separate directories;
the running agent verifies distinct gateway/native/Worker public keys and exact
operation bindings. Stores referenced by these enrollments stay within the
writable private state tree. Keys and enrollment files are deployment inputs,
never package artifacts. The witness signs Root's authenticated completion
projection; it does not claim device attestation.

macOS templates are per-user LaunchAgents with explicit activation and core
dumps disabled. They use the current user's private state and credential
files. `launchd` is a supervisor, not a sandbox; the user's account and
capability scopes define authority. Arrange bounded host log rotation for the
selected stdout/stderr files. Validate a rendered plist with `plutil -lint`
before installing it under `~/Library/LaunchAgents` and using `launchctl`.

`coh doctor --package DIR --package-trust FILE --credential-refs FILE` verifies
the installation and resolves an exact JSON map of the profile's uppercase
credential names to explicit `env:`/`file:` references. It never prints secret
bytes. Its `service_health=not_observed` result is distinct from native provider
health and workload conformance; use the registered provider workflow for those
observations. Optional unavailable providers remain explicitly unavailable.

Format references: [ELF gABI](https://refspecs.linuxfoundation.org/elf/gabi4+/ch4.eheader.html),
[Mach-O loader definitions](https://github.com/apple-oss-distributions/xnu/blob/main/EXTERNAL_HEADERS/mach-o/loader.h),
[CycloneDX 1.6 schema](https://github.com/CycloneDX/specification/blob/1.6/schema/bom-1.6.schema.json),
and [Apple LaunchAgents](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html).

## Selected CUDA and LoRA adoption

The 1.1.0-beta component profiles include the complete signed Python source set,
explicit distribution builder, offline guides and ordinary CI example. Follow
[Adoption](../docs/ADOPTION.md) for a clean installation and
[CI workflows](../docs/CI_WORKFLOWS.md) for durable identity and verified exit status.
Build wheels/sdists from the verified source package; no unvalidated wheel is embedded.
The package version changes file membership without changing the VM manifest schema.

For native LoRA execution, add the following to the CUDA service enrollment after
configuring the dedicated user's systemd manager and pinned native runtime. Enable
lingering for that dedicated account when execution must survive logout, and verify
its user bus exists before starting the system ticket service. The user owns only
its bounded native runtime, CAS and user units, not the installed package.

```toml
[peft]
agent_config = "/var/lib/cohesix/peft/agent.json"
runtime_dir = "/run/user/1001"
```

Use the actual dedicated UID. The renderer requires separate native/Worker evidence
custody, a CUDA profile, and agent configuration inside its writable state root.
It selects `--peft-release-config` and the exact user bus for the existing native
phase executor. Install the reviewed serving unit generated by the shipped LoRA
preparation example in that user's unit directory. Pin its Python/helper/config
paths and verify the helper hash; native phase limits remain the release contract.
No broad sudo, shell action or general service-control grant is introduced.

For PEFT only, the ticket unit uses `ProtectHome=tmpfs` and exposes precisely the
selected user's `bus` and `systemd/private` sockets with `BindReadOnlyPaths`.
Other home/runtime content remains hidden. `ProtectHome=yes` cannot host those
nested bind mounts; see the [systemd execution contract](https://www.man7.org/linux/man-pages/man5/systemd.exec.5.html).
The non-PEFT unit retains `ProtectHome=yes`. A missing manager/socket fails startup;
never remove confinement or expose the entire home to bypass that refusal.
