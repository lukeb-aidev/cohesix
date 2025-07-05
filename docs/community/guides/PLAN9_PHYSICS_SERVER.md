// CLASSIFICATION: COMMUNITY
// Filename: PLAN9_PHYSICS_SERVER.md v0.1
// Author: Lukas Bower
// Date Modified: 2027-01-30

# Plan9 Physics Server

This guide explains how to run the Plan9 physics server on Cohesix.
The server watches `/mnt/physics_jobs` for `physics_job_*.json` files,
processes each job, and writes results to `/sim`. Status information is
exposed via `/srv/physics/status`.

## Starting the Server

1. Mount the orchestrator export using `secure9p`:

   ```rc
   scripts/mount_secure9p.rc
   ```

   The script records session start/stop messages to `/srv/trace/secure9p.log`.

2. Launch the physics server binary:

   ```rc
   physics-server &
   ```

   Logs are appended to `/srv/trace/sim.log`.

## Dropping a Job File

Upload a JSON job file to `/mnt/physics_jobs` matching the schema:

```json
{
  "job_id": "abc123",
  "initial_position": [0.0, 0.0, 0.0],
  "initial_velocity": [1.0, 0.0, 0.0],
  "mass": 1.0,
  "duration": 3.5
}
```

After processing, `/sim/world.json` and `/sim/result.json` will contain the
simulation snapshot and detailed result. The status file reports job count,
last error, and last job timestamp.

## Example Session Log

```
2027-01-30 10:02:01 session start
2027-01-30 10:02:05 completed abc123
2027-01-30 10:02:06 session stop
```

