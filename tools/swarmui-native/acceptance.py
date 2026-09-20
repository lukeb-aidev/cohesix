#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Bind real native SwarmUI interaction evidence to exact host and QEMU artifacts without a simulated bridge.
# Copyright 2026 Lukas Bower
"""Launch or verify the native acceptance lane; drive the visible app using OS accessibility."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


def identity(path: Path) -> dict[str, str]:
    """Identify an immutable local artifact by its actual bytes."""
    resolved = path.resolve(strict=True)
    digest = hashlib.sha256()
    with resolved.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return {"path": str(resolved), "sha256": digest.hexdigest()}


def check_log(path: Path) -> dict[str, bool]:
    """Assert command-boundary observations; fixture logs cannot satisfy native acceptance."""
    if path.stat().st_size > 16 * 1024 * 1024:
        raise ValueError("native log exceeds bound")
    rows = [json.loads(line) for line in path.read_text().splitlines()]
    if not rows or len(rows) > 1024:
        raise ValueError("native observation count")
    if any(row.get("lane") != "native_bridge" or row.get("sequence") != index for index, row in enumerate(rows)):
        raise ValueError("native lane or observation sequence")
    def found(action: str, predicate=lambda value: True) -> bool:
        return any(row["action"] == action and predicate(row["observation"]) for row in rows)
    return {
        "real_bridge": found("startup", lambda o: o.get("fixture") is False and o.get("bridge") == "tauri" and len(o.get("desktop_source_sha256", "")) == 64),
        "secret_refusal": found("connection_input_refused"),
        "authentication_refusal": found("connect", lambda o: o.get("ok") is False),
        "gateway_connect": found("connect", lambda o: o.get("ok") is True and o.get("transport") == "rest"),
        "canonical_shards": found("namespace", lambda o: o.get("ok") is True and o.get("verb") == "ls" and o.get("path") == "/shard"),
        "target_observation": found("namespace", lambda o: o.get("ok") is True and o.get("verb") == "cat" and o.get("path") == "/proc/boot"),
        "live_hive": found("hive_bootstrap", lambda o: o.get("replay") is False) and found("hive_poll"),
        "host_tool": found("host", lambda o: o.get("success") is True),
        "reconnect": sum(row["action"] == "connect" and row["observation"].get("ok") is True for row in rows) >= 2 and found("disconnect"),
        "verified_reference": found("reference", lambda o: o.get("name") == "lora" and o.get("mode") == "REPLAY" and o.get("offline") is True and o.get("outcome") == "succeeded"),
        "recovered_failure": found("reference", lambda o: o.get("name") == "recovery" and o.get("outcome") == "failed"),
        "offline_write_refusal": found("control_refused", lambda o: o.get("reason") == "offline"),
        "clean_shutdown": rows[-1]["action"] == "shutdown" and rows[-1]["observation"].get("host_busy") is False,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=["qemu"], required=True)
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--release-dir", type=Path)
    parser.add_argument("--app", type=Path)
    parser.add_argument("--coh", type=Path)
    parser.add_argument("--gateway", type=Path)
    parser.add_argument("--registry", type=Path)
    parser.add_argument("--qemu-artifact", type=Path)
    parser.add_argument("--source-record", type=Path)
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    state = args.state_dir.absolute()
    if args.verify:
        provenance = json.loads((state / "provenance.json").read_text())
        checks = check_log(state / "native.ndjson")
        checks["process_exit"] = json.loads((state / "process.json").read_text())["exit_code"] == 0
        # Evidence must still name the same bytes, including the package's own app and tools.
        for name, artifact in provenance["artifacts"].items():
            checks[f"identity:{name}"] = identity(Path(artifact["path"]))["sha256"] == artifact["sha256"]
        screenshots = sorted(p for p in state.iterdir() if p.suffix.lower() in {".png", ".jpg", ".jpeg"})
        checks["native_screenshot"] = bool(screenshots)
        report = {"lane": "native_live", "target": "qemu", "verdict": "PASS" if all(checks.values()) else "FAIL", "checks": checks, "log": identity(state / "native.ndjson"), "screenshots": [identity(p) for p in screenshots], "claim": "focused native desktop integration; not target or release qualification"}
        with (state / "result.json").open("x") as output:
            json.dump(report, output, indent=2)
            output.write("\n")
        print(json.dumps(report, indent=2))
        return 0 if all(checks.values()) else 1
    package = args.release_dir.absolute() if args.release_dir else None
    app = args.app or (package / "bin/swarmui" if package else None)
    coh = args.coh or (package / "bin/coh" if package else None)
    gateway = args.gateway or (package / "bin/hive-gateway" if package else None)
    if not all([app, coh, gateway, args.registry, args.qemu_artifact, args.source_record]):
        parser.error("launch requires exact app, coh, gateway, registry, QEMU image and source record")
    state.mkdir(mode=0o700, parents=True, exist_ok=False)
    artifacts = {name: identity(path) for name, path in {"app": app, "coh": coh, "gateway": gateway, "registry": args.registry, "qemu": args.qemu_artifact, "source": args.source_record}.items()}
    provenance = {"lane": "native_live", "target": args.target, "release_dir": str(package) if package else None, "artifacts": artifacts}
    (state / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    env = dict(os.environ, SWARMUI_ACCEPTANCE_LOG=str(state / "native.ndjson"), SWARMUI_TOOL_DIR=str(coh.absolute().parent))
    print(f"Native SwarmUI started. Follow docs/SWARMUI.md acceptance walkthrough. Evidence: {state}", flush=True)
    with (state / "application.log").open("xb") as log:
        child = subprocess.Popen([str(app.absolute())], env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            code = child.wait()
        except (KeyboardInterrupt, SystemExit):
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
            raise
    (state / "process.json").write_text(json.dumps({"exit_code": code}) + "\n")
    return code


if __name__ == "__main__":
    sys.exit(main())
