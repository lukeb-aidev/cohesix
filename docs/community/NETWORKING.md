// CLASSIFICATION: COMMUNITY
// Filename: NETWORKING.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-13

# Cohesix Networking

`cohesix_netd` provides TCP transport for 9P services and performs node
discovery. The daemon broadcasts a presence packet on startup and listens on
port 564 for 9P messages. If the TCP listener fails it sends an HTTP POST via
`ureq` as a fallback channel.

Logs are written to `/srv/network/events.log` with RFC3339 timestamps and are
also forwarded to the runtime validator on error.

## Discovery
- UDP broadcast on port 9864 with the message `cohesix_netd_discovery`
- Workers listen for this packet to locate the Queen

## HTTP Fallback
- POSTs to a configured URL if TCP bind or connection fails
- Used to maintain minimal connectivity during network disruptions
