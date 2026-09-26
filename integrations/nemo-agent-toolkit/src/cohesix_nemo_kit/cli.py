"""Run scoped NeMo native-client workflows with private credential references.

Author: Lukas Bower
Purpose: Provide install, discovery, CUDA submission and A2A recovery entry points.
Copyright 2026 Lukas Bower
"""

from __future__ import annotations

import argparse
import asyncio
from datetime import timedelta
import importlib.metadata
from importlib import resources
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
from typing import Any
from urllib.parse import urlsplit
from urllib.request import Request, urlopen

TOOL_NAMES = (
    "cohesix.available_selected_jobs",
    "cohesix.preflight_selected_job",
    "cohesix.submit_selected_job",
    "cohesix.inspect_job",
    "cohesix.recover_job",
)
VERSIONS = {"nvidia-nat": "1.9.0", "nvidia-nat-mcp": "1.9.0",
            "nvidia-nat-a2a": "1.9.0", "mcp": "1.29.1", "a2a-sdk": "0.3.26"}
ID = re.compile(r"[A-Za-z0-9._-]{1,96}\Z")


def _private_text(path: Path, limit: int = 8192) -> str:
    """Read a small owner-only regular file without following a symlink."""
    if not path.is_absolute():
        raise ValueError("private path must be absolute")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_mode & 0o077 or info.st_size > limit:
            raise ValueError("private file must be owner-only, regular and bounded")
        return os.read(fd, limit + 1).decode("utf-8")
    finally:
        os.close(fd)


def secret(ref: str) -> str:
    """Resolve only explicit private file or environment references."""
    if ref.startswith("file:"):
        value = _private_text(Path(ref[5:])).strip()
    elif ref.startswith("env:"):
        name = ref[4:]
        if not re.fullmatch(r"[A-Z][A-Z0-9_]{0,63}", name):
            raise ValueError("invalid secret environment name")
        value = os.environ.get(name, "")
    else:
        raise ValueError("secret must use file: or env: reference")
    if not 8 <= len(value) <= 8192 or any(char.isspace() for char in value):
        raise ValueError("secret is absent or malformed")
    return value


def settings(path: Path) -> dict[str, str]:
    """Validate one subject's isolated native-client configuration."""
    data = json.loads(_private_text(path, 16384))
    required = {"gateway_url", "subject", "request_auth_ref", "delegated_ticket_ref"}
    if not isinstance(data, dict) or set(data) != required:
        raise ValueError("settings fields must match the selected schema")
    if not isinstance(data["subject"], str) or not ID.fullmatch(data["subject"]):
        raise ValueError("invalid subject label")
    for key in ("gateway_url", "request_auth_ref", "delegated_ticket_ref"):
        if not isinstance(data[key], str):
            raise ValueError(f"invalid {key}")
    url = urlsplit(data["gateway_url"])
    if (url.scheme not in ("http", "https") or not url.hostname or url.username
            or url.password or url.path not in ("", "/") or url.query or url.fragment
            or (url.scheme == "http" and url.hostname not in ("127.0.0.1", "::1", "localhost"))):
        raise ValueError("gateway must be HTTPS or loopback HTTP base URL")
    return data


def model_settings(path: Path) -> dict[str, str]:
    """Require an explicit pinned OpenAI-compatible endpoint and key reference."""
    data = json.loads(_private_text(path, 16384))
    if (not isinstance(data, dict)
            or set(data) != {"base_url", "model_name", "api_key_ref"}
            or any(not isinstance(value, str) for value in data.values())):
        raise ValueError("model settings fields must match the selected schema")
    url = urlsplit(data["base_url"])
    if (url.scheme not in ("http", "https") or not url.hostname or url.username
            or url.password or url.query or url.fragment or url.path.rstrip("/") != "/v1"
            or (url.scheme == "http" and url.hostname not in ("127.0.0.1", "::1", "localhost"))
            or not 1 <= len(data["model_name"]) <= 128):
        raise ValueError("model endpoint must be HTTPS or loopback /v1 with a pinned name")
    return data


def probe_model(model: dict[str, str]) -> None:
    """Check that the configured model is loaded before an agent can submit."""
    credential = secret(model["api_key_ref"])
    request_url = model["base_url"].rstrip("/") + "/models"
    query = Request(request_url, headers={"Authorization": f"Bearer {credential}"})
    with urlopen(query, timeout=10) as response:
        if response.status != 200:
            raise ValueError("model catalogue unavailable")
        payload = response.read(65537)
    if len(payload) > 65536:
        raise ValueError("model catalogue too large")
    catalogue = json.loads(payload)
    names = {row.get("id") for row in catalogue.get("data", []) if isinstance(row, dict)}
    if model["model_name"] not in names:
        raise ValueError("configured model is not loaded")


def versions() -> dict[str, str]:
    """Refuse a different native client release instead of assuming compatibility."""
    observed = {name: importlib.metadata.version(name) for name in VERSIONS}
    if observed != VERSIONS:
        raise ValueError(f"NeMo dependency mismatch: {observed}")
    return observed


def request(path: Path) -> tuple[str, dict[str, Any]]:
    """Validate the stable request identity before any effectful call."""
    data = json.loads(_private_text(path, 65536))
    if not isinstance(data, dict) or set(data) != {"scope_id", "ticket"}:
        raise ValueError("request needs scope_id and ticket")
    scope, ticket = data["scope_id"], data["ticket"]
    if (not isinstance(scope, str) or not ID.fullmatch(scope)
            or not isinstance(ticket, dict)
            or ticket.get("schema") != "host-ticket/v2"
            or ticket.get("action") not in ("gpu.workload.submit", "peft.release")
            or not isinstance(ticket.get("id"), str)
            or not ID.fullmatch(ticket["id"])
            or not isinstance(ticket.get("idempotency_key"), str)
            or not ID.fullmatch(ticket["idempotency_key"])):
        raise ValueError("invalid selected ticket or stable identity")
    return scope, ticket


def headers(config: dict[str, str]) -> dict[str, str]:
    """Bind both protocols to the same verified gateway credential pair."""
    return {"x-cohesix-auth": secret(config["request_auth_ref"]),
            "x-cohesix-ticket": secret(config["delegated_ticket_ref"])}


def result(response: Any) -> dict[str, Any]:
    """Require native MCP typed content and propagate protocol refusal."""
    if response.isError or not isinstance(response.structuredContent, dict):
        raise ValueError("native MCP tool refused or omitted typed content")
    return response.structuredContent


async def mcp_workflow(config: dict[str, str], scope: str | None,
                       ticket: dict[str, Any] | None,
                       recover_id: str | None = None) -> dict[str, Any]:
    """Discover, preflight, submit once, then recover the original admission."""
    from nat.plugins.mcp.client.client_base import MCPStreamableHTTPClient

    async with MCPStreamableHTTPClient(
        config["gateway_url"].rstrip("/") + "/mcp", custom_headers=headers(config),
        user_id=config["subject"], tool_call_timeout=timedelta(seconds=30),
        reconnect_max_attempts=1, reconnect_initial_backoff=0.25,
        reconnect_max_backoff=1.0,
    ) as client:
        tools = await client.get_tools()
        missing = sorted(set(TOOL_NAMES) - set(tools))
        if missing:
            raise ValueError(f"selected MCP tools unavailable: {missing}")
        if recover_id is not None:
            recovered = result(await client.call_tool(
                TOOL_NAMES[4], {"admission_id": recover_id}))
            return recovery_summary(recover_id, recovered, "observation_only")
        selected = result(await client.call_tool(TOOL_NAMES[0], {}))
        if selected.get("schema") != "cohesix-available-selected-jobs/v1":
            raise ValueError("selected job schema mismatch")
        if ticket is None:
            return {"state": "discovered", "tools": list(TOOL_NAMES),
                    "scope_count": len(selected.get("scopes", []))}
        assert scope is not None
        original_id = ticket["id"]
        prepared = result(await client.call_tool(TOOL_NAMES[1],
                                                 {"scope_id": scope, "ticket": ticket}))
        body = prepared.get("request")
        if not isinstance(body, dict) or body.get("ticket", {}).get("id") != original_id:
            raise ValueError("preflight replaced ticket identity")
        try:
            acknowledgement = result(await client.call_tool(TOOL_NAMES[2], body))
            submitted = acknowledgement.get("schema")
        except Exception:
            # A lost ACK is uncertain. Observation may recover it; never replay submit.
            submitted = "uncertain"
    async with MCPStreamableHTTPClient(
        config["gateway_url"].rstrip("/") + "/mcp", custom_headers=headers(config),
        user_id=config["subject"], tool_call_timeout=timedelta(seconds=30),
        reconnect_max_attempts=1,
    ) as client:
        recovered = result(await client.call_tool(TOOL_NAMES[4],
                                                 {"admission_id": original_id}))
    return recovery_summary(original_id, recovered, submitted)


def recovery_summary(original_id: str, recovered: dict[str, Any],
                     submitted: str) -> dict[str, Any]:
    """Project only the native ledger's original result and replay boundary."""
    record = recovered.get("record", {})
    binding = record.get("binding", {})
    if binding.get("admission_id") != original_id or binding.get("ticket_id") != original_id:
        raise ValueError("recovery returned another job")
    return {"state": record.get("execution", "unknown"), "admission_id": original_id,
            "submit_ack_schema": submitted, "result_sha256": record.get("result_sha256"),
            "delivery": record.get("delivery"),
            "effect_replay_allowed": recovered.get("effect_replay_allowed")}


async def a2a_workflow(config: dict[str, str], scope: str | None,
                       ticket: dict[str, Any] | None, lookup_id: str | None,
                       cancel_id: str | None) -> dict[str, Any]:
    """Use the per-process subject's NeMo card, data submission and task helpers."""
    from cohesix_nemo_kit.native import CohesixA2AClient, task_from_event

    base = config["gateway_url"].rstrip("/")
    async with CohesixA2AClient(base, headers(config), streaming=False,
                                task_timeout=timedelta(seconds=45)) as client:
        card = client.agent_card
        if card is None:
            raise ValueError("scoped Agent Card unavailable")
        skills = sorted(skill.id for skill in card.skills)
        if ticket is not None and ticket["action"] not in skills:
            raise ValueError("selected A2A skill unavailable")
        if ticket is None and not skills:
            raise ValueError("selected A2A skills unavailable")
        if ticket is None and lookup_id is None and cancel_id is None:
            return {"state": "discovered", "skills": skills}
        original_id = ticket["id"] if ticket else lookup_id or cancel_id
        if ticket is not None:
            assert scope is not None
            try:
                async for event in client.send_job(scope, ticket):
                    task = task_from_event(event)
                    if task and task.get("id") != original_id:
                        raise ValueError("A2A task substituted original ID")
            except ValueError:
                raise
            except Exception:
                # The send may have reached the gateway; lookup remains authoritative.
                pass
    async with CohesixA2AClient(base, headers(config), streaming=False,
                                task_timeout=timedelta(seconds=45)) as client:
        task = await client.get_task(original_id)
        if task.id != original_id:
            raise ValueError("A2A lookup substituted original ID")
        cancel_state = None
        if cancel_id:
            canceled = await client.cancel_task(cancel_id)
            if canceled.id != cancel_id:
                raise ValueError("A2A cancellation substituted original ID")
            cancel_state = canceled.status.state.value
    return {"state": task.status.state.value, "task_id": original_id,
            "cancel_state": cancel_state, "provider_verified": False}


def _write_summary(path: Path, summary: dict[str, Any]) -> None:
    """Create an owner-only receipt without replacing earlier evidence."""
    if not path.is_absolute():
        raise ValueError("output must be absolute")
    fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as stream:
        json.dump(summary, stream, sort_keys=True, indent=2)
        stream.write("\n")


def runtime_env(mode: str, config: dict[str, str], model: dict[str, str]) -> dict[str, str]:
    """Bind a probed model and only the selected protocol's gateway references."""
    probe_model(model)
    environment = dict(os.environ)
    environment.update({
        "NEMO_MODEL_NAME": model["model_name"],
        "NEMO_MODEL_BASE_URL": model["base_url"],
        "NEMO_MODEL_API_KEY": secret(model["api_key_ref"]),
        "COHESIX_MCP_URL": config["gateway_url"].rstrip("/") + "/mcp",
        "COHESIX_A2A_BASE_URL": config["gateway_url"].rstrip("/"),
        "COHESIX_REQUEST_AUTH_REF": config["request_auth_ref"],
        "COHESIX_DELEGATED_TICKET_REF": config["delegated_ticket_ref"],
        "NEMO_EVAL_DATASET": "/dev/null",
    })
    if mode in ("agent-mcp", "eval-mcp"):
        environment["COHESIX_REQUEST_AUTH"] = secret(config["request_auth_ref"])
        environment["COHESIX_DELEGATED_TICKET"] = secret(config["delegated_ticket_ref"])
    return environment


def run_agent(mode: str, config: dict[str, str], model: dict[str, str],
              prompt: Path, trace: Path) -> dict[str, Any]:
    """Run NeMo's pinned agent with private input and trace."""
    prompt_text = _private_text(prompt, 8192)
    if not prompt_text.strip() or not trace.is_absolute():
        raise ValueError("agent prompt and absolute trace path required")
    environment = runtime_env(mode, config, model)
    filename = "mcp-agent.yaml" if mode == "agent-mcp" else "a2a-agent.yaml"
    configuration = resources.files("cohesix_nemo_kit").joinpath("configs", filename)
    nat = Path(sys.executable).with_name("nat")
    fd = os.open(trace, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as output:
        completed = subprocess.run(
            [str(nat), "run", "--config_file", str(configuration),
             "--input_file", str(prompt), "--user_id", config["subject"]],
            env=environment, stdout=output, stderr=subprocess.STDOUT,
            timeout=600, check=False,
        )
    return {"state": "agent_exited" if completed.returncode == 0 else "agent_failed",
            "exit_code": completed.returncode, "trace": str(trace),
            "provider_verified": False}


def run_evaluation(mode: str, config: dict[str, str], model: dict[str, str],
                   dataset: Path, directory: Path) -> dict[str, Any]:
    """Run pinned NeMo evaluation and profiler on a bounded private dataset."""
    rows = json.loads(_private_text(dataset, 32768))
    if (not isinstance(rows, list) or not 1 <= len(rows) <= 16
            or any(not isinstance(row, dict) or set(row) != {"id", "question", "answer"}
                   or not isinstance(row["id"], str) or not ID.fullmatch(row["id"])
                   or not isinstance(row["question"], str)
                   or not 1 <= len(row["question"].strip()) <= 8192
                   or not isinstance(row["answer"], str)
                   or len(row["answer"]) > 8192 for row in rows)):
        raise ValueError("evaluation needs one to sixteen fixed question rows")
    if not directory.is_absolute() or directory.exists():
        raise ValueError("evaluation directory must be unused and absolute")
    environment = runtime_env(mode, config, model)
    environment["NEMO_EVAL_DATASET"] = str(dataset)
    directory.mkdir(mode=0o700)
    filename = {"eval-direct": "direct-agent.yaml", "eval-mcp": "mcp-agent.yaml",
                "eval-a2a": "a2a-agent.yaml"}[mode]
    configuration = resources.files("cohesix_nemo_kit").joinpath("configs", filename)
    trace = directory / "nat-eval.log"
    nat = Path(sys.executable).with_name("nat")
    fd = os.open(trace, os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as output:
        completed = subprocess.run(
            [str(nat), "eval", "--config_file", str(configuration),
             "--dataset", str(dataset), "--user_id", config["subject"]],
            cwd=directory, env=environment, stdout=output, stderr=subprocess.STDOUT,
            timeout=600, check=False,
        )
    return {"state": "evaluation_exited" if completed.returncode == 0 else "evaluation_failed",
            "exit_code": completed.returncode, "trace": str(trace),
            "output_directory": str(directory), "provider_verified": False}


def main() -> None:
    """Run only the requested native client path; never print credentials."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("doctor", "mcp", "a2a", "agent-mcp", "agent-a2a",
                                         "eval-direct", "eval-mcp", "eval-a2a"))
    parser.add_argument("--settings", required=True, type=Path)
    parser.add_argument("--request", type=Path)
    parser.add_argument("--recover-id")
    parser.add_argument("--get-task-id")
    parser.add_argument("--cancel-id")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--model-settings", type=Path)
    parser.add_argument("--input-file", type=Path)
    parser.add_argument("--trace-log", type=Path)
    parser.add_argument("--dataset", type=Path)
    parser.add_argument("--eval-dir", type=Path)
    args = parser.parse_args()
    try:
        config = settings(args.settings)
        pinned = versions()
        if args.cancel_id and (args.mode != "a2a" or not ID.fullmatch(args.cancel_id)):
            raise ValueError("cancellation requires a valid A2A task ID")
        if args.request and args.mode not in ("mcp", "a2a"):
            raise ValueError("request requires a native protocol workflow")
        if args.recover_id and (args.mode != "mcp" or args.request
                                or not ID.fullmatch(args.recover_id)):
            raise ValueError("MCP recovery needs one original ID and no new request")
        if args.get_task_id and (args.mode != "a2a" or args.request
                                 or not ID.fullmatch(args.get_task_id)):
            raise ValueError("A2A lookup needs one original ID and no new request")
        if args.cancel_id and (args.request or
                               (args.get_task_id and args.get_task_id != args.cancel_id)):
            raise ValueError("A2A cancellation must name the inspected original task")
        scope, ticket = request(args.request) if args.request else (None, None)
        if args.mode == "doctor":
            secret(config["request_auth_ref"])
            secret(config["delegated_ticket_ref"])
            summary = {"state": "ready", "subject_label": config["subject"],
                       "native_versions": pinned, "transport": "streamable-http"}
        elif args.mode == "mcp":
            summary = asyncio.run(mcp_workflow(config, scope, ticket, args.recover_id))
        elif args.mode == "a2a":
            summary = asyncio.run(a2a_workflow(config, scope, ticket,
                                               args.get_task_id, args.cancel_id))
        elif args.mode.startswith("eval-"):
            if not args.model_settings or not args.dataset or not args.eval_dir:
                raise ValueError("evaluation requires model settings, dataset and directory")
            summary = run_evaluation(args.mode, config, model_settings(args.model_settings),
                                     args.dataset, args.eval_dir)
        else:
            if not args.model_settings or not args.input_file or not args.trace_log:
                raise ValueError("agent requires model settings, input file and trace log")
            summary = run_agent(args.mode, config, model_settings(args.model_settings),
                                args.input_file, args.trace_log)
        summary["schema"] = "cohesix-nemo-client-observation/v1"
        summary["protocol"] = args.mode
        if args.output:
            _write_summary(args.output, summary)
        print(json.dumps(summary, sort_keys=True))
    except Exception as exc:
        parser.exit(2, f"cohesix-nemo: {type(exc).__name__}; inspect private evidence and configuration\n")


if __name__ == "__main__":
    main()
