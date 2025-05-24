// CLASSIFICATION: COMMUNITY
// Filename: WATCHDOG_POLICY.md v1.2
// Date Modified: 2025-05-26
// Author: Lukas Bower

# Watchdog Policy

## Purpose

This document defines the 15‑minute watchdog policy for the Cohesix ChatGPT hydration and batch execution pipeline. The watchdog ensures that long‑running tasks (e.g., compiler hydration, OS scaffolding, Codex automation) never stall indefinitely—enabling automated recovery, alerts, and auditability.

## Definitions

- **Watchdog Timer:** A countdown of 15 minutes that resets each time a valid heartbeat is received.
- **Heartbeat:** A lightweight JSON‑RPC or HTTP ‘ping’ emitted by the hydration agent at a fixed interval (every 5 minutes).
- **Hydration Batch:** A discrete unit of work (e.g., Batch C2: stub specs) orchestrated by ChatGPT and the CI.

## Heartbeat Requirements

1. **Interval:** Emit a heartbeat at **no more than 5 minutes** apart (i.e., at least once every 5 minutes).  
2. **Payload:** Include a timestamp, batch ID, and current step status.  
3. **Verification:** The watchdog process verifies timestamp freshness; if the latest heartbeat is older than 15 minutes, it triggers recovery.

## Recovery Procedure

Upon heartbeat timeout (15 minutes without a valid ping):

1. **Container Restart:** Automatically restart the hydration container or ChatGPT agent process to clear any hung state.  
2. **State Integrity Check:** On startup, re-run `validate_metadata_sync.py` and confirm that all expected files from the last checkpoint are present and valid.  
3. **Resume Batches:** Continue from the last known successful step rather than restarting the entire pipeline.  
4. **Retry Limits:** If recovery has been attempted **3 times** without progress, escalate to manual intervention.

## Notification & Escalation

- **Slack Alert:** Post a detailed notification to the #cohesix-ops channel, including batch ID, last heartbeat timestamp, and restart count.  
- **Email Alert:** Send an email to on-call engineers if 3 recovery attempts fail.  
- **Dashboard Update:** Flag the batch as `Failed` in the project status dashboard.

## Failure Modes & Mitigations

| Failure Mode                 | Cause                                  | Mitigation                                         |
|------------------------------|----------------------------------------|----------------------------------------------------|
| Agent Hang                   | Infinite loop or deadlock in code     | Container restart + backoff retry                  |
| Network Partition            | Loss of connection to heartbeat sink  | Buffer pings locally; retry on reconnect           |
| Clock Skew                   | Host time drift                        | Use monotonic timers; ignore minor clock jumps     |
| Persistent Error in Script   | Syntax or runtime error               | Capture logs; do not auto-retry beyond limits      |

## Logging & Audit Trails

- All heartbeat events (success/failure) must be appended to `watchdog.log` with ISO8601 timestamps.  
- Recovery actions must be logged with before/after snapshots of file tree and metadata.  
- Retain logs for **30 days** for post‑mortem analysis.

## Configuration & Deployment

- **Environment Variables:**  
  - `WATCHDOG_INTERVAL=900` (seconds)  
  - `HEARTBEAT_INTERVAL=300` (seconds)  
- **Docker Healthcheck:** Configure the hydration container’s `HEALTHCHECK` to call `/usr/local/bin/heartbeat-check.sh`.  
- **CI Integration:** Incorporate watchdog checks into GitHub Actions using a periodic workflow (`schedule: cron('*/15 * * * *')`).

---

> _This policy ensures that Cohesix’s automated workflows remain resilient, observable, and recoverable under all conditions._