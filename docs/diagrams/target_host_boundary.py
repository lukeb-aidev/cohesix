# Author: Lukas Bower
# Purpose: Regenerate the logical target-host cutaway and labelled static companion.
# Copyright 2026 Lukas Bower

"""Build the compact ASCII STL embedded in ARCHITECTURE.md.

Run ``python3 docs/diagrams/target_host_boundary.py --write`` after changing the
layout. ``--check`` verifies that both generated outputs match this source.
Coordinates are schematic: they encode distinct deployment and address-space
boundaries, not dimensions, capacity, privilege, or measured isolation.
"""

from __future__ import annotations

import argparse
import math
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DOCUMENT = ROOT / "docs/ARCHITECTURE.md"
COMPANION = ROOT / "docs/diagrams/target-host-boundary.svg"
START = "<!-- target-host-stl:start -->"
END = "<!-- target-host-stl:end -->"
# GitHub's native STL camera opens at a fixed distance; this display scale
# fills its initial viewport without assigning real units to the schematic.
STL_DISPLAY_SCALE = 2.5

Point = tuple[float, float, float]
Triangle = tuple[Point, Point, Point]
PlanPoint = tuple[float, float]


def box(
    x0: float, y0: float, z0: float,
    x1: float, y1: float, z1: float,
) -> list[Triangle]:
    """Return an outward-wound, closed cuboid mesh."""
    assert x0 < x1 and y0 < y1 and z0 < z1
    p000, p100 = (x0, y0, z0), (x1, y0, z0)
    p010, p110 = (x0, y1, z0), (x1, y1, z0)
    p001, p101 = (x0, y0, z1), (x1, y0, z1)
    p011, p111 = (x0, y1, z1), (x1, y1, z1)
    quads = (
        (p000, p010, p110, p100),  # bottom
        (p001, p101, p111, p011),  # top
        (p000, p100, p101, p001),  # front
        (p010, p011, p111, p110),  # back
        (p000, p001, p011, p010),  # left
        (p100, p110, p111, p101),  # right
    )
    return [(a, b, c) for a, b, c, _ in quads] + [
        (a, c, d) for a, _, c, d in quads
    ]


def wall(a: PlanPoint, b: PlanPoint, height: float) -> list[Triangle]:
    """Extrude an oriented footprint edge with its outward face on the right."""
    bottom_a, bottom_b = (*a, 0.0), (*b, 0.0)
    top_a, top_b = (*a, height), (*b, height)
    return [(bottom_a, bottom_b, top_b), (bottom_a, top_b, top_a)]


def cross(a: PlanPoint, b: PlanPoint, c: PlanPoint) -> float:
    """Return the signed area of the XY triangle, doubled."""
    return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])


def polygon_prism(points: list[PlanPoint], height: float) -> list[Triangle]:
    """Extrude a simple concave footprint with a manifold outer boundary."""
    area = sum(
        points[i][0] * points[(i + 1) % len(points)][1]
        - points[(i + 1) % len(points)][0] * points[i][1]
        for i in range(len(points))
    )
    if area < 0:
        points.reverse()
    remaining = list(range(len(points)))
    top: list[Triangle] = []
    while len(remaining) > 3:
        for slot, index in enumerate(remaining):
            prev = remaining[slot - 1]
            next_index = remaining[(slot + 1) % len(remaining)]
            a, b, c = points[prev], points[index], points[next_index]
            if cross(a, b, c) <= 0:
                continue
            if any(
                candidate not in (prev, index, next_index)
                and cross(a, b, points[candidate]) >= 0
                and cross(b, c, points[candidate]) >= 0
                and cross(c, a, points[candidate]) >= 0
                for candidate in remaining
            ):
                continue
            top.append(((*a, height), (*b, height), (*c, height)))
            remaining.pop(slot)
            break
        else:
            raise ValueError("cannot triangulate frame footprint")
    a, b, c = (points[index] for index in remaining)
    top.append(((*a, height), (*b, height), (*c, height)))
    bottom = [(c, b, a) for a, b, c in top]
    bottom = [tuple((x, y, 0.0) for x, y, _ in tri) for tri in bottom]
    sides = [
        tri
        for i, point in enumerate(points)
        for tri in wall(point, points[(i + 1) % len(points)], height)
    ]
    return top + bottom + sides


def open_frame(
    x0: float,
    y0: float,
    x1: float,
    y1: float,
    *,
    height: float,
    gap: tuple[str, float, float] | None = None,
    width: float = 0.14,
) -> list[Triangle]:
    """Make one open, watertight wall volume, with an optional side ingress."""
    outer = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    inner = [
        (x0 + width, y0 + width), (x1 - width, y0 + width),
        (x1 - width, y1 - width), (x0 + width, y1 - width),
    ]
    if gap is None:
        top = [
            triangle
            for i in range(4)
            for triangle in (
                ((*outer[i], height), (*outer[(i + 1) % 4], height),
                 (*inner[(i + 1) % 4], height)),
                ((*outer[i], height), (*inner[(i + 1) % 4], height),
                 (*inner[i], height)),
            )
        ]
        bottom = [
            tuple((x, y, 0.0) for x, y, _ in (c, b, a))
            for a, b, c in top
        ]
        sides = [
            tri
            for i in range(4)
            for tri in (
                wall(outer[i], outer[(i + 1) % 4], height)
                + wall(inner[(i + 1) % 4], inner[i], height)
            )
        ]
        return top + bottom + sides

    side, lower, upper = gap
    assert side in ("west", "east")
    assert y0 + width < lower < upper < y1 - width
    points = [
        (x1, lower), (x1, y0), (x0, y0), (x0, y1),
        (x1, y1), (x1, upper), (x1 - width, upper),
        (x1 - width, y1 - width), (x0 + width, y1 - width),
        (x0 + width, y0 + width), (x1 - width, y0 + width),
        (x1 - width, lower),
    ]
    if side == "west":
        points = [(x0 + x1 - x, y) for x, y in points]
    return polygon_prism(points, height)


def geometry() -> list[Triangle]:
    """Show an open target boundary, separate wells, and one console ingress."""
    parts = [
        # The large frames are deployment boundaries, not physical chassis.
        open_frame(0, 0, 14, 10, height=0.75, gap=("east", 7.25, 8.15), width=0.20),
        open_frame(18, 0, 27, 10, height=0.35, gap=("west", 7.25, 8.15), width=0.20),
        # Each smaller open well stands for a distinct target runtime domain.
        open_frame(1.0, 3.4, 4.2, 6.6, height=1.50),  # root / Queen
        open_frame(5.3, 6.2, 8.5, 9.2, height=1.50),  # NineDoor child
        open_frame(10.0, 6.2, 13.2, 9.2, height=1.50,
                   gap=("east", 7.25, 8.15)),  # console-network child
        open_frame(5.3, 0.8, 8.5, 4.0, height=1.50),  # one Worker child
        open_frame(10.0, 0.8, 13.2, 4.0, height=1.50),  # one Pi driver child
        # Low pads are host-side responsibilities, not seL4 compartments.
        box(18.8, 6.2, 0, 22.0, 9.2, 0.22),  # gateway
        box(23.0, 2.2, 0, 26.2, 5.4, 0.22),  # CUDA/PEFT provider
        # A single schematic bridge enters the target console child.
        [*box(13.3, 7.58, 0.18, 18.7, 7.82, 0.42)],
    ]
    return [triangle for part in parts for triangle in part]


def normal(triangle: Triangle) -> Point:
    """Compute a unit facet normal and reject degenerate faces."""
    a, b, c = triangle
    u = tuple(b[i] - a[i] for i in range(3))
    v = tuple(c[i] - a[i] for i in range(3))
    cross = (
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    )
    length = math.sqrt(sum(value * value for value in cross))
    if length <= 0:
        raise ValueError("degenerate STL triangle")
    return tuple(value / length for value in cross)  # type: ignore[return-value]


def ascii_stl() -> str:
    """Serialize the model in GitHub's supported ASCII STL syntax."""
    lines = ["solid target_host_boundary"]
    for triangle in geometry():
        nx, ny, nz = normal(triangle)
        lines.extend(
            (f"facet normal {nx:.0f} {ny:.0f} {nz:.0f}", "  outer loop")
        )
        lines.extend(
            f"    vertex {x * STL_DISPLAY_SCALE:.2f} "
            f"{y * STL_DISPLAY_SCALE:.2f} {z * STL_DISPLAY_SCALE:.2f}"
            for x, y, z in triangle
        )
        lines.extend(("  endloop", "endfacet"))
    lines.append("endsolid target_host_boundary")
    return "\n".join(lines)


def svg_companion() -> str:
    """Give static and screen-reader users a labelled plan of the same layout."""
    cells = (
        (20, 104, "Root / Queen", ""),
        (149, 28, "NineDoor child", ""),
        (280, 28, "Console child", ""),
        (149, 172, "Worker child", ""),
        (280, 172, "Pi driver child", ""),
        (518, 28, "Host gateway", ' class="hostprocess"'),
        (648, 137, "Host GPU / PEFT", ' class="hostprocess"'),
    )
    cell_svg = "\n".join(
        f'  <g><rect{style} x="{x}" y="{y}" width="108" height="62" rx="2"/>'
        f'<text x="{x + 54}" y="{y + 34}" text-anchor="middle">{label}</text></g>'
        for x, y, label, style in cells
    )
    return """<!-- Author: Lukas Bower -->
<!-- Purpose: Provide a labelled static plan of the target-host boundary cutaway. -->
<!-- Copyright 2026 Lukas Bower -->
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 780 278"
     role="img" aria-labelledby="title desc">
  <title id="title">Cohesix target and host boundary, plan view</title>
  <desc id="desc">Five separate target runtime wells stand inside a target
  frame. Host gateway and GPU/PEFT process pads stand outside it. The sole drawn
  cross-boundary connector reaches the console child, not the root, Worker, or
  driver wells.</desc>
  <style>
    rect { fill: white; stroke: #222; stroke-width: 2; }
    .boundary { fill: none; stroke-width: 3; }
    .hostprocess { fill: #eee; stroke-dasharray: 5 3; }
    .connector { fill: #444; stroke: none; }
    text { font: 11px sans-serif; fill: #111; }
    .heading { font-size: 14px; font-weight: bold; }
  </style>
  <rect class="boundary" x="10" y="18" width="400" height="235"/>
  <rect class="boundary" x="508" y="18" width="260" height="235"/>
  <text class="heading" x="20" y="247">seL4 target</text>
  <text class="heading" x="518" y="247">Host side (one or more hosts)</text>
  <rect class="connector" x="389" y="53" width="130" height="8"/>
__CELLS__
</svg>
""".replace("__CELLS__", cell_svg)


def generated_document(existing: str) -> str:
    """Replace only the marked derived mesh block in the architecture guide."""
    if existing.count(START) != 1 or existing.count(END) != 1:
        raise ValueError("architecture document requires one STL marker pair")
    before, remainder = existing.split(START, 1)
    _, after = remainder.split(END, 1)
    return before + START + "\n```stl\n" + ascii_stl() + "\n```\n" + END + after


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--write", action="store_true", help="regenerate outputs")
    action.add_argument("--check", action="store_true", help="check outputs")
    args = parser.parse_args()
    document = generated_document(DOCUMENT.read_text(encoding="utf-8"))
    companion = svg_companion()
    if args.write:
        DOCUMENT.write_text(document, encoding="utf-8")
        COMPANION.write_text(companion, encoding="utf-8")
        print(f"wrote {len(geometry())} STL facets and labelled SVG")
    else:
        stale_document = DOCUMENT.read_text(encoding="utf-8") != document
        stale_companion = (
            not COMPANION.exists()
            or COMPANION.read_text(encoding="utf-8") != companion
        )
        if stale_document or stale_companion:
            raise SystemExit("target-host diagram outputs are stale; run --write")
        print(f"target-host diagram outputs match source ({len(geometry())} facets)")


if __name__ == "__main__":
    main()
