# Author: Lukas Bower
# Purpose: Verify the argument-driven remote Linux release-builder contract.
# Copyright 2026 Lukas Bower

from pathlib import Path
import os
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "linux_host_tools_sync.sh"


def test_remote_builder_has_no_embedded_environment_locations() -> None:
    source = SCRIPT.read_text(encoding="utf-8")

    for required in (
        "--remote-build-dir",
        "--remote-release-dir",
        "--remote-cargo",
        "--remote-cargo-home",
        "--local-out",
        "--manifest-out",
        "--local-tarball",
        "--max-glibc-version",
    ):
        assert required in source
    for forbidden in (
        "merlin2.local",
        "/mnt/nvme",
        "/home/wizard",
        'REMOTE_USER="ubuntu"',
        "cohesix-builder-key.pem",
    ):
        assert forbidden not in source


def test_remote_builder_does_not_install_or_reconfigure_packages() -> None:
    source = SCRIPT.read_text(encoding="utf-8")

    assert "apt-get" not in source
    assert "sources.list" not in source
    assert "debootstrap" not in source
    assert "libnvidia-ml-dev" not in source
    assert "never mutates apt sources or installs system packages" in source


def test_remote_builder_exposes_build_and_archive_modes() -> None:
    result = subprocess.run(
        [str(SCRIPT), "--help"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr
    assert "build-tools" in result.stdout
    assert "archive-bundle" in result.stdout
    assert "--force" in result.stdout


def test_remote_builder_packages_compile_time_inputs() -> None:
    source = SCRIPT.read_text(encoding="utf-8")

    assert 'git --git-dir="$archive_git" archive --format=tar "$source_commit"' in source
    assert "rust-toolchain.toml" in source
    assert "does not match pinned" in source
    assert "toolchain_channel" in source
    assert "status --porcelain=v1 --untracked-files=all" in source


def test_transferred_archive_contains_both_gateway_compile_time_contracts(
    tmp_path: Path,
) -> None:
    """Execute the actual archive recipe against an isolated tracked source tree."""
    repo = tmp_path / "source"
    repo.mkdir()
    required = [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo/config.toml",
        "scripts/rustc-wrapper.sh",
        "configs/generated/cohesix_python_qemu_smp_production.json",
        "configs/generated/cohesix_python_pi4_production.json",
        "third_party/example/include/contract.h",
        ".gitattributes",
    ]
    for relative in required:
        path = repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("fixture\n")
    (repo / ".gitattributes").write_text("third_party export-ignore\n")
    ignored = repo / "configs/generated/untracked-secret.json"
    ignored.write_text("must not enter the source transfer\n")
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    subprocess.run(["git", "-C", str(repo), "add", *required], check=True)
    subprocess.run(
        ["git", "-C", str(repo), "-c", "user.name=Fixture",
         "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture"],
        check=True,
    )
    source = SCRIPT.read_text()
    start = source.index('  local archive_git=', source.index('log "Packaging the exact clean'))
    end = source.index("\n  local source_sha256", start)
    archive = tmp_path / "source.tar.gz"
    commit = subprocess.check_output(
        ["git", "-C", str(repo), "rev-parse", "HEAD"], text=True
    ).strip()
    env = {**os.environ, "ROOT_DIR": str(repo), "source_tarball": str(archive),
           "temp_dir": str(tmp_path), "source_commit": commit}
    subprocess.run(
        ["bash", "-euo", "pipefail", "-c",
         "archive_source() {\n" + source[start:end] + "\n}\narchive_source"],
        env=env, check=True,
    )
    with tarfile.open(archive) as handle:
        names = set(handle.getnames())
    assert set(required) <= names
    assert "configs/generated/untracked-secret.json" not in names
    assert (repo / ".gitattributes").read_text() == "third_party export-ignore\n"
    assert not (repo / ".git/info/attributes").exists()


def test_remote_builder_fails_before_ssh_when_locations_are_missing() -> None:
    result = subprocess.run(
        [
            str(SCRIPT),
            "build-tools",
            "--host",
            "builder.example",
            "--user",
            "builder",
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert "--remote-build-dir is required" in result.stderr


def test_remote_builder_rejects_mode_mismatched_options() -> None:
    result = subprocess.run(
        [
            str(SCRIPT),
            "archive-bundle",
            "--host",
            "builder.example",
            "--user",
            "builder",
            "--remote-build-dir",
            "/srv/cohesix-build",
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode != 0
    assert "build-tools options are not valid" in result.stderr
