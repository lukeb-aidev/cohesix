# Author: Lukas Bower
# Purpose: Verify the portable Pi 4 release-image builder contract.
# Copyright 2026 Lukas Bower

from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "pi4_release_image.sh"


def test_portable_image_builder_requires_all_locations_as_arguments() -> None:
    result = subprocess.run(
        [str(SCRIPT), "--help"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr
    for option in (
        "--stage-dir",
        "--output-image",
        "--output-metadata",
        "--output-sha256",
    ):
        assert option in result.stdout
    assert "/mnt/" not in SCRIPT.read_text(encoding="utf-8")


def test_portable_image_builder_uses_compact_standard_layout() -> None:
    source = SCRIPT.read_text(encoding="utf-8")

    assert "PARTITION_START_LBA=2048" in source
    assert '"partition_scheme": "MBR"' in source
    assert '"filesystem": "FAT32"' in source
    assert '"minimum_target_bytes"' in source
    assert '"schema": "cohesix-pi4-portable-sd-image/v2"' in source
    assert "minimum_disk_bytes = max(64 * 1024 * 1024" in source
    assert "records(source) != records(mounted)" in source
    assert 'dot_clean -m "$mount_point"' in source


def test_image_preflight_requires_sealed_identity_before_device_creation(
    tmp_path: Path,
) -> None:
    source = SCRIPT.read_text(encoding="utf-8")
    preflight = source.split(
        'python3 - "$STAGE_DIR" "$OUTPUT_IMAGE" "$OUTPUT_METADATA" "$OUTPUT_SHA256" <<\'PY\'\n',
        1,
    )[1].split("\nPY\n", 1)[0]
    stage = tmp_path / "stage"
    stage.mkdir()
    (stage / "payload").write_bytes(b"fixture")
    result = subprocess.run(
        [
            sys.executable,
            "-",
            str(stage),
            *(str(tmp_path / name) for name in ("sd.img", "sd.json", "sd.sha256")),
        ],
        input=preflight,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode != 0
    assert "sealed pi4-image-identity.json" in result.stderr
