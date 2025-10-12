#!/usr/bin/env python3
# Author: Lukas Bower
"""Infer the GIC version used by an seL4 build configuration."""

import argparse
import pathlib
import re
import sys
from typing import Dict, Optional

TRUTHY_VALUES = {"1", "y", "Y", "true", "TRUE", "on", "ON", "yes", "YES"}
FALSY_VALUES = {"0", "n", "N", "false", "FALSE", "off", "OFF", "no", "NO"}


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Detect the ARM GIC version from an seL4 config header",
    )
    parser.add_argument(
        "config",
        type=pathlib.Path,
        help="Path to an seL4 configuration header",
    )
    return parser.parse_args()


def load_symbols(config_path: pathlib.Path) -> Dict[str, str]:
    try:
        raw_text = config_path.read_text(encoding="utf-8", errors="ignore")
    except FileNotFoundError as exc:  # pragma: no cover - handled by caller
        raise FileNotFoundError(str(exc))

    symbols: Dict[str, str] = {}
    for raw_line in raw_text.splitlines():
        line = raw_line.strip()
        if not line:
            continue

        if line.startswith("/*") and line.endswith("*/") and "disabled:" in line:
            inner = line[2:-2].strip()
            prefix = "disabled:"
            if inner.startswith(prefix):
                remainder = inner[len(prefix) :].strip()
                symbol = remainder.split()[0]
                symbols.setdefault(symbol, "0")
            continue

        if line.startswith("#define"):
            parts = line.split(None, 2)
            if len(parts) >= 3:
                symbol = parts[1]
                value = parts[2].split("/*", 1)[0].strip()
                symbols[symbol] = value
            continue

        if line.startswith("set("):
            match = re.match(r"set\(\s*([A-Za-z0-9_]+)\s+([^)]+)\)", line)
            if match:
                symbol, value = match.groups()
                symbols[symbol] = value.strip()
            continue

        if "=" in line:
            lhs, rhs = line.split("=", 1)
            symbol = lhs.strip()
            value = rhs.split("/*", 1)[0].split("#", 1)[0].strip()
            if symbol:
                symbols[symbol] = value

    return symbols


def normalise(value: Optional[str]) -> Optional[str]:
    if value is None:
        return None
    stripped = value.strip()
    if stripped.startswith('"') and stripped.endswith('"') and len(stripped) >= 2:
        stripped = stripped[1:-1]
    stripped = stripped.rstrip("uUlL")
    return stripped


def symbol_state(symbols: Dict[str, str], symbol: str) -> int:
    value = normalise(symbols.get(symbol))
    if value is None:
        return 0
    if value in TRUTHY_VALUES:
        return 1
    if value in FALSY_VALUES:
        return -1
    return 0


def detect_gic_version(symbols: Dict[str, str]) -> Optional[int]:
    gic3_symbols = (
        "CONFIG_ARM_GIC_V3",
        "CONFIG_ARM_GIC_V3_SUPPORT",
        "CONFIG_KERNEL_ARM_GIC_V3",
        "CONFIG_KernelArmGicV3",
        "KernelArmGicV3",
    )
    gic2_symbols = (
        "CONFIG_ARM_GIC_V2",
        "CONFIG_ARM_GIC_V2_ONLY",
        "CONFIG_KERNEL_ARM_GIC_V2",
        "CONFIG_KernelArmGicV2",
        "KernelArmGicV2",
    )

    explicit_v3_disabled = False

    for symbol in gic3_symbols:
        state = symbol_state(symbols, symbol)
        if state == 1:
            return 3
        if state == -1:
            explicit_v3_disabled = True

    for symbol in gic2_symbols:
        if symbol_state(symbols, symbol) == 1:
            return 2

    if explicit_v3_disabled:
        return 2

    return None


def main() -> int:
    args = parse_arguments()
    try:
        symbols = load_symbols(args.config)
    except FileNotFoundError:
        print(f"Config file not found: {args.config}", file=sys.stderr)
        return 2

    version = detect_gic_version(symbols)
    if version is None:
        return 1

    print(version)
    return 0


if __name__ == "__main__":
    sys.exit(main())
