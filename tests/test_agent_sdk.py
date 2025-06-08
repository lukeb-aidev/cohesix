# CLASSIFICATION: COMMUNITY
# Filename: test_agent_sdk.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-11

import os
import json
import sys
sys.path.insert(0, str((os.path.dirname(__file__)+'/../python').replace('\\','/')))


def test_agent_context_defaults(tmp_path, monkeypatch):
    base = tmp_path
    os.makedirs(base / "srv/agent_meta")
    os.makedirs(base / "srv/world_state")
    monkeypatch.chdir(base)
    monkeypatch.setenv("COH_BASE", str(base))

    from agent_sdk import AgentContext

    open(base / "srv/agent_meta/role.txt", "w").write("DroneWorker")
    open(base / "srv/agent_meta/uptime.txt", "w").write("5")
    open(base / "srv/agent_meta/last_goal.json", "w").write("{}")
    open(base / "srv/world_state/world.json", "w").write("{}")

    ctx = AgentContext()
    assert ctx.role == "DroneWorker"
    assert ctx.uptime == "5"
    assert isinstance(ctx.last_goal, dict)
    assert isinstance(ctx.world_snapshot, dict)
