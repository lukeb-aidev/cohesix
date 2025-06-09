// CLASSIFICATION: COMMUNITY
// Filename: gui_orchestrator.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package orchestrator

import (
	"context"
	"net/http"

	httpserver "cohesix/internal/orchestrator/http"
)

// Orchestrator provides a thin wrapper around the HTTP server.
type Orchestrator struct {
	srv *httpserver.Server
}

// New returns an orchestrator using the given config.
func New(cfg httpserver.Config) *Orchestrator {
	return &Orchestrator{srv: httpserver.New(cfg)}
}

// Start delegates to the underlying HTTP server.
func (o *Orchestrator) Start(ctx context.Context) error { return o.srv.Start(ctx) }

// Router exposes the HTTP router for tests.
func (o *Orchestrator) Router() http.Handler { return o.srv.Router() }
