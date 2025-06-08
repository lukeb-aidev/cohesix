// CLASSIFICATION: COMMUNITY
// Filename: KIOSK_FEDERATION.md v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

# Kiosk Federation Demo

KioskInteractive workers pull a UI bundle from the Queen at `/srv/ui_bundle/kiosk_v1/`.
The bundle is deployed to `/mnt/kiosk_ui/` when `cohrun kiosk_start` is invoked.
Interaction events append to `/srv/kiosk_federation.json` and can be triggered
with `cohtrace kiosk_ping` for testing.
