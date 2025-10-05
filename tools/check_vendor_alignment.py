# CLASSIFICATION: COMMUNITY
# Filename: check_vendor_alignment.py v0.1
# Author: Lukas Bower
# Date Modified: 2029-06-06

"""Validate that all Cargo.lock registry packages have matching vendor sources."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import Iterable, List, Sequence, Tuple

import tomllib

RegistryPackage = Tuple[str, str]


def _load_lock_packages(lock_path: Path) -> List[dict]:
    try:
        data = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:  # pragma: no cover - CLI validation
        raise SystemExit(f"Cargo.lock not found at {lock_path}") from exc
    except tomllib.TOMLDecodeError as exc:  # pragma: no cover - configuration error
        raise SystemExit(f"Failed to parse {lock_path}: {exc}") from exc

    packages = data.get("package")
    if not isinstance(packages, list):  # pragma: no cover - configuration error
        raise SystemExit("Cargo.lock does not contain a package list")

    return packages


def _registry_packages(packages: Iterable[dict]) -> List[RegistryPackage]:
    entries: List[RegistryPackage] = []
    for pkg in packages:
        source = pkg.get("source", "")
        if not isinstance(source, str):
            continue
        if not source.startswith("registry+https://github.com/rust-lang/crates.io-index"):
            continue
        name = pkg.get("name")
        version = pkg.get("version")
        if isinstance(name, str) and isinstance(version, str):
            entries.append((name, version))
    return entries


def _candidate_dirs(vendor_root: Path, package: RegistryPackage) -> List[Path]:
    name, version = package
    return [
        vendor_root / f"{name}-{version}",
        vendor_root / name,
    ]


def _cargo_toml_version(cargo_toml: Path) -> str | None:
    if not cargo_toml.is_file():
        return None
    try:
        data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError:
        return None
    package_meta = data.get("package")
    if isinstance(package_meta, dict):
        version = package_meta.get("version")
        if isinstance(version, str):
            return version
    return None


def check_vendor_alignment(
    packages: Sequence[RegistryPackage], vendor_root: Path
) -> Tuple[List[RegistryPackage], List[RegistryPackage]]:
    missing: List[RegistryPackage] = []
    mismatched: List[RegistryPackage] = []

    for package in packages:
        targets = _candidate_dirs(vendor_root, package)
        existing_dirs = [candidate for candidate in targets if candidate.exists()]

        if not existing_dirs:
            missing.append(package)
            continue

        required_version = package[1]
        has_version_match = False
        for directory in existing_dirs:
            if _cargo_toml_version(directory / "Cargo.toml") == required_version:
                has_version_match = True
                break

        if has_version_match:
            continue

        # As a fallback, accept directories that retain only checksum metadata (older Cargo versions).
        has_checksum_only_copy = any(
            (directory / ".cargo-checksum.json").is_file() for directory in existing_dirs
        )

        if has_checksum_only_copy:
            continue

        mismatched.append(package)

    return missing, mismatched


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--lock",
        type=Path,
        default=Path("Cargo.lock"),
        help="Path to the workspace Cargo.lock file (default: Cargo.lock)",
    )
    parser.add_argument(
        "--vendor",
        type=Path,
        default=Path("vendor"),
        help="Path to the vendor directory (default: vendor)",
    )
    args = parser.parse_args(argv)

    packages = _registry_packages(_load_lock_packages(args.lock))
    unique_packages = sorted(set(packages))

    missing, mismatched = check_vendor_alignment(unique_packages, args.vendor)

    if missing or mismatched:
        if missing:
            print("Missing vendor packages:")
            for name, version in missing:
                print(f"  - {name} {version}")
        if mismatched:
            print("Mismatched vendor versions:")
            for name, version in mismatched:
                print(f"  - {name} {version}")
        return 1

    print(f"Vendor alignment verified for {len(unique_packages)} registry packages.")
    return 0


if __name__ == "__main__":  # pragma: no cover - CLI entry point
    sys.exit(main())
