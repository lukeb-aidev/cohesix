"""Bridge NeMo 1.9.0's authenticated-card and data-part configuration gap.

Author: Lukas Bower
Purpose: Register one per-user native A2A function group with scoped card headers.
Copyright 2026 Lukas Bower
"""

from __future__ import annotations

from collections.abc import AsyncGenerator
from datetime import timedelta

from pydantic import Field

from nat.builder.context import Context
from nat.builder.workflow_builder import Builder
from nat.cli.register_workflow import register_per_user_function_group
from nat.plugins.a2a.client.client_config import A2AClientConfig
from nat.plugins.a2a.client.client_impl import A2AClientFunctionGroup

from cohesix_nemo_kit.cli import headers
from cohesix_nemo_kit.native import CohesixA2AClient


class CohesixA2AClientConfig(A2AClientConfig, name="cohesix_a2a_client"):
    """Require one credential pair for each isolated per-user process."""

    request_auth_ref: str = Field(..., description="Private file: or env: request credential")
    delegated_ticket_ref: str = Field(..., description="Private file: or env: delegated ticket")


class CohesixA2AFunctionGroup(A2AClientFunctionGroup):
    """Reuse NeMo's per-user tools after its native client authenticates the card."""

    async def __aenter__(self) -> "CohesixA2AFunctionGroup":
        config: CohesixA2AClientConfig = self._config
        if not Context.get().user_id:
            raise ValueError("NeMo per-user context is required")
        if config.auth_provider:
            raise ValueError("Cohesix uses the selected gateway credential pair")
        self._client = CohesixA2AClient(
            str(config.url), headers({"request_auth_ref": config.request_auth_ref,
                                      "delegated_ticket_ref": config.delegated_ticket_ref}),
            agent_card_path=config.agent_card_path,
            task_timeout=timedelta(seconds=min(config.task_timeout.total_seconds(), 60)),
            streaming=False,
        )
        await self._client.__aenter__()
        self._register_functions()
        return self

    async def __aexit__(self, exc_type: object, exc: object, traceback: object) -> None:
        if self._client is not None:
            await self._client.__aexit__(exc_type, exc, traceback)


@register_per_user_function_group(config_type=CohesixA2AClientConfig)
async def cohesix_a2a_client_function_group(
    config: CohesixA2AClientConfig, builder: Builder,
) -> AsyncGenerator[CohesixA2AFunctionGroup, None]:
    """Yield the native NeMo A2A helper set for one verified subject process."""
    async with CohesixA2AFunctionGroup(config, builder) as group:
        yield group
