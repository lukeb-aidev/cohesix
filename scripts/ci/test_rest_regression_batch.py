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


def write_executable(path: Path, body: str) -> None:
    """Write one executable fixture."""

    path.write_text(body, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


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


def test_stage4_script_has_valid_bash_syntax() -> None:
    """The composed FUSE heredoc and lifecycle helpers remain parseable."""

    subprocess.run(
        ["bash", "-n", str(STAGE4_SCRIPT)],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
