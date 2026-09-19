# Author: Lukas Bower
# Purpose: Preserve independent CI identity, retry, bounded wait and non-success outcome contracts without native execution claims.
# Copyright 2026 Lukas Bower
"""Pure CI controller contracts. Signed native verification remains owned by coh."""

from copy import deepcopy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from cohesix import journey
from cohesix.providers import ProviderUnavailable


class JourneyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.state = self.root / "state"
        self.state.mkdir(mode=0o700)
        self.deployment = {
            "schema": "cohesix-peft-deployment/v1",
            "journal": "",
            "request": {
                "schema": "cohesix-peft-release/v1",
                "operation_id": "unissued",
                "model_id": "test-model",
                "entry": "train",
                "input_sha256": "a" * 64,
                "profile_sha256": "b" * 64,
                "baseline": {"generation": 3},
                "evaluation_policy": {"minimum_samples": 16},
            },
            "execution": {
                "graph": "/evidence/graph.json",
                "trust": "/private/trust.json",
                "cas": "/evidence/cas",
                "request": {"id": "ticket-1"},
            },
        }
        self.operation = journey.identity(
            "lora", "release-model", "initial", self.deployment
        )
        self.deployment["request"]["operation_id"] = self.operation
        self.deployment["journal"] = str(self.state / self.operation / "journal")
        self.config = {
            "schema": "cohesix-journey/v1",
            "kind": "lora",
            "purpose": "release-model",
            "rerun": "initial",
            "coh": "/installed/coh",
            "deployment": str(self.root / "deployment.json"),
            "state": str(self.state),
        }
        self.config_path = self.root / "config.json"
        self.write()

    def write(self) -> None:
        Path(self.config["deployment"]).write_bytes(journey.encode(self.deployment))
        self.config_path.write_bytes(journey.encode(self.config))

    def report(
        self, *, submitted: bool = False, acknowledged: bool = False, result=None
    ) -> dict:
        return {
            "schema": "cohesix-peft-report/v1",
            "operation_id": self.operation,
            "authoritative": False,
            "production_use_case_accepted": False,
            "submitted": submitted,
            "acknowledged": acknowledged,
            "result": result,
        }

    def test_identity_ignores_runner_and_ticket_but_binds_immutable_request(
        self,
    ) -> None:
        retry = deepcopy(self.deployment)
        retry["execution"]["request"]["id"] = "new-ticket-refused-by-binding"
        self.assertEqual(
            journey.identity("lora", "release-model", "initial", retry), self.operation
        )
        for field, value in [
            ("input_sha256", "c" * 64),
            ("baseline", {"generation": 4}),
            ("evaluation_policy", {"minimum_samples": 17}),
            ("entry", "import"),
        ]:
            changed = deepcopy(retry)
            changed["request"][field] = value
            self.assertNotEqual(
                journey.identity("lora", "release-model", "initial", changed),
                self.operation,
            )
        self.assertNotEqual(
            journey.identity("lora", "release-model", "approved-rerun", retry),
            self.operation,
        )
        self.assertNotEqual(
            journey.identity("lora", "different-purpose", "initial", retry),
            self.operation,
        )

    def test_cuda_identity_binds_bytes_runtime_and_dependencies(self) -> None:
        source = self.root / "input.json"
        source.write_text('{"workload":"vector_add"}')
        deployment = {
            "contract_sha256": "d" * 64,
            "topology": {"provider-host": "gpu-1"},
            "stages": [
                {
                    "id": "vector",
                    "input": str(source),
                    "after": [],
                    "runtime": {"driver_version": 13020, "runtime_version": 13020},
                }
            ],
        }
        identity = journey.identity("cuda", "vectors", "initial", deployment)
        source.write_text('{"workload":"matrix_multiply"}')
        self.assertNotEqual(
            journey.identity("cuda", "vectors", "initial", deployment), identity
        )
        source.write_text('{"workload":"vector_add"}')
        deployment["stages"][0]["runtime"]["driver_version"] = 13030
        self.assertNotEqual(
            journey.identity("cuda", "vectors", "initial", deployment), identity
        )

    def test_outcomes_never_promote_ack_or_rollback_to_success(self) -> None:
        self.assertEqual(
            journey.classify("lora", self.report(), self.operation), "pending"
        )
        self.assertEqual(
            journey.classify("lora", self.report(submitted=True), self.operation),
            "ambiguous",
        )
        self.assertEqual(
            journey.classify(
                "lora", self.report(submitted=True, acknowledged=True), self.operation
            ),
            "running",
        )
        for native, expected in [
            ("succeeded", "verified"),
            ("recovered_failure", "recovered_failure"),
            ("failed", "failed"),
            ("rollback_failed", "failed"),
            ("cancelled", "cancelled"),
        ]:
            report = self.report(result={"state": native, "graph_sha256": "e" * 64})
            self.assertEqual(journey.classify("lora", report, self.operation), expected)
            self.assertEqual(journey.EXIT[expected] == 0, native == "succeeded")
        bad = self.report(result={"state": "succeeded"})
        self.assertEqual(
            journey.classify("lora", bad, self.operation), "invalid_evidence"
        )

    def test_cuda_success_requires_all_requested_outputs(self) -> None:
        report = {
            "schema": "cohesix-recipe-operation-report/v1",
            "operation_id": self.operation,
            "authoritative": False,
            "production_use_case_accepted": False,
            "all_steps_verified": True,
            "stages": [{"verified": True, "output": {"sha256": "a" * 64}}],
            "attempts": [],
        }
        self.assertEqual(journey.classify("cuda", report, self.operation), "verified")
        report["stages"][0]["output"] = None
        self.assertEqual(
            journey.classify("cuda", report, self.operation), "invalid_evidence"
        )
        report["all_steps_verified"] = False
        report["attempts"] = [
            {
                "state": "verified_terminal",
                "cancel": False,
                "evidence": {"state": "cancelled", "output": None},
            }
        ]
        self.assertEqual(journey.classify("cuda", report, self.operation), "cancelled")

    def fake(self, config: dict, mode: str) -> dict:
        journal = Path(self.deployment["journal"])
        if mode == "plan":
            journal.mkdir(mode=0o700, exist_ok=True)
            (journal / "recipe.json").write_text("{}")
        return self.report()

    def test_changed_admission_and_lost_journal_are_refused_on_retry(self) -> None:
        with patch.object(journey, "call", side_effect=self.fake):
            self.assertEqual(journey.run(self.config_path)["state"], "pending")
            self.deployment["execution"]["request"]["id"] = "different"
            self.write()
            with self.assertRaisesRegex(ValueError, "changed admission"):
                journey.run(self.config_path, submit=True)
            self.deployment["execution"]["request"]["id"] = "ticket-1"
            self.write()
            (Path(self.deployment["journal"]) / "recipe.json").unlink()
            with self.assertRaisesRegex(ValueError, "lost durable journal"):
                journey.run(self.config_path, submit=True)

    def test_lost_ack_new_runner_reconciles_without_second_dispatch(self) -> None:
        submitted = False
        dispatches = []

        def native(config: dict, mode: str) -> dict:
            nonlocal submitted
            if mode == "plan":
                self.fake(config, mode)
            if mode == "apply":
                submitted = True
                dispatches.append(mode)
                raise ProviderUnavailable("timeout", "native_command")
            return self.report(submitted=submitted)

        with patch.object(journey, "call", side_effect=native):
            first = journey.run(self.config_path, submit=True)
            second = journey.run(self.config_path, submit=True)
        self.assertEqual(first["state"], "ambiguous")
        self.assertEqual(second["state"], "ambiguous")
        self.assertEqual(dispatches, ["apply"])
        self.assertEqual(first["operation_id"], second["operation_id"])
        self.assertTrue((self.state / self.operation / "outcome.json").is_file())

    def test_verified_report_is_rechecked_and_corrupt_evidence_fails(self) -> None:
        def native(config: dict, mode: str) -> dict:
            if mode == "plan":
                self.fake(config, mode)
            if mode == "verify":
                raise ProviderUnavailable("probe_failed", "native_command")
            return self.report(result={"state": "succeeded", "graph_sha256": "e" * 64})

        with patch.object(journey, "call", side_effect=native) as call:
            result = journey.run(self.config_path)
        self.assertEqual(result["state"], "invalid_evidence")
        self.assertIn("verify", [item.args[1] for item in call.call_args_list])

    def test_bounded_wait_uses_injected_time_and_returns_timeout(self) -> None:
        now = [0]

        def sleep(seconds: float) -> None:
            now[0] += seconds

        with patch.object(journey, "call", side_effect=self.fake):
            result = journey.run(
                self.config_path, wait_seconds=5, clock=lambda: now[0], sleep=sleep
            )
        self.assertEqual(now[0], 5)
        self.assertEqual(result["exit_code"], 17)
        for invalid in [-1, 3601, True]:
            with self.assertRaises(ValueError):
                journey.run(self.config_path, wait_seconds=invalid)

    def test_private_state_and_strict_documents(self) -> None:
        self.state.chmod(0o755)
        with self.assertRaisesRegex(ValueError, "private"):
            journey.run(self.config_path)
        source = self.root / "duplicate.json"
        source.write_text('{"a":1,"a":2}')
        with self.assertRaises(ValueError):
            journey.document(source)
        source.write_text('{"a":NaN}')
        with self.assertRaises(ValueError):
            journey.document(source)
        source.unlink()
        source.symlink_to(self.config_path)
        with self.assertRaises(ValueError):
            journey.document(source)

    def test_apply_resolves_fresh_authority_on_each_continuation(self) -> None:
        config = dict(
            self.config,
            host="127.0.0.1",
            auth_ref="env:AUTH",
            ticket_ref="file:/private/ticket",
        )
        with patch.object(
            journey, "resolve_secret_reference", return_value="private"
        ) as resolve:
            with patch.object(journey, "run_peft_release", return_value=self.report()):
                journey.call(config, "apply")
                journey.call(config, "apply")
        self.assertEqual(resolve.call_count, 4)
        with patch.object(
            journey, "resolve_secret_reference", side_effect=ValueError("expired")
        ):
            with self.assertRaises(ValueError):
                journey.call(config, "apply")

    def test_doctor_preserves_remote_uncertainty_and_actionable_refusals(self) -> None:
        self.config.update(
            package="/installed",
            trust="/private/package-trust.json",
            auth_ref="env:AUTH",
            ticket_ref="file:/private/ticket",
        )
        self.deployment["execution"]["request"]["expires_unix_ms"] = 20000
        self.write()
        with patch.object(journey, "bounded_command", return_value=b"{}"):
            with patch.object(journey, "call", side_effect=self.fake):
                with patch.object(journey.time, "time", return_value=10):
                    with patch.object(
                        journey, "resolve_secret_reference", return_value="private"
                    ):
                        report = journey.doctor(self.config_path)
                    with patch.object(
                        journey,
                        "resolve_secret_reference",
                        side_effect=ValueError("missing"),
                    ):
                        refused = journey.doctor(self.config_path)
        states = {row["component"]: row["status"] for row in report["checks"]}
        self.assertEqual(
            states,
            {
                "controller": "ready",
                "package-drift": "ready",
                "secrets-authority": "ready",
                "cache-evidence-bounds": "ready",
                "control-target": "not_observed",
                "cuda-executor": "not_observed",
                "native-runtime": "not_observed",
                "resource-bounds": "not_observed",
            },
        )
        self.assertFalse(report["ready_for_submission"])
        row = next(
            row for row in refused["checks"] if row["component"] == "secrets-authority"
        )
        self.assertEqual(row["status"], "unavailable")
        self.assertTrue(row["remediation"])


if __name__ == "__main__":
    unittest.main()
