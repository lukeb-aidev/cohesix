<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Project generated host-integration dependency and support truth. -->
<!-- Author: Lukas Bower -->

# Generated Host-Integration Dependencies

This table is generated from `configs/host_integration_acceptance.toml`. Worker execution, provider availability, package presence, mock or dry-run success, and use-case promotion are independent states.

| Dependency | Obligation | Required mode | Worker roles | Owner milestone |
| --- | --- | --- | --- | --- |
| `a2a-gateway` | `future` | `disabled` | `none` | `m28f-a2a-policy-ir` |
| `authenticated-console-projection` | `release_required` | `live` | `worker-gpu, worker-heartbeat, worker-lora` | `m26e-host-integration-dependency-contract` |
| `cas-artifact` | `release_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `docker-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `federation-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `fuse-mount-projection` | `release_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `general-inference-provider` | `future` | `disabled` | `none` | `m28e-inference-ir-and-compatibility-contract` |
| `general-training-provider` | `future` | `disabled` | `none` | `m28d-live-peft-reference-paths` |
| `gpu-host-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `gpu-receipt-path` | `role_required` | `live` | `worker-gpu` | `m26e-host-worker-integration` |
| `host-ticket-executor` | `release_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `industry-provider-family` | `future` | `disabled` | `none` | `m28d-framework-adapters` |
| `kubernetes-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `mcp-gateway` | `future` | `disabled` | `none` | `m28f-mcp-policy-ir` |
| `nemo-provider` | `future` | `disabled` | `none` | `m28d-nemo-provider-family` |
| `packaging` | `release_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `peft-host-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `peft-receipt-path` | `role_required` | `live` | `worker-lora` | `m26e-host-worker-integration` |
| `production-worker-bundle` | `future` | `disabled` | `none` | `m28g-production-worker-ticket-driver-inventory` |
| `prometheus-otel-export` | `future` | `disabled` | `none` | `m28e-inference-receipts-otel-and-evidence` |
| `python-sdk-projection` | `release_required` | `live` | `worker-gpu, worker-heartbeat, worker-lora` | `m26e-python-library-as-built-compatibility` |
| `rest-gateway-projection` | `release_required` | `live` | `worker-gpu, worker-heartbeat, worker-lora` | `m26e-host-integration-dependency-contract` |
| `semantic-context` | `future` | `disabled` | `none` | `m28c-semantic-ir-and-object-contract` |
| `sidecar-provider` | `optional` | `missing, disabled` | `none` | `m26e-host-integration-dependency-contract` |
| `siem-evidence-export` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `swarmui-projection` | `release_required` | `live` | `worker-gpu, worker-heartbeat, worker-lora` | `m26e-host-integration-dependency-contract` |
| `swarmui-workbench` | `future` | `disabled` | `none` | `m28h-integration-truth-model` |
| `systemd-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `worker-control` | `role_required` | `live` | `worker-gpu, worker-heartbeat, worker-lora` | `m26e-host-worker-integration` |
