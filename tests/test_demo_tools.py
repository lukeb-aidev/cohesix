# Author: Lukas Bower
# Purpose: Verify retained demo identities, input refusal and host-command failure propagation.
# Copyright 2026 Lukas Bower

"""Deterministic tests for demo helpers; no native or target execution is claimed."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("prepare_worker", ROOT / "demo/prepare_worker.py")
assert SPEC is not None and SPEC.loader is not None
PREPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREPARE)


class WorkerPreparationTests(unittest.TestCase):
    def test_identity_is_retained_and_existing_output_is_never_replaced(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "request"
            command = {"spawn": "heartbeat", "ticks": 10, "budget": {"ttl_s": 60, "ops": 100}}
            PREPARE.prepare(output, command, 7, "reviewed-demo", 1800000000000)
            source = (output / "intent.coh").read_text()
            line = next(line for line in source.splitlines() if line.startswith("echo "))
            payload = json.loads(line.removeprefix("echo ").removesuffix(" > /queen/intents/ctl"))
            self.assertEqual(payload, {
                "schema": "queen-intent/v1",
                "id": "reviewed-demo",
                "idempotency_key": "reviewed-demo",
                "issued_unix_ms": 1800000000000,
                "writer_epoch": 7,
                "cmd": json.dumps(command, separators=(",", ":")),
            })
            self.assertNotIn("/actions/queue", source)
            self.assertIn("/actions/queue", (output / "approve.coh").read_text())
            self.assertEqual(output.stat().st_mode & 0o777, 0o700)
            self.assertEqual((output / "intent.coh").stat().st_mode & 0o777, 0o600)
            with self.assertRaises(FileExistsError):
                PREPARE.prepare(output, {"spawn": "lora"}, 8, "replacement", 1800000000001)
            self.assertEqual((output / "intent.coh").read_text(), source)

    def test_refuses_unsafe_identifiers_epochs_and_commands(self) -> None:
        for value in ("", "../worker", "worker#1", "worker\nquit", "-worker", "a" * 65):
            with self.subTest(value=value), self.assertRaises(argparse.ArgumentTypeError):
                PREPARE.identifier(value)
        for value in ("-1", "1.5", "１２", str(1 << 64)):
            with self.subTest(value=value), self.assertRaises(argparse.ArgumentTypeError):
                PREPARE.unsigned(value)
        self.assertEqual(PREPARE.unsigned(str((1 << 64) - 1)), (1 << 64) - 1)
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "request"
            with self.assertRaises(ValueError):
                PREPARE.prepare(output, {"spawn": "worker-bus"}, 1, "demo", 1)
            self.assertFalse(output.exists())

    def test_lora_and_observed_worker_cleanup_use_exact_command(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            for index, command in enumerate(({"spawn": "lora"}, {"kill": "worker-27"})):
                output = Path(directory) / str(index)
                PREPARE.prepare(output, command, 9, f"demo-{index}", 1800000000000)
                source = (output / "intent.coh").read_text()
                line = next(line for line in source.splitlines() if line.startswith("echo "))
                intent = json.loads(line[5:].removesuffix(" > /queen/intents/ctl"))
                self.assertEqual(json.loads(intent["cmd"]), command)


class HostRunnerTests(unittest.TestCase):
    def invoke(
        self, arguments: list[str], environment: dict[str, str]
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(ROOT / "demo/host_tools.sh"), *arguments],
            cwd=ROOT, env=environment, capture_output=True, text=True, check=False,
        )

    def test_catalog_package_journey_and_live_script_routes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            calls = base / "calls"
            binary = base / "coh"
            binary.write_text(
                "#!/usr/bin/env python3\n"
                "import json, os, sys\n"
                "with open(os.environ['DEMO_TEST_CALLS'], 'a') as stream:\n"
                "    stream.write(json.dumps(sys.argv[1:]) + '\\n')\n"
            )
            binary.chmod(0o700)
            (base / "cohsh").symlink_to(binary)
            config = base / "config.json"
            config.write_text("{}")
            environment = {
                **os.environ, "COH_BIN": str(base), "DEMO_TEST_CALLS": str(calls),
                "COH_JOURNEY_BIN": str(binary), "DEMO_TRANSPORT": "rest",
                "COH_REST_URL": "http://127.0.0.1:8080",
            }
            for name in ("COH_POLICY", "COHSH_POLICY"):
                environment.pop(name, None)
            cases = [
                (["catalog"], ["--help"]),
                (["package", str(base), str(config)],
                 ["package", "verify", "--input", str(base), "--trust", str(config)]),
            ]
            for phase in ("validate", "doctor", "run", "submit"):
                expected = ["run" if phase == "submit" else phase, "--config", str(config)]
                if phase == "submit":
                    expected.append("--submit")
                cases.append((["journey", phase, str(config)], expected))
            for action, script in (
                ("inspect", "demo_runbook"), ("authority", "authority"),
                ("telemetry", "telemetry"), ("control", "control_plane"),
                ("evidence", "evidence"),
            ):
                cases.append(([action], [
                    "--transport", "rest", "--rest-url", "http://127.0.0.1:8080",
                    "--script", f"demo/{script}.coh",
                ]))
            for index, (arguments, expected) in enumerate(cases):
                result = self.invoke([*arguments, str(base / f"logs-{index}")], environment)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(calls.read_text().splitlines()[-1]), expected)
            self.assertIn("availability=not-installed", (base / "logs-0/run.txt").read_text())
            environment.update({
                "DEMO_TRANSPORT": "tcp", "COH_TARGET_HOST": "192.0.2.1",
                "COH_TARGET_PORT": "31337",
            })
            result = self.invoke(["inspect", str(base / "tcp")], environment)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(calls.read_text().splitlines()[-1]), [
                "--transport", "tcp", "--tcp-host", "192.0.2.1", "--tcp-port", "31337",
                "--script", "demo/demo_runbook.coh",
            ])

    def test_workflow_argv_and_failures_are_preserved_without_retries(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            binary = base / "coh"
            calls = base / "calls"
            binary.write_text(
                "#!/usr/bin/env python3\n"
                "import json, os, sys\n"
                "with open(os.environ['DEMO_TEST_CALLS'], 'a') as stream:\n"
                "    stream.write(json.dumps(sys.argv[1:]) + '\\n')\n"
                "print('fixture verifier result')\n"
                "raise SystemExit(int(os.environ.get('DEMO_TEST_EXIT', '0')))\n"
            )
            binary.chmod(0o700)
            deployment = base / "deployment with spaces.json"
            deployment.write_text("{}")
            environment = {
                **os.environ, "COH_BIN": str(base), "DEMO_TEST_CALLS": str(calls),
                "COH_REST_URL": "http://127.0.0.1:8080",
                "COH_REST_AUTH_TOKEN": "env:DEMO_TEST_AUTH",
                "COH_TICKET_REF": "file:/private/operator.ticket",
            }
            environment.pop("COH_POLICY", None)
            for kind in ("cuda", "lora"):
                for phase in ("plan", "apply", "watch", "verify", "recover"):
                    output = base / f"{kind}-{phase}"
                    result = self.invoke([kind, phase, str(deployment), str(output)], environment)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    argv = json.loads(calls.read_text().splitlines()[-1])
                    expected = (
                        [phase, "cuda-reference", "--recipe"] if kind == "cuda"
                        else ["peft", "release", phase]
                    ) + ["--deployment", str(deployment)]
                    if phase == "apply" or (kind == "cuda" and phase == "recover"):
                        expected = ["--ticket-ref", "file:/private/operator.ticket", *expected,
                                    "--rest-url", "http://127.0.0.1:8080"]
                    self.assertEqual(argv, expected)
            original = calls.read_text()
            # An existing destination fails before invoking an operation.
            result = self.invoke(
                ["cuda", "apply", str(deployment), str(base / "cuda-apply")],
                environment,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(calls.read_text(), original)
            environment["DEMO_TEST_EXIT"] = "17"
            output = base / "failed-apply"
            result = self.invoke(["cuda", "apply", str(deployment), str(output)], environment)
            self.assertEqual(result.returncode, 17)
            self.assertEqual(len(calls.read_text().splitlines()), 11)
            self.assertNotIn("commands_completed=yes", (output / "run.txt").read_text())
            self.assertIn("fixture verifier result", (output / "cuda-apply.log").read_text())

    def test_bad_phase_and_literal_ticket_fail_before_creating_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            deployment = base / "deployment.json"
            deployment.write_text("{}")
            output = base / "logs"
            environment = {
                **os.environ, "COH_REST_URL": "http://127.0.0.1:8080",
                "COH_REST_AUTH_TOKEN": "env:DEMO_TEST_AUTH", "COH_TICKET_REF": "literal",
            }
            for phase in ("shell", "apply"):
                result = self.invoke(["cuda", phase, str(deployment), str(output)], environment)
                self.assertEqual(result.returncode, 2)
                self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
