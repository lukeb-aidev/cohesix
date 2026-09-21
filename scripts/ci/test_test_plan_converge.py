#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify convergence routing, non-claiming evidence, and fail-fast behavior.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import shlex
import subprocess
import sys
import tempfile
import textwrap
import tomllib
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
CATALOG_PATH = REPO_ROOT / "configs" / "test_plan_actions.toml"
CATALOG_TOOL = REPO_ROOT / "scripts" / "ci" / "test_plan_catalog.py"
CONVERGE_TOOL = REPO_ROOT / "scripts" / "ci" / "test_plan_converge.py"
TARGET_CANARY = REPO_ROOT / "scripts" / "ci" / "test_plan_target_canary.sh"


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"unable to import {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


catalog = load_module("test_plan_catalog", CATALOG_TOOL)
converge = load_module("convergence_test_runner", CONVERGE_TOOL)
worker_logs = load_module("convergence_worker_logs", REPO_ROOT / "scripts/lib/worker_log.py")


def fake_catalog(*, fail_entry: bool = False) -> str:
    entry_command = "exit 7" if fail_entry else "printf entry-ok"
    return textwrap.dedent(
        f"""\
        # Author: Lukas Bower
        # Purpose: Define fake convergence actions for runner tests.
        # Copyright 2026 Lukas Bower

        [catalog]
        schema = "cohesix-test-plan-actions/v2"
        minimum_python = "3.11"
        default_claims = ["common-hermetic"]
        claim_tiers = ["common-hermetic", "qemu-integration", "release"]

        [[convergence_focus]]
        id = "root-mcs"
        targets = ["qemu"]
        description = "Fake root target focus."
        profile = "fake-qemu-profile"
        authoritative_evidence = "fake target observation"
        priority = 10
        trigger_paths = ["apps/root-task/**"]

        [[action]]
        id = "accept.stage1"
        stage = 1
        tier = "common-hermetic"
        scope = "common"
        targets = ["qemu"]
        description = "Fake acceptance stage one."
        command = "printf acceptance-one"
        timeout_seconds = 30
        trigger_paths = ["**"]
        expected_evidence = ["command-result"]
        test_policy = "none"

        [[action]]
        id = "accept.stage2"
        stage = 2
        tier = "qemu-integration"
        scope = "provisioned-target"
        targets = ["qemu"]
        description = "Fake acceptance stage two."
        command = "printf acceptance-two"
        timeout_seconds = 30
        trigger_paths = ["**"]
        expected_evidence = ["command-result"]
        test_policy = "none"

        [[action]]
        id = "accept.stage3"
        stage = 3
        tier = "qemu-integration"
        scope = "target"
        targets = ["qemu"]
        description = "Fake acceptance stage three."
        command = "printf acceptance-three"
        timeout_seconds = 30
        trigger_paths = ["**"]
        expected_evidence = ["command-result"]
        test_policy = "none"

        [[action]]
        id = "accept.stage4"
        stage = 4
        tier = "qemu-integration"
        scope = "target"
        targets = ["qemu"]
        description = "Fake acceptance stage four."
        command = "printf acceptance-four"
        timeout_seconds = 30
        trigger_paths = ["**"]
        expected_evidence = ["command-result"]
        test_policy = "none"

        [[action]]
        id = "accept.stage5"
        stage = 5
        tier = "release"
        scope = "target"
        targets = ["qemu"]
        description = "Fake acceptance stage five."
        command = "printf acceptance-five"
        timeout_seconds = 30
        trigger_paths = ["**"]
        expected_evidence = ["command-result"]
        test_policy = "none"

        [[action]]
        id = "diagnostic.entry"
        stage = 0
        tier = "non-claiming"
        evidence_class = "diagnostic"
        scope = "conditional"
        targets = ["qemu"]
        description = "Fake target-entry action."
        command = "{entry_command}"
        timeout_seconds = 30
        trigger_paths = ["apps/root-task/**"]
        expected_evidence = ["command-result"]
        test_policy = "none"
        convergence_focuses = ["root-mcs"]
        convergence_phase = "target-entry"

        [[action]]
        id = "diagnostic.canary"
        stage = 0
        tier = "non-claiming"
        evidence_class = "diagnostic"
        scope = "conditional"
        targets = ["qemu"]
        description = "Write one fake target observation."
        command = '''printf '%s\n' '{{"schema":"cohesix-target-observation/v2","claiming":false,"result":"PASS"}}' > "$TEST_PLAN_TARGET_OBSERVATION"'''
        timeout_seconds = 30
        trigger_paths = ["apps/root-task/**"]
        expected_evidence = ["non-claiming-target-observation"]
        test_policy = "none"
        convergence_focuses = ["root-mcs"]
        convergence_phase = "target-canary"

        [[action]]
        id = "diagnostic.guard"
        stage = 0
        tier = "non-claiming"
        evidence_class = "diagnostic"
        scope = "conditional"
        targets = ["qemu"]
        description = "Run one fake focused regression."
        command = "printf 'test result: ok. 1 passed\\n'"
        timeout_seconds = 30
        trigger_paths = ["apps/root-task/**"]
        expected_evidence = ["focused-regression-result"]
        test_policy = "nonzero"
        minimum_test_count = 1
        convergence_focuses = ["root-mcs"]
        convergence_phase = "focused-regression"
        """
    )


class ConvergenceSelectionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.data = catalog.load_catalog(CATALOG_PATH)

    def selected(self, target: str, path: str) -> str:
        focus, _ = catalog.select_convergence_focus(
            self.data,
            target=target,
            changed_paths=[path],
        )
        return focus["id"]

    def test_representative_changed_paths_select_first_authority(self) -> None:
        self.assertEqual(
            self.selected("qemu", "apps/root-task/src/kernel.rs"),
            "root-mcs",
        )
        self.assertEqual(
            self.selected("pi4", "apps/pi4-driver-runtime/src/lib.rs"),
            "pi4-driver",
        )
        self.assertEqual(
            self.selected("qemu", "tools/cohesix-py/cohesix/client.py"),
            "python-sdk",
        )
        self.assertEqual(self.selected("qemu", "docs/SECURITY.md"), "docs")

    def test_pi_driver_change_rejects_qemu_as_first_authority(self) -> None:
        with self.assertRaisesRegex(
            catalog.CatalogError,
            "requires target pi4",
        ):
            catalog.select_convergence_focus(
                self.data,
                target="qemu",
                changed_paths=["apps/pi4-driver-runtime/src/lib.rs"],
            )

    def test_root_plan_runs_canary_before_focused_guard(self) -> None:
        actions = catalog.convergence_actions(
            self.data,
            target="qemu",
            focus_id="root-mcs",
        )
        phases = [action["convergence_phase"] for action in actions]
        self.assertLess(phases.index("target-canary"), phases.index("focused-regression"))
        self.assertNotIn("host.workspace-tests", [action["id"] for action in actions])

    def test_qemu_canary_forwards_selected_qemu_binary(self) -> None:
        source = TARGET_CANARY.read_text(encoding="utf-8")
        self.assertIn(
            'local qemu_bin="${TEST_PLAN_CONVERGENCE_QEMU_BIN:-${QEMU_BIN:-qemu-system-aarch64}}"',
            source,
        )
        self.assertIn('--qemu "${qemu_bin}"', source)

    def test_retained_canary_uses_selected_build_and_record_profile(self) -> None:
        """Retained HVF and KVM launches keep their own build and profile label."""
        source = TARGET_CANARY.read_text(encoding="utf-8")
        function = "qemu_canary() {" + source.split("qemu_canary() {", 1)[1].split(
            "\n}\n", 1,
        )[0] + "\n}\n"
        for selected in ("qemu_smp_production", "qemu_smp_kvm_production"):
            with self.subTest(profile=selected), tempfile.TemporaryDirectory() as raw:
                root = Path(raw)
                artifacts = root / "retained image"
                artifacts.mkdir()
                (artifacts / "cohesix-system.cpio").write_bytes(b"fixture")
                (artifacts / "cohesix-qemu-launch-artifacts.json").write_text(
                    json.dumps({"sel4_profile": selected}), encoding="utf-8",
                )
                build = root / "scripts/cohesix-build-run.sh"
                build.parent.mkdir()
                build.write_text(
                    f"#!{sys.executable}\n"
                    "import json, pathlib, sys\n"
                    "with pathlib.Path(__file__).with_suffix('.calls').open('a') as f:\n"
                    "    f.write(json.dumps(sys.argv[1:]) + '\\n')\n",
                    encoding="utf-8",
                )
                build.chmod(0o700)
                result = subprocess.run(
                    ["bash", "-eu", "-c", function + '''
repo_root="$1"
state_dir="$1"
focus=ninedoor
SEL4_BUILD_DIR="$1/selected sel4"
TEST_PLAN_CONVERGENCE_LAUNCH_EXISTING=1
TEST_PLAN_CONVERGENCE_QEMU_OUT_DIR="$1/retained image"
COHSH_AUTH_TOKEN=provisioned-fixture
wait_for_marker() { return 0; }
wait_for_port() { return 0; }
cohsh_binary() { printf '%s' "$1"; }
run_live_operation() { test "$4" = provisioned-fixture; }
unexpected_faults() { return 1; }
stop_qemu() { wait "$qemu_pid"; }
write_observation() { printf '%s\\n' "$profile"; }
qemu_canary
''', "retained-canary", str(root)],
                    check=True, capture_output=True, text=True, timeout=10,
                )
                self.assertEqual(
                    result.stdout.strip(), selected + " / immutable launch record",
                )
                calls = [json.loads(line) for line in build.with_suffix(
                    ".calls",
                ).read_text(encoding="utf-8").splitlines()]
                self.assertEqual(len(calls), 2)
                for call in calls:
                    self.assertEqual(
                        call[call.index("--sel4-build") + 1],
                        str(root / "selected sel4"),
                    )
                    self.assertIn("--launch-existing", call)

    def test_worker_operation_approves_each_governed_lifecycle_write(self) -> None:
        """The selected single-use gate needs a fresh approval per mutation."""
        manifest = tomllib.loads(
            (REPO_ROOT / "configs" / "root_task.toml").read_text(encoding="utf-8")
        )
        policy = manifest["ecosystem"]["policy"]
        self.assertTrue(policy["enable"])
        self.assertIn(
            {"id": "queen-ctl", "target": "/queen/ctl"}, policy["rules"]
        )
        source = (REPO_ROOT / "scripts" / "cohsh" / "converge_worker.coh").read_text(
            encoding="utf-8"
        )
        commands = [
            words
            for line in source.splitlines()
            if (words := shlex.split(line, comments=True)) and words[0] != "EXPECT"
        ]
        mutations = [
            (index, words)
            for index, words in enumerate(commands)
            if words[0] in {"spawn", "kill"}
        ]
        self.assertEqual([words[0] for _, words in mutations], ["spawn", "kill", "spawn"])
        approval_ids: set[str] = set()
        for index, mutation in mutations:
            with self.subTest(mutation=mutation, command_index=index):
                self.assertGreater(index, 0)
                approval_command = commands[index - 1]
                self.assertEqual(approval_command[0], "echo")
                self.assertEqual(approval_command[2:], [">", "/actions/queue"])
                approval = json.loads(approval_command[1])
                self.assertEqual(approval["target"], "/queen/ctl")
                self.assertEqual(approval["decision"], "approve")
                self.assertIsInstance(approval["id"], str)
                self.assertTrue(approval["id"])
                self.assertNotIn(approval["id"], approval_ids)
                approval_ids.add(approval["id"])

    def test_worker_operation_observes_lifecycle_before_dependent_mutations(self) -> None:
        """Each asynchronous phase uses bounded target data before proceeding."""
        source = (REPO_ROOT / "scripts/cohsh/converge_worker.coh").read_text()
        lines = [
            line.strip() for line in source.splitlines()
            if line.strip() and not line.startswith("#")
        ]
        manifest = tomllib.loads((REPO_ROOT / "configs/root_task.toml").read_text())
        self.assertEqual(manifest["sharding"]["shard_bits"], 8)
        mutations = [
            index for index, line in enumerate(lines)
            if line.split()[0] in {"spawn", "kill"}
        ]
        phases = zip(
            mutations, ("worker-1", "worker-1", "worker-2"),
            ("ready", "terminal", "ready"),
        )
        for index, worker_id, state in phases:
            shard = hashlib.sha256(worker_id.encode()).hexdigest()[:2]
            path = f"/shard/{shard}/worker/{worker_id}/telemetry"
            if state == "terminal":
                expected = f'WAIT 2000 TAIL {path} SUBSTR "state":"terminal"'
                self.assertEqual(
                    lines[index + 1:index + 4],
                    ["EXPECT OK", expected, "EXPECT OK"],
                )
            else:
                generation = 1 if worker_id == "worker-1" else 2
                # Every executable slot is constructed before public admission;
                # the supervisor generation is global, while lease/cap are per slot.
                resource_admission = manifest["worker_resource_admission"]
                population = sum(
                    role["executable_slots"]
                    for role in resource_admission["executable_roles"]
                )
                supervisor = 1 if worker_id == "worker-1" else population + 1
                expected = (
                    "WAIT 2000 TAIL /log/queen.log SUBSTR WORKER_TASK_READY "
                    f"role=worker-heartbeat slot=0 lease_epoch={generation} "
                    f"supervisor_generation={supervisor} cap_generation={generation}"
                )
                self.assertEqual(
                    lines[index + 1:index + 6],
                    ["EXPECT OK", expected, "EXPECT OK", f"tail {path}", "EXPECT OK"],
                )
        self.assertEqual(lines.count("cat /log/queen.log"), 2)


class WorkerRestartEvidenceTests(unittest.TestCase):
    @staticmethod
    def transcript(
        *, second_generation: int = 2, contained: bool = True,
        reason: str = "shutdown",
    ) -> str:
        """Independent protocol fixture contains fragmented target record bytes."""
        first = (
            "role=worker-heartbeat slot=0 lease_epoch=1 "
            "supervisor_generation=1 cap_generation=1"
        )
        second = (
            f"role=worker-heartbeat slot=0 lease_epoch={second_generation} "
            f"supervisor_generation={second_generation} "
            f"cap_generation={second_generation}"
        )
        messages = [
            f"WORKER_TASK_READY {first} sequence=1",
            f"WORKER_TASK_TEARDOWN {first} reason={reason} tcb_suspended=yes "
            "records_cleared=yes scheduling_context_unbound=yes "
            "mappings_scrubbed=yes descendants_revoked=yes objects_deleted=yes "
            f"generation_fenced={'yes' if contained else 'no'} state=terminal",
            f"WORKER_TASK_READY {second} sequence=1",
        ]
        fragments = []
        for identity, message in enumerate(messages, 1):
            parts = [message[index:index + 120] for index in range(0, len(message), 120)]
            fragments.append("\n".join(
                f"WORKER_LOG id={identity} part={part} "
                f"last={int(part == len(parts) - 1)} data={data}"
                for part, data in enumerate(parts)
            ))

        def telemetry(worker_id: str, state: str, generation: int) -> str:
            return json.dumps({
                "schema": "worker-runtime-state/v2", "worker_id": worker_id,
                "role": "worker-heartbeat", "state": state,
                "identity": [0, generation, generation, generation],
                "sequence": [1, 0, 0, 0],
            })

        return "\n".join([
            "[cohsh][tcp] remote NineDoor ready as role Queen", "[console] OK AUTH",
            "[console] OK ATTACH role=queen",
            "[console] OK TAIL path=/shard/13/worker/worker-1/telemetry",
            telemetry("worker-1", "ready", 1),
            "[console] OK CAT path=/log/queen.log lines=2",
            fragments[0], "[console] OK ECHO path=/queen/ctl bytes=19",
            "[console] OK TAIL path=/shard/13/worker/worker-1/telemetry",
            telemetry("worker-1", "terminal", 1),
            "[console] OK TAIL path=/shard/1c/worker/worker-2/telemetry",
            telemetry("worker-2", "ready", second_generation),
            "[console] OK CAT path=/log/queen.log lines=8", *fragments,
            "[console] OK QUIT", "closing session",
        ])

    def test_completed_exports_deduplicate_identical_fragments(self) -> None:
        proof = worker_logs.validate_restart(self.transcript())
        self.assertEqual(
            [row["supervisor_generation"] for row in proof["ready_generations"]],
            [1, 2],
        )

    def test_repeated_generation_is_not_restart(self) -> None:
        with self.assertRaisesRegex(ValueError, "generation tuple"):
            worker_logs.validate_restart(self.transcript(second_generation=1))

    def test_incomplete_containment_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "containment"):
            worker_logs.validate_restart(self.transcript(contained=False))

    def test_fault_containment_cannot_replace_requested_shutdown(self) -> None:
        with self.assertRaisesRegex(ValueError, "containment"):
            worker_logs.validate_restart(self.transcript(reason="fault"))

    def test_acknowledgements_and_uart_text_are_not_worker_records(self) -> None:
        text = self.transcript().replace("WORKER_LOG id=", "UART WORKER_LOG id=")
        with self.assertRaisesRegex(ValueError, "two actual READY"):
            worker_logs.validate_restart(text)

    def test_missing_fragment_is_not_completed_proof(self) -> None:
        text = "\n".join(
            line for line in self.transcript().splitlines()
            if "id=2 part=1 " not in line
        )
        with self.assertRaisesRegex(ValueError, "incomplete"):
            worker_logs.validate_restart(text)

    def test_wrong_telemetry_identity_or_shard_is_rejected(self) -> None:
        for text in (
            self.transcript().replace('"worker_id": "worker-2"', '"worker_id": "worker-1"'),
            self.transcript().replace("/shard/1c/worker/worker-2", "/shard/13/worker/worker-2"),
            self.transcript().replace('"identity": [0, 2, 2, 2]', '"identity": [0, 3, 3, 3]'),
        ):
            with self.subTest(text=text), self.assertRaises(ValueError):
                worker_logs.validate_restart(text)

    def test_failed_or_unauthenticated_export_is_rejected(self) -> None:
        for text in (
            self.transcript().replace("[console] OK AUTH", "[console] ERR AUTH"),
            self.transcript().replace("[console] OK AUTH", "unrelated text"),
            self.transcript().replace("[console] OK QUIT", "missing terminal response"),
        ):
            with self.subTest(text=text), self.assertRaises(ValueError):
                worker_logs.validate_restart(text)


class ConvergenceEvidenceTests(unittest.TestCase):
    def invoke(self, catalog_text: str):
        out_root = REPO_ROOT / "out"
        out_root.mkdir(exist_ok=True)
        temporary = tempfile.TemporaryDirectory(dir=out_root)
        parent = Path(temporary.name)
        catalog_path = parent / "catalog.toml"
        state = parent / "state"
        catalog_path.write_text(catalog_text, encoding="utf-8")
        result = subprocess.run(
            [
                sys.executable,
                str(CONVERGE_TOOL),
                "--catalog",
                str(catalog_path),
                "--target",
                "qemu",
                "--focus",
                "root-mcs",
                "--path",
                "apps/root-task/src/kernel.rs",
                "--state-dir",
                str(state),
            ],
            cwd=REPO_ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        return temporary, state, result

    def test_run_needs_no_acceptance_attestation_and_is_non_claiming(self) -> None:
        temporary, state, result = self.invoke(fake_catalog())
        self.addCleanup(temporary.cleanup)
        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(
            (state / "convergence-result.json").read_text(encoding="utf-8")
        )
        self.assertEqual(payload["schema"], converge.RESULT_SCHEMA)
        self.assertFalse(payload["claiming"])
        self.assertFalse(payload["promotion_eligible"])
        self.assertEqual(payload["result"], "PASS")
        self.assertEqual(
            [action["id"] for action in payload["actions"]],
            ["diagnostic.entry", "diagnostic.canary", "diagnostic.guard"],
        )
        self.assertIn(converge.BANNER, result.stdout)
        self.assertFalse(list(state.glob("stage_*.done")))
        self.assertNotIn("cohesix.test-plan-stage", json.dumps(payload))
        self.assertIn("git_commit", payload["source"])
        self.assertIn("source_digest", payload["source"])
        self.assertIn("uart_serial_log", payload)
        self.assertIn("built_image", payload)
        self.assertIn("image_identity", payload)

    def test_first_failure_stops_before_target_and_guard(self) -> None:
        temporary, state, result = self.invoke(fake_catalog(fail_entry=True))
        self.addCleanup(temporary.cleanup)
        self.assertEqual(result.returncode, 1, result.stderr)
        payload = json.loads(
            (state / "convergence-result.json").read_text(encoding="utf-8")
        )
        self.assertEqual(payload["result"], "FAIL")
        self.assertEqual(payload["first_failing_proof_layer"], "target-entry")
        self.assertEqual(len(payload["actions"]), 1)

    def test_tampered_candidate_result_is_rejected(self) -> None:
        temporary, state, result = self.invoke(fake_catalog())
        self.addCleanup(temporary.cleanup)
        self.assertEqual(result.returncode, 0, result.stderr)
        result_path = state / "convergence-result.json"
        payload = json.loads(result_path.read_text(encoding="utf-8"))
        payload["promotion_eligible"] = True
        result_path.write_text(json.dumps(payload), encoding="utf-8")
        with self.assertRaisesRegex(
            converge.ConvergenceError,
            "promotion_eligible=false",
        ):
            converge.validate_result(result_path)

    def test_pi_canary_without_hardware_inputs_is_blocked(self) -> None:
        out_root = REPO_ROOT / "out"
        out_root.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=out_root) as temporary:
            state = Path(temporary)
            observation = state / "target-observation.json"
            environment = os.environ.copy()
            environment.update(
                {
                    "TEST_PLAN_CONVERGENCE": "1",
                    "TEST_PLAN_CONVERGENCE_FOCUS": "pi4-driver",
                    "TEST_PLAN_CONVERGENCE_RUN_ID": "blocked-test",
                    "TEST_PLAN_CONVERGENCE_STATE_DIR": str(state),
                    "TEST_PLAN_TARGET_OBSERVATION": str(observation),
                }
            )
            result = subprocess.run(
                ["bash", str(TARGET_CANARY), "--target", "pi4"],
                cwd=REPO_ROOT,
                env=environment,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

            self.assertEqual(result.returncode, 3, result.stderr)
            payload = json.loads(observation.read_text(encoding="utf-8"))
            self.assertEqual(payload["result"], "BLOCKED")
            self.assertFalse(payload["claiming"])
            # Non-Worker consumers retain the exact v2 observation shape.
            self.assertNotIn("worker_restart_evidence", payload)


if __name__ == "__main__":
    unittest.main()
