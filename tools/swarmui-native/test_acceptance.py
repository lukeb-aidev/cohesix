# Author: Lukas Bower
# Purpose: Reject fixture, incomplete and reordered native acceptance evidence deterministically.
# Copyright 2026 Lukas Bower
import json
import hashlib
from pathlib import Path
import tempfile
import unittest
from acceptance import check_log
from capture import capture


class NativeLogContract(unittest.TestCase):
    def test_fixture_and_reordered_logs_are_refused(self):
        for row in [{"lane": "fixture", "sequence": 0}, {"lane": "native_bridge", "sequence": 1}]:
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "log"
                path.write_text(json.dumps(row))
                with self.assertRaises(ValueError):
                    check_log(path)

    def test_empty_native_shell_does_not_establish_acceptance(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "log"
            path.write_text(json.dumps({"lane": "native_bridge", "sequence": 0, "action": "startup", "observation": {"bridge": "tauri", "fixture": False, "desktop_source_sha256": "a" * 64}}))
            checks = check_log(path)
            self.assertTrue(checks["real_bridge"])
            self.assertFalse(checks["gateway_connect"])
            self.assertFalse(checks["clean_shutdown"])
            self.assertFalse(checks["verified_reference"])


class CaptureContract(unittest.TestCase):
    def test_original_jpeg_frames_keep_their_media_type(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            frame = root / "native.jpg"
            data = b"\xff\xd8\xff\xe0original-native-frame"
            frame.write_bytes(data)
            result = root / "result.json"
            result.write_text(json.dumps({"lane": "native_live", "verdict": "PASS", "screenshots": [{"path": str(frame), "sha256": hashlib.sha256(data).hexdigest()}] * 2}))
            capture(result, root / "capture.html")
            self.assertIn("data:image/jpeg;base64,", (root / "capture.html").read_text())

    def test_refuses_unqualified_or_changed_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = root / "result.json"
            result.write_text(json.dumps({"lane": "fixture", "verdict": "PASS"}))
            with self.assertRaisesRegex(ValueError, "native acceptance"):
                capture(result, root / "capture.html")
            frame = root / "frame.png"
            frame.write_bytes(b"\x89PNG\r\n\x1a\nchanged")
            result.write_text(json.dumps({"lane": "native_live", "verdict": "PASS", "screenshots": [{"path": str(frame), "sha256": "0" * 64}] * 2}))
            with self.assertRaisesRegex(ValueError, "identity mismatch"):
                capture(result, root / "capture.html")
            self.assertFalse((root / "capture.html").exists())


if __name__ == "__main__":
    unittest.main()
