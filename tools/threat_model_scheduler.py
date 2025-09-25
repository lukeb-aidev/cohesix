# CLASSIFICATION: COMMUNITY
# Filename: threat_model_scheduler.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-31
"""Automate threat model review cadence and ADR capture."""

from __future__ import annotations

import argparse
import datetime as dt
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, List


@dataclass(frozen=True)
class ThreatModelReview:
    quarter: int
    year: int
    date: dt.date
    trace_id: str


@dataclass(frozen=True)
class SchedulerResult:
    reviews: List[ThreatModelReview]
    ics_path: Path
    adr_paths: List[Path]


def next_quarter_start(reference: dt.date) -> dt.date:
    quarter_index = (reference.month - 1) // 3
    start_month = quarter_index * 3 + 1
    start_date = reference.replace(month=start_month, day=1)
    if reference > start_date:
        month = start_month + 3
        year = reference.year
        if month > 12:
            month -= 12
            year += 1
        start_date = dt.date(year, month, 1)
    return start_date


def first_business_day(start: dt.date) -> dt.date:
    current = start
    while current.weekday() >= 5:
        current += dt.timedelta(days=1)
    return current


def generate_reviews(start_date: dt.date, occurrences: int) -> List[ThreatModelReview]:
    reviews: List[ThreatModelReview] = []
    current = next_quarter_start(start_date)
    for _ in range(occurrences):
        scheduled = first_business_day(current)
        quarter = ((current.month - 1) // 3) + 1
        trace_id = f"tmr-{scheduled:%Y%m%d}-q{quarter}"
        reviews.append(ThreatModelReview(quarter=quarter, year=scheduled.year, date=scheduled, trace_id=trace_id))
        month = current.month + 3
        year = current.year
        if month > 12:
            month -= 12
            year += 1
        current = dt.date(year, month, 1)
    return reviews


def generate_ics_content(reviews: Iterable[ThreatModelReview]) -> str:
    timestamp = dt.datetime.utcnow().strftime("%Y%m%dT%H%M%SZ")
    lines = [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//Cohesix//Threat Model Cadence//EN",
    ]
    for review in reviews:
        uid = f"{review.trace_id}@cohesix.dev"
        summary = f"Cohesix Threat Model Review Q{review.quarter} {review.year}"
        description = f"Trace ID {review.trace_id}\\nADR: docs/community/architecture/adr/ADR-{review.date:%Y%m%d}-threat-model-q{review.quarter}.md"
        lines.extend(
            [
                "BEGIN:VEVENT",
                f"UID:{uid}",
                f"DTSTAMP:{timestamp}",
                f"DTSTART;VALUE=DATE:{review.date:%Y%m%d}",
                f"SUMMARY:{summary}",
                f"DESCRIPTION:{description}",
                "END:VEVENT",
            ]
        )
    lines.append("END:VCALENDAR")
    return "\r\n".join(lines) + "\r\n"


def generate_adr_content(review: ThreatModelReview, today: dt.date) -> str:
    filename = f"ADR-{review.date:%Y%m%d}-threat-model-q{review.quarter}.md"
    lines = [
        "// CLASSIFICATION: COMMUNITY",
        f"// Filename: {filename} v0.1",
        "// Author: Lukas Bower",
        f"// Date Modified: {today.isoformat()}",
        "",
        f"# Threat Model Review Q{review.quarter} {review.year}",
        "",
        f"- **Date:** {review.date.isoformat()}",
        f"- **Trace ID:** {review.trace_id}",
        "- **Status:** Proposed",
        "- **Context:** Quarterly threat model review per governance backlog.",
        "- **Decision:** Document mitigations, risks, and follow-up actions.",
        "- **Consequences:** Updated threat posture communicated to stakeholders.",
        "",
        "## Notes",
        "",
        "- Capture review outcomes, incident follow-ups, and sign-offs here.",
    ]
    return "\n".join(lines) + "\n"


def ensure_outputs(base_path: Path, start_date: dt.date, occurrences: int) -> SchedulerResult:
    workspace = base_path / "workspace"
    security_dir = workspace / "docs" / "security"
    architecture_dir = workspace / "docs" / "community" / "architecture"
    adr_dir = architecture_dir / "adr"
    adr_dir.mkdir(parents=True, exist_ok=True)
    security_dir.mkdir(parents=True, exist_ok=True)

    reviews = generate_reviews(start_date, occurrences)
    ics_path = security_dir / "threat_model_schedule.ics"
    ics_content = generate_ics_content(reviews)
    ics_path.write_text(ics_content, encoding="utf-8")

    today = dt.date.today()
    adr_paths: List[Path] = []
    for review in reviews:
        adr_path = adr_dir / f"ADR-{review.date:%Y%m%d}-threat-model-q{review.quarter}.md"
        content = generate_adr_content(review, today)
        if adr_path.exists():
            existing = adr_path.read_text(encoding="utf-8")
            if review.trace_id not in existing:
                adr_path.write_text(content, encoding="utf-8")
        else:
            adr_path.write_text(content, encoding="utf-8")
        adr_paths.append(adr_path)

    return SchedulerResult(reviews=reviews, ics_path=ics_path, adr_paths=adr_paths)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate threat model review schedule and ADR templates.")
    parser.add_argument(
        "--start-date",
        help="ISO date used to seed the cadence (default: today)",
    )
    parser.add_argument(
        "--occurrences",
        type=int,
        default=4,
        help="Number of future reviews to schedule (default: 4)",
    )
    parser.add_argument(
        "--base-path",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="Repository root that contains the workspace directory.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    start_date = dt.date.today()
    if args.start_date:
        start_date = dt.date.fromisoformat(args.start_date)
    result = ensure_outputs(args.base_path, start_date, args.occurrences)
    print(f"Generated {len(result.reviews)} threat model reviews")
    print(f"ICS schedule: {result.ics_path}")
    for path in result.adr_paths:
        print(f"ADR template: {path}")


if __name__ == "__main__":  # pragma: no cover - CLI entrypoint
    main()
