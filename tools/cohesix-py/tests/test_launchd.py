# Author: Lukas Bower
# Purpose: Keep SDK launchd operands bound to compiled target ids without native path or label authority.
# Copyright 2026 Lukas Bower
"""Pure generated-map boundary; no native service or admission claim."""

from copy import deepcopy

import pytest

from cohesix import providers
from cohesix.authority import validate_provider_v1
from cohesix.errors import CohesixError


def test_launchd_requires_one_compiled_service_and_exact_target(monkeypatch):
    selected = deepcopy(providers.REGISTRY)
    selected["contract"]["launchd_targets"] = [{"id": "owned-service"}]
    monkeypatch.setattr(providers, "REGISTRY", selected)
    for action in ("start", "stop", "restart", "status-check"):
        validate_provider_v1(
            "launchd." + action, "owned-service", {"service": "owned-service"}
        )
    for target, args in [
        ("unknown", {"service": "unknown"}),
        ("owned-service", {"service": "another"}),
        ("/host/launchd/owned-service", {"service": "owned-service"}),
        ("owned-service", {"service": "owned-service", "domain": "system"}),
        ("owned-service", {"service": "owned-service", "plist": "/tmp/owned.plist"}),
        ("owned-service", {}),
    ]:
        with pytest.raises(CohesixError):
            validate_provider_v1("launchd.restart", target, args)
