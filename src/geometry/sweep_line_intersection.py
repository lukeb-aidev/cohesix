# CLASSIFICATION: COMMUNITY
# Filename: sweep_line_intersection.py v0.2
# Author: Lukas Bower
# Date Modified: 2026-11-20
"""Sweep-line detection of path self-intersections.

Implements a simple plane sweep algorithm to detect
all intersection points among a set of 2D segments.
"""

from __future__ import annotations

from dataclasses import dataclass
from bisect import bisect_left
from typing import List, Tuple, Sequence

Point = Tuple[float, float]
Segment = Tuple[Point, Point]

EPSILON = 1e-9


def _orientation(p: Point, q: Point, r: Point) -> int:
    """Return orientation of ordered triplet (p, q, r).

    Returns:
        0 if colinear, 1 if clockwise, -1 if counterclockwise.
    """
    val = (q[1] - p[1]) * (r[0] - q[0]) - (q[0] - p[0]) * (r[1] - q[1])
    if abs(val) < EPSILON:
        return 0
    return 1 if val > 0 else -1


def _on_segment(p: Point, q: Point, r: Point) -> bool:
    """Return True if q lies on segment pr."""
    return (
        min(p[0], r[0]) - EPSILON <= q[0] <= max(p[0], r[0]) + EPSILON
        and min(p[1], r[1]) - EPSILON <= q[1] <= max(p[1], r[1]) + EPSILON
    )


def _segments_intersect(s1: Segment, s2: Segment) -> Tuple[bool, Point | None]:
    """Check if two segments intersect.

    Returns (True, point) if they intersect at a single point. If they
    overlap, the first overlapping point is returned.
    """
    p1, q1 = s1
    p2, q2 = s2

    o1 = _orientation(p1, q1, p2)
    o2 = _orientation(p1, q1, q2)
    o3 = _orientation(p2, q2, p1)
    o4 = _orientation(p2, q2, q1)

    if o1 != o2 and o3 != o4:
        return True, _intersection_point(p1, q1, p2, q2)

    if o1 == 0 and _on_segment(p1, p2, q1):
        return True, p2
    if o2 == 0 and _on_segment(p1, q2, q1):
        return True, q2
    if o3 == 0 and _on_segment(p2, p1, q2):
        return True, p1
    if o4 == 0 and _on_segment(p2, q1, q2):
        return True, q1

    return False, None


def _intersection_point(p1: Point, q1: Point, p2: Point, q2: Point) -> Point:
    """Return precise intersection of two lines defined by the segments."""
    x1, y1 = p1
    x2, y2 = q1
    x3, y3 = p2
    x4, y4 = q2

    denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4)
    if abs(denom) < EPSILON:
        return (float('nan'), float('nan'))
    px = ((x1 * y2 - y1 * x2) * (x3 - x4) - (x1 - x2) * (x3 * y4 - y3 * x4)) / denom
    py = ((x1 * y2 - y1 * x2) * (y3 - y4) - (y1 - y2) * (x3 * y4 - y3 * x4)) / denom
    return px, py


@dataclass(order=True)
class _Event:
    x: float
    typ: int  # 0 = start, 1 = end
    y: float
    index: int


class _ActiveSet:
    """Ordered set of active segment indices by y-coordinate."""

    def __init__(self, segments: List[Segment]):
        self._segs = segments
        self._active: List[int] = []

    def _y_at(self, idx: int, x: float) -> float:
        (x1, y1), (x2, y2) = self._segs[idx]
        if abs(x1 - x2) < EPSILON:
            return min(y1, y2)
        slope = (y2 - y1) / (x2 - x1)
        return y1 + slope * (x - x1)

    def insert(self, idx: int, x: float) -> int:
        key = self._y_at(idx, x + EPSILON)
        keys = [self._y_at(i, x + EPSILON) for i in self._active]
        pos = bisect_left(keys, key)
        self._active.insert(pos, idx)
        return pos

    def remove(self, idx: int) -> int:
        pos = self._active.index(idx)
        self._active.pop(pos)
        return pos

    def neighbor(self, pos: int, offset: int) -> int | None:
        n = pos + offset
        if 0 <= n < len(self._active):
            return self._active[n]
        return None


def find_self_intersections(
    segments: Sequence[tuple[tuple[float, float], tuple[float, float]]]
) -> List[Point]:
    """Detect intersection points among a collection of segments."""
    ordered_segments: List[Segment] = []
    events: List[_Event] = []

    for idx, seg in enumerate(segments):
        (x1, y1), (x2, y2) = seg
        if (x1, y1) > (x2, y2):
            start, end = (x2, y2), (x1, y1)
        else:
            start, end = (x1, y1), (x2, y2)
        ordered_segments.append((start, end))
        events.append(_Event(start[0], 0, start[1], idx))
        events.append(_Event(end[0], 1, end[1], idx))

    events.sort(key=lambda e: (e.x, e.typ, e.y))

    active = _ActiveSet(ordered_segments)
    intersections: List[Point] = []
    seen = set()

    for ev in events:
        seg = ordered_segments[ev.index]
        sweep_x = ev.x
        if ev.typ == 0:
            pos = active.insert(ev.index, sweep_x)
            above = active.neighbor(pos, -1)
            below = active.neighbor(pos, 1)
            for other in (above, below):
                if other is None:
                    continue
                inter, pt = _segments_intersect(seg, ordered_segments[other])
                if inter and pt is not None:
                    if pt in [seg[0], seg[1], ordered_segments[other][0], ordered_segments[other][1]]:
                        continue
                    key = (round(pt[0], 12), round(pt[1], 12))
                    if key not in seen:
                        seen.add(key)
                        intersections.append((pt[0], pt[1]))
        else:
            pos = active.remove(ev.index)
            above = active.neighbor(pos - 1, 0)
            below = active.neighbor(pos, 0)
            if above is not None and below is not None:
                seg_a = ordered_segments[above]
                seg_b = ordered_segments[below]
                inter, pt = _segments_intersect(seg_a, seg_b)
                if inter and pt is not None:
                    if pt in [seg_a[0], seg_a[1], seg_b[0], seg_b[1]]:
                        continue
                    key = (round(pt[0], 12), round(pt[1], 12))
                    if key not in seen:
                        seen.add(key)
                        intersections.append((pt[0], pt[1]))

    return intersections
