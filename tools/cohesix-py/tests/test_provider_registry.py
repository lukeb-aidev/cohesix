# Author: Lukas Bower
# Purpose: Preserve generated provider projection parity and reject client-selected targets or promotion.
# Copyright 2026 Lukas Bower

from pathlib import Path
import json

import pytest

from cohesix.providers import (
    ProviderUnavailable, action, profile, provider, registry, validate_target,
)
from cohesix.native_providers import docker_api_version, match_jetson_profile, parse_packages


def test_native_capture_reaps_overflowing_children_and_preserves_stdout():
    import sys
    from cohesix.native_providers import bounded_command
    assert bounded_command([sys.executable, "-c", "print('native')"]) == b"native\n"
    with pytest.raises(ProviderUnavailable, match="byte_limit"):
        bounded_command([sys.executable, "-c", "import os; os.write(1, b'x' * 65537)"])
    with pytest.raises(ProviderUnavailable, match="invalid_probe"):
        bounded_command(["relative-command"])


def test_provider_registry_is_exact_compiler_projection():
    root = Path(__file__).resolve().parents[3]
    expected = json.loads((root / "configs/generated/provider_registry.json").read_bytes())
    assert registry() == expected
    assert provider("systemd")["integration_id"] == "systemd-provider"
    contract = action("systemd.restart")
    assert contract["admission_mode"] == "operator_approved"
    assert contract["decision_requirement"] == "unavailable"
    assert contract["maximum_grant_scope"] == ["systemd.restart"]
    assert [phase["id"] for phase in contract["lifecycle"]] == [
        "discover", "preflight", "execute", "observe", "verify", "compensate", "export_evidence",
    ]


def test_client_mutation_cannot_change_generated_provider_truth():
    copy = registry()
    copy["contract"]["families"][0]["availability"] = "live"
    assert all(row["availability"] != "live" for row in registry()["contract"]["families"])
    copy = action("systemd.restart")
    copy["operator_approval_required"] = False
    assert action("systemd.restart")["operator_approval_required"] is True


@pytest.mark.parametrize("value", ["-root", "../ssh", "a/b", "a\nb", "a;echo", "", "x" * 129])
def test_provider_registry_refuses_untrusted_native_targets(value):
    with pytest.raises(ProviderUnavailable, match="invalid_target"):
        validate_target("systemd.restart", value)


def test_provider_registry_keeps_discovery_and_action_unavailability_explicit():
    validate_target("systemd.restart", "cohesix-canary.service")
    validate_target("jetson.discover", None)
    with pytest.raises(ProviderUnavailable, match="invalid_target"):
        validate_target("jetson.discover", "arbitrary-path")
    for lookup, value in ((action, "systemd.shell"), (provider, "unknown"), (profile, "unqualified")):
        with pytest.raises(ProviderUnavailable, match="not_registered"):
            lookup(value)


def test_exact_installed_reference_does_not_accept_an_adjacent_toolkit_version():
    expected = profile("jetson-orin-nano-jp7")
    observed = {field: expected[field] for field in
                ("os", "architecture", "board", "ubuntu", "l4t", "jetpack", "cuda_toolkit")}
    assert expected["cuda_toolkit"] == "13.2.2"
    assert match_jetson_profile(observed, expected) == []
    observed["cuda_toolkit"] = "13.2.1"
    assert match_jetson_profile(observed, expected) == ["cuda_toolkit"]
    del observed["board"]
    assert match_jetson_profile(observed, expected) == ["board", "cuda_toolkit"]


def test_native_package_rows_preserve_exact_versions_and_reject_ambiguity():
    assert parse_packages(b"nvidia-jetpack\t7.2.1-b49\n") == {"nvidia-jetpack": "7.2.1-b49"}
    for raw in (b"package\t1\npackage\t2\n", b"package 1\n", b"package\t\n", b"\xff\t1\n"):
        with pytest.raises(ProviderUnavailable, match="invalid_observation"):
            parse_packages(raw)


@pytest.mark.parametrize("minimum,maximum,success", [
    ("1.24", "1.55", True), ("1.40", "1.40", True),
    ("1.41", "1.55", False), ("1.24", "1.39", False),
    ("1.55", "1.40", False), ("garbage", "1.55", False),
])
def test_docker_api_negotiation_refuses_unsupported_or_malformed_intervals(minimum, maximum, success):
    value = {"MinAPIVersion": minimum, "ApiVersion": maximum}
    if success:
        assert docker_api_version(value) == "1.40"
    else:
        with pytest.raises(ProviderUnavailable):
            docker_api_version(value)
