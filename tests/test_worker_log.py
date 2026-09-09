# Author: Lukas Bower
# Purpose: Preserve strict target Worker fragment bounds, ordering and overlap semantics.
# Copyright 2026 Lukas Bower

import unittest
from pathlib import Path
import tempfile
from unittest.mock import patch

from scripts.lib.worker_log import ExportMonitor, record_matches, records


class WorkerLogTests(unittest.TestCase):
    def test_checkpoint_retains_original_fragments_once_before_later_eviction(self):
        line = ("WORKER_LOG id=4 part=0 last=1 data=WORKER_TASK_READY "
                "role=worker-heartbeat slot=0 supervisor_generation=7 sequence=1")
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "worker.log"
            exports = iter([line + "\n" + line + "\n", line + "\n", "unrelated later log\n"])
            monitor = ExportMonitor(path, lambda: next(exports))
            expected = [{"marker": "WORKER_TASK_READY", "role": "worker-heartbeat",
                         "slot": 0, "sequence": 1}]
            monitor.checkpoint(expected, after_generation=6)
            monitor.checkpoint()
            monitor.checkpoint()
            # The required record survives target eviction without another RPC.
            monitor.checkpoint(expected, after_generation=6)
            self.assertEqual(path.read_text(), line + "\n")

    def test_periodic_capture_yields_to_a_required_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "worker.log"
            def forbidden_read():
                self.fail("periodic capture must not compete with an active checkpoint")
            monitor = ExportMonitor(path, forbidden_read)
            with monitor.export_lock:
                monitor.checkpoint(opportunistic=True)
            self.assertIsNone(monitor.error)
            self.assertFalse(path.exists())

    def test_checkpoint_cannot_replace_missing_or_conflicting_proof(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "worker.log"
            monitor = ExportMonitor(path, lambda: "unrelated log\n")
            with self.assertRaisesRegex(ValueError, "lacks required"):
                monitor.checkpoint([{"marker": "WORKER_TASK_READY"}], timeout_s=0)
            line = ("WORKER_LOG id=4 part=0 last=1 data=WORKER_TASK_READY "
                    "role=worker-heartbeat supervisor_generation=7 sequence=1")
            exports = iter([line, line.replace("sequence=1", "sequence=2")])
            monitor.read = lambda: next(exports)
            monitor.checkpoint()
            monitor.checkpoint()
            with self.assertRaisesRegex(ValueError, "conflicting"):
                records(path.read_text())

    def test_checkpoint_keeps_the_existing_artifact_byte_bound(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "worker.log"
            monitor = ExportMonitor(path, lambda: "WORKER_LOG_ERROR reason=invalid-record\n")
            with patch("scripts.lib.worker_log.MAX_RETAINED_BYTES", 1):
                with self.assertRaisesRegex(ValueError, "byte bound"):
                    monitor.checkpoint()
            self.assertFalse(path.exists())
            path.write_bytes(b"xx")
            with patch("scripts.lib.worker_log.MAX_RETAINED_BYTES", 1):
                with self.assertRaisesRegex(ValueError, "byte bound"):
                    ExportMonitor(path, lambda: "")

    def test_record_identity_does_not_alias_equal_worker_sequences(self):
        line = ("WORKER_TASK_RECEIPT role=worker-gpu slot=2 lease_epoch=1 "
                "supervisor_generation=9 cap_generation=1 action=0x0202 outcome=1 sequence=7")
        expected = {"marker": "WORKER_TASK_RECEIPT", "role": "worker-gpu", "slot": 2,
                    "supervisor_generation": 9, "sequence": 7}
        self.assertTrue(record_matches(line, expected))
        self.assertFalse(record_matches(line, {**expected, "slot": 1}))
        self.assertFalse(record_matches(line, expected, after_generation=9))

    def test_overlapping_exports_preserve_one_complete_record(self):
        first = "WORKER_LOG id=4 part=0 last=0 data=WORKER_TASK_READY role=worker-"
        last = "WORKER_LOG id=4 part=1 last=1 data=heartbeat sequence=1"
        self.assertEqual(records(f"{first}\n{last}\n{first}\n{last}\n"),
                         "WORKER_TASK_READY role=worker-heartbeat sequence=1\n")

    def test_host_command_preview_is_not_a_target_data_record(self):
        text = ("[console] OK CAT path=/log/queen.log data=WORKER_LOG id=4 part=0 last=1 data=WORKER_TASK_RE\n"
                "WORKER_LOG id=4 part=0 last=1 data=WORKER_TASK_READY sequence=1\n")
        self.assertEqual(records(text), "WORKER_TASK_READY sequence=1\n")

    def test_missing_fragment_cannot_establish_a_marker(self):
        text = "WORKER_LOG id=4 part=1 last=1 data=heartbeat sequence=1\n"
        with self.assertRaisesRegex(ValueError, "incomplete"):
            records(text)
        self.assertEqual(records(text, complete=False), "")

    def test_conflicting_exports_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "conflicting"):
            records("WORKER_LOG id=4 part=0 last=1 data=WORKER_TASK_READY a=1\n"
                    "WORKER_LOG id=4 part=0 last=1 data=WORKER_TASK_READY a=2\n")

    def test_bounds_and_target_errors_fail_closed(self):
        for text in (
            "WORKER_LOG id=0 part=6 last=1 data=x\n",
            "WORKER_LOG id=0 part=0 last=1 data=" + "x" * 177 + "\n",
            "WORKER_LOG_ERROR reason=invalid-record\n",
            "WORKER_LOG id=0 part=0 last=1 data=NOT_A_WORKER_RECORD\n",
        ):
            with self.subTest(text=text), self.assertRaises(ValueError):
                records(text)


if __name__ == "__main__":
    unittest.main()
