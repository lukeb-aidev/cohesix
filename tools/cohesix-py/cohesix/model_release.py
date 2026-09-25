# Author: Lukas Bower
# Purpose: Give Python operators a bounded, non-authoritative view of private LoRA releases.
# Copyright 2026 Lukas Bower
"""Operate one immutable PEFT release through the existing Cohesix CLI."""

from __future__ import annotations

from dataclasses import dataclass
import hashlib
import math
from pathlib import Path
import re
from typing import Any

from .journey import document, encode
from .playbooks import run_peft_release


_DIGEST = re.compile(r"[0-9a-f]{64}\Z")
_TERMINAL = {"succeeded", "failed", "recovered_failure", "rollback_failed"}


@dataclass(frozen=True)
class PeftReleaseStatus:
    """CLI projection of one release; only an explicit verify can affirm success."""

    operation_id: str
    request_sha256: str
    state: str
    submitted: bool
    acknowledged: bool
    ambiguous: bool
    graph_sha256: str | None
    requested_outcome_verified: bool
    heldout_baseline_loss: float | None
    heldout_candidate_loss: float | None
    heldout_samples: int | None
    comparison_policy_sha256: str | None
    baseline_generation: int | None
    candidate_adapter_sha256: str | None
    observed_served_generation: int | None
    observed_restored_generation: int | None
    rollback_observed: bool


def _digest(value: object, field: str) -> str:
    if not isinstance(value, str) or _DIGEST.fullmatch(value) is None:
        raise ValueError(f"invalid PEFT release {field}")
    return value


def _generation(value: object, field: str) -> int:
    if type(value) is not int or not 0 <= value < 2**32:
        raise ValueError(f"invalid PEFT release {field}")
    return value


def _loss(value: object, field: str) -> float:
    if type(value) not in {float, int}:
        raise ValueError(f"invalid PEFT release {field}")
    try:
        numeric = float(value)
    except OverflowError as error:
        raise ValueError(f"invalid PEFT release {field}") from error
    if not math.isfinite(numeric) or numeric < 0:
        raise ValueError(f"invalid PEFT release {field}")
    return numeric


def _observations(native: dict[str, object], state: str) -> dict[str, object]:
    """Project only exact signed comparison and phase values; no inferred serving state."""
    observed: dict[str, object] = {
        "heldout_baseline_loss": None, "heldout_candidate_loss": None,
        "heldout_samples": None, "comparison_policy_sha256": None,
        "baseline_generation": None, "candidate_adapter_sha256": None,
        "observed_served_generation": None, "observed_restored_generation": None,
        "rollback_observed": False,
    }
    comparison = native.get("comparison")
    if comparison is not None:
        if not isinstance(comparison, dict) or not isinstance(comparison.get("baseline"), dict):
            raise ValueError("invalid PEFT release comparison")
        observed["comparison_policy_sha256"] = _digest(
            comparison.get("policy_sha256"), "comparison policy")
        observed["candidate_adapter_sha256"] = _digest(
            comparison.get("candidate_sha256"), "candidate adapter")
        observed["baseline_generation"] = _generation(
            comparison["baseline"].get("generation"), "baseline generation")
    rows = native.get("phases")
    if rows is not None:
        if not isinstance(rows, list) or len(rows) > 13:
            raise ValueError("invalid PEFT release phase observations")
        phases = {}
        for row in rows:
            if (not isinstance(row, dict) or not isinstance(row.get("phase"), str)
                    or row["phase"] in phases or not isinstance(row.get("result"), dict)):
                raise ValueError("invalid PEFT release phase observation")
            phases[row["phase"]] = row["result"]
        evaluated = phases.get("evaluate")
        if evaluated is not None:
            detail = evaluated.get("detail")
            if (not isinstance(detail, dict) or not isinstance(detail.get("candidate"), dict)
                    or not isinstance(detail.get("baseline"), dict)):
                raise ValueError("invalid PEFT release evaluation")
            candidate = detail["candidate"]
            baseline = detail["baseline"]
            if (not isinstance(candidate.get("metrics"), dict)
                    or not isinstance(baseline.get("metrics"), dict)):
                raise ValueError("invalid PEFT release held-out metrics")
            observed["heldout_candidate_loss"] = _loss(
                candidate["metrics"].get("eval_loss"), "candidate loss")
            observed["heldout_baseline_loss"] = _loss(
                baseline["metrics"].get("eval_loss"), "baseline loss")
            samples = _generation(candidate.get("samples"), "held-out samples")
            if samples == 0 or baseline.get("samples") != samples:
                raise ValueError("invalid PEFT release held-out sample identity")
            observed["heldout_samples"] = samples
        for phase, field in (("promote", "observed_served_generation"),
                             ("rollback", "observed_restored_generation")):
            result = phases.get(phase)
            if result is not None and result.get("succeeded") is True:
                detail = result.get("detail")
                accepted = detail.get("accepted") if isinstance(detail, dict) else None
                if not isinstance(accepted, dict):
                    raise ValueError("invalid PEFT release accepted observation")
                observed[field] = _generation(accepted.get("generation"), "observed generation")
        observed["rollback_observed"] = observed["observed_restored_generation"] is not None
    if state == "succeeded" and (observed["comparison_policy_sha256"] is None
                                  or observed["observed_served_generation"] is None):
        raise ValueError("successful PEFT release lacks comparison or serving observation")
    if state == "recovered_failure" and not observed["rollback_observed"]:
        raise ValueError("recovered PEFT release lacks restoration observation")
    return observed


def _status(report: dict[str, object], operation_id: str, verified: bool) -> PeftReleaseStatus:
    """Reject substituted or contradictory projections before exposing a decision."""
    if (report.get("schema") != "cohesix-peft-report/v1"
            or report.get("authoritative") is not False
            or report.get("production_use_case_accepted") is not False
            or report.get("operation_id") != operation_id):
        raise ValueError("invalid PEFT release report identity")
    request_sha256 = _digest(report.get("request_sha256"), "request digest")
    submitted = report.get("submitted")
    acknowledged = report.get("acknowledged")
    ambiguous = report.get("ambiguous")
    if (type(submitted) is not bool or type(acknowledged) is not bool
            or type(ambiguous) is not bool or (acknowledged and not submitted)):
        raise ValueError("invalid PEFT release admission state")
    result = report.get("result")
    observed = _observations({}, "outcome_unknown")
    if result is None:
        if ambiguous != submitted or verified:
            raise ValueError("invalid PEFT release pending state")
        state = "outcome_unknown" if submitted else "not_submitted"
        graph_sha256 = None
    else:
        # A signed result may have been admitted through MCP or REST while this
        # CLI controller's own journal remains unsubmitted. The CLI verifier
        # owns proof; this field describes only this controller's submission.
        if (not isinstance(result, dict) or ambiguous
                or not isinstance(result.get("state"), str)
                or result["state"] not in _TERMINAL):
            raise ValueError("invalid PEFT release terminal state")
        state = result["state"]
        graph_sha256 = _digest(result.get("graph_sha256"), "evidence graph digest")
        native = result.get("native")
        if (not isinstance(native, dict)
                or native.get("schema") != "cohesix-peft-recipe-journal/v1"
                or native.get("operation_id") != operation_id
                or native.get("request_sha256") != request_sha256
                or native.get("state") != state):
            raise ValueError("invalid PEFT release native result binding")
        observed = _observations(native, state)
        if verified and state != "succeeded":
            raise ValueError("PEFT release verifier did not accept the requested outcome")
    return PeftReleaseStatus(
        operation_id=operation_id,
        request_sha256=request_sha256,
        state=state,
        submitted=submitted,
        acknowledged=acknowledged,
        ambiguous=ambiguous,
        graph_sha256=graph_sha256,
        requested_outcome_verified=verified,
        **observed,
    )


class PeftReleaseClient:
    """Bind one deployment request and delegate every effect and proof to ``coh``.

    ``apply`` never retries. After a lost response, call ``inspect`` or ``recover``
    with the same deployment; the CLI reconciles its durable journal.
    """

    def __init__(self, *, coh_binary: Path, deployment: Path) -> None:
        self.coh_binary = Path(coh_binary)
        self.deployment = Path(deployment)
        if not self.coh_binary.is_absolute() or not self.deployment.is_absolute():
            raise ValueError("absolute PEFT release paths required")
        request = self._request()
        operation_id = request.get("operation_id")
        if (not isinstance(operation_id, str)
                or re.fullmatch(r"[A-Za-z0-9_-]{1,64}", operation_id) is None):
            raise ValueError("bounded PEFT release operation ID required")
        self.operation_id = operation_id
        self._request_digest = hashlib.sha256(encode(request)).hexdigest()
        self._cli_request_sha256: str | None = None

    def _request(self) -> dict[str, Any]:
        request = document(self.deployment).get("request")
        if not isinstance(request, dict):
            raise ValueError("PEFT release request object required")
        return request

    def _call(self, lifecycle: str, **kwargs: object) -> PeftReleaseStatus:
        request = self._request()
        if (request.get("operation_id") != self.operation_id
                or hashlib.sha256(encode(request)).hexdigest() != self._request_digest):
            raise ValueError("PEFT release request changed; use a new operation")
        report = run_peft_release(
            lifecycle, deployment=self.deployment, coh_binary=self.coh_binary,
            **kwargs,
        )
        if hashlib.sha256(encode(self._request())).hexdigest() != self._request_digest:
            raise ValueError("PEFT release request changed during CLI operation")
        status = _status(report, self.operation_id, lifecycle == "verify")
        if (self._cli_request_sha256 is not None
                and status.request_sha256 != self._cli_request_sha256):
            raise ValueError("PEFT release report request identity changed")
        self._cli_request_sha256 = status.request_sha256
        return status

    def plan(self) -> PeftReleaseStatus:
        """Create or inspect the durable plan without submitting work."""
        return self._call("plan")

    def apply(self, *, auth_ref: str, ticket_ref: str,
              rest_url: str | None = None, host: str | None = None,
              port: int = 31337) -> PeftReleaseStatus:
        """Submit once through Cohesix admission with explicit authority refs."""
        return self._call("apply", auth_ref=auth_ref, ticket_ref=ticket_ref,
                          rest_url=rest_url, host=host, port=port)

    def watch(self) -> PeftReleaseStatus:
        """Observe the journal and signed result without submitting again."""
        return self._call("watch")

    def verify(self) -> PeftReleaseStatus:
        """Require the CLI verifier's accepted, successful requested outcome."""
        return self._call("verify")

    def recover(self) -> PeftReleaseStatus:
        """Read the existing operation after an interruption or lost response."""
        return self._call("recover")

    def inspect(self) -> PeftReleaseStatus:
        """Verify a successful observation; otherwise retain its exact state."""
        status = self.watch()
        return self.verify() if status.state == "succeeded" else status
