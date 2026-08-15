#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Test target-batch routing and crash-safe generated-output transactions.
# Copyright 2026 Lukas Bower

"""Focused wrapper tests for scripts/cohsh/run_regression_batch.sh."""

from __future__ import annotations

import os
from pathlib import Path
import select
import socket
import stat
import subprocess
import sys
import threading


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "cohsh" / "run_regression_batch.sh"


def write_executable(path: Path, body: str) -> None:
    """Write one executable test fixture."""

    path.write_text(body, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def auth_probe_source() -> str:
    """Extract the wrapper's embedded authentication readiness probe."""

    source = SCRIPT.read_text(encoding="utf-8")
    function = source.split("check_auth_ready() {", maxsplit=1)[1]
    return function.split("<<'PY'\n", maxsplit=1)[1].split("\nPY\n}", maxsplit=1)[0]


def run_auth_probe(payload: bytes, *, declared_extra: int = 0) -> int:
    """Run the production probe against one framed loopback response."""

    ready = threading.Event()
    port: list[int] = []

    def serve_once() -> None:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            listener.bind(("127.0.0.1", 0))
            listener.listen(1)
            port.append(listener.getsockname()[1])
            ready.set()
            connection, _ = listener.accept()
            with connection:
                connection.recv(4096)
                total = len(payload) + 4 + declared_extra
                connection.sendall(total.to_bytes(4, "little") + payload)

    server = threading.Thread(target=serve_once, daemon=True)
    server.start()
    assert ready.wait(timeout=1), "loopback auth fixture did not start"
    result = subprocess.run(
        [sys.executable, "-", "127.0.0.1", str(port[0]), "fixture-token"],
        input=auth_probe_source(),
        text=True,
        check=False,
        capture_output=True,
    )
    server.join(timeout=1)
    assert not server.is_alive(), "loopback auth fixture did not finish"
    return result.returncode


def run_path_admission(
    *,
    archive: str,
    artifact: str | None = None,
    result_root: str | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run the wrapper's non-mutating path-admission mode."""

    environment = os.environ.copy()
    environment.update(
        {
            "COHSH_BATCH_PRINT_PATHS": "1",
            "COHSH_LOG_ROOT": archive,
            "TEST_PLAN_SOURCE_DIGEST": "sha256:" + ("a" * 64),
        }
    )
    if artifact is not None:
        environment["COHSH_QEMU_ARTIFACT_ROOT"] = artifact
    if result_root is not None:
        environment["COHSH_TRANSPORT_RESULT_ROOT"] = result_root
    return subprocess.run(
        ["bash", str(SCRIPT)],
        cwd=REPO_ROOT,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
    )


def generated_restore_source() -> str:
    """Extract the production generated-output transaction helpers."""

    source = SCRIPT.read_text(encoding="utf-8")
    return "acquire_generated_output_lock() {" + source.split(
        "acquire_generated_output_lock() {", maxsplit=1
    )[1].split("group_selected() {", maxsplit=1)[0]


def generated_restore_fixture_source(commands: str) -> str:
    """Build one isolated shell program around production transaction helpers."""

    return (
        "set -euo pipefail\n"
        'PROJECT_ROOT="$1"\n'
        'GENERATED_OUTPUT_LOCK_FILE="$PROJECT_ROOT/out/.cohesix-locks/'
        'generated-outputs.lock"\n'
        "GENERATED_OUTPUT_PATHS=(\n"
        '  "apps/root-task/src/generated"\n'
        '  "configs/generated/existing.json"\n'
        '  "configs/generated/initially-missing.json"\n'
        ")\n"
        'generated_snapshot_dir=""\n'
        'generated_snapshot_parent=""\n'
        "generated_snapshot_ready=0\n"
        "generated_output_lock_held=0\n"
        'generated_preserved_restore_work_dirs=("")\n'
        f"{generated_restore_source()}\n"
        f"{commands}\n"
    )


def generated_restore_fixture_environment(
    root: Path,
    extra: dict[str, str] | None = None,
) -> dict[str, str]:
    """Return a bounded, fixture-local environment for restore helpers."""

    temp_root = root.parent / "fixture-tmp"
    temp_root.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment.update(
        {
            "TMPDIR": str(temp_root),
            "COHSH_GENERATED_LOCK_TIMEOUT": "2",
        }
    )
    if extra is not None:
        environment.update(extra)
    return environment


def run_generated_restore_fixture(
    root: Path,
    commands: str,
    *,
    extra_environment: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    """Exercise production restore helpers against an isolated repository."""

    return subprocess.run(
        [
            "bash",
            "-c",
            generated_restore_fixture_source(commands),
            "restore-fixture",
            str(root),
        ],
        env=generated_restore_fixture_environment(root, extra_environment),
        check=False,
        capture_output=True,
        text=True,
    )


def assert_no_generated_transaction_temps(root: Path) -> None:
    """Assert that a completed fixture left no snapshot or restore work dirs."""

    assert not list((root.parent / "fixture-tmp").glob("cohesix-generated.*"))
    assert not list(root.rglob(".cohesix-restore.*"))


def test_relative_log_root_is_canonicalized_before_reset() -> None:
    """A relative caller path becomes an absolute repository-scoped path."""

    relative = "out/test-plan/path-admission"
    environment = os.environ.copy()
    environment.update(
        {
            "COHSH_BATCH_PRINT_PATHS": "1",
            "COHSH_LOG_ROOT": relative,
            "TEST_PLAN_SOURCE_DIGEST": "sha256:" + ("a" * 64),
        }
    )
    result = subprocess.run(
        ["bash", str(SCRIPT)],
        cwd=REPO_ROOT,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )
    values = dict(
        line.split("=", maxsplit=1)
        for line in result.stdout.splitlines()
        if "=" in line
    )
    archive_root = (REPO_ROOT / relative).resolve()
    assert values["ARCHIVE_ROOT"] == str(archive_root)
    assert values["QEMU_ARTIFACT_ROOT"] == str(
        archive_root / "qemu-artifacts"
    )
    assert values["TRANSPORT_RESULT_ROOT"] == str(
        archive_root / "transport-results"
    )


def test_repository_root_is_rejected_before_any_reset() -> None:
    """A broad relative log root cannot turn cleanup into repository deletion."""

    sentinel = REPO_ROOT / "AGENTS.md"
    before = sentinel.read_bytes()
    result = run_path_admission(archive=".")

    assert result.returncode != 0
    assert "unsafe archive root" in result.stderr
    assert sentinel.read_bytes() == before


def test_artifact_and_result_roots_cannot_alias_or_overlap(
    tmp_path: Path,
) -> None:
    """Independent output classes cannot delete or overwrite one another."""

    archive = tmp_path / "archive"
    aliased = run_path_admission(
        archive=str(archive),
        artifact=str(archive),
        result_root=str(tmp_path / "results"),
    )
    assert aliased.returncode != 0
    assert "must not alias the archive root" in aliased.stderr

    artifact = tmp_path / "artifacts"
    overlapping = run_path_admission(
        archive=str(archive),
        artifact=str(artifact),
        result_root=str(artifact / "results"),
    )
    assert overlapping.returncode != 0
    assert "must not alias or overlap" in overlapping.stderr


def test_generated_directory_restore_stages_before_replacing_live_path(
    tmp_path: Path,
) -> None:
    """A failed staged replacement rolls the live generated directory back."""

    production_paths = SCRIPT.read_text(encoding="utf-8").split(
        "GENERATED_OUTPUT_PATHS=(", maxsplit=1
    )[1].split("\n)", maxsplit=1)[0]
    assert '"apps/root-task/src/generated"' in production_paths

    root = tmp_path / "repo"
    generated = root / "apps" / "root-task" / "src" / "generated"
    generated.mkdir(parents=True)
    (generated / "bootstrap.rs").write_text(
        "snapshot bootstrap\n", encoding="utf-8"
    )
    (generated / "mod.rs").write_text("snapshot mod\n", encoding="utf-8")
    existing = root / "configs" / "generated" / "existing.json"
    existing.parent.mkdir(parents=True)
    existing.write_text("snapshot file\n", encoding="utf-8")

    result = run_generated_restore_fixture(
        root,
        "snapshot_generated_outputs\n"
        'printf "live bootstrap\\n" '
        '>"$PROJECT_ROOT/apps/root-task/src/generated/bootstrap.rs"\n'
        'printf "live mod\\n" >"$PROJECT_ROOT/apps/root-task/src/generated/mod.rs"\n'
        "mv() {\n"
        '  if [[ "$1" == */replacement '
        '&& "$2" == "$PROJECT_ROOT/apps/root-task/src/generated" ]]; then\n'
        "    return 73\n"
        "  fi\n"
        '  command mv "$@"\n'
        "}\n"
        "if restore_generated_outputs; then exit 90; fi\n"
        'test "$(cat "$PROJECT_ROOT/apps/root-task/src/generated/bootstrap.rs")" '
        '= "live bootstrap"\n'
        'test "$(cat "$PROJECT_ROOT/apps/root-task/src/generated/mod.rs")" '
        '= "live mod"\n'
        "unset -f mv\n"
        "restore_generated_outputs\n",
    )

    assert result.returncode == 0, result.stderr
    assert (generated / "bootstrap.rs").read_text(encoding="utf-8") == (
        "snapshot bootstrap\n"
    )
    assert (generated / "mod.rs").read_text(encoding="utf-8") == "snapshot mod\n"
    assert_no_generated_transaction_temps(root)


def test_generated_restore_replaces_directory_and_file_from_snapshot(
    tmp_path: Path,
) -> None:
    """Directory, file, and initially absent outputs restore exactly."""

    root = tmp_path / "repo"
    generated = root / "apps" / "root-task" / "src" / "generated"
    generated.mkdir(parents=True)
    (generated / "bootstrap.rs").write_text(
        "snapshot bootstrap\n", encoding="utf-8"
    )
    (generated / "mod.rs").write_text("snapshot mod\n", encoding="utf-8")
    existing = root / "configs" / "generated" / "existing.json"
    existing.parent.mkdir(parents=True)
    existing.write_text("snapshot file\n", encoding="utf-8")

    result = run_generated_restore_fixture(
        root,
        "snapshot_generated_outputs\n"
        'printf "changed\\n" '
        '>"$PROJECT_ROOT/apps/root-task/src/generated/bootstrap.rs"\n'
        'rm "$PROJECT_ROOT/apps/root-task/src/generated/mod.rs"\n'
        'printf "stale\\n" >"$PROJECT_ROOT/apps/root-task/src/generated/stale.rs"\n'
        'printf "changed file\\n" '
        '>"$PROJECT_ROOT/configs/generated/existing.json"\n'
        'printf "new file\\n" '
        '>"$PROJECT_ROOT/configs/generated/initially-missing.json"\n'
        "restore_generated_outputs\n",
    )

    assert result.returncode == 0, result.stderr
    assert sorted(path.name for path in generated.iterdir()) == [
        "bootstrap.rs",
        "mod.rs",
    ]
    assert (generated / "bootstrap.rs").read_text(encoding="utf-8") == (
        "snapshot bootstrap\n"
    )
    assert (generated / "mod.rs").read_text(encoding="utf-8") == (
        "snapshot mod\n"
    )
    assert existing.read_text(encoding="utf-8") == "snapshot file\n"
    assert not (existing.parent / "initially-missing.json").exists()
    assert_no_generated_transaction_temps(root)


def test_generated_restore_validates_all_metadata_before_live_mutation(
    tmp_path: Path,
) -> None:
    """An invalid partition cannot partially restore any generated output."""

    root = tmp_path / "repo"
    generated = root / "apps" / "root-task" / "src" / "generated"
    generated.mkdir(parents=True)
    (generated / "bootstrap.rs").write_text("snapshot bootstrap\n", encoding="utf-8")
    existing = root / "configs" / "generated" / "existing.json"
    existing.parent.mkdir(parents=True)
    existing.write_text("snapshot file\n", encoding="utf-8")
    initially_missing = existing.parent / "initially-missing.json"

    result = run_generated_restore_fixture(
        root,
        "snapshot_generated_outputs\n"
        'printf "live bootstrap\\n" '
        '>"$PROJECT_ROOT/apps/root-task/src/generated/bootstrap.rs"\n'
        'printf "live file\\n" >"$PROJECT_ROOT/configs/generated/existing.json"\n'
        'printf "new file\\n" '
        '>"$PROJECT_ROOT/configs/generated/initially-missing.json"\n'
        'printf "outside/generated\\n" >>"$generated_snapshot_dir/present"\n'
        "if restore_generated_outputs; then exit 90; fi\n"
        'test "$(cat "$PROJECT_ROOT/apps/root-task/src/generated/bootstrap.rs")" '
        '= "live bootstrap"\n'
        'test "$(cat "$PROJECT_ROOT/configs/generated/existing.json")" '
        '= "live file"\n'
        'test "$(cat "$PROJECT_ROOT/configs/generated/initially-missing.json")" '
        '= "new file"\n'
        "awk '$0 != \"outside/generated\"' \"$generated_snapshot_dir/present\" "
        '>"$generated_snapshot_dir/present.fixed"\n'
        'command mv "$generated_snapshot_dir/present.fixed" '
        '"$generated_snapshot_dir/present"\n'
        "restore_generated_outputs\n",
    )

    assert result.returncode == 0, result.stderr
    assert "do not exactly partition outputs" in result.stderr
    assert (generated / "bootstrap.rs").read_text(encoding="utf-8") == (
        "snapshot bootstrap\n"
    )
    assert existing.read_text(encoding="utf-8") == "snapshot file\n"
    assert not initially_missing.exists()
    assert_no_generated_transaction_temps(root)


def test_exit_cleanup_restores_exact_generated_tree_without_temp_leaks(
    tmp_path: Path,
) -> None:
    """EXIT cleanup restores an interrupted transaction and preserves status."""

    root = tmp_path / "repo"
    generated = root / "apps" / "root-task" / "src" / "generated"
    generated.mkdir(parents=True)
    (generated / "bootstrap.rs").write_text("snapshot bootstrap\n", encoding="utf-8")
    existing = root / "configs" / "generated" / "existing.json"
    existing.parent.mkdir(parents=True)
    existing.write_text("snapshot file\n", encoding="utf-8")

    result = run_generated_restore_fixture(
        root,
        "fixture_cleanup() {\n"
        "  local status=$?\n"
        "  local cleanup_status=0\n"
        "  trap '' HUP INT TERM\n"
        "  set +e\n"
        "  kill -TERM $$\n"
        "  restore_generated_outputs || cleanup_status=1\n"
        "  release_generated_output_lock || cleanup_status=1\n"
        "  trap - EXIT\n"
        "  if (( cleanup_status != 0 )); then status=1; fi\n"
        '  exit "$status"\n'
        "}\n"
        "trap fixture_cleanup EXIT\n"
        "snapshot_generated_outputs\n"
        'printf "interrupted\\n" '
        '>"$PROJECT_ROOT/apps/root-task/src/generated/bootstrap.rs"\n'
        'printf "new file\\n" '
        '>"$PROJECT_ROOT/configs/generated/initially-missing.json"\n'
        "exit 23\n",
    )

    assert result.returncode == 23, result.stderr
    assert (generated / "bootstrap.rs").read_text(encoding="utf-8") == (
        "snapshot bootstrap\n"
    )
    assert existing.read_text(encoding="utf-8") == "snapshot file\n"
    assert not (existing.parent / "initially-missing.json").exists()
    assert_no_generated_transaction_temps(root)


def test_production_cleanup_blocks_repeated_signals_before_restore() -> None:
    """A second termination signal cannot interrupt the EXIT transaction."""

    source = SCRIPT.read_text(encoding="utf-8")
    cleanup = source.split("cleanup() {", maxsplit=1)[1].split(
        "terminate_batch() {", maxsplit=1
    )[0]
    ignore = cleanup.find("trap '' HUP INT TERM")
    restore = cleanup.find("restore_generated_outputs")
    assert 0 <= ignore < restore

    terminate = source.split("terminate_batch() {", maxsplit=1)[1].split(
        "trap cleanup EXIT", maxsplit=1
    )[0]
    ignore = terminate.find("trap '' HUP INT TERM")
    exit_call = terminate.find('exit "$status"')
    assert 0 <= ignore < exit_call
    assert "trap - HUP INT TERM" not in terminate


def test_snapshot_disposal_failure_is_inactive_before_exit_reentry(
    tmp_path: Path,
) -> None:
    """A disposal error cannot make EXIT consume partial snapshot metadata."""

    root = tmp_path / "repo"
    generated = root / "apps" / "root-task" / "src" / "generated"
    generated.mkdir(parents=True)
    (generated / "bootstrap.rs").write_text("snapshot bootstrap\n", encoding="utf-8")
    existing = root / "configs" / "generated" / "existing.json"
    existing.parent.mkdir(parents=True)
    existing.write_text("snapshot file\n", encoding="utf-8")

    result = run_generated_restore_fixture(
        root,
        "fixture_cleanup() {\n"
        "  local status=$?\n"
        "  set +e\n"
        "  restore_generated_outputs || status=1\n"
        "  release_generated_output_lock || status=1\n"
        "  trap - EXIT\n"
        '  exit "$status"\n'
        "}\n"
        "trap fixture_cleanup EXIT\n"
        "snapshot_generated_outputs\n"
        'printf "changed\\n" '
        '>"$PROJECT_ROOT/apps/root-task/src/generated/bootstrap.rs"\n'
        "discard_generated_snapshot_dir() {\n"
        '  test "$generated_snapshot_ready" = 0\n'
        '  test -z "$generated_snapshot_dir"\n'
        '  test -z "$generated_snapshot_parent"\n'
        '  command find "$1" -mindepth 1 -delete\n'
        '  command rmdir "$1"\n'
        "  return 71\n"
        "}\n"
        "if restore_generated_outputs; then exit 90; fi\n"
        'test "$(cat "$PROJECT_ROOT/apps/root-task/src/generated/bootstrap.rs")" '
        '= "snapshot bootstrap"\n'
        "exit 23\n",
    )

    assert result.returncode == 23, result.stderr
    assert "failed to discard snapshot" in result.stderr
    assert (generated / "bootstrap.rs").read_text(encoding="utf-8") == (
        "snapshot bootstrap\n"
    )
    assert_no_generated_transaction_temps(root)


def test_generated_restore_preserves_previous_on_double_move_failure(
    tmp_path: Path,
) -> None:
    """A replacement plus rollback failure retains the sole previous tree."""

    root = tmp_path / "repo"
    generated = root / "apps" / "root-task" / "src" / "generated"
    generated.mkdir(parents=True)
    (generated / "bootstrap.rs").write_text("snapshot bootstrap\n", encoding="utf-8")
    existing = root / "configs" / "generated" / "existing.json"
    existing.parent.mkdir(parents=True)
    existing.write_text("snapshot file\n", encoding="utf-8")

    result = run_generated_restore_fixture(
        root,
        "snapshot_generated_outputs\n"
        'printf "live bootstrap\\n" '
        '>"$PROJECT_ROOT/apps/root-task/src/generated/bootstrap.rs"\n'
        "mv() {\n"
        '  if [[ "$1" == */replacement '
        '&& "$2" == "$PROJECT_ROOT/apps/root-task/src/generated" ]]; then\n'
        "    return 73\n"
        "  fi\n"
        '  if [[ "$1" == */previous '
        '&& "$2" == "$PROJECT_ROOT/apps/root-task/src/generated" ]]; then\n'
        "    return 74\n"
        "  fi\n"
        '  command mv "$@"\n'
        "}\n"
        "if restore_generated_outputs; then exit 90; fi\n"
        'test ! -e "$PROJECT_ROOT/apps/root-task/src/generated"\n'
        'preserved="$(find "$PROJECT_ROOT/apps/root-task/src" -maxdepth 1 '
        "-type d -name '.cohesix-restore.generated.*' -print -quit)\"\n"
        'test -n "$preserved"\n'
        'test "$(cat "$preserved/previous/bootstrap.rs")" = "live bootstrap"\n'
        'test "$generated_snapshot_ready" = 1\n'
        "unset -f mv\n"
        "restore_generated_outputs\n"
        'test "$(cat "$PROJECT_ROOT/apps/root-task/src/generated/bootstrap.rs")" '
        '= "snapshot bootstrap"\n'
        'test ! -e "$preserved"\n',
    )

    assert result.returncode == 0, result.stderr
    assert "previous retained at" in result.stderr
    assert (generated / "bootstrap.rs").read_text(encoding="utf-8") == (
        "snapshot bootstrap\n"
    )
    assert_no_generated_transaction_temps(root)


def test_generated_output_lock_excludes_concurrent_owner_and_ignores_stale_text(
    tmp_path: Path,
) -> None:
    """The kernel lock excludes a live owner but never mistakes stale text for one."""

    root = tmp_path / "repo"
    root.mkdir(parents=True)
    owner = subprocess.Popen(
        [
            "bash",
            "-c",
            generated_restore_fixture_source(
                "snapshot_generated_outputs\n"
                "printf 'LOCKED\\n'\n"
                "IFS= read -r release\n"
                "restore_generated_outputs\n"
            ),
            "restore-lock-owner",
            str(root),
        ],
        env=generated_restore_fixture_environment(root),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    assert owner.stdout is not None
    ready, _, _ = select.select([owner.stdout], [], [], 2)
    assert ready, "generated-output lock owner did not become ready"
    assert owner.stdout.readline() == "LOCKED\n"

    contender = run_generated_restore_fixture(
        root,
        "if snapshot_generated_outputs; then exit 90; fi\n"
        'test "$generated_output_lock_held" = 0\n',
        extra_environment={"COHSH_GENERATED_LOCK_TIMEOUT": "0"},
    )
    assert contender.returncode == 0, contender.stderr
    assert "generated-output lock is busy: pid=" in contender.stderr

    assert owner.stdin is not None
    owner.stdin.write("release\n")
    owner.stdin.flush()
    owner_stdout, owner_stderr = owner.communicate(timeout=2)
    assert owner.returncode == 0, owner_stderr
    assert owner_stdout == ""

    successor = run_generated_restore_fixture(
        root,
        "snapshot_generated_outputs\nrestore_generated_outputs\n",
        extra_environment={"COHSH_GENERATED_LOCK_TIMEOUT": "0"},
    )
    assert successor.returncode == 0, successor.stderr
    assert_no_generated_transaction_temps(root)


def test_prepare_only_builds_shared_base_manifest_once_and_restores_generated(
    tmp_path: Path,
) -> None:
    """Three fresh-boot base groups share one prepared immutable artifact."""

    fake_build = tmp_path / "fake-build-run"
    fake_qemu = tmp_path / "qemu-system-aarch64"
    count_file = tmp_path / "build-count"
    write_executable(
        fake_qemu,
        "#!/usr/bin/env bash\n"
        "# Author: Lukas Bower\n"
        "# Purpose: Provide deterministic QEMU identity and accelerator probes.\n"
        "# Copyright 2026 Lukas Bower\n"
        "set -euo pipefail\n"
        "case \"${1:-}\" in\n"
        "  --version) printf 'QEMU emulator version fixture\\n' ;;\n"
        "  -accel)\n"
        "    test \"${2:-}\" = 'help'\n"
        "    printf 'Accelerators supported in QEMU binary:\\ntcg\\n'\n"
        "    ;;\n"
        "  *) exit 2 ;;\n"
        "esac\n",
    )
    write_executable(
        fake_build,
        "#!/usr/bin/env bash\n"
        "# Author: Lukas Bower\n"
        "# Purpose: Create a minimal QEMU artifact fixture for wrapper tests.\n"
        "# Copyright 2026 Lukas Bower\n"
        "set -euo pipefail\n"
        "out_dir=''\n"
        "while [[ $# -gt 0 ]]; do\n"
        "  if [[ \"$1\" == '--out-dir' ]]; then out_dir=\"$2\"; shift 2; else shift; fi\n"
        "done\n"
        "test -n \"$out_dir\"\n"
        "printf 'build\\n' >>\"$FAKE_BUILD_COUNT_FILE\"\n"
        "mkdir -p \"$out_dir/staging\" \"$out_dir/host-tools\"\n"
        "for path in staging/elfloader staging/kernel.elf staging/rootserver "
        "cohesix-system.cpio host-tools/cohsh host-tools/hive-gateway "
        "host-tools/coh host-tools/cas-tool host-tools/gpu-bridge-host "
        "host-tools/host-sidecar-bridge host-tools/host-ticket-agent "
        "host-tools/swarmui; do\n"
        "  printf 'fixture:%s\\n' \"$path\" >\"$out_dir/$path\"\n"
        "done\n"
        "mkdir -p configs/generated\n"
        "printf '{\"fixture\":true}\\n' >configs/generated/root_task_resolved.json\n"
        "printf '{\"topology_fixture\":true}\\n' >configs/generated/root_task_topology.json\n"
        "printf '{\"qemu_fixture\":true}\\n' "
        ">configs/generated/cohesix_python_qemu_smp_production.json\n"
        "printf '{\"pi_fixture\":true}\\n' "
        ">configs/generated/cohesix_python_pi4_production.json\n"
        "printf '{\"surface_fixture\":true}\\n' "
        ">configs/generated/implementation_surface_inventory.json\n"
        "printf '{\"host_fixture\":true}\\n' "
        ">configs/generated/host_integration_dependency.json\n"
        "mkdir -p docs/snippets\n"
        "printf 'host fixture\\n' >docs/snippets/host_integration_dependency.md\n"
        "printf 'fixture = true\\n' >configs/generated/cohsh_policy.toml\n"
        "python3 \"$FAKE_LAUNCH_ARTIFACT_TOOL\" write \\\n"
        "  --out-dir \"$out_dir\" \\\n"
        "  --sel4-build \"$SEL4_BUILD_DIR\" \\\n"
        "  --profile release \\\n"
        "  --cargo-target aarch64-unknown-none \\\n"
        "  --root-task-features cohesix-dev \\\n"
        "  --gic-version 3 \\\n"
        "  --sel4-profile \"$COHESIX_SEL4_PROFILE\" \\\n"
        "  --qemu \"$QEMU_BIN\" \\\n"
        "  --accelerator \"$COHESIX_QEMU_ACCEL\" \\\n"
        "  --virtualization \"$COHESIX_QEMU_VIRT\" \\\n"
        "  --machine-extra \"$COHESIX_QEMU_MACHINE_EXTRA\" \\\n"
        "  --cpu cortex-a57 \\\n"
        "  --smp \"$COHESIX_QEMU_SMP_TOPO\" \\\n"
        "  --net-backend virtio >/dev/null\n",
    )
    sel4 = tmp_path / "sel4"
    config = sel4 / "kernel" / "gen_config" / "kernel_config.h"
    config.parent.mkdir(parents=True)
    config.write_text("#define CONFIG_ARM_GIC_V3 1\n", encoding="utf-8")
    timer_header = sel4 / "kernel" / "gen_headers" / "plat" / "platform_gen.h"
    timer_header.parent.mkdir(parents=True)
    timer_header.write_text("#define TIMER_CLOCK_HZ 24000000\n", encoding="utf-8")
    transport_root = tmp_path / "transport"
    archive = transport_root / "logs"
    artifact_root = transport_root / "artifacts"

    generated = REPO_ROOT / "configs" / "generated" / "root_task_resolved.json"
    generated_before = generated.read_bytes()
    topology = REPO_ROOT / "configs" / "generated" / "root_task_topology.json"
    topology_before = topology.read_bytes()
    restored_paths = [
        REPO_ROOT
        / "configs"
        / "generated"
        / "cohesix_python_qemu_smp_production.json",
        REPO_ROOT
        / "configs"
        / "generated"
        / "cohesix_python_pi4_production.json",
        REPO_ROOT
        / "configs"
        / "generated"
        / "implementation_surface_inventory.json",
        REPO_ROOT
        / "configs"
        / "generated"
        / "host_integration_dependency.json",
        REPO_ROOT / "docs" / "snippets" / "host_integration_dependency.md",
    ]
    restored_before = {path: path.read_bytes() for path in restored_paths}
    policy = REPO_ROOT / "configs" / "generated" / "cohsh_policy.toml"
    policy_existed = policy.exists()
    policy_before = policy.read_bytes() if policy_existed else b""
    environment = os.environ.copy()
    environment.update(
        {
            "COHESIX_BUILD_RUN_BIN": str(fake_build),
            "COHSH_BATCH_GROUPS": "base,base-telemetry,base-shard",
            "COHSH_BATCH_PREPARE_ONLY": "1",
            "COHSH_LOG_ROOT": str(archive),
            "COHSH_QEMU_ARTIFACT_ROOT": str(artifact_root),
            "COHSH_TRANSPORT_RESULT_ROOT": str(transport_root / "results"),
            "FAKE_BUILD_COUNT_FILE": str(count_file),
            "FAKE_LAUNCH_ARTIFACT_TOOL": str(
                REPO_ROOT / "scripts" / "lib" / "qemu_launch_artifacts.py"
            ),
            "QEMU_BIN": str(fake_qemu),
            "COHESIX_QEMU_ACCEL": "tcg",
            "COHESIX_QEMU_VIRT": "off",
            "COHESIX_QEMU_MACHINE_EXTRA": "kernel-irqchip=off",
            "COHESIX_QEMU_SMP_TOPO": "4,cores=4,threads=1,sockets=1",
            "COHESIX_SEL4_PROFILE": "qemu_smp_production",
            "SEL4_BUILD_DIR": str(sel4),
            "TEST_PLAN_SOURCE_DIGEST": "sha256:" + ("a" * 64),
            "TMPDIR": str(tmp_path),
        }
    )
    prepared = subprocess.run(
        ["bash", str(SCRIPT)],
        cwd=REPO_ROOT,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
    )
    assert prepared.returncode == 0, f"{prepared.stdout}\n{prepared.stderr}"

    assert count_file.read_text(encoding="utf-8").splitlines() == ["build"]
    assert (artifact_root / "base" / "qemu-artifact.json").is_file()
    assert (
        artifact_root / "base" / "evidence" / "cohsh_policy.toml"
    ).read_text(encoding="utf-8") == "fixture = true\n"
    assert generated.read_bytes() == generated_before
    assert topology.read_bytes() == topology_before
    for path, before in restored_before.items():
        assert path.read_bytes() == before
    if policy_existed:
        assert policy.read_bytes() == policy_before
    else:
        assert not policy.exists()
    assert not list(tmp_path.glob("cohesix-generated.*"))


def test_qemu_batch_uses_canonical_policy_and_requires_auth_success() -> None:
    """Artifact recording and readiness use current generated policy truth."""

    source = SCRIPT.read_text(encoding="utf-8")
    run_batch = source.split("run_batch() {", maxsplit=1)[1].split(
        "ensure_live_cohsh_bin() {", maxsplit=1
    )[0]

    assert '--policy "$GENERATED_CONFIG_DIR/cohsh_policy.toml"' in source
    assert '--policy "$PROJECT_ROOT/out/cohsh_policy.toml"' not in source
    assert 'if data == b"OK AUTH":' in source
    assert 'or b"ERR AUTH" in data' not in source
    assert 'READY_MARKER="[mark] root-console.start.ok"' in source
    ready = run_batch.index(
        'wait_log_marker "$qemu_log" "$READY_MARKER" "$READY_TIMEOUT"'
    )
    matrix = run_batch.index("run_qemu_response_matrix")
    assert ready < matrix
    assert 'wait_port_ready "$QEMU_TCP_HOST"' not in run_batch
    assert "wait_auth_ready" not in run_batch
    assert "proceeding because TCP console is reachable" not in run_batch


def test_qemu_base_binds_fixed_response_matrix_without_pi_routing() -> None:
    """Only QEMU base evidence runs the one-socket fixed response matrix."""

    source = SCRIPT.read_text(encoding="utf-8")
    run_batch = source.split("run_batch() {", maxsplit=1)[1].split(
        "ensure_live_cohsh_bin() {", maxsplit=1
    )[0]
    pi_batch = source.split("run_pi4_batch() {", maxsplit=1)[1].split(
        "qemu_pid=0", maxsplit=1
    )[0]
    response_matrix = source.split("run_qemu_response_matrix() {", maxsplit=1)[1].split(
        "write_qemu_result() {", maxsplit=1
    )[0]
    selected_count = source.split("selected_script_count() {", maxsplit=1)[1].split(
        "resolve_manifest_auth_token() {", maxsplit=1
    )[0]

    fixed_call = run_batch.index('run_qemu_response_matrix')
    cohsh_loop = run_batch.index('for script in "${scripts[@]}"')
    assert fixed_call < cohsh_loop
    assert "--mode fixed" in response_matrix
    assert 'evidence_scripts+=("$QEMU_RESPONSE_MATRIX_FIXED_LABEL")' in run_batch
    assert "run_qemu_response_matrix" not in pi_batch
    assert 'total=$((total + ${#BASE_SCRIPTS[@]}))' in selected_count
    assert 'if [[ "$BATCH_TARGET" == "qemu" ]]; then' in selected_count
    assert "total=$((total + 1))" in selected_count


def test_operational_base_excludes_retained_host_sidecar_mock_fixture() -> None:
    """The production-truth batch does not execute the retained mock fixture."""

    source = SCRIPT.read_text(encoding="utf-8")
    base_scripts = source.split("BASE_SCRIPTS=(", maxsplit=1)[1].split(
        "\n)", maxsplit=1
    )[0]
    script_catalog = source.split("script_path_for() {", maxsplit=1)[1].split(
        "run_cohsh() {", maxsplit=1
    )[0]
    fixture_path = REPO_ROOT / "scripts" / "cohsh" / "host_sidecar_mock.coh"
    surface_catalog = (
        REPO_ROOT / "configs" / "implementation_surfaces.toml"
    ).read_text(encoding="utf-8")
    fixture_rule = surface_catalog.split(
        'id = "coh-mock-script"', maxsplit=1
    )[1].split("[[tracked_rules]]", maxsplit=1)[0]

    assert '"host_sidecar_mock.coh"' not in base_scripts
    assert "host_sidecar_mock.coh)" in script_catalog
    assert 'echo "scripts/cohsh/host_sidecar_mock.coh"' in script_catalog
    assert fixture_path.is_file()
    assert 'exact = "scripts/cohsh/host_sidecar_mock.coh"' in fixture_rule
    assert 'class = "fixture"' in fixture_rule
    assert "production_reachable = false" in fixture_rule


def test_gated_policy_and_replay_use_only_target_local_controls() -> None:
    """Gated control regressions do not depend on a populated host provider."""

    source = SCRIPT.read_text(encoding="utf-8")
    gated_scripts = source.split("GATED_SCRIPTS=(", maxsplit=1)[1].split(
        "\n)", maxsplit=1
    )[0]
    policy = (REPO_ROOT / "scripts" / "cohsh" / "policy_gate.coh").read_text(
        encoding="utf-8"
    )
    replay = (REPO_ROOT / "scripts" / "cohsh" / "replay_journal.coh").read_text(
        encoding="utf-8"
    )

    assert '"replay_journal.coh"' in gated_scripts
    assert '"policy_gate.coh"' in gated_scripts
    assert "/host/" not in policy
    assert "/host/" not in replay
    assert policy.count('"target":"/queen/ctl"') == 1
    assert policy.count('> /queen/ctl') == 3
    assert policy.count("EXPECT ERR\nEXPECT SUBSTR EPERM") == 2
    assert "cat /policy-gate-proof" in policy
    assert replay.count('"target":"/queen/ctl"') == 2
    assert replay.count('> /queen/ctl') == 2
    assert "cat /replay-proof-1" in replay
    assert "cat /replay-proof-2" in replay
    assert "echo '{\"from\":0}' > /replay/ctl" in replay
    assert "echo '{\"from\":999999}' > /replay/ctl" in replay

    surface_catalog = (
        REPO_ROOT / "configs" / "implementation_surfaces.toml"
    ).read_text(encoding="utf-8")
    for rule_id, path in (
        ("coh-policy-gate-target-script", "scripts/cohsh/policy_gate.coh"),
        ("coh-replay-target-script", "scripts/cohsh/replay_journal.coh"),
    ):
        rule = surface_catalog.split(f'id = "{rule_id}"', maxsplit=1)[1].split(
            "[[tracked_rules]]", maxsplit=1
        )[0]
        assert f'exact = "{path}"' in rule
        assert 'class = "diagnostic"' in rule
        assert "production_reachable = false" in rule
        assert 'current_observed_mode = "target_local_control_regression"' in rule


def test_cas_fixture_trust_is_routed_by_target_profile() -> None:
    """Fixture-positive CAS runs only on the QEMU gated trust profile."""

    source = SCRIPT.read_text(encoding="utf-8")

    def array_entries(name: str) -> list[str]:
        body = source.split(f"{name}=(", maxsplit=1)[1].split(
            "\n)", maxsplit=1
        )[0]
        return [line.strip().strip('"') for line in body.splitlines() if line.strip()]

    base_scripts = source.split("BASE_SCRIPTS=(", maxsplit=1)[1].split(
        "\n)", maxsplit=1
    )[0]
    gated_scripts = source.split("GATED_SCRIPTS=(", maxsplit=1)[1].split(
        "\n)", maxsplit=1
    )[0]
    qemu_fixture_scripts = source.split(
        "QEMU_GATED_FIXTURE_SCRIPTS=(", maxsplit=1
    )[1].split("\n)", maxsplit=1)[0]
    qemu_dispatch = source.rsplit('if group_selected "gated"; then', maxsplit=1)[1]
    pi_dispatch = source.split("run_pi4_batch() {", maxsplit=1)[1].split(
        "qemu_pid=0", maxsplit=1
    )[0]
    selected_count = source.split("selected_script_count() {", maxsplit=1)[1].split(
        "resolve_manifest_auth_token() {", maxsplit=1
    )[0]

    assert '"cas_fixture_signature_rejected.coh"' in base_scripts
    assert '"cas_roundtrip.coh"' not in base_scripts
    assert '"cas_roundtrip.coh"' not in gated_scripts
    assert qemu_fixture_scripts.strip() == '"cas_roundtrip.coh"'
    assert '"${GATED_SCRIPTS[@]}"' in qemu_dispatch
    assert '"${QEMU_GATED_FIXTURE_SCRIPTS[@]}"' in qemu_dispatch
    assert "QEMU_GATED_FIXTURE_SCRIPTS" not in pi_dispatch
    assert (
        'total=$((total + ${#QEMU_GATED_FIXTURE_SCRIPTS[@]}))'
        in selected_count
    )

    base_count = len(array_entries("BASE_SCRIPTS"))
    telemetry_count = len(array_entries("BASE_TELEMETRY_SCRIPTS"))
    shard_count = len(array_entries("BASE_SHARD_SCRIPTS"))
    gated_count = len(array_entries("GATED_SCRIPTS"))
    qemu_fixture_count = len(array_entries("QEMU_GATED_FIXTURE_SCRIPTS"))
    assert (base_count, telemetry_count, shard_count) == (10, 2, 1)
    assert (gated_count, qemu_fixture_count) == (4, 1)
    assert base_count + telemetry_count + shard_count + gated_count == 17
    assert (
        base_count
        + telemetry_count
        + shard_count
        + gated_count
        + qemu_fixture_count
    ) == 18

    rejection = (
        REPO_ROOT / "scripts" / "cohsh" / "cas_fixture_signature_rejected.coh"
    ).read_text(encoding="utf-8")
    positive = (
        REPO_ROOT / "scripts" / "cohsh" / "cas_roundtrip.coh"
    ).read_text(encoding="utf-8")
    assert rejection.count("/updates/100/manifest.cbor") == 3
    assert rejection.count("EXPECT OK") == 3
    assert "EXPECT ERR\nEXPECT SUBSTR EPERM" in rejection
    assert "/policy/preflight/req" not in positive

    operational = (REPO_ROOT / "configs" / "root_task.toml").read_text(
        encoding="utf-8"
    )
    pi4 = (
        REPO_ROOT / "configs" / "root_task_pi4_uboot_aarch64.toml"
    ).read_text(encoding="utf-8")
    gated = (REPO_ROOT / "configs" / "root_task_regression.toml").read_text(
        encoding="utf-8"
    )
    public_key = 'verification_key_path = "resources/keys/cas_verification_key.hex"'
    fixture_key = (
        'verification_key_path = "resources/fixtures/cas_verification_key.hex"'
    )
    assert public_key in operational
    assert public_key in pi4
    assert fixture_key not in operational
    assert fixture_key not in pi4
    assert fixture_key in gated


def test_qemu_close_oracle_is_same_connection_protocol_not_legacy_uart() -> None:
    """QEMU trusts matrix/cohsh ACK plus EOF while Pi lifecycle stays intact."""

    source = SCRIPT.read_text(encoding="utf-8")
    response_matrix = source.split("run_qemu_response_matrix() {", maxsplit=1)[1].split(
        "write_qemu_result() {", maxsplit=1
    )[0]
    run_batch = source.split("run_batch() {", maxsplit=1)[1].split(
        "ensure_live_cohsh_bin() {", maxsplit=1
    )[0]
    live_group = source.split("run_live_group() {", maxsplit=1)[1].split(
        "write_pi4_result() {", maxsplit=1
    )[0]

    assert '"$QEMU_RESPONSE_MATRIX_SCRIPT"' in response_matrix
    assert 'if ! run_cohsh "$script"' in run_batch
    assert "OK QUIT followed by target EOF" in response_matrix
    assert "OK QUIT" in run_batch and "peer EOF" in run_batch
    assert "audit tcp.conn.close" not in response_matrix
    assert "audit tcp.conn.close" not in run_batch
    assert "wait_log_count_increase" not in response_matrix
    assert "wait_log_count_increase" not in run_batch
    assert "count_log_pattern" not in response_matrix
    assert "count_log_pattern" not in run_batch
    assert "QUIT_CLOSE_TIMEOUT" not in response_matrix
    assert "QUIT_CLOSE_TIMEOUT" not in run_batch
    assert 'if run_cohsh "$script"' in live_group
    assert 'run_lifecycle_resume "after-${name}-${script_name}" || true' in live_group


def test_auth_readiness_requires_one_exact_complete_ok_frame() -> None:
    """Readiness rejects protocol errors, lookalikes, and truncated frames."""

    assert run_auth_probe(b"OK AUTH") == 0
    assert run_auth_probe(b"ERR AUTH") != 0
    assert run_auth_probe(b"XOK AUTH") != 0
    assert run_auth_probe(b"OK AUTHENTICATED") != 0
    assert run_auth_probe(b"OK AUTH detail=unexpected") != 0
    assert run_auth_probe(b"OK AUTH", declared_extra=1) != 0
