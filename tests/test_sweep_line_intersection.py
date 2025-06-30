# CLASSIFICATION: COMMUNITY
# Filename: test_sweep_line_intersection.py v0.2
# Author: Lukas Bower
# Date Modified: 2026-11-20
"""Unit tests for sweep-line self-intersection detection."""

import unittest

from src.geometry.sweep_line_intersection import find_self_intersections


class SweepLineIntersectionTest(unittest.TestCase):
    def test_basic_intersection(self) -> None:
        segments = [
            ((0.0, 0.0), (5.0, 5.0)),
            ((0.0, 5.0), (5.0, 0.0)),
            ((1.0, 1.0), (2.0, 2.0)),
        ]
        pts = find_self_intersections(segments)
        self.assertEqual(len(pts), 1)
        self.assertAlmostEqual(pts[0][0], 2.5, places=6)
        self.assertAlmostEqual(pts[0][1], 2.5, places=6)

    def test_no_intersections(self) -> None:
        segments = [((0.0, 0.0), (1.0, 0.0)), ((2.0, 0.0), (3.0, 0.0))]
        pts = find_self_intersections(segments)
        self.assertEqual(pts, [])


if __name__ == "__main__":
    unittest.main()
