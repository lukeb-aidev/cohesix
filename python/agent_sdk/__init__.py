# CLASSIFICATION: COMMUNITY
# Filename: __init__.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-11

"""Agent SDK providing runtime context helpers."""

import json
import os

BASE = os.environ.get("COH_BASE", "")

class AgentContext:
    """Access runtime metadata and world snapshot."""

    def __init__(self):
        self.role = self._read_text(f"{BASE}/srv/agent_meta/role.txt") or 'Unknown'
        self.uptime = self._read_text(f"{BASE}/srv/agent_meta/uptime.txt") or '0'
        self.last_goal = self._read_json(f"{BASE}/srv/agent_meta/last_goal.json")
        self.world_snapshot = self._read_json(f"{BASE}/srv/world_state/world.json")

    def _read_text(self, path: str):
        try:
            return open(path).read().strip()
        except OSError:
            return None

    def _read_json(self, path: str):
        try:
            return json.loads(open(path).read())
        except OSError:
            return {}
        except json.JSONDecodeError:
            return {}

__all__ = ['AgentContext']
