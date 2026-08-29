#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Test structural Milestone 26e implementation-surface drift detection.
# Copyright 2026 Lukas Bower

"""Unit tests for check_implementation_surfaces.py."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest


SCRIPT = Path(__file__).with_name("check_implementation_surfaces.py")
SPEC = importlib.util.spec_from_file_location("check_implementation_surfaces", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
SURFACES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SURFACES)


def classification(
    record_id: str,
    implementation_class: str = "production_live",
    production_reachable: bool = True,
) -> dict[str, object]:
    return {
        "id": record_id,
        "kind": "test",
        "path": "apps/worker-bus" if implementation_class == "model_only" else "test",
        "implementation_class": implementation_class,
        "owner": "test-owner",
        "milestone": "Milestone 26e",
        "production_reachable": production_reachable,
        "selection_source": "test",
        "package_disposition": "test",
        "evidence_requirement": "test",
        "current_observed_mode": "test",
        "evidence_eligible": implementation_class == "production_live",
    }


class ImplementationSurfaceGuardTests(unittest.TestCase):
    def test_rejects_production_reachable_fixture(self) -> None:
        payload = {
            "schema": SURFACES.SCHEMA,
            "packages": [
                {
                    **classification("workspace:fixture", "fixture", True),
                    "targets": [],
                    "features": [],
                }
            ],
            "surfaces": [classification("role:worker-bus", "model_only", False)],
            "tracked_surfaces": [],
        }
        errors = SURFACES.validate_inventory_payload(payload)
        self.assertTrue(any("production-reachable" in error for error in errors))

    def test_detects_spin_only_entrypoint_but_not_service_loop(self) -> None:
        spin = "fn target_entry() -> ! { loop { core::hint::spin_loop(); } }"
        service = "fn target_entry() -> ! { loop { receive_and_reply(); } }"
        self.assertTrue(SURFACES.detect_compiled_spin_stub(spin, "target_entry"))
        self.assertFalse(SURFACES.detect_compiled_spin_stub(service, "target_entry"))

    def test_detects_provider_whose_entire_body_is_noop_success(self) -> None:
        source = "fn publish_snapshot() -> Result<()> { Ok(()) }"
        self.assertEqual(
            SURFACES.detect_noop_success_provider(source), ["publish_snapshot"]
        )

    def test_gpu_fixture_gate_is_qemu_bootstrap_trace_exact(self) -> None:
        exact = '''
fn parse(source_mode: &str) {
    gpu_snapshot_mode_allowed(
        source_mode,
        cfg!(all(feature = "bootstrap-trace", feature = "release-qemu")),
    );
}
fn gpu_snapshot_mode_allowed(source_mode: &str, qemu_evidence_gate: bool) -> bool {
    source_mode == "production" || (qemu_evidence_gate && source_mode == "fixture")
}
fn qemu_lora_export_fixture_allowed(
    source_mode: &str,
    qemu_evidence_gate: bool,
    snapshot_live: bool,
) -> bool {
    source_mode == "fixture" && qemu_evidence_gate && snapshot_live
}
const TELEMETRY: &str = "telemetry.cbor";
const BASE: &str = "base_model.ref";
const POLICY: &str = "policy.toml";
const MARKER: &str = "LORA_EXPORT_FIXTURE_ADMISSION";
'''
        self.assertTrue(SURFACES.root_ninedoor_gpu_mode_gate_is_strict(exact))

        broad_fixture = exact.replace(
            'source_mode == "production" || '
            '(qemu_evidence_gate && source_mode == "fixture")',
            'source_mode == "production" || qemu_evidence_gate',
        )
        self.assertFalse(
            SURFACES.root_ninedoor_gpu_mode_gate_is_strict(broad_fixture)
        )

        ungated_fixture = exact.replace(
            'cfg!(all(feature = "bootstrap-trace", feature = "release-qemu"))',
            "true",
        )
        self.assertFalse(
            SURFACES.root_ninedoor_gpu_mode_gate_is_strict(ungated_fixture)
        )

        mock_allowed = exact.replace(
            'source_mode == "fixture")',
            'source_mode == "fixture" || source_mode == "mock")',
        )
        self.assertFalse(SURFACES.root_ninedoor_gpu_mode_gate_is_strict(mock_allowed))

        lora_production = exact.replace(
            'source_mode == "fixture" && qemu_evidence_gate && snapshot_live',
            'source_mode != "mock" && qemu_evidence_gate && snapshot_live',
        )
        self.assertFalse(
            SURFACES.root_ninedoor_gpu_mode_gate_is_strict(lora_production)
        )

    def test_contract_cannot_be_marked_evidence_eligible(self) -> None:
        payload = {
            "schema": SURFACES.SCHEMA,
            "packages": [],
            "surfaces": [
                {
                    **classification("contract:x", "contract", True),
                    "evidence_eligible": True,
                },
                classification("role:worker-bus", "model_only", False),
            ],
            "tracked_surfaces": [],
        }
        errors = SURFACES.validate_inventory_payload(payload)
        self.assertTrue(any("evidence_eligible" in error for error in errors))

    def test_release_guard_rejects_expected_file_set_drift(self) -> None:
        release = {
            "schema": "cohesix-runtime-release-manifest/v2",
            "version": "0.1.0-alpha1",
            "host_tools": ["bin/coh"],
            "target_images": ["image/kernel.elf"],
            "pi4_stage_files": ["config.txt"],
            "generated_configs": ["configs/generated/root_task_resolved.json"],
            "public_documents": ["docs/QUICKSTART.md"],
            "host_assets": ["resources/keys/cas_verification_key.hex"],
            "operator_scripts": ["scripts/cohsh/boot_v0.coh"],
            "python_artifacts": ["tools/cohesix-py/cohesix/client.py"],
            "cas_fixtures": ["tests/fixtures/cas/max_chunks_v1.txt"],
            "trace_fixtures": ["tests/fixtures/traces/trace_v0.trace"],
            "transcript_fixtures": [
                "tests/fixtures/transcripts/boot_v0/core.txt"
            ],
            "ui_assets": ["apps/swarmui/frontend/index.html"],
            "support_files": [
                "releases/RELEASE_NOTES-0.1.0-alpha1.md"
            ],
            "versioned_migrations": [],
            "generated_bundle_files": ["MANIFEST.sha256"],
            "pi4_generated_bundle_files": [
                "MANIFEST.sha256",
                "VERSION.txt",
                "image/cohesix-pi4-sd.img",
            ],
            "forbidden_paths": [
                "bin/coh-status",
                "bin/nine-door",
                "crates/domain-intents",
                "resources/fixtures/cas_signing_key.hex",
            ],
            "expected_bundle_files": [],
            "expected_pi4_bundle_files": [],
            "asset_records": [],
        }
        errors = SURFACES.validate_release_manifest({"release": release})
        self.assertTrue(any("expected_bundle_files drift" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
