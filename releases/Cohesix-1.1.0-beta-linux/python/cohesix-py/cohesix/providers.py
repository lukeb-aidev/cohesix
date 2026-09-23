# Author: Lukas Bower
# Purpose: Expose compiler-owned provider requirements without granting execution or accepting client promotion.
# Copyright 2026 Lukas Bower

"""Read-only projection of the provider extension of the host integration graph."""

from __future__ import annotations

from copy import deepcopy
import re
from typing import Any

from .errors import CohesixError
from .provider_generated import REGISTRY


class ProviderUnavailable(CohesixError):
    """A named provider, action or exact host profile is unavailable."""

    def __init__(self, code: str, provider: str) -> None:
        self.code = code
        self.provider = provider
        super().__init__(f"{code}: {provider}")


def registry() -> dict[str, Any]:
    """Return a detached generated projection; mutation grants no authority."""
    return deepcopy(REGISTRY)


def provider(provider_id: str) -> dict[str, Any]:
    """Resolve one family while preserving its stable 26e integration id."""
    for family in REGISTRY["contract"]["families"]:
        if family["id"] == provider_id:
            return deepcopy(family)
    raise ProviderUnavailable("not_registered", provider_id)


def action(action_id: str) -> dict[str, Any]:
    """Return the generated action contract, never a client-selected live mode."""
    for family in REGISTRY["contract"]["families"]:
        for item in family["actions"]:
            if item["id"] == action_id:
                return deepcopy(item)
    raise ProviderUnavailable("not_registered", action_id)


def profile(profile_id: str) -> dict[str, Any]:
    """Resolve an exact tested-profile requirement without compatibility guesses."""
    for item in REGISTRY["contract"]["profiles"]:
        if item["id"] == profile_id:
            return deepcopy(item)
    raise ProviderUnavailable("not_registered", profile_id)


def field_bus_point(
    endpoint_id: str, point_id: str
) -> tuple[dict[str, Any], dict[str, Any]]:
    """Resolve compiled maps; callers cannot supply functions, addresses or control values."""
    for endpoint in REGISTRY["contract"].get("field_bus", []):
        if endpoint["id"] == endpoint_id:
            for point in endpoint["points"]:
                if point["id"] == point_id:
                    return deepcopy(endpoint), deepcopy(point)
    raise ProviderUnavailable("not_enabled", "field_bus_point")


def launchd_target(target_id: str) -> dict[str, Any]:
    """Resolve a compiler-selected service; client paths and labels grant nothing."""
    for target in REGISTRY["contract"].get("launchd_targets", []):
        if target["id"] == target_id:
            return deepcopy(target)
    raise ProviderUnavailable("not_enabled", "launchd_target")


def validate_target(action_id: str, target: str | None) -> None:
    """Apply the generated grammar before constructing a request; this is not admission."""
    grammar = action(action_id)["target_grammar"]
    if grammar == "none":
        if target is not None:
            raise ProviderUnavailable("invalid_target", action_id)
        return
    if not isinstance(target, str) or len(target) > 128:
        raise ProviderUnavailable("invalid_target", action_id)
    if grammar == "cas_sha256":
        valid = re.fullmatch(r"[0-9a-f]{64}", target)
    else:
        valid = re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}", target)
    if valid is None or ".." in target:
        raise ProviderUnavailable("invalid_target", action_id)


def surface(surface_id: str) -> dict[str, Any]:
    """Resolve a generated integration surface without a client-owned catalog."""
    for item in REGISTRY["contract"]["integration_surfaces"]:
        if item["id"] == surface_id:
            return deepcopy(item)
    raise ProviderUnavailable("not_registered", surface_id)


def read_visibility(path: str) -> tuple[str, str]:
    """Classify all reads with the generated longest component-prefix rule."""
    if (
        not isinstance(path, str)
        or len(path) > 255
        or not path.startswith("/")
        or (
            path != "/"
            and any(
                re.fullmatch(r"[A-Za-z0-9_.][A-Za-z0-9_.-]{0,127}", part) is None
                or part == "."
                or ".." in part
                for part in path[1:].split("/")
            )
        )
    ):
        raise ProviderUnavailable("invalid_target", "read")
    rules = REGISTRY["contract"]["read_visibility"]
    matches = [
        rule
        for rule in rules
        if rule["prefix"] == "/"
        or path == rule["prefix"]
        or path.startswith(rule["prefix"] + "/")
    ]
    selected = max(matches, key=lambda rule: len(rule["prefix"]))
    return selected["class"], selected["owner_binding"]


def macos_target(target_id: str, action: str) -> dict[str, Any]:
    """Resolve an exact compiled macOS action; caller operands cannot change it."""
    for target in REGISTRY["contract"].get("macos_targets", []):
        if target["id"] == target_id and target["operation"]["action"] == action:
            return target
    raise ProviderUnavailable("not_enabled", "macos_target")
