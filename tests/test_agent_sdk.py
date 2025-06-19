# CLASSIFICATION: COMMUNITY
# Filename: test_agent_sdk.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-15

import os


def test_agent_context_defaults(tmp_path, monkeypatch):
    base = tmp_path
    os.makedirs(base / "srv/agent_meta")
    os.makedirs(base / "srv/world_state")
    monkeypatch.chdir(base)
    monkeypatch.setenv("COH_BASE", str(base))

    import importlib
    import agent_sdk

    importlib.reload(agent_sdk)
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


def test_role_lifecycle_boot(tmp_path, monkeypatch):
    base = tmp_path / "boot"
    os.makedirs(base / "srv/agent_meta")
    os.makedirs(base / "srv/world_state")
    monkeypatch.setenv("COH_BASE", str(base))

    import importlib
    import agent_sdk

    importlib.reload(agent_sdk)
    from agent_sdk import AgentContext

    open(base / "srv/agent_meta/role.txt", "w").write("QueenPrimary")
    open(base / "srv/agent_meta/uptime.txt", "w").write("0")
    open(base / "srv/agent_meta/last_goal.json", "w").write("{}")
    open(base / "srv/world_state/world.json", "w").write("{}")

    ctx = AgentContext()
    assert ctx.role == "QueenPrimary"


def test_simulated_migration(monkeypatch, tmp_path):
    base1 = tmp_path / "old"
    os.makedirs(base1 / "srv/agent_meta")
    os.makedirs(base1 / "srv/world_state")
    open(base1 / "srv/agent_meta/role.txt", "w").write("DroneWorker")
    open(base1 / "srv/agent_meta/uptime.txt", "w").write("1")
    open(base1 / "srv/agent_meta/last_goal.json", "w").write("{}")
    open(base1 / "srv/world_state/world.json", "w").write("{}")
    monkeypatch.setenv("COH_BASE", str(base1))

    import importlib
    import agent_sdk

    importlib.reload(agent_sdk)
    from agent_sdk import AgentContext

    ctx1 = AgentContext()
    assert ctx1.role == "DroneWorker"

    base2 = tmp_path / "new"
    os.makedirs(base2 / "srv/agent_meta")
    os.makedirs(base2 / "srv/world_state")
    open(base2 / "srv/agent_meta/role.txt", "w").write("SensorRelay")
    open(base2 / "srv/agent_meta/uptime.txt", "w").write("2")
    open(base2 / "srv/agent_meta/last_goal.json", "w").write("{}")
    open(base2 / "srv/world_state/world.json", "w").write("{}")
    monkeypatch.setenv("COH_BASE", str(base2))
    importlib.reload(agent_sdk)
    ctx2 = AgentContext()
    assert ctx2.role == "SensorRelay"
