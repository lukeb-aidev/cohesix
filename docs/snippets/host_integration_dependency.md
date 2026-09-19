<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Project generated host-integration dependency and support truth. -->
<!-- Author: Lukas Bower -->

# Generated Host-Integration Dependencies

This table is generated from `configs/host_integration_acceptance.toml`. Worker execution, provider availability, package presence, mock or dry-run success, and use-case promotion are independent states.

| Dependency | Obligation | Required mode | Worker roles | Owner milestone |
| --- | --- | --- | --- | --- |
| `a2a-gateway` | `future` | `disabled` | `none` | `m28c-a2a-policy-ir` |
| `authenticated-console-projection` | `release_required` | `live` | `worker-gpu, worker-heartbeat, worker-lora` | `m26e-host-integration-dependency-contract` |
| `cas-artifact` | `release_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `docker-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `federation-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `fuse-mount-projection` | `release_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `general-inference-provider` | `future` | `disabled` | `none` | `m27e-inference-ir-and-compatibility-contract` |
| `general-training-provider` | `future` | `disabled` | `none` | `m27d-live-peft-reference-paths` |
| `gpu-host-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `gpu-receipt-path` | `role_required` | `live` | `worker-gpu` | `m26e-host-worker-integration` |
| `host-ticket-executor` | `release_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `industry-provider-family` | `future` | `disabled` | `none` | `m27d-framework-adapters` |
| `kubernetes-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `mcp-gateway` | `future` | `disabled` | `none` | `m28c-mcp-policy-ir` |
| `nemo-provider` | `future` | `disabled` | `none` | `m27d-nemo-provider-family` |
| `packaging` | `release_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `peft-host-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `peft-receipt-path` | `role_required` | `live` | `worker-lora` | `m26e-host-worker-integration` |
| `production-worker-bundle` | `future` | `disabled` | `none` | `m28b-production-worker-ticket-driver-inventory` |
| `prometheus-otel-export` | `future` | `disabled` | `none` | `m27e-inference-receipts-otel-and-evidence` |
| `python-sdk-projection` | `release_required` | `live` | `worker-gpu, worker-heartbeat, worker-lora` | `m26e-python-library-as-built-compatibility` |
| `rest-gateway-projection` | `release_required` | `live` | `worker-gpu, worker-heartbeat, worker-lora` | `m26e-host-integration-dependency-contract` |
| `semantic-context` | `future` | `disabled` | `none` | `m27c-semantic-ir-and-object-contract` |
| `sidecar-provider` | `optional` | `missing, disabled` | `none` | `m26e-host-integration-dependency-contract` |
| `siem-evidence-export` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `swarmui-projection` | `release_required` | `live` | `worker-gpu, worker-heartbeat, worker-lora` | `m26e-host-integration-dependency-contract` |
| `swarmui-workbench` | `future` | `disabled` | `none` | `m27f-integration-truth-model` |
| `systemd-provider` | `use_case_required` | `live` | `none` | `m26e-host-integration-dependency-contract` |
| `worker-control` | `role_required` | `live` | `worker-gpu, worker-heartbeat, worker-lora` | `m26e-host-worker-integration` |

## Provider contract extension

Registration declares bounded requirements and preserves each integration id. It does not grant authority or prove a live provider.

| Provider | Integration id | Implementation availability | Actions |
| --- | --- | --- | --- |
| `apple_ml` | `gpu-host-provider` | `not_implemented` |  |
| `can` | `industry-provider-family` | `not_implemented` |  |
| `ccsds` | `industry-provider-family` | `not_implemented` |  |
| `cuda` | `gpu-host-provider` | `not_implemented` | `cuda.discover` |
| `dicom` | `industry-provider-family` | `not_implemented` |  |
| `dnp3` | `sidecar-provider` | `candidate` | `dnp3.control`, `dnp3.read` |
| `docker` | `docker-provider` | `not_implemented` | `docker.restart`, `docker.status-check`, `docker.stop` |
| `endpoint_compliance` | `sidecar-provider` | `candidate` | `endpoint_compliance.observe` |
| `gpu.lease` | `gpu-host-provider` | `not_implemented` | `gpu.lease.grant`, `gpu.lease.release`, `gpu.lease.renew` |
| `gpu.workload` | `gpu-host-provider` | `not_implemented` | `gpu.workload.cancel`, `gpu.workload.observe`, `gpu.workload.submit` |
| `iec104` | `industry-provider-family` | `not_implemented` |  |
| `jetson` | `sidecar-provider` | `not_implemented` | `jetson.discover` |
| `k8s` | `kubernetes-provider` | `not_implemented` | `k8s.cordon`, `k8s.drain`, `k8s.lease.sync` |
| `launchd` | `systemd-provider` | `candidate` | `launchd.restart`, `launchd.start`, `launchd.status-check`, `launchd.stop` |
| `mac_release` | `packaging` | `candidate` | `mac_release.archive`, `mac_release.build`, `mac_release.codesign`, `mac_release.notarize`, `mac_release.test`, `mac_release.upload` |
| `mig` | `gpu-host-provider` | `not_implemented` |  |
| `modbus` | `sidecar-provider` | `candidate` | `modbus.control`, `modbus.read` |
| `model_registry` | `peft-host-provider` | `not_implemented` | `model_registry.discover` |
| `network` | `sidecar-provider` | `not_implemented` | `network.discover` |
| `nvidia` | `gpu-host-provider` | `not_implemented` | `nvidia.discover` |
| `nvml` | `gpu-host-provider` | `not_implemented` | `nvml.discover` |
| `otel` | `prometheus-otel-export` | `not_implemented` |  |
| `peft` | `peft-host-provider` | `not_implemented` | `peft.activate`, `peft.export`, `peft.import`, `peft.rollback` |
| `prometheus` | `prometheus-otel-export` | `not_implemented` |  |
| `siem` | `siem-evidence-export` | `not_implemented` |  |
| `systemd` | `systemd-provider` | `not_implemented` | `systemd.restart`, `systemd.start`, `systemd.status-check`, `systemd.stop` |

| Surface | Stable integration | Owner | Mode | Read visibility |
| --- | --- | --- | --- | --- |
| `cas` | `cas-artifact` | `cas-tool` | `unknown` | `admin_only` |
| `coh` | `host-ticket-executor` | `coh` | `unknown` | `admin_only` |
| `cohsh` | `authenticated-console-projection` | `cohsh` | `unknown` | `admin_only` |
| `direct_console` | `authenticated-console-projection` | `cohsh` | `unknown` | `admin_only` |
| `evidence` | `siem-evidence-export` | `coh` | `unknown` | `admin_only` |
| `federation` | `federation-provider` | `host-ticket-agent` | `unknown` | `admin_only` |
| `fleet` | `worker-control` | `coh` | `unknown` | `admin_only` |
| `fuse_direct` | `fuse-mount-projection` | `coh` | `unknown` | `admin_only` |
| `fuse_rest` | `fuse-mount-projection` | `coh` | `unknown` | `admin_only` |
| `gpu_bridge` | `gpu-host-provider` | `gpu-bridge-host` | `unknown` | `admin_only` |
| `hive_gateway` | `rest-gateway-projection` | `hive-gateway` | `unknown` | `admin_only` |
| `host_sidecar` | `sidecar-provider` | `host-sidecar-bridge` | `unknown` | `admin_only` |
| `host_ticket_agent` | `host-ticket-executor` | `host-ticket-agent` | `unknown` | `admin_only` |
| `jetson` | `sidecar-provider` | `host-sidecar-bridge` | `unknown` | `admin_only` |
| `network` | `sidecar-provider` | `host-sidecar-bridge` | `unknown` | `admin_only` |
| `otel` | `prometheus-otel-export` | `coh` | `unknown` | `admin_only` |
| `prometheus` | `prometheus-otel-export` | `coh` | `unknown` | `admin_only` |
| `python_sdk` | `python-sdk-projection` | `cohesix-py` | `unknown` | `admin_only` |
| `release_bundle` | `packaging` | `release-bundle` | `unknown` | `admin_only` |
| `rest` | `rest-gateway-projection` | `hive-gateway` | `unknown` | `admin_only` |
| `siem` | `siem-evidence-export` | `coh` | `unknown` | `admin_only` |
| `swarmui` | `swarmui-projection` | `swarmui` | `unknown` | `admin_only` |
