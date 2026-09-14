# Author: Lukas Bower
# Purpose: Verify strict authority serialization, live credential refusals and key rotation.
# Copyright 2026 Lukas Bower

"""M27a client checks are projections; they do not replace gateway MAC proof."""

import json
from dataclasses import replace

import pytest

from cohesix import AdmissionCorrelation, QueenIntent
from cohesix.auth import resolve_secret_reference, resolve_tcp_auth_token
from cohesix.backends import RestBackend, TcpBackend
from cohesix.defaults import DEFAULTS
from cohesix.errors import CohesixError
from cohesix.orchestration import HostTicketRequest


@pytest.mark.parametrize("action,target,args", [
    ("systemd.restart", "/host/systemd/node.service/stop", {}),
    ("docker.restart", None, {"container": "--all"}),
    ("k8s.cordon", None, {"node": "node-1", "force": True}),
    ("gpu.lease.grant", "/gpu/GPU-0/lease", {"gpu_id": "GPU-1"}),
    ("peft.import", None, {"job_id": "job", "model_id": "model", "adapter_dir": "../adapter"}),
    ("gpu.lease.renew", None, {"gpu_id": "GPU-0", "ttl_s": True}),
])
def test_provider_operands_refuse_ambiguity_and_injection(action, target, args):
    with pytest.raises(CohesixError):
        HostTicketRequest("ticket", "retry", action, target, args)


@pytest.mark.parametrize("action,target,args", [
    ("systemd.restart", "/host/systemd/node.service/restart", {}),
    ("docker.restart", None, {"container": "node-1"}),
    ("k8s.cordon", None, {"node": "node-1"}),
    ("gpu.lease.grant", "/gpu/GPU-0/lease", {"mem_mb": 32, "streams": 1}),
    ("peft.import", None, {"job_id": "job", "model_id": "model", "adapter_dir": "/private/adapter"}),
])
def test_provider_canonical_forms_preserve_exact_operands(action, target, args):
    payload = HostTicketRequest("ticket", "retry", action, target, args).to_payload()
    assert payload["action"] == action
    assert payload.get("target") == target
    assert payload.get("args", {}) == args


def test_strict_intent_keeps_writer_and_admission_state_separate():
    admission = AdmissionCorrelation("decision", "a" * 64, "b" * 64, 8, 9, 3000)
    intent = QueenIntent("one", "retry", 1000, {"spawn": "heartbeat"}, 1, admission)
    payload = json.loads(intent.encode())
    assert payload["schema"] == "queen-intent/v1"
    assert payload["writer_epoch"] == 1
    assert payload["admission"]["state_epoch"] == 8
    assert payload["admission"]["resource_generation"] == 9
    assert intent.encode() == intent.encode()
    with pytest.raises(CohesixError, match="stale-writer"):
        replace(intent, writer_epoch=2).encode()
    assert "admission" not in json.loads(replace(intent, admission=None).encode())


@pytest.mark.parametrize("value", ["", "../x", "-option", "a/b", "a\nb", "x" * 129])
def test_strict_identity_cannot_be_path_or_option(value):
    with pytest.raises(CohesixError):
        QueenIntent(value, "key", 1, {"spawn": "heartbeat"})


def test_file_rotation_and_invalid_selected_source_never_fall_back(tmp_path, monkeypatch):
    path = tmp_path / "credential"
    path.write_text("deployment-key-one\n")
    reference = f"file:{path}"
    assert resolve_secret_reference(reference) == "deployment-key-one"
    path.write_text("deployment-key-two\n")
    assert resolve_secret_reference(reference) == "deployment-key-two"
    monkeypatch.setenv("COH_AUTH_TOKEN", "alternate-key")
    path.write_text("bootstrap")
    with pytest.raises(ValueError, match="insecure placeholder"):
        resolve_tcp_auth_token(reference)


@pytest.mark.parametrize("placeholder", DEFAULTS["placeholder_credentials"])
def test_live_placeholder_fails_before_socket_connect(placeholder, monkeypatch):
    def forbidden(*args, **kwargs):
        raise AssertionError("invalid credential attempted a connection")
    monkeypatch.setattr("socket.create_connection", forbidden)
    with pytest.raises(ValueError, match="insecure placeholder"):
        TcpBackend("127.0.0.1", 1, placeholder, "queen", None)


def test_rest_missing_delegation_does_not_send_or_retry(monkeypatch):
    monkeypatch.delenv("COH_REST_TICKET", raising=False)
    def forbidden(*args, **kwargs):
        raise AssertionError("unauthorized REST call reached HTTP")
    monkeypatch.setattr("urllib.request.urlopen", forbidden)
    backend = RestBackend("http://127.0.0.1:1", request_auth_token="deployment-auth")
    with pytest.raises(CohesixError, match="delegated ticket required"):
        backend.write_append("/queen/intents/ctl", b"{}")
