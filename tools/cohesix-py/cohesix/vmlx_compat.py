# Author: Lukas Bower
# Purpose: Query one explicitly selected loopback vMLX model without granting it Cohesix job or tool authority.
# Copyright 2026 Lukas Bower
"""Bounded vMLX serving client. Cohesix admission and release verification stay upstream."""

from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import re
from typing import Any
import urllib.error
import urllib.parse
import urllib.request


MAX_RESPONSE = 65_536
MAX_PROMPT = 2_048
MODEL_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}\Z")


class VmlxRefusal(ValueError):
    """An endpoint, identity or bounded serving observation was refused."""


def require(condition: bool, reason: str) -> None:
    """Reject an ambiguous observation before returning content to a caller."""
    if not condition:
        raise VmlxRefusal(reason)


def _reject_nonfinite(_value: str) -> None:
    raise ValueError("nonfinite JSON value")


class _NoRedirects(urllib.request.HTTPRedirectHandler):
    """Keep prompts and an optional local server token on the selected origin."""

    def redirect_request(self, request: urllib.request.Request, fp: Any,
                         code: int, msg: str, headers: Any,
                         newurl: str) -> None:
        return None


@dataclass(frozen=True, repr=False)
class VmlxReply:
    """Private content and bounded observation from one real served generation."""

    text: str
    model: str
    finish_reason: str
    prompt_tokens: int
    completion_tokens: int
    prompt_sha256: str
    output_sha256: str

    def evidence(self) -> dict[str, str | int]:
        """Report identities and counts without disclosing prompt or output bytes."""
        return {
            "model": self.model,
            "finish_reason": self.finish_reason,
            "prompt_tokens": self.prompt_tokens,
            "completion_tokens": self.completion_tokens,
            "prompt_sha256": self.prompt_sha256,
            "output_sha256": self.output_sha256,
        }


class VmlxClient:
    """Read and infer through vMLX only on an explicit loopback endpoint."""

    def __init__(self, endpoint: str, model: str, api_key: str | None = None):
        require(isinstance(endpoint, str) and len(endpoint) <= 256,
                "loopback_vmlx_endpoint_required")
        try:
            selected = urllib.parse.urlsplit(endpoint)
            port = selected.port
        except ValueError as error:
            raise VmlxRefusal("loopback_vmlx_endpoint_required") from error
        require(selected.scheme == "http" and selected.hostname in {
            "localhost", "127.0.0.1", "::1"
        } and port is not None and 1 <= port <= 65_535
            and not selected.username and not selected.password
            and selected.path in {"", "/"} and not selected.query
            and not selected.fragment, "loopback_vmlx_endpoint_required")
        require(isinstance(model, str) and MODEL_ID.fullmatch(model) is not None,
                "invalid_vmlx_model_id")
        require(api_key is None or (isinstance(api_key, str)
                and 0 < len(api_key) <= 4096
                and not any(ord(char) < 32 or ord(char) == 127 for char in api_key)),
                "invalid_vmlx_token")
        self.endpoint = f"http://{selected.netloc}"
        self.model = model
        self.api_key = api_key
        self.opener = urllib.request.build_opener(
            urllib.request.ProxyHandler({}), _NoRedirects()
        )

    def _request(self, path: str, body: dict[str, Any] | None = None) -> dict[str, Any]:
        headers = {"Accept": "application/json"}
        if self.api_key is not None:
            headers["Authorization"] = "Bearer " + self.api_key
        data = None
        if body is not None:
            data = json.dumps(body, separators=(",", ":"), allow_nan=False).encode()
            require(len(data) <= 4096, "vmlx_request_bound")
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(
            self.endpoint + path, data=data, headers=headers,
            method="POST" if data is not None else "GET",
        )
        try:
            with self.opener.open(request, timeout=30) as response:
                require(response.status == 200, "vmlx_http_status")
                declared = response.headers.get("Content-Length")
                require(declared is None or (declared.isascii() and declared.isdigit()
                        and int(declared) <= MAX_RESPONSE), "vmlx_response_bound")
                raw = response.read(MAX_RESPONSE + 1)
        except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError,
                OSError) as error:
            raise VmlxRefusal("vmlx_transport_or_http_refusal") from error
        require(len(raw) <= MAX_RESPONSE, "vmlx_response_bound")
        try:
            value = json.loads(raw, parse_constant=_reject_nonfinite)
        except (UnicodeError, ValueError) as error:
            raise VmlxRefusal("vmlx_response_json") from error
        require(isinstance(value, dict), "vmlx_response_object")
        return value

    def ready(self) -> None:
        """Require both process health and one unambiguous served model ID."""
        health = self._request("/health")
        require(health.get("status") == "healthy"
                and health.get("model_loaded") is True
                and health.get("served_model_name") == self.model,
                "vmlx_model_not_ready")
        listing = self._request("/v1/models")
        data = listing.get("data")
        require(isinstance(data, list) and 1 <= len(data) <= 32
                and all(isinstance(row, dict) and isinstance(row.get("id"), str)
                        for row in data), "vmlx_model_listing_ambiguous")
        ids = [row["id"] for row in data]
        require(ids.count(self.model) == 1 and len(ids) == len(set(ids)),
                "vmlx_model_listing_ambiguous")

    def generate(self, prompt: str, max_tokens: int = 32) -> VmlxReply:
        """Run one bounded text request; never pass tools, URLs or instructions as authority."""
        require(isinstance(prompt, str) and 0 < len(prompt.encode()) <= MAX_PROMPT,
                "vmlx_prompt_bound")
        require(type(max_tokens) is int and 1 <= max_tokens <= 64,
                "vmlx_token_bound")
        self.ready()
        value = self._request("/v1/chat/completions", {
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max_tokens,
            "temperature": 0,
            "stream": False,
        })
        choices = value.get("choices")
        require(value.get("model") == self.model
                and isinstance(choices, list) and len(choices) == 1
                and isinstance(choices[0], dict), "vmlx_model_or_choice_mismatch")
        choice = choices[0]
        message = choice.get("message")
        require(isinstance(message, dict) and message.get("role") == "assistant"
                and isinstance(message.get("content"), str)
                and len(message["content"].encode()) <= 8192
                and not message.get("tool_calls") and not message.get("function_call")
                and choice.get("finish_reason") in {"stop", "length"},
                "vmlx_content_or_tool_refusal")
        usage = value.get("usage")
        require(isinstance(usage, dict)
                and type(usage.get("prompt_tokens")) is int
                and 0 < usage["prompt_tokens"] <= 4096
                and type(usage.get("completion_tokens")) is int
                and 0 < usage["completion_tokens"] <= max_tokens,
                "vmlx_usage_bound")
        output = message["content"]
        return VmlxReply(
            text=output, model=self.model,
            finish_reason=choice["finish_reason"],
            prompt_tokens=usage["prompt_tokens"],
            completion_tokens=usage["completion_tokens"],
            prompt_sha256=hashlib.sha256(prompt.encode()).hexdigest(),
            output_sha256=hashlib.sha256(output.encode()).hexdigest(),
        )
