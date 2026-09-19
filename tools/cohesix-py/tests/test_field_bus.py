# Author: Lukas Bower
# Purpose: Preserve SDK parity with compiled field-bus map selection and refuse arbitrary control payloads before ticket submission.
# Copyright 2026 Lukas Bower

"""Deterministic client contract vectors; no protocol or admission claim."""

from copy import deepcopy

import pytest

from cohesix import providers
from cohesix.authority import validate_provider_v1
from cohesix.errors import CohesixError
from cohesix.orchestration import HostTicketRequest


def test_field_bus_requests_bind_exact_compiled_map_and_native_target(
    monkeypatch,
) -> None:
    selected = deepcopy(providers.REGISTRY)
    selected["contract"]["field_bus"] = [
        {
            "id": "plc",
            "protocol": "modbus",
            "points": [
                {
                    "id": "temperature",
                    "approval_required": False,
                    "operation": {
                        "kind": "modbus_read",
                        "function": 3,
                        "start": 0,
                        "count": 1,
                    },
                },
                {
                    "id": "setpoint",
                    "approval_required": True,
                    "operation": {
                        "kind": "modbus_write",
                        "function": 6,
                        "address": 3,
                        "value": 42,
                    },
                },
            ],
        }
    ]
    monkeypatch.setattr(providers, "REGISTRY", selected)
    args = {"endpoint": "plc", "point": "temperature"}
    request = HostTicketRequest(
        ticket_id="one",
        idempotency_key="once",
        action="modbus.read",
        target="plc",
        args=args,
    )
    assert request.to_payload()["target"] == "plc"
    validate_provider_v1(
        "modbus.control", "plc", {"endpoint": "plc", "point": "setpoint"}
    )
    for action, target, values in [
        ("modbus.control", "plc", args),
        ("dnp3.read", "plc", args),
        ("modbus.read", "/bus/modbus/plc", args),
        ("modbus.read", "other", args),
        ("modbus.read", "plc", {**args, "function": 3}),
        ("modbus.read", "plc", {"endpoint": "plc", "point": "undeclared"}),
    ]:
        with pytest.raises(CohesixError):
            validate_provider_v1(action, target, values)
    with pytest.raises(CohesixError):
        HostTicketRequest(
            ticket_id="one",
            idempotency_key="once",
            action="modbus.read",
            target="plc ",
            args=args,
        )
