// CLASSIFICATION: COMMUNITY
// Filename: gui_orchestrator.md v1.0
// Author: Codex
// Date Modified: 2025-06-07

# Web GUI Orchestrator

This document outlines the architecture for a community-facing dashboard that
exposes live cluster state over a browser connection.

## Overview

The orchestrator queries the `/srv` namespace to display agent status, role
assignments, federation peers, and boot logs. A lightweight WebSocket gateway
bridges requests to internal services.

## Features

- Live agent table with migration controls
- Federation status showing connected Queens
- Boot attestation logs with TPM results
- Role manifest viewer and editing helpers

The frontend speaks JSON over WebSocket to a small Go service that proxies 9P
filesystem calls. Static assets reside under `gui/` and can be served directly by
Plan 9's webfs or an embedded HTTP server.
