#!/usr/bin/env python3
# Author: Lukas Bower
# Purpose: Publish a bounded deterministic sequence of unaltered screenshots from a passing native acceptance record.
# Copyright 2026 Lukas Bower
"""Create a portable showcase capture without invented topology or rewritten pixels."""
from __future__ import annotations

import argparse
import base64
import hashlib
import html
import json
from pathlib import Path


def capture(result: Path, output: Path) -> None:
    """Verify every source screenshot against the completed native lane before embedding it."""
    report = json.loads(result.read_text())
    if report.get("verdict") != "PASS" or report.get("lane") != "native_live":
        raise ValueError("capture requires passing native acceptance")
    records = report.get("screenshots", [])
    if not 2 <= len(records) <= 24:
        raise ValueError("capture requires 2..24 native screenshots")
    frames = []
    for record in records:
        path = Path(record["path"])
        if path.stat().st_size > 16 * 1024 * 1024:
            raise ValueError("screenshot exceeds bound")
        data = path.read_bytes()
        mime = "image/png" if data.startswith(b"\x89PNG\r\n\x1a\n") else "image/jpeg" if data.startswith(b"\xff\xd8\xff") else None
        if mime is None or hashlib.sha256(data).hexdigest() != record["sha256"]:
            raise ValueError("native screenshot identity mismatch")
        frames.append('<figure hidden><img alt="Native SwarmUI, ' + html.escape(path.stem) + '" src="data:' + mime + ';base64,' + base64.b64encode(data).decode() + '"><figcaption>' + html.escape(path.stem) + '</figcaption></figure>')
    document = '''<!doctype html><html lang="en"><meta charset="utf-8">
<!-- Author: Lukas Bower; Purpose: Replay unaltered native acceptance frames; Copyright 2026 Lukas Bower -->
<title>Cohesix · SwarmUI native walkthrough</title>
<style>body{margin:0;background:#111711;color:#ede7d8;font:16px system-ui}header{padding:18px 4vw;display:flex;gap:24px;align-items:center}p{color:#b6c2ad;font-size:13px}button{background:#dac18c;color:#151a13;padding:10px 18px;border:0;border-radius:6px}figure{margin:0 auto;max-width:1400px}img{width:100%;height:auto}figcaption{padding:12px 4vw;font-size:13px}[hidden]{display:none}</style>
<header><strong>COHESIX / SWARMUI</strong><button id="play">Play walkthrough</button><span id="position"></span></header>
<p style="padding:0 4vw">Recorded native desktop frames. Mode and proof labels remain in each original capture. Historical evidence does not establish current readiness.</p>
''' + "\n".join(frames) + '''
<script>const frames=[...document.querySelectorAll('figure')];let index=0,timer;function show(i){index=i;frames.forEach((f,n)=>f.hidden=n!==i);document.getElementById('position').textContent=`${i+1} / ${frames.length}`;}show(0);document.getElementById('play').onclick=()=>{clearInterval(timer);show(0);timer=setInterval(()=>{if(index+1>=frames.length){clearInterval(timer);return;}show(index+1);},2400);};document.addEventListener('visibilitychange',()=>{if(document.hidden)clearInterval(timer);});</script></html>'''
    with output.open("x") as file:
        file.write(document)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--result", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    capture(args.result, args.out)


if __name__ == "__main__":
    main()
