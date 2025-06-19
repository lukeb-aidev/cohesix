# CLASSIFICATION: COMMUNITY
# Filename: __init__.py v0.2
# Author: Lukas Bower
# Date Modified: 2025-07-15

"""Agent SDK providing runtime context helpers."""

__version__ = "0.1.0"

import json
import os

BASE = os.environ.get("COH_BASE", "")


class AgentContext:
    """Access runtime metadata and world snapshot."""

    def __init__(self):
        self.role = self._read_text(f"{BASE}/srv/agent_meta/role.txt") or "Unknown"
        self.uptime = self._read_text(f"{BASE}/srv/agent_meta/uptime.txt") or "0"
        self.last_goal = self._read_json(f"{BASE}/srv/agent_meta/last_goal.json")
        self.world_snapshot = self._read_json(f"{BASE}/srv/world_state/world.json")

    def _read_text(self, path: str):
        """Return the text at *path* or ``None`` if unreadable."""
        try:
            return open(path).read().strip()
        except OSError:
            return None

    def _read_json(self, path: str):
        """Return parsed JSON from *path* or an empty dict on failure."""
        try:
            return json.loads(open(path).read())
        except OSError:
            return {}
        except json.JSONDecodeError:
            return {}


__all__ = ["AgentContext"]
