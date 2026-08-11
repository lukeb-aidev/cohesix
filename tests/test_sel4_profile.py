# Author: Lukas Bower
# Purpose: Verify fail-closed seL4 source and generated-build profile contracts.
# Copyright 2026 Lukas Bower

from __future__ import annotations

import copy
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tomllib
from typing import Any

import pytest

from scripts import sel4_profile


def _cache_type(value: Any) -> str:
    if str(value) in {"ON", "OFF"}:
        return "BOOL"
    return "STRING"


def _write_fake_compiler(path: Path, version: str, target: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "#!/bin/sh\n"
        "case \"$1\" in\n"
        f"  -dumpfullversion) echo {version} ;;\n"
        f"  -dumpmachine) echo {target} ;;\n"
        f"  --version) echo \"{path.name} (GCC) {version}\" ;;\n"
        "  *) exit 2 ;;\n"
        "esac\n",
        encoding="utf-8",
    )
    path.chmod(0o755)


def _minimal_elf(*, rwx: bool = False) -> bytes:
    header = bytearray(64)
    header[:16] = b"\x7fELF\x02\x01\x01" + (b"\0" * 9)
    struct.pack_into(
        "<HHIQQQIHHHHHH",
        header,
        16,
        2,
        183,
        1,
        0x400000,
        64,
        0,
        0,
        64,
        56,
        1,
        0,
        0,
        0,
    )
    flags = 7 if rwx else 5
    program = struct.pack("<IIQQQQQQ", 1, flags, 0, 0x400000, 0x400000, 120, 120, 0x1000)
    return bytes(header) + program


def _pad_fdt(value: bytes) -> bytes:
    return value + (b"\0" * ((-len(value)) % 4))


def _minimal_dtb(profile: dict[str, Any]) -> bytes:
    policy = profile["dtb"]
    strings = b"compatible\0method\0"
    compatible_offset = 0
    method_offset = len(b"compatible\0")
    structure = bytearray()

    def token(value: int) -> None:
        structure.extend(struct.pack(">I", value))

    def begin_node(name: str) -> None:
        token(1)
        structure.extend(_pad_fdt(name.encode("utf-8") + b"\0"))

    def end_node() -> None:
        token(2)

    def string_property(name_offset: int, values: list[str]) -> None:
        encoded = b"\0".join(value.encode("utf-8") for value in values) + b"\0"
        token(3)
        structure.extend(struct.pack(">II", len(encoded), name_offset))
        structure.extend(_pad_fdt(encoded))

    begin_node("")
    for selector in policy["required_string_properties"]:
        name, value = selector.split("=", 1)
        begin_node(f"{name}-holder")
        string_property(method_offset if name == "method" else method_offset, [value])
        end_node()
    if policy["required_compatible"]:
        begin_node("interrupt-controller")
        string_property(compatible_offset, policy["required_compatible"])
        end_node()
    for name in policy["required_nodes"]:
        begin_node(name)
        end_node()
    end_node()
    token(9)

    reserve = b"\0" * 16
    structure_bytes = bytes(structure)
    structure_offset = 40 + len(reserve)
    strings_offset = structure_offset + len(structure_bytes)
    total_size = strings_offset + len(strings)
    header = struct.pack(
        ">10I",
        0xD00DFEED,
        total_size,
        structure_offset,
        strings_offset,
        40,
        17,
        16,
        0,
        len(strings),
        len(structure_bytes),
    )
    return header + reserve + structure_bytes + strings


def _write_build_tree(
    tmp_path: Path,
    contract: dict[str, Any],
    profile_name: str,
) -> Path:
    profile = contract["profiles"][profile_name]
    build_dir = tmp_path / profile_name
    generated_dir = build_dir / "kernel" / "gen_config" / "kernel"
    generated_dir.mkdir(parents=True)

    cache_lines = [
        f"{key}:{_cache_type(value)}={value}"
        for key, value in profile["cmake"].items()
    ]
    cache_lines.extend(
        (
            "SEL4_CACHE_DIR:PATH=",
            "MEMOIZE_CACHE_DIR:INTERNAL=",
        )
    )
    if profile["build_mode"] == "wrapper":
        cache_lines.extend(
            (
                "CMAKE_HOME_DIRECTORY:INTERNAL="
                f"{sel4_profile.WRAPPER_PROJECT.resolve()}",
                "COHESIX_SEL4_PROJECT_ROOT:PATH=/missing/pinned-project",
                "COHESIX_SEL4_WRAPPER_SHA256:INTERNAL="
                f"{sel4_profile.sha256_file(sel4_profile.WRAPPER_CMAKE)}",
            )
        )
        python_tool = sel4_profile.resolve_profile_tool(
            profile,
            "python_tool",
            preserve_symlink=True,
        )
        if python_tool is not None:
            cache_lines.append(f"PYTHON3:INTERNAL={python_tool}")
        objcopy_wrapper = sel4_profile.resolve_profile_tool(
            profile,
            "objcopy_stdout_wrapper",
        )
        if objcopy_wrapper is not None:
            cache_lines.append(f"CMAKE_OBJCOPY:FILEPATH={objcopy_wrapper}")
    else:
        cache_lines.append("CMAKE_HOME_DIRECTORY:INTERNAL=/tmp/sel4-project/kernel")
    cross_prefix = profile["cmake"].get("CROSS_COMPILER_PREFIX")
    if cross_prefix:
        toolchain = contract["toolchain"]
        compiler_bin = sel4_profile.contract_repo_path(
            toolchain["compiler"]["bin_path"],
            "toolchain.compiler.bin_path",
        )
        compiler_dir = build_dir / "CMakeFiles" / "profile-test"
        compiler_dir.mkdir(parents=True)
        compilers = [("C", "gcc"), ("ASM", "gcc")]
        if profile["build_mode"] == "wrapper":
            compilers.insert(1, ("CXX", "g++"))
        for language, suffix in compilers:
            symbol = f"CMAKE_{language}_COMPILER"
            compiler_path = compiler_bin / f"{cross_prefix}{suffix}"
            (compiler_dir / f"CMake{language}Compiler.cmake").write_text(
                f'set({symbol} "{compiler_path}")\n'
                f'set({symbol}_VERSION "{toolchain["version"]}")\n',
                encoding="utf-8",
            )
    if "qemu_gic_version" in profile:
        cache_lines.append(
            f"QEMU_GIC_VERSION:STRING={profile['qemu_gic_version']}"
        )
        cache_lines.append(
            "QEMU_MACHINE:UNINITIALIZED="
            "virt,secure=off,virtualization=on,"
            f"gic-version={profile['qemu_gic_version']},dumpdtb=/tmp/qemu.dtb"
        )
        cache_lines.append(
            "KernelArmGicV3:BOOL="
            f"{'ON' if profile['qemu_gic_version'] == 3 else 'OFF'}"
        )
    (build_dir / "CMakeCache.txt").write_text(
        "\n".join(cache_lines) + "\n",
        encoding="utf-8",
    )
    (generated_dir / "gen_config.json").write_text(
        json.dumps(profile["generated"], indent=2) + "\n",
        encoding="utf-8",
    )
    (build_dir / "build.ninja").write_text(
        "# synthetic profile-test build graph\n",
        encoding="utf-8",
    )
    if "qemu_gic_version" in profile:
        gic_enabled = profile["qemu_gic_version"] == 3
        (generated_dir / "gen_config.h").write_text(
            "#define CONFIG_ARM_GIC_V3_SUPPORT "
            f"{1 if gic_enabled else 0}\n",
            encoding="utf-8",
        )

    dts_literals = profile.get("required_dts_literals", [])
    for relative_dts in profile["dts_files"]:
        dts_path = build_dir / relative_dts
        dts_path.parent.mkdir(parents=True, exist_ok=True)
        dts_path.write_text(
            "\n".join(str(item) for item in dts_literals) + "\n",
            encoding="utf-8",
        )
    if "timer_clock_hz" in profile:
        platform_dir = build_dir / "kernel" / "gen_headers" / "plat"
        platform_dir.mkdir(parents=True)
        (platform_dir / "platform_gen.h").write_text(
            f"#define TIMER_CLOCK_HZ ULL_CONST({profile['timer_clock_hz']})\n",
            encoding="utf-8",
        )
    if str(profile["cmake"].get("ElfloaderRootserversLast", "")).upper() == "ON":
        elfloader_headers = build_dir / "elfloader" / "gen_headers"
        elfloader_headers.mkdir(parents=True)
        (elfloader_headers / "platform_info.h").write_text(
            "int num_memory_regions = 1;\n"
            "struct memory_region { size_t start; size_t end; } "
            "memory_region[1] = {{ .start = 4096, .end = 2147483648 }};\n",
            encoding="utf-8",
        )
    return build_dir


def _write_required_artifacts(
    build_dir: Path,
    contract: dict[str, Any],
    profile_name: str,
) -> None:
    profile = contract["profiles"][profile_name]
    build_start = sel4_profile.require_fresh_profile_build_start(
        build_dir,
        profile,
    )
    elf_labels = set(profile["artifact_policy"]["elf_artifacts"])
    for label, candidates in sel4_profile.artifact_candidates(
        build_dir, profile
    ).items():
        artifact = candidates[0]
        artifact.parent.mkdir(parents=True, exist_ok=True)
        if label in elf_labels:
            artifact.write_bytes(_minimal_elf())
        elif label.endswith("dtb"):
            artifact.write_bytes(_minimal_dtb(profile))
        elif label == "elfloader_archive":
            artifact.write_bytes(
                b"A" * int(profile["minimum_elfloader_archive_bytes"])
            )
        else:
            artifact.write_bytes(b"profile-test-artifact\n")
    source_root = Path("/missing/pinned-project")
    source_evidence = {
        "root": str(source_root),
        "policy": profile["source_policy"],
        "repositories": {},
        "errors": [],
    }
    stamp = sel4_profile.profile_build_stamp(
        contract,
        profile_name,
        profile,
        source_root,
        build_dir,
        source_evidence,
        build_start=build_start,
        jobs=4,
        status="complete",
        require_outputs=True,
    )
    sel4_profile.write_wrapper_build_input_stamp(build_dir, stamp)


def _refresh_completed_build_stamp(
    build_dir: Path,
    contract: dict[str, Any],
    profile_name: str,
) -> None:
    """Refresh synthetic provenance after an intentional test-artifact rewrite."""

    profile = contract["profiles"][profile_name]
    source_root = Path("/missing/pinned-project")
    source_evidence = {
        "root": str(source_root),
        "policy": profile["source_policy"],
        "repositories": {},
        "errors": [],
    }
    previous_stamp = json.loads(
        (build_dir / sel4_profile.BUILD_INPUT_STAMP_NAME).read_text(
            encoding="utf-8"
        )
    )
    build_start = previous_stamp["causal_freshness"]["build_start"]
    stamp = sel4_profile.profile_build_stamp(
        contract,
        profile_name,
        profile,
        source_root,
        build_dir,
        source_evidence,
        build_start=build_start,
        jobs=4,
        status="complete",
        require_outputs=True,
    )
    sel4_profile.write_wrapper_build_input_stamp(build_dir, stamp)


def _errors(evidence: dict[str, Any]) -> str:
    return "\n".join(evidence["errors"])


def _init_git_repo(path: Path) -> str:
    path.mkdir(parents=True)
    subprocess.run(("git", "init", "-q", str(path)), check=True)
    subprocess.run(
        ("git", "-C", str(path), "config", "user.name", "Profile Test"),
        check=True,
    )
    subprocess.run(
        ("git", "-C", str(path), "config", "user.email", "profile@test.invalid"),
        check=True,
    )
    (path / "tracked.txt").write_text("baseline\n", encoding="utf-8")
    subprocess.run(("git", "-C", str(path), "add", "tracked.txt"), check=True)
    subprocess.run(
        ("git", "-C", str(path), "commit", "-q", "-m", "baseline"),
        check=True,
    )
    return subprocess.run(
        ("git", "-C", str(path), "rev-parse", "HEAD"),
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    ).stdout.strip()


def _overlay_source_fixture(
    tmp_path: Path,
    contract: dict[str, Any],
) -> tuple[dict[str, Any], Path, Path, Path]:
    """Create a pinned pristine source plus a source-controlled test patch."""

    local_contract = copy.deepcopy(contract)
    source_root = tmp_path / "source"
    kernel = source_root / "kernel"
    overlay = kernel / "src" / "plat" / "bcm2711" / "overlay-rpi4.dts"
    overlay.parent.mkdir(parents=True)
    subprocess.run(("git", "init", "-q", str(kernel)), check=True)
    subprocess.run(
        ("git", "-C", str(kernel), "config", "user.name", "Profile Test"),
        check=True,
    )
    subprocess.run(
        ("git", "-C", str(kernel), "config", "user.email", "profile@test.invalid"),
        check=True,
    )
    baseline = "/dts-v1/;\n/ {};\n"
    overlay.write_text(baseline, encoding="utf-8")
    subprocess.run(("git", "-C", str(kernel), "add", "."), check=True)
    subprocess.run(
        ("git", "-C", str(kernel), "commit", "-q", "-m", "baseline"),
        check=True,
    )
    commit = subprocess.run(
        ("git", "-C", str(kernel), "rev-parse", "HEAD"),
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    ).stdout.strip()
    overlay.write_text("/dts-v1/;\n/ { proof-node {}; };\n", encoding="utf-8")
    diff = subprocess.run(
        (
            "git",
            "-C",
            str(kernel),
            "diff",
            "--binary",
            "--full-index",
            "--no-ext-diff",
            "--no-renames",
            "--no-color",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--",
            "src/plat/bcm2711/overlay-rpi4.dts",
        ),
        check=True,
        stdout=subprocess.PIPE,
    ).stdout
    overlay.write_text(baseline, encoding="utf-8")

    fake_root = tmp_path / "cohesix"
    patch = fake_root / "configs" / "sel4" / "patches" / "test-overlay.patch"
    patch.parent.mkdir(parents=True)
    patch.write_bytes(diff)
    local_contract["source"]["repositories"] = {"kernel": commit}
    local_contract["source"]["pi4_overlay"].update(
        {
            "diff_sha256": sel4_profile.sha256_bytes(diff),
            "patch_file": "configs/sel4/patches/test-overlay.patch",
        }
    )
    return local_contract, source_root, overlay, fake_root


@pytest.fixture
def supply_chain_contract(
    tmp_path: Path,
    request: pytest.FixtureRequest,
) -> tuple[dict[str, Any], dict[str, Path]]:
    """Create isolated compiler, Python, and mkimage supply-chain fixtures."""

    with sel4_profile.DEFAULT_CONTRACT.open("rb") as stream:
        contract = copy.deepcopy(tomllib.load(stream))
    identity = sel4_profile.sha256_bytes(str(tmp_path).encode("utf-8"))[:16]
    tool_dir = sel4_profile.ROOT / "target" / "sel4-profile-tests" / identity
    tool_dir.mkdir(parents=True, exist_ok=False)
    request.addfinalizer(lambda: shutil.rmtree(tool_dir, ignore_errors=True))

    toolchain = contract["toolchain"]
    compiler = toolchain["compiler"]
    compiler_archive = tool_dir / "downloads" / "arm-gnu-toolchain.tar.xz"
    compiler_archive.parent.mkdir()
    compiler_archive.write_bytes(b"profile-test-arm-gnu-toolchain-archive\n")
    compiler_install = tool_dir / "arm-gnu-toolchain"
    compiler_bin = compiler_install / "bin"
    compiler_bin.mkdir(parents=True)
    cross_prefix = toolchain["cross_prefix"]
    hash_fields = {
        "gcc": "gcc_sha256",
        "g++": "gxx_sha256",
        "cpp": "cpp_sha256",
        "as": "as_sha256",
        "ld": "ld_sha256",
        "objcopy": "objcopy_sha256",
        "ar": "ar_sha256",
        "ranlib": "ranlib_sha256",
    }
    program_sha256: dict[str, str] = {}
    for suffix, hash_field in hash_fields.items():
        program = compiler_bin / f"{cross_prefix}{suffix}"
        _write_fake_compiler(
            program,
            toolchain["version"],
            toolchain["target_triple"],
        )
        digest = sel4_profile.sha256_file(program)
        compiler[hash_field] = digest
        program_sha256[suffix] = digest

    compiler_provenance = (
        compiler_install / "cohesix-compiler-provenance.json"
    )
    compiler.update(
        {
            "source_archive": str(
                compiler_archive.relative_to(sel4_profile.ROOT)
            ),
            "source_archive_sha256": sel4_profile.sha256_file(
                compiler_archive
            ),
            "source_archive_size": compiler_archive.stat().st_size,
            "install_path": str(
                compiler_install.relative_to(sel4_profile.ROOT)
            ),
            "bin_path": str(compiler_bin.relative_to(sel4_profile.ROOT)),
            "path_prefixes": [
                str(compiler_bin.relative_to(sel4_profile.ROOT))
            ],
            "provenance_path": str(
                compiler_provenance.relative_to(sel4_profile.ROOT)
            ),
        }
    )
    compiler_provenance.write_text(
        json.dumps(
            {
                "schema": "cohesix-compiler-provenance/v1",
                "source": {
                    "provider": compiler["provider"],
                    "url": compiler["source_url"],
                    "archive_path": str(compiler_archive.resolve()),
                    "archive_sha256": compiler[
                        "source_archive_sha256"
                    ],
                    "archive_size": compiler["source_archive_size"],
                    "release": compiler["source_version"],
                },
                "compiler": {
                    "version": toolchain["version"],
                    "target": toolchain["target_triple"],
                    "bin_path": str(compiler_bin.resolve()),
                    "program_sha256": program_sha256,
                },
                "setup_script_sha256": sel4_profile.sha256_file(
                    sel4_profile.ROOT / "toolchain" / "setup_macos_arm64.sh"
                ),
                "profile_contract_sha256": sel4_profile.sha256_file(
                    sel4_profile.DEFAULT_CONTRACT
                ),
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )

    locks = tool_dir / "locks"
    locks.mkdir()
    python_contract = contract["toolchain"]["python"]
    bootstrap_requirements = (
        ("pip", "25.0"),
        ("setuptools", python_contract["setuptools_version"]),
        ("wheel", "0.45.1"),
    )
    build_requirements = (
        ("sel4-deps", python_contract["sel4_deps_version"]),
        ("protobuf", python_contract["protobuf_version"]),
        *tuple(
            (f"fixture-dependency-{index:02d}", "1.0.0")
            for index in range(1, 29)
        ),
        ("pytest", "9.1.1"),
        ("iniconfig", "2.3.0"),
        ("packaging", "26.2"),
        ("pluggy", "1.6.0"),
        ("pygments", "2.20.0"),
    )

    def write_lock(
        path: Path,
        requirements: tuple[tuple[str, str], ...],
    ) -> None:
        lines = ["# Isolated seL4 profile validator fixture lock."]
        for name, version in requirements:
            digest = sel4_profile.sha256_bytes(
                f"{name}=={version}".encode("utf-8")
            )
            lines.extend(
                (
                    f"{name}=={version} \\",
                    f"    --hash=sha256:{digest}",
                )
            )
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    lock_requirements = {
        "bootstrap_lock": bootstrap_requirements,
        "requirements_lock": build_requirements,
    }
    for field, requirements in lock_requirements.items():
        destination = locks / Path(str(python_contract[field])).name
        write_lock(destination, requirements)
        python_contract[field] = str(destination.relative_to(sel4_profile.ROOT))
        python_contract[f"{field}_sha256"] = sel4_profile.sha256_file(destination)
    python_tool = tool_dir / "venv" / "bin" / "python"
    python_tool.parent.mkdir(parents=True)
    python_contract["path"] = str(python_tool.relative_to(sel4_profile.ROOT))
    locked = sel4_profile.validate_python_lock_contract(python_contract)
    distributions = {
        name: str(record["version"]) for name, record in locked.items()
    }
    installed_distributions = {
        name: {
            "version": version,
            "file_count": 1,
            "sha256": sel4_profile.sha256_bytes(
                f"{name}=={version}\n".encode("utf-8")
            ),
        }
        for name, version in distributions.items()
    }
    python_probe = tool_dir / "python-environment.json"
    python_probe.write_text(
        json.dumps(
            {
                "schema": "cohesix-python-environment/v1",
                "implementation": python_contract["implementation"],
                "major_minor_version": python_contract["major_minor_version"],
                "version": f"{python_contract['major_minor_version']}.99",
                "executable": str(python_tool.resolve()),
                "prefix": str(python_tool.parent.parent.resolve()),
                "distributions": distributions,
                "installed_content": {
                    "algorithm": "sha256-canonical-installed-files-v1",
                    "distributions": installed_distributions,
                    "sha256": sel4_profile.canonical_sha256(
                        installed_distributions
                    ),
                },
            },
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    python_tool.write_text(
        "#!/bin/sh\n"
        "if [ \"${1:-}\" = \"--version\" ]; then\n"
        "  echo 'Python 3.13.99'\n"
        "  exit 0\n"
        "fi\n"
        "if [ \"${1:-}\" = \"-c\" ]; then\n"
        f"  exec /bin/cat '{python_probe}'\n"
        "fi\n"
        "exit 2\n",
        encoding="utf-8",
    )
    python_tool.chmod(0o755)
    for profile in contract["profiles"].values():
        profile["python_tool"] = python_contract["path"]
        if "minimum_elfloader_archive_bytes" in profile:
            profile["minimum_elfloader_archive_bytes"] = 32

    mkimage_contract = contract["toolchain"]["mkimage"]
    mkimage_build = tool_dir / "u-boot-build"
    fake_mkimage = mkimage_build / "tools" / "mkimage"
    fake_mkimage.parent.mkdir(parents=True)
    fake_mkimage.write_text(
        "#!/bin/sh\n"
        "if [ \"${1:-}\" = \"-V\" ]; then\n"
        "  echo 'mkimage version 2026.01'\n"
        "  exit 0\n"
        "fi\n"
        "exit 0\n",
        encoding="utf-8",
    )
    fake_mkimage.chmod(0o755)
    archive = tool_dir / "downloads" / "u-boot-2026.01.tar.bz2"
    archive.parent.mkdir(exist_ok=True)
    archive.write_bytes(b"profile-test-u-boot-archive\n")
    snapshot = tool_dir / "u-boot-source"
    snapshot.mkdir()
    provenance = mkimage_build / "cohesix-mkimage-provenance.json"
    relative_mkimage = str(fake_mkimage.relative_to(sel4_profile.ROOT))
    mkimage_contract.update(
        {
            "path": relative_mkimage,
            "source_archive": str(archive.relative_to(sel4_profile.ROOT)),
            "source_archive_sha256": sel4_profile.sha256_file(archive),
            "source_archive_size": archive.stat().st_size,
            "snapshot_path": str(snapshot.relative_to(sel4_profile.ROOT)),
            "build_path": str(mkimage_build.relative_to(sel4_profile.ROOT)),
            "provenance_path": str(provenance.relative_to(sel4_profile.ROOT)),
        }
    )
    for profile in contract["profiles"].values():
        if "mkimage_tool" in profile:
            profile["mkimage_tool"] = relative_mkimage
    provenance.parent.mkdir(parents=True, exist_ok=True)
    provenance.write_text(
        json.dumps(
            {
                "schema": "cohesix-mkimage-provenance/v1",
                "source": {
                    "provider": mkimage_contract["provider"],
                    "url": mkimage_contract["source_url"],
                    "archive_path": str(archive.resolve()),
                    "archive_sha256": mkimage_contract[
                        "source_archive_sha256"
                    ],
                    "archive_size": mkimage_contract["source_archive_size"],
                    "version": mkimage_contract["source_version"],
                    "commit": mkimage_contract["source_commit"],
                },
                "mkimage": {
                    "path": str(fake_mkimage.resolve()),
                    "sha256": sel4_profile.sha256_file(fake_mkimage),
                    "version": mkimage_contract["version"],
                },
                "setup_script_sha256": sel4_profile.sha256_file(
                    sel4_profile.ROOT / "toolchain" / "setup_macos_arm64.sh"
                ),
                "profile_contract_sha256": sel4_profile.sha256_file(
                    sel4_profile.DEFAULT_CONTRACT
                ),
                "source_date_epoch": mkimage_contract["source_date_epoch"],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    paths = {
        "python_probe": python_probe,
        "bootstrap_lock": locks / "python-bootstrap.lock",
        "requirements_lock": locks / "python-build-requirements.lock",
        "compiler_gcc": compiler_bin / f"{cross_prefix}gcc",
        "compiler_bin": compiler_bin,
        "compiler_archive": compiler_archive,
        "compiler_provenance": compiler_provenance,
        "mkimage": fake_mkimage,
        "mkimage_provenance": provenance,
        "mkimage_archive": archive,
    }
    return contract, paths


@pytest.fixture
def contract(
    supply_chain_contract: tuple[dict[str, Any], dict[str, Path]],
) -> dict[str, Any]:
    return supply_chain_contract[0]


@pytest.fixture
def supply_chain_paths(
    supply_chain_contract: tuple[dict[str, Any], dict[str, Path]],
) -> dict[str, Path]:
    return supply_chain_contract[1]


def test_valid_qemu_diagnostic_contract_passes(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")

    evidence = sel4_profile.validate_build(
        contract,
        "qemu-smp-diagnostic",
        build_dir,
        for_runtime=True,
    )

    assert evidence["valid"] is True, _errors(evidence)


def test_legacy_domain_schedule_key_fails_even_when_empty(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    with (build_dir / "CMakeCache.txt").open("a", encoding="utf-8") as stream:
        stream.write("KernelDomainSchedule:INTERNAL=\n")

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
    )

    assert evidence["valid"] is False
    assert "forbidden legacy CMake cache key" in _errors(evidence)


@pytest.mark.parametrize("cache_key", ("SEL4_CACHE_DIR", "MEMOIZE_CACHE_DIR"))
def test_poisoned_memoization_cache_fails_closed(
    tmp_path: Path,
    contract: dict[str, Any],
    cache_key: str,
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    cache = build_dir / "CMakeCache.txt"
    cache.write_text(
        cache.read_text(encoding="utf-8").replace(
            f"{cache_key}:{'PATH' if cache_key == 'SEL4_CACHE_DIR' else 'INTERNAL'}=",
            f"{cache_key}:{'PATH' if cache_key == 'SEL4_CACHE_DIR' else 'INTERNAL'}=/tmp/poisoned-cache",
        ),
        encoding="utf-8",
    )

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
    )

    assert evidence["valid"] is False
    assert "seL4 memoization cache must be disabled" in _errors(evidence)


def test_gic_cache_and_dts_mismatch_fails(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    dts = build_dir / "kernel" / "kernel.dts"
    dts.write_text(
        'compatible = "arm,cortex-a15-gic";\nmethod = "smc";\n',
        encoding="utf-8",
    )

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
    )

    assert evidence["valid"] is False
    assert "missing required literal 'arm,gic-v3'" in _errors(evidence)
    assert "contains forbidden literal 'arm,cortex-a15-gic'" in _errors(evidence)


@pytest.mark.parametrize(
    ("selector_line", "expected_error"),
    (
        ("", "CMake cache is missing required key QEMU_GIC_VERSION"),
        ("QEMU_GIC_VERSION:STRING=2\n", "CMake QEMU_GIC_VERSION mismatch"),
    ),
)
def test_qemu_gic_source_selector_must_match_profile(
    tmp_path: Path,
    contract: dict[str, Any],
    selector_line: str,
    expected_error: str,
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    cache = build_dir / "CMakeCache.txt"
    cache.write_text(
        cache.read_text(encoding="utf-8").replace(
            "QEMU_GIC_VERSION:STRING=3\n",
            selector_line,
        ),
        encoding="utf-8",
    )

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
    )

    assert evidence["valid"] is False
    assert expected_error in _errors(evidence)


def test_copied_wrapper_path_is_rejected(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    cache = build_dir / "CMakeCache.txt"
    text = cache.read_text(encoding="utf-8").replace(
        str(sel4_profile.WRAPPER_PROJECT.resolve()),
        "/tmp/copied/tools/sel4-profile-project",
    )
    cache.write_text(text, encoding="utf-8")

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
    )

    assert evidence["valid"] is False
    assert "not the Cohesix seL4 profile wrapper" in _errors(evidence)


def test_configured_wrapper_digest_must_match_current_wrapper(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    cache = build_dir / "CMakeCache.txt"
    cache.write_text(
        cache.read_text(encoding="utf-8").replace(
            sel4_profile.sha256_file(sel4_profile.WRAPPER_CMAKE),
            "0" * 64,
        ),
        encoding="utf-8",
    )

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
    )

    assert evidence["valid"] is False
    assert "configured wrapper digest mismatch" in _errors(evidence)


def test_wrapper_regenerates_fixed_address_platform_info(tmp_path: Path) -> None:
    platform_sift = tmp_path / "platform_sift.py"
    platform_sift.write_text(
        "print('int num_memory_regions = 1;')\n"
        "print('struct memory_region { unsigned long start; unsigned long end; }')\n"
        "print('memory_region[1] = {{ .start = 4096, .end = 8192 }};')\n",
        encoding="utf-8",
    )
    platform_yaml = tmp_path / "platform_gen.yaml"
    platform_yaml.write_text(
        "memory:\n- start: 4096\n  end: 8192\n",
        encoding="utf-8",
    )
    platform_info = tmp_path / "gen_headers" / "platform_info.h"
    platform_info.parent.mkdir()

    subprocess.run(
        (
            "cmake",
            "-DCOHESIX_GENERATE_ELFLOADER_PLATFORM_INFO=ON",
            f"-DCOHESIX_PYTHON3={sys.executable}",
            f"-DCOHESIX_PLATFORM_SIFT={platform_sift}",
            f"-DCOHESIX_PLATFORM_YAML={platform_yaml}",
            f"-DCOHESIX_PLATFORM_INFO={platform_info}",
            "-P",
            str(sel4_profile.WRAPPER_CMAKE),
        ),
        check=True,
    )

    assert "memory_region" in platform_info.read_text(encoding="utf-8")
    wrapper = sel4_profile.WRAPPER_CMAKE.read_text(encoding="utf-8")
    assert "add_dependencies(elfloader cohesix_elfloader_platform_info)" in wrapper


def test_rootservers_last_rejects_empty_platform_info(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["pi4_production"]
    build_dir = _write_build_tree(tmp_path, contract, "pi4_production")
    _write_required_artifacts(build_dir, contract, "pi4_production")
    platform_info = build_dir / "elfloader" / "gen_headers" / "platform_info.h"
    platform_info.write_text(
        "#pragma once\n/* no platform YAML file available */\n",
        encoding="utf-8",
    )

    evidence = sel4_profile.validate_build(
        contract,
        "pi4-production",
        build_dir,
        require_artifacts=True,
    )

    assert evidence["valid"] is False
    assert "requires a generated memory_region declaration" in _errors(evidence)
    assert (
        evidence["configuration"]["elfloader_platform_info"][
            "has_memory_regions"
        ]
        is False
    )


def test_rootservers_last_allows_prebuild_platform_info_placeholder(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "pi4_production")
    platform_info = build_dir / "elfloader" / "gen_headers" / "platform_info.h"
    platform_info.write_text(
        "#pragma once\n/* generated by the artifact target */\n",
        encoding="utf-8",
    )

    evidence = sel4_profile.validate_build(
        contract,
        "pi4-production",
        build_dir,
        require_artifacts=False,
    )

    assert evidence["valid"] is True, _errors(evidence)
    assert (
        evidence["configuration"]["elfloader_platform_info"][
            "has_memory_regions"
        ]
        is False
    )


def test_release_source_proof_must_match_configured_source(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    local_contract = copy.deepcopy(contract)
    source_root = tmp_path / "source"
    kernel = source_root / "kernel"
    commit = _init_git_repo(kernel)
    local_contract["source"]["repositories"] = {"kernel": commit}
    build_dir = _write_build_tree(tmp_path, local_contract, "qemu_smp_production")

    evidence = sel4_profile.validate_build(
        local_contract,
        "qemu_smp_production",
        build_dir,
        source_root=source_root,
        for_release=True,
    )

    assert evidence["valid"] is False
    assert "configured source root does not match" in _errors(evidence)


def test_compiler_metadata_must_match_bound_cross_prefix(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    compiler = (
        build_dir
        / "CMakeFiles"
        / "profile-test"
        / "CMakeCCompiler.cmake"
    )
    compiler.write_text(
        'set(CMAKE_C_COMPILER "/opt/toolchain/aarch64-linux-gnu-gcc")\n',
        encoding="utf-8",
    )

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
    )

    assert evidence["valid"] is False
    assert "CMAKE_C_COMPILER mismatch" in _errors(evidence)
    assert evidence["compilers"]["C"][0]["basename"] == (
        "aarch64-linux-gnu-gcc"
    )


def test_diagnostic_profile_cannot_be_release_evidence(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
        for_release=True,
    )

    assert evidence["valid"] is False
    assert "not release eligible" in _errors(evidence)


def test_release_validation_implies_source_and_artifact_proof(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_production")

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_production",
        build_dir,
        for_release=True,
    )

    assert evidence["valid"] is False
    assert evidence["requirements"]["source"] is True
    assert evidence["requirements"]["artifacts"] is True
    assert "pinned source repository is missing" in _errors(evidence)
    assert "required rootserver artifact is missing" in _errors(evidence)


def test_required_artifacts_include_rootserver_image_and_qemu_dtbs(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["qemu_smp_diagnostic"]
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    _write_required_artifacts(build_dir, contract, "qemu_smp_diagnostic")

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
        require_artifacts=True,
    )
    assert evidence["valid"] is True, _errors(evidence)
    assert {"rootserver", "system_image", "kernel_dtb", "qemu_dtb"} <= set(
        evidence["artifacts"]
    )

    rootserver = build_dir / "apps" / "sel4test-driver" / "sel4test-driver"
    rootserver.unlink()
    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
        require_artifacts=True,
    )
    assert "required rootserver artifact is missing" in _errors(evidence)

    rootserver.write_bytes(b"")
    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
        require_artifacts=True,
    )
    assert "required rootserver artifact is empty" in _errors(evidence)

    rootserver.write_bytes(_minimal_elf())
    (build_dir / "qemu-arm-virt.dtb").unlink()
    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
        require_artifacts=True,
    )
    assert "required qemu_dtb artifact is missing" in _errors(evidence)


def test_launcher_gic_header_must_match_qemu_profile(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    header = build_dir / "kernel" / "gen_config" / "kernel" / "gen_config.h"
    header.write_text(
        "/* disabled: CONFIG_ARM_GIC_V3_SUPPORT */\n",
        encoding="utf-8",
    )

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
    )

    assert evidence["valid"] is False
    assert "QEMU launcher GIC inference mismatch" in _errors(evidence)
    assert evidence["launcher_gic"]["detected_version"] == 2


def test_proof_eligibility_rejects_operational_pi_overlay(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "bcm2711_proof_eligibility")
    (build_dir / "kernel.dts").write_text(
        "device-untypes@600000000\n",
        encoding="utf-8",
    )

    evidence = sel4_profile.validate_build(
        contract,
        "bcm2711_proof_eligibility",
        build_dir,
    )

    assert evidence["valid"] is False
    assert "forbidden literal 'device-untypes@600000000'" in _errors(evidence)


def test_pi_counter_and_physical_export_mismatch_fails(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "pi4_diagnostic")
    cache = build_dir / "CMakeCache.txt"
    cache.write_text(
        cache.read_text(encoding="utf-8").replace(
            "KernelArmExportPCNTUser:BOOL=OFF",
            "KernelArmExportPCNTUser:BOOL=ON",
        ),
        encoding="utf-8",
    )
    header = build_dir / "kernel" / "gen_headers" / "plat" / "platform_gen.h"
    header.write_text("#define TIMER_CLOCK_HZ 1\n", encoding="utf-8")

    evidence = sel4_profile.validate_build(
        contract,
        "pi4_diagnostic",
        build_dir,
    )

    assert evidence["valid"] is False
    assert "KernelArmExportPCNTUser mismatch" in _errors(evidence)
    assert "TIMER_CLOCK_HZ mismatch" in _errors(evidence)


def test_wrong_source_commit_and_dirty_source_fail(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    local_contract = copy.deepcopy(contract)
    source_root = tmp_path / "source"
    kernel = source_root / "kernel"
    commit = _init_git_repo(kernel)
    local_contract["source"]["repositories"] = {"kernel": "0" * 40}
    profile = local_contract["profiles"]["qemu_smp_diagnostic"]

    evidence = sel4_profile.validate_source(local_contract, profile, source_root)
    assert "source commit mismatch" in "\n".join(evidence["errors"])

    local_contract["source"]["repositories"] = {"kernel": commit}
    (kernel / "tracked.txt").write_text("dirty\n", encoding="utf-8")
    evidence = sel4_profile.validate_source(local_contract, profile, source_root)
    assert "source repository must be clean" in "\n".join(evidence["errors"])


def test_proof_source_policy_rejects_any_kernel_patch(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    local_contract = copy.deepcopy(contract)
    source_root = tmp_path / "source"
    kernel = source_root / "kernel"
    commit = _init_git_repo(kernel)
    local_contract["source"]["repositories"] = {"kernel": commit}
    (kernel / "tracked.txt").write_text("patched\n", encoding="utf-8")
    profile = local_contract["profiles"]["bcm2711_proof_eligibility"]

    evidence = sel4_profile.validate_source(local_contract, profile, source_root)

    assert "source repository must be clean" in "\n".join(evidence["errors"])


def test_configure_refuses_tracked_generated_tree() -> None:
    with pytest.raises(sel4_profile.ProfileError, match="tracked reference tree"):
        sel4_profile.ensure_safe_transient_build_dir(
            sel4_profile.ROOT / "seL4" / "SMP_build"
        )


def test_repo_managed_pi_profile_rejects_pre_m26e_tracked_tree() -> None:
    canonical_contract = sel4_profile.load_contract(
        sel4_profile.DEFAULT_CONTRACT
    )
    build_dir = sel4_profile.ROOT / "seL4" / "build_UBOOT"

    evidence = sel4_profile.validate_repo_managed_build(
        canonical_contract,
        "pi4_diagnostic",
        build_dir,
        for_runtime=True,
    )

    assert evidence["valid"] is False
    assert "contract_values_sha256 mismatch" in _errors(evidence)
    assert evidence["build_mode"] == "repository-managed-artifacts"
    assert evidence["claim_eligibility"]["runtime"] is True
    assert evidence["claim_eligibility"]["artifact_set_shipping"] is False
    assert evidence["repo_managed"]["tracked"] is True
    assert evidence["repo_managed"]["clean"] is True


def test_repo_managed_pi_profile_rejects_noncanonical_path(
    tmp_path: Path,
) -> None:
    canonical_contract = sel4_profile.load_contract(
        sel4_profile.DEFAULT_CONTRACT
    )

    evidence = sel4_profile.validate_repo_managed_build(
        canonical_contract,
        "pi4_diagnostic",
        tmp_path,
        for_runtime=True,
    )

    assert evidence["valid"] is False
    assert "repository-managed profile selection mismatch" in _errors(evidence)


def test_repo_managed_cli_rejects_source_and_release_claims(
    capsys: pytest.CaptureFixture[str],
) -> None:
    build_dir = sel4_profile.ROOT / "seL4" / "build_UBOOT"

    source_status = sel4_profile.main(
        (
            "validate",
            "--repo-managed",
            "--profile",
            "pi4_diagnostic",
            "--build-dir",
            str(build_dir),
            "--source",
            "/tmp/not-used",
        )
    )
    source_stderr = capsys.readouterr().err
    release_status = sel4_profile.main(
        (
            "validate",
            "--repo-managed",
            "--profile",
            "pi4_diagnostic",
            "--build-dir",
            str(build_dir),
            "--for-release",
        )
    )
    release_stderr = capsys.readouterr().err

    assert source_status == 2
    assert "source validation belongs to a fresh source-build lane" in source_stderr
    assert release_status == 2
    assert "--repo-managed artifacts are not release proof" in release_stderr


def test_configure_refuses_nonempty_transient_tree(tmp_path: Path) -> None:
    build_dir = tmp_path / "profile-build"
    build_dir.mkdir()
    assert sel4_profile.ensure_fresh_build_dir(build_dir) == build_dir.resolve()
    (build_dir / "stale.txt").write_text("stale\n", encoding="utf-8")

    with pytest.raises(sel4_profile.ProfileError, match="new or empty"):
        sel4_profile.ensure_fresh_build_dir(build_dir)


def test_validate_all_uses_default_build_dirs_and_aggregates_failures(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    local_contract = copy.deepcopy(contract)
    local_contract["profiles"] = {
        "qemu_smp_diagnostic": local_contract["profiles"][
            "qemu_smp_diagnostic"
        ],
        "pi4_diagnostic": local_contract["profiles"]["pi4_diagnostic"],
    }
    local_contract["profiles"]["qemu_smp_diagnostic"][
        "default_build_dir"
    ] = "qemu_smp_diagnostic"
    local_contract["profiles"]["pi4_diagnostic"][
        "default_build_dir"
    ] = "missing-pi"
    _write_build_tree(tmp_path, local_contract, "qemu_smp_diagnostic")

    evidence = sel4_profile.validate_all_builds(
        local_contract,
        base_dir=tmp_path,
        diagnostic_relaxed=True,
    )

    assert evidence["valid"] is False
    assert evidence["failed_profiles"] == ["pi4_diagnostic"]
    assert evidence["profiles"]["qemu_smp_diagnostic"]["valid"] is True
    assert evidence["profiles"]["pi4_diagnostic"]["valid"] is False


def test_wrapper_is_parameterized_and_has_no_legacy_schedule() -> None:
    wrapper = (
        sel4_profile.ROOT / "tools" / "sel4-profile-project" / "CMakeLists.txt"
    ).read_text(encoding="utf-8")

    assert "COHESIX_SEL4_PROJECT_ROOT" in wrapper
    assert "KernelDomainSchedule" not in wrapper
    assert "/Users/" not in wrapper


def test_operational_profiles_reserve_root_cspace_for_generated_anchors(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    for profile_name in (
        "qemu_smp_production",
        "qemu_smp_diagnostic",
        "pi4_production",
        "pi4_diagnostic",
    ):
        profile = contract["profiles"][profile_name]

        assert profile["cmake"]["KernelRootCNodeSizeBits"] == "14"
        command = sel4_profile.wrapper_configure_command(
            profile,
            tmp_path / "source",
            tmp_path / profile_name,
        )
        assert "-DKernelRootCNodeSizeBits=14" in command


def test_sel4_16_profiles_pin_virtual_timer_offset_updates_off(
    contract: dict[str, Any],
) -> None:
    for profile_name, profile in contract["profiles"].items():
        assert profile["cmake"]["KernelArmVtimerUpdateVOffset"] == "OFF", profile_name
        assert profile["generated"]["VTIMER_UPDATE_VOFFSET"] is False, profile_name


def test_operational_profiles_are_mcs_only_with_exact_virtual_counter_truth(
    contract: dict[str, Any],
) -> None:
    expected_timer_hz = {
        "qemu_smp_production": 62_500_000,
        "qemu_smp_diagnostic": 62_500_000,
        "pi4_production": 54_000_000,
        "pi4_diagnostic": 54_000_000,
    }

    for profile_name, timer_hz in expected_timer_hz.items():
        profile = contract["profiles"][profile_name]
        assert profile["runtime_eligible"] is True, profile_name
        assert profile["cmake"]["MCS"] == "ON", profile_name
        assert profile["cmake"]["KernelIsMCS"] == "ON", profile_name
        assert profile["generated"]["KERNEL_MCS"] is True, profile_name
        assert profile["cmake"]["KernelArmExportVCNTUser"] == "ON", profile_name
        assert profile["generated"]["EXPORT_VCNT_USER"] is True, profile_name
        assert profile["cmake"]["KernelArmExportPCNTUser"] == "OFF", profile_name
        assert profile["cmake"]["KernelArmExportPTMRUser"] == "OFF", profile_name
        assert profile["cmake"]["KernelArmExportVTMRUser"] == "OFF", profile_name
        assert profile["timer_clock_hz"] == timer_hz, profile_name

    classic_runtime_profiles = [
        name
        for name, profile in contract["profiles"].items()
        if profile["runtime_eligible"]
        and (
            profile["cmake"].get("KernelIsMCS") != "ON"
            or profile["generated"].get("KERNEL_MCS") is not True
        )
    ]
    assert classic_runtime_profiles == []


def test_wrapper_preserves_profile_root_cnode_before_upstream_settings() -> None:
    wrapper = sel4_profile.WRAPPER_CMAKE.read_text(encoding="utf-8")

    capture = wrapper.index(
        'set(_cohesix_profile_root_cnode_size_bits "")'
    )
    upstream_settings = wrapper.index(
        'include("${_cohesix_sel4test_dir}/settings.cmake")'
    )
    apply_profile = wrapper.index(
        '"${_cohesix_profile_root_cnode_size_bits}"\n'
        "    CACHE INTERNAL"
    )

    assert capture < upstream_settings < apply_profile
    assert "set(_cohesix_profile_root_cnode_size_bits 13)" in wrapper
    assert "Root CNode size selected by the Cohesix seL4 profile" in wrapper


def test_qemu_configure_sets_arch_without_unused_qemu_smp(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["qemu_smp_diagnostic"]
    command = sel4_profile.wrapper_configure_command(
        profile,
        tmp_path / "source",
        tmp_path / "build",
    )

    assert "-DAARCH64=ON" in command
    assert "-DCROSS_COMPILER_PREFIX=aarch64-none-elf-" in command
    expected_python = sel4_profile.resolve_profile_tool(
        profile,
        "python_tool",
        preserve_symlink=True,
    )
    assert f"-DPYTHON3={expected_python}" in command
    assert "-DQEMU_GIC_VERSION=3" in command
    assert any(argument.startswith("-DQEMU_MACHINE=") for argument in command)
    assert "-DKernelArmGicV3=ON" not in command
    assert not any(argument.startswith("-DQEMU_SMP=") for argument in command)
    assert "-DSEL4_CACHE_DIR=" in command


def test_qemu_production_reserves_rootserver_archive_capacity() -> None:
    contract = sel4_profile.load_contract()
    wrapper = sel4_profile.WRAPPER_CMAKE.read_text(encoding="utf-8")

    profile = contract["profiles"]["qemu_smp_production"]
    assert profile["minimum_elfloader_archive_bytes"] >= 8 * 1024 * 1024
    assert (
        int(profile["cmake"]["COHESIX_ROOTSERVER_ARCHIVE_RESERVE_BYTES"])
        >= 6 * 1024 * 1024
    )
    assert "cohesix_rootserver_archive_reserve" in wrapper
    assert "target_sources(sel4test-driver" in wrapper


def test_qemu_archive_capacity_fails_closed(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_production")
    archive = build_dir / "elfloader" / "archive.archive.o.cpio"
    archive.write_bytes(b"A" * 31)

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_production",
        build_dir,
        require_artifacts=True,
    )

    assert evidence["valid"] is False
    assert "elfloader archive capacity is below the profile minimum" in _errors(
        evidence
    )


def test_pi_configure_binds_objcopy_stdout_wrapper(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["pi4_production"]
    command = sel4_profile.wrapper_configure_command(
        profile,
        tmp_path / "source",
        tmp_path / "build",
    )

    expected = sel4_profile.resolve_profile_tool(
        profile,
        "objcopy_stdout_wrapper",
    )
    assert f"-DCMAKE_OBJCOPY={expected}" in command


def test_pi_validation_rejects_unwrapped_objcopy(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "pi4_production")
    cache = build_dir / "CMakeCache.txt"
    expected = sel4_profile.resolve_profile_tool(
        contract["profiles"]["pi4_production"],
        "objcopy_stdout_wrapper",
    )
    cache.write_text(
        cache.read_text(encoding="utf-8").replace(
            f"CMAKE_OBJCOPY:FILEPATH={expected}",
            "CMAKE_OBJCOPY:FILEPATH=/tmp/aarch64-none-elf-objcopy",
        ),
        encoding="utf-8",
    )

    evidence = sel4_profile.validate_build(
        contract,
        "pi4-production",
        build_dir,
    )

    assert evidence["valid"] is False
    assert "not the profile-bound stdout wrapper" in _errors(evidence)


def test_all_profiles_bind_bare_metal_cross_compiler(
    contract: dict[str, Any],
) -> None:
    profiles = list(contract["profiles"].values())

    assert profiles
    assert all(
        profile["cmake"].get("CROSS_COMPILER_PREFIX")
        == "aarch64-none-elf-"
        for profile in profiles
    )


def test_verified_config_command_forwards_bound_cross_compiler(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["bcm2711_proof_eligibility"]
    config = tmp_path / profile["verified_config"]
    config.parent.mkdir(parents=True)
    config.write_text("# profile test\n", encoding="utf-8")

    command = sel4_profile.verified_config_build_command(profile, tmp_path)

    assert "-DCROSS_COMPILER_PREFIX=aarch64-none-elf-" in command
    assert command[-2:] == ["-DSEL4_CACHE_DIR=", "-DMEMOIZE_CACHE_DIR="]


def test_wrapper_build_requests_declared_rootserver_image(tmp_path: Path) -> None:
    command = sel4_profile.wrapper_build_command(tmp_path / "build", 6)

    assert command == [
        "cmake",
        "--build",
        str(tmp_path / "build"),
        "--target",
        "rootserver_image",
        "--parallel",
        "6",
    ]


def test_build_records_created_outputs_and_rejects_tree_reuse(
    tmp_path: Path,
    contract: dict[str, Any],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    profile = contract["profiles"]["qemu_smp_diagnostic"]
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    source_root = Path("/missing/pinned-project")
    source_evidence = {
        "root": str(source_root),
        "policy": profile["source_policy"],
        "repositories": {},
        "errors": [],
    }
    monkeypatch.setattr(
        sel4_profile,
        "validate_source",
        lambda *_args, **_kwargs: copy.deepcopy(source_evidence),
    )
    monkeypatch.setattr(
        sel4_profile,
        "validate_build",
        lambda *_args, **_kwargs: {"valid": True, "errors": []},
    )
    original_run_checked = sel4_profile.run_checked
    build_invocations = 0

    def run_checked_with_outputs(
        argv: Any,
        **kwargs: Any,
    ) -> subprocess.CompletedProcess[str]:
        nonlocal build_invocations
        if list(argv[:2]) == ["cmake", "--build"]:
            build_invocations += 1
            elf_labels = set(profile["artifact_policy"]["elf_artifacts"])
            for label, candidates in sel4_profile.artifact_candidates(
                build_dir,
                profile,
            ).items():
                artifact = candidates[0]
                artifact.parent.mkdir(parents=True, exist_ok=True)
                if label in elf_labels:
                    artifact.write_bytes(_minimal_elf())
                elif label.endswith("dtb"):
                    artifact.write_bytes(_minimal_dtb(profile))
                else:
                    artifact.write_bytes(b"profile-test-artifact\n")
            return subprocess.CompletedProcess(list(argv), 0, "built\n", "")
        return original_run_checked(argv, **kwargs)

    monkeypatch.setattr(sel4_profile, "run_checked", run_checked_with_outputs)

    sel4_profile.build_profile(
        contract,
        "qemu_smp_diagnostic",
        source_root,
        build_dir,
        jobs=3,
        dry_run=False,
    )

    stamp = json.loads(
        (build_dir / sel4_profile.BUILD_INPUT_STAMP_NAME).read_text(
            encoding="utf-8"
        )
    )
    freshness = stamp["causal_freshness"]
    assert freshness["build_start"]["stamp"]["exists"] is False
    assert all(
        record["existing"] == []
        for record in freshness["build_start"]["outputs"].values()
    )
    assert set(freshness["post_build_outputs"]) == set(
        profile["artifact_policy"]["elf_artifacts"]
    )
    assert build_invocations == 1

    with pytest.raises(sel4_profile.ProfileError, match="re-stamping or reusing"):
        sel4_profile.build_profile(
            contract,
            "qemu_smp_diagnostic",
            source_root,
            build_dir,
            jobs=3,
            dry_run=False,
        )
    assert build_invocations == 1


def test_build_rejects_preexisting_output_without_stamp(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["qemu_smp_diagnostic"]
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    kernel = build_dir / "kernel" / "kernel.elf"
    kernel.write_bytes(_minimal_elf())

    with pytest.raises(
        sel4_profile.ProfileError,
        match="already contains build-created outputs",
    ):
        sel4_profile.require_fresh_profile_build_start(build_dir, profile)


def test_pi_build_path_binds_profile_mkimage_tool(
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["pi4_production"]
    python = sel4_profile.resolve_profile_tool(
        profile,
        "python_tool",
        preserve_symlink=True,
    )
    mkimage = sel4_profile.resolve_profile_tool(profile, "mkimage_tool")

    environment = sel4_profile.wrapper_build_environment(
        contract,
        profile,
        {"PATH": "/usr/bin"},
    )

    assert environment is not None
    assert environment["PATH"] == os.pathsep.join(
        (
            str(Path(contract["toolchain"]["cpio"]["path"]).parent),
            *(
                str(
                    sel4_profile.contract_repo_path(
                        value,
                        "toolchain.compiler.path_prefixes",
                    )
                )
                for value in contract["toolchain"]["compiler"][
                    "path_prefixes"
                ]
            ),
            str(python.parent),
            str(mkimage.parent),
            "/usr/bin",
        )
    )


def test_wrapper_build_path_binds_profile_python_tool(
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["qemu_smp_production"]
    python = sel4_profile.resolve_profile_tool(
        profile,
        "python_tool",
        preserve_symlink=True,
    )

    environment = sel4_profile.wrapper_build_environment(
        contract,
        profile,
        {"PATH": "/usr/bin"},
    )

    assert environment is not None
    assert environment["PATH"] == os.pathsep.join(
        (
            str(Path(contract["toolchain"]["cpio"]["path"]).parent),
            *(
                str(
                    sel4_profile.contract_repo_path(
                        value,
                        "toolchain.compiler.path_prefixes",
                    )
                )
                for value in contract["toolchain"]["compiler"][
                    "path_prefixes"
                ]
            ),
            str(python.parent),
            "/usr/bin",
        )
    )


def test_wrapper_build_path_selects_pinned_gnu_cpio(
    contract: dict[str, Any],
) -> None:
    """The bare upstream cpio command must resolve to the pinned GNU binary."""

    profile = contract["profiles"]["qemu_smp_production"]
    environment = sel4_profile.wrapper_build_environment(
        contract,
        profile,
        {"PATH": "/usr/bin:/bin"},
    )
    cpio_path = Path(contract["toolchain"]["cpio"]["path"])
    cpio = sel4_profile.gnu_cpio_supply_chain_input(contract)

    assert environment is not None
    assert shutil.which("cpio", path=environment["PATH"]) == str(cpio_path)
    assert environment["PATH"].split(os.pathsep)[0] == str(cpio_path.parent)
    assert cpio["version"] == "cpio (GNU cpio) 2.15"
    assert cpio["executable"]["sha256"] == contract["toolchain"]["cpio"][
        "sha256"
    ]
    assert set(cpio["required_options"]) == {
        "--append",
        "--owner",
        "--quiet",
        "--format",
        "--file",
        "--reproducible",
    }


def test_wrapper_build_path_rejects_bsd_cpio(
    tmp_path: Path,
    contract: dict[str, Any],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """A compatible filename must not substitute for GNU cpio semantics."""

    fake_cpio = tmp_path / "cpio"
    fake_cpio.write_text(
        "#!/bin/sh\n"
        "if [ \"$1\" = \"--version\" ]; then\n"
        "  echo 'bsdcpio 3.5.3 - libarchive 3.7.4'\n"
        "  exit 0\n"
        "fi\n"
        "echo '--append --owner --quiet --format --file --reproducible'\n",
        encoding="utf-8",
    )
    fake_cpio.chmod(0o755)
    local_contract = copy.deepcopy(contract)
    local_contract["toolchain"]["cpio"].update(
        {
            "path": str(fake_cpio),
            "sha256": sel4_profile.sha256_file(fake_cpio),
        }
    )

    with pytest.raises(sel4_profile.ProfileError, match="version mismatch"):
        sel4_profile.wrapper_build_environment(
            local_contract,
            local_contract["profiles"]["qemu_smp_production"],
            {"PATH": "/usr/bin:/bin"},
        )


def test_verified_build_path_binds_profile_python_tool(
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["bcm2711_proof_eligibility"]
    python = sel4_profile.resolve_profile_tool(
        profile,
        "python_tool",
        preserve_symlink=True,
    )

    environment = sel4_profile.wrapper_build_environment(
        contract,
        profile,
        {"PATH": "/usr/bin"},
    )

    assert environment is not None
    assert environment["PATH"] == os.pathsep.join(
        (
            *(
                str(
                    sel4_profile.contract_repo_path(
                        value,
                        "toolchain.compiler.path_prefixes",
                    )
                )
                for value in contract["toolchain"]["compiler"][
                    "path_prefixes"
                ]
            ),
            str(python.parent),
            "/usr/bin",
        )
    )


def test_real_contract_declares_pinned_generated_mkimage() -> None:
    real_contract = sel4_profile.load_contract()

    assert (
        real_contract["profiles"]["pi4_production"]["mkimage_tool"]
        == "out/toolchain/u-boot-tools-build/tools/mkimage"
    )
    assert (
        real_contract["toolchain"]["mkimage"]["source_archive_sha256"]
        == "b60d5865cefdbc75da8da4156c56c458e00de75a49b80c1a2e58a96e30ad0d54"
    )
    assert real_contract["toolchain"]["mkimage"]["provider"] == (
        "denx-release-tarball"
    )


def test_real_contract_declares_exact_cellar_gnu_cpio() -> None:
    """The Mac profile must not trust BSD cpio or a mutable Homebrew opt link."""

    cpio = sel4_profile.load_contract()["toolchain"]["cpio"]

    assert cpio == {
        "path": "/opt/homebrew/Cellar/cpio/2.15/bin/cpio",
        "provider": "homebrew",
        "formula": "cpio",
        "version": "2.15",
        "sha256": (
            "b09b46a77c735ab2d0687e87c7fadd2b4060a8c9492f1d896ee507b1d5262304"
        ),
        "required_options": [
            "--append",
            "--owner",
            "--quiet",
            "--format",
            "--file",
            "--reproducible",
        ],
    }


def test_wrapper_host_inputs_bind_exact_gnu_cpio(
    contract: dict[str, Any],
) -> None:
    """The causal host-input record must include the selected archive tool."""

    inputs = sel4_profile.wrapper_build_inputs(
        contract,
        "qemu_smp_production",
        contract["profiles"]["qemu_smp_production"],
    )

    assert inputs["schema"] == "cohesix-sel4-wrapper-host-inputs/v4"
    assert inputs["cpio_tool"] == sel4_profile.gnu_cpio_supply_chain_input(
        contract
    )


def test_missing_profile_mkimage_is_rejected(
    contract: dict[str, Any],
) -> None:
    profile = copy.deepcopy(contract["profiles"]["pi4_production"])
    profile["mkimage_tool"] = "third_party/u-boot/tools/missing-mkimage"

    with pytest.raises(sel4_profile.ProfileError, match="mkimage_tool is missing"):
        sel4_profile.wrapper_build_environment(
            contract,
            profile,
            {"PATH": "/usr/bin"},
        )


def test_profile_mkimage_command_name_is_fail_closed(
    contract: dict[str, Any],
) -> None:
    profile = copy.deepcopy(contract["profiles"]["pi4_production"])
    mkimage = sel4_profile.resolve_profile_tool(profile, "mkimage_tool")
    assert mkimage is not None
    wrong_name = mkimage.parent / "not-mkimage"
    shutil.copy2(mkimage, wrong_name)
    wrong_name.chmod(0o755)
    profile["mkimage_tool"] = str(wrong_name.relative_to(sel4_profile.ROOT))

    with pytest.raises(sel4_profile.ProfileError, match="must be named mkimage"):
        sel4_profile.wrapper_build_environment(
            contract,
            profile,
            {"PATH": "/usr/bin"},
        )


def test_setup_script_provisions_contract_bound_compiler_cpio_python_and_mkimage() -> None:
    real_contract = sel4_profile.load_contract()
    compiler_contract = real_contract["toolchain"]["compiler"]
    cpio_contract = real_contract["toolchain"]["cpio"]
    python_contract = real_contract["toolchain"]["python"]
    mkimage_contract = real_contract["toolchain"]["mkimage"]
    setup = (sel4_profile.ROOT / "toolchain" / "setup_macos_arm64.sh").read_text(
        encoding="utf-8"
    )

    compiler_url_base, compiler_archive_name = compiler_contract[
        "source_url"
    ].rsplit("/", 1)
    assert f'COMPILER_ARCHIVE_NAME="{compiler_archive_name}"' in setup
    assert (
        f'COMPILER_ARCHIVE_URL="{compiler_url_base}/'
        '${COMPILER_ARCHIVE_NAME}"' in setup
    )
    assert (
        f'COMPILER_ARCHIVE_SHA256="'
        f'{compiler_contract["source_archive_sha256"]}"' in setup
    )
    assert (
        f'COMPILER_ARCHIVE_SIZE="{compiler_contract["source_archive_size"]}"'
        in setup
    )
    compiler_hash_variables = {
        "gcc_sha256": "COMPILER_GCC_SHA256",
        "gxx_sha256": "COMPILER_GXX_SHA256",
        "cpp_sha256": "COMPILER_CPP_SHA256",
        "as_sha256": "COMPILER_AS_SHA256",
        "ld_sha256": "COMPILER_LD_SHA256",
        "objcopy_sha256": "COMPILER_OBJCOPY_SHA256",
        "ar_sha256": "COMPILER_AR_SHA256",
        "ranlib_sha256": "COMPILER_RANLIB_SHA256",
    }
    for field, variable in compiler_hash_variables.items():
        assert f'{variable}="{compiler_contract[field]}"' in setup
    assert 'tar -xJf "${COMPILER_ARCHIVE}"' in setup
    assert 'export PATH="${COMPILER_BIN}:${PATH}"' in setup
    assert '"schema": "cohesix-compiler-provenance/v1"' in setup
    assert "\n    cpio\n" in setup
    assert 'declared = tomllib.load(stream)["toolchain"]["cpio"]' in setup
    assert 'version = subprocess.run(' in setup
    assert 'declared["required_options"]' in setup
    assert cpio_contract["path"] in setup or 'declared["path"]' in setup
    assert 'PROFILE_VENV="${TOOLCHAIN_ROOT}/sel4-profile-venv"' in setup
    assert (
        f'PYTHON_BOOTSTRAP_LOCK_SHA256="'
        f'{python_contract["bootstrap_lock_sha256"]}"' in setup
    )
    assert (
        f'PYTHON_REQUIREMENTS_LOCK_SHA256="'
        f'{python_contract["requirements_lock_sha256"]}"' in setup
    )
    assert 'guard_replace_dir "${PROFILE_VENV}"' in setup
    assert '"${HOMEBREW_PYTHON}" -m venv "${PROFILE_VENV}"' in setup
    assert '--require-hashes --requirement "${PYTHON_REQUIREMENTS_LOCK}"' in setup
    assert "if observed != expected:" in setup
    assert f'UBOOT_SOURCE_URL="{mkimage_contract["source_url"]}"' in setup
    assert (
        f'UBOOT_SOURCE_SHA256="{mkimage_contract["source_archive_sha256"]}"'
        in setup
    )
    assert f'UBOOT_SOURCE_SIZE="{mkimage_contract["source_archive_size"]}"' in setup
    assert f'MKIMAGE_VERSION="{mkimage_contract["version"]}"' in setup
    assert 'tar -xjf "${UBOOT_SOURCE_ARCHIVE}"' in setup
    assert 'GNU_MAKE="$(brew --prefix make)/bin/gmake"' in setup
    assert 'OPENSSL_PREFIX="$(brew --prefix openssl@3)"' in setup
    assert 'PKG_CONFIG_PATH="${OPENSSL_PKG_CONFIG_PATH}"' in setup
    assert 'HOSTCFLAGS="-I${OPENSSL_PREFIX}/include"' in setup
    assert 'HOSTLDFLAGS="-L${OPENSSL_PREFIX}/lib"' in setup
    assert 'HOSTLDLIBS="${OPENSSL_HOST_LDLIBS}"' in setup
    assert '"${GNU_MAKE}" -C "${UBOOT_SNAPSHOT}"' in setup
    assert "tools-only_defconfig" in setup
    assert "tools-only" in setup
    assert "\n    make -C " not in setup


def test_contract_rejects_tampered_python_lock_digest(tmp_path: Path) -> None:
    text = sel4_profile.DEFAULT_CONTRACT.read_text(encoding="utf-8")
    with sel4_profile.DEFAULT_CONTRACT.open("rb") as stream:
        contract = tomllib.load(stream)
    declared = contract["toolchain"]["python"][
        "requirements_lock_sha256"
    ]
    tampered = tmp_path / "tampered-lock-digest.toml"
    tampered.write_text(
        text.replace(declared, "0" * 64, 1),
        encoding="utf-8",
    )

    with pytest.raises(sel4_profile.ProfileError, match="lock digest mismatch"):
        sel4_profile.load_contract(tampered)


def test_contract_rejects_compiler_path_escape(tmp_path: Path) -> None:
    text = sel4_profile.DEFAULT_CONTRACT.read_text(encoding="utf-8")
    with sel4_profile.DEFAULT_CONTRACT.open("rb") as stream:
        contract = tomllib.load(stream)
    declared = contract["toolchain"]["compiler"]["bin_path"]
    tampered = tmp_path / "tampered-compiler-path.toml"
    tampered.write_text(
        text.replace(
            f'bin_path = "{declared}"',
            'bin_path = "../escaped-compiler/bin"',
            1,
        ),
        encoding="utf-8",
    )

    with pytest.raises(sel4_profile.ProfileError, match="escapes the repository"):
        sel4_profile.load_contract(tampered)


def test_contract_rejects_mutable_gnu_cpio_opt_path(tmp_path: Path) -> None:
    """The cpio contract must name the resolved versioned Cellar executable."""

    text = sel4_profile.DEFAULT_CONTRACT.read_text(encoding="utf-8")
    tampered = tmp_path / "tampered-cpio-path.toml"
    tampered.write_text(
        text.replace(
            'path = "/opt/homebrew/Cellar/cpio/2.15/bin/cpio"',
            'path = "/opt/homebrew/opt/cpio/bin/cpio"',
            1,
        ),
        encoding="utf-8",
    )

    with pytest.raises(sel4_profile.ProfileError, match="exact Apple Silicon"):
        sel4_profile.load_contract(tampered)


def test_contract_rejects_unknown_evidence_class(tmp_path: Path) -> None:
    text = sel4_profile.DEFAULT_CONTRACT.read_text(encoding="utf-8")
    tampered = tmp_path / "tampered-evidence-class.toml"
    tampered.write_text(
        text.replace(
            'evidence_class = "production"',
            'evidence_class = "release-ish"',
            1,
        ),
        encoding="utf-8",
    )

    with pytest.raises(
        sel4_profile.ProfileError,
        match="unsupported evidence_class",
    ):
        sel4_profile.load_contract(tampered)


def test_contract_binds_evidence_class_to_eligibility(tmp_path: Path) -> None:
    text = sel4_profile.DEFAULT_CONTRACT.read_text(encoding="utf-8")
    diagnostic = (
        'evidence_class = "diagnostic"\n'
        'target = "qemu"\n'
        'default_build_dir = "out/sel4/profile-v2/qemu-smp-diagnostic"\n'
        'release_eligible = false\n'
        'runtime_eligible = true'
    )
    tampered = tmp_path / "tampered-evidence-eligibility.toml"
    tampered.write_text(
        text.replace(
            diagnostic,
            diagnostic.replace("release_eligible = false", "release_eligible = true"),
            1,
        ),
        encoding="utf-8",
    )

    with pytest.raises(
        sel4_profile.ProfileError,
        match="diagnostic.*evidence must set",
    ):
        sel4_profile.load_contract(tampered)


def test_contract_rejects_untyped_eligibility_flags(tmp_path: Path) -> None:
    text = sel4_profile.DEFAULT_CONTRACT.read_text(encoding="utf-8")
    tampered = tmp_path / "tampered-evidence-eligibility-type.toml"
    tampered.write_text(
        text.replace("release_eligible = true", 'release_eligible = "true"', 1),
        encoding="utf-8",
    )

    with pytest.raises(
        sel4_profile.ProfileError,
        match="release/runtime eligibility must be boolean",
    ):
        sel4_profile.load_contract(tampered)


def test_python_lock_file_tamper_is_rejected(
    contract: dict[str, Any],
    supply_chain_paths: dict[str, Path],
) -> None:
    lock = supply_chain_paths["requirements_lock"]
    lock.write_text(
        lock.read_text(encoding="utf-8") + "# tampered\n",
        encoding="utf-8",
    )

    with pytest.raises(sel4_profile.ProfileError, match="lock digest mismatch"):
        sel4_profile.python_supply_chain_inputs(
            contract,
            contract["profiles"]["qemu_smp_production"],
        )


def test_python_package_version_tamper_is_rejected(
    contract: dict[str, Any],
    supply_chain_paths: dict[str, Path],
) -> None:
    probe_path = supply_chain_paths["python_probe"]
    probe = json.loads(probe_path.read_text(encoding="utf-8"))
    probe["distributions"]["pytest"] = "99.0.0"
    probe_path.write_text(json.dumps(probe) + "\n", encoding="utf-8")

    with pytest.raises(
        sel4_profile.ProfileError,
        match="Python distribution mismatch",
    ):
        sel4_profile.python_supply_chain_inputs(
            contract,
            contract["profiles"]["qemu_smp_production"],
        )


def test_python_installed_content_tamper_is_rejected(
    contract: dict[str, Any],
    supply_chain_paths: dict[str, Path],
) -> None:
    probe_path = supply_chain_paths["python_probe"]
    probe = json.loads(probe_path.read_text(encoding="utf-8"))
    probe["installed_content"]["sha256"] = "0" * 64
    probe_path.write_text(json.dumps(probe) + "\n", encoding="utf-8")

    with pytest.raises(
        sel4_profile.ProfileError,
        match="installed-content digest mismatch",
    ):
        sel4_profile.python_supply_chain_inputs(
            contract,
            contract["profiles"]["qemu_smp_production"],
        )


def test_compiler_binary_hash_tamper_is_rejected(
    contract: dict[str, Any],
    supply_chain_paths: dict[str, Path],
) -> None:
    compiler = supply_chain_paths["compiler_gcc"]
    compiler.write_bytes(compiler.read_bytes() + b"\n# tampered\n")

    with pytest.raises(
        sel4_profile.ProfileError,
        match="compiler binary digest mismatch",
    ):
        sel4_profile.compiler_supply_chain_inputs(contract)


def test_generated_compiler_path_tamper_is_rejected(
    tmp_path: Path,
    contract: dict[str, Any],
    supply_chain_paths: dict[str, Path],
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    lookalike = tmp_path / "lookalike" / supply_chain_paths["compiler_gcc"].name
    _write_fake_compiler(
        lookalike,
        contract["toolchain"]["version"],
        contract["toolchain"]["target_triple"],
    )
    metadata = next(
        build_dir.glob("CMakeFiles/*/CMakeCCompiler.cmake")
    )
    metadata.write_text(
        metadata.read_text(encoding="utf-8").replace(
            str(supply_chain_paths["compiler_gcc"]),
            str(lookalike),
            1,
        ),
        encoding="utf-8",
    )

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
    )

    assert evidence["valid"] is False
    assert "not from the pinned compiler bin" in _errors(evidence)


def test_compiler_provenance_tamper_is_rejected(
    contract: dict[str, Any],
    supply_chain_paths: dict[str, Path],
) -> None:
    provenance_path = supply_chain_paths["compiler_provenance"]
    provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
    provenance["source"]["release"] = "tampered"
    provenance_path.write_text(
        json.dumps(provenance) + "\n",
        encoding="utf-8",
    )

    with pytest.raises(
        sel4_profile.ProfileError,
        match="compiler provenance does not match",
    ):
        sel4_profile.compiler_supply_chain_inputs(contract)


def test_mkimage_provenance_tamper_is_rejected(
    contract: dict[str, Any],
    supply_chain_paths: dict[str, Path],
) -> None:
    provenance_path = supply_chain_paths["mkimage_provenance"]
    provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
    provenance["source"]["commit"] = "0" * 40
    provenance_path.write_text(
        json.dumps(provenance) + "\n",
        encoding="utf-8",
    )

    with pytest.raises(
        sel4_profile.ProfileError,
        match="mkimage provenance does not match",
    ):
        sel4_profile.mkimage_supply_chain_inputs(
            contract,
            contract["profiles"]["pi4_production"],
        )


def test_mkimage_binary_tamper_is_rejected(
    contract: dict[str, Any],
    supply_chain_paths: dict[str, Path],
) -> None:
    mkimage = supply_chain_paths["mkimage"]
    mkimage.write_bytes(mkimage.read_bytes() + b"\n# tampered\n")

    with pytest.raises(
        sel4_profile.ProfileError,
        match="mkimage provenance does not match",
    ):
        sel4_profile.mkimage_supply_chain_inputs(
            contract,
            contract["profiles"]["pi4_production"],
        )


def test_artifact_validation_rejects_stale_wrapper_build_input_stamp(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["qemu_smp_diagnostic"]
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    _write_required_artifacts(build_dir, contract, "qemu_smp_diagnostic")
    stamp = build_dir / sel4_profile.BUILD_INPUT_STAMP_NAME
    inputs = json.loads(stamp.read_text(encoding="utf-8"))
    inputs["status"] = "pending"
    stamp.write_text(json.dumps(inputs) + "\n", encoding="utf-8")

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
        require_artifacts=True,
    )

    assert evidence["valid"] is False
    assert "profile build-input stamp does not match" in _errors(evidence)


@pytest.mark.parametrize(
    "relative_path",
    (
        "build.ninja",
        "kernel/kernel.elf",
    ),
)
def test_build_input_stamp_rejects_configuration_or_artifact_mutation(
    tmp_path: Path,
    contract: dict[str, Any],
    relative_path: str,
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    _write_required_artifacts(build_dir, contract, "qemu_smp_diagnostic")
    with (build_dir / relative_path).open("ab") as stream:
        stream.write(b"post-build mutation\n")

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
        require_artifacts=True,
    )

    assert evidence["valid"] is False
    assert "profile build-input stamp does not match" in _errors(evidence)


def test_build_input_stamp_rejects_stale_source_revision(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    local_contract = copy.deepcopy(contract)
    source_root = tmp_path / "source"
    kernel = source_root / "kernel"
    initial_commit = _init_git_repo(kernel)
    local_contract["source"]["repositories"] = {"kernel": initial_commit}
    profile = local_contract["profiles"]["qemu_smp_diagnostic"]
    build_dir = _write_build_tree(
        tmp_path,
        local_contract,
        "qemu_smp_diagnostic",
    )
    cache = build_dir / "CMakeCache.txt"
    cache.write_text(
        cache.read_text(encoding="utf-8").replace(
            "/missing/pinned-project",
            str(source_root.resolve()),
        ),
        encoding="utf-8",
    )
    _write_required_artifacts(build_dir, local_contract, "qemu_smp_diagnostic")
    source_evidence = sel4_profile.validate_source(
        local_contract,
        profile,
        source_root,
    )
    existing_stamp = json.loads(
        (build_dir / sel4_profile.BUILD_INPUT_STAMP_NAME).read_text(
            encoding="utf-8"
        )
    )
    stamp = sel4_profile.profile_build_stamp(
        local_contract,
        "qemu_smp_diagnostic",
        profile,
        source_root,
        build_dir,
        source_evidence,
        build_start=existing_stamp["causal_freshness"]["build_start"],
        jobs=4,
        status="complete",
        require_outputs=True,
    )
    sel4_profile.write_wrapper_build_input_stamp(build_dir, stamp)

    (kernel / "tracked.txt").write_text("next revision\n", encoding="utf-8")
    subprocess.run(("git", "-C", str(kernel), "add", "tracked.txt"), check=True)
    subprocess.run(
        ("git", "-C", str(kernel), "commit", "-q", "-m", "next"),
        check=True,
    )
    current_commit = subprocess.run(
        ("git", "-C", str(kernel), "rev-parse", "HEAD"),
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    ).stdout.strip()
    local_contract["source"]["repositories"] = {"kernel": current_commit}

    evidence = sel4_profile.validate_build(
        local_contract,
        "qemu_smp_diagnostic",
        build_dir,
        source_root=source_root,
        require_source=True,
        require_artifacts=True,
    )

    assert evidence["valid"] is False
    assert "profile build-input stamp does not match" in _errors(evidence)


def test_validate_all_requires_source_and_artifacts_unless_explicitly_relaxed(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    local_contract = copy.deepcopy(contract)
    local_contract["profiles"] = {
        "qemu_smp_diagnostic": local_contract["profiles"]["qemu_smp_diagnostic"]
    }
    local_contract["profiles"]["qemu_smp_diagnostic"][
        "default_build_dir"
    ] = "qemu_smp_diagnostic"
    _write_build_tree(tmp_path, local_contract, "qemu_smp_diagnostic")

    closure = sel4_profile.validate_all_builds(local_contract, base_dir=tmp_path)
    assert closure["valid"] is False
    assert closure["requirements"]["source"] is True
    assert closure["requirements"]["artifacts"] is True
    errors = _errors(closure["profiles"]["qemu_smp_diagnostic"])
    assert "pinned source repository is missing" in errors
    assert "required kernel artifact is missing" in errors

    diagnostic = sel4_profile.validate_all_builds(
        local_contract,
        base_dir=tmp_path,
        diagnostic_relaxed=True,
    )
    assert diagnostic["valid"] is True
    assert diagnostic["requirements"]["diagnostic_relaxed"] is True
    assert diagnostic["requirements"]["source"] is False
    assert diagnostic["requirements"]["artifacts"] is False


def test_evidence_binds_repository_inputs_configuration_and_compiler(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["qemu_smp_diagnostic"]
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    _write_required_artifacts(build_dir, contract, "qemu_smp_diagnostic")

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
        contract_path=sel4_profile.DEFAULT_CONTRACT,
        require_artifacts=True,
    )

    assert evidence["valid"] is True, _errors(evidence)
    assert evidence["schema"] == "cohesix-sel4-profile-evidence/v2"
    assert len(evidence["cohesix_repository"]["commit"]) == 40
    assert isinstance(evidence["cohesix_repository"]["dirty"], bool)
    assert evidence["inputs"]["contract"]["file"]["sha256"]
    assert evidence["inputs"]["contract"]["values_sha256"]
    assert evidence["inputs"]["validator"]["sha256"]
    assert evidence["inputs"]["wrapper"]["sha256"]
    configuration = evidence["configuration"]
    assert configuration["cmake_cache"]["sha256"]
    assert configuration["cmake_cache"]["validated_values"]["KernelPlatform"] == {
        "expected": "qemu-arm-virt",
        "actual": "qemu-arm-virt",
    }
    assert configuration["generated_config"]["sha256"]
    assert configuration["generated_config"]["validated_values"][
        "ARM_GIC_V3_SUPPORT"
    ] == {"expected": True, "actual": True}
    assert len(configuration["dts"]) == 2
    assert set(configuration["dtb"]["inspections"]) == {
        "kernel_dtb",
        "qemu_dtb",
    }
    compiler = evidence["compilers"]["C"][0]
    assert compiler["resolved_sha256"]
    assert compiler["live_version"] == contract["toolchain"]["version"]
    assert compiler["target_triple"] == contract["toolchain"]["target_triple"]


@pytest.mark.parametrize(
    ("version", "target", "expected_error"),
    (
        ("99.0.0", "aarch64-none-elf", "compiler version mismatch"),
        ("15.2.1", "aarch64-unknown-linux-gnu", "compiler target mismatch"),
    ),
)
def test_resolved_compiler_identity_must_match_contract(
    tmp_path: Path,
    contract: dict[str, Any],
    supply_chain_paths: dict[str, Path],
    version: str,
    target: str,
    expected_error: str,
) -> None:
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    _write_fake_compiler(
        supply_chain_paths["compiler_gcc"],
        version,
        target,
    )

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
    )

    assert evidence["valid"] is False
    assert expected_error in _errors(evidence)


def test_nonshipping_rwx_is_recorded_and_never_claimed_as_shipping(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["qemu_smp_production"]
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_production")
    _write_required_artifacts(build_dir, contract, "qemu_smp_production")
    (build_dir / "kernel" / "kernel.elf").write_bytes(_minimal_elf(rwx=True))
    _refresh_completed_build_stamp(build_dir, contract, "qemu_smp_production")

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_production",
        build_dir,
        require_artifacts=True,
    )

    assert evidence["valid"] is True, _errors(evidence)
    assert evidence["claim_eligibility"] == {
        "profile_configuration_for_release": True,
        "runtime": True,
        "artifact_set_shipping": False,
        "cohesix_system_image": False,
    }
    exceptions = evidence["artifact_policy"]["exceptions"]
    assert any(item["artifact"] == "kernel" for item in exceptions)
    assert all(item["id"] == "upstream-sel4test-rwx-load" for item in exceptions)


def test_shipping_artifact_policy_rejects_rwx_load_segments(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    local_contract = copy.deepcopy(contract)
    profile = local_contract["profiles"]["qemu_smp_production"]
    profile["artifact_policy"]["class"] = "cohesix-shipping-image"
    profile["artifact_policy"]["shipping_eligible"] = True
    profile["artifact_policy"]["cohesix_system_image"] = True
    profile["artifact_policy"]["rwx_policy"] = "reject"
    build_dir = _write_build_tree(tmp_path, local_contract, "qemu_smp_production")
    _write_required_artifacts(build_dir, local_contract, "qemu_smp_production")
    (build_dir / "kernel" / "kernel.elf").write_bytes(_minimal_elf(rwx=True))
    _refresh_completed_build_stamp(
        build_dir,
        local_contract,
        "qemu_smp_production",
    )

    evidence = sel4_profile.validate_build(
        local_contract,
        "qemu_smp_production",
        build_dir,
        require_artifacts=True,
    )

    assert evidence["valid"] is False
    assert "ELF artifact kernel contains RWX LOAD segments" in _errors(evidence)


@pytest.mark.parametrize(
    ("mutation", "expected_error"),
    (
        ("elf32", "little-endian ELF64"),
        ("big-endian", "little-endian ELF64"),
        ("wrong-machine", "not an AArch64 ELF"),
        ("et-dyn", "not an ET_EXEC ELF"),
        ("zero-entry", "zero ELF entry point"),
        ("no-program-headers", "no ELF program headers"),
        ("truncated-program-headers", "truncated ELF program headers"),
        ("no-load", "no readable PT_LOAD"),
        ("unreadable", "is not readable"),
        ("invalid-sizes", "invalid file/memory sizes"),
        ("outside-file", "extends beyond the file"),
        ("bad-alignment", "non-power-of-two alignment"),
        ("incongruent-alignment", "incongruent alignment"),
        ("unknown-flags", "unknown flags"),
        ("no-executable-load", "no readable executable PT_LOAD"),
        ("entry-outside-executable", "outside executable PT_LOAD"),
    ),
)
def test_elf_inspection_rejects_malformed_aarch64_images(
    tmp_path: Path,
    mutation: str,
    expected_error: str,
) -> None:
    data = bytearray(_minimal_elf())
    if mutation == "elf32":
        data[4] = 1
    elif mutation == "big-endian":
        data[5] = 2
    elif mutation == "wrong-machine":
        struct.pack_into("<H", data, 18, 62)
    elif mutation == "et-dyn":
        struct.pack_into("<H", data, 16, 3)
    elif mutation == "zero-entry":
        struct.pack_into("<Q", data, 24, 0)
    elif mutation == "no-program-headers":
        struct.pack_into("<H", data, 56, 0)
    elif mutation == "truncated-program-headers":
        del data[100:]
    elif mutation == "no-load":
        struct.pack_into("<I", data, 64, 0)
    elif mutation == "unreadable":
        struct.pack_into("<I", data, 68, 1)
    elif mutation == "invalid-sizes":
        struct.pack_into("<Q", data, 96, 121)
    elif mutation == "outside-file":
        struct.pack_into("<Q", data, 96, 121)
        struct.pack_into("<Q", data, 104, 121)
    elif mutation == "bad-alignment":
        struct.pack_into("<Q", data, 112, 3)
    elif mutation == "incongruent-alignment":
        struct.pack_into("<Q", data, 80, 0x400001)
    elif mutation == "unknown-flags":
        struct.pack_into("<I", data, 68, 0xC)
    elif mutation == "no-executable-load":
        struct.pack_into("<I", data, 68, 0x4)
    elif mutation == "entry-outside-executable":
        struct.pack_into("<Q", data, 24, 0x500000)
    else:  # pragma: no cover - the parameter table is closed above.
        raise AssertionError(f"unknown mutation: {mutation}")
    image = tmp_path / f"{mutation}.elf"
    image.write_bytes(data)

    with pytest.raises(sel4_profile.ProfileError, match=expected_error):
        sel4_profile.inspect_elf_load_segments(image)


def test_each_qemu_dtb_must_independently_match_gic_and_psci(
    tmp_path: Path,
    contract: dict[str, Any],
) -> None:
    profile = contract["profiles"]["qemu_smp_diagnostic"]
    build_dir = _write_build_tree(tmp_path, contract, "qemu_smp_diagnostic")
    _write_required_artifacts(build_dir, contract, "qemu_smp_diagnostic")
    bad_profile = copy.deepcopy(profile)
    bad_profile["dtb"]["required_compatible"] = ["arm,cortex-a15-gic"]
    bad_profile["dtb"]["required_string_properties"] = []
    (build_dir / "qemu-arm-virt.dtb").write_bytes(_minimal_dtb(bad_profile))

    evidence = sel4_profile.validate_build(
        contract,
        "qemu_smp_diagnostic",
        build_dir,
        require_artifacts=True,
    )

    assert evidence["valid"] is False
    errors = _errors(evidence)
    assert "DTB qemu_dtb lacks compatible value 'arm,gic-v3'" in errors
    assert "DTB qemu_dtb lacks required string property method='smc'" in errors
    assert "DTB kernel_dtb lacks compatible" not in errors


def test_pi_overlay_diff_digest_is_independent_of_git_core_abbrev(
    tmp_path: Path,
    contract: dict[str, Any],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    local_contract, source_root, _overlay, fake_root = _overlay_source_fixture(
        tmp_path,
        contract,
    )
    monkeypatch.setattr(sel4_profile, "ROOT", fake_root)
    preparation = sel4_profile.prepare_source(
        local_contract,
        "pi4-diagnostic",
        source_root,
        dry_run=False,
    )
    assert preparation["action"] == "applied"
    profile = local_contract["profiles"]["pi4_diagnostic"]
    kernel = source_root / "kernel"

    observed_hashes = []
    for abbreviation in ("5", "40"):
        subprocess.run(
            ("git", "-C", str(kernel), "config", "core.abbrev", abbreviation),
            check=True,
        )
        evidence = sel4_profile.validate_source(local_contract, profile, source_root)
        assert evidence["errors"] == []
        observed_hashes.append(
            evidence["repositories"]["kernel"]["overlay_diff_sha256"]
        )
    assert observed_hashes == [local_contract["source"]["pi4_overlay"]["diff_sha256"]] * 2


def test_prepare_source_is_dry_run_safe_and_idempotent(
    tmp_path: Path,
    contract: dict[str, Any],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    local_contract, source_root, overlay, fake_root = _overlay_source_fixture(
        tmp_path,
        contract,
    )
    monkeypatch.setattr(sel4_profile, "ROOT", fake_root)
    baseline = overlay.read_bytes()

    preview = sel4_profile.prepare_source(
        local_contract,
        "pi4-diagnostic",
        source_root,
        dry_run=True,
    )
    assert preview["action"] == "would-apply"
    assert overlay.read_bytes() == baseline

    applied = sel4_profile.prepare_source(
        local_contract,
        "pi4-diagnostic",
        source_root,
        dry_run=False,
    )
    assert applied["action"] == "applied"
    prepared_bytes = overlay.read_bytes()
    assert prepared_bytes != baseline

    repeated = sel4_profile.prepare_source(
        local_contract,
        "pi4-diagnostic",
        source_root,
        dry_run=False,
    )
    assert repeated["action"] == "already-applied"
    assert overlay.read_bytes() == prepared_bytes


def test_prepare_source_rejects_foreign_dirt_without_mutation(
    tmp_path: Path,
    contract: dict[str, Any],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    local_contract, source_root, overlay, fake_root = _overlay_source_fixture(
        tmp_path,
        contract,
    )
    monkeypatch.setattr(sel4_profile, "ROOT", fake_root)
    baseline = overlay.read_bytes()
    (source_root / "kernel" / "foreign.txt").write_text(
        "foreign\n",
        encoding="utf-8",
    )

    with pytest.raises(sel4_profile.ProfileError, match="pristine pinned checkout"):
        sel4_profile.prepare_source(
            local_contract,
            "pi4-diagnostic",
            source_root,
            dry_run=False,
        )
    assert overlay.read_bytes() == baseline


def test_overlay_patch_digest_and_path_are_fail_closed(
    tmp_path: Path,
    contract: dict[str, Any],
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    local_contract, _source_root, _overlay, fake_root = _overlay_source_fixture(
        tmp_path,
        contract,
    )
    monkeypatch.setattr(sel4_profile, "ROOT", fake_root)

    local_contract["source"]["pi4_overlay"]["diff_sha256"] = "0" * 64
    with pytest.raises(sel4_profile.ProfileError, match="patch digest mismatch"):
        sel4_profile.pi4_overlay_patch(local_contract)

    local_contract["source"]["pi4_overlay"]["patch_file"] = "../outside.patch"
    with pytest.raises(sel4_profile.ProfileError, match="escapes"):
        sel4_profile.pi4_overlay_patch(local_contract)


def test_real_overlay_patch_bytes_match_recorded_digest() -> None:
    real_contract = sel4_profile.load_contract()
    overlay, patch, patch_bytes = sel4_profile.pi4_overlay_patch(real_contract)

    assert patch == (
        sel4_profile.ROOT
        / "configs"
        / "sel4"
        / "patches"
        / "bcm2711-vl805-device-untyped.patch"
    ).resolve()
    assert sel4_profile.sha256_bytes(patch_bytes) == overlay["diff_sha256"]
    assert patch_bytes.startswith(b"diff --git ")
    assert patch_bytes.count(b"\ndiff --git ") == 0
    assert b"+ * Author: Lukas Bower" in patch_bytes
    assert b"+ * Purpose:" in patch_bytes
    assert b"+ * Copyright 2026 Lukas Bower" in patch_bytes


def test_prepare_source_cli_is_explicit() -> None:
    arguments = sel4_profile.parse_arguments(
        (
            "prepare-source",
            "--profile",
            "pi4-diagnostic",
            "--source",
            "/tmp/sel4-source",
            "--dry-run",
        )
    )

    assert arguments.command == "prepare-source"
    assert arguments.profile == "pi4-diagnostic"
    assert arguments.source == Path("/tmp/sel4-source")
    assert arguments.dry_run is True


def test_real_contract_defaults_are_unique_canonical_out_paths() -> None:
    real_contract = sel4_profile.load_contract()
    defaults = [
        profile["default_build_dir"]
        for profile in real_contract["profiles"].values()
    ]

    assert len(defaults) == len(set(defaults))
    assert all(path.startswith("out/sel4/") for path in defaults)


def test_active_qemu_entrypoints_default_to_production_contract() -> None:
    canonical = "out/sel4/profile-v2/qemu-smp-production"
    entrypoints = (
        "scripts/cohesix-build-run.sh",
        "scripts/release_bundle.sh",
        "scripts/qemu-run.sh",
        "scripts/tcp_cohsh_smoke.sh",
        "scripts/tcp_repro.sh",
        "scripts/cohsh/run_regression_batch.sh",
        "configs/test_plan_actions.toml",
    )

    for relative in entrypoints:
        source = (sel4_profile.ROOT / relative).read_text(encoding="utf-8")
        assert canonical in source, relative
        assert "seL4/SMP_build" not in source, relative

    build_run = (sel4_profile.ROOT / entrypoints[0]).read_text(encoding="utf-8")
    assert 'SEL4_BUILD_DIR="${SEL4_BUILD_DIR:-${SEL4_BUILD:-' in build_run
    assert "validate_selected_qemu_profile" in build_run
    assert "--for-runtime" in build_run
    assert '[[ "$GIC_VER" == "3" ]]' in build_run
    assert "validate_gicv3_override_safety" in build_run
    assert "must not override the profile-owned virt,gic-version=3 machine" in build_run
    assert 'virt,gic-version=${GIC_VER}' in build_run

    release = (sel4_profile.ROOT / entrypoints[1]).read_text(encoding="utf-8")
    assert "validate_release_sel4_profile" in release
    assert "--profile qemu_smp_production" in release
    assert "--for-release" in release

    for relative in entrypoints[5:]:
        source = (sel4_profile.ROOT / relative).read_text(encoding="utf-8")
        assert "qemu_smp_production" in source, relative


def test_build_run_rejects_every_machine_or_gic_override() -> None:
    script = sel4_profile.ROOT / "scripts/cohesix-build-run.sh"
    prelude = (
        'source "$1"; QEMU_MACHINE_EXTRA=""; COHSH_QEMU_ARGS=""; '
        "EXTRA_QEMU_ARGS=(); "
    )

    accepted = subprocess.run(
        ["bash", "-c", prelude + "validate_gicv3_override_safety", "bash", str(script)],
        cwd=sel4_profile.ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert accepted.returncode == 0, accepted.stderr

    whitespace_cohsh_args = subprocess.run(
        [
            "bash",
            "-c",
            prelude
            + 'COHSH_QEMU_ARGS="   "; '
            + "validate_gicv3_override_safety",
            "bash",
            str(script),
        ],
        cwd=sel4_profile.ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert whitespace_cohsh_args.returncode == 0, whitespace_cohsh_args.stderr

    rejected_setups = (
        'QEMU_MACHINE_EXTRA="gic-version=2"; ',
        'QEMU_MACHINE_EXTRA="type=virt"; ',
        'EXTRA_QEMU_ARGS=(-machine virt); ',
        'EXTRA_QEMU_ARGS=(--machine virt); ',
        'EXTRA_QEMU_ARGS=(-Mvirt); ',
        'EXTRA_QEMU_ARGS=("gic-version=2"); ',
        'COHSH_QEMU_ARGS="-M virt"; ',
        'COHSH_QEMU_ARGS="-machine virt,gic-version=2"; ',
        'COHSH_QEMU_ARGS="--machine virt,gic-version=2"; ',
    )
    for setup in rejected_setups:
        rejected = subprocess.run(
            [
                "bash",
                "-c",
                prelude + setup + "validate_gicv3_override_safety",
                "bash",
                str(script),
            ],
            cwd=sel4_profile.ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
        assert rejected.returncode != 0, setup
        assert "profile-owned" in rejected.stderr, rejected.stderr


def test_launch_existing_rejects_qemu_artifact_and_topology_overrides() -> None:
    script = sel4_profile.ROOT / "scripts/cohesix-build-run.sh"
    prelude = (
        'source "$1"; QEMU_MACHINE_EXTRA=""; COHSH_QEMU_ARGS=""; '
        "EXTRA_QEMU_ARGS=(); LAUNCH_EXISTING=1; "
    )

    accepted = subprocess.run(
        [
            "bash",
            "-c",
            prelude
            + 'EXTRA_QEMU_ARGS=(-S -gdb tcp:127.0.0.1:1234); '
            + "validate_gicv3_override_safety",
            "bash",
            str(script),
        ],
        cwd=sel4_profile.ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    assert accepted.returncode == 0, accepted.stderr

    rejected_setups = (
        'EXTRA_QEMU_ARGS=(--kernel other.elf); ',
        'EXTRA_QEMU_ARGS=(--initrd=other.cpio); ',
        'EXTRA_QEMU_ARGS=(--smp 1); ',
        'EXTRA_QEMU_ARGS=(--cpu max); ',
        'EXTRA_QEMU_ARGS=(--device loader,file=other.elf); ',
        'EXTRA_QEMU_ARGS=(--device loader,addr=0x80000000,data=0); ',
        'COHSH_QEMU_ARGS="--kernel other.elf"; ',
        'COHSH_QEMU_ARGS="--initrd=other.cpio"; ',
        'COHSH_QEMU_ARGS="--smp 1"; ',
        'COHSH_QEMU_ARGS="--device loader,file=other.elf"; ',
        'COHSH_QEMU_ARGS="--device loader,addr=0x80000000,data=0"; ',
    )
    for setup in rejected_setups:
        rejected = subprocess.run(
            [
                "bash",
                "-c",
                prelude + setup + "validate_gicv3_override_safety",
                "bash",
                str(script),
            ],
            cwd=sel4_profile.ROOT,
            text=True,
            capture_output=True,
            check=False,
        )
        assert rejected.returncode != 0, setup
        assert "immutable QEMU inputs or topology" in rejected.stderr


def test_build_run_regenerates_complete_compiler_owned_surface() -> None:
    source = (
        sel4_profile.ROOT / "scripts/cohesix-build-run.sh"
    ).read_text(encoding="utf-8")

    for option in (
        "--implementation-surface-inventory",
        "--host-integration-graph",
        "--host-integration-doc",
        "--cohesix-py-defaults",
        "--cohsh-client-rust",
        "--coh-policy-rust",
        "--swarmui-defaults-rust",
    ):
        assert option in source
    assert source.count("--bin coh-rtc-python-profile") == 2
    assert "cohesix_python_qemu_smp_production.json" in source
    assert "cohesix_python_pi4_production.json" in source
    assert "Selected-profile host tool is missing after its source build" in source
    assert "Host tool not built" not in source
    assert 'ROOT_TASK_FEATURES="release-qemu,bootstrap-trace"' in source
    assert 'ROOT_TASK_FEATURES="cohesix-dev"' not in source
    assert 'has_root_task_feature "release-qemu"' in source
    assert 'NET_BACKEND="virtio"' in source
