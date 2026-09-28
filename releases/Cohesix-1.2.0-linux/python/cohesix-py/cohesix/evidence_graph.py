# Author: Lukas Bower
# Purpose: Route signed graph verification to the same bounded Rust verifier used by host tools.
# Copyright 2026 Lukas Bower

"""The Python SDK never independently decides receipt authenticity."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from .native_providers import bounded_command
from .providers import ProviderUnavailable


def verify_graph(graph: Path, trust: Path, cas: Path, *, coh_binary: Path) -> dict[str, Any]:
    """Validate with an explicit installed coh executable and external trust file.

    The returned object is a derived verification projection, not a new receipt.
    No trust key or record is accepted merely because it appears in the pack.
    """
    if not coh_binary.is_absolute():
        raise ProviderUnavailable("invalid_verifier", "evidence")
    arguments = [str(coh_binary.resolve(strict=True)), "evidence", "verify",
                 "--input", str(graph.resolve(strict=True)),
                 "--trust", str(trust.resolve(strict=True)),
                 "--cas", str(cas.resolve(strict=True))]
    raw = bounded_command(arguments, timeout_s=30)
    result = json.loads(raw)
    if (not isinstance(result, dict)
            or result.get("schema") != "cohesix-verified-graph-projection/v1"
            or result.get("authoritative") is not False):
        raise ProviderUnavailable("invalid_verifier_output", "evidence")
    return result
