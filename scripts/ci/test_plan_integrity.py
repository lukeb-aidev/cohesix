#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Check exact test-plan evidence, catalog ownership, and staged-runner contracts.
# Copyright 2026 Lukas Bower

"""Validate test-plan relationships that span documentation and scripts."""

from __future__ import annotations

import hashlib
import pathlib
import re
import subprocess
import sys


ROOT = pathlib.Path(__file__).resolve().parents[2]
DOC = ROOT / "docs" / "TEST_PLAN.md"
RUNNER = ROOT / "scripts" / "ci" / "test_plan_run.sh"
COMMON = ROOT / "scripts" / "ci" / "test_plan_common.sh"
EVIDENCE = ROOT / "scripts" / "ci" / "test_plan_evidence.py"
RESOURCES = ROOT / "scripts" / "ci" / "test_plan_resources.sh"
HOST_GATE = ROOT / "scripts" / "ci" / "host_hermetic_gate.sh"
PYTHON_GATE = ROOT / "scripts" / "ci" / "python_test_gate.sh"
QEMU_HELPER = ROOT / "scripts" / "ci" / "qemu_artifact.py"
PI4_IMAGE_BUILD = ROOT / "scripts" / "pi4-image-build.sh"
STAGES = {
    stage: ROOT / "scripts" / "ci" / f"test_plan_stage_{stage:02d}_{name}.sh"
    for stage, name in (
        (1, "integrity"),
        (2, "host_fast"),
        (3, "qemu_tcp_regression"),
        (4, "rest_multiplexer"),
        (5, "due_diligence"),
    )
}
DUE_DILIGENCE = ROOT / "scripts" / "ci" / "due_diligence_gate.sh"
QEMU_BATCH = ROOT / "scripts" / "cohsh" / "run_regression_batch.sh"
CI = ROOT / ".github" / "workflows" / "ci.yml"
HASH_ENTRY = re.compile(
    r"^- `([^`]+)` — `sha256:([0-9a-f]{64})`$",
    re.MULTILINE,
)
FILTERED_LIBRARY_TEST = re.compile(r"cargo test [^\n\"]* --lib\s+\S+")


def read(path: pathlib.Path) -> str:
    """Read a repository text file."""

    return path.read_text(encoding="utf-8")


def require(
    errors: list[str],
    text: str,
    tokens: tuple[str, ...],
    label: str,
) -> None:
    """Require every exact token in one text surface."""

    for token in tokens:
        if token not in text:
            errors.append(f"{label}: missing required contract `{token}`")


def forbid(
    errors: list[str],
    text: str,
    tokens: tuple[str, ...],
    label: str,
) -> None:
    """Reject obsolete or duplicate contract tokens."""

    for token in tokens:
        if token in text:
            errors.append(f"{label}: forbidden obsolete contract `{token}`")


def verify_fixture_hashes(errors: list[str], document: str) -> None:
    """Preserve exact fixture hashing without duplicating the action catalog."""

    entries = HASH_ENTRY.findall(document)
    if not entries:
        errors.append("docs/TEST_PLAN.md: no fixture hash entries found")
        return
    for relative, expected in entries:
        path = ROOT / relative
        if not path.is_file():
            errors.append(f"docs/TEST_PLAN.md: missing hashed fixture {relative}")
            continue
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != expected:
            errors.append(
                f"docs/TEST_PLAN.md: hash mismatch for {relative}: "
                f"expected={expected} actual={actual}"
            )


def verify_runner_list(errors: list[str]) -> None:
    """Require the executable runner inventory to name the five stages once."""

    result = subprocess.run(
        [str(RUNNER), "--list"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        errors.append(
            f"scripts/ci/test_plan_run.sh --list failed: {result.stderr.strip()}"
        )
        return
    for stage, path in STAGES.items():
        relative = path.relative_to(ROOT).as_posix()
        expected = f"{stage}  {relative}"
        if result.stdout.count(expected) != 1:
            errors.append(
                "scripts/ci/test_plan_run.sh --list must name exactly once: "
                f"{expected}"
            )


def verify_python_lane(errors: list[str]) -> None:
    """Require broad Python discovery and all test-plan contract tests."""

    gate = ROOT / "scripts" / "ci" / "python_test_gate.sh"
    result = subprocess.run(
        [str(gate), "--list-tests"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        errors.append(f"python test lane listing failed: {result.stderr.strip()}")
        return
    selected = set(result.stdout.splitlines())
    required = {
        "tests",
        "tools/cohesix-py/tests",
        "scripts/ci/test_due_diligence_lifecycle.py",
        "scripts/ci/test_host_hermetic_gate.py",
        "scripts/ci/test_python_test_gate.py",
        "scripts/ci/test_qemu_artifact.py",
        "scripts/ci/test_swarmui_ui_gate.py",
        "scripts/ci/test_test_plan_catalog.py",
        "scripts/ci/test_test_plan_integrity.py",
        "scripts/ci/test_test_plan_resources.py",
        "scripts/ci/test_test_plan_runner.py",
    }
    missing = sorted(
        path
        for path in required
        if (ROOT / path).is_file() and path not in selected
    )
    if missing:
        errors.append(f"python test lane omits contract tests: {missing}")


def verify_headers(errors: list[str]) -> None:
    """Require repository metadata on every test-plan-owned human file."""

    paths = [
        DOC,
        ROOT / "configs" / "test_plan_actions.toml",
        ROOT / "configs" / "test-plan-python-requirements.lock",
        ROOT / "scripts" / "ci" / "check_test_plan.sh",
        HOST_GATE,
        PYTHON_GATE,
        QEMU_HELPER,
        ROOT / "scripts" / "ci" / "swarmui_ui_gate.sh",
        ROOT / "scripts" / "ci" / "test_plan_catalog.py",
        EVIDENCE,
        RESOURCES,
        ROOT / "scripts" / "ci" / "test_plan_integrity.py",
        COMMON,
        RUNNER,
        *STAGES.values(),
        DUE_DILIGENCE,
        QEMU_BATCH,
        PI4_IMAGE_BUILD,
        CI,
    ]
    for path in paths:
        if not path.is_file():
            errors.append(f"missing test-plan-owned file: {path.relative_to(ROOT)}")
            continue
        head = "\n".join(read(path).splitlines()[:8])
        for token in (
            "Author: Lukas Bower",
            "Purpose:",
            "Copyright 2026 Lukas Bower",
        ):
            if token not in head:
                errors.append(
                    f"{path.relative_to(ROOT)}: missing file header `{token}`"
                )


def main() -> int:
    """Run all cross-file integrity checks."""

    errors: list[str] = []
    document = read(DOC)
    runner = read(RUNNER)
    common = read(COMMON)
    evidence = read(EVIDENCE)
    resources = read(RESOURCES)
    host_gate = read(HOST_GATE)
    python_gate = read(PYTHON_GATE)
    qemu_helper = read(QEMU_HELPER)
    pi4_image_build = read(PI4_IMAGE_BUILD)
    stage_01 = read(STAGES[1])
    stage_02 = read(STAGES[2])
    stage_03 = read(STAGES[3])
    stage_04 = read(STAGES[4])
    stage_05 = read(STAGES[5])
    due_diligence = read(DUE_DILIGENCE)
    qemu_batch = read(QEMU_BATCH)
    ci = read(CI)

    verify_fixture_hashes(errors, document)
    verify_runner_list(errors)
    verify_python_lane(errors)
    verify_headers(errors)

    require(
        errors,
        document,
        (
            "## Claim tiers and PASS terminology",
            "`common-hermetic`",
            "`qemu-integration`",
            "`pi4-transport`",
            "`pi4-hardware`",
            "`ui`",
            "`performance`",
            "`federation`",
            "`release`",
            "must never be described as `pi4-hardware`",
            "--reuse-common-from <state-dir>",
            "`stage_XX.attestation`",
            "configs/test_plan_actions.toml",
            "unknown changed path selects the complete catalog",
            "TP_HOST_JOBS",
        ),
        "docs/TEST_PLAN.md",
    )
    require(
        errors,
        runner,
        (
            "--resume",
            "--force",
            "--reuse-common-from",
            "tp_verify_stage_attestation",
            "tp_qualify_stage_attestation",
            "TEST_PLAN_STAGED_RUN",
            "refresh stage 5",
        ),
        "scripts/ci/test_plan_run.sh",
    )
    require(
        errors,
        common,
        (
            "tp_run_catalog_action",
            "tp_verify_stage_attestation",
            "TEST_PLAN_SOURCE_DIGEST",
            "TEST_PLAN_ATTEMPT_MANIFEST",
            "duration_ms",
            "tp_redact_command",
            "test-plan state directory is already active",
            "-u BASH_ENV",
        ),
        "scripts/ci/test_plan_common.sh",
    )
    require(
        errors,
        evidence,
        (
            'ACTION_SCHEMA = "cohesix.test-plan-action/v1"',
            'STAGE_SCHEMA = "cohesix.test-plan-stage-attestation/v1"',
            "tracked_diff_sha256",
            "untracked",
            "external_sel4_records",
            "toolchain_digest",
            "required_artifacts",
            "redact_stream",
            "import_common",
            "test_plan_resources.sh",
        ),
        "scripts/ci/test_plan_evidence.py",
    )
    require(
        errors,
        resources,
        (
            "tp_configure_resource_limits",
            "CARGO_BUILD_JOBS",
            "CMAKE_BUILD_PARALLEL_LEVEL",
            "RUST_TEST_THREADS",
            "RAYON_NUM_THREADS",
            "TP_UI_WORKERS",
            "TP_ALLOW_OVERSUBSCRIBE",
        ),
        "scripts/ci/test_plan_resources.sh",
    )
    require(
        errors,
        stage_01,
        (
            "--stage 1",
            "--scope common",
            "tp_run_catalog_action",
            "TP_SKIP_PYTHON",
        ),
        "Stage 01",
    )
    forbid(
        errors,
        stage_01,
        ("test-plan-hash-check", "scripts/ci/check_test_plan.sh"),
        "Stage 01",
    )
    require(
        errors,
        stage_02,
        (
            "--scope provisioned-target",
            "tp_run_catalog_action",
            "ACTION_SET scope=provisioned-target",
        ),
        "Stage 02",
    )
    if "cargo test " in stage_02:
        errors.append(
            "Stage 02: literal Cargo test commands duplicate catalog authority"
        )
    if FILTERED_LIBRARY_TEST.search(stage_02):
        errors.append("Stage 02: filtered library tests can silently match zero")
    require(
        errors,
        stage_03,
        (
            'stage3_root="${TP_ATTEMPT_DIR}/transport"',
            "COHSH_TRANSPORT_RESULT_ROOT",
            "COHSH_QEMU_ARTIFACT_ROOT",
            "COHSH_REQUIRE_RESULT_EVIDENCE=1",
            "verify-aggregate",
            "stage_03_artifact_root.path",
            "publish-root",
        ),
        "Stage 03",
    )
    require(
        errors,
        stage_04,
        (
            "TEST_PLAN_STAGED_RUN",
            'stage4_root="${TP_ATTEMPT_DIR}/rest"',
            "stage_03_artifact_root.path",
            "verify-pi4-continuity",
            "verify-qemu-target",
            "stage_04_artifact_root.path",
            "publish-root",
            'COHSH_PARALLELISM="${core_parallelism}"',
            "stage4_stop_process_tree",
        ),
        "Stage 04",
    )
    require(
        errors,
        stage_05,
        (
            "DD_REUSE_STAGED_EVIDENCE_FROM",
            "DD_REUSE_STAGED_EVIDENCE_TARGET",
            "stage_05_artifact_root.path",
            "publish-root",
        ),
        "Stage 05",
    )
    forbid(
        errors,
        stage_05,
        (
            "cargo test --workspace",
            "cargo clippy --workspace",
            "scripts/check-generated.sh",
        ),
        "Stage 05",
    )
    require(
        errors,
        pi4_image_build,
        (
            "resolve_build_jobs",
            "TP_HOST_JOBS",
            "CARGO_BUILD_JOBS",
            "CMAKE_BUILD_PARALLEL_LEVEL",
        ),
        "scripts/pi4-image-build.sh",
    )

    require(
        errors,
        due_diligence,
        (
            "run_catalog_action",
            "integrity.cargo-metadata",
            "integrity.generated-contracts",
            "host_hermetic_gate.sh",
            "--common-only",
            "cargo audit",
            "cargo deny check advisories",
            "DD_REUSE_STAGED_EVIDENCE_FROM",
            "tp_verify_stage_attestation",
            "DD_COLLECT_ALL",
            "--collect-all",
            "staged-evidence-state",
            "cargo-audit-version",
            "cargo-deny-version",
        ),
        "standalone due diligence",
    )
    forbid(
        errors,
        due_diligence,
        ("cargo test -p secure9p-codec", "cargo test -p tests"),
        "standalone due diligence",
    )

    require(
        errors,
        qemu_batch,
        (
            'prepare_qemu_artifact "base"',
            'prepare_qemu_artifact "gated"',
            "qemu_artifact.py",
            "restore_generated_outputs",
            "trap cleanup EXIT",
            "write_transport_aggregate",
            "resolve_qemu_host_ports",
            "validate_output_roots",
        ),
        "QEMU regression batch",
    )
    if "cargo run -p coh-rtc" in qemu_batch:
        errors.append(
            "QEMU regression batch: duplicate coh-rtc invocation remains"
        )
    if qemu_batch.count('prepare_qemu_artifact "base"') != 1:
        errors.append("QEMU regression batch must prepare base exactly once")
    if qemu_batch.count('prepare_qemu_artifact "gated"') != 1:
        errors.append("QEMU regression batch must prepare gated exactly once")

    require(
        errors,
        qemu_helper,
        (
            "verify_aggregate",
            "verify_pi4_continuity",
            "verify_gateway_binding",
            "publish_root",
        ),
        "QEMU artifact helper",
    )
    require(
        errors,
        host_gate,
        (
            "--integrity-only",
            "--common-only",
            "--scope provisioned-target",
            "--stage 1",
            "--stage 2",
            "tp_configure_resource_limits",
        ),
        "host hermetic gate",
    )
    require(
        errors,
        python_gate,
        (
            "test-plan-python-requirements.lock",
            "--require-hashes",
            "--only-binary=:all:",
            '"${venv_python}" -m pytest -q "${python_test_paths[@]}"',
        ),
        "Python test gate",
    )
    require(
        errors,
        read(ROOT / "scripts" / "ci" / "swarmui_ui_gate.sh"),
        (
            "tp_configure_resource_limits",
            '--workers="${TP_UI_WORKERS}"',
        ),
        "SwarmUI UI gate",
    )

    require(
        errors,
        ci,
        (
            "source-integrity:",
            "host-hermetic:",
            "swarmui-ui:",
            "dependency-audit:",
            "ci:",
            "scripts/ci/host_hermetic_gate.sh --integrity-only",
            "scripts/ci/host_hermetic_gate.sh --common-only",
            "scripts/ci/swarmui_ui_gate.sh --run",
            "cargo audit",
            "cargo deny check advisories",
        ),
        ".github/workflows/ci.yml",
    )
    forbid(
        errors,
        ci,
        (
            "cargo test --workspace -- --test-threads=1",
            "github.event_name != 'schedule'",
        ),
        ".github/workflows/ci.yml",
    )

    for inline in re.findall(r"`([^`]+)`", document):
        if re.search(r"(^|\s)python(\s|$)", inline) and "python3" not in inline:
            errors.append(
                f"docs/TEST_PLAN.md: use python3 in command `{inline}`"
            )

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print("test-plan cross-file contracts ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
