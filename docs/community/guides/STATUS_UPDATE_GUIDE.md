// CLASSIFICATION: COMMUNITY
// Filename: STATUS_UPDATE_GUIDE.md v1.3
// Date Modified: 2025-07-31
// Author: Lukas Bower

# Status Update Guide

Cohesix performs automated status updates every **3 hours**, providing a concise snapshot of ongoing work:

## Contents
- **Project:** Cohesix
- **Interval:** Every 3 hours
- **Batch:** Current batch identifier (e.g., C1, O2, T3)
- **Task Summary:** Brief description of tasks completed or in progress
- **Git Status:** One of **Hydrated**, **Pending**, **Missing**, **Deprecated**

## Status Categories

| Status       | Meaning                                           |
|--------------|---------------------------------------------------|
| Hydrated     | Stub, code, documentation, or metadata has been created or updated   |
| Pending      | Task or component is queued, awaiting processing  |
| Missing      | Required file or component is absent              |
| Deprecated   | Component is superseded and no longer supported   |

## Update Template

```
[HH:MM UTC] Cohesix - Batch <batch_id> - <task_summary> - Git Status: <Status>
```

Use UTC timestamps and avoid duplicate batch IDs within a 24-hour window.

## Example

```
03:00 UTC Cohesix - Batch C2 - CI smoke tests added - Git Status: Hydrated
```

## Trace and Validator Context

Every status entry is accompanied by a validator trace snapshot. Trace files are stored under `/log/status_trace/` and include:
- Batch ID
- Timestamp
- Summary string
- Git status classification

These are ingested by `cohtrace` for replay and CI validation.
