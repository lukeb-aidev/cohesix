# Author: Lukas Bower
# Purpose: Refuse caller-selected macOS release inputs and action substitutions at the SDK boundary.
# Copyright 2026 Lukas Bower
"""Compiler-owned operands remain distinct from caller tickets."""

from copy import deepcopy
import pytest
from cohesix import providers
from cohesix.authority import validate_provider_v1
from cohesix.errors import CohesixError
from cohesix.orchestration import HostTicketRequest


def test_macos_action_is_bound_to_exact_compiled_target(monkeypatch):
    selected = deepcopy(providers.REGISTRY)
    selected["contract"]["macos_targets"] = [
        {"id": "release", "operation": {"action": "mac_release.build"}},
        {"id": "scan", "operation": {"action": "endpoint_compliance.observe"}},
    ]
    monkeypatch.setattr(providers, "REGISTRY", selected)
    request = HostTicketRequest(
        "release-attempt",
        "release-attempt",
        "mac_release.build",
        "release",
        {"target_id": "release"},
    )
    assert request.to_payload()["target"] == "release"
    validate_provider_v1("endpoint_compliance.observe", "scan", {"target_id": "scan"})
    for action, target, args in [
        ("mac_release.codesign", "release", {"target_id": "release"}),
        ("mac_release.build", "other", {"target_id": "other"}),
        ("mac_release.build", "release", {"target_id": "release", "source": "/tmp"}),
        ("mac_release.build", "release", {"target_id": "scan"}),
    ]:
        with pytest.raises(CohesixError):
            validate_provider_v1(action, target, args)
