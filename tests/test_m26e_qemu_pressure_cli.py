# Author: Lukas Bower
# Purpose: Verify release-pressure cleanup ownership and approved operator command setup.
# Copyright 2026 Lukas Bower

"""Check preflight and emitted commands without cleanup, build, or QEMU."""

import ast
import hashlib
import http.client
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


def test_linux_replay_preserves_source_record_and_exclusive_session(tmp_path: Path) -> None:
    """Replay custody must not precreate the collector's exclusively owned output."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    preparation = source.split('else\n    export COHESIX_QEMU_ACCEL=kvm', 1)[1].split(
        '\nfi\nvalidate_resolved_console_token', 1
    )[0]
    emission = source.split('TARGET_SESSION="$RUN_DIR/session/target-session.json"', 1)[1].split(
        '\nlog "target session emitted:', 1
    )[0]
    artifact = tmp_path / "artifact"
    artifact.mkdir()
    original = b'{"host":"macos","identity":"original guest custody"}\n'
    launch = artifact / "cohesix-qemu-launch-artifacts.json"
    launch.write_bytes(original)
    tools = tmp_path / "target/release"
    tools.mkdir(parents=True)
    for name in ("cohsh", "hive-gateway", "gpu-bridge-host", "host-ticket-agent"):
        path = tools / name
        path.write_text("#!/bin/sh\nexit 0\n")
        path.chmod(0o755)
    shim = tmp_path / "collector"
    shim.write_text(
        f"#!{sys.executable}\n"
        "import pathlib,sys\n"
        "args=sys.argv[1:]\n"
        "if 'write' in args:\n"
        " p=pathlib.Path(args[args.index('--out-dir')+1])\n"
        " (p/'cohesix-qemu-launch-artifacts.json').write_text('native Linux envelope')\n"
        "if 'emit-qemu-target-session' in args:\n"
        " p=pathlib.Path(args[args.index('--out-dir')+1])\n"
        " p.mkdir(mode=0o700,exist_ok=False)\n"
        " (p/'target-session.json').write_text('{}')\n"
    )
    shim.chmod(0o755)
    run = tmp_path / "run"
    result = subprocess.run(
        ["bash", "-eu", "-c",
         'RUN_DIR="$1" OUT_ROOT="$2" TARGET_DIR="$3" HARNESS_PYTHON="$4"\n'
         'REPO_ROOT="$5" SEL4_BUILD="$5" SEL4_PROFILE=qemu_smp_kvm_production\n'
         'QEMU_BIN=qemu REUSE_ARTIFACTS=1 RESOLVED_MANIFEST=manifest GENERATED_INVENTORY=topology\n'
         'cargo() { :; }; log() { :; }; python3() { echo 3; }\n'
         'die() { echo "$*" >&2; exit 1; }\n'
         + preparation + '\n' + emission,
         "replay-session-test", str(run), str(artifact), str(tools.parent), str(shim), str(tmp_path)],
        capture_output=True, text=True, timeout=10,
    )
    assert result.returncode == 0, result.stderr
    assert (run / "session/target-session.json").is_file()
    assert (run / "session/source-host-launch-record.json").read_bytes() == original
    assert launch.read_text() == "native Linux envelope"
    assert not (run / "source-host-launch-record.json").exists()


def test_pressure_helpers_preserve_sealed_preflight_agent_state(tmp_path: Path) -> None:
    """Pressure resumes the completed WAL without changing the sealed copy."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    function = "start_pressure_helpers() {" + source.split(
        "start_pressure_helpers() {", 1,
    )[1].split("\nstop_pressure_helpers() {", 1)[0]
    boot = tmp_path / "boot with spaces"
    preflight = boot / "host-ticket-agent"
    preflight.mkdir(parents=True)
    sentinels = {
        "execution-journal.topology.json": json.dumps({
            "schema": "host-ticket-execution-lanes/v1", "lanes": 8,
        }),
        "cursor.lane-00-of-08.json": '{"raw_next_spec_index":31}\n',
        "execution-journal.lane-00-of-08.jsonl": "sealed completed ticket\n",
        "agent.lock": "",
    }
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
        assert (pressure_paths[0].parent / name).read_text() == contents


def test_receipt_agent_uses_the_pressure_lane_topology(tmp_path: Path) -> None:
    """Preflight must produce journals that the pressure agent can resume."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    embedded = source.split("drive_receipt_matrix() {", 1)[1].split("<<'PY'\n", 1)[1]
    embedded = embedded.split("\nPY\n", 1)[0]
    function = next(node for node in ast.parse(embedded).body
                    if isinstance(node, ast.FunctionDef) and node.name == "run_agent")
    commands = []

    def run(command: list[str], **_kwargs: object) -> SimpleNamespace:
        commands.append(command)
        return SimpleNamespace(returncode=0, stdout="", stderr="")

    scope = {
        "agent": tmp_path / "agent", "manifest": tmp_path / "manifest.json",
        "state_dir": tmp_path / "state", "boot": tmp_path,
        "agent_log": tmp_path / "agent.log",
        "subprocess": SimpleNamespace(run=run),
    }
    exec(compile(ast.Module(body=[function], type_ignores=[]), "receipt-agent", "exec"), scope)
    scope["run_agent"]()
    command, = commands
    assert command[command.index("--execution-lanes") + 1] == "8"
    assert "--run-once" in command


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


@pytest.mark.parametrize("method", ["GET", "POST"])
@pytest.mark.parametrize("ticket", [None, "exact-caller-ticket"])
def test_result_barrier_preserves_caller_delegation_and_gateway_refusal(
    method: str, ticket: str | None,
) -> None:
    """The barrier forwards authority unchanged and never supplies a missing ticket."""
    observed = []

    class Upstream(BaseHTTPRequestHandler):
        def log_message(self, *_args: object) -> None:
            pass

        def do_GET(self) -> None:
            self.reply()

        def do_POST(self) -> None:
            self.reply()

        def reply(self) -> None:
            observed.append((self.command, self.headers.get_all("x-cohesix-ticket")))
            self.send_response(200 if self.headers.get("x-cohesix-ticket") else 403)
            self.send_header("Content-Length", "2")
            self.end_headers()
            self.wfile.write(b"{}")

    upstream = HTTPServer(("127.0.0.1", 0), Upstream)
    thread = threading.Thread(target=upstream.serve_forever, daemon=True)
    thread.start()
    try:
        with TerminalResultBarrier(
            f"http://127.0.0.1:{upstream.server_port}", "test-token", "held-result",
            lambda _: pytest.fail("this request must not retire a Worker"),
        ) as barrier:
            request = urllib.request.Request(
                barrier.url + "/v1/fs/cat?path=/host/tickets/spec.snapshot",
                headers={"Authorization": "Bearer test-token"}, method=method,
            )
            if ticket:
                request.add_header("x-cohesix-ticket", ticket)
                with urllib.request.urlopen(request, timeout=5) as response:
                    assert response.status == 200
            else:
                with pytest.raises(urllib.error.HTTPError) as refused:
                    urllib.request.urlopen(request, timeout=5)
                assert refused.value.code == 403
            assert observed == [(method, [ticket] if ticket else None)]
            connection = http.client.HTTPConnection("127.0.0.1", barrier.server.server_port)
            try:
                connection.putrequest(method, "/v1/fs/cat?path=/host/tickets/spec.snapshot")
                connection.putheader("Authorization", "Bearer test-token")
                connection.putheader("x-cohesix-ticket", "first")
                connection.putheader("x-cohesix-ticket", "second")
                connection.endheaders()
                response = connection.getresponse()
                assert response.status == 400
                response.read()
            finally:
                connection.close()
            assert observed == [(method, [ticket] if ticket else None)]
            assert barrier.held == 0
    finally:
        upstream.shutdown()
        upstream.server_close()
        thread.join(timeout=5)


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
        ), host_ticket_authority_fields=lambda client: {"writer_epoch": 9}),
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
    assert writes[0][1]["writer_epoch"] == 9
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
    (repo / "configs").mkdir()
    (repo / "configs/root_task.toml").write_text(
        '[[tickets]]\nrole = "queen"\nsecret_ref = "env:PRESSURE_TEST_KEY"\n',
        encoding="utf-8",
    )
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


@pytest.mark.parametrize("directory", ["out", "target"])
def test_pressure_manifest_cannot_be_cleaned(
    checkout: Path, monkeypatch: pytest.MonkeyPatch, directory: str,
) -> None:
    """An explicit manifest must survive the destructive output cleanup."""
    monkeypatch.setenv("COH_RTC_MANIFEST", str(checkout / directory / "retained"))
    result = invoke(checkout, "--clean-root", str(checkout))
    assert result.returncode == 2
    assert "manifest must be outside the cleaned" in result.stderr


def test_pressure_build_receives_selected_manifest(tmp_path: Path) -> None:
    """The compiler receives the same profile chosen before environment cleanup."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    function = "build_selected_pressure_artifacts() {" + source.split(
        "build_selected_pressure_artifacts() {", 1,
    )[1].split("\n}\n", 1)[0] + "\n}\n"
    selected = tmp_path / "selected profile.toml"
    selected.write_text("profile identity\n", encoding="utf-8")
    build = tmp_path / "build probe"
    build.write_text(
        f"#!{sys.executable}\n"
        "import json, os, sys\n"
        "print(json.dumps([os.environ['COH_RTC_MANIFEST'], sys.argv[1:]]))\n",
        encoding="utf-8",
    )
    build.chmod(0o700)
    result = subprocess.run(
        ["bash", "-eu", "-c", function + '''
SOURCE_MANIFEST="$1"
BUILD_RUN="$2"
BUILD_ARGS=(--profile release --transport tcp)
unset COH_RTC_MANIFEST
build_selected_pressure_artifacts
''', "selected-pressure-build", str(selected), str(build)],
        check=True, capture_output=True, text=True, timeout=10,
    )
    assert json.loads(result.stdout) == [str(selected), [
        "--clean", "--no-run", "--profile", "release", "--transport", "tcp",
    ]]


def test_pressure_gateway_mints_a_finite_caller(tmp_path: Path) -> None:
    """The REST caller is scoped, bounded and separate from request AUTH."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    function = "mint_pressure_delegation() {" + source.split(
        "mint_pressure_delegation() {", 1,
    )[1].split("\n}\n", 1)[0] + "\n}\n"
    tools = tmp_path / "host tools"
    tools.mkdir()
    probe = tools / "cohsh"
    probe.write_text(
        f"#!{sys.executable}\n"
        "import json, os, pathlib, sys\n"
        "key = os.environ['M26E_DELEGATION_SECRET']\n"
        "assert len(key) == 64 and all(c in '0123456789abcdef' for c in key)\n"
        "pathlib.Path(__file__).with_suffix('.json').write_text(json.dumps(sys.argv[1:]))\n"
        "print('finite-test-caller')\n",
        encoding="utf-8",
    )
    probe.chmod(0o700)
    result = subprocess.run(
        ["bash", "-eu", "-c", function + '''
HOST_TOOLS="$1"
SOURCE_MANIFEST="$2"
mint_pressure_delegation
test "$COH_REST_TICKET" = finite-test-caller
test "$HIVE_GATEWAY_DELEGATION_KEY_REF" = env:M26E_DELEGATION_SECRET
''', "pressure-delegation", str(tools), str(tmp_path / "selected.toml")],
        check=True, capture_output=True, text=True, timeout=10,
    )
    assert result.stdout == ""
    assert json.loads(probe.with_suffix(".json").read_text()) == [
        "--mint-ticket", "--role", "queen", "--ticket-subject", "m26e-pressure",
        "--ticket-config", str(tmp_path / "selected.toml"),
        "--ticket-secret", "env:M26E_DELEGATION_SECRET",
        "--ticket-read-scope", "/", "--ticket-write-scope", "/",
        "--ticket-ttl-s", "3600", "--ticket-ops", "1000000",
    ]


def test_pressure_canary_receives_selected_artifacts_and_auth(tmp_path: Path) -> None:
    """A provisioned image never falls back to the default console credential."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    function = "prove_selected_pressure_authentication() {" + source.split(
        "prove_selected_pressure_authentication() {", 1,
    )[1].split("\n}\n", 1)[0] + "\n}\n"
    probe = tmp_path / "scripts/ci/test_plan_target_canary.sh"
    probe.parent.mkdir(parents=True)
    probe.write_text(
        f"#!{sys.executable}\n"
        "import json, os, sys\n"
        "names = ['COHSH_AUTH_TOKEN', 'SEL4_BUILD_DIR', "
        "'TEST_PLAN_CONVERGENCE_LAUNCH_EXISTING', "
        "'TEST_PLAN_CONVERGENCE_QEMU_OUT_DIR', "
        "'TEST_PLAN_CONVERGENCE_QEMU_BIN', 'TEST_PLAN_CONVERGENCE_FOCUS']\n"
        "print(json.dumps([sys.argv[1:], {n: os.environ[n] for n in names}]))\n",
        encoding="utf-8",
    )
    probe.chmod(0o700)
    result = subprocess.run(
        ["bash", "-eu", "-c", function + '''
HARNESS_PYTHON="$1"
AUTH_STATE_DIR="$PWD/observation"
AUTH_OBSERVATION="$AUTH_STATE_DIR/result.json"
M26E_CONSOLE_AUTH_TOKEN=provisioned-fixture-token
SEL4_BUILD="$PWD/selected sel4"
OUT_ROOT="$PWD/retained image"
QEMU_BIN="$PWD/selected qemu"
COHSH_AUTH_TOKEN=wrong-inherited-fixture
prove_selected_pressure_authentication
''', "selected-pressure-canary", sys.executable],
        cwd=tmp_path, check=True, capture_output=True, text=True, timeout=10,
    )
    assert json.loads(result.stdout) == [["--target", "qemu"], {
        "COHSH_AUTH_TOKEN": "provisioned-fixture-token",
        "SEL4_BUILD_DIR": str(tmp_path / "selected sel4"),
        "TEST_PLAN_CONVERGENCE_LAUNCH_EXISTING": "1",
        "TEST_PLAN_CONVERGENCE_QEMU_OUT_DIR": str(tmp_path / "retained image"),
        "TEST_PLAN_CONVERGENCE_QEMU_BIN": str(tmp_path / "selected qemu"),
        "TEST_PLAN_CONVERGENCE_FOCUS": "ninedoor",
    }]


def test_unselected_checkout_cannot_enter_cleanup(checkout: Path) -> None:
    result = invoke(checkout)
    assert result.returncode == 2
    assert "refusing to clean an unexpected repository root" in result.stderr


@pytest.mark.parametrize("failed_stage", [0, 1, 3])
def test_pressure_stages_isolate_credentials_and_stop_on_failure(
    tmp_path: Path, failed_stage: int,
) -> None:
    """Common fixtures stay hermetic and every target stage keeps selected AUTH."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    function = "run_pressure_staged_plan() {" + source.split(
        "run_pressure_staged_plan() {", 1,
    )[1].split("\n}\n", 1)[0] + "\n}\n"
    probe = tmp_path / "scripts/ci/test_plan_run.sh"
    probe.parent.mkdir(parents=True)
    names = [
        "COH_AUTH_TOKEN", "COH_AUTH_TOKEN_REF", "COHSH_AUTH_TOKEN",
        "HIVE_GATEWAY_REQUEST_AUTH_TOKEN", "COH_REST_AUTH_TOKEN",
        "COHSH_REST_AUTH_TOKEN", "COH_REST_TICKET", "COH_PRESSURE_AUTHORITY_MANIFEST",
    ]
    probe.write_text(
        f"#!{sys.executable}\n"
        "import json, os, pathlib, sys\n"
        "stage = int(sys.argv[sys.argv.index('--stage')+1])\n"
        "with pathlib.Path('calls.jsonl').open('a') as output:\n"
        f"    output.write(json.dumps([sys.argv[1:], {{n: os.environ.get(n) for n in {names!r}}}])+'\\n')\n"
        "sys.exit(17 if stage == int(os.environ['FAILED_STAGE']) else 0)\n",
        encoding="utf-8",
    )
    probe.chmod(0o700)
    assignments = "\n".join(f"export {name}=inherited-fixture" for name in names)
    result = subprocess.run(
        ["bash", "-eu", "-c", function + assignments + '''
export FAILED_STAGE="$1"
TEST_PLAN_STATE_DIR="$PWD/fresh state"
M26E_CONSOLE_AUTH_TOKEN=selected-console-fixture
M26E_REST_AUTH_TOKEN=selected-rest-fixture
run_pressure_staged_plan
''', "pressure-stage-credentials", str(failed_stage)],
        cwd=tmp_path, capture_output=True, text=True, check=False, timeout=10,
    )
    assert result.returncode == (17 if failed_stage else 0)
    calls = [json.loads(line) for line in (tmp_path / "calls.jsonl").read_text().splitlines()]
    assert len(calls) == (failed_stage or 5)
    for stage, (arguments, environment) in enumerate(calls, start=1):
        assert arguments == [
            "--target", "qemu", "--state-dir", str(tmp_path / "fresh state"),
            "--stage", str(stage),
        ]
        expected = dict.fromkeys(names)
        if stage > 1:
            expected.update(COHSH_AUTH_TOKEN="selected-console-fixture",
                            HIVE_GATEWAY_REQUEST_AUTH_TOKEN="selected-rest-fixture")
        assert environment == expected


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
         'REPO_ROOT="$4"\nHARNESS_PYTHON="$5"\n'
         'export COH_PRESSURE_AUTHORITY_MANIFEST="$4/configs/generated/root_task_resolved.json"\n'
         'run_cohsh_command "$2" "$3" 17\n',
         "qualification-test", str(host_tools), str(tmp_path), command, str(ROOT), sys.executable],
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


@pytest.mark.parametrize("used", [0, 222])
def test_live_budget_preflight_reads_frozen_manifest_and_refuses_exhaustion(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, used: int,
) -> None:
    """Exercise the actual shell-embedded preflight with read-only backend data."""
    from cohesix import backends
    from scripts import rest_perf_harness as rest

    manifest = tmp_path / "manifest.json"
    manifest.write_text(json.dumps({
        "authority": {
            "production": True, "legacy_queen_ctl": False,
            "strict_queen_intents": True, "writer_epoch_required": True,
            "queen_dedupe_entries": 512, "queen_intent_max_bytes": 2048,
            "writer_epoch": 9,
        },
        "worker_runtime": {"max_workers": 256},
    }))
    reads = []

    class Backend:
        def __init__(self, *args, **kwargs):
            pass

        def read_file(self, path, maximum):
            reads.append(path)
            data = {
                "/proc/authority": {
                    "schema": "authority/v1", "identity": "gateway_enforced",
                    "writer_epoch": 9, "epoch_required": True,
                    "production": True, "strict_intents": True,
                },
                "/proc/queen/dedupe": {
                    "schema": "queen-dedupe/v1", "capacity": 512, "entries": used,
                },
            }[path]
            return json.dumps(data).encode() + b"\n"

        def close(self):
            reads.append("closed")

    monkeypatch.setattr(backends, "TcpBackend", Backend)
    monkeypatch.setenv("COH_PRESSURE_AUTHORITY_MANIFEST", str(manifest))
    monkeypatch.setenv("COH_AUTH_TOKEN", "test-only")
    monkeypatch.setattr(sys, "argv", ["preflight", str(ROOT), str(tmp_path)])
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    body = source.split("<<'PY_BUDGET'\n", 1)[1].split("\nPY_BUDGET", 1)[0]
    if used == 222:
        with pytest.raises(rest.RestError, match="only 290 remain"):
            exec(compile(body, "pressure-budget-preflight", "exec"), {})
        assert not (tmp_path / "queen-intent-budget.json").exists()
    else:
        exec(compile(body, "pressure-budget-preflight", "exec"), {})
        result = json.loads((tmp_path / "queen-intent-budget.json").read_text())
        assert result["required"] == 291
        assert result["manifest_sha256"] == hashlib.sha256(manifest.read_bytes()).hexdigest()
    assert reads == ["/proc/authority", "/proc/queen/dedupe", "closed"]


def test_service_fault_waits_for_armed_debugger_before_operator(tmp_path: Path) -> None:
    """A service fault cannot race an uninstalled breakpoint."""
    source = (ROOT / "scripts/m26e_qemu_pressure.sh").read_text()
    function = "drive_service_fault_plan() {" + source.split(
        "drive_service_fault_plan() {", 1,
    )[1].split("\n}\n", 1)[0] + "\n}\n"
    (tmp_path / "uart.live.log").write_text("")
    result = subprocess.run(
        ["bash", "-eu", "-c", function + '''
HARNESS_PYTHON=true GDB_BIN=fixture TARGET_SESSION=fixture
GENERATED_INVENTORY=fixture OUT_ROOT=fixture AUTH_OBSERVATION=fixture
sleep() { exit 42; }
wait_for_marker_count() { printf 'wait %s\n' "$2"; }
run_cohsh_command() { printf 'operator %s\n' "$2"; }
drive_service_fault_plan "$1" ninedoor-service during-call-standard fixture TEARDOWN 350
''', "service-fault-test", str(tmp_path)],
        check=True, capture_output=True, text=True, timeout=10,
    )
    assert result.stdout.splitlines() == [
        "wait M26E_GDB_SERVICE_ARMED service=ninedoor-service mode=during-call-standard result=ready",
        "operator ls /", "wait TEARDOWN",
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


def test_strict_fault_encoding_failure_cannot_execute_approval(tmp_path: Path) -> None:
    """A fault caller's expected-error branch must not disable encoding failure."""
    source = (ROOT / 'scripts/m26e_qemu_pressure.sh').read_text()
    function = 'run_cohsh_command() {' + source.split('run_cohsh_command() {', 1)[1].split('\n}\n', 1)[0] + '\n}\n'
    selected = json.loads((ROOT / 'configs/generated/root_task_resolved.json').read_text())
    selected['authority'].update(production=True, legacy_queen_ctl=False,
                                 writer_epoch_required=True, queen_dedupe_entries=512)
    manifest = tmp_path / 'selected.json'
    manifest.write_text(json.dumps(selected))
    fake = tmp_path / 'cohsh'
    fake.write_text('#!/bin/sh\nprintf invoked > "$CONTROL_CALLED"\n')
    fake.chmod(0o700)
    result = subprocess.run([
        'bash', '-eu', '-c', function + '''
GATEWAY_PID=
REPO_ROOT="$1"
HARNESS_PYTHON="$2"
HOST_TOOLS="$3"
M26E_CONSOLE_AUTH_TOKEN=fixture
export COH_PRESSURE_AUTHORITY_MANIFEST="$4"
export CONTROL_CALLED="$3/invoked"
if run_cohsh_command "$3" 'kill ../../worker1' 17 NONE; then exit 99; fi
''', 'strict-encoding', str(ROOT), sys.executable, str(tmp_path), str(manifest),
    ], capture_output=True, text=True, timeout=10)
    assert result.returncode == 0
    assert 'invalid-authority-identifier' in result.stderr
    assert not (tmp_path / 'invoked').exists()
