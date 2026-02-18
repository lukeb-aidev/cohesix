# Author: Lukas Bower
# Purpose: Provide host integration adapters for systemd, Docker, Kubernetes, NVML, and PEFT runtimes.
# Copyright 2026 Lukas Bower

"""Host integration adapters for Cohesix Python orchestration.

All probes are read-only and degrade gracefully when host dependencies are absent.
"""

from __future__ import annotations

import importlib.metadata
import json
import shutil
import subprocess
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Dict, Iterable, List, Optional, Sequence

from .errors import CohesixError


def _utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _safe_command(
    args: Sequence[str],
    timeout_s: float = 5.0,
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            list(args),
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout_s,
        )
    except FileNotFoundError as exc:
        raise CohesixError(f"command not found: {args[0]}") from exc
    except subprocess.TimeoutExpired as exc:
        raise CohesixError(f"command timed out: {' '.join(args)}") from exc


def _coerce_status(ok: bool, has_data: bool) -> str:
    if ok:
        return "ok"
    if has_data:
        return "degraded"
    return "skipped"


@dataclass(frozen=True)
class ProbeResult:
    """Normalized probe result for host integration providers."""

    provider: str
    status: str
    data: Dict[str, object]
    error: Optional[str] = None


@dataclass(frozen=True)
class HostSnapshot:
    """Aggregated host probe results for audit and telemetry shipping."""

    captured_at_utc: str
    results: Dict[str, ProbeResult] = field(default_factory=dict)


def probe_systemd(services: Iterable[str], timeout_s: float = 5.0) -> ProbeResult:
    """Collect per-service systemd status with `systemctl show`."""

    service_list = [svc.strip() for svc in services if svc and svc.strip()]
    if not service_list:
        return ProbeResult(
            provider="systemd",
            status="skipped",
            data={"services": {}},
            error="no services requested",
        )
    if shutil.which("systemctl") is None:
        return ProbeResult(
            provider="systemd",
            status="skipped",
            data={"services": {}},
            error="systemctl unavailable",
        )

    output: Dict[str, object] = {}
    had_error = False
    for service in service_list:
        result = _safe_command(
            [
                "systemctl",
                "show",
                service,
                "--property=ActiveState,SubState,UnitFileState",
                "--no-pager",
            ],
            timeout_s=timeout_s,
        )
        if result.returncode != 0:
            had_error = True
            output[service] = {
                "status": "error",
                "error": (result.stderr or result.stdout).strip()[:512],
            }
            continue
        fields: Dict[str, str] = {}
        for line in result.stdout.splitlines():
            line = line.strip()
            if "=" not in line:
                continue
            key, value = line.split("=", 1)
            fields[key] = value
        output[service] = {
            "status": "ok",
            "active": fields.get("ActiveState", "unknown"),
            "sub": fields.get("SubState", "unknown"),
            "unit_file": fields.get("UnitFileState", "unknown"),
        }

    status = _coerce_status(not had_error, has_data=bool(output))
    return ProbeResult(
        provider="systemd",
        status=status,
        data={"services": output},
        error=None if not had_error else "one or more services failed",
    )


def probe_docker(max_containers: int = 256, timeout_s: float = 5.0) -> ProbeResult:
    """Collect Docker container state with SDK-first, CLI fallback behavior."""

    containers: List[Dict[str, object]] = []
    had_error = False

    try:
        import docker  # type: ignore

        client = docker.from_env()
        try:
            for entry in client.containers.list(all=True)[:max_containers]:
                status = "unknown"
                state = getattr(entry, "status", None)
                if isinstance(state, str):
                    status = state
                containers.append(
                    {
                        "id": entry.id[:12],
                        "name": entry.name,
                        "image": str(entry.image.tags[0]) if entry.image.tags else "<none>",
                        "status": status,
                    }
                )
        finally:
            close_fn = getattr(client, "close", None)
            if callable(close_fn):
                close_fn()
    except Exception:
        if shutil.which("docker") is None:
            return ProbeResult(
                provider="docker",
                status="skipped",
                data={"containers": []},
                error="docker SDK and CLI unavailable",
            )
        result = _safe_command(
            ["docker", "ps", "--all", "--format", "{{json .}}"],
            timeout_s=timeout_s,
        )
        if result.returncode != 0:
            return ProbeResult(
                provider="docker",
                status="degraded",
                data={"containers": []},
                error=(result.stderr or result.stdout).strip()[:512],
            )
        for line in result.stdout.splitlines()[:max_containers]:
            line = line.strip()
            if not line:
                continue
            try:
                entry = json.loads(line)
            except json.JSONDecodeError:
                had_error = True
                continue
            containers.append(
                {
                    "id": str(entry.get("ID", "")),
                    "name": str(entry.get("Names", "")),
                    "image": str(entry.get("Image", "")),
                    "status": str(entry.get("Status", "")),
                }
            )

    status = _coerce_status(not had_error, has_data=bool(containers))
    return ProbeResult(
        provider="docker",
        status=status,
        data={"containers": containers},
        error=None if not had_error else "some container records could not be parsed",
    )


def probe_kubernetes(
    namespace: str = "default",
    label_selector: str = "",
    timeout_s: float = 5.0,
    max_pods: int = 512,
) -> ProbeResult:
    """Collect pod phase summaries via kubernetes SDK, with kubectl fallback."""

    phases: Dict[str, int] = {}
    pods: List[Dict[str, str]] = []

    try:
        from kubernetes import client as k8s_client  # type: ignore
        from kubernetes import config as k8s_config  # type: ignore

        try:
            k8s_config.load_kube_config()
        except Exception:
            k8s_config.load_incluster_config()
        v1 = k8s_client.CoreV1Api()
        response = v1.list_namespaced_pod(
            namespace=namespace,
            label_selector=label_selector,
            limit=max_pods,
            _request_timeout=timeout_s,
        )
        for item in response.items:
            pod_name = getattr(item.metadata, "name", "unknown")
            pod_phase = getattr(item.status, "phase", "Unknown")
            pods.append({"name": pod_name, "phase": pod_phase})
            phases[pod_phase] = phases.get(pod_phase, 0) + 1
    except Exception:
        if shutil.which("kubectl") is None:
            return ProbeResult(
                provider="k8s",
                status="skipped",
                data={"namespace": namespace, "pods": [], "phases": {}},
                error="kubernetes SDK and kubectl unavailable",
            )
        args = ["kubectl", "get", "pods", "-n", namespace, "-o", "json"]
        if label_selector:
            args.extend(["-l", label_selector])
        result = _safe_command(args, timeout_s=timeout_s)
        if result.returncode != 0:
            return ProbeResult(
                provider="k8s",
                status="degraded",
                data={"namespace": namespace, "pods": [], "phases": {}},
                error=(result.stderr or result.stdout).strip()[:512],
            )
        try:
            payload = json.loads(result.stdout)
        except json.JSONDecodeError as exc:
            raise CohesixError("kubectl returned invalid JSON payload") from exc
        items = payload.get("items", [])
        if isinstance(items, list):
            for item in items[:max_pods]:
                metadata = item.get("metadata", {})
                status = item.get("status", {})
                pod_name = str(metadata.get("name", "unknown"))
                pod_phase = str(status.get("phase", "Unknown"))
                pods.append({"name": pod_name, "phase": pod_phase})
                phases[pod_phase] = phases.get(pod_phase, 0) + 1

    return ProbeResult(
        provider="k8s",
        status="ok",
        data={"namespace": namespace, "pods": pods, "phases": phases},
    )


def probe_nvml(max_devices: int = 32, timeout_s: float = 5.0) -> ProbeResult:
    """Collect GPU inventory and utilization metrics using NVML or nvidia-smi."""

    devices: List[Dict[str, object]] = []
    error: Optional[str] = None

    try:
        import pynvml  # type: ignore

        pynvml.nvmlInit()
        try:
            count = int(pynvml.nvmlDeviceGetCount())
            for index in range(min(count, max_devices)):
                handle = pynvml.nvmlDeviceGetHandleByIndex(index)
                uuid = pynvml.nvmlDeviceGetUUID(handle)
                if isinstance(uuid, bytes):
                    uuid = uuid.decode("utf-8", errors="replace")
                name = pynvml.nvmlDeviceGetName(handle)
                if isinstance(name, bytes):
                    name = name.decode("utf-8", errors="replace")
                memory = pynvml.nvmlDeviceGetMemoryInfo(handle)
                utilization = None
                temperature = None
                try:
                    utilization = int(pynvml.nvmlDeviceGetUtilizationRates(handle).gpu)
                except Exception:
                    utilization = None
                try:
                    temperature = int(
                        pynvml.nvmlDeviceGetTemperature(
                            handle, pynvml.NVML_TEMPERATURE_GPU
                        )
                    )
                except Exception:
                    temperature = None
                devices.append(
                    {
                        "index": index,
                        "uuid": str(uuid),
                        "name": str(name),
                        "memory_total_mb": int(memory.total // (1024 * 1024)),
                        "memory_used_mb": int(memory.used // (1024 * 1024)),
                        "utilization_gpu_pct": utilization,
                        "temperature_c": temperature,
                    }
                )
        finally:
            try:
                pynvml.nvmlShutdown()
            except Exception:
                pass
    except Exception:
        if shutil.which("nvidia-smi") is None:
            return ProbeResult(
                provider="nvidia",
                status="skipped",
                data={"devices": []},
                error="NVML and nvidia-smi unavailable",
            )
        result = _safe_command(
            [
                "nvidia-smi",
                "--query-gpu=index,uuid,name,memory.total,memory.used,utilization.gpu,temperature.gpu",
                "--format=csv,noheader,nounits",
            ],
            timeout_s=timeout_s,
        )
        if result.returncode != 0:
            return ProbeResult(
                provider="nvidia",
                status="degraded",
                data={"devices": []},
                error=(result.stderr or result.stdout).strip()[:512],
            )
        for line in result.stdout.splitlines()[:max_devices]:
            parts = [part.strip() for part in line.split(",")]
            if len(parts) < 7:
                error = "one or more nvidia-smi rows were malformed"
                continue
            devices.append(
                {
                    "index": int(parts[0]),
                    "uuid": parts[1],
                    "name": parts[2],
                    "memory_total_mb": int(parts[3]),
                    "memory_used_mb": int(parts[4]),
                    "utilization_gpu_pct": int(parts[5]),
                    "temperature_c": int(parts[6]),
                }
            )

    status = _coerce_status(error is None, has_data=bool(devices))
    return ProbeResult(
        provider="nvidia",
        status=status,
        data={"devices": devices},
        error=error,
    )


def probe_peft_runtime() -> ProbeResult:
    """Collect PEFT/LoRA runtime package availability and versions."""

    modules = ["torch", "transformers", "peft", "accelerate", "bitsandbytes"]
    versions: Dict[str, Optional[str]] = {}
    missing: List[str] = []
    for module_name in modules:
        try:
            versions[module_name] = importlib.metadata.version(module_name)
        except importlib.metadata.PackageNotFoundError:
            versions[module_name] = None
            missing.append(module_name)

    status = "ok" if not missing else "degraded"
    error = None if not missing else f"missing packages: {', '.join(missing)}"
    return ProbeResult(
        provider="peft",
        status=status,
        data={"versions": versions},
        error=error,
    )


def collect_host_snapshot(
    systemd_services: Optional[Iterable[str]] = None,
    include_docker: bool = True,
    include_k8s: bool = True,
    include_nvml: bool = True,
    include_peft: bool = True,
    k8s_namespace: str = "default",
    k8s_label_selector: str = "",
) -> HostSnapshot:
    """Collect all selected host probes into a single timestamped snapshot."""

    results: Dict[str, ProbeResult] = {}
    if systemd_services is not None:
        results["systemd"] = probe_systemd(systemd_services)
    if include_docker:
        results["docker"] = probe_docker()
    if include_k8s:
        results["k8s"] = probe_kubernetes(
            namespace=k8s_namespace,
            label_selector=k8s_label_selector,
        )
    if include_nvml:
        results["nvidia"] = probe_nvml()
    if include_peft:
        results["peft"] = probe_peft_runtime()
    return HostSnapshot(captured_at_utc=_utc_now(), results=results)


def snapshot_to_ndjson(snapshot: HostSnapshot) -> str:
    """Render a host snapshot into NDJSON lines for telemetry shipping."""

    lines = []
    for provider_name in sorted(snapshot.results.keys()):
        probe = snapshot.results[provider_name]
        payload = {
            "captured_at_utc": snapshot.captured_at_utc,
            "provider": probe.provider,
            "status": probe.status,
            "error": probe.error,
            "data": probe.data,
        }
        lines.append(json.dumps(payload, separators=(",", ":"), sort_keys=True))
    return "\n".join(lines) + ("\n" if lines else "")
