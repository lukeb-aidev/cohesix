"""Audit transcript helpers for Cohesix Python clients."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Iterable, List, Optional


@dataclass
class CohesixAudit:
    """Collects ACK/ERR lines and output lines for deterministic transcripts."""

    lines: List[str] = field(default_factory=list)

    def push_ack(self, status: str, verb: str, detail: Optional[str] = None) -> None:
        status_upper = status.upper()
        if status_upper not in ("OK", "ERR"):
            raise ValueError("status must be OK or ERR")
        line = f"{status_upper} {verb}"
        if detail:
            line = f"{line} {detail}"
        self.lines.append(line)

    def push_line(self, line: str) -> None:
        self.lines.append(line)

    def extend(self, lines: Iterable[str]) -> None:
        self.lines.extend(lines)
