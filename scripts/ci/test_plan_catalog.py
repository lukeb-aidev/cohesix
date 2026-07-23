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
ACTION_ID_RE = re.compile(r"^[a-z0-9]+(?:[.-][a-z0-9]+)*$")
ALLOWED_SCOPES = {"common", "provisioned-target", "target", "conditional"}
ALLOWED_TARGETS = {"qemu", "pi4"}
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
    if not isinstance(metadata, dict):
        errors.append("missing [catalog] metadata")
        metadata = {}
    if metadata.get("schema") != "cohesix-test-plan-actions/v1":
        errors.append("catalog.schema must be cohesix-test-plan-actions/v1")
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
        if tier and tier not in tiers:
            errors.append(f"{action_id}: unknown tier {tier!r}")
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
    """Return actions matching all supplied selectors, preserving order."""

    actions: list[dict[str, Any]] = data["action"]
    return [
        action
        for action in actions
        if (stage is None or action["stage"] == stage)
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
        stage = str(action["stage"]) if action["stage"] else "conditional"
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


def matching_actions(
    data: dict[str, Any], changed_paths: list[str]
) -> tuple[list[dict[str, Any]], list[str]]:
    """Map changed paths to catalog actions and return unmatched paths."""

    selected: list[dict[str, Any]] = []
    unmatched: list[str] = []
    for changed in changed_paths:
        matches = [
            action
            for action in data["action"]
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
        selected = list(data["action"])
    return selected, unmatched


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
            rendered = markdown_catalog(data)
            if args.write:
                if args.doc is None:
                    raise CatalogError("--write requires --doc")
                doc = args.doc.resolve()
                current = doc.read_text(encoding="utf-8")
                updated = replace_doc_catalog(current, rendered)
                if updated != current:
                    doc.write_text(updated, encoding="utf-8")
            else:
                print(rendered)
        elif args.subcommand == "check-doc":
            current = args.doc.resolve().read_text(encoding="utf-8")
            expected = replace_doc_catalog(current, markdown_catalog(data))
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
    except (CatalogError, OSError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
