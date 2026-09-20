#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Separate publication-only changes from an immutable qualified release source.
# Copyright 2026 Lukas Bower

"""Reject runtime drift before reusing exact tested release artifacts."""

from __future__ import annotations

import argparse
import ast
import hashlib
from pathlib import Path
import subprocess
import sys
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent / "ci"))
import qemu_artifact as evidence  # noqa: E402


INVENTORY = "configs/generated/implementation_surface_inventory.json"
GRAPH = "configs/generated/host_integration_dependency.json"
DOCUMENTS = frozenset({
    "README.md", "docs/BUILD_PLAN.md", "docs/TEST_PLAN.md", "docs/FAILOVER.md",
    "docs/HOST_TOOLS.md", "docs/QUICKSTART.md", "docs/REPO_LAYOUT.md",
    "docs/STATUS.md", "releases/RELEASE_NOTES-1.1.0-beta.md",
})
FACTORY = frozenset({
    "scripts/release_bundle.sh", "scripts/release_inputs.py",
    "scripts/release_publication.py", "scripts/release_qualify.py",
})
HELP_ONLY = frozenset({
    "scripts/failover_watchdog.py", "scripts/rest_perf_harness.py",
})


def git(root: Path, *args: str) -> bytes:
    """Run a read-only Git operation with separate arguments."""

    return subprocess.check_output(["git", "-C", str(root), *args])


def clean_tree(root: Path) -> tuple[str, dict[str, tuple[str, str]]]:
    """Require a complete, clean Git root and retain tracked modes and objects."""

    if Path(git(root, "rev-parse", "--show-toplevel").decode().strip()).resolve() != root:
        raise evidence.EvidenceError("publication inputs must be Git checkout roots")
    if git(root, "status", "--porcelain=v1", "--untracked-files=all"):
        raise evidence.EvidenceError(f"publication requires a clean checkout: {root}")
    records = {}
    for entry in git(root, "ls-tree", "-r", "-z", "HEAD").split(b"\0"):
        if entry:
            meta, name = entry.split(b"\t", 1)
            mode, _kind, object_id = meta.decode().split()
            records[name.decode()] = (mode, object_id)
    return git(root, "rev-parse", "HEAD").decode().strip(), records


def normalized_help(source: str) -> str:
    """Ignore only the module docstring and literal argparse help text."""

    module = ast.parse(source)
    if (
        module.body and isinstance(module.body[0], ast.Expr)
        and isinstance(module.body[0].value, ast.Constant)
        and isinstance(module.body[0].value.value, str)
    ):
        module.body.pop(0)
    for node in ast.walk(module):
        if (
            isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute)
            and node.func.attr == "add_argument"
        ):
            for keyword in node.keywords:
                if (
                    keyword.arg == "help" and isinstance(keyword.value, ast.Constant)
                    and isinstance(keyword.value.value, str)
                ):
                    keyword.value = ast.Constant(value="")
    return ast.dump(module, include_attributes=False)


def normalized_inventory(value: dict[str, Any]) -> dict[str, Any]:
    """Ignore only retired distribution README rows, never release selection."""

    value = dict(value)
    value["tracked_surfaces"] = [
        row for row in value["tracked_surfaces"]
        if not (
            row.get("path", "").startswith("releases/Cohesix-0.")
            and row.get("path", "").endswith("/README.md")
            and row.get("package_disposition") == "historical_release_only"
            and row.get("production_reachable") is False
        )
    ]
    return value


def classify_change(root: Path, qualified: Path, path: str, deleted: bool) -> str:
    """Admit the owner-approved publication delta; all runtime changes fail."""

    if deleted and path.startswith(("releases/Cohesix-0.", "releases/RELEASE_NOTES-0.")):
        return "obsolete-distribution-removal"
    if deleted:
        raise evidence.EvidenceError(f"publication cannot delete a current input: {path}")
    if path in DOCUMENTS:
        return "release-documentation"
    if path == ".gitignore":
        return "local-output-exclusions"
    if path in FACTORY:
        return "publication-tooling"
    if path in HELP_ONLY and normalized_help((root / path).read_text()) == normalized_help(
        (qualified / path).read_text()
    ):
        return "operator-help-only"
    if path == INVENTORY:
        current = evidence.read_json(root / path)
        previous = evidence.read_json(qualified / path)
        if normalized_inventory(current) == normalized_inventory(previous):
            return "retired-document-inventory"
    if path == GRAPH:
        current = evidence.read_json(root / path)
        previous = evidence.read_json(qualified / path)
        field = "implementation_surface_inventory_sha256"
        for document, tree in ((current, root), (previous, qualified)):
            expected = hashlib.sha256((tree / INVENTORY).read_bytes()).hexdigest()
            if document["meta"][field] != expected:
                raise evidence.EvidenceError("host graph has an invalid inventory binding")
            document["meta"][field] = ""
        if current == previous:
            return "retired-document-inventory-binding"
    raise evidence.EvidenceError(f"qualified runtime or contract changed: {path}")


def publication_record(root: Path, qualified: Path) -> dict[str, Any]:
    """Bind two clean commits without rewriting qualification source identity."""

    commit, current = clean_tree(root)
    qualified_commit, previous = clean_tree(qualified)
    ancestor = subprocess.run(
        ["git", "-C", str(root), "merge-base", "--is-ancestor", qualified_commit, commit],
        check=False, capture_output=True,
    )
    if ancestor.returncode:
        raise evidence.EvidenceError("qualified source must be an ancestor of publication")
    changes = []
    for path in sorted(current.keys() | previous.keys()):
        before, after = previous.get(path), current.get(path)
        if before == after:
            continue
        for record in (before, after):
            if record is not None and record[0] not in {"100644", "100755"}:
                raise evidence.EvidenceError(f"publication cannot change a link or submodule: {path}")
        if before and after and before[0] != after[0]:
            raise evidence.EvidenceError(f"publication cannot change executable modes: {path}")
        classification = classify_change(root, qualified, path, after is None)
        change: dict[str, Any] = {"path": path, "classification": classification}
        for name, tree, record in (("before", qualified, before), ("after", root, after)):
            change[name] = None if record is None else {
                "git_mode": record[0], "git_blob": record[1],
                "sha256": evidence.sha256_file(tree / path),
            }
        changes.append(change)
    return {
        "schema": "cohesix-release-publication/v1",
        "qualified_source_commit": qualified_commit,
        "qualified_source_digest": evidence.source_digest(qualified),
        "publication_commit": commit,
        "publication_source_digest": evidence.source_digest(root),
        "changes": changes,
        "runtime_sources_unchanged": True,
        "packaging_is_target_acceptance": False,
    }


def main() -> int:
    """Write the factory's publication record before it creates any outputs."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, required=True)
    parser.add_argument("--qualified-source-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        record = publication_record(
            args.repo_root.resolve(strict=True), args.qualified_source_root.resolve(strict=True)
        )
        evidence.atomic_write_json(args.output, record)
        print(record["qualified_source_digest"])
    except (evidence.EvidenceError, OSError, ValueError, KeyError, TypeError,
            SyntaxError, subprocess.CalledProcessError) as error:
        print(f"release-publication: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
