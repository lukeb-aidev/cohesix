#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Validate the target-neutral Cohesix wheel and emit target-bound Python projection evidence.
# Copyright 2026 Lukas Bower

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
wheel_smoke=false
wheel_dir=""
package_manifest=""
python_matrix="3.11,3.13"
target=""
profile_contract=""
matrix="$repo_root/configs/host_integration_acceptance.toml"
state_dir=""
target_session="${COHESIX_PYTHON_TARGET_SESSION:-}"

usage() {
  echo "usage: $0 [--wheel-smoke] --wheel-dir DIR --package-manifest FILE --state-dir DIR [--python-matrix 3.11,3.13] [--target qemu|pi4 --profile-contract FILE --matrix FILE --target-session FILE]" >&2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --wheel-smoke)
      wheel_smoke=true
      shift
      ;;
    --wheel-dir)
      wheel_dir=${2:-}
      shift 2
      ;;
    --package-manifest)
      package_manifest=${2:-}
      shift 2
      ;;
    --python-matrix)
      python_matrix=${2:-}
      shift 2
      ;;
    --target)
      target=${2:-}
      shift 2
      ;;
    --profile-contract)
      profile_contract=${2:-}
      shift 2
      ;;
    --matrix)
      matrix=${2:-}
      shift 2
      ;;
    --state-dir)
      state_dir=${2:-}
      shift 2
      ;;
    --target-session)
      target_session=${2:-}
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "python-compat: unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [[ -z "$wheel_dir" || -z "$package_manifest" || -z "$state_dir" ]]; then
  usage
  exit 2
fi
if [[ "$wheel_smoke" != true && "$target" != "qemu" && "$target" != "pi4" ]]; then
  echo "python-compat: matrix mode requires --target qemu or --target pi4" >&2
  exit 2
fi
if [[ -n "$target" && -z "$profile_contract" ]]; then
  echo "python-compat: --target requires --profile-contract" >&2
  exit 2
fi

mkdir -p "$state_dir"

resolve_python() {
  local version=$1
  local override=""
  local candidate=""
  case "$version" in
    3.11)
      override=${COHESIX_PYTHON_3_11:-}
      ;;
    3.13)
      override=${COHESIX_PYTHON_3_13:-}
      ;;
    *)
      echo "python-compat: unsupported matrix interpreter $version" >&2
      return 2
      ;;
  esac
  if [[ -n "$override" ]]; then
    candidate=$override
  elif command -v "python$version" >/dev/null 2>&1; then
    candidate=$(command -v "python$version")
  elif [[ -x "/opt/homebrew/bin/python$version" ]]; then
    candidate="/opt/homebrew/bin/python$version"
  else
    echo "python-compat: required Python $version interpreter is unavailable" >&2
    return 2
  fi
  "$candidate" - "$version" <<'PY'
import sys

expected = tuple(int(part) for part in sys.argv[1].split("."))
if sys.version_info[:2] != expected:
    raise SystemExit(
        f"interpreter version mismatch: expected {sys.argv[1]}, got "
        f"{sys.version_info.major}.{sys.version_info.minor}"
    )
PY
  printf '%s\n' "$candidate"
}

IFS=',' read -r -a requested_versions <<< "$python_matrix"
if [[ ${#requested_versions[@]} -eq 0 ]]; then
  echo "python-compat: empty Python matrix" >&2
  exit 2
fi
interpreters=()
versions=()
for raw_version in "${requested_versions[@]}"; do
  version=${raw_version//[[:space:]]/}
  if [[ "$version" != "3.11" && "$version" != "3.13" ]]; then
    echo "python-compat: matrix must contain only 3.11 and 3.13" >&2
    exit 2
  fi
  for existing in "${versions[@]:-}"; do
    if [[ "$existing" == "$version" ]]; then
      echo "python-compat: duplicate Python matrix entry $version" >&2
      exit 2
    fi
  done
  versions+=("$version")
  interpreters+=("$(resolve_python "$version")")
done

shopt -s nullglob
wheel_candidates=("$wheel_dir"/cohesix-*.whl)
shopt -u nullglob
if [[ ${#wheel_candidates[@]} -ne 1 ]]; then
  echo "python-compat: expected exactly one cohesix wheel in $wheel_dir" >&2
  exit 2
fi
wheel=${wheel_candidates[0]}
if [[ ! -f "$wheel" || -L "$wheel" ]]; then
  echo "python-compat: wheel must be a regular non-symlink file" >&2
  exit 2
fi

qemu_contract="$repo_root/configs/generated/cohesix_python_qemu_smp_production.json"
pi4_contract="$repo_root/configs/generated/cohesix_python_pi4_production.json"

inspect_wheel() {
  local output=$1
  "${interpreters[0]}" - "$wheel" "$qemu_contract" "$pi4_contract" "$output" <<'PY'
import hashlib
import json
import os
import sys
import zipfile
from email import message_from_bytes
from pathlib import Path

wheel, qemu_path, pi4_path, output = map(Path, sys.argv[1:])
if wheel.stat().st_size <= 0 or wheel.stat().st_size > 64 * 1024 * 1024:
    raise SystemExit("python-compat: wheel size is outside the 1..64 MiB bound")
with zipfile.ZipFile(wheel) as archive:
    names = sorted(archive.namelist())
    if any("qemu_smp_production" in name or "pi4_production" in name for name in names):
        raise SystemExit("python-compat: target-qualified contract leaked into shared wheel")
    required_modules = {
        "cohesix/__init__.py",
        "cohesix/backends.py",
        "cohesix/client.py",
        "cohesix/evidence.py",
        "cohesix/generated.py",
        "cohesix/orchestration.py",
        "cohesix/playbooks.py",
        "cohesix/receipts.py",
        "cohesix/worker.py",
    }
    missing = sorted(required_modules - set(names))
    if missing:
        raise SystemExit(f"python-compat: wheel is missing modules: {missing}")
    metadata_names = [name for name in names if name.endswith(".dist-info/METADATA")]
    entry_names = [name for name in names if name.endswith(".dist-info/entry_points.txt")]
    if len(metadata_names) != 1 or len(entry_names) != 1:
        raise SystemExit("python-compat: wheel metadata or entry point is not exact")
    metadata = message_from_bytes(archive.read(metadata_names[0]))
    extras = sorted(metadata.get_all("Provides-Extra", []))
    if extras != ["dev", "integrations", "ml"]:
        raise SystemExit(f"python-compat: declared extras differ: {extras}")
    if metadata.get("Requires-Python") != ">=3.11":
        raise SystemExit("python-compat: wheel Requires-Python must be >=3.11")
    entry_points = archive.read(entry_names[0]).decode("utf-8")
    if "cohesix-playbook = cohesix.playbook_cli:main" not in entry_points:
        raise SystemExit("python-compat: public cohesix-playbook entry point is absent")

def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

contracts = {}
for target, path, profile in (
    ("qemu", qemu_path, "qemu_smp_production"),
    ("pi4", pi4_path, "pi4_production"),
):
    raw = path.read_bytes()
    value = json.loads(raw)
    if value.get("target") != target or value.get("target_profile") != profile:
        raise SystemExit(f"python-compat: {target} profile contract identity mismatch")
    contracts[target] = {
        "filename": path.name,
        "sha256": hashlib.sha256(raw).hexdigest(),
        "bytes": len(raw),
        "manifest_sha256": value.get("manifest_sha256"),
        "target_profile": profile,
    }
if contracts["qemu"]["sha256"] == contracts["pi4"]["sha256"]:
    raise SystemExit("python-compat: QEMU and Pi contracts must be independently generated")

record = {
    "schema": "cohesix-python-wheel-inspection/v1",
    "wheel": {
        "filename": wheel.name,
        "sha256": digest(wheel),
        "bytes": wheel.stat().st_size,
        "members_sha256": hashlib.sha256("\n".join(names).encode()).hexdigest(),
        "modules": sorted(required_modules),
    },
    "profile_contracts": contracts,
    "python_requires": ">=3.11",
    "extras": extras,
    "entry_points": ["cohesix-playbook=cohesix.playbook_cli:main"],
}
tmp = output.with_suffix(output.suffix + ".partial")
tmp.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
os.replace(tmp, output)
PY
}

run_smoke() {
  local interpreter=$1
  local version=$2
  local selected_contract=${3:-$qemu_contract}
  local expected_target=${4:-qemu}
  local label=${version//./_}
  local venv_dir="$state_dir/venv-$label"
  local report="$state_dir/python-$version-smoke.json"
  "$interpreter" -m venv --clear "$venv_dir"
  "$venv_dir/bin/python" -m pip install --disable-pip-version-check \
    --no-deps --no-index "$wheel" >"$state_dir/python-$version-pip.log" 2>&1
  "$venv_dir/bin/python" - \
    "$selected_contract" "$expected_target" "$report" "$state_dir/mock-$label" <<'PY'
import hashlib
import json
import os
import platform
import subprocess
import sys
from importlib import metadata
from pathlib import Path

contract_path = Path(sys.argv[1])
expected_target = sys.argv[2]
output = Path(sys.argv[3])
mock_root = Path(sys.argv[4])
import cohesix
from cohesix import CohesixClient, CohesixError, MockBackend, load_profile_contract
from cohesix.generated import DEFAULTS
from cohesix.integrations import probe_peft_runtime

required_exports = {
    "CohesixClient",
    "MockBackend",
    "TargetProfileContract",
    "WorkerClient",
    "WorkerIdentity",
    "WorkerObservation",
    "WorkerReceipt",
    "load_profile_contract",
    "parse_receipt",
}
if not required_exports.issubset(set(cohesix.__all__)):
    raise SystemExit("python-compat: installed wheel public API is incomplete")
if DEFAULTS.get("manifest_sha256") is not None or DEFAULTS.get("execution_proof") != "none":
    raise SystemExit("python-compat: installed wheel defaults are not target-neutral")
contract = load_profile_contract(contract_path, expected_target=expected_target)
client = CohesixClient(MockBackend(str(mock_root)), profile_contract=contract)
for role, worker_id in (("heartbeat", "smoke-heart"), ("gpu", "smoke-gpu"), ("lora", "smoke-lora")):
    if not client.worker_spawn(role, worker_id).request_admitted:
        raise SystemExit("python-compat: mock spawn admission failed")
    observation = client.worker_wait_ready(role, worker_id, timeout_s=0.2)
    if observation.state.execution_proof != "host-model":
        raise SystemExit("python-compat: mock observation proof class widened")
    client.worker_teardown(role, worker_id)
try:
    client.worker_spawn("bus", "smoke-bus")
except CohesixError:
    pass
else:
    raise SystemExit("python-compat: WorkerBus spawn was not refused")
probe = probe_peft_runtime()
if probe.status == "ok" and any(value is None for value in probe.data["versions"].values()):
    raise SystemExit("python-compat: missing optional runtime reported ready")
entry = subprocess.run(
    [sys.executable, "-m", "cohesix.playbook_cli", "--list"],
    check=False,
    capture_output=True,
    text=True,
)
if entry.returncode != 0 or "mac-release-factory" not in entry.stdout:
    raise SystemExit("python-compat: public playbook entry point smoke failed")
record = {
    "schema": "cohesix-python-smoke/v1",
    "implementation": platform.python_implementation(),
    "python_version": platform.python_version(),
    "platform": platform.platform(),
    "package_version": metadata.version("cohesix"),
    "profile_contract_sha256": contract.contract_sha256,
    "target": expected_target,
    "target_defaults": "neutral",
    "optional_peft_status": probe.status,
    "worker_roles": ["worker-gpu", "worker-heartbeat", "worker-lora"],
    "worker_bus": "model-only-refused",
    "result": "PASS",
}
tmp = output.with_suffix(output.suffix + ".partial")
tmp.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
os.replace(tmp, output)
PY
}

inspection="$state_dir/wheel-inspection.json"
if [[ "$wheel_smoke" == true ]]; then
  inspect_wheel "$inspection"
  smoke_reports=()
  for index in "${!versions[@]}"; do
    run_smoke "${interpreters[$index]}" "${versions[$index]}"
    smoke_reports+=("$state_dir/python-${versions[$index]}-smoke.json")
  done
  "${interpreters[0]}" - "$inspection" "$package_manifest" "${smoke_reports[@]}" <<'PY'
import json
import os
import sys
from pathlib import Path

inspection_path = Path(sys.argv[1])
output = Path(sys.argv[2])
smoke_paths = [Path(value) for value in sys.argv[3:]]
inspection = json.loads(inspection_path.read_text(encoding="utf-8"))
smokes = [json.loads(path.read_text(encoding="utf-8")) for path in smoke_paths]
versions = sorted(record["python_version"] for record in smokes)
if not any(value.startswith("3.11.") for value in versions) or not any(
    value.startswith("3.13.") for value in versions
):
    raise SystemExit("python-compat: package manifest requires Python 3.11 and 3.13")
record = {
    "schema": "cohesix-python-package/v1",
    "meta": {
        "author": "Lukas Bower",
        "purpose": "Bind one target-neutral wheel to independently generated QEMU and Pi contracts.",
    },
    "wheel": inspection["wheel"],
    "profile_contracts": inspection["profile_contracts"],
    "python_requires": inspection["python_requires"],
    "extras": inspection["extras"],
    "entry_points": inspection["entry_points"],
    "interpreters": sorted(smokes, key=lambda item: item["python_version"]),
    "proof_boundary": {
        "package_install_is_target_proof": False,
        "mock_is_target_proof": False,
        "python_projection_is_authority": False,
    },
}
output.parent.mkdir(parents=True, exist_ok=True)
tmp = output.with_suffix(output.suffix + ".partial")
tmp.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
os.replace(tmp, output)
PY
  echo "python-compat: wheel smoke PASS manifest=$package_manifest"
fi

if [[ -n "$target" ]]; then
  if [[ ! -f "$package_manifest" || -L "$package_manifest" ]]; then
    echo "python-compat: package manifest must be a regular non-symlink file" >&2
    exit 2
  fi
  if [[ ! -f "$profile_contract" || -L "$profile_contract" || ! -f "$matrix" ]]; then
    echo "python-compat: profile contract and matrix must be regular files" >&2
    exit 2
  fi
  if [[ -z "$target_session" ]]; then
    for candidate in \
      "$state_dir/target-session.json" \
      "$(dirname "$state_dir")/$target-target-session.json" \
      "$(dirname "$state_dir")/m26e-$target-target-session.json"; do
      if [[ -f "$candidate" && ! -L "$candidate" ]]; then
        target_session=$candidate
        break
      fi
    done
  fi
  if [[ -z "$target_session" || ! -f "$target_session" || -L "$target_session" ]]; then
    echo "python-compat: matrix mode requires an existing regular --target-session record" >&2
    exit 2
  fi

  inspect_wheel "$inspection"
  matrix_reports=()
  for index in "${!versions[@]}"; do
    run_smoke \
      "${interpreters[$index]}" "${versions[$index]}" "$profile_contract" "$target"
    matrix_reports+=("$state_dir/python-${versions[$index]}-smoke.json")
  done
  evidence="$state_dir/python-sdk-projection.json"
  first_venv="$state_dir/venv-${versions[0]//./_}"
  "$first_venv/bin/python" - \
    "$repo_root" "$wheel" "$package_manifest" "$profile_contract" "$matrix" \
    "$target_session" "$target" "$evidence" "${matrix_reports[@]}" <<'PY'
import hashlib
import importlib.util
import json
import os
import platform
import sys
from pathlib import Path

(
    repo_root,
    wheel,
    package_manifest,
    profile_path,
    matrix,
    target_session_path,
    target,
    output,
    *smoke_values,
) = sys.argv[1:]
wheel = Path(wheel)
package_manifest = Path(package_manifest)
profile_path = Path(profile_path)
matrix = Path(matrix)
target_session_path = Path(target_session_path)
output = Path(output)
smoke_paths = [Path(value) for value in smoke_values]

from cohesix.evidence import build_python_projection_evidence
from cohesix.worker import load_profile_contract

def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

contract = load_profile_contract(profile_path, expected_target=target)
manifest = json.loads(package_manifest.read_text(encoding="utf-8"))
if manifest.get("schema") != "cohesix-python-package/v1":
    raise SystemExit("python-compat: unsupported package manifest")
if manifest.get("wheel", {}).get("sha256") != sha(wheel):
    raise SystemExit("python-compat: wheel hash differs from package manifest")
contract_record = manifest.get("profile_contracts", {}).get(target, {})
if contract_record.get("sha256") != contract.contract_sha256:
    raise SystemExit("python-compat: profile contract hash differs from package manifest")
if contract_record.get("manifest_sha256") != contract.manifest_sha256:
    raise SystemExit("python-compat: target manifest hash differs from package manifest")

session_record = json.loads(target_session_path.read_text(encoding="utf-8"))
validator_path = Path(repo_root) / "scripts/worker_task_evidence.py"
validator_spec = importlib.util.spec_from_file_location(
    "cohesix_worker_task_evidence", validator_path
)
if validator_spec is None or validator_spec.loader is None:
    raise SystemExit("python-compat: cannot load the Worker evidence validator")
validator = importlib.util.module_from_spec(validator_spec)
sys.modules[validator_spec.name] = validator
validator_spec.loader.exec_module(validator)
try:
    validator.validate_integration(session_record, target)
except ValueError as exc:
    raise SystemExit(f"python-compat: target-session record failed validation: {exc}") from exc
if (
    session_record.get("schema") != "cohesix-worker-integration-evidence/v1"
    or session_record.get("record_kind") != "worker-integration"
    or session_record.get("dependency_id")
    not in {"worker-control", "gpu-receipt-path", "peft-receipt-path"}
    or session_record.get("observed_mode") != "live"
    or session_record.get("verdict") != "PASS"
    or session_record.get("blockers") != []
    or session_record.get("manifest_sha256") != contract.manifest_sha256
    or session_record.get("execution_proof")
    != ("qemu" if target == "qemu" else "fresh-pi")
):
    raise SystemExit(
        "python-compat: target-session input must be one accepted target role record"
    )
target_session = session_record.get("target_session")
if not isinstance(target_session, dict):
    raise SystemExit("python-compat: target-session record is invalid")

graph_path = Path(repo_root) / "configs/generated/host_integration_dependency.json"
smokes = [json.loads(path.read_text(encoding="utf-8")) for path in smoke_paths]
outcomes = [
    {
        "id": f"cpython-{record['python_version'].split('.')[0]}-{record['python_version'].split('.')[1]}",
        "class": "projection-compatibility",
        "result": "accepted",
    }
    for record in smokes
]
raw = [
    {"id": path.name, "sha256": sha(path), "bytes": path.stat().st_size}
    for path in smoke_paths
]
raw.append(
    {
        "id": package_manifest.name,
        "sha256": sha(package_manifest),
        "bytes": package_manifest.stat().st_size,
    }
)
raw.append(
    {
        "id": target_session_path.name,
        "sha256": sha(target_session_path),
        "bytes": target_session_path.stat().st_size,
    }
)
raw.sort(key=lambda item: (item["id"], item["sha256"], item["bytes"]))
machine = platform.machine().lower()
system = platform.system().lower()
if system == "darwin" and machine in ("arm64", "aarch64"):
    host_profile = "macos-arm64"
    architecture = "aarch64"
elif system == "linux" and machine in ("x86_64", "amd64"):
    host_profile = "linux-x86-64"
    architecture = "x86_64"
else:
    raise SystemExit(f"python-compat: unsupported release host {system}/{machine}")
record = build_python_projection_evidence(
    contract=contract,
    dependency_graph_sha256=sha(graph_path),
    matrix_sha256=sha(matrix),
    wheel_sha256=sha(wheel),
    host={
        "profile": host_profile,
        "os": "macos" if system == "darwin" else system,
        "architecture": architecture,
        "provider_version": ",".join(
            f"CPython-{item['python_version']}" for item in smokes
        ),
    },
    target_session=target_session,
    interpreter_outcomes=outcomes,
    raw_evidence=raw,
)
output.parent.mkdir(parents=True, exist_ok=True)
tmp = output.with_suffix(output.suffix + ".partial")
tmp.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
os.replace(tmp, output)
PY
  echo "python-compat: target projection PASS target=$target evidence=$evidence"
fi
