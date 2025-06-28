// CLASSIFICATION: COMMUNITY
// Filename: CLOUD_DEPLOYMENT.md
// Author: Lukas Bower
// Date Modified: 2025-06-28

# Cohesix Cloud Deployment Guide

## Overview

This guide describes how to deploy Cohesix in a cloud-native setup where the QueenPrimary role runs on a cloud platform (e.g., AWS EC2, EKS, GCP Compute) and orchestrates DroneWorker, KioskInteractive, or other roles either in the cloud or on edge devices (like Jetson).

---

## 🚀 Capabilities Enabled
- Run a **QueenPrimary** node in the cloud to handle orchestration, validation, and cluster management.
- Deploy **DroneWorkers** or **KioskInteractive** roles on cloud VMs, Kubernetes pods, or edge devices that register with the Queen.
- Dynamically scale workers via cloud auto-scaling or Kubernetes deployments.
- Aggregate telemetry and enforce Secure9P or validator policies centrally.

---

## ⚙️ Deployment Scenarios
### Queen in the cloud + edge workers
- QueenPrimary on EC2/EKS managing Jetson DroneWorkers connected via Secure9P.
### Full cloud cluster
- QueenPrimary and multiple DroneWorkers on EKS or GKE, automatically scaling up with workloads.
### Hybrid testing
- Run QueenPrimary on a developer laptop or small EC2 instance and spawn ephemeral Workers in the cloud for testing validator enforcement.

---

## 🔧 Environment Setup
### Typical environment variables
| Variable             | Example                         | Purpose                                 |
|-----------------------|--------------------------------|-----------------------------------------|
| `COHROLE`             | `QueenPrimary`                 | Selects the system role.                |
| `CLOUD_HOOK_URL`      | `https://queen-coordinator`    | Where Workers register & report.       |
| `COHESIX_SRV_ROOT`    | `/tmp/srv`                     | Redirects /srv in non-root setups.      |
| `NO_CUDA`             | `1`                            | Disables CUDA initialization.           |

### Example QueenPrimary start
```bash
export COHROLE=QueenPrimary
export CLOUD_HOOK_URL=https://my-cohesix-orchestrator
./target/release/cohesix-shell
```

### Example DroneWorker start
```bash
export COHROLE=DroneWorker
export CLOUD_HOOK_URL=https://my-cohesix-orchestrator
./target/release/cohesix-shell
```

---

## 🚀 Kubernetes & Cloud Scaling
### Kubernetes Deployment snippet
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: cohesix-queen
spec:
  replicas: 1
  template:
    spec:
      containers:
      - name: cohesix
        image: yourregistry/cohesix:latest
        env:
        - name: COHROLE
          value: "QueenPrimary"
        - name: CLOUD_HOOK_URL
          value: "http://queen-service"
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: cohesix-worker
spec:
  replicas: 5
  template:
    spec:
      containers:
      - name: cohesix
        image: yourregistry/cohesix:latest
        env:
        - name: COHROLE
          value: "DroneWorker"
        - name: CLOUD_HOOK_URL
          value: "http://queen-service"
```

---

## 🔎 Developer Quickstart
- Run QueenPrimary locally:
```bash
COHROLE=QueenPrimary ./target/release/cohesix-shell
```
- Start Workers:
```bash
COHROLE=DroneWorker CLOUD_HOOK_URL=http://localhost:8080 ./target/release/cohesix-shell
```
- Use `cohtrace cloud` to view orchestration state.

---

## ✅ Verifying Deployment
- Check Queen logs under `/log/trace` for registration and validator activity.
- Run `cohtrace cloud` on any node to verify Queen ID, last heartbeat, and connected Workers.

---

## ⚠️ Security & Feature Flags
- `secure9p` should be enabled in cloud production. Use build flags or secure configuration.
- `NO_CUDA` disables GPU modules if cloud nodes don’t have CUDA.
- Always validate Secure9P policy files align with `RoleManifest`.

---

## 🎯 Summary
With this guide, you can deploy QueenPrimary in the cloud, scale Workers dynamically, enforce validator & Secure9P rules, and debug via `cohtrace cloud` — achieving a robust cloud-edge Cohesix orchestration model.