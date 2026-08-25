# Author: Lukas Bower
# Purpose: Project generated Cohesix Worker lifecycle contracts without creating target authority.
# Copyright 2026 Lukas Bower

"""Strict, non-authoritative Worker lifecycle projection for the Python SDK."""

from __future__ import annotations

import hashlib
import json
import re
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Optional

from .backends import Backend, MockBackend
from .errors import CohesixError
from .generated import GPU_RECEIPT_ACTIONS, PEFT_RECEIPT_ACTIONS, PROFILE_SCHEMA

MAX_PROFILE_CONTRACT_BYTES = 64 * 1024
MAX_WORKER_OBSERVATION_BYTES = 16 * 1024
WORKER_OBSERVATION_SCHEMA = "cohesix-worker-observation/v1"
CANONICAL_ROLES = (
    "worker-heartbeat",
    "worker-gpu",
    "worker-bus",
    "worker-lora",
)
EXECUTABLE_ROLES = ("worker-heartbeat", "worker-gpu", "worker-lora")
LIFECYCLE_STATES = (
    "absent",
    "queued",
    "starting",
    "ready",
    "closing",
    "faulted",
    "terminal",
)
RECEIPT_STATES = ("none", "pending", "confirmed", "rejected", "stale")
ARTIFACT_STATES = ("missing", "verified", "mismatch")
EXECUTION_PROOFS = ("none", "host-model", "qemu", "fresh-pi")

_TOKEN = re.compile(r"^[A-Za-z0-9._-]+$")
_SHA256 = re.compile(r"^[0-9a-f]{64}$")
_ROLE_ALIASES = {
    "heartbeat": "worker-heartbeat",
    "worker": "worker-heartbeat",
    "worker-heartbeat": "worker-heartbeat",
    "gpu": "worker-gpu",
    "worker-gpu": "worker-gpu",
    "bus": "worker-bus",
    "worker-bus": "worker-bus",
    "lora": "worker-lora",
    "worker-lora": "worker-lora",
}
_CONTROL_ROLE = {
    "worker-heartbeat": "heartbeat",
    "worker-gpu": "gpu",
    "worker-lora": "lora",
}
_ROLE_CONTRACT = {
    "worker-heartbeat": ("executable", "/worker", ""),
    "worker-gpu": ("executable", "/gpu", "/gpu/<id>/lease"),
    "worker-bus": ("model-only", "/bus", ""),
    "worker-lora": ("executable", "/worker", ""),
}
_TARGET_ROLE_SLOTS = {
    "qemu": {
        "worker-heartbeat": 1,
        "worker-gpu": 127,
        "worker-bus": 0,
        "worker-lora": 128,
    },
    "pi4": {
        "worker-heartbeat": 1,
        "worker-gpu": 127,
        "worker-bus": 0,
        "worker-lora": 128,
    },
}
_TARGET_SHARD_BITS = {"qemu": 6, "pi4": 8}
_PROHIBITED_KEYS = {
    "auth_token",
    "authorization",
    "capability",
    "capability_value",
    "cptr",
    "raw_badge",
    "secret",
    "ticket_secret",
    "token",
}


@dataclass(frozen=True)
class TargetProfileContract:
    """Validated target-qualified generated profile plus exact file digest."""

    target: str
    target_profile: str
    manifest_sha256: str
    contract_sha256: str
    worker: Mapping[str, Any]
    schemas: Mapping[str, Any]
    namespace: Mapping[str, Any]
    vocabularies: Mapping[str, Any]
    receipts: Mapping[str, Any]
    bounds: Mapping[str, Any]
    source: str = "file"

    @property
    def maximum_live_tasks(self) -> int:
        """Return the compiler-admitted simultaneous Worker count."""

        return int(self.worker["maximum_live_tasks"])

    @property
    def establishes_target_identity(self) -> bool:
        """Only an exact generated local contract establishes target identity."""

        return self.source == "file"

    def role_declaration(self, role: str) -> str:
        """Return the generated executable/model-only declaration."""

        canonical = normalize_worker_role(role)
        for record in self.worker["roles"]:
            if record["role"] == canonical:
                return str(record["declaration"])
        raise CohesixError(f"profile contract has no Worker role {canonical}")

    def telemetry_path(self, public_instance_id: str) -> str:
        """Resolve the canonical generated sharded telemetry path."""

        worker_id = validate_worker_id(public_instance_id)
        if not bool(self.namespace["sharding_enabled"]):
            return f"/worker/{worker_id}/telemetry"
        shard_bits = int(self.namespace["shard_bits"])
        if not 1 <= shard_bits <= 8:
            raise CohesixError("profile contract has invalid shard_bits")
        shard = hashlib.sha256(worker_id.encode("ascii")).digest()[0]
        if shard_bits < 8:
            shard >>= 8 - shard_bits
        return f"/shard/{shard:02x}/worker/{worker_id}/telemetry"

    def legacy_telemetry_path(self, public_instance_id: str) -> str:
        """Return the legacy alias only when the selected profile enables it."""

        if not bool(self.namespace["legacy_worker_alias"]):
            raise CohesixError("legacy /worker alias is disabled by the target contract")
        return f"/worker/{validate_worker_id(public_instance_id)}/telemetry"


@dataclass(frozen=True)
class WorkerIdentity:
    """Five-part public Worker generation identity."""

    role: str
    slot: int
    lease_epoch: int
    supervisor_generation: int
    cap_generation: int


@dataclass(frozen=True)
class WorkerStateAxes:
    """Independent declaration, lifecycle, artifact, receipt, and proof axes."""

    declaration: str
    lifecycle: str
    artifact: str
    receipt: str
    execution_proof: str


@dataclass(frozen=True)
class WorkerObservation:
    """Bounded host projection of one Worker; never direct target evidence."""

    public_instance_id: str
    identity: Optional[WorkerIdentity]
    state: WorkerStateAxes
    request_admitted: bool
    provider_completed: bool
    receipt_sequence: int
    runtime_release_accepted: bool = False
    production_use_case_accepted: bool = False


@dataclass(frozen=True)
class WorkerControlResult:
    """Result of writing an existing Queen control file."""

    public_instance_id: str
    role: str
    request_admitted: bool
    lifecycle: str
    bytes_written: int


def load_profile_contract(
    value: str | Path | Mapping[str, Any],
    *,
    expected_target: Optional[str] = None,
    source: str = "file",
) -> TargetProfileContract:
    """Load and strictly validate one generated target-qualified contract."""

    if isinstance(value, Mapping):
        if source == "file":
            source = "mapping"
        raw = _canonical_json_bytes(value)
        data = dict(value)
    else:
        path = Path(value)
        if path.is_symlink() or not path.is_file():
            raise CohesixError("profile contract must be a regular non-symlink file")
        raw = path.read_bytes()
        if len(raw) > MAX_PROFILE_CONTRACT_BYTES:
            raise CohesixError("profile contract exceeds bounded size")
        try:
            parsed = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise CohesixError("profile contract is not valid UTF-8 JSON") from exc
        if not isinstance(parsed, dict):
            raise CohesixError("profile contract must be a JSON object")
        data = parsed
    if len(raw) > MAX_PROFILE_CONTRACT_BYTES:
        raise CohesixError("profile contract exceeds bounded size")
    _reject_sensitive(data)
    _require_keys(
        data,
        {
            "schema",
            "meta",
            "target_profile",
            "target",
            "manifest_profile",
            "manifest_sha256",
            "worker",
            "schemas",
            "namespace",
            "vocabularies",
            "receipts",
            "bounds",
            "host_integration",
            "proof_boundary",
        },
        "profile contract",
    )
    if data["schema"] != PROFILE_SCHEMA:
        raise CohesixError("unsupported Python profile contract schema")
    meta = _mapping(data["meta"], "meta")
    _require_keys(meta, {"author", "purpose"}, "profile contract meta")
    if meta["author"] != "Lukas Bower" or not isinstance(meta["purpose"], str):
        raise CohesixError("profile contract metadata is invalid")
    target = str(data["target"])
    if target not in ("qemu", "pi4") or (
        expected_target is not None and target != expected_target
    ):
        raise CohesixError("profile contract names the wrong target")
    profile = str(data["target_profile"])
    expected_profile = "qemu_smp_production" if target == "qemu" else "pi4_production"
    if profile != expected_profile:
        raise CohesixError("profile contract names the wrong production profile")
    expected_manifest_profile = (
        "virt-aarch64" if target == "qemu" else "pi4-uboot-aarch64"
    )
    if data["manifest_profile"] != expected_manifest_profile:
        raise CohesixError("profile contract names the wrong manifest profile")
    manifest_sha256 = _require_sha256(data["manifest_sha256"], "manifest_sha256")

    worker = _mapping(data["worker"], "worker")
    _require_keys(
        worker,
        {
            "implementation_epoch",
            "maximum_live_tasks",
            "namespace_capacity_per_role",
            "roles",
            "scheduling_profile",
            "task_abi_schema",
            "task_abi_version",
        },
        "Worker contract",
    )
    if (
        worker["implementation_epoch"] != 26
        or worker["scheduling_profile"] != "mcs-passive"
    ):
        raise CohesixError("profile contract Worker execution epoch is invalid")
    _bounded_int(
        worker["namespace_capacity_per_role"],
        "namespace_capacity_per_role",
        1,
        256,
    )
    roles = worker.get("roles")
    if not isinstance(roles, list) or len(roles) != len(CANONICAL_ROLES):
        raise CohesixError("profile contract must contain exactly four Worker roles")
    seen_roles: set[str] = set()
    executable_slots = 0
    for role_record in roles:
        record = _mapping(role_record, "Worker role")
        _require_keys(
            record,
            {
                "role",
                "declaration",
                "executable_slots",
                "ticket_scope",
                "telemetry_path_template",
                "lease_path_template",
            },
            "Worker role",
        )
        role = str(record["role"])
        if role not in CANONICAL_ROLES or role in seen_roles:
            raise CohesixError("profile contract has an unknown or duplicate Worker role")
        seen_roles.add(role)
        declaration = str(record["declaration"])
        slots = _bounded_int(record["executable_slots"], "executable_slots", 0, 256)
        expected_declaration, ticket_scope, lease_path = _ROLE_CONTRACT[role]
        expected_slots = _TARGET_ROLE_SLOTS[target][role]
        if (
            declaration != expected_declaration
            or slots != expected_slots
            or record["ticket_scope"] != ticket_scope
            or record["lease_path_template"] != lease_path
        ):
            raise CohesixError("profile contract Worker declaration is inconsistent")
        if record["telemetry_path_template"] != "/shard/<label>/worker/<id>/telemetry":
            raise CohesixError("profile contract has non-canonical Worker telemetry path")
        executable_slots += slots
    if seen_roles != set(CANONICAL_ROLES):
        raise CohesixError("profile contract Worker role matrix is incomplete")
    maximum_live_tasks = _bounded_int(
        worker.get("maximum_live_tasks"), "maximum_live_tasks", 1, 256
    )
    if maximum_live_tasks != executable_slots:
        raise CohesixError("profile contract maximum live task count is inconsistent")
    if worker.get("task_abi_schema") != "worker-task-abi/v2" or worker.get(
        "task_abi_version"
    ) != 2:
        raise CohesixError("profile contract Worker ABI is unsupported")

    schemas = _mapping(data["schemas"], "schemas")
    _require_keys(
        schemas,
        {
            "host_ticket_accepted",
            "host_ticket_result_accepted",
            "worker_gpu_receipt",
            "worker_integration_evidence",
            "worker_lora_receipt",
            "worker_observation",
        },
        "profile contract schemas",
    )
    if tuple(schemas.get("host_ticket_accepted", ())) != (
        "host-ticket/v1",
        "host-ticket/v2",
    ) or tuple(schemas.get("host_ticket_result_accepted", ())) != (
        "host-ticket-result/v1",
        "host-ticket-result/v2",
    ):
        raise CohesixError("profile contract host-ticket compatibility matrix is invalid")
    if (
        schemas["worker_gpu_receipt"] != "worker-gpu-receipt/v1"
        or schemas["worker_lora_receipt"] != "worker-lora-receipt/v1"
        or schemas["worker_observation"] != WORKER_OBSERVATION_SCHEMA
        or schemas["worker_integration_evidence"]
        != "cohesix-worker-integration-evidence/v1"
    ):
        raise CohesixError("profile contract Worker schemas are invalid")
    namespace = _mapping(data["namespace"], "namespace")
    _require_keys(
        namespace,
        {
            "legacy_telemetry_path_template",
            "legacy_worker_alias",
            "shard_bits",
            "sharding_enabled",
            "telemetry_path_template",
        },
        "profile contract namespace",
    )
    if namespace.get("telemetry_path_template") != "/shard/<label>/worker/<id>/telemetry":
        raise CohesixError("profile contract canonical namespace is invalid")
    if namespace.get("legacy_telemetry_path_template") != "/worker/<id>/telemetry":
        raise CohesixError("profile contract legacy namespace is invalid")
    if namespace.get("sharding_enabled") is not True:
        raise CohesixError("production profile contract must enable canonical sharding")
    if not isinstance(namespace.get("legacy_worker_alias"), bool):
        raise CohesixError("profile contract legacy alias gate is invalid")
    if (
        _bounded_int(namespace.get("shard_bits"), "shard_bits", 1, 8)
        != _TARGET_SHARD_BITS[target]
    ):
        raise CohesixError("profile contract must use the canonical target shard width")
    receipts = _mapping(data["receipts"], "receipts")
    _require_keys(
        receipts,
        {"gpu_actions", "max_control_inflight", "peft_actions"},
        "profile contract receipts",
    )
    if tuple(receipts.get("gpu_actions", ())) != tuple(GPU_RECEIPT_ACTIONS) or tuple(
        receipts.get("peft_actions", ())
    ) != tuple(PEFT_RECEIPT_ACTIONS):
        raise CohesixError("profile contract receipt action matrix is invalid")
    if receipts.get("max_control_inflight") != 1:
        raise CohesixError("profile contract must permit exactly one Worker control in flight")
    vocabularies = _mapping(data["vocabularies"], "vocabularies")
    _require_keys(
        vocabularies,
        {
            "artifact",
            "execution_proof",
            "integration_obligation",
            "integration_observed_mode",
            "lifecycle",
            "receipt",
        },
        "profile contract vocabularies",
    )
    for key, exact in (
        ("lifecycle", LIFECYCLE_STATES),
        ("receipt", RECEIPT_STATES),
        ("artifact", ARTIFACT_STATES),
        ("execution_proof", EXECUTION_PROOFS),
    ):
        if tuple(vocabularies.get(key, ())) != exact:
            raise CohesixError(f"profile contract {key} vocabulary is invalid")
    if tuple(vocabularies["integration_obligation"]) != (
        "role_required",
        "release_required",
        "use_case_required",
        "optional",
        "future",
    ) or tuple(vocabularies["integration_observed_mode"]) != (
        "unknown",
        "missing",
        "disabled",
        "fixture",
        "mock",
        "dry-run",
        "live",
    ):
        raise CohesixError("profile contract integration vocabulary is invalid")
    bounds = _mapping(data["bounds"], "bounds")
    _require_keys(
        bounds,
        {
            "console_max_json_bytes",
            "console_max_line_bytes",
            "console_max_path_bytes",
            "secure9p_msize",
            "secure9p_walk_depth",
            "worker_ready_timeout_ms",
            "worker_receipt_label_bytes",
            "worker_shared_page_bytes",
            "worker_shutdown_grace_ms",
        },
        "profile contract bounds",
    )
    _bounded_int(bounds.get("worker_ready_timeout_ms"), "ready timeout", 1, 60_000)
    _bounded_int(bounds.get("worker_shutdown_grace_ms"), "shutdown grace", 1, 60_000)
    _bounded_int(bounds.get("console_max_json_bytes"), "JSON bound", 1, 8192)
    _bounded_int(bounds.get("console_max_line_bytes"), "console line bound", 1, 8192)
    _bounded_int(bounds.get("console_max_path_bytes"), "console path bound", 1, 8192)
    _bounded_int(bounds.get("worker_receipt_label_bytes"), "receipt label bound", 1, 64)
    _bounded_int(bounds.get("worker_shared_page_bytes"), "shared page bound", 1, 65536)
    if bounds.get("secure9p_msize") != 8192 or bounds.get("secure9p_walk_depth") != 8:
        raise CohesixError("profile contract violates Secure9P red lines")
    host_integration = _mapping(data["host_integration"], "host_integration")
    _require_keys(
        host_integration,
        {"modes", "python_dependency_id", "schema"},
        "profile contract host integration",
    )
    if (
        host_integration["schema"] != "host-integration-dependency/v1"
        or host_integration["python_dependency_id"] != "python-sdk-projection"
        or tuple(host_integration["modes"])
        != tuple(vocabularies["integration_observed_mode"])
    ):
        raise CohesixError("profile contract host integration binding is invalid")
    proof = _mapping(data["proof_boundary"], "proof_boundary")
    _require_keys(
        proof,
        {
            "profile_contract_establishes_execution_proof",
            "profile_contract_establishes_target_identity",
            "python_projection_is_authority",
            "static_defaults_establish_execution_proof",
        },
        "profile contract proof boundary",
    )
    if (
        proof["profile_contract_establishes_execution_proof"] is not False
        or proof["profile_contract_establishes_target_identity"] is not True
        or proof["python_projection_is_authority"] is not False
        or proof["static_defaults_establish_execution_proof"] is not False
    ):
        raise CohesixError("profile contract widens Python proof authority")

    return TargetProfileContract(
        target=target,
        target_profile=profile,
        manifest_sha256=manifest_sha256,
        contract_sha256=hashlib.sha256(raw).hexdigest(),
        worker=worker,
        schemas=schemas,
        namespace=namespace,
        vocabularies=vocabularies,
        receipts=receipts,
        bounds=bounds,
        source=source,
    )


class WorkerClient:
    """Compose existing namespace operations using one generated contract."""

    def __init__(
        self,
        backend: Backend,
        profile_contract: str | Path | Mapping[str, Any] | TargetProfileContract,
    ) -> None:
        self.backend = backend
        if isinstance(profile_contract, TargetProfileContract):
            self.contract = profile_contract
        else:
            self.contract = load_profile_contract(profile_contract)
        if isinstance(self.backend, MockBackend):
            self.backend._configure_worker_shard_bits(
                int(self.contract.namespace["shard_bits"])
            )

    def spawn(self, role: str, public_instance_id: str, *, slot: int = 0) -> WorkerControlResult:
        """Submit a bounded Queen spawn request; ACK means admission only."""

        canonical_role = normalize_worker_role(role)
        if canonical_role == "worker-bus":
            raise CohesixError("worker-bus is model-only and cannot be spawned")
        if self.contract.role_declaration(canonical_role) != "executable":
            raise CohesixError(f"Worker role {canonical_role} is not executable")
        worker_id = validate_worker_id(public_instance_id)
        slot_value = _bounded_int(slot, "slot", 0, 63)
        payload = _bounded_control_payload(
            {
                "spawn": _CONTROL_ROLE[canonical_role],
                "worker_id": worker_id,
                "slot": slot_value,
            },
            int(self.contract.bounds["console_max_json_bytes"]),
        )
        written = self.backend.write_append("/queen/ctl", payload)
        return WorkerControlResult(
            public_instance_id=worker_id,
            role=canonical_role,
            request_admitted=written == len(payload),
            lifecycle="queued",
            bytes_written=written,
        )

    def observe(
        self,
        role: str,
        public_instance_id: str,
        *,
        expected_identity: Optional[WorkerIdentity] = None,
    ) -> WorkerObservation:
        """Read and validate the newest complete canonical telemetry record."""

        canonical_role = normalize_worker_role(role)
        worker_id = validate_worker_id(public_instance_id)
        path = self.contract.telemetry_path(worker_id)
        payload = self.backend.tail_file(path, MAX_WORKER_OBSERVATION_BYTES)
        return parse_worker_observation(
            payload,
            self.contract,
            expected_role=canonical_role,
            expected_instance_id=worker_id,
            expected_identity=expected_identity,
            host_model=isinstance(self.backend, MockBackend),
        )

    def wait_ready(
        self,
        role: str,
        public_instance_id: str,
        *,
        timeout_s: Optional[float] = None,
        poll_s: float = 0.05,
        expected_identity: Optional[WorkerIdentity] = None,
    ) -> WorkerObservation:
        """Wait within the generated READY bound without treating spawn ACK as READY."""

        maximum = int(self.contract.bounds["worker_ready_timeout_ms"]) / 1000.0
        timeout = maximum if timeout_s is None else float(timeout_s)
        if timeout < 0 or timeout > maximum:
            raise CohesixError(f"READY timeout must be in 0..={maximum:g} seconds")
        if poll_s <= 0 or poll_s > maximum:
            raise CohesixError("READY poll interval is outside the generated bound")
        deadline = time.monotonic() + timeout
        last: Optional[WorkerObservation] = None
        while True:
            last = self.observe(
                role,
                public_instance_id,
                expected_identity=expected_identity,
            )
            if last.state.lifecycle == "ready":
                return last
            if last.state.lifecycle in ("faulted", "terminal", "closing"):
                raise CohesixError(
                    f"Worker entered {last.state.lifecycle} before READY"
                )
            if time.monotonic() >= deadline:
                raise CohesixError("Worker READY deadline expired")
            time.sleep(min(poll_s, max(0.0, deadline - time.monotonic())))

    def teardown(self, role: str, public_instance_id: str) -> WorkerControlResult:
        """Submit bounded teardown; ACK means closing was admitted, not completed."""

        canonical_role = normalize_worker_role(role)
        if canonical_role == "worker-bus":
            raise CohesixError("worker-bus is model-only and has no target lifecycle")
        worker_id = validate_worker_id(public_instance_id)
        payload = _bounded_control_payload(
            {"kill": worker_id, "role": canonical_role},
            int(self.contract.bounds["console_max_json_bytes"]),
        )
        written = self.backend.write_append("/queen/ctl", payload)
        return WorkerControlResult(
            public_instance_id=worker_id,
            role=canonical_role,
            request_admitted=written == len(payload),
            lifecycle="closing",
            bytes_written=written,
        )


def parse_worker_observation(
    payload: bytes,
    contract: TargetProfileContract,
    *,
    expected_role: str,
    expected_instance_id: str,
    expected_identity: Optional[WorkerIdentity] = None,
    host_model: bool = False,
) -> WorkerObservation:
    """Parse a host projection while refusing target-proof or release promotion."""

    if len(payload) > MAX_WORKER_OBSERVATION_BYTES:
        raise CohesixError("Worker observation exceeds bounded size")
    lines = [line for line in payload.splitlines() if line.strip()]
    if not lines:
        raise CohesixError("Worker observation is absent")
    try:
        data = json.loads(lines[-1].decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise CohesixError("Worker observation is not valid UTF-8 JSON") from exc
    if not isinstance(data, dict):
        raise CohesixError("Worker observation must be a JSON object")
    _reject_sensitive(data)
    _require_keys(
        data,
        {
            "schema",
            "public_instance_id",
            "identity",
            "state",
            "request_admitted",
            "provider_completed",
            "receipt_sequence",
        },
        "Worker observation",
    )
    if data["schema"] != WORKER_OBSERVATION_SCHEMA:
        raise CohesixError("unsupported Worker observation schema")
    worker_id = validate_worker_id(str(data["public_instance_id"]))
    canonical_role = normalize_worker_role(expected_role)
    if worker_id != validate_worker_id(expected_instance_id):
        raise CohesixError("Worker observation names the wrong public instance")
    identity_data = data["identity"]
    identity: Optional[WorkerIdentity]
    if identity_data is None:
        identity = None
    else:
        record = _mapping(identity_data, "Worker identity")
        _require_keys(
            record,
            {
                "role",
                "slot",
                "lease_epoch",
                "supervisor_generation",
                "cap_generation",
            },
            "Worker identity",
        )
        identity = WorkerIdentity(
            role=normalize_worker_role(str(record["role"])),
            slot=_bounded_int(record["slot"], "slot", 0, 63),
            lease_epoch=_bounded_int(record["lease_epoch"], "lease_epoch", 1, 2**64 - 1),
            supervisor_generation=_bounded_int(
                record["supervisor_generation"], "supervisor_generation", 1, 2**64 - 1
            ),
            cap_generation=_bounded_int(
                record["cap_generation"], "cap_generation", 1, 2**64 - 1
            ),
        )
        if identity.role != canonical_role:
            raise CohesixError("Worker observation role does not match the request")
    if expected_identity is not None and identity != expected_identity:
        raise CohesixError("Worker observation names a stale or wrong generation")
    state_data = _mapping(data["state"], "Worker state")
    _require_keys(
        state_data,
        {"declaration", "lifecycle", "artifact", "receipt", "execution_proof"},
        "Worker state",
    )
    declaration = str(state_data["declaration"])
    lifecycle = _enum(state_data["lifecycle"], LIFECYCLE_STATES, "lifecycle")
    artifact = _enum(state_data["artifact"], ARTIFACT_STATES, "artifact")
    receipt = _enum(state_data["receipt"], RECEIPT_STATES, "receipt")
    execution_proof = _enum(
        state_data["execution_proof"], EXECUTION_PROOFS, "execution_proof"
    )
    if declaration != contract.role_declaration(canonical_role):
        raise CohesixError("Worker observation declaration differs from target contract")
    if lifecycle == "absent" and identity is not None:
        raise CohesixError("absent Worker observation carries an identity")
    if lifecycle != "absent" and identity is None:
        raise CohesixError("live Worker observation is missing its identity")
    if execution_proof in ("qemu", "fresh-pi"):
        raise CohesixError("host Worker observation cannot establish target proof")
    expected_projection = "host-model" if host_model else "none"
    if execution_proof != expected_projection:
        raise CohesixError("Worker observation proof does not match backend class")
    if canonical_role == "worker-heartbeat" and receipt != "none":
        raise CohesixError("Heartbeat Worker cannot carry a GPU or PEFT receipt")
    receipt_sequence = _bounded_int(
        data["receipt_sequence"], "receipt_sequence", 0, 2**64 - 1
    )
    if receipt == "none" and receipt_sequence != 0:
        raise CohesixError("receipt sequence is nonzero without a receipt")
    if receipt != "none" and receipt_sequence == 0:
        raise CohesixError("receipt state requires a committed receipt sequence")
    if not isinstance(data["request_admitted"], bool) or not isinstance(
        data["provider_completed"], bool
    ):
        raise CohesixError("Worker observation boolean axes are invalid")
    return WorkerObservation(
        public_instance_id=worker_id,
        identity=identity,
        state=WorkerStateAxes(
            declaration=declaration,
            lifecycle=lifecycle,
            artifact=artifact,
            receipt=receipt,
            execution_proof=execution_proof,
        ),
        request_admitted=data["request_admitted"],
        provider_completed=data["provider_completed"],
        receipt_sequence=receipt_sequence,
    )


def normalize_worker_role(value: str) -> str:
    """Normalize only documented Worker role aliases."""

    normalized = value.strip().lower()
    role = _ROLE_ALIASES.get(normalized)
    if role is None:
        raise CohesixError(f"unknown Worker role {value!r}")
    return role


def validate_worker_id(value: str) -> str:
    """Validate a bounded public Worker id without inferring its role."""

    worker_id = value.strip()
    if not worker_id or len(worker_id.encode("utf-8")) > 128 or not _TOKEN.fullmatch(worker_id):
        raise CohesixError("Worker public instance id must be a bounded ASCII token")
    return worker_id


def _bounded_control_payload(value: Mapping[str, Any], maximum: int) -> bytes:
    raw = _canonical_json_bytes(value) + b"\n"
    if len(raw) > maximum:
        raise CohesixError(f"Worker control payload exceeds generated bound {maximum}")
    return raw


def _canonical_json_bytes(value: Mapping[str, Any]) -> bytes:
    try:
        return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    except (TypeError, ValueError) as exc:
        raise CohesixError("value is not bounded JSON") from exc


def _require_keys(value: Mapping[str, Any], expected: set[str], context: str) -> None:
    actual = set(value)
    if actual != expected:
        raise CohesixError(
            f"{context} fields differ: missing={sorted(expected - actual)} "
            f"unknown={sorted(actual - expected)}"
        )


def _mapping(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise CohesixError(f"{context} must be an object")
    return value


def _bounded_int(value: Any, field: str, minimum: int, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
        raise CohesixError(f"{field} is outside {minimum}..={maximum}")
    return value


def _enum(value: Any, choices: tuple[str, ...], field: str) -> str:
    text = str(value)
    if text not in choices:
        raise CohesixError(f"unknown {field} value {text!r}")
    return text


def _require_sha256(value: Any, field: str) -> str:
    text = str(value)
    if not _SHA256.fullmatch(text):
        raise CohesixError(f"{field} must be lowercase SHA-256 hexadecimal")
    return text


def _reject_sensitive(value: Any) -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            if str(key).lower() in _PROHIBITED_KEYS:
                raise CohesixError("contract or observation contains prohibited authority data")
            _reject_sensitive(child)
    elif isinstance(value, list):
        for child in value:
            _reject_sensitive(child)
    elif isinstance(value, str):
        lowered = value.lower()
        if any(
            marker in lowered
            for marker in (
                "authorization: bearer ",
                "bearer ey",
                "capability_value=",
                "cohesix-ticket-",
                "raw_badge=",
                "secret=",
                "token=",
            )
        ):
            raise CohesixError(
                "contract or observation contains prohibited authority data"
            )
