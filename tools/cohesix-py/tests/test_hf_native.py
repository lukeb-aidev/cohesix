# Author: Lukas Bower
# Purpose: Preserve native artifact confinement and independently keyed source-attestation bindings without treating fixtures as CUDA acceptance.
# Copyright 2026 Lukas Bower
"""Deterministic native provider input tests; live HF execution is separate evidence."""
from __future__ import annotations

import json
import importlib.util
from pathlib import Path
import tempfile
import unittest

from cohesix.hf_native import Provider, Refused, encode, phase_lock, regular, sha, write


class NativeInputs(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name).resolve()
        (self.root / "objects").mkdir()
        (self.root / "bundles").mkdir()
        self.provider = Provider.__new__(Provider)
        self.provider.root = self.root

    def tearDown(self) -> None:
        self.directory.cleanup()

    def test_regular_cas_reads_reject_links_corruption_size_and_traversal(self) -> None:
        payload = b"independent input fixture"
        ref = self.provider.store(payload)
        self.assertEqual(self.provider.blob(ref, 64), payload)
        with self.assertRaises(Refused):
            self.provider.blob(ref, 4)
        with self.assertRaises(Refused):
            self.provider.blob("../input", 64)
        (self.root / "objects" / ref).write_bytes(b"changed")
        with self.assertRaises(Refused):
            self.provider.blob(ref, 64)
        (self.root / "link").symlink_to(self.root / "objects", target_is_directory=True)
        with self.assertRaises(Refused):
            regular(self.root / "link" / ref, 64)
        with self.assertRaises(Refused):
            regular(self.root / "objects" / ".." / "link" / ref, 64)
        write(self.root / "journal.json", encode({"intent": True}))
        self.assertEqual(json.loads(
            regular(self.root / "journal.json", 64)), {"intent": True})
        self.assertEqual((self.root / "journal.json").stat().st_mode & 0o777, 0o600)

    def test_bundle_rejects_pickle_and_path_injection_before_materializing_bytes(self) -> None:
        ref = self.provider.store(b"fixture")
        for name in ["weights.pkl", "pytorch_model.bin", "../adapter.json", "/tmp/model.json"]:
            manifest = {"files": {name: ref}, "source": "fixture",
                        "revision": "pinned", "license_ref": "Apache-2.0"}
            bundle = self.provider.store(encode(manifest))
            with self.assertRaises(Refused):
                self.provider.bundle(bundle)

    def test_compensation_fence_rejects_delayed_forward_execution(self) -> None:
        with phase_lock(self.root, False):
            with self.assertRaises(BlockingIOError):
                with phase_lock(self.root, True):
                    self.fail("concurrent native ownership")
        write(self.root / "forward-cancelled.json", encode({"cancelled": True}))
        with self.assertRaises(Refused):
            with phase_lock(self.root, False):
                self.fail("delayed forward helper ran after compensation")
        with phase_lock(self.root, True):
            self.assertTrue((self.root / "forward-cancelled.json").exists())

    def test_native_commit_deadline_cannot_extend_an_expired_grant(self) -> None:
        from unittest.mock import patch
        self.provider.operation = self.root
        self.provider.phase = "promote"
        self.provider.authority_expiry = 10001
        with patch("cohesix.hf_native.time.time", return_value=10.0):
            self.provider.current_authority()
            self.provider.authority_expiry = 10000
            with self.assertRaises(Refused):
                self.provider.current_authority()

    def test_full_state_checkpoint_binds_source_profile_files_and_data_position(self) -> None:
        from cohesix.hf_native import CHECKPOINT_FILES

        self.provider.config = {"profile_sha256": "11" * 32}
        self.provider.profile = {"settings": {"max_steps": 32}}
        self.provider.request = {"operation_id": "new-attempt"}
        self.provider.operation = self.root / "operations" / "new-attempt"
        self.provider.operation.mkdir(parents=True)
        source = self.root / "operations" / "interrupted-attempt"
        source.mkdir()
        source_request = {"profile_sha256": "11" * 32, "input_sha256": "22" * 32,
                          "entry": "train", "operation_id": "interrupted-attempt"}
        request_ref = self.provider.store(encode(source_request))
        files = {name: self.provider.store(encode({"global_step": 8}) if name == "trainer_state.json"
                                           else b"native-checkpoint-fixture:" + name.encode())
                 for name in CHECKPOINT_FILES}
        manifest = {"schema": "cohesix-peft-checkpoint/v1", "source_operation": "interrupted-attempt",
                    "request_sha256": request_ref, "profile_sha256": "11" * 32,
                    "input_sha256": "22" * 32, "step": 8, "files": files}
        reference = self.provider.store(encode(manifest))
        write(source / "checkpoint.json", encode({"checkpoint_sha256": reference, "step": 8}))
        path, observed = self.provider.checkpoint(reference, materialize=True)
        self.assertEqual(observed["step"], 8)
        self.assertEqual((path / "optimizer.pt").read_bytes(), b"native-checkpoint-fixture:optimizer.pt")
        for field, value in [("profile_sha256", "33" * 32), ("step", 32)]:
            changed = {**manifest, field: value}
            with self.assertRaises(Refused):
                self.provider.checkpoint(self.provider.store(encode(changed)))
        write(source / "checkpoint.json", encode({"checkpoint_sha256": "44" * 32, "step": 8}))
        with self.assertRaises(Refused):
            self.provider.checkpoint(reference)

    @unittest.skipUnless(importlib.util.find_spec("cryptography"), "requires qualified native cryptography stack")
    def test_source_attestation_binds_actual_bundle_dataset_and_configured_key(self) -> None:
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
        from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
        key = Ed25519PrivateKey.from_private_bytes(bytes([61]) * 32)
        self.provider.profile = {"context": {"base_sha256": "11" * 32}, "tokenizer_sha256": "22" * 32,
                                 "train_data": "33" * 32, "license_refs": ["Apache-2.0"],
                                 "attestation_keys": {"fixture": key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw).hex()}}
        payload = {"schema": "cohesix-peft-source-attestation/v1", "source_sha256": "44" * 32,
                   "base_sha256": "11" * 32, "tokenizer_sha256": "22" * 32, "dataset_sha256": "33" * 32,
                   "adapter_bundle_sha256": "55" * 32, "permitted_use": "training-and-private-inference", "license_refs": ["Apache-2.0"]}
        record = {"payload": payload, "key_id": "fixture",
                  "signature": key.sign(encode(payload)).hex()}
        ref = self.provider.store(encode(record))
        self.provider.attest([ref], "44" * 32, "55" * 32)
        for source, adapter in [("66" * 32, "55" * 32), ("44" * 32, None), ("44" * 32, "77" * 32)]:
            with self.assertRaises(Refused):
                self.provider.attest([ref], source, adapter)
        imported = {**payload, "schema": "cohesix-peft-source-attestation/v2",
                    "dataset_sha256": None, "training_provenance": "unknown"}
        import_ref = self.provider.store(encode({"payload": imported, "key_id": "fixture",
                                                 "signature": key.sign(encode(imported)).hex()}))
        self.provider.attest([import_ref], "44" * 32, "55" * 32, unknown_training=True)
        with self.assertRaises(Refused):
            self.provider.attest([ref], "44" * 32, "55" * 32, unknown_training=True)
        record["signature"] = "00" * 64
        bad = self.provider.store(encode(record))
        with self.assertRaises(Exception):
            self.provider.attest([bad], "44" * 32, "55" * 32)


if __name__ == "__main__":
    unittest.main()
