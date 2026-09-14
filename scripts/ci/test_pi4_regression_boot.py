# Author: Lukas Bower
# Purpose: Reject stale or substituted Pi group identities and bound collector lifetime.
# Copyright 2026 Lukas Bower
"""Pure evidence-binding and controlled subprocess contracts, not Pi proof."""

from pathlib import Path
import json
import os
import subprocess
import sys

import pytest

from pi4_regression_boot import collector_digest, run_collector, verify_fresh_boot


def evidence(boot: str) -> dict[str, str]:
    return {"schema": "cohesix.test-plan.target-evidence.v1",
            "claim_tier": "pi4-transport",
            "target": "pi4", "source_digest": "sha256:" + "a" * 64,
            "image_identity": "sha256:" + "b" * 64, "target_host": "192.0.2.2",
            "gateway_url": "http://127.0.0.1:31338", "boot_id": boot}


def test_fresh_boot_preserves_every_binding() -> None:
    verify_fresh_boot(evidence("boot-a"), evidence("boot-b"))
    with pytest.raises(ValueError, match="reused"):
        verify_fresh_boot(evidence("boot-a"), evidence("boot-a"))


@pytest.mark.parametrize("field", ["schema", "claim_tier", "target", "source_digest",
    "image_identity", "target_host", "gateway_url", "gateway_target_host"])
def test_any_changed_binding_is_rejected(field: str) -> None:
    current = evidence("boot-b")
    current[field] = "substituted"
    with pytest.raises(ValueError, match=field):
        verify_fresh_boot(evidence("boot-a"), current)


def test_collector_bounds_and_exact_bytes(tmp_path: Path) -> None:
    path = tmp_path / "collector"
    path.write_bytes(b"abc")
    assert collector_digest(path) == (
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    )
    path.write_bytes(b"")
    with pytest.raises(ValueError, match="bytes"):
        collector_digest(path)
    with path.open("wb") as stream:
        stream.truncate(4 * 1024 * 1024 + 1)
    with pytest.raises(ValueError, match="bytes"):
        collector_digest(path)


def test_collector_failure_and_timeout_preserve_logs(tmp_path: Path) -> None:
    failed = tmp_path / "failed.log"
    assert run_collector(
        [sys.executable, "-c", "print('failed boot'); raise SystemExit(7)"], failed,
    ) == 7
    assert failed.read_text() == "failed boot\n"
    timed = tmp_path / "timed.log"
    # The zero injected budget tests cancellation, not an uncontrolled sleep.
    assert run_collector(
        [sys.executable, "-c", "import signal; signal.pause()"], timed, timeout=0,
    ) == 124
    assert timed.exists()


@pytest.mark.parametrize("mode", ["valid", "missing", "reused"])
def test_group_coordinator_keeps_distinct_boots_and_finishes_on_base(
    tmp_path: Path, mode: str,
) -> None:
    """Exercise shell orchestration with a receipt fixture, never target models."""
    root = Path(__file__).resolve().parents[2]
    source = (root / "scripts/cohsh/run_regression_batch.sh").read_text()
    coordinator = "run_pi4_batch() {" + source.split(
        "run_pi4_batch() {", 1,
    )[1].split("\nqemu_pid=0", 1)[0]
    initial = tmp_path / "initial.json"
    initial.write_text(json.dumps(evidence("initial")))
    collector = tmp_path / "collector"
    collector.write_text(
        f"#!{sys.executable}\n"
        "import json, pathlib, sys\n"
        "args = dict(zip(sys.argv[1::2], sys.argv[2::2]))\n"
        "prior = json.loads(pathlib.Path(args['--prior-evidence']).read_text())\n"
        "prior['boot_id'] = 'fixture-' + args['--group']\n"
        f"if {mode!r} == 'reused' and args['--group'] == 'base':\n"
        "    prior['boot_id'] = 'fixture-base-telemetry'\n"
        "out = pathlib.Path(args['--out']) / 'target-evidence.json'\n"
        "out.write_text(json.dumps(prior))\n"
    )
    collector.chmod(0o700)
    program = """set -euo pipefail
PROJECT_ROOT="$1"
ARCHIVE_ROOT="$2/batch"
TRANSPORT_RESULT_ROOT="$2/results"
TARGET_EVIDENCE_FILE="$2/initial.json"
COHSH_PI4_BOOT_COLLECTOR="$2/collector"
TEST_SOURCE_DIGEST="sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
QEMU_ARTIFACT_HELPER="$PROJECT_ROOT/scripts/ci/qemu_artifact.py"
TCP_HOST=192.0.2.2
TCP_PORT=31337
PORT_TIMEOUT=1
AUTH_READY_TIMEOUT=1
COHSH_AUTH_TOKEN=fixture
BASE_SCRIPTS=(a b c d e f g h i j)
BASE_TELEMETRY_SCRIPTS=(a b)
BASE_SHARD_SCRIPTS=(a)
GATED_SCRIPTS=(a b c d)
group_selected() { return 0; }
reset_scoped_directory() { mkdir -p "$1"; }
write_lifecycle_resume_script() { :; }
write_lifecycle_touch_script() { :; }
ensure_live_cohsh_bin() { :; }
wait_port_ready() { :; }
wait_auth_ready() { :; }
run_live_group() {
    shift
    pi_pass=$((pi_pass + $#))
    pi_total=$((pi_total + $#))
}
write_pi4_result() {
    printf '%s %s\\n' "$1" "$TARGET_EVIDENCE_FILE" >>"${ARCHIVE_ROOT}/coordinator-results"
}
write_transport_aggregate() { mkdir -p "$TRANSPORT_RESULT_ROOT"; }
"""
    # The result stub records orchestration only; qemu_artifact's separate
    # tests own result hashing and target-evidence verification.
    if mode == "missing":
        program = program.replace('COHSH_PI4_BOOT_COLLECTOR="$2/collector"',
                                  'unset COHSH_PI4_BOOT_COLLECTOR')
    completed = subprocess.run(
        ["bash", "-c", program + coordinator + "\nrun_pi4_batch\n",
         "fixture", str(root), str(tmp_path)],
        env=os.environ.copy(), capture_output=True, text=True, check=False,
    )
    if mode != "valid":
        assert completed.returncode != 0
        assert not (tmp_path / "results/final-target-evidence.json").exists()
        if mode == "missing":
            assert "requires COHSH_PI4_BOOT_COLLECTOR" in completed.stderr
            assert not (tmp_path / "batch").exists()
        else:
            assert "reused" in completed.stderr
        return
    assert completed.returncode == 0, completed.stdout + completed.stderr
    rows = (tmp_path / "batch/coordinator-results").read_text().splitlines()
    groups = []
    boot_ids = []
    for row in rows:
        group, path = row.split(" ", 1)
        groups.append(group)
        boot_ids.append(json.loads(Path(path).read_text())["boot_id"])
    assert groups == ["base-telemetry", "base-shard", "gated", "base"]
    assert len(set(boot_ids)) == 4
    final = json.loads((tmp_path / "results/final-target-evidence.json").read_text())
    assert final["boot_id"] == "fixture-base"
    assert "RESULT pass=17 fail=0 total=17" in completed.stdout
