// CLASSIFICATION: COMMUNITY
// Filename: gui_orchestrator.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package orchestrator

import (
	"context"
	"net/http"

	orchhttp "cohesix/internal/orchestrator/http"
)

// Controller defines the methods the GUI orchestrator exposes.
type Controller interface {
	Start(context.Context) error
	Router() http.Handler
}

// New returns a Controller backed by the HTTP server implementation.
func New(cfg orchhttp.Config, log orchhttp.Logger) Controller {
	return orchhttp.New(cfg, log)
}
