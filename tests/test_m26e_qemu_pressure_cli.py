# Author: Lukas Bower
# Purpose: Verify release-pressure cleanup ownership and approved operator command setup.
# Copyright 2026 Lukas Bower

"""Check preflight and emitted commands without cleanup, build, or QEMU."""

import ast
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import tomllib
from types import SimpleNamespace
import urllib.error
import urllib.request

import pytest
from scripts.ci import check_host_integration_inventory as host_integration
from scripts.lib.host_ticket_result_barrier import TerminalResultBarrier


ROOT = Path(__file__).resolve().parents[1]


def test_pressure_helpers_preserve_sealed_preflight_agent_state(tmp_path: Path) -> None:
    """The eight-lane pressure agent cannot reuse the one-lane receipt WAL."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    function = "start_pressure_helpers() {" + source.split(
        "start_pressure_helpers() {", 1,
    )[1].split("\nstop_pressure_helpers() {", 1)[0]
    boot = tmp_path / "boot with spaces"
    preflight = boot / "host-ticket-agent"
    preflight.mkdir(parents=True)
    sentinels = {name: f"sealed preflight {name}\n" for name in (
        "cursor.json", "execution-journal.json", "agent.lock",
    )}
    for name, contents in sentinels.items():
        (preflight / name).write_text(contents)
    tools = tmp_path / "host-tools"
    tools.mkdir()
    for name in ("gpu-bridge-host", "host-ticket-agent"):
        executable = tools / name
        executable.write_text(
            f"#!{sys.executable}\n"
            "import json, pathlib, sys\n"
            "pathlib.Path(__file__).with_suffix('.argv.json').write_text("
            "json.dumps(sys.argv[1:]))\n"
        )
        executable.chmod(0o755)
    result = subprocess.run(
        ["bash", "-eu", "-c", function + "\n"
         'stop_pid() { :; }; sleep() { :; }; kill() { :; }\n'
         'AGENT_PID= GPU_REFRESH_PID= M26E_REST_AUTH_TOKEN=test-token\n'
         'HOST_TOOLS="$2" RESOLVED_MANIFEST="$3"\n'
         'start_pressure_helpers "$1"\nwait\n',
         "pressure-helper-test", str(boot), str(tools), str(tmp_path / "manifest.json")],
        capture_output=True, text=True, timeout=10,
    )
    assert result.returncode == 0, result.stderr
    argv = json.loads((tools / "host-ticket-agent.argv.json").read_text())
    assert argv[argv.index("--execution-lanes") + 1] == "8"
    pressure_paths = [Path(argv[argv.index(option) + 1]) for option in (
        "--cursor", "--execution-journal", "--agent-lock",
    )]
    assert len({path.parent for path in pressure_paths}) == 1
    assert pressure_paths[0].parent.is_dir()
    assert pressure_paths[0].parent != preflight
    for name, contents in sentinels.items():
        assert (preflight / name).read_text() == contents


def test_terminal_result_barrier_orders_retirement_before_unchanged_publication() -> None:
    """Only the correlated terminal ECHO is held; credentials remain mandatory."""
    events = []

    class Upstream(BaseHTTPRequestHandler):
        def log_message(self, *_args: object) -> None:
            pass

        def do_POST(self) -> None:
            events.append(("published", self.rfile.read(int(self.headers["Content-Length"]))))
            self.send_response(201)
            self.send_header("Content-Length", "2")
            self.end_headers()
            self.wfile.write(b"{}")

    upstream = HTTPServer(("127.0.0.1", 0), Upstream)
    thread = threading.Thread(target=upstream.serve_forever, daemon=True)
    thread.start()
    result = {"schema": "host-ticket-result/v2", "id": "ticket-1", "state": "expired"}
    body = json.dumps({"path": "/host/tickets/status", "line": json.dumps(result)}).encode()
    try:
        with TerminalResultBarrier(
            f"http://127.0.0.1:{upstream.server_port}", "test-token", "ticket-1",
            lambda row: events.append(("retired", row)),
        ) as barrier:
            request = urllib.request.Request(barrier.url + "/v1/fs/echo", data=body)
            with pytest.raises(urllib.error.HTTPError) as refused:
                urllib.request.urlopen(request, timeout=5)
            assert refused.value.code == 401
            assert events == []
            request.add_header("Authorization", "Bearer test-token")
            with urllib.request.urlopen(request, timeout=5) as response:
                assert response.status == 201
                assert response.read() == b"{}"
            barrier.verify()
            assert events == [("retired", result), ("published", body)]
            with pytest.raises(urllib.error.HTTPError) as duplicate:
                urllib.request.urlopen(request, timeout=5)
            assert duplicate.value.code == 502
            with pytest.raises(ValueError, match="exactly one"):
                barrier.verify()
            assert len(events) == 2
    finally:
        upstream.shutdown()
        upstream.server_close()
        thread.join(timeout=5)


@pytest.mark.parametrize("upstream", ["https://127.0.0.1:8080", "http://localhost:8080",
                                      "http://127.0.0.1:8080/private"])
def test_terminal_result_barrier_requires_local_gateway(upstream: str) -> None:
    with pytest.raises(ValueError, match="loopback"):
        TerminalResultBarrier(upstream, "test-token", "ticket-1", lambda _: None)


@pytest.mark.parametrize("status", [0, 7])
def test_background_uart_capture_does_not_echo_input_eof(
    tmp_path: Path, status: int,
) -> None:
    """Capture command output without terminal control bytes from the launcher."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    function = "start_uart_capture() {" + source.split(
        "start_uart_capture() {", 1,
    )[1].split("\nverify_live_artifacts() {", 1)[0]
    uart = tmp_path / "uart.log"
    result = subprocess.run(
        ["bash", "-eu", "-c", function + '\nHARNESS_PYTHON="$2"\n'
         'start_uart_capture "$1" "$2" -c "$3"\n',
         "capture-test", str(uart), sys.executable,
         f'import sys; print("capture-sentinel"); sys.exit({status})'],
        stdin=subprocess.DEVNULL, capture_output=True, timeout=10,
    )
    assert result.returncode == status, result.stderr
    raw = uart.read_bytes()
    assert b"capture-sentinel\r\n" in raw
    assert all(byte >= 0x20 or byte in (9, 10, 13) for byte in raw)
    assert result.stdout == raw


@pytest.mark.parametrize("worker_log", ["complete", "missing", "truncated"])
def test_host_integration_binds_complete_worker_exports(
    tmp_path: Path, worker_log: str,
) -> None:
    """Worker proof comes from authenticated fragments, with UART kept distinct."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    embedded = source.split("emit_host_integration() {", 1)[1].split("<<'PY'\n", 1)[1]
    embedded = embedded.split("\nPY\n", 1)[0]
    graph, manifest, uart, cohsh, matrix, worker, stale, output = [
        tmp_path / name for name in (
            "graph.json", "manifest.json", "uart.log", "cohsh.log",
            "matrix.json", "worker.log", "stale.json", "observations.json",
        )
    ]
    stale.write_text(json.dumps({
        "schema": "cohesix-stale-ticket-observations/v1",
        "records": [
            {"action": action, "role": role}
            for role, actions in (
                ("worker-gpu", ("gpu.lease.grant", "gpu.lease.renew", "gpu.lease.release")),
                ("worker-lora", ("peft.export", "peft.import", "peft.activate", "peft.rollback")),
            ) for action in actions
        ],
    }))
    graph.write_text("{}\n")
    manifest.write_text("{}\n")
    uart.write_text("QEMU boot identity belongs to UART\n")
    cohsh.write_text("OK SPAWN\nOK KILL\nERR worker-bus model-only mode=fixture\n")
    matrix.write_text(json.dumps({
        "schema": "cohesix-qemu-receipt-matrix/v1",
        "records": [
            {"action": action, "role": role, "outcome": outcome}
            for role, actions in (
                ("worker-gpu", ("gpu.lease.grant", "gpu.lease.renew", "gpu.lease.release")),
                ("worker-lora", ("peft.export", "peft.import", "peft.activate", "peft.rollback")),
            )
            for action in actions for outcome in ("succeeded", "failed", "expired")
        ],
    }))
    markers = [
        "WORKER_TASK_RECEIPT", "WORKER_TASK_COMPLETION",
        "WORKER_TASK_READY role=worker-heartbeat",
        "WORKER_TASK_READY role=worker-gpu", "WORKER_TASK_READY role=worker-lora",
        "GPU_BRIDGE_FIXTURE_ADMISSION fixture=qemu",
        "LORA_EXPORT_FIXTURE_ADMISSION fixture=qemu",
    ]
    if worker_log == "missing":
        markers.remove("WORKER_TASK_RECEIPT")
    fragments = []
    for index, marker in enumerate(markers):
        fragments += [
            f"WORKER_LOG id={index} part=0 last=0 data={marker[:8]}",
            f"WORKER_LOG id={index} part=1 last=1 data={marker[8:]}",
        ]
    if worker_log == "truncated":
        fragments.pop()
    worker.write_text("\n".join(fragments) + "\n")
    result = subprocess.run(
        [sys.executable, "-c", embedded, *map(str, (
            graph, manifest, uart, cohsh, matrix, worker, stale, output,
        ))], cwd=ROOT, text=True, capture_output=True, timeout=10,
    )
    if worker_log != "complete":
        assert result.returncode != 0
        assert not output.exists()
        assert ("WORKER_TASK_RECEIPT" if worker_log == "missing" else "incomplete") in result.stderr
        return
    assert result.returncode == 0, result.stderr
    observations = list(host_integration._load_observations(
        output, hashlib.sha256(graph.read_bytes()).hexdigest(),
        hashlib.sha256(manifest.read_bytes()).hexdigest(),
    ).values())
    assert {row["dependency_id"] for row in observations} == {
        "gpu-receipt-path", "peft-receipt-path", "worker-control",
    }
    expected = {"id": "worker-log-transcript", "bytes": worker.stat().st_size,
                "sha256": hashlib.sha256(worker.read_bytes()).hexdigest()}
    for row in observations:
        assert expected in row["raw_evidence"]


@pytest.mark.parametrize("worker_completed", [True, False])
def test_expired_receipt_keeps_the_admitted_worker_alive(worker_completed: bool) -> None:
    """Expiry requires a live recipient; generation invalidation has its own lane."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    embedded = source.split("drive_receipt_matrix() {", 1)[1].split("<<'PY'\n", 1)[1]
    embedded = embedded.split("\nPY\n", 1)[0]
    submit = next(node for node in ast.parse(embedded).body
                  if isinstance(node, ast.FunctionDef) and node.name == "submit")
    writes = []

    def echo(path: str, line: str) -> SimpleNamespace:
        writes.append((path, json.loads(line)))
        return SimpleNamespace(status="OK")

    times = iter([0, 0, 0, 21])
    scope = {
        "sequence": 0, "json": json,
        "ready": lambda role: SimpleNamespace(
            worker_id="worker7", supervisor_generation=3, cap_generation=4,
            role="worker-lora", slot=0, lease_epoch=1,
            receipt_sequence=0, completion_sequence=0,
        ),
        "client": SimpleNamespace(echo=echo), "run_agent": lambda: None,
        "time": SimpleNamespace(monotonic=lambda: next(times), sleep=lambda _: None),
        "terminal": lambda ticket: "expired", "records": [],
        "capture_worker_records": lambda: None,
        "rest": SimpleNamespace(read_host_ticket_current=lambda *args: SimpleNamespace(
            state="rejected", worker_id="worker7", role="worker-lora", slot=0,
            lease_epoch=1, supervisor_generation=3, cap_generation=4,
            lifecycle="ready", receipt_sequence=int(worker_completed),
            completion_sequence=int(worker_completed),
            control_sequence=0,
        )),
    }
    exec(compile(ast.Module(body=[submit], type_ignores=[]), "receipt-submit", "exec"), scope)
    if not worker_completed:
        with pytest.raises(RuntimeError, match="lacks its exact Worker receipt"):
            scope["submit"]("peft.export", "worker-lora", {}, "job", "expired", "operation")
        return
    scope["submit"]("peft.export", "worker-lora", {}, "job", "expired", "operation")
    assert len(writes) == 1
    assert writes[0][0] == "/host/tickets/spec"
    assert writes[0][1]["expires_unix_ms"] == 1
    assert writes[0][1]["receipt_worker_id"] == "worker7"
    assert writes[0][1]["receipt_supervisor_generation"] == 3
    assert writes[0][1]["receipt_cap_generation"] == 4


def test_receipt_fixture_contains_the_exported_adapters_base(tmp_path: Path) -> None:
    """The QEMU export's base_model.ref must resolve after a real PEFT import."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    function = "prepare_host_fixture() {" + source.split(
        "prepare_host_fixture() {", 1,
    )[1].split("\ntrigger_disposable_worker_control() {", 1)[0]
    subprocess.run(
        ["bash", "-eu", "-c", function + '\nprepare_host_fixture "$1"\n',
         "fixture-test", str(tmp_path)],
        check=True, capture_output=True, text=True, timeout=10,
    )
    available = tmp_path / "peft-registry/available"
    exported_base = tomllib.loads((available / "fixture-base-model/manifest.toml").read_text())
    assert exported_base["model"]["id"] == "fixture-base-model"
    assert exported_base["model"]["format"] == "gguf"
    for path in available.glob("*/manifest.toml"):
        model = tomllib.loads(path.read_text())["model"]
        if "base" in model:
            assert (available / model["base"] / "manifest.toml").is_file()


@pytest.fixture
def checkout(tmp_path: Path) -> Path:
    """Create a disposable checkout with output that preflight must preserve."""
    repo = tmp_path.resolve() / "checkout"
    (repo / "scripts").mkdir(parents=True)
    shutil.copy2(ROOT / "scripts/m26e_qemu_pressure.sh", repo / "scripts")
    subprocess.run(["git", "init", "--quiet", str(repo)], check=True)
    for name in ("out", "target"):
        (repo / name).mkdir()
        (repo / name / "retained").write_text("keep\n", encoding="utf-8")
    (repo / "nonexecutable-qemu").write_text("not an executable\n", encoding="utf-8")
    (repo / ".gitignore").write_text("/out/\n/target/\n", encoding="utf-8")
    subprocess.run(["git", "add", "."], cwd=repo, check=True)
    subprocess.run(
        ["git", "-c", "user.name=Test Fixture", "-c",
         "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false",
         "-c", "core.hooksPath=/dev/null", "commit", "--quiet", "-m", "fixture"],
        cwd=repo, check=True,
    )
    subprocess.run(["git", "checkout", "--detach", "--quiet"], cwd=repo, check=True)
    return repo


def invoke(checkout: Path, *options: str) -> subprocess.CompletedProcess[str]:
    """Stop in preflight and verify both output sentinels after every case."""
    result = subprocess.run(
        ["bash", str(checkout / "scripts/m26e_qemu_pressure.sh"),
         "--check-only", "--qemu", str(checkout / "nonexecutable-qemu"), *options],
        cwd=checkout, text=True, capture_output=True, check=False, timeout=10,
    )
    for name in ("out", "target"):
        assert (checkout / name / "retained").read_text(encoding="utf-8") == "keep\n"
    return result


def test_unselected_checkout_cannot_enter_cleanup(checkout: Path) -> None:
    result = invoke(checkout)
    assert result.returncode == 2
    assert "refusing to clean an unexpected repository root" in result.stderr


@pytest.mark.parametrize("selection", ["parent", "relative", "alias"])
def test_clean_root_must_be_the_exact_checkout(checkout: Path, selection: str) -> None:
    selected = {"parent": str(checkout.parent), "relative": "."}.get(selection)
    if selection == "alias":
        alias = checkout.parent / "checkout-alias"
        alias.symlink_to(checkout, target_is_directory=True)
        selected = str(alias)
    assert selected is not None
    result = invoke(checkout, "--clean-root", selected)
    assert result.returncode == 2
    assert "--clean-root must equal this exact checkout root" in result.stderr


def test_selected_checkout_advances_only_to_tool_preflight(checkout: Path) -> None:
    result = invoke(checkout, "--clean-root", str(checkout))
    assert result.returncode != 0
    assert f"file is not executable: {checkout / 'nonexecutable-qemu'}" in result.stderr


def test_replay_cannot_authorize_cleanup(checkout: Path) -> None:
    result = invoke(checkout, "--clean-root", str(checkout), "--reuse-artifacts")
    assert result.returncode == 2
    assert "--clean-root cannot be used with --reuse-artifacts" in result.stderr


@pytest.mark.parametrize(("arguments", "expected"), [
    (["--run-dir", "out/../escape"], "may not contain '..'"),
    (["--run-dir", "out/toolchain/sel4-profile-venv/evidence"], "direct child"),
    (["--sel4-source", "/"], "outside its required root"),
    (["--profile-python", "/bin/python"], "canonical repository virtualenv"),
])
def test_selected_checkout_rejects_hostile_path_overrides(
    checkout: Path, arguments: list[str], expected: str,
) -> None:
    """Exercise path admission independently of the caller's checkout or dirt."""
    source = checkout / "out" / "sel4" / "source"
    source.mkdir(parents=True)
    (checkout / "out" / "toolchain" / "sel4-profile-venv").mkdir(parents=True)
    result = invoke(
        checkout, "--clean-root", str(checkout),
        "--qemu", str(Path(sys.executable).resolve()),
        "--sel4-source", str(source), *arguments,
    )
    assert result.returncode != 0
    assert expected in result.stderr


@pytest.mark.parametrize("kind", ["tracked", "untracked"])
def test_selected_checkout_rejects_uncommitted_source(
    checkout: Path, kind: str,
) -> None:
    path = checkout / ("nonexecutable-qemu" if kind == "tracked" else "new-source")
    path.write_text("uncommitted input\n", encoding="utf-8")
    result = invoke(checkout, "--clean-root", str(checkout))
    assert result.returncode == 2
    assert "--clean-root requires an exact clean candidate checkout" in result.stderr


@pytest.mark.parametrize("command", ["spawn heartbeat ticks=100", "kill worker1", "ls /"])
def test_control_script_approves_only_mutations(tmp_path: Path, command: str) -> None:
    """The generated script follows the documented single-use approval order."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text(encoding="utf-8")
    function = "run_cohsh_command() {" + source.split(
        "run_cohsh_command() {", 1,
    )[1].split("\n}\n", 1)[0] + "\n}\n"
    host_tools = tmp_path / "host-tools"
    host_tools.mkdir()
    cohsh = host_tools / "cohsh"
    cohsh.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    cohsh.chmod(0o700)
    subprocess.run(
        ["bash", "-eu", "-c", function + '\nGATEWAY_PID=\n'
         'HOST_TOOLS="$1"\nM26E_CONSOLE_AUTH_TOKEN=fixture\n'
         'run_cohsh_command "$2" "$3" 17\n',
         "qualification-test", str(host_tools), str(tmp_path), command],
        check=True, timeout=10, capture_output=True, text=True,
    )
    lines = (tmp_path / "cohsh-command-17.coh").read_text(encoding="utf-8").splitlines()
    commands = [line for line in lines if not line.startswith("#")]
    approval = (
        'echo \'{"id":"m26e-control-17","target":"/queen/ctl",'
        '"decision":"approve"}\' > /actions/queue'
    )
    prefix = ["attach queen", "EXPECT OK"]
    if command.startswith(("spawn ", "kill ")):
        prefix += [approval, "EXPECT SUBSTR path=/actions/queue"]
    assert commands == prefix + [command, "EXPECT OK", "quit"]


@pytest.mark.parametrize("role", ["worker-heartbeat", "worker-gpu", "worker-lora"])
def test_fault_plan_drives_two_passive_lifecycle_calls(tmp_path: Path, role: str) -> None:
    """Each role needs received IPC between READY and the two execution faults."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text(encoding="utf-8")
    function = "drive_worker_fault_plan() {" + source.split(
        "drive_worker_fault_plan() {", 1,
    )[1].split("\n}\n", 1)[0] + "\n}\n"
    host_tools = tmp_path / "host-tools"
    host_tools.mkdir()
    fixture = host_tools / "gpu-bridge-host"
    fixture.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    fixture.chmod(0o700)
    (tmp_path / "uart.live.log").write_text("", encoding="utf-8")
    result = subprocess.run(
        ["bash", "-eu", "-c", function + '''
HARNESS_PYTHON=true
GDB_BIN=fixture
TARGET_SESSION=fixture
GENERATED_INVENTORY=fixture
WORKER_MANIFEST=fixture
WORKER_HEART_ELF=fixture
WORKER_GPU_ELF=fixture
WORKER_LORA_ELF=fixture
M26E_CONSOLE_AUTH_TOKEN=fixture
HOST_TOOLS="$1/host-tools"
GDB_RUNNER_PID=
sleep() { :; }
wait_for_marker_count() { :; }
capture_worker_log() { :; }
worker_marker_count() { printf '0\n'; }
spawn_command_for_role() { printf 'spawn %s\n' "$1"; }
run_cohsh_command() { printf 'operator %s\n' "$2"; }
trigger_disposable_worker_control() { printf 'shutdown %s %s\n' "$2" "$3"; }
drive_worker_fault_plan "$1" "$2" 100
''', "fault-plan-test", str(tmp_path), role],
        check=True, capture_output=True, text=True, timeout=10,
    )
    assert result.stdout.splitlines() == [
        f"operator spawn {role}", f"operator spawn {role}",
        f"shutdown {role} 110", f"operator spawn {role}",
        f"shutdown {role} 111", f"operator spawn {role}",
    ]


def test_fault_control_refuses_an_existing_gateway_owner(tmp_path: Path) -> None:
    """A phase error must stop before attempting direct TCP authentication."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text(encoding="utf-8")
    function = "trigger_disposable_worker_control() {" + source.split(
        "trigger_disposable_worker_control() {", 1,
    )[1].split("\ndrive_worker_fault_plan() {", 1)[0]
    result = subprocess.run(
        ["bash", "-eu", "-c", function + '''
die() { printf '%s\n' "$*" >&2; exit 2; }
GATEWAY_PID=123
trigger_disposable_worker_control "$1" worker-heartbeat 1
''', "fault-owner-test", str(tmp_path)],
        check=False, capture_output=True, text=True, timeout=10,
    )
    assert result.returncode == 2
    assert "requires the pre-gateway phase" in result.stderr
