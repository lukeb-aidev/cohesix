# Author: Lukas Bower
# Purpose: Verify the Rust risk bootstrap binds its toolchain, Cargo sources, environment, and private scanner output.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import os
import pwd
import shutil
import stat
import subprocess
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
GATE = REPO_ROOT / "scripts" / "ci" / "rust_risk_gate.sh"
CANONICAL_HOME = Path(pwd.getpwuid(os.getuid()).pw_dir)


class RustRiskGateBootstrapTests(unittest.TestCase):
    @staticmethod
    def canonical_gate_environment() -> dict[str, str]:
        """Return only the ambient values a non-override gate run requires."""
        environment = {
            "HOME": str(CANONICAL_HOME),
            "PATH": os.environ["PATH"],
        }
        if "TMPDIR" in os.environ:
            environment["TMPDIR"] = os.environ["TMPDIR"]
        return environment

    def test_canonical_gate_environment_drops_runner_overrides(self) -> None:
        overrides = {
            "CARGO_HOME": "/tmp/runner-cargo-home",
            "RUSTUP_HOME": "/tmp/runner-rustup-home",
            "RUSTUP_TOOLCHAIN": "runner-toolchain",
            "CARGO_TARGET_AARCH64_UNKNOWN_NONE_RUNNER": "/tmp/runner",
        }
        with mock.patch.dict(os.environ, overrides):
            environment = self.canonical_gate_environment()

        self.assertEqual(environment["HOME"], str(CANONICAL_HOME))
        for variable_name in overrides:
            self.assertNotIn(variable_name, environment)

    @staticmethod
    def fake_cargo_environment(root: Path, marker: Path) -> dict[str, str]:
        """Return a minimal environment with a marker-writing fake Cargo."""
        fake_cargo = root / "cargo"
        fake_cargo.write_text(
            "#!/bin/sh\n"
            ": > \"${RUST_RISK_FAKE_CARGO_MARKER}\"\n"
            "exit 0\n"
        )
        fake_cargo.chmod(
            fake_cargo.stat().st_mode
            | stat.S_IXUSR
            | stat.S_IXGRP
            | stat.S_IXOTH
        )
        environment = RustRiskGateBootstrapTests.canonical_gate_environment()
        environment["PATH"] = f"{root}{os.pathsep}{environment['PATH']}"
        environment["RUST_RISK_FAKE_CARGO_MARKER"] = str(marker)
        return environment

    def test_toolchain_runner_and_linker_overrides_fail_before_cargo(self) -> None:
        for variable_name in (
            "CARGO_TARGET_AARCH64_UNKNOWN_NONE_RUNNER",
            "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER",
            "RUSTUP_TOOLCHAIN",
            "RUSTUP_HOME",
            "CARGO_HOME",
        ):
            with self.subTest(variable_name=variable_name):
                with tempfile.TemporaryDirectory() as temp_dir:
                    root = Path(temp_dir)
                    marker = root / "cargo-invoked"
                    environment = self.fake_cargo_environment(root, marker)
                    environment[variable_name] = str(root / "hostile-override")

                    result = subprocess.run(
                        ["bash", str(GATE), "--counts-only"],
                        cwd=REPO_ROOT,
                        env=environment,
                        check=False,
                        capture_output=True,
                        text=True,
                    )

                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(variable_name, result.stderr)
                    self.assertIn(
                        "refuses compiler, runner, or target-directory override",
                        result.stderr,
                    )
                    self.assertFalse(marker.exists(), "hostile override reached Cargo")

    def test_caller_target_directory_is_rejected_before_cargo(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            marker = root / "cargo-invoked"
            environment = self.fake_cargo_environment(root, marker)
            environment["RUST_RISK_TARGET_DIR"] = str(root / "preloaded-target")

            result = subprocess.run(
                ["bash", str(GATE), "--counts-only"],
                cwd=REPO_ROOT,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("RUST_RISK_TARGET_DIR", result.stderr)
            self.assertFalse(marker.exists(), "caller target directory reached Cargo")

    def test_external_cargo_config_fails_before_cargo(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            marker = root / "cargo-invoked"
            copied_repo = root / "workspace" / "cohesix"
            copied_gate = copied_repo / "scripts" / "ci" / GATE.name
            copied_gate.parent.mkdir(parents=True)
            shutil.copy2(GATE, copied_gate)
            config = root / "workspace" / ".cargo" / "config.toml"
            config.parent.mkdir()
            config.write_text(
                "[target.aarch64-apple-darwin]\n"
                "linker = \"/tmp/hostile-linker\"\n"
            )
            environment = self.fake_cargo_environment(root, marker)

            result = subprocess.run(
                ["bash", str(copied_gate), "--counts-only"],
                cwd=copied_repo,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("refuses external Cargo config", result.stderr)
            self.assertIn(str(config), result.stderr)
            self.assertFalse(marker.exists(), "external config reached Cargo")

    def test_noncanonical_home_cannot_select_a_fake_pinned_toolchain(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            marker = root / "fake-tool-invoked"
            rustc = CANONICAL_HOME / ".cargo" / "bin" / "rustc"
            host = subprocess.check_output(
                [str(rustc), "--print", "host-tuple"],
                env={"HOME": str(CANONICAL_HOME), "PATH": "/usr/bin:/bin"},
                text=True,
            ).strip()
            fake_home = root / "fake-home"
            fake_bin = (
                fake_home
                / ".rustup"
                / "toolchains"
                / f"1.97.1-{host}"
                / "bin"
            )
            fake_bin.mkdir(parents=True)
            for tool_name in ("cargo", "rustc", "rustdoc"):
                tool = fake_bin / tool_name
                tool.write_text(
                    "#!/bin/sh\n"
                    ": > \"${RUST_RISK_FAKE_TOOL_MARKER}\"\n"
                    "exit 0\n"
                )
                tool.chmod(tool.stat().st_mode | stat.S_IXUSR)
            environment = self.fake_cargo_environment(root, root / "cargo-invoked")
            environment["HOME"] = str(fake_home)
            environment["RUST_RISK_FAKE_TOOL_MARKER"] = str(marker)

            result = subprocess.run(
                ["bash", str(GATE), "--counts-only"],
                cwd=REPO_ROOT,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("refuses non-canonical HOME", result.stderr)
            self.assertFalse(marker.exists(), "fake pinned toolchain executed")

    def test_named_toolchain_resolution_ignores_directory_toolchain_file(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            copied_repo = Path(temp_dir) / "cohesix"
            for relative in (
                Path("scripts/ci/rust_risk_gate.sh"),
                Path("scripts/rustc-wrapper.sh"),
                Path(".cargo/config.toml"),
                Path("rust-toolchain.toml"),
            ):
                destination = copied_repo / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(REPO_ROOT / relative, destination)
            (copied_repo / "rust-toolchain").write_text(
                "definitely-not-installed\n"
            )
            environment = self.canonical_gate_environment()

            result = subprocess.run(
                [
                    "bash",
                    str(copied_repo / "scripts/ci/rust_risk_gate.sh"),
                    "--counts-only",
                ],
                cwd=copied_repo,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("tools/rust-risk-audit/Cargo.toml", result.stderr)
            self.assertNotIn("definitely-not-installed", result.stderr)

    def test_private_cargo_home_reextracts_registry_sources(self) -> None:
        default_sha2 = next(
            (CANONICAL_HOME / ".cargo" / "registry" / "src").glob(
                "*/sha2-0.11.0/src/lib.rs"
            )
        )
        with tempfile.TemporaryDirectory() as _temp_dir:
            system_temp = Path(tempfile.gettempdir())
            existing_bootstraps = set(system_temp.glob("cohesix-rust-risk.*"))
            environment = self.canonical_gate_environment()
            process = subprocess.Popen(
                ["bash", str(GATE), "--counts-only"],
                cwd=REPO_ROOT,
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            private_sha2 = None
            private_inode = None
            private_is_symlink = None
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline and process.poll() is None:
                matches = list(
                    path
                    for bootstrap in system_temp.glob("cohesix-rust-risk.*")
                    if bootstrap not in existing_bootstraps
                    for path in bootstrap.glob(
                        "cargo-home/registry/src/*/sha2-0.11.0/src/lib.rs"
                    )
                )
                if matches:
                    private_sha2 = matches[0]
                    private_inode = private_sha2.stat().st_ino
                    private_is_symlink = private_sha2.is_symlink()
                    break
                time.sleep(0.01)

            stdout, stderr = process.communicate(timeout=60)
            self.assertEqual(process.returncode, 0, stderr or stdout)
            self.assertIsNotNone(private_sha2, "private sha2 extraction was not observed")
            self.assertFalse(private_is_symlink)
            self.assertNotEqual(default_sha2.stat().st_ino, private_inode)


if __name__ == "__main__":
    unittest.main()
