#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Validate, query, and render the canonical Cohesix test action catalog.
# Copyright 2026 Lukas Bower

"""Read and validate the single-source Cohesix test action catalog."""

from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
import pathlib
import re
import shlex
import sys
from typing import Any

import tomllib


ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_CATALOG = ROOT / "configs" / "test_plan_actions.toml"
DEFAULT_DOC = ROOT / "docs" / "TEST_PLAN.md"
DOC_START = "<!-- test-plan-catalog:start -->"
DOC_END = "<!-- test-plan-catalog:end -->"
CONVERGENCE_DOC_START = "<!-- test-plan-convergence:start -->"
CONVERGENCE_DOC_END = "<!-- test-plan-convergence:end -->"
ACTION_ID_RE = re.compile(r"^[a-z0-9]+(?:[.-][a-z0-9]+)*$")
ALLOWED_SCOPES = {"common", "provisioned-target", "target", "conditional"}
ALLOWED_TARGETS = {"qemu", "pi4"}
ALLOWED_EVIDENCE_CLASSES = {"acceptance", "diagnostic"}
CONVERGENCE_PHASE_ORDER = {
    "target-entry": 10,
    "target-canary": 20,
    "focused-regression": 30,
    "focused-host": 30,
    "docs-integrity": 10,
}
ALLOWED_POLICIES = {
    "none",
    "nonzero",
    "artifact-count",
    "manual-evidence",
}


class CatalogError(ValueError):
    """Report an invalid catalog or query."""


def load_catalog(path: pathlib.Path) -> dict[str, Any]:
    """Load a TOML catalog and return its validated data."""

    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise CatalogError(f"{path}: unable to load catalog: {error}") from error
    validate_catalog(data, path)
    return data


def normalize_command(command: str) -> str:
    """Return a stable semantic form for exact duplicate detection."""

    try:
        words = shlex.split(command)
    except ValueError:
        return " ".join(command.split())
    ignored_assignments = {"CARGO_INCREMENTAL=0"}
    return " ".join(word for word in words if word not in ignored_assignments)


def repository_command_paths(command: str) -> list[str]:
    """Return repository-owned script paths referenced by a command."""

    try:
        words = shlex.split(command)
    except ValueError:
        return []
    return sorted(
        {
            word
            for word in words
            if word.startswith(("scripts/", "tools/"))
            and not any(character in word for character in "*?[]")
        }
    )


def cargo_test_shape(command: str) -> dict[str, Any] | None:
    """Return the package/feature shape of one Cargo test command."""

    try:
        words = shlex.split(command)
    except ValueError:
        return None
    try:
        cargo_index = words.index("cargo")
    except ValueError:
        return None
    if cargo_index + 1 >= len(words) or words[cargo_index + 1] != "test":
        return None
    cargo_words = words[cargo_index + 2 :]
    if "--" in cargo_words:
        cargo_words = cargo_words[: cargo_words.index("--")]
    package = ""
    excludes: set[str] = set()
    features: set[str] = set()
    workspace = False
    no_default_features = False
    all_features = False
    index = 0
    while index < len(cargo_words):
        word = cargo_words[index]
        if word == "--workspace":
            workspace = True
        elif word in {"-p", "--package"} and index + 1 < len(cargo_words):
            index += 1
            package = cargo_words[index]
        elif word.startswith("--package="):
            package = word.split("=", maxsplit=1)[1]
        elif word == "--exclude" and index + 1 < len(cargo_words):
            index += 1
            excludes.add(cargo_words[index])
        elif word.startswith("--exclude="):
            excludes.add(word.split("=", maxsplit=1)[1])
        elif word in {"-F", "--features"} and index + 1 < len(cargo_words):
            index += 1
            features.update(
                value
                for value in re.split(r"[\s,]+", cargo_words[index])
                if value
            )
        elif word.startswith("--features="):
            features.update(
                value
                for value in re.split(
                    r"[\s,]+",
                    word.split("=", maxsplit=1)[1],
                )
                if value
            )
        elif word == "--no-default-features":
            no_default_features = True
        elif word == "--all-features":
            all_features = True
        index += 1
    return {
        "workspace": workspace,
        "package": package,
        "excludes": excludes,
        "features": features,
        "no_default_features": no_default_features,
        "all_features": all_features,
    }


def action_payload(action: dict[str, Any]) -> bytes:
    """Return the canonical JSON representation of one action."""

    return json.dumps(
        action,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=True,
    ).encode("utf-8")


def catalog_digest(data: dict[str, Any]) -> str:
    """Return a deterministic digest for the validated catalog."""

    return hashlib.sha256(
        json.dumps(
            data,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=True,
        ).encode("utf-8")
    ).hexdigest()


def _require_string(
    action: dict[str, Any], field: str, action_id: str, errors: list[str]
) -> str:
    value = action.get(field)
    if not isinstance(value, str) or not value.strip():
        errors.append(f"{action_id}: {field} must be a non-empty string")
        return ""
    return value


def _require_string_list(
    action: dict[str, Any], field: str, action_id: str, errors: list[str]
) -> list[str]:
    value = action.get(field)
    if (
        not isinstance(value, list)
        or not value
        or any(not isinstance(item, str) or not item for item in value)
    ):
        errors.append(f"{action_id}: {field} must be a non-empty string list")
        return []
    return value


def validate_catalog(data: dict[str, Any], path: pathlib.Path) -> None:
    """Fail when the catalog is incomplete, ambiguous, or duplicates work."""

    errors: list[str] = []
    metadata = data.get("catalog")
    actions = data.get("action")
    focuses = data.get("convergence_focus", [])
    if not isinstance(metadata, dict):
        errors.append("missing [catalog] metadata")
        metadata = {}
    if metadata.get("schema") != "cohesix-test-plan-actions/v2":
        errors.append("catalog.schema must be cohesix-test-plan-actions/v2")
    tiers = metadata.get("claim_tiers")
    if (
        not isinstance(tiers, list)
        or not tiers
        or any(not isinstance(tier, str) or not tier for tier in tiers)
        or len(tiers) != len(set(tiers))
    ):
        errors.append("catalog.claim_tiers must be a unique non-empty string list")
        tiers = []
    default_claims = metadata.get("default_claims")
    if (
        not isinstance(default_claims, list)
        or not default_claims
        or any(
            not isinstance(claim, str) or claim not in tiers
            for claim in default_claims
        )
        or len(default_claims) != len(set(default_claims))
    ):
        errors.append(
            "catalog.default_claims must be a unique non-empty subset of "
            "catalog.claim_tiers"
        )
    if not isinstance(actions, list) or not actions:
        errors.append("catalog must contain at least one [[action]]")
        actions = []
    if not isinstance(focuses, list):
        errors.append("convergence_focus must be a list of tables")
        focuses = []

    focus_ids: set[str] = set()
    focus_targets: dict[str, set[str]] = {}
    focus_action_counts: dict[str, int] = {}
    for index, raw_focus in enumerate(focuses, start=1):
        if not isinstance(raw_focus, dict):
            errors.append(f"convergence focus #{index} must be a table")
            continue
        focus_id = _require_string(
            raw_focus,
            "id",
            f"convergence focus #{index}",
            errors,
        )
        if focus_id and not ACTION_ID_RE.fullmatch(focus_id):
            errors.append(f"{focus_id}: convergence focus id has invalid syntax")
        if focus_id in focus_ids:
            errors.append(f"{focus_id}: duplicate convergence focus id")
        focus_ids.add(focus_id)
        targets = _require_string_list(
            raw_focus,
            "targets",
            focus_id,
            errors,
        )
        unknown_targets = sorted(set(targets) - ALLOWED_TARGETS)
        if unknown_targets:
            errors.append(
                f"{focus_id}: convergence focus has unknown targets "
                f"{unknown_targets}"
            )
        focus_targets[focus_id] = set(targets)
        focus_action_counts[focus_id] = 0
        _require_string(raw_focus, "description", focus_id, errors)
        _require_string(raw_focus, "profile", focus_id, errors)
        _require_string(
            raw_focus,
            "authoritative_evidence",
            focus_id,
            errors,
        )
        _require_string_list(raw_focus, "trigger_paths", focus_id, errors)
        priority = raw_focus.get("priority")
        if (
            not isinstance(priority, int)
            or isinstance(priority, bool)
            or priority <= 0
        ):
            errors.append(
                f"{focus_id}: convergence focus priority must be a positive integer"
            )

    seen_ids: set[str] = set()
    seen_commands: dict[str, str] = {}
    cargo_test_shapes: list[tuple[str, dict[str, Any]]] = []
    stage_ids: dict[int, list[str]] = {stage: [] for stage in range(1, 6)}
    for index, raw_action in enumerate(actions, start=1):
        if not isinstance(raw_action, dict):
            errors.append(f"action #{index} must be a table")
            continue
        action = raw_action
        action_id = _require_string(action, "id", f"action #{index}", errors)
        if action_id and not ACTION_ID_RE.fullmatch(action_id):
            errors.append(f"{action_id}: id has invalid syntax")
        if action_id in seen_ids:
            errors.append(f"{action_id}: duplicate action id")
        seen_ids.add(action_id)

        stage = action.get("stage")
        if not isinstance(stage, int) or isinstance(stage, bool) or stage not in range(0, 6):
            errors.append(f"{action_id}: stage must be an integer from 0 through 5")
        elif stage:
            stage_ids[stage].append(action_id)

        tier = _require_string(action, "tier", action_id, errors)
        evidence_class = action.get("evidence_class", "acceptance")
        if evidence_class not in ALLOWED_EVIDENCE_CLASSES:
            errors.append(
                f"{action_id}: evidence_class must be one of "
                f"{sorted(ALLOWED_EVIDENCE_CLASSES)}"
            )
        if evidence_class == "acceptance" and tier and tier not in tiers:
            errors.append(f"{action_id}: unknown tier {tier!r}")
        if evidence_class == "diagnostic" and tier != "non-claiming":
            errors.append(
                f"{action_id}: diagnostic actions must use tier='non-claiming'"
            )
        scope = _require_string(action, "scope", action_id, errors)
        if scope and scope not in ALLOWED_SCOPES:
            errors.append(f"{action_id}: unknown scope {scope!r}")
        targets = _require_string_list(action, "targets", action_id, errors)
        unknown_targets = sorted(set(targets) - ALLOWED_TARGETS)
        if unknown_targets:
            errors.append(f"{action_id}: unknown targets {unknown_targets}")
        _require_string(action, "description", action_id, errors)
        _require_string_list(action, "trigger_paths", action_id, errors)
        _require_string_list(action, "expected_evidence", action_id, errors)

        timeout = action.get("timeout_seconds")
        if not isinstance(timeout, int) or isinstance(timeout, bool) or timeout <= 0:
            errors.append(f"{action_id}: timeout_seconds must be a positive integer")
        policy = _require_string(action, "test_policy", action_id, errors)
        if policy and policy not in ALLOWED_POLICIES:
            errors.append(f"{action_id}: unknown test_policy {policy!r}")
        manual = action.get("manual", False)
        if not isinstance(manual, bool):
            errors.append(f"{action_id}: manual must be a boolean")
        command = action.get("command")
        if not isinstance(command, str):
            errors.append(f"{action_id}: command must be a string")
            command = ""
        if manual:
            if stage != 0 or policy != "manual-evidence":
                errors.append(
                    f"{action_id}: manual evidence actions must use stage=0 "
                    "and test_policy=manual-evidence"
                )
        elif not command.strip():
            errors.append(f"{action_id}: executable action has an empty command")
        if stage == 0 and scope != "conditional":
            errors.append(f"{action_id}: stage 0 actions must use conditional scope")
        if stage and scope == "conditional":
            errors.append(f"{action_id}: staged actions cannot use conditional scope")
        if evidence_class == "diagnostic" and (
            stage != 0 or scope != "conditional" or manual
        ):
            errors.append(
                f"{action_id}: diagnostic actions must be executable "
                "stage-0 conditional actions"
            )

        convergence_focuses = action.get("convergence_focuses", [])
        convergence_phase = action.get("convergence_phase")
        if convergence_focuses:
            if (
                not isinstance(convergence_focuses, list)
                or any(
                    not isinstance(focus, str) or not focus
                    for focus in convergence_focuses
                )
                or len(convergence_focuses) != len(set(convergence_focuses))
            ):
                errors.append(
                    f"{action_id}: convergence_focuses must be a unique "
                    "non-empty string list"
                )
                convergence_focuses = []
            unknown_focuses = sorted(set(convergence_focuses) - focus_ids)
            if unknown_focuses:
                errors.append(
                    f"{action_id}: unknown convergence focuses {unknown_focuses}"
                )
            if convergence_phase not in CONVERGENCE_PHASE_ORDER:
                errors.append(
                    f"{action_id}: convergence_phase must be one of "
                    f"{sorted(CONVERGENCE_PHASE_ORDER)}"
                )
            for focus in convergence_focuses:
                focus_action_counts[focus] = focus_action_counts.get(focus, 0) + 1
                if focus in focus_targets and not (
                    set(targets) & focus_targets[focus]
                ):
                    errors.append(
                        f"{action_id}: target set does not overlap convergence "
                        f"focus {focus}"
                    )
        elif convergence_phase is not None:
            errors.append(
                f"{action_id}: convergence_phase requires convergence_focuses"
            )

        if policy == "nonzero":
            minimum = action.get("minimum_test_count")
            if (
                not isinstance(minimum, int)
                or isinstance(minimum, bool)
                or minimum <= 0
            ):
                errors.append(
                    f"{action_id}: nonzero policy requires positive "
                    "minimum_test_count"
                )
            if "cargo test" in command:
                before_harness = command.split(" -- ", maxsplit=1)[0]
                if re.search(r"\s--lib\s+\S+", before_harness):
                    errors.append(
                        f"{action_id}: filtered --lib tests are forbidden; "
                        "catalog a complete feature suite"
                    )
                try:
                    words = shlex.split(command)
                except ValueError:
                    words = []
                if "--lib" in words and "--" in words:
                    harness_words = words[words.index("--") + 1 :]
                    position = 0
                    while position < len(harness_words):
                        word = harness_words[position]
                        if word in {
                            "--skip",
                            "--test-threads",
                            "--color",
                            "--format",
                        }:
                            position += 2
                            continue
                        if word.startswith("-"):
                            position += 1
                            continue
                        errors.append(
                            f"{action_id}: filtered --lib tests are forbidden; "
                            "catalog a complete feature suite"
                        )
                        break
        elif "minimum_test_count" in action:
            errors.append(
                f"{action_id}: minimum_test_count is only valid for nonzero policy"
            )

        normalized = normalize_command(command)
        if normalized and not manual:
            prior = seen_commands.get(normalized)
            if prior and not action.get("allow_duplicate_command", False):
                errors.append(
                    f"{action_id}: duplicates command owned by {prior}; "
                    "use one action or document target dispatch explicitly"
                )
            else:
                seen_commands.setdefault(normalized, action_id)
        test_shape = cargo_test_shape(command)
        if test_shape is not None:
            cargo_test_shapes.append((action_id, test_shape))
        for relative in repository_command_paths(command):
            referenced = path.parent.parent / relative
            if not referenced.is_file():
                errors.append(
                    f"{action_id}: command references missing repository file "
                    f"{relative}"
                )

    workspace_shapes = [
        (action_id, shape)
        for action_id, shape in cargo_test_shapes
        if shape["workspace"]
        and not shape["features"]
        and not shape["no_default_features"]
        and not shape["all_features"]
    ]
    for action_id, shape in cargo_test_shapes:
        package = shape["package"]
        if (
            not package
            or shape["workspace"]
            or shape["features"]
            or shape["no_default_features"]
            or shape["all_features"]
        ):
            continue
        for workspace_id, workspace_shape in workspace_shapes:
            if package not in workspace_shape["excludes"]:
                errors.append(
                    f"{action_id}: default-feature package tests overlap "
                    f"{workspace_id}; exclude {package} from the workspace "
                    "action or remove the duplicate package action"
                )

    for stage, ids in stage_ids.items():
        if not ids:
            errors.append(f"stage {stage} has no catalog action")
    for focus_id, count in focus_action_counts.items():
        if count == 0:
            errors.append(
                f"{focus_id}: convergence focus has no executable catalog action"
            )

    if errors:
        formatted = "\n".join(f"  - {error}" for error in errors)
        raise CatalogError(f"{path}: invalid test-plan catalog:\n{formatted}")


def select_actions(
    data: dict[str, Any],
    *,
    stage: int | None = None,
    scope: str | None = None,
    target: str | None = None,
    tier: str | None = None,
) -> list[dict[str, Any]]:
    """Return acceptance actions matching selectors, preserving order."""

    actions: list[dict[str, Any]] = data["action"]
    return [
        action
        for action in actions
        if action.get("evidence_class", "acceptance") == "acceptance"
        and (stage is None or action["stage"] == stage)
        and (scope is None or action["scope"] == scope)
        and (target is None or target in action["targets"])
        and (tier is None or action["tier"] == tier)
    ]


def find_action(data: dict[str, Any], action_id: str) -> dict[str, Any]:
    """Return one action or fail with a useful diagnostic."""

    matches = [action for action in data["action"] if action["id"] == action_id]
    if not matches:
        raise CatalogError(f"unknown action id: {action_id}")
    return matches[0]


def markdown_catalog(data: dict[str, Any]) -> str:
    """Render the canonical compact action table for TEST_PLAN.md."""

    rows = [
        DOC_START,
        "| Action | Stage | Claim tier | Scope / target | Command or proof |",
        "| --- | ---: | --- | --- | --- |",
    ]
    for action in data["action"]:
        evidence_class = action.get("evidence_class", "acceptance")
        stage = str(action["stage"]) if action["stage"] else "conditional"
        if evidence_class == "diagnostic":
            stage = "NON-CLAIMING diagnostic"
        target = ", ".join(action["targets"])
        scope_target = f"{action['scope']} / {target}"
        proof = (
            "evidence-only: " + ", ".join(action["expected_evidence"])
            if action.get("manual", False)
            else f"`{action['command']}`"
        )
        rows.append(
            f"| `{action['id']}` | {stage} | `{action['tier']}` | "
            f"{scope_target} | {proof} |"
        )
    rows.append(DOC_END)
    return "\n".join(rows)


def markdown_convergence(data: dict[str, Any]) -> str:
    """Render convergence routing from the same canonical catalog."""

    rows = [
        CONVERGENCE_DOC_START,
        "| Focus | Target | First authoritative evidence | Exact profile |",
        "| --- | --- | --- | --- |",
    ]
    for focus in data["convergence_focus"]:
        rows.append(
            f"| `{focus['id']}` | {', '.join(focus['targets'])} | "
            f"{focus['authoritative_evidence']} | `{focus['profile']}` |"
        )
    rows.append(CONVERGENCE_DOC_END)
    return "\n".join(rows)


def replace_doc_catalog(text: str, rendered: str) -> str:
    """Replace the generated catalog block in a document."""

    start = text.find(DOC_START)
    end = text.find(DOC_END)
    if start < 0 or end < 0 or end < start:
        raise CatalogError(
            f"document must contain one {DOC_START} ... {DOC_END} block"
        )
    if text.find(DOC_START, start + 1) >= 0 or text.find(DOC_END, end + 1) >= 0:
        raise CatalogError("document contains duplicate test-plan catalog markers")
    end += len(DOC_END)
    return text[:start] + rendered + text[end:]


def replace_generated_block(
    text: str,
    *,
    start_marker: str,
    end_marker: str,
    rendered: str,
) -> str:
    """Replace one uniquely marked generated documentation block."""

    start = text.find(start_marker)
    end = text.find(end_marker)
    if start < 0 or end < 0 or end < start:
        raise CatalogError(
            f"document must contain one {start_marker} ... {end_marker} block"
        )
    if (
        text.find(start_marker, start + 1) >= 0
        or text.find(end_marker, end + 1) >= 0
    ):
        raise CatalogError(f"document contains duplicate {start_marker} markers")
    end += len(end_marker)
    return text[:start] + rendered + text[end:]


def render_document(data: dict[str, Any], text: str) -> str:
    """Refresh both catalog-owned TEST_PLAN.md blocks."""

    updated = replace_doc_catalog(text, markdown_catalog(data))
    return replace_generated_block(
        updated,
        start_marker=CONVERGENCE_DOC_START,
        end_marker=CONVERGENCE_DOC_END,
        rendered=markdown_convergence(data),
    )


def matching_actions(
    data: dict[str, Any], changed_paths: list[str]
) -> tuple[list[dict[str, Any]], list[str]]:
    """Map changed paths to acceptance actions and return unmatched paths."""

    acceptance_actions = [
        action
        for action in data["action"]
        if action.get("evidence_class", "acceptance") == "acceptance"
    ]
    selected: list[dict[str, Any]] = []
    unmatched: list[str] = []
    for changed in changed_paths:
        matches = [
            action
            for action in acceptance_actions
            if any(
                fnmatch.fnmatchcase(changed, pattern)
                or pathlib.PurePosixPath(changed).match(pattern)
                for pattern in action["trigger_paths"]
            )
        ]
        if not matches:
            unmatched.append(changed)
        for action in matches:
            if action not in selected:
                selected.append(action)
    if unmatched:
        selected = acceptance_actions
    return selected, unmatched


def path_matches(path: str, patterns: list[str]) -> bool:
    """Return whether one repository path matches any catalog pattern."""

    return any(
        fnmatch.fnmatchcase(path, pattern)
        or pathlib.PurePosixPath(path).match(pattern)
        for pattern in patterns
    )


def find_focus(data: dict[str, Any], focus_id: str) -> dict[str, Any]:
    """Return one convergence focus or fail with a useful diagnostic."""

    matches = [
        focus
        for focus in data["convergence_focus"]
        if focus["id"] == focus_id
    ]
    if not matches:
        raise CatalogError(f"unknown convergence focus: {focus_id}")
    return matches[0]


def select_convergence_focus(
    data: dict[str, Any],
    *,
    target: str,
    changed_paths: list[str],
) -> tuple[dict[str, Any], list[str]]:
    """Select the highest-priority compatible focus for changed paths."""

    if target not in ALLOWED_TARGETS:
        raise CatalogError(f"unknown convergence target: {target}")
    if not changed_paths:
        raise CatalogError(
            "automatic convergence selection requires at least one changed path"
        )
    path_focuses: list[dict[str, Any]] = []
    unmatched: list[str] = []
    for changed in changed_paths:
        matches = [
            focus
            for focus in data["convergence_focus"]
            if path_matches(changed, focus["trigger_paths"])
        ]
        if not matches:
            unmatched.append(changed)
            continue
        selected_for_path = max(matches, key=lambda item: item["priority"])
        if target not in selected_for_path["targets"]:
            raise CatalogError(
                f"{changed}: first authoritative convergence focus "
                f"{selected_for_path['id']} requires target "
                f"{', '.join(selected_for_path['targets'])}, not {target}"
            )
        path_focuses.append(selected_for_path)
    if unmatched:
        raise CatalogError(
            "automatic convergence selection has unmatched paths; choose "
            "--focus explicitly after changed-path analysis: "
            + ", ".join(unmatched)
        )
    selected = max(path_focuses, key=lambda item: item["priority"])
    return selected, changed_paths


def convergence_actions(
    data: dict[str, Any], *, target: str, focus_id: str
) -> list[dict[str, Any]]:
    """Return the ordered target-entry, canary, and focused guard actions."""

    focus = find_focus(data, focus_id)
    if target not in focus["targets"]:
        raise CatalogError(
            f"convergence focus {focus_id} does not support target {target}"
        )
    indexed = [
        (index, action)
        for index, action in enumerate(data["action"])
        if focus_id in action.get("convergence_focuses", [])
        and target in action["targets"]
    ]
    return [
        action
        for _, action in sorted(
            indexed,
            key=lambda item: (
                CONVERGENCE_PHASE_ORDER[item[1]["convergence_phase"]],
                item[0],
            ),
        )
    ]


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse command-line arguments."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", type=pathlib.Path, default=DEFAULT_CATALOG)
    subparsers = parser.add_subparsers(dest="subcommand", required=True)
    subparsers.add_parser("validate")
    subparsers.add_parser("digest")

    list_parser = subparsers.add_parser("list")
    list_parser.add_argument("--stage", type=int)
    list_parser.add_argument("--scope", choices=sorted(ALLOWED_SCOPES))
    list_parser.add_argument("--target", choices=sorted(ALLOWED_TARGETS))
    list_parser.add_argument("--tier")
    list_parser.add_argument(
        "--format", choices=("ids", "json", "markdown"), default="ids"
    )

    action_parser = subparsers.add_parser("action")
    action_parser.add_argument("--id", required=True)
    action_parser.add_argument(
        "--field",
        choices=(
            "json",
            "command",
            "timeout_seconds",
            "test_policy",
            "minimum_test_count",
            "convergence_phase",
            "evidence_class",
            "digest",
        ),
        default="json",
    )

    render_parser = subparsers.add_parser("render-doc")
    render_parser.add_argument("--doc", type=pathlib.Path)
    render_parser.add_argument("--write", action="store_true")

    check_parser = subparsers.add_parser("check-doc")
    check_parser.add_argument("--doc", type=pathlib.Path, default=DEFAULT_DOC)

    recommend_parser = subparsers.add_parser("recommend")
    recommend_parser.add_argument("paths", nargs="*")
    recommend_parser.add_argument(
        "--stdin0",
        action="store_true",
        help="read NUL-delimited changed paths from stdin",
    )
    recommend_parser.add_argument(
        "--format", choices=("ids", "json", "tiers"), default="ids"
    )

    converge_parser = subparsers.add_parser("converge")
    converge_parser.add_argument(
        "--target", choices=sorted(ALLOWED_TARGETS), required=True
    )
    converge_parser.add_argument("--focus")
    converge_parser.add_argument("paths", nargs="*")
    converge_parser.add_argument(
        "--format", choices=("ids", "json"), default="ids"
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the requested catalog operation."""

    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        data = load_catalog(args.catalog.resolve())
        if args.subcommand == "validate":
            print(
                f"test-plan catalog ok: actions={len(data['action'])} "
                f"digest={catalog_digest(data)}"
            )
        elif args.subcommand == "digest":
            print(catalog_digest(data))
        elif args.subcommand == "list":
            actions = select_actions(
                data,
                stage=args.stage,
                scope=args.scope,
                target=args.target,
                tier=args.tier,
            )
            if args.format == "ids":
                print("\n".join(action["id"] for action in actions))
            elif args.format == "json":
                print(json.dumps(actions, indent=2, sort_keys=True))
            else:
                print(markdown_catalog({"action": actions}))
        elif args.subcommand == "action":
            action = find_action(data, args.id)
            if args.field == "json":
                print(json.dumps(action, indent=2, sort_keys=True))
            elif args.field == "digest":
                print(hashlib.sha256(action_payload(action)).hexdigest())
            else:
                value = action.get(args.field, "")
                print(value)
        elif args.subcommand == "render-doc":
            if args.write:
                if args.doc is None:
                    raise CatalogError("--write requires --doc")
                doc = args.doc.resolve()
                current = doc.read_text(encoding="utf-8")
                updated = render_document(data, current)
                if updated != current:
                    doc.write_text(updated, encoding="utf-8")
            else:
                print(markdown_catalog(data))
                print()
                print(markdown_convergence(data))
        elif args.subcommand == "check-doc":
            current = args.doc.resolve().read_text(encoding="utf-8")
            expected = render_document(data, current)
            if current != expected:
                raise CatalogError(
                    f"{args.doc}: generated action catalog is stale; run "
                    "scripts/ci/test_plan_catalog.py render-doc "
                    f"--doc {args.doc} --write"
                )
            print("test-plan catalog documentation is current")
        elif args.subcommand == "recommend":
            changed_paths = list(args.paths)
            if args.stdin0:
                stdin_paths = [
                    path
                    for path in sys.stdin.read().split("\0")
                    if path
                ]
                changed_paths.extend(stdin_paths)
            if not changed_paths:
                raise CatalogError(
                    "recommend requires changed paths or --stdin0 input"
                )
            actions, unmatched = matching_actions(data, changed_paths)
            if args.format == "json":
                print(
                    json.dumps(
                        {
                            "actions": [action["id"] for action in actions],
                            "tiers": sorted({action["tier"] for action in actions}),
                            "unmatched_paths": unmatched,
                        },
                        indent=2,
                        sort_keys=True,
                    )
                )
            elif args.format == "tiers":
                print("\n".join(sorted({action["tier"] for action in actions})))
            else:
                print("\n".join(action["id"] for action in actions))
            if unmatched:
                print(
                    "unmatched paths select the full catalog: "
                    + ", ".join(unmatched),
                    file=sys.stderr,
                )
        elif args.subcommand == "converge":
            if args.focus:
                focus = find_focus(data, args.focus)
                selected_paths = list(args.paths)
            else:
                focus, selected_paths = select_convergence_focus(
                    data,
                    target=args.target,
                    changed_paths=list(args.paths),
                )
            actions = convergence_actions(
                data,
                target=args.target,
                focus_id=focus["id"],
            )
            if args.format == "json":
                print(
                    json.dumps(
                        {
                            "target": args.target,
                            "focus": focus,
                            "changed_paths": selected_paths,
                            "actions": actions,
                        },
                        indent=2,
                        sort_keys=True,
                    )
                )
            else:
                print("\n".join(action["id"] for action in actions))
    except (CatalogError, OSError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
