<!-- Author: Lukas Bower -->
# Worker Heartbeat Crate

`worker-heart` packages the heartbeat worker behaviour defined in `docs/ARCHITECTURE.md` §2-§3.
Workers append telemetry through NineDoor while operating under budgets issued by the root task.
The current skeleton documents that contract and offers a placeholder API that integration tests
can exercise until the real seL4 event loop is implemented.
