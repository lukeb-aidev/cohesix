# Author: Lukas Bower
# Purpose: Reject undeclared Python package inputs and malformed archive content before installation.
# Copyright 2026 Lukas Bower
"""Independent archive vectors for the M27b distribution contract."""

import base64
import csv
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import zipfile

import pytest

MODULE = Path(__file__).resolve().parents[1] / "scripts/install/build_python_package.py"
SPEC = importlib.util.spec_from_file_location("build_python_package", MODULE)
assert SPEC and SPEC.loader
package = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(package)


def test_python_sources_are_explicit_product_files() -> None:
    rows = ["README.md", "pyproject.toml", "cohesix/__init__.py", "cohesix/client.py"]
    inventory = {"schema": "cohesix-implementation-surface-inventory/v1",
                 "release": {"python_artifacts": ["tools/cohesix-py/" + path for path in rows]}}
    assert package.source_files(inventory) == sorted(rows)
    for path in [
        "tests/test_auth.py", "cohesix/__pycache__/client.pyc",
        "cohesix/../secret.py", "credential.private",
    ]:
        inventory["release"]["python_artifacts"].append("tools/cohesix-py/" + path)
        with pytest.raises(ValueError):
            package.source_files(inventory)
        inventory["release"]["python_artifacts"].pop()


def wheel_bytes(*, extra: bool = False, wrong_record: bool = False, wrong_arch: bool = False) -> bytes:
    prefix = "cohesix-1.0.dist-info/"
    contents = {
        "cohesix/__init__.py": b"# independent package fixture\n",
        prefix + "METADATA": b"Metadata-Version: 2.1\nName: cohesix\nVersion: 1.0\nRequires-Python: >=3.11\n",
        prefix + "WHEEL": (
            b"Wheel-Version: 1.0\nRoot-Is-Purelib: true\nTag: "
            + (b"cp311-linux\n" if wrong_arch else b"py3-none-any\n")
        ),
        prefix + "entry_points.txt": b"[console_scripts]\n",
        prefix + "top_level.txt": b"cohesix\n",
    }
    if extra:
        contents["cohesix/credential.private"] = b"unexpected\n"
    records = io.StringIO()
    writer = csv.writer(records)
    for name, data in contents.items():
        digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
        writer.writerow([name, "sha256=" + ("broken" if wrong_record else digest), len(data)])
    writer.writerow([prefix + "RECORD", "", ""])
    contents[prefix + "RECORD"] = records.getvalue().encode()
    result = io.BytesIO()
    with zipfile.ZipFile(result, "w") as archive:
        for name, data in contents.items():
            archive.writestr(name, data)
    return result.getvalue()


def test_wheel_requires_exact_files_architecture_source_and_record(tmp_path: Path) -> None:
    path = tmp_path / "cohesix-1.0-py3-none-any.whl"
    expected = {"cohesix/__init__.py": hashlib.sha256(b"# independent package fixture\n").hexdigest()}
    path.write_bytes(wheel_bytes())
    report = package.inspect_wheel(path, expected, "1.0")
    assert report["architecture"] == "any"
    assert report["files"] == ["cohesix-1.0.dist-info/METADATA", "cohesix-1.0.dist-info/RECORD",
                               "cohesix-1.0.dist-info/WHEEL", "cohesix-1.0.dist-info/entry_points.txt",
                               "cohesix-1.0.dist-info/top_level.txt", "cohesix/__init__.py"]
    for flags in [{"extra": True}, {"wrong_record": True}, {"wrong_arch": True}]:
        path.write_bytes(wheel_bytes(**flags))
        with pytest.raises(ValueError):
            package.inspect_wheel(path, expected, "1.0")
    path.write_bytes(wheel_bytes())
    with pytest.raises(ValueError, match="source digest"):
        package.inspect_wheel(path, {"cohesix/__init__.py": "0" * 64}, "1.0")


def test_sdist_rejects_traversal_special_files_and_unlisted_sources(tmp_path: Path) -> None:
    path = tmp_path / "cohesix-1.0.tar.gz"
    for name, kind in [
        ("cohesix-1.0/../escape", tarfile.REGTYPE),
        ("cohesix-1.0/cohesix/key", tarfile.SYMTYPE),
        ("cohesix-1.0/tests/test_fixture.py", tarfile.REGTYPE),
    ]:
        with tarfile.open(path, "w:gz") as archive:
            member = tarfile.TarInfo(name)
            member.type = kind
            if kind == tarfile.SYMTYPE:
                member.linkname = "/outside/private"
            archive.addfile(member, io.BytesIO())
        with pytest.raises(ValueError):
            package.inspect_sdist(path, {}, "1.0")
    path.unlink()
    original = tmp_path / "original"
    original.write_bytes(b"untrusted package source")
    path.symlink_to(original)
    with pytest.raises(ValueError, match="regular file"):
        package.read_regular(path)
