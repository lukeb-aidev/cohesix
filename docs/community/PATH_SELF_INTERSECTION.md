// CLASSIFICATION: COMMUNITY
// Filename: PATH_SELF_INTERSECTION.md v0.1
// Author: Lukas Bower
// Date Modified: 2026-11-19

# Path Self-Intersection Detection

This document summarizes a simple plane sweep approach for detecting self-intersections in polygonal paths. The algorithm works for open or closed paths on a two-dimensional plane.

## Algorithm Outline

1. **Event Generation**: Represent each line segment of the path by its left and right endpoints (ordered by x then y). Store events for segment start and end.
2. **Sweep Line Ordering**: Process events in increasing x order, maintaining an ordered set of active segments that intersect the vertical sweep line. Segments are ordered by their y-coordinate at the sweep position.
3. **Neighbor Tests**: Whenever a segment is inserted or removed from the active set, check it against immediate neighbors for intersections using orientation tests. Only adjacent segments in the set can intersect at the current sweep position.
4. **Intersection Report**: If any intersection between non-consecutive segments is found, report the intersection coordinates and the indices of the segments.

## Complexity

The sweep-line algorithm processes `n` segments in `O(n log n + k)` time where `k` is the number of intersections found. Memory usage is `O(n)` for the event queue and active set. The method efficiently identifies self-intersections without comparing all segment pairs.

