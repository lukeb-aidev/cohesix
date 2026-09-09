# Author: Lukas Bower
# Purpose: Preserve strict target Worker fragment bounds, ordering and overlap semantics.
# Copyright 2026 Lukas Bower

import unittest

from scripts.lib.worker_log import records


class WorkerLogTests(unittest.TestCase):
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
