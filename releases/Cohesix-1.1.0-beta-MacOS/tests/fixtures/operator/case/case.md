# Evidence case

Scenario: "federation"

Offline review; no target, hardware, health, external-execution, or authoritative-receipt proof.

- Correlation `["fixture-1","attempt-1","hive-a","hive-b"]`: **recorded-terminal**
  - request: observed; host/tickets/spec / timeline event 0 / sha256:1b42d32fdb34f70625ee7403a03487e6b4085bf33f5583f178f0a5cdbf09dd0a
  - decision: unknown
  - state-lease-lifecycle: unknown
  - host-result: observed; host/tickets/status / timeline event 1 / sha256:7a1c291626d55a21ab7d0940c666e41c84df6dfe16de7909b1fb95422bc91803
  - receipt-deadletter: missing
  - federation-relay: observed; host/tickets/spec / timeline event 0 / sha256:1b42d32fdb34f70625ee7403a03487e6b4085bf33f5583f178f0a5cdbf09dd0a; host/tickets/status / timeline event 1 / sha256:7a1c291626d55a21ab7d0940c666e41c84df6dfe16de7909b1fb95422bc91803

Capture inventory:

- `host/tickets/deadletter`: "missing"
- `host/tickets/spec`: "observed"
- `host/tickets/status`: "observed"
