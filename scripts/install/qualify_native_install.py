#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Verify independently trusted native packages against their installed bytes.
# Copyright 2026 Lukas Bower
"""Check publisher trust, package identity and installed code on the current host."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import stat
import subprocess
import tarfile
from typing import Any

from native_package import classify, load_reference

MAX_MANIFEST_BYTES = 1024 * 1024
MAX_FILES = 2048
MAX_FILE_BYTES = 512 * 1024 * 1024
MAX_INSTALLED_BYTES = 2 * 1024 * 1024 * 1024
MAC_ROOT = Path("/Library/Application Support/Cohesix")
MAC_APP = Path("/Applications/SwarmUI.app")
LINUX_ROOT = Path("/usr/lib/cohesix")
HEX_SHA256 = re.compile(r"[0-9a-f]{64}\Z")


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    """Reject duplicate keys in publisher and installed-payload manifests."""
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate manifest key: {key}")
        result[key] = value
    return result


def read_json(path: Path) -> dict[str, Any]:
    """Read one bounded regular JSON object without following its final symlink."""
    if (path.is_symlink() or not path.is_file()
            or path.stat().st_size > MAX_MANIFEST_BYTES):
        raise ValueError("installer manifest must be a bounded regular file")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    with os.fdopen(descriptor, "rb") as source:
        raw = source.read(MAX_MANIFEST_BYTES + 1)
    if len(raw) > MAX_MANIFEST_BYTES:
        raise ValueError("installer manifest exceeded byte bound")
    value = json.loads(raw, object_pairs_hook=unique_object)
    if not isinstance(value, dict):
        raise ValueError("installer manifest must be an object")
    return value


def digest(path: Path, maximum: int = MAX_FILE_BYTES) -> str:
    """Hash a bounded regular file with a stable open descriptor."""
    if path.is_symlink() or not path.is_file() or path.stat().st_size > maximum:
        raise ValueError(f"installer file is missing, linked or oversized: {path}")
    value = hashlib.sha256()
    size = 0
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    with os.fdopen(descriptor, "rb") as source:
        before = os.fstat(source.fileno())
        if not stat.S_ISREG(before.st_mode) or before.st_size > maximum:
            raise ValueError("installer file changed before readback")
        while block := source.read(1024 * 1024):
            size += len(block)
            if size > maximum:
                raise ValueError("installer file exceeded byte bound")
            value.update(block)
        after = os.fstat(source.fileno())
    if (size != before.st_size or
            (before.st_dev, before.st_ino, before.st_mtime_ns, before.st_size)
            != (after.st_dev, after.st_ino, after.st_mtime_ns, after.st_size)):
        raise ValueError("installer file changed during readback")
    return value.hexdigest()


def command(arguments: list[str], *, timeout: int = 120) -> str:
    """Run a fixed platform verifier and retain only bounded output."""
    result = subprocess.run(arguments, capture_output=True, timeout=timeout,
                            check=False)
    output = result.stdout + result.stderr
    if result.returncode or len(output) > MAX_MANIFEST_BYTES:
        raise ValueError(f"installer verification command failed: {arguments[0]}")
    return output.decode("utf-8", errors="strict")


def package_path(manifest: Path, name: str) -> Path:
    """Keep every artifact named by the manifest beside that manifest."""
    if (not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_.~+-]{1,160}", name)
            or name in {".", ".."}):
        raise ValueError("invalid installer artifact name")
    path = manifest.parent / name
    if path.is_symlink() or not path.is_file():
        raise ValueError("installer artifact missing or linked")
    return path


def reference_fields(reference: dict[str, Any], manifest: dict[str, Any]) -> None:
    """Bind a package manifest to the externally selected release archive."""
    if (manifest.get("schema") != "cohesix-m28g-installers/v1"
            or any(manifest.get(key) != reference[key] for key in
                   ("host", "version", "source_commit", "archive_sha256"))):
        raise ValueError("installer manifest differs from independent release reference")


def installed_path(root: Path, relative: str) -> Path:
    """Confine readback to absolute package-owned code and reject symlink parents."""
    path = PurePosixPath(relative)
    if (not path.is_absolute() or path.as_posix() != relative
            or "." in path.parts or ".." in path.parts
            or "\\" in relative or any(ord(char) < 32 for char in relative)):
        raise ValueError("unsafe installed path")
    resolved = root.joinpath(*path.parts[1:])
    if any(parent.is_symlink() for parent in (resolved, *resolved.parents)):
        raise ValueError("installed path has a symlink component")
    return resolved


def verify_records(root: Path, rows: Any, allowed: tuple[str, ...]) -> int:
    """Read back all declared installed bytes under platform-owned paths."""
    if not isinstance(rows, list) or not rows or len(rows) > MAX_FILES:
        raise ValueError("installed payload file count is invalid")
    seen: set[str] = set()
    total = 0
    for row in rows:
        if not isinstance(row, dict) or set(row) != {"path", "size", "sha256"}:
            raise ValueError("invalid installed payload record")
        name, size, expected = row["path"], row["size"], row["sha256"]
        if (not isinstance(name, str) or name in seen
                or not any(name.startswith(prefix) for prefix in allowed)
                or not isinstance(size, int) or isinstance(size, bool)
                or size < 0 or size > MAX_FILE_BYTES
                or not isinstance(expected, str) or not HEX_SHA256.fullmatch(expected)):
            raise ValueError("invalid or duplicate installed payload identity")
        seen.add(name)
        if total + size > MAX_INSTALLED_BYTES:
            raise ValueError("installed payload exceeds total byte bound")
        path = installed_path(root, name)
        if path.stat().st_size != size or digest(path) != expected:
            raise ValueError(f"installed payload differs from signed package: {name}")
        total += size
    return total


def selected_subset(bundle: Path, destination: str,
                    subset: str) -> list[dict[str, Any]]:
    """Map archive-owned files into their native installation root."""
    if subset not in {"controller", "desktop"}:
        raise ValueError("unknown installer subset")
    records = []
    for line in (bundle / "MANIFEST.sha256").read_text().splitlines():
        expected, relative = line.split("  ", 1)
        if classify(relative) == subset:
            source = bundle / relative
            if digest(source) != expected:
                raise ValueError("selected archive file changed")
            records.append({"path": destination + "/" + relative,
                            "size": source.stat().st_size, "sha256": expected})
    return records


def signed_host_tool_record(source: Path, installed: Path,
                            team: str) -> dict[str, Any]:
    """Bind a signed installed Mach-O to the release UUID and Developer ID."""
    uuids = []
    for path in (source, installed):
        output = command(["/usr/bin/dwarfdump", "--uuid", str(path)])
        found = set(re.findall(r"UUID: ([0-9A-F-]{36})", output))
        if not found:
            raise ValueError("release host tool has no Mach-O UUID")
        uuids.append(found)
    if uuids[0] != uuids[1]:
        raise ValueError("signed host tool differs from the release build")
    command(["/usr/bin/codesign", "--verify", "--strict", str(installed)])
    details = command(["/usr/bin/codesign", "-dvv", str(installed)])
    if (
        f"TeamIdentifier={team}" not in details
        or "Authority=Developer ID Application:" not in details
        or re.search(r"^CodeDirectory .*\(.*runtime.*\)", details, re.MULTILINE) is None
        or re.search(r"^Timestamp=.+", details, re.MULTILINE) is None
    ):
        raise ValueError("installed host tool lacks Developer ID runtime signature")
    return {"path": str(installed), "size": installed.stat().st_size,
            "sha256": digest(installed)}


def qualify_macos(reference: dict[str, Any], manifest_path: Path,
                  manifest: dict[str, Any]) -> dict[str, Any]:
    """Verify notarized package, exact receipt, signed app and installed bytes."""
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise ValueError("Mac installer readback requires Apple Silicon macOS")
    if set(manifest) != {"schema", "host", "version", "source_commit",
                         "archive_sha256", "package", "package_sha256",
                         "notarization_id", "team_id", "state"}:
        raise ValueError("Mac installer manifest fields changed")
    if (manifest["state"] != "built-signed-notarized-unqualified"
            or manifest["team_id"] != reference["team_id"]
            or not isinstance(manifest["package_sha256"], str)
            or not HEX_SHA256.fullmatch(manifest["package_sha256"])):
        raise ValueError("Mac installer publisher identity changed")
    package = package_path(manifest_path, manifest["package"])
    if digest(package, 2 * 1024 * 1024 * 1024) != manifest["package_sha256"]:
        raise ValueError("Mac installer package hash changed")
    signed = command(["/usr/sbin/pkgutil", "--check-signature", str(package)])
    if ("Developer ID Installer" not in signed
            or reference["team_id"] not in signed):
        raise ValueError("Mac installer signer differs from selected Team ID")
    command(["/usr/bin/xcrun", "stapler", "validate", str(package)])
    command(["/usr/sbin/spctl", "--assess", "--type", "install", str(package)])
    receipt = command(["/usr/sbin/pkgutil", "--pkg-info", "com.cohesix.host"])
    if f"version: {reference['version'].split('-', 1)[0]}" not in receipt:
        raise ValueError("installed Mac receipt has the wrong version")
    owned = set(command(["/usr/sbin/pkgutil", "--files", "com.cohesix.host"]).splitlines())
    payload_path = MAC_ROOT / "installed-payload.json"
    payload = read_json(payload_path)
    if (payload.get("schema") != "cohesix-m28g-installed-payload/v1"
            or payload.get("package") != "cohesix-macos-host"
            or any(payload.get(key) != reference[key] for key in
                   ("host", "version", "source_commit", "archive_sha256"))):
        raise ValueError("installed Mac manifest differs from release reference")
    rows = payload.get("files")
    expected = selected_subset(Path(reference["bundle"]), MAC_ROOT.as_posix(),
                               "controller")
    for index, row in enumerate(expected):
        installed = Path(row["path"])
        if installed.parent == MAC_ROOT / "bin":
            source = Path(reference["bundle"]) / "bin" / installed.name
            expected[index] = signed_host_tool_record(
                source, installed, reference["team_id"]
            )
    from build_macos_pkg import APP_ROOT, UNINSTALL, app_records
    expected.extend(app_records(Path(reference["signed_app"])))
    expected.append({"path": str(MAC_ROOT / "bin/cohesix-uninstall"),
                     "size": UNINSTALL.stat().st_size, "sha256": digest(UNINSTALL)})
    if (not isinstance(rows, list) or
            sorted(rows, key=lambda row: row["path"]) != sorted(expected, key=lambda row: row["path"])):
        raise ValueError("installed Mac file inventory differs from selected source")
    for name in [row["path"] for row in expected] + [str(payload_path),
                                                       str(MAC_ROOT / "INSTALLED.sha256")]:
        if name.removeprefix("/") not in owned:
            raise ValueError("installed Mac file lacks an installer receipt")
    total = verify_records(Path("/"), rows, (str(MAC_ROOT) + "/", str(MAC_APP) + "/"))
    from sign_swarmui_macos import signature
    signature(MAC_APP, reference["team_id"], "Developer ID Application")
    command(["/usr/bin/xcrun", "stapler", "validate", str(MAC_APP)])
    command(["/usr/sbin/spctl", "--assess", "--type", "execute", str(MAC_APP)])
    return {"kind": "installer", "host": "macos", "version": reference["version"],
            "source_commit": reference["source_commit"],
            "archive_sha256": reference["archive_sha256"],
            "packages": [{"path": package.name, "sha256": manifest["package_sha256"]}],
            "installed_file_count": len(rows), "installed_bytes": total,
            "checks": ["publisher", "notarization", "receipt", "installed-readback",
                       "app-signature"],
            "proof_limit": "GUI launch and install lifecycle require separate live reports"}


def deb_contents(package: Path) -> dict[str, dict[str, Any]]:
    """Stream a signed-manifest-bound Debian payload without extracting it."""
    process = subprocess.Popen(["dpkg-deb", "--fsys-tarfile", str(package)],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    if process.stdout is None:
        raise ValueError("dpkg-deb has no payload stream")
    files: dict[str, dict[str, Any]] = {}
    total = 0
    try:
        with tarfile.open(fileobj=process.stdout, mode="r|") as archive:
            for member in archive:
                if member.isdir() and member.name in {".", "./"}:
                    continue
                name = member.name.removeprefix("./")
                path = PurePosixPath("/" + name)
                if (not name or path.as_posix() != "/" + name
                        or ".." in path.parts or "\\" in name):
                    raise ValueError("unsafe Debian payload path")
                if member.isdir():
                    continue
                if not member.isfile() or name in files or len(files) >= MAX_FILES:
                    raise ValueError("Debian payload has linked, special or duplicate files")
                if member.size > MAX_FILE_BYTES or total + member.size > 1024 * 1024 * 1024:
                    raise ValueError("Debian payload exceeds byte bound")
                stream = archive.extractfile(member)
                if stream is None:
                    raise ValueError("Debian payload file cannot be read")
                value = hashlib.sha256()
                copied = 0
                while block := stream.read(1024 * 1024):
                    copied += len(block)
                    if copied > member.size:
                        raise ValueError("Debian payload changed during read")
                    value.update(block)
                if copied != member.size:
                    raise ValueError("Debian payload is truncated")
                files[path.as_posix()] = {"path": path.as_posix(),
                                          "size": copied, "sha256": value.hexdigest()}
                total += copied
        if process.wait(timeout=120):
            raise ValueError("dpkg-deb could not decode the selected package")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)
    return files


def verify_deb_control(package: Path) -> None:
    """Refuse package hooks, service enrollment and unselected control files."""
    process = subprocess.Popen(["dpkg-deb", "--ctrl-tarfile", str(package)],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    if process.stdout is None:
        raise ValueError("dpkg-deb has no control stream")
    names: set[str] = set()
    try:
        with tarfile.open(fileobj=process.stdout, mode="r|") as archive:
            for member in archive:
                if member.isdir() and member.name in {".", "./"}:
                    continue
                name = member.name.removeprefix("./")
                if (name not in {"control", "md5sums"} or not member.isfile()
                        or name in names or member.size > 65536):
                    raise ValueError("Debian control archive has an unselected script or file")
                names.add(name)
        if process.wait(timeout=120) or "control" not in names:
            raise ValueError("Debian control archive is incomplete")
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)


def qualify_linux(reference: dict[str, Any], manifest_path: Path,
                  manifest: dict[str, Any]) -> dict[str, Any]:
    """Verify independent GPG trust, both dpkg receipts and installed files."""
    if platform.system() != "Linux" or platform.machine() != "aarch64":
        raise ValueError("Linux installer readback requires native AArch64")
    if set(manifest) != {"schema", "host", "version", "source_commit",
                         "archive_sha256", "packages", "publisher_key", "state"}:
        raise ValueError("Linux installer manifest fields changed")
    if manifest["state"] != "built-signed-unqualified":
        raise ValueError("Linux installer has the wrong publication state")
    keyring_name = os.environ.get("COHESIX_DEB_TRUSTED_KEYRING", "")
    fingerprint = os.environ.get("COHESIX_DEB_PUBLISHER_FINGERPRINT", "").upper()
    keyring = Path(keyring_name) if keyring_name else Path("/")
    if (not keyring.is_absolute() or not keyring.is_file() or keyring.is_symlink()
            or not re.fullmatch(r"[0-9A-F]{40,64}", fingerprint)):
        raise ValueError("select an external public keyring and publisher fingerprint")
    signature = package_path(manifest_path, "installers.json.asc")
    verified = command(["gpgv", "--status-fd=1", "--keyring", str(keyring),
                        str(signature), str(manifest_path)])
    signers = re.findall(r"^\[GNUPG:\] VALIDSIG ([0-9A-F]{40,64})\b",
                         verified.upper(), flags=re.MULTILINE)
    if signers != [fingerprint]:
        raise ValueError("Linux publisher signature differs from trusted fingerprint")
    packages = manifest["packages"]
    if (not isinstance(packages, list) or len(packages) != 2
            or {row.get("package") for row in packages if isinstance(row, dict)}
            != {"cohesix-controller", "cohesix-swarmui"}):
        raise ValueError("Linux installer must contain controller and desktop packages")
    records: list[dict[str, Any]] = []
    package_results = []
    for row in packages:
        if (not isinstance(row, dict) or set(row) != {"package", "path", "size", "sha256"}
                or not isinstance(row["sha256"], str)
                or not HEX_SHA256.fullmatch(row["sha256"])):
            raise ValueError("invalid Linux package record")
        package = package_path(manifest_path, row["path"])
        if package.stat().st_size != row["size"] or digest(package, 2 * 1024 * 1024 * 1024) != row["sha256"]:
            raise ValueError("Linux package differs from signed manifest")
        verify_deb_control(package)
        control = command(["dpkg-deb", "--field", str(package), "Package", "Version", "Architecture"])
        version = reference["version"].replace("-beta", "~beta")
        if (f"Package: {row['package']}" not in control or f"Version: {version}" not in control
                or "Architecture: arm64" not in control):
            raise ValueError("Linux package control identity differs from reference")
        installed = command(["dpkg-query", "-W", "-f=${Version} ${Architecture} ${Status}",
                             row["package"]])
        if installed.strip() != f"{version} arm64 install ok installed":
            raise ValueError("Linux package is not installed at selected version")
        contents = deb_contents(package)
        roots = ("/usr/lib/cohesix/", "/usr/share/doc/", "/usr/share/applications/",
                 "/usr/share/icons/hicolor/")
        for name in contents:
            if not any(name.startswith(prefix) for prefix in roots):
                raise ValueError("Debian package installs outside selected code roots")
        verify_records(Path("/"), list(contents.values()), roots)
        payload_name = f"/usr/share/doc/{row['package']}/payload.json"
        payload = read_json(Path(payload_name))
        if (payload.get("schema") != "cohesix-m28g-installed-payload/v1"
                or payload.get("package") != row["package"]
                or any(payload.get(key) != reference[key] for key in
                       ("host", "version", "source_commit", "archive_sha256"))):
            raise ValueError("installed Debian payload identity differs from release")
        declared = payload.get("files")
        expected = [value for key, value in contents.items() if key != payload_name]
        if (not isinstance(declared, list) or
                sorted(declared, key=lambda item: item["path"])
                != sorted(expected, key=lambda item: item["path"])):
            raise ValueError("installed Debian payload inventory differs from package")
        subset = "controller" if row["package"] == "cohesix-controller" else "desktop"
        source = selected_subset(Path(reference["bundle"]), LINUX_ROOT.as_posix(),
                                 subset)
        if subset == "desktop":
            from build_ubuntu_arm64_deb import DESKTOP_ENTRY, ICON
            for name, asset in (
                ("/usr/share/applications/com.cohesix.swarmui.desktop", DESKTOP_ENTRY),
                ("/usr/share/icons/hicolor/scalable/apps/com.cohesix.swarmui.svg", ICON),
            ):
                source.append({"path": name, "size": asset.stat().st_size,
                               "sha256": digest(asset)})
        if sorted(declared, key=lambda item: item["path"]) != sorted(
                source, key=lambda item: item["path"]):
            raise ValueError("Debian payload differs from selected release source")
        records.extend(contents.values())
        package_results.append({"path": package.name, "sha256": row["sha256"]})
    return {"kind": "installer", "host": "linux", "version": reference["version"],
            "source_commit": reference["source_commit"],
            "archive_sha256": reference["archive_sha256"],
            "packages": package_results, "publisher_fingerprint": fingerprint,
            "installed_file_count": len(records),
            "installed_bytes": sum(row["size"] for row in records),
            "checks": ["publisher", "package-control", "dpkg-receipt", "installed-readback"],
            "proof_limit": "GNOME launch and install lifecycle require separate live reports"}


def qualify(reference_path: Path, manifest_path: Path) -> dict[str, Any]:
    """Select platform-specific verification from an external reference."""
    header = read_json(reference_path)
    host = header.get("host")
    if host not in {"macos", "linux"}:
        raise ValueError("unknown installer host")
    reference = load_reference(reference_path, host)
    manifest = read_json(manifest_path)
    reference_fields(reference, manifest)
    if host == "macos":
        return qualify_macos(reference, manifest_path, manifest)
    return qualify_linux(reference, manifest_path, manifest)
