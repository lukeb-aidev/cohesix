"""Use NeMo's native protocol clients with Cohesix's scoped wire contract.

Author: Lukas Bower
Purpose: Supply protected card discovery and data-part submission absent from NeMo 1.9.0.
Copyright 2026 Lukas Bower
"""

from __future__ import annotations

import json
from collections.abc import AsyncGenerator
from typing import Any
from uuid import uuid4

from a2a.types import DataPart, Message, Part, Role
from nat.plugins.a2a.client.client_base import A2ABaseClient


class CohesixA2AClient(A2ABaseClient):
    """Keep NeMo's task helpers while authenticating card fetch and data jobs."""

    def __init__(self, base_url: str, headers: dict[str, str], **kwargs: Any) -> None:
        super().__init__(base_url, **kwargs)
        self._cohesix_headers = headers

    async def _resolve_agent_card(self) -> None:
        # NeMo 1.9.0 applies its auth interceptor only after card discovery.
        if self._httpx_client is None:
            raise RuntimeError("A2A HTTP client is not initialized")
        self._httpx_client.headers.update(self._cohesix_headers)
        await super()._resolve_agent_card()

    async def send_job(self, scope_id: str, ticket: dict[str, Any]) -> AsyncGenerator[Any, None]:
        """Submit the generated skill with its required single A2A data part."""
        if self._client is None:
            raise RuntimeError("A2A client is not initialized")
        data = {"skillId": ticket["action"], "scopeId": scope_id, "ticket": ticket}
        # The A2A SDK retains the original ticket ID as Cohesix's task ID.
        message = Message(
            role=Role.user,
            parts=[Part(root=DataPart(data=data))],
            message_id=uuid4().hex,
        )
        async for event in self._client.send_message(message):
            yield event

    async def send_message(self, message_text: str, task_id: str | None = None,
                           context_id: str | None = None) -> AsyncGenerator[Any, None]:
        """Accept only the exact JSON intent that maps to Cohesix's data part."""
        if task_id is not None or context_id is not None or len(message_text) > 8192:
            raise ValueError("Cohesix A2A continuation or oversized intent refused")
        data = json.loads(message_text)
        if (not isinstance(data, dict) or set(data) != {"skillId", "scopeId", "ticket"}
                or data["skillId"] not in ("gpu.workload.submit", "peft.release")
                or not isinstance(data["scopeId"], str)
                or not isinstance(data["ticket"], dict)
                or data["ticket"].get("action") != data["skillId"]):
            raise ValueError("Cohesix A2A requires exact selected data intent")
        async for event in self.send_job(data["scopeId"], data["ticket"]):
            yield event


def task_from_event(event: Any) -> dict[str, Any] | None:
    """Return a bounded task projection from NeMo's SDK event shape."""
    candidate = event[0] if isinstance(event, tuple) else event
    if getattr(candidate, "kind", None) == "task" or candidate.__class__.__name__ == "Task":
        return json.loads(candidate.model_dump_json(by_alias=True))
    return None
