# Author: Lukas Bower
# Purpose: Reject cross-container events and retain only the Engine's canonical immutable identity fields.
# Copyright 2026 Lukas Bower

import importlib.util
import json
from pathlib import Path

import pytest

SOURCE = Path(__file__).resolve().parents[1] / "scripts/ci/provider_execution_lane.py"
SPEC = importlib.util.spec_from_file_location("provider_execution_lane", SOURCE)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def test_engine_actor_identity_is_required_and_attributes_are_not_exported():
    identity = "a" * 64
    record = {"Type": "container", "Actor": {"ID": identity, "Attributes": {"secret": "withheld"}},
              "Action": "die", "timeNano": 123}
    assert MODULE.docker_events(json.dumps(record).encode(), identity) == [
        {"action": "die", "container_id": identity, "time_nano": 123}]
    record["Actor"]["ID"] = "b" * 64
    with pytest.raises(ValueError, match="identity mismatch"):
        MODULE.docker_events(json.dumps(record).encode(), identity)
