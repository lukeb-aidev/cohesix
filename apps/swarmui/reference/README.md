<!-- Author: Lukas Bower -->
<!-- Purpose: Preserve the provenance and verification boundary of the packaged reference journeys. -->
<!-- Copyright 2026 Lukas Bower -->
# Historical reference evidence

These are byte-identical canonical signed graphs, public trust enrollments and referenced CAS objects from accepted M27c/M27d executions. SwarmUI embeds their bytes and rechecks the shared verifier and every CAS digest at the original recorded trust time. They prove historical execution, never current authority, readiness or a new release. No private signing key or bearer ticket is included.

- `cuda`: `out/m27c/live-session-03/recipe-01/deployment.json`; graph SHA-256 `864deb453deb8b64172e97daf7ca1d11d1908d85e3f335c6d79d431d60ae85a4`.
- `lora`: `out/m27d/live-session-03/m27d-import-03/deployment.json`; graph SHA-256 `8daf8cc168e84e59013f5cbccc79e8894508799eda10aad663252601f5b99a82`.
- `recovery`: `out/m27d/live-session-03/m27d-canary-02/deployment.json`; graph SHA-256 `7658280f10f88be60d2dca2231171be624739675d6c8c0e365ef39c55362f8fe`.

CUDA shows bounded native execution. LoRA shows native validation, evaluation, scan, stage, load, canary and promotion. Recovery shows a failed candidate with successful rollback and remains failed. QEMU target receipt, Worker binding, external native observation and artifact integrity remain separate proof classes.
