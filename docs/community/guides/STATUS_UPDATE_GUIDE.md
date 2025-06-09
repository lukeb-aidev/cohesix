// CLASSIFICATION: COMMUNITY
// Filename: STATUS_UPDATE_GUIDE.md v1.3
// Date Modified: 2025-05-25
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
| Hydrated     | Stub, file, or code has been created or updated   |
| Pending      | Task or component is queued, awaiting processing  |
| Missing      | Required file or component is absent              |
| Deprecated   | Component is superseded and no longer supported   |

## Update Template

```
[HH:MM UTC] Cohesix - Batch <batch_id> - <task_summary> - Git Status: <Status>
```

## Example

```
03:00 UTC Cohesix - Batch C2 - CI smoke tests added - Git Status: Hydrated
```
