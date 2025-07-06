// CLASSIFICATION: COMMUNITY
// Filename: K8S_ORCHESTRATION.md v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-31

# Kubernetes & Serverless Orchestration

This document explains how Cohesix can leverage Kubernetes and serverless
functions for elastic scaling without compromising the pure UEFI + Plan9
architecture. Cohesix nodes remain self-contained VMs started by QEMU,
while K8s only schedules and monitors these VMs. Serverless functions react
to filesystem hooks over 9P for automation such as trace uploads.

## Purpose
- **Elastic deployment** of DroneWorkers or Kiosk nodes on cloud clusters.
- **External automation** using AWS Lambda or GCP Cloud Functions to react to
  `/srv/alerts` or `/srv/trace` events.
- **Preserve Cohesix purity** by avoiding guest agents or container runtime code
  inside the VM. All heavy logic lives outside and communicates via 9P.

## Design Overview
Cohesix boots in QEMU with roles passed via `COHROLE`. Each pod encapsulates a
single VM. Kubernetes simply schedules these pods and can scale them up or down.
Serverless functions mount the namespace using `9pfuse` and process any queued
files.

```
+----------- Kubernetes Cluster -----------+
| +-------------------------------------+ |
| |  Cohesix Pod (QueenPrimary)         | |
| |  - QEMU starts cohesix.efi          | |
| |                                     | |
| |  [9P service: /srv/...]             | |
| +-------------------------------------+ |
|        ^                                |
|        | Secure9P / Service             |
| +-------------------------------------+ |
| |  Cohesix Pods (DroneWorker)          | |
| +-------------------------------------+ |
+-------------^---------------------------+
              |
              v 9pfuse
+---------------------------+
| Serverless Function       |
| - mounts /mnt/cohesix     |
| - uploads traces to S3    |
+---------------------------+
```

## Security Model Comparison

| Layer/Component       | Primary Responsibility                              | Security Mechanisms                                   |
|-----------------------|----------------------------------------------------|-------------------------------------------------------|
| Cohesix Node          | Enforce validator rules, Secure9P policies          | seL4 proofs, immutable `CohRole`, sandboxed srv        |
| Kubernetes Scheduler  | Start/stop pods, restart on failure                 | Pod isolation, network policies, IAM roles for nodes  |
| Serverless Functions  | Read alerts, upload traces or metrics               | IAM permissions, ephemeral container with `9pfuse`    |

All validator logic and policy enforcement remain inside Cohesix. Kubernetes and
serverless layers only orchestrate node lifecycle and external automation.

## Terraform Example (EKS)
```hcl
module "eks" {
  source  = "terraform-aws-modules/eks/aws"
  version = "19.10"

  cluster_name    = "cohesix-cluster"
  cluster_version = "1.27"
  subnets         = module.vpc.private_subnets
  vpc_id          = module.vpc.vpc_id
}

resource "kubernetes_daemonset_v1" "cohesix_nodes" {
  metadata {
    name = "cohesix-worker"
    labels = { app = "cohesix" }
  }
  spec {
    selector { match_labels = { app = "cohesix" } }
    template {
      metadata { labels = { app = "cohesix" } }
      spec {
        tolerations { key = "virtualization" operator = "Exists" }
        container {
          name  = "cohesix"
          image = "ghcr.io/cohesix/cohesix-qemu:latest"
          security_context { privileged = true }
          command = [
            "qemu-system-x86_64", "-m", "2048",
            "-kernel", "/assets/cohesix.efi",
            "-append", "COHROLE=${ROLE}"
          ]
          env { name = "ROLE" value = "DroneWorker" }
        }
      }
    }
  }
}
```
This module creates an EKS cluster and schedules a privileged DaemonSet where
each pod starts a Cohesix VM via QEMU. The same pattern may target GKE by using
the `google_container_cluster` module and Kubernetes provider.

## Serverless Trace Uploader (AWS Lambda)
```python
import os
import subprocess
import boto3

MNT = "/tmp/cohesix"
S3_BUCKET = os.environ["TRACE_BUCKET"]
HOST = os.environ.get("COHESIX_HOST", "localhost")

s3 = boto3.client("s3")

def _mount():
    if not os.path.ismount(MNT):
        os.makedirs(MNT, exist_ok=True)
        subprocess.check_call(["9pfuse", HOST, MNT])


def handler(event, context):
    _mount()
    alert_dir = os.path.join(MNT, "srv", "alerts")
    for name in os.listdir(alert_dir):
        full = os.path.join(alert_dir, name)
        with open(full, "rb") as f:
            s3.put_object(Bucket=S3_BUCKET, Key=name, Body=f.read())
        os.remove(full)
    return {"status": "uploaded"}
```
This function mounts the Cohesix namespace using `9pfuse`, reads any files under
`/srv/alerts`, uploads them to S3, and removes them. The Lambda runtime contains
`9pfuse` and required libraries via a custom container image.

## Checklist
- [x] Kubernetes orchestrates QEMU pods only.
- [x] Serverless functions handle `/srv` hooks with `9pfuse`.
- [x] No Python or heavy tooling inside Cohesix VMs.

### Next Steps
- Explore multi‑region clusters with [Federation](../governance/FEDERATION.md).
- Add CI tests for Terraform modules and Lambda integration.

