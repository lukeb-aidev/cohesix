#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify REST regression security and bounded Stage 04 service lifecycle.
# Copyright 2026 Lukas Bower

"""Focused security and lifecycle tests for the REST regression stage."""

from __future__ import annotations

import os
from pathlib import Path
import stat
import subprocess
import time


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "cohsh" / "REST_regression_batch.sh"
STAGE4_SCRIPT = (
    REPO_ROOT / "scripts" / "ci" / "test_plan_stage_04_rest_multiplexer.sh"
)
STAGE4_TIMEOUT_ENV_NAMES = (
    "TP_STAGE4_GATEWAY_CONTROL_RESPONSE_TIMEOUT_MS",
    "TP_STAGE4_GATEWAY_TELEMETRY_RESPONSE_TIMEOUT_MS",
    "TP_STAGE4_REST_CLIENT_TIMEOUT_MS",
    "HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS",
    "HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS",
    "COHSH_REST_RESPONSE_TIMEOUT_MS",
)
STAGE4_CONTEXT_ENV_NAMES = (
    "COHESIX_GATEWAY_URL",
    "COHSH_REST_RESPONSE_TIMEOUT_MS",
    "COHSH_REST_URL",
    "COH_REST_URL",
    "HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS",
    "HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS",
    "HIVE_GATEWAY_REQUEST_AUTH_TOKEN",
    "HIVE_GATEWAY_URL",
    "TP_STAGE4_FUSE_COH_BIN",
    "TP_STAGE4_FUSE_MOUNT_DIR",
    "TP_STAGE4_FUSE_MOUNT_LOG",
)


def write_executable(path: Path, body: str) -> None:
    """Write one executable fixture."""

    path.write_text(body, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def run_stage4_timeout_contract(
    *,
    external_gateway: bool,
    overrides: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run only the Stage 04 timeout resolver with a controlled environment."""

    source = STAGE4_SCRIPT.read_text(encoding="utf-8")
    helper_start = source.index(
        "readonly stage4_gateway_queue_wait_limit_ms=5000"
    )
    helper_end = source.index("stage4_process_tree() {")
    helpers = source[helper_start:helper_end]
    program = (
        "set -euo pipefail\n"
        "tp_log() { printf '%s\\n' \"$1\" >&2; }\n"
        f"{helpers}\n"
        f"stage4_resolve_timeout_contract "
        f"{'1' if external_gateway else '0'}\n"
        "printf '%s\\n' \""
        "${stage4_gateway_timeout_declaration}|"
        "${stage4_gateway_control_response_timeout_ms}|"
        "${stage4_gateway_telemetry_response_timeout_ms}|"
        "${stage4_rest_client_response_timeout_ms}|"
        "${COHSH_REST_RESPONSE_TIMEOUT_MS}\"\n"
    )
    environment = os.environ.copy()
    for name in STAGE4_TIMEOUT_ENV_NAMES:
        environment.pop(name, None)
    environment.update(overrides or {})
    return subprocess.run(
        ["bash", "--noprofile", "--norc", "-c", program],
        cwd=REPO_ROOT,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
    )


def test_rest_token_is_inherited_without_appearing_in_argv(
    tmp_path: Path,
) -> None:
    """The wrapper passes secrets via environment, never command arguments."""

    secret = "private-test-token"
    invocation_log = tmp_path / "invocations.log"
    fake_cohsh = tmp_path / "cohsh"
    write_executable(
        fake_cohsh,
        "#!/usr/bin/env bash\n"
        "# Author: Lukas Bower\n"
        "# Purpose: Capture REST wrapper arguments for a security test.\n"
        "# Copyright 2026 Lukas Bower\n"
        "set -euo pipefail\n"
        "test \"${HIVE_GATEWAY_REQUEST_AUTH_TOKEN:-}\" = \"$EXPECTED_TOKEN\"\n"
        "if [[ \" $* \" == *\" $EXPECTED_TOKEN \"* ]]; then exit 91; fi\n"
        "printf '%s\\n' \"$*\" >>\"$INVOCATION_LOG\"\n",
    )
    script_root = tmp_path / "scripts"
    script_root.mkdir()
    (script_root / "smoke.coh").write_text("ping\n", encoding="utf-8")
    environment = os.environ.copy()
    environment.update(
        {
            "COHESIX_GATEWAY_URL": "http://127.0.0.1:8080",
            "HIVE_GATEWAY_REQUEST_AUTH_TOKEN": secret,
            "COHSH_BIN": str(fake_cohsh),
            "COHSH_LOG_ROOT": str(tmp_path / "logs"),
            "COHSH_SCRIPT_ROOT": str(script_root),
            "COHSH_SCRIPT_LIST": "smoke.coh",
            "COHSH_PARALLELISM": "1",
            "EXPECTED_TOKEN": secret,
            "INVOCATION_LOG": str(invocation_log),
        }
    )

    subprocess.run(
        ["bash", str(SCRIPT)],
        cwd=REPO_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )
    invocations = invocation_log.read_text(encoding="utf-8")
    assert secret not in invocations
    assert "--rest-auth-token" not in invocations
    assert "--rest-url" not in invocations


def test_stage4_rejects_explicit_local_port_conflicts() -> None:
    """Explicit QEMU and gateway binds cannot silently share one TCP port."""

    source = STAGE4_SCRIPT.read_text(encoding="utf-8")
    assert 'qemu_tcp_port_explicit=1' in source
    assert 'gateway_bind_explicit=1' in source
    assert (
        '[[ "${qemu_tcp_port}" == "${gateway_bind_port}" ]]' in source
    )
    assert (
        '[[ "${qemu_tcp_port_explicit}" == "1" \\\n'
        '      && "${gateway_bind_explicit}" == "1" ]]' in source
    )
    assert (
        "local QEMU and hive-gateway ports must be distinct" in source
    )


def test_stage4_timeout_contract_uses_canonical_local_defaults() -> None:
    """Local Stage 04 composes 5s + max(120s, 120s) + 5s."""

    completed = run_stage4_timeout_contract(external_gateway=False)

    assert completed.returncode == 0, completed.stderr
    assert completed.stdout.strip() == (
        "local-configured|120000|120000|130000|130000"
    )


def test_stage4_timeout_contract_tracks_the_larger_broker_budget() -> None:
    """The REST client covers the larger configured gateway response budget."""

    completed = run_stage4_timeout_contract(
        external_gateway=False,
        overrides={
            "TP_STAGE4_GATEWAY_CONTROL_RESPONSE_TIMEOUT_MS": "120000",
            "TP_STAGE4_GATEWAY_TELEMETRY_RESPONSE_TIMEOUT_MS": "180000",
        },
    )

    assert completed.returncode == 0, completed.stderr
    assert completed.stdout.strip() == (
        "local-configured|120000|180000|190000|190000"
    )


def test_stage4_external_gateway_requires_explicit_broker_budgets() -> None:
    """An external gateway cannot inherit unverified local timeout defaults."""

    missing = run_stage4_timeout_contract(external_gateway=True)
    assert missing.returncode != 0
    assert "external gateway requires explicit" in missing.stderr

    declared = run_stage4_timeout_contract(
        external_gateway=True,
        overrides={
            "HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS": "120000",
            "HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS": "180000",
        },
    )
    assert declared.returncode == 0, declared.stderr
    assert declared.stdout.strip() == (
        "external-explicit|120000|180000|190000|190000"
    )


def test_stage4_rejects_client_timeout_below_composed_minimum() -> None:
    """An explicit cohsh response budget cannot clip a legal gateway wait."""

    completed = run_stage4_timeout_contract(
        external_gateway=False,
        overrides={"TP_STAGE4_REST_CLIENT_TIMEOUT_MS": "129999"},
    )

    assert completed.returncode != 0
    assert "must be within 130000..1210000ms" in completed.stderr


def test_stage4_rejects_conflicting_timeout_declarations() -> None:
    """Harness and product environment aliases cannot select two budgets."""

    completed = run_stage4_timeout_contract(
        external_gateway=False,
        overrides={
            "TP_STAGE4_GATEWAY_CONTROL_RESPONSE_TIMEOUT_MS": "120000",
            "HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS": "180000",
        },
    )

    assert completed.returncode != 0
    assert "conflicting gateway control broker response timeout" in (
        completed.stderr
    )


def test_stage4_wires_and_records_the_resolved_timeout_contract() -> None:
    """Readiness, batches, Python parity, and evidence share one deadline."""

    source = STAGE4_SCRIPT.read_text(encoding="utf-8")
    assert (
        '--broker-control-response-timeout-ms "'
        '${stage4_gateway_control_response_timeout_ms}"' in source
    )
    assert (
        '--broker-telemetry-response-timeout-ms "'
        '${stage4_gateway_telemetry_response_timeout_ms}"' in source
    )
    assert source.count(
        'COHSH_REST_RESPONSE_TIMEOUT_MS="'
        '${stage4_rest_client_response_timeout_ms}"'
    ) >= 4
    assert (
        'timeout_s=float(os.environ[\\"COHSH_REST_RESPONSE_TIMEOUT_MS\\"]) '
        "/ 1000.0" in source
    )
    for field in (
        "gateway_timeout_declaration",
        "gateway_broker_queue_wait_limit_ms",
        "gateway_broker_control_response_timeout_ms",
        "gateway_broker_telemetry_response_timeout_ms",
        "rest_response_delivery_grace_ms",
        "cohsh_rest_response_timeout_ms",
    ):
        assert f"printf '{field}=%s\\n'" in source


def test_stage4_restores_runner_owned_environment_before_final_context() -> None:
    """Runner-local endpoint and deadline exports cannot look like drift."""

    source = STAGE4_SCRIPT.read_text(encoding="utf-8")
    helper_start = source.index("readonly stage4_gateway_queue_wait_limit_ms=5000")
    helper_end = source.index("stage4_process_tree() {")
    helpers = source[helper_start:helper_end]
    program = (
        "set -euo pipefail\n"
        "tp_log() { printf '%s\\n' \"$1\" >&2; }\n"
        f"{helpers}\n"
        "stage4_resolve_timeout_contract 0\n"
        "export COHESIX_GATEWAY_URL=http://127.0.0.1:64120\n"
        "export COHSH_REST_URL=http://127.0.0.1:64120\n"
        "export COH_REST_URL=http://127.0.0.1:64120\n"
        "export HIVE_GATEWAY_URL=http://127.0.0.1:64120\n"
        "export HIVE_GATEWAY_REQUEST_AUTH_TOKEN=runner-secret\n"
        "export TP_STAGE4_FUSE_COH_BIN=/runner/coh\n"
        "stage4_restore_context_environment\n"
        "printf '%s\\n' \"${COHESIX_GATEWAY_URL}\"\n"
        "for name in COHSH_REST_RESPONSE_TIMEOUT_MS COHSH_REST_URL "
        "COH_REST_URL HIVE_GATEWAY_BROKER_CONTROL_RESPONSE_TIMEOUT_MS "
        "HIVE_GATEWAY_BROKER_TELEMETRY_RESPONSE_TIMEOUT_MS "
        "HIVE_GATEWAY_REQUEST_AUTH_TOKEN HIVE_GATEWAY_URL "
        "TP_STAGE4_FUSE_COH_BIN TP_STAGE4_FUSE_MOUNT_DIR "
        "TP_STAGE4_FUSE_MOUNT_LOG; do\n"
        "  [[ -z \"${!name+x}\" ]] || exit 91\n"
        "done\n"
    )
    environment = os.environ.copy()
    for name in STAGE4_CONTEXT_ENV_NAMES:
        environment.pop(name, None)
    environment["COHESIX_GATEWAY_URL"] = "https://inherited.example.test"
    completed = subprocess.run(
        ["bash", "--noprofile", "--norc", "-c", program],
        cwd=REPO_ROOT,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
    )

    assert completed.returncode == 0, completed.stderr
    assert completed.stdout.strip() == "https://inherited.example.test"
    assert (
        "stage4_restore_context_environment\n  tp_stage_exit_trap"
        in source
    )
    assert (
        "stage4_restore_context_environment\ntp_stage_complete 4" in source
    )


def test_stage4_term_ignoring_process_is_killed_within_bound(
    tmp_path: Path,
) -> None:
    """Stage 04 escalates from TERM to KILL instead of waiting forever."""

    source = STAGE4_SCRIPT.read_text(encoding="utf-8")
    helper_start = source.index("stage4_process_tree() {")
    helper_end = source.index("stage4_cleanup() {")
    cleanup_helpers = source[helper_start:helper_end]
    assert 'stage4_wait_processes "${processes}" 50' in cleanup_helpers
    assert 'stage4_wait_processes "${processes}" 20' in cleanup_helpers
    cleanup_helpers = cleanup_helpers.replace(
        'stage4_wait_processes "${processes}" 50',
        'stage4_wait_processes "${processes}" 2',
    ).replace(
        'stage4_wait_processes "${processes}" 20',
        'stage4_wait_processes "${processes}" 2',
    )
    program = (
        "set -euo pipefail\n"
        "tp_log() { :; }\n"
        f"{cleanup_helpers}\n"
        "ready_file=\"$1\"\n"
        "python3 - \"$ready_file\" <<'PY' &\n"
        "import pathlib\n"
        "import signal\n"
        "import sys\n"
        "import time\n"
        "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
        "pathlib.Path(sys.argv[1]).touch()\n"
        "time.sleep(30)\n"
        "PY\n"
        "child_pid=$!\n"
        "for ((i = 0; i < 50; i += 1)); do\n"
        "  [[ -e \"$ready_file\" ]] && break\n"
        "  sleep 0.02\n"
        "done\n"
        "[[ -e \"$ready_file\" ]]\n"
        "stage4_stop_process_tree \"$child_pid\" fixture\n"
        "! kill -0 \"$child_pid\" >/dev/null 2>&1\n"
    )
    ready_file = tmp_path / "stage4-cleanup-ready"
    started = time.monotonic()
    subprocess.run(
        [
            "bash",
            "--noprofile",
            "--norc",
            "-c",
            program,
            "bash",
            str(ready_file),
        ],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert time.monotonic() - started < 9


def test_stage4_fuse_cleanup_is_bounded() -> None:
    """Both unmount and mount-process teardown use finite TERM/KILL waits."""

    source = STAGE4_SCRIPT.read_text(encoding="utf-8")
    assert "bounded_unmount() {" in source
    assert 'wait_for_exit "${unmount_pid}" 50' in source
    assert 'kill -KILL "${unmount_pid}"' in source
    assert 'stop_mount_process() {' in source
    assert 'wait_for_exit "${coh_mount_pid}" 50' in source
    assert 'kill -KILL "${coh_mount_pid}"' in source
    assert 'wait "${coh_mount_pid}"' not in source


def test_stage4_rest_parallelism_obeys_the_host_budget() -> None:
    """The REST client fan-out cannot exceed the shared test-plan job cap."""

    source = STAGE4_SCRIPT.read_text(encoding="utf-8")

    assert "if ((TP_HOST_JOBS < core_parallelism)); then" in source
    assert 'COHSH_PARALLELISM="${core_parallelism}"' in source
    assert "tp_stage_exit_trap" in source


def test_stage4_waits_for_root_console_before_starting_gateway(
    tmp_path: Path,
) -> None:
    """The packaged gateway owns the first post-ready authenticated session."""

    source = STAGE4_SCRIPT.read_text(encoding="utf-8")
    helper_start = source.index("stage4_wait_log_marker() {")
    helper_end = source.index("stage4_wait_gateway_ready() {")
    helper = source[helper_start:helper_end]
    marker = "[mark] root-console.start.ok"
    ready_log = tmp_path / "ready.log"
    missing_log = tmp_path / "missing.log"
    ready_log.write_text(f"booting\n{marker}\n", encoding="utf-8")
    program = (
        "set -euo pipefail\n"
        f"{helper}\n"
        "stage4_wait_log_marker \"$1\" \"$2\" 1 \"$$\"\n"
        "true &\n"
        "dead_pid=$!\n"
        "wait \"${dead_pid}\"\n"
        "set +e\n"
        "stage4_wait_log_marker \"$3\" \"$2\" 1 \"${dead_pid}\"\n"
        "status=$?\n"
        "set -e\n"
        "[[ \"${status}\" -eq 1 ]]\n"
    )
    completed = subprocess.run(
        [
            "bash",
            "--noprofile",
            "--norc",
            "-c",
            program,
            "bash",
            str(ready_log),
            marker,
            str(missing_log),
        ],
        cwd=REPO_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert completed.returncode == 0, completed.stderr
    launch_index = source.index('"${artifact_helper}" launch \\')
    ready_index = source.index('stage4_wait_log_marker \\')
    gateway_index = source.index(
        '"${artifact_dir}/host-tools/hive-gateway" \\'
    )
    assert launch_index < ready_index < gateway_index
    assert "stage4_wait_port_ready" not in source
    assert "stage4_check_auth_ready" not in source
    assert "stage4_wait_auth_ready" not in source
    assert "TP_STAGE4_AUTH_READY_TIMEOUT" not in source


def test_stage4_script_has_valid_bash_syntax() -> None:
    """The composed FUSE heredoc and lifecycle helpers remain parseable."""

    subprocess.run(
        ["bash", "-n", str(STAGE4_SCRIPT)],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
