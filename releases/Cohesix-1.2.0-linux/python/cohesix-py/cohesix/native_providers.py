# Author: Lukas Bower
# Purpose: Collect bounded native host observations without executing ticket actions or promoting provider conformance.
# Copyright 2026 Lukas Bower

"""Native discovery only. Observations are not authoritative execution receipts."""

from __future__ import annotations

import http.client
import json
import os
from pathlib import Path
import platform
import re
import selectors
import signal
import socket
import subprocess
import time
from typing import Any, Mapping, Sequence

from .providers import ProviderUnavailable, profile

MAX_BYTES = 65536
MAX_ROWS = 32


def bounded_command(
    argv: Sequence[str],
    timeout_s: float = 5.0,
    *,
    credential_refs: Mapping[str, str] | None = None,
) -> bytes:
    """Bound the entire fixed native command's lifetime and combined pipe bytes."""
    if not argv or not Path(argv[0]).is_absolute() or not 0 < timeout_s <= 30:
        raise ProviderUnavailable("invalid_probe", "native")
    environment = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LC_ALL": "C"}
    if credential_refs:
        from .auth import resolve_secret_reference

        if set(credential_refs) - {"COH_AUTH_TOKEN", "COH_REST_AUTH_TOKEN"}:
            raise ProviderUnavailable("invalid_credential_destination", "native")
        environment.update(
            {key: resolve_secret_reference(ref) for key, ref in credential_refs.items()}
        )
    try:
        child = subprocess.Popen(
            list(argv),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
            env=environment,
        )
    except OSError as exc:
        raise ProviderUnavailable("not_implemented", "native_command") from exc
    deadline = time.monotonic() + timeout_s
    output = bytearray()
    total = 0
    try:
        with selectors.DefaultSelector() as selector:
            for name, stream in (("stdout", child.stdout), ("stderr", child.stderr)):
                if stream is None:
                    raise ProviderUnavailable("probe_failed", "native_pipe")
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ, name)
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise ProviderUnavailable("timeout", "native_command")
                for key, _ in selector.select(remaining):
                    chunk = os.read(key.fd, min(8192, MAX_BYTES + 1 - total))
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    total += len(chunk)
                    if total > MAX_BYTES:
                        raise ProviderUnavailable("byte_limit", "native_command")
                    if key.data == "stdout":
                        output.extend(chunk)
        remaining = deadline - time.monotonic()
        if remaining <= 0 or child.wait(timeout=remaining) != 0:
            raise ProviderUnavailable("probe_failed", "native_command")
        return bytes(output)
    except subprocess.TimeoutExpired as exc:
        raise ProviderUnavailable("timeout", "native_command") from exc
    finally:
        # Before reaping, the owned pid cannot be reused. On failure kill its
        # group so descendants cannot retain a pipe; never signal a reaped pid.
        if child.returncode is None:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        child.wait()
        for stream in (child.stdout, child.stderr):
            if stream is not None:
                stream.close()


def bounded_file(path: Path, maximum: int = MAX_BYTES) -> str:
    """Read a fixed native data file with an explicit byte and UTF-8 bound."""
    try:
        with path.open("rb") as stream:
            raw = stream.read(maximum + 1)
        if len(raw) > maximum:
            raise ProviderUnavailable("byte_limit", "native_file")
        return raw.decode("utf-8").rstrip("\x00\n")
    except (OSError, UnicodeDecodeError) as exc:
        raise ProviderUnavailable("not_supported", "native_file") from exc


def parse_packages(raw: bytes) -> dict[str, str]:
    """Parse exact package identities; duplicate and malformed rows are refused."""
    if len(raw) > MAX_BYTES:
        raise ProviderUnavailable("byte_limit", "packages")
    result: dict[str, str] = {}
    try:
        for line in raw.decode("utf-8").splitlines():
            name, version = line.split("\t")
            if (
                name in result
                or len(result) >= MAX_ROWS
                or re.fullmatch(r"[a-z0-9][a-z0-9+.-]{0,127}", name) is None
                or re.fullmatch(r"[A-Za-z0-9.+:~_-]{1,128}", version) is None
            ):
                raise ValueError("invalid package row")
            result[name] = version
    except (ValueError, UnicodeDecodeError) as exc:
        raise ProviderUnavailable("invalid_observation", "packages") from exc
    return result


def match_jetson_profile(
    observed: dict[str, Any], expected: dict[str, Any]
) -> list[str]:
    """Return exact identity mismatches; inventory alone never passes execution."""
    fields = ("os", "architecture", "board", "ubuntu", "l4t", "jetpack", "cuda_toolkit")
    return [field for field in fields if observed.get(field) != expected[field]]


def jetson_snapshot() -> dict[str, Any]:
    """Read board, exact packages, power policy and thermal sources without mutations."""
    if platform.system() != "Linux" or platform.machine() != "aarch64":
        raise ProviderUnavailable("not_supported", "jetson")
    board = bounded_file(Path("/proc/device-tree/model"), 512)
    if "Jetson Orin Nano" not in board:
        raise ProviderUnavailable("not_supported", "jetson_board")
    packages = parse_packages(
        bounded_command(
            [
                "/usr/bin/dpkg-query",
                "-W",
                "-f=${Package}\t${Version}\n",
                "nvidia-jetpack",
                "nvidia-l4t-core",
                "cuda-toolkit-13-2",
                "cuda-cudart-13-2",
                "cuda-nvcc-13-2",
                "nvidia-container-toolkit",
            ]
        )
    )
    os_release = dict(
        line.split("=", 1)
        for line in bounded_file(Path("/etc/os-release")).splitlines()
        if "=" in line
    )
    toolkit = json.loads(bounded_file(Path("/usr/local/cuda/version.json")))
    thermal = []
    for zone in sorted(Path("/sys/class/thermal").glob("thermal_zone*"))[:MAX_ROWS]:
        try:
            thermal.append(
                {
                    "source": str(zone),
                    "type": bounded_file(zone / "type", 128),
                    "temperature_mc": int(bounded_file(zone / "temp", 32)),
                }
            )
        except (ProviderUnavailable, ValueError):
            thermal.append({"source": str(zone), "state": "not_supported"})
    try:
        power: dict[str, Any] = {
            "source": "nvpmodel-query",
            "observation": bounded_command(["/usr/sbin/nvpmodel", "-q"]).decode(
                "utf-8"
            ),
        }
    except ProviderUnavailable:
        power = {"state": "not_supported"}
    clocks = []
    for root, pattern, filename, scale in (
        (Path("/sys/devices/system/cpu/cpufreq"), "policy*", "scaling_cur_freq", 1000),
        (Path("/sys/class/devfreq"), "*", "cur_freq", 1),
    ):
        for device in sorted(root.glob(pattern))[:MAX_ROWS]:
            source = device / filename
            try:
                rate = int(bounded_file(source, 32)) * scale
                if not 0 <= rate <= 10**12:
                    raise ValueError("clock bound")
                clocks.append({"source": str(source), "frequency_hz": rate})
            except (ProviderUnavailable, ValueError):
                clocks.append({"source": str(source), "state": "not_supported"})
    cooling = []
    for device in sorted(Path("/sys/class/thermal").glob("cooling_device*"))[:MAX_ROWS]:
        try:
            kind = bounded_file(device / "type", 128)
            state = int(bounded_file(device / "cur_state", 32))
            maximum = int(bounded_file(device / "max_state", 32))
            if not 0 <= state <= maximum <= 2**32 - 1:
                raise ValueError("cooling bound")
            cooling.append(
                {
                    "source": str(device),
                    "type": kind,
                    "state": state,
                    "maximum_state": maximum,
                }
            )
        except (ProviderUnavailable, ValueError):
            cooling.append({"source": str(device), "state": "not_supported"})
    return {
        "os": "linux",
        "architecture": platform.machine(),
        "board": board,
        "ubuntu": os_release.get("VERSION_ID", "").strip('"'),
        "l4t": packages["nvidia-l4t-core"].split("-", 1)[0],
        "jetpack": packages["nvidia-jetpack"].split("-", 1)[0],
        "cuda_toolkit": toolkit["cuda"]["version"],
        "packages": packages,
        "kernel": platform.release(),
        "thermal": thermal,
        "power": power,
        "clocks": {"source": "linux-cpufreq-devfreq", "observations": clocks},
        "throttling": {
            "source": "linux-thermal-cooling-devices",
            "observations": cooling,
            "semantics": "native cooling state, including supported throttle-alert devices",
        },
    }


def network_snapshot() -> dict[str, Any]:
    """Read native link/address/route counters; unsupported hosts never succeed as no-ops."""
    if platform.system() == "Linux":
        sources: dict[str, Any] = {}
        for key, args in (
            ("links", ["-s", "link", "show"]),
            ("addresses", ["address", "show"]),
            ("routes", ["route", "show"]),
        ):
            parsed = json.loads(bounded_command(["/usr/sbin/ip", "-json", *args]))
            if not isinstance(parsed, list) or len(parsed) > MAX_ROWS:
                raise ProviderUnavailable("row_limit", "network")
            sources[key] = parsed
        return {"source": "linux-rtnetlink-ip-json", **sources}
    if platform.system() == "Darwin":
        helper = os.environ.get("COHESIX_MACOS_NETWORK_HELPER")
        if not helper or not Path(helper).is_absolute():
            raise ProviderUnavailable("not_enabled", "macos_network_helper")
        value = json.loads(bounded_command([helper]))
        if (
            not isinstance(value, dict)
            or value.get("schema") != "cohesix-macos-network/v1"
            or len(value.get("interfaces", [])) > MAX_ROWS
        ):
            raise ProviderUnavailable("invalid_observation", "macos_network")
        # The native helper reports default routes; this bounded native table
        # preserves all routes without inventing parsed reachability or liveness.
        table = bounded_command(["/usr/sbin/netstat", "-rn"])
        value["route_table"] = {
            "source": "Darwin-netstat-routing-socket",
            "text": table.decode("utf-8"),
        }
        return value
    raise ProviderUnavailable("not_supported", "network")


class DockerConnection(http.client.HTTPConnection):
    """HTTP connection to an explicitly selected local Engine socket."""

    def __init__(self, path: Path) -> None:
        super().__init__("localhost", timeout=5)
        self.path = path

    def connect(self) -> None:
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        try:
            self.sock.connect(str(self.path))
        except BaseException:
            self.sock.close()
            self.sock = None
            raise


def docker_api_version(version: dict[str, Any]) -> str:
    """Negotiate the implemented 1.40 API contract within the daemon's interval."""
    if not isinstance(version, dict):
        raise ProviderUnavailable("invalid_observation", "docker_api_version")

    def parse(value: Any) -> tuple[int, int]:
        if not isinstance(value, str) or re.fullmatch(r"1\.[0-9]{2}", value) is None:
            raise ProviderUnavailable("invalid_observation", "docker_api_version")
        major, minor = value.split(".")
        return int(major), int(minor)

    lower = parse(version.get("MinAPIVersion"))
    upper = parse(version.get("ApiVersion"))
    if not lower <= (1, 40) <= upper:
        raise ProviderUnavailable("not_supported", "docker_api_version")
    return "1.40"


def docker_snapshot(socket_path: Path = Path("/var/run/docker.sock")) -> dict[str, Any]:
    """Inspect bounded Engine API objects without exposing labels or environment secrets."""

    def get(path: str) -> Any:
        connection = DockerConnection(socket_path)
        try:
            connection.request("GET", path)
            response = connection.getresponse()
            raw = response.read(MAX_BYTES + 1)
            if response.status != 200 or len(raw) > MAX_BYTES:
                raise ProviderUnavailable("invalid_observation", "docker")
            return json.loads(raw)
        except (OSError, http.client.HTTPException, ValueError) as exc:
            raise ProviderUnavailable("unavailable", "docker") from exc
        finally:
            connection.close()

    version = get("/version")
    api = docker_api_version(version)
    containers = get(f"/v{api}/containers/json?all=1&limit={MAX_ROWS}")
    if (
        not isinstance(containers, list)
        or len(containers) > MAX_ROWS
        or any(not isinstance(row, dict) for row in containers)
    ):
        raise ProviderUnavailable("row_limit", "docker")
    return {
        "source": "docker-engine-api",
        "api_version": api,
        "version": version.get("Version"),
        "containers": [
            {key: row.get(key) for key in ("Id", "ImageID", "State", "Status")}
            for row in containers
        ],
    }


def discover(provider_id: str, profile_id: str, now_ms: int) -> dict[str, Any]:
    """Return an expiring native observation with an explicit non-receipt proof class."""
    expected = profile(profile_id)
    if type(now_ms) is not int or not 0 <= now_ms < 2**63 - 60000:
        raise ProviderUnavailable("invalid_time", provider_id)
    if provider_id == "jetson":
        data = jetson_snapshot()
    elif provider_id == "network":
        data = network_snapshot()
    elif provider_id == "docker":
        data = docker_snapshot()
    else:
        raise ProviderUnavailable("not_implemented", provider_id)
    mismatches = match_jetson_profile(data, expected) if provider_id == "jetson" else []
    observation = {
        "schema": "cohesix-native-observation/v1",
        "provider_id": provider_id,
        "profile_id": profile_id,
        "proof_class": "read_only",
        "observed_mode": "live",
        "observation_unix_ms": now_ms,
        "expires_unix_ms": now_ms + expected["observation_ttl_ms"],
        "profile_mismatches": mismatches,
        "data": data,
    }
    if len(json.dumps(observation, sort_keys=True).encode()) > MAX_BYTES:
        raise ProviderUnavailable("byte_limit", provider_id)
    return observation
